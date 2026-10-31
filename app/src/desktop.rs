//! 桌面偏好的执行层：系统登录项、关窗策略与托盘勾选项。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::menu::CheckMenuItem;
use tauri::{AppHandle, Wry};
use tauri_plugin_autostart::ManagerExt;

use crate::prefs::Prefs;

pub const LOG_FILE: &str = "desktop.log";

#[derive(Default)]
struct Items {
    autostart: Option<CheckMenuItem<Wry>>,
    close_to_tray: Option<CheckMenuItem<Wry>>,
}

pub struct Settings {
    app: AppHandle,
    data_root: PathBuf,
    prefs: Mutex<Prefs>,
    items: Mutex<Items>,
}

impl Settings {
    pub fn new(app: AppHandle, data_root: PathBuf, prefs: Prefs) -> Self {
        Self {
            app,
            data_root,
            prefs: Mutex::new(prefs),
            items: Mutex::new(Items::default()),
        }
    }

    pub fn attach_items(&self, autostart: CheckMenuItem<Wry>, close_to_tray: CheckMenuItem<Wry>) {
        let mut items = self.lock_items();
        items.autostart = Some(autostart);
        items.close_to_tray = Some(close_to_tray);
    }

    pub fn autostart(&self) -> bool {
        self.lock_prefs().autostart()
    }

    pub fn close_to_tray(&self) -> bool {
        self.lock_prefs().close_to_tray()
    }

    pub fn port(&self) -> u16 {
        self.lock_prefs().port()
    }

    /// 启动对账：按偏好把系统登录项拉回一致（幂等）。
    pub fn reconcile(&self) {
        let desired = self.autostart();
        let result = if desired {
            self.app.autolaunch().enable()
        } else {
            self.app.autolaunch().disable()
        };
        if let Err(error) = result {
            let message = format!("无法应用开机自启动偏好：{error}");
            log(&self.data_root, &message);
        }
    }

    pub fn set_autostart(&self, on: bool) -> Result<(), String> {
        let close_to_tray = self.close_to_tray();
        let port = self.port();
        self.apply(on, close_to_tray, port)
    }

    pub fn set_close_to_tray(&self, on: bool) -> Result<(), String> {
        let autostart = self.autostart();
        let port = self.port();
        self.apply(autostart, on, port)
    }

    fn apply(&self, autostart: bool, close_to_tray: bool, port: u16) -> Result<(), String> {
        let before = self.lock_prefs().clone();

        // ① 系统登录项：失败则整体回滚，保证 UI 只有“全成功 / 全失败”。
        let result = if autostart {
            self.app.autolaunch().enable()
        } else {
            self.app.autolaunch().disable()
        };
        if let Err(error) = result {
            let mut rolled = before.clone();
            if let Ok(actual) = self.app.autolaunch().is_enabled() {
                rolled.set_autostart(actual);
            }
            self.sync_items(&rolled);
            let message = format!("无法调整开机自启动：{error}");
            log(&self.data_root, &message);
            return Err(message);
        }

        // ② 以系统读回为准，再落盘。
        let actual_autostart = self.app.autolaunch().is_enabled().unwrap_or(autostart);
        let mut next = before.clone();
        next.set_autostart(actual_autostart);
        next.set_close_to_tray(close_to_tray);
        next.set_port(port);

        if let Err(error) = next.save(&self.data_root) {
            self.sync_items(&before);
            log(&self.data_root, &error);
            return Err(error);
        }

        *self.lock_prefs() = next.clone();
        self.sync_items(&next);
        Ok(())
    }

    fn sync_items(&self, prefs: &Prefs) {
        let items = self.lock_items();
        if let Some(item) = &items.autostart {
            let _ = item.set_checked(prefs.autostart());
        }
        if let Some(item) = &items.close_to_tray {
            let _ = item.set_checked(prefs.close_to_tray());
        }
    }

    fn lock_prefs(&self) -> std::sync::MutexGuard<'_, Prefs> {
        self.prefs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn lock_items(&self) -> std::sync::MutexGuard<'_, Items> {
        self.items
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

pub fn log(data_root: &Path, message: &str) {
    use std::io::Write;
    let path = data_root.join(LOG_FILE);
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let _ = writeln!(file, "{message}");
}
