use std::path::PathBuf;

/// Context-free by design: `lib.rs` needs the config (`client_id`,
/// hotkey bindings) to build the global-shortcut plugin, which must be
/// registered on the `Builder` *before* an `AppHandle` exists — so this
/// can't go through `tauri::Manager::path().app_config_dir()`. Matches the
/// spec's documented `%APPDATA%/<identifier>/config.json` layout directly.
pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").expect("APPDATA should be set on Windows");
    PathBuf::from(base).join("com.twitchcliphotkeys.desktop")
}
