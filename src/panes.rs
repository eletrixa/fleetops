//! panes ctx: wezterm pane list — parse, classify Claude panes, build jump commands.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/panes.rs
//! Deps:    serde/serde_json; crate::runner (fetch only — parsing is pure)
//! Tested:  inline `#[cfg(test)]` against tests/fixtures/wezterm-list.json (captured live 2026-07-10)
//!
//! Key responsibilities:
//! - Discover ALL live wezterm instances (tasklist PIDs × `gui-sock-<pid>` files) — a `cli`
//!   call answers only from the instance owning the invoking pane's interop, so fleet running
//!   on the TUI monitor sees zero Claude panes unless each instance is targeted explicitly
//!   via a WSLENV-forwarded `WEZTERM_UNIX_SOCKET` (verified live 2026-07-10; flag `/w`).
//! - Parse `cli list --format json` tolerantly; classify titles (braille spinner = Working,
//!   `✳` = Idle, else not a Claude pane); merge instances; per-window 0-based tab indexing
//!   (matches `activate-tab --tab-index`).
//! - Build `list` / `activate-tab` / `activate-pane` / `list-clients` argv+env (pure);
//!   `focused_pane_id` reads the least-idle client's focused pane (spec 009 `fleet snapshot`).
//!
//! Design constraints:
//! - Glyph convention is undocumented (dossier assumption A2): classification must stay a pure
//!   table-tested function so a format change is a one-function fix.
//! - Stale gui-sock files HANG on connect (verified) — only tasklist-live PIDs are queried,
//!   every call stays timeout-bounded.
//! - Read-only over the fleet: the only mutating verbs are activate-tab/-pane (focus).

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
    /// Windows-form socket path of the owning wezterm instance (empty = invoker's own) —
    /// pane/tab ids are only unique WITHIN an instance, and jumps must target the right one.
    pub socket: String,
    /// wezterm pane id — identity and jump target.
    pub pane_id: u64,
    /// wezterm tab id — display grouping.
    pub tab_id: u64,
    /// 0-based index of this pane's tab within its window — matches `wezterm cli activate-tab
    /// --tab-index` (0 = left-most tab), derived from list order, counting ALL tabs incl.
    /// non-Claude ones (spec 009; supersedes the wave-7 1-based tab-bar number).
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

