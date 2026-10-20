use tauri::{AppHandle, Emitter, State};

use crate::auth::{self, device_flow};
use crate::config::{validate_bindings, AppConfig, HotkeyBinding};
use crate::hotkeys::{self, ActiveSession};
use crate::state::{AppState, AUTH_BASE_URL, HELIX_BASE_URL, KEYRING_SERVICE, SCOPES};
use crate::twitch::HelixClient;

#[derive(Debug, Clone, serde::Serialize)]
pub struct AuthStatus {
    pub connected: bool,
    pub login: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HotkeyRegistrationError {
    pub id: String,
    pub label: String,
    pub message: String,
}

/// `register_all`'s results are keyed by binding id (Task 9's tested
/// interface) — resolve it to the user-visible label here, once, so no
/// user-facing error (frontend or backend `Result<_, String>` text) ever
/// shows a raw `hk-<timestamp>-<rand>` id, which means nothing to someone
/// trying to find which row in Settings failed.
fn label_for<'a>(bindings: &'a [HotkeyBinding], id: &'a str) -> &'a str {
    bindings
        .iter()
        .find(|b| b.id == id)
        .map(|b| b.label.as_str())
        .unwrap_or(id)
}

pub fn validate_before_save(bindings: &[HotkeyBinding]) -> Result<(), String> {
    validate_bindings(bindings).map_err(|e| format!("{e:?}"))
}

pub fn config_path() -> std::path::PathBuf {
    crate::config::paths::config_dir().join("config.json")
}

pub fn load_config() -> AppConfig {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn auth_login(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    log::info!("auth_login: starting device code flow");
    let client_id = state
        .client_id
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "client_id not set".to_string())?;
    let http = state.http.clone();

    let device = device_flow::start_device_flow(&http, AUTH_BASE_URL, &client_id, SCOPES)
        .await
        .map_err(|e| {
            log::error!("auth_login: start_device_flow failed: {e:?}");
            format!("{e:?}")
        })?;

    let _ = app.emit("auth:user_code", &device.user_code);
    let _ = tauri_plugin_opener::open_url(device.verification_uri.clone(), None::<&str>);

    let poll_result = device_flow::poll_for_token(
        &http,
        AUTH_BASE_URL,
        &client_id,
        &device.device_code,
        device.interval,
        device.expires_in,
    )
    .await;

    match poll_result {
        Ok(mut tokens) => {
            let client = HelixClient {
                http: http.clone(),
                base_url: HELIX_BASE_URL.to_string(),
                client_id: client_id.clone(),
                auth_base_url: AUTH_BASE_URL.to_string(),
            };
            let user = crate::twitch::users::get_current_user(&client, &mut tokens)
                .await
                .map_err(|e| {
                    log::error!("auth_login: get_current_user failed after token poll: {e:?}");
                    format!("{e:?}")
                })?;
            tokens.user_id = user.id.clone();

            let _ = auth::token_store::save_tokens(KEYRING_SERVICE, &tokens);

            *state.tokens.lock().await = Some(tokens);
            *state.user_login.lock().unwrap() = Some(user.login.clone());
            *state.session.lock().unwrap() = Some(ActiveSession {
                broadcaster_id: user.id.clone(),
                editor_id: user.id,
            });

            log::info!("auth_login: connected as '{}'", user.login);
            let _ = app.emit("auth:success", &user.login);
            Ok(())
        }
        Err(e) => {
            log::warn!("auth_login: device code poll did not complete: {e:?}");
            let _ = app.emit("auth:timeout", format!("{e:?}"));
            Err(format!("{e:?}"))
        }
    }
}

#[tauri::command]
pub fn auth_logout(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("auth_logout: disconnecting");
    *state.user_login.lock().unwrap() = None;
    *state.session.lock().unwrap() = None;
    auth::token_store::clear_tokens(KEYRING_SERVICE).map_err(|e| format!("{e:?}"))
}

#[tauri::command]
pub fn auth_status(state: State<'_, AppState>) -> AuthStatus {
    let login = state.user_login.lock().unwrap().clone();
    AuthStatus {
        connected: state.session.lock().unwrap().is_some(),
        login,
    }
}

#[tauri::command]
pub fn get_hotkeys(state: State<'_, AppState>) -> Vec<HotkeyBinding> {
    state.bindings.lock().unwrap().clone()
}

#[tauri::command]
pub fn save_hotkeys(
    state: State<'_, AppState>,
    app: AppHandle,
    bindings: Vec<HotkeyBinding>,
) -> Result<(), String> {
    validate_before_save(&bindings)?;

    // Catch syntactically-invalid combos (unknown key code/modifier, e.g. a
    // binding half-edited in the UI) *before* persisting or touching the OS
    // registration, not after — `register_all`'s own per-binding result
    // only separately covers OS-level refusals (reserved combos) once the
    // binding is already syntactically well-formed.
    let unparseable: Vec<&str> = bindings
        .iter()
        .filter(|b| hotkeys::parse_shortcut(b).is_err())
        .map(|b| b.label.as_str())
        .collect();
    if !unparseable.is_empty() {
        log::warn!("save_hotkeys: rejected unparseable bindings: {}", unparseable.join(", "));
        return Err(format!(
            "hotkeys with an invalid key/modifier: {}",
            unparseable.join(", ")
        ));
    }

    let mut config = load_config();
    config.hotkey_bindings = bindings.clone();
    save_config(&config)?;

    *state.bindings.lock().unwrap() = bindings.clone();

    let results = hotkeys::register_all(&app, &bindings);
    store_hotkey_errors(&state, &bindings, &results);

    let failed: Vec<String> = results
        .iter()
        .filter_map(|(id, r)| {
            r.as_ref()
                .err()
                .map(|e| format!("{}: {e:?}", label_for(&bindings, id)))
        })
        .collect();
    if !failed.is_empty() {
        log::warn!("save_hotkeys: registration failures: {}", failed.join("; "));
        return Err(format!(
            "some hotkeys couldn't be registered: {}",
            failed.join("; ")
        ));
    }
    log::info!("save_hotkeys: saved and registered {} binding(s)", bindings.len());
    Ok(())
}

fn store_hotkey_errors(
    state: &State<'_, AppState>,
    bindings: &[HotkeyBinding],
    results: &[(String, Result<(), hotkeys::HotkeyError>)],
) {
    let errors: Vec<HotkeyRegistrationError> = results
        .iter()
        .filter_map(|(id, r)| {
            r.as_ref().err().map(|e| HotkeyRegistrationError {
                id: id.clone(),
                label: label_for(bindings, id).to_string(),
                message: format!("{e:?}"),
            })
        })
        .collect();
    *state.hotkey_errors.lock().unwrap() = errors;
}

#[tauri::command]
pub fn get_hotkey_errors(state: State<'_, AppState>) -> Vec<HotkeyRegistrationError> {
    state.hotkey_errors.lock().unwrap().clone()
}

#[tauri::command]
pub fn get_client_id(state: State<'_, AppState>) -> Option<String> {
    state.client_id.lock().unwrap().clone()
}

#[tauri::command]
pub fn set_client_id(state: State<'_, AppState>, id: String) -> Result<(), String> {
    *state.client_id.lock().unwrap() = Some(id.clone());
    let mut config = load_config();
    config.client_id = Some(id);
    save_config(&config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(id: &str, modifiers: &[&str], code: &str, duration_seconds: u8) -> HotkeyBinding {
        HotkeyBinding {
            id: id.to_string(),
            modifiers: modifiers.iter().map(|m| m.to_string()).collect(),
            code: code.to_string(),
            duration_seconds,
            label: format!("Clip {duration_seconds}s"),
        }
    }

    #[test]
    fn validate_before_save_rejects_invalid_bindings_without_touching_storage() {
        let bindings = vec![
            binding("a", &["Control", "Alt"], "Digit1", 30),
            binding("b", &["Alt", "Control"], "Digit1", 45),
        ];

        assert!(validate_before_save(&bindings).is_err());
    }
}
