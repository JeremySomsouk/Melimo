//! Lazy discovery and bounded measurements of actual audio delivery.
use super::*;
use std::time::Instant;
use tokio::sync::Mutex;

const REGISTRY: &str = "https://api.invidious.io/instances.json?sort_by=health";
const CACHE_TTL: Duration = Duration::from_secs(600);
const PROBE_BYTES: usize = 64 * 1024;

pub struct AutomaticProvider {
    fixed: Option<InvidiousProvider>,
    discovery: InvidiousProvider,
    cache: Mutex<Option<(Instant, Vec<Url>)>>,
}

impl AutomaticProvider {
    pub fn new(instance: Option<Url>) -> Result<Self, String> {
        Ok(Self {
            fixed: instance.map(InvidiousProvider::new).transpose()?,
            discovery: InvidiousProvider::new(
                Url::parse(REGISTRY).map_err(|_| "Invalid registry URL.")?,
            )?,
            cache: Mutex::new(None),
        })
    }

    async fn candidates(&self) -> Result<Vec<Url>, String> {
        let mut cache = self.cache.lock().await;
        if let Some((at, urls)) = cache.as_ref()
            && at.elapsed() < CACHE_TTL
        {
            return Ok(urls.clone());
        }
        let value = self.discovery.json(self.discovery.instance.clone()).await?;
        let urls = registry_urls(&value);
        if urls.is_empty() {
            return Err("No HTTPS Invidious API instances are available. Retry later or configure an instance.".into());
        }
        *cache = Some((Instant::now(), urls.clone()));
        Ok(urls)
    }

    async fn failed(&self, url: &Url) {
        let mut cache = self.cache.lock().await;
        if let Some((_, urls)) = cache.as_mut() {
            urls.retain(|candidate| candidate != url);
            if urls.is_empty() {
                *cache = None;
            }
        }
    }

    pub async fn search_tracks(&self, query: String) -> Result<Vec<Track>, String> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        if let Some(provider) = &self.fixed {
            return provider.search_tracks(query).await;
        }
        for url in self.candidates().await? {
            let provider = InvidiousProvider::new(url.clone())?;
            match tokio::time::timeout(
                Duration::from_secs(4),
                provider.search_tracks(query.clone()),
            )
            .await
            {
                Ok(Ok(tracks)) => return Ok(tracks),
                _ => self.failed(&url).await,
            }
        }
        Err(
            "No discovered Invidious instance could search. Retry later or configure an instance."
                .into(),
        )
    }

    pub async fn stream(&self, item: &Track, audio: mpsc::Sender<Vec<u8>>) -> Result<(), String> {
        if let Some(provider) = &self.fixed {
            return provider.stream(item, audio).await;
        }
        // At most three concurrent requests targeting 64 KiB per candidate. Keep the
        // response open so the winning probe becomes playback without redownloading.
        let mut best = None;
        for batch in self.candidates().await?.chunks(3) {
            let mut probes = tokio::task::JoinSet::new();
            for url in batch.iter().cloned() {
                let item = item.clone();
                probes.spawn(async move {
                    let result =
                        tokio::time::timeout(Duration::from_secs(6), probe(url.clone(), &item))
                            .await;
                    (url, result)
                });
            }
            while let Some(result) = probes.join_next().await {
                if let Ok((url, result)) = result {
                    match result {
                        Ok(Ok(sample)) => {
                            if best.as_ref().is_none_or(|previous: &Sample| {
                                sample.bytes_per_second > previous.bytes_per_second
                            }) {
                                best = Some(sample);
                            }
                        }
                        _ => self.failed(&url).await,
                    }
                }
            }
            if best.is_some() {
                break;
            }
        }
        let Some(mut sample) = best else {
            return Err(
                "No discovered instance could deliver audio. Retry later or configure an instance."
                    .into(),
            );
        };
        let expected = sample.response.content_length();
        let mut received = sample.prefix.len() as u64;
        for bytes in sample.prefix.chunks(2048) {
            if audio.send(bytes.to_vec()).await.is_err() {
                return Ok(());
            }
        }
        loop {
            let chunk = match sample.response.chunk().await {
                Ok(chunk) => chunk,
                Err(error) => {
                    self.failed(&sample.instance).await;
                    return Err(network_error(error));
                }
            };
            let Some(chunk) = chunk else {
                break;
            };
            received += chunk.len() as u64;
            for bytes in chunk.chunks(2048) {
                if audio.send(bytes.to_vec()).await.is_err() {
                    return Ok(());
                }
            }
        }
        if received == 0 || expected.is_some_and(|size| size != received) {
            self.failed(&sample.instance).await;
            return Err("Audio stream ended unexpectedly. Select the item to retry.".into());
        }
        Ok(())
    }
}

struct Sample {
    instance: Url,
    response: Response,
    prefix: Vec<u8>,
    bytes_per_second: f64,
}

async fn probe(instance: Url, item: &Track) -> Result<Sample, String> {
    let provider = InvidiousProvider::new(instance.clone())?;
    let source = provider.resolve_audio(item).await?;
    let start = Instant::now();
    let mut response = provider.open_stream(source).await?;
    status_error(response.status())?;
    let mut prefix = Vec::new();
    while prefix.len() < PROBE_BYTES {
        let Some(chunk) = response.chunk().await.map_err(network_error)? else {
            break;
        };
        if prefix.len() + chunk.len() > 256 * 1024 {
            return Err("Audio probe exceeded its buffer limit.".into());
        }
        prefix.extend_from_slice(&chunk);
    }
    if prefix.is_empty() {
        return Err("Empty audio stream.".into());
    }
    let bytes_per_second = prefix.len() as f64 / start.elapsed().as_secs_f64().max(0.001);
    Ok(Sample {
        instance,
        response,
        prefix,
        bytes_per_second,
    })
}

