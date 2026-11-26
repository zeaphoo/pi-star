//! 渲染器看门狗：主窗口的页面卡死时自动重建，而不是等用户报告。
//!
//! `window::marker_script` 在页面主线程上每秒 POST 一次
//! `/api/desktop/alive?client=main`。主线程被死循环占住时心跳停止，即使
//! `pi-star` 服务和原生窗口仍然活着、也不会有 JS 异常可上报。这里从桌面壳
//! 轮询心跳年龄，超时后销毁并重建主窗口（新窗口 = 新的 WebContent 进程）。
//!
//! 只在主窗口可见、未最小化且获得焦点时判定，避免 WebKit 对后台窗口的定时器
//! 节流造成误杀；重建后有一段时间的启动宽限和冷却。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::desktop;
use crate::supervisor::Supervisor;
use crate::window::MAIN;

/// 与 `window::marker_script` 里注入的 client 名一致。
const CLIENT: &str = "main";
const POLL: Duration = Duration::from_secs(2);
/// 心跳间隔 1 秒；留出 GC/调度抖动，超过这个年龄判定主线程卡住。
const STALE: Duration = Duration::from_secs(8);
/// 新窗口/重新可见后的宽限期：页面还没开始心跳时不要误判。
const STARTUP_GRACE: Duration = Duration::from_secs(20);
/// 重建后至少等这么久再做下一次判定。
const RELOAD_GRACE: Duration = Duration::from_secs(20);
/// 连续重建次数上限；超过后改为长冷却，避免无限重启。
const MAX_RELOADS: u32 = 3;
const COOLDOWN: Duration = Duration::from_secs(120);

pub fn start(app: AppHandle, supervisor: Arc<Supervisor>, port: u16, data_root: PathBuf) {
    std::thread::spawn(move || watch(app, supervisor, port, data_root));
}

fn watch(app: AppHandle, supervisor: Arc<Supervisor>, port: u16, data_root: PathBuf) {
    let mut watched_since: Option<Instant> = None;
    let mut last_reload: Option<Instant> = None;
    let mut reloads = 0u32;

    loop {
        std::thread::sleep(POLL);

        let Some(window) = app.get_webview_window(MAIN) else {
            // 托盘常驻时主窗口本来就不存在；只重置宽限，不重置重建计数，
            // 否则每次重建后的空窗期都会把连续次数清零。
            watched_since = None;
            continue;
        };
        let watched = window.is_visible().unwrap_or(false)
            && !window.is_minimized().unwrap_or(false)
            && window.is_focused().unwrap_or(false);
        if !watched {
            // 后台/隐藏窗口会被 WebKit 节流甚至暂停定时器，不能算卡死。
            watched_since = None;
            continue;
        }

        let since = *watched_since.get_or_insert_with(Instant::now);
        if since.elapsed() < STARTUP_GRACE {
            continue;
        }
        if let Some(at) = last_reload {
            let cooldown = if reloads >= MAX_RELOADS {
                COOLDOWN
            } else {
                RELOAD_GRACE
            };
            if at.elapsed() < cooldown {
                continue;
            }
        }

        match heartbeat_age_ms(port) {
            Some(age) if age <= STALE.as_millis() as u64 => {
                if reloads > 0 {
                    desktop::log(
                        &data_root,
                        &format!("渲染器心跳恢复（age={age}ms），看门狗计数清零"),
                    );
                }
                reloads = 0;
            }
            age => {
                reloads += 1;
                let detail = match age {
                    Some(age) => format!("{age}ms 未收到心跳"),
                    None => "从未收到心跳".to_string(),
                };
                desktop::log(
                    &data_root,
                    &format!("渲染器无响应：{detail}，重建主窗口（连续第 {reloads} 次）"),
                );
                last_reload = Some(Instant::now());
                watched_since = None;
                rebuild(&app, &supervisor);
            }
        }
    }
}

/// 销毁主窗口后稍等事件循环处理完，再让监督器按当前状态重开控制台。
///
/// 不能只 `eval("location.reload()")`：那同样要等在被占住的渲染器主线程上，
/// 卡死时永远不会执行；销毁窗口会让 WebKit 结束旧的 WebContent 进程。
fn rebuild(app: &AppHandle, supervisor: &Arc<Supervisor>) {
    let handle = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window(MAIN) {
            let _ = window.destroy();
        }
    }) {
        eprintln!("看门狗无法销毁主窗口：{error}");
        return;
    }

    // `destroy` 通过事件队列派发；留一拍让窗口真正消失，否则 open_console
    // 只会把旧窗口提到前面。
    std::thread::sleep(Duration::from_millis(800));

    let handle = app.clone();
    let sup = Arc::clone(supervisor);
    if let Err(error) = app.run_on_main_thread(move || {
        if handle.get_webview_window(MAIN).is_none() {
            sup.open_console(&handle);
        }
    }) {
        eprintln!("看门狗无法重开主窗口：{error}");
    }
}

/// `GET /api/desktop/alive` 的 `ageMs`；请求失败或没有心跳时返回 `None`。
fn heartbeat_age_ms(port: u16) -> Option<u64> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr.into(), Duration::from_millis(500)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .ok()?;
    stream
        .set_write_timeout(Some(Duration::from_millis(500)))
        .ok()?;
    write!(
        stream,
        "GET /api/desktop/alive?client={CLIENT} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    let body = response.split("\r\n\r\n").nth(1)?.trim();
    parse_age(body)
}

fn parse_age(body: &str) -> Option<u64> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value.get("ageMs").and_then(serde_json::Value::as_u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_age_is_optional_until_the_first_beat() {
        assert_eq!(
            parse_age(r#"{"client":"main","alive":false,"ageMs":null}"#),
            None
        );
        assert_eq!(
            parse_age(r#"{"client":"main","alive":true,"ageMs":123}"#),
            Some(123)
        );
        assert_eq!(parse_age("not json"), None);
    }
}
