# Spec 004 — Wave 4: status fold + stall detector + doctor + polish

> The dossier's highest-stakes path: one pure, table-tested fold decides every shown status;
> `fleet doctor` reports drift; accounts get stable colors.

## Status fold (pure)

Inputs per session: native status (busy/idle/shell/other), pending-question flag,
seconds since last transcript append (None = no transcript).

| Rule (first match wins) | Status | Color |
|---|---|---|
| pending question | `NeedsAnswer` | magenta, bold |
| native `waiting` (input-blocked; found live 2026-07-10, disproves dossier A6) | `Waiting` | magenta, bold |
| busy ∧ transcript age > 300 s | `Stalled?` | red |
| busy | `Working` | green |
| idle | `Idle` | yellow |
| shell | `Shell` | dark gray |
| other/unknown native status | `Unknown` | red (drift signal) |

- `Stalled?` covers the invisible permission-prompt class (dossier risk row 1); its threshold is
  a named const `STALL_AFTER_SECS = 300`.
  <!-- ponytail: fixed threshold; make configurable when a real session proves 300 wrong -->
- Sort: NeedsAnswer, Waiting, Stalled?, Unknown, Working, Idle, Shell; stable by name within a bucket.
- Every fold change needs a table row (pre-mortem #3).

## Doctor (`fleet doctor`, read-only)

Prints a drift report and exits 0 (1 only on I/O failure to even scan):
session files total / live / stale-dead / parse-failed; unknown native status strings seen;
per live session: transcript found? account attributed? pane matched?; wezterm reachable + pane
count; CC versions seen in session files. No file is ever written.

## Polish

- Account color: stable hash of account name → palette of 6 (matches 6 accounts); shown as
  colored account cell.
- Footer: `N sessions · M need answer · refreshed Xs ago` + last sensor error.
- Header row count in title: ` fleet — N sessions `.

## Seams & tests

- `fold::status` — the priority table, every row + boundary (299/300/301 s).
- `fold::sort_key` — bucket order test.
- doctor report rendered pure from a `DoctorFacts` struct → string; assembly tested with
  canned facts (no live fs in tests).
- account color: same input → same color; distinct for the 6 known accounts.
