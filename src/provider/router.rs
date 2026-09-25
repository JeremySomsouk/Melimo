//! Provider routing belongs here, never in the player or queue.
use super::{
    BrowseKind, BrowseResults, Lyrics, MusicProvider, ProviderId, Track, deezer::DeezerProvider,
    invidious::AutomaticProvider, mock::MockProvider,
};
use tokio::sync::mpsc;

#[derive(Default)]
pub struct Providers {
    pub deezer: Option<DeezerProvider>,
    pub invidious: Option<AutomaticProvider>,
    pub mock: bool,
}
impl Providers {
    pub fn available(&self) -> Vec<ProviderId> {
        if self.mock {
            return vec![ProviderId::Mock];
        }
        let mut ids = Vec::new();
        if self.deezer.is_some() {
            ids.push(ProviderId::Deezer);
        }
        if self.invidious.is_some() {
            ids.push(ProviderId::Invidious);
        }
        ids
    }
    fn deezer(&self) -> Result<&DeezerProvider, String> {
        self.deezer
            .as_ref()
            .ok_or_else(|| "Deezer is not connected. Start melimo --login to enable it.".into())
    }
    fn invidious(&self) -> Result<&AutomaticProvider, String> {
        self.invidious.as_ref().ok_or_else(|| {
            "Invidious is disabled. Configure [invidious] and restart Mélimo.".into()
        })
    }
    pub async fn search(&self, provider: ProviderId, query: String) -> Result<Vec<Track>, String> {
        match provider {
            ProviderId::Deezer => self.deezer()?.search_tracks(query).await,
            ProviderId::Invidious => self.invidious()?.search_tracks(query).await,
            ProviderId::Mock if self.mock => MockProvider.search_tracks(query).await,
            ProviderId::Mock => Err("Offline demo is not enabled.".into()),
        }
    }
    pub async fn browse(
        &self,
        provider: ProviderId,
        kind: BrowseKind,
        query: String,
    ) -> Result<BrowseResults, String> {
        match provider {
            ProviderId::Deezer => self.deezer()?.browse(kind, query).await,
            ProviderId::Mock if self.mock => MockProvider.browse(kind, query).await,
            _ => Err("Discovery and playlists are unavailable for this provider. Use / to search videos.".into()),
        }
    }
    pub async fn stream(&self, item: Track, audio: mpsc::Sender<Vec<u8>>) -> Result<(), String> {
        match item.provider {
            ProviderId::Deezer => self.deezer()?.stream_track(item.id, audio).await,
            ProviderId::Invidious => self.invidious()?.stream(&item, audio).await,
            ProviderId::Mock => MockProvider.stream_track(item.id, audio).await,
        }
    }
    pub async fn lyrics(&self, item: Track) -> Result<Lyrics, String> {
        if item.provider != ProviderId::Deezer {
            return Err("Lyrics are unavailable for this provider.".into());
        }
        self.deezer()?.lyrics(item.id).await
    }
    pub async fn toggle_favorite(&self, item: Track) -> Result<bool, String> {
        if item.provider != ProviderId::Deezer {
            return Err("Favorites are unavailable for this provider.".into());
        }
        self.deezer()?.toggle_favorite(item.id).await
    }
    pub async fn reauthenticate(&self, arl: crate::config::Arl) -> Result<(), String> {
        self.deezer()?.reauthenticate(arl).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn unsupported_operations_never_fall_through_to_another_provider() {
        let providers = Providers {
            mock: true,
            ..Default::default()
        };
        assert_eq!(providers.available(), [ProviderId::Mock]);
        assert_eq!(
            providers
                .search(ProviderId::Mock, String::new())
                .await
                .unwrap()
                .len(),
            8
        );
        assert!(
            providers
                .search(ProviderId::Invidious, "test".into())
                .await
                .unwrap_err()
                .contains("disabled")
        );
        let mut item = MockProvider
            .search_tracks(String::new())
            .await
            .unwrap()
            .remove(0);
        item.provider = ProviderId::Invidious;
        assert!(providers.lyrics(item.clone()).await.is_err());
        assert!(providers.toggle_favorite(item.clone()).await.is_err());
        let (tx, _rx) = mpsc::channel(1);
        assert!(
            providers
                .stream(item, tx)
                .await
                .unwrap_err()
                .contains("disabled")
        );
    }
}
