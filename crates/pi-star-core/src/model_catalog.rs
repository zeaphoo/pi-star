//! The models.dev catalog used by the Models panel's "Auto-fill" button and
//! by the on-demand model refresh. Ported from pi-web's `lib/model-catalog.ts`.

use std::collections::BTreeMap;

use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub key: String,
    pub provider_id: String,
    pub provider_name: String,
    pub provider_base_url: Option<String>,
    pub id: String,
    pub name: String,
    pub reasoning: Option<bool>,
    pub input: Option<Vec<String>>,
    pub context_window: Option<u64>,
    pub max_tokens: Option<u64>,
    pub cost: CatalogCost,
}

#[derive(Debug, Clone, Default)]
pub struct CatalogCost {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

impl CatalogEntry {
    pub fn to_json(&self) -> Value {
        let mut value = json!({
            "key": self.key,
            "providerId": self.provider_id,
            "providerName": self.provider_name,
            "id": self.id,
            "name": self.name,
            "cost": {
                "input": self.cost.input,
                "output": self.cost.output,
                "cacheRead": self.cost.cache_read,
                "cacheWrite": self.cost.cache_write,
            },
        });
        if let Some(url) = &self.provider_base_url {
            value["providerBaseUrl"] = Value::String(url.clone());
        }
        if let Some(reasoning) = self.reasoning {
            value["reasoning"] = Value::Bool(reasoning);
        }
        if let Some(input) = &self.input {
            value["input"] = json!(input);
        }
        if let Some(context) = self.context_window {
            value["contextWindow"] = Value::from(context);
        }
        if let Some(max_tokens) = self.max_tokens {
            value["maxTokens"] = Value::from(max_tokens);
        }
        value
    }
}

fn is_record(value: &Value) -> bool {
    value.is_object()
}

fn clean_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn non_negative(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
}

fn positive_u64(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64).filter(|value| *value > 0)
}

fn read_cost(value: Option<&Value>) -> CatalogCost {
    let Some(value) = value.filter(|value| is_record(value)) else {
        return CatalogCost::default();
    };
    CatalogCost {
        input: non_negative(value.get("input")),
        output: non_negative(value.get("output")),
        cache_read: non_negative(value.get("cache_read")),
        cache_write: non_negative(value.get("cache_write")),
    }
}

fn read_input(value: &Value) -> Option<Vec<String>> {
    let modalities = value.get("modalities")?.get("input")?.as_array()?;
    let mut input: Vec<String> = Vec::new();
    for item in modalities {
        let Some(item) = item.as_str() else { continue };
        let item = item.trim().to_ascii_lowercase();
        if matches!(item.as_str(), "text" | "image") && !input.contains(&item) {
            input.push(item);
        }
    }
    (!input.is_empty()).then_some(input)
}

pub fn flatten_models_dev(value: &Value) -> Vec<CatalogEntry> {
    let Some(providers) = value.as_object() else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for (provider_id, provider) in providers {
        let Some(models) = provider.get("models").and_then(Value::as_object) else {
            continue;
        };
        let provider_name =
            clean_string(provider.get("name")).unwrap_or_else(|| provider_id.clone());
        let provider_base_url = clean_string(provider.get("api"));
        for (fallback_id, model) in models {
            let id = clean_string(model.get("id")).unwrap_or_else(|| fallback_id.clone());
            if id.is_empty() {
                continue;
            }
            let name = clean_string(model.get("name")).unwrap_or_else(|| id.clone());
            let context_window = positive_u64(model.pointer("/limit/context"));
            let max_tokens = positive_u64(model.pointer("/limit/output"));
            entries.push(CatalogEntry {
                key: format!("{provider_id}/{id}"),
                provider_id: provider_id.clone(),
                provider_name: provider_name.clone(),
                provider_base_url: provider_base_url.clone(),
                id,
                name,
                reasoning: model.get("reasoning").and_then(Value::as_bool),
                input: read_input(model),
                context_window,
                max_tokens,
                cost: read_cost(model.get("cost")),
            });
        }
    }
    entries
}

