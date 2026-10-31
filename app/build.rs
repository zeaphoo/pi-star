fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            // 生成 allow-desktop-action / deny-desktop-action，供 splash capability 使用。
            .app_manifest(tauri_build::AppManifest::new().commands(&["desktop_action"])),
    )
    .expect("运行 tauri-build 失败");
}
