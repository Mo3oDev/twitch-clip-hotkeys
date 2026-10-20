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
pub const SCOPES: &str = "clips:edit channel:manage:clips user:write:chat";

/// The real `HelixOps` impl (Task 8 deferred this adapter to whichever task
/// first needs a non-fake implementation — that's this one). Holds the
/// shared, possibly-absent token set behind a `tokio::sync::Mutex` so a
/// refreshed token (mutated in place by `HelixClient::request`) is visible
/// to every subsequent call and persisted back to Credential Manager.
pub struct RealHelixOps {
    http: reqwest::Client,
    client_id: Arc<StdMutex<Option<String>>>,
    tokens: Arc<TokioMutex<Option<TokenSet>>>,
}

impl RealHelixOps {
    pub fn new(
        client_id: Arc<StdMutex<Option<String>>>,
        http: reqwest::Client,
        tokens: Arc<TokioMutex<Option<TokenSet>>>,
    ) -> Self {
        Self {
            http,
            client_id,
            tokens,
        }
    }

    /// `client_id` can change after this adapter is built (`set_client_id`
    /// is read fresh here, unlike the first version of this adapter, which
    /// baked it into a fixed `HelixClient` at construction time — breaking
    /// first-run setup, since `ops` is built before the user has pasted a
    /// `client_id` into Settings). Building a `HelixClient` per call is
    /// cheap: it's a few `String`s plus a `reqwest::Client` clone, which is
    /// itself just an `Arc` bump.
    fn client(&self) -> HelixClient {
        HelixClient {
            http: self.http.clone(),
            base_url: HELIX_BASE_URL.to_string(),
            client_id: self.client_id.lock().unwrap().clone().unwrap_or_default(),
            auth_base_url: AUTH_BASE_URL.to_string(),
        }
    }

    /// Fetches the token owner's current login name, persisting any token
    /// refresh triggered along the way. Used only for the cosmetic
    /// "Connected as X" display when a session is restored from Credential
    /// Manager at startup — `ActiveSession`'s ids already came from the
    /// cached token, so clip creation doesn't depend on this succeeding.
    pub async fn fetch_login(&self) -> Option<String> {
        let client = self.client();
        let mut guard = self.tokens.lock().await;
        let tok = guard.as_mut()?;
        let login = crate::twitch::users::get_current_user(&client, tok)
            .await
            .ok()
            .map(|u| u.login);
        let _ = crate::auth::token_store::save_tokens(KEYRING_SERVICE, tok);
        login
    }
}

impl HelixOps for RealHelixOps {
    async fn get_stream(&self, broadcaster_id: &str) -> Result<Option<StreamInfo>, HelixError> {
        let client = self.client();
        let mut guard = self.tokens.lock().await;
        let Some(tok) = guard.as_mut() else {
            return Err(HelixError::Unauthorized);
        };
        let result = crate::twitch::streams::get_stream(&client, tok, broadcaster_id).await;
        let _ = crate::auth::token_store::save_tokens(KEYRING_SERVICE, tok);
        result
    }

    async fn get_current_vod(&self, broadcaster_id: &str) -> Result<Option<VodInfo>, HelixError> {
        let client = self.client();
        let mut guard = self.tokens.lock().await;
        let Some(tok) = guard.as_mut() else {
            return Err(HelixError::Unauthorized);
        };
        let result = crate::twitch::videos::get_current_vod(&client, tok, broadcaster_id).await;
        let _ = crate::auth::token_store::save_tokens(KEYRING_SERVICE, tok);
        result
    }

    async fn create_clip_from_vod(
        &self,
        req: CreateClipFromVodRequest,
    ) -> Result<ClipRef, HelixError> {
        let client = self.client();
        let mut guard = self.tokens.lock().await;
        let Some(tok) = guard.as_mut() else {
            return Err(HelixError::Unauthorized);
        };
        let result = crate::twitch::clips::create_clip_from_vod(&client, tok, req).await;
        let _ = crate::auth::token_store::save_tokens(KEYRING_SERVICE, tok);
        result
    }

    async fn create_clip_classic(&self, broadcaster_id: &str) -> Result<ClipRef, HelixError> {
        let client = self.client();
        let mut guard = self.tokens.lock().await;
        let Some(tok) = guard.as_mut() else {
            return Err(HelixError::Unauthorized);
        };
        let result = crate::twitch::clips::create_clip_classic(&client, tok, broadcaster_id).await;
        let _ = crate::auth::token_store::save_tokens(KEYRING_SERVICE, tok);
        result
    }

    async fn get_clip(&self, clip_id: &str) -> Result<Option<ClipRef>, HelixError> {
        let client = self.client();
        let mut guard = self.tokens.lock().await;
        let Some(tok) = guard.as_mut() else {
            return Err(HelixError::Unauthorized);
        };
        let result = crate::twitch::clips::get_clip(&client, tok, clip_id).await;
        let _ = crate::auth::token_store::save_tokens(KEYRING_SERVICE, tok);
        result
    }

    async fn send_chat_message(&self, broadcaster_id: &str, message: &str) -> Result<(), HelixError> {
        let client = self.client();
        let mut guard = self.tokens.lock().await;
        let Some(tok) = guard.as_mut() else {
            return Err(HelixError::Unauthorized);
        };
        let result =
            crate::twitch::chat::send_chat_message(&client, tok, broadcaster_id, broadcaster_id, message)
                .await;
        let _ = crate::auth::token_store::save_tokens(KEYRING_SERVICE, tok);
        result
    }
}

pub struct AppState {
    pub http: reqwest::Client,
    pub client_id: Arc<StdMutex<Option<String>>>,
    pub tokens: Arc<TokioMutex<Option<TokenSet>>>,
    pub user_login: Arc<StdMutex<Option<String>>>,
    pub session: Arc<StdMutex<Option<ActiveSession>>>,
    pub bindings: Arc<StdMutex<Vec<HotkeyBinding>>>,
    pub hotkey_errors: Arc<StdMutex<Vec<crate::commands::HotkeyRegistrationError>>>,
}
