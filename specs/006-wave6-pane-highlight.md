# Spec 006 — Wave 6: pane highlight (OSC 11 tint + finish pulse)

> Research: `plans/002-pane-highlight/`. Wezterm cannot color a per-pane border (open FR
> wezterm#7641); the proven per-pane highlight is a background tint via OSC 11 written to the
> session's own `/dev/pts/N`, reset via OSC 111 — empirically verified through ConPTY on this
> box (probe 2026-07-10). No `.wezterm.lua` changes.

## Behaviour

Status-driven pane tints, written by fleetops to each session's pts:

| Board status | Pane tint |
|---|---|
| NeedsAnswer, Waiting | steady amber |
| Stalled | steady dark red |
| Working → Idle transition ("just finished") | **pulse** (bright/dim green alternation, ~1 s) settling into steady dark green |
| Idle with green already on | green stays (sticky until "noticed") |
| everything else (Working, Shell, Unknown, plain Idle) | no tint (reset) |

"Noticed" clears green: (a) status leaves Idle (user prompted again / session died), or
(b) Enter-jump to that row from the board. Amber/red are NOT cleared by jump — they reflect a
state that persists until resolved.

Hygiene: on the **first applied snapshot** of a run, sessions whose desired tint is None get
one reset write anyway — clears stale tints left by a crashed/killed previous `fleet`.
On quit, all currently-tinted panes are reset (best-effort, bounded, must never hang exit).
Vanished sessions (in tint state but not in the new snapshot) get a reset to their last pts.

Targeting guard: a session is highlightable only if its environ has `WEZTERM_PANE` (it renders
in a wezterm pane) AND its stdout resolves to `/dev/pts/*` (headless `-p`/piped sessions are
skipped). Assumption: a session's pts never changes for its lifetime (fd 1 fixed at spawn).

Opt-out: `fleet --no-highlight` — model still computes, the loop drops the commands.
`fleet doctor` untouched this wave.

## Escape lane (from the probe, exact bytes)

- set: `ESC ] 11 ; #RRGGBB ESC \` — per-pane, survives ConPTY
- reset: `ESC ] 111 ESC \` — restores config default (no need to remember the original)
- Colors are tunable constants in one place (`src/highlight.rs`); board theme is black-bg:
  amber `#453000`, red `#3a0d0d`, green `#0a3512`, pulse-bright `#1a7a30`.

## Seams & structure

- `discovery`: `LiveSession.pts: Option<String>` — `read_link(<proc_root>/<pid>/fd/1)`, kept
  only when the target starts with `/dev/pts/`. Testable via tempdir fake proc (symlink).
- `board`: `SessionRow.pts: Option<String>` — populated only when `wezterm_pane.is_some()`
  (the wezterm guard lives here, not in the writer).
- **`src/highlight.rs` (new)** — pure core + thin writer:
  - `Tint { None, Amber, Red, Green }` (Copy, Eq); `desired_tint(Status) -> Tint` (table above;
    Idle → None here — green stickiness/transition is the model's job, it owns prev-state);
  - `HighlightCmd { Steady { pts, tint }, Pulse { pts } }` — `Steady(None)` writes reset;
  - `osc_set(color) -> Vec<u8>` / `osc_reset() -> &'static [u8]` — exact-bytes tested;
  - writer: `spawn_apply(Vec<HighlightCmd>)` — detached tokio task; opens the pts write-only
    with `O_NONBLOCK | O_NOCTTY` (hardcoded Linux octal consts + comment — WSL2-only crate, no
    new dependency), `spawn_blocking` for the blocking open/write, all failures silently
    dropped (a dead pane is normal, the footer must not spam); `Pulse` sleeps between frames
    (never on the UI task); `reset_all(...)` for quit cleanup, awaited with a short timeout.
- `model` (pure): tint state `HashMap<session_id, (Tint, pts)>` + `pending_highlights:
  Vec<HighlightCmd>` drained by the loop exactly like `pending_jump`. Transition detection in
  `Msg::Snapshot` handling: prev statuses read from `self.rows` before overwrite; late older
  sweeps (existing seq guard) must produce no commands. Dedup: a command is emitted only when
  desired ≠ current — steady states write once, not every 2 s sweep.
- `tui/mod.rs` loop: drain `pending_highlights` → `highlight::spawn_apply` (skip when
  `--no-highlight`); after loop exit, reset tinted panes before terminal restore.
- `main.rs`: parse `--no-highlight` (hand-rolled, house style).

## Tests (red first)

- `highlight`: `desired_tint` table; `osc_set`/`osc_reset` exact bytes (`\x1b]11;#453000\x1b\\`).
- `model` table: finish transition emits Pulse; NeedsAnswer emits amber Steady once, second
  identical snapshot emits nothing; green sticky across Idle snapshots; status change off Idle
  emits reset; jump on green row emits reset, jump on amber row doesn't; vanished session emits
  reset; first-snapshot hygiene resets untinted sessions with pts; rows without pts never emit;
  late older sweep emits nothing.
- `discovery`: scan picks up `fd/1 → /dev/pts/7` symlink; `fd/1 → /dev/null` filtered out.
- `board`: pts flows to the row only when `wezterm_pane` is present.
- Live-verify harness: one `#[ignore]` test that reads `FLEET_PROBE_PTS` env and drives
  amber → pulse → reset against a real pts (used manually against a scratch pane).
