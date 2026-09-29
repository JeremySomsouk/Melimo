use super::theme;
use crate::app::state::{App, DISCOVER, DISCOVER_GROUPS, PlaybackState, View};
use ratatui::text::Line;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    widgets::{Block, Gauge, Paragraph, Row, Table, TableState, Wrap},
};
use unicode_width::UnicodeWidthStr;

pub fn render(frame: &mut Frame, app: &App, provider: &str, table: &mut TableState) {
    let theme = theme::Theme::from_env();
    frame.render_widget(Block::default().style(theme.base()), frame.area());
    let [header, body, footer, player] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(if app.notice.is_some() { 2 } else { 1 }),
        Constraint::Length(
            if frame.area().height >= 18
                && !(app.view == View::NowPlaying && frame.area().height < 22)
            {
                6
            } else {
                1
            },
        ),
    ])
    .areas(frame.area());
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled("Mélimo  /  music for your terminal", theme.accent()),
            Line::styled(provider.to_owned(), theme.secondary()),
        ]),
        header,
    );
    if app.show_help {
        frame.render_widget(Paragraph::new("/  Edit search · Enter submits\nBackspace  Delete last character · Ctrl+U  Clear query\nj/k or ↑/↓  Select · g/G  First/last\nEnter  Play selected track\nSpace  Pause/resume · ←/→  Seek 10s · +/-  Volume · m  Mute · l  Lyrics/karaoke · s  Stop\nd  Discover genres, moods, Flow & favorites\nP  Switch search provider · Tab  Tracks / playlists (Deezer)\na  Play all displayed tracks · n  Next queued track\np  Return to player · b  Queue · r  Shuffle and play\ne  Enqueue selected track · Delete  Remove queued track\nf  Toggle selected/current Deezer favorite · L  Refresh login\nq / Esc  Back, or quit from Discover\nCtrl+C  Always quit\n?  Toggle help").block(theme.panel("Help")).wrap(Wrap { trim: true }), body);
    } else if app.view == View::Discover {
        render_discover(frame, body, app, &theme, table);
    } else if app.view == View::Queue && app.queue.is_empty() {
        render_state(
            frame,
            body,
            &theme,
            "Up next",
            "Your queue is empty",
            "Browse with d, then press e on a track to add it.",
            "Add several tracks to keep the music going.",
        );
    } else if app.view == View::Queue {
        table.select(app.selected);
        frame.render_stateful_widget(
            Table::new(
                app.queue.iter().map(|t| {
                    Row::new([
                        format!("[{}] {}", t.provider.label(), t.title),
                        t.artist.clone(),
                        format_time(t.duration_secs),
                    ])
                }),
                [
                    Constraint::Percentage(50),
                    Constraint::Min(8),
                    Constraint::Length(6),
                ],
            )
            .block(theme.panel(format!(
                "Up next · {} · Enter jumps here · Delete removes · r shuffles & plays",
                app.queue.len()
            )))
            .row_highlight_style(theme.selected())
            .highlight_symbol("› "),
            body,
            table,
        );
    } else if app.view == View::NowPlaying {
        if let Some(track) = &app.opened {
            let block = theme.panel("Now Playing");
            let inner = block.inner(body);
            frame.render_widget(block, body);
            let [details, progress, controls, lyrics_area] = Layout::vertical([
                Constraint::Length(4),
                Constraint::Length(1),
                Constraint::Length(if frame.area().height < 18 {
                    0
                } else if inner.width < 70 {
                    3
                } else {
                    2
                }),
                Constraint::Min(0),
            ])
            .areas(inner);
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled(
                        format!("[{}] {}", track.provider.label(), track.title),
                        theme.title(),
                    ),
                    Line::raw(track.artist.clone()),
                    Line::styled(
                        format!("{} · {}", track.provider.name(), track.album),
                        theme.secondary(),
                    ),
                    Line::styled(
                        match &app.playback {
                            PlaybackState::Error(error) => error.as_str(),
                            _ => playback_label(app),
                        },
                        theme.accent(),
                    ),
                ]),
                details,
            );
            frame.render_widget(progress_gauge(app, track.duration_secs, &theme), progress);
            let pause = if app.pause_requested {
                "Resume"
            } else {
                "Pause"
            };
            let controls_text = if inner.width < 70 {
                format!(
                    "Space {pause} · ←/→ seek\n+/- volume {} · m mute\nl lyrics · n next · b queue · ? help",
                    volume_label(app)
                )
            } else {
                format!(
                    "Space {pause} · ←/→ seek 10s · +/- volume {} · m mute\nl lyrics · n next · s stop · b queue · f favorite",
                    volume_label(app)
                )
            };
            frame.render_widget(Paragraph::new(controls_text), controls);
            if app.karaoke {
                let block = theme.panel("Lyrics · synchronized when available");
                let area = block.inner(lyrics_area);
                frame.render_widget(block, lyrics_area);
                match &app.lyrics {
                    None => render_state_text(
                        frame,
                        area,
                        &theme,
                        "Loading lyrics…",
                        "l returns to the player.",
                        "Playback continues while lyrics load.",
                    ),
                    Some(Err(_)) => render_state_text(
                        frame,
                        area,
                        &theme,
                        "Lyrics unavailable",
                        "l returns to the player.",
                        "No lyrics were supplied for this track or session.",
                    ),
                    Some(Ok(lyrics)) if lyrics.lines.is_empty() && lyrics.plain.is_empty() => {
                        render_state_text(
                            frame,
                            area,
                            &theme,
                            "No lyrics available",
                            "l returns to the player.",
                            "Try another song for synchronized lyrics.",
                        )
                    }
                    Some(Ok(lyrics)) if !lyrics.lines.is_empty() => {
                        let active = lyrics.active_line(app.elapsed_ms);
                        let rows = usize::from(if area.height > 1 {
                            area.height - 1
                        } else {
                            area.height
                        });
                        let start = active.unwrap_or(0).saturating_sub(rows / 2);
                        let mut lines: Vec<Line> = lyrics
                            .lines
                            .iter()
                            .enumerate()
                            .skip(start)
                            .take(rows)
                            .map(|(i, line)| {
                                if Some(i) == active {
                                    Line::styled(format!("› {}", line.text), theme.selected())
                                } else {
                                    Line::styled(format!("  {}", line.text), theme.secondary())
                                }
                            })
                            .collect();
                        lines.push(Line::styled(lyrics.credits.clone(), theme.secondary()));
                        frame.render_widget(Paragraph::new(lines), area);
                    }
                    Some(Ok(lyrics)) => frame.render_widget(
                        Paragraph::new(format!(
                            "Unsynchronized lyrics\n{}\n{}",
                            lyrics.plain, lyrics.credits
                        ))
                        .wrap(Wrap { trim: false }),
                        area,
                    ),
                }
            }
        } else {
            render_state(
                frame,
                body,
                &theme,
                "Now Playing",
                "Nothing playing yet",
                "Press d to discover music or / to search.",
                "Select a track and press Enter to start listening.",
            );
        }
    } else {
        let [search, results] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(body);
        let input = if app.editing {
            format!("{}▏", app.query)
        } else {
            app.query.clone()
        };
        let scroll = input
            .width()
            .saturating_sub(usize::from(search.width.saturating_sub(2)))
            .min(u16::MAX as usize) as u16;
        frame.render_widget(
            Paragraph::new(input)
                .scroll((0, scroll))
                .block(theme.panel(format!(
                    "Search {} · {} · Tab switches",
                    if app.playlist_search {
                        "playlists"
                    } else {
                        "tracks"
                    },
                    if app.editing { "typing" } else { "/ to edit" }
                ))),
            search,
        );
        let message = if app.loading {
            Some((
                "Finding your music…",
                "Press d to return to Discover.",
                "Results will appear here when ready.",
            ))
        } else if let Some(error) = &app.error {
            Some((
                "Could not load music",
                "Press / to search again or d to browse.",
                error.as_str(),
            ))
        } else if !app.searched {
            Some((
                "Find your next song",
                "Press / to search or d to browse.",
                "Use Tab outside text entry to switch tracks and playlists.",
            ))
        } else if (app.showing_playlists && app.playlists.is_empty())
            || (!app.showing_playlists && app.tracks.is_empty())
        {
            Some((
                "No music found",
                "Try other keywords with / or browse with d.",
                "This search or collection has no available results.",
            ))
        } else {
            None
        };
        let title = app
            .collection
            .as_deref()
            .unwrap_or(if app.showing_playlists {
                "Playlists"
            } else {
                "Tracks"
            });
        if let Some((heading, hint, detail)) = message {
            render_state(
                frame,
                results,
                &theme,
                title,
                heading,
                if app.editing {
                    "Enter searches; Esc returns to browsing."
                } else {
                    hint
                },
                detail,
            );
        } else if app.showing_playlists {
            let rows = app.playlists.iter().map(|p| {
                Row::new([
                    p.title.clone(),
                    if p.tracks > 0 {
                        p.tracks.to_string()
                    } else {
                        "-".into()
                    },
                ])
            });
            table.select(app.selected);
            frame.render_stateful_widget(
                Table::new(rows, [Constraint::Min(10), Constraint::Length(8)])
                    .header(
                        Row::new(["Playlist · Enter to inspect", "Tracks"]).style(theme.title()),
                    )
                    .block(theme.panel(format!("{title} · {}", app.playlists.len())))
                    .row_highlight_style(theme.selected())
                    .highlight_symbol("› "),
                results,
                table,
            );
        } else {
            let rows = app.tracks.iter().map(|track| {
                Row::new(vec![
                    format!("[{}] {}", track.provider.label(), track.title),
                    track.artist.clone(),
                    format_time(track.duration_secs),
                ])
            });
            let widget = Table::new(
                rows,
                [
                    Constraint::Percentage(52),
                    Constraint::Min(8),
                    Constraint::Length(6),
                ],
            )
            .header(Row::new(["Title", "Artist", "Time"]).style(theme.title()))
            .block(theme.panel(format!("{title} · {} · a plays all", app.tracks.len())))
            .row_highlight_style(theme.selected())
            .highlight_symbol("› ");
            table.select(app.selected);
            frame.render_stateful_widget(widget, results, table);
        }
    }
    let footer_text = if frame.area().width < 70 && !app.editing && !app.show_help {
        "? help · p player · d discover"
    } else if app.show_help {
        "? / q close help · Ctrl+C quit"
    } else if app.editing {
        "Enter search · Esc leave input · Ctrl+U clear"
    } else if app.view == View::NowPlaying {
        "Space pause/resume · ←/→ seek · +/- volume · m mute · l lyrics · n next · b queue · d Discover"
    } else if app.view == View::Queue {
        "Enter play from here · r shuffle/play · n next · f favorite · p player · q back"
    } else if app.view == View::Discover {
        "Enter browse · ←/→ groups · P provider · / search · p player · L login · ? help"
    } else if app.showing_playlists {
        "Enter inspect · a play playlist · r shuffle/play · / search · p player · b queue"
    } else {
        "Enter play · a all · r shuffle · e enqueue · f favorite · p player · b queue"
    };
    frame.render_widget(
        Paragraph::new(format!(
            "{footer_text}\n{}",
            app.notice.as_deref().unwrap_or("")
        )),
        footer,
    );
    render_bottom_player(frame, player, app, &theme);
}

