//! The one sensor pipeline: scan sessions + telemetry + panes + codex → sorted board rows.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/collect.rs
//! Deps:    discovery, telemetry, board, codex, panes, paths (all fetched/pure seams)
//! Tested:  n/a directly — its steps are table-tested in their own modules; this is the shared
//!          orchestration so `tui::sweep` and `snapshot::run` can never diverge (spec 009).
//!
//! Key responsibilities:
//! - Run the identical row-assembly both the live board and `fleet snapshot` need, once: Claude
//!   rows (`board::assemble`) then Codex rows (`codex::scan`) appended, sorted once
//!   (`board::sort_rows`) — snapshot and board come from THIS code, so numbers never disagree.
//!
//! Design constraints:
//! - Blocking fs work (`discovery::scan`, tail reads, `codex::scan`) — call inside
//!   `spawn_blocking`, never on the UI task.
//! - The caches are borrowed for the whole call; the TUI passes its persistent ones (held under
//!   the sweep mutex), the snapshot passes fresh ones. Read-only over the fleet.

use std::path::Path;

use crate::board::{self, SessionRow};
use crate::discovery::{self, ScanStats};
use crate::error::AppResult;
use crate::panes::{PaneCache, PaneRow};
use crate::telemetry::{TailCache, Telemetry};
use crate::{codex, paths};

/// One full sensor pass, ready for the board or the snapshot.
#[derive(Debug)]
pub struct Collected {
    /// Assembled, sorted session rows (Claude + Codex, one sort).
    pub rows: Vec<SessionRow>,
    /// Discovery tallies (footer + doctor + snapshot exit code).
    pub stats: ScanStats,
    /// A degraded lane (e.g. wezterm unreachable) — rows are still valid.
    pub lane_error: Option<String>,
    /// Live Codex rows folded into `rows` this pass (footer `· N codex`).
    pub codex_count: usize,
}

/// Scan + fold the whole fleet into sorted rows, reusing the given caches. `panes_result` is the
/// already-fetched `panes::list_all_panes` output (fetched off the blocking task).
pub fn collect(
    tails: &mut TailCache,
    pane_cache: &mut PaneCache,
    panes_result: AppResult<(Vec<PaneRow>, Option<String>)>,
) -> Collected {
    let claude_dir = paths::claude_dir();
    let (sessions, stats) = discovery::scan(&claude_dir.join("sessions"), Path::new("/proc"));
    let projects = claude_dir.join("projects");
    let telemetry: Vec<Telemetry> = sessions
        .iter()
        .map(|s| tails.read(&projects, &s.file.cwd, &s.file.session_id))
        .collect();
    let live_ids: Vec<&str> = sessions
        .iter()
        .map(|s| s.file.session_id.as_str())
        .collect();
    tails.retain(&live_ids);
    let (pane_rows, lane_error) = pane_cache.fold(panes_result);
    let mut rows = board::assemble(&sessions, &telemetry, &pane_rows);
    let codex_rows = codex::scan(&paths::codex_dir(), Path::new("/proc"), &pane_rows);
    let codex_count = codex_rows.len();
    rows.extend(codex_rows);
    board::sort_rows(&mut rows);
    Collected {
        rows,
        stats,
        lane_error,
        codex_count,
    }
}
