//! Sub-agent profiles: the built-ins and the files under `agents/` that the
//! Settings › Sub-agents panel lists. Mirrors pi-web's `lib/subagents.ts` and
//! `lib/subagent-settings.ts` closely enough that both UIs see the same scopes,
//! fields and switches.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use serde_json::{Map, Value, json};

use crate::config::Config;
use crate::settings;

pub const DEFAULT_TOOLS: [&str; 7] = ["read", "bash", "edit", "write", "grep", "find", "ls"];
pub const READ_ONLY_TOOLS: [&str; 4] = ["read", "grep", "find", "ls"];
const BUILTIN_TOOLS: [&str; 7] = DEFAULT_TOOLS;
const THINKING_LEVELS: [&str; 7] = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
const MANAGED_KEYS: [&str; 17] = [
    "description",
    "display_name",
    "tools",
    "load_skills",
    "load_extensions",
    "codemode",
    "mcp",
    "enabled",
    "inherit_context",
    "run_in_background",
    "model",
    "thinking",
    "max_turns",
    "prompt_mode",
    "color",
    "isolation",
    "persist_session",
];

pub fn builtin_profiles() -> Vec<Value> {
    vec![
        json!({
            "name": "general-purpose",
            "displayName": "General purpose",
            "description": "Handle a focused implementation or investigation task",
            "systemPrompt": "Work autonomously on the delegated task. Keep the final answer concise and include important files, decisions, and remaining risks.",
            "tools": DEFAULT_TOOLS,
            "loadSkills": false,
            "loadExtensions": false,
            "codemode": "off",
            "mcp": "off",
            "promptMode": "append",
            "inheritContext": false,
            "runInBackground": false,
            "enabled": true,
            "scope": "builtin",
        }),
        json!({
            "name": "explore",
            "displayName": "Explore",
            "description": "Quickly inspect a codebase without modifying it",
            "systemPrompt": "Explore the codebase to answer the delegated question. Do not modify files. Report concrete findings with file paths and relevant symbols.",
            "tools": READ_ONLY_TOOLS,
            "loadSkills": false,
            "loadExtensions": false,
            "codemode": "off",
            "mcp": "off",
            "promptMode": "append",
            "inheritContext": false,
            "runInBackground": false,
            "enabled": true,
            "scope": "builtin",
        }),
        json!({
            "name": "plan",
            "displayName": "Plan",
            "description": "Design an implementation plan without modifying files",
            "systemPrompt": "Produce an implementation-ready plan for the delegated task. Inspect the repository as needed, do not modify files, and call out dependencies, risks, and verification steps.",
            "tools": READ_ONLY_TOOLS,
            "loadSkills": false,
            "loadExtensions": false,
            "codemode": "off",
            "mcp": "off",
            "promptMode": "append",
            "inheritContext": false,
            "runInBackground": false,
            "enabled": true,
            "scope": "builtin",
        }),
    ]
}

pub fn settings_path(config: &Config) -> PathBuf {
    config.agent_dir.join("agents").join("settings.json")
}

