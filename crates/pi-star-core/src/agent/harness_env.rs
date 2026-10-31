//! `ExecutionEnv` wrapper that enforces pi-star's allowed roots.
//!
//! The merged harness runs the same built-in tools as the legacy runtime, but
//! those tools otherwise talk to `OsExecutionEnv` directly. This wrapper
//! authorizes every addressed path (after resolving symlinks) against
//! [`AllowedRoots`], mirroring the HTTP file APIs. The OS temp dir stays
//! reachable because tools legitimately stage large outputs there.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use pi_star_tools::env::{
    AppendFile, ExecutionEnv, FileContent, FileInfo, FileSystem, Shell, ShellExecOptions,
    ShellOutput,
};
use pi_star_tools::error::{ExecutionError, FileError, FileErrorCode};
use pi_star_tools::file_mutation_queue::{MutatingEnv, MutationQueueRegistry};
use pi_star_tools::os_env::OsExecutionEnv;

use crate::file_access::{AllowedRoots, resolve_existing};

pub struct AllowedEnv {
    inner: OsExecutionEnv,
    roots: Arc<AllowedRoots>,
    /// Temp paths this env created. Tools stage large outputs there and read
    /// them back; everything else under the OS temp dir stays denied.
    temp_paths: parking_lot::Mutex<HashSet<PathBuf>>,
}

impl AllowedEnv {
    pub fn new(cwd: impl Into<PathBuf>, roots: Arc<AllowedRoots>) -> Self {
        Self {
            inner: OsExecutionEnv::with_cwd(cwd.into()),
            roots,
            temp_paths: parking_lot::Mutex::new(HashSet::new()),
        }
    }

    fn is_allowed(&self, path: &Path) -> bool {
        let resolved = resolve_existing(path);
        if self.roots.is_allowed_existing(&resolved) {
            return true;
        }
        let temp = self.temp_paths.lock();
        temp.contains(&resolved) || temp.iter().any(|root| resolved.starts_with(root))
    }

    fn remember_temp(&self, path: &Path) {
        self.temp_paths.lock().insert(resolve_existing(path));
    }

    async fn authorize(
        &self,
        path: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<PathBuf, FileError> {
        let absolute = self.inner.absolute_path(path, cancel).await?;
        if self.is_allowed(&absolute) {
            Ok(absolute)
        } else {
            Err(FileError::new(
                FileErrorCode::PermissionDenied,
                "path is outside the allowed roots",
            )
            .with_path(path))
        }
    }

    fn text_path(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }
}

#[async_trait]
impl FileSystem for AllowedEnv {
    fn cwd(&self) -> &Path {
        self.inner.cwd()
    }

