//! Renderer liveness for the desktop shell.
//!
//! The shell injects a small script into every webview that POSTs a beat once a
//! second. A beat runs on the page's main thread, so if the renderer wedges
//! (an infinite loop, a stuck effect chain) the beats stop even while the HTTP
//! server keeps running. `app/src/watchdog.rs` polls the GET form and rebuilds
//! the main window when it goes quiet.
//!
//! The endpoints are deliberately public (loopback-only by default): the login
//! page has to beat before a session cookie exists, and a beat carries no data.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Query, State};
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;

use crate::routes::{bad_request, no_store};
use crate::state::AppState;

const DEFAULT_CLIENT: &str = "main";
const MAX_CLIENT_LEN: usize = 64;
/// Only the desktop pages beat; keep a small map so a stray caller cannot grow it.
const MAX_CLIENTS: usize = 16;

#[derive(Debug, Deserialize, Default)]
pub struct AliveQuery {
    client: Option<String>,
}

fn client_name(query: &AliveQuery) -> Result<String, Response> {
    let client = query.client.as_deref().unwrap_or(DEFAULT_CLIENT).trim();
    if client.is_empty() || client.len() > MAX_CLIENT_LEN {
        return Err(bad_request("client must be 1-64 characters"));
    }
    Ok(client.to_string())
}

/// `POST /api/desktop/alive?client=main`: record a renderer heartbeat.
pub async fn beat(State(state): State<Arc<AppState>>, Query(query): Query<AliveQuery>) -> Response {
    let client = match client_name(&query) {
        Ok(client) => client,
        Err(response) => return response,
    };
    {
        let mut beats = state.desktop_beats.lock();
        if beats.len() >= MAX_CLIENTS && !beats.contains_key(&client) {
            beats.clear();
        }
        beats.insert(client, Instant::now());
    }
    no_store(json!({ "ok": true }))
}

/// `GET /api/desktop/alive?client=main`: how long ago the renderer last beat.
pub async fn status(
    State(state): State<Arc<AppState>>,
    Query(query): Query<AliveQuery>,
) -> Response {
    let client = match client_name(&query) {
        Ok(client) => client,
        Err(response) => return response,
    };
    let age_ms = state
        .desktop_beats
        .lock()
        .get(&client)
        .map(|beat| beat.elapsed().as_millis() as u64);
    no_store(json!({
        "client": client,
        "alive": age_ms.is_some(),
        "ageMs": age_ms,
    }))
}
