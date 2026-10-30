use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::routes::{bad_request, internal, no_store, not_found};
use crate::state::{AppState, PendingOAuth};

const ANTHROPIC_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const ANTHROPIC_AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const ANTHROPIC_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const ANTHROPIC_REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";
const ANTHROPIC_SCOPES: &str = "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";

pub async fn login(State(state): State<Arc<AppState>>, Path(provider): Path<String>) -> Response {
    if provider == "github-copilot" {
        return copilot_login(state).await;
    }
    if provider != "anthropic" {
        return crate::routes::json_error(
            axum::http::StatusCode::NOT_IMPLEMENTED,
            format!("OAuth login for {provider} is not implemented yet"),
        );
    }
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let token = format!("anthropic-{}", uuid::Uuid::new_v4());
    let (sender, receiver) = tokio::sync::oneshot::channel();
    state.oauth_flows.lock().insert(
        token.clone(),
        PendingOAuth {
            sender: Some(sender),
        },
    );
    let mut url = match reqwest::Url::parse(ANTHROPIC_AUTHORIZE_URL) {
        Ok(url) => url,
        Err(error) => return internal(error),
    };
    url.query_pairs_mut()
        .append_pair("code", "true")
        .append_pair("client_id", ANTHROPIC_CLIENT_ID)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", ANTHROPIC_REDIRECT_URI)
        .append_pair("scope", ANTHROPIC_SCOPES)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &verifier);
    let stream = async_stream::stream! {
        yield Ok::<_, std::convert::Infallible>(
            axum::response::sse::Event::default().data(json!({
                "type": "auth",
                "url": url.to_string(),
                "instructions": "Complete login in your browser, then paste the authorization code or final redirect URL here.",
                "token": token,
            }).to_string())
        );
        let code = tokio::select! {
            code = receiver => code.ok(),
            _ = tokio::time::sleep(std::time::Duration::from_secs(600)) => None,
        };
        let Some(code) = code else {
            state.oauth_flows.lock().remove(&token);
            yield Ok(axum::response::sse::Event::default().data(json!({"type":"cancelled"}).to_string()));
            return;
        };
        match exchange_code(&state, &code, &verifier).await {
            Ok(()) => {
                state.oauth_flows.lock().remove(&token);
                state.invalidate_models();
                yield Ok(axum::response::sse::Event::default().data(json!({"type":"success"}).to_string()));
            }
            Err(error) => {
                state.oauth_flows.lock().remove(&token);
                yield Ok(axum::response::sse::Event::default().data(json!({"type":"error","message":error}).to_string()));
            }
        }
    };
    axum::response::sse::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

const GITHUB_COPILOT_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";

async fn copilot_login(state: Arc<AppState>) -> Response {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap_or_default();
    let start = client
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .header("User-Agent", "GitHubCopilotChat/0.35.0")
        .form(&[
            ("client_id", GITHUB_COPILOT_CLIENT_ID),
            ("scope", "read:user"),
        ])
        .send()
        .await;
    let start = match start {
        Ok(response) => response,
        Err(error) => return internal(error),
    };
    let start: Value = match start.json().await {
        Ok(value) => value,
        Err(error) => return internal(error),
    };
    let Some(device_code) = start
        .get("device_code")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return internal("GitHub did not return a device code");
    };
    let user_code = start
        .get("user_code")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let verification_uri = start
        .get("verification_uri")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let interval = start
        .get("interval")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .clamp(1, 30);
    let expires_in = start
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(900);
    let stream = async_stream::stream! {
        yield Ok::<_, std::convert::Infallible>(
            axum::response::sse::Event::default().data(json!({
                "type": "device_code",
                "userCode": user_code,
                "verificationUri": verification_uri,
                "intervalSeconds": interval,
                "expiresInSeconds": expires_in,
            }).to_string())
        );
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(expires_in + 30);
        loop {
            if tokio::time::Instant::now() >= deadline {
                yield Ok(axum::response::sse::Event::default().data(json!({"type":"error","message":"Device login expired"}).to_string()));
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            let response = client
                .post("https://github.com/login/oauth/access_token")
                .header("Accept", "application/json")
                .header("User-Agent", "GitHubCopilotChat/0.35.0")
                .form(&[
                    ("client_id", GITHUB_COPILOT_CLIENT_ID),
                    ("device_code", device_code.as_str()),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ])
                .send()
                .await;
            let Ok(response) = response else { continue };
            let Ok(value) = response.json::<Value>().await else { continue };
            if let Some(token) = value.get("access_token").and_then(Value::as_str) {
                match exchange_copilot_token(&client, token).await {
                    Ok((access, expires)) => {
                        let path = state.config.agent_dir.join("auth.json");
                        let mut auth = pi_star_core::models::read_json(&path).unwrap_or_else(|| json!({}));
                        if let Some(object) = auth.as_object_mut() {
                            object.insert("github-copilot".into(), json!({
                                "type": "oauth",
                                "refresh": token,
                                "access": access,
                                "expires": expires,
                            }));
                        }
                        if let Err(error) = pi_star_core::settings::write_json_atomic(&path, &auth, 0o600) {
                            yield Ok(axum::response::sse::Event::default().data(json!({"type":"error","message":error.to_string()}).to_string()));
                            break;
                        }
                        state.invalidate_models();
                        yield Ok(axum::response::sse::Event::default().data(json!({"type":"success"}).to_string()));
                        break;
                    }
                    Err(error) => {
                        yield Ok(axum::response::sse::Event::default().data(json!({"type":"error","message":error}).to_string()));
                        break;
                    }
                }
            }
            let error = value.get("error").and_then(Value::as_str).unwrap_or("");
            if error == "authorization_pending" {
                continue;
            }
            if error == "slow_down" {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
            if !error.is_empty() {
                yield Ok(axum::response::sse::Event::default().data(json!({
                    "type": "error",
                    "message": value.get("error_description").and_then(Value::as_str).unwrap_or(error),
                }).to_string()));
                break;
            }
        }
    };
    axum::response::sse::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

async fn exchange_copilot_token(
    client: &reqwest::Client,
    github_token: &str,
) -> Result<(String, i64), String> {
    let response = client
        .get("https://api.github.com/copilot_internal/v2/token")
        .header("Accept", "application/json")
        .header("User-Agent", "GitHubCopilotChat/0.35.0")
        .header("Editor-Version", "vscode/1.107.0")
        .header("Editor-Plugin-Version", "copilot-chat/0.35.0")
        .header("Copilot-Integration-Id", "vscode-chat")
        .bearer_auth(github_token)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(format!("Copilot token exchange HTTP {status}: {value}"));
    }
    let access = value
        .get("token")
        .and_then(Value::as_str)
        .ok_or("Copilot token response has no token")?;
    let expires = value
        .get("expires_at")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| chrono::Utc::now().timestamp() + 3600)
        * 1000;
    Ok((access.to_string(), expires))
}

