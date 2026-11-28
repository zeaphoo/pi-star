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

// ---------------------------------------------------------------------------
// pi's `notices.*` transcript notices (one `showCacheMissNotices` before pi
// 1.6.1) and its `steeringMode` / `followUpMode` queue modes.
// ---------------------------------------------------------------------------

/// pi's `notices` settings: one switch per kind of transcript notice, in the
/// order `/settings` lists them. All default to off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Notices {
    pub cache_misses: bool,
    pub cache_warming: bool,
    pub compaction_costs: bool,
    pub reasoning_drops: bool,
}

impl Notices {
    /// Whether any notice at all is shown; a cheap skip for the entry scan.
    pub fn any(self) -> bool {
        self.cache_misses || self.cache_warming || self.compaction_costs || self.reasoning_drops
    }
}

/// The `notices` keys one settings object sets (absent ones are not defaulted).
fn notices_of(settings: &Value) -> Notices {
    let read = |key: &str| {
        settings
            .get("notices")
            .and_then(|notices| notices.get(key))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    Notices {
        cache_misses: read("cacheMisses"),
        cache_warming: read("cacheWarming"),
        compaction_costs: read("compactionCosts"),
        reasoning_drops: read("reasoningDrops"),
    }
}

/// The global file's notices, for Settings › General.
pub fn notices_settings(config: &Config) -> Notices {
    notices_of(&read_settings(config))
}

/// Global `notices`, with a project's `.pi/settings.json` overriding per switch.
pub fn effective_notices(config: &Config, cwd: &Path) -> Notices {
    let global = read_settings(config);
    let project = read_json(&cwd.join(".pi").join("settings.json"));
    effective_notices_of(&global, project.as_ref())
}

pub fn effective_notices_of(global: &Value, project: Option<&Value>) -> Notices {
    let mut notices = notices_of(global);
    let Some(project) = project else {
        return notices;
    };
    let switches = [
        ("cacheMisses", &mut notices.cache_misses),
        ("cacheWarming", &mut notices.cache_warming),
        ("compactionCosts", &mut notices.compaction_costs),
        ("reasoningDrops", &mut notices.reasoning_drops),
    ];
    for (key, slot) in switches {
        if let Some(value) = project
            .get("notices")
            .and_then(|notices| notices.get(key))
            .and_then(Value::as_bool)
        {
            *slot = value;
        }
    }
    notices
}

/// Write the switches present in `body` into the global `notices` object.
/// pi's defaults are all off, so a `false` writes the key explicitly (a project
/// file may set it to true and Settings › General must be able to say no).
pub fn update_notices_settings(config: &Config, body: &Value) -> Result<Notices> {
    let pick = |key: &str| body.get(key).and_then(Value::as_bool);
    let given = [
        ("cacheMisses", pick("cacheMisses")),
        ("cacheWarming", pick("cacheWarming")),
        ("compactionCosts", pick("compactionCosts")),
        ("reasoningDrops", pick("reasoningDrops")),
    ];
    if given.iter().all(|(_, value)| value.is_none()) {
        return Err(anyhow!(
            "Nothing to change: give cacheMisses, cacheWarming, compactionCosts or reasoningDrops"
        ));
    }
    update_settings(config, |settings| {
        let entry = settings
            .entry("notices".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let object = entry
            .as_object_mut()
            .ok_or_else(|| anyhow!("settings.json's notices is not an object"))?;
        for (key, value) in given {
            if let Some(value) = value {
                object.insert(key.to_string(), Value::Bool(value));
            }
        }
        Ok(())
    })?;
    Ok(notices_settings(config))
}

/// pi's `steeringMode` / `followUpMode`: how many queued messages a drain takes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QueueModeSetting {
    #[default]
    OneAtATime,
    All,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QueueModes {
    pub steering_mode: QueueModeSetting,
    pub follow_up_mode: QueueModeSetting,
}

/// pi reads an older file's `queueMode` as `steeringMode`.
fn queue_mode_of(settings: &Value, key: &str) -> QueueModeSetting {
    let value = settings
        .get(key)
        .or_else(|| (key == "steeringMode").then(|| settings.get("queueMode")).flatten());
    match value.and_then(Value::as_str) {
        Some("all") => QueueModeSetting::All,
        _ => QueueModeSetting::OneAtATime,
    }
}

fn queue_modes_of(settings: &Value) -> QueueModes {
    QueueModes {
        steering_mode: queue_mode_of(settings, "steeringMode"),
        follow_up_mode: queue_mode_of(settings, "followUpMode"),
    }
}

pub fn queue_modes(config: &Config) -> QueueModes {
    queue_modes_of(&read_settings(config))
}

/// A project's `.pi/settings.json` wins, as it does in pi.
pub fn effective_queue_modes(config: &Config, cwd: &Path) -> QueueModes {
    let project = read_json(&cwd.join(".pi").join("settings.json"));
    let Some(project) = project else {
        return queue_modes(config);
    };
    let global = queue_modes(config);
    let has = |key: &str| {
        project.get(key).and_then(Value::as_str).is_some()
            || (key == "steeringMode" && project.get("queueMode").and_then(Value::as_str).is_some())
    };
    QueueModes {
        steering_mode: if has("steeringMode") { queue_mode_of(&project, "steeringMode") } else { global.steering_mode },
        follow_up_mode: if has("followUpMode") { queue_mode_of(&project, "followUpMode") } else { global.follow_up_mode },
    }
}

/// Write the modes present in `body` into the global settings file.
/// `one-at-a-time` is pi's default, so choosing it removes the key.
pub fn update_queue_modes(config: &Config, body: &Value) -> Result<QueueModes> {
    let parse = |key: &str| -> Result<Option<QueueModeSetting>> {
        let Some(value) = body.get(key) else {
            return Ok(None);
        };
        match value.as_str() {
            Some("one-at-a-time") => Ok(Some(QueueModeSetting::OneAtATime)),
            Some("all") => Ok(Some(QueueModeSetting::All)),
            _ => Err(anyhow!(
                "{key} must be \"one-at-a-time\" or \"all\""
            )),
        }
    };
    let steering = parse("steeringMode")?;
    let follow_up = parse("followUpMode")?;
    if steering.is_none() && follow_up.is_none() {
        return Err(anyhow!(
            "Nothing to change: give steeringMode or followUpMode"
        ));
    }
    update_settings(config, |settings| {
        if let Some(mode) = steering {
            // pi migrates the older `queueMode` into `steeringMode` and drops it.
            settings.remove("queueMode");
            if mode == QueueModeSetting::All {
                settings.insert("steeringMode".into(), Value::String("all".into()));
            } else {
                settings.remove("steeringMode");
            }
        }
        if let Some(mode) = follow_up {
            if mode == QueueModeSetting::All {
                settings.insert("followUpMode".into(), Value::String("all".into()));
            } else {
                settings.remove("followUpMode");
            }
        }
        Ok(())
    })?;
    Ok(queue_modes(config))
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

/// Project-local resources the SDK gates behind trust.
const TRUST_REQUIRING_PROJECT_ENTRIES: [&str; 8] = [
    "settings.json",
    "mcp.json",
    "extensions",
    "skills",
    "prompts",
    "themes",
    "SYSTEM.md",
    "APPEND_SYSTEM.md",
];

/// Whether `cwd` has project-local resources the SDK gates behind trust:
/// trust-requiring entries under its `.pi`, or `.agents/skills` in the folder
/// or an ancestor. The user's own `~/.agents/skills` is global and never counts.
pub fn has_trust_requiring_project_resources(cwd: &Path) -> bool {
    let config_dir = cwd.join(".pi");
    if TRUST_REQUIRING_PROJECT_ENTRIES
        .iter()
        .any(|entry| config_dir.join(entry).exists())
    {
        return true;
    }
    let user_skills = crate::config::home_dir().map(|home| {
        crate::project::canonical(&home)
            .join(".agents")
            .join("skills")
    });
    let mut current = crate::project::canonical(cwd);
    loop {
        let skills = current.join(".agents").join("skills");
        if user_skills
            .as_ref()
            .is_none_or(|user_skills| !crate::project::same_path(user_skills, &skills))
            && skills.exists()
        {
            return true;
        }
        let Some(parent) = current.parent() else {
            return false;
        };
        if parent == current {
            return false;
        }
        current = parent.to_path_buf();
    }
}

pub fn project_trust(config: &Config, cwd: &Path) -> ProjectTrustStatus {
    let requires_trust = has_trust_requiring_project_resources(cwd);
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

pub fn home_dir() -> Option<PathBuf> {
    crate::config::home_dir()
}

#[cfg(test)]
mod trust_tests {
    use super::*;

    #[test]
    fn every_pi_resource_requires_trust() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("project");
        fs::create_dir_all(&cwd).unwrap();
        assert!(!has_trust_requiring_project_resources(&cwd));

        for entry in TRUST_REQUIRING_PROJECT_ENTRIES {
            let path = cwd.join(".pi").join(entry);
            if entry.contains('.') {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, "").unwrap();
            } else {
                fs::create_dir_all(&path).unwrap();
            }
            assert!(
                has_trust_requiring_project_resources(&cwd),
                "{entry} must require trust"
            );
            if path.is_dir() {
                fs::remove_dir_all(&path).unwrap();
            } else {
                fs::remove_file(&path).unwrap();
            }
            assert!(!has_trust_requiring_project_resources(&cwd));
        }
    }

    #[test]
    fn agents_skills_in_the_folder_or_an_ancestor_requires_trust() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("repo").join("nested");
        fs::create_dir_all(&cwd).unwrap();
        assert!(!has_trust_requiring_project_resources(&cwd));

        let project_skills = temp.path().join("repo").join(".agents").join("skills");
        fs::create_dir_all(&project_skills).unwrap();
        assert!(has_trust_requiring_project_resources(&cwd));
        fs::remove_dir_all(&project_skills).unwrap();
        assert!(!has_trust_requiring_project_resources(&cwd));

        let own_skills = cwd.join(".agents").join("skills");
        fs::create_dir_all(&own_skills).unwrap();
        assert!(has_trust_requiring_project_resources(&cwd));
    }
}
