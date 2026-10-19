use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotkeyBinding {
    pub id: String,
    pub modifiers: Vec<String>,
    pub code: String,
    pub duration_seconds: u8,
    pub label: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub client_id: Option<String>,
    pub hotkey_bindings: Vec<HotkeyBinding>,
    /// Whether `notifications::message_tray_hint` has ever been shown.
    /// `#[serde(default)]` so a `config.json` written before this field
    /// existed still deserializes (as `false`, correctly showing the hint
    /// once more) instead of failing to load entirely.
    #[serde(default)]
    pub tray_hint_shown: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValidationError {
    DurationOutOfRange(String),
    DuplicateCombo(String, String),
    NoModifier(String),
}

pub fn validate_bindings(bindings: &[HotkeyBinding]) -> Result<(), ValidationError> {
    for binding in bindings {
        if !(5..=60).contains(&binding.duration_seconds) {
            return Err(ValidationError::DurationOutOfRange(binding.id.clone()));
        }
    }

    let mut seen: std::collections::HashMap<(Vec<String>, String), String> =
        std::collections::HashMap::new();
    for binding in bindings {
        let mut modifiers = binding.modifiers.clone();
        modifiers.sort();
        let key = (modifiers, binding.code.clone());
        if let Some(existing_id) = seen.get(&key) {
            return Err(ValidationError::DuplicateCombo(
                existing_id.clone(),
                binding.id.clone(),
            ));
        }
        seen.insert(key, binding.id.clone());
    }

    for binding in bindings {
        if binding.modifiers.is_empty() {
            return Err(ValidationError::NoModifier(binding.id.clone()));
        }
    }

    Ok(())
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
    fn rejects_duration_below_5() {
        let bindings = vec![binding("a", &["Control"], "Digit1", 4)];
        assert_eq!(
            validate_bindings(&bindings),
            Err(ValidationError::DurationOutOfRange("a".to_string()))
        );
    }

    #[test]
    fn rejects_duration_above_60() {
        let bindings = vec![binding("a", &["Control"], "Digit1", 61)];
        assert_eq!(
            validate_bindings(&bindings),
            Err(ValidationError::DurationOutOfRange("a".to_string()))
        );
    }

    #[test]
    fn rejects_duplicate_combo() {
        let bindings = vec![
            binding("a", &["Control", "Alt"], "Digit1", 30),
            binding("b", &["Alt", "Control"], "Digit1", 45),
        ];
        assert_eq!(
            validate_bindings(&bindings),
            Err(ValidationError::DuplicateCombo("a".to_string(), "b".to_string()))
        );
    }

    #[test]
    fn rejects_binding_with_no_modifier() {
        let bindings = vec![binding("a", &[], "Digit1", 30)];
        assert_eq!(
            validate_bindings(&bindings),
            Err(ValidationError::NoModifier("a".to_string()))
        );
    }

    #[test]
    fn accepts_valid_distinct_bindings() {
        let bindings = vec![
            binding("a", &["Control", "Alt"], "Digit1", 30),
            binding("b", &["Control", "Alt"], "Digit2", 45),
            binding("c", &["Control", "Alt"], "Digit3", 60),
        ];
        assert_eq!(validate_bindings(&bindings), Ok(()));
    }
}
