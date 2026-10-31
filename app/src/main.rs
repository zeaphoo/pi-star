//! Pi Star 桌面壳入口：单实例、偏好、托盘、监督器与窗口。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod config;
mod desktop;
mod prefs;
mod supervisor;
mod window;

use std::sync::Arc;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

use config::AppConfig;
use desktop::Settings;
use prefs::{LoadState, Prefs};
use supervisor::{Supervisor, TRAY_ID};

fn main() {
    supervisor::install_signal_handlers();

    tauri::Builder::default()
        // 单实例必须最先注册：第二个实例在派生 pi-star 之前退出。
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if argv.iter().any(|arg| arg == "--hidden") {
                return;
            }
            if let Some(supervisor) = app.try_state::<Arc<Supervisor>>() {
                supervisor.open_console(app);
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .invoke_handler(tauri::generate_handler![commands::desktop_action])
        .setup(|app| {
            let handle = app.handle().clone();

            let data_root = match handle.path().app_data_dir() {
                Ok(path) => path,
                Err(error) => {
                    window::show_fatal(&handle, &format!("无法解析应用数据目录：{error}"));
                    return Ok(());
                }
            };
            if let Err(error) = std::fs::create_dir_all(&data_root) {
                window::show_fatal(
                    &handle,
                    &format!("创建数据目录 {} 失败：{error}", data_root.display()),
                );
                return Ok(());
            }

            let loaded = Prefs::load(&data_root);
            if loaded.state != LoadState::Ok {
                if let Err(error) = loaded.prefs.save(&data_root) {
                    desktop::log(&data_root, &format!("写入桌面偏好失败：{error}"));
                }
            }

            let config = match AppConfig::resolve(&handle, &loaded.prefs) {
                Ok(config) => config,
                Err(error) => {
                    window::show_fatal(&handle, &error);
                    return Ok(());
                }
            };

            let settings = Arc::new(Settings::new(
                handle.clone(),
                config.data_root.clone(),
                loaded.prefs,
            ));
            app.manage(Arc::clone(&settings));
            settings.reconcile();

            let resource_dir = handle.path().resource_dir().ok();
            let bin = config::locate_pi_star(resource_dir.as_deref());

            let status_item =
                MenuItem::with_id(app, "status", "状态：启动中…", false, None::<&str>)?;
            let open_item = MenuItem::with_id(app, "open", "打开 Pi Star", true, None::<&str>)?;
            let restart_item =
                MenuItem::with_id(app, "restart", "重启 Pi Star", true, None::<&str>)?;
            let autostart_item = CheckMenuItem::with_id(
                app,
                "autostart",
                "开机自启动",
                true,
                settings.autostart(),
                None::<&str>,
            )?;
            let close_to_tray_item = CheckMenuItem::with_id(
                app,
                "close-to-tray",
                "关闭窗口时最小化到托盘",
                true,
                settings.close_to_tray(),
                None::<&str>,
            )?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &status_item,
                    &PredefinedMenuItem::separator(app)?,
                    &open_item,
                    &restart_item,
                    &PredefinedMenuItem::separator(app)?,
                    &autostart_item,
                    &close_to_tray_item,
                    &PredefinedMenuItem::separator(app)?,
                    &quit_item,
                ],
            )?;
            settings.attach_items(autostart_item.clone(), close_to_tray_item.clone());

            let supervisor = Supervisor::new(config, bin, status_item);
            app.manage(Arc::clone(&supervisor));
            window::sync_dock_presence(&handle, false);

            let tray_supervisor = Arc::clone(&supervisor);
            let tray_settings = Arc::clone(&settings);
            TrayIconBuilder::with_id(TRAY_ID)
                .icon(tauri::include_image!("icons/32x32.png"))
                .tooltip("Pi Star")
                .menu(&menu)
                .show_menu_on_left_click(!cfg!(target_os = "windows"))
                .on_tray_icon_event(|tray, event| {
                    #[cfg(target_os = "windows")]
                    if matches!(
                        event,
                        tauri::tray::TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } | tauri::tray::TrayIconEvent::DoubleClick {
                            button: tauri::tray::MouseButton::Left,
                            ..
                        }
                    ) {
                        let app = tray.app_handle();
                        if let Some(supervisor) = app.try_state::<Arc<Supervisor>>() {
                            supervisor.open_console(app);
                        }
                    }
                    #[cfg(not(target_os = "windows"))]
                    let _ = (tray, event);
                })
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "open" => tray_supervisor.open_console(app),
                    "restart" => tray_supervisor.restart(app),
                    "autostart" => {
                        let intent = autostart_item.is_checked().unwrap_or(false);
                        if let Err(error) = tray_settings.set_autostart(intent) {
                            let _ = autostart_item.set_checked(!intent);
                            window::show_notice(app, "开机自启动未生效", &error);
                        }
                    }
                    "close-to-tray" => {
                        let intent = close_to_tray_item.is_checked().unwrap_or(false);
                        if let Err(error) = tray_settings.set_close_to_tray(intent) {
                            let _ = close_to_tray_item.set_checked(!intent);
                            window::show_notice(app, "关闭行为未生效", &error);
                        }
                    }
                    "quit" => {
                        tray_supervisor.shutdown();
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            supervisor.start(&handle);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { code, api, .. } => {
                let stay = code.is_none()
                    && app
                        .try_state::<Arc<Settings>>()
                        .is_some_and(|settings| settings.close_to_tray())
                    && app.try_state::<Arc<Supervisor>>().is_some();
                if stay {
                    api.prevent_exit();
                }
            }
            tauri::RunEvent::Exit => {
                if let Some(supervisor) = app.try_state::<Arc<Supervisor>>() {
                    supervisor.shutdown();
                }
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => {
                if let Some(supervisor) = app.try_state::<Arc<Supervisor>>() {
                    supervisor.open_console(app);
                }
            }
            _ => {}
        });
}
