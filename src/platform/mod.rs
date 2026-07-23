//! platform ctx: OS-facts seam — everything fleetops knows about a live process, per target.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/platform/mod.rs
//! Deps:    none (types only); impls live in `linux.rs` / `macos.rs`
//! Tested:  type-level tests inline; impls carry their own (linux: tempdir fake roots — plain
//!          `std::fs`, so those tests run on every target; macos: live self-probes)
//!
//! Key responsibilities:
//! - `ProcFacts`: the one trait the scans consume — pid enumeration, per-pid `ProcSnapshot`,
//!   procStart liveness verdicts, and the platform pty prefix.
//! - `StartId`: the LOSSLESS start identity (Linux raw tick string, macOS tvsec+tvusec) —
//!   compared exactly, never converted; epoch seconds are for Codex rollout-age joins only.
//! - `PlatformStats`: fact-acquisition tallies for the doctor/footer (drift must be loud).
//!
//! Design constraints:
//! - Read-only over the fleet. No domain types here — `discovery::parse_environ` stays above.
//! - Snapshot acquisition is race-guarded: providers read the start identity, gather facts,
//!   re-read, and report `Raced` on any change (PID reuse within a sweep must never mix facts).

/// Lossless, platform-native process start identity — the liveness/PID-reuse token.
///
/// Never converted for comparison: two processes can share an epoch second, and ticks→secs
/// integer division loses sub-second identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartId {
    /// Raw `/proc/<pid>/stat` field-22 tick string (exactly what Claude writes as `procStart`).
    /// (Constructed by the Linux provider; on macOS production builds only tests build it.)
    #[cfg_attr(all(target_os = "macos", not(test)), allow(dead_code))]
    Linux(String),
    /// `pbi_start_tvsec` + `pbi_start_tvusec` from BSDInfo.
    /// (Constructed by the macOS provider; dead on pure-Linux builds.)
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Mac {
        /// Kernel start time, whole epoch seconds.
        tvsec: u64,
        /// Sub-second component — part of the identity for the race re-read.
        tvusec: u64,
    },
}

/// Typed fact-acquisition outcome — every fact can fail differently, and the doctor must see
/// how. `Ok(None)`-style "present but not applicable" is modeled inside `T` where needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcqResult<T> {
    /// Fact acquired.
    Ok(T),
    /// Kernel/API refused (EPERM etc.).
    Denied,
    /// Returned but empty where content was expected — e.g. env region omitted for a
    /// `cs_restricted` target (XNU omits it silently, argv intact; never an EPERM).
    Unavailable,
    /// Returned but undecodable (malformed procargs2 buffer, truncated region, …).
    Malformed,
}

impl<T> AcqResult<T> {
    /// The acquired value, if any.
    pub fn ok(self) -> Option<T> {
        match self {
            Self::Ok(v) => Some(v),
            _ => None,
        }
    }

    /// Borrowing accessor.
    pub const fn as_ref(&self) -> AcqResult<&T> {
        match self {
            Self::Ok(v) => AcqResult::Ok(v),
            Self::Denied => AcqResult::Denied,
            Self::Unavailable => AcqResult::Unavailable,
            Self::Malformed => AcqResult::Malformed,
        }
    }
}

/// Everything the scans need to know about one live process, gathered race-guarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcSnapshot {
    /// Lossless start identity — liveness token, compared exactly.
    pub start_id: StartId,
    /// Derived epoch seconds — Codex rollout-age joining ONLY, never liveness.
    pub start_epoch_secs: u64,
    /// Short process name (Linux `comm`, macOS `pbi_comm`), trailing whitespace trimmed.
    pub comm: Option<String>,
    /// Exact argv (argc-bounded on macOS; NUL-split cmdline on Linux).
    pub argv: AcqResult<Vec<Vec<u8>>>,
    /// Process working directory.
    pub cwd: Option<String>,
    /// Raw NUL-separated environment bytes — fed to `discovery::parse_environ` unchanged.
    pub env_block: AcqResult<Vec<u8>>,
    /// Resolved target of fd 1 when it is a pty under the platform prefix. `Ok(None)` = fd 1
    /// exists but is not such a pty (redirected/pipe); `Denied` = the kernel refused while the
    /// process was live (doctor: `fd1_denied`).
    pub fd1_pty: AcqResult<Option<String>>,
}

/// Snapshot acquisition outcome.
#[derive(Debug)]
pub enum SnapshotOutcome {
    /// Process not running (or vanished before the first identity read).
    Gone,
    /// Start identity changed between the first and second read — PID reused mid-gather;
    /// facts discarded.
    Raced,
    /// Race-guarded snapshot.
    Present(Box<ProcSnapshot>),
}

