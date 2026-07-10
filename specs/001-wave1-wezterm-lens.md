# Spec 001 — Wave 1: wezterm lens (walking skeleton)

> Dossier: `plans/001-fleetops-architecture/00-SYNTHESIS.md` — wave 1 ≈ Option D.
> Ship: a board that lists Claude panes from wezterm and jumps to the selected one.
> Everything here is replaced-by-addition in waves 2–4 (sessions/, JSONL, /proc sensors).

## Goal

One binary `fleet`. Poll `wezterm.exe cli list --format json` every ~2 s, parse pane rows,
classify each title glyph, render a selectable board, jump on Enter via
`wezterm.exe cli activate-pane --pane-id <id>`.

## Data contract (verified live 2026-07-10, fixture `tests/fixtures/wezterm-list.json`)

`wezterm.exe cli list --format json` → JSON array of panes. Fields used:

| Field | Type | Use |
|---|---|---|
| `pane_id` | u64 | identity + jump target |
| `tab_id` | u64 | display grouping |
| `title` | string | glyph + semantic name |
| `cwd` | string | file URL — `file:///C:/…` (Windows) or `file://wsl.localhost/<distro>/…` (WSL) |
| `is_active` | bool | marker on board |

All other fields ignored (tolerant parse: unknown fields skipped, missing optional fields defaulted).

## Glyph classification (assumption A2 — no fallback in this wave, by design)

| Title prefix | Status |
|---|---|
| Braille spinner char (U+2800–U+28FF), e.g. `⠂`, `⠐` | `Working` |
| `✳` (U+2733) | `Idle` |
| Anything else (`wslhost.exe`, empty, shells) | not a Claude pane — excluded from board |

Semantic name = title with the glyph prefix and following whitespace stripped.

## Behaviour

- **Poll**: sensor task runs the list command on a ~2 s tick, sends `Msg::Panes(Vec<PaneRow>)`
  over mpsc. Command timeout 5 s; on error send `Msg::SensorError(String)` — board keeps last
  rows and shows the error + staleness in the footer. UI task never blocks.
- **Board**: one table — status glyph+word (colored: Working=green, Idle=yellow), semantic name,
  short cwd, pane id. Sorted by `pane_id`. Selection survives refresh (keyed by `pane_id`;
  if the selected pane vanished, clamp to nearest row).
- **Short cwd**: `file://wsl.localhost/<distro>/a/b` → `/a/b`; `file:///C:/x/y` → `C:/x/y`;
  unparseable → shown verbatim.
- **Keys**: `j`/`↓` down, `k`/`↑` up, `Enter` jump (activate-pane), `r` force refresh, `q`/`Esc`/`Ctrl-C` quit.
- **Jump**: fire-and-forget `activate-pane`; error → footer message, never a crash.
- **Footer**: pane count, last-refresh age, last sensor error if any.

## Non-goals (later waves)

Tokens/context %, NeedsAnswer, sessions outside wezterm, account attribution, doctor, history.

## Seams & tests (TDD order)

1. 🔴 `panes::tests` — parse fixture JSON → rows (count, fields); tolerant on unknown fields;
   glyph classifier table test (spinner variants / ✳ / wslhost / empty / bare emoji);
   cwd shortener table test; `activate_pane_args(pane_id)` pure argv builder test;
   `list_args()` builder test.
2. 🔴 `tui::model` — `update()` unit tests: Panes msg replaces rows + preserves selection by
   pane_id; selection clamp; quit; error msg recorded.
3. 🔴 `tui::view` — TestBackend render of a fixture `App` (rows + selection + footer), assert
   buffer content.
4. 🟢 wire `main.rs`: tokio runtime, RAII terminal guard + panic hook, select! loop
   (EventStream / sensor mpsc / tick).
5. ♻ refactor for specs, ♻ refactor for rules; `./check.sh` green.

Runner seam ported from tokenomics `src/runner.rs` (CommandSpec / Runner / Exec / CannedRunner)
— proven code, same license/house style.

## Dependencies (new crate, ask-first satisfied by stack decree + dossier)

ratatui, crossterm (event-stream), tokio (rt-multi-thread, process, time, sync, macros),
futures, serde + serde_json, thiserror, async-trait.