fn read_settings_raw(config: &Config) -> Value {
    std::fs::read_to_string(settings_path(config))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

pub fn disabled_builtins(config: &Config) -> HashSet<String> {
    read_settings_raw(config)
        .get("disabledBuiltIns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|name| name.trim().to_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}

fn write_settings(config: &Config, value: &Value, mode: u32) -> Result<()> {
    settings::write_json_atomic(&settings_path(config), value, mode)
}

pub fn set_builtin_enabled(config: &Config, enabled: bool) -> Result<()> {
    let mut stored = read_settings_raw(config);
    let object = stored
        .as_object_mut()
        .ok_or_else(|| anyhow!("agents/settings.json is not an object"))?;
    object.insert("version".into(), json!(1));
    object.insert("builtInEnabled".into(), Value::Bool(enabled));
    write_settings(config, &stored, 0o600)
}

pub fn set_builtin_disabled(config: &Config, name: &str, disabled: bool) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("Sub-agent name is required"));
    }
    let mut stored = read_settings_raw(config);
    let object = stored
        .as_object_mut()
        .ok_or_else(|| anyhow!("agents/settings.json is not an object"))?;
    let mut list: Vec<String> = object
        .get("disabledBuiltIns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    let key = trimmed.to_lowercase();
    let already = list.iter().any(|entry| entry.to_lowercase() == key);
    if disabled != already {
        if disabled {
            list.push(trimmed.to_string());
        } else {
            list.retain(|entry| entry.to_lowercase() != key);
        }
        object.insert("version".into(), json!(1));
        object.insert(
            "disabledBuiltIns".into(),
            Value::Array(list.into_iter().map(Value::String).collect()),
        );
        write_settings(config, &stored, 0o600)?;
    }
    Ok(())
}

pub fn set_max_concurrent(config: &Config, value: u32) -> Result<()> {
    if !(1..=32).contains(&value) {
        return Err(anyhow!("maxConcurrent must be an integer between 1 and 32"));
    }
    let mut stored = read_settings_raw(config);
    let object = stored
        .as_object_mut()
        .ok_or_else(|| anyhow!("agents/settings.json is not an object"))?;
    object.insert("version".into(), json!(1));
    object.insert("maxConcurrent".into(), json!(value));
    write_settings(config, &stored, 0o600)
}

fn profile_dirs(config: &Config, cwd: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (config.agent_dir.join("agents"), "global"),
        (cwd.join(".agents").join("agents"), "workspace"),
        (cwd.join(".pi").join("agents"), "project"),
    ]
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn name_list(value: Option<&Value>) -> Vec<String> {
    let items: Vec<String> = match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect(),
        Some(Value::String(value)) => value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    };
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item.to_lowercase()))
        .collect()
}

fn resource_switch(value: Option<&Value>, fallback: bool) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => {
            let value = value.trim().to_ascii_lowercase();
            !matches!(value.as_str(), "none" | "false")
        }
        Some(Value::Array(_)) => true,
        _ => fallback,
    }
}

fn profile_name_list(value: Option<&Value>) -> Option<Vec<String>> {
    match value {
        Some(Value::Array(_)) => Some(name_list(value)),
        Some(Value::String(value)) => {
            let trimmed = value.trim().to_ascii_lowercase();
            if matches!(trimmed.as_str(), "none" | "all" | "true" | "false") {
                None
            } else {
                let list = name_list(Some(&Value::String(value.clone())));
                (!list.is_empty()).then_some(list)
            }
        }
        _ => None,
    }
}

fn parse_tools(value: Option<&Value>, fallback: &[&str]) -> Vec<String> {
    let list = name_list(value);
    if list.iter().any(|item| item == "none") {
        return Vec::new();
    }
    if list.iter().any(|item| item == "all" || item == "*") {
        return DEFAULT_TOOLS.iter().map(|item| item.to_string()).collect();
    }
    if list.is_empty() {
        return fallback.iter().map(|item| item.to_string()).collect();
    }
    let allowed: HashSet<&str> = BUILTIN_TOOLS.iter().copied().collect();
    let mut seen = HashSet::new();
    list.into_iter()
        .filter(|item| allowed.contains(item.as_str()))
        .filter(|item| seen.insert(item.to_lowercase()))
        .collect()
}

fn extension_selectors(value: Option<&Value>) -> Vec<String> {
    name_list(value)
        .into_iter()
        .filter(|item| item.to_ascii_lowercase().starts_with("ext:"))
        .collect()
}

fn parse_codemode(value: Option<&Value>) -> &'static str {
    match value {
        Some(Value::Bool(true)) => "on",
        Some(Value::String(value)) if value == "on" => "on",
        Some(Value::String(value)) if value == "inherit" => "inherit",
        _ => "off",
    }
}

fn parse_mcp(value: Option<&Value>) -> &'static str {
    match value {
        Some(Value::Bool(true)) => "on",
        Some(Value::String(value)) if value == "on" => "on",
        _ => "off",
    }
}

fn parse_bool(value: Option<&Value>, fallback: bool) -> bool {
    value.and_then(Value::as_bool).unwrap_or(fallback)
}