/// argv for `wezterm.exe cli list --format json`. `--no-auto-start` on every cli call: a
/// socketless environment must yield a visible lane error, never silently spawn a mux server.
pub fn list_args() -> Vec<String> {
    ["cli", "--no-auto-start", "list", "--format", "json"]
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// argv for `wezterm.exe cli activate-pane --pane-id <id>`.
pub fn activate_pane_args(pane_id: u64) -> Vec<String> {
    vec![
        "cli".to_string(),
        "--no-auto-start".to_string(),
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
        "--no-auto-start".to_string(),
        "activate-tab".to_string(),
        "--tab-id".to_string(),
        tab_id.to_string(),
    ]
}

/// argv for `wezterm.exe cli list-clients --format json` — the focused-pane source (spec 009).
pub fn list_clients_args() -> Vec<String> {
    ["cli", "--no-auto-start", "list-clients", "--format", "json"]
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// The wezterm binary as reachable from WSL2.
pub const WEZTERM: &str = "wezterm.exe";
/// Where the interop binary actually lives on this box — the fallback when fleet is launched
/// with a minimal PATH (keybinding/launcher shells often lack /mnt/c/...).
const WEZTERM_ABSOLUTE: &str = "/mnt/c/Program Files/WezTerm/wezterm.exe";

/// Resolve the wezterm program: plain name when PATH can find it (normal shells), the absolute
/// install path when it can't but the file exists, else the plain name (spawn error stays
/// visible in the footer). Pure over its inputs for testability.
fn resolve_wezterm(path_var: Option<&std::ffi::OsStr>, absolute: &std::path::Path) -> String {
    let on_path =
        path_var.is_some_and(|p| std::env::split_paths(p).any(|dir| dir.join(WEZTERM).is_file()));
    if on_path {
        WEZTERM.to_string()
    } else if absolute.is_file() {
        absolute.to_string_lossy().into_owned()
    } else {
        WEZTERM.to_string()
    }
}

/// The resolved program, computed once per process.
fn wezterm_program() -> String {
    static PROGRAM: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PROGRAM
        .get_or_init(|| {
            let path_var = std::env::var_os("PATH");
            resolve_wezterm(path_var.as_deref(), std::path::Path::new(WEZTERM_ABSOLUTE))
        })
        .clone()
}

/// Where a wezterm instance's `gui-sock-<pid>` files live, in the two forms `discover_sockets`
/// needs: `wsl` to stat the files from WSL, `win` to build the Windows-form `WEZTERM_UNIX_SOCKET`
/// value forwarded back to `wezterm.exe`. Both derive from one resolved Windows username.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SockDir {
    wsl: std::path::PathBuf,
    win: String,
}

/// Resolve the wezterm socket directory at runtime — the Windows username is a machine fact, not
/// a compile-time constant. Order (first hit wins, cheapest first):
/// 1. `$WEZTERM_UNIX_SOCKET` — wezterm's own var, pointing straight at the invoking pane's
///    socket; its parent directory is authoritative, no guessing.
/// 2. Glob `<glob_root>/*/.local/share/wezterm` for a dir holding a `gui-sock-*` file; one match
///    wins outright, several prefer the `$USER`-named dir, else the newest socket's dir.
/// 3. `None` — the caller degrades to the invoker's own instance (the board still works).
///
/// `glob_root` is a seam: production passes `/mnt/c/Users`, tests a temp layout. Pure over its
/// inputs so the whole ladder is unit-tested without touching the real filesystem or env.
/// ponytail: assumes the default `~/.local/share/wezterm` layout under `/mnt/c/Users/<name>`; a
/// Windows-side custom `$XDG_DATA_HOME` is only followed via branch 1's `WEZTERM_UNIX_SOCKET`.
fn resolve_sock_dir(
    glob_root: &std::path::Path,
    env_socket: Option<&str>,
    user: Option<&str>,
) -> Option<SockDir> {
    if let Some(sock) = env_socket.filter(|s| !s.is_empty()) {
        if let Some(dir) = sock_dir_from_socket(sock) {
            return Some(dir);
        }
    }
    let mut candidates = glob_wezterm_dirs(glob_root);
    let chosen = match candidates.len() {
        0 => return None,
        1 => candidates.remove(0),
        _ => pick_user_dir(candidates, user),
    };
    Some(sock_dir_from_wsl(&chosen))
}

/// Both dir forms from `$WEZTERM_UNIX_SOCKET` (a `.../gui-sock-N` path) — its parent is the dir.
/// A `/`-leading value is a WSL path; anything else is a Windows path (`C:\…` or `C:/…`).
fn sock_dir_from_socket(sock: &str) -> Option<SockDir> {
    if sock.starts_with('/') {
        let wsl = std::path::Path::new(sock).parent()?.to_path_buf();
        let win = wsl_to_win(&wsl)?;
        Some(SockDir { wsl, win })
    } else {
        let (dir, _) = sock.rsplit_once(['\\', '/'])?;
        let win = dir.replace('/', "\\");
        let wsl = win_to_wsl(&win)?;
        Some(SockDir { wsl, win })
    }
}

/// A resolved WSL dir → both forms (Windows form best-effort; unused when spawning is impossible,
/// e.g. a temp-dir test root that does not live under `/mnt/<drive>`).
fn sock_dir_from_wsl(wsl: &std::path::Path) -> SockDir {
    let win = wsl_to_win(wsl).unwrap_or_else(|| wsl.to_string_lossy().into_owned());
    SockDir {
        wsl: wsl.to_path_buf(),
        win,
    }
}

/// `/mnt/c/Users/user/x` → `C:\Users\user\x`. `None` if not under `/mnt/<drive>/`.
fn wsl_to_win(wsl: &std::path::Path) -> Option<String> {
    let (drive, path) = wsl.to_str()?.strip_prefix("/mnt/")?.split_once('/')?;
    Some(format!(
        "{}:\\{}",
        drive.to_ascii_uppercase(),
        path.replace('/', "\\")
    ))
}

/// `C:\Users\user\x` (or `C:/Users/user/x`) → `/mnt/c/Users/user/x`. `None` if not `<drive>:`-rooted.
fn win_to_wsl(win: &str) -> Option<std::path::PathBuf> {
    let norm = win.replace('\\', "/");
    let (drive, rest) = norm.split_once(":/")?;
    let drive = drive.chars().next()?.to_ascii_lowercase();
    Some(std::path::PathBuf::from(format!("/mnt/{drive}/{rest}")))
}

/// Every `<glob_root>/<user>/.local/share/wezterm` that exists and holds a `gui-sock-*` file.
fn glob_wezterm_dirs(glob_root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(glob_root) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path().join(".local/share/wezterm"))
        .filter(|dir| newest_sock_mtime(dir).is_some())
        .collect()
}

