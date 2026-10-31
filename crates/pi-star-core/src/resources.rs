//! Resource discovery shared by the settings panels and the QuickPI prompt.
//!
//! Mirrors the parts of pi's `DefaultPackageManager` / `DefaultResourceLoader`
//! that decide which extensions, skills, prompt templates and themes a session
//! sees: configured packages, settings entries, auto-discovered directories and
//! `.agents/skills` ancestors. The UI needs the same lists pi-web gets from the
//! TypeScript SDK, including source metadata for the settings badges.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use globset::Glob;
use serde_json::{Value, json};

use crate::config::Config;
use crate::settings;

pub const CONFIG_DIR_NAME: &str = ".pi";
const RESOURCE_TYPES: [&str; 4] = ["extensions", "skills", "prompts", "themes"];

#[derive(Debug, Clone)]
pub struct ParsedPackageSource {
    pub original: String,
    pub kind: PackageSourceKind,
}

#[derive(Debug, Clone)]
pub enum PackageSourceKind {
    Npm {
        name: String,
        spec: Option<String>,
    },
    Git {
        url: String,
        host: String,
        path: String,
    },
    Local(PathBuf),
}

#[derive(Debug, Clone)]
pub struct ResourceMetadata {
    pub source: String,
    pub scope: String,
    pub origin: String,
    pub base_dir: PathBuf,
    pub package_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ResourcePath {
    pub path: PathBuf,
    pub metadata: ResourceMetadata,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedResources {
    pub extensions: Vec<ResourcePath>,
    pub skills: Vec<ResourcePath>,
    pub prompts: Vec<ResourcePath>,
    pub themes: Vec<ResourcePath>,
    pub diagnostics: Vec<Value>,
    pub project_resources_loaded: bool,
}

#[derive(Debug, Clone)]
pub struct PackageEntry {
    pub source: String,
    pub scope: String,
    pub filter: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub source: String,
    pub scope: String,
    pub filter: Option<Value>,
    pub installed_path: Option<PathBuf>,
    pub legacy_path: Option<PathBuf>,
}

fn is_pattern(value: &str) -> bool {
    value.starts_with('!')
        || value.starts_with('+')
        || value.starts_with('-')
        || value.contains('*')
        || value.contains('?')
}

fn glob_match(pattern: &str, candidate: &str) -> bool {
    let pattern = pattern.trim_start_matches("./");
    let candidate = candidate.replace('\\', "/");
    let pattern = pattern.replace('\\', "/");
    Glob::new(&pattern)
        .map(|glob| glob.compile_matcher().is_match(&candidate))
        .unwrap_or(false)
}

fn matches_any_pattern(path: &Path, patterns: &[String], base: &Path) -> bool {
    let rel = path
        .strip_prefix(base)
        .map(|value| value.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let path_posix = path.to_string_lossy().replace('\\', "/");
    let parent_rel = path
        .parent()
        .and_then(|parent| parent.strip_prefix(base).ok())
        .map(|value| value.to_string_lossy().replace('\\', "/"));
    let parent_name = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str());
    patterns.iter().any(|pattern| {
        glob_match(pattern, &rel)
            || glob_match(pattern, name)
            || glob_match(pattern, &path_posix)
            || parent_rel
                .as_deref()
                .is_some_and(|parent| glob_match(pattern, parent))
            || parent_name.is_some_and(|parent| glob_match(pattern, parent))
    })
}

fn matches_exact_pattern(path: &Path, patterns: &[String], base: &Path) -> bool {
    if patterns.is_empty() {
        return false;
    }
    let rel = path
        .strip_prefix(base)
        .map(|value| value.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));
    let path_posix = path.to_string_lossy().replace('\\', "/");
    let parent_rel = path
        .parent()
        .and_then(|parent| parent.strip_prefix(base).ok())
        .map(|value| value.to_string_lossy().replace('\\', "/"));
    patterns.iter().any(|pattern| {
        let normalized = pattern.trim_start_matches("./").replace('\\', "/");
        normalized == rel
            || normalized == path_posix
            || parent_rel.as_deref() == Some(normalized.as_str())
    })
}

fn apply_patterns(all: &[PathBuf], patterns: &[String], base: &Path) -> HashSet<PathBuf> {
    let mut includes = Vec::new();
    let mut excludes = Vec::new();
    let mut force_includes = Vec::new();
    let mut force_excludes = Vec::new();
    for pattern in patterns {
        if let Some(value) = pattern.strip_prefix('+') {
            force_includes.push(value.to_string());
        } else if let Some(value) = pattern.strip_prefix('-') {
            force_excludes.push(value.to_string());
        } else if let Some(value) = pattern.strip_prefix('!') {
            excludes.push(value.to_string());
        } else {
            includes.push(pattern.clone());
        }
    }
    let mut result: Vec<PathBuf> = if includes.is_empty() {
        all.to_vec()
    } else {
        all.iter()
            .filter(|path| matches_any_pattern(path, &includes, base))
            .cloned()
            .collect()
    };
    if !excludes.is_empty() {
        result.retain(|path| !matches_any_pattern(path, &excludes, base));
    }
    if !force_includes.is_empty() {
        for path in all {
            if !result.contains(path) && matches_exact_pattern(path, &force_includes, base) {
                result.push(path.clone());
            }
        }
    }
    if !force_excludes.is_empty() {
        result.retain(|path| !matches_exact_pattern(path, &force_excludes, base));
    }
    result.into_iter().collect()
}

fn enabled_by_overrides(path: &Path, patterns: &[String], base: &Path) -> bool {
    let excludes: Vec<String> = patterns
        .iter()
        .filter_map(|pattern| pattern.strip_prefix('!').map(str::to_owned))
        .collect();
    let force_includes: Vec<String> = patterns
        .iter()
        .filter_map(|pattern| pattern.strip_prefix('+').map(str::to_owned))
        .collect();
    let force_excludes: Vec<String> = patterns
        .iter()
        .filter_map(|pattern| pattern.strip_prefix('-').map(str::to_owned))
        .collect();
    let mut enabled = !(!excludes.is_empty() && matches_any_pattern(path, &excludes, base));
    if !force_includes.is_empty() && matches_exact_pattern(path, &force_includes, base) {
        enabled = true;
    }
    if !force_excludes.is_empty() && matches_exact_pattern(path, &force_excludes, base) {
        enabled = false;
    }
    enabled
}

fn canonical(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

fn precedence(metadata: &ResourceMetadata) -> u8 {
    if metadata.origin == "package" {
        return 4;
    }
    let base = if metadata.scope == "project" { 0 } else { 2 };
    base + u8::from(metadata.source != "local")
}

fn sort_and_dedupe(mut resources: Vec<ResourcePath>) -> Vec<ResourcePath> {
    resources.sort_by_key(|resource| precedence(&resource.metadata));
    let mut seen = HashSet::new();
    resources.retain(|resource| seen.insert(canonical(&resource.path)));
    resources
}

pub fn parse_package_source(source: &str) -> Result<ParsedPackageSource> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("source is required"));
    }
    if let Some(value) = trimmed.strip_prefix("npm:") {
        let (name, spec) = split_npm_spec(value);
        return Ok(ParsedPackageSource {
            original: format!("npm:{value}"),
            kind: PackageSourceKind::Npm { name, spec },
        });
    }
    if let Some(value) = trimmed.strip_prefix("git:") {
        let url = if let Some(rest) = value.strip_prefix("github:") {
            format!("https://github.com/{rest}")
        } else {
            value.to_string()
        };
        let (host, path) = git_host_path(&url)?;
        return Ok(ParsedPackageSource {
            original: format!("git:{value}"),
            kind: PackageSourceKind::Git { url, host, path },
        });
    }
    if trimmed.starts_with("https://")
        || trimmed.starts_with("http://")
        || trimmed.starts_with("git@")
    {
        let (host, path) = git_host_path(trimmed)?;
        return Ok(ParsedPackageSource {
            original: trimmed.to_string(),
            kind: PackageSourceKind::Git {
                url: trimmed.to_string(),
                host,
                path,
            },
        });
    }
    if trimmed.starts_with('/')
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.starts_with("~/")
    {
        let path = if let Some(rest) = trimmed.strip_prefix("~/") {
            settings::home_dir().unwrap_or_default().join(rest)
        } else {
            PathBuf::from(trimmed)
        };
        return Ok(ParsedPackageSource {
            original: trimmed.to_string(),
            kind: PackageSourceKind::Local(path),
        });
    }
    let (name, spec) = split_npm_spec(trimmed);
    Ok(ParsedPackageSource {
        original: format!("npm:{trimmed}"),
        kind: PackageSourceKind::Npm { name, spec },
    })
}

