use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::Result;
use globset::Glob;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultModel {
    pub provider: String,
    pub model_id: String,
}

pub fn load_models(config: &Config) -> Result<ModelsData> {
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
        });
    let defs = model_defs(config)?;
    let mut list = Vec::new();
    let mut models = BTreeMap::new();
    let mut thinking_levels = BTreeMap::new();
    let mut thinking_level_maps = BTreeMap::new();
    for def in defs.values() {
        let key = format!("{}/{}", def.provider, def.id);
        if let Some(patterns) = &enabled {
            if !patterns
                .iter()
                .any(|pattern| matches_pattern(pattern, &key))
            {
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
    Ok(ModelsData {
        models,
        model_list: list,
        default_model,
        default_thinking_level: saved_default.clone(),
        saved_default_thinking_level: saved_default,
        thinking_levels,
        thinking_level_maps,
        thinking_level_pins: BTreeMap::new(),
    })
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

fn matches_pattern(pattern: &str, key: &str) -> bool {
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
    fn strips_comments_and_trailing_commas() {
        let text = r#"{"a": 1, // x
        "b": [1,2,],}"#;
        let value: Value = serde_json::from_str(&strip_json_comments(text)).unwrap();
        assert_eq!(value["a"], 1);
    }
}