/// Among several candidate dirs, prefer the one named for `$USER`; else the newest socket's dir.
fn pick_user_dir(candidates: Vec<std::path::PathBuf>, user: Option<&str>) -> std::path::PathBuf {
    if let Some(u) = user {
        // The `<user>` segment is the dir 3 levels above `.local/share/wezterm`.
        if let Some(hit) = candidates
            .iter()
            .find(|d| d.ancestors().nth(3).and_then(|p| p.file_name()) == Some(u.as_ref()))
        {
            return hit.clone();
        }
    }
    candidates
        .into_iter()
        .max_by_key(|d| newest_sock_mtime(d))
        .unwrap_or_default()
}

/// The newest `gui-sock-*` mtime in `dir`, or `None` if the dir holds no socket file.
fn newest_sock_mtime(dir: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("gui-sock-"))
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
}

/// The wezterm socket dir, resolved once per process (`None` = not found → own-instance fallback).
/// Globbing `/mnt/c` is drvfs-slow, so this only ever runs inside `discover_sockets`' blocking task.
fn sock_dir() -> Option<SockDir> {
    static DIR: std::sync::OnceLock<Option<SockDir>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        resolve_sock_dir(
            std::path::Path::new("/mnt/c/Users"),
            std::env::var("WEZTERM_UNIX_SOCKET").ok().as_deref(),
            std::env::var("USER").ok().as_deref(),
        )
    })
    .clone()
}

/// argv for `tasklist.exe` filtered to wezterm-gui processes, CSV form.
pub fn tasklist_args() -> Vec<String> {
    ["/FI", "IMAGENAME eq wezterm-gui.exe", "/FO", "CSV"]
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// Build the bounded `tasklist.exe` command.
pub fn tasklist_spec() -> CommandSpec {
    CommandSpec {
        program: "tasklist.exe".to_string(),
        args: tasklist_args(),
        env: Vec::new(),
        timeout: Duration::from_secs(5),
    }
}

/// Parse tasklist CSV → wezterm-gui PIDs. Tolerant: malformed lines skipped.
pub fn parse_tasklist_pids(bytes: &[u8]) -> Vec<u32> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split("\",\"");
            let image = fields.next()?.trim_start_matches('"');
            if !image.eq_ignore_ascii_case("wezterm-gui.exe") {
                return None;
            }
            fields.next()?.parse().ok()
        })
        .collect()
}

/// The env pair that targets one wezterm instance from WSL: the socket var itself plus a
/// per-process WSLENV telling interop to forward it (flag `/w` = WSL→Win32 direction).
fn socket_env(socket_win: &str) -> Vec<(String, String)> {
    vec![
        ("WEZTERM_UNIX_SOCKET".to_string(), socket_win.to_string()),
        ("WSLENV".to_string(), "WEZTERM_UNIX_SOCKET/w".to_string()),
    ]
}

/// Discover live instances: tasklist PIDs whose `gui-sock-<pid>` file exists.
/// Dead PIDs' stale socket files HANG on connect — this filter is load-bearing.
pub async fn discover_sockets(runner: &dyn Runner) -> AppResult<Vec<String>> {
    let bytes = runner.run(&tasklist_spec()).await?;
    let pids = parse_tasklist_pids(&bytes);
    // /mnt/c (drvfs) stats can block for seconds — never on an async runtime worker. Resolving the
    // socket dir globs /mnt/c too (same drvfs cost), so it also happens here, cached per process.
    tokio::task::spawn_blocking(move || {
        let Some(dir) = sock_dir() else {
            return Vec::new(); // no wezterm dir on this box — degrade to the invoker's own instance
        };
        pids.into_iter()
            .filter(|pid| dir.wsl.join(format!("gui-sock-{pid}")).is_file())
            .map(|pid| format!("{}\\gui-sock-{pid}", dir.win))
            .collect()
    })
    .await
    .map_err(|e| AppError::Subprocess {
        program: "tasklist.exe".to_string(),
        message: format!("socket filter task: {e}"),
    })
}

/// Query every live instance and merge their Claude panes. Degrades per instance: a failing
/// instance's error rides along as the lane note (the footer must say the pane lane is
/// degraded — silence here blanks that instance's TAB/PANE with no explanation); only total
/// failure (or discovery failure) is an error.
pub async fn list_all_panes(runner: &dyn Runner) -> AppResult<(Vec<PaneRow>, Option<String>)> {
    let sockets = discover_sockets(runner).await?;
    if sockets.is_empty() {
        // No instance discovered (tasklist empty?) — fall back to the invoker's own instance.
        return Ok((list_panes(runner, "").await?, None));
    }
    let queries = sockets.iter().map(|s| list_panes(runner, s));
    merge_instance_results(futures::future::join_all(queries).await)
}