fn parse_profile(path: &Path, scope: &str) -> Option<Value> {
    let source = std::fs::read_to_string(path).ok()?;
    let (frontmatter, body) = pi_star_harness::frontmatter::parse_frontmatter(&source).ok()?;
    let object = frontmatter.as_object()?;
    let name = string_value(object.get("name")).or_else(|| {
        path.file_stem()
            .and_then(|value| value.to_str())
            .map(str::to_owned)
    })?;
    if !valid_name(&name) {
        return None;
    }
    let display_name = string_value(object.get("display_name")).unwrap_or_else(|| name.clone());
    let description = string_value(object.get("description")).unwrap_or_else(|| name.clone());
    let tools = parse_tools(object.get("tools"), &DEFAULT_TOOLS);
    let disallowed = parse_tools(object.get("disallowed_tools"), &[]);
    let disallowed_set: HashSet<String> = disallowed.into_iter().collect();
    let tools: Vec<String> = tools
        .into_iter()
        .filter(|tool| !disallowed_set.contains(tool))
        .collect();
    let disallowed_extension_tools = extension_selectors(object.get("disallowed_tools"));
    let extension_selectors = extension_selectors(object.get("tools"));
    let extension_tools: Vec<String> = extension_selectors
        .iter()
        .filter(|tool| {
            let allowed = tool.to_ascii_lowercase();
            !disallowed_extension_tools.iter().any(|denied| {
                let denied = denied.to_ascii_lowercase();
                denied == "*" || allowed == denied || allowed.starts_with(&format!("{denied}/"))
            })
        })
        .cloned()
        .collect();
    let skills = profile_name_list(object.get("skills"));
    let extensions = profile_name_list(object.get("extensions"))
        .filter(|list| !list.iter().any(|item| item == "*"));
    let thinking = string_value(object.get("thinking"))
        .filter(|value| THINKING_LEVELS.contains(&value.as_str()));
    let max_turns = object
        .get("max_turns")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0);
    let mut profile = Map::new();
    profile.insert("name".into(), json!(name));
    profile.insert("displayName".into(), json!(display_name));
    profile.insert("description".into(), json!(description));
    profile.insert("systemPrompt".into(), json!(body.trim()));
    profile.insert("tools".into(), json!(tools));
    if !extension_tools.is_empty() {
        profile.insert("extensionTools".into(), json!(extension_tools));
    }
    if !disallowed_extension_tools.is_empty() {
        profile.insert(
            "disallowedExtensionTools".into(),
            json!(disallowed_extension_tools),
        );
    }
    if let Some(skills) = skills {
        profile.insert("skills".into(), json!(skills));
    }
    if let Some(extensions) = extensions {
        profile.insert("extensions".into(), json!(extensions));
    }
    profile.insert(
        "loadSkills".into(),
        json!(resource_switch(
            object.get("load_skills").or_else(|| object.get("skills")),
            false
        )),
    );
    profile.insert(
        "loadExtensions".into(),
        json!(resource_switch(
            object
                .get("load_extensions")
                .or_else(|| object.get("extensions")),
            !extension_selectors.is_empty()
        )),
    );
    profile.insert(
        "codemode".into(),
        json!(parse_codemode(object.get("codemode"))),
    );
    profile.insert("mcp".into(), json!(parse_mcp(object.get("mcp"))));
    if let Some(model) = string_value(object.get("model")) {
        profile.insert("model".into(), json!(model));
    }
    if let Some(thinking) = thinking {
        profile.insert("thinking".into(), json!(thinking));
    }
    if let Some(max_turns) = max_turns {
        profile.insert("maxTurns".into(), json!(max_turns));
    }
    profile.insert(
        "inheritContext".into(),
        json!(parse_bool(object.get("inherit_context"), false)),
    );
    profile.insert(
        "runInBackground".into(),
        json!(parse_bool(object.get("run_in_background"), false)),
    );
    profile.insert(
        "promptMode".into(),
        json!(
            if object.get("prompt_mode").and_then(Value::as_str) == Some("replace") {
                "replace"
            } else {
                "append"
            }
        ),
    );
    if let Some(color) = string_value(object.get("color")) {
        profile.insert("color".into(), json!(color));
    }
    if let Some(isolation) = object
        .get("isolation")
        .and_then(Value::as_str)
        .filter(|value| matches!(*value, "worktree" | "off"))
    {
        profile.insert("isolation".into(), json!(isolation));
    }
    if let Some(persist) = object.get("persist_session").and_then(Value::as_bool) {
        profile.insert("persistSession".into(), json!(persist));
    }
    profile.insert(
        "enabled".into(),
        json!(parse_bool(object.get("enabled"), true)),
    );
    profile.insert("scope".into(), json!(scope));
    profile.insert("filePath".into(), json!(path.to_string_lossy()));
    Some(Value::Object(profile))
}

