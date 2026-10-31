//! QuickPI's system prompt, rendered the way pi's coding agent renders it.
//!
//! pi's `buildSystemPrompt` composes ordered, independently replaceable
//! sections (preamble, tools, rules, docs, addendum, project_context, skills,
//! cwd) and joins them with blank lines. The old pi-star prompt was a single
//! sentence plus skill bodies; this module restores the section format, the
//! project context files, `SYSTEM.md`/`APPEND_SYSTEM.md`, the docs paths and
//! the model-visible `<available_skills>` listing.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::Value;

use crate::config::Config;
use crate::resources;
use crate::settings;

const DEFAULT_PREAMBLE: &str = "You are an expert coding assistant operating inside pi, a coding agent harness. You help users by reading files, executing commands, editing code, and writing new files.";

fn tool_snippet(name: &str) -> Option<&'static str> {
    match name {
        "read" => Some("Read file contents"),
        "bash" => Some("Execute bash commands (ls, grep, find, etc.)"),
        "powershell" => Some("Execute PowerShell commands"),
        "edit" => Some(
            "Make precise file edits with exact text replacement, including multiple disjoint edits in one call",
        ),
        "write" => Some("Create or overwrite files"),
        "grep" => Some("Search file contents for patterns (respects .gitignore)"),
        "find" => Some("Find files by glob pattern (respects .gitignore)"),
        "ls" => Some("List directory contents"),
        _ => None,
    }
}

pub fn tool_guidelines(name: &str) -> &'static [&'static str] {
    match name {
        "read" => &["Use read to examine files instead of cat or sed."],
        "bash" => {
            &["You can inspect PI_* environment variables for current model and session details."]
        }
        "powershell" => {
            &["You can inspect PI_* environment variables for current model and session details."]
        }
        "edit" => &[
            "Use edit for precise changes (edits[].oldText must match exactly)",
            "When changing multiple separate locations in one file, use one edit call with multiple entries in edits[] instead of multiple edit calls",
            "Each edits[].oldText is matched against the original file, not after earlier edits are applied. Do not emit overlapping or nested edits. Merge nearby changes into one edit.",
            "Keep edits[].oldText as small as possible while still being unique in the file. Do not pad with large unchanged regions.",
        ],
        "write" => &["Use write only for new files or complete rewrites."],
        _ => &[],
    }
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn section(name: &str, content: &str) -> String {
    format!("<{name}>\n{content}\n</{name}>")
}

fn build_rules(tools: &[String]) -> String {
    let has_bash = tools.iter().any(|tool| tool == "bash");
    let has_powershell = tools.iter().any(|tool| tool == "powershell");
    let has_grep = tools.iter().any(|tool| tool == "grep");
    let has_find = tools.iter().any(|tool| tool == "find");
    let has_ls = tools.iter().any(|tool| tool == "ls");
    let mut rules: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut add = |rule: &str| {
        let normalized = rule.trim();
        if !normalized.is_empty() && seen.insert(normalized.to_string()) {
            rules.push(normalized.to_string());
        }
    };
    if (has_bash || has_powershell) && !has_grep && !has_find && !has_ls {
        if has_bash && has_powershell {
            add(
                "Use bash or PowerShell for file operations like listing, searching, and finding files",
            );
        } else if has_powershell {
            add("Use PowerShell for file operations like listing, searching, and finding files");
        } else {
            add("Use bash for file operations like ls, rg, find");
        }
    }
    for tool in tools {
        for rule in tool_guidelines(tool) {
            add(rule);
        }
    }
    add("Be concise in your responses");
    add("Show file paths clearly when working with files");
    rules
        .into_iter()
        .map(|rule| format!("- {rule}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn pi_package_dir() -> Option<&'static PathBuf> {
    static CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if let Ok(dir) = std::env::var("PI_PACKAGE_DIR") {
                let path = PathBuf::from(dir);
                if path.is_dir() {
                    return Some(path);
                }
            }
            let cli = std::env::var("PI_STAR_PI_CLI").unwrap_or_else(|_| "pi".to_string());
            let candidate = if cli.contains('/') {
                Some(PathBuf::from(&cli))
            } else {
                std::env::var_os("PATH").and_then(|paths| {
                    std::env::split_paths(&paths)
                        .map(|dir| dir.join(&cli))
                        .find(|path| path.is_file())
                })
            }?;
            let resolved = std::fs::canonicalize(&candidate).ok()?;
            let mut current = resolved.parent()?;
            loop {
                if current.join("package.json").is_file() {
                    return Some(current.to_path_buf());
                }
                current = current.parent()?;
            }
        })
        .as_ref()
}

fn docs_section() -> Option<String> {
    let root = pi_package_dir()?;
    let readme = root.join("README.md");
    let docs = root.join("docs");
    let examples = root.join("examples");
    Some(format!(
        "Pi documentation (read only when the user asks about pi itself, its SDK, extensions, themes, skills, or TUI):\n- Main documentation: {}\n- Additional docs: {}\n- Examples: {} (extensions, custom tools, SDK)\n- When reading pi docs or examples, resolve docs/... under Additional docs and examples/... under Examples, not the current working directory\n- When asked about: extensions (docs/extensions.md, examples/extensions/), themes (docs/themes.md), skills (docs/skills.md), prompt templates (docs/prompt-templates.md), TUI components (docs/tui.md), keybindings (docs/keybindings.md), SDK integrations (docs/sdk.md), custom providers (docs/custom-provider.md), adding models (docs/models.md), pi packages (docs/packages.md), environment variables (docs/environment-variables.md), MCP servers (docs/mcp.md), codemode scripts and non-LLM models such as classifiers and image models (docs/codemode.md)\n- When working on pi topics, read the docs and examples, and follow .md cross-references before implementing\n- Always read pi .md files completely and follow links to related docs (e.g. tui.md for TUI API details)",
        readme.display(),
        docs.display(),
        examples.display(),
    ))
}

const CONTEXT_CANDIDATES: [&str; 5] = [
    "AGENTS.override.md",
    "AGENTS.md",
    "AGENTS.MD",
    "CLAUDE.md",
    "CLAUDE.MD",
];

fn context_file_in(dir: &Path) -> Option<(PathBuf, String)> {
    for name in CONTEXT_CANDIDATES {
        let path = dir.join(name);
        if path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                return Some((path, content));
            }
        }
    }
    None
}

