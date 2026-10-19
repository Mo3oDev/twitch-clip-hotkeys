use std::collections::HashSet;

use chrono::{DateTime, Utc};
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClipOutcome {
    VodSuccess { clip: ClipRef, duration_seconds: u8 },
    FallbackSuccess { clip: ClipRef },
    #[allow(dead_code)]
    StillProcessing { clip_id: String },
    Offline,
    Failed(String),
}

pub async fn execute_clip_request(
    ops: &impl HelixOps,
    broadcaster_id: &str,
    editor_id: &str,
    binding: &HotkeyBinding,
    now: DateTime<Utc>,
    in_flight: &Mutex<HashSet<String>>,
) -> ClipOutcome {
    {
        let mut guard = in_flight.lock().await;
        if guard.contains(&binding.id) {
            return ClipOutcome::Failed("already in flight".to_string());
        }
        guard.insert(binding.id.clone());
    }

    let outcome =
        execute_clip_request_inner(ops, broadcaster_id, editor_id, binding, now).await;

    in_flight.lock().await.remove(&binding.id);

    outcome
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

    if let Some((vod, offset)) = usable_vod_offset {
        let req = CreateClipFromVodRequest {
            editor_id: editor_id.to_string(),
            broadcaster_id: broadcaster_id.to_string(),
            vod_id: vod.id,
            vod_offset: offset,
            duration_seconds: binding.duration_seconds,
            title: format!("Clip {}s", binding.duration_seconds),
        };
        if let Ok(clip) = ops.create_clip_from_vod(req).await {
            return ClipOutcome::VodSuccess {
                clip,
                duration_seconds: binding.duration_seconds,
            };
        }
        // Any VOD-path failure (including rate limiting) falls through to
        // the classic path below, like any other failure.
    }

    match ops.create_clip_classic(broadcaster_id).await {
        Ok(clip) => ClipOutcome::FallbackSuccess { clip },
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

    #[derive(Default)]
    struct FakeCalls {
        get_stream: usize,
        get_current_vod: usize,
        create_clip_from_vod: usize,
        create_clip_classic: usize,
        get_clip: usize,
    }

    struct FakeOps {
        stream: Result<Option<StreamInfo>, HelixError>,
        vod: Result<Option<VodInfo>, HelixError>,
        vod_clip: Result<ClipRef, HelixError>,
        classic_clip: Result<ClipRef, HelixError>,
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
        let in_flight = Mutex::new(HashSet::new());
        execute_clip_request(ops, "141981764", "141981764", binding, now, &in_flight).await
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
    async fn in_flight_binding_is_rejected_without_calling_ops() {
        let ops = FakeOps::default();
        let the_binding = binding(30);
        let in_flight = Mutex::new(HashSet::from([the_binding.id.clone()]));

        let outcome =
            execute_clip_request(&ops, "141981764", "141981764", &the_binding, now(), &in_flight)
                .await;

        assert!(matches!(outcome, ClipOutcome::Failed(_)));
        let calls = ops.calls.lock().unwrap();
        assert_eq!(calls.get_stream, 0);
        assert_eq!(calls.get_current_vod, 0);
        assert_eq!(calls.create_clip_from_vod, 0);
        assert_eq!(calls.create_clip_classic, 0);
    }
}
