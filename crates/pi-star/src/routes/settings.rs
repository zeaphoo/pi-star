use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use pi_star_core::models::{self, model_defs, read_json, strip_json_comments};
use pi_star_core::settings;

use crate::routes::{bad_request, forbidden, internal, no_store, not_found};
use crate::state::AppState;

#[derive(Debug, Deserialize, Default)]
pub struct CwdQuery {
    cwd: Option<String>,
}

pub async fn models(State(state): State<Arc<AppState>>, Query(query): Query<CwdQuery>) -> Response {
    if let Some(cwd) = query.cwd.filter(|value| !value.is_empty()) {
        let cwd = PathBuf::from(cwd);
        if !cwd.is_dir() {
            return bad_request(format!("Directory does not exist: {}", cwd.display()));
        }
        if !state.roots.is_allowed_existing(&cwd) {
            return forbidden("Access denied");
        }
    }
    match state.models() {
        Ok(data) => no_store(data),
        Err(error) => internal(error),
    }
}

#[derive(Debug, Deserialize)]
pub struct DefaultModelBody {
    #[serde(rename = "provider")]
    provider: Option<String>,
    #[serde(rename = "modelId")]
    model_id: Option<String>,
    #[serde(rename = "thinkingLevel")]
    thinking_level: Option<String>,
}

pub async fn set_default_model(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DefaultModelBody>,
) -> Response {
    if let (Some(provider), Some(model_id)) = (body.provider.as_ref(), body.model_id.as_ref()) {
        if models::find_model(&state.config, provider, model_id)
            .ok()
            .flatten()
            .is_none()
        {
            return not_found(format!("Model not available: {provider}/{model_id}"));
        }
        if let Err(error) = settings::set_default_model(
            &state.config,
            provider,
            model_id,
            body.thinking_level.as_deref(),
        ) {
            return internal(error);
        }
    } else if let Some(level) = body.thinking_level.as_deref() {
        if let Err(error) = settings::set_default_thinking_level(&state.config, level) {
            return internal(error);
        }
        state.invalidate_models();
    } else {
        return bad_request("Expected provider and modelId, or a valid thinkingLevel");
    }
    no_store(json!({"ok": true}))
}

pub async fn enabled_models(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CwdQuery>,
) -> Response {
    let _ = query;
    match enabled_models_view(&state) {
        Ok(view) => no_store(view),
        Err(error) => internal(error),
    }
}

fn enabled_models_view(state: &AppState) -> anyhow::Result<Value> {
    let settings_value = settings::read_settings(&state.config);
    let patterns = settings_value
        .get("enabledModels")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        });
    let data = state.models()?;
    let all_enabled = patterns.is_none();
    let mut providers: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut enabled_total = 0usize;
    for model in &data.model_list {
        let reference = format!("{}/{}", model.provider, model.id);
        let enabled = patterns.as_ref().is_none_or(|patterns| {
            patterns.iter().any(|pattern| {
                pattern == "*"
                    || pattern == &reference
                    || pattern == &format!("{}/*", model.provider)
                    || glob_match(pattern, &reference)
            })
        });
        if enabled {
            enabled_total += 1;
        }
        providers
            .entry(model.provider.clone())
            .or_default()
            .push(json!({
                "id": model.id,
                "name": model.name,
                "ref": reference,
                "enabled": enabled,
            }));
    }
    let providers = providers
        .into_iter()
        .map(|(id, models)| {
            let enabled_count = models
                .iter()
                .filter(|model| model.get("enabled").and_then(Value::as_bool) == Some(true))
                .count();
            json!({
                "id": id,
                "name": id,
                "kind": "builtin",
                "enabledCount": enabled_count,
                "models": models,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "allEnabled": all_enabled,
        "patterns": patterns,
        "stalePatterns": [],
        "enabledTotal": if all_enabled { data.model_list.len() } else { enabled_total },
        "availableTotal": data.model_list.len(),
        "providers": providers,
        "scope": "global",
        "settingsPath": state.config.settings_path().to_string_lossy(),
        "editable": true,
    }))
}

fn glob_match(pattern: &str, value: &str) -> bool {
    globset::Glob::new(pattern)
        .ok()
        .map(|glob| glob.compile_matcher().is_match(value))
        .unwrap_or(false)
}

