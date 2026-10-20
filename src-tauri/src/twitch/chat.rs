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
    client
        .request_json(tokens, reqwest::Method::POST, "/helix/chat/messages", &body)
        .await?;
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
}
