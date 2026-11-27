//! The one WebSocket endpoint. It owns the connection task, the wire protocol
//! and the terminal input path; channels themselves live in [`super::Hub`].

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::state::AppState;

use super::{
    Attach, ConnHandle, MAX_CHANNELS_PER_CONN, MAX_FRAME_BYTES, MAX_TERMINAL_INPUT_BYTES, Outbound,
};

#[derive(Debug, Deserialize, Default)]
pub struct StreamQuery {
    /// The page id, for the Debug panel and the per-page connection limit.
    holder: Option<String>,
}

pub async fn handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Response {
    let holder = query.holder.filter(|value| valid_holder(value));
    let (address, client) = crate::debug_streams::describe_stream_client(&headers);
    let (tx, rx) = tokio::sync::mpsc::channel::<Outbound>(1024);
    let conn = match state.hub.register_conn(holder, tx, address, client) {
        Ok(conn) => conn,
        Err(()) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                axum::Json(json!({
                    "error": "Too many stream connections for this page",
                    "code": "too_many_streams",
                })),
            )
                .into_response();
        }
    };
    ws.max_message_size(MAX_FRAME_BYTES)
        .on_upgrade(move |socket| run(state, socket, conn, rx))
}

fn valid_holder(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

async fn run(
    state: Arc<AppState>,
    socket: WebSocket,
    conn: ConnHandle,
    mut rx: tokio::sync::mpsc::Receiver<Outbound>,
) {
    let (mut sink, mut stream) = socket.split();
    let mut subs: HashMap<u64, String> = HashMap::new();

    let ready = json!({
        "op": "ready",
        "protocol": 1,
        "limits": {
            "channels": MAX_CHANNELS_PER_CONN,
            "frameBytes": MAX_FRAME_BYTES,
            "terminalInputBytes": MAX_TERMINAL_INPUT_BYTES,
        },
    });
    let sent = sink.send(Message::Text(ready.to_string().into())).await;
    if sent.is_err() {
        cleanup(&state, &conn, &mut subs);
        return;
    }

    let mut heartbeat = tokio::time::interval(Duration::from_secs(25));
    heartbeat.tick().await;
    let mut awaiting_pong = false;
    let mut missed = 0u32;

    loop {
        tokio::select! {
            out = rx.recv() => match out {
                Some(Outbound::Text(text)) => {
                    if sink.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Some(Outbound::Close(code, reason)) => {
                    let _ = sink
                        .send(Message::Close(Some(CloseFrame { code, reason: reason.into() })))
                        .await;
                    break;
                }
                None => break,
            },
            message = stream.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    if text.len() > MAX_FRAME_BYTES {
                        break;
                    }
                    handle_text(&state, &conn, &mut subs, &text).await;
                }
                Some(Ok(Message::Ping(payload))) => {
                    if sink.send(Message::Pong(payload)).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Pong(_))) => {
                    awaiting_pong = false;
                    missed = 0;
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Err(_)) => break,
                _ => {}
            },
            _ = conn.shutdown.notify.notified() => break,
            _ = heartbeat.tick() => {
                if awaiting_pong {
                    missed += 1;
                    if missed >= 2 {
                        break;
                    }
                }
                awaiting_pong = true;
                if sink.send(Message::Ping(Default::default())).await.is_err() {
                    break;
                }
            }
        }
    }
    cleanup(&state, &conn, &mut subs);
}

fn cleanup(state: &Arc<AppState>, conn: &ConnHandle, subs: &mut HashMap<u64, String>) {
    for key in subs.values() {
        state.hub.unsubscribe(conn, key);
    }
    state.hub.drop_conn(conn);
}

async fn handle_text(
    state: &Arc<AppState>,
    conn: &ConnHandle,
    subs: &mut HashMap<u64, String>,
    text: &str,
) {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return;
    };
    let Some(op) = value.get("op").and_then(Value::as_str) else {
        return;
    };
    match op {
        "ping" => {
            conn.try_send(json!({"op": "pong"}).to_string());
        }
        "hello" => {
            // Protocol negotiation is intentionally trivial: the ready frame
            // carries the version the server speaks.
        }
        "sub" => {
            let Some(id) = value.get("id").and_then(Value::as_u64) else {
                return;
            };
            let Some(key) = value.get("channel").and_then(Value::as_str) else {
                return;
            };
            if subs.len() >= MAX_CHANNELS_PER_CONN {
                conn.try_send(sub_err(
                    id,
                    key,
                    "too_many_channels",
                    "Too many channels on this connection",
                ));
                return;
            }
            let cursor = value
                .get("cursor")
                .cloned()
                .filter(|cursor| !cursor.is_null());
            match state.hub.subscribe(state, conn, key, cursor).await {
                Ok(Attach::Subscribed) => {
                    subs.insert(id, key.to_string());
                    state.hub.note_channel(conn.id, key);
                    conn.try_send(json!({"op": "sub_ok", "id": id, "channel": key}).to_string());
                }
                Ok(Attach::Gap) => {
                    // The hub already sent a `gap` frame; the client
                    // resubscribes without a cursor.
                }
                Err(error) => {
                    conn.try_send(sub_err(id, key, error.code(), &error.message()));
                }
            }
        }
        "unsub" => {
            let Some(id) = value.get("id").and_then(Value::as_u64) else {
                return;
            };
            if let Some(key) = subs.remove(&id) {
                state.hub.unsubscribe(conn, &key);
                state.hub.forget_channel(conn.id, &key);
            }
        }
        "cmd" => {
            handle_cmd(state, conn, &value).await;
        }
        "ack" => {
            // Reserved for per-channel credit. Frames are bounded server-side
            // already, so acknowledgements only feed diagnostics today.
        }
        _ => {}
    }
}

fn sub_err(id: u64, channel: &str, code: &str, message: &str) -> String {
    json!({
        "op": "sub_err",
        "id": id,
        "channel": channel,
        "code": code,
        "message": message,
    })
    .to_string()
}

async fn handle_cmd(state: &Arc<AppState>, conn: &ConnHandle, value: &Value) {
    let Some(channel) = value.get("channel").and_then(Value::as_str) else {
        return;
    };
    let Some(payload) = value.get("payload") else {
        return;
    };
    if let Some(id) = channel.strip_prefix("terminal:") {
        match payload.get("type").and_then(Value::as_str) {
            Some("input") => {
                if let Some(data) = payload.get("data").and_then(Value::as_str) {
                    if data.len() <= MAX_TERMINAL_INPUT_BYTES {
                        state.terminals.write(id, data);
                    }
                }
            }
            Some("resize") => {
                let cols = payload
                    .get("cols")
                    .and_then(Value::as_u64)
                    .unwrap_or(80)
                    .clamp(2, 1000) as u16;
                let rows = payload
                    .get("rows")
                    .and_then(Value::as_u64)
                    .unwrap_or(24)
                    .clamp(2, 1000) as u16;
                state.terminals.resize(id, cols, rows);
            }
            _ => {}
        }
        return;
    }
    if let Some(id) = channel.strip_prefix("agent:") {
        let request_id = payload.get("requestId").cloned().unwrap_or(Value::Null);
        let reply =
            crate::routes::agent::run_command(state.clone(), id.to_string(), payload.clone()).await;
        let status = reply.status().as_u16();
        let body = axum::body::to_bytes(reply.into_body(), 8 * 1024 * 1024)
            .await
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .unwrap_or(Value::Null);
        conn.try_send(
            json!({
                "op": "cmd_result",
                "channel": channel,
                "id": request_id,
                "status": status,
                "body": body,
            })
            .to_string(),
        );
    }
}
