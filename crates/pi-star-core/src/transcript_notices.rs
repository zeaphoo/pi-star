//! pi's `notices.*` transcript notices (one `showCacheMissNotices` before pi
//! 1.6.1), derived from session entries exactly like pi's TUI. Nothing is
//! persisted: a reload or reconnect recomputes the same notices.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};

use crate::session::{SessionContext, entry_id, entry_type};
use crate::settings::Notices;
use crate::{Config, models::read_json, settings};

const CACHE_TTL_MS: i64 = 5 * 60 * 1000;
const NOISE_FLOOR_TOKENS: i64 = 1024;
const NOTICE_MIN_TOKENS: i64 = 20_000;
const NOTICE_MIN_COST: f64 = 0.1;

#[derive(Debug, Clone)]
struct PreviousRequest {
    prompt_tokens: i64,
    model_key: String,
    timestamp: i64,
    reported_cache: bool,
}

#[derive(Debug, Clone)]
pub struct CacheMiss {
    missed_tokens: i64,
    missed_cost: f64,
    idle_ms: i64,
    model_changed: bool,
}

pub fn enabled(config: &Config, cwd: &std::path::Path) -> Notices {
    let global = settings::read_settings(config);
    let project = read_json(&cwd.join(".pi").join("settings.json"));
    settings::effective_notices_of(&global, project.as_ref())
}

fn usage_of(message: &Value) -> Option<&Value> {
    let usage = message.get("usage")?;
    if usage.get("input").and_then(Value::as_f64).is_some()
        && usage.get("output").and_then(Value::as_f64).is_some()
        && usage.get("cacheRead").and_then(Value::as_f64).is_some()
        && usage.get("cacheWrite").and_then(Value::as_f64).is_some()
    {
        Some(usage)
    } else {
        None
    }
}

fn assistant_message(entry: &Value) -> Option<&Value> {
    if entry_type(entry) != Some("message") {
        return None;
    }
    let message = entry.get("message")?;
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    usage_of(message)?;
    Some(message)
}

fn prompt_tokens(usage: &Value) -> i64 {
    usage.get("input").and_then(Value::as_f64).unwrap_or(0.0) as i64
        + usage
            .get("cacheRead")
            .and_then(Value::as_f64)
            .unwrap_or(0.0) as i64
        + usage
            .get("cacheWrite")
            .and_then(Value::as_f64)
            .unwrap_or(0.0) as i64
}

fn cost(usage: &Value, key: &str) -> f64 {
    usage
        .get("cost")
        .and_then(|cost| cost.get(key))
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
}

fn detect_miss(
    previous: Option<&PreviousRequest>,
    message: &Value,
    prices: &HashMap<String, f64>,
) -> Option<CacheMiss> {
    let usage = usage_of(message)?;
    let tokens = prompt_tokens(usage);
    let cache_read = usage
        .get("cacheRead")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as i64;
    let cache_write = usage
        .get("cacheWrite")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as i64;
    let previous = previous?;
    if tokens <= 0 || (cache_read + cache_write == 0 && !previous.reported_cache) {
        return None;
    }
    let missed_tokens = previous.prompt_tokens.min(tokens) - cache_read;
    if missed_tokens <= NOISE_FLOOR_TOKENS {
        return None;
    }
    let input = usage.get("input").and_then(Value::as_f64).unwrap_or(0.0) as i64;
    let paid_tokens = input + cache_write;
    let paid_per_token = if paid_tokens > 0 {
        (cost(usage, "input") + cost(usage, "cacheWrite")) / paid_tokens as f64
    } else {
        0.0
    };
    let read_per_token = if cache_read > 0 {
        cost(usage, "cacheRead") / cache_read as f64
    } else {
        let provider = message
            .get("provider")
            .and_then(Value::as_str)
            .unwrap_or("");
        let model = message.get("model").and_then(Value::as_str).unwrap_or("");
        prices
            .get(&format!("{provider}/{model}"))
            .copied()
            .unwrap_or(0.0)
            / 1_000_000.0
    };
    let timestamp = message
        .get("timestamp")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    Some(CacheMiss {
        missed_tokens,
        missed_cost: missed_tokens as f64 * (paid_per_token - read_per_token).max(0.0),
        idle_ms: (timestamp - previous.timestamp).max(0),
        model_changed: format!(
            "{}/{}",
            message
                .get("provider")
                .and_then(Value::as_str)
                .unwrap_or(""),
            message.get("model").and_then(Value::as_str).unwrap_or("")
        ) != previous.model_key,
    })
}

