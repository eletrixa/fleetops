//! discovery ctx: live-session scan — sessions/*.json + platform liveness + account attribution.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/discovery.rs
//! Deps:    serde/serde_json; platform (OS facts seam); std::fs (called via spawn_blocking)
//! Tested:  inline `#[cfg(test)]` — fixture tests/fixtures/session-file.json + tempdir scan
//!          over the Linux provider (plain fs, so these run on every target)
//!
//! Key responsibilities:
//! - Parse `~/.claude/sessions/<pid>.json` tolerantly (undocumented internal, assumption A1).
//! - Liveness invariant: session is live iff the platform snapshot's LOSSLESS start identity
//!   matches the file's `procStart` (PID-reuse guard — exact equality, never epoch-converted;
//!   Linux compares raw stat ticks, macOS the parsed UTC-ctime second, wave 7).
//! - Attribute account from the process environment `CLAUDE_ACCOUNT` (absent → unknown).
//! - Keep the session's pty (fd 1 under the platform prefix) — the highlight write target.
//!
//! Design constraints:
//! - Read-only over the fleet; never writes into any Claude dir.
//! - Stale files for dead PIDs are EXPECTED (20/36 at recon) — they are counted, never shown live.
//! - Parsers stay pure over bytes; `scan` touches only the sessions dir + the `ProcFacts` seam.

use std::path::Path;

use serde::Deserialize;

use crate::platform::{Liveness, PlatformStats, ProcFacts, SnapshotOutcome};

/// Native coarse status from the session file. Unknown strings preserved (doctor drift signal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeStatus {
    /// Claude is processing.
    Busy,
    /// Waiting at the prompt.
    Idle,
    /// User dropped to shell mode.
    Shell,
    /// Blocked on user input (permission prompt / queued question) — found live 2026-07-10,
    /// the state class the transcript never shows.
    Waiting,
    /// A status string this version of fleetops doesn't know — surfaced, never hidden.
    Other(String),
}

impl From<&str> for NativeStatus {
    fn from(s: &str) -> Self {
        match s {
            "busy" => Self::Busy,
            "idle" => Self::Idle,
            "shell" => Self::Shell,
            "waiting" => Self::Waiting,
            other => Self::Other(other.to_string()),
        }
    }
}

/// One parsed `sessions/<pid>.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFile {
    /// Claude Code process id.
    pub pid: u32,
    /// The session UUID — the aggregate identity.
    pub session_id: String,
    /// Session working directory.
    pub cwd: String,
    /// `/proc/<pid>/stat` starttime at launch, as a string (the liveness token).
    pub proc_start: String,
    /// Derived session name (semantic title arrives via telemetry, wave 3).
    pub name: String,
    /// Native coarse status.
    pub status: NativeStatus,
    /// Last update, epoch ms.
    pub updated_at_ms: u64,
    /// Claude Code version that wrote the file (doctor drift signal).
    pub version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawSessionFile {
    pid: u32,
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(default)]
    cwd: String,
    #[serde(rename = "procStart")]
    proc_start: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: String,
    #[serde(rename = "updatedAt", default)]
    updated_at_ms: u64,
    #[serde(default)]
    version: Option<String>,
}

/// A live, attributed session — the wave-2 aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSession {
    /// Parsed session file.
    pub file: SessionFile,
    /// `CLAUDE_ACCOUNT` from the process environment, if readable.
    pub account: Option<String>,
    /// `WEZTERM_PANE` from the process environment — exact pane identity (wave 5, needs the
    /// WSLENV forwarding; absent on sessions started before the wezterm restart).
    pub wezterm_pane: Option<u64>,
    /// The session's own pty (fd 1 under the platform prefix — `/dev/pts/*` on Linux,
    /// `/dev/ttys*` on macOS; wave 6, spec 006). The highlight write target.
    pub pts: Option<String>,
}

