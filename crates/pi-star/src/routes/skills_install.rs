use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::routes::{bad_request, forbidden, internal, no_store};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct InstallBody {
    package: Option<String>,
    scope: Option<String>,
    cwd: Option<String>,
}

pub async fn install(
    State(state): State<Arc<AppState>>,
    Json(body): Json<InstallBody>,
) -> Response {
    let Some(package) = body.package.filter(|package| !package.trim().is_empty()) else {
        return bad_request("package required");
    };
    let scope = body.scope.unwrap_or_else(|| "global".to_string());
    let cwd = body.cwd.map(PathBuf::from);
    if scope == "project" && cwd.is_none() {
        return bad_request("cwd required for project install");
    }
    if let Some(cwd) = &cwd {
        if !state.roots.is_allowed_existing(cwd) {
            return forbidden("Access denied");
        }
        if scope == "project" && !pi_star_core::settings::project_trust(&state.config, cwd).trusted
        {
            return forbidden("Project resources must be trusted before installing project skills");
        }
    }
    match install_skill(&state, &package, &scope, cwd.as_deref()) {
        Ok(skill) => no_store(json!({"success": true, "skill": skill})),
        Err(error) => internal(error),
    }
}

pub async fn update(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let (Some(package), Some(cwd), Some(scope)) = (
        body.get("package").and_then(Value::as_str),
        body.get("cwd").and_then(Value::as_str),
        body.get("scope").and_then(Value::as_str),
    ) else {
        return bad_request("cwd, package, and scope are required");
    };
    let cwd = PathBuf::from(cwd);
    if !state.roots.is_allowed_existing(&cwd) {
        return forbidden("Access denied");
    }
    match install_skill(&state, package, scope, Some(&cwd)) {
        Ok(skill) => no_store(json!({"success": true, "skill": skill, "output": "Skill updated"})),
        Err(error) => internal(error),
    }
}

pub(crate) fn install_info(state: &AppState, cwd: &Path, scope: &str, name: &str) -> Option<Value> {
    let lock_path = lock_path(state, cwd, scope);
    let lock = std::fs::read_to_string(lock_path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())?;
    let entry = lock
        .get("skills")?
        .as_object()?
        .iter()
        .find_map(|(key, entry)| key.eq_ignore_ascii_case(name).then_some(entry))?;
    let source = entry.get("source").and_then(Value::as_str)?;
    let skill_path = entry.get("skillPath").and_then(Value::as_str);
    let hash = entry
        .get(if scope == "global" {
            "skillFolderHash"
        } else {
            "computedHash"
        })
        .and_then(Value::as_str);
    let source = source
        .trim_start_matches("git+")
        .trim_start_matches("https://github.com/")
        .trim_start_matches("git@github.com:")
        .trim_end_matches(".git")
        .trim_end_matches('/');
    Some(json!({
        "package": format!("{source}@{name}"),
        "scope": scope,
        "source": source,
        "sourceType": entry.get("sourceType").and_then(Value::as_str).unwrap_or("github"),
        "skillsShUrl": format!("https://skills.sh/{source}/{name}"),
        "skillPath": skill_path,
        "ref": entry.get("ref").and_then(Value::as_str),
        "versionHash": hash,
        "canCheckForUpdates": skill_path.is_some() && hash.is_some(),
    }))
}

fn lock_path(_state: &AppState, cwd: &Path, scope: &str) -> PathBuf {
    if scope == "project" {
        cwd.join("skills-lock.json")
    } else if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
        PathBuf::from(xdg).join("skills").join(".skill-lock.json")
    } else {
        pi_star_core::settings::home_dir()
            .unwrap_or_default()
            .join(".agents")
            .join(".skill-lock.json")
    }
}