    async fn absolute_path(
        &self,
        path: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<PathBuf, FileError> {
        self.inner.absolute_path(path, cancel).await
    }

    async fn join_path(
        &self,
        parts: &[&str],
        cancel: Option<&CancellationToken>,
    ) -> Result<PathBuf, FileError> {
        self.inner.join_path(parts, cancel).await
    }

    async fn read_text_file(
        &self,
        path: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<String, FileError> {
        let path = self.authorize(path, cancel).await?;
        self.inner
            .read_text_file(&Self::text_path(&path), cancel)
            .await
    }

    async fn read_text_lines(
        &self,
        path: &str,
        max_lines: Option<usize>,
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<String>, FileError> {
        let path = self.authorize(path, cancel).await?;
        self.inner
            .read_text_lines(&Self::text_path(&path), max_lines, cancel)
            .await
    }

    async fn read_binary_file(
        &self,
        path: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<u8>, FileError> {
        let path = self.authorize(path, cancel).await?;
        self.inner
            .read_binary_file(&Self::text_path(&path), cancel)
            .await
    }

    async fn write_file(
        &self,
        path: &str,
        content: FileContent,
        cancel: Option<&CancellationToken>,
    ) -> Result<(), FileError> {
        let path = self.authorize(path, cancel).await?;
        self.inner
            .write_file(&Self::text_path(&path), content, cancel)
            .await
    }

    async fn append_file(
        &self,
        path: &str,
        content: FileContent,
        cancel: Option<&CancellationToken>,
    ) -> Result<(), FileError> {
        let path = self.authorize(path, cancel).await?;
        self.inner
            .append_file(&Self::text_path(&path), content, cancel)
            .await
    }

    async fn open_append_file(&self, path: &str) -> Result<Option<Box<dyn AppendFile>>, FileError> {
        let path = self.authorize(path, None).await?;
        self.inner.open_append_file(&Self::text_path(&path)).await
    }

    async fn truncate_file(
        &self,
        path: &str,
        len: u64,
        cancel: Option<&CancellationToken>,
    ) -> Result<(), FileError> {
        let path = self.authorize(path, cancel).await?;
        self.inner
            .truncate_file(&Self::text_path(&path), len, cancel)
            .await
    }

    async fn rename_file(
        &self,
        source: &str,
        dest: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<(), FileError> {
        let source = self.authorize(source, cancel).await?;
        let dest = self.authorize(dest, cancel).await?;
        self.inner
            .rename_file(&Self::text_path(&source), &Self::text_path(&dest), cancel)
            .await
    }

    async fn file_info(
        &self,
        path: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<FileInfo, FileError> {
        let path = self.authorize(path, cancel).await?;
        self.inner.file_info(&Self::text_path(&path), cancel).await
    }

    async fn list_dir(
        &self,
        path: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<FileInfo>, FileError> {
        let path = self.authorize(path, cancel).await?;
        self.inner.list_dir(&Self::text_path(&path), cancel).await
    }

    async fn canonical_path(
        &self,
        path: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<PathBuf, FileError> {
        let path = self.authorize(path, cancel).await?;
        let canonical = self
            .inner
            .canonical_path(&Self::text_path(&path), cancel)
            .await?;
        if self.is_allowed(&canonical) {
            Ok(canonical)
        } else {
            Err(FileError::new(
                FileErrorCode::PermissionDenied,
                "path is outside the allowed roots",
            )
            .with_path(path.display().to_string()))
        }
    }

    async fn exists(
        &self,
        path: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<bool, FileError> {
        let path = self.authorize(path, cancel).await?;
        self.inner.exists(&Self::text_path(&path), cancel).await
    }

    async fn create_dir(
        &self,
        path: &str,
        recursive: bool,
        cancel: Option<&CancellationToken>,
    ) -> Result<(), FileError> {
        let path = self.authorize(path, cancel).await?;
        self.inner
            .create_dir(&Self::text_path(&path), recursive, cancel)
            .await
    }

    async fn remove(
        &self,
        path: &str,
        recursive: bool,
        force: bool,
        cancel: Option<&CancellationToken>,
    ) -> Result<(), FileError> {
        let path = self.authorize(path, cancel).await?;
        self.inner
            .remove(&Self::text_path(&path), recursive, force, cancel)
            .await
    }

    async fn create_temp_dir(
        &self,
        prefix: Option<&str>,
        cancel: Option<&CancellationToken>,
    ) -> Result<PathBuf, FileError> {
        let path = self.inner.create_temp_dir(prefix, cancel).await?;
        self.remember_temp(&path);
        Ok(path)
    }

    async fn create_temp_file(
        &self,
        prefix: &str,
        suffix: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<PathBuf, FileError> {
        let path = self.inner.create_temp_file(prefix, suffix, cancel).await?;
        self.remember_temp(&path);
        Ok(path)
    }

    async fn cleanup(&self) {
        <OsExecutionEnv as FileSystem>::cleanup(&self.inner).await
    }
}

#[async_trait]
impl Shell for AllowedEnv {
    async fn exec<'a>(
        &'a self,
        command: &str,
        options: ShellExecOptions<'a>,
    ) -> Result<ShellOutput, ExecutionError> {
        self.inner.exec(command, options).await
    }

    async fn cleanup(&self) {
        <OsExecutionEnv as Shell>::cleanup(&self.inner).await
    }
}

impl ExecutionEnv for AllowedEnv {}

impl MutatingEnv for AllowedEnv {
    fn as_env(&self) -> &dyn ExecutionEnv {
        self
    }

    fn mutation_registry(&self) -> &MutationQueueRegistry {
        // `OsExecutionEnv` owns the per-path queue registry; reuse it so the
        // mutation order is the same as when tools talk to the OS env directly.
        <OsExecutionEnv as MutatingEnv>::mutation_registry(&self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pi_star_tools::env::FileKind;

    #[tokio::test]
    async fn denies_paths_outside_the_allowed_roots() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        let outside = temp.path().join("outside.txt");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&outside, "secret").unwrap();
        let roots = Arc::new(AllowedRoots::new());
        roots.add(&root);
        let env = AllowedEnv::new(&root, roots);

        let denied = env
            .read_text_file(outside.to_string_lossy().as_ref(), None)
            .await
            .unwrap_err();
        assert_eq!(denied.code, FileErrorCode::PermissionDenied);

        let write_denied = env
            .write_file(
                outside.to_string_lossy().as_ref(),
                FileContent::Text("overwrite".into()),
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(write_denied.code, FileErrorCode::PermissionDenied);
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "secret");
    }

    #[tokio::test]
    async fn allows_reads_writes_and_listing_inside_a_root() {
        let temp = tempfile::tempdir().unwrap();
        let roots = Arc::new(AllowedRoots::new());
        roots.add(temp.path());
        let env = AllowedEnv::new(temp.path(), roots);

        env.write_file("a.txt", FileContent::Text("hello".into()), None)
            .await
            .unwrap();
        assert_eq!(env.read_text_file("a.txt", None).await.unwrap(), "hello");
        let listing = env.list_dir(".", None).await.unwrap();
        assert!(listing.iter().any(|entry| entry.name == "a.txt"));
        assert!(matches!(
            env.file_info("a.txt", None).await.unwrap().kind,
            FileKind::File
        ));
    }

    #[tokio::test]
    async fn denies_renames_across_the_roots() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        let outside = temp.path().join("outside.txt");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.txt"), "a").unwrap();
        let roots = Arc::new(AllowedRoots::new());
        roots.add(&root);
        let env = AllowedEnv::new(&root, roots);
        let error = env
            .rename_file("a.txt", outside.to_string_lossy().as_ref(), None)
            .await
            .unwrap_err();
        assert_eq!(error.code, FileErrorCode::PermissionDenied);
        assert!(root.join("a.txt").exists());
    }
}
