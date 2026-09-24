//! Synthetic local HTTP sessions. Never contact Deezer or read environment secrets.
use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
    time::timeout,
};

const PING: &str = r#"{"error":[],"results":{"SESSION":"test-session"}}"#;
const AUTH: &str = r#"{"error":[],"results":{"USER":{"USER_ID":123},"checkForm":"test-token"}}"#;
const TRACKS: &str = r#"{"error":[],"results":{"data":[{"SNG_ID":"42","SNG_TITLE":"Example","ART_NAME":"Artist","DURATION":"61"}]}}"#;

struct Reply {
    status: &'static str,
    headers: String,
    body: String,
}
impl Reply {
    fn json(body: &str) -> Self {
        Self {
            status: "200 OK",
            headers: "Content-Type: application/json\r\n".into(),
            body: body.into(),
        }
    }
}

struct Request {
    head: String,
    body: Vec<u8>,
}
struct Server {
    endpoint: String,
    task: JoinHandle<Vec<Request>>,
}

impl Server {
    async fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "http://{}/ajax/gw-light.php",
            listener.local_addr().unwrap()
        );
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for reply in replies {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let header_end = loop {
                    let byte = stream.read_u8().await.unwrap();
                    bytes.push(byte);
                    assert!(bytes.len() < 16_384, "oversized test request");
                    if bytes.ends_with(b"\r\n\r\n") {
                        break bytes.len();
                    }
                };
                let head = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                let length = head
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                assert!(length < 16_384);
                let mut body = vec![0; length];
                stream.read_exact(&mut body).await.unwrap();
                requests.push(Request { head, body });
                let header = format!(
                    "HTTP/1.1 {}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    reply.status,
                    reply.headers,
                    reply.body.len()
                );
                stream.write_all(header.as_bytes()).await.unwrap();
                // The client may close early when it rejects headers/body size.
                let _ = stream.write_all(reply.body.as_bytes()).await;
            }
            requests
        });
        Self { endpoint, task }
    }

    fn provider(&self) -> DeezerProvider {
        let mut provider = DeezerProvider::new(Arl::parse("a".repeat(192)).unwrap()).unwrap();
        // Only test builds permit a loopback endpoint or cleartext HTTP.
        provider.test_endpoint = Some(self.endpoint.clone());
        provider.client = client_builder()
            .https_only(false)
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        provider
    }

    async fn finish(self) -> Vec<Request> {
        timeout(Duration::from_secs(3), self.task)
            .await
            .expect("missing HTTP request")
            .unwrap()
    }
}

#[tokio::test]
async fn authentication_search_and_session_reuse() {
    let server = Server::start(vec![
        Reply::json(PING),
        Reply::json(AUTH),
        Reply::json(TRACKS),
        Reply::json(TRACKS),
    ])
    .await;
    let provider = server.provider();
    provider.check_auth().await.unwrap();
    let query = "Écho \"quoted\" & spaces";
    assert_eq!(provider.search(query.into()).await.unwrap()[0].id, "42");
    provider.search("second".into()).await.unwrap();
    let requests = server.finish().await;
    assert!(requests[0].head.starts_with("GET "));
    assert!(requests[0].head.contains("method=deezer.ping"));
    assert!(!requests[0].head.to_lowercase().contains("cookie:"));
    assert!(requests[1].head.starts_with("POST "));
    assert!(requests[1].head.contains("method=deezer.getUserData"));
    assert!(
        requests[1]
            .head
            .contains(&format!("sid=test-session; arl={}", "a".repeat(192)))
    );
    for request in &requests[2..] {
        assert!(request.head.contains("method=search.music"));
        assert!(request.head.contains("api_token=test-token"));
    }
    let body: Value = serde_json::from_slice(&requests[2].body).unwrap();
    assert_eq!(
        body,
        json!({"query": query, "filter":"ALL", "output":"TRACK", "start":0, "nb":50})
    );
}

#[tokio::test]
async fn expired_session_is_cleared_and_next_search_authenticates_again() {
    let mut denied = Reply::json("PRIVATE_RESPONSE");
    denied.status = "401 Unauthorized";
    let server = Server::start(vec![
        Reply::json(PING),
        Reply::json(AUTH),
        denied,
        Reply::json(PING),
        Reply::json(AUTH),
        Reply::json(TRACKS),
    ])
    .await;
    let provider = server.provider();
    assert!(matches!(
        provider.search("first".into()).await,
        Err(DeezerError::Authentication)
    ));
    assert!(provider.session.lock().await.is_none());
    assert_eq!(provider.search("retry".into()).await.unwrap().len(), 1);
    assert_eq!(server.finish().await.len(), 6);
}

