mod discovery;
#[cfg(test)]
mod http_tests;
mod lyrics;
mod playback;
mod response;

use super::{MusicProvider, Track};
use crate::config::Arl;
use reqwest::{
    Client,
    header::{COOKIE, HeaderValue},
    redirect::Policy,
};
use response::{Ping, SearchResults, UserData};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::Mutex;

const ENDPOINT: &str = "https://www.deezer.com/ajax/gw-light.php";
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum DeezerError {
    #[error("Cannot connect to Deezer. Check your connection and try searching again.")]
    Network,
    #[error(
        "This track is unavailable for playback with this account or region. Try another track."
    )]
    Unavailable,
    #[error("Deezer did not respond in time. Try searching again.")]
    Timeout,
    #[error("Deezer rejected authentication. Refresh DEEZER_ARL and restart Mélimo.")]
    Authentication,
    #[error(
        "Deezer rejected the request. Try again later; the unofficial interface may have changed."
    )]
    Service,
    #[error("Unexpected Deezer response. The unofficial interface may have changed.")]
    Response,
    #[error(
        "Deezer returned a web page instead of API data. The gateway may be blocked or changed; this does not confirm an invalid cookie."
    )]
    Gateway,
    #[error(
        "Deezer denied access to the gateway (HTTP 403). This may be an access restriction or an expired session."
    )]
    AccessDenied,
}

fn network_error(error: reqwest::Error) -> DeezerError {
    // Never expose the reqwest error: its URL can contain an API token.
    if error.is_timeout() {
        DeezerError::Timeout
    } else {
        DeezerError::Network
    }
}

struct Session {
    cookie: HeaderValue,
    token: String,
    license: Option<String>,
    user_id: u64,
    favorites: Option<String>,
}

pub struct DeezerProvider {
    client: Client,
    arl: std::sync::RwLock<Arl>,
    session: Mutex<Option<Session>>,
    #[cfg(test)]
    test_endpoint: Option<String>,
}

fn client_builder() -> reqwest::ClientBuilder {
    Client::builder()
        .https_only(true)
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("Melimo/", env!("CARGO_PKG_VERSION")))
}

impl DeezerProvider {
    pub fn new(arl: Arl) -> Result<Self, DeezerError> {
        let client = client_builder().build().map_err(network_error)?;
        Ok(Self {
            client,
            arl: std::sync::RwLock::new(arl),
            session: Mutex::new(None),
            #[cfg(test)]
            test_endpoint: None,
        })
    }

    fn endpoint(&self) -> &str {
        #[cfg(test)]
        if let Some(endpoint) = &self.test_endpoint {
            return endpoint;
        }
        ENDPOINT
    }

    /// Validate credentials without returning any account data.
    pub async fn check_auth(&self) -> Result<(), DeezerError> {
        let mut session = self.session.lock().await;
        // A failed explicit check must not leave an older session cached.
        *session = None;
        *session = Some(self.authenticate().await?);
        Ok(())
    }

    fn request(
        &self,
        method: &str,
        token: &str,
        cookie: Option<&HeaderValue>,
        body: Value,
    ) -> reqwest::RequestBuilder {
        // Fixed HTTPS endpoint only; cookies are never default client headers and
        // redirects are disabled, including same-origin redirects.
        let request = if method == "deezer.ping" {
            self.client.get(self.endpoint())
        } else {
            self.client.post(self.endpoint()).json(&body)
        };
        let mut request = request.query(&[
            ("method", method),
            ("input", "3"),
            ("api_version", "1.0"),
            ("api_token", token),
        ]);
        if let Some(cookie) = cookie {
            request = request.header(COOKIE, cookie);
        }
        request
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        token: &str,
        cookie: Option<&HeaderValue>,
        body: Value,
    ) -> Result<T, DeezerError> {
        let request = self.request(method, token, cookie, body);
        let mut response = request.send().await.map_err(network_error)?;
        if response.status().as_u16() == 401 {
            return Err(DeezerError::Authentication);
        }
        if response.status().as_u16() == 403 {
            return Err(DeezerError::AccessDenied);
        }
        if !response.status().is_success() {
            return Err(DeezerError::Service);
        }
        if response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/html"))
            })
        {
            return Err(DeezerError::Gateway);
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
        {
            return Err(DeezerError::Response);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(DeezerError::Response);
            }
            bytes.extend_from_slice(&chunk);
        }
        response::results(&bytes)
    }

    async fn authenticate(&self) -> Result<Session, DeezerError> {
        let ping: Ping = self.call("deezer.ping", "", None, json!({})).await?;
        let cookie = {
            let arl = self.arl.read().map_err(|_| DeezerError::Authentication)?;
            session_cookie(&arl, &ping.session)?
        };
        let user: UserData = self
            .call("deezer.getUserData", "", Some(&cookie), json!({}))
            .await?;
        let favorites = user.favorites_id();
        let user_id = user.user_id()?;
        let license = user.license_token();
        let token = user.authenticated_token()?;
        Ok(Session {
            cookie,
            token,
            license,
            user_id,
            favorites,
        })
    }

    async fn search(&self, query: String) -> Result<Vec<Track>, DeezerError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut session = self.session.lock().await;
        if session.is_none() {
            *session = Some(self.authenticate().await?);
        }
        let current = session.as_ref().ok_or(DeezerError::Authentication)?;
        let response: Result<SearchResults, _> = self
            .call(
                "search.music",
                &current.token,
                Some(&current.cookie),
                json!({
                    "query": query, "filter": "ALL", "output": "TRACK", "start": 0, "nb": 50,
                }),
            )
            .await;
        match response {
            Ok(response) => response.tracks(),
            Err(error) => {
                // Next search can reauthenticate after an expired session.
                *session = None;
                Err(error)
            }
        }
    }
}

