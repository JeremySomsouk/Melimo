use super::action::Action;
use crate::provider::{BrowseKind, BrowseResults, Playlist, ProviderId, Track};
use rand::seq::SliceRandom;
use std::collections::VecDeque;

#[derive(Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Search,
    NowPlaying,
    Discover,
    Queue,
}

#[derive(Default, PartialEq, Eq, Clone)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Loading,
    Playing,
    Paused,
    Finished,
    Error(String),
}

/// Output gain with a mute toggle that remembers the previous level.
#[derive(Clone, Copy)]
pub struct Volume {
    level: f32,
    muted: bool,
    before_mute: f32,
}

impl Default for Volume {
    fn default() -> Self {
        Self {
            level: 1.0,
            muted: false,
            before_mute: 1.0,
        }
    }
}

impl Volume {
    const STEP: f32 = 0.05;

    /// Gain actually sent to the audio device (0.0 while muted).
    pub fn effective(&self) -> f32 {
        if self.muted { 0.0 } else { self.level }
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    pub fn percent(&self) -> u8 {
        (self.effective() * 100.0).round() as u8
    }

    pub fn up(&mut self) {
        let base = self.unmuted_level();
        self.level = (base + Self::STEP).min(1.0);
        self.muted = false;
    }

    pub fn down(&mut self) {
        let base = self.unmuted_level();
        self.level = (base - Self::STEP).max(0.0);
        self.muted = false;
    }

    pub fn toggle_mute(&mut self) {
        if self.muted {
            self.muted = false;
        } else {
            self.before_mute = self.level;
            self.muted = true;
        }
    }

    fn unmuted_level(&self) -> f32 {
        if self.muted {
            self.before_mute
        } else {
            self.level
        }
    }
}

#[derive(Default)]
pub struct App {
    pub search_provider: ProviderId,
    pub providers: Vec<ProviderId>,
    pub karaoke: bool,
    pub lyrics: Option<Result<crate::provider::Lyrics, String>>,
    pub quit: bool,
    pub show_help: bool,
    pub editing: bool,
    pub query: String,
    pub tracks: Vec<Track>,
    pub selected: Option<usize>,
    pub opened: Option<Track>,
    pub view: View,
    pub loading: bool,
    pub error: Option<String>,
    pub searched: bool,
    pub playlists: Vec<Playlist>,
    pub playlist_search: bool,
    pub showing_playlists: bool,
    pub collection: Option<String>,
    pub queue: VecDeque<Track>,
    pub notice: Option<String>,
    pending_playlist_play: Option<bool>,
    request_id: u64,
    pub playback_id: u64,
    pub playback: PlaybackState,
    pub elapsed: u64,
    pub elapsed_ms: u64,
    pub buffering: bool,
    pub pause_requested: bool,
    pub volume: Volume,
}

pub struct SearchRequest {
    pub provider: ProviderId,
    pub id: u64,
    pub query: String,
    pub kind: BrowseKind,
}

pub const DISCOVER: &[(&str, &str)] = &[
    ("For you · Deezer Flow", ""),
    ("Familiar · your favorites", ""),
    ("Discover · Flow excluding favorites", ""),
    ("Your playlists", ""),
    ("Genre · Hip-hop / rap", "hip hop"),
    ("Genre · Rock", "rock"),
    ("Genre · Pop", "pop"),
    ("Genre · Jazz", "jazz"),
    ("Genre · Electronic", "electronic"),
    ("Genre · Classical", "classical"),
    ("Focus / study", "focus"),
    ("Relax / chill", "chill"),
    ("Workout / running", "workout"),
    ("Party", "party"),
    ("Sleep", "sleep"),
    ("Happy", "happy"),
    ("Sad", "sad"),
    ("Search playlists by keyword…", ""),
];

impl App {
    fn begin_playback(&mut self, track: Track) {
        self.lyrics = None;
        self.opened = Some(track);
        self.playback_id += 1;
        self.playback = PlaybackState::Loading;
        self.elapsed = 0;
        self.elapsed_ms = 0;
        self.pause_requested = false;
        self.buffering = false;
    }
    fn browse(&mut self, kind: BrowseKind, query: String) -> Option<SearchRequest> {
        self.request_id += 1;
        self.pending_playlist_play = None;
        self.loading = true;
        self.error = None;
        self.editing = false;
        self.view = View::Search;
        self.showing_playlists = matches!(kind, BrowseKind::Playlists | BrowseKind::Library);
        self.tracks.clear();
        self.playlists.clear();
        self.selected = None;
        Some(SearchRequest {
            provider: self.search_provider,
            id: self.request_id,
            query,
            kind,
        })
    }
    fn result_count(&self) -> usize {
        if self.view == View::Queue {
            self.queue.len()
        } else if self.view == View::Discover {
            DISCOVER.len()
        } else if self.showing_playlists {
            self.playlists.len()
        } else {
            self.tracks.len()
        }
    }

