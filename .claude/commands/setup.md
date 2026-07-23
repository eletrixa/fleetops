---
description: Set up fleetops on this machine — check platform, build, install, wire the pane lane, interpret doctor
---

# fleetops setup

Set up fleetops (this repo) for the user's machine. Work through the steps in order, report
what you find at each one, and adapt to what's actually installed — every lane of fleetops
degrades gracefully, so a missing piece means a reduced board, not a failed setup. Never guess:
when a check is ambiguous, show the user the output and ask.

Hard rules while setting up:

- fleetops is **read-only over Claude state** — never create, edit, or delete anything under
  `~/.claude` or `~/.codex`. Setup only reads them to check they exist.
- Ask before installing anything system-level (rustup, wezterm) or editing shell profiles /
  Windows environment variables.

## 1. Platform

Run `uname -a` and check for `/proc/self/stat`. fleetops targets **WSL2** (any Linux with
`/proc` can discover sessions, but the pane-jump lane shells out to the Windows `wezterm.exe`
across WSL interop). On macOS or native Windows, stop and explain: the board would come up
empty — there is nothing to set up.

## 2. Toolchain and build

`cargo --version` — if rustup is missing, offer to install it (ask first). The toolchain is
pinned by `rust-toolchain.toml`; rustup fetches it automatically on first build. Then:

```bash
cargo build --release
```

The binary lands at `target/release/fleet`.

## 3. Install on PATH

Ask where they want it (default: symlink into `~/.local/bin`):

```bash
mkdir -p ~/.local/bin && ln -sf "$PWD/target/release/fleet" ~/.local/bin/fleet
```

Verify with `command -v fleet`. If `~/.local/bin` isn't on their PATH, show the export line
for their shell profile but let them add it.

## 4. Detect the fleet's data sources

Check each and tell the user what the board will and won't show:

- **`~/.claude/sessions/*.json`** — the discovery source. If the directory is missing or empty,
  the board is empty until a Claude Code session runs. If their Claude state lives somewhere
  nonstandard, set `FLEET_CLAUDE_DIR=<dir>` in the environment that launches `fleet`.
- **`~/.codex/`** — if present, Codex CLI sessions appear on the board automatically
  (`FLEET_CODEX_DIR` overrides).
- **wezterm** — `command -v wezterm.exe` or `/mnt/c/Program Files/WezTerm/wezterm.exe`. Present:
  pane mapping + Enter-to-jump work. Absent: the board still works; only the jump lane is off.
  (Native-Linux wezterm is currently not wired to the pane lane — the binary name is resolved
  as `wezterm.exe`.)

## 5. Optional: exact pane identity

Without forwarding, panes are matched by title/cwd (good, occasionally ambiguous). For exact
matching, Windows must forward `WEZTERM_PANE` into WSL via WSLENV. If the user wants it, have
them run **in Windows** (PowerShell/cmd, not WSL) — confirm before touching their Windows env,
and warn it only affects newly opened panes:

```
setx WSLENV "%WSLENV%:WEZTERM_PANE/u"
```

## 6. Verify

Run `fleet doctor` and walk the user through every line of the report — it names each lane
(sessions, transcripts, wezterm, pane identity) and exactly what's degraded. Then have them
launch `fleet` in a spare pane. Keys: j/k move · Enter jumps · r refreshes · q quits.