fn project_context_files(config: &Config, cwd: &Path) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(global) = context_file_in(&config.agent_dir) {
        seen.insert(global.0.clone());
        files.push(global);
    }
    let mut ancestors = Vec::new();
    let mut current = cwd.to_path_buf();
    loop {
        if let Some(file) = context_file_in(&current) {
            if seen.insert(file.0.clone()) {
                ancestors.insert(0, file);
            }
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent.to_path_buf();
    }
    files.extend(ancestors);
    files
}

fn project_context_section(files: &[(PathBuf, String)]) -> Option<String> {
    if files.is_empty() {
        return None;
    }
    let mut parts = vec!["Project-specific instructions and guidelines:".to_string()];
    for (path, content) in files {
        parts.push(format!(
            "<project_instructions path=\"{}\">\n{}\n</project_instructions>",
            path.to_string_lossy(),
            content
        ));
    }
    Some(parts.join("\n\n"))
}

fn discover_system_prompt(config: &Config, cwd: &Path) -> Option<String> {
    let trusted = settings::project_trust(config, cwd).trusted;
    let project = cwd.join(resources::CONFIG_DIR_NAME).join("SYSTEM.md");
    if trusted && project.is_file() {
        return std::fs::read_to_string(project).ok();
    }
    let global = config.agent_dir.join("SYSTEM.md");
    global
        .is_file()
        .then(|| std::fs::read_to_string(global).ok())
        .flatten()
}

