use std::collections::HashSet;
use std::sync::{Arc, Mutex as StdMutex};

use tauri::Manager;
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Mutex as TokioMutex;

mod auth;
mod clip_flow;
mod commands;
mod config;
mod hotkeys;
mod notifications;
mod state;
mod tray;
mod twitch;

use hotkeys::ActiveSession;
use state::{AppState, RealHelixOps, KEYRING_SERVICE};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let loaded_config = commands::load_config();
    let client_id = Arc::new(StdMutex::new(loaded_config.client_id.clone()));
    let bindings = Arc::new(StdMutex::new(loaded_config.hotkey_bindings.clone()));

    let cached_tokens = auth::token_store::load_tokens(KEYRING_SERVICE)
        .ok()
        .flatten();
    let session = Arc::new(StdMutex::new(cached_tokens.as_ref().map(|t| ActiveSession {
        broadcaster_id: t.user_id.clone(),
        editor_id: t.user_id.clone(),
    })));
    let tokens = Arc::new(TokioMutex::new(cached_tokens));
    let user_login = Arc::new(StdMutex::new(None::<String>));
    let in_flight = Arc::new(TokioMutex::new(HashSet::new()));
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("reqwest client should build with a fixed timeout config");

    let ops = Arc::new(RealHelixOps::new(
        client_id.clone(),
        http.clone(),
        tokens.clone(),
    ));

    let hotkey_errors = Arc::new(StdMutex::new(Vec::new()));
    let tray_hint_shown = Arc::new(StdMutex::new(loaded_config.tray_hint_shown));

    let app_state = AppState {
        http: http.clone(),
        client_id: client_id.clone(),
        tokens: tokens.clone(),
        user_login: user_login.clone(),
        session: session.clone(),
        bindings: bindings.clone(),
        hotkey_errors: hotkey_errors.clone(),
    };

    let setup_bindings = bindings.clone();
    let login_ops = ops.clone();

    tauri::Builder::default()
        // Must be the first plugin registered: a second launch is detected
        // here and forwarded to this callback in the *first* instance
        // instead of starting a second process — without this, each launch
        // of the installed app (or `tauri dev`) silently ran as its own
        // independent background instance, each registering its own copy
        // of the same global hotkeys and racing to refresh the same token.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("settings") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            hotkeys::build_plugin(ops, session.clone(), bindings.clone(), in_flight).build(),
        )
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::auth_login,
            commands::auth_logout,
            commands::auth_status,
            commands::get_hotkeys,
            commands::save_hotkeys,
            commands::get_client_id,
            commands::set_client_id,
            commands::get_hotkey_errors,
        ])
        .setup(move |app| {
            tray::build_tray(app.handle())?;

            // The settings window is the app's only window; Tauri's default
            // is to quit when the last window closes, which would kill the
            // hotkey listener mid-stream. Hide it instead — the tray's
            // "Abrir ajustes" / left-click brings it back (`tray.rs`).
            if let Some(window) = app.get_webview_window("settings") {
                let app_handle = app.handle().clone();
                let tray_hint_shown = tray_hint_shown.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();

                        let mut shown = tray_hint_shown.lock().unwrap();
                        if !*shown {
                            *shown = true;
                            let content = notifications::message_tray_hint();
                            let _ = app_handle
                                .notification()
                                .builder()
                                .title(content.title)
                                .body(content.body)
                                .show();
                            let mut config = commands::load_config();
                            config.tray_hint_shown = true;
                            let _ = commands::save_config(&config);
                        }
                    }
                });
            }

            // Previously only `eprintln!`'d — invisible both because
            // nothing reads it and because a release build has no console
            // (`main.rs` sets `windows_subsystem = "windows"`). Stored in
            // `AppState` instead so Settings can show it via
            // `get_hotkey_errors` (Review Focus item 2: a reserved combo
            // must surface, not silently look like success).
            let results = hotkeys::register_all(app.handle(), &setup_bindings.lock().unwrap());
            let startup_errors: Vec<commands::HotkeyRegistrationError> = results
                .into_iter()
                .filter_map(|(id, r)| {
                    r.err()
                        .map(|e| commands::HotkeyRegistrationError { id, message: format!("{e:?}") })
                })
                .collect();
            *hotkey_errors.lock().unwrap() = startup_errors;

            // Fire-and-forget: refresh the cached display login name for a
            // session restored from Credential Manager. Cosmetic only — the
            // session's broadcaster/editor id already came from the cached
            // token, so clip creation works even if this never resolves.
            // Routed through `ops` rather than a one-off `HelixClient` so a
            // refresh triggered by this call (expired cached access token)
            // is persisted back to Credential Manager, not just applied
            // in-memory (see Task 11 Ruling I5 in the ledger).
            if client_id.lock().unwrap().is_some() {
                let user_login = user_login.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(login) = login_ops.fetch_login().await {
                        *user_login.lock().unwrap() = Some(login);
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
