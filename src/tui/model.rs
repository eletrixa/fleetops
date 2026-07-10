//! TUI state machine: `App` + `update(Msg)` — pure, no I/O.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/tui/model.rs
//! Deps:    crate::board (SessionRow), crate::discovery (ScanStats), crate::highlight
//!          (Tint, HighlightCmd)
//! Tested:  inline `#[cfg(test)]` unit tests (selection, refresh, quit, errors, pane-highlight
//!          transitions per spec 006)
//!
//! Key responsibilities:
//! - Hold board state: session rows, selection (keyed by sessionId), refresh age, sensor errors.
//! - Fold every `Msg` into the next state; expose effect requests (jump/refresh/highlight) as
//!   flags the event loop drains — the model itself never performs them.
//! - Diff each sweep's statuses against the previous one (`tint_state`) to decide pane-tint
//!   writes: finish pulse, steady amber/red, sticky green until "noticed" (spec 006).
//!
//! Design constraints:
//! - Pure: no clocks, no subprocess, no terminal. Age is counted in ticks fed by the loop.
//! - Selection is keyed by `session_id`, not row index — a refresh must not steal the cursor.
//! - `tint_state` invariant: never holds `Tint::None` — a reset removes the entry instead, so
//!   "absent" and "no tint applied" mean the same thing throughout this module.

use std::collections::{HashMap, HashSet};

use crate::board::{MatchedPane, SessionRow};
use crate::discovery::ScanStats;
use crate::fold::Status;
use crate::highlight::{self, HighlightCmd, Tint};

/// User intent, mapped from keys (see `keys.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Move selection up one row.
    Up,
    /// Move selection down one row.
    Down,
    /// Jump to the selected session's pane.
    Jump,
    /// Force a sensor refresh now.
    Refresh,
    /// Quit the board.
    Quit,
}

/// One full sensor sweep: assembled rows + scan tallies + a non-fatal lane error, if any.
#[derive(Debug, Default)]
pub struct Snapshot {
    /// Monotone sweep number, assigned at spawn time. Sweeps run concurrently and finish out
    /// of order (a wezterm timeout takes 5 s, the next poll fires at 2 s) — a snapshot older
    /// than the newest applied one is dropped, never shown (stale-shown-as-live class).
    pub seq: u64,
    /// Assembled, sorted session rows.
    pub rows: Vec<SessionRow>,
    /// Discovery tallies (footer + doctor).
    pub stats: ScanStats,
    /// A degraded lane (e.g. wezterm unreachable) — rows are still valid.
    pub lane_error: Option<String>,
    /// Live Codex TUI rows folded into `rows` this sweep — the footer appends `· N codex` when
    /// this is > 0 (spec 008; Codex rows aren't counted in `ScanStats.live`, which is
    /// Claude-only).
    pub codex_count: usize,
}

/// Everything the event loop can feed the model.
#[derive(Debug)]
pub enum Msg {
    /// A completed sensor sweep.
    Snapshot(Box<Snapshot>),
    /// A whole-sweep or jump failure to surface in the footer.
    Error(String),
    /// A key mapped to an action.
    Key(Action),
    /// One second elapsed.
    Tick,
}

/// The board state. `view` reads it; only `update` writes it.
#[derive(Debug, Default)]
pub struct App {
    /// Session rows, sorted by attention bucket then name.
    pub rows: Vec<SessionRow>,
    /// Latest scan tallies.
    pub stats: ScanStats,
    /// Live Codex rows in the latest snapshot — the footer's `· N codex` suffix (spec 008).
    pub codex_count: usize,
    /// Selected session's id (survives refreshes); `None` when the board is empty.
    pub selected: Option<String>,
    /// Seconds since the last successful sweep.
    pub refresh_age_secs: u64,
    /// Last sensor/jump error, shown in the footer until the next clean sweep.
    pub error: Option<String>,
    /// Loop flag: exit.
    pub should_quit: bool,
    /// Effect request: jump to this pane (drained by the loop) — activate the tab too (pane
    /// activation alone doesn't bring the tab forward), against the pane's own instance.
    pub pending_jump: Option<MatchedPane>,
    /// Effect request: poll the sensors now (drained by the loop).
    pub refresh_requested: bool,
    /// Effect request: OSC pane-tint writes computed for the latest processed message —
    /// drained by the loop exactly like `pending_jump` (spec 006).
    pub pending_highlights: Vec<HighlightCmd>,
    /// Seq of the newest applied snapshot; older arrivals are dropped.
    applied_seq: u64,
    /// Per-session last-applied tint + its pts (spec 006) — dedup/stickiness/"noticed"
    /// bookkeeping for the transition detection in `Msg::Snapshot`.
    tint_state: HashMap<String, (Tint, String)>,
    /// Whether the first-snapshot hygiene reset (spec 006) has already fired this run.
    hygiene_swept: bool,
}

