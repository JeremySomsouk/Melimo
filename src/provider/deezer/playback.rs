//! Account-authorized MP3 streaming. No media URLs, tokens or audio are persisted.
use super::*;
use blowfish::{
    Blowfish,
    cipher::{BlockDecrypt, KeyInit},
};
use md5::{Digest, Md5};
use tokio::sync::mpsc;

const MEDIA_ENDPOINT: &str = "https://media.deezer.com/v1/get_url";
const STRIPE: usize = 2048;
const MAX_AUDIO_BYTES: usize = 64 * 1024 * 1024;

impl DeezerProvider {
    pub(super) async fn stream(
        &self,
        id: String,
        audio: mpsc::Sender<Vec<u8>>,
    ) -> Result<(), DeezerError> {
        let numeric_id = id.parse::<u64>().map_err(|_| DeezerError::Unavailable)?;
        // Refresh account/track tokens for each playback, including expired sessions.
        let mut session = self.session.lock().await;
        *session = None;
        *session = Some(self.authenticate().await?);
        let current = session.as_ref().ok_or(DeezerError::Authentication)?;
        let license = current
            .license
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or(DeezerError::Unavailable)?;
        let songs: Value = self
            .call(
                "song.getListData",
                &current.token,
                Some(&current.cookie),
                json!({"sng_ids": [numeric_id]}),
            )
            .await?;
        let (track_id, token) = track_token(&songs)?;
        let media_endpoint = MEDIA_ENDPOINT;
        #[cfg(test)]
        let media_endpoint = self.test_endpoint.as_deref().unwrap_or(media_endpoint);
        let request = self.client.post(media_endpoint).json(&json!({
            "license_token": license,
            "media": [{"type":"FULL", "formats":[{"cipher":"BF_CBC_STRIPE", "format":"MP3_128"}]}],
            "track_tokens": [token]
        }));
        drop(session);
        let response = request.send().await.map_err(network_error)?;
        let bytes = limited_response(response, MAX_RESPONSE_BYTES).await?;
        let media: Value = serde_json::from_slice(&bytes).map_err(|_| DeezerError::Response)?;
        let url = media_url(&media)?;
        #[cfg(test)]
        let url = self
            .test_endpoint
            .as_deref()
            .map(|s| reqwest::Url::parse(s).unwrap())
            .unwrap_or(url);
        // No cookie or API token headers on the media host. Never follow redirects.
        let mut response = tokio::time::timeout(
            Duration::from_secs(20),
            self.client
                .get(url)
                .timeout(Duration::from_secs(2 * 60 * 60))
                .send(),
        )
        .await
        .map_err(|_| DeezerError::Timeout)?
        .map_err(network_error)?;
        if !response.status().is_success() {
            return Err(DeezerError::Unavailable);
        }
        let expected = response.content_length();
        if expected.is_some_and(|n| n > MAX_AUDIO_BYTES as u64) {
            return Err(DeezerError::Response);
        }
        let mut decoder = Stripes::new(&track_id);
        let mut total = 0usize;
        loop {
            // A stalled network must not leave the player waiting forever.
            let chunk = tokio::time::timeout(Duration::from_secs(20), response.chunk())
                .await
                .map_err(|_| DeezerError::Timeout)?
                .map_err(network_error)?;
            let Some(chunk) = chunk else {
                break;
            };
            total += chunk.len();
            if total > MAX_AUDIO_BYTES {
                return Err(DeezerError::Response);
            }
            for part in chunk.chunks(STRIPE) {
                for decoded in decoder.push(part) {
                    if audio.send(decoded).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
        if total == 0 || expected.is_some_and(|n| n != total as u64) {
            return Err(DeezerError::Response);
        }
        if !decoder.pending.is_empty() {
            let _ = audio.send(decoder.pending).await;
        }
        Ok(())
    }
}

fn track_token(songs: &Value) -> Result<(String, String), DeezerError> {
    let song = songs["data"]
        .as_array()
        .and_then(|a| a.first())
        .ok_or(DeezerError::Unavailable)?;
    let song = if song["FALLBACK"]["TRACK_TOKEN"].is_string() {
        &song["FALLBACK"]
    } else {
        song
    };
    let id = match &song["SNG_ID"] {
        Value::String(s) => s.parse::<u64>().map_err(|_| DeezerError::Response)?,
        n => n.as_u64().ok_or(DeezerError::Response)?,
    };
    let token = song["TRACK_TOKEN"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or(DeezerError::Unavailable)?;
    Ok((id.to_string(), token.into()))
}

fn media_url(media: &Value) -> Result<reqwest::Url, DeezerError> {
    let item = &media["data"][0];
    if item["errors"].as_array().is_some_and(|a| !a.is_empty()) {
        return Err(DeezerError::Unavailable);
    }
    let stream = &item["media"][0];
    if stream["cipher"]["type"] != "BF_CBC_STRIPE"
        || stream["format"] != "MP3_128"
        || stream["media_type"] != "FULL"
    {
        return Err(DeezerError::Unavailable);
    }
    let url = reqwest::Url::parse(
        stream["sources"][0]["url"]
            .as_str()
            .ok_or(DeezerError::Unavailable)?,
    )
    .map_err(|_| DeezerError::Response)?;
    validate_media_url(&url)?;
    Ok(url)
}

fn validate_media_url(url: &reqwest::Url) -> Result<(), DeezerError> {
    let allowed = url
        .host_str()
        .is_some_and(|h| h.ends_with(".dzcdn.net") || h.ends_with(".deezer.com"));
    if !allowed
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
    {
        return Err(DeezerError::Response);
    }
    Ok(())
}

async fn limited_response(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, DeezerError> {
    if !response.status().is_success() {
        return Err(DeezerError::Unavailable);
    }
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(DeezerError::Response);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(network_error)? {
        if bytes.len() + chunk.len() > limit {
            return Err(DeezerError::Response);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

struct Stripes {
    cipher: Blowfish,
    pending: Vec<u8>,
    index: usize,
}
impl Stripes {
    fn new(id: &str) -> Self {
        // Public web-player protocol constant; not a user credential.
        let secret = b"g4el58wc0zvf9na1";
        let digest = format!("{:x}", Md5::digest(id.as_bytes()));
        let mut key = [0; 16];
        for i in 0..16 {
            key[i] = digest.as_bytes()[i] ^ digest.as_bytes()[i + 16] ^ secret[i];
        }
        Self {
            cipher: Blowfish::new_from_slice(&key).expect("fixed 16-byte key"),
            pending: Vec::with_capacity(STRIPE),
            index: 0,
        }
    }
    fn push(&mut self, mut data: &[u8]) -> Vec<Vec<u8>> {
        let mut ready = Vec::new();
        while !data.is_empty() {
            let n = (STRIPE - self.pending.len()).min(data.len());
            self.pending.extend_from_slice(&data[..n]);
            data = &data[n..];
            if self.pending.len() == STRIPE {
                let mut stripe = std::mem::replace(&mut self.pending, Vec::with_capacity(STRIPE));
                if self.index.is_multiple_of(3) {
                    let mut previous = [0, 1, 2, 3, 4, 5, 6, 7];
                    for block in stripe.as_chunks_mut::<8>().0 {
                        let ciphertext: [u8; 8] = *block;
                        self.cipher.decrypt_block(block.into());
                        for (byte, iv) in block.iter_mut().zip(previous) {
                            *byte ^= iv;
                        }
                        previous = ciphertext;
                    }
                }
                ready.push(stripe);
                self.index += 1;
            }
        }
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blowfish::cipher::BlockEncrypt;
    #[test]
    fn stripes_survive_arbitrary_network_boundaries_and_partial_tail() {
        let plain: Vec<u8> = (0..STRIPE * 7 + 37).map(|i| (i % 251) as u8).collect();
        let mut encrypted = plain.clone();
        let cipher = Stripes::new("42").cipher;
        for (i, stripe) in encrypted.chunks_mut(STRIPE).enumerate() {
            if i.is_multiple_of(3) && stripe.len() == STRIPE {
                let mut previous = [0, 1, 2, 3, 4, 5, 6, 7];
                for block in stripe.as_chunks_mut::<8>().0 {
                    for (b, iv) in block.iter_mut().zip(previous) {
                        *b ^= iv;
                    }
                    cipher.encrypt_block(block.into());
                    previous.copy_from_slice(block);
                }
            }
        }
        for size in [1, 7, 2047, 2048, 4099] {
            let mut decoder = Stripes::new("42");
            let mut actual = Vec::new();
            for chunk in encrypted.chunks(size) {
                for stripe in decoder.push(chunk) {
                    actual.extend(stripe);
                }
            }
            actual.extend(decoder.pending);
            assert_eq!(actual, plain);
        }
    }
    #[test]
    fn media_urls_reject_external_hosts_credentials_and_cleartext() {
        for url in [
            "http://e-cdns-proxy-0.dzcdn.net/a",
            "https://dzcdn.net.evil.test/a",
            "https://127.0.0.1/a",
            "https://user@a.dzcdn.net/a",
            "https://a.dzcdn.net:444/a",
        ] {
            assert!(validate_media_url(&reqwest::Url::parse(url).unwrap()).is_err());
        }
        assert!(
            validate_media_url(
                &reqwest::Url::parse("https://e-cdns-proxy-0.dzcdn.net/mobile/1/secret").unwrap()
            )
            .is_ok()
        );
    }
    #[test]
    fn resolves_fallback_and_rejects_entitlement_errors() {
        let songs = json!({"data":[{"SNG_ID":"1","TRACK_TOKEN":"old","FALLBACK":{"SNG_ID":42,"TRACK_TOKEN":"new"}}]});
        assert_eq!(track_token(&songs).unwrap(), ("42".into(), "new".into()));
        let error = media_url(&json!({"data":[{"errors":[{"message":"PRIVATE"}]}]})).unwrap_err();
        assert!(!error.to_string().contains("PRIVATE"));
    }
}
