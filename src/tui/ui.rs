use super::theme;
use crate::app::state::{App, DISCOVER, PlaybackState, View};
use ratatui::text::{Line, Span};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    widgets::{Gauge, Paragraph, Row, Table, TableState, Wrap},
};
use unicode_width::UnicodeWidthStr;

pub fn render(frame: &mut Frame, app: &App, provider: &str, table: &mut TableState) {
    frame.render_widget(Paragraph::new("").style(theme::base()), frame.area());
    let compact = frame.area().width < 72;
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .areas(frame.area());
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Mélimo ", theme::selected()),
            Span::styled(format!("  {provider}"), theme::muted()),
            Span::styled(
                if compact {
                    ""
                } else {
                    "  ·  your music, here"
                },
                theme::muted(),
            ),
        ])),
        header,
    );
    if app.show_help {
        frame.render_widget(Paragraph::new("/  Edit search · Enter submits\nBackspace  Delete last character · Ctrl+U  Clear query\nj/k or ↑/↓  Select · g/G  First/last\nEnter  Play selected track\nSpace  Pause/resume · ←/→  Seek 10s · +/-  Volume · m  Mute · l  Lyrics/karaoke · s  Stop\nd  Discover genres, moods, Flow & favorites\nP  Switch search provider · Tab  Tracks / playlists (Deezer)\na  Play all displayed tracks · n  Next queued track\np  Return to player · b  Queue · r  Shuffle and play\ne  Enqueue selected track · Delete  Remove queued track\nf  Toggle selected/current Deezer favorite · L  Refresh login\nq / Esc  Back, or quit from Discover\nCtrl+C  Always quit\n?  Toggle help").block(theme::panel().title("Help")).wrap(Wrap { trim: true }), body);
    } else if app.view == View::Discover {
        let rows = DISCOVER.iter().map(|(title, _)| Row::new([*title]));
        table.select(app.selected);
        frame.render_stateful_widget(
            Table::new(rows, [Constraint::Percentage(100)])
                .block(theme::panel().title("Discover · genre, mood & your music"))
                .row_highlight_style(theme::selected())
                .highlight_symbol("› "),
            body,
            table,
        );
    } else if app.view == View::Queue && app.queue.is_empty() {
        frame.render_widget(
            Paragraph::new("Your queue is empty.\nSearch with /, then press e to add a track.")
                .style(theme::muted())
                .block(theme::panel().title("Up next"))
                .wrap(Wrap { trim: true }),
            body,
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
            .block(theme::panel().title(format!("Up next · {} tracks", app.queue.len())))
            .row_highlight_style(theme::selected())
            .highlight_symbol("› "),
            body,
            table,
        );
    } else if app.view == View::NowPlaying {
        if let Some(track) = &app.opened {
            let block = theme::panel().title("Now Playing");
            let inner = block.inner(body);
            frame.render_widget(block, body);
            let [details, progress, controls, lyrics_area] = Layout::vertical([
                Constraint::Length(4),
                Constraint::Length(1),
                Constraint::Length(if compact { 3 } else { 2 }),
                Constraint::Min(0),
            ])
            .areas(inner);
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled(
                        format!("[{}] {}", track.provider.label(), track.title),
                        theme::title(),
                    ),
                    Line::styled(track.artist.clone(), theme::base()),
                    Line::styled(
                        format!("{} · {}", track.provider.name(), track.album),
                        theme::muted(),
                    ),
                    Line::styled(
                        playback_label(app),
                        if matches!(app.playback, PlaybackState::Error(_)) {
                            theme::warning()
                        } else {
                            theme::title()
                        },
                    ),
                ]),
                details,
            );
            frame.render_widget(
                Gauge::default()
                    .gauge_style(theme::selected())
                    .ratio(if track.duration_secs == 0 {
                        0.0
                    } else {
                        (app.elapsed as f64 / track.duration_secs as f64).clamp(0.0, 1.0)
                    })
                    .label(format!(
                        "{} / {}",
                        format_time(app.elapsed),
                        format_time(track.duration_secs)
                    )),
                progress,
            );
            let transport = if app.pause_requested {
                "Resume"
            } else {
                "Pause"
            };
            let hints = if compact {
                format!(
                    "[Space] {transport}  [←/→] seek\n[+/-] {}  [m] mute  [l] lyrics\n[n] next  [s] stop  [b] queue  [f] fav",
                    volume_label(app)
                )
            } else {
                format!(
                    "[Space] {transport}  [←/→] seek 10s  [+/-] {}  [m] mute\n[l] lyrics  [n] next  [s] stop  [b] queue  [f] favorite",
                    volume_label(app)
                )
            };
            frame.render_widget(Paragraph::new(hints).style(theme::muted()), controls);
            if app.karaoke {
                let block = theme::panel().title(if compact {
                    "Lyrics"
                } else {
                    "Lyrics · synchronized when available"
                });
                let area = block.inner(lyrics_area);
                frame.render_widget(block, lyrics_area);
                match &app.lyrics {
                    None => frame.render_widget(
                        Paragraph::new("Loading lyrics…").style(theme::muted()),
                        area,
                    ),
                    Some(Err(_)) => frame.render_widget(
                        Paragraph::new("Lyrics unavailable for this track or session."),
                        area,
                    ),
                    Some(Ok(lyrics)) if !lyrics.lines.is_empty() => {
                        let active = lyrics.active_line(app.elapsed_ms);
                        let show_credits = area.height > 1;
                        let rows = usize::from(area.height.saturating_sub(u16::from(show_credits)));
                        let start = active.unwrap_or(0).saturating_sub(rows / 2);
                        let mut lines: Vec<Line> = lyrics
                            .lines
                            .iter()
                            .enumerate()
                            .skip(start)
                            .take(rows)
                            .map(|(i, line)| {
                                if Some(i) == active {
                                    Line::styled(format!("› {}", line.text), theme::selected())
                                } else {
                                    Line::styled(format!("  {}", line.text), theme::muted())
                                }
                            })
                            .collect();
                        if show_credits {
                            lines.push(Line::styled(lyrics.credits.clone(), theme::muted()));
                        }
                        frame.render_widget(Paragraph::new(lines), area);
                    }
                    Some(Ok(lyrics)) => frame.render_widget(
                        Paragraph::new(if lyrics.plain.is_empty() {
                            "No lyrics available.".to_string()
                        } else {
                            format!(
                                "Unsynchronized lyrics\n{}\n{}",
                                lyrics.plain, lyrics.credits
                            )
                        })
                        .wrap(Wrap { trim: false }),
                        area,
                    ),
                }
            }
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
                .block(theme::panel().title(format!(
                    "Search [{}] {} · {} · P provider",
                    app.search_provider.label(),
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
            Some("Loading…")
        } else if let Some(error) = &app.error {
            Some(error.as_str())
        } else if !app.searched {
            Some(
                "Press /, type keywords, then Enter. P switches provider; Tab tracks/playlists; d Discover.",
            )
        } else if (app.showing_playlists && app.playlists.is_empty())
            || (!app.showing_playlists && app.tracks.is_empty())
        {
            Some("No matches found. Press / to try other keywords, or d for Discover.")
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
        if let Some(message) = message {
            frame.render_widget(
                Paragraph::new(message)
                    .style(if app.error.is_some() {
                        theme::warning()
                    } else {
                        theme::muted()
                    })
                    .block(theme::panel().title(title))
                    .wrap(Wrap { trim: true }),
                results,
            );
        } else if app.showing_playlists {
            let rows = app.playlists.iter().map(|p| {
                Row::new([
                    p.title.clone(),
                    if p.tracks > 0 {
                        p.tracks.to_string()
                    } else {
                        "—".into()
                    },
                ])
            });
            table.select(app.selected);
            frame.render_stateful_widget(
                Table::new(rows, [Constraint::Min(10), Constraint::Length(8)])
                    .header(
                        Row::new(["Playlist · Enter to inspect", "Tracks"]).style(theme::title()),
                    )
                    .block(theme::panel().title(format!("{title} · {}", app.playlists.len())))
                    .row_highlight_style(theme::selected())
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
            .header(Row::new(["Title", "Artist", "Time"]).style(theme::title()))
            .block(theme::panel().title(format!("{title} · {} · a plays all", app.tracks.len())))
            .row_highlight_style(theme::selected())
            .highlight_symbol("› ");
            table.select(app.selected);
            frame.render_stateful_widget(widget, results, table);
        }
    }
    let footer_text = if app.show_help {
        "? / q close help · Ctrl+C quit"
    } else if app.editing {
        "Enter search · Esc leave input · Ctrl+U clear"
    } else if app.view == View::NowPlaying {
        "Space pause/resume · ←/→ seek · +/- volume · m mute · l lyrics · n next · b queue · d Discover"
    } else if app.view == View::Queue {
        "Enter play from here · r shuffle/play · n next · f favorite · p player · q back"
    } else if app.view == View::Discover {
        "Enter browse · / search · P provider · p player · L login · ? help"
    } else if app.showing_playlists {
        "Enter inspect · a play playlist · r shuffle/play · / search · p player · b queue"
    } else {
        "Enter play · a all · r shuffle · e enqueue · f favorite · p player · b queue"
    };
    let footer_text = if compact && !app.editing && !app.show_help {
        "/ search · p player · ? help"
    } else {
        footer_text
    };
    let playing = app
        .opened
        .as_ref()
        .map(|track| {
            format!(
                "{} · {} — {} · {} · vol {} · {} queued",
                playback_label(app),
                track.artist,
                track.title,
                format_time(app.elapsed),
                volume_label(app),
                app.queue.len()
            )
        })
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(footer_text, theme::muted()),
            Line::styled(playing, theme::title()),
            Line::styled(app.notice.as_deref().unwrap_or(""), theme::warning()),
        ]),
        footer,
    );
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
        PlaybackState::Error(error) => error,
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
                assert!(text.contains("› Active demo line"));
                assert!(text.contains("[l] lyrics"));
                assert!(text.contains("[f] fav"));
            }
        }
    }

    #[test]
    fn empty_queue_explains_how_to_add_tracks() {
        let mut app = App::default();
        app.update(crate::app::action::Action::ShowQueue);
        let mut terminal = Terminal::new(TestBackend::new(60, 18)).unwrap();
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
        assert!(text.contains("Your queue is empty."));
        assert!(text.contains("press e to add a track"));
    }

    #[test]
    fn time_formatting() {
        assert_eq!(format_time(65), "1:05");
        assert_eq!(format_time(0), "0:00");
    }
}
