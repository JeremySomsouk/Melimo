pub mod action;
pub mod state;

use crate::{
    provider::{BrowseKind, router::Providers},
    tui,
};
use action::Action;
use crossterm::{execute, terminal::SetTitle};
use ratatui::widgets::TableState;
use state::{App, PlaybackState, SearchRequest};
use std::{io, sync::Arc};
use tokio::{sync::mpsc, task::JoinHandle};

fn apply(
    app: &mut App,
    action: Action,
    playback: &mut Option<crate::playback::Playback>,
    provider: &Arc<Providers>,
    tx: &mpsc::Sender<Action>,
) -> Option<SearchRequest> {
    let old_id = app.playback_id;
    let volume_changed = matches!(
        action,
        Action::VolumeUp | Action::VolumeDown | Action::ToggleMute
    );
    if matches!(action, Action::TogglePause)
        && let Some(player) = playback.as_ref()
    {
        player.toggle_pause();
    }
    let request = app.update(action);
    if volume_changed && let Some(player) = playback.as_ref() {
        player.set_volume(app.volume.effective());
    }
    if app.playback_id != old_id {
        *playback = None;
        if app.playback == PlaybackState::Loading
            && let Some(track) = &app.opened
        {
            *playback = Some(crate::playback::Playback::start(
                Arc::clone(provider),
                track.clone(),
                app.playback_id,
                tx.clone(),
                app.elapsed,
                app.pause_requested,
                app.volume.effective(),
            ));
        }
    } else if matches!(
        app.playback,
        PlaybackState::Error(_) | PlaybackState::Finished
    ) {
        *playback = None;
    }
    request
}

pub fn run(terminal: &mut ratatui::DefaultTerminal, provider: Providers) -> io::Result<()> {
    let provider = Arc::new(provider);
    let mut app = App::default();
    app.providers = provider.available();
    app.search_provider = app.providers.first().copied().unwrap_or_default();
    app.update(Action::Discover);
    let mut table = TableState::default();
    let (tx, mut rx) = mpsc::channel(8);
    let mut playback = None;
    let mut job: Option<JoinHandle<()>> = None;
    let mut favorite_job: Option<JoinHandle<()>> = None;
    let mut lyrics_job: Option<JoinHandle<()>> = None;
    let mut lyrics_id = None;
    let mut dirty = true;
    let mut last_title: Option<String> = None;
    let result = (|| {
        while !app.quit {
            while let Ok(action) = rx.try_recv() {
                dirty |= action_changes_display(&app, &action);
                if matches!(action, Action::FavoriteFinished(_)) {
                    favorite_job = None;
                }
                apply(&mut app, action, &mut playback, &provider, &tx);
            }
            if lyrics_id.is_some_and(|id| id != app.playback_id) {
                if let Some(job) = lyrics_job.take() {
                    job.abort();
                }
                lyrics_id = None;
            }
            if app.karaoke
                && app.lyrics.is_none()
                && lyrics_id.is_none()
                && let Some(track) = &app.opened
            {
                let track_id = track.clone();
                let id = app.playback_id;
                lyrics_id = Some(id);
                let provider = Arc::clone(&provider);
                let tx = tx.clone();
                lyrics_job = Some(tokio::spawn(async move {
                    let result = provider.lyrics(track_id).await;
                    let _ = tx.send(Action::LyricsFinished { id, result }).await;
                }));
            }
            if dirty {
                terminal.draw(|frame| {
                    tui::ui::render(frame, &app, app.search_provider.name(), &mut table)
                })?;
                dirty = false;
                let title = terminal_title(&app);
                if last_title.as_deref() != Some(title.as_str()) {
                    let _ = execute!(io::stdout(), SetTitle(title.clone()));
                    last_title = Some(title);
                }
            }
            if let Some(action) = tui::event::read_action(&app)? {
                dirty = true;
                if matches!(action, Action::Login) {
                    if provider.deezer.is_none() {
                        app.notice = Some(
                            "Run melimo --login to enable Deezer login in this session.".into(),
                        );
                        continue;
                    }
                    apply(
                        &mut app,
                        Action::StopPlayback,
                        &mut playback,
                        &provider,
                        &tx,
                    );
                    if let Some(job) = job.take() {
                        job.abort();
                    }
                    if let Some(job) = favorite_job.take() {
                        job.abort();
                    }
                    app.update(Action::Discover);
                    ratatui::restore();
                    let result = crate::config::login_arl().and_then(|arl| {
                        tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current()
                                .block_on(provider.reauthenticate(arl.clone()))
                        })?;
                        Ok(crate::config::store::save(&arl).is_ok())
                    });
                    while rx.try_recv().is_ok() {}
                    *terminal = ratatui::init();
                    app.notice = Some(match result {
                        Ok(true) => "Deezer session refreshed.".into(),
                        Ok(false) => "Session refreshed, but login could not be saved.".into(),
                        Err(error) => error,
                    });
                    continue;
                }
                if matches!(action, Action::ToggleFavorite) {
                    if favorite_job.is_none() {
                        let track = match app.view {
                            state::View::NowPlaying => app.opened.as_ref(),
                            state::View::Queue => app.selected.and_then(|i| app.queue.get(i)),
                            state::View::Search if !app.showing_playlists => {
                                app.selected.and_then(|i| app.tracks.get(i))
                            }
                            _ => None,
                        };
                        if let Some(track) = track {
                            let id = track.clone();
                            let provider = Arc::clone(&provider);
                            let tx = tx.clone();
                            app.notice = Some("Updating favorite…".into());
                            favorite_job = Some(tokio::spawn(async move {
                                let result = provider.toggle_favorite(id).await;
                                let _ = tx.send(Action::FavoriteFinished(result)).await;
                            }));
                        }
                    }
                    continue;
                }
                if let Some(request) = apply(&mut app, action, &mut playback, &provider, &tx) {
                    if let Some(previous) = job.take() {
                        previous.abort();
                    }
                    let provider = Arc::clone(&provider);
                    let tx = tx.clone();
                    job = Some(tokio::spawn(async move {
                        let action = if matches!(request.kind, BrowseKind::Tracks) {
                            Action::SearchFinished {
                                id: request.id,
                                result: provider.search(request.provider, request.query).await,
                            }
                        } else {
                            Action::BrowseFinished {
                                id: request.id,
                                result: provider
                                    .browse(request.provider, request.kind, request.query)
                                    .await,
                            }
                        };
                        let _ = tx.send(action).await;
                    }));
                }
            }
        }
        Ok(())
    })();
    if let Some(job) = job {
        job.abort();
    }
    if let Some(job) = favorite_job {
        job.abort();
    }
    if let Some(job) = lyrics_job {
        job.abort();
    }
    drop(playback);
    let _ = execute!(io::stdout(), SetTitle(String::new()));
    result
}