fn split_npm_spec(value: &str) -> (String, Option<String>) {
    let value = value.trim();
    if value.is_empty() {
        return (String::new(), None);
    }
    let name_end = if value.starts_with('@') {
        value.find('/').map(|slash| {
            value[slash + 1..]
                .find('@')
                .map(|at| slash + 1 + at)
                .unwrap_or(value.len())
        })
    } else {
        value.find('@').filter(|index| *index > 0)
    }
    .unwrap_or(value.len());
    let name = value[..name_end].to_string();
    let spec = value
        .get(name_end..)
        .and_then(|rest| rest.strip_prefix('@'))
        .filter(|spec| !spec.is_empty())
        .map(str::to_owned);
    (name, spec)
}

fn git_host_path(url: &str) -> Result<(String, String)> {
    let normalized = url
        .trim_start_matches("git+")
        .trim_end_matches(".git")
        .trim_end_matches('/');
    if let Some(rest) = normalized.strip_prefix("git@") {
        let (host, path) = rest
            .split_once(':')
            .ok_or_else(|| anyhow!("invalid git source"))?;
        return Ok((host.to_string(), path.to_string()));
    }
    let parsed = reqwest::Url::parse(normalized).map_err(|_| anyhow!("invalid git source"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("invalid git source"))?
        .to_string();
    let path = parsed.path().trim_start_matches('/').to_string();
    if path.is_empty() {
        return Err(anyhow!("invalid git source"));
    }
    Ok((host, path))
}

fn base_dir_for(config: &Config, cwd: &Path, scope: &str) -> PathBuf {
    match scope {
        "project" => cwd.join(CONFIG_DIR_NAME),
        _ => config.agent_dir.clone(),
    }
}

pub fn install_path(config: &Config, cwd: &Path, scope: &str, source: &str) -> Option<PathBuf> {
    let parsed = parse_package_source(source).ok()?;
    let base = base_dir_for(config, cwd, scope);
    match parsed.kind {
        PackageSourceKind::Npm { name, .. } => {
            Some(base.join("npm").join("node_modules").join(name))
        }
        PackageSourceKind::Git { host, path, .. } => Some(base.join("git").join(host).join(path)),
        PackageSourceKind::Local(path) => Some(if path.is_absolute() {
            path
        } else {
            base.join(path)
        }),
    }
}

fn legacy_install_path(config: &Config, cwd: &Path, scope: &str, source: &str) -> Option<PathBuf> {
    let parsed = parse_package_source(source).ok()?;
    // Older pi-star builds installed both npm and git packages directly under
    // the project root, and copied local packages into `git/local/<name>`.
    let base = if scope == "project" {
        cwd.to_path_buf()
    } else {
        config.agent_dir.clone()
    };
    match parsed.kind {
        PackageSourceKind::Npm { name, .. } => {
            let path = base.join("npm").join("node_modules").join(name);
            path.exists().then_some(path)
        }
        PackageSourceKind::Git { host, path, .. } => {
            let path = base.join("git").join(host).join(path);
            path.exists().then_some(path)
        }
        PackageSourceKind::Local(path) => {
            let name = path.file_name()?;
            let copy = base.join("git").join("local").join(name);
            copy.exists().then_some(copy)
        }
    }
}

pub fn configured_packages(config: &Config, cwd: &Path) -> Vec<PackageEntry> {
    let mut packages = Vec::new();
    if settings::project_trust(config, cwd).trusted {
        let project = read_json_file(&cwd.join(CONFIG_DIR_NAME).join("settings.json"));
        append_package_entries(&mut packages, project.get("packages"), "project");
    }
    let global = settings::read_settings(config);
    append_package_entries(&mut packages, global.get("packages"), "user");
    packages
}

fn append_package_entries(packages: &mut Vec<PackageEntry>, value: Option<&Value>, scope: &str) {
    let Some(entries) = value.and_then(Value::as_array) else {
        return;
    };
    for entry in entries {
        match entry {
            Value::String(source) => packages.push(PackageEntry {
                source: source.clone(),
                scope: scope.to_string(),
                filter: None,
            }),
            Value::Object(object) => {
                let Some(source) = object.get("source").and_then(Value::as_str) else {
                    continue;
                };
                packages.push(PackageEntry {
                    source: source.to_string(),
                    scope: scope.to_string(),
                    filter: Some(entry.clone()),
                });
            }
            _ => {}
        }
    }
}

pub fn installed_packages(config: &Config, cwd: &Path) -> Vec<InstalledPackage> {
    configured_packages(config, cwd)
        .into_iter()
        .map(|entry| {
            let primary = install_path(config, cwd, &entry.scope, &entry.source);
            let legacy = legacy_install_path(config, cwd, &entry.scope, &entry.source);
            let installed_path = primary
                .as_ref()
                .filter(|path| path.exists())
                .cloned()
                .or_else(|| legacy.clone())
                .or(primary.clone());
            InstalledPackage {
                source: entry.source,
                scope: entry.scope,
                filter: entry.filter,
                installed_path,
                legacy_path: legacy,
            }
        })
        .collect()
}

fn read_json_file(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

fn is_project_trusted(config: &Config, cwd: &Path) -> bool {
    settings::project_trust(config, cwd).trusted
}

fn matches_kind(path: &Path, kind: &str) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match kind {
        "extensions" => extension == "ts" || extension == "js",
        "skills" | "prompts" => extension == "md",
        "themes" => extension == "json",
        _ => false,
    }
}

fn walk_files(root: &Path, kind: &str, out: &mut Vec<PathBuf>) {
    if !root.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let file_type = entry.file_type().ok();
        let is_dir = file_type.is_some_and(|kind| kind.is_dir())
            || (file_type.is_some_and(|kind| kind.is_symlink()) && path.is_dir());
        let is_file = file_type.is_some_and(|kind| kind.is_file())
            || (file_type.is_some_and(|kind| kind.is_symlink()) && path.is_file());
        if is_dir {
            walk_files(&path, kind, out);
        } else if is_file && matches_kind(&path, kind) {
            out.push(path);
        }
    }
}

