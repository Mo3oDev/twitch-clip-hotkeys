use super::client::{HelixClient, HelixError};
use crate::auth::TokenSet;

/// Posts as the broadcaster in their own channel — `sender_id` is the same
/// id as `broadcaster_id` here, which only needs `user:write:chat` (the
/// extra `user:bot`/`channel:bot` scopes only apply when the sender is a
/// *different* account chatting in someone else's channel).
pub async fn send_chat_message(
    client: &HelixClient,
    tokens: &mut TokenSet,
    broadcaster_id: &str,
    sender_id: &str,
    message: &str,
) -> Result<(), HelixError> {
    let body = serde_json::json!({
        "broadcaster_id": broadcaster_id,
        "sender_id": sender_id,
        "message": message,
    });
    let value = client
        .request_json(tokens, reqwest::Method::POST, "/helix/chat/messages", &body)
        .await?;

    // Twitch returns 200 even when the message was dropped (AutoMod,
    // follower/sub-only mode, etc.) — the real result is `data[0].is_sent`,
    // which `request_json`'s caller would otherwise never see. Mapping a
    // drop to an `Err` here means the existing best-effort `log::warn!` at
    // the `post_clip_to_chat` call site picks it up for free.
    let first = value.get("data").and_then(|d| d.as_array()).and_then(|a| a.first());
    let is_sent = first.and_then(|m| m.get("is_sent")).and_then(|v| v.as_bool()).unwrap_or(true);
    if !is_sent {
        let code = first
            .and_then(|m| m.get("drop_reason"))
            .and_then(|d| d.get("code"))
            .and_then(|c| c.as_str())
            .unwrap_or("unknown");
        return Err(HelixError::Api(format!("message dropped: {code}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn tokens() -> TokenSet {
        TokenSet {
            access_token: "a".to_string(),
            refresh_token: "r".to_string(),
            expires_at: 0,
            scope: vec!["user:write:chat".to_string()],
            user_id: "u1".to_string(),
        }
    }

    #[tokio::test]
    async fn send_chat_message_posts_expected_json_body() {
        let helix = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/messages"))
            .and(body_json(serde_json::json!({
                "broadcaster_id": "141981764",
                "sender_id": "141981764",
                "message": "hola chat",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"message_id": "m1", "is_sent": true}]
            })))
            .mount(&helix)
            .await;

        let client = HelixClient {
            http: reqwest::Client::new(),
            base_url: helix.uri(),
            client_id: "client123".to_string(),
            auth_base_url: "http://unused.invalid".to_string(),
        };
        let mut tok = tokens();

        send_chat_message(&client, &mut tok, "141981764", "141981764", "hola chat")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn send_chat_message_reports_dropped_message_as_error() {
        let helix = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "message_id": "m1",
                    "is_sent": false,
                    "drop_reason": {"code": "followers_only", "message": "followers only"}
                }]
            })))
            .mount(&helix)
            .await;

        let client = HelixClient {
            http: reqwest::Client::new(),
            base_url: helix.uri(),
            client_id: "client123".to_string(),
            auth_base_url: "http://unused.invalid".to_string(),
        };
        let mut tok = tokens();

        let result = send_chat_message(&client, &mut tok, "141981764", "141981764", "hola chat")
            .await;

        assert_eq!(
            result,
            Err(HelixError::Api("message dropped: followers_only".to_string()))
        );
    }
}