fn discover_append_system_prompt(config: &Config, cwd: &Path) -> Option<String> {
    let trusted = settings::project_trust(config, cwd).trusted;
    let project = cwd
        .join(resources::CONFIG_DIR_NAME)
        .join("APPEND_SYSTEM.md");
    if trusted && project.is_file() {
        return std::fs::read_to_string(project).ok();
    }
    let global = config.agent_dir.join("APPEND_SYSTEM.md");
    global
        .is_file()
        .then(|| std::fs::read_to_string(global).ok())
        .flatten()
}

fn skills_section(config: &Config, cwd: &Path, tools: &[String]) -> Option<String> {
    let reader = if tools.iter().any(|tool| tool == "read") {
        Some("read")
    } else if tools.iter().any(|tool| tool == "bash") {
        Some("bash")
    } else {
        None
    }?;
    let (skills, _, _) = resources::load_skills(config, cwd);
    let visible: Vec<_> = skills
        .into_iter()
        .filter(|skill| !skill.disable_model_invocation)
        .collect();
    if visible.is_empty() {
        return None;
    }
    let mut lines = vec![
        "The following skills provide specialized instructions for specific tasks.".to_string(),
        if reader == "read" {
            "Use the read tool to load a skill's file when the task matches its description."
                .to_string()
        } else {
            "Use bash to load a skill's file when the task matches its description.".to_string()
        },
        "When a skill file references a relative path, resolve it against the skill directory (parent of SKILL.md / dirname of the path) and use that absolute path in tool commands.".to_string(),
        String::new(),
        "<available_skills>".to_string(),
    ];
    for skill in visible {
        lines.push("  <skill>".to_string());
        lines.push(format!("    <name>{}</name>", escape_xml(&skill.name)));
        lines.push(format!(
            "    <description>{}</description>",
            escape_xml(&skill.description)
        ));
        lines.push(format!(
            "    <location>{}</location>",
            escape_xml(&skill.file_path.to_string_lossy())
        ));
        lines.push("  </skill>".to_string());
    }
    lines.push("</available_skills>".to_string());
    Some(lines.join("\n"))
}

const DEFAULT_TOOL_NAMES: [&str; 4] = ["read", "bash", "edit", "write"];

fn merge_default_tools(base: Option<&Value>, overrides: Option<&Value>) -> Option<Vec<String>> {
    let base = base.and_then(Value::as_array);
    let overrides = overrides.and_then(Value::as_array);
    match (base, overrides) {
        (_, None) => base.map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        }),
        (Some(base), Some(overrides))
            if overrides.iter().all(|entry| {
                entry
                    .as_str()
                    .is_some_and(|value| value.starts_with('+') || value.starts_with('-'))
            }) =>
        {
            let mut merged: Vec<String> = base
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            merged.extend(
                overrides
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned),
            );
            Some(merged)
        }
        (_, Some(overrides)) => Some(
            overrides
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        ),
    }
}

fn resolve_default_tools(entries: &[String]) -> Vec<String> {
    let plain: Vec<String> = entries
        .iter()
        .filter(|entry| !entry.starts_with('+') && !entry.starts_with('-'))
        .cloned()
        .collect();
    let mut tools = if !plain.is_empty() || entries.is_empty() {
        plain
    } else {
        DEFAULT_TOOL_NAMES
            .iter()
            .map(|name| name.to_string())
            .collect()
    };
    for entry in entries {
        let Some(name) = entry.get(1..).filter(|name| !name.is_empty()) else {
            continue;
        };
        if entry.starts_with('+') {
            if !tools.iter().any(|tool| tool == name) {
                tools.push(name.to_string());
            }
        } else if entry.starts_with('-') {
            tools.retain(|tool| tool != name);
        }
    }
    tools
}

