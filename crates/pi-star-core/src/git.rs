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
            files: Vec::new(),
            additions: 0,
            deletions: 0,
        };
    };
    let entries = read_porcelain(&root);
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
        files,
        additions,
        deletions,
    }
}

pub fn diff(cwd: &Path, file_path: &Path) -> GitDiffResponse {
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
        "HEAD".to_string(),
        "--".to_string(),
    ];
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
