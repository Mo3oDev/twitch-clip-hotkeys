use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::client::{HelixClient, HelixError};
use crate::auth::TokenSet;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StreamInfo {
    pub started_at: DateTime<Utc>,
}

pub async fn get_stream(
    client: &HelixClient,
    tokens: &mut TokenSet,
    broadcaster_id: &str,
) -> Result<Option<StreamInfo>, HelixError> {
    let value = client
        .request(
            tokens,
            reqwest::Method::GET,
            "/helix/streams",
            &[("user_id", broadcaster_id)],
        )
        .await?;
    let data = value
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| HelixError::Api("missing data array".to_string()))?;
    match data.first() {
        None => Ok(None),
        Some(v) => serde_json::from_value(v.clone())
            .map(Some)
            .map_err(|e| HelixError::Api(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn tokens() -> TokenSet {
        TokenSet {
            access_token: "a".to_string(),
            refresh_token: "r".to_string(),
            expires_at: 0,
            scope: vec!["clips:edit".to_string()],
            user_id: "u1".to_string(),
        }
    }

    fn client(server: &MockServer) -> HelixClient {
        HelixClient {
            http: reqwest::Client::new(),
            base_url: server.uri(),
            client_id: "client123".to_string(),
            auth_base_url: "http://unused.invalid".to_string(),
        }
    }

    #[tokio::test]
    async fn get_stream_returns_started_at_when_live() {
        let helix = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/helix/streams"))
            .and(query_param("user_id", "141981764"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "id": "123", "user_id": "141981764", "user_login": "twitchdev",
                    "game_id": "494131", "type": "live", "title": "Playing",
                    "started_at": "2026-10-19T18:00:00Z", "viewer_count": 10
                }]
            })))
            .mount(&helix)
            .await;

        let c = client(&helix);
        let mut tok = tokens();
        let result = get_stream(&c, &mut tok, "141981764").await.unwrap();

        assert_eq!(
            result,
            Some(StreamInfo {
                started_at: "2026-10-19T18:00:00Z".parse().unwrap(),
            })
        );
    }

    #[tokio::test]
    async fn get_stream_returns_none_when_offline() {
        let helix = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/helix/streams"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": []
            })))
            .mount(&helix)
            .await;

        let c = client(&helix);
        let mut tok = tokens();
        let result = get_stream(&c, &mut tok, "141981764").await.unwrap();

        assert_eq!(result, None);
    }
}
