use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::Result;
use globset::Glob;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::Config;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDef {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub api: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    pub reasoning: bool,
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub cost: Value,
    #[serde(default)]
    pub thinking_level_map: BTreeMap<String, Value>,
    #[serde(default)]
    pub compat: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelListItem {
    pub id: String,
    pub name: String,
    pub provider: String,
    /// The provider's display name, as the selector labels its group.
    pub provider_name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub input: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsData {
    pub models: BTreeMap<String, String>,
    pub model_list: Vec<ModelListItem>,
    pub default_model: Option<DefaultModel>,
    pub default_thinking_level: Option<String>,
    pub saved_default_thinking_level: Option<String>,
    pub thinking_levels: BTreeMap<String, Vec<String>>,
    pub thinking_level_maps: BTreeMap<String, BTreeMap<String, Value>>,
    pub thinking_level_pins: BTreeMap<String, Value>,
    /// True when `enabledModels` narrows the list.
    pub is_scoped: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultModel {
    pub provider: String,
    pub model_id: String,
}

pub fn load_models(config: &Config, scope_all: bool) -> Result<ModelsData> {
    let settings = read_json(&config.settings_path()).unwrap_or(Value::Null);
    let enabled = settings
        .get("enabledModels")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|patterns| !patterns.is_empty())
        .filter(|_| !scope_all);
    let custom_names = custom_provider_names(config);
    let auth = read_json(&config.agent_dir.join("auth.json")).unwrap_or(Value::Null);
    let defs = model_defs(config)?;
    let refs = defs
        .values()
        .map(|def| format!("{}/{}", def.provider, def.id))
        .collect::<Vec<_>>();
    let scope = enabled
        .as_deref()
        .filter(|patterns| !patterns.is_empty())
        .map(|patterns| enabled_model_refs(patterns, &refs));
    let mut list = Vec::new();
    let mut models = BTreeMap::new();
    let mut thinking_levels = BTreeMap::new();
    let mut thinking_level_maps = BTreeMap::new();
    for def in defs.values() {
        // pi only lists models of providers it can authenticate; pi-star must
        // match, or the selector offers catalog models whose requests fail.
        if !provider_available(def, &auth) {
            continue;
        }
        let key = format!("{}/{}", def.provider, def.id);
        if let Some(scope) = &scope {
            if !scope.contains(&key) {
                continue;
            }
        }
        models.insert(key.clone(), def.name.clone());
        let levels = if def.reasoning {
            vec!["off", "minimal", "low", "medium", "high", "xhigh", "max"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        } else {
            vec!["off".to_string()]
        };
        thinking_levels.insert(key.clone(), levels);
        if !def.thinking_level_map.is_empty() {
            thinking_level_maps.insert(key, def.thinking_level_map.clone());
        }
        list.push(ModelListItem {
            id: def.id.clone(),
            name: def.name.clone(),
            provider: def.provider.clone(),
            provider_name: custom_names
                .get(&def.provider)
                .cloned()
                .unwrap_or_else(|| provider_display_name(&def.provider)),
            input: def.input.clone(),
        });
    }
    list.sort_by(|a, b| {
        a.provider
            .cmp(&b.provider)
            .then_with(|| a.name.cmp(&b.name))
    });
    let default_provider = settings.get("defaultProvider").and_then(Value::as_str);
    let default_model_id = settings.get("defaultModel").and_then(Value::as_str);
    let default_model = default_provider
        .zip(default_model_id)
        .and_then(|(provider, model_id)| {
            let key = format!("{provider}/{model_id}");
            models.contains_key(&key).then(|| DefaultModel {
                provider: provider.to_string(),
                model_id: model_id.to_string(),
            })
        })
        .or_else(|| {
            list.first().map(|model| DefaultModel {
                provider: model.provider.clone(),
                model_id: model.id.clone(),
            })
        });
    let saved_default = settings
        .get("defaultThinkingLevel")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let thinking_level_pins = thinking_pins(enabled.as_deref(), &models);
    Ok(ModelsData {
        models,
        model_list: list,
        default_model,
        default_thinking_level: saved_default.clone(),
        saved_default_thinking_level: saved_default,
        thinking_levels,
        thinking_level_maps,
        thinking_level_pins,
        is_scoped: enabled.is_some() && !scope_all,
    })
}

/// A `:level` suffix on an `enabledModels` pattern pins that reasoning level.
fn split_thinking_pin(pattern: &str) -> (&str, Option<&str>) {
    match pattern.rsplit_once(':') {
        Some((base, level))
            if matches!(
                level,
                "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
            ) =>
        {
            (base, Some(level))
        }
        _ => (pattern, None),
    }
}

fn thinking_pins(
    patterns: Option<&[String]>,
    models: &BTreeMap<String, String>,
) -> BTreeMap<String, Value> {
    let mut pins = BTreeMap::new();
    for pattern in patterns.unwrap_or_default() {
        let (base, level) = split_thinking_pin(pattern);
        let Some(level) = level else { continue };
        for key in models.keys() {
            if matches_pattern(base, key) {
                pins.insert(key.clone(), Value::String(level.to_string()));
            }
        }
    }
    pins
}

/// Whether a model's provider has a credential pi-star can send: one pi wrote
/// to auth.json (api key or OAuth), a key models.json carries, or the
/// provider's `<ID>_API_KEY` environment variable. Unauthenticated catalog
/// providers stay out of the selector, as they do in pi.
fn provider_available(def: &ModelDef, auth: &Value) -> bool {
    if def
        .api_key
        .as_deref()
        .is_some_and(|key| !key.trim().is_empty())
    {
        return true;
    }
    if auth.get(&def.provider).is_some() {
        return true;
    }
    let env = format!(
        "{}_API_KEY",
        def.provider.to_uppercase().replace(['-', '.'], "_")
    );
    std::env::var_os(env).is_some()
}

/// `name` of each models.json provider: what the panel and selector show instead
/// of the id.
pub fn custom_provider_names(config: &Config) -> BTreeMap<String, String> {
    read_json(&config.models_path())
        .and_then(|value| {
            value
                .get("providers")
                .and_then(Value::as_object)
                .map(|providers| {
                    providers
                        .iter()
                        .filter_map(|(id, provider)| {
                            provider
                                .get("name")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|name| !name.is_empty())
                                .map(|name| (id.clone(), name.to_string()))
                        })
                        .collect()
                })
        })
        .unwrap_or_default()
}

/// pi's provider display names, as its runtime reports them; a provider
/// models.json added without a `name` falls back to its id.
pub fn provider_display_name(id: &str) -> String {
    match id {
        "amazon-bedrock" => "Amazon Bedrock",
        "anthropic" => "Anthropic",
        "ant-ling" => "Ant Ling",
        "azure" => "Azure",
        "baseten" => "Baseten",
        "cerebras" => "Cerebras",
        "cloudflare-ai-gateway" => "Cloudflare AI Gateway",
        "cloudflare-workers-ai" => "Cloudflare Workers AI",
        "deepseek" => "DeepSeek",
        "fireworks" => "Fireworks",
        "github-copilot" => "GitHub Copilot",
        "google" => "Google",
        "google-vertex" => "Google Vertex AI",
        "groq" => "Groq",
        "huggingface" => "Hugging Face",
        "kimi-coding" => "Kimi For Coding",
        "meta" => "Meta",
        "minimax" => "MiniMax",
        "minimax-cn" => "MiniMax CN",
        "mistral" => "Mistral",
        "moonshotai" => "Moonshot AI",
        "moonshotai-cn" => "Moonshot AI CN",
        "nvidia" => "NVIDIA",
        "openai" => "OpenAI",
        "openai-codex" => "OpenAI Codex (legacy)",
        "opencode" => "OpenCode Zen",
        "opencode-go" => "OpenCode Go",
        "openrouter" => "OpenRouter",
        "qwen-token-plan" => "Qwen Token Plan",
        "qwen-token-plan-cn" => "Qwen Token Plan CN",
        "qwen-token-plan-individual" => "Qwen Token Plan Individual",
        "together" => "Together",
        "typesafe" => "TypeSafe",
        "vercel-ai-gateway" => "Vercel AI Gateway",
        "xai" => "xAI",
        "xiaomi" => "Xiaomi",
        "xiaomi-token-plan-ams" => "Xiaomi Token Plan AMS",
        "xiaomi-token-plan-cn" => "Xiaomi Token Plan CN",
        "xiaomi-token-plan-sgp" => "Xiaomi Token Plan SGP",
        "zai" => "Z.AI",
        "zai-coding-cn" => "Z.AI Coding CN",
        _ => id,
    }
    .to_string()
}

pub fn model_defs(config: &Config) -> Result<HashMap<String, ModelDef>> {
    let mut defs: HashMap<String, ModelDef> = HashMap::new();
    let auth = read_json(&config.agent_dir.join("auth.json")).unwrap_or(Value::Null);
    if let Some(store) = read_json(&config.agent_dir.join("models-store.json")) {
        if let Some(providers) = store.as_object() {
            for (provider, value) in providers {
                let Some(models) = value.get("models").and_then(Value::as_array) else {
                    continue;
                };
                for model in models {
                    if let Some(def) = model_from_catalog(provider, model, &auth) {
                        defs.insert(format!("{}/{}", def.provider, def.id), def);
                    }
                }
            }
        }
    }
    if let Some(models_config) = read_json(&config.models_path()) {
        if let Some(providers) = models_config.get("providers").and_then(Value::as_object) {
            for (provider, value) in providers {
                let Some(models) = value.get("models").and_then(Value::as_array) else {
                    continue;
                };
                for model in models {
                    if let Some(mut def) = model_from_catalog(provider, model, &auth) {
                        if let Some(api) = value.get("api").and_then(Value::as_str) {
                            def.api = api.to_string();
                        }
                        if let Some(base_url) = value.get("baseUrl").and_then(Value::as_str) {
                            def.base_url = base_url.to_string();
                        }
                        if let Some(api_key) = value.get("apiKey").and_then(Value::as_str) {
                            def.api_key = Some(api_key.to_string());
                        }
                        defs.insert(format!("{}/{}", def.provider, def.id), def);
                    }
                }
            }
        }
    }
    // A provider's auth key applies to catalog models that did not name one.
    for def in defs.values_mut() {
        if def.api_key.is_none() {
            def.api_key = auth
                .get(&def.provider)
                .and_then(|entry| {
                    if entry.get("type").and_then(Value::as_str) == Some("oauth") {
                        entry.get("access").and_then(Value::as_str)
                    } else {
                        entry.get("key").and_then(Value::as_str)
                    }
                })
                .map(str::to_owned);
        }
    }
    Ok(defs)
}

fn model_from_catalog(provider: &str, model: &Value, _auth: &Value) -> Option<ModelDef> {
    let id = model.get("id").and_then(Value::as_str)?.to_string();
    let name = model
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&id)
        .to_string();
    let api = model
        .get("api")
        .and_then(Value::as_str)
        .unwrap_or("openai-completions")
        .to_string();
    let base_url = model
        .get("baseUrl")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let reasoning = model
        .get("reasoning")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let input = model
        .get("input")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_else(|| vec!["text".to_string()]);
    Some(ModelDef {
        id,
        name,
        provider: model
            .get("provider")
            .and_then(Value::as_str)
            .unwrap_or(provider)
            .to_string(),
        api,
        base_url,
        api_key: model
            .get("apiKey")
            .and_then(Value::as_str)
            .map(str::to_owned),
        reasoning,
        input,
        context_window: model.get("contextWindow").and_then(Value::as_u64),
        max_tokens: model.get("maxTokens").and_then(Value::as_u64),
        cost: model.get("cost").cloned().unwrap_or(Value::Null),
        thinking_level_map: model
            .get("thinkingLevelMap")
            .and_then(Value::as_object)
            .map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default(),
        compat: model.get("compat").cloned().unwrap_or(Value::Null),
    })
}

pub fn find_model(config: &Config, provider: &str, model_id: &str) -> Result<Option<ModelDef>> {
    Ok(model_defs(config)?.remove(&format!("{provider}/{model_id}")))
}

/// `enabledModels` patterns resolved against the available model references.
pub fn resolve_patterns(
    patterns: &[String],
    refs: &[String],
) -> Vec<(String, Vec<String>, Option<String>)> {
    patterns
        .iter()
        .map(|pattern| {
            let (base, pin) = split_thinking_pin(pattern.trim_start_matches('!'));
            let matched = refs
                .iter()
                .filter(|reference| matches_pattern(base, reference))
                .cloned()
                .collect();
            (pattern.clone(), matched, pin.map(str::to_owned))
        })
        .collect()
}

/// The model references the scope enables: all of them when no pattern narrows.
pub fn enabled_model_refs(
    patterns: &[String],
    refs: &[String],
) -> std::collections::BTreeSet<String> {
    // `!ref` is the exclusion an earlier pi-star wrote; pi itself only reads
    // positive patterns, so a later edit normalizes it away (patterns_for).
    let positives = patterns.iter().any(|pattern| !pattern.starts_with('!'));
    let mut enabled = if patterns.is_empty() || !positives {
        refs.iter().cloned().collect()
    } else {
        let mut set = std::collections::BTreeSet::new();
        for (pattern, matched, _) in resolve_patterns(patterns, refs) {
            if !pattern.starts_with('!') {
                set.extend(matched);
            }
        }
        set
    };
    for (pattern, matched, _) in resolve_patterns(patterns, refs) {
        if pattern.starts_with('!') {
            for reference in matched {
                enabled.remove(&reference);
            }
        }
    }
    enabled
}

/// The thinking level each enabled model is pinned to by a `:level` pattern.
pub fn thinking_pins_for(patterns: &[String], refs: &[String]) -> BTreeMap<String, String> {
    let mut pins = BTreeMap::new();
    for (pattern, matched, pin) in resolve_patterns(patterns, refs) {
        if pattern.starts_with('!') {
            continue;
        }
        if let Some(pin) = pin {
            for reference in matched {
                pins.insert(reference, pin.clone());
            }
        }
    }
    pins
}

/// The patterns that spell out `enabled`, keeping pinned thinking levels.
pub fn patterns_for(
    enabled: &std::collections::BTreeSet<String>,
    refs: &[String],
    pins: &BTreeMap<String, String>,
) -> Vec<String> {
    if enabled.len() == refs.len() {
        return Vec::new();
    }
    enabled
        .iter()
        .map(|reference| match pins.get(reference) {
            Some(level) => format!("{reference}:{level}"),
            None => reference.clone(),
        })
        .collect()
}

fn matches_pattern(pattern: &str, key: &str) -> bool {
    let (pattern, _) = split_thinking_pin(pattern);
    if pattern == "*" || pattern == key {
        return true;
    }
    Glob::new(pattern)
        .ok()
        .map(|glob| glob.compile_matcher().is_match(key))
        .unwrap_or(false)
}

pub fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    // models/settings are JSONC in practice.
    serde_json::from_str(&strip_json_comments(&text)).ok()
}