fn render_state(
    frame: &mut Frame,
    area: Rect,
    theme: &theme::Theme,
    title: &str,
    heading: &str,
    hint: &str,
    detail: &str,
) {
    let panel = theme.panel(title);
    let inner = panel.inner(area);
    frame.render_widget(panel, area);
    render_state_text(frame, inner, theme, heading, hint, detail);
}

fn render_state_text(
    frame: &mut Frame,
    area: Rect,
    theme: &theme::Theme,
    heading: &str,
    hint: &str,
    detail: &str,
) {
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(heading, theme.accent()),
            Line::styled(hint, theme.title()),
            Line::styled(detail, theme.secondary()),
        ])
        .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_discover(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    theme: &theme::Theme,
    table: &mut TableState,
) {
    if area.width < 90 {
        table.select(app.selected);
        frame.render_stateful_widget(
            Table::new(
                DISCOVER.iter().map(|(title, _)| Row::new([*title])),
                [Constraint::Percentage(100)],
            )
            .block(theme.panel("Discover · genre, mood & your music"))
            .row_highlight_style(theme.selected())
            .highlight_symbol("› "),
            area,
            table,
        );
        return;
    }
    let columns = Layout::horizontal([
        Constraint::Percentage(40),
        Constraint::Percentage(28),
        Constraint::Percentage(32),
    ])
    .split(area);
    for ((title, indices), column) in DISCOVER_GROUPS.iter().zip(columns.iter()) {
        let mut selection = TableState::default();
        selection.select(
            app.selected
                .and_then(|selected| indices.iter().position(|&index| index == selected)),
        );
        let rows = indices.iter().map(|&index| {
            let label = DISCOVER[index].0;
            Row::new([label.strip_prefix("Genre · ").unwrap_or(label)])
        });
        frame.render_stateful_widget(
            Table::new(rows, [Constraint::Percentage(100)])
                .block(theme.panel(*title))
                .row_highlight_style(theme.selected())
                .highlight_symbol("› "),
            *column,
            &mut selection,
        );
    }
}

