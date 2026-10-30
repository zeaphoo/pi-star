use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct ParsedSource {
    pub original: String,
    pub kind: SourceKind,
}

#[derive(Debug, Clone)]
pub enum SourceKind {
    Npm(String),
    Git {
        url: String,
        host: String,
        path: String,
    },
    Local(PathBuf),
}

pub fn parse_source(source: &str) -> Result<ParsedSource> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("source is required"));
    }
    if let Some(value) = trimmed.strip_prefix("npm:") {
        return Ok(ParsedSource {
            original: format!("npm:{value}"),
            kind: SourceKind::Npm(value.to_string()),
        });
    }
    if let Some(value) = trimmed.strip_prefix("git:") {
        let url = if let Some(rest) = value.strip_prefix("github:") {
            format!("https://github.com/{rest}")
        } else {
            value.to_string()
        };
        let (host, path) = git_host_path(&url)?;
        return Ok(ParsedSource {
            original: format!("git:{value}"),
            kind: SourceKind::Git { url, host, path },
        });
    }
    if trimmed.starts_with("https://")
        || trimmed.starts_with("http://")
        || trimmed.starts_with("git@")
    {
        let (host, path) = git_host_path(trimmed)?;
        return Ok(ParsedSource {
            original: trimmed.to_string(),
            kind: SourceKind::Git {
                url: trimmed.to_string(),
                host,
                path,
            },
        });
    }
    if trimmed.starts_with('/') || trimmed.starts_with("./") || trimmed.starts_with("~/") {
        let path = if let Some(rest) = trimmed.strip_prefix("~/") {
            pi_star_core::settings::home_dir()
                .unwrap_or_default()
                .join(rest)
        } else {
            PathBuf::from(trimmed)
        };
        return Ok(ParsedSource {
            original: trimmed.to_string(),
            kind: SourceKind::Local(path),
        });
    }
    Ok(ParsedSource {
        original: format!("npm:{trimmed}"),
        kind: SourceKind::Npm(trimmed.to_string()),
    })
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

pub fn install_path(
    state: &AppState,
    source: &str,
    scope: &str,
    cwd: Option<&Path>,
) -> Option<PathBuf> {
    let parsed = parse_source(source).ok()?;
    let root = root_for(state, scope, cwd)?;
    match parsed.kind {
        SourceKind::Npm(name) => Some(root.join("npm").join("node_modules").join(name)),
        SourceKind::Git { host, path, .. } => Some(root.join("git").join(host).join(path)),
        SourceKind::Local(path) => Some(root.join("git").join("local").join(path.file_name()?)),
    }
}

fn root_for(state: &AppState, scope: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    if scope == "project" {
        Some(cwd?.to_path_buf())
    } else {
        Some(state.config.agent_dir.clone())
    }
}

