# Plan: macOS support for fleetops (rev 3 — post adversarial review rounds 1+2)

## Goal

Make `fleet` fully functional on macOS: session discovery, liveness, account/pane attribution,
Codex lane, wezterm jump, and pane highlight — with Linux/WSL2 behavior unchanged.

## Verified facts (probed live on a macOS 15 machine, Claude Code 2.1.215)

1. Claude Code on macOS **does** write `~/.claude/sessions/<pid>.json`, same schema as Linux.
2. `procStart` on macOS is a ctime-style string, e.g. `"Mon Jul 20 15:58:39 2026"` — **UTC** in
   the one probed version. `ps -p <pid> -o lstart=` prints the same instant in **local time** —
   naive string compare fails; comparison must go through epoch seconds.
3. Claude session stdout is a pty at `/dev/ttysNNN` (verified via `lsof -d 1`), not `/dev/pts/N`.
4. wezterm on macOS is a native binary — no `.exe`, no WSL interop, no `/mnt/c` socket scan.

**Probe results (2026-07-23, gates GREEN):**

- (P1) `procStart` derivation: probed 13 live sessions (launches spread over 4 days) —
  file epoch vs `ps lstart` epoch delta = **0 for all 13**. `procStart` is the kernel start
  second, rendered as UTC ctime. Exact-equality liveness confirmed viable.
- (P2) `KERN_PROCARGS2` on live claude (node) processes: env fully readable (96 vars), no
  cs_restricted omission. Codex processes also readable. The `env_permission_denied`
  diagnostic stays (other machines / hardened runtimes may differ).
- Side finding: dev machine's terminal is ghostty, not wezterm — the pane lane will be
  legitimately empty here; wezterm-lane tests rely on fixtures + a wezterm install for live
  verify. (A ghostty lane is a non-goal.)

## Design

### New seam: `src/platform/` (cfg-gated OS-facts layer)

All `/proc` reads move behind a platform module. Parsers stay pure over bytes and stay in the
domain modules; the platform layer returns **raw facts only** — no domain types.

```
src/platform/mod.rs      — contract + cfg re-export
src/platform/linux.rs    — current /proc code moved verbatim (path-root injectable, as today)
src/platform/macos.rs    — libproc + procargs2 impl
fleetops-procargs/       — workspace sub-crate: the ONE unsafe sysctl(KERN_PROCARGS2) call,
                           exposing safe `fn procargs2(pid: u32) -> io::Result<Vec<u8>>`.
                           Main crate keeps `unsafe_code = "forbid"` untouched.
```

The contract is a **process snapshot**, gathered together and revalidated (see §Atomicity):

```rust
pub struct ProcSnapshot {
    /// LOSSLESS platform-native start identity — the liveness/PID-reuse token, compared for
    /// exact equality and NEVER converted. Linux: the raw stat-field-22 tick string (what
    /// Claude writes into procStart there). macOS: "(tvsec,tvusec)" from pbi_start_tvsec/_tvusec.
    /// Epoch seconds are NOT an identity: two processes can share a start second, and
    /// ticks→secs integer division loses sub-second identity.
    pub start_id: StartId,
    /// Derived epoch seconds — Codex rollout-age joining ONLY, never liveness.
    pub start_epoch_secs: Option<u64>,
    pub comm: Option<String>,          // short name, Linux-comm-equivalent (pbi_comm, 16-byte)
    pub argv: AcqResult<Vec<Vec<u8>>>, // exact argv (argc-bounded), for the Codex argv0-only gate
    pub cwd: Option<PathBuf>,
    pub env_block: AcqResult<Vec<u8>>, // raw NUL-separated env-region bytes
    pub fd1_target: AcqResult<Option<String>>, // pty path of fd 1; Ok(None) = fd1 not a pty
}

/// Typed acquisition outcome — every fact can fail differently, and the doctor must see how.
pub enum AcqResult<T> {
    Ok(T),
    /// Kernel/API refused (EPERM etc.).
    Denied,
    /// Returned but undecodable (malformed procargs2 buffer, truncated env region, …).
    Malformed,
    /// Fact source vanished mid-read (process exit race) — snapshot gets discarded anyway.
    Gone,
}
```