fn registry_urls(value: &serde_json::Value) -> Vec<Url> {
    let Some(rows) = value.as_array() else {
        return Vec::new();
    };
    let mut urls = Vec::new();
    for row in rows.iter().take(200) {
        let Some(info) = row.get(1) else {
            continue;
        };
        if info.get("api").and_then(|v| v.as_bool()) != Some(true) {
            continue;
        }
        let Some(uri) = info.get("uri").and_then(|v| v.as_str()) else {
            continue;
        };
        let Ok(url) = crate::config::invidious::validate_url(uri) else {
            continue;
        };
        if url.scheme() == "https" && !urls.contains(&url) {
            urls.push(url);
            if urls.len() == 6 {
                break;
            }
        }
    }
    urls
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_filters_disabled_insecure_and_duplicate_instances() {
        let entries = serde_json::json!([
            ["one", {"api":true,"uri":"https://one.invalid"}],
            ["duplicate", {"api":true,"uri":"https://one.invalid/"}],
            ["disabled", {"api":false,"uri":"https://disabled.invalid"}],
            ["http", {"api":true,"uri":"http://http.invalid"}],
            ["credentials", {"api":true,"uri":"https://user:password@bad.invalid"}],
            ["two", {"api":true,"uri":"https://two.invalid"}]
        ]);
        let urls = registry_urls(&entries);
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0].as_str(), "https://one.invalid/");
        assert_eq!(urls[1].as_str(), "https://two.invalid/");
        assert!(registry_urls(&serde_json::json!({})).is_empty());
    }

    #[tokio::test]
    async fn search_fails_over_and_evicts_failed_candidate() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        let bad = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bad_url = Url::parse(&format!("http://{}/", bad.local_addr().unwrap())).unwrap();
        let good = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let good_url = Url::parse(&format!("http://{}/", good.local_addr().unwrap())).unwrap();
        let first = tokio::spawn(async move {
            let (mut socket, _) = bad.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.unwrap());
            }
            let reply =
                b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            socket.write_all(reply).await.unwrap();
        });
        let second = tokio::spawn(async move {
            let (mut socket, _) = good.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.unwrap());
            }
            let reply = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]";
            socket.write_all(reply).await.unwrap();
        });
        let provider = AutomaticProvider::new(None).unwrap();
        *provider.cache.lock().await = Some((Instant::now(), vec![bad_url, good_url.clone()]));
        let results = tokio::time::timeout(
            Duration::from_secs(2),
            provider.search_tracks("music".into()),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(results.is_empty());
        assert_eq!(provider.candidates().await.unwrap(), vec![good_url]);
        first.await.unwrap();
        second.await.unwrap();
    }

    #[tokio::test]
    async fn empty_search_does_not_discover_and_explicit_instance_is_preserved() {
        let provider = AutomaticProvider::new(None).unwrap();
        assert!(
            provider
                .search_tracks(String::new())
                .await
                .unwrap()
                .is_empty()
        );
        assert!(provider.cache.lock().await.is_none());
        let url = Url::parse("https://explicit.invalid/").unwrap();
        let fixed = AutomaticProvider::new(Some(url.clone())).unwrap();
        assert_eq!(fixed.fixed.unwrap().instance, url);
    }

    async fn audio_server(delay_ms: u64, byte: u8) -> (Url, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let source = url.join("audio").unwrap().to_string();
        let task = tokio::spawn(async move {
            for media in [false, true] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(socket.read_u8().await.unwrap());
                }
                let body = if media {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    vec![byte; 128 * 1024]
                } else {
                    serde_json::to_vec(&serde_json::json!({
                        "videoId": "abcdefghijk", "title": "Synthetic", "author": "Test",
                        "lengthSeconds": 10,
                        "adaptiveFormats": [{
                            "url": source, "type": "audio/mp4; codecs=mp4a.40.2",
                            "bitrate": "128000"
                        }]
                    }))
                    .unwrap()
                };
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                socket.write_all(header.as_bytes()).await.unwrap();
                // A losing probe may close the connection before consuming the body.
                let _ = socket.write_all(&body).await;
            }
        });
        (url, task)
    }

    #[tokio::test]
    async fn faster_audio_wins_and_probe_bytes_are_delivered_exactly_once() {
        let (slow, slow_task) = audio_server(300, 1).await;
        let (fast, fast_task) = audio_server(0, 2).await;
        let provider = AutomaticProvider::new(None).unwrap();
        *provider.cache.lock().await = Some((Instant::now(), vec![slow, fast]));
        let item = Track {
            provider: ProviderId::Invidious,
            id: "abcdefghijk".into(),
            title: "Synthetic".into(),
            artist: "Test".into(),
            album: String::new(),
            duration_secs: 10,
        };
        let (tx, mut rx) = mpsc::channel(256);
        tokio::time::timeout(Duration::from_secs(3), provider.stream(&item, tx))
            .await
            .unwrap()
            .unwrap();
        let mut bytes = Vec::new();
        while let Some(chunk) = rx.recv().await {
            bytes.extend(chunk);
        }
        assert_eq!(bytes, vec![2; 128 * 1024]);
        slow_task.await.unwrap();
        fast_task.await.unwrap();
    }
}
