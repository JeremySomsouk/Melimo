use super::DeezerError;
use crate::provider::Track;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;

// Do not derive Debug for envelopes: auth responses can contain account data.
#[derive(Deserialize)]
struct Envelope {
    error: Value,
    results: Value,
}

pub(super) fn results<T: DeserializeOwned>(body: &[u8]) -> Result<T, DeezerError> {
    let envelope: Envelope = serde_json::from_slice(body).map_err(|_| DeezerError::Response)?;
    let success = match &envelope.error {
        Value::Array(errors) => errors.is_empty(),
        Value::Object(errors) => errors.is_empty(),
        Value::Null => true,
        _ => false,
    };
    if !success {
        return Err(DeezerError::Service);
    }
    serde_json::from_value(envelope.results).map_err(|_| DeezerError::Response)
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Number {
    Integer(u64),
    Text(String),
}

impl Number {
    pub(super) fn get(self) -> Result<u64, DeezerError> {
        match self {
            Self::Integer(n) => Ok(n),
            Self::Text(s) => s.parse().map_err(|_| DeezerError::Response),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct Ping {
    #[serde(rename = "SESSION")]
    pub session: String,
}

#[derive(Deserialize)]
struct User {
    #[serde(rename = "USER_ID")]
    id: Number,
    #[serde(rename = "OPTIONS", default)]
    options: Value,
    #[serde(rename = "LOVEDTRACKS_ID", default)]
    favorites: Option<Number>,
}

#[derive(Deserialize)]
pub(super) struct UserData {
    #[serde(rename = "USER")]
    user: User,
    #[serde(rename = "checkForm")]
    token: Option<String>,
}

impl UserData {
    pub fn favorites_id(&self) -> Option<String> {
        self.user.favorites.as_ref().and_then(|n| match n {
            Number::Integer(id) => Some(id.to_string()),
            Number::Text(s) => s.parse::<u64>().ok().map(|id| id.to_string()),
        })
    }
    pub fn user_id(&self) -> Result<u64, DeezerError> {
        match &self.user.id {
            Number::Integer(id) => Ok(*id),
            Number::Text(id) => id.parse().map_err(|_| DeezerError::Response),
        }
    }
    pub fn license_token(&self) -> Option<String> {
        self.user.options["license_token"]
            .as_str()
            .map(str::to_owned)
    }
    pub fn authenticated_token(self) -> Result<String, DeezerError> {
        if self.user.id.get()? == 0 {
            return Err(DeezerError::Authentication);
        }
        self.token
            .filter(|token| !token.is_empty() && token != "0" && token != "null")
            .ok_or(DeezerError::Authentication)
    }
}

#[derive(Deserialize)]
pub(super) struct SearchResults {
    data: Vec<Song>,
}

#[derive(Deserialize)]
struct Song {
    #[serde(rename = "SNG_ID")]
    id: Number,
    #[serde(rename = "SNG_TITLE")]
    title: String,
    #[serde(rename = "ART_NAME")]
    artist: String,
    #[serde(rename = "ALB_TITLE", default)]
    album: String,
    #[serde(rename = "DURATION")]
    duration: Number,
}

pub(super) fn display_text(value: String) -> String {
    // Provider metadata must not inject terminal controls or multi-line rows.
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(512)
        .collect()
}

impl SearchResults {
    pub fn tracks(self) -> Result<Vec<Track>, DeezerError> {
        self.data
            .into_iter()
            .map(|song| {
                Ok(Track {
                    provider: crate::provider::ProviderId::Deezer,
                    id: song.id.get()?.to_string(),
                    title: display_text(song.title),
                    artist: display_text(song.artist),
                    album: display_text(song.album),
                    duration_secs: song.duration.get()?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_string_and_numeric_fields_without_account_fixtures() {
        let body = br#"{"error":[],"results":{"data":[{"SNG_ID":"42","SNG_TITLE":"Example","ART_NAME":"Artist","ALB_TITLE":"Album","DURATION":"61"},{"SNG_ID":43,"SNG_TITLE":"Other\n\u001b","ART_NAME":"Artist","DURATION":120}]}}"#;
        let tracks = results::<SearchResults>(body).unwrap().tracks().unwrap();
        assert_eq!(tracks[0].id, "42");
        assert_eq!(tracks[0].duration_secs, 61);
        assert_eq!(tracks[1].title, "Other");
        assert!(tracks[1].album.is_empty());
    }
    #[test]
    fn anonymous_user_is_not_authenticated() {
        let body = br#"{"error":{},"results":{"USER":{"USER_ID":"0"},"checkForm":"anonymous"}}"#;
        assert!(matches!(
            results::<UserData>(body).unwrap().authenticated_token(),
            Err(DeezerError::Authentication)
        ));
        let body =
            br#"{"error":[],"results":{"USER":{"USER_ID":123},"checkForm":"synthetic-token"}}"#;
        assert_eq!(
            results::<UserData>(body)
                .unwrap()
                .authenticated_token()
                .unwrap(),
            "synthetic-token"
        );
    }
    #[test]
    fn malformed_or_service_errors_never_echo_body() {
        for body in [
            br#"{"error":{"message":"PRIVATE_COOKIE"},"results":{}}"#.as_slice(),
            b"PRIVATE_COOKIE",
            br#"{"error":[],"results":{"data":[{}]}}"#,
        ] {
            let error = match results::<SearchResults>(body) {
                Err(e) => e,
                Ok(_) => panic!("expected failure"),
            };
            assert!(!error.to_string().contains("PRIVATE_COOKIE"));
            assert!(!format!("{error:?}").contains("PRIVATE_COOKIE"));
        }
    }
}
