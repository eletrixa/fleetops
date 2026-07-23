//! platform/macos ctx: the libproc + procargs2 implementation of `ProcFacts`.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/platform/macos.rs
//! Deps:    libproc (BSDInfo start time, fd vnode paths, pid enumeration); chrono (ctime
//!          parse); fleetops-procargs (the isolated unsafe sysctl)
//! Tested:  inline `#[cfg(test)]` — self-probes against the test process (always live) plus
//!          pure ctime-parse cases; provider-shape tests use the trait fake in discovery/codex.
//!
//! Key responsibilities:
//! - `StartId::Mac { tvsec, tvusec }` — lossless identity from BSDInfo; race re-read compares
//!   BOTH fields (a same-second PID swap is caught by tvusec).
//! - Liveness: Claude writes `procStart` as a ctime string in **UTC** (probe P1: 13/13 live
//!   sessions delta 0 vs kernel) — parse as UTC, compare whole seconds against `tvsec` exactly.
//! - Env: procargs2 → argc-bounded decode; empty env region with argv intact is the
//!   `cs_restricted` shape → `AcqResult::Unavailable`, never `Denied` (xnu omits silently).
//!
//! Design constraints:
//! - Read-only over the fleet; no subprocess spawns — everything is in-process libproc/sysctl.

use chrono::NaiveDateTime;
use libproc::libproc::bsd_info::BSDInfo;
use libproc::libproc::proc_pid::{pidcwd, pidinfo};
use libproc::processes::{pids_by_type, ProcFilter};

use crate::platform::procargs;

use super::{AcqResult, Liveness, ProcFacts, ProcSnapshot, SnapshotOutcome, StartId};

/// libproc-backed facts.
#[derive(Debug, Default)]
pub struct MacProc;

impl MacProc {
    /// The production provider.
    pub const fn new() -> Self {
        Self
    }
}

/// The ctime layout Claude Code writes into `procStart` on macOS (UTC — probe P1).
const PROC_START_FMT: &str = "%a %b %e %H:%M:%S %Y";

/// Parse the macOS `procStart` ctime string as UTC epoch seconds.
fn parse_proc_start(s: &str) -> Option<i64> {
    NaiveDateTime::parse_from_str(s.trim(), PROC_START_FMT)
        .ok()
        .map(|dt| dt.and_utc().timestamp())
}

/// Lossless start identity from BSDInfo.
fn start_id(pid: u32) -> Option<StartId> {
    let info = pidinfo::<BSDInfo>(pid.try_into().ok()?, 0).ok()?;
    Some(StartId::Mac {
        tvsec: info.pbi_start_tvsec,
        tvusec: info.pbi_start_tvusec,
    })
}

/// `pbi_comm` as a trimmed string.
fn comm(pid: u32) -> Option<String> {
    let info = pidinfo::<BSDInfo>(pid.try_into().ok()?, 0).ok()?;
    let bytes: Vec<u8> = info
        .pbi_comm
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c.cast_unsigned())
        .collect();
    String::from_utf8(bytes)
        .ok()
        .map(|s| s.trim_end().to_string())
}

/// fd 1's resolved vnode path — `None` for sockets/pipes/closed fds or on refusal (a missing
/// pty degrades the highlight target only; refusal is tallied by the caller via `Denied`).
fn fd1_path(pid: u32) -> Result<Option<String>, ()> {
    fleetops_procargs::fd_path(pid, 1).map_err(|_| ())
}

impl ProcFacts for MacProc {
    fn pids(&self) -> Vec<u32> {
        pids_by_type(ProcFilter::All).unwrap_or_default()
    }

    fn comm(&self, pid: u32) -> Option<String> {
        comm(pid)
    }

