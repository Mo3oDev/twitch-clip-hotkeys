use crate::clip_flow::ClipOutcome;

pub struct NotificationContent {
    pub title: String,
    pub body: String,
}

fn content(title: &str, body: &str) -> NotificationContent {
    NotificationContent {
        title: title.to_string(),
        body: body.to_string(),
    }
}

pub fn message_for_outcome(outcome: &ClipOutcome) -> NotificationContent {
    match outcome {
        ClipOutcome::VodSuccess {
            duration_seconds, ..
        } => content(
            &format!("Clip creado ({duration_seconds}s)"),
            &format!("Clip exacto de {duration_seconds}s listo en tu canal."),
        ),
        ClipOutcome::FallbackSuccess { .. } => content(
            "Clip creado (duración aproximada)",
            "Se usó el método alterno; la duración exacta no se pudo garantizar. Revisa/edita el clip si quieres ajustarlo.",
        ),
        ClipOutcome::Offline => content(
            "Clip no creado",
            "El stream está offline, no se puede clipear.",
        ),
        ClipOutcome::Failed(_) => content(
            "No se pudo crear el clip",
            "Error de conexión con Twitch. Intenta de nuevo en unos segundos.",
        ),
        ClipOutcome::StillProcessing { .. } => content(
            "Clip en proceso",
            "Twitch está procesando el clip; puede tardar un poco más en aparecer.",
        ),
    }
}

pub fn message_not_connected() -> NotificationContent {
    content(
        "Twitch no conectado",
        "Abre Ajustes y conecta tu cuenta para usar los clips.",
    )
}

pub fn message_session_expired() -> NotificationContent {
    content(
        "Reconexión necesaria",
        "Tu sesión de Twitch expiró. Abre Ajustes para reconectar.",
    )
}

/// Shown exactly once, ever (persisted via `AppConfig.tray_hint_shown`),
/// the first time the user closes the settings window — explains that
/// closing it only hides the window rather than quitting, which a first
/// run can otherwise read as "the app won't close."
pub fn message_tray_hint() -> NotificationContent {
    content(
        "Minimizado a la bandeja",
        "Twitch Clip Hotkeys sigue activo. Click derecho en el ícono para salir.",
    )
}

#[cfg(test)]
mod tests {
    use super::message_tray_hint;
    use crate::clip_flow::ClipOutcome;
    use crate::twitch::ClipRef;

    #[test]
    fn tray_hint_message() {
        let content = message_tray_hint();
        assert_eq!(content.title, "Minimizado a la bandeja");
        assert_eq!(
            content.body,
            "Twitch Clip Hotkeys sigue activo. Click derecho en el ícono para salir."
        );
    }

    fn clip() -> ClipRef {
        ClipRef {
            id: "c1".to_string(),
            edit_url: "https://www.twitch.tv/d4ni_jr/clip/c1/edit".to_string(),
        }
    }

    #[test]
    fn vod_success_message() {
        let outcome = ClipOutcome::VodSuccess {
            clip: clip(),
            duration_seconds: 30,
        };
        let content = super::message_for_outcome(&outcome);
        assert_eq!(content.title, "Clip creado (30s)");
        assert_eq!(content.body, "Clip exacto de 30s listo en tu canal.");
    }

    #[test]
    fn fallback_success_message() {
        let outcome = ClipOutcome::FallbackSuccess { clip: clip() };
        let content = super::message_for_outcome(&outcome);
        assert_eq!(content.title, "Clip creado (duración aproximada)");
        assert_eq!(
            content.body,
            "Se usó el método alterno; la duración exacta no se pudo garantizar. Revisa/edita el clip si quieres ajustarlo."
        );
    }

    #[test]
    fn offline_message() {
        let content = super::message_for_outcome(&ClipOutcome::Offline);
        assert_eq!(content.title, "Clip no creado");
        assert_eq!(content.body, "El stream está offline, no se puede clipear.");
    }

    #[test]
    fn failed_message() {
        let content = super::message_for_outcome(&ClipOutcome::Failed("boom".to_string()));
        assert_eq!(content.title, "No se pudo crear el clip");
        assert_eq!(
            content.body,
            "Error de conexión con Twitch. Intenta de nuevo en unos segundos."
        );
    }

    #[test]
    fn still_processing_message() {
        let content = super::message_for_outcome(&ClipOutcome::StillProcessing {
            clip_id: "c1".to_string(),
        });
        assert_eq!(content.title, "Clip en proceso");
        assert_eq!(
            content.body,
            "Twitch está procesando el clip; puede tardar un poco más en aparecer."
        );
    }

    #[test]
    fn not_connected_message() {
        let content = super::message_not_connected();
        assert_eq!(content.title, "Twitch no conectado");
        assert_eq!(
            content.body,
            "Abre Ajustes y conecta tu cuenta para usar los clips."
        );
    }

    #[test]
    fn session_expired_message() {
        let content = super::message_session_expired();
        assert_eq!(content.title, "Reconexión necesaria");
        assert_eq!(
            content.body,
            "Tu sesión de Twitch expiró. Abre Ajustes para reconectar."
        );
    }
}