#[derive(Debug, Deserialize, Default)]
pub struct RenamePair {
    from: String,
    to: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EnabledModelsBody {
    op: Option<String>,
    enabled: Option<bool>,
    refs: Option<Vec<String>>,
    provider: Option<String>,
    /// `resync`: provider and model renames made by the models.json save.
    #[serde(default)]
    renames: Vec<RenamePair>,
    #[serde(default)]
    model_renames: Vec<RenamePair>,
    /// Providers that were fully enabled before the save.
    #[serde(default)]
    fully_enabled: Vec<String>,
}

pub async fn set_enabled_models(
    State(state): State<Arc<AppState>>,
    Json(body): Json<EnabledModelsBody>,
) -> Response {
    let settings_value = settings::read_settings(&state.config);
    let mut patterns = settings_value
        .get("enabledModels")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    match body.op.as_deref() {
        Some("clear") => patterns.clear(),
        Some("models") | Some("provider") => {
            let enabled = body.enabled.unwrap_or(true);
            let refs = if let Some(provider) = body.provider {
                state
                    .models()
                    .map(|data| {
                        data.model_list
                            .iter()
                            .filter(|model| model.provider == provider)
                            .map(|model| format!("{}/{}", model.provider, model.id))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            } else {
                body.refs.unwrap_or_default()
            };
            for reference in refs {
                let pattern = reference.clone();
                if enabled {
                    if !patterns.contains(&pattern) {
                        patterns.push(pattern);
                    }
                } else {
                    patterns.retain(|existing| existing != &pattern);
                    patterns.push(format!("!{pattern}"));
                }
            }
        }
        Some("prune") => {}
        Some("resync") => {
            // Repair the patterns after models.json changed: carry renames
            // through each entry and keep fully enabled providers fully on.
            for pattern in &mut patterns {
                for rename in &body.model_renames {
                    if pattern == &rename.from {
                        *pattern = rename.to.clone();
                    }
                }
                if let Some((provider, rest)) = pattern.split_once('/') {
                    if let Some(rename) = body.renames.iter().find(|rename| rename.from == provider)
                    {
                        *pattern = format!("{}/{}", rename.to, rest);
                    }
                }
            }
            let available = state
                .models()
                .map(|data| data.model_list)
                .unwrap_or_default();
            for provider in &body.fully_enabled {
                let provider = body
                    .renames
                    .iter()
                    .find(|rename| &rename.from == provider)
                    .map(|rename| rename.to.clone())
                    .unwrap_or_else(|| provider.clone());
                for model in available.iter().filter(|model| model.provider == provider) {
                    let reference = format!("{}/{}", model.provider, model.id);
                    if !patterns.contains(&reference) {
                        patterns.push(reference);
                    }
                }
            }
        }
        _ => return bad_request("Invalid op"),
    }
    if let Err(error) = settings::set_enabled_models(&state.config, patterns) {
        return internal(error);
    }
    state.invalidate_models();
    match enabled_models_view(&state) {
        Ok(view) => no_store(view),
        Err(error) => internal(error),
    }
}

pub async fn refresh_models() -> Response {
    no_store(json!({"ok": true}))
}

pub async fn models_config(State(state): State<Arc<AppState>>) -> Response {
    match std::fs::read_to_string(state.config.models_path()) {
        Ok(text) => match serde_json::from_str::<Value>(&strip_json_comments(&text)) {
            Ok(value) => no_store(value),
            Err(error) => crate::routes::json_error(
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                error.to_string(),
            ),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            no_store(json!({"providers": {}}))
        }
        Err(error) => internal(error),
    }
}

pub async fn set_models_config(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    if !body.is_object() {
        return bad_request("Expected a JSON object");
    }
    if let Err(error) = pi_star_core::models::check_image_max_bytes(&body) {
        return bad_request(error);
    }
    if let Some(providers) = body.get("providers").and_then(Value::as_object) {
        for (name, provider) in providers {
            if provider.get("api").and_then(Value::as_str) == Some("oauth")
                && provider.get("baseUrl").and_then(Value::as_str).is_none()
            {
                return bad_request(format!("Provider {name} uses oauth but has no Base URL"));
            }
        }
    }
    match settings::write_json_atomic(&state.config.models_path(), &body, 0o600) {
        Ok(()) => {
            state.invalidate_models();
            no_store(json!({"success": true}))
        }
        Err(error) => internal(error),
    }
}

pub async fn catalog() -> Response {
    no_store(json!({"models": [], "recommendation": null, "source": "https://models.dev/api.json"}))
}

#[derive(Debug, Deserialize)]
pub struct DiscoverBody {
    #[serde(rename = "providerName")]
    provider_name: Option<String>,
    provider: Option<Value>,
}

pub async fn discover(
    State(_state): State<Arc<AppState>>,
    Json(body): Json<DiscoverBody>,
) -> Response {
    let Some(name) = body.provider_name else {
        return bad_request("providerName is required");
    };
    let provider = body.provider.unwrap_or(Value::Null);
    let base_url = provider
        .get("baseUrl")
        .and_then(Value::as_str)
        .unwrap_or("");
    if base_url.is_empty() {
        return bad_request("Base URL is required");
    }
    let api = provider
        .get("api")
        .and_then(Value::as_str)
        .unwrap_or("openai-completions");
    let api_key = provider.get("apiKey").and_then(Value::as_str);
    let url = if base_url.trim_end_matches('/').ends_with("/v1") {
        format!("{}/models", base_url.trim_end_matches('/'))
    } else {
        format!("{}/v1/models", base_url.trim_end_matches('/'))
    };
    let client = reqwest::Client::new();
    let mut request = client.get(&url).header("Accept", "application/json");
    if let Some(key) = api_key {
        if api == "anthropic-messages" {
            request = request
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01");
        } else {
            request = request.bearer_auth(key);
        }
    }
    match request.send().await {
        Ok(response) => {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            if !status.is_success() {
                return crate::routes::json_error(
                    axum::http::StatusCode::BAD_GATEWAY,
                    format!(
                        "Upstream returned HTTP {status}: {}",
                        &text[..text.len().min(500)]
                    ),
                );
            }
            let payload: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let models = payload
                .get("data")
                .and_then(Value::as_array)
                .or_else(|| payload.get("models").and_then(Value::as_array))
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|model| {
                    let id = model.get("id").and_then(Value::as_str)?;
                    Some(json!({"id": id, "name": model.get("name").and_then(Value::as_str).unwrap_or(id)}))
                })
                .collect::<Vec<_>>();
            if models.is_empty() {
                return crate::routes::json_error(
                    axum::http::StatusCode::BAD_GATEWAY,
                    "No models found in the upstream response",
                );
            }
            let _ = name;
            no_store(json!({"models": models, "endpoint": url}))
        }
        Err(error) => {
            crate::routes::json_error(axum::http::StatusCode::BAD_GATEWAY, error.to_string())
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct TestModelBody {
    #[serde(rename = "providerName")]
    provider_name: Option<String>,
    provider: Option<Value>,
    model: Option<Value>,
}

pub async fn test_model(
    State(state): State<Arc<AppState>>,
    Json(body): Json<TestModelBody>,
) -> Response {
    let Some(provider_name) = body.provider_name else {
        return bad_request("providerName is required");
    };
    let provider = body.provider.unwrap_or(Value::Null);
    let model = body.model.unwrap_or(Value::Null);
    let Some(model_id) = model.get("id").and_then(Value::as_str) else {
        return bad_request("Model ID is required");
    };
    let base_url = provider
        .get("baseUrl")
        .and_then(Value::as_str)
        .unwrap_or("");
    let api_key = provider.get("apiKey").and_then(Value::as_str);
    if base_url.is_empty() {
        return bad_request("Base URL is required");
    }
    if api_key.is_none() {
        return no_store(
            json!({"ok": false, "error": format!("No API key found for \"{provider_name}\"")}),
        );
    }
    let api = provider
        .get("api")
        .and_then(Value::as_str)
        .unwrap_or("openai-completions");
    let started = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap_or_default();
    let response = if api == "anthropic-messages" {
        client
            .post(format!("{}/v1/messages", base_url.trim_end_matches('/')))
            .header("x-api-key", api_key.unwrap_or_default())
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": model_id,
                "max_tokens": 16,
                "messages": [{"role":"user","content":"Reply with OK only."}]
            }))
            .send()
            .await
    } else {
        client
            .post(format!(
                "{}/v1/chat/completions",
                base_url.trim_end_matches('/')
            ))
            .bearer_auth(api_key.unwrap_or_default())
            .json(&json!({
                "model": model_id,
                "max_tokens": 16,
                "messages": [{"role":"user","content":"Reply with OK only."}]
            }))
            .send()
            .await
    };
    let _ = state;
    match response {
        Ok(response) => {
            let status = response.status();
            let ok = status.is_success();
            let text = response.text().await.unwrap_or_default();
            no_store(json!({
                "ok": ok,
                "latencyMs": started.elapsed().as_millis() as u64,
                "status": status.as_u16(),
                "responseText": text.chars().take(300).collect::<String>(),
            }))
        }
        Err(error) => no_store(json!({"ok": false, "error": error.to_string()})),
    }
}

pub async fn providers(State(state): State<Arc<AppState>>) -> Response {
    let defs = model_defs(&state.config).unwrap_or_default();
    let auth = read_json(&state.config.agent_dir.join("auth.json")).unwrap_or(Value::Null);
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for model in defs.values() {
        *counts.entry(model.provider.clone()).or_default() += 1;
    }
    let anthropic_logged_in = auth
        .get("anthropic")
        .and_then(|entry| entry.get("type"))
        .and_then(Value::as_str)
        == Some("oauth");
    let mut providers = counts
        .into_iter()
        .map(|(id, count)| {
            json!({
                "id": id,
                "name": id,
                "hasApiKeyLogin": true,
                "hasOAuth": id == "anthropic",
                "status": {"configured": auth.get(&id).is_some()},
                "modelCount": count,
                "supportsOAuth": id == "anthropic",
                "configured": auth.get(&id).is_some(),
                "displayName": id,
            })
        })
        .collect::<Vec<_>>();
    if !providers
        .iter()
        .any(|provider| provider.get("id").and_then(Value::as_str) == Some("anthropic"))
    {
        providers.push(json!({
            "id": "anthropic",
            "name": "Anthropic",
            "hasApiKeyLogin": true,
            "hasOAuth": true,
            "status": {"configured": auth.get("anthropic").is_some()},
            "modelCount": 0,
            "supportsOAuth": true,
            "configured": auth.get("anthropic").is_some(),
            "displayName": "Anthropic",
        }));
    }
    let oauth_providers = vec![
        json!({
            "id": "anthropic",
            "name": "Anthropic (Claude Pro/Max)",
            "usesCallbackServer": true,
            "loggedIn": anthropic_logged_in,
            "supportsApiKey": true,
        }),
        json!({
            "id": "github-copilot",
            "name": "GitHub Copilot",
            "usesCallbackServer": false,
            "loggedIn": auth.get("github-copilot").and_then(|entry| entry.get("type")).and_then(Value::as_str) == Some("oauth"),
            "supportsApiKey": false,
        }),
    ];
    no_store(json!({
        "providers": providers,
        "oauthProviders": oauth_providers,
        "apiKeyProviders": providers,
    }))
}

pub async fn logout(
    State(state): State<Arc<AppState>>,
    AxumPath(provider): AxumPath<String>,
) -> Response {
    let path = state.config.agent_dir.join("auth.json");
    let mut auth = read_json(&path).unwrap_or_else(|| json!({}));
    if let Some(object) = auth.as_object_mut() {
        object.remove(&provider);
    }
    match settings::write_json_atomic(&path, &auth, 0o600) {
        Ok(()) => no_store(json!({"ok": true})),
        Err(error) => internal(error),
    }
}

#[derive(Debug, Deserialize)]
pub struct ApiKeyBody {
    #[serde(rename = "apiKey")]
    api_key: Option<String>,
    key: Option<String>,
}

pub async fn set_api_key(
    State(state): State<Arc<AppState>>,
    AxumPath(provider): AxumPath<String>,
    Json(body): Json<ApiKeyBody>,
) -> Response {
    let Some(key) = body
        .api_key
        .or(body.key)
        .filter(|key| !key.trim().is_empty())
    else {
        return bad_request("apiKey is required");
    };
    let path = state.config.agent_dir.join("auth.json");
    let mut auth = read_json(&path).unwrap_or_else(|| json!({}));
    let Some(object) = auth.as_object_mut() else {
        return internal("auth.json is not an object");
    };
    object.insert(provider, json!({"type": "api_key", "key": key}));
    match settings::write_json_atomic(&path, &auth, 0o600) {
        Ok(()) => no_store(json!({"ok": true})),
        Err(error) => internal(error),
    }
}

pub async fn delete_api_key(
    State(state): State<Arc<AppState>>,
    AxumPath(provider): AxumPath<String>,
) -> Response {
    let path = state.config.agent_dir.join("auth.json");
    let mut auth = read_json(&path).unwrap_or_else(|| json!({}));
    if let Some(object) = auth.as_object_mut() {
        object.remove(&provider);
    }
    match settings::write_json_atomic(&path, &auth, 0o600) {
        Ok(()) => no_store(json!({"ok": true})),
        Err(error) => internal(error),
    }
}

pub async fn tools(State(state): State<Arc<AppState>>) -> Response {
    no_store(settings::tools_settings(&state.config))
}

pub async fn set_tools(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    match settings::update_tools_settings(&state.config, &body) {
        Ok(settings) => no_store(settings),
        Err(error) => internal(error),
    }
}

pub async fn cache_miss(State(state): State<Arc<AppState>>) -> Response {
    no_store(json!({"showCacheMissNotices": settings::cache_miss_notices(&state.config)}))
}

pub async fn set_cache_miss(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    let enabled = body
        .get("showCacheMissNotices")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match settings::set_cache_miss_notices(&state.config, enabled) {
        Ok(value) => no_store(json!({"showCacheMissNotices": value})),
        Err(error) => internal(error),
    }
}

pub async fn subagent_settings(State(state): State<Arc<AppState>>) -> Response {
    no_store(settings::subagent_settings(&state.config))
}

pub async fn set_subagent_settings(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    match settings::set_subagent_settings(&state.config, &body) {
        Ok(settings) => no_store(settings),
        Err(error) => internal(error),
    }
}

pub async fn subagent_profiles(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CwdQuery>,
) -> Response {
    let cwd = query.cwd.map(PathBuf::from);
    no_store(json!({"profiles": scan_subagent_profiles(&state, cwd.as_deref())}))
}

pub async fn save_subagent_profile(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    let Some(profile) = body.get("profile") else {
        return bad_request("profile required");
    };
    let Some(name) = profile
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
    else {
        return bad_request("profile.name required");
    };
    if name.contains('/') || name.contains('\\') || name == ".." {
        return bad_request("invalid profile name");
    }
    let scope = body
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("global");
    let dir = match subagent_profile_dir(&state, scope, body.get("cwd").and_then(Value::as_str)) {
        Some(dir) => dir,
        None => return bad_request("cwd is required for a project profile"),
    };
    if let Err(error) = std::fs::create_dir_all(&dir) {
        return internal(error);
    }
    let path = dir.join(format!("{name}.md"));
    let text = render_subagent_profile(profile);
    match std::fs::write(&path, text) {
        Ok(()) => no_store(json!({
            "profile": subagent_profile_from_file(&path, scope)
        })),
        Err(error) => internal(error),
    }
}

pub async fn toggle_subagent_profile(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    let Some(name) = body.get("name").and_then(Value::as_str) else {
        return bad_request("name required");
    };
    let Some(enabled) = body.get("enabled").and_then(Value::as_bool) else {
        return bad_request("enabled required");
    };
    let scope = body
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("global");
    let Some(dir) = subagent_profile_dir(&state, scope, body.get("cwd").and_then(Value::as_str))
    else {
        return bad_request("cwd is required for a project profile");
    };
    let path = dir.join(format!("{name}.md"));
    let Ok(text) = std::fs::read_to_string(&path) else {
        return not_found("Agent profile not found");
    };
    let updated = set_frontmatter_bool(&text, "enabled", enabled);
    match std::fs::write(&path, updated) {
        Ok(()) => no_store(json!({"profile": subagent_profile_from_file(&path, scope)})),
        Err(error) => internal(error),
    }
}

pub async fn delete_subagent_profile(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    let Some(name) = body.get("name").and_then(Value::as_str) else {
        return bad_request("name required");
    };
    let scope = body
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("global");
    let Some(dir) = subagent_profile_dir(&state, scope, body.get("cwd").and_then(Value::as_str))
    else {
        return bad_request("cwd is required for a project profile");
    };
    match std::fs::remove_file(dir.join(format!("{name}.md"))) {
        Ok(()) => no_store(json!({"ok": true})),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            not_found("Agent profile not found")
        }
        Err(error) => internal(error),
    }
}

fn subagent_profile_dir(state: &AppState, scope: &str, cwd: Option<&str>) -> Option<PathBuf> {
    if scope == "project" {
        Some(PathBuf::from(cwd?).join(".pi").join("agents"))
    } else {
        Some(state.config.agent_dir.join("agents"))
    }
}

fn scan_subagent_profiles(state: &AppState, cwd: Option<&Path>) -> Vec<Value> {
    let mut profiles = Vec::new();
    let mut dirs = vec![(state.config.agent_dir.join("agents"), "global")];
    if let Some(cwd) = cwd {
        dirs.push((cwd.join(".pi").join("agents"), "project"));
    }
    for (dir, scope) in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            if let Some(profile) = subagent_profile_from_file(&path, scope) {
                profiles.push(profile);
            }
        }
    }
    profiles.sort_by(|a, b| {
        a.get("name")
            .and_then(Value::as_str)
            .cmp(&b.get("name").and_then(Value::as_str))
    });
    profiles
}