fn as_previous(message: &Value, reported_cache: bool) -> Option<PreviousRequest> {
    let usage = usage_of(message)?;
    let prompt_tokens = prompt_tokens(usage);
    if prompt_tokens <= 0 {
        return None;
    }
    Some(PreviousRequest {
        prompt_tokens,
        model_key: format!(
            "{}/{}",
            message
                .get("provider")
                .and_then(Value::as_str)
                .unwrap_or(""),
            message.get("model").and_then(Value::as_str).unwrap_or("")
        ),
        timestamp: message
            .get("timestamp")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        reported_cache: reported_cache
            || usage
                .get("cacheRead")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                > 0.0
            || usage
                .get("cacheWrite")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                > 0.0,
    })
}

fn cache_read_prices(entries: &[Value]) -> HashMap<String, f64> {
    let mut prices = HashMap::new();
    for entry in entries {
        let Some(message) = assistant_message(entry) else {
            continue;
        };
        let Some(usage) = usage_of(message) else {
            continue;
        };
        let read = usage
            .get("cacheRead")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        if read <= 0.0 {
            continue;
        }
        let key = format!(
            "{}/{}",
            message
                .get("provider")
                .and_then(Value::as_str)
                .unwrap_or(""),
            message.get("model").and_then(Value::as_str).unwrap_or("")
        );
        prices.insert(key, cost(usage, "cacheRead") / read * 1_000_000.0);
    }
    prices
}

fn scan_cache_misses(
    entries: &[Value],
    prices: &HashMap<String, f64>,
) -> HashMap<String, CacheMiss> {
    let mut previous: Option<PreviousRequest> = None;
    let mut misses = HashMap::new();
    for entry in entries {
        match entry_type(entry) {
            Some("compaction" | "branch_summary") => {
                previous = None;
                continue;
            }
            Some("usage") if entry.get("kind").and_then(Value::as_str) == Some("cache_warm") => {
                let Some(usage) = entry.get("usage") else {
                    continue;
                };
                if usage_of(&json!({"usage": usage})).is_none() {
                    continue;
                }
                let tokens = prompt_tokens(usage);
                if tokens > 0 {
                    previous = Some(PreviousRequest {
                        prompt_tokens: tokens,
                        model_key: format!(
                            "{}/{}",
                            entry.get("provider").and_then(Value::as_str).unwrap_or(""),
                            entry.get("model").and_then(Value::as_str).unwrap_or("")
                        ),
                        timestamp: entry
                            .get("timestamp")
                            .and_then(Value::as_str)
                            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                            .map(|value| value.timestamp_millis())
                            .unwrap_or(0),
                        reported_cache: true,
                    });
                }
                continue;
            }
            _ => {}
        }
        let Some(message) = assistant_message(entry) else {
            continue;
        };
        if let Some(miss) = detect_miss(previous.as_ref(), message, prices) {
            if let Some(id) = entry_id(entry) {
                misses.insert(id, miss);
            }
        }
        if let Some(next) = as_previous(
            message,
            previous.as_ref().map(|p| p.reported_cache).unwrap_or(false),
        ) {
            previous = Some(next);
        }
    }
    misses
}

fn cache_miss_notice(miss: &CacheMiss) -> Option<Value> {
    if miss.missed_tokens < NOTICE_MIN_TOKENS && miss.missed_cost < NOTICE_MIN_COST {
        return None;
    }
    let mut notice = Map::new();
    notice.insert("kind".into(), Value::String("cache_miss".into()));
    notice.insert("missedTokens".into(), json!(miss.missed_tokens));
    notice.insert("missedCost".into(), json!(miss.missed_cost));
    if miss.model_changed {
        notice.insert("modelChanged".into(), Value::Bool(true));
    } else if miss.idle_ms >= CACHE_TTL_MS {
        notice.insert(
            "idleMinutes".into(),
            json!((miss.idle_ms as f64 / 60_000.0).round() as i64),
        );
    }
    Some(Value::Object(notice))
}

/// The reasoning a provider reported withholding from the request that
/// produced `message`, as pi's TUI `getDroppedReasoning()` reads it: the first
/// diagnostic that lists any. Anthropic drops thinking blocks
/// (`thinking_dropped`), OpenAI drops reasoning items (`reasoning`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct DroppedReasoning {
    provider: &'static str,
    count: usize,
    reasons: Vec<String>,
}

