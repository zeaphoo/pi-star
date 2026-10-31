//! 仅内置于 splash 的桌面动作。主窗口没有 capability，调不到这里。

use std::path::Path;
use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::supervisor::Supervisor;

#[tauri::command]
pub fn desktop_action(app: AppHandle, action: String) -> Result<(), String> {
    match action.as_str() {
        "retry" => {
            if let Some(supervisor) = app.try_state::<Arc<Supervisor>>() {
                supervisor.restart(&app);
            }
            Ok(())
        }
        "quit" => {
            let supervisor = app
                .try_state::<Arc<Supervisor>>()
                .map(|state| Arc::clone(&state));
            let handle = app.clone();
            // 清理可能需要几秒，放到后台，避免阻塞 IPC。
            std::thread::spawn(move || {
                if let Some(supervisor) = supervisor {
                    supervisor.shutdown();
                }
                handle.exit(0);
            });
            Ok(())
        }
        "open-log" => {
            let Some(supervisor) = app.try_state::<Arc<Supervisor>>() else {
                return Err("监督器尚未初始化".to_string());
            };
            open_path(&supervisor.log_path())
        }
        other => Err(format!("未知桌面动作：{other}")),
    }
}

fn open_path(path: &Path) -> Result<(), String> {
    if !path.exists() {
        std::fs::write(path, b"").map_err(|error| format!("创建日志失败：{error}"))?;
    }

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(path);
        command
    };
    #[cfg(windows)]
    let mut command = {
        let mut command = std::process::Command::new("cmd");
        command.arg("/C").arg("start").arg("").arg(path);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path);
        command
    };

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("打开 {} 失败：{error}", path.display()))
}
