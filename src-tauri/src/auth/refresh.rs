use serde::Deserialize;

use super::device_flow::AuthError;
use super::TokenSet;

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

pub async fn refresh_access_token(
    http: &reqwest::Client,
    base_url: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<TokenSet, AuthError> {
    let response = http
        .post(format!("{base_url}/oauth2/token"))
        .form(&[
            ("client_id", client_id),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| AuthError::Network(e.to_string()))?;

    if !response.status().is_success() {
        let error_body = response
            .json::<TwitchErrorBody>()
            .await
            .map_err(|e| AuthError::Network(e.to_string()))?;
        return Err(AuthError::Twitch(error_body.message));
    }

    let body = response
        .json::<TwitchTokenResponse>()
        .await
        .map_err(|e| AuthError::Network(e.to_string()))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_secs() as i64;

    Ok(TokenSet {
        access_token: body.access_token,
        refresh_token: body.refresh_token,
        expires_at: now + body.expires_in,
        scope: body.scope,
        user_id: body.user_id.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn refresh_success_returns_new_token_set() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "new-access",
                "refresh_token": "new-refresh",
                "expires_in": 14820,
                "scope": ["clips:edit"],
                "token_type": "bearer"
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let result = refresh_access_token(&http, &server.uri(), "client123", "old-refresh")
            .await
            .unwrap();

        assert_eq!(result.access_token, "new-access");
        assert_eq!(result.refresh_token, "new-refresh");
    }

    #[tokio::test]
    async fn refresh_invalid_grant_returns_twitch_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "message": "Invalid refresh token"
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let result = refresh_access_token(&http, &server.uri(), "client123", "old-refresh").await;

        assert_eq!(
            result,
            Err(AuthError::Twitch("Invalid refresh token".to_string()))
        );
    }
}