macOS `procStart` liveness: parse the ctime string → epoch secs → require exact match with
`tvsec` AND treat `tvusec` as part of `start_id` for the atomicity re-read (the file string
has no sub-second part, so file↔kernel comparison is per-`tvsec`; the double-read
identity check uses the full `(tvsec,tvusec)` so a same-second PID swap is still caught).

**cs_restricted correction:** XNU silently omits the env region for restricted targets while
still returning argv — no EPERM. Detection: argv decoded fine but env region empty/absent →
`env_block = AcqResult::Malformed`-class diagnostic reported as "environment unavailable
(possibly restricted)", never confidently `Denied`. `Denied` is reserved for actual sysctl
errors. Probe P2 showed live claude/codex are NOT restricted on the dev machine.

Domain code keeps calling the existing pure `discovery::parse_environ` over the raw bytes; on
macOS the decoder feeds it ONLY the env region (argc-delimited), never argv bytes — an argv
string `WEZTERM_PANE=…` must not be misread as environment.

Per-OS acquisition:

| Fact | Linux | macOS |
|---|---|---|
| `start_id` (lossless) | raw stat field 22 tick string | `(pbi_start_tvsec, pbi_start_tvusec)` |
| `start_epoch_secs` | boot time + stat field 22 ticks (existing codex.rs math, moved here) | `pbi_start_tvsec` |
| `comm` | `/proc/<pid>/comm` | `pbi_comm` |
| `argv` | `/proc/<pid>/cmdline` NUL-split | `fleetops-procargs` → argc-bounded decoder |
| `env_block` | `/proc/<pid>/environ` | same procargs2 buffer, region after argv |
| `cwd` | readlink `/proc/<pid>/cwd` | `libproc` `pidfdinfo`/`pidinfo` vnode (PROC_PIDVNODEPATHINFO) |
| `fd1_target` | readlink `/proc/<pid>/fd/1`, keep `/dev/pts/*` | `libproc` fd list → tty vnode path, keep `/dev/ttys*` |
| enumerate pids | walk `/proc` | `libproc::processes::pids_by_type(All)` |

The pty prefix (`/dev/pts/` vs `/dev/ttys`) is a platform constant shared by discovery, codex
gate, and highlight.

**Call sites that stop hardcoding `/proc`** (the shared sensor pipeline, both TUI and
snapshot/doctor paths): `src/collect.rs:50` (`discovery::scan`) and `src/collect.rs:63`
(`codex::scan`). Both take the platform provider instead of `Path::new("/proc")`.

### procargs2 decoder (pure, in main crate)

Input: raw KERN_PROCARGS2 buffer (allocated at `KERN_ARGMAX`). Layout: `argc: i32`, exec path,
NUL padding, argc NUL-terminated argv strings, then env strings. Decoder returns
`(argv: Vec<Vec<u8>>, env_region: &[u8])`. Fixture-tested: normal case, exec-path padding runs,
empty argv entries, argc=0, buffer truncation, malformed argc, env absent, argv string that
looks like an env assignment (must stay argv).

### procStart comparison (macOS)

- Parse `"%a %b %e %H:%M:%S %Y"` with chrono as **UTC** → epoch seconds.
- Liveness: **exact equality** with `start_epoch_secs`. No tolerance — ±1s would accept a
  recycled PID started in an adjacent second and silently weaken the PID-reuse invariant
  (`discovery.rs:217-223`). Gate: probe P1 must confirm exact match first.
- **Near-miss drift counter**: `0 < |delta| ≤ 2s` increments a doctor-visible
  `start_mismatch_near` stat — catches both a future Claude change to "observed time" and a
  future TZ semantic change (UTC→local parses fine but shifts hours; that lands in plain
  `start_mismatch`, also counted). A fleet where every session start-mismatches is a loud
  doctor signal, never a silent empty board.
- Malformed `procStart` string → its own `date_parse_failed` counter (NOT `parse_failed`,
  which stays "session JSON unparseable"; NOT `stale_dead`, which stays "dead/reused PID").

### Atomicity (PID-reuse race between reads)

Snapshot acquisition re-verifies identity: read `start_id`, gather remaining facts, read
`start_id` **again** — if the LOSSLESS identity changed or the process vanished, the snapshot
is discarded (session counted stale). Comparing epoch seconds here would let a same-second PID
swap through; the re-read compares raw ticks (Linux) / `(tvsec,tvusec)` (macOS). Same
discipline on both OSes (fixes a latent Linux race: `discovery.rs:194-205` reads stat, then
environ, then fd/1 unguarded).