pub fn strip_json_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            out.push(ch);
            continue;
        }
        if ch == '/' {
            match chars.peek() {
                Some('/') => {
                    for next in chars.by_ref() {
                        if next == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                }
                Some('*') => {
                    chars.next();
                    let mut previous = '\0';
                    for next in chars.by_ref() {
                        if previous == '*' && next == '/' {
                            break;
                        }
                        previous = next;
                    }
                }
                _ => out.push(ch),
            }
        } else {
            out.push(ch);
        }
    }
    // Trailing commas.
    let mut result = String::with_capacity(out.len());
    let chars: Vec<char> = out.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ',' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && (chars[j] == '}' || chars[j] == ']') {
                i += 1;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_patterns_resolve_to_refs_and_pins() {
        let refs = vec![
            "anthropic/claude".to_string(),
            "openai/gpt".to_string(),
            "openai/o3".to_string(),
        ];
        let patterns = vec!["openai/*".to_string(), "anthropic/claude:high".to_string()];
        let resolutions = resolve_patterns(&patterns, &refs);
        assert_eq!(resolutions[0].1, vec!["openai/gpt", "openai/o3"]);
        assert_eq!(resolutions[1].1, vec!["anthropic/claude"]);
        assert_eq!(resolutions[1].2.as_deref(), Some("high"));
        let pins = thinking_pins_for(&patterns, &refs);
        assert_eq!(
            pins.get("anthropic/claude").map(String::as_str),
            Some("high")
        );
    }

    #[test]
    fn unauthenticated_catalog_providers_stay_out_of_the_list() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("models-store.json"),
            json!({
                "configured-one": { "models": [{ "id": "chat", "name": "Chat" }] },
                "unconfigured-one": { "models": [{ "id": "glm", "name": "GLM" }] }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("auth.json"),
            json!({ "configured-one": { "type": "api_key", "key": "test-key" } }).to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("models.json"),
            json!({
                "providers": {
                    "custom-one": {
                        "apiKey": "test-key",
                        "models": [{ "id": "local", "name": "Local" }]
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        let config = crate::config::Config::new(Some(dir.path().to_path_buf()), None).unwrap();
        let data = load_models(&config, false).unwrap();
        let providers = data
            .model_list
            .iter()
            .map(|model| model.provider.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            providers,
            ["configured-one", "custom-one"].into_iter().collect()
        );
    }

    #[test]
    fn a_legacy_exclusion_pattern_narrows_from_all() {
        let refs = vec!["a/x".to_string(), "a/y".to_string()];
        let enabled = enabled_model_refs(&["!a/y".to_string()], &refs);
        assert_eq!(enabled.into_iter().collect::<Vec<_>>(), vec!["a/x"]);
        let enabled = enabled_model_refs(&["a/x".to_string(), "!a/y".to_string()], &refs);
        assert_eq!(enabled.into_iter().collect::<Vec<_>>(), vec!["a/x"]);
    }

    #[test]
    fn enabling_every_model_writes_no_patterns() {
        let refs = vec!["a/x".to_string(), "a/y".to_string()];
        let all = enabled_model_refs(&[], &refs);
        assert_eq!(all.len(), 2);
        assert!(patterns_for(&all, &refs, &BTreeMap::new()).is_empty());
        let mut few = all.clone();
        few.remove("a/y");
        assert_eq!(patterns_for(&few, &refs, &BTreeMap::new()), vec!["a/x"]);
    }

    #[test]
    fn strips_comments_and_trailing_commas() {
        let text = r#"{"a": 1, // x
        "b": [1,2,],}"#;
        let value: Value = serde_json::from_str(&strip_json_comments(text)).unwrap();
        assert_eq!(value["a"], 1);
    }
}

// ── models.json image size limits ────────────────────────────────────────────
//
// pi's schema wants `inputLimits.images.resize.maxBytes` to be an integer of at
// least 1 and drops the whole models.json on anything else, so the save route
// checks every custom model that sets it. Mirrors pi-web's
// `lib/model-image-limits.ts` (`parseImageMaxBytes`, `checkImageMaxBytes`).

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// A whole number of bytes, at least 1; None for anything pi would refuse.
pub fn parse_image_max_bytes(value: &Value) -> Option<u64> {
    let parsed = match value {
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() || !text.chars().all(|ch| ch.is_ascii_digit()) {
                return None;
            }
            text.parse::<u64>().ok()?
        }
        Value::Number(number) => number.as_u64()?,
        _ => return None,
    };
    (parsed >= 1 && parsed <= MAX_SAFE_INTEGER).then_some(parsed)
}

fn image_max_bytes(model: &Value) -> Option<&Value> {
    (model.get("inputLimits")?.get("images")?.get("resize"))?.get("maxBytes")
}

/// The first custom model whose limit pi would refuse, as a message naming it.
pub fn check_image_max_bytes(config: &Value) -> Result<(), String> {
    let Some(providers) = config.get("providers").and_then(Value::as_object) else {
        return Ok(());
    };
    for (provider_id, provider) in providers {
        let Some(models) = provider.get("models").and_then(Value::as_array) else {
            continue;
        };
        for model in models {
            // The store drops models with a blank id before writing.
            if model
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id.trim().is_empty())
            {
                continue;
            }
            let Some(value) = image_max_bytes(model) else {
                continue;
            };
            if parse_image_max_bytes(value).is_none() {
                let id = model.get("id").and_then(Value::as_str).unwrap_or("?");
                return Err(format!(
                    "Image size limit of {provider_id}/{id} must be a positive whole number of bytes"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod image_limit_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_only_positive_safe_whole_bytes() {
        assert_eq!(parse_image_max_bytes(&json!(" 2048 ")), Some(2048));
        assert_eq!(parse_image_max_bytes(&json!(2048)), Some(2048));
        assert_eq!(parse_image_max_bytes(&json!(0)), None);
        assert_eq!(parse_image_max_bytes(&json!(-1)), None);
        assert_eq!(parse_image_max_bytes(&json!(2048.5)), None);
        assert_eq!(parse_image_max_bytes(&json!("1e6")), None);
        assert_eq!(parse_image_max_bytes(&json!("")), None);
        assert_eq!(parse_image_max_bytes(&json!(null)), None);
        assert_eq!(
            parse_image_max_bytes(&json!(9_007_199_254_740_992u64)),
            None
        );
    }

    #[test]
    fn checks_every_custom_model_and_names_the_first_bad_one() {
        let valid = json!({
            "providers": {
                "gateway": { "models": [
                    { "id": "a", "inputLimits": { "images": { "resize": { "maxBytes": 524288 } } } },
                    { "id": "b" },
                    { "id": " ", "inputLimits": { "images": { "resize": { "maxBytes": "x" } } } }
                ] },
                "managed": { "modelOverrides": { "c": { "contextWindow": 1 } } }
            }
        });
        assert!(check_image_max_bytes(&valid).is_ok());
        let invalid = json!({
            "providers": { "gateway": { "models": [
                { "id": "vision", "inputLimits": { "images": { "resize": { "maxBytes": "1.5" } } } }
            ] } }
        });
        assert_eq!(
            check_image_max_bytes(&invalid),
            Err(
                "Image size limit of gateway/vision must be a positive whole number of bytes"
                    .into()
            )
        );
        assert!(check_image_max_bytes(&json!({})).is_ok());
    }
}

/// Merge a models.dev catalog into pi's `models-store.json` shape. Providers
/// named by `only` (or every provider when empty) are replaced; other stored
/// providers stay untouched. Returns whether the visible model list changed.
pub fn apply_catalog_to_store(
    store: &mut Value,
    entries: &[crate::model_catalog::CatalogEntry],
    only: Option<&str>,
) -> bool {
    let before = store_signature(store);
    let mut grouped: BTreeMap<String, Vec<&crate::model_catalog::CatalogEntry>> = BTreeMap::new();
    for entry in entries {
        if let Some(provider) = only.filter(|provider| !provider.is_empty()) {
            if entry.provider_id != provider && entry.provider_name != provider {
                continue;
            }
        }
        grouped
            .entry(entry.provider_id.clone())
            .or_default()
            .push(entry);
    }
    if store.get("__proto__").is_some() {
        store
            .as_object_mut()
            .map(|object| object.remove("__proto__"));
    }
    if !store.is_object() {
        *store = json!({});
    }
    for (provider, models) in grouped {
        let object = store.as_object_mut().expect("store is an object");
        let provider_entry = object.entry(provider.clone()).or_insert_with(|| json!({}));
        if !provider_entry.is_object() {
            *provider_entry = json!({});
        }
        let api = if provider.contains("anthropic") {
            "anthropic-messages"
        } else {
            "openai-completions"
        };
        let models_json: Vec<Value> = models
            .iter()
            .map(|entry| {
                let mut model = json!({
                    "id": entry.id,
                    "name": entry.name,
                    "provider": entry.provider_id,
                    "api": api,
                    "reasoning": entry.reasoning.unwrap_or(false),
                    "input": entry.input.clone().unwrap_or_else(|| vec!["text".to_string()]),
                    "cost": {
                        "input": entry.cost.input.unwrap_or(0.0),
                        "output": entry.cost.output.unwrap_or(0.0),
                        "cacheRead": entry.cost.cache_read.unwrap_or(0.0),
                        "cacheWrite": entry.cost.cache_write.unwrap_or(0.0),
                    },
                });
                if let Some(base_url) = &entry.provider_base_url {
                    model["baseUrl"] = Value::String(base_url.clone());
                }
                if let Some(context) = entry.context_window {
                    model["contextWindow"] = Value::from(context);
                }
                if let Some(max_tokens) = entry.max_tokens {
                    model["maxTokens"] = Value::from(max_tokens);
                }
                model
            })
            .collect();
        provider_entry["models"] = Value::Array(models_json);
    }
    before != store_signature(store)
}

fn store_signature(store: &Value) -> String {
    let Some(providers) = store.as_object() else {
        return String::new();
    };
    let mut rows = Vec::new();
    for (provider, value) in providers {
        let Some(models) = value.get("models").and_then(Value::as_array) else {
            continue;
        };
        for model in models {
            rows.push(format!(
                "{provider}\u{0}{}\u{0}{}",
                model.get("id").and_then(Value::as_str).unwrap_or(""),
                model.get("name").and_then(Value::as_str).unwrap_or(""),
            ));
        }
    }
    rows.sort();
    rows.join("\n")
}
