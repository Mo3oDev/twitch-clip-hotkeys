use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::client::{HelixClient, HelixError};
use crate::auth::TokenSet;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct VodInfo {
    pub id: String,
    pub created_at: DateTime<Utc>,
}

pub async fn get_current_vod(
    client: &HelixClient,
    tokens: &mut TokenSet,
    broadcaster_id: &str,
) -> Result<Option<VodInfo>, HelixError> {
    let value = client
        .request(
            tokens,
            reqwest::Method::GET,
            "/helix/videos",
            &[
                ("user_id", broadcaster_id),
                ("type", "archive"),
                ("first", "1"),
            ],
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
    async fn get_current_vod_returns_vod_when_present() {
        let helix = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/helix/videos"))
            .and(query_param("user_id", "141981764"))
            .and(query_param("type", "archive"))
            .and(query_param("first", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "id": "335921245", "user_id": "141981764", "user_login": "twitchdev",
                    "title": "Live stream", "created_at": "2026-10-19T18:00:00Z",
                    "url": "https://www.twitch.tv/videos/335921245"
                }]
            })))
            .mount(&helix)
            .await;

        let c = client(&helix);
        let mut tok = tokens();
        let result = get_current_vod(&c, &mut tok, "141981764").await.unwrap();

        assert_eq!(
            result,
            Some(VodInfo {
                id: "335921245".to_string(),
                created_at: "2026-10-19T18:00:00Z".parse().unwrap(),
            })
        );
    }

    #[tokio::test]
    async fn get_current_vod_returns_none_when_vods_disabled() {
        let helix = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/helix/videos"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": []
            })))
            .mount(&helix)
            .await;

        let c = client(&helix);
        let mut tok = tokens();
        let result = get_current_vod(&c, &mut tok, "141981764").await.unwrap();

        assert_eq!(result, None);
    }
}
