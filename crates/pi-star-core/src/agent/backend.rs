//! Per-session agent backend selection.
//!
//! The choice is persisted as a `custom` entry so it travels with the pi v3
//! session and pi itself simply ignores it. Missing entries mean the standard
//! Node.js pi backend (`rpc`).

use std::path::Path;

use anyhow::Result;
use serde_json::{Value, json};

use crate::session::{append_lines, new_entry, read_session};

pub const BACKEND_CUSTOM_TYPE: &str = "pi-star:agent-backend";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackendKind {
    /// Standard pi, executed by the Node.js CLI over `--mode rpc`.
    #[default]
    Rpc,
    /// QuickPI, the in-process Rust harness.
    Quick,
}

impl BackendKind {
    pub fn id(self) -> &'static str {
        match self {
            BackendKind::Rpc => "rpc",
            BackendKind::Quick => "quick",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BackendKind::Rpc => "标准 PI (Node.js)",
            BackendKind::Quick => "极速 PI (QuickPI)",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "rpc" | "pi" | "standard" => Some(BackendKind::Rpc),
            "quick" | "quickpi" | "rust" => Some(BackendKind::Quick),
            _ => None,
        }
    }
}

/// Every selectable backend, in display order.
pub fn options() -> Vec<Value> {
    [BackendKind::Rpc, BackendKind::Quick]
        .into_iter()
        .map(|kind| json!({ "id": kind.id(), "name": kind.label() }))
        .collect()
}

/// The session's persisted choice, or the `rpc` default.
pub fn session_backend(path: &Path) -> BackendKind {
    let Ok(session) = read_session(path) else {
        return BackendKind::default();
    };
    session
        .entries
        .iter()
        .rev()
        .find_map(|entry| {
            (entry.get("type").and_then(Value::as_str) == Some("custom")
                && entry.get("customType").and_then(Value::as_str) == Some(BACKEND_CUSTOM_TYPE))
            .then(|| {
                entry
                    .pointer("/data/backend")
                    .and_then(Value::as_str)
                    .and_then(BackendKind::parse)
            })
            .flatten()
        })
        .unwrap_or_default()
}

/// Persist a new choice on the session's active leaf.
pub fn set_session_backend(path: &Path, kind: BackendKind) -> Result<()> {
    let session = read_session(path)?;
    let parent = session.leaf_id();
    let entry = new_entry(
        "custom",
        parent.as_deref(),
        [
            (
                "customType".to_string(),
                Value::String(BACKEND_CUSTOM_TYPE.to_string()),
            ),
            (
                "data".to_string(),
                json!({ "backend": kind.id(), "label": kind.label() }),
            ),
        ]
        .into_iter()
        .collect(),
    );
    append_lines(path, &[entry])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_the_standard_backend_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::Config::new(Some(dir.path().join("agent")), None).unwrap();
        let path = crate::session::create_session_file(&config, dir.path()).unwrap();
        assert_eq!(session_backend(&path), BackendKind::Rpc);

        set_session_backend(&path, BackendKind::Quick).unwrap();
        assert_eq!(session_backend(&path), BackendKind::Quick);
        set_session_backend(&path, BackendKind::Rpc).unwrap();
        assert_eq!(session_backend(&path), BackendKind::Rpc);

        let session = read_session(&path).unwrap();
        assert!(session.entries.iter().any(|entry| {
            entry.get("customType").and_then(Value::as_str) == Some(BACKEND_CUSTOM_TYPE)
        }));
    }

    #[test]
    fn options_use_the_product_labels() {
        let values = options();
        assert_eq!(values[0]["id"], "rpc");
        assert_eq!(values[0]["name"], "标准 PI (Node.js)");
        assert_eq!(values[1]["id"], "quick");
        assert_eq!(values[1]["name"], "极速 PI (QuickPI)");
    }
}
