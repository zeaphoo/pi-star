//! 桌面壳配置：应用数据目录、稳定端口、pi-star 定位与健康确认。
//!
//! 端口是稳定 origin 的一部分：webview 的 localStorage、cookie 和缓存都以
//! `http://127.0.0.1:<port>` 为键，因此这里不在冲突时随机换端口。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use tauri::{AppHandle, Manager};

use crate::prefs::Prefs;

/// 桌面壳默认端口（与 pi-star 独立运行的默认值一致）。
pub const DEFAULT_PORT: u16 = 30151;
/// 健康响应的归属标识。
const PI_STAR_NAME: &str = "pi-star";
/// 单次健康请求的超时。
const HEALTH_TIMEOUT: Duration = Duration::from_millis(600);

#[derive(Debug, Clone)]
pub struct AppConfig {
    /// 桌面壳自己的数据目录（`desktop.toml`、日志）。
    pub data_root: PathBuf,
    /// 传给 pi-star 的监听端口。
    pub port: u16,
    /// 登录自启：成功后不主动弹窗。
    pub hidden: bool,
    /// 仅 debug：直接加载外部 URL（Vite），跳过 spawn 与监督。
    pub dev_url: Option<String>,
    /// 子进程意外退出时是否自动重启。
    pub restart_on_failure: bool,
}

impl AppConfig {
    pub fn resolve(app: &AppHandle, prefs: &Prefs) -> Result<Self, String> {
        let data_root = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("无法解析应用数据目录：{error}"))?;
        std::fs::create_dir_all(&data_root)
            .map_err(|error| format!("创建数据目录 {} 失败：{error}", data_root.display()))?;
        Ok(Self {
            data_root,
            port: prefs.port(),
            hidden: hidden_requested(),
            dev_url: dev_url(),
            restart_on_failure: true,
        })
    }

    pub fn log_path(&self) -> PathBuf {
        self.data_root.join("pi-star.log")
    }
}

/// 只认精确的 `--hidden` token（`--hidden=true` 不算）。
fn hidden_requested() -> bool {
    std::env::args_os().any(|arg| arg == "--hidden")
}

/// 仅 debug 构建支持 UI 热更新入口。
fn dev_url() -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var("PI_STAR_DESKTOP_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 定位 `pi-star` 可执行文件。
///
/// 发布构建只查应用同目录和 Tauri 资源目录，绝不查 `PATH`；调试构建额外查仓库
/// `target/{debug,release}` 与 `PATH`，方便 `cargo tauri dev`。
pub fn locate_pi_star(resource_dir: Option<&Path>) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().ok();
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    locate_in(
        exe.as_deref(),
        resource_dir,
        cfg!(debug_assertions),
        &path_dirs,
    )
}

fn locate_in(
    exe: Option<&Path>,
    resource_dir: Option<&Path>,
    dev: bool,
    path_dirs: &[PathBuf],
) -> Result<PathBuf, String> {
    let names = executable_names();
    let mut tried: Vec<String> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();

    if let Some(dir) = exe.and_then(Path::parent) {
        if let Some(found) = find_in(dir, &names, &mut tried, &mut seen) {
            return Ok(found);
        }
    }

    if let Some(dir) = resource_dir {
        if let Some(found) = find_in(dir, &names, &mut tried, &mut seen) {
            return Ok(found);
        }
    }

    if dev {
        if let Some(dir) = exe.and_then(Path::parent) {
            // app/target/<profile>/PiStarApp → 仓库根 target/<profile>/pi-star
            for profile in ["debug", "release"] {
                let candidate = dir
                    .join("..")
                    .join("..")
                    .join("..")
                    .join("target")
                    .join(profile);
                if let Some(found) = find_in(&candidate, &names, &mut tried, &mut seen) {
                    return Ok(found);
                }
            }
        }
        for dir in path_dirs {
            if let Some(found) = find_in(dir, &names, &mut tried, &mut seen) {
                return Ok(found);
            }
        }
        tried.push("PATH 各目录下的 pi-star".to_string());
    }

    Err(format!(
        "未找到 pi-star 可执行文件。已尝试：\n  - {}",
        tried.join("\n  - ")
    ))
}

