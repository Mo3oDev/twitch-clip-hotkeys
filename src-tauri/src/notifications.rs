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
            &format!("Clip created ({duration_seconds}s)"),
            &format!("Exact {duration_seconds}s clip ready on your channel."),
        ),
        ClipOutcome::FallbackSuccess { .. } => content(
            "Clip created (approximate duration)",
            "Used the fallback method; the exact duration couldn't be guaranteed. Review/edit the clip if you want to adjust it.",
        ),
        ClipOutcome::Offline => content(
            "Clip not created",
            "The stream is offline, can't create a clip.",
        ),
        ClipOutcome::Failed(_) => content(
            "Couldn't create the clip",
            "Connection error with Twitch. Try again in a few seconds.",
        ),
        ClipOutcome::StillProcessing { .. } => content(
            "Clip processing",
            "Twitch is still processing the clip; it may take a bit longer to show up.",
        ),
    }
}

pub fn message_not_connected() -> NotificationContent {
    content(
        "Twitch not connected",
        "Open Settings and connect your account to use clips.",
    )
}

pub fn message_session_expired() -> NotificationContent {
    content(
        "Reconnection needed",
        "Your Twitch session expired. Open Settings to reconnect.",
    )
}

/// Shown exactly once, ever (persisted via `AppConfig.tray_hint_shown`),
/// the first time the user closes the settings window — explains that
/// closing it only hides the window rather than quitting, which a first
/// run can otherwise read as "the app won't close."
pub fn message_tray_hint() -> NotificationContent {
    content(
        "Minimized to tray",
        "Twitch Clip Hotkeys is still active. Right-click the icon to quit.",
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
        assert_eq!(content.title, "Minimized to tray");
        assert_eq!(
            content.body,
            "Twitch Clip Hotkeys is still active. Right-click the icon to quit."
        );
    }

    fn clip() -> ClipRef {
        ClipRef {
            id: "c1".to_string(),
            edit_url: "https://www.twitch.tv/twitchdev/clip/c1/edit".to_string(),
        }
    }

    #[test]
    fn vod_success_message() {
        let outcome = ClipOutcome::VodSuccess {
            clip: clip(),
            duration_seconds: 30,
        };
        let content = super::message_for_outcome(&outcome);
        assert_eq!(content.title, "Clip created (30s)");
        assert_eq!(content.body, "Exact 30s clip ready on your channel.");
    }

    #[test]
    fn fallback_success_message() {
        let outcome = ClipOutcome::FallbackSuccess { clip: clip() };
        let content = super::message_for_outcome(&outcome);
        assert_eq!(content.title, "Clip created (approximate duration)");
        assert_eq!(
            content.body,
            "Used the fallback method; the exact duration couldn't be guaranteed. Review/edit the clip if you want to adjust it."
        );
    }

    #[test]
    fn offline_message() {
        let content = super::message_for_outcome(&ClipOutcome::Offline);
        assert_eq!(content.title, "Clip not created");
        assert_eq!(content.body, "The stream is offline, can't create a clip.");
    }

    #[test]
    fn failed_message() {
        let content = super::message_for_outcome(&ClipOutcome::Failed("boom".to_string()));
        assert_eq!(content.title, "Couldn't create the clip");
        assert_eq!(
            content.body,
            "Connection error with Twitch. Try again in a few seconds."
        );
    }

    #[test]
    fn still_processing_message() {
        let content = super::message_for_outcome(&ClipOutcome::StillProcessing {
            clip_id: "c1".to_string(),
        });
        assert_eq!(content.title, "Clip processing");
        assert_eq!(
            content.body,
            "Twitch is still processing the clip; it may take a bit longer to show up."
        );
    }

    #[test]
    fn not_connected_message() {
        let content = super::message_not_connected();
        assert_eq!(content.title, "Twitch not connected");
        assert_eq!(
            content.body,
            "Open Settings and connect your account to use clips."
        );
    }

    #[test]
    fn session_expired_message() {
        let content = super::message_session_expired();
        assert_eq!(content.title, "Reconnection needed");
        assert_eq!(
            content.body,
            "Your Twitch session expired. Open Settings to reconnect."
        );
    }
}