fn subagent_profile_from_file(path: &Path, scope: &str) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let mut name = path.file_stem()?.to_string_lossy().into_owned();
    let mut description = String::new();
    let mut tools = Vec::new();
    let mut model = None;
    let mut thinking = None;
    let mut enabled = true;
    let mut body = text.clone();
    if lines.next()?.trim() == "---" {
        let mut offset = 4;
        for line in text.lines().skip(1) {
            offset += line.len() + 1;
            if line.trim() == "---" {
                body = text.get(offset..).unwrap_or("").to_string();
                break;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim().trim_matches('"').trim_matches('\'');
            match key.trim() {
                "name" => name = value.to_string(),
                "description" => description = value.to_string(),
                "tools" => {
                    tools = value
                        .trim_matches('[')
                        .trim_matches(']')
                        .split(',')
                        .map(str::trim)
                        .filter(|item| !item.is_empty())
                        .map(str::to_owned)
                        .collect()
                }
                "model" => model = Some(value.to_string()),
                "thinking" => thinking = Some(value.to_string()),
                "enabled" => enabled = value != "false",
                _ => {}
            }
        }
    }
    Some(json!({
        "name": name,
        "displayName": name,
        "description": description,
        "systemPrompt": body.trim(),
        "tools": tools,
        "loadSkills": true,
        "loadExtensions": true,
        "inheritContext": true,
        "runInBackground": false,
        "promptMode": "append",
        "enabled": enabled,
        "scope": scope,
        "filePath": path.to_string_lossy(),
        "model": model,
        "thinking": thinking,
    }))
}

fn render_subagent_profile(profile: &Value) -> String {
    let text = |key: &str| profile.get(key).and_then(Value::as_str).unwrap_or("");
    let name = text("name");
    let mut frontmatter = String::new();
    frontmatter.push_str("---\n");
    frontmatter.push_str(&format!("name: {name}\n"));
    if !text("description").is_empty() {
        frontmatter.push_str(&format!("description: {}\n", text("description")));
    }
    if let Some(tools) = profile.get("tools").and_then(Value::as_array) {
        let tools = tools
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        if !tools.is_empty() {
            frontmatter.push_str(&format!("tools: [{tools}]\n"));
        }
    }
    if !text("model").is_empty() {
        frontmatter.push_str(&format!("model: {}\n", text("model")));
    }
    if !text("thinking").is_empty() {
        frontmatter.push_str(&format!("thinking: {}\n", text("thinking")));
    }
    if profile.get("enabled").and_then(Value::as_bool) == Some(false) {
        frontmatter.push_str("enabled: false\n");
    }
    frontmatter.push_str("---\n");
    frontmatter.push_str(text("systemPrompt"));
    frontmatter.push('\n');
    frontmatter
}

fn set_frontmatter_bool(text: &str, key: &str, value: bool) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    if lines.first().map(|line| line.trim()) != Some("---") {
        return text.to_string();
    }
    let end = lines
        .iter()
        .skip(1)
        .position(|line| line.trim() == "---")
        .map(|index| index + 1);
    let Some(end) = end else {
        return text.to_string();
    };
    lines.retain(|line| !line.trim_start().starts_with(&format!("{key}:")));
    if !value {
        lines.insert(end, format!("{key}: false"));
    }
    let mut result = lines.join("\n");
    result.push('\n');
    result
}