fn find_in(
    dir: &Path,
    names: &[&'static str],
    tried: &mut Vec<String>,
    seen: &mut Vec<PathBuf>,
) -> Option<PathBuf> {
    for name in names {
        let candidate = dir.join(name);
        if seen.iter().any(|item| item == &candidate) {
            continue;
        }
        if candidate.is_file() {
            return Some(resolved(candidate));
        }
        seen.push(candidate.clone());
        tried.push(candidate.display().to_string());
    }
    None
}

fn resolved(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

fn executable_names() -> [&'static str; 2] {
    if cfg!(windows) {
        ["pi-star.exe", "pi-star"]
    } else {
        ["pi-star", "pi-star.exe"]
    }
}

/// 健康检查结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    /// 确认是 pi-star；`files` 为内置/磁盘 UI 文件数。
    Ready { files: usize },
    /// 连接失败或超时：子进程可能仍在启动。
    Unreachable,
    /// 有 HTTP 响应，但不是 pi-star（端口被其他程序占用）。
    Foreign { detail: String },
}

/// 对回环地址做一次原始 HTTP 健康检查。
pub fn check_health(addr: &str, password: Option<&str>) -> Health {
    let Ok(addr) = addr.parse::<SocketAddr>() else {
        return Health::Foreign {
            detail: format!("监听地址非法：{addr}"),
        };
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, HEALTH_TIMEOUT) else {
        return Health::Unreachable;
    };
    let _ = stream.set_read_timeout(Some(HEALTH_TIMEOUT));
    let _ = stream.set_write_timeout(Some(HEALTH_TIMEOUT));

    let auth = password
        .map(|password| {
            let raw = format!("pi:{password}");
            format!(
                "Authorization: Basic {}\r\n",
                base64::engine::general_purpose::STANDARD.encode(raw)
            )
        })
        .unwrap_or_default();
    let request =
        format!("GET /api/health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n{auth}\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return Health::Unreachable;
    }

    let mut response = Vec::new();
    // A slow/busy server is not a foreign server. Only a complete HTTP response
    // containing another app's health body is foreign; I/O errors/timeouts stay
    // retryable until the startup deadline.
    if stream.read_to_end(&mut response).is_err() {
        return Health::Unreachable;
    }
    let text = String::from_utf8_lossy(&response);
    let Some((head, body)) = text.split_once("\r\n\r\n") else {
        return Health::Unreachable;
    };
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        return Health::Foreign {
            detail: format!(
                "健康接口返回非 200：{}",
                head.lines().next().unwrap_or("unknown")
            ),
        };
    }
    if !body.contains(&format!("\"name\":\"{PI_STAR_NAME}\"")) {
        return Health::Foreign {
            detail: "端口由其他程序占用（健康响应不是 pi-star）".to_string(),
        };
    }
    Health::Ready {
        files: parse_asset_files(body),
    }
}

/// 从健康 JSON 中读取 `assets.files`。只做最小解析，避免为一次探测引入 JSON 依赖。
fn parse_asset_files(body: &str) -> usize {
    let Some(rest) = body.split("\"files\":").nth(1) else {
        return 0;
    };
    let digits: String = rest
        .trim_start()
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    digits.parse().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_tree(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pistar-locate-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp");
        dir
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent");
        std::fs::write(path, b"").expect("write");
    }

    #[test]
    fn exe_dir_wins() {
        let root = temp_tree("exe");
        let exe_dir = root.join("bin");
        let exe = exe_dir.join("PiStarApp");
        touch(&exe);
        touch(&exe_dir.join("pi-star"));
        let found = locate_in(Some(&exe), None, false, &[]).expect("found");
        assert_eq!(
            found,
            std::fs::canonicalize(exe_dir.join("pi-star")).unwrap()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resource_dir_is_checked_in_release() {
        let root = temp_tree("resource");
        let exe = root.join("usr/bin/PiStarApp");
        touch(&exe);
        let resource = root.join("usr/lib/PiStar");
        touch(&resource.join("pi-star"));
        let found = locate_in(Some(&exe), Some(&resource), false, &[]).expect("found");
        assert_eq!(
            found,
            std::fs::canonicalize(resource.join("pi-star")).unwrap()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn release_ignores_path() {
        let root = temp_tree("path");
        let exe = root.join("bin/PiStarApp");
        touch(&exe);
        let path_dir = root.join("path");
        touch(&path_dir.join("pi-star"));
        let error = locate_in(Some(&exe), None, false, &[path_dir]).expect_err("must fail");
        assert!(error.contains("未找到 pi-star"), "{error}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn debug_falls_back_to_repo_target() {
        let root = temp_tree("dev");
        let exe = root.join("app/target/debug/PiStarApp");
        touch(&exe);
        let repo_bin = root.join("target/debug/pi-star");
        touch(&repo_bin);
        let found = locate_in(Some(&exe), None, true, &[]).expect("found");
        assert_eq!(found, std::fs::canonicalize(repo_bin).unwrap());
        let _ = std::fs::remove_dir_all(root);
    }

    fn serve_once(response: &'static str) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = [0u8; 2048];
            let read = stream.read(&mut request).unwrap_or(0);
            let _ = stream.write_all(response.as_bytes());
            String::from_utf8_lossy(&request[..read]).to_string()
        });
        (addr.to_string(), handle)
    }

    #[test]
    fn health_check_recognizes_pi_star_and_sends_basic_auth() {
        let (addr, handle) = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"ok\":true,\"name\":\"pi-star\",\"assets\":{\"source\":\"disk\",\"files\":7}}",
        );
        let health = check_health(&addr, Some("secret"));
        assert_eq!(health, Health::Ready { files: 7 });
        let request = handle.join().expect("join");
        assert!(request.contains("GET /api/health HTTP/1.1"), "{request}");
        assert!(
            request.contains("Authorization: Basic cGk6c2VjcmV0"),
            "{request}"
        );
    }

    #[test]
    fn health_check_rejects_foreign_server() {
        let (addr, handle) = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n<html>other app</html>",
        );
        let health = check_health(&addr, None);
        assert!(matches!(health, Health::Foreign { .. }), "{health:?}");
        let _ = handle.join();
    }

    #[test]
    fn health_check_treats_incomplete_response_as_retryable() {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            // Close without a response (connection reset / empty body).
        });
        let health = check_health(&addr.to_string(), None);
        assert_eq!(health, Health::Unreachable);
        let _ = handle.join();
    }

    #[test]
    fn health_parser_reads_files() {
        assert_eq!(
            parse_asset_files(
                r#"{"ok":true,"name":"pi-star","assets":{"source":"disk","files":234}}"#
            ),
            234
        );
        assert_eq!(parse_asset_files(r#"{"ok":true}"#), 0);
        assert_eq!(
            parse_asset_files(r#"{"assets":{"source":"embedded","files":0}}"#),
            0
        );
    }
}
