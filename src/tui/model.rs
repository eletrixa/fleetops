//! TUI state machine: `App` + `update(Msg)` — pure, no I/O.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/tui/model.rs
//! Deps:    crate::panes (PaneRow)
//! Tested:  inline `#[cfg(test)]` unit tests (selection, refresh, quit, errors)
//!
//! Key responsibilities:
//! - Hold board state: rows, selection (keyed by pane_id), refresh age, last sensor error.
//! - Fold every `Msg` into the next state; expose effect requests (jump/refresh) as flags
//!   the event loop drains — the model itself never performs them.
//!
//! Design constraints:
//! - Pure: no clocks, no subprocess, no terminal. Age is counted in ticks fed by the loop.
//! - Selection is keyed by `pane_id`, not row index — a refresh must not steal the cursor.

use crate::panes::PaneRow;

/// User intent, mapped from keys (see `keys.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Move selection up one row.
    Up,
    /// Move selection down one row.
    Down,
    /// Jump to the selected pane.
    Jump,
    /// Force a sensor refresh now.
    Refresh,
    /// Quit the board.
    Quit,
}

/// Everything the event loop can feed the model.
#[derive(Debug)]
pub enum Msg {
    /// Fresh pane rows from the wezterm sensor (already sorted by pane_id).
    Panes(Vec<PaneRow>),
    /// A sensor or jump failure to surface in the footer.
    Error(String),
    /// A key mapped to an action.
    Key(Action),
    /// One second elapsed.
    Tick,
}

/// The board state. `view` reads it; only `update` writes it.
#[derive(Debug, Default)]
pub struct App {
    /// Claude pane rows, sorted by pane_id.
    pub rows: Vec<PaneRow>,
    /// Selected pane's id (survives refreshes); `None` when the board is empty.
    pub selected: Option<u64>,
    /// Seconds since the last successful sensor refresh.
    pub refresh_age_secs: u64,
    /// Last sensor/jump error, shown in the footer until the next good refresh.
    pub error: Option<String>,
    /// Loop flag: exit.
    pub should_quit: bool,
    /// Effect request: jump to this pane (drained by the loop).
    pub pending_jump: Option<u64>,
    /// Effect request: poll the sensor now (drained by the loop).
    pub refresh_requested: bool,
}

impl App {
    /// Index of the selected row, if it still exists.
    pub fn selected_index(&self) -> Option<usize> {
        let id = self.selected?;
        self.rows.iter().position(|r| r.pane_id == id)
    }

    /// Fold one message into the next state.
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Panes(rows) => {
                let old_index = self.selected_index();
                self.rows = rows;
                self.refresh_age_secs = 0;
                self.error = None;
                self.reselect(old_index);
            }
            Msg::Error(e) => self.error = Some(e),
            Msg::Tick => self.refresh_age_secs = self.refresh_age_secs.saturating_add(1),
            Msg::Key(action) => self.apply(action),
        }
    }

    /// Keep the selection on the same pane if it survived; otherwise clamp to the nearest row.
    fn reselect(&mut self, old_index: Option<usize>) {
        if self.rows.is_empty() {
            self.selected = None;
            return;
        }
        let still_there = self
            .selected
            .is_some_and(|id| self.rows.iter().any(|r| r.pane_id == id));
        if still_there {
            return;
        }
        let index = old_index.unwrap_or(0).min(self.rows.len() - 1);
        self.selected = Some(self.rows[index].pane_id);
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Refresh => self.refresh_requested = true,
            Action::Jump => self.pending_jump = self.selected,
            Action::Up => self.move_selection(-1),
            Action::Down => self.move_selection(1),
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let current = self.selected_index().unwrap_or(0);
        let last = self.rows.len() - 1;
        let next = current.saturating_add_signed(delta).min(last);
        self.selected = Some(self.rows[next].pane_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panes::PaneStatus;

    fn row(pane_id: u64, name: &str) -> PaneRow {
        PaneRow {
            pane_id,
            tab_id: 1,
            status: PaneStatus::Working,
            name: name.to_string(),
            cwd: "/tui/x".to_string(),
            is_active: false,
        }
    }

    fn app_with(ids: &[u64]) -> App {
        let mut app = App::default();
        app.update(Msg::Panes(ids.iter().map(|&i| row(i, "t")).collect()));
        app
    }

    #[test]
    fn first_panes_msg_selects_first_row() {
        let app = app_with(&[3, 7, 9]);
        assert_eq!(app.selected, Some(3));
        assert_eq!(app.selected_index(), Some(0));
    }

    #[test]
    fn selection_moves_and_clamps_at_edges() {
        let mut app = app_with(&[3, 7, 9]);
        app.update(Msg::Key(Action::Up));
        assert_eq!(app.selected, Some(3), "clamped at top");
        app.update(Msg::Key(Action::Down));
        app.update(Msg::Key(Action::Down));
        app.update(Msg::Key(Action::Down));
        assert_eq!(app.selected, Some(9), "clamped at bottom");
    }

    #[test]
    fn refresh_preserves_selection_by_pane_id() {
        let mut app = app_with(&[3, 7, 9]);
        app.update(Msg::Key(Action::Down)); // select 7
                                            // New pane 5 appears before 7 — index shifts, identity must not.
        app.update(Msg::Panes(vec![
            row(3, "a"),
            row(5, "n"),
            row(7, "b"),
            row(9, "c"),
        ]));
        assert_eq!(app.selected, Some(7));
    }

    #[test]
    fn vanished_selection_clamps_to_nearest_row() {
        let mut app = app_with(&[3, 7, 9]);
        app.update(Msg::Key(Action::Down));
        app.update(Msg::Key(Action::Down)); // select 9 (index 2)
        app.update(Msg::Panes(vec![row(3, "a"), row(7, "b")]));
        assert_eq!(app.selected, Some(7), "clamped to last remaining row");
    }

    #[test]
    fn empty_board_clears_selection_and_jump_is_noop() {
        let mut app = app_with(&[3]);
        app.update(Msg::Panes(Vec::new()));
        assert_eq!(app.selected, None);
        app.update(Msg::Key(Action::Jump));
        assert_eq!(app.pending_jump, None);
    }

    #[test]
    fn jump_requests_selected_pane() {
        let mut app = app_with(&[3, 7]);
        app.update(Msg::Key(Action::Jump));
        assert_eq!(app.pending_jump, Some(3));
    }

    #[test]
    fn good_refresh_resets_age_and_clears_error() {
        let mut app = app_with(&[3]);
        app.update(Msg::Tick);
        app.update(Msg::Tick);
        app.update(Msg::Error("boom".into()));
        assert_eq!(app.refresh_age_secs, 2);
        app.update(Msg::Panes(vec![row(3, "t")]));
        assert_eq!(app.refresh_age_secs, 0);
        assert_eq!(app.error, None);
    }

    #[test]
    fn error_is_kept_with_stale_rows() {
        let mut app = app_with(&[3]);
        app.update(Msg::Error("wezterm.exe: timed out after 5s".into()));
        assert_eq!(app.rows.len(), 1, "last rows kept");
        assert!(app
            .error
            .as_deref()
            .is_some_and(|e| e.contains("timed out")));
    }

    #[test]
    fn quit_and_refresh_flags() {
        let mut app = app_with(&[3]);
        app.update(Msg::Key(Action::Refresh));
        assert!(app.refresh_requested);
        app.update(Msg::Key(Action::Quit));
        assert!(app.should_quit);
    }
}