/// Collect skills the way pi does: a directory holding SKILL.md is a skill and
/// is not descended into; otherwise recurse, and direct-root `.md` files are
/// skills too.
fn walk_skills(root: &Path, include_root_md: bool, out: &mut Vec<PathBuf>) {
    if !root.is_dir() {
        return;
    }
    let skill = root.join("SKILL.md");
    if skill.is_file() {
        out.push(skill);
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let is_dir = path.is_dir();
        let is_file = path.is_file();
        if is_dir {
            walk_skills(&path, false, out);
            continue;
        }
        if is_file && include_root_md && name.ends_with(".md") {
            out.push(path);
        }
    }
}

fn collect_from_paths(paths: &[PathBuf], kind: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_file() && matches_kind(path, kind) {
            files.push(path.clone());
        } else if path.is_dir() {
            if kind == "skills" {
                walk_skills(path, true, &mut files);
            } else if kind == "extensions" {
                collect_extension_entries(path, &mut files);
            } else {
                walk_files(path, kind, &mut files);
            }
        }
    }
    files
}

fn collect_extension_entries(dir: &Path, out: &mut Vec<PathBuf>) {
    // A package.json `pi.extensions` manifest or an index entry turns a
    // directory into one extension instead of scanning its children.
    let manifest = read_json_file(&dir.join("package.json"));
    let declared = manifest
        .get("pi")
        .and_then(|pi| pi.get("extensions"))
        .and_then(Value::as_array);
    if let Some(entries) = declared {
        for entry in entries.iter().filter_map(Value::as_str) {
            let path = dir.join(entry);
            if path.is_file() {
                out.push(path);
            }
        }
        if !out.is_empty() {
            return;
        }
    }
    for name in ["index.ts", "index.js"] {
        let path = dir.join(name);
        if path.is_file() {
            out.push(path);
            return;
        }
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_extension_entries(&path, out);
        } else if path.is_file() && matches_kind(&path, "extensions") {
            out.push(path);
        }
    }
}

