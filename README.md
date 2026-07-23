# fleetops

**An ops board for every Claude Code session running on your machine.**

Each Claude Code session is a ship; fleetops is the bridge's ops board — one terminal view of the
whole fleet: which sessions are working, which are idle, and which are blocked waiting on you, plus
tokens spent, context-window fill, and the wezterm pane each one lives in (jump to any of them with a
keypress).

<!-- TODO: demo GIF here — record with docs/demo/board.tape (`vhs docs/demo/board.tape`) -->

## Features

- **Session discovery** — finds every live Claude Code session from `~/.claude/sessions/<pid>.json`,
  confirming liveness against `/proc` (PID-reuse-safe: the file's recorded start time must match
  `/proc/<pid>/stat`). Stale files for dead PIDs are counted, never shown as live.
- **At-a-glance status** — a pure fold over each session's native status, pending-question flag, and
  transcript activity yields one of: **working**, **idle**, **needs answer** (a pending
  `AskUserQuestion`), **waiting** (blocked on input the transcript can't show, e.g. a permission
  prompt), **stalled?** (busy but the transcript stopped growing), **shell**, or **unknown** (a
  native status this build doesn't recognize — a drift signal, never hidden).
- **Tokens & context %** — reads the transcript tail for the last assistant `usage` line and renders
  a context-window gauge (out of 200k, or 1M once a session exceeds 200k) plus a compact token
  count. Approximate, never a bill.
- **wezterm pane mapping & jump** — matches each session to its wezterm pane (exact
  `WEZTERM_PANE` identity when forwarded, else title/cwd) and jumps to it on **Enter**
  (`activate-tab` then `activate-pane`). Discovers every live wezterm instance so a board running on
  one monitor can still drive panes in another.
- **Codex lane** — Codex CLI sessions (which keep no per-pid session file) are discovered from
  `/proc` + their rollout transcript and folded onto the same board.
- **Read-only over the fleet** — the only actions that change anything are focusing a pane (the
  jump) and an optional brief highlight of the jumped-to pane (disable with `--no-highlight`).
  fleetops never writes into any Claude config or session directory.
- **`doctor` and `snapshot` subcommands** — `fleet doctor` prints a read-only drift report (are the
  undocumented sources still parseable?); `fleet snapshot` emits one JSON object of exactly what the
  board would render, for dashboards and scripts.

## Platform: WSL2 only

fleetops targets **WSL2 (Linux)** and does nothing useful anywhere else:

- Session discovery reads `/proc` — absent on macOS and native Windows.
- Pane control shells out to the Windows `wezterm.exe` across the WSL interop boundary. The
  Windows-side wezterm socket directory is auto-detected (from `$WEZTERM_UNIX_SOCKET`, or by
  scanning `/mnt/c/Users/*/.local/share/wezterm`) — no per-machine username configuration.

On macOS or native Windows the board simply comes up empty; `fleet doctor` prints a
`/proc not found — fleetops targets WSL2/Linux` hint.

## Install

```bash
cargo build --release
# binary at target/release/fleet
```

Requires a recent stable Rust toolchain (see `rust-toolchain.toml`) and, for the pane lane, wezterm
installed on the Windows side (`wezterm.exe` reachable from WSL).

**Using Claude Code?** Run `claude` in the repo and type `/setup` — a checked-in command
(`.claude/commands/setup.md`) walks Claude through checking your platform, building, installing
onto PATH, detecting which lanes your machine supports, and interpreting `fleet doctor`.

## Quick start

```bash
fleet              # launch the board
fleet --no-highlight   # board, without the jump-target pane highlight
fleet doctor       # read-only diagnostics / drift report
fleet snapshot     # one-shot JSON of the current board, to stdout
```

Keys: **j/k** or **↑/↓** move the selection · **Enter** jumps to the selected session's wezterm pane
· **r** refreshes · **q**/**Esc** quits.

## What it reads & privacy

Everything fleetops reads is local, and nothing is ever transmitted off the machine. It reads no
credentials — not tokens, not API keys. Specifically, per live session it reads:

- **`~/.claude/sessions/<pid>.json`** — pid, session id, cwd, native status, name, version.
- **`/proc/<pid>/stat`** — process start time, for the liveness / PID-reuse check.
- **`/proc/<pid>/environ`** — only two variables are kept: `CLAUDE_ACCOUNT` (account label) and
  `WEZTERM_PANE` (exact pane identity). Everything else in the environment is ignored.
- **`/proc/<pid>/fd/1`** — the session's controlling pty, kept only when it resolves under
  `/dev/pts/` (the target for the optional pane highlight).
- **Transcript tail** (`~/.claude/projects/<slug>/<uuid>.jsonl`, last 256 KiB) — only **token
  counts**, the **ai-title**, and a **pending-question flag** are extracted. Message text is never
  read into state, logged, or stored.
- **wezterm pane list** — `wezterm.exe cli list --format json`, for pane titles/cwd and jump targets.

No data leaves your machine; fleetops makes no network requests.

## Unofficial

fleetops is an independent, unofficial tool. It is **not affiliated with, endorsed by, or supported
by Anthropic**. "Claude" is a trademark of Anthropic. It relies on undocumented, internal file
formats that can change at any time — `fleet doctor` exists to surface exactly that kind of drift.

## Maintenance

Passively maintained. Issues and PRs are welcome, but responses may be slow and features are added
only as they earn their keep.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the
work by you, as defined in the Apache-2.0 license, shall be dual-licensed as above, without any
additional terms or conditions.
