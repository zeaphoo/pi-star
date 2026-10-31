//! Bundled `pi` runtime discovery.
//!
//! Desktop installers ship `pi-runtime/` beside the `pi-star` binary:
//!
//! ```text
//! pi-runtime/
//!   node(.exe)
//!   pi/                    @earendil-works/pi-coding-agent + node_modules
//!   agent-extension/       pi-star's host extension
//! ```
//!
//! `PI_STAR_PI_CLI` remains the explicit escape hatch for a user-installed
//! `pi`. Without it, `PI_STAR_PI_NODE` + `PI_STAR_PI_JS` (or auto-discovery)
//! runs the bundled CLI through the bundled Node runtime.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const RUNTIME_DIR: &str = "pi-runtime";

#[derive(Debug, Clone)]
pub struct PiRuntime {
    pub dir: PathBuf,
    pub node: PathBuf,
    pub cli_js: PathBuf,
    pub package_dir: PathBuf,
    pub extension: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiCommand {
    pub program: PathBuf,
    pub prefix: Vec<OsString>,
}

/// Locate the bundled runtime. Explicit env first, then beside the executable,
/// then (debug only) the repository staging directory.
pub fn resolve() -> Option<PiRuntime> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os("PI_STAR_PI_RUNTIME") {
        candidates.push(PathBuf::from(dir));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join(RUNTIME_DIR));
        }
    }
    #[cfg(debug_assertions)]
    {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        candidates.push(manifest.join("..").join("..").join("app").join(RUNTIME_DIR));
        candidates.push(
            manifest
                .join("..")
                .join("..")
                .join("dist")
                .join(RUNTIME_DIR),
        );
    }
    candidates.iter().find_map(|dir| from_dir(dir))
}

fn from_dir(dir: &Path) -> Option<PiRuntime> {
    let node = dir.join(node_name());
    if !node.is_file() {
        return None;
    }
    let package_dir = dir.join("pi");
    if !package_dir.join("package.json").is_file() {
        return None;
    }
    let cli_js = ["dist/bundle/cli.js", "dist/cli.js"]
        .iter()
        .map(|relative| package_dir.join(relative))
        .find(|path| path.is_file())?;
    let extension = dir.join("agent-extension").join("index.ts");
    Some(PiRuntime {
        dir: dir.to_path_buf(),
        node,
        cli_js,
        package_dir,
        extension: extension.is_file().then_some(extension),
    })
}

fn node_name() -> &'static str {
    if cfg!(windows) { "node.exe" } else { "node" }
}

/// Populate the environment consumed by `pi_rpc`, the prompt builder and the
/// export path. An explicit `PI_STAR_PI_CLI` wins and disables auto-detection.
pub fn apply_env() {
    if std::env::var_os("PI_STAR_PI_CLI").is_some() {
        return;
    }
    let Some(runtime) = resolve() else {
        return;
    };
    set_if_absent("PI_STAR_PI_RUNTIME", &runtime.dir);
    set_if_absent("PI_STAR_PI_NODE", &runtime.node);
    set_if_absent("PI_STAR_PI_JS", &runtime.cli_js);
    set_if_absent("PI_PACKAGE_DIR", &runtime.package_dir);
    if let Some(extension) = &runtime.extension {
        set_if_absent("PI_STAR_AGENT_EXTENSION", extension);
    }
    tracing::debug!(
        dir = %runtime.dir.display(),
        node = %runtime.node.display(),
        pi = %runtime.cli_js.display(),
        "using bundled pi runtime"
    );
}

/// The `pi` invocation used by RPC, export and other CLI helpers.
pub fn command() -> PiCommand {
    if let Some(cli) = std::env::var_os("PI_STAR_PI_CLI") {
        return PiCommand {
            program: PathBuf::from(cli),
            prefix: Vec::new(),
        };
    }
    if let (Some(node), Some(js)) = (
        std::env::var_os("PI_STAR_PI_NODE"),
        std::env::var_os("PI_STAR_PI_JS"),
    ) {
        return PiCommand {
            program: PathBuf::from(node),
            prefix: vec![OsString::from(js)],
        };
    }
    if let Some(runtime) = resolve() {
        return PiCommand {
            program: runtime.node,
            prefix: vec![runtime.cli_js.into_os_string()],
        };
    }
    PiCommand {
        program: PathBuf::from("pi"),
        prefix: Vec::new(),
    }
}

pub fn extension_path() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os("PI_STAR_AGENT_EXTENSION") {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Some(path);
        }
    }
    resolve().and_then(|runtime| runtime.extension)
}

fn set_if_absent(name: &str, value: &Path) {
    if std::env::var_os(name).is_none() {
        // SAFETY: called once from `main` before any worker thread is started.
        unsafe { std::env::set_var(name, value) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_layout_is_recognized() {
        let root = std::env::temp_dir().join(format!(
            "pistar-runtime-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("pi/dist/bundle")).unwrap();
        std::fs::create_dir_all(root.join("agent-extension")).unwrap();
        std::fs::write(root.join(node_name()), b"").unwrap();
        std::fs::write(root.join("pi/package.json"), "{}").unwrap();
        std::fs::write(root.join("pi/dist/bundle/cli.js"), "").unwrap();
        std::fs::write(root.join("agent-extension/index.ts"), "").unwrap();

        let runtime = from_dir(&root).expect("runtime");
        assert_eq!(runtime.node, root.join(node_name()));
        assert_eq!(runtime.cli_js, root.join("pi/dist/bundle/cli.js"));
        assert!(runtime.extension.is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn incomplete_layout_is_rejected() {
        let root = std::env::temp_dir().join(format!(
            "pistar-runtime-missing-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(node_name()), b"").unwrap();
        assert!(from_dir(&root).is_none());
        let _ = std::fs::remove_dir_all(root);
    }
}