#[tokio::test]
async fn anonymous_cookie_stops_before_search_and_does_not_cache_session() {
    let anonymous = r#"{"error":[],"results":{"USER":{"USER_ID":0},"checkForm":"anonymous"}}"#;
    let server = Server::start(vec![Reply::json(PING), Reply::json(anonymous)]).await;
    let provider = server.provider();
    assert!(matches!(
        provider.search("test".into()).await,
        Err(DeezerError::Authentication)
    ));
    assert!(provider.session.lock().await.is_none());
    assert_eq!(server.finish().await.len(), 2);
}

#[tokio::test]
async fn html_and_forbidden_responses_do_not_claim_invalid_credentials() {
    for (status, headers, is_html) in [
        ("200 OK", "Content-Type: text/html; charset=utf-8\r\n", true),
        ("403 Forbidden", "", false),
    ] {
        let server = Server::start(vec![Reply {
            status,
            headers: headers.into(),
            body: "PRIVATE_RESPONSE".into(),
        }])
        .await;
        let error = server.provider().check_auth().await.unwrap_err();
        assert!(if is_html {
            matches!(error, DeezerError::Gateway)
        } else {
            matches!(error, DeezerError::AccessDenied)
        });
        assert!(!format!("{error} {error:?}").contains("PRIVATE_RESPONSE"));
        server.finish().await;
    }
}

#[tokio::test]
async fn redirect_is_not_followed_even_on_same_host() {
    let server = Server::start(vec![Reply {
        status: "302 Found",
        headers: "Location: /should-not-receive-cookie\r\n".into(),
        body: String::new(),
    }])
    .await;
    let provider = server.provider();
    let cookie = session_cookie(&provider.arl.read().unwrap(), "test-session").unwrap();
    let result: Result<Value, _> = provider
        .call("deezer.getUserData", "", Some(&cookie), json!({}))
        .await;
    assert!(matches!(result, Err(DeezerError::Service)));
    assert_eq!(server.finish().await.len(), 1);
}

#[tokio::test]
async fn oversized_response_is_rejected() {
    let server = Server::start(vec![Reply::json(&"x".repeat(MAX_RESPONSE_BYTES + 1))]).await;
    assert!(matches!(
        server.provider().check_auth().await,
        Err(DeezerError::Response)
    ));
    server.finish().await;
}

#[tokio::test]
async fn playback_resolves_full_media_without_leaking_cookies_to_media_requests() {
    let auth = r#"{"error":[],"results":{"USER":{"USER_ID":123,"OPTIONS":{"license_token":"test-license"}},"checkForm":"test-token"}}"#;
    let songs = r#"{"error":[],"results":{"data":[{"SNG_ID":"42","TRACK_TOKEN":"test-track"}]}}"#;
    let media = r#"{"data":[{"media":[{"media_type":"FULL","format":"MP3_128","cipher":{"type":"BF_CBC_STRIPE"},"sources":[{"url":"https://e-cdns-proxy-0.dzcdn.net/test"}]}]}]}"#;
    let server = Server::start(vec![
        Reply::json(PING),
        Reply::json(auth),
        Reply::json(songs),
        Reply::json(media),
        Reply::json("short unencrypted tail"),
    ])
    .await;
    let provider = server.provider();
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    provider.stream_track("42".into(), tx).await.unwrap();
    assert_eq!(rx.recv().await.unwrap(), b"short unencrypted tail");
    assert!(rx.recv().await.is_none());
    let requests = server.finish().await;
    assert!(requests[2].head.contains("method=song.getListData"));
    let body: Value = serde_json::from_slice(&requests[3].body).unwrap();
    assert_eq!(body["license_token"], "test-license");
    assert_eq!(body["track_tokens"], json!(["test-track"]));
    assert_eq!(body["media"][0]["formats"][0]["format"], "MP3_128");
    for request in &requests[3..] {
        assert!(!request.head.to_lowercase().contains("cookie:"));
        assert!(!request.head.contains("api_token"));
        assert!(!request.head.contains("test-license"));
    }
}

#[tokio::test]
async fn missing_playback_license_stops_before_requesting_media() {
    let server = Server::start(vec![Reply::json(PING), Reply::json(AUTH)]).await;
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    assert!(
        server
            .provider()
            .stream_track("42".into(), tx)
            .await
            .unwrap_err()
            .contains("unavailable")
    );
    assert_eq!(server.finish().await.len(), 2);
}