// Keep millisecond state current without redrawing identical frames at 10 Hz.
fn action_changes_display(app: &App, action: &Action) -> bool {
    if let Action::PlaybackUpdate {
        id,
        state,
        elapsed_ms,
        buffering,
    } = action
    {
        if *id != app.playback_id || matches!(app.playback, PlaybackState::Error(_)) {
            return false;
        }
        let active_line = |time| {
            app.lyrics
                .as_ref()
                .and_then(|r| r.as_ref().ok())
                .and_then(|lyrics| lyrics.active_line(time))
        };
        return *state != app.playback
            || *buffering != app.buffering
            || elapsed_ms / 1000 != app.elapsed
            || (app.karaoke
                && app.view == state::View::NowPlaying
                && active_line(*elapsed_ms) != active_line(app.elapsed_ms));
    }
    true
}

/// Title shown in the terminal tab/pane while playing.
fn terminal_title(app: &App) -> String {
    match &app.opened {
        Some(track) => {
            let icon = match &app.playback {
                PlaybackState::Playing if app.buffering => "…",
                PlaybackState::Playing => "▶",
                PlaybackState::Paused => "⏸",
                PlaybackState::Loading => "…",
                _ => "■",
            };
            format!("{icon} {} — {} · Mélimo", track.artist, track.title)
                .chars()
                .filter(|c| !c.is_control())
                .take(256)
                .collect()
        }
        None => "Mélimo".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_ticks_only_redraw_visible_changes() {
        let mut app = App::default();
        app.playback = PlaybackState::Playing;
        let tick = |ms| Action::PlaybackUpdate {
            id: 0,
            state: PlaybackState::Playing,
            elapsed_ms: ms,
            buffering: false,
        };
        assert!(!action_changes_display(&app, &tick(100)));
        assert!(action_changes_display(&app, &tick(1000)));
        app.karaoke = true;
        app.view = state::View::NowPlaying;
        app.lyrics = Some(Ok(crate::provider::Lyrics {
            lines: vec![crate::provider::LyricLine {
                at_ms: 250,
                text: "Synthetic".into(),
            }],
            ..Default::default()
        }));
        assert!(action_changes_display(&app, &tick(300)));
        app.elapsed_ms = 300;
        assert!(!action_changes_display(&app, &tick(400)));
        app.playback_id = 1;
        assert!(!action_changes_display(&app, &tick(1000)));
    }

    #[test]
    fn title_reflects_track_and_playback_state() {
        let mut app = App::default();
        assert_eq!(terminal_title(&app), "Mélimo");
        app.opened = Some(crate::provider::Track {
            provider: crate::provider::ProviderId::Mock,
            id: "1".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            duration_secs: 200,
        });
        app.playback = PlaybackState::Playing;
        assert_eq!(terminal_title(&app), "▶ Artist — Song · Mélimo");
        app.playback = PlaybackState::Paused;
        assert_eq!(terminal_title(&app), "⏸ Artist — Song · Mélimo");
        app.buffering = true;
        app.playback = PlaybackState::Playing;
        assert_eq!(terminal_title(&app), "… Artist — Song · Mélimo");
        app.opened.as_mut().unwrap().title = "\u{1b}]52;clipboard\u{7}".repeat(100);
        let title = terminal_title(&app);
        assert!(!title.chars().any(char::is_control));
        assert!(title.chars().count() <= 256);
    }
}