fn normalize_provider(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

fn normalize_model_id(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .strip_prefix("models/")
        .unwrap_or(&value.trim().to_ascii_lowercase())
        .to_string()
}

fn hostname(value: Option<&str>) -> Option<String> {
    let value = value?;
    let rest = value
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(value);
    let host = rest
        .split('/')
        .next()?
        .split(':')
        .next()?
        .trim_end_matches('.');
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

fn host_matches(actual: &str, expected: &str) -> bool {
    actual == expected || actual.ends_with(&format!(".{expected}"))
}

fn provider_matches(entry: &CatalogEntry, hint: &str) -> bool {
    let hint = normalize_provider(hint);
    !hint.is_empty()
        && (normalize_provider(&entry.provider_id) == hint
            || normalize_provider(&entry.provider_name) == hint)
}

fn base_url_matches(entry: &CatalogEntry, base_url: &str) -> bool {
    let Some(actual) = hostname(Some(base_url)) else {
        return false;
    };
    let known: &[&str] = match normalize_provider(&entry.provider_id).as_str() {
        "anthropic" => &["api.anthropic.com"],
        "google" => &["generativelanguage.googleapis.com"],
        "openai" => &["api.openai.com"],
        "openrouter" => &["openrouter.ai"],
        _ => &[],
    };
    known
        .iter()
        .copied()
        .chain(hostname(entry.provider_base_url.as_deref()).as_deref())
        .any(|expected| host_matches(&actual, expected))
}

fn exact_match(entry: &CatalogEntry, query: &str) -> bool {
    let query = normalize_model_id(query);
    if query.is_empty() {
        return false;
    }
    let id = normalize_model_id(&entry.id);
    let full = format!("{}/{}", entry.provider_id.to_ascii_lowercase(), id);
    id == query || full == query
}

fn valid_price(entry: &CatalogEntry) -> bool {
    entry.cost.input.is_some() && entry.cost.output.is_some()
}

fn mode_string(values: &[(String, usize)], total: usize) -> Option<String> {
    let mut groups: BTreeMap<String, usize> = BTreeMap::new();
    for (value, count) in values {
        *groups.entry(value.clone()).or_default() += count;
    }
    let mut ranked: Vec<(String, usize)> = groups.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    let winner = ranked.first()?;
    if winner.1 * 10 < total * 6 || ranked.get(1).is_some_and(|next| next.1 == winner.1) {
        return None;
    }
    Some(winner.0.clone())
}

fn consensus_preset(entries: &[&CatalogEntry]) -> Value {
    let total = entries.len();
    let mut preset = json!({});
    if let Some(name) = mode_string(
        &entries
            .iter()
            .map(|entry| (entry.name.to_ascii_lowercase(), 1usize))
            .collect::<Vec<_>>(),
        total,
    ) {
        // Use the winning spelling, not its lower-case key.
        let name = entries
            .iter()
            .find(|entry| entry.name.to_ascii_lowercase() == name)
            .map(|entry| entry.name.clone())
            .unwrap_or(name);
        preset["name"] = Value::String(name);
    }
    let reasoning_values: Vec<(String, usize)> = entries
        .iter()
        .filter_map(|entry| entry.reasoning.map(|value| (value.to_string(), 1)))
        .collect();
    if let Some(reasoning) = mode_string(&reasoning_values, total) {
        preset["reasoning"] = Value::Bool(reasoning == "true");
    }
    if let Some(input) = mode_string(
        &entries
            .iter()
            .filter_map(|entry| {
                entry.input.as_ref().map(|input| {
                    let mut sorted = input.clone();
                    sorted.sort();
                    (sorted.join(","), 1)
                })
            })
            .collect::<Vec<_>>(),
        total,
    ) {
        preset["input"] = json!(
            input
                .split(',')
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
        );
    }
    if let Some(context) = mode_string(
        &entries
            .iter()
            .filter_map(|entry| entry.context_window.map(|value| (value.to_string(), 1)))
            .collect::<Vec<_>>(),
        total,
    ) {
        if let Ok(value) = context.parse::<u64>() {
            preset["contextWindow"] = Value::from(value);
        }
    }
    if let Some(max_tokens) = mode_string(
        &entries
            .iter()
            .filter_map(|entry| entry.max_tokens.map(|value| (value.to_string(), 1)))
            .collect::<Vec<_>>(),
        total,
    ) {
        if let Ok(value) = max_tokens.parse::<u64>() {
            preset["maxTokens"] = Value::from(value);
        }
    }
    preset
}

fn price_for(entry: &CatalogEntry, method: &str) -> Value {
    json!({
        "status": "reliable",
        "method": method,
        "cost": {
            "input": entry.cost.input,
            "output": entry.cost.output,
            "cacheRead": entry.cost.cache_read.unwrap_or(0.0),
            "cacheWrite": entry.cost.cache_write.unwrap_or(0.0),
        },
        "providerId": entry.provider_id,
        "providerName": entry.provider_name,
        "support": 1,
        "total": 1,
    })
}

pub fn recommend(
    entries: &[CatalogEntry],
    query: &str,
    provider_hint: &str,
    base_url: &str,
) -> Value {
    let exact: Vec<&CatalogEntry> = entries
        .iter()
        .filter(|entry| exact_match(entry, query))
        .collect();
    if exact.is_empty() {
        return json!({
            "exactMatches": 0,
            "metadataMethod": "none",
            "preset": {},
            "price": {"status": "unreliable", "reason": "no-exact-match", "support": 0, "total": 0},
        });
    }
    let provider_entries: Vec<&CatalogEntry> = exact
        .iter()
        .copied()
        .filter(|entry| provider_matches(entry, provider_hint))
        .collect();
    let base_entries: Vec<&CatalogEntry> = exact
        .iter()
        .copied()
        .filter(|entry| base_url_matches(entry, base_url))
        .collect();
    let metadata_entry = provider_entries
        .first()
        .or_else(|| base_entries.first())
        .copied();
    let method = if !provider_entries.is_empty() {
        "provider"
    } else if !base_entries.is_empty() {
        "base-url"
    } else {
        "consensus"
    };
    let mut preset = if let Some(entry) = metadata_entry {
        let mut value = json!({"name": entry.name});
        if let Some(reasoning) = entry.reasoning {
            value["reasoning"] = Value::Bool(reasoning);
        }
        if let Some(input) = &entry.input {
            value["input"] = json!(input);
        }
        if let Some(context) = entry.context_window {
            value["contextWindow"] = Value::from(context);
        }
        if let Some(max_tokens) = entry.max_tokens {
            value["maxTokens"] = Value::from(max_tokens);
        }
        value
    } else {
        consensus_preset(&exact)
    };
    let price = if let Some(entry) = provider_entries.iter().find(|entry| valid_price(entry)) {
        price_for(entry, "provider")
    } else if let Some(entry) = base_entries.iter().find(|entry| valid_price(entry)) {
        price_for(entry, "base-url")
    } else {
        let priced: Vec<&CatalogEntry> = exact
            .iter()
            .copied()
            .filter(|entry| valid_price(entry))
            .collect();
        if priced.len() < 2 {
            json!({"status": "unreliable", "reason": if priced.is_empty() {"no-valid-price"} else {"insufficient-support"}, "support": priced.len(), "total": priced.len()})
        } else if let Some(first) = priced.first() {
            price_for(first, "consensus")
        } else {
            json!({"status": "unreliable", "reason": "no-valid-price", "support": 0, "total": 0})
        }
    };
    if price["status"] == "reliable" {
        preset["cost"] = price["cost"].clone();
    }
    json!({
        "exactMatches": exact.len(),
        "metadataMethod": method,
        "matchedProviderId": metadata_entry.map(|entry| entry.provider_id.clone()),
        "matchedProviderName": metadata_entry.map(|entry| entry.provider_name.clone()),
        "preset": preset,
        "price": price,
    })
}

fn match_rank(entry: &CatalogEntry, query: &str, provider_hint: &str) -> f64 {
    let id = entry.id.to_ascii_lowercase();
    let name = entry.name.to_ascii_lowercase();
    let provider_id = entry.provider_id.to_ascii_lowercase();
    let provider_name = entry.provider_name.to_ascii_lowercase();
    let full_id = format!("{provider_id}/{id}");
    let mut rank = 20.0;
    if query.is_empty() {
        rank = 10.0;
    } else if id == query || full_id == query {
        rank = 0.0;
    } else if name == query {
        rank = 1.0;
    } else if id.starts_with(query) || name.starts_with(query) {
        rank = 2.0;
    } else if full_id.starts_with(query) || provider_id == query || provider_name == query {
        rank = 3.0;
    } else if id.contains(query) || name.contains(query) {
        rank = 4.0;
    } else if full_id.contains(query) || provider_name.contains(query) {
        rank = 5.0;
    }
    if rank < 20.0
        && !provider_hint.is_empty()
        && (provider_id == provider_hint || provider_name == provider_hint)
    {
        rank -= 0.5;
    }
    rank
}

pub fn search(
    entries: &[CatalogEntry],
    query: &str,
    provider_hint: &str,
    limit: usize,
) -> Vec<Value> {
    let query = query.trim().to_ascii_lowercase();
    let provider = provider_hint.trim().to_ascii_lowercase();
    let limit = limit.clamp(1, 100);
    let mut rows: Vec<(f64, &CatalogEntry)> = entries
        .iter()
        .map(|entry| (match_rank(entry, &query, &provider), entry))
        .filter(|(rank, _)| query.is_empty() || *rank < 20.0)
        .collect();
    rows.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.1.provider_name
                    .to_ascii_lowercase()
                    .cmp(&b.1.provider_name.to_ascii_lowercase())
            })
            .then_with(|| {
                a.1.name
                    .to_ascii_lowercase()
                    .cmp(&b.1.name.to_ascii_lowercase())
            })
            .then_with(|| {
                a.1.id
                    .to_ascii_lowercase()
                    .cmp(&b.1.id.to_ascii_lowercase())
            })
    });
    rows.into_iter()
        .take(limit)
        .map(|(_, entry)| entry.to_json())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Value {
        json!({
            "demo": {
                "name": "Demo",
                "api": "https://api.demo.test",
                "models": {
                    "demo-large": {
                        "id": "demo-large", "name": "Demo Large", "reasoning": true,
                        "modalities": {"input": ["text", "image", "audio"]},
                        "limit": {"context": 128000, "output": 8192},
                        "cost": {"input": 1, "output": 2, "cache_read": 0.1}
                    }
                }
            }
        })
    }

    #[test]
    fn flattens_models_dev_metadata() {
        let entries = flatten_models_dev(&catalog());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "demo/demo-large");
        assert_eq!(
            entries[0].input.as_deref(),
            Some(&["text".to_string(), "image".to_string()][..])
        );
        assert_eq!(entries[0].context_window, Some(128000));
        assert_eq!(entries[0].cost.cache_read, Some(0.1));
    }

    #[test]
    fn searches_and_recommends_exact_models() {
        let entries = flatten_models_dev(&catalog());
        assert_eq!(
            search(&entries, "demo-large", "", 50)[0]["key"],
            "demo/demo-large"
        );
        let recommendation = recommend(&entries, "demo-large", "demo", "https://api.demo.test");
        assert_eq!(recommendation["metadataMethod"], "provider");
        assert_eq!(recommendation["preset"]["name"], "Demo Large");
        assert_eq!(recommendation["price"]["status"], "reliable");
        assert_eq!(recommendation["price"]["cost"]["output"], 2.0);
        assert_eq!(
            recommend(&entries, "missing", "", "")["metadataMethod"],
            "none"
        );
    }
}