fn package_files(package_root: &Path, kind: &str) -> Vec<PathBuf> {
    let manifest = read_json_file(&package_root.join("package.json"));
    let declared = manifest
        .get("pi")
        .and_then(|pi| pi.get(kind))
        .and_then(Value::as_array);
    if let Some(entries) = declared {
        let paths: Vec<PathBuf> = entries
            .iter()
            .filter_map(Value::as_str)
            .map(|entry| package_root.join(entry))
            .collect();
        return collect_from_paths(&paths, kind);
    }
    let dir = package_root.join(kind);
    if dir.is_dir() {
        return collect_from_paths(&[dir], kind);
    }
    Vec::new()
}

fn add_resource(
    list: &mut Vec<ResourcePath>,
    path: PathBuf,
    metadata: &ResourceMetadata,
    enabled: bool,
) {
    if path.as_os_str().is_empty() {
        return;
    }
    list.push(ResourcePath {
        path,
        metadata: metadata.clone(),
        enabled,
    });
}

fn local_entries(
    config: &Config,
    cwd: &Path,
    scope: &str,
    list: &mut Vec<ResourcePath>,
    kind: &str,
    value: Option<&Value>,
) {
    let Some(entries) = value.and_then(Value::as_array) else {
        return;
    };
    let base = base_dir_for(config, cwd, scope);
    let plain: Vec<String> = entries
        .iter()
        .filter_map(Value::as_str)
        .filter(|entry| !is_pattern(entry))
        .map(str::to_owned)
        .collect();
    let patterns: Vec<String> = entries
        .iter()
        .filter_map(Value::as_str)
        .filter(|entry| is_pattern(entry))
        .map(str::to_owned)
        .collect();
    let resolved: Vec<PathBuf> = plain
        .iter()
        .map(|entry| {
            let path = PathBuf::from(entry);
            if path.is_absolute() {
                path
            } else {
                base.join(path)
            }
        })
        .collect();
    let files = collect_from_paths(&resolved, kind);
    let enabled = apply_patterns(&files, &patterns, &base);
    for path in files {
        let metadata = ResourceMetadata {
            source: "local".to_string(),
            scope: scope.to_string(),
            origin: "top-level".to_string(),
            base_dir: base.clone(),
            package_root: None,
        };
        add_resource(list, path.clone(), &metadata, enabled.contains(&path));
    }
}

