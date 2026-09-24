use super::*;
use crate::provider::{BrowseKind, BrowseResults, Playlist};

impl DeezerProvider {
    pub(super) async fn favorite(&self, id: String) -> Result<bool, DeezerError> {
        let song_id = id.parse::<u64>().map_err(|_| DeezerError::Response)?;
        let mut session = self.session.lock().await;
        if session.is_none() {
            *session = Some(self.authenticate().await?);
        }
        let current = session.as_ref().ok_or(DeezerError::Authentication)?;
        // Resolve actual membership each time, rather than guessing from the UI.
        // Page all favorite IDs so a toggle cannot mistake an older favorite for new.
        let mut start = 0;
        let mut present = false;
        loop {
            let result: Value = self
                .call(
                    "song.getFavoriteIds",
                    &current.token,
                    Some(&current.cookie),
                    json!({"start":start,"nb":1000}),
                )
                .await?;
            let ids = result["data"].as_array().ok_or(DeezerError::Response)?;
            for value in ids {
                let value = value.get("SNG_ID").unwrap_or(value);
                let n = value
                    .as_u64()
                    .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
                    .ok_or(DeezerError::Response)?;
                present |= n == song_id;
            }
            if present || ids.len() < 1000 {
                break;
            }
            start += 1000;
            if start >= 100_000 {
                return Err(DeezerError::Response);
            }
        }
        let method = if present {
            "song.removeFavorites"
        } else {
            "song.addFavorites"
        };
        let result: Value = self
            .call(
                method,
                &current.token,
                Some(&current.cookie),
                json!({"IDS":[song_id]}),
            )
            .await?;
        if result == Value::Bool(false) {
            return Err(DeezerError::Service);
        }
        Ok(!present)
    }

    pub(super) async fn discover(
        &self,
        kind: BrowseKind,
        query: String,
    ) -> Result<BrowseResults, DeezerError> {
        let mut session = self.session.lock().await;
        if session.is_none() {
            *session = Some(self.authenticate().await?);
        }
        let current = session.as_ref().ok_or(DeezerError::Authentication)?;
        let result = self.discover_session(current, kind, query).await;
        if result.is_err() {
            *session = None;
        }
        result
    }

    async fn playlist_tracks(
        &self,
        session: &Session,
        id: &str,
    ) -> Result<Vec<Track>, DeezerError> {
        let mut tracks = Vec::new();
        for start in (0..1000).step_by(100) {
            let page: SearchResults = self
                .call(
                    "playlist.getSongs",
                    &session.token,
                    Some(&session.cookie),
                    json!({"playlist_id":id,"start":start,"nb":100}),
                )
                .await?;
            let page = page.tracks()?;
            let count = page.len();
            tracks.extend(page.into_iter().take(100));
            if count < 100 {
                break;
            }
        }
        Ok(tracks)
    }

    async fn discover_session(
        &self,
        current: &Session,
        kind: BrowseKind,
        query: String,
    ) -> Result<BrowseResults, DeezerError> {
        if let BrowseKind::Playlist(id) = &kind {
            return self
                .playlist_tracks(current, id)
                .await
                .map(BrowseResults::Tracks);
        }
        if matches!(kind, BrowseKind::Favorites) {
            return self
                .playlist_tracks(
                    current,
                    current
                        .favorites
                        .as_deref()
                        .ok_or(DeezerError::Unavailable)?,
                )
                .await
                .map(BrowseResults::Tracks);
        }
        let favorites = if matches!(kind, BrowseKind::Discoveries) {
            self.playlist_tracks(
                current,
                current
                    .favorites
                    .as_deref()
                    .ok_or(DeezerError::Unavailable)?,
            )
            .await?
        } else {
            Vec::new()
        };
        let (method, body) = match &kind {
            BrowseKind::Tracks => (
                "search.music",
                json!({"query":query,"filter":"ALL","output":"TRACK","start":0,"nb":50}),
            ),
            BrowseKind::Playlists => (
                "search.music",
                json!({"query":query,"filter":"ALL","output":"PLAYLIST","start":0,"nb":50}),
            ),
            BrowseKind::Library => ("deezer.userMenu", json!({})),
            _ => ("radio.getUserRadio", json!({"user_id":current.user_id})),
        };
        let value: Value = self
            .call(method, &current.token, Some(&current.cookie), body)
            .await?;
        match kind {
            BrowseKind::Playlists => Ok(BrowseResults::Playlists(playlists(value)?)),
            BrowseKind::Library => Ok(BrowseResults::Playlists(playlists(
                value["PLAYLISTS"].clone(),
            )?)),
            _ => {
                let results: SearchResults =
                    serde_json::from_value(value).map_err(|_| DeezerError::Response)?;
                let mut tracks = results.tracks()?;
                if matches!(kind, BrowseKind::Discoveries) {
                    let ids: std::collections::HashSet<_> =
                        favorites.into_iter().map(|t| t.id).collect();
                    tracks.retain(|t| !ids.contains(&t.id));
                }
                Ok(BrowseResults::Tracks(tracks))
            }
        }
    }
}

fn playlists(value: Value) -> Result<Vec<Playlist>, DeezerError> {
    #[derive(serde::Deserialize)]
    struct Item {
        #[serde(rename = "PLAYLIST_ID")]
        id: response::Number,
        #[serde(rename = "TITLE")]
        title: String,
        #[serde(rename = "NB_SONG")]
        count: Option<response::Number>,
    }
    #[derive(serde::Deserialize)]
    struct List {
        data: Vec<Item>,
    }
    let list: List = serde_json::from_value(value).map_err(|_| DeezerError::Response)?;
    list.data
        .into_iter()
        .map(|item| {
            Ok(Playlist {
                id: item.id.get()?.to_string(),
                title: response::display_text(item.title),
                tracks: item.count.map(|c| c.get()).transpose()?.unwrap_or(0),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_playlist_search_and_library_items_safely() {
        let lists = playlists(json!({"data":[{"PLAYLIST_ID":"123","TITLE":"Focus\n\u{001b}","NB_SONG":"25"},{"PLAYLIST_ID":456,"TITLE":"Relax"}]})).unwrap();
        assert_eq!(
            lists[0],
            Playlist {
                id: "123".into(),
                title: "Focus".into(),
                tracks: 25
            }
        );
        assert_eq!(lists[1].tracks, 0);
    }
}
