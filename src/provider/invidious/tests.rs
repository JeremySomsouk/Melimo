//! Synthetic HTTP only: no public instances, account data or signed media URLs.
use super::*;
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
    time::timeout,
};

const ID: &str = "abcdefghijk";
struct Reply {
    status: u16,
    body: Vec<u8>,
}
impl Reply {
    fn json(value: serde_json::Value) -> Self {
        Self {
            status: 200,
            body: serde_json::to_vec(&value).unwrap(),
        }
    }
    fn status(status: u16) -> Self {
        Self {
            status,
            body: b"PRIVATE_UPSTREAM_DETAILS".to_vec(),
        }
    }
}
struct Server {
    url: Url,
    task: JoinHandle<Vec<String>>,
}
impl Server {
    async fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!(
            "http://{}/prefix/",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for reply in replies {
                let (mut socket, _) = timeout(Duration::from_secs(5), listener.accept())
                    .await
                    .unwrap()
                    .unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(
                        timeout(Duration::from_secs(5), socket.read_u8())
                            .await
                            .unwrap()
                            .unwrap(),
                    );
                    assert!(header.len() < 16384);
                }
                requests.push(String::from_utf8(header).unwrap());
                let head = format!(
                    "HTTP/1.1 {} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    reply.status,
                    reply.body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&reply.body).await;
            }
            requests
        });
        Self { url, task }
    }
    fn provider(&self) -> InvidiousProvider {
        let mut p = InvidiousProvider::new(self.url.clone()).unwrap();
        p.client = Client::builder()
            .no_proxy()
            .read_timeout(Duration::from_millis(200))
            .build()
            .unwrap();
        p
    }
    async fn requests(self) -> Vec<String> {
        timeout(Duration::from_secs(5), self.task)
            .await
            .unwrap()
            .unwrap()
    }
}
fn video() -> serde_json::Value {
    json!({"type":"video", "videoId":ID, "title":"Title\n\u{1b}", "author":"Channel", "lengthSeconds":120,
    "adaptiveFormats":[
        {"url":"/video", "type":"video/mp4", "bitrate":"999999"},
        {"url":"/opus", "type":"audio/webm; codecs=\"opus\"", "bitrate":"999999"},
        {"url":"/audio", "type":"audio/mp4; codecs=\"mp4a.40.2\"", "bitrate":"128000"}
    ]})
}
fn item() -> Track {
    serde_json::from_value::<Video>(video())
        .unwrap()
        .item()
        .unwrap()
}

#[tokio::test]
async fn search_metadata_resolve_and_stream_use_one_anonymous_pipeline() {
    let fixture = include_bytes!("../../../tests/fixtures/tone.m4a");
    let server = Server::start(vec![
        Reply::json(json!([video(), {"type":"channel"}])),
        Reply::json(video()),
        Reply {
            status: 200,
            body: fixture.to_vec(),
        },
    ])
    .await;
    let provider = server.provider();
    let tracks = provider.search_tracks("hello & café".into()).await.unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].provider, ProviderId::YouTube);
    assert_eq!(tracks[0].title, "Title");
    let (tx, mut rx) = mpsc::channel(2);
    let collect = tokio::spawn(async move {
        let mut bytes = Vec::new();
        while let Some(chunk) = rx.recv().await {
            bytes.extend(chunk);
        }
        bytes
    });
    provider.stream(&tracks[0], tx).await.unwrap();
    assert_eq!(collect.await.unwrap(), fixture);
    let requests = server.requests().await;
    assert!(requests[0].starts_with("GET /prefix/api/v1/search?q=hello+%26+caf%C3%A9&type=video "));
    assert!(requests[1].starts_with("GET /prefix/api/v1/videos/abcdefghijk?local=true "));
    assert!(requests[2].starts_with("GET /audio "));
    for request in requests {
        assert!(!request.to_ascii_lowercase().contains("cookie:"));
        assert!(!request.to_ascii_lowercase().contains("authorization:"));
    }
}

