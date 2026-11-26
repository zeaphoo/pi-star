use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;

use pi_star_core::git;
use pi_star_core::process::output_with_timeout;
use pi_star_core::project::ProjectInfo;

use crate::routes::{bad_request, blocking, forbidden, internal, no_store, not_found};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CwdQuery {
    cwd: Option<String>,
    path: Option<String>,
    scope: Option<String>,
    // Context lines for a diff (`--unified`); other routes ignore it.
    context: Option<u32>,
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
    let result = match blocking(move || git::status(&cwd)).await {
        Ok(result) => result,
        Err(response) => return response,
    };
    no_store(result)
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
    let scope = query.scope.unwrap_or_else(|| "all".to_string());
    let context = query.context.unwrap_or(3);
    let result =
        match blocking(move || git::diff_scoped_with_context(&cwd, &path, &scope, context)).await {
            Ok(result) => result,
            Err(response) => return response,
        };
    no_store(result)
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
    let prepared = match blocking(move || {
        let project = ProjectInfo::for_cwd(&cwd);
        let list = git::list_worktrees(&cwd);
        let current = list
            .iter()
            .find(|worktree| pi_star_core::project::same_path(Path::new(&worktree.path), &cwd))
            .map(|worktree| worktree.path.clone());
        let project_key = pi_star_core::project::project_key(Path::new(&project.project_root));
        (project, list, current, project_key)
    })
    .await
    {
        Ok(prepared) => prepared,
        Err(response) => return response,
    };
    let (project, list, current, project_key) = prepared;
    for worktree in &list {
        state.roots.add(&worktree.path);
    }
    no_store(json!({
        "projectRoot": project.project_root,
        "projectKey": project_key,
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
    let branch = body.branch.trim().to_string();
    if branch.is_empty() {
        return bad_request("Branch name is required");
    }
    let branch_for_work = branch.clone();
    let outcome = match blocking(move || -> Result<PathBuf, Response> {
        let Some(repo_root) = git::repository_root(&cwd) else {
            return Err(bad_request("Not a git repository"));
        };
        if let Some(holder) = git::list_worktrees(&cwd)
            .into_iter()
            .find(|worktree| worktree.branch.as_deref() == Some(branch_for_work.as_str()))
        {
            return Err((
                axum::http::StatusCode::CONFLICT,
                axum::Json(json!({
                    "error": format!(
                        "Branch {branch_for_work} is already checked out in {}",
                        holder.path
                    ),
                    "code": "branch_in_use",
                    "branch": branch_for_work,
                    "worktreePath": holder.path,
                })),
            )
                .into_response());
        }
        let dir_name = sanitize_branch(&branch_for_work);
        let base = PathBuf::from(format!("{}-worktrees", repo_root.to_string_lossy()));
        let worktree_path = base.join(dir_name);
        if worktree_path.exists() {
            return Err(bad_request(format!(
                "Directory already exists: {}",
                worktree_path.display()
            )));
        }
        if let Err(error) = std::fs::create_dir_all(&base) {
            return Err(internal(error));
        }
        let mut command = Command::new("git");
        command.arg("-C").arg(&repo_root).args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch_for_work}"),
        ]);
        pi_star_core::sanitize_std_command(&mut command);
        let branch_exists = output_with_timeout(&mut command, git::GIT_TIMEOUT)
            .map(|output| output.status.success())
            .unwrap_or(false);
        if let Err(error) =
            git::add_worktree(&repo_root, &worktree_path, &branch_for_work, !branch_exists)
        {
            return Err(bad_request(error.to_string()));
        }
        Ok(worktree_path)
    })
    .await
    {
        Ok(outcome) => outcome,
        Err(response) => return response,
    };
    let worktree_path = match outcome {
        Ok(path) => path,
        Err(response) => return response,
    };
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
    let force = body.force.unwrap_or(false);
    let outcome = match blocking(move || -> Result<(), Response> {
        let list = git::list_worktrees(&cwd);
        let Some(target) = list
            .iter()
            .find(|worktree| pi_star_core::project::same_path(Path::new(&worktree.path), &path))
        else {
            return Err(bad_request("Not a worktree of this repository"));
        };
        if target.is_main {
            return Err(bad_request("Cannot remove the main worktree"));
        }
        match git::remove_worktree(&cwd, Path::new(&target.path), force) {
            Ok(()) => Ok(()),
            Err(error) => {
                let message = error.to_string();
                let dirty = message.contains("contains modified or untracked files")
                    || message.contains("is dirty")
                    || message.to_lowercase().contains("containing submodules");
                if dirty {
                    Err((
                        axum::http::StatusCode::CONFLICT,
                        axum::Json(json!({ "error": message, "dirty": true })),
                    )
                        .into_response())
                } else {
                    Err(bad_request(message))
                }
            }
        }
    })
    .await
    {
        Ok(outcome) => outcome,
        Err(response) => return response,
    };
    match outcome {
        Ok(()) => no_store(json!({ "success": true })),
        Err(response) => response,
    }
}

use axum::response::IntoResponse;

