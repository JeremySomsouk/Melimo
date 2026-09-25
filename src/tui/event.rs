use crate::app::{
    action::Action,
    state::{App, View},
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::{io, time::Duration};

pub fn read_action(app: &App) -> io::Result<Option<Action>> {
    if !event::poll(Duration::from_millis(50))? {
        return Ok(None);
    }
    match event::read()? {
        Event::Key(key) => Ok(map_key(key, app)),
        Event::Resize(..) => Ok(Some(Action::Redraw)),
        _ => Ok(None),
    }
}

fn map_key(key: KeyEvent, app: &App) -> Option<Action> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') => Some(Action::Quit),
            KeyCode::Char('u') if app.editing && !app.show_help => Some(Action::ClearQuery),
            _ => None,
        };
    }
    if app.show_help {
        return match key.code {
            KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::Esc => Some(Action::Back),
            _ => None,
        };
    }
    if app.editing {
        return match key.code {
            KeyCode::Esc => Some(Action::Back),
            KeyCode::Enter => Some(Action::SubmitSearch),
            KeyCode::Backspace => Some(Action::Backspace),
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::ALT) => {
                Some(Action::Insert(c))
            }
            _ => None,
        };
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Some(Action::Back),
        KeyCode::Char('?') => Some(Action::ToggleHelp),
        KeyCode::Char('/') => Some(Action::FocusSearch),
        KeyCode::Left if app.view == View::NowPlaying => Some(Action::SeekRelative(-10)),
        KeyCode::Right if app.view == View::NowPlaying => Some(Action::SeekRelative(10)),
        KeyCode::Char(' ') => Some(Action::TogglePause),
        KeyCode::Char('+') | KeyCode::Char('=') => Some(Action::VolumeUp),
        KeyCode::Char('-') | KeyCode::Char('_') => Some(Action::VolumeDown),
        KeyCode::Char('m') => Some(Action::ToggleMute),
        KeyCode::Char('s') => Some(Action::StopPlayback),
        KeyCode::Char('e') => Some(Action::Enqueue),
        KeyCode::Delete if app.view == View::Queue => Some(Action::RemoveQueued),
        KeyCode::Char('l') => Some(Action::ToggleLyrics),
        KeyCode::Char('p') => Some(Action::ShowPlayer),
        KeyCode::Char('P') => Some(Action::CycleProvider),
        KeyCode::Char('b') => Some(Action::ShowQueue),
        KeyCode::Char('r') => Some(Action::ShufflePlay),
        KeyCode::Char('f') => Some(Action::ToggleFavorite),
        KeyCode::Char('L') => Some(Action::Login),
        KeyCode::Char('d') => Some(Action::Discover),
        KeyCode::Char('n') => Some(Action::NextTrack),
        KeyCode::Tab => Some(Action::ToggleSearchKind),
        KeyCode::Char('a') if app.view == View::Search => Some(Action::PlayAll),
        _ if app.view == View::NowPlaying => None,
        KeyCode::Char('j') | KeyCode::Down => Some(Action::MoveDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::MoveUp),
        KeyCode::Char('g') | KeyCode::Home => Some(Action::First),
        KeyCode::Char('G') | KeyCode::End => Some(Action::Last),
        KeyCode::Enter => Some(Action::OpenTrack),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn volume_shortcuts_are_global_but_not_in_text_entry() {
        let app = App::default();
        for (key, action) in [
            (KeyCode::Char('+'), Action::VolumeUp),
            (KeyCode::Char('='), Action::VolumeUp),
            (KeyCode::Char('-'), Action::VolumeDown),
            (KeyCode::Char('_'), Action::VolumeDown),
            (KeyCode::Char('m'), Action::ToggleMute),
        ] {
            let got = map_key(KeyEvent::new(key, KeyModifiers::NONE), &app);
            assert!(
                matches!((&got, &action), (Some(a), b) if std::mem::discriminant(a) == std::mem::discriminant(b)),
                "{key:?} should map to its volume action"
            );
        }
        // While editing a query, the same keys must stay literal text.
        let mut editing = App::default();
        editing.update(Action::FocusSearch);
        assert!(matches!(
            map_key(
                KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE),
                &editing
            ),
            Some(Action::Insert('m'))
        ));
    }

    #[test]
    fn player_transport_shortcuts() {
        let mut app = App::default();
        app.update(Action::ShowPlayer);
        app.view = View::NowPlaying;
        for (key, delta) in [(KeyCode::Left, -10), (KeyCode::Right, 10)] {
            assert!(
                matches!(map_key(KeyEvent::new(key, KeyModifiers::NONE), &app), Some(Action::SeekRelative(d)) if d == delta)
            );
        }
        assert!(matches!(
            map_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE), &app),
            Some(Action::TogglePause)
        ));
        assert!(matches!(
            map_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE), &app),
            Some(Action::ToggleLyrics)
        ));
    }
    #[test]
    fn text_entry_does_not_trigger_shortcuts() {
        let mut app = App::default();
        app.update(Action::FocusSearch);
        for c in ['q', '/', '?', 'j', 'é', 'l', ' '] {
            assert!(
                matches!(map_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), &app), Some(Action::Insert(value)) if value == c)
            );
        }
        assert!(matches!(
            map_key(
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                &app
            ),
            Some(Action::Quit)
        ));
    }
}
