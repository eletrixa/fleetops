//! panes ctx: wezterm pane list — parse, classify Claude panes, build jump commands.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/panes.rs
//! Deps:    serde/serde_json; crate::runner (fetch only — parsing is pure)
//! Tested:  inline `#[cfg(test)]` against tests/fixtures/wezterm-list.json (captured live 2026-07-10)
//!
//! Key responsibilities:
//! - Parse `wezterm.exe cli list --format json` output tolerantly (unknown fields skipped).
//! - Classify pane titles: braille spinner = Working, `✳` = Idle, else not a Claude pane.
//! - Shorten `file://` cwd URLs for display; build `list` / `activate-pane` argv (pure).
//!
//! Design constraints:
//! - Glyph convention is undocumented (dossier assumption A2): classification must stay a pure
//!   table-tested function so a format change is a one-function fix.
//! - Read-only over the fleet: the only mutating verb built here is `activate-pane` (focus).

use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::runner::{CommandSpec, Runner};

/// Status of a Claude pane, read from its title glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneStatus {
    /// Title starts with a braille spinner frame (U+2800–U+28FF) — Claude is working.
    Working,
    /// Title starts with `✳` — Claude is idle (waiting for the user).
    Idle,
}

/// One Claude pane row on the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRow {
    /// wezterm pane id — identity and jump target.
    pub pane_id: u64,
    /// wezterm tab id — display grouping.
    pub tab_id: u64,
    /// 1-based position of this pane's tab within its window (the tab-bar number the maintainer sees;
    /// derived from list order, counting ALL tabs incl. non-Claude ones).
    pub tab_index: u64,
    /// Glyph-derived status.
    pub status: PaneStatus,
    /// Title with the glyph prefix stripped — the session's semantic name.
    pub name: String,
    /// Shortened cwd for display.
    pub cwd: String,
    /// Whether wezterm reports this pane as the active one.
    pub is_active: bool,
}

/// Raw wezterm pane entry — only the fields we read; everything else is skipped.
#[derive(Debug, Deserialize)]
struct RawPane {
    pane_id: u64,
    tab_id: u64,
    #[serde(default)]
    window_id: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    is_active: bool,
}