### Codex lane (codex.rs)

- `is_codex_tui` inputs come from the snapshot: `comm == "codex"` (pbi_comm on macOS), argv
  with exactly one element (argc-bounded — `codex exec`, `codex --version`, wrappers rejected),
  `fd1_target` under the platform pty prefix.
- Rollout-age gate keeps working: `start_epoch_secs` is first-class in the contract (the
  existing boot-time+ticks math becomes the Linux impl of it).

### wezterm lane (panes.rs)

- Program resolution: cfg — Linux/WSL2 unchanged (`wezterm.exe`, `/mnt/c/...` fallback); macOS
  `"wezterm"` with `/Applications/WezTerm.app/Contents/MacOS/wezterm` absolute fallback.
- Socket discovery (macOS): enumerate `gui-sock-*` in `~/.local/share/wezterm` (macOS default
  runtime dir), **plus** the directory containing `$WEZTERM_UNIX_SOCKET` when set. The env var
  **seeds** candidates — it never short-circuits enumeration (all-instances guarantee,
  `panes.rs:8-17`).
- **Stale-socket guard ported**: candidate `gui-sock-<pid>` files are intersected with live
  `wezterm-gui` PIDs (via the platform pid enumeration — replaces Windows `tasklist`), because
  connecting to a stale socket can hang (`panes.rs:331-345`). Existing per-call timeouts kept.
- **Scope**: only the invoking UID's instances. Sockets not owned by the current UID are
  skipped (and counted for doctor).