impl App {
    /// True when every spawned sweep (up to `latest_seq`) has landed. The loop coalesces manual
    /// refreshes on this — key autorepeat must not stack a sweep (and its subprocess fan-out)
    /// per keypress. An errored sweep never lands; the next 2 s poll unblocks that case.
    pub const fn sweeps_settled(&self, latest_seq: u64) -> bool {
        self.applied_seq >= latest_seq
    }

    /// Index of the selected row, if it still exists.
    pub fn selected_index(&self) -> Option<usize> {
        let id = self.selected.as_deref()?;
        self.rows.iter().position(|r| r.session_id == id)
    }

    /// Fold one message into the next state.
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Snapshot(snapshot) => {
                if snapshot.seq < self.applied_seq {
                    return; // an older sweep finished late — never let it stomp fresh data
                }
                self.applied_seq = snapshot.seq;
                let old_index = self.selected_index();
                let prev_statuses: HashMap<String, Status> = self
                    .rows
                    .iter()
                    .map(|r| (r.session_id.clone(), r.status))
                    .collect();
                self.rows = snapshot.rows;
                self.stats = snapshot.stats;
                self.codex_count = snapshot.codex_count;
                self.refresh_age_secs = 0;
                self.error = snapshot.lane_error;
                self.reselect(old_index);
                self.update_highlights(&prev_statuses);
            }
            Msg::Error(e) => self.error = Some(e),
            Msg::Tick => self.refresh_age_secs = self.refresh_age_secs.saturating_add(1),
            Msg::Key(action) => self.apply(action),
        }
    }

    /// Keep the selection on the same session if it survived; otherwise clamp to the nearest row.
    fn reselect(&mut self, old_index: Option<usize>) {
        if self.rows.is_empty() {
            self.selected = None;
            return;
        }
        let still_there = self
            .selected
            .as_deref()
            .is_some_and(|id| self.rows.iter().any(|r| r.session_id == id));
        if still_there {
            return;
        }
        let index = old_index.unwrap_or(0).min(self.rows.len() - 1);
        self.selected = Some(self.rows[index].session_id.clone());
    }

    /// Diff this sweep's statuses against the previous one and populate `pending_highlights`
    /// (spec 006): finish-pulse, steady amber/red, sticky green until "noticed", first-run
    /// hygiene, and resets for sessions that vanished since the last sweep.
    fn update_highlights(&mut self, prev_statuses: &HashMap<String, Status>) {
        let is_first_snapshot = !self.hygiene_swept;
        self.hygiene_swept = true;

        for row in &self.rows {
            let Some(pts) = row.pts.clone() else {
                continue; // no pts — never highlightable (headless/non-wezterm session)
            };
            let id = row.session_id.clone();
            let prev_status = prev_statuses.get(&id).copied();
            let current_tint = self
                .tint_state
                .get(&id)
                .map_or(Tint::None, |(tint, _)| *tint);

            let just_finished = prev_status == Some(Status::Working) && row.status == Status::Idle;
            if just_finished {
                self.pending_highlights
                    .push(HighlightCmd::Pulse { pts: pts.clone() });
                self.tint_state.insert(id, (Tint::Green, pts));
                continue;
            }
            if row.status == Status::Idle && current_tint == Tint::Green {
                continue; // sticky green — "noticed" only by leaving Idle or a jump
            }

            let desired = highlight::desired_tint(row.status);
            if desired == current_tint {
                if is_first_snapshot && desired == Tint::None {
                    // Hygiene: clear a stale tint left by a crashed/killed previous `fleet`.
                    self.pending_highlights.push(HighlightCmd::Steady {
                        pts,
                        tint: Tint::None,
                    });
                }
                continue;
            }
            self.pending_highlights.push(HighlightCmd::Steady {
                pts: pts.clone(),
                tint: desired,
            });
            if desired == Tint::None {
                self.tint_state.remove(&id);
            } else {
                self.tint_state.insert(id, (desired, pts));
            }
        }

        let live_ids: HashSet<&str> = self.rows.iter().map(|r| r.session_id.as_str()).collect();
        let vanished: Vec<(String, String)> = self
            .tint_state
            .iter()
            .filter(|(id, _)| !live_ids.contains(id.as_str()))
            .map(|(id, (_, pts))| (id.clone(), pts.clone()))
            .collect();
        for (id, pts) in vanished {
            self.pending_highlights.push(HighlightCmd::Steady {
                pts,
                tint: Tint::None,
            });
            self.tint_state.remove(&id);
        }
    }

    /// pts of every session currently holding a tint — the quit-time cleanup target (spec 006).
    pub fn tinted_pts(&self) -> Vec<String> {
        self.tint_state
            .values()
            .map(|(_, pts)| pts.clone())
            .collect()
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Refresh => self.refresh_requested = true,
            Action::Jump => {
                let Some(index) = self.selected_index() else {
                    return;
                };
                let row = &self.rows[index];
                let session_id = row.session_id.clone();
                match &row.pane {
                    Some(p) => {
                        self.pending_jump = Some(p.clone());
                        // Jump "notices" a sticky finish-green tint and clears it (spec 006);
                        // amber/red reflect an unresolved state and are left untouched. Only
                        // fires when the jump actually dispatched — a failed jump never notices.
                        if matches!(self.tint_state.get(&session_id), Some((Tint::Green, _))) {
                            if let Some((_, pts)) = self.tint_state.remove(&session_id) {
                                self.pending_highlights.push(HighlightCmd::Steady {
                                    pts,
                                    tint: Tint::None,
                                });
                            }
                        }
                    }
                    None if row.pane_ambiguous => {
                        self.error = Some(format!("jump: several panes match '{}'", row.name));
                    }
                    None => {
                        self.error = Some(format!("jump: no pane matched '{}'", row.name));
                    }
                }
            }
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
        self.selected = Some(self.rows[next].session_id.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::MatchedPane;
    use crate::fold::Status;
    use crate::highlight::{HighlightCmd, Tint};

    /// Existing rows: default status `Working`, no pts (pre-wave-6 shape preserved).
    fn row(id: &str, pane: Option<(u64, u64)>, ambiguous: bool) -> SessionRow {
        row_full(id, pane, ambiguous, Status::Working, None)
    }

    fn row_full(
        id: &str,
        pane: Option<(u64, u64)>,
        ambiguous: bool,
        status: Status,
        pts: Option<&str>,
    ) -> SessionRow {
        SessionRow {
            session_id: id.to_string(),
            name: format!("session {id}"),
            account: Some("golf-acct".to_string()),
            status,
            cwd: "/tui/x".to_string(),
            context_tokens: Some(50_000),
            ctx_pct: None,
            secs_since_append: Some(3),
            pane: pane.map(|(tab_id, pane_id)| MatchedPane {
                socket: String::new(),
                tab_id,
                pane_id,
                tab_index: 1,
            }),
            pane_ambiguous: ambiguous,
            pts: pts.map(str::to_string),
        }
    }

    fn snapshot(rows: Vec<SessionRow>) -> Msg {
        Msg::Snapshot(Box::new(Snapshot {
            rows,
            ..Snapshot::default()
        }))
    }

    fn app_with(ids: &[&str]) -> App {
        let mut app = App::default();
        app.update(snapshot(
            ids.iter().map(|id| row(id, Some((1, 9)), false)).collect(),
        ));
        app
    }

    #[test]
    fn first_snapshot_selects_first_row() {
        let app = app_with(&["a", "b", "c"]);
        assert_eq!(app.selected.as_deref(), Some("a"));
    }

    #[test]
    fn selection_moves_and_clamps_at_edges() {
        let mut app = app_with(&["a", "b", "c"]);
        app.update(Msg::Key(Action::Up));
        assert_eq!(app.selected.as_deref(), Some("a"), "clamped at top");
        for _ in 0..5 {
            app.update(Msg::Key(Action::Down));
        }
        assert_eq!(app.selected.as_deref(), Some("c"), "clamped at bottom");
    }

    #[test]
    fn refresh_preserves_selection_by_session_id() {
        let mut app = app_with(&["a", "b", "c"]);
        app.update(Msg::Key(Action::Down)); // select b
                                            // Rows resort: b moves to index 2.
        app.update(snapshot(vec![
            row("c", None, false),
            row("a", None, false),
            row("b", None, false),
        ]));
        assert_eq!(app.selected.as_deref(), Some("b"));
        assert_eq!(app.selected_index(), Some(2));
    }

    #[test]
    fn vanished_selection_clamps_to_nearest_row() {
        let mut app = app_with(&["a", "b", "c"]);
        app.update(Msg::Key(Action::Down));
        app.update(Msg::Key(Action::Down)); // select c (index 2)
        app.update(snapshot(vec![row("a", None, false), row("b", None, false)]));
        assert_eq!(
            app.selected.as_deref(),
            Some("b"),
            "clamped to last remaining row"
        );
    }

    #[test]
    fn empty_board_clears_selection() {
        let mut app = app_with(&["a"]);
        app.update(snapshot(Vec::new()));
        assert_eq!(app.selected, None);
        app.update(Msg::Key(Action::Jump));
        assert_eq!(app.pending_jump, None);
    }

    #[test]
    fn jump_uses_matched_tab_and_pane() {
        let mut app = app_with(&["a"]);
        app.update(Msg::Key(Action::Jump));
        assert_eq!(
            app.pending_jump.as_ref().map(|p| (p.tab_id, p.pane_id)),
            Some((1, 9)),
            "tab id needed to bring the tab forward"
        );
    }

    #[test]
    fn jump_without_pane_match_reports_not_crashes() {
        let mut app = App::default();
        app.update(snapshot(vec![row("a", None, false)]));
        app.update(Msg::Key(Action::Jump));
        assert_eq!(app.pending_jump, None);
        assert!(app
            .error
            .as_deref()
            .is_some_and(|e| e.contains("no pane matched")));

        let mut app = App::default();
        app.update(snapshot(vec![row("a", None, true)]));
        app.update(Msg::Key(Action::Jump));
        assert!(app
            .error
            .as_deref()
            .is_some_and(|e| e.contains("several panes")));
    }

    #[test]
    fn clean_snapshot_resets_age_and_error_degraded_lane_error_sticks() {
        let mut app = app_with(&["a"]);
        app.update(Msg::Tick);
        app.update(Msg::Error("boom".into()));
        app.update(snapshot(vec![row("a", None, false)]));
        assert_eq!(app.refresh_age_secs, 0);
        assert_eq!(app.error, None);

        app.update(Msg::Snapshot(Box::new(Snapshot {
            rows: vec![row("a", None, false)],
            lane_error: Some("wezterm.exe: timed out after 5s".to_string()),
            ..Snapshot::default()
        })));
        assert!(app
            .error
            .as_deref()
            .is_some_and(|e| e.contains("timed out")));
    }

    #[test]
    fn late_older_sweep_is_dropped() {
        let mut app = App::default();
        app.update(Msg::Snapshot(Box::new(Snapshot {
            seq: 2,
            rows: vec![row("fresh", Some((1, 9)), false)],
            ..Snapshot::default()
        })));
        // An older sweep (seq 1) finishes late — with stale rows and a stale lane error.
        app.update(Msg::Snapshot(Box::new(Snapshot {
            seq: 1,
            rows: vec![],
            lane_error: Some("wezterm.exe: timed out after 5s".to_string()),
            ..Snapshot::default()
        })));
        assert_eq!(app.rows.len(), 1, "fresh rows kept");
        assert_eq!(app.error, None, "stale lane error never shown");
    }

    #[test]
    fn sweeps_settled_tracks_the_applied_seq() {
        let mut app = App::default();
        assert!(app.sweeps_settled(0), "nothing spawned = settled");
        assert!(!app.sweeps_settled(1), "sweep 1 in flight");
        app.update(Msg::Snapshot(Box::new(Snapshot {
            seq: 1,
            ..Snapshot::default()
        })));
        assert!(app.sweeps_settled(1));
    }

    #[test]
    fn quit_and_refresh_flags() {
        let mut app = app_with(&["a"]);
        app.update(Msg::Key(Action::Refresh));
        assert!(app.refresh_requested);
        app.update(Msg::Key(Action::Quit));
        assert!(app.should_quit);
    }

    // --- spec 006: pane-highlight transition table (RED phase — model logic not wired yet) ---

    #[test]
    fn finish_transition_emits_pulse() {
        let mut app = App::default();
        app.update(snapshot(vec![row_full(
            "a",
            None,
            false,
            Status::Working,
            Some("/dev/pts/3"),
        )]));
        app.pending_highlights.clear(); // drop this sweep's hygiene write, as the loop would
        app.update(snapshot(vec![row_full(
            "a",
            None,
            false,
            Status::Idle,
            Some("/dev/pts/3"),
        )]));
        assert_eq!(
            app.pending_highlights,
            vec![HighlightCmd::Pulse {
                pts: "/dev/pts/3".to_string()
            }],
            "Working -> Idle is the finish transition"
        );
    }

    #[test]
    fn needs_answer_emits_amber_steady_once_not_every_sweep() {
        let mut app = App::default();
        let make = || {
            vec![row_full(
                "a",
                None,
                false,
                Status::NeedsAnswer,
                Some("/dev/pts/4"),
            )]
        };
        app.update(snapshot(make()));
        assert_eq!(
            app.pending_highlights,
            vec![HighlightCmd::Steady {
                pts: "/dev/pts/4".to_string(),
                tint: Tint::Amber
            }],
            "first sighting of NeedsAnswer sets steady amber"
        );
        app.pending_highlights.clear();
        app.update(snapshot(make())); // identical second sweep
        assert!(
            app.pending_highlights.is_empty(),
            "steady state already applied — no repeat write every sweep"
        );
    }

    #[test]
    fn green_sticky_across_idle_snapshots_emits_nothing_more() {
        let mut app = App::default();
        app.update(snapshot(vec![row_full(
            "a",
            None,
            false,
            Status::Working,
            Some("/dev/pts/5"),
        )]));
        app.pending_highlights.clear();
        app.update(snapshot(vec![row_full(
            "a",
            None,
            false,
            Status::Idle,
            Some("/dev/pts/5"),
        )])); // finish -> pulse settling into sticky green
        app.pending_highlights.clear();
        app.update(snapshot(vec![row_full(
            "a",
            None,
            false,
            Status::Idle,
            Some("/dev/pts/5"),
        )])); // still idle, still green
        assert!(
            app.pending_highlights.is_empty(),
            "green stays sticky — no repeat command while still Idle"
        );
    }

    #[test]
    fn leaving_idle_resets_the_sticky_green() {
        let mut app = App::default();
        app.update(snapshot(vec![row_full(
            "a",
            None,
            false,
            Status::Working,
            Some("/dev/pts/6"),
        )]));
        app.pending_highlights.clear();
        app.update(snapshot(vec![row_full(
            "a",
            None,
            false,
            Status::Idle,
            Some("/dev/pts/6"),
        )])); // finish -> green
        app.pending_highlights.clear();
        app.update(snapshot(vec![row_full(
            "a",
            None,
            false,
            Status::Working,
            Some("/dev/pts/6"),
        )])); // prompted again — Idle is left
        assert_eq!(
            app.pending_highlights,
            vec![HighlightCmd::Steady {
                pts: "/dev/pts/6".to_string(),
                tint: Tint::None
            }],
            "leaving Idle clears the sticky green"
        );
    }

    #[test]
    fn jump_clears_green_but_not_amber() {
        let mut app = App::default();
        app.update(snapshot(vec![row_full(
            "g",
            Some((1, 9)),
            false,
            Status::Working,
            Some("/dev/pts/7"),
        )]));
        app.pending_highlights.clear();
        app.update(snapshot(vec![row_full(
            "g",
            Some((1, 9)),
            false,
            Status::Idle,
            Some("/dev/pts/7"),
        )])); // finish -> green
        app.pending_highlights.clear();
        app.update(Msg::Key(Action::Jump));
        assert_eq!(
            app.pending_highlights,
            vec![HighlightCmd::Steady {
                pts: "/dev/pts/7".to_string(),
                tint: Tint::None
            }],
            "jump 'notices' green and clears it"
        );

        let mut app = App::default();
        app.update(snapshot(vec![row_full(
            "a",
            Some((1, 9)),
            false,
            Status::NeedsAnswer,
            Some("/dev/pts/8"),
        )]));
        app.pending_highlights.clear();
        app.update(Msg::Key(Action::Jump));
        assert!(
            app.pending_highlights.is_empty(),
            "amber/red persist until resolved — jump doesn't clear them"
        );
    }

    #[test]
    fn failed_jump_does_not_clear_green() {
        let mut app = App::default();
        app.update(snapshot(vec![row_full(
            "g",
            None,
            false,
            Status::Working,
            Some("/dev/pts/7"),
        )]));
        app.pending_highlights.clear();
        app.update(snapshot(vec![row_full(
            "g",
            None,
            false,
            Status::Idle,
            Some("/dev/pts/7"),
        )])); // finish -> green, but pane: None so the jump below fails
        app.pending_highlights.clear();
        app.update(Msg::Key(Action::Jump));
        assert!(
            app.pending_highlights.is_empty(),
            "a failed jump must not clear the sticky green"
        );
        assert!(
            app.error
                .as_deref()
                .is_some_and(|e| e.contains("no pane matched")),
            "jump failure still reported"
        );
    }

    #[test]
    fn vanished_session_resets_its_last_pts() {
        let mut app = App::default();
        app.update(snapshot(vec![row_full(
            "a",
            None,
            false,
            Status::NeedsAnswer,
            Some("/dev/pts/9"),
        )]));
        app.pending_highlights.clear();
        app.update(snapshot(Vec::new())); // session gone
        assert_eq!(
            app.pending_highlights,
            vec![HighlightCmd::Steady {
                pts: "/dev/pts/9".to_string(),
                tint: Tint::None
            }],
            "a vanished tinted session must be reset, not left tinted forever"
        );
    }

    #[test]
    fn first_snapshot_hygiene_resets_untinted_sessions_once() {
        let mut app = App::default();
        app.update(snapshot(vec![row_full(
            "a",
            None,
            false,
            Status::Working,
            Some("/dev/pts/10"),
        )]));
        assert_eq!(
            app.pending_highlights,
            vec![HighlightCmd::Steady {
                pts: "/dev/pts/10".to_string(),
                tint: Tint::None
            }],
            "first sweep of a run clears stale tints left by a crashed previous fleet"
        );
        app.pending_highlights.clear();
        app.update(snapshot(vec![row_full(
            "a",
            None,
            false,
            Status::Working,
            Some("/dev/pts/10"),
        )])); // identical second sweep
        assert!(
            app.pending_highlights.is_empty(),
            "hygiene reset fires once per run, not every sweep"
        );
    }

    #[test]
    fn rows_without_pts_never_emit_a_highlight() {
        let mut app = App::default();
        app.update(snapshot(vec![row_full(
            "a",
            None,
            false,
            Status::NeedsAnswer,
            None,
        )]));
        assert!(
            app.pending_highlights.is_empty(),
            "no pts to write to — headless/non-wezterm sessions are never targeted"
        );
    }

    #[test]
    fn late_older_sweep_emits_no_highlight_commands() {
        let mut app = App::default();
        app.update(Msg::Snapshot(Box::new(Snapshot {
            seq: 2,
            rows: vec![row_full(
                "a",
                None,
                false,
                Status::Working,
                Some("/dev/pts/11"),
            )],
            ..Snapshot::default()
        })));
        app.pending_highlights.clear();
        app.update(Msg::Snapshot(Box::new(Snapshot {
            seq: 1, // stale — dropped by the existing seq guard before highlight logic runs
            rows: vec![row_full(
                "a",
                None,
                false,
                Status::NeedsAnswer,
                Some("/dev/pts/11"),
            )],
            ..Snapshot::default()
        })));
        assert!(
            app.pending_highlights.is_empty(),
            "a late-finishing older sweep must not emit stale highlight commands either"
        );
    }
}