/// pi's TUI `REASONING_DROP_DIAGNOSTICS`.
const REASONING_DROP_DIAGNOSTICS: [(&str, &str, &str); 2] = [
    (
        "anthropic_input_transformations",
        "anthropic",
        "thinking_dropped",
    ),
    ("openai_input_transformations", "openai", "reasoning"),
];

fn dropped_reasoning(message: &Value) -> Option<DroppedReasoning> {
    let diagnostics = message.get("diagnostics").and_then(Value::as_array)?;
    for diagnostic in diagnostics {
        let Some(kind) = diagnostic.get("type").and_then(Value::as_str) else {
            continue;
        };
        let Some((_, provider, dropped_type)) = REASONING_DROP_DIAGNOSTICS
            .iter()
            .find(|(diagnostic_type, _, _)| *diagnostic_type == kind)
        else {
            continue;
        };
        let Some(transformations) = diagnostic
            .pointer("/details/transformations")
            .and_then(Value::as_array)
        else {
            continue;
        };
        let dropped: Vec<&Value> = transformations
            .iter()
            .filter(|transformation| {
                transformation.get("type").and_then(Value::as_str) == Some(*dropped_type)
            })
            .collect();
        if dropped.is_empty() {
            continue;
        }
        let mut reasons: Vec<String> = Vec::new();
        for transformation in &dropped {
            if let Some(reason) = transformation.get("reason").and_then(Value::as_str)
                && !reasons.iter().any(|known| known == reason)
            {
                reasons.push(reason.to_string());
            }
        }
        return Some(DroppedReasoning {
            provider,
            count: dropped.len(),
            reasons,
        });
    }
    None
}

fn reasoning_dropped_notice(
    dropped: &DroppedReasoning,
    previous: Option<&DroppedReasoning>,
) -> Option<Value> {
    // Providers report a withheld block again on every later request that
    // replays it, so the TUI shows a drop only when the previous response's was
    // another provider's or fewer.
    if previous.is_some_and(|previous| {
        previous.provider == dropped.provider && dropped.count <= previous.count
    }) {
        return None;
    }
    let mut notice = Map::new();
    notice.insert("kind".into(), Value::String("reasoning_dropped".into()));
    notice.insert("provider".into(), Value::String(dropped.provider.into()));
    notice.insert("count".into(), json!(dropped.count));
    if !dropped.reasons.is_empty() {
        notice.insert("reasons".into(), json!(dropped.reasons));
    }
    Some(Value::Object(notice))
}

fn summary_cost(source: &str, usage: Option<&Value>) -> Option<Value> {
    let usage = usage?;
    let input = usage.get("input").and_then(Value::as_f64).unwrap_or(0.0);
    let output = usage.get("output").and_then(Value::as_f64).unwrap_or(0.0);
    let cache_read = usage
        .get("cacheRead")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let cache_write = usage
        .get("cacheWrite")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    Some(json!({
        "kind": "summary_cost",
        "source": source,
        "tokens": (input + output + cache_read + cache_write) as i64,
        "cost": cost(usage, "total"),
    }))
}

fn renders_standalone(entry: &Value) -> bool {
    match entry_type(entry) {
        Some("message") => !matches!(
            entry.pointer("/message/role").and_then(Value::as_str),
            Some("toolResult" | "system")
        ),
        Some("compaction") => true,
        Some("branch_summary") => entry
            .get("summary")
            .and_then(Value::as_str)
            .is_some_and(|summary| !summary.is_empty()),
        Some("custom_message") => entry
            .get("display")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        _ => false,
    }
}

fn push_notice(map: &mut HashMap<String, Vec<Value>>, id: &str, notice: Option<Value>) {
    if let Some(notice) = notice {
        map.entry(id.to_string()).or_default().push(notice);
    }
}

fn ancestor<'a>(
    entry: &Value,
    by_id: &'a HashMap<String, &'a Value>,
    matches: impl Fn(&Value) -> bool,
) -> Option<&'a Value> {
    let mut current = entry
        .get("parentId")
        .and_then(Value::as_str)
        .and_then(|id| by_id.get(id).copied());
    while let Some(candidate) = current {
        if matches(candidate) {
            return Some(candidate);
        }
        current = candidate
            .get("parentId")
            .and_then(Value::as_str)
            .and_then(|id| by_id.get(id).copied());
    }
    None
}

