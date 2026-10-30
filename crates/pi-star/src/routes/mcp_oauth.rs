use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::routes::{bad_request, internal, no_store, not_found};
use crate::state::AppState;

const FLOW_TTL: Duration = Duration::from_secs(600);

#[derive(Debug, Clone)]
pub struct McpSignInFlow {
    pub id: String,
    pub name: String,
    pub scope: String,
    pub config_key: String,
    pub server_url: String,
    pub state: String,
    pub verifier: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
    pub scope_value: String,
    pub phase: String,
    pub error: Option<String>,
    pub created: Instant,
}

impl McpSignInFlow {
    fn json(&self) -> Value {
        let mut value = json!({
            "flowId": self.id,
            "name": self.name,
            "scope": self.scope,
            "configKey": self.config_key,
            "phase": self.phase,
            "expiresInMs": FLOW_TTL.saturating_sub(self.created.elapsed()).as_millis() as u64,
        });
        if self.phase == "authorize" {
            let mut url = reqwest::Url::parse(&self.authorization_endpoint).ok();
            if let Some(url) = url.as_mut() {
                url.query_pairs_mut()
                    .append_pair("response_type", "code")
                    .append_pair("client_id", &self.client_id)
                    .append_pair("redirect_uri", &self.redirect_uri)
                    .append_pair("state", &self.state)
                    .append_pair("code_challenge", &pkce_challenge(&self.verifier))
                    .append_pair("code_challenge_method", "S256");
                if !self.scope_value.is_empty() {
                    url.query_pairs_mut()
                        .append_pair("scope", &self.scope_value);
                }
                value["authorizationUrl"] = json!(url.to_string());
            }
            value["redirectUrl"] = json!(self.redirect_uri);
        }
        if let Some(error) = &self.error {
            value["error"] = json!(error);
        }
        if self.phase == "done" {
            value["result"] = json!({
                "state": "connected",
                "tools": [],
                "toolCount": 0,
                "durationMs": 0,
                "testedAt": chrono::Utc::now().timestamp_millis(),
            });
        }
        value
    }
}

#[derive(Debug, Deserialize)]
pub struct StartBody {
    scope: Option<String>,
    name: Option<String>,
    cwd: Option<String>,
}