#[derive(Debug, Deserialize)]
pub struct LoginPostBody {
    token: Option<String>,
    code: Option<String>,
}

pub async fn login_post(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<String>,
    Json(body): Json<LoginPostBody>,
) -> Response {
    if provider != "anthropic" {
        return crate::routes::json_error(
            axum::http::StatusCode::NOT_IMPLEMENTED,
            format!("OAuth login for {provider} is not implemented yet"),
        );
    }
    let (Some(token), Some(code)) = (body.token, body.code) else {
        return bad_request("token and code required");
    };
    let sender = {
        let mut flows = state.oauth_flows.lock();
        let Some(flow) = flows.get_mut(&token) else {
            return not_found("No pending login for token");
        };
        flow.sender.take()
    };
    let Some(sender) = sender else {
        return bad_request("Login token was already used");
    };
    if sender.send(code).is_err() {
        return bad_request("Login flow has ended");
    }
    no_store(json!({"ok": true, "provider": provider}))
}

async fn exchange_code(state: &AppState, input: &str, verifier: &str) -> Result<(), String> {
    let (code, returned_state) =
        parse_code(input).ok_or_else(|| "No authorization code in the pasted value".to_string())?;
    if let Some(returned_state) = returned_state {
        if returned_state != verifier {
            return Err("OAuth state mismatch".into());
        }
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .post(ANTHROPIC_TOKEN_URL)
        .json(&json!({
            "grant_type": "authorization_code",
            "client_id": ANTHROPIC_CLIENT_ID,
            "code": code,
            "state": verifier,
            "redirect_uri": ANTHROPIC_REDIRECT_URI,
            "code_verifier": verifier,
        }))
        .send()
        .await
        .map_err(|error| format!("token exchange failed: {error}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "token exchange HTTP {status}: {}",
            &text[..text.len().min(500)]
        ));
    }
    let payload: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let access = payload
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or("token response has no access_token")?;
    let refresh = payload
        .get("refresh_token")
        .and_then(Value::as_str)
        .unwrap_or("");
    let expires_in = payload
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or(3600);
    let path = state.config.agent_dir.join("auth.json");
    let mut auth = pi_star_core::models::read_json(&path).unwrap_or_else(|| json!({}));
    let Some(object) = auth.as_object_mut() else {
        return Err("auth.json is not an object".into());
    };
    object.insert(
        "anthropic".into(),
        json!({
            "type": "oauth",
            "refresh": refresh,
            "access": access,
            "expires": chrono::Utc::now().timestamp_millis() + expires_in * 1000 - 5 * 60 * 1000,
        }),
    );
    pi_star_core::settings::write_json_atomic(&path, &auth, 0o600)
        .map_err(|error| error.to_string())
}

fn parse_code(input: &str) -> Option<(String, Option<String>)> {
    let value = input.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(url) = reqwest::Url::parse(value) {
        let code = url
            .query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.into_owned())?;
        let state = url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned());
        return Some((code, state));
    }
    if let Some((code, state)) = value.split_once('#') {
        return Some((code.to_string(), Some(state.to_string())));
    }
    if value.contains("code=") {
        let url = reqwest::Url::parse(&format!("http://localhost/?{value}")).ok()?;
        let code = url
            .query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.into_owned())?;
        let state = url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned());
        return Some((code, state));
    }
    Some((value.to_string(), None))
}
