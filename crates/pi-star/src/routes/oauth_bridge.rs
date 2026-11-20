//! Generic OAuth login for pi's built-in providers.
//!
//! pi-star implements Anthropic and GitHub Copilot natively. Every other
//! OAuth-capable provider is driven through pi's own `@earendil-works/pi-ai`
//! flows in a short-lived Node process, so the subscription list in the Models
//! panel matches pi-web instead of hiding the providers pi-star cannot
//! hand-code yet.
//!
//! The same flow backs the deprecated SSE route and the `auth:<provider>`
//! stream-bus channel.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, oneshot};

use crate::pi_runtime;
use crate::routes::auth::{Flow, spawn_flow};
use crate::state::{AppState, PendingOAuth};

const BRIDGE_SCRIPT: &str = include_str!("oauth_bridge.mjs");

/// The Node command and pi package directory the bridge imports from.
fn bridge_runtime() -> Option<(PathBuf, PathBuf)> {
    Some((pi_runtime::node_command()?, pi_runtime::package_dir()?))
}

/// Remove prompt tokens this flow registered, including when the consumer goes
/// away before the login finishes.
struct PromptCleanup {
    state: Arc<AppState>,
    tokens: Arc<parking_lot::Mutex<Vec<String>>>,
}

impl Drop for PromptCleanup {
    fn drop(&mut self) {
        let tokens = {
            let mut tokens = self.tokens.lock();
            std::mem::take(&mut *tokens)
        };
        let mut flows = self.state.oauth_flows.lock();
        for token in tokens {
            flows.remove(&token);
        }
    }
}

/// Starts the Node bridge and returns the flow the bus and the SSE route read.
pub async fn start_flow(
    state: Arc<AppState>,
    provider: String,
) -> Result<Flow, (StatusCode, String)> {
    let Some(definition) = pi_star_core::models::builtin_provider(&provider) else {
        return Err((
            StatusCode::NOT_IMPLEMENTED,
            format!("OAuth login for {provider} is not implemented yet"),
        ));
    };
    if definition.oauth_name.is_none() {
        return Err((
            StatusCode::NOT_IMPLEMENTED,
            format!("OAuth login for {provider} is not implemented yet"),
        ));
    }
    let Some((node, package_dir)) = bridge_runtime() else {
        return Err((
            StatusCode::NOT_IMPLEMENTED,
            format!("OAuth login for {provider} requires the bundled pi runtime"),
        ));
    };

    let mut command = tokio::process::Command::new(node);
    command
        .arg("--input-type=module")
        .arg("-e")
        .arg(BRIDGE_SCRIPT)
        .arg(&provider)
        .current_dir(package_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(device_id) = ensure_device_id(&state.config) {
        command.env("PI_STAR_DEVICE_ID", device_id);
    }

    let mut child = command.spawn().map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Could not start OAuth login for {provider}: {error}"),
        )
    })?;
    let Some(stdout) = child.stdout.take() else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "OAuth login process has no stdout".into(),
        ));
    };
    let Some(stdin) = child.stdin.take() else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "OAuth login process has no stdin".into(),
        ));
    };
    let stdin = Arc::new(Mutex::new(stdin));
    let mut lines = BufReader::new(stdout).lines();

    let stream_state = state.clone();
    let config = state.config.clone();
    let prompt_tokens = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
    let cleanup = PromptCleanup {
        state: state.clone(),
        tokens: prompt_tokens.clone(),
    };

    Ok(spawn_flow(move |tx| async move {
        let mut child = child;
        let _cleanup = cleanup;
        loop {
            let event = match lines.next_line().await {
                Ok(Some(line)) => match serde_json::from_str::<Value>(&line) {
                    Ok(event) => event,
                    Err(_) => continue,
                },
                Ok(None) => {
                    let _ = tx
                        .send(json!({"type": "error", "message": "OAuth login ended unexpectedly"}))
                        .await;
                    break;
                }
                Err(error) => {
                    let _ = tx
                        .send(json!({"type": "error", "message": format!("OAuth login failed: {error}")}))
                        .await;
                    break;
                }
            };

            match event.get("type").and_then(Value::as_str) {
                Some("prompt_request") | Some("select_request") => {
                    if let Some(token) = event
                        .get("token")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                    {
                        let (sender, receiver) = oneshot::channel::<String>();
                        stream_state.oauth_flows.lock().insert(
                            token.clone(),
                            PendingOAuth {
                                sender: Some(sender),
                            },
                        );
                        prompt_tokens.lock().push(token.clone());
                        let stdin = stdin.clone();
                        tokio::spawn(async move {
                            if let Ok(code) = receiver.await {
                                let payload = json!({"token": token, "code": code});
                                let mut stdin = stdin.lock().await;
                                let _ = stdin.write_all(format!("{payload}\n").as_bytes()).await;
                                let _ = stdin.flush().await;
                            }
                        });
                    }
                    if tx.send(event).await.is_err() {
                        break;
                    }
                }
                Some("success") => {
                    let credential = event.get("credential").cloned().unwrap_or(Value::Null);
                    let reply = match write_credential(&config, &provider, credential) {
                        Ok(()) => {
                            stream_state.invalidate_models();
                            json!({"type": "success"})
                        }
                        Err(error) => json!({"type": "error", "message": error}),
                    };
                    let _ = tx.send(reply).await;
                    break;
                }
                Some("error") => {
                    let _ = tx.send(event).await;
                    break;
                }
                _ => {
                    if tx.send(event).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = child.start_kill();
    }))
}

fn write_credential(
    config: &pi_star_core::Config,
    provider: &str,
    credential: Value,
) -> Result<(), String> {
    let Some(mut credential) = credential.as_object().cloned() else {
        return Err("The OAuth provider returned no credential".into());
    };
    credential.insert("type".into(), json!("oauth"));
    let path = config.agent_dir.join("auth.json");
    let mut auth = pi_star_core::models::read_json(&path).unwrap_or_else(|| json!({}));
    let Some(object) = auth.as_object_mut() else {
        return Err("auth.json is not an object".into());
    };
    object.insert(provider.to_string(), Value::Object(credential));
    pi_star_core::settings::write_json_atomic(&path, &auth, 0o600)
        .map_err(|error| error.to_string())
}

/// pi's OAuth flows use the installation device ID for OpenAI; keep it in the
/// same settings file pi uses so both apps authenticate as one host.
fn ensure_device_id(config: &pi_star_core::Config) -> Option<String> {
    let path = config.settings_path();
    let mut settings = pi_star_core::models::read_json(&path).unwrap_or_else(|| json!({}));
    if let Some(device_id) = settings
        .get("deviceId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|device_id| !device_id.is_empty())
    {
        return Some(device_id.to_string());
    }
    if !settings.is_object() {
        settings = json!({});
    }
    let device_id = uuid::Uuid::new_v4().to_string();
    settings["deviceId"] = Value::String(device_id.clone());
    let _ = pi_star_core::settings::write_json_atomic(&path, &settings, 0o600);
    Some(device_id)
}
