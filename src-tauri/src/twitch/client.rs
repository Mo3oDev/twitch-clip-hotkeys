#[derive(Debug, Clone, PartialEq)]
pub enum HelixError {
    Unauthorized,
    RateLimited,
    Network(String),
    Api(String),
}

pub struct HelixClient {
    pub http: reqwest::Client,
    pub base_url: String,
    pub client_id: String,
    pub auth_base_url: String,
}

impl HelixClient {
    pub async fn request(
        &self,
        tokens: &mut crate::auth::TokenSet,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<serde_json::Value, HelixError> {
        self.send_with_retry(tokens, method, path, &|b| b.query(query))
            .await
    }

    /// Like `request`, but for endpoints that take a JSON body instead of
    /// query params (e.g. Send Chat Message) — Helix mixes both styles
    /// across endpoints, so this isn't just `request` with an extra arg.
    pub async fn request_json(
        &self,
        tokens: &mut crate::auth::TokenSet,
        method: reqwest::Method,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, HelixError> {
        self.send_with_retry(tokens, method, path, &|b| b.json(body))
            .await
    }

    async fn send_with_retry(
        &self,
        tokens: &mut crate::auth::TokenSet,
        method: reqwest::Method,
        path: &str,
        build: &(dyn Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder + Send + Sync),
    ) -> Result<serde_json::Value, HelixError> {
        match self.send_once(tokens, method.clone(), path, build).await? {
            Ok(value) => Ok(value),
            Err(()) => {
                // First 401: refresh exactly once, then retry exactly once.
                // A refresh that fails because Twitch rejected it (revoked/
                // invalid refresh token) is a real `Unauthorized`; a refresh
                // that failed to even reach Twitch (offline, DNS, timeout —
                // see `AUTH_BASE_URL`'s `reqwest` call in `refresh_access_token`)
                // is a `Network` error, not an auth failure — conflating the
                // two previously showed "reconnect to Twitch" on a plain
                // network outage.
                let mut refreshed = crate::auth::refresh::refresh_access_token(
                    &self.http,
                    &self.auth_base_url,
                    &self.client_id,
                    &tokens.refresh_token,
                )
                .await
                .map_err(|e| match e {
                    crate::auth::device_flow::AuthError::Network(msg) => HelixError::Network(msg),
                    _ => HelixError::Unauthorized,
                })?;
                // Twitch's refresh response has no `user_id` field, so
                // `refresh_access_token` always returns it empty — carry
                // the pre-refresh value forward, or every downstream
                // consumer (and the copy persisted to Credential Manager)
                // ends up with a blank broadcaster id after the first
                // refresh.
                refreshed.user_id = tokens.user_id.clone();
                *tokens = refreshed;

                match self.send_once(tokens, method, path, build).await? {
                    Ok(value) => Ok(value),
                    Err(()) => Err(HelixError::Unauthorized),
                }
            }
        }
    }

    /// Sends one request. `Ok(Err(()))` signals "got a 401, caller decides
    /// whether to retry" without treating it as a hard error yet.
    async fn send_once(
        &self,
        tokens: &crate::auth::TokenSet,
        method: reqwest::Method,
        path: &str,
        build: &(dyn Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder + Send + Sync),
    ) -> Result<Result<serde_json::Value, ()>, HelixError> {
        let builder = self
            .http
            .request(method, format!("{}{}", self.base_url, path))
            .header("Client-Id", &self.client_id)
            .header("Authorization", format!("Bearer {}", tokens.access_token));
        let response = build(builder)
            .send()
            .await
            .map_err(|e| HelixError::Network(e.to_string()))?;

        match response.status().as_u16() {
            401 => Ok(Err(())),
            429 => Err(HelixError::RateLimited),
            200..=299 => response
                .json::<serde_json::Value>()
                .await
                .map_err(|e| HelixError::Network(e.to_string()))
                .map(Ok),
            _ => {
                let body = response.text().await.unwrap_or_default();
                Err(HelixError::Api(body))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::TokenSet;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn tokens(access_token: &str) -> TokenSet {
        TokenSet {
            access_token: access_token.to_string(),
            refresh_token: "r".to_string(),
            expires_at: 0,
            scope: vec!["clips:edit".to_string()],
            user_id: "u1".to_string(),
        }
    }

    fn client(helix: &MockServer, auth: &MockServer) -> HelixClient {
        HelixClient {
            http: reqwest::Client::new(),
            base_url: helix.uri(),
            client_id: "client123".to_string(),
            auth_base_url: auth.uri(),
        }
    }

    #[tokio::test]
    async fn passes_through_200() {
        let helix = MockServer::start().await;
        let auth = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .and(header("Client-Id", "client123"))
            .and(header("Authorization", "Bearer good-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": []})))
            .expect(1)
            .mount(&helix)
            .await;

        let c = client(&helix, &auth);
        let mut tok = tokens("good-token");
        let result = c
            .request(&mut tok, reqwest::Method::GET, "/helix/users", &[])
            .await
            .unwrap();

        assert_eq!(result, serde_json::json!({"data": []}));
    }

    #[tokio::test]
    async fn retries_once_after_401_then_succeeds() {
        let helix = MockServer::start().await;
        let auth = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .and(header("Authorization", "Bearer old-token"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&helix)
            .await;
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .and(header("Authorization", "Bearer refreshed-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": []})))
            .expect(1)
            .mount(&helix)
            .await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "refreshed-token",
                "refresh_token": "new-refresh",
                "expires_in": 14820,
                "scope": ["clips:edit"],
                "token_type": "bearer"
            })))
            .expect(1)
            .mount(&auth)
            .await;

        let c = client(&helix, &auth);
        let mut tok = tokens("old-token");
        let result = c
            .request(&mut tok, reqwest::Method::GET, "/helix/users", &[])
            .await
            .unwrap();

        assert_eq!(result, serde_json::json!({"data": []}));
        assert_eq!(tok.access_token, "refreshed-token");
        // Twitch's refresh response has no `user_id` — must be carried
        // forward from the pre-refresh token, not left blank.
        assert_eq!(tok.user_id, "u1");
    }

    #[tokio::test]
    async fn gives_up_after_second_401() {
        let helix = MockServer::start().await;
        let auth = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .respond_with(ResponseTemplate::new(401))
            .expect(2)
            .mount(&helix)
            .await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "refreshed-token",
                "refresh_token": "new-refresh",
                "expires_in": 14820,
                "scope": ["clips:edit"],
                "token_type": "bearer"
            })))
            .expect(1)
            .mount(&auth)
            .await;

        let c = client(&helix, &auth);
        let mut tok = tokens("old-token");
        let result = c
            .request(&mut tok, reqwest::Method::GET, "/helix/users", &[])
            .await;

        assert_eq!(result, Err(HelixError::Unauthorized));
    }

    #[tokio::test]
    async fn maps_429_to_rate_limited() {
        let helix = MockServer::start().await;
        let auth = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .respond_with(ResponseTemplate::new(429))
            .expect(1)
            .mount(&helix)
            .await;

        let c = client(&helix, &auth);
        let mut tok = tokens("good-token");
        let result = c
            .request(&mut tok, reqwest::Method::GET, "/helix/users", &[])
            .await;

        assert_eq!(result, Err(HelixError::RateLimited));
    }
}