/// Merge per-instance results: rows from every healthy instance + the first failure (if any).
fn merge_instance_results(
    results: Vec<AppResult<Vec<PaneRow>>>,
) -> AppResult<(Vec<PaneRow>, Option<String>)> {
    let mut merged = Vec::new();
    let mut any_ok = false;
    let mut first_err = None;
    for result in results {
        match result {
            Ok(mut rows) => {
                any_ok = true;
                merged.append(&mut rows);
            }
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
    }
    match (any_ok, first_err) {
        (false, Some(e)) => Err(e), // no instance answered — the lane is down
        (_, e) => Ok((merged, e.map(|e| e.to_string()))),
    }
}

/// Last-good pane list: a wezterm lane error must not blank the board's TAB/PANE columns —
/// stale matches (with the error in the footer) beat no matches.
#[derive(Debug, Default)]
pub struct PaneCache {
    last: Vec<PaneRow>,
}

impl PaneCache {
    /// Fold a lane result: a CLEAN success replaces the cache; a partial success (an instance
    /// failed) must not — its rows would silently evict the failing instance's last-good panes.
    /// Failures return the last good list; every degradation carries its footer string.
    pub fn fold(
        &mut self,
        result: AppResult<(Vec<PaneRow>, Option<String>)>,
    ) -> (Vec<PaneRow>, Option<String>) {
        match result {
            Ok((rows, None)) => {
                self.last.clone_from(&rows);
                (rows, None)
            }
            Ok((rows, Some(e))) => {
                if self.last.is_empty() {
                    (rows, Some(e)) // cold cache: partial beats blank (uncached — retry heals)
                } else {
                    (self.last.clone(), Some(e))
                }
            }
            Err(e) => (self.last.clone(), Some(e.to_string())),
        }
    }
}

/// Build a bounded wezterm command against one instance ("" = invoker's own).
fn wezterm_spec(socket_win: &str, args: Vec<String>) -> CommandSpec {
    CommandSpec {
        program: wezterm_program(),
        args,
        env: if socket_win.is_empty() {
            Vec::new()
        } else {
            socket_env(socket_win)
        },
        timeout: Duration::from_secs(5),
    }
}

/// Build the bounded `cli list` command against one instance ("" = invoker's own).
pub fn list_spec(socket_win: &str) -> CommandSpec {
    wezterm_spec(socket_win, list_args())
}

/// Build the bounded `activate-pane` command against the pane's instance.
pub fn activate_pane_spec(socket_win: &str, pane_id: u64) -> CommandSpec {
    wezterm_spec(socket_win, activate_pane_args(pane_id))
}

/// Build the bounded `activate-tab` command against the pane's instance.
pub fn activate_tab_spec(socket_win: &str, tab_id: u64) -> CommandSpec {
    wezterm_spec(socket_win, activate_tab_args(tab_id))
}

/// Build the bounded `list-clients` command against one instance ("" = invoker's own).
pub fn list_clients_spec(socket_win: &str) -> CommandSpec {
    wezterm_spec(socket_win, list_clients_args())
}

/// One `list-clients` client entry — only the two fields the focused-pane pick needs.
#[derive(Debug, Deserialize)]
struct RawClient {
    #[serde(default)]
    focused_pane_id: Option<u64>,
    #[serde(default)]
    idle_time: Option<RawDuration>,
}

/// wezterm serializes a `Duration` as `{secs, nanos}`; only whole seconds matter here.
#[derive(Debug, Deserialize)]
struct RawDuration {
    #[serde(default)]
    secs: u64,
}

/// Parse `list-clients --format json` → `(focused_pane_id, idle_secs)` per client that has a
/// focused pane. Tolerant: garbage or an unexpected shape yields an empty list (the focused
/// pane is best-effort, never an error). A client with no `idle_time` sorts last (`u64::MAX`).
pub fn parse_clients(bytes: &[u8]) -> Vec<(u64, u64)> {
    let clients: Vec<RawClient> = serde_json::from_slice(bytes).unwrap_or_default();
    clients
        .into_iter()
        .filter_map(|c| Some((c.focused_pane_id?, c.idle_time.map_or(u64::MAX, |d| d.secs))))
        .collect()
}

/// The focused pane across all clients: the one with the least idle time (the client the user is
/// actively on). `None` when no client reports a focused pane.
/// ponytail: least-idle is a heuristic; if multiple GUIs are ever active at once and this picks
/// the wrong one, key on the client whose pane is also `is_active` in the pane list.
pub fn pick_focused_pane_id(clients: &[(u64, u64)]) -> Option<u64> {
    clients
        .iter()
        .min_by_key(|(_, idle)| *idle)
        .map(|(pane, _)| *pane)
}

/// The fleet's focused pane id: query `list-clients` on every live instance (own instance when
/// none is discovered, mirroring `list_all_panes`) and pick the least-idle client's focused pane
/// (spec 009). Best-effort — an unreachable lane is `None`, never an error.
pub async fn focused_pane_id(runner: &dyn Runner) -> Option<u64> {
    let sockets = discover_sockets(runner).await.unwrap_or_default();
    let sockets = if sockets.is_empty() {
        vec![String::new()]
    } else {
        sockets
    };
    let queries = sockets.iter().map(|s| {
        let spec = list_clients_spec(s);
        async move {
            runner
                .run(&spec)
                .await
                .map(|bytes| parse_clients(&bytes))
                .unwrap_or_default()
        }
    });
    let clients: Vec<(u64, u64)> = futures::future::join_all(queries)
        .await
        .into_iter()
        .flatten()
        .collect();
    pick_focused_pane_id(&clients)
}

/// Run `cli list` against one instance and return its Claude pane rows, sorted by `pane_id`.
pub async fn list_panes(runner: &dyn Runner, socket_win: &str) -> AppResult<Vec<PaneRow>> {
    let bytes = runner.run(&list_spec(socket_win)).await?;
    parse_pane_list(&bytes, socket_win)
}

/// Parse `cli list --format json` bytes into Claude pane rows, sorted by `pane_id`.
/// Non-Claude panes (no recognized glyph) are excluded; rows are stamped with their instance.
pub fn parse_pane_list(bytes: &[u8], socket_win: &str) -> AppResult<Vec<PaneRow>> {
    let raw: Vec<RawPane> =
        serde_json::from_slice(bytes).map_err(|e| AppError::Parse(format!("wezterm list: {e}")))?;
    // Tab indexing: wezterm lists panes in window/tab order, so a tab's 0-based index within its
    // window = order of first appearance (matches `activate-tab --tab-index`, 0 = left-most).
    // Counted over ALL panes (non-Claude tabs occupy tab slots too) BEFORE the pane_id sort
    // below destroys that order.
    let mut tab_positions: std::collections::HashMap<(u64, u64), u64> =
        std::collections::HashMap::new();
    let mut per_window: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
    for p in &raw {
        tab_positions
            .entry((p.window_id, p.tab_id))
            .or_insert_with(|| {
                let counter = per_window.entry(p.window_id).or_insert(0);
                let index = *counter; // 0-based: assign, then advance
                *counter += 1;
                index
            });
    }
    let mut rows: Vec<PaneRow> = raw
        .into_iter()
        .filter_map(|p| {
            let (status, name) = classify_title(&p.title)?;
            Some(PaneRow {
                socket: socket_win.to_string(),
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
        let rows = parse_pane_list(FIXTURE, "").expect("fixture parses");
        assert!(!rows.is_empty(), "fixture has Claude panes");
        assert!(rows.windows(2).all(|w| w[0].pane_id < w[1].pane_id));
        // The fixture contains wslhost.exe and empty-title panes — none may survive.
        assert!(rows.iter().all(|r| !r.name.contains("wslhost")));
    }

    #[test]
    fn fixture_row_fields_are_extracted() {
        let rows = parse_pane_list(FIXTURE, "").expect("fixture parses");
        let fleet = rows
            .iter()
            .find(|r| r.name.contains("FleetOps"))
            .expect("this session's pane is in the fixture");
        assert_eq!(fleet.status, PaneStatus::Working);
        assert_eq!(fleet.cwd, "/tui/fleetops");
        // Fixture order: tab_id 1 first, then tab_id 3 (this pane) — the 2nd tab → 0-based
        // index 1 (matches `activate-tab --tab-index`, spec 009).
        assert_eq!(fleet.tab_index, 1);
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
        let rows = parse_pane_list(json.as_bytes(), "").expect("tolerant parse");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].pane_id, 7);
        assert_eq!(rows[0].cwd, "");
        assert!(!rows[0].is_active);
    }

    #[test]
    fn garbage_input_is_a_parse_error() {
        assert!(matches!(
            parse_pane_list(b"not json", ""),
            Err(AppError::Parse(_))
        ));
    }

    #[test]
    fn argv_builders() {
        // --no-auto-start everywhere: a socketless call must fail visibly, never spawn a
        // headless mux server as a side effect of a 2 s sweep.
        assert_eq!(
            list_args(),
            ["cli", "--no-auto-start", "list", "--format", "json"]
        );
        assert_eq!(
            activate_pane_args(42),
            ["cli", "--no-auto-start", "activate-pane", "--pane-id", "42"]
        );
        assert_eq!(
            activate_tab_args(7),
            ["cli", "--no-auto-start", "activate-tab", "--tab-id", "7"]
        );
    }

    #[test]
    fn merge_surfaces_partial_instance_failure_and_keeps_total_failure_an_error() {
        let row = |id: u64| PaneRow {
            socket: "C:\\sock-a".to_string(),
            pane_id: id,
            tab_id: 1,
            tab_index: 1,
            status: PaneStatus::Working,
            name: "x".to_string(),
            cwd: "/x".to_string(),
            is_active: false,
        };
        let timeout = || {
            Err(AppError::Timeout {
                program: WEZTERM.to_string(),
                seconds: 5,
            })
        };
        // Partial: healthy rows AND the failing instance's error — the footer must not stay silent.
        let (rows, err) =
            merge_instance_results(vec![Ok(vec![row(1)]), timeout()]).expect("partial is Ok");
        assert_eq!(rows.len(), 1);
        assert!(err.is_some_and(|e| e.contains("timed out")));
        // Clean: no error.
        let (rows, err) =
            merge_instance_results(vec![Ok(vec![row(1)]), Ok(vec![row(2)])]).expect("clean merge");
        assert_eq!(rows.len(), 2);
        assert_eq!(err, None);
        // Total failure: an error, not an empty success.
        assert!(merge_instance_results(vec![timeout(), timeout()]).is_err());
    }

    #[test]
    fn pane_cache_never_overwrites_last_good_with_a_partial_list() {
        let full = vec![
            PaneRow {
                socket: "C:\\sock-a".to_string(),
                pane_id: 1,
                tab_id: 1,
                tab_index: 1,
                status: PaneStatus::Working,
                name: "a".to_string(),
                cwd: "/a".to_string(),
                is_active: false,
            },
            PaneRow {
                socket: "C:\\sock-b".to_string(),
                pane_id: 1,
                tab_id: 1,
                tab_index: 1,
                status: PaneStatus::Idle,
                name: "b".to_string(),
                cwd: "/b".to_string(),
                is_active: false,
            },
        ];
        let mut cache = PaneCache::default();
        let (got, err) = cache.fold(Ok((full.clone(), None)));
        assert_eq!(got.len(), 2);
        assert_eq!(err, None);

        // Instance B degraded: last-good (both instances) survives, error surfaces.
        let partial = vec![full[0].clone()];
        let (got, err) = cache.fold(Ok((partial, Some("sock-b: timed out".to_string()))));
        assert_eq!(got, full, "stale full list beats fresh partial list");
        assert!(err.is_some_and(|e| e.contains("timed out")));

        // Cold cache + partial: the partial rows are still better than nothing.
        let mut cold = PaneCache::default();
        let (got, err) = cold.fold(Ok((vec![full[0].clone()], Some("e".to_string()))));
        assert_eq!(got.len(), 1);
        assert!(err.is_some());
    }

    #[test]
    fn resolve_wezterm_prefers_path_then_absolute_fallback() {
        let tmp = std::env::temp_dir().join(format!("fleet-wez-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let exe = tmp.join("wezterm.exe");

        // Not on PATH, fallback file exists → absolute fallback wins.
        std::fs::write(&exe, b"").unwrap();
        let path_var = std::ffi::OsString::from("/nonexistent-dir");
        assert_eq!(
            resolve_wezterm(Some(&path_var), &exe),
            exe.to_string_lossy().as_ref()
        );

        // On PATH → plain program name (PATH resolution at spawn).
        let path_var = std::ffi::OsString::from(format!("/nonexistent-dir:{}", tmp.display()));
        assert_eq!(resolve_wezterm(Some(&path_var), &exe), WEZTERM);

        // Neither → plain name (spawn error stays visible in the footer).
        std::fs::remove_file(&exe).unwrap();
        let path_var = std::ffi::OsString::from("/nonexistent-dir");
        assert_eq!(resolve_wezterm(Some(&path_var), &exe), WEZTERM);
        assert_eq!(resolve_wezterm(None, &exe), WEZTERM);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn pane_cache_keeps_last_good_list_on_lane_error() {
        let mut cache = PaneCache::default();
        let rows = vec![PaneRow {
            socket: String::new(),
            pane_id: 1,
            tab_id: 1,
            tab_index: 1,
            status: PaneStatus::Working,
            name: "x".to_string(),
            cwd: "/x".to_string(),
            is_active: false,
        }];

        // Success populates the cache and reports no error.
        let (got, err) = cache.fold(Ok((rows.clone(), None)));
        assert_eq!(got, rows);
        assert_eq!(err, None);

        // Failure returns the LAST GOOD list (stale matches beat no matches) + the error.
        let (got, err) = cache.fold(Err(AppError::Timeout {
            program: WEZTERM.to_string(),
            seconds: 5,
        }));
        assert_eq!(got, rows, "stale pane list survives a lane error");
        assert!(err.is_some_and(|e| e.contains("timed out")));

        // Next clean success replaces it again.
        let (got, err) = cache.fold(Ok((Vec::new(), None)));
        assert!(got.is_empty());
        assert_eq!(err, None);
    }

    #[tokio::test]
    async fn list_panes_runs_the_list_spec() {
        let runner = CannedRunner::new(FIXTURE.to_vec());
        let rows = list_panes(&runner, "").await.expect("canned list parses");
        assert!(!rows.is_empty());
        let spec = runner.last_spec().expect("spec recorded");
        // Program resolves via PATH or the absolute fallback depending on the test env.
        assert!(spec.program.ends_with(WEZTERM), "got {}", spec.program);
        assert_eq!(spec.args, list_args());
    }

    #[test]
    fn tasklist_csv_parses_to_pids() {
        let csv = b"\"Image Name\",\"PID\",\"Session Name\",\"Session#\",\"Mem Usage\"\r\n\
\"wezterm-gui.exe\",\"18840\",\"Console\",\"1\",\"139,280 K\"\r\n\
\"wezterm-gui.exe\",\"3428\",\"Console\",\"1\",\"218,680 K\"\r\n\
garbage line\r\n";
        assert_eq!(parse_tasklist_pids(csv), vec![18_840, 3_428]);
        assert!(parse_tasklist_pids(b"INFO: No tasks are running.\r\n").is_empty());
    }

    #[tokio::test]
    async fn list_panes_stamps_rows_with_their_instance_and_targets_it() {
        let runner = CannedRunner::new_seq(vec![Ok(FIXTURE.to_vec()), Ok(FIXTURE.to_vec())]);
        let a = list_panes(&runner, "C:\\sock-a").await.expect("instance a");
        let b = list_panes(&runner, "C:\\sock-b").await.expect("instance b");
        assert!(a.iter().all(|p| p.socket == "C:\\sock-a"));
        assert!(b.iter().all(|p| p.socket == "C:\\sock-b"));

        let specs = runner.all_specs();
        assert_eq!(specs.len(), 2);
        // Each call carries the WSLENV-forwarded socket env targeting ITS instance.
        assert_eq!(
            specs[0].env,
            vec![
                ("WEZTERM_UNIX_SOCKET".to_string(), "C:\\sock-a".to_string()),
                ("WSLENV".to_string(), "WEZTERM_UNIX_SOCKET/w".to_string()),
            ]
        );
        assert_eq!(specs[1].env[0].1, "C:\\sock-b");
    }

    #[test]
    fn parse_clients_extracts_focused_pane_and_idle_secs() {
        // Ground-truthed shape (list-clients --format json, captured live 2026-07-12).
        let json = br#"[{"username":"user","hostname":"host","pid":12848,"connection_elapsed":{"secs":25969,"nanos":7},"idle_time":{"secs":3,"nanos":7},"workspace":"default","focused_pane_id":21,"ssh_auth_sock":null},{"pid":2,"idle_time":{"secs":100,"nanos":0},"focused_pane_id":5}]"#;
        assert_eq!(parse_clients(json), vec![(21, 3), (5, 100)]);
        // A client with no focused pane is skipped; a missing idle_time sorts last.
        let j2 = br#"[{"idle_time":{"secs":1}},{"focused_pane_id":9}]"#;
        assert_eq!(parse_clients(j2), vec![(9, u64::MAX)]);
        // Garbage / wrong shape → empty, never an error (focused pane is best-effort).
        assert!(parse_clients(b"not json").is_empty());
        assert!(parse_clients(b"[]").is_empty());
    }

    #[test]
    fn pick_focused_pane_id_prefers_the_least_idle_client() {
        assert_eq!(pick_focused_pane_id(&[(21, 3), (5, 100)]), Some(21));
        assert_eq!(pick_focused_pane_id(&[(5, 100), (21, 3)]), Some(21));
        assert_eq!(pick_focused_pane_id(&[]), None);
    }

    #[tokio::test]
    async fn focused_pane_id_falls_back_to_own_instance_and_picks_the_focused_pane() {
        // Empty tasklist → no instances discovered → fall back to the invoker's own instance →
        // exactly one list-clients call answers the focused pane.
        let tasklist = b"INFO: No tasks are running.\r\n".to_vec();
        let clients = br#"[{"idle_time":{"secs":2},"focused_pane_id":21}]"#.to_vec();
        let runner = CannedRunner::new_seq(vec![Ok(tasklist), Ok(clients)]);
        assert_eq!(focused_pane_id(&runner).await, Some(21));
        let specs = runner.all_specs();
        assert_eq!(specs.len(), 2, "tasklist then list-clients");
        assert_eq!(specs[1].args, list_clients_args());
    }

    #[test]
    fn jump_specs_carry_the_socket_env() {
        let tab = activate_tab_spec("C:\\sock-a", 7);
        assert_eq!(
            tab.args,
            ["cli", "--no-auto-start", "activate-tab", "--tab-id", "7"]
        );
        assert_eq!(tab.env[0].1, "C:\\sock-a");
        let pane = activate_pane_spec("", 9);
        assert!(pane.env.is_empty(), "own instance needs no socket env");
    }

    /// Build `<root>/<user>/.local/share/wezterm/gui-sock-<pid>` and return the socket path.
    fn mk_sock(root: &std::path::Path, user: &str, pid: u32) -> std::path::PathBuf {
        let dir = root.join(user).join(".local/share/wezterm");
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join(format!("gui-sock-{pid}"));
        std::fs::write(&sock, b"").unwrap();
        sock
    }

    fn set_mtime(f: &std::path::Path, secs: u64) {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        std::fs::File::options()
            .write(true)
            .open(f)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    fn tmp_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("fleet-sock-{}-{tag}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn resolve_sock_dir_env_socket_is_authoritative_both_forms() {
        // Windows-form value → parent dir, WSL form derived. Glob root is irrelevant (not touched).
        let got = resolve_sock_dir(
            std::path::Path::new("/nonexistent"),
            Some("C:\\Users\\user\\.local\\share\\wezterm\\gui-sock-42"),
            None,
        )
        .expect("env socket resolves");
        assert_eq!(got.win, "C:\\Users\\user\\.local\\share\\wezterm");
        assert_eq!(
            got.wsl,
            std::path::Path::new("/mnt/c/Users/user/.local/share/wezterm")
        );

        // WSL-form value → same dir, Windows form derived back.
        let got = resolve_sock_dir(
            std::path::Path::new("/nonexistent"),
            Some("/mnt/c/Users/user/.local/share/wezterm/gui-sock-42"),
            None,
        )
        .expect("wsl-form env socket resolves");
        assert_eq!(
            got.wsl,
            std::path::Path::new("/mnt/c/Users/user/.local/share/wezterm")
        );
        assert_eq!(got.win, "C:\\Users\\user\\.local\\share\\wezterm");
    }

    #[test]
    fn resolve_sock_dir_globs_the_single_user_dir_that_has_a_socket() {
        let root = tmp_root("single");
        mk_sock(&root, "rob", 7);
        // A user dir whose wezterm folder holds NO socket must be ignored, not chosen.
        std::fs::create_dir_all(root.join("empty/.local/share/wezterm")).unwrap();

        let got = resolve_sock_dir(&root, None, None).expect("one socketed dir resolves");
        assert_eq!(got.wsl, root.join("rob/.local/share/wezterm"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolve_sock_dir_none_when_no_dir_holds_a_socket() {
        let root = tmp_root("none");
        std::fs::create_dir_all(root.join("rob/.local/share/wezterm")).unwrap(); // dir, no socket
        assert_eq!(resolve_sock_dir(&root, None, None), None);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolve_sock_dir_multi_user_prefers_the_current_user_over_mtime() {
        let root = tmp_root("multi-user");
        let alice = mk_sock(&root, "alice", 1);
        mk_sock(&root, "rob", 2);
        // alice's socket is newer, but $USER=rob must still win.
        set_mtime(&alice, 9_000);
        let got = resolve_sock_dir(&root, None, Some("rob")).expect("resolves");
        assert_eq!(got.wsl, root.join("rob/.local/share/wezterm"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolve_sock_dir_multi_user_falls_back_to_newest_socket() {
        let root = tmp_root("multi-mtime");
        let older = mk_sock(&root, "alice", 1);
        let newer = mk_sock(&root, "bob", 2);
        set_mtime(&older, 1_000);
        set_mtime(&newer, 2_000);
        // No matching $USER → newest socket's dir wins.
        let got = resolve_sock_dir(&root, None, Some("nobody")).expect("resolves");
        assert_eq!(got.wsl, root.join("bob/.local/share/wezterm"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn path_converters_round_trip() {
        assert_eq!(
            wsl_to_win(std::path::Path::new("/mnt/c/Users/user/x")).unwrap(),
            "C:\\Users\\user\\x"
        );
        assert_eq!(
            win_to_wsl("C:\\Users\\user\\x").unwrap(),
            std::path::Path::new("/mnt/c/Users/user/x")
        );
        // wezterm.exe accepts forward slashes too.
        assert_eq!(
            win_to_wsl("C:/Users/user/x").unwrap(),
            std::path::Path::new("/mnt/c/Users/user/x")
        );
        // Non-convertible inputs degrade to None (caller keeps the WSL form).
        assert!(wsl_to_win(std::path::Path::new("/home/user/x")).is_none());
        assert!(win_to_wsl("not-a-win-path").is_none());
    }
}