pub async fn subagent_view(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let Some(path) = pi_star_core::session::resolve_session_path(&state.config, &id) else {
        return not_found("Subagent not found");
    };
    let Ok(session) = pi_star_core::session::read_session(&path) else {
        return not_found("Subagent not found");
    };
    let info = session.info();
    let relation = info.relation.and_then(|relation| match relation {
        pi_star_core::session::SessionRelation::Subagent {
            parent_session_id,
            profile,
            description,
            status,
        } => Some(json!({
            "sessionId": id,
            "parentSessionId": parent_session_id,
            "profile": profile,
            "description": description,
            "status": status,
        })),
        _ => None,
    });
    match relation {
        Some(run) => no_store(json!({ "run": run })),
        None => not_found("Subagent not found"),
    }
}

pub async fn subagent_action(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let action = body.get("action").and_then(Value::as_str).unwrap_or("");
    let Some(runtime) = state.agents.get(&id) else {
        return crate::routes::json_error(
            axum::http::StatusCode::CONFLICT,
            "Subagent is not running",
        );
    };
    match action {
        "abort" => {
            runtime.abort();
            no_store(json!({"ok": true}))
        }
        "steer" => {
            let Some(message) = body.get("message").and_then(Value::as_str) else {
                return bad_request("message required");
            };
            match runtime.send(&json!({
                "type": "prompt",
                "message": message,
                "streamingBehavior": "steer",
            })) {
                Ok(_) => no_store(json!({"ok": true})),
                Err(error) => internal(error),
            }
        }
        _ => bad_request("action must be steer or abort"),
    }
}

pub async fn project_trust(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CwdQuery>,
) -> Response {
    let Some(cwd) = query.cwd else {
        return bad_request("cwd is required");
    };
    let cwd = PathBuf::from(cwd);
    if !state.roots.is_allowed(&cwd) {
        return forbidden("Access denied");
    }
    if !cwd.is_dir() {
        return not_found("Directory not found");
    }
    let status = settings::project_trust(&state.config, &cwd);
    let listing = project_mcp_listing(&cwd);
    no_store(json!({
        "requiresTrust": status.requires_trust,
        "trusted": status.trusted,
        "decision": status.decision,
        "decisionPath": status.decision_path,
        "inherited": status.inherited,
        "decisionError": status.decision_error,
        "mcpServers": listing,
    }))
}