#[derive(Debug, Deserialize, Default)]
pub struct ReposQuery {
    root: Option<String>,
    depth: Option<usize>,
}

/// Scans the current project's directory for Git repositories. The root must be
/// one of the project roots the server has already admitted; discoveries stay
/// inside that root and are filtered through the same allow-list again.
pub async fn repos(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ReposQuery>,
) -> Response {
    let Some(root) = query.root.filter(|value| !value.trim().is_empty()) else {
        return bad_request("root must be an absolute path");
    };
    let root = PathBuf::from(root);
    if !root.is_absolute() {
        return bad_request("root must be an absolute path");
    }
    if !state.roots.is_allowed(&root) || !state.roots.is_allowed_existing(&root) {
        return forbidden("Access denied");
    }
    if !root.is_dir() {
        return not_found("Directory not found");
    }
    // Two levels below the project root are enough for the sibling-repo
    // layouts the Changes tab targets; deeper recursion mostly walks
    // directories that hold no repositories and makes the first open slower.
    let depth = query.depth.unwrap_or(2).clamp(1, 5);
    let roots = state.roots.clone();
    let index_root = root.clone();
    let repositories = match blocking(move || {
        let mut repositories = git::scan_repositories(&index_root, depth);
        repositories.retain(|repository| roots.is_allowed_existing(Path::new(&repository.path)));
        repositories
    })
    .await
    {
        Ok(repositories) => repositories,
        Err(response) => return response,
    };
    no_store(json!({
        "root": root.to_string_lossy(),
        "repos": repositories,
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct HistoryQuery {
    cwd: Option<String>,
    limit: Option<usize>,
    skip: Option<usize>,
}

pub async fn history(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HistoryQuery>,
) -> Response {
    let Some(cwd) = query.cwd.filter(|value| !value.trim().is_empty()) else {
        return bad_request("cwd must be an absolute path");
    };
    let cwd = PathBuf::from(cwd);
    if !cwd.is_absolute() {
        return bad_request("cwd must be an absolute path");
    }
    if !state.roots.is_allowed(&cwd) || !state.roots.is_allowed_existing(&cwd) {
        return forbidden("Access denied");
    }
    if !cwd.is_dir() {
        return not_found("Directory not found");
    }
    let limit = query.limit.unwrap_or(50);
    let skip = query.skip.unwrap_or(0);
    let commits = match blocking(move || git::history(&cwd, limit, skip)).await {
        Ok(commits) => commits,
        Err(response) => return response,
    };
    no_store(json!({ "commits": commits }))
}

#[derive(Debug, Deserialize, Default)]
pub struct CommitQuery {
    cwd: Option<String>,
    sha: Option<String>,
}

pub async fn commit(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CommitQuery>,
) -> Response {
    let Some(cwd) = query.cwd.filter(|value| !value.trim().is_empty()) else {
        return bad_request("cwd must be an absolute path");
    };
    let Some(sha) = query.sha.filter(|value| !value.trim().is_empty()) else {
        return bad_request("sha is required");
    };
    let cwd = PathBuf::from(cwd);
    if !cwd.is_absolute() {
        return bad_request("cwd must be an absolute path");
    }
    if !state.roots.is_allowed(&cwd) || !state.roots.is_allowed_existing(&cwd) {
        return forbidden("Access denied");
    }
    if !cwd.is_dir() {
        return not_found("Directory not found");
    }
    let detail = match blocking(move || git::commit_detail(&cwd, &sha)).await {
        Ok(detail) => detail,
        Err(response) => return response,
    };
    match detail {
        Some(detail) => no_store(detail),
        None => not_found("Commit not found"),
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct CommitDiffQuery {
    cwd: Option<String>,
    sha: Option<String>,
    path: Option<String>,
    #[serde(rename = "oldPath")]
    old_path: Option<String>,
    context: Option<u32>,
}

pub async fn commit_diff(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CommitDiffQuery>,
) -> Response {
    let Some(cwd) = query.cwd.filter(|value| !value.trim().is_empty()) else {
        return bad_request("cwd must be an absolute path");
    };
    let Some(sha) = query.sha.filter(|value| !value.trim().is_empty()) else {
        return bad_request("sha is required");
    };
    let Some(path) = query.path.filter(|value| !value.trim().is_empty()) else {
        return bad_request("path is required");
    };
    let cwd = PathBuf::from(cwd);
    if !cwd.is_absolute() {
        return bad_request("cwd must be an absolute path");
    }
    if !state.roots.is_allowed(&cwd) || !state.roots.is_allowed_existing(&cwd) {
        return forbidden("Access denied");
    }
    if !cwd.is_dir() {
        return not_found("Directory not found");
    }
    let old_path = query.old_path;
    let context = query.context.unwrap_or(3);
    let patch = match blocking(move || {
        git::commit_diff_with_context(&cwd, &sha, &path, old_path.as_deref(), context)
    })
    .await
    {
        Ok(patch) => patch,
        Err(response) => return response,
    };
    match patch {
        Some(patch) => no_store(json!({ "patch": patch })),
        None => not_found("Diff not found"),
    }
}
