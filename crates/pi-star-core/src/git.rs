use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, anyhow};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileStatus {
    pub file_path: String,
    pub status: String,
    pub code: String,
    pub index_status: String,
    pub worktree_status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusResponse {
    pub is_git_repository: bool,
    pub repository_root: Option<String>,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged_count: u32,
    pub unstaged_count: u32,
    pub untracked_count: u32,
    pub conflicts_count: u32,
    pub files: Vec<GitFileStatus>,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffResponse {
    pub supported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
}

#[derive(Debug, Clone)]
struct PorcelainEntry {
    path: String,
    original_path: Option<String>,
    index_status: String,
    worktree_status: String,
}

pub fn status(cwd: &Path) -> GitStatusResponse {
    let Some(root) = repository_root(cwd) else {
        return GitStatusResponse {
            is_git_repository: false,
            repository_root: None,
            branch: None,
            upstream: None,
            ahead: 0,
            behind: 0,
            staged_count: 0,
            unstaged_count: 0,
            untracked_count: 0,
            conflicts_count: 0,
            files: Vec::new(),
            additions: 0,
            deletions: 0,
        };
    };
    let porcelain = read_porcelain_v2(&root);
    let entries = porcelain.entries;
    let mut files = Vec::new();
    for entry in &entries {
        let file_path = root.join(&entry.path);
        let relative = file_path.strip_prefix(cwd).unwrap_or(&file_path);
        let file_path = cwd.join(relative).to_string_lossy().into_owned();
        let (status, code) = classify(&entry.index_status, &entry.worktree_status);
        files.push(GitFileStatus {
            file_path,
            status: status.to_string(),
            code: code.to_string(),
            index_status: entry.index_status.clone(),
            worktree_status: entry.worktree_status.clone(),
        });
    }
    let (additions, deletions) = numstat(&root);
    GitStatusResponse {
        is_git_repository: true,
        repository_root: Some(root.to_string_lossy().into_owned()),
        branch: porcelain.branch,
        upstream: porcelain.upstream,
        ahead: porcelain.ahead,
        behind: porcelain.behind,
        staged_count: porcelain.staged,
        unstaged_count: porcelain.unstaged,
        untracked_count: porcelain.untracked,
        conflicts_count: porcelain.conflicts,
        files,
        additions,
        deletions,
    }
}

pub fn diff(cwd: &Path, file_path: &Path) -> GitDiffResponse {
    diff_scoped(cwd, file_path, "all")
}

pub fn diff_scoped(cwd: &Path, file_path: &Path, scope: &str) -> GitDiffResponse {
    let Some(root) =
        repository_root(cwd).or_else(|| repository_root(file_path.parent().unwrap_or(cwd)))
    else {
        return GitDiffResponse {
            supported: false,
            status: None,
            patch: None,
        };
    };
    let Ok(relative) = file_path.strip_prefix(&root) else {
        return GitDiffResponse {
            supported: false,
            status: None,
            patch: None,
        };
    };
    let relative = relative.to_string_lossy().replace('\\', "/");
    let entry = read_porcelain(&root)
        .into_iter()
        .find(|entry| entry.path == relative);
    let Some(entry) = entry else {
        return GitDiffResponse {
            supported: false,
            status: None,
            patch: None,
        };
    };
    let (status, _) = classify(&entry.index_status, &entry.worktree_status);
    if status == "untracked" {
        if scope == "staged" {
            return GitDiffResponse {
                supported: false,
                status: None,
                patch: None,
            };
        }
        let Ok(content) = std::fs::read_to_string(file_path) else {
            return GitDiffResponse {
                supported: false,
                status: None,
                patch: None,
            };
        };
        if content.as_bytes().contains(&0) {
            return GitDiffResponse {
                supported: false,
                status: None,
                patch: None,
            };
        }
        return GitDiffResponse {
            supported: true,
            status: Some(status.to_string()),
            patch: Some(added_patch(&relative, &content)),
        };
    }
    let paths = match &entry.original_path {
        Some(original) if original != &relative => vec![original.clone(), relative.clone()],
        _ => vec![relative.clone()],
    };
    let mut args = vec![
        "diff".to_string(),
        "--no-color".to_string(),
        "--no-ext-diff".to_string(),
        "--unified=3".to_string(),
    ];
    match scope {
        "staged" => args.push("--cached".to_string()),
        "unstaged" => {}
        _ => args.push("HEAD".to_string()),
    }
    args.push("--".to_string());
    args.extend(paths);
    let Ok(patch) = git_text(&root, &args) else {
        return GitDiffResponse {
            supported: false,
            status: None,
            patch: None,
        };
    };
    if !patch.contains("\n@@ ") {
        return GitDiffResponse {
            supported: false,
            status: None,
            patch: None,
        };
    }
    GitDiffResponse {
        supported: true,
        status: Some(status.to_string()),
        patch: Some(patch),
    }
}

fn added_patch(path: &str, content: &str) -> String {
    let has_trailing = content.ends_with('\n');
    let mut lines: Vec<&str> = content.split('\n').collect();
    if has_trailing {
        lines.pop();
    }
    let body = lines
        .iter()
        .map(|line| format!("+{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let no_newline = if !has_trailing && !lines.is_empty() {
        "\n\\ No newline at end of file"
    } else {
        ""
    };
    format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n{body}{no_newline}",
        lines.len()
    )
}

fn classify(index: &str, worktree: &str) -> (&'static str, &'static str) {
    let pair = format!("{index}{worktree}");
    if pair == "??" {
        ("untracked", "U")
    } else if ["DD", "AU", "UD", "UA", "DU", "AA", "UU"].contains(&pair.as_str())
        || pair.contains('U')
    {
        ("conflict", "C")
    } else if pair.contains('D') {
        ("deleted", "D")
    } else if pair.contains('R') || pair.contains('C') {
        ("renamed", "R")
    } else if pair.contains('A') {
        ("added", "A")
    } else {
        ("modified", "M")
    }
}

fn read_porcelain(root: &Path) -> Vec<PorcelainEntry> {
    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain=v1", "-z"])
        .env("LC_ALL", "C")
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut records = text.split('\0');
    let mut entries = Vec::new();
    while let Some(record) = records.next() {
        if record.len() < 4 || &record[2..3] != " " {
            continue;
        }
        let index_status = record[0..1].to_string();
        let worktree_status = record[1..2].to_string();
        let path = record[3..].to_string();
        let original_path = if matches!(index_status.as_str(), "R" | "C")
            || matches!(worktree_status.as_str(), "R" | "C")
        {
            records.next().map(str::to_owned)
        } else {
            None
        };
        entries.push(PorcelainEntry {
            path,
            original_path,
            index_status,
            worktree_status,
        });
    }
    entries
}

fn numstat(root: &Path) -> (u64, u64) {
    let Ok(text) = git_text(
        root,
        &[
            "diff".to_string(),
            "--numstat".to_string(),
            "HEAD".to_string(),
        ],
    ) else {
        return (0, 0);
    };
    let mut additions = 0;
    let mut deletions = 0;
    for line in text.lines() {
        let mut parts = line.split('\t');
        if let (Some(a), Some(d)) = (parts.next(), parts.next()) {
            additions += a.parse::<u64>().unwrap_or(0);
            deletions += d.parse::<u64>().unwrap_or(0);
        }
    }
    (additions, deletions)
}

pub fn repository_root(dir: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .env("LC_ALL", "C")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then(|| PathBuf::from(text))
}

fn git_text(root: &Path, args: &[String]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("LC_ALL", "C")
        .output()?;
    if !output.status.success() {
        return Err(anyhow!("git command failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub is_main: bool,
    pub bare: bool,
}

pub fn list_worktrees(cwd: &Path) -> Vec<WorktreeInfo> {
    let Ok(text) = git_text(
        cwd,
        &[
            "worktree".to_string(),
            "list".to_string(),
            "--porcelain".to_string(),
        ],
    ) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    let mut current: Option<WorktreeInfo> = None;
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(info) = current.take() {
                result.push(info);
            }
            current = Some(WorktreeInfo {
                path: path.to_string(),
                branch: None,
                head: None,
                is_main: result.is_empty(),
                bare: false,
            });
        } else if let Some(head) = line.strip_prefix("HEAD ") {
            if let Some(info) = current.as_mut() {
                info.head = Some(head.to_string());
            }
        } else if let Some(branch) = line.strip_prefix("branch ") {
            if let Some(info) = current.as_mut() {
                info.branch = Some(branch.trim_start_matches("refs/heads/").to_string());
            }
        } else if line == "bare" {
            if let Some(info) = current.as_mut() {
                info.bare = true;
            }
        }
    }
    if let Some(info) = current {
        result.push(info);
    }
    result
}

pub fn add_worktree(cwd: &Path, path: &Path, branch: &str, create_branch: bool) -> Result<()> {
    let mut args = vec![
        "worktree".to_string(),
        "add".to_string(),
        path.to_string_lossy().into_owned(),
    ];
    if create_branch {
        args.push("-b".to_string());
        args.push(branch.to_string());
    } else {
        args.push(branch.to_string());
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(&args)
        .env("LC_ALL", "C")
        .output()?;
    if !output.status.success() {
        return Err(anyhow!(
            String::from_utf8_lossy(&output.stderr).trim().to_string()
        ));
    }
    Ok(())
}

pub fn remove_worktree(cwd: &Path, path: &Path, force: bool) -> Result<()> {
    let mut args = vec!["worktree".to_string(), "remove".to_string()];
    if force {
        args.push("--force".to_string());
    }
    args.push(path.to_string_lossy().into_owned());
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(&args)
        .env("LC_ALL", "C")
        .output()?;
    if !output.status.success() {
        return Err(anyhow!(
            String::from_utf8_lossy(&output.stderr).trim().to_string()
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRepoSummary {
    pub path: String,
    pub name: String,
    pub relative_path: String,
    pub is_current: bool,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    pub conflicts: u32,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHistoryCommit {
    pub sha: String,
    pub short_sha: String,
    pub parents: Vec<String>,
    pub author: String,
    pub author_email: String,
    pub date: String,
    pub subject: String,
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitFile {
    pub path: String,
    pub old_path: Option<String>,
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
    pub binary: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitDetail {
    #[serde(flatten)]
    pub commit: GitHistoryCommit,
    pub body: String,
    pub additions: u64,
    pub deletions: u64,
    pub files: Vec<GitCommitFile>,
}

#[derive(Debug, Default)]
struct PorcelainV2 {
    branch: Option<String>,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
    staged: u32,
    unstaged: u32,
    untracked: u32,
    conflicts: u32,
    entries: Vec<PorcelainEntry>,
}

fn split_xy(xy: &str) -> (String, String) {
    let mut chars = xy.chars();
    let index = chars.next().unwrap_or('.');
    let worktree = chars.next().unwrap_or('.');
    let normalize = |c: char| if c == '.' { ' ' } else { c };
    (
        normalize(index).to_string(),
        normalize(worktree).to_string(),
    )
}

fn count_entry(result: &mut PorcelainV2, entry: &PorcelainEntry) {
    if entry.index_status == "?" {
        result.untracked += 1;
        return;
    }
    if entry.index_status == "U" || entry.worktree_status == "U" {
        result.conflicts += 1;
        return;
    }
    if entry.index_status != " " {
        result.staged += 1;
    }
    if entry.worktree_status != " " {
        result.unstaged += 1;
    }
}

fn read_porcelain_v2(root: &Path) -> PorcelainV2 {
    let mut result = PorcelainV2::default();
    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=all",
            "-z",
        ])
        .env("LC_ALL", "C")
        .output()
    else {
        return result;
    };
    if !output.status.success() {
        return result;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut records = text.split('\0');
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        if let Some(rest) = record.strip_prefix("# branch.head ") {
            result.branch = (rest != "(detached)").then(|| rest.to_string());
            continue;
        }
        if let Some(rest) = record.strip_prefix("# branch.upstream ") {
            result.upstream = Some(rest.to_string());
            continue;
        }
        if let Some(rest) = record.strip_prefix("# branch.ab ") {
            let mut parts = rest.split_whitespace();
            if let Some(ahead) = parts.next().and_then(|v| v.strip_prefix('+')) {
                result.ahead = ahead.parse().unwrap_or(0);
            }
            if let Some(behind) = parts.next().and_then(|v| v.strip_prefix('-')) {
                result.behind = behind.parse().unwrap_or(0);
            }
            continue;
        }
        let entry = match record.as_bytes()[0] {
            b'1' => {
                let parts: Vec<&str> = record.splitn(9, ' ').collect();
                if parts.len() < 9 {
                    continue;
                }
                let (index, worktree) = split_xy(parts[1]);
                Some(PorcelainEntry {
                    path: parts[8].to_string(),
                    original_path: None,
                    index_status: index,
                    worktree_status: worktree,
                })
            }
            b'2' => {
                let parts: Vec<&str> = record.splitn(10, ' ').collect();
                if parts.len() < 10 {
                    continue;
                }
                let (index, worktree) = split_xy(parts[1]);
                Some(PorcelainEntry {
                    path: parts[9].to_string(),
                    original_path: records.next().map(str::to_owned),
                    index_status: index,
                    worktree_status: worktree,
                })
            }
            b'u' => {
                let parts: Vec<&str> = record.splitn(11, ' ').collect();
                if parts.len() < 11 {
                    continue;
                }
                let (index, worktree) = split_xy(parts[1]);
                Some(PorcelainEntry {
                    path: parts[10].to_string(),
                    original_path: None,
                    index_status: index,
                    worktree_status: worktree,
                })
            }
            b'?' => Some(PorcelainEntry {
                path: record[2..].to_string(),
                original_path: None,
                index_status: "?".to_string(),
                worktree_status: "?".to_string(),
            }),
            _ => None,
        };
        if let Some(entry) = entry {
            count_entry(&mut result, &entry);
            result.entries.push(entry);
        }
    }
    result
}

fn short_head(root: &Path) -> Option<String> {
    git_text(
        root,
        &[
            "rev-parse".to_string(),
            "--short".to_string(),
            "HEAD".to_string(),
        ],
    )
    .ok()
    .map(|text| text.trim().to_string())
    .filter(|text| !text.is_empty())
}

fn should_skip_repo_scan_dir(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "node_modules"
                | "target"
                | "dist"
                | "build"
                | "vendor"
                | "coverage"
                | "test-results"
                | "__pycache__"
                | ".venv"
                | "venv"
        )
}

fn same_repo_path(a: &Path, b: &Path) -> bool {
    fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf())
        == fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf())
}

fn add_repo(found: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: &Path, limit: usize) {
    if found.len() >= limit {
        return;
    }
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if seen.insert(canonical.clone()) {
        found.push(canonical);
    }
}

fn collect_git_dirs(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    found: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    limit: usize,
) {
    if found.len() >= limit || depth > max_depth {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut children = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" || should_skip_repo_scan_dir(&name) {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let child = entry.path();
        if child.join(".git").exists() {
            add_repo(found, seen, &child, limit);
            // A repository's own dependencies are not where sibling repos live.
            continue;
        }
        if depth < max_depth {
            children.push(child);
        }
    }
    children.sort();
    for child in children {
        collect_git_dirs(&child, depth + 1, max_depth, found, seen, limit);
    }
}

fn repo_summary(root: &Path, scan_root: &Path, is_current: bool) -> GitRepoSummary {
    let porcelain = read_porcelain_v2(root);
    let relative = root
        .strip_prefix(scan_root)
        .ok()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .filter(|path| !path.is_empty())
        .unwrap_or_default();
    let dirty =
        porcelain.staged + porcelain.unstaged + porcelain.untracked + porcelain.conflicts > 0;
    GitRepoSummary {
        path: root.to_string_lossy().into_owned(),
        name: root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_else(|| root.to_str().unwrap_or("repository"))
            .to_string(),
        relative_path: relative,
        is_current,
        branch: porcelain.branch,
        head: short_head(root),
        ahead: porcelain.ahead,
        behind: porcelain.behind,
        staged: porcelain.staged,
        unstaged: porcelain.unstaged,
        untracked: porcelain.untracked,
        conflicts: porcelain.conflicts,
        dirty,
    }
}

pub fn scan_repositories(root: &Path, max_depth: usize) -> Vec<GitRepoSummary> {
    const LIMIT: usize = 20;
    let max_depth = max_depth.clamp(1, 5);
    let current_repo = repository_root(root);
    let mut found = Vec::new();
    let mut seen = HashSet::new();

    if let Some(repo) = current_repo.as_ref() {
        add_repo(&mut found, &mut seen, repo, LIMIT);
    } else if root.join(".git").exists() {
        add_repo(&mut found, &mut seen, root, LIMIT);
    }
    collect_git_dirs(root, 1, max_depth, &mut found, &mut seen, LIMIT);

    let current = current_repo
        .as_deref()
        .or_else(|| root.join(".git").exists().then_some(root));
    let mut repos: Vec<GitRepoSummary> = found
        .into_iter()
        .map(|path| {
            let is_current = current
                .as_ref()
                .is_some_and(|repo| same_repo_path(repo, &path));
            repo_summary(&path, root, is_current)
        })
        .collect();
    repos.sort_by(|a, b| {
        b.is_current
            .cmp(&a.is_current)
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });
    repos
}

pub fn history(cwd: &Path, limit: usize, skip: usize) -> Vec<GitHistoryCommit> {
    let Some(root) = repository_root(cwd) else {
        return Vec::new();
    };
    let limit = limit.clamp(1, 200).to_string();
    let skip = skip.to_string();
    let Ok(text) = git_text(
        &root,
        &[
            "log".to_string(),
            "--no-color".to_string(),
            "--date=iso-strict".to_string(),
            "--pretty=format:%H%x00%h%x00%P%x00%an%x00%ae%x00%aI%x00%s%x00%D%x1e".to_string(),
            "-n".to_string(),
            limit,
            "--skip".to_string(),
            skip,
            "HEAD".to_string(),
            "--".to_string(),
        ],
    ) else {
        return Vec::new();
    };
    text.split('\x1e')
        .map(|record| record.trim_start_matches(['\n', '\r']))
        .filter(|record| !record.is_empty())
        .filter_map(|record| {
            let fields: Vec<&str> = record.splitn(8, '\0').collect();
            if fields.len() < 8 {
                return None;
            }
            Some(GitHistoryCommit {
                sha: fields[0].to_string(),
                short_sha: fields[1].to_string(),
                parents: fields[2].split_whitespace().map(str::to_string).collect(),
                author: fields[3].to_string(),
                author_email: fields[4].to_string(),
                date: fields[5].to_string(),
                subject: fields[6].to_string(),
                refs: fields[7]
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect(),
            })
        })
        .collect()
}

fn valid_commit_sha(sha: &str) -> bool {
    (4..=64).contains(&sha.len()) && sha.chars().all(|c| c.is_ascii_hexdigit())
}

fn safe_repo_relative(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return None;
    }
    Some(path.to_path_buf())
}

fn commit_files(root: &Path, sha: &str) -> Vec<GitCommitFile> {
    let Ok(status_text) = git_text(
        root,
        &[
            "show".to_string(),
            "--format=".to_string(),
            "--name-status".to_string(),
            "-z".to_string(),
            "--find-renames".to_string(),
            sha.to_string(),
        ],
    ) else {
        return Vec::new();
    };
    let mut entries: Vec<(String, Option<String>, String)> = Vec::new();
    let mut records = status_text.split('\0');
    while let Some(status) = records.next() {
        if status.is_empty() {
            continue;
        }
        let kind = status.chars().next().unwrap_or('M');
        let mapped = match kind {
            'A' => "added",
            'D' => "deleted",
            'R' => "renamed",
            'C' => "copied",
            _ => "modified",
        }
        .to_string();
        if matches!(kind, 'R' | 'C') {
            let old = records.next().unwrap_or("").to_string();
            let new = records.next().unwrap_or("").to_string();
            entries.push((new, Some(old), mapped));
        } else {
            let path = records.next().unwrap_or("").to_string();
            entries.push((path, None, mapped));
        }
    }

    let mut stats: HashMap<String, (u64, u64, bool)> = HashMap::new();
    if let Ok(numstat_text) = git_text(
        root,
        &[
            "show".to_string(),
            "--format=".to_string(),
            "--numstat".to_string(),
            "-z".to_string(),
            "--find-renames".to_string(),
            sha.to_string(),
        ],
    ) {
        let mut records = numstat_text.split('\0');
        while let Some(record) = records.next() {
            if record.is_empty() {
                continue;
            }
            let mut parts = record.splitn(3, '\t');
            let additions = parts.next().unwrap_or("");
            let deletions = parts.next().unwrap_or("");
            let path = parts.next().unwrap_or("");
            let (path, binary) = if path.is_empty() {
                let _old = records.next();
                (
                    records.next().unwrap_or("").to_string(),
                    additions == "-" || deletions == "-",
                )
            } else {
                (path.to_string(), additions == "-" || deletions == "-")
            };
            stats.insert(
                path,
                (
                    additions.parse().unwrap_or(0),
                    deletions.parse().unwrap_or(0),
                    binary,
                ),
            );
        }
    }

    entries
        .into_iter()
        .map(|(path, old_path, status)| {
            let (additions, deletions, binary) = stats.get(&path).copied().unwrap_or((0, 0, false));
            GitCommitFile {
                path,
                old_path,
                status,
                additions,
                deletions,
                binary,
            }
        })
        .collect()
}

pub fn commit_detail(cwd: &Path, sha: &str) -> Option<GitCommitDetail> {
    if !valid_commit_sha(sha) {
        return None;
    }
    let root = repository_root(cwd)?;
    let text = git_text(
        &root,
        &[
            "show".to_string(),
            "-s".to_string(),
            "--no-color".to_string(),
            "--date=iso-strict".to_string(),
            "--pretty=format:%H%x00%h%x00%P%x00%an%x00%ae%x00%aI%x00%s%x00%D%x00%B".to_string(),
            sha.to_string(),
        ],
    )
    .ok()?;
    let fields: Vec<&str> = text.splitn(9, '\0').collect();
    if fields.len() < 9 {
        return None;
    }
    let files = commit_files(&root, sha);
    let additions = files.iter().map(|file| file.additions).sum();
    let deletions = files.iter().map(|file| file.deletions).sum();
    Some(GitCommitDetail {
        commit: GitHistoryCommit {
            sha: fields[0].to_string(),
            short_sha: fields[1].to_string(),
            parents: fields[2].split_whitespace().map(str::to_string).collect(),
            author: fields[3].to_string(),
            author_email: fields[4].to_string(),
            date: fields[5].to_string(),
            subject: fields[6].to_string(),
            refs: fields[7]
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect(),
        },
        body: fields[8].trim_end().to_string(),
        additions,
        deletions,
        files,
    })
}

pub fn commit_diff(cwd: &Path, sha: &str, path: &str, old_path: Option<&str>) -> Option<String> {
    if !valid_commit_sha(sha) {
        return None;
    }
    let root = repository_root(cwd)?;
    let path = safe_repo_relative(path)?;
    let mut paths = vec![path];
    if let Some(old_path) = old_path.and_then(safe_repo_relative) {
        paths.push(old_path);
    }
    let mut args = vec![
        "show".to_string(),
        "--no-color".to_string(),
        "--format=".to_string(),
        "--patch".to_string(),
        "--find-renames".to_string(),
        sha.to_string(),
        "--".to_string(),
    ];
    args.extend(
        paths
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned()),
    );
    let patch = git_text(&root, &args).ok()?;
    (!patch.trim().is_empty()).then_some(patch)
}