fn project_mcp_listing(cwd: &Path) -> Vec<Value> {
    let path = cwd.join(".pi").join("mcp.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    value
        .get("mcpServers")
        .and_then(Value::as_object)
        .map(|servers| {
            servers
                .iter()
                .filter_map(|(name, server)| {
                    server.as_object()?;
                    Some(json!({"name": name, "enabled": server.get("enabled").and_then(Value::as_bool).unwrap_or(true)}))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub async fn set_project_trust(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    let Some(cwd) = body.get("cwd").and_then(Value::as_str) else {
        return bad_request("cwd is required");
    };
    let cwd = PathBuf::from(cwd);
    let trusted = body.get("trusted").and_then(Value::as_bool).unwrap_or(true);
    match settings::set_project_trust(&state.config, &cwd, trusted) {
        Ok(status) => no_store(json!({
            "requiresTrust": status.requires_trust,
            "trusted": status.trusted,
            "decision": status.decision,
            "decisionPath": status.decision_path,
            "inherited": status.inherited,
            "mcpServers": project_mcp_listing(&cwd),
        })),
        Err(error) => internal(error),
    }
}

pub async fn mcp(State(state): State<Arc<AppState>>, Query(query): Query<CwdQuery>) -> Response {
    no_store(mcp_overview(&state, query.cwd.as_deref()))
}

fn mcp_overview(state: &AppState, cwd: Option<&str>) -> Value {
    let mut files = Vec::new();
    let mut servers = Vec::new();
    let global_path = state.config.agent_dir.join("mcp.json");
    collect_mcp_file(&global_path, "global", &mut files, &mut servers);
    if let Some(cwd) = cwd {
        let project_path = PathBuf::from(cwd).join(".pi").join("mcp.json");
        collect_mcp_file(&project_path, "project", &mut files, &mut servers);
    }
    // The panel renders Code mode even when the MCP section is not open: keep
    // the shape complete (`sandbox` is read unconditionally).
    let tools = settings::tools_settings(&state.config);
    let settings_path = state.config.settings_path().to_string_lossy().into_owned();
    let mut mode = json!({
        "value": tools.codemode_mode.value,
        "settingsPath": settings_path,
    });
    if let Some(invalid) = tools.codemode_mode.invalid {
        mode["invalid"] = Value::String(invalid);
    }
    let mut inline_budget = json!({
        "settingsPath": settings_path,
        "default": 3000,
        "max": 1_000_000,
    });
    if let Some(value) = tools.codemode_inline_budget.value {
        inline_budget["value"] = Value::from(value);
    }
    if let Some(invalid) = tools.codemode_inline_budget.invalid {
        inline_budget["invalid"] = Value::String(invalid);
    }
    json!({
        "mcp": {"available": true},
        "codemode": {
            "sandbox": {"state": "not-checked"},
            "builtinDisabled": false,
            "preference": tools.codemode,
            "mode": mode,
            "inlineBudget": inline_budget,
        },
        "files": files,
        "servers": servers,
    })
}

fn collect_mcp_file(path: &Path, scope: &str, files: &mut Vec<Value>, servers: &mut Vec<Value>) {
    let exists = path.is_file();
    let mut problems = Vec::new();
    let mut entries: Vec<(String, Value)> = Vec::new();
    if exists {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(value) => {
                    if let Some(server_map) = value.get("mcpServers").and_then(Value::as_object) {
                        entries.extend(
                            server_map
                                .iter()
                                .map(|(name, server)| (name.clone(), server.clone())),
                        );
                    } else {
                        problems.push(json!({"reason": "invalid-shape", "error": "mcpServers must be an object"}));
                    }
                }
                Err(error) => {
                    problems.push(json!({"reason": "unparsable", "error": error.to_string()}))
                }
            },
            Err(error) => {
                problems.push(json!({"reason": "unreadable", "error": error.to_string()}))
            }
        }
    }
    files.push(json!({
        "scope": scope,
        "path": path.to_string_lossy(),
        "exists": exists,
        "problems": problems,
    }));
    for (name, server) in entries {
        let object = server.as_object();
        let config_key = hex::encode(Sha256::digest(
            serde_json::to_vec(&server).unwrap_or_default(),
        ));
        let env_names = object
            .and_then(|object| object.get("env"))
            .and_then(Value::as_object)
            .map(|env| env.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let header_names = object
            .and_then(|object| object.get("headers"))
            .and_then(Value::as_object)
            .map(|headers| headers.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let url = object
            .and_then(|object| object.get("url"))
            .and_then(Value::as_str);
        servers.push(json!({
            "name": name,
            "scope": scope,
            "sourcePath": path.to_string_lossy(),
            "configKey": config_key,
            "enabled": object.and_then(|object| object.get("enabled")).and_then(Value::as_bool).unwrap_or(true),
            "validated": true,
            "transport": if url.is_some() { "http" } else { "stdio" },
            "exposure": object.and_then(|object| object.get("exposure")).and_then(Value::as_str).unwrap_or("default"),
            "command": object.and_then(|object| object.get("command")).and_then(Value::as_str),
            "args": object.and_then(|object| object.get("args")).cloned().unwrap_or_else(|| json!([])),
            "envNames": env_names,
            "headerNames": header_names,
            "url": url,
            "usesOAuth": false,
            "commandFields": [],
            "variableReferences": [],
        }));
    }
}

pub async fn mcp_post(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let action = body.get("action").and_then(Value::as_str).unwrap_or("");
    let scope = body
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("global");
    let path = if scope == "project" {
        let Some(cwd) = body.get("cwd").and_then(Value::as_str) else {
            return bad_request("A project server needs the project's cwd");
        };
        PathBuf::from(cwd).join(".pi").join("mcp.json")
    } else {
        state.config.agent_dir.join("mcp.json")
    };
    let mut config = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_else(|| json!({"mcpServers": {}}));
    let Some(servers) = config.get_mut("mcpServers").and_then(Value::as_object_mut) else {
        return internal("mcp.json has no mcpServers object");
    };
    match action {
        "add" => {
            let Some(name) = body.get("name").and_then(Value::as_str) else {
                return bad_request("name is required");
            };
            let server = body
                .get("server")
                .or_else(|| body.get("config"))
                .cloned()
                .unwrap_or_else(|| json!({}));
            servers.insert(name.to_string(), server);
        }
        "remove" => {
            let Some(name) = body.get("name").and_then(Value::as_str) else {
                return bad_request("name is required");
            };
            servers.remove(name);
        }
        "set-enabled" | "enable" | "disable" => {
            let enabled =
                action != "disable" && body.get("enabled").and_then(Value::as_bool).unwrap_or(true);
            if let Some(names) = body.get("servers").and_then(Value::as_array) {
                for name in names.iter().filter_map(Value::as_str) {
                    if let Some(server) = servers.get_mut(name).and_then(Value::as_object_mut) {
                        server.insert("enabled".into(), Value::Bool(enabled));
                    }
                }
            } else if let Some(name) = body.get("name").and_then(Value::as_str) {
                if let Some(server) = servers.get_mut(name).and_then(Value::as_object_mut) {
                    server.insert("enabled".into(), Value::Bool(enabled));
                }
            }
        }
        "set-exposure" => {
            let Some(name) = body.get("name").and_then(Value::as_str) else {
                return bad_request("name is required");
            };
            if let Some(server) = servers.get_mut(name).and_then(Value::as_object_mut) {
                if let Some(exposure) = body.get("exposure").cloned() {
                    server.insert("exposure".into(), exposure);
                }
            }
        }
        "undo" | "set-in-project" | "sign-out" => {}
        _ => return bad_request("Invalid action"),
    }
    if let Err(error) = settings::write_json_atomic(&path, &config, 0o600) {
        return internal(error);
    }
    no_store(mcp_overview(
        &state,
        body.get("cwd").and_then(Value::as_str),
    ))
}

#[derive(Debug, Deserialize)]
pub struct McpTestBody {
    scope: Option<String>,
    name: Option<String>,
    cwd: Option<String>,
}

pub async fn mcp_test(
    State(state): State<Arc<AppState>>,
    Json(body): Json<McpTestBody>,
) -> Response {
    let name = body.name.unwrap_or_default();
    let scope = body.scope.unwrap_or_else(|| "global".to_string());
    let test_cwd = body.cwd.clone();
    if name.is_empty() {
        return bad_request("name is required");
    }
    let path = if scope == "project" {
        let Some(cwd) = body.cwd else {
            return bad_request("cwd is required for a project server");
        };
        let cwd = PathBuf::from(cwd);
        if !state.roots.is_allowed_existing(&cwd) {
            return forbidden("Access denied");
        }
        if !settings::project_trust(&state.config, &cwd).trusted {
            return forbidden("The project is not trusted");
        }
        cwd.join(".pi").join("mcp.json")
    } else {
        state.config.agent_dir.join("mcp.json")
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return not_found("MCP server not found");
    };
    let Ok(config) = serde_json::from_str::<Value>(&text) else {
        return bad_request("mcp.json is not valid JSON");
    };
    let Some(entry) = config
        .get("mcpServers")
        .and_then(Value::as_object)
        .and_then(|servers| servers.get(&name))
        .cloned()
    else {
        return not_found("MCP server not found");
    };
    let config_key = hex::encode(Sha256::digest(
        serde_json::to_vec(&entry).unwrap_or_default(),
    ));
    let result = run_mcp_test(&entry, test_cwd.as_deref()).await;
    no_store(json!({
        "name": name,
        "scope": scope,
        "configKey": config_key,
        "result": result,
    }))
}

async fn run_mcp_test(entry: &Value, cwd: Option<&str>) -> Value {
    let started = std::time::Instant::now();
    let result = if let Some(url) = entry.get("url").and_then(Value::as_str) {
        mcp_test_http(entry, url).await
    } else {
        mcp_test_stdio(entry, cwd).await
    };
    match result {
        Ok(mut value) => {
            value["durationMs"] = json!(started.elapsed().as_millis() as u64);
            value["testedAt"] = json!(chrono::Utc::now().timestamp_millis());
            value
        }
        Err(error) => json!({
            "state": "failed",
            "error": error,
            "tools": [],
            "toolCount": 0,
            "durationMs": started.elapsed().as_millis() as u64,
            "testedAt": chrono::Utc::now().timestamp_millis(),
        }),
    }
}

fn mcp_headers(entry: &Value) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(object) = entry.get("headers").and_then(Value::as_object) {
        for (name, value) in object {
            if let (Ok(name), Some(value)) = (
                reqwest::header::HeaderName::from_bytes(name.as_bytes()),
                expand_env(value.as_str().unwrap_or("")).as_str().into(),
            ) {
                if let Ok(value) = reqwest::header::HeaderValue::from_str(value) {
                    headers.insert(name, value);
                }
            }
        }
    }
    headers
}

async fn mcp_test_http(entry: &Value, url: &str) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())?;
    let headers = mcp_headers(entry);
    let initialize = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "pi-star", "version": env!("CARGO_PKG_VERSION")}
        }
    });
    let response = client
        .post(url)
        .headers(headers.clone())
        .header("Accept", "application/json, text/event-stream")
        .json(&initialize)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let init = response_json(response).await?;
    let server_info = init
        .get("result")
        .and_then(|result| result.get("serverInfo"))
        .cloned();
    let initialized = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
    let _ = client
        .post(url)
        .headers(headers.clone())
        .json(&initialized)
        .send()
        .await;
    let tools = json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}});
    let response = client
        .post(url)
        .headers(headers)
        .header("Accept", "application/json, text/event-stream")
        .json(&tools)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let tools_value = response_json(response).await?;
    let tools = tools_value
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .map(|tool| {
                    let read_only = tool
                        .pointer("/annotations/readOnlyHint")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    json!({
                        "name": tool.get("name").and_then(Value::as_str).unwrap_or(""),
                        "description": tool.get("description").and_then(Value::as_str).map(|value| value.lines().next().unwrap_or("").chars().take(160).collect::<String>()),
                        "readOnly": read_only,
                        "exposure": "default",
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(json!({
        "state": "connected",
        "tools": tools,
        "toolCount": tools.len(),
        "serverInfo": server_info.map(|info| json!({
            "name": info.get("name").and_then(Value::as_str).unwrap_or(""),
            "version": info.get("version").and_then(Value::as_str).unwrap_or(""),
        })),
    }))
}

async fn response_json(response: reqwest::Response) -> Result<Value, String> {
    let text = response.text().await.map_err(|error| error.to_string())?;
    if let Ok(value) = serde_json::from_str::<Value>(&text) {
        return Ok(value);
    }
    for line in text.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            if let Ok(value) = serde_json::from_str::<Value>(data.trim()) {
                return Ok(value);
            }
        }
    }
    Err("MCP server returned an invalid response".to_string())
}

#[allow(unused_assignments)]
async fn mcp_test_stdio(entry: &Value, cwd: Option<&str>) -> Result<Value, String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let command = entry
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| "MCP server has no command".to_string())?;
    let args: Vec<String> = entry
        .get("args")
        .and_then(Value::as_array)
        .map(|args| {
            args.iter()
                .filter_map(Value::as_str)
                .map(expand_env)
                .collect()
        })
        .unwrap_or_default();
    let mut child = tokio::process::Command::new(command)
        .args(&args)
        .current_dir(cwd.unwrap_or("."))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .env(
            "LANG",
            std::env::var("LANG").unwrap_or_else(|_| "C.UTF-8".into()),
        )
        .envs(
            entry
                .get("env")
                .and_then(Value::as_object)
                .map(|env| {
                    env.iter()
                        .map(|(name, value)| {
                            (name.clone(), expand_env(value.as_str().unwrap_or("")))
                        })
                        .collect::<std::collections::HashMap<_, _>>()
                })
                .unwrap_or_default(),
        )
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("could not start the server: {error}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "MCP server has no stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "MCP server has no stdout".to_string())?;
    let mut lines = BufReader::new(stdout).lines();
    let initialize = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "pi-star", "version": env!("CARGO_PKG_VERSION")}
        }
    });
    let write = async {
        stdin.write_all(initialize.to_string().as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await
    };
    write.await.map_err(|error| error.to_string())?;
    let mut server_info = Value::Null;
    let mut tools = Vec::new();
    let mut initialized = false;
    let deadline = tokio::time::sleep(std::time::Duration::from_secs(15));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => return Err("MCP server did not answer within 15 seconds".to_string()),
            line = lines.next_line() => {
                let Some(line) = line.map_err(|error| error.to_string())? else {
                    return Err("MCP server exited before answering".to_string());
                };
                let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
                match value.get("id").and_then(Value::as_i64) {
                    Some(1) => {
                        server_info = value.pointer("/result/serverInfo").cloned().unwrap_or(Value::Null);
                        if !initialized {
                            initialized = true;
                            let mut note = json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string();
                            note.push('\n');
                            let _ = stdin.write_all(note.as_bytes()).await;
                            let _ = stdin.flush().await;
                            let mut request = json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}).to_string();
                            request.push('\n');
                            let _ = stdin.write_all(request.as_bytes()).await;
                            let _ = stdin.flush().await;
                        }
                    }
                    Some(2) => {
                        tools = value
                            .pointer("/result/tools")
                            .and_then(Value::as_array)
                            .map(|tools| {
                                tools.iter().map(|tool| json!({
                                    "name": tool.get("name").and_then(Value::as_str).unwrap_or(""),
                                    "description": tool.get("description").and_then(Value::as_str).map(|value| value.lines().next().unwrap_or("").chars().take(160).collect::<String>()),
                                    "readOnly": tool.pointer("/annotations/readOnlyHint").and_then(Value::as_bool).unwrap_or(false),
                                    "exposure": "default",
                                })).collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
    let _ = child.kill().await;
    Ok(json!({
        "state": "connected",
        "tools": tools,
        "toolCount": tools.len(),
        "serverInfo": server_info,
        "cwd": cwd,
    }))
}

fn expand_env(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '$' {
            result.push(ch);
            continue;
        }
        if chars.peek() == Some(&'{') {
            chars.next();
            let mut name = String::new();
            for next in chars.by_ref() {
                if next == '}' {
                    break;
                }
                name.push(next);
            }
            result.push_str(&std::env::var(&name).unwrap_or_default());
        } else {
            let mut name = String::new();
            while let Some(next) = chars.peek() {
                if next.is_ascii_alphanumeric() || *next == '_' {
                    name.push(*next);
                    chars.next();
                } else {
                    break;
                }
            }
            if name.is_empty() {
                result.push('$');
            } else {
                result.push_str(&std::env::var(&name).unwrap_or_default());
            }
        }
    }
    result
}

pub async fn skills(State(state): State<Arc<AppState>>, Query(query): Query<CwdQuery>) -> Response {
    let Some(cwd) = query.cwd else {
        return bad_request("cwd required");
    };
    let cwd = PathBuf::from(cwd);
    if !state.roots.is_allowed_existing(&cwd) {
        return forbidden("Access denied");
    }
    match scan_skills(&state, &cwd) {
        Ok(skills) => no_store(json!({
            "skills": skills,
            "diagnostics": [],
            "projectResourcesLoaded": true,
        })),
        Err(error) => internal(error),
    }
}

fn skill_roots(state: &AppState, cwd: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = vec![
        state.config.agent_dir.join("skills"),
        settings::home_dir()
            .unwrap_or_default()
            .join(".agents")
            .join("skills"),
    ];
    if let Some(cwd) = cwd {
        roots.push(cwd.join(".pi").join("skills"));
        roots.push(cwd.join(".agents").join("skills"));
    }
    roots
}

fn scan_skills(state: &AppState, cwd: &Path) -> anyhow::Result<Vec<Value>> {
    let mut roots = skill_roots(state, Some(cwd));
    let mut skills = Vec::new();
    for root in roots.drain(..) {
        if !root.is_dir() {
            continue;
        }
        for entry in ignore::WalkBuilder::new(&root)
            .max_depth(Some(3))
            .build()
            .flatten()
        {
            if entry.file_type().is_some_and(|kind| kind.is_file())
                && entry.path().file_name().and_then(|name| name.to_str()) == Some("SKILL.md")
            {
                if let Ok(text) = std::fs::read_to_string(entry.path()) {
                    let (name, description, disabled) = parse_skill_frontmatter(&text)
                        .unwrap_or_else(|| {
                            (
                                entry
                                    .path()
                                    .parent()
                                    .and_then(|path| path.file_name())
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("skill")
                                    .to_string(),
                                String::new(),
                                false,
                            )
                        });
                    let base_dir = entry.path().parent().unwrap_or(entry.path());
                    let scope = if base_dir.starts_with(cwd) {
                        "project"
                    } else {
                        "global"
                    };
                    let mut skill = json!({
                        "name": name,
                        "description": description,
                        "filePath": entry.path().to_string_lossy(),
                        "baseDir": base_dir.to_string_lossy(),
                        "disableModelInvocation": disabled,
                        "sourceInfo": {"scope": scope},
                    });
                    if let Some(install) =
                        crate::routes::skills_install::install_info(state, cwd, scope, &name)
                    {
                        skill["install"] = install;
                    }
                    skills.push(skill);
                }
            }
        }
    }
    Ok(skills)
}

fn parse_skill_frontmatter(text: &str) -> Option<(String, String, bool)> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut name = None;
    let mut description = None;
    let mut disabled = false;
    for line in lines {
        let line = line.trim();
        if line == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("name:") {
            name = Some(
                value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string(),
            );
        } else if let Some(value) = line.strip_prefix("description:") {
            description = Some(
                value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string(),
            );
        } else if line.starts_with("disable-model-invocation:") {
            disabled = line.ends_with("true");
        }
    }
    Some((name?, description.unwrap_or_default(), disabled))
}

pub async fn patch_skills(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let disable = body
        .get("disableModelInvocation")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut files = Vec::new();
    if let Some(path) = body.get("filePath").and_then(Value::as_str) {
        files.push(path.to_string());
    }
    if let Some(paths) = body.get("filePaths").and_then(Value::as_array) {
        files.extend(paths.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    let mut results = Vec::new();
    for file in files {
        let path = PathBuf::from(&file);
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            results.push(json!({"filePath": file, "error": "Not a skill file"}));
            continue;
        }
        if !state.roots.is_allowed_existing(&path) && !path.starts_with(&state.config.agent_dir) {
            results.push(json!({"filePath": file, "error": "Access denied"}));
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            results.push(json!({"filePath": file, "error": "file not found"}));
            continue;
        };
        let updated = set_skill_disabled(&text, disable);
        if updated != text {
            if let Err(error) = std::fs::write(&path, updated) {
                results.push(json!({"filePath": file, "error": error.to_string()}));
                continue;
            }
        }
        results.push(json!({"filePath": file}));
    }
    no_store(json!({"results": results}))
}

fn set_skill_disabled(text: &str, disabled: bool) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    if lines.first().map(|line| line.trim()) != Some("---") {
        return text.to_string();
    }
    let end = lines
        .iter()
        .skip(1)
        .position(|line| line.trim() == "---")
        .map(|index| index + 1);
    let Some(end) = end else {
        return text.to_string();
    };
    lines.retain(|line| !line.trim_start().starts_with("disable-model-invocation:"));
    if disabled {
        lines.insert(end, "disable-model-invocation: true".to_string());
    }
    let mut result = lines.join("\n");
    result.push('\n');
    result
}

const SKILL_CONTENT_MAX_BYTES: u64 = 1024 * 1024;

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        })
}

