//! platform/linux ctx: the `/proc` implementation of `ProcFacts` — current behavior, moved.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/platform/linux.rs
//! Deps:    std::fs only (plain reads over an injectable root)
//! Tested:  inline `#[cfg(test)]` against tempdir fake roots — plain `std::fs`, so these tests
//!          compile and run on EVERY target (macOS CI still exercises the Linux provider).
//!
//! Key responsibilities:
//! - `StartId::Linux(raw tick string)` — byte-for-byte what Claude writes as `procStart`
//!   (liveness stays exact string equality, the pre-seam invariant).
//! - Race guard: stat → facts → stat again; tick-string change ⇒ `Raced`.
//! - `start_epoch_secs` = `btime + ticks/HZ` (the existing codex.rs join math, moved here).
//!
//! Design constraints:
//! - Compiled on all targets (tests inject a fake root); production construction is cfg-gated
//!   in `platform::provider`.
//! - Read-only over the fleet.

// Production provider on Linux; on macOS it exists for the cross-target tests only.
#![cfg_attr(target_os = "macos", allow(dead_code))]

use std::path::{Path, PathBuf};

use super::{AcqResult, Liveness, ProcFacts, ProcSnapshot, SnapshotOutcome, StartId};

/// Kernel clock ticks/sec — hardcoded per recon (WSL2). A wrong value only loosens the Codex
/// join guard (degrading to newest-per-cwd), never rejects a live process.
const HZ: u64 = 100;

/// `/proc`-backed facts, root injectable for tests.
#[derive(Debug)]
pub struct LinuxProc {
    root: PathBuf,
    /// Boot time (epoch secs) from `<root>/stat` `btime` — read once per sweep; 0 when
    /// unreadable (loosens the Codex join guard only).
    btime: u64,
}

impl LinuxProc {
    /// Provider over `root` (production: `/proc`; tests: a tempdir).
    pub fn new(root: PathBuf) -> Self {
        let btime = read_btime(&root);
        Self { root, btime }
    }

    fn pid_dir(&self, pid: u32) -> PathBuf {
        self.root.join(pid.to_string())
    }

    /// Raw stat starttime tick string, `None` when the process is gone/unreadable.
    fn start_ticks(&self, pid: u32) -> Option<String> {
        let stat = std::fs::read_to_string(self.pid_dir(pid).join("stat")).ok()?;
        starttime_from_stat(&stat).map(str::to_string)
    }
}

/// Extract starttime (field 22) from `/proc/<pid>/stat` content.
/// comm (field 2) may contain spaces and parens — fields are counted after the LAST `)`.
pub fn starttime_from_stat(stat: &str) -> Option<&str> {
    let after_comm = &stat[stat.rfind(')')? + 1..];
    // after_comm starts at field 3 (state); starttime is field 22 → index 19 here.
    after_comm.split_ascii_whitespace().nth(19)
}

/// `btime` (boot time, epoch secs) from `<root>/stat` — missing/unreadable degrades to 0.
fn read_btime(root: &Path) -> u64 {
    std::fs::read_to_string(root.join("stat"))
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find_map(|l| l.strip_prefix("btime "))
                .and_then(|v| v.trim().parse().ok())
        })
        .unwrap_or(0)
}