fn read_dir_profiles(dir: &Path, scope: &str) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            entry.path().is_file()
                && entry.path().extension().and_then(|value| value.to_str()) == Some("md")
        })
        .filter_map(|entry| parse_profile(&entry.path(), scope))
        .collect()
}

pub fn list_profiles(config: &Config, cwd: &Path) -> Vec<Value> {
    let disabled = disabled_builtins(config);
    let mut profiles: Vec<Value> = builtin_profiles()
        .into_iter()
        .map(|mut profile| {
            let name = profile
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_lowercase();
            profile["enabled"] = json!(!disabled.contains(&name));
            profile
        })
        .collect();
    for (dir, scope) in profile_dirs(config, cwd) {
        profiles.extend(read_dir_profiles(&dir, scope));
    }
    profiles.sort_by(|a, b| {
        let left = a
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_lowercase();
        let right = b
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_lowercase();
        left.cmp(&right)
    });
    profiles
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

fn writable_dir(config: &Config, cwd: &Path, scope: &str) -> Result<PathBuf> {
    let dir = match scope {
        "global" => config.agent_dir.join("agents"),
        "project" => cwd.join(".pi").join("agents"),
        _ => return Err(anyhow!("Agent scope must be global or project")),
    };
    if scope == "project" {
        // Reject symlinked paths that escape the project root.
        let mut ancestor = dir.clone();
        while !ancestor.exists() {
            let Some(parent) = ancestor.parent() else {
                return Err(anyhow!(
                    "Agent profile directory is outside the project root"
                ));
            };
            ancestor = parent.to_path_buf();
        }
        let root = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        let ancestor = std::fs::canonicalize(&ancestor).unwrap_or(ancestor);
        if !ancestor.starts_with(&root) {
            return Err(anyhow!(
                "Agent profile directory is outside the project root"
            ));
        }
    }
    Ok(dir)
}

fn validate_saved_profile(profile: &Value) -> Result<(String, Vec<String>, Vec<String>)> {
    let name = profile
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .ok_or_else(|| anyhow!("profile.name required"))?;
    if !valid_name(name) {
        return Err(anyhow!(
            "Agent name may contain only letters, numbers, dots, underscores, and hyphens"
        ));
    }
    let tools = parse_tools(profile.get("tools"), &DEFAULT_TOOLS);
    let extension_tools = name_list(profile.get("extensionTools"));
    if let Some(thinking) = profile.get("thinking").and_then(Value::as_str) {
        if !THINKING_LEVELS.contains(&thinking) {
            return Err(anyhow!("Invalid thinking level: {thinking}"));
        }
    }
    Ok((name.to_string(), tools, extension_tools))
}

fn yaml_string(value: &Value) -> String {
    yaml_serde::to_string(value)
        .unwrap_or_else(|_| serde_json::to_string(value).unwrap_or_default())
        .trim_end()
        .to_string()
}

pub fn save_profile(config: &Config, cwd: &Path, scope: &str, profile: &Value) -> Result<Value> {
    let (name, tools, extension_tools) = validate_saved_profile(profile)?;
    let dir = writable_dir(config, cwd, scope)?;
    std::fs::create_dir_all(&dir)?;
    let file_path = dir.join(format!("{name}.md"));
    let stored = std::fs::read_to_string(&file_path)
        .ok()
        .and_then(|text| pi_star_harness::frontmatter::parse_frontmatter(&text).ok())
        .map(|(frontmatter, _)| frontmatter)
        .unwrap_or(Value::Null);
    let stored_object = stored.as_object().cloned().unwrap_or_default();

    let mut managed = Map::new();
    let text = |key: &str| {
        profile
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_default()
    };
    managed.insert(
        "description".into(),
        json!(if text("description").is_empty() {
            name.clone()
        } else {
            text("description").to_string()
        }),
    );
    managed.insert(
        "display_name".into(),
        json!(if text("displayName").is_empty() {
            name.clone()
        } else {
            text("displayName").to_string()
        }),
    );
    let mut all_tools = tools.clone();
    all_tools.extend(extension_tools.clone());
    for stored_tool in name_list(stored_object.get("tools")) {
        if stored_tool.to_ascii_lowercase().starts_with("ext:")
            && !all_tools
                .iter()
                .any(|tool| tool.eq_ignore_ascii_case(&stored_tool))
        {
            all_tools.push(stored_tool);
        }
    }
    managed.insert(
        "tools".into(),
        json!(if all_tools.is_empty() {
            "none".to_string()
        } else {
            all_tools.join(", ")
        }),
    );
    let load_skills = profile
        .get("loadSkills")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let load_extensions = profile
        .get("loadExtensions")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    managed.insert("load_skills".into(), json!(load_skills));
    managed.insert("load_extensions".into(), json!(load_extensions));
    managed.insert(
        "codemode".into(),
        json!(match profile.get("codemode").and_then(Value::as_str) {
            Some(value @ ("inherit" | "on" | "off")) => value,
            _ => "off",
        }),
    );
    let stored_mcp = stored_object.get("mcp");
    let requested_mcp = profile.get("mcp").and_then(Value::as_str);
    if requested_mcp == Some("on") || stored_mcp.is_some() {
        managed.insert(
            "mcp".into(),
            json!(if requested_mcp == Some("on") {
                "on"
            } else {
                "off"
            }),
        );
    }
    managed.insert(
        "enabled".into(),
        json!(
            profile
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        ),
    );
    managed.insert(
        "inherit_context".into(),
        json!(
            profile
                .get("inheritContext")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    managed.insert(
        "run_in_background".into(),
        json!(
            profile
                .get("runInBackground")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    managed.insert(
        "prompt_mode".into(),
        json!(
            if profile.get("promptMode").and_then(Value::as_str) == Some("replace") {
                "replace"
            } else {
                "append"
            }
        ),
    );
    if let Some(skills) = profile.get("skills").and_then(Value::as_array) {
        managed.insert(
            "skills".into(),
            json!(name_list(Some(&Value::Array(skills.clone())))),
        );
    } else if let Some(stored_skills) = stored_object.get("skills") {
        if matches!(stored_skills, Value::Bool(_) | Value::String(_)) {
            managed.insert("skills".into(), json!(load_skills));
        }
    }
    if let Some(stored_extensions) = stored_object.get("extensions") {
        if matches!(stored_extensions, Value::Bool(_) | Value::String(_)) {
            managed.insert("extensions".into(), json!(load_extensions));
        }
    }
    if !text("model").is_empty() {
        managed.insert("model".into(), json!(text("model")));
    }
    if !text("thinking").is_empty() {
        managed.insert("thinking".into(), json!(text("thinking")));
    }
    if let Some(max_turns) = profile.get("maxTurns").and_then(Value::as_i64) {
        if max_turns > 0 {
            managed.insert("max_turns".into(), json!(max_turns));
        }
    }
    if !text("color").is_empty() {
        managed.insert("color".into(), json!(text("color")));
    }
    if let Some(isolation) = profile
        .get("isolation")
        .and_then(Value::as_str)
        .filter(|value| matches!(*value, "worktree" | "off"))
    {
        managed.insert("isolation".into(), json!(isolation));
    }
    if let Some(persist) = profile.get("persistSession").and_then(Value::as_bool) {
        managed.insert("persist_session".into(), json!(persist));
    }

    let mut frontmatter = managed.clone();
    for (key, value) in &stored_object {
        if !MANAGED_KEYS.contains(&key.as_str()) && !frontmatter.contains_key(key) {
            frontmatter.insert(key.clone(), value.clone());
        }
    }
    let system_prompt = profile
        .get("systemPrompt")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let text = format!(
        "---\n{}\n---\n\n{}\n",
        yaml_string(&Value::Object(frontmatter)),
        system_prompt
    );
    std::fs::write(&file_path, text)?;
    parse_profile(&file_path, scope)
        .ok_or_else(|| anyhow!("Could not read the saved agent profile"))
}

pub fn delete_profile(config: &Config, cwd: &Path, scope: &str, name: &str) -> Result<()> {
    let name = name.trim();
    if !valid_name(name) {
        return Err(anyhow!(
            "Agent name may contain only letters, numbers, dots, underscores, and hyphens"
        ));
    }
    let dir = writable_dir(config, cwd, scope)?;
    let path = dir.join(format!("{name}.md"));
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub fn toggle_profile(
    config: &Config,
    cwd: &Path,
    scope: &str,
    name: &str,
    enabled: bool,
) -> Result<Value> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("name required"));
    }
    if scope == "builtin" {
        let profile = builtin_profiles()
            .into_iter()
            .find(|profile| {
                profile
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value.eq_ignore_ascii_case(name))
            })
            .ok_or_else(|| anyhow!("Agent profile not found"))?;
        set_builtin_disabled(config, name, !enabled)?;
        let mut profile = profile;
        profile["enabled"] = json!(enabled);
        return Ok(profile);
    }
    let dir = writable_dir(config, cwd, scope)?;
    let path = dir.join(format!("{name}.md"));
    let text = std::fs::read_to_string(&path).map_err(|_| anyhow!("Agent profile not found"))?;
    let updated = set_frontmatter_bool(&text, "enabled", enabled);
    std::fs::write(&path, updated)?;
    parse_profile(&path, scope).ok_or_else(|| anyhow!("Could not read the agent profile"))
}

fn set_frontmatter_bool(text: &str, key: &str, value: bool) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    if lines.first().map(|line| line.trim()) != Some("---") {
        return text.to_string();
    }
    let Some(end) = lines
        .iter()
        .skip(1)
        .position(|line| line.trim() == "---")
        .map(|index| index + 1)
    else {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_listed_and_toggle_through_settings() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config::new(Some(temp.path().join("agent")), None).unwrap();
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let profiles = list_profiles(&config, &cwd);
        assert!(
            profiles
                .iter()
                .any(|profile| profile["name"] == "general-purpose")
        );
        set_builtin_disabled(&config, "explore", true).unwrap();
        let profiles = list_profiles(&config, &cwd);
        let explore = profiles
            .iter()
            .find(|profile| profile["name"] == "explore")
            .unwrap();
        assert_eq!(explore["enabled"], false);
    }

    #[test]
    fn profile_round_trips_managed_and_unknown_frontmatter() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config::new(Some(temp.path().join("agent")), None).unwrap();
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let dir = cwd.join(".pi").join("agents");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("review.md"),
            "---\nname: review\ndescription: old\nallowed_subagents: [a]\ncodemode: inherit\n---\n\nOld prompt\n",
        )
        .unwrap();
        let saved = save_profile(
            &config,
            &cwd,
            "project",
            &json!({
                "name": "review",
                "displayName": "Review",
                "description": "Reviews code",
                "systemPrompt": "New prompt",
                "tools": ["read", "grep"],
                "loadSkills": true,
                "loadExtensions": false,
                "codemode": "inherit",
                "mcp": "off",
                "promptMode": "append",
                "inheritContext": false,
                "runInBackground": false,
                "enabled": true,
            }),
        )
        .unwrap();
        assert_eq!(saved["displayName"], "Review");
        let text = std::fs::read_to_string(dir.join("review.md")).unwrap();
        assert!(text.contains("allowed_subagents"));
        assert!(text.contains("codemode: inherit"));
        assert!(text.contains("New prompt"));
        let profiles = list_profiles(&config, &cwd);
        let review = profiles
            .iter()
            .find(|profile| profile["name"] == "review" && profile["scope"] == "project")
            .unwrap();
        assert_eq!(review["loadSkills"], true);
        assert_eq!(review["tools"], json!(["read", "grep"]));
    }
}
