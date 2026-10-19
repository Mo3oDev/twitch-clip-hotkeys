use std::path::PathBuf;
use tauri::Manager;

pub fn config_dir(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .expect("app_config_dir should resolve on a running Tauri app")
}