#[tokio::test]
async fn discovery_search_library_and_flow_use_authenticated_requests() {
    use crate::provider::{BrowseKind, BrowseResults};
    let auth = r#"{"error":[],"results":{"USER":{"USER_ID":123,"LOVEDTRACKS_ID":"99"},"checkForm":"test-token"}}"#;
    let lists =
        r#"{"error":[],"results":{"data":[{"PLAYLIST_ID":"7","TITLE":"Jazz focus","NB_SONG":5}]}}"#;
    let library =
        r#"{"error":[],"results":{"PLAYLISTS":{"data":[{"PLAYLIST_ID":"7","TITLE":"My mix"}]}}}"#;
    let flow = r#"{"error":[],"results":{"data":[{"SNG_ID":"42","SNG_TITLE":"Favorite","ART_NAME":"Artist","DURATION":1},{"SNG_ID":"43","SNG_TITLE":"Discovery","ART_NAME":"Artist","DURATION":1}]}}"#;
    let server = Server::start(vec![
        Reply::json(PING),
        Reply::json(auth),
        Reply::json(lists),
        Reply::json(library),
        Reply::json(TRACKS),
        Reply::json(TRACKS),
        Reply::json(flow),
    ])
    .await;
    let provider = server.provider();
    assert!(
        matches!(provider.browse(BrowseKind::Playlists, "jazz focus".into()).await.unwrap(), BrowseResults::Playlists(v) if v[0].title == "Jazz focus")
    );
    assert!(
        matches!(provider.browse(BrowseKind::Library, String::new()).await.unwrap(), BrowseResults::Playlists(v) if v[0].title == "My mix")
    );
    provider
        .browse(BrowseKind::Playlist("7".into()), String::new())
        .await
        .unwrap();
    assert!(
        matches!(provider.browse(BrowseKind::Discoveries, String::new()).await.unwrap(), BrowseResults::Tracks(v) if v.len() == 1 && v[0].id == "43")
    );
    let requests = server.finish().await;
    let search: Value = serde_json::from_slice(&requests[2].body).unwrap();
    assert_eq!(search["output"], "PLAYLIST");
    assert_eq!(search["query"], "jazz focus");
    assert!(requests[3].head.contains("method=deezer.userMenu"));
    let favorites: Value = serde_json::from_slice(&requests[5].body).unwrap();
    assert_eq!(favorites["playlist_id"], "99");
    assert!(requests[6].head.contains("method=radio.getUserRadio"));
}

#[tokio::test]
async fn favorite_toggle_reads_membership_before_adding_or_removing() {
    let absent = r#"{"error":[],"results":{"data":[]}}"#;
    let present = r#"{"error":[],"results":{"data":[{"SNG_ID":"42","DATE_FAVORITE":123}]}}"#;
    let success = r#"{"error":[],"results":true}"#;
    let server = Server::start(vec![
        Reply::json(PING),
        Reply::json(AUTH),
        Reply::json(absent),
        Reply::json(success),
        Reply::json(present),
        Reply::json(success),
    ])
    .await;
    let provider = server.provider();
    assert!(provider.toggle_favorite("42".into()).await.unwrap());
    assert!(!provider.toggle_favorite("42".into()).await.unwrap());
    let requests = server.finish().await;
    assert!(requests[2].head.contains("song.getFavoriteIds"));
    assert!(requests[3].head.contains("song.addFavorites"));
    assert!(requests[5].head.contains("song.removeFavorites"));
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[3].body).unwrap(),
        json!({"IDS":[42]})
    );
}

#[tokio::test]
async fn refreshing_login_replaces_cached_session_and_credentials() {
    let server = Server::start(vec![
        Reply::json(PING),
        Reply::json(AUTH),
        Reply::json(PING),
        Reply::json(AUTH),
    ])
    .await;
    let provider = server.provider();
    provider.check_auth().await.unwrap();
    provider
        .reauthenticate(Arl::parse("b".repeat(192)).unwrap())
        .await
        .unwrap();
    let requests = server.finish().await;
    assert!(requests[1].head.contains(&"a".repeat(192)));
    assert!(requests[3].head.contains(&"b".repeat(192)));
    assert!(!requests[3].head.contains(&"a".repeat(192)));
}

#[tokio::test]
async fn lyrics_use_authenticated_gateway_and_track_id() {
    let server = Server::start(vec![Reply::json(PING), Reply::json(AUTH),
        Reply::json(r#"{"error":[],"results":{"LYRICS_SYNC_JSON":[{"milliseconds":1250,"line":"Synthetic demo"}]}}"#)
    ]).await;
    let lyrics = server.provider().lyrics("42".into()).await.unwrap();
    assert_eq!(lyrics.active_line(1249), None);
    assert_eq!(lyrics.active_line(1250), Some(0));
    let requests = server.finish().await;
    assert!(requests[2].head.contains("song.getLyrics"));
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[2].body).unwrap(),
        json!({"SNG_ID":"42"})
    );
}

#[tokio::test]
async fn failed_login_refresh_preserves_previous_session() {
    let server = Server::start(vec![
        Reply::json(PING),
        Reply::json(AUTH),
        Reply::json(PING),
        Reply::json(r#"{"error":[],"results":{"USER":{"USER_ID":0},"checkForm":"0"}}"#),
    ])
    .await;
    let provider = server.provider();
    provider.check_auth().await.unwrap();
    assert!(
        provider
            .reauthenticate(Arl::parse("b".repeat(192)).unwrap())
            .await
            .is_err()
    );
    assert_eq!(provider.arl.read().unwrap().expose(), "a".repeat(192));
    assert!(provider.session.lock().await.is_some());
    server.finish().await;
}