    pub fn update(&mut self, action: Action) -> Option<SearchRequest> {
        match action {
            Action::CycleProvider => {
                if let Some(index) = self
                    .providers
                    .iter()
                    .position(|p| *p == self.search_provider)
                {
                    self.search_provider = self.providers[(index + 1) % self.providers.len()];
                }
                self.request_id += 1;
                self.pending_playlist_play = None;
                self.loading = false;
                self.playlist_search = false;
                self.showing_playlists = false;
                self.tracks.clear();
                self.playlists.clear();
                self.selected = None;
                self.collection = None;
                self.error = None;
                self.searched = false;
                self.view = View::Search;
                self.notice = Some(format!(
                    "{} search · / to edit",
                    self.search_provider.name()
                ));
            }
            Action::ToggleLyrics => {
                self.karaoke = !self.karaoke;
                if self.opened.is_some() {
                    self.view = View::NowPlaying;
                }
            }
            Action::LyricsFinished { id, result } => {
                if id == self.playback_id {
                    self.lyrics = Some(result);
                }
            }
            Action::Login | Action::ToggleFavorite | Action::Redraw => {}
            Action::Enqueue => {
                if self.view == View::Search
                    && !self.showing_playlists
                    && let Some(track) = self.selected.and_then(|i| self.tracks.get(i)).cloned()
                {
                    self.queue.push_back(track);
                    self.notice = Some("Added to the queue. Press b to view it.".into());
                }
            }
            Action::RemoveQueued => {
                if self.view == View::Queue
                    && let Some(index) = self.selected
                {
                    self.queue.remove(index);
                    self.selected = self.queue.len().checked_sub(1).map(|last| index.min(last));
                }
            }
            Action::FavoriteFinished(result) => {
                self.notice = Some(match result {
                    Ok(true) => "Added to Deezer favorites.".into(),
                    Ok(false) => "Removed from Deezer favorites.".into(),
                    Err(error) => error,
                })
            }
            Action::ShowPlayer => {
                if self.opened.is_some() {
                    self.view = View::NowPlaying;
                    self.editing = false;
                } else {
                    self.notice = Some("Choose a track or playlist first.".into());
                }
            }
            Action::ShowQueue => {
                self.view = View::Queue;
                self.editing = false;
                self.selected = (!self.queue.is_empty()).then_some(0);
            }
            Action::PlayAll | Action::ShufflePlay
                if self.showing_playlists && self.view == View::Search =>
            {
                if let Some(playlist) = self.selected.and_then(|i| self.playlists.get(i)).cloned() {
                    let shuffle = matches!(action, Action::ShufflePlay);
                    self.collection = Some(format!("{} · first 1,000 tracks", playlist.title));
                    let request = self.browse(BrowseKind::Playlist(playlist.id), String::new());
                    self.pending_playlist_play = Some(shuffle);
                    return request;
                }
            }
            Action::ShufflePlay => {
                let mut tracks: Vec<Track> = if self.view == View::Search && !self.showing_playlists
                {
                    self.tracks.clone()
                } else {
                    self.queue.iter().cloned().collect()
                };
                if !tracks.is_empty() {
                    tracks.shuffle(&mut rand::rng());
                    self.queue = tracks.into();
                    if let Some(track) = self.queue.pop_front() {
                        self.begin_playback(track);
                        self.view = View::NowPlaying;
                    }
                }
            }
            Action::Discover => {
                self.request_id += 1; // Ignore an outstanding search when changing views.
                self.loading = false;
                self.view = View::Discover;
                self.pending_playlist_play = None;
                self.editing = false;
                self.selected = Some(0);
                if self.search_provider == ProviderId::YouTube {
                    self.view = View::Search;
                    self.selected = (!self.tracks.is_empty()).then_some(0);
                    self.notice = Some("YouTube: / to search videos · P switches provider.".into());
                }
            }
            Action::ToggleSearchKind => {
                if self.search_provider == ProviderId::YouTube {
                    self.notice = Some(
                        "YouTube playlists are not available yet. Use / to search videos.".into(),
                    );
                    return None;
                }
                self.playlist_search = !self.playlist_search;
                self.showing_playlists = self.playlist_search;
                self.request_id += 1;
                self.loading = false;
                self.tracks.clear();
                self.playlists.clear();
                self.selected = None;
                self.collection = None;
                self.error = None;
                self.searched = false;
                self.view = View::Search;
            }
            Action::PlayAll if !self.tracks.is_empty() && !self.showing_playlists => {
                self.queue = self.tracks.iter().skip(1).cloned().collect();
                self.begin_playback(self.tracks[0].clone());
                self.view = View::NowPlaying;
            }
            Action::PlayAll => {}
            Action::NextTrack => {
                if let Some(track) = self.queue.pop_front() {
                    self.begin_playback(track);
                } else {
                    self.update(Action::StopPlayback);
                }
            }
            Action::BrowseFinished { id, result } if id == self.request_id => {
                self.loading = false;
                self.searched = true;
                match result {
                    Ok(BrowseResults::Tracks(tracks)) => {
                        self.tracks = tracks;
                        self.showing_playlists = false;
                    }
                    Ok(BrowseResults::Playlists(lists)) => {
                        self.playlists = lists;
                        self.showing_playlists = true;
                    }
                    Err(error) => self.error = Some(error),
                }
                self.selected = (self.result_count() > 0).then_some(0);
                if let Some(shuffle) = self.pending_playlist_play.take()
                    && self.error.is_none()
                    && !self.tracks.is_empty()
                {
                    self.update(if shuffle {
                        Action::ShufflePlay
                    } else {
                        Action::PlayAll
                    });
                }
            }
            Action::BrowseFinished { .. } => {}
            Action::TogglePause => {
                if matches!(
                    self.playback,
                    PlaybackState::Loading | PlaybackState::Playing | PlaybackState::Paused
                ) {
                    self.pause_requested = !self.pause_requested;
                }
            }
            Action::VolumeUp => self.volume.up(),
            Action::VolumeDown => self.volume.down(),
            Action::ToggleMute => self.volume.toggle_mute(),
            Action::SeekRelative(delta) => {
                if let Some(track) = &self.opened
                    && !matches!(
                        self.playback,
                        PlaybackState::Stopped | PlaybackState::Error(_)
                    )
                {
                    self.elapsed = self
                        .elapsed
                        .saturating_add_signed(delta)
                        .min(track.duration_secs.saturating_sub(1));
                    self.elapsed_ms = self.elapsed.saturating_mul(1000);
                    self.playback_id += 1;
                    self.playback = PlaybackState::Loading;
                    self.buffering = true;
                }
            }
            Action::StopPlayback => {
                self.pending_playlist_play = None;
                self.playback_id += 1;
                self.playback = PlaybackState::Stopped;
                self.queue.clear();
                self.buffering = false;
            }
            Action::PlaybackUpdate {
                id,
                state,
                elapsed_ms,
                buffering,
            } => {
                if id == self.playback_id && !matches!(self.playback, PlaybackState::Error(_)) {
                    let finished = state == PlaybackState::Finished;
                    self.playback = state;
                    self.elapsed_ms = elapsed_ms;
                    self.elapsed = elapsed_ms / 1000;
                    self.buffering = buffering;
                    if finished && let Some(track) = self.queue.pop_front() {
                        self.begin_playback(track);
                    }
                }
            }
            Action::Quit => self.quit = true,
            Action::Back if self.show_help => self.show_help = false,
            Action::Back if self.editing => self.editing = false,
            Action::Back if self.view == View::Queue => self.view = View::Search,
            Action::Back if self.view == View::NowPlaying => self.view = View::Search,
            Action::Back if self.view == View::Search => {
                if self.search_provider == ProviderId::YouTube {
                    self.quit = true;
                    return None;
                }
                self.view = View::Discover;
                self.selected = Some(0);
                self.request_id += 1;
                self.loading = false;
            }
            Action::Back => self.quit = true,
            Action::ToggleHelp => self.show_help = !self.show_help,
            Action::FocusSearch => {
                self.view = View::Search;
                self.editing = true;
                self.selected = (self.result_count() > 0).then_some(0);
            }
            Action::Insert(c) => self.query.push(c),
            Action::Backspace => {
                self.query.pop();
            }
            Action::ClearQuery => self.query.clear(),
            Action::SubmitSearch => {
                self.collection = None;
                return self.browse(
                    if self.playlist_search {
                        BrowseKind::Playlists
                    } else {
                        BrowseKind::Tracks
                    },
                    self.query.trim().into(),
                );
            }
            Action::SearchFinished { id, result } if id == self.request_id => {
                self.loading = false;
                self.searched = true;
                match result {
                    Ok(tracks) => {
                        self.selected = (!tracks.is_empty()).then_some(0);
                        self.tracks = tracks;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            Action::SearchFinished { .. } => {} // Ignore obsolete responses.
            Action::MoveDown => {
                if let Some(index) = self.selected {
                    self.selected = Some((index + 1).min(self.result_count().saturating_sub(1)));
                }
            }
            Action::MoveUp => {
                self.selected = self.selected.map(|i| i.saturating_sub(1));
            }
            Action::First => self.selected = (self.result_count() > 0).then_some(0),
            Action::Last => self.selected = self.result_count().checked_sub(1),
            Action::OpenTrack if self.view == View::Queue => {
                if let Some(index) = self.selected
                    && index < self.queue.len()
                {
                    self.queue.drain(..index);
                    if let Some(track) = self.queue.pop_front() {
                        self.begin_playback(track);
                        self.view = View::NowPlaying;
                    }
                }
            }
            Action::OpenTrack if self.view == View::Discover => {
                if let Some(index) = self.selected {
                    self.collection = None;
                    if index == 0 {
                        self.collection = Some("For you · Flow".into());
                        return self.browse(BrowseKind::Flow, String::new());
                    }
                    if index == 1 {
                        self.collection = Some("Familiar · favorites (first 1,000)".into());
                        return self.browse(BrowseKind::Favorites, String::new());
                    }
                    if index == 2 {
                        self.collection = Some("Flow · excluding first 1,000 favorites".into());
                        return self.browse(BrowseKind::Discoveries, String::new());
                    }
                    if index == 3 {
                        self.collection = Some("Your playlists".into());
                        return self.browse(BrowseKind::Library, String::new());
                    }
                    self.playlist_search = true;
                    if let Some((_, query)) = DISCOVER.get(index) {
                        self.query = (*query).into();
                        if query.is_empty() {
                            self.view = View::Search;
                            self.editing = true;
                            self.showing_playlists = true;
                            self.tracks.clear();
                            self.playlists.clear();
                            self.selected = None;
                            self.searched = false;
                            self.error = None;
                        } else {
                            return self.browse(BrowseKind::Playlists, (*query).into());
                        }
                    }
                }
            }
            Action::OpenTrack if self.showing_playlists => {
                if let Some(playlist) = self.selected.and_then(|i| self.playlists.get(i)).cloned() {
                    self.collection = Some(format!("{} · first 1,000 tracks", playlist.title));
                    return self.browse(BrowseKind::Playlist(playlist.id), String::new());
                }
            }
            Action::OpenTrack => {
                if let Some(track) = self.selected.and_then(|i| self.tracks.get(i)).cloned() {
                    self.queue.clear();
                    self.begin_playback(track);
                    self.view = View::NowPlaying;
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Track {
        Track {
            provider: crate::provider::ProviderId::Mock,
            id: "mock:0".into(),
            title: "Title".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            duration_secs: 123,
        }
    }

    #[test]
    fn mixed_queue_preserves_provider_even_when_ids_match() {
        let mut deezer = track();
        deezer.provider = crate::provider::ProviderId::Deezer;
        let mut youtube = deezer.clone();
        youtube.provider = crate::provider::ProviderId::YouTube;
        let mut app = App {
            tracks: vec![deezer.clone(), youtube.clone()],
            selected: Some(0),
            ..App::default()
        };
        app.update(Action::PlayAll);
        assert_eq!(app.opened.as_ref(), Some(&deezer));
        app.tracks.clear();
        app.update(Action::NextTrack);
        assert_eq!(app.opened.as_ref(), Some(&youtube));
        app.update(Action::TogglePause);
        app.update(Action::SeekRelative(10));
        assert!(app.pause_requested);
        assert_eq!(app.opened.as_ref(), Some(&youtube));
        app.update(Action::StopPlayback);
        assert!(app.queue.is_empty());
    }

    #[test]
    fn provider_switch_invalidates_search_but_preserves_player_and_queue() {
        let mut app = App {
            providers: vec![ProviderId::Deezer, ProviderId::YouTube],
            tracks: vec![track(), track()],
            ..App::default()
        };
        app.update(Action::PlayAll);
        let playing = app.opened.clone();
        let playback_id = app.playback_id;
        let request = app.update(Action::SubmitSearch).unwrap();
        assert_eq!(request.provider, ProviderId::Deezer);
        app.update(Action::CycleProvider);
        app.update(Action::SearchFinished {
            id: request.id,
            result: Ok(vec![track()]),
        });
        assert!(app.tracks.is_empty());
        assert_eq!(app.opened, playing);
        assert_eq!(app.playback_id, playback_id);
        assert_eq!(app.queue.len(), 1);
        assert_eq!(app.search_provider, ProviderId::YouTube);
        app.update(Action::ToggleSearchKind);
        assert!(!app.playlist_search);
        app.update(Action::Discover);
        assert!(app.view == View::Search);
        let request = app.update(Action::SubmitSearch).unwrap();
        assert_eq!(request.provider, ProviderId::YouTube);
        assert!(matches!(request.kind, BrowseKind::Tracks));
    }

    #[test]
    fn volume_steps_mute_and_restore() {
        let mut app = App::default();
        assert_eq!(app.volume.effective(), 1.0);
        app.update(Action::VolumeDown);
        assert!((app.volume.effective() - 0.95).abs() < 1e-6);
        app.update(Action::ToggleMute);
        assert!(app.volume.is_muted());
        assert_eq!(app.volume.effective(), 0.0);
        assert_eq!(app.volume.percent(), 0);
        app.update(Action::ToggleMute);
        assert!(!app.volume.is_muted());
        assert!((app.volume.effective() - 0.95).abs() < 1e-6);
        // Stepping up while muted unmutes and raises from the remembered level.
        app.update(Action::ToggleMute);
        app.update(Action::VolumeUp);
        assert!(!app.volume.is_muted());
        assert!((app.volume.effective() - 1.0).abs() < 1e-6);
        for _ in 0..5 {
            app.update(Action::VolumeUp);
        }
        assert_eq!(app.volume.effective(), 1.0);
    }

    #[test]
    fn seek_clamps_preserves_pause_and_rejects_old_events() {
        let mut app = App {
            tracks: vec![track()],
            selected: Some(0),
            ..App::default()
        };
        app.update(Action::OpenTrack);
        app.update(Action::TogglePause);
        let old_id = app.playback_id;
        app.update(Action::SeekRelative(500));
        assert_eq!(app.elapsed, 122);
        assert!(app.pause_requested);
        app.update(Action::PlaybackUpdate {
            id: old_id,
            state: PlaybackState::Playing,
            elapsed_ms: 1000,
            buffering: false,
        });
        assert_eq!(app.elapsed, 122);
        app.update(Action::LyricsFinished {
            id: old_id,
            result: Ok(Default::default()),
        });
        assert!(app.lyrics.is_none());
        app.update(Action::SeekRelative(-500));
        assert_eq!(app.elapsed, 0);
        app.update(Action::TogglePause);
        assert!(!app.pause_requested);
    }

    #[test]
    fn stop_cancels_pending_playlist_autoplay() {
        let mut app = App {
            pending_playlist_play: Some(true),
            ..App::default()
        };
        app.update(Action::StopPlayback);
        app.update(Action::BrowseFinished {
            id: app.request_id,
            result: Ok(BrowseResults::Tracks(vec![track()])),
        });
        assert!(app.opened.is_none());
        assert!(app.playback == PlaybackState::Stopped);
    }

    #[test]
    fn player_shortcut_and_shuffle_preserve_the_selected_set() {
        let tracks: Vec<_> = (0..8)
            .map(|i| {
                let mut t = track();
                t.id = i.to_string();
                t
            })
            .collect();
        let mut app = App {
            tracks,
            selected: Some(0),
            ..App::default()
        };
        app.update(Action::ShufflePlay);
        let mut ids = vec![app.opened.as_ref().unwrap().id.clone()];
        ids.extend(app.queue.iter().map(|t| t.id.clone()));
        ids.sort();
        assert_eq!(ids, (0..8).map(|i| i.to_string()).collect::<Vec<_>>());
        let playing_id = app.playback_id;
        app.update(Action::ShowQueue);
        app.update(Action::RemoveQueued);
        assert_eq!(app.queue.len(), 6);
        app.update(Action::ShowPlayer);
        assert!(app.view == View::NowPlaying);
        assert_eq!(app.playback_id, playing_id);
    }
    #[test]
    fn playlist_play_shortcut_loads_then_starts_the_queue() {
        let mut app = App {
            showing_playlists: true,
            playlists: vec![Playlist {
                id: "42".into(),
                title: "Mix".into(),
                tracks: 2,
            }],
            selected: Some(0),
            ..App::default()
        };
        let request = app.update(Action::PlayAll).unwrap();
        assert!(app.opened.is_none());
        app.update(Action::BrowseFinished {
            id: request.id,
            result: Ok(BrowseResults::Tracks(vec![track(), track()])),
        });
        assert!(app.view == View::NowPlaying);
        assert_eq!(app.queue.len(), 1);
    }
    #[test]
    fn play_all_advances_its_snapshot_even_after_browsing_elsewhere() {
        let mut second = track();
        second.id = "second".into();
        let mut app = App {
            tracks: vec![track(), second.clone()],
            selected: Some(0),
            ..App::default()
        };
        app.update(Action::PlayAll);
        assert_eq!(app.queue.len(), 1);
        let id = app.playback_id;
        app.update(Action::Discover);
        app.tracks.clear();
        app.update(Action::PlaybackUpdate {
            id,
            state: PlaybackState::Finished,
            elapsed_ms: 123,
            buffering: false,
        });
        assert_eq!(app.opened.as_ref().unwrap().id, "second");
        assert!(app.view == View::Discover);
        assert!(app.playback == PlaybackState::Loading);
        app.update(Action::PlaybackUpdate {
            id,
            state: PlaybackState::Error("old".into()),
            elapsed_ms: 0,
            buffering: false,
        });
        assert!(app.playback == PlaybackState::Loading);
        app.update(Action::StopPlayback);
        assert!(app.queue.is_empty());
    }
    #[test]
    fn genre_opens_playlist_search_and_enter_inspects_before_playing() {
        let mut app = App::default();
        app.update(Action::Discover);
        app.selected = Some(4);
        let request = app.update(Action::OpenTrack).unwrap();
        assert!(matches!(request.kind, BrowseKind::Playlists));
        assert_eq!(request.query, "hip hop");
        app.update(Action::BrowseFinished {
            id: request.id,
            result: Ok(BrowseResults::Playlists(vec![Playlist {
                id: "42".into(),
                title: "Rap".into(),
                tracks: 2,
            }])),
        });
        let request = app.update(Action::OpenTrack).unwrap();
        assert!(matches!(request.kind, BrowseKind::Playlist(id) if id == "42"));
        assert!(app.opened.is_none());
    }
    #[test]
    fn stopped_and_replaced_tracks_ignore_late_playback_updates() {
        let mut app = App {
            tracks: vec![track()],
            selected: Some(0),
            ..App::default()
        };
        app.update(Action::OpenTrack);
        let old = app.playback_id;
        app.update(Action::StopPlayback);
        app.update(Action::PlaybackUpdate {
            id: old,
            state: PlaybackState::Playing,
            elapsed_ms: 10,
            buffering: false,
        });
        assert!(app.playback == PlaybackState::Stopped);
        app.update(Action::OpenTrack);
        app.update(Action::PlaybackUpdate {
            id: old,
            state: PlaybackState::Error("obsolete".into()),
            elapsed_ms: 0,
            buffering: false,
        });
        assert!(app.playback == PlaybackState::Loading);
        app.update(Action::PlaybackUpdate {
            id: app.playback_id,
            state: PlaybackState::Error("network".into()),
            elapsed_ms: 0,
            buffering: false,
        });
        app.update(Action::PlaybackUpdate {
            id: app.playback_id,
            state: PlaybackState::Finished,
            elapsed_ms: 10,
            buffering: false,
        });
        assert!(matches!(app.playback, PlaybackState::Error(_)));
    }
    #[test]
    fn newer_search_wins_even_if_old_response_arrives_last() {
        let mut app = App::default();
        let old = app.update(Action::SubmitSearch).unwrap().id;
        let new = app.update(Action::SubmitSearch).unwrap().id;
        app.update(Action::SearchFinished {
            id: new,
            result: Ok(vec![track()]),
        });
        app.update(Action::SearchFinished {
            id: old,
            result: Err("obsolete".into()),
        });
        assert_eq!(app.tracks.len(), 1);
        assert!(!app.loading);
        assert!(app.error.is_none());
    }

    #[test]
    fn navigation_handles_empty_results_and_boundaries() {
        let mut app = App::default();
        for action in [
            Action::MoveDown,
            Action::MoveUp,
            Action::Last,
            Action::OpenTrack,
        ] {
            app.update(action);
        }
        assert!(app.selected.is_none());
        let id = app.update(Action::SubmitSearch).unwrap().id;
        app.update(Action::SearchFinished {
            id,
            result: Ok(vec![track(), track()]),
        });
        for _ in 0..5 {
            app.update(Action::MoveDown);
        }
        assert_eq!(app.selected, Some(1));
        app.update(Action::First);
        app.update(Action::MoveUp);
        assert_eq!(app.selected, Some(0));
        app.update(Action::OpenTrack);
        assert!(app.opened.is_some());
        app.update(Action::Back);
        assert!(app.view == View::Search);
        assert!(!app.quit);
    }

    #[test]
    fn failed_search_recovers_and_unicode_backspace_is_safe() {
        let mut app = App::default();
        app.update(Action::Insert('é'));
        app.update(Action::Backspace);
        assert!(app.query.is_empty());
        let id = app.update(Action::SubmitSearch).unwrap().id;
        app.update(Action::SearchFinished {
            id,
            result: Err("Unavailable".into()),
        });
        assert!(!app.loading);
        assert!(app.error.is_some());
        let id = app.update(Action::SubmitSearch).unwrap().id;
        app.update(Action::SearchFinished {
            id,
            result: Ok(vec![]),
        });
        assert!(app.error.is_none());
        assert!(app.selected.is_none());
    }
}
