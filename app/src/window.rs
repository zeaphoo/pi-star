//! 窗口编排：内置 splash、主窗口、状态注入、Dock 行为与提示窗口。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tauri::webview::{NewWindowFeatures, NewWindowResponse, PageLoadEvent};
use tauri::{
    AppHandle, LogicalSize, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
    Wry,
};

use crate::supervisor::{FailureKind, StatusView, Supervisor};

pub const SPLASH: &str = "splash";
pub const MAIN: &str = "main";
pub const SPLASH_SIZE: (f64, f64) = (520.0, 380.0);
pub const MAIN_SIZE: (f64, f64) = (1280.0, 840.0);
pub const MIN_MAIN_SIZE: (f64, f64) = (900.0, 600.0);

/// Unique labels for windows opened by `window.open`.
static POPUP_SEQ: AtomicUsize = AtomicUsize::new(0);

/// 注入到 pi-star 页面的只读桌面标记。不包含任何 Tauri command 能力。
fn marker_script() -> String {
    format!(
        r#"(function(){{var m={{version:1,desktop:true,platform:"{platform}",shell:"tauri"}};try{{Object.defineProperty(window,"__PI_STAR__",{{value:Object.freeze(m),writable:false,configurable:false}});}}catch(e){{window.__PI_STAR__=Object.freeze(m);}}}})();"#,
        platform = std::env::consts::OS
    )
}

pub fn ensure_splash(app: &AppHandle, supervisor: &Arc<Supervisor>) {
    if let Some(window) = app.get_webview_window(SPLASH) {
        let _ = window.show();
        let _ = window.set_focus();
        supervisor.apply_status_to(&window);
        return;
    }

    let sup = Arc::clone(supervisor);
    let Ok(window) = WebviewWindowBuilder::new(app, SPLASH, WebviewUrl::App("index.html".into()))
        .title("Pi Star")
        .inner_size(SPLASH_SIZE.0, SPLASH_SIZE.1)
        .resizable(false)
        .on_page_load(move |window, _payload| sup.apply_status_to(&window))
        .build()
    else {
        return;
    };
    let _ = window.set_size(LogicalSize::new(SPLASH_SIZE.0, SPLASH_SIZE.1));
    let _ = window.center();
    supervisor.apply_status_to(&window);
}

pub fn open_main(app: &AppHandle, supervisor: &Arc<Supervisor>, target: &str) {
    if let Some(window) = app.get_webview_window(MAIN) {
        bring_to_front(&window);
        return;
    }

    let Ok(url) = target.parse::<tauri::Url>() else {
        supervisor.fail(
            app,
            FailureKind::LoadTimeout,
            format!("控制台地址非法：{target}"),
        );
        return;
    };

    let sup = Arc::clone(supervisor);
    let popup_app = app.clone();
    let popup_origin = url.clone();
    let built = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::External(url))
        .title("Pi Star")
        .inner_size(MAIN_SIZE.0, MAIN_SIZE.1)
        .min_inner_size(MIN_MAIN_SIZE.0, MIN_MAIN_SIZE.1)
        .visible(false)
        .initialization_script(marker_script())
        .on_new_window(move |url, features| {
            handle_new_window(&popup_app, &popup_origin, url, features)
        })
        .on_page_load(move |window, payload| {
            if payload.event() == PageLoadEvent::Finished {
                sup.mark_main_ready();
                let _ = window.show();
                let _ = window.set_focus();
                if let Some(splash) = window.app_handle().get_webview_window(SPLASH) {
                    let _ = splash.close();
                }
            }
        })
        .build();

    let window = match built {
        Ok(window) => window,
        Err(error) => {
            supervisor.fail(
                app,
                FailureKind::LoadTimeout,
                format!("创建主窗口失败：{error}"),
            );
            return;
        }
    };
    let _ = window.set_size(LogicalSize::new(MAIN_SIZE.0, MAIN_SIZE.1));
    let _ = window.center();
    sync_dock_presence(app, true);
    {
        let handle = window.app_handle().clone();
        window.on_window_event(move |event| {
            if matches!(event, WindowEvent::Destroyed) {
                sync_dock_presence(&handle, false);
            }
        });
    }

    let sup = Arc::clone(supervisor);
    let handle = app.clone();
    let log_path = supervisor.log_path();
    let remaining = supervisor.startup_remaining();
    std::thread::spawn(move || {
        if !remaining.is_zero() {
            std::thread::sleep(remaining);
        }
        if !sup.main_ready() {
            sup.fail(
                &handle,
                FailureKind::LoadTimeout,
                format!(
                    "主窗口在启动期限（{} 秒）内没有加载完成。\n日志：{}",
                    remaining.as_secs().max(1),
                    log_path.display()
                ),
            );
        }
    });
}

