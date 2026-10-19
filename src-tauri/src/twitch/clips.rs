use serde::Deserialize;

use super::client::{HelixClient, HelixError};
use crate::auth::TokenSet;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ClipRef {
    pub id: String,
    pub edit_url: String,
}

pub struct CreateClipFromVodRequest {
    pub editor_id: String,
    pub broadcaster_id: String,
    pub vod_id: String,
    pub vod_offset: u32,
    pub duration_seconds: u8,
    pub title: String,
}

fn parse_single_clip(value: &serde_json::Value) -> Result<ClipRef, HelixError> {
    let first = value
        .get("data")
        .and_then(|d| d.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| HelixError::Api("missing clip in response".to_string()))?;
    serde_json::from_value(first.clone()).map_err(|e| HelixError::Api(e.to_string()))
}

pub async fn create_clip_from_vod(
    client: &HelixClient,
    tokens: &mut TokenSet,
    req: CreateClipFromVodRequest,
) -> Result<ClipRef, HelixError> {
    let vod_offset = req.vod_offset.to_string();
    let duration = req.duration_seconds.to_string();
    let value = client
        .request(
            tokens,
            reqwest::Method::POST,
            "/helix/videos/clips",
            &[
                ("editor_id", req.editor_id.as_str()),
                ("broadcaster_id", req.broadcaster_id.as_str()),
                ("vod_id", req.vod_id.as_str()),
                ("vod_offset", vod_offset.as_str()),
                ("duration", duration.as_str()),
                ("title", req.title.as_str()),
            ],
        )
        .await?;
    parse_single_clip(&value)
}

pub async fn create_clip_classic(
    client: &HelixClient,
    tokens: &mut TokenSet,
    broadcaster_id: &str,
) -> Result<ClipRef, HelixError> {
    let value = client
        .request(
            tokens,
            reqwest::Method::POST,
            "/helix/clips",
            &[("broadcaster_id", broadcaster_id)],
        )
        .await?;
    parse_single_clip(&value)
}

pub async fn get_clip(
    client: &HelixClient,
    tokens: &mut TokenSet,
    clip_id: &str,
) -> Result<Option<ClipRef>, HelixError> {
    let value = client
        .request(
            tokens,
            reqwest::Method::GET,
            "/helix/clips",
            &[("id", clip_id)],
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
    async fn create_clip_from_vod_sends_exact_duration_and_parses_response() {
        let helix = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/helix/videos/clips"))
            .and(query_param("editor_id", "141981764"))
            .and(query_param("broadcaster_id", "141981764"))
            .and(query_param("vod_id", "335921245"))
            .and(query_param("vod_offset", "40"))
            .and(query_param("duration", "30"))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
                "data": [{"id": "FiveWordsForClipSlug", "edit_url": "https://www.twitch.tv/twitchdev/clip/FiveWordsForClipSlug/edit"}]
            })))
            .mount(&helix)
            .await;

        let c = client(&helix);
        let mut tok = tokens();
        let req = CreateClipFromVodRequest {
            editor_id: "141981764".to_string(),
            broadcaster_id: "141981764".to_string(),
            vod_id: "335921245".to_string(),
            vod_offset: 40,
            duration_seconds: 30,
            title: "Clip 30s".to_string(),
        };
        let result = create_clip_from_vod(&c, &mut tok, req).await.unwrap();

        assert_eq!(
            result,
            ClipRef {
                id: "FiveWordsForClipSlug".to_string(),
                edit_url: "https://www.twitch.tv/twitchdev/clip/FiveWordsForClipSlug/edit"
                    .to_string(),
            }
        );
    }

    #[tokio::test]
    async fn create_clip_classic_parses_response() {
        let helix = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/helix/clips"))
            .and(query_param("broadcaster_id", "141981764"))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
                "data": [{"id": "FiveWordsForClipSlug", "edit_url": "https://www.twitch.tv/twitchdev/clip/FiveWordsForClipSlug/edit"}]
            })))
            .mount(&helix)
            .await;

        let c = client(&helix);
        let mut tok = tokens();
        let result = create_clip_classic(&c, &mut tok, "141981764")
            .await
            .unwrap();

        assert_eq!(result.id, "FiveWordsForClipSlug");
    }

    #[tokio::test]
    async fn get_clip_returns_some_when_found() {
        let helix = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/helix/clips"))
            .and(query_param("id", "FiveWordsForClipSlug"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "FiveWordsForClipSlug", "edit_url": "https://www.twitch.tv/twitchdev/clip/FiveWordsForClipSlug/edit"}]
            })))
            .mount(&helix)
            .await;

        let c = client(&helix);
        let mut tok = tokens();
        let result = get_clip(&c, &mut tok, "FiveWordsForClipSlug")
            .await
            .unwrap();

        assert_eq!(result.map(|c| c.id), Some("FiveWordsForClipSlug".to_string()));
    }

    #[tokio::test]
    async fn get_clip_returns_none_when_not_yet_processed() {
        let helix = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/helix/clips"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": []
            })))
            .mount(&helix)
            .await;

        let c = client(&helix);
        let mut tok = tokens();
        let result = get_clip(&c, &mut tok, "FiveWordsForClipSlug")
            .await
            .unwrap();

        assert_eq!(result, None);
    }
}
