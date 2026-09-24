use super::{DeezerError, DeezerProvider};
use crate::provider::{LyricLine, Lyrics};
use serde_json::{Value, json};

impl DeezerProvider {
    pub(super) async fn fetch_lyrics(&self, id: String) -> Result<Lyrics, DeezerError> {
        let mut session = self.session.lock().await;
        if session.is_none() {
            *session = Some(self.authenticate().await?);
        }
        let current = session.as_ref().ok_or(DeezerError::Authentication)?;
        let response = self
            .call::<Value>(
                "song.getLyrics",
                &current.token,
                Some(&current.cookie),
                json!({"SNG_ID": id}),
            )
            .await;
        match response {
            Ok(value) => parse(value),
            Err(error) => {
                *session = None;
                Err(error)
            }
        }
    }
}

fn parse(value: Value) -> Result<Lyrics, DeezerError> {
    if !value.is_object() {
        return Err(DeezerError::Response);
    }
    let data = &value;
    let mut lines = Vec::new();
    // Some gateway versions encode this array as a JSON string.
    let sync = data["LYRICS_SYNC_JSON"]
        .as_str()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .unwrap_or_else(|| data["LYRICS_SYNC_JSON"].clone());
    if let Some(entries) = sync.as_array() {
        for entry in entries.iter().take(10000) {
            let time = entry["milliseconds"]
                .as_u64()
                .or_else(|| entry["milliseconds"].as_str().and_then(|s| s.parse().ok()))
                .or_else(|| entry["lrc_timestamp"].as_str().and_then(timestamp));
            if let (Some(at_ms), Some(text)) = (time, entry["line"].as_str()) {
                lines.push(LyricLine {
                    at_ms,
                    text: clean(text),
                });
            }
        }
    }
    lines.sort_by_key(|line| line.at_ms);
    Ok(Lyrics {
        lines,
        plain: clean(data["LYRICS_TEXT"].as_str().unwrap_or_default()),
        credits: ["LYRICS_COPYRIGHTS", "LYRICS_WRITERS"]
            .iter()
            .filter_map(|key| data[*key].as_str())
            .map(clean)
            .collect::<Vec<_>>()
            .join(" · "),
    })
}

fn timestamp(value: &str) -> Option<u64> {
    let (minutes, seconds) = value.trim_matches(['[', ']']).split_once(':')?;
    let minutes: u64 = minutes.parse().ok()?;
    let seconds: f64 = seconds.parse().ok()?;
    if !seconds.is_finite() || !(0.0..60.0).contains(&seconds) {
        return None;
    }
    minutes
        .checked_mul(60000)?
        .checked_add((seconds * 1000.0).round() as u64)
}

fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_timestamps_sorts_and_preserves_plain_fallback() {
        let lyrics = parse(json!({
            "LYRICS_SYNC_JSON": [{"milliseconds": "2000", "line": "Second"},
                {"milliseconds": 0, "line": "First"}, {"line": "invalid"}],
            "LYRICS_TEXT": "Original demo text", "LYRICS_WRITERS": "Demo author"
        }))
        .unwrap();
        assert_eq!(lyrics.lines.len(), 2);
        assert_eq!(lyrics.active_line(1999), Some(0));
        assert_eq!(lyrics.active_line(2000), Some(1));
        assert_eq!(lyrics.plain, "Original demo text");
        assert!(parse(Value::Null).is_err());
        let encoded = parse(
            json!({"LYRICS_SYNC_JSON": "[{\"lrc_timestamp\":\"[01:02.50]\",\"line\":\"Demo\"}]"}),
        )
        .unwrap();
        assert_eq!(encoded.lines[0].at_ms, 62500);
        assert_eq!(timestamp("[0:NaN]"), None);
    }
}