/// Create a native window for same-origin `window.open` requests. External
/// links go to the system browser; they never get a Tauri webview.
fn handle_new_window(
    app: &AppHandle,
    origin: &tauri::Url,
    url: tauri::Url,
    features: NewWindowFeatures,
) -> NewWindowResponse<Wry> {
    if url.origin() == origin.origin() {
        let label = format!(
            "pi-star-window-{}",
            POPUP_SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let nested_app = app.clone();
        let nested_origin = origin.clone();
        let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
            .title("Pi Star")
            .inner_size(1200.0, 800.0)
            .window_features(features)
            .initialization_script(marker_script())
            .on_document_title_changed(|window, title| {
                let _ = window.set_title(&title);
            })
            .on_new_window(move |url, features| {
                handle_new_window(&nested_app, &nested_origin, url, features)
            });
        match builder.build() {
            Ok(window) => NewWindowResponse::Create { window },
            Err(error) => {
                eprintln!("创建新窗口失败：{error}");
                NewWindowResponse::Deny
            }
        }
    } else {
        open_external(&url);
        NewWindowResponse::Deny
    }
}

fn open_external(url: &tauri::Url) {
    if !matches!(url.scheme(), "http" | "https" | "mailto") {
        return;
    }
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(url.as_str());
        command
    };
    #[cfg(windows)]
    let mut command = {
        let mut command = std::process::Command::new("cmd");
        command.arg("/C").arg("start").arg("").arg(url.as_str());
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(url.as_str());
        command
    };
    let _ = command.spawn();
}

/// Close every console window (main and popups) but keep splash/notice.
pub fn close_console_windows(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label != SPLASH && label != "notice" {
            let _ = window.close();
        }
    }
}

pub fn bring_to_front(window: &WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        let _ = window.unminimize();
    }
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn inject_status_to_splash(app: &AppHandle, supervisor: &Arc<Supervisor>) {
    if let Some(window) = app.get_webview_window(SPLASH) {
        supervisor.apply_status_to(&window);
    }
}

pub fn inject_status(window: &WebviewWindow, view: &StatusView) {
    let status = json_string(&view.status);
    let hint = json_option(&view.hint);
    let detail = json_option(&view.detail);
    let log_path = json_option(&view.log_path);
    let _ = window.eval(format!(
        "window.pistarSetStatus && window.pistarSetStatus({status}, {hint}, {detail}, {}, {log_path});",
        if view.failed { "true" } else { "false" },
    ));
}

/// 偏好类操作失败时的可见提示：复用 splash 页，不引入对话框依赖。
pub fn show_notice(app: &AppHandle, title: &str, detail: &str) {
    const NOTICE: &str = "notice";
    let view = StatusView {
        status: title.to_string(),
        hint: None,
        detail: Some(detail.to_string()),
        failed: false,
        log_path: None,
    };

    if let Some(window) = app.get_webview_window(NOTICE) {
        let _ = window.show();
        let _ = window.set_focus();
        inject_status(&window, &view);
        return;
    }

    let title_for_load = title.to_string();
    let detail_for_load = detail.to_string();
    let Ok(window) = WebviewWindowBuilder::new(app, NOTICE, WebviewUrl::App("index.html".into()))
        .title("Pi Star")
        .inner_size(SPLASH_SIZE.0, SPLASH_SIZE.1)
        .resizable(false)
        .on_page_load(move |window, _payload| {
            inject_status(
                &window,
                &StatusView {
                    status: title_for_load.clone(),
                    hint: None,
                    detail: Some(detail_for_load.clone()),
                    failed: false,
                    log_path: None,
                },
            );
        })
        .build()
    else {
        return;
    };
    let _ = window.set_size(LogicalSize::new(SPLASH_SIZE.0, SPLASH_SIZE.1));
    let _ = window.center();
    inject_status(&window, &view);
}

pub fn show_fatal(app: &AppHandle, message: &str) {
    let view = StatusView {
        status: "启动失败".to_string(),
        hint: None,
        detail: Some(message.to_string()),
        failed: true,
        log_path: None,
    };
    let for_load = view.clone();
    let Ok(window) = WebviewWindowBuilder::new(app, SPLASH, WebviewUrl::App("index.html".into()))
        .title("Pi Star")
        .inner_size(SPLASH_SIZE.0, SPLASH_SIZE.1)
        .resizable(false)
        .on_page_load(move |window, _payload| inject_status(&window, &for_load))
        .build()
    else {
        return;
    };
    let _ = window.set_size(LogicalSize::new(SPLASH_SIZE.0, SPLASH_SIZE.1));
    let _ = window.center();
    inject_status(&window, &view);
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn json_option(value: &Option<String>) -> String {
    match value {
        Some(value) => serde_json::to_string(value).unwrap_or_else(|_| "null".to_string()),
        None => "null".to_string(),
    }
}

#[cfg(target_os = "macos")]
pub fn sync_dock_presence(app: &AppHandle, present: bool) {
    let policy = if present {
        tauri::ActivationPolicy::Regular
    } else {
        tauri::ActivationPolicy::Accessory
    };
    if let Err(error) = app.set_activation_policy(policy) {
        eprintln!("切换 Dock 图标形态失败：{error}");
    }
}

#[cfg(not(target_os = "macos"))]
pub fn sync_dock_presence(_app: &AppHandle, _present: bool) {}