/// Verdict of comparing a session file's `procStart` string against a snapshot's `StartId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// Identities match — the file describes THIS process.
    Match,
    /// Parsed fine, kernel disagrees. `near` = within 2s (drift signal, still NOT live).
    Mismatch {
        /// Delta ≤ 2s — semantic-drift tripwire for the doctor.
        near: bool,
    },
    /// The file's `procStart` didn't parse for this platform (format drift). Only the macOS
    /// provider constructs it (Linux procStart is an opaque token — no parse step).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    DateParseFailed,
}

/// Platform fact-acquisition tallies, accumulated across one sweep (doctor + footer + snapshot).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PlatformStats {
    /// `procStart` string malformed for this platform (format drift).
    pub date_parse_failed: usize,
    /// Parsed fine, kernel disagrees (semantic/TZ drift) — includes the near subset.
    pub start_mismatch: usize,
    /// `0 < |delta| ≤ 2s` subset of `start_mismatch`.
    pub start_mismatch_near: usize,
    /// Env acquisition refused by the kernel/API.
    pub env_denied: usize,
    /// Env region returned-but-empty (possibly `cs_restricted`).
    pub env_unavailable: usize,
    /// Env/argv buffer undecodable.
    pub env_malformed: usize,
    /// fd-1 info refused while the process was live.
    pub fd1_denied: usize,
    /// Start identity changed between reads — snapshot discarded.
    pub identity_raced: usize,
}

impl PlatformStats {
    /// Fold another sweep-fragment's tallies in (Claude + Codex scans sum into one report).
    pub const fn absorb(&mut self, other: &Self) {
        self.date_parse_failed += other.date_parse_failed;
        self.start_mismatch += other.start_mismatch;
        self.start_mismatch_near += other.start_mismatch_near;
        self.env_denied += other.env_denied;
        self.env_unavailable += other.env_unavailable;
        self.env_malformed += other.env_malformed;
        self.fd1_denied += other.fd1_denied;
        self.identity_raced += other.identity_raced;
    }

    /// Tally one env-block outcome.
    pub const fn count_env<T>(&mut self, env: &AcqResult<T>) {
        match env {
            AcqResult::Ok(_) => {}
            AcqResult::Denied => self.env_denied += 1,
            AcqResult::Unavailable => self.env_unavailable += 1,
            AcqResult::Malformed => self.env_malformed += 1,
        }
    }

    /// Tally one liveness verdict (only the non-Match outcomes count).
    pub const fn count_liveness(&mut self, verdict: Liveness) {
        match verdict {
            Liveness::Match => {}
            Liveness::Mismatch { near } => {
                self.start_mismatch += 1;
                if near {
                    self.start_mismatch_near += 1;
                }
            }
            Liveness::DateParseFailed => self.date_parse_failed += 1,
        }
    }
}

/// wezterm socket-discovery tallies (doctor + footer; macOS lane primarily — the Linux/WSL2
/// path fills what it can observe).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PaneDiscoveryStats {
    /// `gui-sock-*` files seen across candidate dirs.
    pub sockets_found: usize,
    /// Socket files with no matching live `wezterm-gui` pid — connecting to these can HANG;
    /// skipped (the load-bearing guard, both platforms).
    pub sockets_stale: usize,
    /// Socket files owned by another user — out of scope, skipped.
    pub sockets_foreign_uid: usize,
    /// Live instances whose `cli list` call errored (the merge keeps going; footer degrades).
    pub instances_failed: usize,
}

/// The OS-facts seam the scans consume. Object-safe; sweeps take `&dyn ProcFacts`.
pub trait ProcFacts {
    /// All live pids (Codex enumeration; Claude discovery goes file→pid instead).
    fn pids(&self) -> Vec<u32>;
    /// Short process name only — the cheap pre-gate, so enumeration sweeps don't pay a full
    /// snapshot for every process on the system.
    fn comm(&self, pid: u32) -> Option<String>;
    /// Race-guarded snapshot of one process.
    fn snapshot(&self, pid: u32) -> SnapshotOutcome;
    /// Compare a session file's `procStart` string against a snapshot's start identity.
    fn liveness(&self, snapshot: &StartId, file_proc_start: &str) -> Liveness;
    /// The platform pty prefix (`/dev/pts/` vs `/dev/ttys`) — fd1/highlight target guard.
    fn pty_prefix(&self) -> &'static str;
}

mod linux;
mod procargs;
#[cfg(any(test, not(target_os = "macos")))]
pub use linux::LinuxProc;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::MacProc;

/// The production provider for this build target, constructed once per sweep.
#[cfg(target_os = "macos")]
pub const fn provider() -> MacProc {
    MacProc::new()
}

/// The production provider for this build target, constructed once per sweep.
#[cfg(not(target_os = "macos"))]
pub fn provider() -> LinuxProc {
    LinuxProc::new(std::path::PathBuf::from("/proc"))
}