fn session_cookie(arl: &Arl, sid: &str) -> Result<HeaderValue, DeezerError> {
    if sid.is_empty()
        || sid.len() > 512
        || !sid
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(DeezerError::Response);
    }
    let mut cookie = HeaderValue::from_str(&format!("sid={sid}; arl={}", arl.expose()))
        .map_err(|_| DeezerError::Response)?;
    cookie.set_sensitive(true);
    Ok(cookie)
}

impl MusicProvider for DeezerProvider {
    async fn lyrics(&self, id: String) -> Result<super::Lyrics, String> {
        self.fetch_lyrics(id).await.map_err(|e| e.to_string())
    }
    async fn reauthenticate(&self, arl: Arl) -> Result<(), String> {
        let mut session = self.session.lock().await;
        let previous_session = session.take();
        let previous_arl = std::mem::replace(
            &mut *self
                .arl
                .write()
                .map_err(|_| "Cannot refresh credentials.")?,
            arl,
        );
        match self.authenticate().await {
            Ok(new_session) => {
                *session = Some(new_session);
                Ok(())
            }
            Err(error) => {
                *self
                    .arl
                    .write()
                    .map_err(|_| "Cannot restore credentials.")? = previous_arl;
                *session = previous_session;
                Err(error.to_string())
            }
        }
    }

    async fn toggle_favorite(&self, id: String) -> Result<bool, String> {
        self.favorite(id).await.map_err(|e| e.to_string())
    }

    async fn browse(
        &self,
        kind: super::BrowseKind,
        query: String,
    ) -> Result<super::BrowseResults, String> {
        self.discover(kind, query).await.map_err(|e| e.to_string())
    }
    async fn stream_track(
        &self,
        id: String,
        audio: tokio::sync::mpsc::Sender<Vec<u8>>,
    ) -> Result<(), String> {
        self.stream(id, audio).await.map_err(|e| e.to_string())
    }
    async fn search_tracks(&self, query: String) -> Result<Vec<Track>, String> {
        self.search(query).await.map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> DeezerProvider {
        DeezerProvider::new(Arl::parse("a".repeat(192)).unwrap()).unwrap()
    }

    #[test]
    fn request_pins_credentials_to_gateway_and_escapes_query_text() {
        let provider = provider();
        let cookie = session_cookie(&provider.arl.read().unwrap(), "synthetic-session").unwrap();
        assert!(cookie.is_sensitive());
        assert!(session_cookie(&provider.arl.read().unwrap(), "bad; other=value").is_err());
        let query = "Écho \"quoted\" & spaces";
        let request = provider
            .request(
                "search.music",
                "synthetic-token",
                Some(&cookie),
                json!({"query": query}),
            )
            .build()
            .unwrap();
        assert_eq!(request.url().scheme(), "https");
        assert_eq!(request.url().host_str(), Some("www.deezer.com"));
        assert_eq!(request.url().path(), "/ajax/gw-light.php");
        assert_eq!(request.method(), reqwest::Method::POST);
        assert!(request.headers()[COOKIE].is_sensitive());
        let body: Value =
            serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(body["query"], query);
        let ping = provider
            .request("deezer.ping", "", None, json!({}))
            .build()
            .unwrap();
        assert!(!ping.headers().contains_key(COOKIE));
        assert_eq!(ping.method(), reqwest::Method::GET);
        assert!(ping.body().is_none());
    }

    #[tokio::test]
    async fn network_errors_drop_sensitive_urls() {
        let provider = provider();
        // HTTPS-only policy rejects this before any connection is attempted.
        let error = provider
            .client
            .get("http://localhost/?api_token=SYNTHETIC_PRIVATE_TOKEN")
            .send()
            .await
            .unwrap_err();
        let safe = network_error(error);
        assert!(!format!("{safe} {safe:?}").contains("SYNTHETIC_PRIVATE_TOKEN"));
    }
}
