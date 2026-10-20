use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use tokio::sync::Mutex;

use crate::config::HotkeyBinding;
use crate::twitch::{ClipRef, CreateClipFromVodRequest, HelixError, StreamInfo, VodInfo};

pub trait HelixOps {
    fn get_stream(
        &self,
        broadcaster_id: &str,
    ) -> impl std::future::Future<Output = Result<Option<StreamInfo>, HelixError>> + Send;
    fn get_current_vod(
        &self,
        broadcaster_id: &str,
    ) -> impl std::future::Future<Output = Result<Option<VodInfo>, HelixError>> + Send;
    fn create_clip_from_vod(
        &self,
        req: CreateClipFromVodRequest,
    ) -> impl std::future::Future<Output = Result<ClipRef, HelixError>> + Send;
    fn create_clip_classic(
        &self,
        broadcaster_id: &str,
    ) -> impl std::future::Future<Output = Result<ClipRef, HelixError>> + Send;
    fn get_clip(
        &self,
        clip_id: &str,
    ) -> impl std::future::Future<Output = Result<Option<ClipRef>, HelixError>> + Send;
    fn send_chat_message(
        &self,
        broadcaster_id: &str,
        message: &str,
    ) -> impl std::future::Future<Output = Result<(), HelixError>> + Send;
}

/// Two random words keep every clip title distinct from both the stream's
/// own title and from every other clip — Twitch doesn't generate one for
/// the classic path at all, and reusing `binding.label` ("Clip 30s") for
/// every single clip made titles useless for telling them apart later.
const TITLE_WORDS: &[&str] = &[
    "hunger", "ember", "frost", "nova", "quartz", "rogue", "storm", "echo", "pulse", "glitch",
    "raid", "surge", "vortex", "blitz", "omen", "rift", "talon", "zephyr", "cobalt", "drift",
    "point", "shadow", "flare", "ridge", "static", "tempo", "wraith", "crater", "haze", "signal",
];

fn generate_clip_title(duration_seconds: u8, rng: &mut impl rand::Rng) -> String {
    let a = TITLE_WORDS[rng.random_range(0..TITLE_WORDS.len())];
    let b = TITLE_WORDS[rng.random_range(0..TITLE_WORDS.len())];
    format!("{a}-{b}-{duration_seconds}s")
}