pub async fn install(
    state: &AppState,
    source: &str,
    scope: &str,
    cwd: Option<&Path>,
) -> Result<PathBuf> {
    let parsed = parse_source(source)?;
    let reference = serde_json::to_string(&json!({"source": parsed.original, "scope": scope, "cwd": cwd.map(|p| p.to_string_lossy())})).unwrap_or_default();
    if reference.is_empty() {
        return Err(anyhow!("could not record the package source"));
    }
    let target = install_path(state, source, scope, cwd)
        .ok_or_else(|| anyhow!("cwd is required for a project package"))?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match &parsed.kind {
        SourceKind::Npm(name) => {
            let root = target
                .parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .ok_or_else(|| anyhow!("invalid npm install path"))?
                .to_path_buf();
            ensure_npm_project(&root)?;
            run("npm", &["install", "--no-audit", "--no-fund", name], None).await?;
            // The package name may include a scope; npm still installs it under
            // <root>/node_modules/<name>, which is what target already names.
        }
        SourceKind::Git { url, .. } => {
            if target.exists() {
                std::fs::remove_dir_all(&target)?;
            }
            let output = tokio::process::Command::new("git")
                .args(["clone", "--depth", "1", url])
                .arg(&target)
                .env("GIT_TERMINAL_PROMPT", "0")
                .output()
                .await?;
            if !output.status.success() {
                return Err(anyhow!(
                    "{}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            if target.join("package.json").is_file() {
                let _ = run(
                    "npm",
                    &["install", "--omit=dev", "--no-audit", "--no-fund"],
                    Some(&target),
                )
                .await;
            }
        }
        SourceKind::Local(path) => {
            if !path.exists() {
                return Err(anyhow!("local package path does not exist"));
            }
            if target.exists() {
                std::fs::remove_dir_all(&target)?;
            }
            copy_dir(path, &target)?;
            if target.join("package.json").is_file() {
                let _ = run(
                    "npm",
                    &["install", "--omit=dev", "--no-audit", "--no-fund"],
                    Some(&target),
                )
                .await;
            }
        }
    }
    add_setting_package(state, &parsed.original, scope, cwd)?;
    Ok(target)
}

pub async fn update(
    state: &AppState,
    source: &str,
    scope: &str,
    cwd: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(target) = install_path(state, source, scope, cwd) {
        if target.exists() {
            let _ = std::fs::remove_dir_all(&target);
        }
    }
    install(state, source, scope, cwd).await
}

pub fn remove(state: &AppState, source: &str, scope: &str, cwd: Option<&Path>) -> Result<()> {
    if let Some(target) = install_path(state, source, scope, cwd) {
        if target.exists() {
            std::fs::remove_dir_all(&target)?;
        }
    }
    remove_setting_package(state, source, scope, cwd)
}

fn ensure_npm_project(root: &Path) -> Result<()> {
    std::fs::create_dir_all(root)?;
    let package_json = root.join("package.json");
    if !package_json.exists() {
        std::fs::write(
            &package_json,
            "{\n  \"name\": \"pi-extensions\",\n  \"private\": true\n}\n",
        )?;
    }
    Ok(())
}

async fn run(program: &str, args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let mut command = tokio::process::Command::new(program);
    command.args(args).env("GIT_TERMINAL_PROMPT", "0");
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output().await?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn copy_dir(source: &Path, target: &Path) -> Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let destination = target.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &destination)?;
        } else if path.is_file() {
            std::fs::copy(&path, &destination)?;
        }
    }
    Ok(())
}

fn settings_path(state: &AppState, scope: &str, cwd: Option<&Path>) -> Result<PathBuf> {
    if scope == "project" {
        Ok(cwd
            .ok_or_else(|| anyhow!("cwd is required for a project package"))?
            .join(".pi")
            .join("settings.json"))
    } else {
        Ok(state.config.settings_path())
    }
}

fn add_setting_package(
    state: &AppState,
    source: &str,
    scope: &str,
    cwd: Option<&Path>,
) -> Result<()> {
    let path = settings_path(state, scope, cwd)?;
    let mut settings = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_else(|| json!({}));
    let packages = settings
        .as_object_mut()
        .ok_or_else(|| anyhow!("settings.json is not an object"))?
        .entry("packages".to_string())
        .or_insert_with(|| json!([]));
    let packages = packages
        .as_array_mut()
        .ok_or_else(|| anyhow!("packages is not an array"))?;
    if !packages
        .iter()
        .any(|entry| package_source(entry) == Some(source))
    {
        packages.push(Value::String(source.to_string()));
    }
    pi_star_core::settings::write_json_atomic(&path, &settings, 0o600)
}

fn remove_setting_package(
    state: &AppState,
    source: &str,
    scope: &str,
    cwd: Option<&Path>,
) -> Result<()> {
    let path = settings_path(state, scope, cwd)?;
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let Ok(mut settings) = serde_json::from_str::<Value>(&text) else {
        return Ok(());
    };
    if let Some(packages) = settings.get_mut("packages").and_then(Value::as_array_mut) {
        packages.retain(|entry| package_source(entry) != Some(source));
    }
    pi_star_core::settings::write_json_atomic(&path, &settings, 0o600)
}

fn package_source(entry: &Value) -> Option<&str> {
    entry
        .as_str()
        .or_else(|| entry.get("source").and_then(Value::as_str))
}