pub fn collect(
    entries: &[Value],
    leaf_id: Option<&str>,
    notices: Notices,
) -> HashMap<String, Vec<Value>> {
    if !notices.any() {
        return HashMap::new();
    }
    let by_id: HashMap<String, &Value> = entries
        .iter()
        .filter_map(|entry| entry_id(entry).map(|id| (id, entry)))
        .collect::<Vec<_>>()
        .into_iter()
        .collect();
    let by_id: HashMap<String, &Value> = by_id.into_iter().collect();
    let mut on_branch = None;
    if let Some(leaf_id) = leaf_id {
        let mut branch = HashSet::new();
        let mut current = by_id.get(leaf_id).copied();
        while let Some(entry) = current {
            if let Some(id) = entry_id(entry) {
                branch.insert(id);
            }
            current = entry
                .get("parentId")
                .and_then(Value::as_str)
                .and_then(|id| by_id.get(id).copied());
        }
        on_branch = Some(branch);
    }
    let prices = cache_read_prices(entries);
    let misses = if notices.cache_misses {
        scan_cache_misses(entries, &prices)
    } else {
        HashMap::new()
    };
    let mut notices_by_entry: HashMap<String, Vec<Value>> = HashMap::new();
    for entry in entries {
        let Some(id) = entry_id(entry) else { continue };
        match entry_type(entry) {
            Some("message") => {
                let Some(message) = assistant_message(entry) else {
                    continue;
                };
                if matches!(
                    message.get("stopReason").and_then(Value::as_str),
                    Some("aborted" | "error")
                ) {
                    continue;
                }
                if let Some(miss) = misses.get(&id) {
                    push_notice(&mut notices_by_entry, &id, cache_miss_notice(miss));
                }
                if notices.reasoning_drops
                    && let Some(dropped) = dropped_reasoning(message)
                {
                    let previous = ancestor(entry, &by_id, |candidate| {
                        assistant_message(candidate).is_some()
                    })
                    .and_then(|previous| {
                        assistant_message(previous).and_then(dropped_reasoning)
                    });
                    push_notice(
                        &mut notices_by_entry,
                        &id,
                        reasoning_dropped_notice(&dropped, previous.as_ref()),
                    );
                }
            }
            Some("usage")
                if notices.cache_warming
                    && entry.get("kind").and_then(Value::as_str) == Some("cache_warm")
                    && on_branch.as_ref().is_none_or(|branch| branch.contains(&id)) =>
            {
                let Some(anchor) = ancestor(entry, &by_id, renders_standalone) else {
                    continue;
                };
                let Some(anchor_id) = entry_id(anchor) else {
                    continue;
                };
                let usage = entry.get("usage");
                let mut notice = json!({
                    "kind": "cache_warm",
                    "cost": usage.map(|usage| cost(usage, "total")).unwrap_or(0.0),
                });
                if let Some(note) = entry.get("note").and_then(Value::as_str) {
                    notice["note"] = Value::String(note.to_string());
                }
                push_notice(&mut notices_by_entry, &anchor_id, Some(notice));
            }
            Some("compaction") if notices.compaction_costs => {
                push_notice(
                    &mut notices_by_entry,
                    &id,
                    summary_cost("compaction", entry.get("usage")),
                );
            }
            Some("branch_summary")
                if notices.compaction_costs
                    && entry
                        .get("summary")
                        .and_then(Value::as_str)
                        .is_some_and(|summary| !summary.is_empty()) =>
            {
                push_notice(
                    &mut notices_by_entry,
                    &id,
                    summary_cost("branch_summary", entry.get("usage")),
                );
            }
            _ => {}
        }
    }
    notices_by_entry
}

/// Add `transcriptNotices` to the messages that have one, keeping every other
/// field of the context untouched.
pub fn attach(context: &mut SessionContext, notices: &HashMap<String, Vec<Value>>) {
    if notices.is_empty() {
        return;
    }
    for (index, entry_id) in context.entry_ids.iter().enumerate() {
        let Some(list) = notices.get(entry_id) else {
            continue;
        };
        if let Some(message) = context
            .messages
            .get_mut(index)
            .and_then(Value::as_object_mut)
        {
            message.insert("transcriptNotices".into(), Value::Array(list.clone()));
        }
    }
}