pub async fn skill_content(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SkillContentQuery>,
) -> Response {
    let Some(file_path) = query.file_path else {
        return bad_request("filePath is required");
    };
    let path = PathBuf::from(file_path);
    let cwd = query.cwd.as_deref().map(Path::new);
    let in_skill_root = skill_roots(&state, cwd)
        .iter()
        .any(|root| path.starts_with(root));
    if !state.roots.is_allowed_existing(&path)
        && !path.starts_with(&state.config.agent_dir)
        && !in_skill_root
    {
        return forbidden("Access denied");
    }
    let target = if let Some(document) = query.document {
        let document_path = Path::new(&document);
        if document_path.is_absolute()
            || document.contains(['\\', ':', '\0'])
            || document.split('/').any(|segment| segment == "..")
        {
            return bad_request("Document path must stay inside the skill folder");
        }
        if !is_markdown_path(document_path) {
            return crate::routes::json_error(
                axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Only Markdown documents can be opened",
            );
        }
        let Some(base) = path.parent() else {
            return bad_request("Skill document is outside the skill folder");
        };
        let candidate = base.join(document_path);
        if !candidate.is_file() {
            return not_found("Skill document not found");
        }
        let (Ok(base), Ok(candidate)) = (base.canonicalize(), candidate.canonicalize()) else {
            return not_found("Skill document not found");
        };
        if !candidate.starts_with(&base) || !is_markdown_path(&candidate) {
            return forbidden("Document is outside the skill folder");
        }
        candidate
    } else {
        path
    };
    if !target.is_file() {
        return not_found("Skill file not found");
    }
    if std::fs::metadata(&target).is_ok_and(|metadata| metadata.len() > SKILL_CONTENT_MAX_BYTES) {
        return crate::routes::json_error(
            axum::http::StatusCode::PAYLOAD_TOO_LARGE,
            "Skill file is larger than 1 MiB",
        );
    }
    match std::fs::read_to_string(&target) {
        Ok(text) => no_store(json!({"content": text})),
        Err(_) => not_found("Skill file not found"),
    }
}

