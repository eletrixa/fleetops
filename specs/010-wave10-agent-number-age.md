# Spec 010 — Wave 10: board `#` agent-number column + snapshot `age_secs`

> Status: **Draft** (agents never promote to Active — the maintainer does).
>
> The maintainer requested (FLEET v2 design contract): the Stream Deck FLEET page now shows one
> column per agent with the **agent board number `n`** on the face (not the wezterm tab number).
> To keep the TUI board and the Stream Deck reading the same identity, the board's leading column
> becomes that same `n`, and the snapshot grows the age the board already renders so downstream
> surfaces (Stream Deck badges) can show it without a second telemetry path.

## Behaviour

### 1. Board `#` column (agent number `n`, leading — replaces TAB)

- The board's leading column is now **`#`** and shows the **agent board number `n`**: the
  **1-based row order** (`i + 1`), the same `n` the `fleet snapshot` `sessions[i].n` carries.
- The old **TAB column is removed** (its 1-based `tab_index + 1` display and the `tab_cell`
  placeholder logic go away). The **PANE column stays** — "tabs back to panes": the jump target a
  human reads off the board is the pane id, not a tab number.
- New header order: `# | STATUS | DIR | SESSION | CTX | TOK | ACCT | AGE | PANE`.
- `n` is present on **every** row (it is pure row order), including sessions with no matched pane
  — there is no unmatched placeholder for `#` (unlike the old TAB column).
- The snapshot JSON is unchanged by this column move: it still carries the 0-based `tab_index`
  (automation drives `wezterm cli activate-tab --tab-index` with it). The board simply stops
  *displaying* the tab number; `tab_index` lives on only in the JSON.

### 2. Snapshot `age_secs`

- Each `sessions[]` entry gains **`age_secs`: `<number|null>`** — seconds since the session's
  transcript last appended, i.e. `SessionRow.secs_since_append` (set in `board::assemble` from
  `tel.secs_since_append`), `null` when unknown (no transcript / never appended).
- This is the same value the board's AGE column humanizes (`format_age`); the snapshot exposes the
  raw seconds so a consumer can humanize on its own terms.
- **Every other snapshot field is unchanged**, including the 0-based `tab_index`. Field order:
  `n, name, status, tokens, ctx_pct, age_secs, pane_id, tab_index, cwd, session_id`.

## Seams & structure

- **`src/tui/view.rs`**: header `"TAB"` → `"#"`; the leading cell renders `(i + 1).to_string()`
  from the row enumeration instead of `tab_cell(r)`; `tab_cell` is deleted; the leading column
  constraint stays `Length(3)`. `pane_cell` / the PANE column are untouched.
- **`src/snapshot.rs`**: `SessionJson` gains `age_secs: Option<u64>`, populated from
  `r.secs_since_append`, placed after `ctx_pct` (telemetry group). `render_json` is the
  deterministic test surface.

## Deterministic tests (red first)

- `view`: the leading column header is `#` (not `TAB`) and precedes STATUS; the leading cell shows
  the 1-based agent number `n` on each row (row 1 → `1`, row 2 → `2`), present even for a row with
  no matched pane, and it precedes STATUS.
- `snapshot`: `render_json` — `age_secs` equals `secs_since_append` on a row that has it, and is
  `null` on a row that does not; `tab_index` (and every other field) still present and correct.

## Out of scope

- No new dependency; no change to the sensor pipeline or to how `secs_since_append` is computed.
- Read-only over the fleet is unchanged. The Stream Deck face rendering that consumes `n` /
  `age_secs` lives in the sd-badges lane, not here.
