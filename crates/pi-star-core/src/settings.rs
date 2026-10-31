use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::config::Config;
use crate::models::{read_json, strip_json_comments};

static SETTINGS_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodemodeModeSetting {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodemodeInlineBudgetSetting {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSettings {
    pub is_windows: bool,
    pub power_shell_enabled: bool,
    pub codemode: String,
    pub codemode_mode: CodemodeModeSetting,
    pub codemode_inline_budget: CodemodeInlineBudgetSetting,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSettings {
    pub built_in_enabled: bool,
    pub disabled_built_ins: Vec<String>,
    pub max_concurrent: u32,
}

impl Default for SubagentSettings {
    fn default() -> Self {
        Self {
            // Matches pi-web: file profiles are always available; enabling the
            // built-in general-purpose/explore/plan profiles is opt-in.
            built_in_enabled: false,
            disabled_built_ins: Vec::new(),
            max_concurrent: 10,
        }
    }
}

pub fn read_settings(config: &Config) -> Value {
    read_json(&config.settings_path()).unwrap_or_else(|| json!({}))
}

pub fn settings_lock() -> parking_lot::MutexGuard<'static, ()> {
    SETTINGS_LOCK.lock()
}

pub fn write_settings(config: &Config, value: &Value) -> Result<()> {
    write_json_atomic(&config.settings_path(), value, 0o600)
}

pub fn update_settings<F>(config: &Config, edit: F) -> Result<Value>
where
    F: FnOnce(&mut Map<String, Value>) -> Result<()>,
{
    let _guard = settings_lock();
    let mut value = read_settings(config);
    let object = value
        .as_object_mut()
        .ok_or_else(|| anyhow!("settings.json is not an object"))?;
    edit(object)?;
    write_settings(config, &value)?;
    Ok(value)
}

pub fn tools_settings(config: &Config) -> ToolSettings {
    let settings = read_settings(config);
    let default_tools = settings
        .get("defaultTools")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let codemode = if default_tools
        .iter()
        .any(|tool| *tool == "+codemode" || *tool == "codemode")
    {
        "always".to_string()
    } else {
        "automatic".to_string()
    };
    let codemode_mode = settings
        .get("codemode")
        .and_then(|value| value.get("mode"))
        .and_then(Value::as_str)
        .unwrap_or("on");
    let codemode_inline_budget = settings
        .get("codemode")
        .and_then(|value| value.get("inlineBudget"))
        .and_then(Value::as_u64);
    ToolSettings {
        is_windows: cfg!(windows),
        power_shell_enabled: settings
            .get("powershell")
            .and_then(Value::as_bool)
            .unwrap_or(cfg!(windows)),
        codemode,
        codemode_mode: CodemodeModeSetting {
            value: if codemode_mode == "only" {
                "only"
            } else {
                "on"
            }
            .to_string(),
            invalid: None,
        },
        codemode_inline_budget: CodemodeInlineBudgetSetting {
            value: codemode_inline_budget,
            invalid: None,
        },
    }
}

pub fn update_tools_settings(config: &Config, body: &Value) -> Result<ToolSettings> {
    update_settings(config, |settings| {
        if let Some(enabled) = body.get("powerShellEnabled").and_then(Value::as_bool) {
            settings.insert("powershell".into(), Value::Bool(enabled));
        }
        if let Some(codemode) = body.get("codemode").and_then(Value::as_str) {
            let mut tools = settings
                .get("defaultTools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            tools.retain(|tool| tool.as_str() != Some("+codemode"));
            if codemode == "always" {
                tools.push(Value::String("+codemode".into()));
            }
            settings.insert("defaultTools".into(), Value::Array(tools));
        }
        if let Some(mode) = body.get("codemodeMode").and_then(Value::as_str) {
            let entry = settings
                .entry("codemode".to_string())
                .or_insert_with(|| json!({}));
            if let Some(object) = entry.as_object_mut() {
                object.insert(
                    "mode".into(),
                    Value::String(if mode == "only" { "only" } else { "on" }.into()),
                );
            }
        }
        if let Some(budget) = body.get("codemodeInlineBudget").and_then(Value::as_u64) {
            let entry = settings
                .entry("codemode".to_string())
                .or_insert_with(|| json!({}));
            if let Some(object) = entry.as_object_mut() {
                object.insert("inlineBudget".into(), Value::Number(budget.into()));
            }
        }
        Ok(())
    })?;
    Ok(tools_settings(config))
}

pub fn cache_miss_notices(config: &Config) -> bool {
    read_settings(config)
        .get("showCacheMissNotices")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn set_cache_miss_notices(config: &Config, enabled: bool) -> Result<bool> {
    update_settings(config, |settings| {
        settings.insert("showCacheMissNotices".into(), Value::Bool(enabled));
        Ok(())
    })?;
    Ok(enabled)
}

pub fn set_enabled_models(config: &Config, patterns: Vec<String>) -> Result<()> {
    update_settings(config, |settings| {
        settings.insert(
            "enabledModels".into(),
            Value::Array(patterns.into_iter().map(Value::String).collect()),
        );
        Ok(())
    })?;
    Ok(())
}

pub fn set_default_model(
    config: &Config,
    provider: &str,
    model_id: &str,
    thinking_level: Option<&str>,
) -> Result<()> {
    update_settings(config, |settings| {
        settings.insert("defaultProvider".into(), Value::String(provider.into()));
        settings.insert("defaultModel".into(), Value::String(model_id.into()));
        if let Some(level) = thinking_level {
            settings.insert("defaultThinkingLevel".into(), Value::String(level.into()));
        }
        Ok(())
    })?;
    Ok(())
}

pub fn set_default_thinking_level(config: &Config, level: &str) -> Result<()> {
    update_settings(config, |settings| {
        settings.insert("defaultThinkingLevel".into(), Value::String(level.into()));
        Ok(())
    })?;
    Ok(())
}

pub fn subagent_settings(config: &Config) -> SubagentSettings {
    let path = config.agent_dir.join("agents").join("settings.json");
    let value = read_json(&path).unwrap_or(Value::Null);
    SubagentSettings {
        built_in_enabled: value
            .get("builtInEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        disabled_built_ins: value
            .get("disabledBuiltIns")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        max_concurrent: value
            .get("maxConcurrent")
            .and_then(Value::as_u64)
            .map(|n| n.clamp(1, 32) as u32)
            .unwrap_or(10),
    }
}

pub fn set_subagent_settings(config: &Config, body: &Value) -> Result<SubagentSettings> {
    let path = config.agent_dir.join("agents").join("settings.json");
    let mut current = read_json(&path).unwrap_or_else(|| json!({"version": 1}));
    let object = current
        .as_object_mut()
        .ok_or_else(|| anyhow!("agents/settings.json is not an object"))?;
    if let Some(enabled) = body.get("builtInEnabled").and_then(Value::as_bool) {
        object.insert("builtInEnabled".into(), Value::Bool(enabled));
    }
    if let Some(max) = body.get("maxConcurrent").and_then(Value::as_u64) {
        object.insert(
            "maxConcurrent".into(),
            Value::Number(max.clamp(1, 32).into()),
        );
    }
    write_json_atomic(&path, &current, 0o600)?;
    Ok(subagent_settings(config))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTrustStatus {
    pub requires_trust: bool,
    pub trusted: bool,
    pub decision: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_path: Option<String>,
    pub inherited: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_error: Option<String>,
}

pub fn project_trust(config: &Config, cwd: &Path) -> ProjectTrustStatus {
    let requires_trust = cwd.join(".pi").join("mcp.json").is_file()
        || cwd.join(".pi").join("extensions").is_dir()
        || cwd.join(".pi").join("settings.json").is_file();
    let trust = read_json(&config.agent_dir.join("trust.json")).unwrap_or_else(|| json!({}));
    let canonical = crate::project::canonical(cwd);
    let mut candidate = canonical.as_path();
    loop {
        if let Some(value) = trust
            .get(candidate.to_string_lossy().as_ref())
            .and_then(Value::as_bool)
        {
            return ProjectTrustStatus {
                requires_trust,
                trusted: value,
                decision: Some(value),
                decision_path: Some(candidate.to_string_lossy().into_owned()),
                inherited: candidate != canonical.as_path(),
                decision_error: None,
            };
        }
        let Some(parent) = candidate.parent() else {
            break;
        };
        if parent == candidate {
            break;
        }
        candidate = parent;
    }
    ProjectTrustStatus {
        requires_trust,
        trusted: !requires_trust,
        decision: None,
        decision_path: None,
        inherited: false,
        decision_error: None,
    }
}

pub fn set_project_trust(config: &Config, cwd: &Path, trusted: bool) -> Result<ProjectTrustStatus> {
    let path = config.agent_dir.join("trust.json");
    let _guard = settings_lock();
    let mut trust = read_json(&path).unwrap_or_else(|| json!({}));
    let object = trust
        .as_object_mut()
        .ok_or_else(|| anyhow!("trust.json is not an object"))?;
    object.insert(
        crate::project::canonical(cwd)
            .to_string_lossy()
            .into_owned(),
        Value::Bool(trusted),
    );
    write_json_atomic(&path, &trust, 0o600)?;
    Ok(project_trust(config, cwd))
}

pub fn write_json_atomic(path: &Path, value: &Value, mode: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    let temp = path.with_file_name(format!(
        ".{}-{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("settings"),
        uuid::Uuid::new_v4()
    ));
    {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(fs::Permissions::from_mode(mode));
        }
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(&temp, path)?;
    Ok(())
}

pub fn parse_jsonc(text: &str) -> Result<Value> {
    Ok(serde_json::from_str(&strip_json_comments(text))?)
}

pub fn read_text(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)?)
}

pub fn default_cwd() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pi-cwd")
        .join(chrono::Local::now().format("%Y%m%d").to_string())
}

pub fn home_dir() -> Option<PathBuf> {
    crate::config::home_dir()
}
