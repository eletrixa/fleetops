//! Key/event → `Action` mapping — pure.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/tui/keys.rs
//! Deps:    crossterm (KeyEvent types only)
//! Tested:  inline `#[cfg(test)]` table test
//!
//! Key responsibilities:
//! - Map key presses to `Action`s; everything unmapped is `None`.
//!
//! Design constraints:
//! - Pure table — no state, no I/O. Only `KeyEventKind::Press` maps (Windows terminals send
//!   Release events too).

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::model::Action;

/// Map a key event to a user action; `None` = ignored.
pub fn map(key: KeyEvent) -> Option<Action> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(Action::Quit);
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
        KeyCode::Char('j') | KeyCode::Down => Some(Action::Down),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::Up),
        KeyCode::Char('r') => Some(Action::Refresh),
        KeyCode::Enter => Some(Action::Jump),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn key_table() {
        let cases = [
            (press(KeyCode::Char('q')), Some(Action::Quit)),
            (press(KeyCode::Esc), Some(Action::Quit)),
            (
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                Some(Action::Quit),
            ),
            (press(KeyCode::Char('j')), Some(Action::Down)),
            (press(KeyCode::Down), Some(Action::Down)),
            (press(KeyCode::Char('k')), Some(Action::Up)),
            (press(KeyCode::Up), Some(Action::Up)),
            (press(KeyCode::Char('r')), Some(Action::Refresh)),
            (press(KeyCode::Enter), Some(Action::Jump)),
            (press(KeyCode::Char('x')), None),
        ];
        for (event, want) in cases {
            assert_eq!(map(event), want, "event {event:?}");
        }
    }

    #[test]
    fn release_events_are_ignored() {
        let mut event = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        event.kind = KeyEventKind::Release;
        assert_eq!(map(event), None);
    }
}