    fn snapshot(&self, pid: u32) -> SnapshotOutcome {
        let Some(id_before) = start_id(pid) else {
            return SnapshotOutcome::Gone;
        };
        let comm = comm(pid);
        let cwd = pid
            .try_into()
            .ok()
            .and_then(|p| pidcwd(p).ok())
            .and_then(|p| p.to_str().map(str::to_string));
        let (argv, env_block) = match fleetops_procargs::procargs2(pid) {
            Ok(raw) => {
                procargs::decode(&raw).map_or((AcqResult::Malformed, AcqResult::Malformed), |p| {
                    let env = if p.env_region.iter().all(|&b| b == 0) {
                        // argv intact, env region empty/padding-only → cs_restricted shape.
                        AcqResult::Unavailable
                    } else if p.env_region.last() != Some(&0) {
                        // Unterminated tail = kernel truncation mid-string — a partial var
                        // must never reach parse_environ as if complete.
                        AcqResult::Malformed
                    } else {
                        AcqResult::Ok(p.env_region)
                    };
                    (AcqResult::Ok(p.argv), env)
                })
            }
            Err(e) if e.raw_os_error() == Some(libc::EPERM) => {
                (AcqResult::Denied, AcqResult::Denied)
            }
            Err(_) => (AcqResult::Denied, AcqResult::Denied),
        };
        let fd1_pty = match fd1_path(pid) {
            Ok(target) => AcqResult::Ok(target.filter(|t| t.starts_with(self.pty_prefix()))),
            Err(()) => AcqResult::Denied,
        };
        // Race re-read: BOTH tvsec and tvusec must be unchanged. A vanished process is `Gone`
        // (normal exit mid-sweep), NOT `Raced`.
        match start_id(pid) {
            None => return SnapshotOutcome::Gone,
            Some(ref id_after) if id_after != &id_before => return SnapshotOutcome::Raced,
            Some(_) => {}
        }
        let StartId::Mac { tvsec, .. } = id_before else {
            unreachable!("mac provider builds mac ids");
        };
        SnapshotOutcome::Present(Box::new(ProcSnapshot {
            start_id: id_before,
            start_epoch_secs: tvsec,
            comm,
            argv,
            cwd,
            env_block,
            fd1_pty,
        }))
    }

    fn liveness(&self, snapshot: &StartId, file_proc_start: &str) -> Liveness {
        let StartId::Mac { tvsec, .. } = snapshot else {
            return Liveness::Mismatch { near: false };
        };
        let Some(file_secs) = parse_proc_start(file_proc_start) else {
            return Liveness::DateParseFailed;
        };
        let Ok(kernel_secs) = i64::try_from(*tvsec) else {
            return Liveness::Mismatch { near: false };
        };
        if file_secs == kernel_secs {
            Liveness::Match
        } else {
            Liveness::Mismatch {
                near: (file_secs - kernel_secs).unsigned_abs() <= 2,
            }
        }
    }

    fn pty_prefix(&self) -> &'static str {
        "/dev/ttys"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_start_ctime_parses_as_utc() {
        // The probe-P1 fixture: file says 15:58:39 (UTC), local wall clock said 17:58:39 CEST.
        let secs = parse_proc_start("Mon Jul 20 15:58:39 2026").expect("parses");
        assert_eq!(secs, 1_784_563_119, "UTC interpretation, not local");
        // %e handles the space-padded single-digit day ctime produces.
        assert!(parse_proc_start("Wed Jul  1 01:02:03 2026").is_some());
        assert!(parse_proc_start("not a date").is_none());
    }

    #[test]
    fn own_process_snapshots_live() {
        let p = MacProc::new();
        let SnapshotOutcome::Present(snap) = p.snapshot(std::process::id()) else {
            panic!("own pid is always live");
        };
        assert!(matches!(snap.start_id, StartId::Mac { .. }));
        assert!(snap.start_epoch_secs > 1_500_000_000, "sane epoch");
        assert!(matches!(snap.argv, AcqResult::Ok(_)), "own argv readable");
    }

    #[test]
    fn liveness_matches_own_start_second() {
        let p = MacProc::new();
        let SnapshotOutcome::Present(snap) = p.snapshot(std::process::id()) else {
            panic!("own pid live");
        };
        let StartId::Mac { tvsec, .. } = snap.start_id else {
            panic!("mac id")
        };
        // Render tvsec the way Claude Code does (UTC ctime) and round-trip the comparison.
        let rendered = chrono::DateTime::from_timestamp(i64::try_from(tvsec).unwrap(), 0)
            .unwrap()
            .format(PROC_START_FMT)
            .to_string();
        assert_eq!(p.liveness(&snap.start_id, &rendered), Liveness::Match);
        assert_eq!(
            p.liveness(&snap.start_id, "Mon Jul 20 00:00:00 2020"),
            Liveness::Mismatch { near: false }
        );
        assert_eq!(
            p.liveness(&snap.start_id, "garbage"),
            Liveness::DateParseFailed
        );
    }
}
