pub mod deezer;
pub mod mock;

use std::future::Future;

/// Identity travels with every item, including snapshots in a mixed queue.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProviderId {
    #[default]
    Deezer,
    YouTube,
    Mock,
}

impl ProviderId {
    pub fn label(self) -> &'static str {
        match self {
            Self::Deezer => "DZR",
            Self::YouTube => "YT",
            Self::Mock => "DEMO",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Deezer => "Deezer",
            Self::YouTube => "YouTube / Invidious",
            Self::Mock => "Mock · offline",
        }
    }
}

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn provider_identity_is_explicit_and_labels_are_distinct() {
        assert_ne!(ProviderId::Deezer, ProviderId::YouTube);
        assert_ne!(ProviderId::Deezer.label(), ProviderId::YouTube.label());
        assert_eq!(ProviderId::YouTube.name(), "YouTube / Invidious");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub provider: ProviderId,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    pub id: String,
    pub title: String,
    pub tracks: u64,
}
#[derive(Clone, Default)]
pub enum BrowseKind {
    #[default]
    Tracks,
    Playlists,
    Playlist(String),
    Library,
    Flow,
    Favorites,
    Discoveries,
}
pub enum BrowseResults {
    Tracks(Vec<Track>),
    Playlists(Vec<Playlist>),
}

#[derive(Default)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    pub plain: String,
    pub credits: String,
}
pub struct LyricLine {
    pub at_ms: u64,
    pub text: String,
}
impl Lyrics {
    pub fn active_line(&self, elapsed_ms: u64) -> Option<usize> {
        self.lines
            .partition_point(|line| line.at_ms <= elapsed_ms)
            .checked_sub(1)
    }
}

pub trait MusicProvider: Send + Sync + 'static {
    fn lyrics(&self, _id: String) -> impl Future<Output = Result<Lyrics, String>> + Send {
        async { Err("Lyrics are unavailable for this provider.".into()) }
    }
    fn reauthenticate(
        &self,
        _arl: crate::config::Arl,
    ) -> impl Future<Output = Result<(), String>> + Send {
        async { Err("Restart in Deezer mode to log in; mock mode stays offline.".into()) }
    }
    fn toggle_favorite(&self, _id: String) -> impl Future<Output = Result<bool, String>> + Send {
        async { Err("Favorites are unavailable in mock mode.".into()) }
    }
    fn browse(
        &self,
        _kind: BrowseKind,
        _query: String,
    ) -> impl Future<Output = Result<BrowseResults, String>> + Send {
        async { Err("Discovery is unavailable for this provider.".into()) }
    }
    fn stream_track(
        &self,
        _id: String,
        _audio: tokio::sync::mpsc::Sender<Vec<u8>>,
    ) -> impl Future<Output = Result<(), String>> + Send {
        async { Err("Mock tracks are metadata only. Use Deezer mode for audio playback.".into()) }
    }
    fn name(&self) -> &'static str;
    fn search_tracks(
        &self,
        query: String,
    ) -> impl Future<Output = Result<Vec<Track>, String>> + Send;
}
