# Spec 008 — Wave 8: Codex sessions on the board

> the maintainer, 2026-07-10: "we also started using codex here in wsl so it would be good if you
> would also register Codex open windows." Recon (verified live, codex-cli 0.144.1): Codex
> TUI = node shim + `codex` musl child; environ carries `WEZTERM_PANE` + `TERM_PROGRAM`;
> `fd/1 → /dev/pts/N`; transcript = `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`
> (JSONL events); **no per-pid session file, no OSC pane title, no account env**. The clean
> pid→session join lives in `~/.codex/logs_2.sqlite` (`process_uuid = pid:<pid>:…`) — that
> needs a sqlite dependency, so **v1 joins without it** (below) and sqlite is the recorded
> upgrade trigger if cwd-join ambiguity bites in practice.

## Behaviour

- Codex TUI sessions appear as board rows next to Claude ones: same columns, same sort
  buckets, same Enter-jump (env pane id), same wave-6 pane highlighting (pts + `WEZTERM_PANE`
  gate identical).
- ACCT shows `codex` (colored by the existing `account_color` hash — it's a label).
- SESSION name: the last `user_message` text from the rollout tail (first line, truncated to
  60 chars); a TUI with no rollout yet (verified pre-first-prompt state) shows
  `codex — no prompt yet`.
- CTX: exact — the rollout's last `token_count` carries `total_token_usage.total` AND
  `model_context_window`; TOK shows the total. (This forces the ctx% seam below: Claude's
  200k/1M inference must not be applied to Codex windows.)
- Footer: appends `· N codex` when N > 0 (codex rows are not in `ScanStats.live`).
- `fleet doctor`: out of scope this wave (rows only).

## Status fold (pure, from the rollout tail)

| Rollout tail | Status |
|---|---|
| last event `task_complete` | Idle |
| `task_started` / `response_item` / `token_count` after the last `task_complete`, mtime age ≤ 300 s | Working |
| same, mtime age > 300 s | Stalled |
| `exec_approval_request` \| `apply_patch_approval_request` \| `elicitation_request` \| `request_user_input` with no later `task_complete` | NeedsAnswer |
| no rollout joined | Idle |

Assumption A-008-1 (unverified): the approval-request event family is strings-in-binary
evidence only — never yet observed on disk here. Fold it anyway; doctor drift-flagging can
come later if the names differ in practice.

## Discovery & join (no sqlite, v1)

- Process scan (injectable `proc_root`): a Codex TUI is a `/proc/<pid>` with `comm == "codex"`
  AND `cmdline` = argv0 only (the interactive TUI runs argument-less; `codex exec`,
  `codex --version` etc. are transient and skipped) AND `fd/1 → /dev/pts/*`. The node shim has
  `comm == "node"` — skipped for free. Per hit: cwd (`/proc/<pid>/cwd` readlink), environ via
  the existing `discovery::parse_environ` (gives `wezterm_pane`; `CLAUDE_ACCOUNT` absent),
  pts (`fd/1`), starttime ticks (existing `starttime_from_stat`).
- Rollout index (injectable `codex_root`): walk `sessions/*/*/*/rollout-*.jsonl`, newest first
  by filename (timestamp-sortable), cap 300 files; parse line 0 `session_meta`
  (`{payload: {id, cwd}}` — tolerant serde, unknown fields skipped).
- Join per process: candidates = rollouts whose `session_meta.cwd` == process cwd, filtered by
  the **liveness guard**: rollout `mtime` (epoch secs) ≥ process start wallclock − 600 s,
  where start wallclock = `btime` (from `/proc/stat`) + `starttime / 100` (HZ=100 on this WSL2
  kernel — hardcoded with comment; a wrong HZ only loosens the guard, degrading to
  newest-per-cwd). Pick the newest candidate. **Two live Codex processes sharing a cwd → no
  join for either** (never guess, house rule) — both render as `codex — session ambiguous`,
  status Idle, no ctx.
- session_id: the rollout uuid; unjoined rows use `codex-pid-<pid>` (stable per process).

## Seams & structure

- **`src/codex.rs` (new)**: pure parsers (`parse_session_meta`, `fold_rollout_tail` over the
  last 64 KiB, `is_codex_tui` over comm/cmdline facts, the join fn over already-read facts) +
  one fs-touching `scan(codex_root, proc_root, panes) -> Vec<SessionRow>` called from the
  sensor's `spawn_blocking` (same pattern as `discovery::scan`). Pane match via the existing
  `board::match_pane` (env pane id only — names `[]`, WSL cwd never matches).
- `src/paths.rs`: `codex_dir()` (`$HOME/.codex`), mirroring `claude_dir()` conventions.
- **ctx% seam**: `SessionRow` gains `ctx_pct: Option<u8>`, computed at assembly (Claude:
  existing `telemetry::ctx_used_pct` recipe; Codex: `total * 100 / model_context_window`).
  `view` renders the bar from `ctx_pct` and stops calling `ctx_used_pct` itself.
- `board`: extract the sort into `pub fn sort_rows(&mut Vec<SessionRow>)`; `assemble` keeps
  using it; the sweep concatenates Claude + Codex rows and sorts once.
- `tui/mod.rs` sweep: `codex::scan(...)` inside the existing `spawn_blocking`, rows appended
  before the single sort; codex count carried on `Snapshot` for the footer.
- Highlight: nothing to do — codex rows carry pts + wezterm_pane and the model's transition
  logic is row-driven.

## Tests (red first)

- `codex`: `is_codex_tui` table (argv0-only vs `exec`/`--version`, comm mismatch, non-pts
  fd/1); `parse_session_meta` fixture line; `fold_rollout_tail` table per the status table
  above (incl. approval-request → NeedsAnswer, garbage lines skipped); join table (unique cwd
  → newest; two procs same cwd → both unjoined; stale rollout older than start−slack
  rejected; no rollout → placeholder row); ctx pct math from a `token_count` line.
- `codex::scan` integration: tempdir fake `/proc` (comm, cmdline with NULs, environ, fd/1 +
  cwd symlinks) + fake `~/.codex/sessions` tree → assembled rows with pts/pane/status/name.
- `board`: `sort_rows` extraction keeps assemble ordering test green.
- `view`: a codex row renders (`codex` in ACCT, name, ctx bar from `ctx_pct`).
- Existing suites stay green (SessionRow field additions ripple through test constructors).
