# Spec 007 — Wave 7: DIR up front, with a project badge (emoji + color)

> The maintainer requested: "move dir instead of Tab so its status, DIR SESSION CTX and then the
> rest. also try to DIR somehow color code and use emoticons so its visually clear what dir
> project it is — that is important."

## Behaviour

- Column order becomes: **STATUS | DIR | SESSION | CTX | TOK | ACCT | AGE | TAB | PANE**
  (was: STATUS | TAB | SESSION | ACCT | CTX | TOK | AGE | DIR | PANE). Nothing else about the
  cells changes (TAB/PANE still show `≈?`/`—` when unmatched, etc.).
- DIR cell renders `<emoji> <dir_name>` and is colored — **the same dir name always gets the
  same emoji and the same color** (pure hash, like `account_color`), so a project is
  recognizable at a glance across sessions, accounts, and restarts.
- Identity is the displayed `dir_name` (last path segment), not the full cwd — the same
  project checked out twice shares a badge; that's a feature.
- No config file, no per-project mapping — deterministic hash only (upgrade path: a TOML
  override map, only if hash collisions start biting on real projects).

## Badge design

- `dir_badge(dir: &str) -> (char, Color)` — pure, in `view.rs` next to `account_color`.
- Emoji palette: **single-codepoint, width-2, no variation selectors** (TestBackend/wezterm
  width sanity): `🦀 🧠 🚀 📦 🌊 🔥 🐙 🎯 🌿 💎 ⚡ 🍋` (12).
- Color palette: the 6 bright ratatui colors (as `account_color`), hashed with an
  **independent seed** so emoji and color don't correlate — 72 effective combos.
- Hash: djb2 + splitmix64 finalizer (copy the `account_color` recipe; different seeds for the
  emoji pick and the color pick). Tune seeds so a representative set of project dirs —
  `fleetops`, `tokenomics`, `project-a`, `project-b`, `project-c`, `project-d` — get 6 distinct
  (emoji, color) pairs (same pattern as the account-seed note).

## Seams & tests

- `view.rs`: reorder header + row cells + constraints (DIR gets `Max(18)` for the badge);
  `dir_badge` pure fn.
- Tests (red first):
  - header order: `STATUS`, `DIR`, `SESSION` appear in that left-to-right order on the header
    line (assert via column positions in the rendered header row, not just `contains`);
  - `dir_badge` stability: two calls, same result;
  - distinctness: the 6 real dirs above → 6 distinct `(emoji, color)` pairs;
  - DIR cell shows the emoji + name (rendered screen contains e.g. the badge emoji for
    `/tui/fleetops`'s `fleetops`);
  - existing tests keep passing (column reorder may move fixed positions they assert).
