# Spec 009 — Wave 9: `fleet snapshot` + tab-index column

> Status: **Draft** (agents never promote to Active — the maintainer does).
>
> The maintainer requested: a headless one-shot that dumps exactly what the board would render, as
> JSON, for external tools (dashboards, scripts) — plus a leading TAB number on the board so the
> tab a session lives in is visible at a glance. The board number is **1-based** (matches the
> wezterm tab bar and the Stream Deck TAB keys); the JSON `tab_index` is **0-based** (matches
> `wezterm cli activate-tab --tab-index`, for automation).

## Behaviour

### 1. `fleet snapshot` (headless, one JSON object to stdout)

- Gathers **exactly the rows the TUI board would render, in the same order** — reusing the same
  discovery → telemetry → fold → panes → codex pipeline (`collect::collect`), never a second
  data path, so a snapshot and the live board can never disagree.
- Prints ONE JSON object to stdout:

  ```json
  {
    "focused_pane_id": <number|null>,
    "sessions": [
      {
        "n":          <number>,   // 1-based, board row order
        "name":       <string>,   // semantic name shown on the board
        "status":     <string>,   // exact fold::Status variant name
        "tokens":     <number|null>,
        "ctx_pct":    <number|null>,
        "pane_id":    <number|null>,   // matched wezterm pane, null if unmatched
        "tab_index":  <number|null>,   // 0-based tab index (activate-tab --tab-index), null if unmatched
        "cwd":        <string|null>,
        "session_id": <string>
      }
    ]
  }
  ```

- `status` is the exact `fold::Status` variant name: one of `Working`, `Idle`, `NeedsAnswer`,
  `Waiting`, `Stalled`, `Unknown`, `Shell` (pinned by `fold::Status::name`).
- `focused_pane_id` comes from `wezterm cli list-clients --format json` — the `focused_pane_id`
  of the client with the **least idle time** across all discovered instances (the client the
  user is actively on); `null` when the lane is unreachable or reports no focused pane. A
  degraded wezterm lane is **not** a scan failure.
- Exit code: **0** on success even with 0 sessions; **non-zero** only on scan failure — the
  sessions dir being unreadable (`ScanStats.dir_unreadable`, same rule as `fleet doctor`) or the
  blocking scan task crashing.
- Serialized with `serde_json` (already a dependency), pretty-printed.

### 2. Board TAB column (1-based tab number, leading)

- The underlying `tab_index` is **0-based**, matching `wezterm cli activate-tab --tab-index`
  (`0` = left-most tab, counting all tabs incl. non-Claude ones — "order of first appearance of
  a tab_id within its window", zero-origin). That 0-based value is what the JSON snapshot emits,
  so automation can drive `activate-tab --tab-index` with it directly.
- The board's TAB **column displays `tab_index + 1`** — a **1-based** tab number, matching
  the maintainer's wezterm tab bar (`format-tab-title` in `~/.wezterm.lua` renders `tab.tab_index + 1`)
  and the Stream Deck TAB keys (labeled 1–6). Display is for the human at a glance; the JSON
  stays 0-based for automation. (The board no longer shows the raw 0-based index.)
- The TAB column moves to **immediately before STATUS** (was second-from-last). New order:
  `TAB | STATUS | DIR | SESSION | CTX | TOK | ACCT | AGE | PANE`. One TAB column, not two.
- Unmatched sessions render a **dim** placeholder: `—` (no pane) or `≈?` (ambiguous), styled
  DarkGray.

## Seams & structure

- **`src/collect.rs` (new)**: `Collected { rows, stats, lane_error, codex_count }` + `collect(&mut
  TailCache, &mut PaneCache, panes_result) -> Collected` — the ONE sensor pipeline
  (`discovery::scan` → telemetry → `PaneCache::fold` → `board::assemble` → `codex::scan` →
  `board::sort_rows`). Both `tui::sweep` and `snapshot::run` call it (no forked path).
- **`src/snapshot.rs` (new)**: private serde structs + pure `render_json(focused, rows)` + async
  `run(runner) -> (String, bool)` (`bool` = scan-ok → exit code). `render_json` is the
  deterministic test surface.
- **`src/panes.rs`**: `list_clients_args`/`list_clients_spec`, pure `parse_clients(bytes) ->
  Vec<(pane_id, idle_secs)>` + `pick_focused_pane_id` (least idle), async `focused_pane_id`
  (reuses `discover_sockets`); `tab_index` derivation flipped to 0-based.
- **`src/fold.rs`**: `Status::name() -> &'static str` (the contract's status strings, pinned).
- **`src/main.rs`**: `snapshot` subcommand; usage string updated.

## Deterministic tests (red first)

- `fold`: `Status::name` returns the exact 7 variant strings.
- `panes`: `parse_clients` extracts `(focused_pane_id, idle_secs)`, skips clients with no focused
  pane, tolerates garbage; `pick_focused_pane_id` picks least-idle / `None` on empty;
  `focused_pane_id` (CannedRunner) falls back to the own instance and returns the parsed id;
  `tab_index` derivation is 0-based (fixture: FleetOps pane in the 2nd tab → index 1).
- `snapshot`: `render_json` — field shape, `n` 1-based order, `status` names, `null` for
  tokens/ctx_pct/pane_id/tab_index on unmatched rows, `focused_pane_id` null + empty sessions.
- `view`: TAB column precedes STATUS and shows the **1-based** tab number (`tab_index + 1`);
  unmatched renders a dim placeholder.

## Out of scope

- `fleet doctor` gains nothing this wave. No new dependency (serde/serde_json already present).
- Read-only over the fleet is unchanged; `snapshot` only reads (`list`, `list-clients`).
