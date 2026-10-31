//! 桌面壳偏好：`<data_root>/desktop.toml`。
//!
//! 只有桌面壳写这个文件；缺失、缺字段或损坏时回落到默认值并原子物化写回。

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::config::DEFAULT_PORT;

/// 偏好文件名（位于 app data）。
pub const FILE: &str = "desktop.toml";
/// 文件结构版本，仅用于将来的迁移。
const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prefs {
    pub autostart: Option<bool>,
    pub close_to_tray: Option<bool>,
    pub port: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct FileShape {
    #[serde(default)]
    autostart: Option<bool>,
    #[serde(default)]
    close_to_tray: Option<bool>,
    #[serde(default)]
    port: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    Ok,
    Missing,
    Partial,
    Unreadable,
}

pub struct Loaded {
    pub prefs: Prefs,
    pub state: LoadState,
}

impl Prefs {
    pub fn load(data_root: &Path) -> Loaded {
        let Ok(text) = std::fs::read_to_string(path(data_root)) else {
            return Loaded {
                prefs: Prefs::default(),
                state: LoadState::Missing,
            };
        };
        let Ok(shape) = toml::from_str::<FileShape>(&text) else {
            return Loaded {
                prefs: Prefs::default(),
                state: LoadState::Unreadable,
            };
        };
        let state =
            if shape.autostart.is_none() || shape.close_to_tray.is_none() || shape.port.is_none() {
                LoadState::Partial
            } else {
                LoadState::Ok
            };
        Loaded {
            prefs: Prefs {
                autostart: shape.autostart,
                close_to_tray: shape.close_to_tray,
                port: shape.port,
            },
            state,
        }
    }

    /// 开机自启动默认关闭：桌面壳不主动注册登录项。
    pub fn autostart(&self) -> bool {
        self.autostart.unwrap_or(false)
    }

    /// 关闭窗口默认留在托盘，避免误伤正在运行的 Agent。
    pub fn close_to_tray(&self) -> bool {
        self.close_to_tray.unwrap_or(true)
    }

    /// 桌面端口。0 不是有效值（会变成随机 origin）；非法值回落默认值。
    pub fn port(&self) -> u16 {
        self.port.filter(|port| *port > 0).unwrap_or(DEFAULT_PORT)
    }

    pub fn set_autostart(&mut self, on: bool) {
        self.autostart = Some(on);
    }

    pub fn set_close_to_tray(&mut self, on: bool) {
        self.close_to_tray = Some(on);
    }

    pub fn set_port(&mut self, port: u16) {
        self.port = Some(port);
    }

    /// 把生效值原子写入 `desktop.toml`。
    pub fn save(&self, data_root: &Path) -> Result<(), String> {
        let target = path(data_root);
        let temp = data_root.join(format!("{FILE}.tmp"));
        let text = format!(
            "# 由 Pi Star 桌面壳维护；手工修改端口后需重启应用生效。\n\
             # 修改 port 会改变 webview 的 origin，主题/侧边栏/登录状态会重置。\n\
             schema = {SCHEMA}\n\
             autostart = {}\n\
             close_to_tray = {}\n\
             port = {}\n",
            self.autostart(),
            self.close_to_tray(),
            self.port()
        );
        std::fs::write(&temp, text)
            .map_err(|error| format!("写入 {} 失败：{error}", temp.display()))?;
        if let Err(error) = std::fs::rename(&temp, &target) {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("提交 {} 失败：{error}", target.display()));
        }
        Ok(())
    }
}

fn path(data_root: &Path) -> PathBuf {
    data_root.join(FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pistar-prefs-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create");
        dir
    }

    #[test]
    fn missing_uses_defaults() {
        let root = temp_root("missing");
        let loaded = Prefs::load(&root);
        assert_eq!(loaded.state, LoadState::Missing);
        assert!(!loaded.prefs.autostart());
        assert!(loaded.prefs.close_to_tray());
        assert_eq!(loaded.prefs.port(), DEFAULT_PORT);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_falls_back() {
        let root = temp_root("corrupt");
        std::fs::write(root.join(FILE), "autostart = [oops\n").unwrap();
        let loaded = Prefs::load(&root);
        assert_eq!(loaded.state, LoadState::Unreadable);
        assert!(!loaded.prefs.autostart());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn partial_is_reported_and_keeps_present_values() {
        let root = temp_root("partial");
        std::fs::write(root.join(FILE), "autostart = true\nport = 30152\n").unwrap();
        let loaded = Prefs::load(&root);
        assert_eq!(loaded.state, LoadState::Partial);
        assert!(loaded.prefs.autostart());
        assert_eq!(loaded.prefs.port(), 30152);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn zero_port_falls_back_to_default() {
        let root = temp_root("zero");
        std::fs::write(
            root.join(FILE),
            "autostart = false\nclose_to_tray = true\nport = 0\n",
        )
        .unwrap();
        let loaded = Prefs::load(&root);
        assert_eq!(loaded.state, LoadState::Ok);
        assert_eq!(loaded.prefs.port(), DEFAULT_PORT);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn save_roundtrips_without_temp_file() {
        let root = temp_root("save");
        let mut prefs = Prefs::default();
        prefs.set_autostart(true);
        prefs.set_close_to_tray(false);
        prefs.set_port(30155);
        prefs.save(&root).expect("save");
        let loaded = Prefs::load(&root);
        assert_eq!(loaded.state, LoadState::Ok);
        assert_eq!(loaded.prefs, prefs);
        assert!(!root.join(format!("{FILE}.tmp")).exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
