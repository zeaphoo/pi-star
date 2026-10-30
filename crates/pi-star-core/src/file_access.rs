use std::path::{Component, Path, PathBuf};

use parking_lot::RwLock;

use crate::project::canonical;

/// The set of directories the file, Git and terminal APIs may touch.
///
/// Roots come from persisted session cwds/project roots plus directories the
/// current process explicitly allowed (a new session's cwd, an `allow-link`
/// confirmation). Checks resolve symlinks before comparing.
pub struct AllowedRoots {
    roots: RwLock<Vec<PathBuf>>,
}

impl AllowedRoots {
    pub fn new() -> Self {
        Self {
            roots: RwLock::new(Vec::new()),
        }
    }

    pub fn add(&self, root: impl AsRef<Path>) {
        let root = canonical(root.as_ref());
        if root.as_os_str().is_empty() {
            return;
        }
        let mut roots = self.roots.write();
        if !roots.iter().any(|existing| same_root(existing, &root)) {
            roots.push(root);
        }
    }

    pub fn add_many<I, P>(&self, roots: I)
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        for root in roots {
            self.add(root);
        }
    }

    pub fn snapshot(&self) -> Vec<PathBuf> {
        self.roots.read().clone()
    }

    /// Lexical containment that tolerates `..` and separator differences.
    pub fn is_allowed(&self, target: &Path) -> bool {
        let target = normalize_lexical(target);
        self.roots
            .read()
            .iter()
            .any(|root| is_within(&target, root))
    }

    /// Containment after resolving every existing symlink in `target`.
    pub fn is_allowed_existing(&self, target: &Path) -> bool {
        let target = resolve_existing(target);
        self.roots
            .read()
            .iter()
            .any(|root| is_within(&target, root))
    }

    /// Return the target only when it is inside an allowed root.
    pub fn authorize(&self, target: &Path) -> Option<PathBuf> {
        if self.is_allowed_existing(target) {
            Some(resolve_existing(target))
        } else {
            None
        }
    }
}

impl Default for AllowedRoots {
    fn default() -> Self {
        Self::new()
    }
}

pub fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

/// Resolve symlinks in the longest existing prefix and keep the missing tail.
pub fn resolve_existing(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        normalize_lexical(path)
    } else {
        normalize_lexical(&std::env::current_dir().unwrap_or_default().join(path))
    };
    let mut candidate = absolute.clone();
    let mut tail = Vec::new();
    loop {
        match candidate.canonicalize() {
            Ok(mut resolved) => {
                for part in tail.iter().rev() {
                    resolved.push(part);
                }
                return resolve_dots(resolved);
            }
            Err(_) => match candidate.file_name() {
                Some(name) => {
                    tail.push(name.to_os_string());
                    if !candidate.pop() {
                        return absolute;
                    }
                }
                None => return absolute,
            },
        }
    }
}

fn resolve_dots(path: PathBuf) -> PathBuf {
    normalize_lexical(&path)
}

fn same_root(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

fn is_within(target: &Path, root: &Path) -> bool {
    let target_text = target.to_string_lossy();
    let root_text = root.to_string_lossy();
    if cfg!(windows) {
        let target_text = target_text.replace('\\', "/").to_lowercase();
        let root_text = root_text.replace('\\', "/").to_lowercase();
        target_text == root_text || target_text.starts_with(&format!("{root_text}/"))
    } else {
        target == root || target.starts_with(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_traversal() {
        let roots = AllowedRoots::new();
        roots.add("/tmp/root");
        assert!(roots.is_allowed(Path::new("/tmp/root/a")));
        assert!(!roots.is_allowed(Path::new("/tmp/root/../other")));
        assert!(!roots.is_allowed(Path::new("/tmp/other")));
    }
}