fn add_package_resources(
    list: &mut Vec<ResourcePath>,
    package_root: &Path,
    package: &InstalledPackage,
    kind: &str,
    diagnostics: &mut Vec<Value>,
) {
    let mut files = package_files(package_root, kind);
    let metadata = ResourceMetadata {
        source: package.source.clone(),
        scope: package.scope.clone(),
        origin: "package".to_string(),
        base_dir: package_root.to_path_buf(),
        package_root: Some(package_root.to_path_buf()),
    };
    if let Some(filter) = &package.filter {
        if let Some(patterns) = filter.get(kind).and_then(Value::as_array) {
            let patterns: Vec<String> = patterns
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            if patterns.is_empty() {
                // An empty array explicitly disables this resource type.
                return;
            }
            let enabled = apply_patterns(&files, &patterns, package_root);
            for path in files {
                add_resource(list, path.clone(), &metadata, enabled.contains(&path));
            }
            return;
        }
        if filter.get("autoload").and_then(Value::as_bool) == Some(false) {
            // The user-level entry supplies the resources; this entry only
            // changes how they autoload. Nothing to add here.
            return;
        }
    }
    if files.is_empty()
        && kind == "extensions"
        && package_root.is_dir()
        && matches!(
            parse_package_source(&package.source).map(|parsed| parsed.kind),
            Ok(PackageSourceKind::Local(_))
        )
    {
        // A local directory with no resource folders is itself an extension,
        // matching pi's local-source fallback.
        add_resource(list, package_root.to_path_buf(), &metadata, true);
        return;
    }
    files.retain(|path| path.exists());
    for path in files {
        add_resource(list, path, &metadata, true);
    }
    if package.installed_path.is_none() {
        diagnostics.push(json!({
            "type": "warning",
            "source": package.source,
            "message": "Configured package path was not found.",
        }));
    }
}

