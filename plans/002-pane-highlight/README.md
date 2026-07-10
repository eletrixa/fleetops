# 002 — Pane highlighting: research dossier

**Question (the maintainer, 2026-07-10):** Enter-jump works. Can fleetops also highlight the *pane*
(border color per status), and give a stronger *pane-level* highlight when a session finishes?
Not the tab — the pane.

**TL;DR verdict:** True per-pane **border** color is impossible in wezterm today (global
`colors.split` only; open feature request [#7641](https://github.com/wezterm/wezterm/issues/7641)
— filed by someone building the exact same Claude-session-monitor use case). The achievable
per-pane highlight is a **background tint via OSC 11**, and it is fully working on this box:
an external WSL process can tint exactly one pane by writing the escape to that pane's
`/dev/pts/N`, and reset with OSC 111. Empirically proven end-to-end through
WSL pty → wsl.exe → ConPTY → wezterm on the installed build (nightly `20260705-075440`).
"Finished" gets a pulse (alternating tints) settling into a steady tint — all driveable from
fleetops with **zero wezterm.lua changes**.

## Hard limits (wezterm, as of 2026-07)

1. **No per-pane border/divider color.** `colors.split` is global; per-pane config overrides
   don't exist (overrides are per-window only). Maintainer's stated workaround is exactly the
   OSC-11 lane ([Discussion #4744](https://github.com/wezterm/wezterm/discussions/4744),
   [#5752](https://github.com/wezterm/wezterm/discussions/5752)). Open FR:
   [#7641](https://github.com/wezterm/wezterm/issues/7641).
2. **No CLI verb styles anything** — no `set-user-var`, no color/border command, no
   trigger-Lua-event ([#7307](https://github.com/wezterm/wezterm/issues/7307),
   [#6879](https://github.com/wezterm/wezterm/issues/6879) open). `cli send-text` feeds pane
   *input* (paste), never the terminal parser.
3. **`visual_bell` flash color is global** — the flash targets the ringing pane, but all panes
   share one flash color.
4. **`wezterm.time.call_after` unreliable outside config load**
   ([#3026](https://github.com/wezterm/wezterm/issues/3026)) — Lua-side polling must ride
   `update-status`.

## Empirical evidence (probe on this box, 2026-07-10)

Scratch window spawned, tested, killed — no live panes touched. Full logs:
session scratchpad `probe/` (hex dumps of OSC 11 query replies).

| Lane | Verdict | Evidence |
|------|---------|----------|
| Write to another pane's `/dev/pts/N` (same user) | WORKS | injected text visible in `cli get-text` |
| OSC 11 bg-set injected externally via pts | **WORKS, survives ConPTY** | in-pane `\e]11;?` query: `rgb:0000/0000/0000` before → `rgb:4545/2020/2020` after |
| Per-pane isolation | WORKS | sibling pane in same window stayed `rgb:0000/0000/0000` |
| OSC 111 reset (undocumented in wezterm docs) | WORKS | query returns exact baseline after reset |
| BEL → `visual_bell` flash | NEEDS-CONFIG | `visual_bell` unset in the maintainer's `.wezterm.lua`; flash is per-pane once configured, color global |
| OSC 1337 SetUserVar via pts | WORKS (Lua-only observable) | no garbage rendered; not visible in `cli list` (no user-var field — confirmed, 18 fields) |
| OSC 2 title-set via pts (lane proof) | WORKS | `cli list` TITLE flipped for that pane only |

**Session → pts mapping needs no hook:** fleetops already holds each session's PID
(`~/.claude/sessions/<pid>.json`); `readlink /proc/<pid>/fd/0` yields the pts directly.
Verified live: 7 sessions mapped, e.g. `pid=3274394 tty=/dev/pts/14 WEZTERM_PANE=12`.
Notable: pts is *more* precise than `WEZTERM_PANE` — three processes shared inherited
`WEZTERM_PANE=12` but each had its own pts. The pts is ground truth for "which terminal
display this session renders in". Guard: only inject when the session's environ has
`WEZTERM_PANE`/`TERM_PROGRAM=WezTerm` (don't tint arbitrary ttys).

## Options

| | A: direct pts injection | B: BEL + `visual_bell` | C: Lua engine (state file + `inject_output`) | D: upstream FR #7641 |
|---|---|---|---|---|
| What | fleetops writes OSC 11/111 to session's pts on status transitions | fleetops writes `\a`; wezterm flashes that pane | fleetops writes state JSON; `.wezterm.lua` `update-status` poller reads it via `\\wsl.localhost\`, calls `pane:inject_output("\e]11;…")` | real per-pane border color |
| Per-pane | yes (proven) | yes (flash only) | yes | yes |
| Border or bg | bg tint | bg flash | bg tint (+ tab colors, toasts for free) | border |
| Config change | **none** | `.wezterm.lua`: add `visual_bell` | `.wezterm.lua`: poller block | n/a |
| Fleetops change | small (highlight module) | trivial | state-file writer (breaks "writes no files") | none |
| Risks | new mutating verb (escape bytes to live panes); apps that paint own bg mask the tint (Claude Code uses default bg — tint shows) | one global flash color; audible default is SystemBeep | more machinery, two moving parts, hot-reload coupling | not shipped; no maintainer response |

## Recommendation

**Option A** as wave 6 — everything stays in fleetops, proven today:

- Detect transitions in the `Msg::Snapshot` handler (`src/tui/model.rs:104-115`) — old rows
  (keyed by `session_id`) still available before overwrite; that's the natural diff seam.
- Steady tints by status: `NeedsAnswer` → amber, just-finished (`Working`→`Idle`) → green
  **pulse** (3 alternations ~1 s, then steady) — the "higher highlight" for finish.
  `Working` again / pane focused (`is_active` in `cli list`) → OSC 111 reset.
- Injection: open pts write-only + `O_NOCTTY`, `spawn_blocking`/async write, bounded — never
  block the UI task. Skip sessions without `WEZTERM_PANE` in environ.
- Boundary note: this adds fleetops' third mutating verb (after `activate-tab`/`activate-pane`).
  Still no fleet *file* mutation. Consider a `--no-highlight` opt-out flag.

Later, optional: B (one `visual_bell` line in `.wezterm.lua`) stacks a flash on top for
finish events; C only if the maintainer wants tab-bar coloring and toasts driven from the same state;
D — thumbs-up and watch [#7641](https://github.com/wezterm/wezterm/issues/7641).

Side note: the maintainer's `.wezterm.lua` already has `CLAUDE_STATUS` user-var tab badges
(🔔/✅/⏳ + gold blink) fed by `claude-wezterm-status.sh` hooks — that chain was dead only
because `WEZTERM_PANE` never reached WSL (docs/RESEARCH.md:20-22); spec 005's WSLENV fix
unblocked it, so tab-level signaling may already work again. Pane-level (this dossier) is new.

## Sources

Docs: [escape-sequences](https://wezterm.org/escape-sequences.html) ·
[appearance/colors](https://wezterm.org/config/appearance.html) ·
[visual_bell](https://wezterm.org/config/lua/config/visual_bell.html) ·
[bell event](https://wezterm.org/config/lua/window-events/bell.html) ·
[user-var-changed](https://wezterm.org/config/lua/window-events/user-var-changed.html) ·
[inject_output](https://wezterm.org/config/lua/pane/inject_output.html) ·
[update-status](https://wezterm.org/config/lua/window-events/update-status.html) ·
[FAQ (ConPTY stripping)](https://wezterm.org/faq.html) ·
[multiplexing/WslDomain](https://wezterm.org/multiplexing.html).
Issues: [#7641](https://github.com/wezterm/wezterm/issues/7641),
[#7307](https://github.com/wezterm/wezterm/issues/7307),
[#6879](https://github.com/wezterm/wezterm/issues/6879),
[#3675](https://github.com/wezterm/wezterm/issues/3675),
[#3524](https://github.com/wezterm/wezterm/issues/3524),
[#3026](https://github.com/wezterm/wezterm/issues/3026),
[#1528](https://github.com/wezterm/wezterm/issues/1528).
Discussions: [#4744](https://github.com/wezterm/wezterm/discussions/4744),
[#5752](https://github.com/wezterm/wezterm/discussions/5752),
[#3337](https://github.com/wezterm/wezterm/discussions/3337),
[#6588](https://github.com/wezterm/wezterm/discussions/6588).
Community: [mwop.net wezterm notifications](https://mwop.net/blog/2024-10-21-wezterm-notify-send.html),
[smart-splits.nvim user-var pattern](https://github.com/mrjones2014/smart-splits.nvim).