#[derive(Debug, Deserialize)]
pub struct SkillContentQuery {
    #[serde(rename = "filePath")]
    file_path: Option<String>,
    cwd: Option<String>,
    document: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SkillSearchBody {
    query: Option<String>,
    limit: Option<u32>,
}

pub async fn skill_search(Json(body): Json<SkillSearchBody>) -> Response {
    let Some(query) = body.query.filter(|value| !value.trim().is_empty()) else {
        return bad_request("query required");
    };
    let limit = body.limit.unwrap_or(50).clamp(1, 50);
    let base = std::env::var("SKILLS_API_URL").unwrap_or_else(|_| "https://skills.sh".into());
    let url = format!(
        "{}/api/search?q={}&limit={limit}",
        base.trim_end_matches('/'),
        urlencoding(query.trim())
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap_or_default();
    let response = match client.get(&url).send().await {
        Ok(response) => response,
        Err(error) => return internal(error),
    };
    if !response.status().is_success() {
        return crate::routes::json_error(
            axum::http::StatusCode::BAD_GATEWAY,
            format!("skills.sh search failed: HTTP {}", response.status()),
        );
    }
    let payload: Value = response.json().await.unwrap_or(Value::Null);
    let results = payload
        .get("skills")
        .and_then(Value::as_array)
        .map(|skills| {
            skills
                .iter()
                .filter_map(|skill| {
                    let name = skill.get("name").and_then(Value::as_str)?.trim();
                    let source = skill.get("source").and_then(Value::as_str).unwrap_or("").trim();
                    let slug = skill.get("id").and_then(Value::as_str).unwrap_or("").trim();
                    if name.is_empty() || (source.is_empty() && slug.is_empty()) {
                        return None;
                    }
                    let package = format!("{}@{name}", if source.is_empty() { slug } else { source });
                    let installs = format_installs(skill.get("installs").and_then(Value::as_u64).unwrap_or(0));
                    Some(json!({
                        "package": package,
                        "installs": installs,
                        "url": if slug.is_empty() { String::new() } else { format!("{base}/{slug}") },
                    }))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    no_store(json!({"results": results}))
}

fn format_installs(count: u64) -> String {
    if count == 0 {
        String::new()
    } else if count >= 1_000_000 {
        format!("{:.1}M installs", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K installs", count as f64 / 1_000.0)
    } else {
        format!("{count} install{}", if count == 1 { "" } else { "s" })
    }
}

fn urlencoding(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

pub async fn plugins(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CwdQuery>,
) -> Response {
    no_store(plugins_view(&state, query.cwd.as_deref()))
}

fn plugins_view(state: &AppState, cwd: Option<&str>) -> Value {
    let mut packages = Vec::new();
    let global = settings::read_settings(&state.config);
    append_plugin_packages(&mut packages, state, global.get("packages"), "global", cwd);
    if let Some(cwd) = cwd {
        let project = std::fs::read_to_string(PathBuf::from(cwd).join(".pi").join("settings.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .unwrap_or(Value::Null);
        append_plugin_packages(
            &mut packages,
            state,
            project.get("packages"),
            "project",
            Some(cwd),
        );
    }
    json!({
        "packages": packages,
        "standaloneExtensions": [],
        "totals": {"extensions": packages.len(), "skills": 0, "prompts": 0, "themes": 0},
        "diagnostics": [],
        "projectResourcesLoaded": true,
    })
}

fn append_plugin_packages(
    packages: &mut Vec<Value>,
    state: &AppState,
    value: Option<&Value>,
    scope: &str,
    cwd: Option<&str>,
) {
    let Some(entries) = value.and_then(Value::as_array) else {
        return;
    };
    for entry in entries {
        let (source, disabled) = match entry {
            Value::String(source) => (source.clone(), false),
            Value::Object(object) => {
                let Some(source) = object.get("source").and_then(Value::as_str) else {
                    continue;
                };
                let disabled = ["extensions", "skills", "prompts", "themes"]
                    .iter()
                    .all(|key| {
                        object
                            .get(*key)
                            .and_then(Value::as_array)
                            .is_some_and(|items| items.is_empty())
                    });
                (source.to_string(), disabled)
            }
            _ => continue,
        };
        let installed_path =
            crate::routes::plugins_ops::install_path(state, &source, scope, cwd.map(Path::new));
        let mut package = json!({
            "source": source,
            "scope": scope,
            "canCheckForUpdates": true,
            "filtered": false,
            "disabled": disabled,
            "counts": {"extensions": 0, "skills": 0, "prompts": 0, "themes": 0},
            "resources": [],
            "status": if disabled { "disabled" } else { "installed" },
        });
        if let Some(path) = installed_path {
            if path.exists() {
                package["installedPath"] = json!(path.to_string_lossy());
                if let Ok(text) = std::fs::read_to_string(path.join("package.json")) {
                    if let Ok(manifest) = serde_json::from_str::<Value>(&text) {
                        package["packageName"] =
                            manifest.get("name").cloned().unwrap_or(Value::Null);
                        package["version"] =
                            manifest.get("version").cloned().unwrap_or(Value::Null);
                        package["description"] =
                            manifest.get("description").cloned().unwrap_or(Value::Null);
                    }
                }
            } else {
                package["status"] = json!("missing");
            }
        }
        packages.push(package);
    }
}

pub async fn plugins_post(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let action = body.get("action").and_then(Value::as_str).unwrap_or("");
    let scope = body
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("global");
    let source = body.get("source").and_then(Value::as_str).unwrap_or("");
    let cwd = body.get("cwd").and_then(Value::as_str).map(PathBuf::from);
    if matches!(action, "install" | "update") {
        if action == "install" {
            if source.is_empty() {
                return bad_request("source is required");
            }
            if let Err(error) =
                crate::routes::plugins_ops::install(&state, source, scope, cwd.as_deref()).await
            {
                return internal(error);
            }
        } else if source.is_empty() {
            for (item_source, item_scope) in configured_packages(&state, cwd.as_deref()) {
                if item_scope != scope {
                    continue;
                }
                if let Err(error) = crate::routes::plugins_ops::update(
                    &state,
                    &item_source,
                    &item_scope,
                    cwd.as_deref(),
                )
                .await
                {
                    return internal(error);
                }
            }
        } else if let Err(error) =
            crate::routes::plugins_ops::update(&state, source, scope, cwd.as_deref()).await
        {
            return internal(error);
        }
        return no_store(plugins_view(
            &state,
            cwd.as_deref().and_then(|path| path.to_str()),
        ));
    }
    if source.is_empty() {
        return bad_request("source is required");
    }
    if action == "remove" {
        if let Err(error) =
            crate::routes::plugins_ops::remove(&state, source, scope, cwd.as_deref())
        {
            return internal(error);
        }
    }
    let path = if scope == "project" {
        let Some(cwd) = &cwd else {
            return bad_request("cwd is required for a project package");
        };
        cwd.join(".pi").join("settings.json")
    } else {
        state.config.settings_path()
    };
    let mut config = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_else(|| json!({}));
    let Some(object) = config.as_object_mut() else {
        return internal("settings.json is not an object");
    };
    let packages = object
        .entry("packages".to_string())
        .or_insert_with(|| json!([]));
    let Some(packages) = packages.as_array_mut() else {
        return internal("packages is not an array");
    };
    match action {
        "remove" => packages.retain(|entry| plugin_source(entry) != Some(source)),
        "disable" | "enable" => {
            let disabled = action == "disable";
            for entry in packages.iter_mut() {
                if plugin_source(entry) != Some(source) {
                    continue;
                }
                if disabled {
                    *entry = json!({
                        "source": source,
                        "extensions": [],
                        "skills": [],
                        "prompts": [],
                        "themes": [],
                    });
                } else if let Some(object) = entry.as_object_mut() {
                    for key in ["extensions", "skills", "prompts", "themes"] {
                        object.remove(key);
                    }
                }
            }
        }
        _ => return bad_request("Invalid action"),
    }
    if let Err(error) = settings::write_json_atomic(&path, &config, 0o600) {
        return internal(error);
    }
    no_store(plugins_view(
        &state,
        cwd.as_deref().and_then(|path| path.to_str()),
    ))
}

fn configured_packages(state: &AppState, cwd: Option<&Path>) -> Vec<(String, String)> {
    let mut result = Vec::new();
    let global = settings::read_settings(&state.config);
    append_package_sources(&mut result, global.get("packages"), "global");
    if let Some(cwd) = cwd {
        let project = std::fs::read_to_string(cwd.join(".pi").join("settings.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .unwrap_or(Value::Null);
        append_package_sources(&mut result, project.get("packages"), "project");
    }
    result
}

fn append_package_sources(sources: &mut Vec<(String, String)>, value: Option<&Value>, scope: &str) {
    let Some(entries) = value.and_then(Value::as_array) else {
        return;
    };
    for entry in entries {
        let Some(source) = plugin_source(entry) else {
            continue;
        };
        sources.push((source.to_string(), scope.to_string()));
    }
}

fn plugin_source(entry: &Value) -> Option<&str> {
    entry
        .as_str()
        .or_else(|| entry.get("source").and_then(Value::as_str))
}

pub async fn plugins_check(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    let Some(cwd) = body.get("cwd").and_then(Value::as_str) else {
        return bad_request("cwd required");
    };
    let cwd = PathBuf::from(cwd);
    if !state.roots.is_allowed_existing(&cwd) {
        return forbidden("Access denied");
    }
    let requested = body.get("source").and_then(Value::as_str);
    let requested_scope = body.get("scope").and_then(Value::as_str);
    let mut sources = configured_packages(&state, Some(&cwd));
    if let (Some(source), Some(scope)) = (requested, requested_scope) {
        sources = vec![(source.to_string(), scope.to_string())];
    }
    let mut updates = Vec::new();
    for (source, scope) in sources {
        let parsed = match crate::routes::plugins_ops::parse_source(&source) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let (kind, name) = match &parsed.kind {
            crate::routes::plugins_ops::SourceKind::Npm(name) => ("npm", name.clone()),
            _ => ("git", source.clone()),
        };
        let installed =
            crate::routes::plugins_ops::install_path(&state, &source, &scope, Some(&cwd))
                .and_then(|path| std::fs::read_to_string(path.join("package.json")).ok())
                .and_then(|text| serde_json::from_str::<Value>(&text).ok());
        let version = installed
            .as_ref()
            .and_then(|manifest| manifest.get("version"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        if kind == "npm" {
            let output = tokio::process::Command::new("npm")
                .args(["view", &name, "version"])
                .output()
                .await;
            let latest = output
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
            let state_label = match latest.as_deref() {
                Some(latest) if latest != version => "update-available",
                Some(_) => "up-to-date",
                None => "unsupported",
            };
            updates.push(json!({
                "source": source,
                "scope": scope,
                "displayName": name,
                "type": "npm",
                "state": state_label,
                "message": latest.clone(),
            }));
        } else {
            updates.push(json!({
                "source": source,
                "scope": scope,
                "displayName": name,
                "type": "git",
                "state": "unsupported",
            }));
        }
    }
    if requested.is_some() && updates.is_empty() {
        return not_found("Configured package not found");
    }
    no_store(json!({"updates": updates}))
}

#[derive(Debug, Deserialize)]
pub struct ProviderUsageBody {
    #[serde(rename = "providerId")]
    provider_id: Option<String>,
}

pub async fn provider_usage(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ProviderUsageBody>,
) -> Response {
    let provider_id = body.provider_id.unwrap_or_default();
    let endpoint = match provider_id.as_str() {
        "deepseek" => "https://api.deepseek.com/user/balance",
        "openrouter" => "https://openrouter.ai/api/v1/key",
        "moonshotai" => "https://api.moonshot.ai/v1/users/me/balance",
        "moonshotai-cn" => "https://api.moonshot.cn/v1/users/me/balance",
        "vercel-ai-gateway" => "https://ai-gateway.vercel.sh/v1/credits",
        "opencode-go" => "https://opencode.ai/zen/go/v1/usage",
        "kimi-coding" => "https://api.kimi.com/coding/v1/usages",
        "minimax" => "https://api.minimax.io/v1/token_plan/remains",
        "minimax-cn" => "https://api.minimaxi.com/v1/token_plan/remains",
        "openai-codex" => "https://chatgpt.com/backend-api/wham/usage",
        _ => return bad_request("Unsupported provider"),
    };
    let auth = read_json(&state.config.agent_dir.join("auth.json")).unwrap_or(Value::Null);
    let Some(key) = auth
        .get(&provider_id)
        .and_then(|entry| entry.get("key"))
        .and_then(Value::as_str)
    else {
        return no_store(json!({
            "providerId": provider_id,
            "status": "auth-unavailable",
            "message": "Connect this provider before querying usage.",
        }));
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default();
    let response = match client.get(endpoint).bearer_auth(key).send().await {
        Ok(response) => response,
        Err(_) => {
            return no_store(json!({
                "providerId": provider_id,
                "status": "query-failed",
                "message": "The provider usage query failed.",
            }));
        }
    };
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() || text.len() > 65_536 {
        return no_store(json!({
            "providerId": provider_id,
            "status": "query-failed",
            "message": "The provider usage query failed.",
        }));
    }
    let payload: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let captured_at = chrono::Utc::now().timestamp_millis();
    let (buckets, metrics) = usage_report(&provider_id, &payload);
    no_store(json!({
        "providerId": provider_id,
        "status": "ready",
        "report": {
            "providerId": provider_id,
            "providerName": provider_display_name(&provider_id),
            "capturedAt": captured_at,
            "buckets": buckets,
            "metrics": metrics,
        }
    }))
}

fn provider_display_name(id: &str) -> &'static str {
    match id {
        "deepseek" => "DeepSeek",
        "openrouter" => "OpenRouter",
        "moonshotai" => "Moonshot AI",
        "moonshotai-cn" => "Moonshot AI CN",
        "vercel-ai-gateway" => "Vercel AI Gateway",
        "opencode-go" => "OpenCode Go",
        "kimi-coding" => "Kimi For Coding",
        "minimax" => "MiniMax",
        "minimax-cn" => "MiniMax CN",
        "openai-codex" => "OpenAI Codex",
        _ => "Provider",
    }
}

fn usage_report(provider_id: &str, payload: &Value) -> (Vec<Value>, Vec<Value>) {
    let mut buckets = Vec::new();
    let mut metrics = Vec::new();
    match provider_id {
        "deepseek" => {
            if let Some(infos) = payload.get("balance_infos").and_then(Value::as_array) {
                for info in infos {
                    let currency = info
                        .get("currency")
                        .and_then(Value::as_str)
                        .unwrap_or("CNY");
                    let remaining = info
                        .get("total_balance")
                        .and_then(Value::as_str)
                        .and_then(|value| value.parse::<f64>().ok());
                    buckets.push(json!({
                        "id": format!("balance-{currency}"),
                        "label": "Balance",
                        "remaining": remaining,
                        "unit": "currency",
                        "currency": currency,
                    }));
                }
            }
        }
        "openrouter" => {
            let data = payload.get("data").unwrap_or(payload);
            if let Some(usage) = data.get("usage").and_then(Value::as_f64) {
                metrics.push(json!({"id":"usage","label":"Usage","value":usage,"unit":"currency","currency":"USD"}));
            }
            if let Some(limit) = data.get("limit").and_then(Value::as_f64) {
                metrics.push(json!({"id":"limit","label":"Limit","value":limit,"unit":"currency","currency":"USD"}));
            }
        }
        "moonshotai" | "moonshotai-cn" => {
            let data = payload.get("data").unwrap_or(payload);
            let currency = if provider_id.ends_with("-cn") {
                "CNY"
            } else {
                "USD"
            };
            for (key, label) in [
                ("available_balance", "Available balance"),
                ("voucher_balance", "Voucher balance"),
                ("cash_balance", "Cash balance"),
            ] {
                if let Some(value) = data.get(key).and_then(Value::as_f64) {
                    metrics.push(json!({"id":key,"label":label,"value":value,"unit":"currency","currency":currency}));
                }
            }
        }
        _ => {
            collect_usage_numbers(payload, "", &mut metrics);
        }
    }
    (buckets, metrics)
}

fn collect_usage_numbers(value: &Value, prefix: &str, metrics: &mut Vec<Value>) {
    if metrics.len() >= 20 {
        return;
    }
    if let Some(object) = value.as_object() {
        for (key, value) in object {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            if let Some(number) = value.as_f64() {
                metrics.push(json!({"id": path, "label": key, "value": number}));
            } else if value.is_object() || value.is_array() {
                collect_usage_numbers(value, &path, metrics);
            }
        }
    } else if let Some(array) = value.as_array() {
        for (index, value) in array.iter().take(5).enumerate() {
            collect_usage_numbers(value, &format!("{prefix}.{index}"), metrics);
        }
    }
}
