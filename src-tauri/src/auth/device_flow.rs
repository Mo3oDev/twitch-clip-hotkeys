use serde::Deserialize;

use super::TokenSet;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u32,
    pub interval: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AuthError {
    Pending,
    SlowDown,
    Expired,
    Network(String),
    Twitch(String),
}

#[derive(Debug, Deserialize)]
struct TwitchTokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    scope: Vec<String>,
    #[serde(default)]
    user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TwitchErrorBody {
    message: String,
}

pub async fn start_device_flow(
    http: &reqwest::Client,
    base_url: &str,
    client_id: &str,
    scopes: &str,
) -> Result<DeviceCodeResponse, AuthError> {
    let response = http
        .post(format!("{base_url}/oauth2/device"))
        .form(&[("client_id", client_id), ("scopes", scopes)])
        .send()
        .await
        .map_err(|e| AuthError::Network(e.to_string()))?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AuthError::Twitch(body));
    }

    response
        .json::<DeviceCodeResponse>()
        .await
        .map_err(|e| AuthError::Network(e.to_string()))
}

pub async fn poll_for_token(
    http: &reqwest::Client,
    base_url: &str,
    client_id: &str,
    device_code: &str,
    interval_secs: u32,
    expires_in_secs: u32,
) -> Result<TokenSet, AuthError> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(expires_in_secs as u64);
    let mut wait = std::time::Duration::from_secs(interval_secs as u64);

    loop {
        let response = http
            .post(format!("{base_url}/oauth2/token"))
            .form(&[
                ("client_id", client_id),
                ("device_code", device_code),
                (
                    "grant_type",
                    "urn:ietf:params:oauth:grant-type:device_code",
                ),
            ])
            .send()
            .await
            .map_err(|e| AuthError::Network(e.to_string()))?;

        if response.status().is_success() {
            let body = response
                .json::<TwitchTokenResponse>()
                .await
                .map_err(|e| AuthError::Network(e.to_string()))?;
            return Ok(TokenSet {
                access_token: body.access_token,
                refresh_token: body.refresh_token,
                expires_at: chrono_now_plus(body.expires_in),
                scope: body.scope,
                user_id: body.user_id.unwrap_or_default(),
            });
        }

        let error_body = response
            .json::<TwitchErrorBody>()
            .await
            .map_err(|e| AuthError::Network(e.to_string()))?;

        match error_body.message.as_str() {
            "authorization_pending" => {}
            "slow_down" => {
                wait += std::time::Duration::from_secs(1);
            }
            "expired_token" => return Err(AuthError::Expired),
            other => return Err(AuthError::Twitch(other.to_string())),
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(AuthError::Expired);
        }

        tokio::time::sleep(wait).await;
    }
}

/// Plain Unix-seconds arithmetic, kept local to avoid pulling `chrono` into
/// this task — Task 7/8 introduce `chrono` for VOD-offset math; this crate
/// isn't pinned to use it here by the spec, so `std::time` suffices.
fn chrono_now_plus(seconds_from_now: i64) -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_secs() as i64;
    now + seconds_from_now
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn start_device_flow_parses_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/device"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "device_code": "devcode123",
                "user_code": "ABCDEFGH",
                "verification_uri": "https://www.twitch.tv/activate?public=true&device-code=ABCDEFGH",
                "expires_in": 1800,
                "interval": 5
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let result = start_device_flow(&http, &server.uri(), "client123", "clips:edit")
            .await
            .unwrap();

        assert_eq!(
            result,
            DeviceCodeResponse {
                device_code: "devcode123".to_string(),
                user_code: "ABCDEFGH".to_string(),
                verification_uri:
                    "https://www.twitch.tv/activate?public=true&device-code=ABCDEFGH".to_string(),
                expires_in: 1800,
                interval: 5,
            }
        );
    }

    #[tokio::test]
    async fn poll_for_token_retries_on_authorization_pending_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "message": "authorization_pending"
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "a",
                "refresh_token": "r",
                "expires_in": 14820,
                "scope": ["clips:edit"],
                "token_type": "bearer"
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let result = poll_for_token(&http, &server.uri(), "client123", "devcode123", 0, 30)
            .await
            .unwrap();

        assert_eq!(result.access_token, "a");
        assert_eq!(result.refresh_token, "r");
    }

    #[tokio::test]
    async fn poll_for_token_backs_off_on_slow_down() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "message": "slow_down"
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "a",
                "refresh_token": "r",
                "expires_in": 14820,
                "scope": ["clips:edit"],
                "token_type": "bearer"
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        // interval_secs: 0 so the test runs fast; this only asserts that a
        // slow_down response does not abort the poll loop (it must retry and
        // eventually succeed), not the actual backoff duration — timing the
        // backoff would make the test slow and flaky.
        let result = poll_for_token(&http, &server.uri(), "client123", "devcode123", 0, 30)
            .await
            .unwrap();

        assert_eq!(result.access_token, "a");
    }

    #[tokio::test]
    async fn poll_for_token_errors_on_expired_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "message": "expired_token"
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let result = poll_for_token(&http, &server.uri(), "client123", "devcode123", 0, 30).await;

        assert_eq!(result, Err(AuthError::Expired));
    }
}
