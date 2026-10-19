use std::sync::{Arc, Mutex as StdMutex};

use tokio::sync::Mutex as TokioMutex;

use crate::auth::TokenSet;
use crate::clip_flow::HelixOps;
use crate::config::HotkeyBinding;
use crate::hotkeys::ActiveSession;
use crate::twitch::{ClipRef, CreateClipFromVodRequest, HelixClient, HelixError, StreamInfo, VodInfo};

pub const KEYRING_SERVICE: &str = "twitch-clip-hotkeys";
pub const AUTH_BASE_URL: &str = "https://id.twitch.tv";
pub const HELIX_BASE_URL: &str = "https://api.twitch.tv";
pub const SCOPES: &str = "clips:edit";

/// The real `HelixOps` impl (Task 8 deferred this adapter to whichever task
/// first needs a non-fake implementation — that's this one). Holds the
/// shared, possibly-absent token set behind a `tokio::sync::Mutex` so a
/// refreshed token (mutated in place by `HelixClient::request`) is visible
/// to every subsequent call and persisted back to Credential Manager.
pub struct RealHelixOps {
    client: HelixClient,
    tokens: Arc<TokioMutex<Option<TokenSet>>>,
}

impl RealHelixOps {
    pub fn new(
        client_id: String,
        http: reqwest::Client,
        tokens: Arc<TokioMutex<Option<TokenSet>>>,
    ) -> Self {
        Self {
            client: HelixClient {
                http,
                base_url: HELIX_BASE_URL.to_string(),
                client_id,
                auth_base_url: AUTH_BASE_URL.to_string(),
            },
            tokens,
        }
    }
}

impl HelixOps for RealHelixOps {
    async fn get_stream(&self, broadcaster_id: &str) -> Result<Option<StreamInfo>, HelixError> {
        let mut guard = self.tokens.lock().await;
        let Some(tok) = guard.as_mut() else {
            return Err(HelixError::Unauthorized);
        };
        let result = crate::twitch::streams::get_stream(&self.client, tok, broadcaster_id).await;
        let _ = crate::auth::token_store::save_tokens(KEYRING_SERVICE, tok);
        result
    }

    async fn get_current_vod(&self, broadcaster_id: &str) -> Result<Option<VodInfo>, HelixError> {
        let mut guard = self.tokens.lock().await;
        let Some(tok) = guard.as_mut() else {
            return Err(HelixError::Unauthorized);
        };
        let result = crate::twitch::videos::get_current_vod(&self.client, tok, broadcaster_id).await;
        let _ = crate::auth::token_store::save_tokens(KEYRING_SERVICE, tok);
        result
    }

    async fn create_clip_from_vod(
        &self,
        req: CreateClipFromVodRequest,
    ) -> Result<ClipRef, HelixError> {
        let mut guard = self.tokens.lock().await;
        let Some(tok) = guard.as_mut() else {
            return Err(HelixError::Unauthorized);
        };
        let result = crate::twitch::clips::create_clip_from_vod(&self.client, tok, req).await;
        let _ = crate::auth::token_store::save_tokens(KEYRING_SERVICE, tok);
        result
    }

    async fn create_clip_classic(&self, broadcaster_id: &str) -> Result<ClipRef, HelixError> {
        let mut guard = self.tokens.lock().await;
        let Some(tok) = guard.as_mut() else {
            return Err(HelixError::Unauthorized);
        };
        let result = crate::twitch::clips::create_clip_classic(&self.client, tok, broadcaster_id).await;
        let _ = crate::auth::token_store::save_tokens(KEYRING_SERVICE, tok);
        result
    }

    async fn get_clip(&self, clip_id: &str) -> Result<Option<ClipRef>, HelixError> {
        let mut guard = self.tokens.lock().await;
        let Some(tok) = guard.as_mut() else {
            return Err(HelixError::Unauthorized);
        };
        let result = crate::twitch::clips::get_clip(&self.client, tok, clip_id).await;
        let _ = crate::auth::token_store::save_tokens(KEYRING_SERVICE, tok);
        result
    }
}

pub struct AppState {
    pub http: reqwest::Client,
    pub client_id: StdMutex<Option<String>>,
    pub tokens: Arc<TokioMutex<Option<TokenSet>>>,
    pub user_login: Arc<StdMutex<Option<String>>>,
    pub session: Arc<StdMutex<Option<ActiveSession>>>,
    pub bindings: Arc<StdMutex<Vec<HotkeyBinding>>>,
}
