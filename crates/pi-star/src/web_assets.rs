//! 前端静态资源：release 内置进二进制，debug 读磁盘，`--ui-dir` 可显式覆盖。
//!
//! 资源来源只有两个：
//! - `Embedded`：`rust-embed` 在 release 编译期把 `ui/dist` 编进二进制；
//! - `Disk`：debug 默认目录，或用户通过 `--ui-dir` 指定的目录。
//!
//! 路由语义与原来的 `ServeDir + ServeFile` 保持一致：精确命中优先，未命中回退到
//! `index.html`；`/api` 的 404 由外层路由处理，不会走到这里。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

use crate::state::AppState;

#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../ui/dist"]
// 干净检出（尚未跑 `npm run build`）时目录不存在：不阻断编译。release 启动时会
// 显式检查文件数并报错；debug 则允许先启动 API，由桌面壳/页面提示缺少 UI。
#[allow_missing = true]
struct UiAssets;

/// 静态资源来源。
#[allow(dead_code)] // `Embedded` 只在 release 构造。
#[derive(Debug, Clone)]
enum Source {
    Disk(PathBuf),
    Embedded,
}

/// 可克隆的静态资源句柄。
#[derive(Debug, Clone)]
pub struct WebAssets {
    source: Source,
    files: usize,
}

/// 供 `/api/health` 与启动事件使用的来源描述。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Descriptor {
    pub source: &'static str,
    pub path: Option<String>,
    pub files: usize,
}

impl WebAssets {
    /// 解析资源来源。`override_dir` 来自 `--ui-dir` / `PI_STAR_UI_DIR`。
    pub fn resolve(override_dir: Option<&Path>) -> Result<Self, String> {
        if let Some(dir) = override_dir {
            let dir = absolute(dir);
            if !dir.join("index.html").is_file() {
                return Err(format!(
                    "--ui-dir 指定的目录缺少 index.html：{}",
                    dir.display()
                ));
            }
            let files = count_files(&dir);
            return Ok(Self {
                source: Source::Disk(dir),
                files,
            });
        }

        #[cfg(not(debug_assertions))]
        {
            let files = UiAssets::iter().count();
            if files == 0 {
                return Err(
                    "二进制没有内置前端资源：请先在 ui/ 执行 `npm run build`，再重新编译 pi-star"
                        .to_string(),
                );
            }
            Ok(Self {
                source: Source::Embedded,
                files,
            })
        }

        #[cfg(debug_assertions)]
        {
            let dir = absolute(Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../ui/dist"
            )));
            if !dir.join("index.html").is_file() {
                tracing::warn!(
                    path = %dir.display(),
                    "frontend build assets not found; serving API only"
                );
            }
            let files = count_files(&dir);
            Ok(Self {
                source: Source::Disk(dir),
                files,
            })
        }
    }

    pub fn describe(&self) -> Descriptor {
        match &self.source {
            Source::Disk(dir) => Descriptor {
                source: "disk",
                path: Some(dir.display().to_string()),
                files: self.files,
            },
            Source::Embedded => Descriptor {
                source: "embedded",
                path: None,
                files: self.files,
            },
        }
    }

    async fn read(&self, path: &str) -> Option<Vec<u8>> {
        match &self.source {
            // 资源都很小；异步读避免阻塞 Tokio worker。
            Source::Disk(dir) => tokio::fs::read(dir.join(path)).await.ok(),
            Source::Embedded => UiAssets::get(path).map(|file| file.data.into_owned()),
        }
    }
}

/// 静态资源 fallback handler。`get()` 同时响应 HEAD，Axum 会自动去掉响应体。
pub async fn serve(State(state): State<Arc<AppState>>, uri: Uri) -> Response {
    let Some(requested) = request_path(uri.path()) else {
        return not_found();
    };

    let (bytes, served_path) = match state.web_assets.read(&requested).await {
        Some(bytes) => (bytes, requested),
        None => match state.web_assets.read("index.html").await {
            Some(index) => (index, "index.html".to_string()),
            None => return not_found(),
        },
    };

    let mime = mime_guess::from_path(&served_path).first_or_octet_stream();
    let length = bytes.len();
    let mut response = Response::new(Body::from(bytes));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref())
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string())
            .unwrap_or_else(|_| HeaderValue::from_static("0")),
    );
    response
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "not found",
    )
        .into_response()
}

/// 把 URL path 转成资源相对路径，并拒绝路径穿越。
fn request_path(path: &str) -> Option<String> {
    let requested = path.trim_start_matches('/');
    if requested.is_empty() {
        return Some("index.html".to_string());
    }
    if requested
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return None;
    }
    if requested.contains('\\') {
        return None;
    }
    Some(requested.to_string())
}

fn absolute(path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    std::fs::canonicalize(&joined).unwrap_or(joined)
}

/// 统计目录下的文件数，仅用于健康自诊断。
fn count_files(root: &Path) -> usize {
    let mut total = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                total += 1;
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_path_normalizes_root() {
        assert_eq!(request_path("/"), Some("index.html".to_string()));
        assert_eq!(request_path("/login"), Some("login".to_string()));
        assert_eq!(
            request_path("/assets/app.js"),
            Some("assets/app.js".to_string())
        );
    }

    #[test]
    fn request_path_rejects_traversal() {
        assert_eq!(request_path("/../Cargo.toml"), None);
        assert_eq!(request_path("/assets/../../secret"), None);
        assert_eq!(request_path("/assets\\..\\secret"), None);
        assert_eq!(request_path("/assets//app.js"), None);
    }

    #[test]
    fn explicit_directory_must_have_index() {
        let temp = tempfile::tempdir().unwrap();
        let error = WebAssets::resolve(Some(temp.path())).expect_err("missing index");
        assert!(error.contains("index.html"), "{error}");
    }

    #[test]
    fn explicit_directory_is_reported_as_disk() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("index.html"), "hi").unwrap();
        std::fs::create_dir_all(temp.path().join("assets")).unwrap();
        std::fs::write(temp.path().join("assets/app.js"), "js").unwrap();

        let assets = WebAssets::resolve(Some(temp.path())).expect("resolve");
        let descriptor = assets.describe();
        assert_eq!(descriptor.source, "disk");
        assert_eq!(descriptor.files, 2);
        assert!(descriptor.path.is_some());
    }
}