/// Scan tallies for the doctor and footer (files seen vs shown).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ScanStats {
    /// `*.json` files in the sessions dir.
    pub total_files: usize,
    /// Files that failed to parse (drift signal).
    pub parse_failed: usize,
    /// Parsed files whose PID is dead or reused (expected leftovers).
    pub stale_dead: usize,
    /// Live sessions returned.
    pub live: usize,
    /// The sessions dir itself could not be read — an empty fleet must not look identical
    /// to a failed scan (doctor exits 1 on this; the board footer surfaces it).
    pub dir_unreadable: bool,
}

/// Parse one session file. Unknown fields are skipped; missing optional fields defaulted.
pub fn parse_session_file(bytes: &[u8]) -> Option<SessionFile> {
    let raw: RawSessionFile = serde_json::from_slice(bytes).ok()?;
    Some(SessionFile {
        pid: raw.pid,
        session_id: raw.session_id,
        cwd: raw.cwd,
        proc_start: raw.proc_start,
        name: raw.name,
        status: NativeStatus::from(raw.status.as_str()),
        updated_at_ms: raw.updated_at_ms,
        version: raw.version,
    })
}

/// Facts read from process environment bytes (NUL-separated) in one pass.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EnvironFacts {
    /// `CLAUDE_ACCOUNT` — account attribution.
    pub account: Option<String>,
    /// `WEZTERM_PANE` — exact pane identity; non-numeric values ignored.
    pub wezterm_pane: Option<u64>,
}

/// Extract the fields fleetops needs from environ bytes.
pub fn parse_environ(environ: &[u8]) -> EnvironFacts {
    let mut facts = EnvironFacts::default();
    for entry in environ
        .split(|&b| b == 0)
        .filter_map(|e| std::str::from_utf8(e).ok())
    {
        if let Some(v) = entry.strip_prefix("CLAUDE_ACCOUNT=") {
            facts.account = Some(v.to_string());
        } else if let Some(v) = entry.strip_prefix("WEZTERM_PANE=") {
            facts.wezterm_pane = v.parse().ok();
        }
    }
    facts
}