fn auto_resources(
    config: &Config,
    cwd: &Path,
    kind: &str,
    list: &mut Vec<ResourcePath>,
    trusted: bool,
) {
    let global_settings = settings::read_settings(config);
    let project_settings = if trusted {
        read_json_file(&cwd.join(CONFIG_DIR_NAME).join("settings.json"))
    } else {
        Value::Null
    };
    let global_overrides: Vec<String> = global_settings
        .get(kind)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let project_overrides: Vec<String> = project_settings
        .get(kind)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();

    let mut add_auto = |dir: &Path, scope: &str, base: &Path, overrides: &[String]| {
        if !dir.is_dir() {
            return;
        }
        let mut files = if kind == "skills" {
            let mut found = Vec::new();
            walk_skills(dir, true, &mut found);
            found
        } else if kind == "extensions" {
            let mut found = Vec::new();
            collect_extension_entries(dir, &mut found);
            found
        } else {
            let mut found = Vec::new();
            walk_files(dir, kind, &mut found);
            found
        };
        files.sort();
        for path in files {
            let metadata = ResourceMetadata {
                source: "auto".to_string(),
                scope: scope.to_string(),
                origin: "top-level".to_string(),
                base_dir: base.to_path_buf(),
                package_root: None,
            };
            let enabled = enabled_by_overrides(&path, overrides, base);
            add_resource(list, path, &metadata, enabled);
        }
    };

    let agent_dir = config.agent_dir.clone();
    let project_pi = cwd.join(CONFIG_DIR_NAME);
    if trusted {
        add_auto(
            &project_pi.join(kind),
            "project",
            &project_pi,
            &project_overrides,
        );
    }
    if kind == "skills" {
        add_auto(
            &settings::home_dir()
                .unwrap_or_default()
                .join(".agents")
                .join("skills"),
            "user",
            &settings::home_dir().unwrap_or_default().join(".agents"),
            &global_overrides,
        );
        if trusted {
            for dir in ancestor_agents_skills(cwd) {
                add_auto(
                    &dir,
                    "project",
                    dir.parent().unwrap_or(&dir),
                    &project_overrides,
                );
            }
        }
    }
    add_auto(&agent_dir.join(kind), "user", &agent_dir, &global_overrides);
}

fn ancestor_agents_skills(cwd: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let root = git_repo_root(cwd);
    let mut current = cwd.to_path_buf();
    loop {
        dirs.push(current.join(".agents").join("skills"));
        if root.as_ref().is_some_and(|root| current == *root) {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent.to_path_buf();
    }
    dirs
}

fn git_repo_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        let parent = current.parent()?;
        if parent == current {
            return None;
        }
        current = parent.to_path_buf();
    }
}

pub fn resolve_resources(config: &Config, cwd: &Path) -> ResolvedResources {
    let trusted = is_project_trusted(config, cwd);
    let mut resolved = ResolvedResources {
        project_resources_loaded: trusted,
        ..Default::default()
    };
    let packages = installed_packages(config, cwd);
    for package in &packages {
        let Some(root) = package
            .installed_path
            .as_ref()
            .or(package.legacy_path.as_ref())
            .filter(|path| path.exists())
        else {
            resolved.diagnostics.push(json!({
                "type": "warning",
                "source": package.source,
                "message": "Package is configured but not installed yet.",
            }));
            continue;
        };
        for kind in RESOURCE_TYPES {
            let target = match kind {
                "extensions" => &mut resolved.extensions,
                "skills" => &mut resolved.skills,
                "prompts" => &mut resolved.prompts,
                _ => &mut resolved.themes,
            };
            add_package_resources(target, root, package, kind, &mut resolved.diagnostics);
        }
    }

    let global = settings::read_settings(config);
    let project = if trusted {
        read_json_file(&cwd.join(CONFIG_DIR_NAME).join("settings.json"))
    } else {
        Value::Null
    };
    for kind in RESOURCE_TYPES {
        let target = match kind {
            "extensions" => &mut resolved.extensions,
            "skills" => &mut resolved.skills,
            "prompts" => &mut resolved.prompts,
            _ => &mut resolved.themes,
        };
        if trusted {
            local_entries(config, cwd, "project", target, kind, project.get(kind));
        }
        local_entries(config, cwd, "user", target, kind, global.get(kind));
        auto_resources(config, cwd, kind, target, trusted);
    }

    resolved.extensions = sort_and_dedupe(resolved.extensions);
    resolved.skills = sort_and_dedupe(resolved.skills);
    resolved.prompts = sort_and_dedupe(resolved.prompts);
    resolved.themes = sort_and_dedupe(resolved.themes);
    resolved
}