- **Documented limitation**: mux domains / GUI instances with a configured custom
  `socket_path` outside the default runtime dir are not discovered (same class of limitation
  as today's Windows-side scan). Stated in README.
- No wezterm installed → existing visible lane-error degradation, board still functions.

### highlight.rs

- OSC 11/111 write mechanism is portable, BUT the writer hardcodes Linux numeric
  `O_NOCTTY|O_NONBLOCK` in `custom_flags` (`highlight.rs:109-128`) — Darwin values differ
  (`O_NOCTTY=0x20000`, `O_NONBLOCK=0x4`). Fix: `libc::O_NOCTTY | libc::O_NONBLOCK` (libc
  consts are safe; `libc` becomes a direct dep — it's already in the tree via tokio).
  This is a correctness fix for Linux readability too (named consts over magic numbers).
- Accepted pty prefix comes from the platform constant.

### doctor — diagnostic transport (concrete aggregation path)

New stats structs with a defined route to the report; `ScanStats` keeps its current meaning
(Claude-discovery counters) and is NOT overloaded:

```rust
/// Platform fact-acquisition tallies, accumulated by the provider across one sweep.
pub struct PlatformStats {
    pub date_parse_failed: usize,     // procStart string malformed (format drift)
    pub start_mismatch: usize,        // parsed fine, kernel disagrees (semantic/TZ drift)
    pub start_mismatch_near: usize,   // 0 < |delta| <= 2s subset of the above
    pub env_denied: usize,            // sysctl/API refused
    pub env_unavailable: usize,       // returned-but-empty (possibly cs_restricted)
    pub env_malformed: usize,         // undecodable buffer / truncation
    pub fd1_denied: usize,
    pub identity_raced: usize,        // double-read start_id mismatch → snapshot discarded
}

/// wezterm socket-discovery tallies (macOS lane).
pub struct PaneDiscoveryStats {
    pub sockets_found: usize,
    pub sockets_stale: usize,         // no matching live wezterm-gui pid
    pub sockets_foreign_uid: usize,
    pub instances_failed: usize,      // live socket, cli call errored (existing merge keeps going)
}
```

Route: `discovery::scan`/`codex::scan` return `PlatformStats` alongside their existing results;
pane discovery returns `PaneDiscoveryStats` next to its `Vec<String>`; `Collected`
(`collect.rs:29-39`) grows both fields next to `ScanStats` + `lane_error`; doctor and the board
footer read them from `Collected` — one path, both consumers, `snapshot` JSON includes them.

Per-platform checks, each with a distinct signal:

- sessions dir readable; session JSON `parse_failed` (unchanged meaning); `stale_dead`
  (unchanged meaning)
- everything in `PlatformStats` / `PaneDiscoveryStats` above
- wezterm binary resolution; libproc reachability (macOS), `/proc` reachability (Linux)

### Dependencies

```toml
[target.'cfg(target_os = "macos")'.dependencies]
libproc = "0.14"                    # pids, BSDInfo (start time), fd/vnode paths — safe API
chrono = { version = "0.4", default-features = false, features = ["std"] }  # ctime parse
fleetops-procargs = { path = "fleetops-procargs" }  # the one unsafe sysctl, isolated

[dependencies]
libc = "0.2"                        # O_NOCTTY/O_NONBLOCK consts (safe), both platforms
```

Main crate `unsafe_code = "forbid"` unchanged; the sub-crate carries
`#![deny(unsafe_op_in_unsafe_fn)]` and is ~60 lines + tests.

### Testing

- Existing tempdir fake-`/proc` tests: the Linux fs-reading impl compiles on all targets under
  `#[cfg(test)]` (it is plain `std::fs` over an injectable root — portable in tests by
  construction). macOS impl additionally gets a trait-level fake for snapshot-shape tests.
- procargs2 decoder: byte fixtures per §decoder (incl. argv/env boundary and truncation).
- procStart: ctime parse fixtures (UTC trap: file-UTC vs local-expectation), exact-equality
  boundary (delta=1 → stale + near-miss counter), garbage → `date_parse_failed`.
- Codex classifier: macOS-shaped inputs (pbi_comm, argc-bounded argv) incl. rejection of
  `codex exec` / wrapper processes.
- wezterm: multi-instance (env socket + sibling instances), stale socket skipped, foreign-uid
  socket skipped, permission-denied socket dir, one-failing-instance-among-healthy (existing
  merge semantics, `panes.rs:331-390`).
- Atomicity: fake provider mutates `start_id` between reads → snapshot discarded; INCLUDING
  the same-epoch-second case (two distinct raw identities — different ticks / different
  `tvusec` — mapping to one epoch second must still be caught).
- Linux regression: liveness still compares the raw tick string byte-for-byte (existing
  `discovery.rs` fixture tests keep passing unchanged).
- Live verify on this Mac: `fleet doctor`, `fleet snapshot` against real sessions; probes P1/P2
  recorded in the PR description.

## Non-goals

- Native Windows support.
- Terminal multiplexers other than wezterm (tmux/iTerm2 lanes).
- wezterm custom `socket_path` mux-domain discovery (documented limitation).
- Any write into Claude/Codex dirs (invariant preserved).

## Risks

1. **procStart semantic drift** (format or UTC→local or observed-vs-kernel time): mitigated by
   probe P1 pre-implementation + `date_parse_failed`/`start_mismatch(_near)` doctor counters at
   runtime. An all-sessions-mismatch fleet is diagnosable in one `fleet doctor` run.
2. **KERN_PROCARGS2 env omission** (cs_restricted) loses `CLAUDE_ACCOUNT` **and**
   `WEZTERM_PANE` — degrades account attribution AND exact pane identity/highlight for the
   affected session (falls back to title/cwd pane matching, as designed for pre-forwarding
   sessions). Probe P2 + `env_permission_denied` diagnostic make it visible, never silent.
3. **KERN_PROCARGS2 truncation** — buffer at `KERN_ARGMAX`; decoder treats truncated env region
   as `Absent`-with-diagnostic, never mis-parses.
4. **libproc fd→vnode awkwardness** — primary path is in-process libproc (synchronous, fits
   the existing `spawn_blocking` discovery flow). If its fd-info wrapper proves unusable:
   `fd1_target` degrades to `Denied` with the `fd1_denied` diagnostic (board loses highlight
   targets, nothing else) — shipped that way rather than blocked. The batched-exec contingency
   (`lsof -a -d1 -p <pid,…> -F n`, one exec per sweep) has a defined control-flow placement if
   ever needed: it runs in the async prefetch phase where the pane lane already awaits its
   `Runner` (`panes.rs:331-359`), producing a `pid → fd1` map passed INTO `collect` alongside
   the pane result (`collect.rs:42-50`) — the synchronous scan then consumes the map instead of
   calling libproc for fd1. Not built speculatively.
