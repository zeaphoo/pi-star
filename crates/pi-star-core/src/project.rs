use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use pi_star_tools::utils::process::output_with_timeout;

/// Discovery must fail fast on a stalled volume instead of blocking a listing.
const GIT_TIMEOUT: Duration = Duration::from_secs(12);

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
                    // A submodule's git dir (`<super>/.git/modules/<name>`)
                    // names its checkout; otherwise a bare repository has no
                    // working tree parent and is its own root.
                    let checkout = configured_checkout(&path);
                    Some(checkout.unwrap_or(path))
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

/// The checkout a git dir names as `core.worktree`, when the checkout points
/// back at that git dir.
///
/// A submodule's common dir is `<super>/.git/modules/<name>`, so its own name
/// says nothing about where the checkout is. Reading `core.worktree` alone
/// would move the project root (and with it a file root) wherever a config
/// says, so it counts only when the candidate's `.git` file names this git
/// dir.
fn configured_checkout(common_dir: &Path) -> Option<PathBuf> {
    let config = common_dir.join("config");
    if !config.is_file() {
        return None;
    }
    let mut command = Command::new("git");
    command
        .arg("config")
        .arg("--file")
        .arg(&config)
        .args(["--get", "core.worktree"])
        .env("LC_ALL", "C");
    pi_star_tools::sanitize_std_command(&mut command);
    let output = output_with_timeout(&mut command, GIT_TIMEOUT).ok()?;
    if !output.status.success() {
        return None;
    }
    let configured = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if configured.is_empty() {
        return None;
    }
    let candidate = canonical(&common_dir.join(configured));
    let pointer = std::fs::read_to_string(candidate.join(".git")).ok()?;
    let target = pointer
        .trim()
        .strip_prefix("gitdir:")
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let target = canonical(&candidate.join(target));
    same_path(&target, common_dir).then_some(candidate)
}

fn git_output(cwd: &Path, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(cwd).args(args).env("LC_ALL", "C");
    pi_star_tools::sanitize_std_command(&mut command);
    let output = output_with_timeout(&mut command, GIT_TIMEOUT).ok()?;
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

    #[test]
    fn a_submodules_git_dir_names_its_checkout_only_when_the_checkout_points_back() {
        let temp = tempfile::tempdir().unwrap();
        let superproject = temp.path();
        let git_dir = superproject.join(".git").join("modules").join("sub");
        let checkout = superproject.join("sub");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::create_dir_all(&checkout).unwrap();
        std::fs::write(
            git_dir.join("config"),
            format!(
                "[core]\n\tworktree = {}\n",
                checkout.to_string_lossy()
            ),
        )
        .unwrap();
        std::fs::write(checkout.join(".git"), "gitdir: ../.git/modules/sub\n").unwrap();

        assert_eq!(
            configured_checkout(&canonical(&git_dir)),
            Some(canonical(&checkout))
        );

        // A different folder named as core.worktree is refused: a bare clone's
        // config alone must never move the project root.
        let elsewhere = superproject.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join(".git"), "gitdir: ../.git/modules/other\n").unwrap();
        std::fs::write(
            git_dir.join("config"),
            format!("[core]\n\tworktree = {}\n", elsewhere.to_string_lossy()),
        )
        .unwrap();
        assert_eq!(configured_checkout(&canonical(&git_dir)), None);
    }
}