/// Scan `sessions_dir`, filter by liveness through the platform seam, attribute accounts.
/// Blocking fs/syscall work — the sensor calls this inside `spawn_blocking`.
pub fn scan(
    sessions_dir: &Path,
    proc: &dyn ProcFacts,
) -> (Vec<LiveSession>, ScanStats, PlatformStats) {
    let mut stats = ScanStats::default();
    let mut platform = PlatformStats::default();
    let mut live = Vec::new();
    let Ok(entries) = std::fs::read_dir(sessions_dir) else {
        stats.dir_unreadable = true;
        return (live, stats, platform);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        stats.total_files += 1;
        let Some(file) = std::fs::read(&path)
            .ok()
            .and_then(|b| parse_session_file(&b))
        else {
            stats.parse_failed += 1;
            continue;
        };
        let snap = match proc.snapshot(file.pid) {
            SnapshotOutcome::Gone => {
                stats.stale_dead += 1;
                continue;
            }
            SnapshotOutcome::Raced => {
                platform.identity_raced += 1;
                stats.stale_dead += 1;
                continue;
            }
            SnapshotOutcome::Present(snap) => snap,
        };
        let verdict = proc.liveness(&snap.start_id, &file.proc_start);
        platform.count_liveness(verdict);
        if verdict != Liveness::Match {
            stats.stale_dead += 1; // dead-or-reused semantics unchanged; drift counted above
            continue;
        }
        platform.count_env(&snap.env_block);
        if matches!(snap.fd1_pty, crate::platform::AcqResult::Denied) {
            platform.fd1_denied += 1;
        }
        let environ = snap
            .env_block
            .as_ref()
            .ok()
            .map(|b| parse_environ(b))
            .unwrap_or_default();
        live.push(LiveSession {
            file,
            account: environ.account,
            wezterm_pane: environ.wezterm_pane,
            pts: snap.fd1_pty.ok().flatten(),
        });
    }
    stats.live = live.len();
    (live, stats, platform)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/session-file.json");

    #[test]
    fn fixture_parses() {
        let f = parse_session_file(FIXTURE).expect("live fixture parses");
        assert_eq!(f.pid, 105_315);
        assert_eq!(f.session_id, "a01d7cea-b33a-4295-aa48-7a058966cdcb");
        assert_eq!(f.cwd, "/home/user/project-a");
        assert_eq!(f.proc_start, "126796");
        assert_eq!(f.name, "project-a-fe");
        assert_eq!(f.status, NativeStatus::Shell);
    }

    #[test]
    fn unknown_status_is_preserved_not_dropped() {
        let json = br#"{"pid":1,"sessionId":"s","procStart":"9","status":"pondering"}"#;
        let f = parse_session_file(json).expect("tolerant");
        assert_eq!(f.status, NativeStatus::Other("pondering".to_string()));
        assert_eq!(f.cwd, "", "missing optionals defaulted");
    }

    #[test]
    fn waiting_status_is_first_class() {
        // Found live 2026-07-10 (session 166350) — the input-blocked state.
        let json = br#"{"pid":1,"sessionId":"s","procStart":"9","status":"waiting"}"#;
        let f = parse_session_file(json).expect("parses");
        assert_eq!(f.status, NativeStatus::Waiting);
    }

    #[test]
    fn garbage_and_missing_required_fields_are_none() {
        assert!(parse_session_file(b"not json").is_none());
        assert!(
            parse_session_file(br#"{"pid":1}"#).is_none(),
            "sessionId required"
        );
    }

    #[test]
    fn parse_environ_extracts_account_and_pane() {
        let environ =
            b"PATH=/usr/bin\0CLAUDE_ACCOUNT=alpha\0WEZTERM_PANE=26\0CLAUDE_CONFIG_DIR=/x\0";
        let facts = parse_environ(environ);
        assert_eq!(facts.account.as_deref(), Some("alpha"));
        assert_eq!(facts.wezterm_pane, Some(26));

        assert_eq!(parse_environ(b"PATH=/usr/bin\0"), EnvironFacts::default());
        // non-numeric pane id ignored, account still extracted
        let weird = parse_environ(b"WEZTERM_PANE=abc\0CLAUDE_ACCOUNT=bravo\0");
        assert_eq!(weird.wezterm_pane, None);
        assert_eq!(weird.account.as_deref(), Some("bravo"));
    }

    /// Build a fake proc root: `<root>/<pid>/stat` (+ optional environ).
    fn fake_proc(root: &Path, pid: u32, starttime: &str, account: Option<&str>) {
        let dir = root.join(pid.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("stat"),
            format!("{pid} (claude) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 {starttime} 23"),
        )
        .unwrap();
        if let Some(a) = account {
            std::fs::write(dir.join("environ"), format!("CLAUDE_ACCOUNT={a}\0")).unwrap();
        }
    }

    fn session_json(pid: u32, proc_start: &str, status: &str) -> String {
        format!(
            r#"{{"pid":{pid},"sessionId":"sid-{pid}","cwd":"/w","procStart":"{proc_start}","name":"n{pid}","status":"{status}","updatedAt":1}}"#
        )
    }

    #[test]
    fn scan_keeps_live_drops_dead_and_reused_counts_parse_failures() {
        let tmp = std::env::temp_dir().join(format!("fleet-test-{}", std::process::id()));
        let sessions = tmp.join("sessions");
        let proc_root = tmp.join("proc");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::create_dir_all(&proc_root).unwrap();

        std::fs::write(sessions.join("1.json"), session_json(1, "100", "busy")).unwrap();
        fake_proc(&proc_root, 1, "100", Some("alpha")); // live
        std::fs::write(sessions.join("2.json"), session_json(2, "200", "idle")).unwrap();
        fake_proc(&proc_root, 2, "999", None); // PID reused (starttime differs)
        std::fs::write(sessions.join("3.json"), session_json(3, "300", "busy")).unwrap();
        // pid 3: no proc entry — dead
        std::fs::write(sessions.join("4.json"), "garbage").unwrap();
        std::fs::write(sessions.join("README.md"), "not a session").unwrap();

        let proc = crate::platform::LinuxProc::new(proc_root);
        let (live, stats, platform) = scan(&sessions, &proc);
        std::fs::remove_dir_all(&tmp).ok();

        assert_eq!(stats.total_files, 4);
        assert_eq!(stats.parse_failed, 1);
        assert_eq!(stats.stale_dead, 2);
        assert_eq!(stats.live, 1);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].file.pid, 1);
        assert_eq!(live[0].account.as_deref(), Some("alpha"));
        assert_eq!(
            platform.start_mismatch, 1,
            "the reused PID registers as an identity mismatch drift signal"
        );
        assert_eq!(platform.identity_raced, 0);
    }

    #[test]
    fn live_session_without_environ_is_live_with_unknown_account() {
        let tmp = std::env::temp_dir().join(format!("fleet-env-{}", std::process::id()));
        let sessions = tmp.join("sessions");
        let proc_root = tmp.join("proc");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::create_dir_all(&proc_root).unwrap();
        std::fs::write(sessions.join("5.json"), session_json(5, "500", "busy")).unwrap();
        fake_proc(&proc_root, 5, "500", None); // stat readable, environ absent

        let proc = crate::platform::LinuxProc::new(proc_root);
        let (live, stats, platform) = scan(&sessions, &proc);
        std::fs::remove_dir_all(&tmp).ok();

        assert_eq!(
            stats.live, 1,
            "unreadable environ never drops a live session"
        );
        assert_eq!(live[0].account, None, "absent → unknown, not error");
        assert_eq!(live[0].wezterm_pane, None);
        assert_eq!(
            platform.env_denied, 1,
            "the unreadable environ IS surfaced to the doctor"
        );
    }

    #[test]
    fn scan_of_missing_dir_is_flagged_not_a_silent_empty_fleet() {
        let proc = crate::platform::LinuxProc::new(std::path::PathBuf::from("/nonexistent-proc"));
        let (live, stats, _platform) = scan(Path::new("/nonexistent-fleet-dir"), &proc);
        assert!(live.is_empty());
        assert!(stats.dir_unreadable);
        assert_eq!(stats.total_files, 0);
    }

    #[test]
    fn scan_reads_pts_from_fd_1_symlink_dev_pts_only() {
        let tmp = std::env::temp_dir().join(format!("fleet-pts-{}", std::process::id()));
        let sessions = tmp.join("sessions");
        let proc_root = tmp.join("proc");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::create_dir_all(&proc_root).unwrap();

        std::fs::write(sessions.join("1.json"), session_json(1, "100", "busy")).unwrap();
        fake_proc(&proc_root, 1, "100", None);
        let fd1 = proc_root.join("1").join("fd");
        std::fs::create_dir_all(&fd1).unwrap();
        std::os::unix::fs::symlink("/dev/pts/7", fd1.join("1")).unwrap();

        std::fs::write(sessions.join("2.json"), session_json(2, "200", "busy")).unwrap();
        fake_proc(&proc_root, 2, "200", None);
        let fd2 = proc_root.join("2").join("fd");
        std::fs::create_dir_all(&fd2).unwrap();
        std::os::unix::fs::symlink("/dev/null", fd2.join("1")).unwrap();

        let proc = crate::platform::LinuxProc::new(proc_root);
        let (live, _stats, _platform) = scan(&sessions, &proc);
        std::fs::remove_dir_all(&tmp).ok();

        let pts_of = |pid: u32| live.iter().find(|s| s.file.pid == pid).unwrap().pts.clone();
        assert_eq!(
            pts_of(1),
            Some("/dev/pts/7".to_string()),
            "fd/1 -> a real pts is kept"
        );
        assert_eq!(pts_of(2), None, "fd/1 -> /dev/null is filtered out");
    }
}