#[derive(Debug, Clone)]
pub struct LoadedSkill {
    pub name: String,
    pub description: String,
    pub file_path: PathBuf,
    pub base_dir: PathBuf,
    pub disable_model_invocation: bool,
    pub source_info: Value,
}

pub fn load_skills(config: &Config, cwd: &Path) -> (Vec<LoadedSkill>, Vec<Value>, bool) {
    let resolved = resolve_resources(config, cwd);
    let mut skills = Vec::new();
    // Package-level warnings belong to the Plugins panel, not to the skill
    // loader's diagnostics (which pi reports separately).
    let mut diagnostics = Vec::new();
    let mut seen_names: HashMap<String, PathBuf> = HashMap::new();
    let mut seen_paths = HashSet::new();
    for resource in resolved.skills.iter().filter(|resource| resource.enabled) {
        let path = &resource.path;
        if !path.is_file() {
            continue;
        }
        let real = canonical(path);
        if !seen_paths.insert(real.clone()) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            diagnostics.push(json!({
                "type": "warning",
                "message": "failed to read skill file",
                "path": path.to_string_lossy(),
            }));
            continue;
        };
        let parsed = pi_star_harness::frontmatter::parse_frontmatter(&content);
        let frontmatter = match parsed {
            Ok((frontmatter, _)) => frontmatter,
            Err(error) => {
                if path.file_name().and_then(|value| value.to_str()) == Some("SKILL.md") {
                    diagnostics.push(json!({
                        "type": "warning",
                        "message": error,
                        "path": path.to_string_lossy(),
                    }));
                }
                continue;
            }
        };
        let description = frontmatter
            .get("description")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        let declared = path.file_name().and_then(|value| value.to_str()) == Some("SKILL.md");
        if !declared && description.is_empty() {
            continue;
        }
        if description.is_empty() {
            diagnostics.push(json!({
                "type": "warning",
                "message": "description is required",
                "path": path.to_string_lossy(),
            }));
            continue;
        }
        let parent_name = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .unwrap_or("skill");
        let name = frontmatter
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(parent_name)
            .to_string();
        if let Some(existing) = seen_names.get(&name) {
            diagnostics.push(json!({
                "type": "collision",
                "message": format!("name \"{name}\" collision"),
                "path": path.to_string_lossy(),
                "collision": {
                    "resourceType": "skill",
                    "name": name,
                    "winnerPath": existing.to_string_lossy(),
                    "loserPath": path.to_string_lossy(),
                },
            }));
            continue;
        }
        seen_names.insert(name.clone(), path.clone());
        let metadata = &resource.metadata;
        let source_info = json!({
            "path": path.to_string_lossy(),
            "source": metadata.source,
            "scope": metadata.scope,
            "origin": metadata.origin,
            "baseDir": metadata.base_dir.to_string_lossy(),
        });
        skills.push(LoadedSkill {
            name,
            description,
            file_path: path.clone(),
            base_dir: path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| path.clone()),
            disable_model_invocation: frontmatter
                .get("disable-model-invocation")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            source_info,
        });
    }
    (skills, diagnostics, resolved.project_resources_loaded)
}

/// What pi's resource loader calls `sourceInfo` for one resource path.
pub fn source_info_for(resource: &ResourcePath) -> Value {
    json!({
        "path": resource.path.to_string_lossy(),
        "source": resource.metadata.source,
        "scope": resource.metadata.scope,
        "origin": resource.metadata.origin,
        "baseDir": resource.metadata.base_dir.to_string_lossy(),
    })
}