fn render_bottom_player(frame: &mut Frame, area: Rect, app: &App, theme: &theme::Theme) {
    if area.height <= 1 {
        let text = app.opened.as_ref().map_or_else(
            || "Mélimo · Select a track to play".to_string(),
            |track| {
                format!(
                    "{} · {} / {} · {}",
                    playback_label(app),
                    track.title,
                    track.artist,
                    format_time(app.elapsed)
                )
            },
        );
        frame.render_widget(Paragraph::new(text).style(theme.accent()), area);
        return;
    }
    let block = theme.panel(format!("Now playing · {}", playback_label(app)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [title, artist, progress, controls] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    if let Some(track) = &app.opened {
        frame.render_widget(
            Paragraph::new(format!("[{}] {}", track.provider.label(), track.title))
                .style(theme.title()),
            title,
        );
        frame.render_widget(
            Paragraph::new(format!(
                "{} · vol {} · {} queued",
                track.artist,
                volume_label(app),
                app.queue.len()
            ))
            .style(theme.secondary()),
            artist,
        );
        if let PlaybackState::Error(error) = &app.playback {
            frame.render_widget(
                Paragraph::new(error.as_str()).style(theme.secondary()),
                progress,
            );
        } else {
            frame.render_widget(progress_gauge(app, track.duration_secs, theme), progress);
        }
    } else {
        frame.render_widget(
            Paragraph::new("Your next song starts here").style(theme.title()),
            title,
        );
        frame.render_widget(
            Paragraph::new("Select a track or playlist to play").style(theme.secondary()),
            artist,
        );
    }
    let hint = if app.show_help {
        "Close help to use playback controls"
    } else if app.editing {
        "Esc leaves search to use playback controls"
    } else if matches!(app.playback, PlaybackState::Error(_)) {
        "n skips queued track · d browse · / search"
    } else if app.opened.is_none() {
        "d discover · / search"
    } else if inner.width < 70 {
        if app.pause_requested {
            "Space Resume · n next · m mute · p player"
        } else {
            "Space Pause · n next · m mute · p player"
        }
    } else if app.pause_requested {
        "Space Resume · n next · s stop · +/- volume · m mute · b queue · p player"
    } else {
        "Space Pause · n next · s stop · +/- volume · m mute · b queue · p player"
    };
    frame.render_widget(Paragraph::new(hint).style(theme.accent()), controls);
}

fn progress_gauge(app: &App, duration: u64, theme: &theme::Theme) -> Gauge<'static> {
    Gauge::default()
        .gauge_style(theme.progress())
        .ratio(if duration == 0 {
            0.0
        } else {
            (app.elapsed as f64 / duration as f64).clamp(0.0, 1.0)
        })
        .label(format!(
            "{} / {}",
            format_time(app.elapsed),
            format_time(duration)
        ))
}

fn volume_label(app: &App) -> String {
    if app.volume.is_muted() {
        "muted".into()
    } else {
        format!("{}%", app.volume.percent())
    }
}

fn playback_label(app: &App) -> &str {
    match &app.playback {
        PlaybackState::Stopped => "Stopped",
        PlaybackState::Loading => "Loading audio…",
        PlaybackState::Playing if app.buffering => "Buffering…",
        PlaybackState::Playing => "Playing",
        PlaybackState::Paused => "Paused",
        PlaybackState::Finished => "Finished",
        PlaybackState::Error(_) => "Playback unavailable",
    }
}

fn format_time(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn provider_labels_follow_results_queue_and_player() {
        use crate::provider::{ProviderId, Track};
        let track = Track {
            provider: ProviderId::Invidious,
            id: "abcdefghijk".into(),
            title: "Synthetic video".into(),
            artist: "Channel".into(),
            album: String::new(),
            duration_secs: 120,
        };
        let mut app = App::default();
        app.search_provider = ProviderId::Invidious;
        app.tracks = vec![track.clone()];
        app.queue = vec![track.clone()].into();
        app.opened = Some(track);
        app.selected = Some(0);
        app.searched = true;
        for width in [40, 60, 100] {
            for view in [View::Search, View::Queue, View::NowPlaying] {
                app.view = view;
                let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
                terminal
                    .draw(|f| {
                        render(
                            f,
                            &app,
                            app.search_provider.name(),
                            &mut TableState::default(),
                        )
                    })
                    .unwrap();
                let text: String = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect();
                assert!(text.contains("[INV]"));
                assert!(text.contains("Channel"));
            }
        }
    }
    #[test]
    fn renders_small_and_normal_terminals() {
        for (width, height) in [(1, 1), (20, 5), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut app = App::default();
            app.update(crate::app::action::Action::FocusSearch);
            for c in "Écho 日本".repeat(30).chars() {
                app.update(crate::app::action::Action::Insert(c));
            }
            terminal
                .draw(|frame| render(frame, &app, "Mock", &mut TableState::default()))
                .unwrap();
        }
    }
    #[test]
    fn player_renders_progress_controls_and_active_lyric_on_all_sizes() {
        use crate::provider::{LyricLine, Lyrics, Track};
        let mut app = App::default();
        app.update(crate::app::action::Action::ToggleLyrics);
        app.opened = Some(Track {
            provider: crate::provider::ProviderId::Mock,
            id: "demo".into(),
            title: "Demo".into(),
            artist: "Demo".into(),
            album: "Demo".into(),
            duration_secs: 120,
        });
        app.view = View::NowPlaying;
        app.elapsed = 30;
        app.elapsed_ms = 30500;
        app.playback = PlaybackState::Paused;
        app.pause_requested = true;
        app.lyrics = Some(Ok(Lyrics {
            lines: vec![
                LyricLine {
                    at_ms: 0,
                    text: "Opening demo line".into(),
                },
                LyricLine {
                    at_ms: 30250,
                    text: "Active demo line".into(),
                },
                LyricLine {
                    at_ms: 40000,
                    text: "Next demo line".into(),
                },
            ],
            ..Default::default()
        }));
        for (width, height) in [(1, 1), (20, 5), (40, 18), (60, 24), (100, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| render(frame, &app, "Mock", &mut TableState::default()))
                .unwrap();
            if width >= 40 {
                let text: String = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect();
                assert!(text.contains("0:30 / 2:00"));
                assert!(text.contains("Resume"));
                assert!(text.contains("m mute"));
                let buffer = terminal.backend().buffer();
                assert!(buffer.content.iter().any(|cell| cell.symbol() == "D"
                    && cell.modifier.contains(ratatui::style::Modifier::BOLD)));
                assert!(text.contains("› Active demo line"));
            }
        }
    }

    #[test]
    fn bottom_player_persists_across_views_and_input_modes() {
        let mut app = App::default();
        app.opened = Some(crate::provider::Track {
            provider: crate::provider::ProviderId::Mock,
            id: "demo".into(),
            title: "Sunrise Drive".into(),
            artist: "Lofi Keys".into(),
            album: "Demo".into(),
            duration_secs: 236,
        });
        app.elapsed = 84;
        for width in [60, 100] {
            for view in [View::Discover, View::Search, View::Queue, View::NowPlaying] {
                app.view = view;
                for state in [
                    PlaybackState::Playing,
                    PlaybackState::Paused,
                    PlaybackState::Loading,
                    PlaybackState::Error("Unavailable".into()),
                ] {
                    app.playback = state;
                    for (editing, help) in [(false, false), (true, false), (false, true)] {
                        app.editing = editing;
                        app.show_help = help;
                        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
                        terminal
                            .draw(|frame| render(frame, &app, "Mock", &mut TableState::default()))
                            .unwrap();
                        let dock: String = terminal
                            .backend()
                            .buffer()
                            .content
                            .iter()
                            .skip(usize::from(width) * 18)
                            .map(|cell| cell.symbol())
                            .collect();
                        assert!(dock.contains("Sunrise Drive"));
                        assert!(dock.contains("Lofi Keys"));
                        if matches!(app.playback, PlaybackState::Error(_)) {
                            assert!(dock.contains("Unavailable"));
                        } else {
                            assert!(dock.contains("1:24 / 3:56"));
                        }
                        assert!(dock.contains(playback_label(&app)));
                        assert!(dock.contains(if editing {
                            "Esc leaves search"
                        } else if help {
                            "Close help"
                        } else if matches!(app.playback, PlaybackState::Error(_)) {
                            "n skips"
                        } else {
                            "n next"
                        }));
                    }
                }
            }
        }
    }

    #[test]
    fn discovery_columns_keep_each_selected_item_visible() {
        let mut app = App::default();
        app.view = View::Discover;
        for (width, height) in [(60, 18), (100, 18), (120, 24)] {
            for (index, (label, _)) in DISCOVER.iter().enumerate() {
                app.selected = Some(index);
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| render(frame, &app, "Mock", &mut TableState::default()))
                    .unwrap();
                let text: String = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect();
                assert!(
                    text.contains(label.strip_prefix("Genre · ").unwrap_or(label)),
                    "missing {label} at {width}x{height}"
                );
                if width >= 90 {
                    for (title, _) in DISCOVER_GROUPS {
                        assert!(text.contains(title));
                    }
                }
            }
        }
    }

    #[test]
    fn state_panels_keep_recovery_visible_on_narrow_terminals() {
        for (width, height) in [(40, 16), (60, 24), (100, 24)] {
            for (view, loading, searched, error, heading, hint) in [
                (
                    View::Queue,
                    false,
                    false,
                    None,
                    "Your queue is empty",
                    "Browse with d",
                ),
                (
                    View::NowPlaying,
                    false,
                    false,
                    None,
                    "Nothing playing yet",
                    "Press d",
                ),
                (
                    View::Search,
                    true,
                    false,
                    None,
                    "Finding your music",
                    "Press d",
                ),
                (
                    View::Search,
                    false,
                    false,
                    None,
                    "Find your next song",
                    "Press /",
                ),
                (
                    View::Search,
                    false,
                    true,
                    None,
                    "No music found",
                    "Try other keywords",
                ),
                (
                    View::Search,
                    false,
                    true,
                    Some("Synthetic failure".into()),
                    "Could not load music",
                    "Press /",
                ),
            ] {
                let mut app = App::default();
                app.view = view;
                app.loading = loading;
                app.searched = searched;
                app.error = error;
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| render(frame, &app, "Mock", &mut TableState::default()))
                    .unwrap();
                let text: String = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect();
                assert!(text.contains(heading), "{width}x{height}: {heading}");
                assert!(text.contains(hint), "{width}x{height}: {hint}");
            }
        }
    }

    #[test]
    fn short_player_hides_transport_hints() {
        let mut app = App::default();
        app.opened = Some(crate::provider::Track {
            provider: crate::provider::ProviderId::Mock,
            id: "demo".into(),
            title: "Demo".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            duration_secs: 0,
        });
        app.view = View::NowPlaying;
        let mut terminal = Terminal::new(TestBackend::new(60, 16)).unwrap();
        terminal
            .draw(|frame| render(frame, &app, "Mock", &mut TableState::default()))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Demo"));
        assert!(!text.contains("Space Pause"));
        assert!(!text.contains("m mute"));
    }

    #[test]
    fn time_formatting() {
        assert_eq!(format_time(65), "1:05");
        assert_eq!(format_time(0), "0:00");
    }
}
