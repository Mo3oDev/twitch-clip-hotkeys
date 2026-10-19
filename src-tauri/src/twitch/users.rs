use serde::Deserialize;

use super::client::{HelixClient, HelixError};
use crate::auth::TokenSet;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct User {
    pub id: String,
    pub login: String,
}

pub async fn get_current_user(
    client: &HelixClient,
    tokens: &mut TokenSet,
) -> Result<User, HelixError> {
    let value = client
        .request(tokens, reqwest::Method::GET, "/helix/users", &[])
        .await?;
    let first = value
        .get("data")
        .and_then(|d| d.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| HelixError::Api("missing user in response".to_string()))?;
    serde_json::from_value(first.clone()).map_err(|e| HelixError::Api(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
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

    #[tokio::test]
    async fn get_current_user_parses_response() {
        let helix = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "141981764", "login": "twitchdev", "display_name": "TwitchDev"}]
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
        let user = get_current_user(&client, &mut tok).await.unwrap();

        assert_eq!(
            user,
            User {
                id: "141981764".to_string(),
                login: "twitchdev".to_string(),
            }
        );
    }
}
