//! Opt-in synthetic render benchmark and cell snapshots; never contacts a provider.
use super::ui::render;
use crate::{
    app::{
        action::Action,
        state::{App, PlaybackState, View},
    },
    provider::{LyricLine, Lyrics, Track},
};
use ratatui::{Terminal, backend::TestBackend, widgets::TableState};
use std::time::Instant;

#[test]
#[ignore = "manual synthetic render benchmark; optionally exports cells to MELIMO_REVIEW_DIR"]
fn synthetic_render_review() {
    let mut app = App::default();
    app.opened = Some(Track {
        id: "synthetic".into(),
        title: "A little closer to the stars".into(),
        artist: "The Imaginary Orchestra".into(),
        album: "Synthetic sessions".into(),
        duration_secs: 210,
    });
    app.playback = PlaybackState::Playing;
    app.elapsed = 64;
    app.elapsed_ms = 64000;
    app.karaoke = true;
    app.lyrics = Some(Ok(Lyrics {
        lines: vec![
            LyricLine {
                at_ms: 0,
                text: "Let the city settle down".into(),
            },
            LyricLine {
                at_ms: 60000,
                text: "We are a little closer to the stars".into(),
            },
            LyricLine {
                at_ms: 70000,
                text: "Another rhythm finds its way".into(),
            },
        ],
        credits: "Original synthetic demo lyrics".into(),
        ..Default::default()
    }));
    app.queue = (0..1000)
        .map(|i| Track {
            id: i.to_string(),
            title: format!("Synthetic track {i:04}"),
            artist: "Demo artist".into(),
            album: "Demo album".into(),
            duration_secs: 180,
        })
        .collect();
    for view in [View::Discover, View::Queue, View::NowPlaying] {
        app.view = view;
        app.selected = Some(0);
        let name = match app.view {
            View::Discover => "discover",
            View::Queue => "queue",
            _ => "player",
        };
        for (width, height) in [(40, 18), (60, 24), (100, 30)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut table = TableState::default();
            let mut times = Vec::new();
            for _ in 0..100 {
                let start = Instant::now();
                terminal
                    .draw(|f| render(f, &app, "Offline demo", &mut table))
                    .unwrap();
                times.push(start.elapsed().as_micros());
            }
            times.sort_unstable();
            eprintln!(
                "render {name} {width}x{height}: median={}us p95={}us",
                times[50], times[95]
            );
            if let Some(dir) = std::env::var_os("MELIMO_REVIEW_DIR") {
                let cells: Vec<_> = terminal.backend().buffer().content.iter().map(|c| serde_json::json!({"text":c.symbol(), "fg":format!("{:?}",c.fg), "bg":format!("{:?}",c.bg), "modifiers":format!("{:?}",c.modifier)})).collect();
                let path = std::path::PathBuf::from(dir).join(format!("{name}-{width}.json"));
                std::fs::write(
                    path,
                    serde_json::to_vec(
                        &serde_json::json!({"width":width,"height":height,"cells":cells}),
                    )
                    .unwrap(),
                )
                .unwrap();
            }
        }
    }
    app.update(Action::StopPlayback);
}