/// Best-effort: a failed chat post never changes the clip outcome the user
/// sees — the clip itself already succeeded, and chat is just an extra
/// announcement, not something worth surfacing as an error (e.g. missing
/// `user:write:chat` scope on an account that never re-connected after this
/// scope was added).
async fn post_clip_to_chat(ops: &impl HelixOps, broadcaster_id: &str, title: &str, clip: &ClipRef) {
    let message = format!("🎬 {title}: {}", clip.edit_url);
    if let Err(err) = ops.send_chat_message(broadcaster_id, &message).await {
        log::warn!("failed to post clip link to chat: {err:?}");
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", content = "data")]
pub enum ClipOutcome {
    VodSuccess { clip: ClipRef, duration_seconds: u8 },
    FallbackSuccess { clip: ClipRef },
    #[allow(dead_code)]
    StillProcessing { clip_id: String },
    Offline,
    Failed(String),
}

/// Rapid repeated presses of the same binding — a nervous double-tap during
/// a hype moment, not just literal overlap — used to be allowed the instant
/// the previous request *finished*, which for a fast VOD-path success could
/// be well under a second. A fixed cooldown measured from the *start* of
/// the last accepted press (using the same injected `now` as the VOD-offset
/// math, so this stays deterministically testable) also subsumes the old
/// in-flight check: a still-running request's start time is always within
/// the cooldown window of "now".
const CLIP_COOLDOWN: Duration = Duration::seconds(10);

pub async fn execute_clip_request(
    ops: &impl HelixOps,
    broadcaster_id: &str,
    editor_id: &str,
    binding: &HotkeyBinding,
    now: DateTime<Utc>,
    cooldowns: &Mutex<HashMap<String, DateTime<Utc>>>,
) -> ClipOutcome {
    {
        let mut guard = cooldowns.lock().await;
        if let Some(last) = guard.get(&binding.id) {
            if now.signed_duration_since(*last) < CLIP_COOLDOWN {
                return ClipOutcome::Failed("cooldown".to_string());
            }
        }
        guard.insert(binding.id.clone(), now);
    }

    execute_clip_request_inner(ops, broadcaster_id, editor_id, binding, now).await
}

async fn execute_clip_request_inner(
    ops: &impl HelixOps,
    broadcaster_id: &str,
    editor_id: &str,
    binding: &HotkeyBinding,
    now: DateTime<Utc>,
) -> ClipOutcome {
    let stream = match ops.get_stream(broadcaster_id).await {
        Ok(Some(stream)) => stream,
        Ok(None) => return ClipOutcome::Offline,
        Err(err) => return ClipOutcome::Failed(format!("{err:?}")),
    };

    let usable_vod_offset = usable_vod(ops, broadcaster_id, &stream, binding, now)
        .await;

    let title = generate_clip_title(binding.duration_seconds, &mut rand::rng());

    if let Some((vod, offset)) = usable_vod_offset {
        let req = CreateClipFromVodRequest {
            editor_id: editor_id.to_string(),
            broadcaster_id: broadcaster_id.to_string(),
            vod_id: vod.id,
            vod_offset: offset,
            duration_seconds: binding.duration_seconds,
            title: title.clone(),
        };
        if let Ok(clip) = ops.create_clip_from_vod(req).await {
            post_clip_to_chat(ops, broadcaster_id, &title, &clip).await;
            return ClipOutcome::VodSuccess {
                clip,
                duration_seconds: binding.duration_seconds,
            };
        }
        // Any VOD-path failure (including rate limiting) falls through to
        // the classic path below, like any other failure.
    }

    match ops.create_clip_classic(broadcaster_id).await {
        Ok(clip) => {
            // The classic endpoint has no `title` param at all — Twitch
            // assigns its own name here regardless, so `title` only ends up
            // in the chat message, not on the clip itself.
            post_clip_to_chat(ops, broadcaster_id, &title, &clip).await;
            ClipOutcome::FallbackSuccess { clip }
        }
        Err(err) => ClipOutcome::Failed(format!("{err:?}")),
    }
}

/// Returns the VOD to clip from and the offset to request, only when both
/// are actually usable: the VOD must belong to the current broadcast (not a
/// stale video from the previous one), and the elapsed time since the
/// stream started must be at least the requested duration once clamped to a
/// non-negative value (clock skew between this machine and Twitch's server
/// can otherwise produce a negative "elapsed" figure).
async fn usable_vod(
    ops: &impl HelixOps,
    broadcaster_id: &str,
    stream: &StreamInfo,
    binding: &HotkeyBinding,
    now: DateTime<Utc>,
) -> Option<(VodInfo, u32)> {
    let vod = match ops.get_current_vod(broadcaster_id).await {
        Ok(Some(vod)) if vod.created_at >= stream.started_at => vod,
        _ => return None,
    };

    let elapsed = now.signed_duration_since(stream.started_at).num_seconds();
    let elapsed = u32::try_from(elapsed.max(0)).unwrap_or(0);

    if elapsed < binding.duration_seconds as u32 {
        return None;
    }

    Some((vod, elapsed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    #[test]
    fn failed_variant_serializes_with_adjacent_tag() {
        // Internal tagging (`#[serde(tag = "type")]` alone) cannot serialize
        // a tuple variant whose payload isn't a map, and silently fails
        // (swallowed by the `let _ = app.emit(...)` call site) — this would
        // have dropped every failed-clip event on the frontend floor.
        let json = serde_json::to_value(ClipOutcome::Failed("boom".to_string())).unwrap();
        assert_eq!(json, serde_json::json!({"type": "Failed", "data": "boom"}));
    }

    #[derive(Default)]
    struct FakeCalls {
        get_stream: usize,
        get_current_vod: usize,
        create_clip_from_vod: usize,
        create_clip_classic: usize,
        get_clip: usize,
        send_chat_message: usize,
    }

    struct FakeOps {
        stream: Result<Option<StreamInfo>, HelixError>,
        vod: Result<Option<VodInfo>, HelixError>,
        vod_clip: Result<ClipRef, HelixError>,
        classic_clip: Result<ClipRef, HelixError>,
        chat_message: StdMutex<Option<String>>,
        calls: StdMutex<FakeCalls>,
    }

    impl Default for FakeOps {
        fn default() -> Self {
            FakeOps {
                stream: Ok(Some(StreamInfo {
                    started_at: "2026-10-19T18:00:00Z".parse().unwrap(),
                })),
                vod: Ok(Some(VodInfo {
                    id: "vod1".to_string(),
                    created_at: "2026-10-19T18:00:00Z".parse().unwrap(),
                })),
                vod_clip: Ok(ClipRef {
                    id: "vod-clip".to_string(),
                    edit_url: "https://example.invalid/vod-clip".to_string(),
                }),
                classic_clip: Ok(ClipRef {
                    id: "classic-clip".to_string(),
                    edit_url: "https://example.invalid/classic-clip".to_string(),
                }),
                chat_message: StdMutex::new(None),
                calls: StdMutex::new(FakeCalls::default()),
            }
        }
    }

    impl HelixOps for FakeOps {
        async fn get_stream(&self, _broadcaster_id: &str) -> Result<Option<StreamInfo>, HelixError> {
            self.calls.lock().unwrap().get_stream += 1;
            self.stream.clone()
        }

        async fn get_current_vod(&self, _broadcaster_id: &str) -> Result<Option<VodInfo>, HelixError> {
            self.calls.lock().unwrap().get_current_vod += 1;
            self.vod.clone()
        }

        async fn create_clip_from_vod(
            &self,
            _req: CreateClipFromVodRequest,
        ) -> Result<ClipRef, HelixError> {
            self.calls.lock().unwrap().create_clip_from_vod += 1;
            self.vod_clip.clone()
        }

        async fn create_clip_classic(&self, _broadcaster_id: &str) -> Result<ClipRef, HelixError> {
            self.calls.lock().unwrap().create_clip_classic += 1;
            self.classic_clip.clone()
        }

        async fn get_clip(&self, _clip_id: &str) -> Result<Option<ClipRef>, HelixError> {
            self.calls.lock().unwrap().get_clip += 1;
            Ok(None)
        }

        async fn send_chat_message(
            &self,
            _broadcaster_id: &str,
            message: &str,
        ) -> Result<(), HelixError> {
            self.calls.lock().unwrap().send_chat_message += 1;
            *self.chat_message.lock().unwrap() = Some(message.to_string());
            Ok(())
        }
    }

    fn binding(duration_seconds: u8) -> HotkeyBinding {
        HotkeyBinding {
            id: "b1".to_string(),
            modifiers: vec!["Control".to_string(), "Alt".to_string()],
            code: "Digit1".to_string(),
            duration_seconds,
            label: format!("Clip {duration_seconds}s"),
        }
    }

    fn now() -> DateTime<Utc> {
        "2026-10-19T18:01:00Z".parse().unwrap() // 60s after the fakes' started_at
    }

    async fn run(ops: &FakeOps, binding: &HotkeyBinding, now: DateTime<Utc>) -> ClipOutcome {
        let cooldowns = Mutex::new(HashMap::new());
        execute_clip_request(ops, "141981764", "141981764", binding, now, &cooldowns).await
    }

    #[tokio::test]
    async fn offline_stream_aborts_without_calling_vod_or_clip_endpoints() {
        let mut ops = FakeOps::default();
        ops.stream = Ok(None);

        let outcome = run(&ops, &binding(30), now()).await;

        assert_eq!(outcome, ClipOutcome::Offline);
        let calls = ops.calls.lock().unwrap();
        assert_eq!(calls.get_current_vod, 0);
        assert_eq!(calls.create_clip_from_vod, 0);
        assert_eq!(calls.create_clip_classic, 0);
    }

    #[tokio::test]
    async fn vod_offset_less_than_duration_skips_straight_to_fallback() {
        let ops = FakeOps::default();
        // started_at == now - 10s, duration 30s: elapsed (10) < duration (30).
        let outcome = run(&ops, &binding(30), "2026-10-19T18:00:10Z".parse().unwrap()).await;

        assert_eq!(
            outcome,
            ClipOutcome::FallbackSuccess {
                clip: ops.classic_clip.clone().unwrap()
            }
        );
        let calls = ops.calls.lock().unwrap();
        assert_eq!(calls.create_clip_from_vod, 0);
        assert_eq!(calls.create_clip_classic, 1);
    }

    #[tokio::test]
    async fn stale_vod_from_previous_broadcast_is_detected_and_falls_back() {
        let mut ops = FakeOps::default();
        ops.vod = Ok(Some(VodInfo {
            id: "old-vod".to_string(),
            created_at: "2026-10-19T17:00:00Z".parse().unwrap(), // before stream.started_at
        }));

        let outcome = run(&ops, &binding(30), now()).await;

        assert_eq!(
            outcome,
            ClipOutcome::FallbackSuccess {
                clip: ops.classic_clip.clone().unwrap()
            }
        );
        let calls = ops.calls.lock().unwrap();
        assert_eq!(calls.create_clip_from_vod, 0);
    }

    #[tokio::test]
    async fn clock_skew_producing_negative_offset_is_clamped_and_falls_back() {
        let ops = FakeOps::default();
        // started_at is 5s in the future relative to `now` -> negative elapsed.
        let outcome = run(&ops, &binding(30), "2026-10-19T17:59:55Z".parse().unwrap()).await;

        assert_eq!(
            outcome,
            ClipOutcome::FallbackSuccess {
                clip: ops.classic_clip.clone().unwrap()
            }
        );
        let calls = ops.calls.lock().unwrap();
        assert_eq!(calls.create_clip_from_vod, 0);
        assert_eq!(calls.create_clip_classic, 1);
    }

    #[tokio::test]
    async fn vod_path_success_returns_vod_success_with_exact_duration() {
        let ops = FakeOps::default();
        let outcome = run(&ops, &binding(45), now()).await;

        assert_eq!(
            outcome,
            ClipOutcome::VodSuccess {
                clip: ops.vod_clip.clone().unwrap(),
                duration_seconds: 45,
            }
        );
        let calls = ops.calls.lock().unwrap();
        assert_eq!(calls.create_clip_from_vod, 1);
        assert_eq!(calls.create_clip_classic, 0);
        assert_eq!(calls.send_chat_message, 1);
        assert!(ops
            .chat_message
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .contains("https://example.invalid/vod-clip"));
    }

    #[tokio::test]
    async fn classic_fallback_success_also_posts_to_chat() {
        let ops = FakeOps::default();
        let outcome = run(&ops, &binding(30), "2026-10-19T18:00:10Z".parse().unwrap()).await;

        assert_eq!(
            outcome,
            ClipOutcome::FallbackSuccess {
                clip: ops.classic_clip.clone().unwrap()
            }
        );
        assert_eq!(ops.calls.lock().unwrap().send_chat_message, 1);
        assert!(ops
            .chat_message
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .contains("https://example.invalid/classic-clip"));
    }

    #[tokio::test]
    async fn offline_stream_never_posts_to_chat() {
        let mut ops = FakeOps::default();
        ops.stream = Ok(None);

        run(&ops, &binding(30), now()).await;

        assert_eq!(ops.calls.lock().unwrap().send_chat_message, 0);
    }

    #[test]
    fn generate_clip_title_has_two_words_and_duration_suffix() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);

        let title = generate_clip_title(45, &mut rng);

        assert!(title.ends_with("-45s"), "title was {title}");
        let parts: Vec<&str> = title.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert!(TITLE_WORDS.contains(&parts[0]));
        assert!(TITLE_WORDS.contains(&parts[1]));
    }

    #[tokio::test]
    async fn vod_path_failure_falls_back_to_classic() {
        let mut ops = FakeOps::default();
        ops.vod_clip = Err(HelixError::Api("boom".to_string()));

        let outcome = run(&ops, &binding(30), now()).await;

        assert_eq!(
            outcome,
            ClipOutcome::FallbackSuccess {
                clip: ops.classic_clip.clone().unwrap()
            }
        );
    }

    #[tokio::test]
    async fn both_paths_failing_returns_failed() {
        let mut ops = FakeOps::default();
        ops.vod_clip = Err(HelixError::Api("boom".to_string()));
        ops.classic_clip = Err(HelixError::Api("boom too".to_string()));

        let outcome = run(&ops, &binding(30), now()).await;

        assert!(matches!(outcome, ClipOutcome::Failed(_)));
    }

    #[tokio::test]
    async fn rate_limited_on_vod_path_falls_back_like_any_other_failure() {
        let mut ops = FakeOps::default();
        ops.vod_clip = Err(HelixError::RateLimited);

        let outcome = run(&ops, &binding(30), now()).await;

        assert_eq!(
            outcome,
            ClipOutcome::FallbackSuccess {
                clip: ops.classic_clip.clone().unwrap()
            }
        );
    }

    #[tokio::test]
    async fn binding_pressed_again_within_cooldown_is_rejected_without_calling_ops() {
        let ops = FakeOps::default();
        let the_binding = binding(30);
        // Pressed 3s ago — inside the 10s cooldown.
        let cooldowns = Mutex::new(HashMap::from([(
            the_binding.id.clone(),
            now() - Duration::seconds(3),
        )]));

        let outcome =
            execute_clip_request(&ops, "141981764", "141981764", &the_binding, now(), &cooldowns)
                .await;

        assert!(matches!(outcome, ClipOutcome::Failed(_)));
        let calls = ops.calls.lock().unwrap();
        assert_eq!(calls.get_stream, 0);
        assert_eq!(calls.get_current_vod, 0);
        assert_eq!(calls.create_clip_from_vod, 0);
        assert_eq!(calls.create_clip_classic, 0);
    }

    #[tokio::test]
    async fn binding_pressed_after_cooldown_elapsed_is_allowed() {
        let ops = FakeOps::default();
        let the_binding = binding(45);
        // Pressed 11s ago — just past the 10s cooldown.
        let cooldowns = Mutex::new(HashMap::from([(
            the_binding.id.clone(),
            now() - Duration::seconds(11),
        )]));

        let outcome =
            execute_clip_request(&ops, "141981764", "141981764", &the_binding, now(), &cooldowns)
                .await;

        assert_eq!(
            outcome,
            ClipOutcome::VodSuccess {
                clip: ops.vod_clip.clone().unwrap(),
                duration_seconds: 45,
            }
        );
    }
}
