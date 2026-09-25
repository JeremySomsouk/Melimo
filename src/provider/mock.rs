use super::{MusicProvider, Track};

pub struct MockProvider;

impl MusicProvider for MockProvider {
    async fn browse(
        &self,
        kind: super::BrowseKind,
        query: String,
    ) -> Result<super::BrowseResults, String> {
        use super::{BrowseKind, BrowseResults, Playlist};
        tokio::task::yield_now().await;
        match kind {
            BrowseKind::Playlists | BrowseKind::Library => {
                Ok(BrowseResults::Playlists(vec![Playlist {
                    id: "mock:after-hours".into(),
                    title: format!(
                        "{} · offline demo",
                        if query.is_empty() {
                            "After Hours"
                        } else {
                            &query
                        }
                    ),
                    tracks: 8,
                }]))
            }
            _ => Ok(BrowseResults::Tracks(catalog())),
        }
    }

    async fn search_tracks(&self, query: String) -> Result<Vec<Track>, String> {
        // Fictional metadata, not playable tracks. Yield through the same task path
        // real providers will use without introducing artificial latency.
        tokio::task::yield_now().await;
        let query = query.trim().to_lowercase();
        Ok(catalog()
            .into_iter()
            .filter(|track| {
                format!("{} {} {}", track.title, track.artist, track.album)
                    .to_lowercase()
                    .contains(&query)
            })
            .collect())
    }
}

fn catalog() -> Vec<Track> {
    [
        ("Night Train", "Paper Satellites", "After Hours", 213),
        ("Blue Window", "Paper Satellites", "After Hours", 184),
        ("Lumière", "Écho", "Matin", 196),
        ("Le départ", "Écho", "Matin", 241),
        ("Slow River", "Juniper", "Outside", 205),
        ("Open Sky", "Juniper", "Outside", 178),
        ("Small Hours", "Quiet Company", "Rooms", 263),
        ("Home Again", "Quiet Company", "Rooms", 227),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (title, artist, album, duration_secs))| Track {
        provider: crate::provider::ProviderId::Mock,
        id: format!("mock:{index}"),
        title: title.into(),
        artist: artist.into(),
        album: album.into(),
        duration_secs,
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn search_matches_title_artist_and_album_without_case() {
        for query in [" NIGHT ", "satellites", "after hours", "ÉCHO"] {
            assert!(
                !MockProvider
                    .search_tracks(query.into())
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
        assert_eq!(
            MockProvider
                .search_tracks(String::new())
                .await
                .unwrap()
                .len(),
            8
        );
        assert!(
            MockProvider
                .search_tracks("no match".into())
                .await
                .unwrap()
                .is_empty()
        );
    }
}
