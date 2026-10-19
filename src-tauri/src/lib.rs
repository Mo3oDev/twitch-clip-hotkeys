use std::collections::HashSet;
use std::sync::{Arc, Mutex as StdMutex};

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
use state::{AppState, RealHelixOps, AUTH_BASE_URL, HELIX_BASE_URL, KEYRING_SERVICE};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let loaded_config = commands::load_config();
    let client_id = loaded_config.client_id.clone();
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
    let http = reqwest::Client::new();

    let ops = Arc::new(RealHelixOps::new(
        client_id.clone().unwrap_or_default(),
        http.clone(),
        tokens.clone(),
    ));

    let app_state = AppState {
        http: http.clone(),
        client_id: StdMutex::new(client_id.clone()),
        tokens: tokens.clone(),
        user_login: user_login.clone(),
        session: session.clone(),
        bindings: bindings.clone(),
    };

    let setup_bindings = bindings.clone();

    tauri::Builder::default()
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
        ])
        .setup(move |app| {
            tray::build_tray(app.handle())?;

            let results = hotkeys::register_all(app.handle(), &setup_bindings.lock().unwrap());
            for (id, result) in results {
                if let Err(e) = result {
                    eprintln!("hotkey {id} failed to register: {e:?}");
                }
            }

            // Fire-and-forget: refresh the cached display login name for a
            // session restored from Credential Manager. Cosmetic only — the
            // session's broadcaster/editor id already came from the cached
            // token, so clip creation works even if this never resolves.
            if let Some(client_id) = client_id.clone() {
                let tokens = tokens.clone();
                let user_login = user_login.clone();
                let http = http.clone();
                tauri::async_runtime::spawn(async move {
                    let mut guard = tokens.lock().await;
                    if let Some(tok) = guard.as_mut() {
                        let client = twitch::HelixClient {
                            http,
                            base_url: HELIX_BASE_URL.to_string(),
                            client_id,
                            auth_base_url: AUTH_BASE_URL.to_string(),
                        };
                        if let Ok(user) = twitch::users::get_current_user(&client, tok).await {
                            *user_login.lock().unwrap() = Some(user.login);
                        }
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
