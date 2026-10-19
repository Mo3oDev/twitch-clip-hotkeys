use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::clip_flow::{self, HelixOps};
use crate::config::HotkeyBinding;

#[derive(Debug, Clone, PartialEq)]
pub enum HotkeyError {
    UnknownCode(String),
    UnknownModifier(String),
    RegistrationFailed(String),
}

pub fn parse_shortcut(binding: &HotkeyBinding) -> Result<Shortcut, HotkeyError> {
    let code: Code = binding
        .code
        .parse()
        .map_err(|_| HotkeyError::UnknownCode(binding.code.clone()))?;

    let mut mods = Modifiers::empty();
    for modifier in &binding.modifiers {
        mods |= match modifier.as_str() {
            "Control" => Modifiers::CONTROL,
            "Alt" => Modifiers::ALT,
            "Shift" => Modifiers::SHIFT,
            "Super" | "Meta" => Modifiers::SUPER,
            other => return Err(HotkeyError::UnknownModifier(other.to_string())),
        };
    }

    Ok(Shortcut::new(Some(mods), code))
}

/// Unregisters every existing shortcut, then registers each binding
/// independently so one OS-level failure (e.g. a reserved combo) doesn't
/// block the others — the caller gets a per-binding result to surface.
pub fn register_all(
    app: &tauri::AppHandle,
    bindings: &[HotkeyBinding],
) -> Vec<(String, Result<(), HotkeyError>)> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();

    bindings
        .iter()
        .map(|binding| {
            let result = parse_shortcut(binding).and_then(|shortcut| {
                gs.register(shortcut)
                    .map_err(|e| HotkeyError::RegistrationFailed(e.to_string()))
            });
            (binding.id.clone(), result)
        })
        .collect()
}

/// Builds the `global-shortcut` plugin with its press handler already
/// attached. Task 11 passes the result to `.plugin(...)` at app setup —
/// the plugin's handler is fixed at `Builder::build()` time, so it cannot
/// be attached after the plugin is already registered. Dynamically
/// (re-)registering individual shortcuts afterward via `register_all` still
/// reaches this same handler.
///
/// Only `ShortcutState::Pressed` is handled (not `Released`, which would
/// double the dispatch), and the clip request itself runs in a spawned
/// task so the global keyboard hook is never blocked.
pub fn build_plugin<O, R>(
    ops: std::sync::Arc<O>,
    broadcaster_id: String,
    editor_id: String,
    bindings: std::sync::Arc<std::sync::Mutex<Vec<HotkeyBinding>>>,
    in_flight: std::sync::Arc<tokio::sync::Mutex<std::collections::HashSet<String>>>,
) -> tauri_plugin_global_shortcut::Builder<R>
where
    O: HelixOps + Send + Sync + 'static,
    R: tauri::Runtime,
{
    use tauri::Emitter;

    tauri_plugin_global_shortcut::Builder::new().with_handler(move |app, shortcut, event| {
        if event.state != ShortcutState::Pressed {
            return;
        }

        let found = {
            let guard = bindings.lock().unwrap();
            guard
                .iter()
                .find(|b| parse_shortcut(b).map(|s| &s == shortcut).unwrap_or(false))
                .cloned()
        };
        let Some(binding) = found else {
            return;
        };

        let ops = ops.clone();
        let broadcaster_id = broadcaster_id.clone();
        let editor_id = editor_id.clone();
        let in_flight = in_flight.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let outcome = clip_flow::execute_clip_request(
                ops.as_ref(),
                &broadcaster_id,
                &editor_id,
                &binding,
                chrono::Utc::now(),
                &in_flight,
            )
            .await;
            let _ = app.emit("clip:result", outcome);
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(modifiers: &[&str], code: &str) -> HotkeyBinding {
        HotkeyBinding {
            id: "b1".to_string(),
            modifiers: modifiers.iter().map(|m| m.to_string()).collect(),
            code: code.to_string(),
            duration_seconds: 30,
            label: "Clip 30s".to_string(),
        }
    }

    #[test]
    fn parse_shortcut_builds_expected_modifiers_and_code() {
        let shortcut = parse_shortcut(&binding(&["Control", "Alt"], "Digit1")).unwrap();

        assert!(shortcut.matches(Modifiers::CONTROL | Modifiers::ALT, Code::Digit1));
        assert!(!shortcut.matches(Modifiers::CONTROL, Code::Digit1));
        assert!(!shortcut.matches(Modifiers::CONTROL | Modifiers::ALT, Code::Digit2));
    }

    #[test]
    fn parse_shortcut_rejects_unknown_code() {
        let result = parse_shortcut(&binding(&["Control"], "NotARealKey"));

        assert_eq!(
            result,
            Err(HotkeyError::UnknownCode("NotARealKey".to_string()))
        );
    }
}