pub async fn start(State(state): State<Arc<AppState>>, Json(body): Json<StartBody>) -> Response {
    let name = body.name.unwrap_or_default();
    let scope = body.scope.unwrap_or_else(|| "global".into());
    if name.is_empty() {
        return bad_request("name is required");
    }
    let path = if scope == "project" {
        let Some(cwd) = body.cwd else {
            return bad_request("cwd is required for a project server");
        };
        std::path::PathBuf::from(cwd).join(".pi").join("mcp.json")
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
    let Some(server_url) = entry.get("url").and_then(Value::as_str) else {
        return crate::routes::json_error(
            axum::http::StatusCode::CONFLICT,
            "Only an HTTP MCP server can sign in with OAuth",
        );
    };
    if entry
        .get("headers")
        .and_then(Value::as_object)
        .is_some_and(|headers| headers.contains_key("Authorization"))
    {
        return crate::routes::json_error(
            axum::http::StatusCode::CONFLICT,
            "This server already has an Authorization header",
        );
    }
    let metadata = match discover(server_url).await {
        Ok(metadata) => metadata,
        Err(error) => return crate::routes::json_error(axum::http::StatusCode::BAD_GATEWAY, error),
    };
    let redirect_uri = format!(
        "http://127.0.0.1:{}/api/mcp/oauth/callback",
        state.server_port()
    );
    let (client_id, client_secret) = match register_client(&metadata, &redirect_uri).await {
        Ok(pair) => pair,
        Err(error) => return crate::routes::json_error(axum::http::StatusCode::BAD_GATEWAY, error),
    };
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let state_value = uuid::Uuid::new_v4().to_string();
    let flow = McpSignInFlow {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.clone(),
        scope,
        config_key: hex::encode(Sha256::digest(
            serde_json::to_vec(&entry).unwrap_or_default(),
        )),
        server_url: server_url.to_string(),
        state: state_value,
        verifier,
        authorization_endpoint: metadata.authorization_endpoint,
        token_endpoint: metadata.token_endpoint,
        client_id,
        client_secret,
        redirect_uri,
        scope_value: metadata.scope,
        phase: "authorize".into(),
        error: None,
        created: Instant::now(),
    };
    state
        .mcp_signins
        .lock()
        .insert(flow.id.clone(), flow.clone());
    no_store(flow.json())
}

pub async fn status(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let flows = state.mcp_signins.lock();
    let Some(flow) = flows.get(&id) else {
        return not_found("No sign-in has that id");
    };
    no_store(flow.json())
}

#[derive(Debug, Deserialize)]
pub struct PasteBody {
    #[serde(rename = "redirectUrl")]
    redirect_url: Option<String>,
}

pub async fn paste(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<PasteBody>,
) -> Response {
    let Some(redirect_url) = body.redirect_url else {
        return bad_request("redirectUrl is required");
    };
    let flow = {
        let flows = state.mcp_signins.lock();
        flows.get(&id).cloned()
    };
    let Some(flow) = flow else {
        return not_found("No sign-in has that id");
    };
    match complete_flow(&state, flow, &redirect_url).await {
        Ok(flow) => no_store(flow.json()),
        Err(error) => internal(error),
    }
}

pub async fn cancel(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let flow = state.mcp_signins.lock().remove(&id);
    match flow {
        Some(mut flow) => {
            flow.phase = "cancelled".into();
            no_store(flow.json())
        }
        None => not_found("No sign-in has that id"),
    }
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub async fn callback(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    if let Some(error) = query.error {
        return Html(format!(
            "<h1>Sign-in failed</h1><p>{}</p>",
            html_escape(&error)
        ))
        .into_response();
    }
    let (Some(code), Some(state_value)) = (query.code, query.state) else {
        return Html("<h1>Sign-in failed</h1><p>Missing code or state.</p>".to_string())
            .into_response();
    };
    let flow = {
        let flows = state.mcp_signins.lock();
        flows
            .values()
            .find(|flow| flow.state == state_value)
            .cloned()
    };
    let Some(flow) = flow else {
        return Html("<h1>Sign-in failed</h1><p>This sign-in expired.</p>".to_string())
            .into_response();
    };
    let redirect = format!(
        "{}?code={}&state={}",
        flow.redirect_uri,
        urlencoding(&code),
        urlencoding(&state_value)
    );
    match complete_flow(&state, flow, &redirect).await {
        Ok(_) => Html(
            "<h1>Signed in</h1><p>You can close this window and return to pi-star.</p>".to_string(),
        )
        .into_response(),
        Err(error) => Html(format!(
            "<h1>Sign-in failed</h1><p>{}</p>",
            html_escape(&error)
        ))
        .into_response(),
    }
}

struct Metadata {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: Option<String>,
    scope: String,
}

async fn discover(server_url: &str) -> Result<Metadata, String> {
    let url = reqwest::Url::parse(server_url).map_err(|error| error.to_string())?;
    let mut origin = format!(
        "{}://{}",
        url.scheme(),
        url.host_str().ok_or("invalid URL")?
    );
    if let Some(port) = url.port() {
        origin.push_str(&format!(":{port}"));
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())?;
    let mut authorization_endpoint = None;
    let mut token_endpoint = None;
    let mut registration_endpoint = None;
    let mut scope = String::new();
    for candidate in [
        format!("{origin}/.well-known/oauth-authorization-server"),
        format!("{origin}/.well-known/openid-configuration"),
        format!("{origin}/.well-known/oauth-protected-resource"),
    ] {
        let Ok(response) = client.get(&candidate).send().await else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let Ok(value) = response.json::<Value>().await else {
            continue;
        };
        if let Some(endpoint) = value.get("authorization_endpoint").and_then(Value::as_str) {
            authorization_endpoint = Some(endpoint.to_string());
        }
        if let Some(endpoint) = value.get("token_endpoint").and_then(Value::as_str) {
            token_endpoint = Some(endpoint.to_string());
        }
        if let Some(endpoint) = value.get("registration_endpoint").and_then(Value::as_str) {
            registration_endpoint = Some(endpoint.to_string());
        }
        if let Some(value) = value.get("scopes_supported").and_then(Value::as_array) {
            scope = value
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ");
        }
        if authorization_endpoint.is_some() && token_endpoint.is_some() {
            break;
        }
    }
    Ok(Metadata {
        authorization_endpoint: authorization_endpoint
            .ok_or("OAuth authorization endpoint not found")?,
        token_endpoint: token_endpoint.ok_or("OAuth token endpoint not found")?,
        registration_endpoint,
        scope,
    })
}

async fn register_client(
    metadata: &Metadata,
    redirect_uri: &str,
) -> Result<(String, Option<String>), String> {
    let Some(endpoint) = &metadata.registration_endpoint else {
        return Err("The OAuth server does not support dynamic client registration; configure auth.clientId".into());
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .post(endpoint)
        .json(&json!({
            "client_name": "pi-star",
            "redirect_uris": [redirect_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        }))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(format!("client registration HTTP {status}"));
    }
    let client_id = value
        .get("client_id")
        .and_then(Value::as_str)
        .ok_or("client registration returned no client_id")?
        .to_string();
    Ok((
        client_id,
        value
            .get("client_secret")
            .and_then(Value::as_str)
            .map(str::to_owned),
    ))
}

async fn complete_flow(
    state: &AppState,
    mut flow: McpSignInFlow,
    redirect_url: &str,
) -> Result<McpSignInFlow, String> {
    let parsed = reqwest::Url::parse(redirect_url).ok();
    let code = parsed
        .as_ref()
        .and_then(|url| url.query_pairs().find(|(key, _)| key == "code"))
        .map(|(_, value)| value.into_owned())
        .or_else(|| redirect_url.split('#').next().map(str::to_owned))
        .ok_or("redirect URL has no code")?;
    if let Some(state_value) = parsed
        .as_ref()
        .and_then(|url| url.query_pairs().find(|(key, _)| key == "state"))
        .map(|(_, value)| value.into_owned())
    {
        if state_value != flow.state {
            return Err("OAuth state mismatch".into());
        }
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?;
    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code),
        ("redirect_uri", flow.redirect_uri.clone()),
        ("client_id", flow.client_id.clone()),
        ("code_verifier", flow.verifier.clone()),
    ];
    if let Some(secret) = &flow.client_secret {
        form.push(("client_secret", secret.clone()));
    }
    let response = client
        .post(&flow.token_endpoint)
        .form(&form)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(format!("token exchange HTTP {status}: {value}"));
    }
    let Some(access_token) = value.get("access_token").and_then(Value::as_str) else {
        return Err("token response has no access_token".into());
    };
    let key = format!("mcp__{}|{}", flow.name.replace('-', "_"), flow.server_url);
    let path = state.config.agent_dir.join("mcp-auth.json");
    let mut auth = pi_star_core::models::read_json(&path).unwrap_or_else(|| json!({}));
    let Some(object) = auth.as_object_mut() else {
        return Err("mcp-auth.json is not an object".into());
    };
    object.insert(
        key,
        json!({
            "serverUrl": flow.server_url,
            "tokens": value,
            "tokensExpireAt": value
                .get("expires_in")
                .and_then(Value::as_i64)
                .map(|seconds| chrono::Utc::now().timestamp_millis() + seconds * 1000),
            "clientInformation": {
                "client_id": flow.client_id,
                "client_secret": flow.client_secret,
            },
        }),
    );
    pi_star_core::settings::write_json_atomic(&path, &auth, 0o600)
        .map_err(|error| error.to_string())?;
    let _ = access_token;
    flow.phase = "done".into();
    state
        .mcp_signins
        .lock()
        .insert(flow.id.clone(), flow.clone());
    Ok(flow)
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
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

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