/// The tool loadout a session starts with when it has no persisted
/// `pi-web:tool-selection`: settings.json `defaultTools`, merged across the
/// project and global layers, with pi's own read/bash/edit/write fallback.
pub fn default_tool_names(config: &Config, cwd: &Path) -> Vec<String> {
    let global = settings::read_settings(config);
    let project = if settings::project_trust(config, cwd).trusted {
        std::fs::read_to_string(cwd.join(resources::CONFIG_DIR_NAME).join("settings.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let merged = merge_default_tools(global.get("defaultTools"), project.get("defaultTools"));
    match merged {
        Some(entries) => resolve_default_tools(&entries),
        None => DEFAULT_TOOL_NAMES
            .iter()
            .map(|name| name.to_string())
            .collect(),
    }
}

/// Render the QuickPI system prompt for a session. `active_tools` is the
/// loadout after `pi-web:tool-selection` and `set_tools` are applied.
pub fn build_system_prompt(config: &Config, cwd: &Path, active_tools: &[String]) -> String {
    let custom_prompt = discover_system_prompt(config, cwd)
        .map(|prompt| prompt.trim().to_string())
        .filter(|prompt| !prompt.is_empty());
    let mut sections: Vec<String> = Vec::new();
    match custom_prompt {
        Some(prompt) => sections.push(prompt),
        None => {
            sections.push(DEFAULT_PREAMBLE.to_string());
            let visible: Vec<(&String, &str)> = active_tools
                .iter()
                .filter_map(|tool| tool_snippet(tool).map(|snippet| (tool, snippet)))
                .collect();
            let tools = if visible.is_empty() {
                "(none)".to_string()
            } else {
                visible
                    .iter()
                    .map(|(name, snippet)| format!("- {name}: {snippet}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            sections.push(section(
                "tools",
                &format!(
                    "{tools}\n\nIn addition to the tools above, you may have access to other custom tools depending on the project."
                ),
            ));
            sections.push(section("rules", &build_rules(active_tools)));
            if let Some(docs) = docs_section() {
                sections.push(section("docs", &docs));
            }
        }
    }
    if let Some(append) = discover_append_system_prompt(config, cwd)
        .map(|prompt| prompt.trim().to_string())
        .filter(|prompt| !prompt.is_empty())
    {
        sections.push(section("addendum", &append));
    }
    if let Some(context) = project_context_section(&project_context_files(config, cwd)) {
        sections.push(section("project_context", &context));
    }
    if let Some(skills) = skills_section(config, cwd, active_tools) {
        sections.push(section("skills", &skills));
    }
    sections.push(section("cwd", &cwd.to_string_lossy().replace('\\', "/")));
    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_follow_pi_order_and_include_cwd() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config::new(Some(temp.path().join("agent")), None).unwrap();
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let prompt = build_system_prompt(&config, &cwd, &["read".to_string(), "bash".to_string()]);
        assert!(prompt.starts_with(DEFAULT_PREAMBLE));
        assert!(prompt.contains("<tools>"));
        assert!(prompt.contains("- read: Read file contents"));
        assert!(prompt.contains("<rules>"));
        assert!(prompt.contains("- Use read to examine files instead of cat or sed."));
        assert!(prompt.contains("<cwd>"));
        assert!(prompt.ends_with("</cwd>"));
        assert!(prompt.find("<tools>").unwrap() < prompt.find("<rules>").unwrap());
        assert!(prompt.find("<rules>").unwrap() < prompt.find("<cwd>").unwrap());
    }

    #[test]
    fn system_and_append_files_are_custom_and_addendum() {
        let temp = tempfile::tempdir().unwrap();
        let agent = temp.path().join("agent");
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(agent.join("SYSTEM.md"), "CUSTOM PROMPT\n").unwrap();
        std::fs::write(agent.join("APPEND_SYSTEM.md"), "APPENDED\n").unwrap();
        std::fs::write(cwd.join("AGENTS.md"), "project notes\n").unwrap();
        let config = Config::new(Some(agent), None).unwrap();
        let prompt = build_system_prompt(&config, &cwd, &["read".to_string()]);
        assert!(prompt.starts_with("CUSTOM PROMPT"));
        assert!(!prompt.contains("<tools>"));
        assert!(prompt.contains("<addendum>\nAPPENDED\n</addendum>"));
        assert!(prompt.contains("<project_context>"));
        assert!(prompt.contains("project notes"));
    }
}
