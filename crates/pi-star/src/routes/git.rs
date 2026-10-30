use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;

use pi_star_core::git;
use pi_star_core::project::ProjectInfo;

use crate::routes::{bad_request, forbidden, internal, no_store, not_found};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CwdQuery {
    cwd: Option<String>,
    path: Option<String>,
}

pub async fn status(State(state): State<Arc<AppState>>, Query(query): Query<CwdQuery>) -> Response {
    let Some(cwd) = query.cwd.filter(|value| !value.trim().is_empty()) else {
        return bad_request("cwd must be an absolute path");
    };
    let cwd = PathBuf::from(cwd);
    if !cwd.is_absolute() {
        return bad_request("cwd must be an absolute path");
    }
    if !state.roots.is_allowed(&cwd) {
        return forbidden("Access denied");
    }
    if !cwd.is_dir() {
        return not_found("Directory not found");
    }
    if !state.roots.is_allowed_existing(&cwd) {
        return forbidden("Access denied");
    }
    no_store(git::status(&cwd))
}

pub async fn diff(State(state): State<Arc<AppState>>, Query(query): Query<CwdQuery>) -> Response {
    let Some(cwd) = query.cwd.filter(|value| !value.trim().is_empty()) else {
        return bad_request("cwd must be an absolute path");
    };
    let Some(path) = query.path.filter(|value| !value.trim().is_empty()) else {
        return bad_request("path must be an absolute path");
    };
    let cwd = PathBuf::from(cwd);
    let path = PathBuf::from(path);
    if !cwd.is_absolute() || !path.is_absolute() {
        return bad_request("cwd and path must be absolute paths");
    }
    if !state.roots.is_allowed(&cwd) || !state.roots.is_allowed(&path) {
        return forbidden("Access denied");
    }
    if !state.roots.is_allowed_existing(&cwd) {
        return forbidden("Access denied");
    }
    no_store(git::diff(&cwd, &path))
}

#[derive(Debug, Deserialize, Default)]
pub struct WorktreeQuery {
    cwd: Option<String>,
}

pub async fn worktrees(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorktreeQuery>,
) -> Response {
    let Some(cwd) = query.cwd else {
        return bad_request("cwd is required");
    };
    let cwd = PathBuf::from(cwd);
    if !state.roots.is_allowed(&cwd) || !state.roots.is_allowed_existing(&cwd) {
        return forbidden("Access denied");
    }
    let project = ProjectInfo::for_cwd(&cwd);
    let list = git::list_worktrees(&cwd);
    for worktree in &list {
        state.roots.add(&worktree.path);
    }
    let current = list
        .iter()
        .find(|worktree| pi_star_core::project::same_path(Path::new(&worktree.path), &cwd))
        .map(|worktree| worktree.path.clone());
    no_store(json!({
        "projectRoot": project.project_root,
        "projectKey": pi_star_core::project::project_key(Path::new(&project.project_root)),
        "isGit": !list.is_empty(),
        "isTopLevel": project.is_top_level,
        "currentWorktreePath": current,
        "worktrees": list,
    }))
}

#[derive(Debug, Deserialize)]
pub struct AddWorktreeBody {
    cwd: String,
    branch: String,
}

pub async fn add_worktree(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AddWorktreeBody>,
) -> Response {
    let cwd = PathBuf::from(&body.cwd);
    if !state.roots.is_allowed(&cwd) || !state.roots.is_allowed_existing(&cwd) {
        return forbidden("Access denied");
    }
    let branch = body.branch.trim();
    if branch.is_empty() {
        return bad_request("Branch name is required");
    }
    let Some(repo_root) = git::repository_root(&cwd) else {
        return bad_request("Not a git repository");
    };
    if let Some(holder) = git::list_worktrees(&cwd)
        .into_iter()
        .find(|worktree| worktree.branch.as_deref() == Some(branch))
    {
        return (
            axum::http::StatusCode::CONFLICT,
            axum::Json(json!({
                "error": format!("Branch {branch} is already checked out in {}", holder.path),
                "code": "branch_in_use",
                "branch": branch,
                "worktreePath": holder.path,
            })),
        )
            .into_response();
    }
    let dir_name = sanitize_branch(branch);
    let base = PathBuf::from(format!("{}-worktrees", repo_root.to_string_lossy()));
    let worktree_path = base.join(dir_name);
    if worktree_path.exists() {
        return bad_request(format!(
            "Directory already exists: {}",
            worktree_path.display()
        ));
    }
    if let Err(error) = std::fs::create_dir_all(&base) {
        return internal(error);
    }
    let branch_exists = Command::new("git")
        .arg("-C")
        .arg(&repo_root)
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if let Err(error) = git::add_worktree(&repo_root, &worktree_path, branch, !branch_exists) {
        return bad_request(error.to_string());
    }
    state.roots.add(&worktree_path);
    no_store(json!({ "path": worktree_path.to_string_lossy(), "branch": branch }))
}

fn sanitize_branch(branch: &str) -> String {
    branch
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

#[derive(Debug, Deserialize)]
pub struct RemoveWorktreeBody {
    cwd: String,
    path: String,
    force: Option<bool>,
}

pub async fn remove_worktree(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RemoveWorktreeBody>,
) -> Response {
    let cwd = PathBuf::from(&body.cwd);
    let path = PathBuf::from(&body.path);
    if !state.roots.is_allowed(&cwd) {
        return forbidden("Access denied");
    }
    let list = git::list_worktrees(&cwd);
    let Some(target) = list
        .iter()
        .find(|worktree| pi_star_core::project::same_path(Path::new(&worktree.path), &path))
    else {
        return bad_request("Not a worktree of this repository");
    };
    if target.is_main {
        return bad_request("Cannot remove the main worktree");
    }
    match git::remove_worktree(&cwd, Path::new(&target.path), body.force.unwrap_or(false)) {
        Ok(()) => no_store(json!({ "success": true })),
        Err(error) => {
            let message = error.to_string();
            let dirty = message.contains("contains modified or untracked files")
                || message.contains("is dirty")
                || message.to_lowercase().contains("containing submodules");
            if dirty {
                (
                    axum::http::StatusCode::CONFLICT,
                    axum::Json(json!({ "error": message, "dirty": true })),
                )
                    .into_response()
            } else {
                bad_request(message)
            }
        }
    }
}

use axum::response::IntoResponse;