impl ProcFacts for LinuxProc {
    fn pids(&self) -> Vec<u32> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.parse().ok())
            .collect()
    }

    fn comm(&self, pid: u32) -> Option<String> {
        std::fs::read_to_string(self.pid_dir(pid).join("comm"))
            .ok()
            .map(|c| c.trim_end().to_string())
    }

    fn snapshot(&self, pid: u32) -> SnapshotOutcome {
        let Some(ticks_before) = self.start_ticks(pid) else {
            return SnapshotOutcome::Gone;
        };
        let dir = self.pid_dir(pid);
        let comm = self.comm(pid);
        let argv = std::fs::read(dir.join("cmdline")).map_or(AcqResult::Denied, |bytes| {
            AcqResult::Ok(
                bytes
                    .split(|&b| b == 0)
                    .filter(|a| !a.is_empty())
                    .map(<[u8]>::to_vec)
                    .collect(),
            )
        });
        let cwd = std::fs::read_link(dir.join("cwd"))
            .ok()
            .and_then(|t| t.to_str().map(str::to_string));
        let env_block = std::fs::read(dir.join("environ")).map_or(AcqResult::Denied, AcqResult::Ok);
        // fd/1 unreadable on Linux for a live same-user process ⇒ effectively "no pty"; a
        // permission split (`Denied`) only exists behind hardened /proc mounts — readlink
        // failure maps to Ok(None), the pre-seam behavior.
        let fd1_pty = AcqResult::Ok(
            std::fs::read_link(dir.join("fd").join("1"))
                .ok()
                .and_then(|t| t.to_str().map(str::to_string))
                .filter(|t| t.starts_with(self.pty_prefix())),
        );
        // Race re-read: the LOSSLESS identity must be unchanged, or the facts above may belong
        // to a reused PID.
        if self.start_ticks(pid).as_deref() != Some(ticks_before.as_str()) {
            return SnapshotOutcome::Raced;
        }
        let start_epoch_secs = ticks_before
            .parse::<u64>()
            .map_or(0, |t| self.btime + t / HZ);
        SnapshotOutcome::Present(Box::new(ProcSnapshot {
            start_id: StartId::Linux(ticks_before),
            start_epoch_secs,
            comm,
            argv,
            cwd,
            env_block,
            fd1_pty,
        }))
    }

    fn liveness(&self, snapshot: &StartId, file_proc_start: &str) -> Liveness {
        let StartId::Linux(ticks) = snapshot else {
            return Liveness::Mismatch { near: false };
        };
        if ticks == file_proc_start {
            return Liveness::Match;
        }
        // Near-miss drift signal: both parse as ticks and land within 2s (HZ ticks * 2).
        let near = ticks
            .parse::<i64>()
            .ok()
            .zip(file_proc_start.parse::<i64>().ok())
            .is_some_and(|(a, b)| (a - b).unsigned_abs() <= 2 * HZ);
        Liveness::Mismatch { near }
    }

    fn pty_prefix(&self) -> &'static str {
        "/dev/pts/"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("fleet-plat-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn fake_proc(root: &Path, pid: u32, starttime: &str) {
        let dir = root.join(pid.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("stat"),
            format!("{pid} (claude) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 {starttime} 23"),
        )
        .unwrap();
    }

    #[test]
    fn starttime_survives_parens_and_spaces_in_comm() {
        let stat = "42 (weird) name)) R 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 START 23";
        assert_eq!(starttime_from_stat(stat), Some("START"));
        assert_eq!(starttime_from_stat("no parens here"), None);
    }

    #[test]
    fn snapshot_gathers_facts_and_start_identity() {
        let root = fake_root("snap");
        fake_proc(&root, 7, "1234");
        let dir = root.join("7");
        std::fs::write(dir.join("comm"), "claude\n").unwrap();
        std::fs::write(dir.join("cmdline"), b"claude\0--flag\0").unwrap();
        std::fs::write(dir.join("environ"), b"CLAUDE_ACCOUNT=a\0").unwrap();
        std::fs::write(root.join("stat"), "btime 1000\n").unwrap();

        let p = LinuxProc::new(root.clone());
        let SnapshotOutcome::Present(snap) = p.snapshot(7) else {
            panic!("live process snapshots");
        };
        std::fs::remove_dir_all(&root).ok();

        assert_eq!(snap.start_id, StartId::Linux("1234".into()));
        assert_eq!(snap.start_epoch_secs, 1000 + 1234 / HZ);
        assert_eq!(snap.comm.as_deref(), Some("claude"));
        assert_eq!(
            snap.argv,
            AcqResult::Ok(vec![b"claude".to_vec(), b"--flag".to_vec()])
        );
        assert_eq!(
            snap.env_block,
            AcqResult::Ok(b"CLAUDE_ACCOUNT=a\0".to_vec())
        );
        assert_eq!(
            snap.fd1_pty,
            AcqResult::Ok(None),
            "no fd dir → no pty, still a snapshot"
        );
    }

    #[test]
    fn dead_pid_is_gone() {
        let root = fake_root("gone");
        let p = LinuxProc::new(root.clone());
        assert!(matches!(p.snapshot(99), SnapshotOutcome::Gone));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn liveness_is_exact_string_equality_with_near_tripwire() {
        let p = LinuxProc::new(PathBuf::from("/nonexistent"));
        let id = StartId::Linux("1000".into());
        assert_eq!(p.liveness(&id, "1000"), Liveness::Match);
        // 150 ticks = 1.5s at HZ=100 → near mismatch, NOT live.
        assert_eq!(p.liveness(&id, "1150"), Liveness::Mismatch { near: true });
        assert_eq!(p.liveness(&id, "9999"), Liveness::Mismatch { near: false });
        // Garbage that isn't equal parses as non-near mismatch, not a date failure (Linux
        // procStart is an opaque token; only equality matters).
        assert_eq!(p.liveness(&id, "junk"), Liveness::Mismatch { near: false });
    }

    #[test]
    fn pids_enumerates_numeric_entries_only() {
        let root = fake_root("pids");
        fake_proc(&root, 12, "1");
        std::fs::create_dir_all(root.join("not-a-pid")).unwrap();
        let p = LinuxProc::new(root.clone());
        assert_eq!(p.pids(), vec![12]);
        std::fs::remove_dir_all(&root).ok();
    }
}