fn install_skill(
    state: &AppState,
    package: &str,
    scope: &str,
    cwd: Option<&Path>,
) -> anyhow::Result<Value> {
    let (source, requested_skill) = package
        .rsplit_once('@')
        .filter(|(source, _)| !source.is_empty())
        .map(|(source, skill)| (source.to_string(), skill.to_string()))
        .unwrap_or_else(|| (package.to_string(), String::new()));
    let repo_url = if source.starts_with("http://")
        || source.starts_with("https://")
        || source.starts_with("git@")
        || source.starts_with("file://")
        || source.ends_with(".git")
    {
        source.clone()
    } else {
        format!("https://github.com/{source}.git")
    };
    let clone_dir = std::env::temp_dir().join(format!("pi-star-skill-{}", uuid::Uuid::new_v4()));
    let output = std::process::Command::new("git")
        .args(["clone", "--depth", "1", "--filter=blob:none", &repo_url])
        .arg(&clone_dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        let _ = std::fs::remove_dir_all(&clone_dir);
        return Err(anyhow::anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let Some((skill_dir, skill_name, skill_path)) = find_skill(&clone_dir, &requested_skill) else {
        let _ = std::fs::remove_dir_all(&clone_dir);
        return Err(anyhow::anyhow!("No SKILL.md found in the package"));
    };
    let target_root = if scope == "project" {
        cwd.unwrap_or(Path::new(".")).join(".agents").join("skills")
    } else {
        pi_star_core::settings::home_dir()
            .unwrap_or_default()
            .join(".agents")
            .join("skills")
    };
    std::fs::create_dir_all(&target_root)?;
    let target = target_root.join(&skill_name);
    if target.exists() {
        std::fs::remove_dir_all(&target)?;
    }
    copy_directory(&skill_dir, &target)?;
    let hash = hash_directory(&target)?;
    let lock_path = if scope == "project" {
        cwd.unwrap_or(Path::new(".")).join("skills-lock.json")
    } else if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
        PathBuf::from(xdg).join("skills").join(".skill-lock.json")
    } else {
        pi_star_core::settings::home_dir()
            .unwrap_or_default()
            .join(".agents")
            .join(".skill-lock.json")
    };
    let mut lock = std::fs::read_to_string(&lock_path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_else(|| json!({"version": 1, "skills": {}}));
    if lock.get("skills").and_then(Value::as_object).is_none() {
        lock["skills"] = json!({});
    }
    lock["skills"][&skill_name] = json!({
        "source": source,
        "sourceType": "github",
        "skillPath": skill_path,
        "ref": "HEAD",
        "skillFolderHash": hash,
        "computedHash": hash,
    });
    pi_star_core::settings::write_json_atomic(&lock_path, &lock, 0o600)?;
    let _ = std::fs::remove_dir_all(&clone_dir);
    let _ = state;
    Ok(json!({
        "name": skill_name,
        "description": "",
        "filePath": target.join("SKILL.md").to_string_lossy(),
        "baseDir": target.to_string_lossy(),
        "disableModelInvocation": false,
        "sourceInfo": {"scope": scope},
        "install": {
            "package": format!("{source}@{skill_name}"),
            "scope": scope,
            "source": source,
            "sourceType": "github",
            "skillsShUrl": format!("https://skills.sh/{source}/{skill_name}"),
            "skillPath": skill_path,
            "ref": "HEAD",
            "versionHash": hash,
            "canCheckForUpdates": true,
        }
    }))
}

fn find_skill(root: &Path, requested: &str) -> Option<(PathBuf, String, String)> {
    let mut candidates = Vec::new();
    collect_skill_files(root, root, &mut candidates, 0);
    for (dir, relative) in candidates {
        let text = std::fs::read_to_string(dir.join("SKILL.md")).ok()?;
        let name = parse_skill_name(&text).unwrap_or_else(|| {
            dir.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("skill")
                .to_string()
        });
        let directory_name = dir.file_name().and_then(|name| name.to_str()).unwrap_or("");
        if requested.is_empty()
            || name.eq_ignore_ascii_case(requested)
            || directory_name.eq_ignore_ascii_case(requested)
        {
            return Some((dir, name, relative));
        }
    }
    None
}

fn parse_skill_name(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("name:") {
            let value = value.trim().trim_matches('"').trim_matches('\'');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn collect_skill_files(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, String)>, depth: usize) {
    if depth > 6 || out.len() >= 200 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) == Some(".git") {
                continue;
            }
            collect_skill_files(root, &path, out, depth + 1);
        } else if path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
            let relative = path
                .parent()
                .and_then(|parent| parent.strip_prefix(root).ok())
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            out.push((path.parent().unwrap().to_path_buf(), relative));
        }
    }
}

fn copy_directory(source: &Path, target: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let destination = target.join(entry.file_name());
        if path.is_dir() {
            copy_directory(&path, &destination)?;
        } else if path.is_file() {
            std::fs::copy(&path, &destination)?;
        }
    }
    Ok(())
}

fn hash_directory(dir: &Path) -> anyhow::Result<String> {
    let mut files = Vec::new();
    collect_files(dir, dir, &mut files);
    files.sort();
    let mut hasher = Sha256::new();
    for (relative, path) in files {
        hasher.update(relative.as_bytes());
        hasher.update(b"\0");
        hasher.update(std::fs::read(path)?);
        hasher.update(b"\0");
    }
    Ok(hex::encode(hasher.finalize()))
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out);
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, path));
        }
    }
}
