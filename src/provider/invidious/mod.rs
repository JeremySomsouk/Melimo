//! Anonymous Invidious API access, separate from stream resolution and transport.
use super::{AudioStream, MusicProvider, ProviderId, StreamResolver, Track};
use reqwest::{Client, Response, StatusCode, Url};
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;

pub struct InvidiousProvider {
    instance: Url,
    client: Client,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Video {
    video_id: String,
    title: String,
    author: String,
    length_seconds: u64,
    #[serde(default)]
    live_now: bool,
    #[serde(default)]
    is_upcoming: bool,
    #[serde(default)]
    adaptive_formats: Vec<Format>,
}
#[derive(Deserialize)]
struct Format {
    url: String,
    #[serde(rename = "type")]
    mime: String,
    #[serde(default)]
    bitrate: String,
}

impl Video {
    fn item(&self) -> Result<Track, String> {
        validate_id(&self.video_id)?;
        if self.live_now || self.is_upcoming || self.length_seconds == 0 {
            return Err(
                "YouTube live and upcoming streams are not supported yet. Choose a recorded video."
                    .into(),
            );
        }
        Ok(Track {
            provider: ProviderId::YouTube,
            id: self.video_id.clone(),
            title: display_text(&self.title),
            artist: display_text(&self.author),
            album: String::new(),
            duration_secs: self.length_seconds,
        })
    }
}
fn display_text(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(512)
        .collect()
}
fn validate_id(id: &str) -> Result<(), String> {
    if id.len() == 11
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        Ok(())
    } else {
        Err("Invalid YouTube video identifier.".into())
    }
}
fn network_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "Invidious request timed out. Retry or check your configured instance."
    } else {
        "Cannot reach Invidious or its audio stream. Check the instance and your connection."
    }
    .into()
}
fn status_error(status: StatusCode) -> Result<(), String> {
    if status.is_success() {
        return Ok(());
    }
    Err(match status {
        StatusCode::TOO_MANY_REQUESTS => "Invidious rate limit reached. Wait before retrying.",
        StatusCode::NOT_FOUND | StatusCode::GONE => {
            "YouTube video or Invidious endpoint is unavailable."
        }
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            "YouTube access denied: the video may have age, region or account restrictions."
        }
        _ => "Invidious returned an HTTP error. Retry or check your configured instance.",
    }
    .into())
}

impl InvidiousProvider {
    pub fn new(instance: Url) -> Result<Self, String> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|_| "Cannot initialize Invidious HTTP client.")?;
        Ok(Self { instance, client })
    }

    async fn json(&self, url: Url) -> Result<serde_json::Value, String> {
        let mut response = self
            .client
            .get(url)
            .timeout(Duration::from_secs(25))
            .send()
            .await
            .map_err(network_error)?;
        status_error(response.status())?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                return Err("Invidious response is too large.".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| "Invalid Invidious response. Check that the instance API is enabled.")?;
        if let Some(error) = value.get("error") {
            // Classify without displaying upstream text/URLs or private instance details.
            let message = error.as_str().unwrap_or_default().to_ascii_lowercase();
            return Err(if message.contains("age") || message.contains("region") || message.contains("country") || message.contains("sign in") || message.contains("private") || message.contains("restrict") {
                "YouTube video is restricted (age, region or account). Choose another video."
            } else { "YouTube video is unavailable or the instance could not retrieve it. Try another video or instance." }.into());
        }
        Ok(value)
    }

    async fn metadata(&self, id: &str) -> Result<Video, String> {
        validate_id(id)?;
        let url = self
            .instance
            .join(&format!("api/v1/videos/{id}"))
            .map_err(|_| "Invalid Invidious endpoint.")?;
        let video: Video = serde_json::from_value(self.json(url).await?)
            .map_err(|_| "Invalid Invidious video metadata.")?;
        if video.video_id != id {
            return Err("Invidious returned metadata for a different video.".into());
        }
        video.item()?;
        Ok(video)
    }

    async fn open_stream(&self, stream: AudioStream) -> Result<Response, String> {
        self.client
            .get(stream.url)
            .send()
            .await
            .map_err(network_error)
    }

    pub async fn stream(&self, item: &Track, audio: mpsc::Sender<Vec<u8>>) -> Result<(), String> {
        let mut response = self.open_stream(self.resolve_audio(item).await?).await?;
        if matches!(
            response.status(),
            StatusCode::FORBIDDEN | StatusCode::GONE | StatusCode::UNAUTHORIZED
        ) {
            // Signed URLs can expire: resolve once more before sending any bytes.
            response = self.open_stream(self.resolve_audio(item).await?).await?;
            if matches!(
                response.status(),
                StatusCode::FORBIDDEN | StatusCode::GONE | StatusCode::UNAUTHORIZED
            ) {
                return Err("YouTube audio URL expired or access was denied after refresh. Try another video or instance.".into());
            }
        }
        status_error(response.status())?;
        let expected = response.content_length();
        let mut received = 0u64;
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            received += chunk.len() as u64;
            for bytes in chunk.chunks(2048) {
                if audio.send(bytes.to_vec()).await.is_err() {
                    return Ok(());
                }
            }
        }
        if received == 0 || expected.is_some_and(|size| received != size) {
            return Err(
                "YouTube audio stream ended unexpectedly. Select the item to retry.".into(),
            );
        }
        Ok(())
    }
}

impl StreamResolver for InvidiousProvider {
    async fn resolve_audio(&self, item: &Track) -> Result<AudioStream, String> {
        if item.provider != ProviderId::YouTube {
            return Err("Invidious cannot resolve this media provider.".into());
        }
        let video = self.metadata(&item.id).await?;
        let mut formats: Vec<_> = video
            .adaptive_formats
            .into_iter()
            .filter(|f| {
                let mime = f.mime.to_ascii_lowercase();
                mime.split(';')
                    .next()
                    .is_some_and(|t| t.trim() == "audio/mp4")
                    && mime.contains("mp4a.40.2") // AAC-LC supported by the existing decoder.
            })
            .collect();
        formats.sort_by_key(|f| std::cmp::Reverse(f.bitrate.parse::<u64>().unwrap_or(0)));
        for format in formats {
            if let Ok(url) = self.instance.join(&format.url)
                && matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none()
            {
                return Ok(AudioStream { url });
            }
        }
        Err("YouTube has no usable audio-only AAC/M4A format. Try another video or Invidious instance.".into())
    }
}

impl MusicProvider for InvidiousProvider {
    async fn search_tracks(&self, query: String) -> Result<Vec<Track>, String> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut url = self
            .instance
            .join("api/v1/search")
            .map_err(|_| "Invalid Invidious endpoint.")?;
        url.query_pairs_mut()
            .append_pair("q", query.trim())
            .append_pair("type", "video");
        let result = self.json(url).await?;
        let rows = result
            .as_array()
            .ok_or("Invalid Invidious search response.")?;
        let mut tracks = Vec::new();
        for row in rows {
            if row.get("type").and_then(|v| v.as_str()) != Some("video") {
                continue;
            }
            let video: Video = serde_json::from_value(row.clone())
                .map_err(|_| "Invalid Invidious search metadata.")?;
            if video.live_now || video.is_upcoming || video.length_seconds == 0 {
                continue;
            }
            tracks.push(video.item()?);
        }
        Ok(tracks)
    }
}

#[cfg(test)]
mod tests;