pub fn live_assistant_notices(entries: &[Value], message: &Value, settings: Notices) -> Vec<Value> {
    if !settings.any() {
        return Vec::new();
    }
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return Vec::new();
    }
    if usage_of(message).is_none() {
        return Vec::new();
    }
    if matches!(
        message.get("stopReason").and_then(Value::as_str),
        Some("aborted" | "error")
    ) {
        return Vec::new();
    }
    let prices = cache_read_prices(entries);
    let previous = entries
        .iter()
        .rev()
        .filter_map(|entry| assistant_message(entry))
        .find_map(|candidate| as_previous(candidate, false));
    let mut notices = Vec::new();
    if settings.cache_misses
        && let Some(miss) = detect_miss(previous.as_ref(), message, &prices)
        && let Some(notice) = cache_miss_notice(&miss)
    {
        notices.push(notice);
    }
    if settings.reasoning_drops
        && let Some(dropped) = dropped_reasoning(message)
    {
        let previous_dropped = entries
            .iter()
            .rev()
            .filter_map(assistant_message)
            .next()
            .and_then(dropped_reasoning);
        if let Some(notice) = reasoning_dropped_notice(&dropped, previous_dropped.as_ref()) {
            notices.push(notice);
        }
    }
    notices
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant(
        id: &str,
        parent: Option<&str>,
        input: f64,
        cache_read: f64,
        total_cost: f64,
    ) -> Value {
        json!({
            "type": "message",
            "id": id,
            "parentId": parent,
            "timestamp": "2026-01-01T00:00:00.000Z",
            "message": {
                "role": "assistant",
                "provider": "test",
                "model": "test",
                "content": [{"type":"text","text":"ok"}],
                "timestamp": 1_700_000_000_000i64,
                "usage": {
                    "input": input,
                    "output": 1,
                    "cacheRead": cache_read,
                    "cacheWrite": 0,
                    "cost": {"input": total_cost, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": total_cost}
                }
            }
        })
    }

    #[test]
    fn detects_and_thresholds_a_cache_miss() {
        let entries = vec![
            assistant("a1", None, 9_500.0, 500.0, 0.0),
            assistant("a2", Some("a1"), 30_000.0, 0.0, 0.5),
        ];
        let notices = collect(&entries, Some("a2"), Notices { cache_misses: true, ..Default::default() });
        let list = notices.get("a2").expect("miss notice");
        assert_eq!(list[0]["kind"], "cache_miss");
        assert!(list[0]["missedCost"].as_f64().unwrap() > 0.1);
    }

    #[test]
    fn every_switch_is_off_by_default() {
        let entries = vec![
            assistant("a1", None, 9_500.0, 500.0, 0.0),
            assistant("a2", Some("a1"), 30_000.0, 0.0, 0.5),
        ];
        assert!(collect(&entries, Some("a2"), Notices::default()).is_empty());
    }

    #[test]
    fn each_switch_shows_only_its_own_notice() {
        let entries = vec![
            assistant("a1", None, 9_500.0, 500.0, 0.0),
            assistant("a2", Some("a1"), 30_000.0, 0.0, 0.5),
        ];
        let warming_only = collect(
            &entries,
            Some("a2"),
            Notices { cache_warming: true, ..Default::default() },
        );
        assert!(warming_only.is_empty());
    }

    #[test]
    fn a_project_setting_overrides_the_global_one() {
        let global = json!({"notices": {"cacheMisses": true, "reasoningDrops": true}});
        assert!(!settings::effective_notices_of(&json!({}), None).any());
        let defaults = settings::effective_notices_of(&global, None);
        assert!(defaults.cache_misses && defaults.reasoning_drops);
        assert!(!defaults.cache_warming && !defaults.compaction_costs);
        let project = json!({"notices": {"cacheMisses": false}});
        let effective = settings::effective_notices_of(&global, Some(&project));
        assert!(!effective.cache_misses);
        assert!(effective.reasoning_drops);
    }

    #[test]
    fn reports_a_reasoning_drop_for_both_providers() {
        let mut message = assistant("a1", None, 10.0, 0.0, 0.0);
        message["message"]["diagnostics"] = json!([{
            "type": "openai_input_transformations",
            "details": {"transformations": [
                {"type": "reasoning", "reason": "prefix_mismatch"},
                {"type": "reasoning", "reason": "prefix_mismatch"}
            ]}
        }]);
        let dropped = dropped_reasoning(&message["message"]).expect("drop");
        assert_eq!(dropped.provider, "openai");
        assert_eq!(dropped.count, 2);
        assert_eq!(dropped.reasons, vec!["prefix_mismatch".to_string()]);
        let notice = reasoning_dropped_notice(&dropped, None).expect("notice");
        assert_eq!(notice["kind"], "reasoning_dropped");
        assert_eq!(notice["provider"], "openai");
        assert_eq!(notice["count"], 2);
    }
}