#[tokio::test]
async fn expired_audio_url_is_resolved_again_once_before_any_bytes_are_sent() {
    let server = Server::start(vec![
        Reply::json(video()),
        Reply::status(403),
        Reply::json(video()),
        Reply {
            status: 200,
            body: vec![1, 2, 3],
        },
    ])
    .await;
    let (tx, mut rx) = mpsc::channel(2);
    server.provider().stream(&item(), tx).await.unwrap();
    assert_eq!(rx.recv().await.unwrap(), [1, 2, 3]);
    assert_eq!(server.requests().await.len(), 4);
    let server = Server::start(vec![
        Reply::json(video()),
        Reply::status(403),
        Reply::json(video()),
        Reply::status(403),
    ])
    .await;
    let (tx, mut rx) = mpsc::channel(2);
    assert!(
        server
            .provider()
            .stream(&item(), tx)
            .await
            .unwrap_err()
            .contains("expired")
    );
    assert!(rx.recv().await.is_none());
    assert_eq!(server.requests().await.len(), 4);
}

#[tokio::test]
async fn errors_are_actionable_and_do_not_echo_upstream_details() {
    for (status, expected) in [
        (429, "rate limit"),
        (404, "unavailable"),
        (403, "restrictions"),
        (500, "HTTP error"),
    ] {
        let server = Server::start(vec![Reply::status(status)]).await;
        let error = server
            .provider()
            .search_tracks("query".into())
            .await
            .unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("PRIVATE_UPSTREAM_DETAILS"));
        server.requests().await;
    }
    for (body, expected) in [
        (json!({"error":"PRIVATE region restricted"}), "restricted"),
        (json!({"error":"SENSITIVE removed"}), "unavailable"),
        (json!({"bad":"PRIVATE"}), "Invalid"),
    ] {
        let server = Server::start(vec![Reply::json(body)]).await;
        let error = server
            .provider()
            .search_tracks("query".into())
            .await
            .unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("PRIVATE"));
        server.requests().await;
    }
}

#[tokio::test]
async fn timeout_and_unreachable_instance_are_clean_failures() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let mut p = InvidiousProvider::new(url).unwrap();
    p.client = Client::builder()
        .no_proxy()
        .read_timeout(Duration::from_millis(30))
        .build()
        .unwrap();
    let error = p.search_tracks("test".into()).await.unwrap_err();
    assert!(error.contains("timed out"), "{error}");
    drop(listener);
    assert!(
        p.search_tracks("test".into())
            .await
            .unwrap_err()
            .contains("Cannot reach")
    );
}

#[tokio::test]
async fn rejects_unusable_audio_restricted_ids_and_mismatched_metadata() {
    assert!(validate_id("../secret").is_err());
    let mut wrong = item();
    wrong.provider = ProviderId::Deezer;
    let p = InvidiousProvider::new(Url::parse("https://example.invalid/").unwrap()).unwrap();
    assert!(p.resolve_audio(&wrong).await.is_err());
    for mime in [
        "video/mp4",
        "audio/webm; codecs=\"opus\"",
        "audio/mp4; codecs=\"mp4a.40.5\"",
    ] {
        let mut body = video();
        body["adaptiveFormats"] = json!([{"url":"/audio", "type":mime}]);
        let server = Server::start(vec![Reply::json(body)]).await;
        assert!(
            server
                .provider()
                .resolve_audio(&item())
                .await
                .err()
                .unwrap()
                .contains("no usable")
        );
        server.requests().await;
    }
    let mut body = video();
    body["videoId"] = json!("xxxxxxxxxxx");
    let server = Server::start(vec![Reply::json(body)]).await;
    assert!(
        server
            .provider()
            .resolve_audio(&item())
            .await
            .err()
            .unwrap()
            .contains("different video")
    );
    server.requests().await;
}

#[tokio::test]
async fn skips_live_and_upcoming_results_and_empty_queries() {
    let mut live = video();
    live["liveNow"] = json!(true);
    let mut upcoming = video();
    upcoming["isUpcoming"] = json!(true);
    let server = Server::start(vec![Reply::json(json!([live, upcoming]))]).await;
    let p = server.provider();
    assert!(p.search_tracks("   ".into()).await.unwrap().is_empty());
    assert!(p.search_tracks("live".into()).await.unwrap().is_empty());
    server.requests().await;
}
