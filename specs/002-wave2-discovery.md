# Spec 002 — Wave 2: discovery (Session aggregate)

> Dossier waves 2–4 = full Option A. This wave: rows become **sessions**, not panes.
> Panes stay as the jump lane (cwd × title match).

## Sources (verified live 2026-07-10, CC v2.1.206)

- `~/.claude/sessions/<pid>.json` — one per session, natively maintained:
  `{pid, sessionId, cwd, procStart, name, status: "busy"|"idle"|"shell"|"waiting", updatedAt, ...}`.
  (`waiting` found live during wave 4 — input-blocked state, disproves dossier A6.)
  ⚠️ stale files persist for dead PIDs — liveness is mandatory.
- `/proc/<pid>/stat` — field 22 (starttime, counted after the last `)` — comm may contain
  spaces/parens) must equal the session file's `procStart` string (PID-reuse guard). Verified:
  all live sessions match exactly.
- `/proc/<pid>/environ` — NUL-separated; `CLAUDE_ACCOUNT=<name>` attributes the account.
  Unreadable/absent → account unknown (render blank), never an error.

## Behaviour

- Discovery snapshot runs in the existing ~2 s poll (blocking fs reads via `spawn_blocking`).
- Scan `sessions/` dir: parse each `*.json` tolerantly (unknown fields skipped, unknown `status`
  strings preserved as `Other`); a file that fails to parse is skipped and counted (doctor shows it).
- Keep only live sessions (`/proc` guard). Attribute account per live pid.
- Board rows = live sessions sorted by session name; selection keyed by `sessionId`.
- Pane match (jump lane): **title first** — pane title (glyph stripped) == session ai-title or
  native name; unique → matched, several → ambiguous. Fallback: session `cwd` == pane short cwd
  (verified live: WSL panes report the WINDOWS cwd `file:///C:/Users/user/`; OSC7 cwd never
  crosses out of WSL, so cwd only matches the rare native pane). Ambiguous/unmatched → jump
  shows footer error, never guesses.
- Columns: STATUS (busy/idle/shell from native file for now — fold arrives wave 4), SESSION (name),
  ACCT, CWD, PANE (`tab:pane` or `—`).

## Seams & tests

- `discovery::parse_session_file` — pure, fixture from live file; tolerant/garbage cases.
- `discovery::starttime_from_stat` — pure over stat line bytes; comm-with-parens case.
- `discovery::account_from_environ` — pure over NUL bytes.
- `discovery::scan` — integration over a tempdir `sessions/` + fake proc root (plain dirs/files).
- `panes::match_pane` — pure table test (unique cwd, ambiguous cwd + title tie-break, none).
- model/view updated tests: selection by sessionId, session columns render.
