# Spec 003 — Wave 3: telemetry (transcript tail)

> Adds per-session tokens, context %, semantic ai-title, pending-question flag from the
> transcript JSONL — the same numbers the status bar shows. Approximate, never a bill.

## Sources (verified live 2026-07-10)

- Transcript: `~/.claude/projects/<slug>/<sessionId>.jsonl` where `slug` = session `cwd` with every
  char outside `[A-Za-z0-9-]` replaced by `-` (verified: `/home/user/my-project` →
  `-home-user-my-project`; dots and underscores both dash).
- Lines used (tolerant parse, all other types skipped):
  - `{"type":"assistant","message":{"usage":{input_tokens, cache_read_input_tokens,
    cache_creation_input_tokens, output_tokens}, "content":[...]}}` — context tokens =
    input + cache_read + cache_creation (statusline recipe); ctx% = that vs 200k window.
  - `{"type":"ai-title","aiTitle":"..."}` — last one wins; overrides the native `name`.
  - `AskUserQuestion`: `tool_use` block (`name":"AskUserQuestion"`, id) on an assistant line;
    a later `tool_result` for the same id (user line `content[].tool_use_id`) resolves it.
    Unresolved at EOF = pending question.
- Permission prompts are NEVER in JSONL (dossier) — stall detector covers that class (wave 4).

## Behaviour

- Per poll, per live session: stat the transcript; if `(size, mtime)` unchanged since last poll,
  reuse previous facts. Changed → read the last 256 KiB (whole file if smaller), drop the first
  partial line, parse.
- Missing transcript (session pre-first-message) → no telemetry, row renders `—`.
- `mtime` (secs since last append) is the transcript-activity age used by the wave-4 stall detector.
- Board gains: CTX% column (context tokens vs 200k; vs 1M once usage exceeds 200k — a session
  can't sit over its own window, seen live at 408k), TOK column (context tokens, compact
  `123k`), name column prefers ai-title over native name.
- ponytail: a pending question older than the 256 KiB tail window is invisible — accepted; asks
  and their answers are adjacent in practice.

## Seams & tests

- `telemetry::project_slug` — pure table (plain, dots, underscores, root).
- `telemetry::parse_tail` — pure over bytes: usage extraction (last assistant wins), ai-title
  (last wins), pending question (ask+result resolved / ask unresolved / result-only), partial
  first line dropped, unknown types skipped, garbage lines skipped (never an error).
- `telemetry::format_tokens` — pure (999 → `999`, 117k, 1.2M).
- fetch caching: unit test over injected `(size, mtime)` pairs.