/// argv for `wezterm.exe cli list --format json`.
pub fn list_args() -> Vec<String> {
    ["cli", "list", "--format", "json"]
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// argv for `wezterm.exe cli activate-pane --pane-id <id>`.
pub fn activate_pane_args(pane_id: u64) -> Vec<String> {
    vec![
        "cli".to_string(),
        "activate-pane".to_string(),
        "--pane-id".to_string(),
        pane_id.to_string(),
    ]
}

/// argv for `wezterm.exe cli activate-tab --tab-id <id>` — activate-pane alone focuses the
/// pane within its tab but does NOT bring the tab forward; a jump runs both.
pub fn activate_tab_args(tab_id: u64) -> Vec<String> {
    vec![
        "cli".to_string(),
        "activate-tab".to_string(),
        "--tab-id".to_string(),
        tab_id.to_string(),
    ]
}

/// The wezterm binary as reachable from WSL2.
pub const WEZTERM: &str = "wezterm.exe";

/// Build the bounded `cli list` command.
pub fn list_spec() -> CommandSpec {
    CommandSpec {
        program: WEZTERM.to_string(),
        args: list_args(),
        timeout: Duration::from_secs(5),
    }
}

/// Build the bounded `activate-pane` command.
pub fn activate_pane_spec(pane_id: u64) -> CommandSpec {
    CommandSpec {
        program: WEZTERM.to_string(),
        args: activate_pane_args(pane_id),
        timeout: Duration::from_secs(5),
    }
}

/// Build the bounded `activate-tab` command.
pub fn activate_tab_spec(tab_id: u64) -> CommandSpec {
    CommandSpec {
        program: WEZTERM.to_string(),
        args: activate_tab_args(tab_id),
        timeout: Duration::from_secs(5),
    }
}

/// Run `cli list` via `runner` and return the Claude pane rows, sorted by `pane_id`.
pub async fn list_panes(runner: &dyn Runner) -> AppResult<Vec<PaneRow>> {
    let bytes = runner.run(&list_spec()).await?;
    parse_pane_list(&bytes)
}

/// Parse `cli list --format json` bytes into Claude pane rows, sorted by `pane_id`.
/// Non-Claude panes (no recognized glyph) are excluded.
pub fn parse_pane_list(bytes: &[u8]) -> AppResult<Vec<PaneRow>> {
    let raw: Vec<RawPane> =
        serde_json::from_slice(bytes).map_err(|e| AppError::Parse(format!("wezterm list: {e}")))?;
    // Tab-bar numbering: wezterm lists panes in window/tab order, so a tab's 1-based position
    // within its window = order of first appearance. Counted over ALL panes (non-Claude tabs
    // occupy tab-bar slots too) BEFORE the pane_id sort below destroys that order.
    let mut tab_positions: std::collections::HashMap<(u64, u64), u64> =
        std::collections::HashMap::new();
    let mut per_window: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
    for p in &raw {
        tab_positions
            .entry((p.window_id, p.tab_id))
            .or_insert_with(|| {
                let counter = per_window.entry(p.window_id).or_insert(0);
                *counter += 1;
                *counter
            });
    }
    let mut rows: Vec<PaneRow> = raw
        .into_iter()
        .filter_map(|p| {
            let (status, name) = classify_title(&p.title)?;
            Some(PaneRow {
                pane_id: p.pane_id,
                tab_id: p.tab_id,
                tab_index: tab_positions
                    .get(&(p.window_id, p.tab_id))
                    .copied()
                    .unwrap_or(0),
                status,
                name,
                cwd: short_cwd(&p.cwd),
                is_active: p.is_active,
            })
        })
        .collect();
    rows.sort_by_key(|r| r.pane_id);
    Ok(rows)
}

/// Classify a pane title by its leading glyph; `None` = not a Claude pane.
/// Returns the status and the title with glyph + following whitespace stripped.
fn classify_title(title: &str) -> Option<(PaneStatus, String)> {
    let mut chars = title.chars();
    let first = chars.next()?;
    let status = match first {
        '\u{2800}'..='\u{28FF}' => PaneStatus::Working,
        '✳' => PaneStatus::Idle,
        _ => return None,
    };
    Some((status, chars.as_str().trim_start().to_string()))
}

/// Shorten a wezterm `file://` cwd URL for display.
/// `file://wsl.localhost/<distro>/a/b` → `/a/b`; `file:///C:/x/y` → `C:/x/y`; else verbatim.
fn short_cwd(cwd: &str) -> String {
    let trimmed = cwd.trim_end_matches('/');
    if let Some(rest) = trimmed.strip_prefix("file://wsl.localhost/") {
        // Drop the distro segment, keep the absolute WSL path.
        return match rest.split_once('/') {
            Some((_distro, path)) => format!("/{path}"),
            None => "/".to_string(),
        };
    }
    if let Some(rest) = trimmed.strip_prefix("file:///") {
        return rest.to_string();
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::CannedRunner;

    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/wezterm-list.json");

    #[test]
    fn fixture_parses_to_claude_rows_only_sorted_by_pane_id() {
        let rows = parse_pane_list(FIXTURE).expect("fixture parses");
        assert!(!rows.is_empty(), "fixture has Claude panes");
        assert!(rows.windows(2).all(|w| w[0].pane_id < w[1].pane_id));
        // The fixture contains wslhost.exe and empty-title panes — none may survive.
        assert!(rows.iter().all(|r| !r.name.contains("wslhost")));
    }

    #[test]
    fn fixture_row_fields_are_extracted() {
        let rows = parse_pane_list(FIXTURE).expect("fixture parses");
        let fleet = rows
            .iter()
            .find(|r| r.name.contains("FleetOps"))
            .expect("this session's pane is in the fixture");
        assert_eq!(fleet.status, PaneStatus::Working);
        assert_eq!(fleet.cwd, "/tui/fleetops");
        // Fixture order: tab 1 first, then tab 3 (this pane) — 2nd slot on the tab bar.
        assert_eq!(fleet.tab_index, 2);
    }

    #[test]
    fn classify_title_table() {
        let cases: &[(&str, Option<(PaneStatus, &str)>)] = &[
            ("⠂ Fix the bug", Some((PaneStatus::Working, "Fix the bug"))),
            ("⠐ Resume", Some((PaneStatus::Working, "Resume"))),
            ("⣿dense", Some((PaneStatus::Working, "dense"))),
            ("✳ Review skills", Some((PaneStatus::Idle, "Review skills"))),
            ("✳", Some((PaneStatus::Idle, ""))),
            ("wslhost.exe", None),
            ("", None),
            ("→ arrow title", None),
            ("plain shell", None),
        ];
        for (title, want) in cases {
            let got = classify_title(title);
            let want = want.map(|(s, n)| (s, n.to_string()));
            assert_eq!(got, want, "title {title:?}");
        }
    }

    #[test]
    fn short_cwd_table() {
        let cases = [
            ("file://wsl.localhost/Ubuntu/tui/fleetops/", "/tui/fleetops"),
            ("file://wsl.localhost/Ubuntu/", "/"),
            ("file:///C:/Users/user/", "C:/Users/user"),
            ("", ""),
            ("weird", "weird"),
        ];
        for (input, want) in cases {
            assert_eq!(short_cwd(input), want, "cwd {input:?}");
        }
    }

    #[test]
    fn unknown_fields_and_missing_optionals_are_tolerated() {
        let json = r#"[{"pane_id": 7, "tab_id": 1, "title": "⠢ x", "novel_field": {"a": 1}}]"#;
        let rows = parse_pane_list(json.as_bytes()).expect("tolerant parse");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].pane_id, 7);
        assert_eq!(rows[0].cwd, "");
        assert!(!rows[0].is_active);
    }

    #[test]
    fn garbage_input_is_a_parse_error() {
        assert!(matches!(
            parse_pane_list(b"not json"),
            Err(AppError::Parse(_))
        ));
    }

    #[test]
    fn argv_builders() {
        assert_eq!(list_args(), ["cli", "list", "--format", "json"]);
        assert_eq!(
            activate_pane_args(42),
            ["cli", "activate-pane", "--pane-id", "42"]
        );
        assert_eq!(
            activate_tab_args(7),
            ["cli", "activate-tab", "--tab-id", "7"]
        );
    }

    #[tokio::test]
    async fn list_panes_runs_the_list_spec() {
        let runner = CannedRunner::new(FIXTURE.to_vec());
        let rows = list_panes(&runner).await.expect("canned list parses");
        assert!(!rows.is_empty());
        let spec = runner.last_spec().expect("spec recorded");
        assert_eq!(spec.program, WEZTERM);
        assert_eq!(spec.args, list_args());
    }
}
