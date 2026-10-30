use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub project_root: String,
    pub branch: Option<String>,
    pub is_worktree: bool,
    pub is_top_level: bool,
}

impl ProjectInfo {
    pub fn for_cwd(cwd: &Path) -> Self {
        let root = canonical(cwd);
        let show_toplevel = git_output(cwd, &["rev-parse", "--show-toplevel"]);
        let common_dir = git_output(
            cwd,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .or_else(|| git_output(cwd, &["rev-parse", "--git-common-dir"]));
        let branch = git_output(cwd, &["symbolic-ref", "--short", "HEAD"]);

        let Some(toplevel) = show_toplevel else {
            return Self {
                project_root: root.to_string_lossy().into_owned(),
                branch,
                is_worktree: false,
                is_top_level: true,
            };
        };
        let toplevel = canonical(Path::new(&toplevel));
        let project_root = common_dir
            .as_deref()
            .and_then(|dir| {
                let path = if Path::new(dir).is_absolute() {
                    PathBuf::from(dir)
                } else {
                    cwd.join(dir)
                };
                let path = canonical(&path);
                if path.file_name().and_then(|n| n.to_str()) == Some(".git") {
                    path.parent().map(Path::to_path_buf)
                } else {
                    // A bare repository has no working tree parent.
                    Some(path)
                }
            })
            .unwrap_or_else(|| toplevel.clone());

        let is_top_level = same_path(&root, &toplevel);
        let is_worktree = is_top_level && !same_path(&toplevel, &project_root);
        Self {
            project_root: project_root.to_string_lossy().into_owned(),
            branch,
            is_worktree,
            is_top_level,
        }
    }
}

pub fn project_key(cwd: &Path) -> String {
    let text = canonical(cwd).to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

pub fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    let a = canonical(a);
    let b = canonical(b);
    if cfg!(windows) {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

fn git_output(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_git_directory_is_its_own_project() {
        let info = ProjectInfo::for_cwd(Path::new("/tmp"));
        assert!(!info.is_worktree);
    }
}
