---
plan: 001-fleetops-architecture
status: draft
owner: the maintainer
created: 2026-07-10
type: research
---

# Fleetops architecture — architecture decision

**Goal:** Decide the data plane, store, and process topology for fleetops — the Rust/ratatui TUI monitoring all local Claude Code sessions (name, status incl. needs-input, tokens, context %, wezterm pane jump).

**Status:** Draft — dossier complete, decision pending the maintainer's acceptance (flip to `active` when accepted).

**Trigger:** 2026-07-10 conversation — "monitor all of our open claude code windows"; name `fleetops` chosen; oh-architecture run decreed before specs/code.

## Read order

| # | Doc | What it does |
|---|-----|--------------|
| **00** | [SYNTHESIS](./00-SYNTHESIS.md) | 5-minute decision summary — matrix, diagrams, recommendation: **Option A, sensor fusion single process** |
| 01 | [Deep dive](./01-deep-dive.md) | Verified data sources D1–D9 (live recon + official docs), tokenomics patterns + pain, assumption log |
| 02 | [Case studies](./02-case-studies.md) | Prior-art sweep: five data-plane lanes, the two KEY needs-input questions, PRAISE/PAIN |
| 03 | [Options](./03-options.md) | The 4 candidates with diagrams, seams, evolution paths |
| 04 | [Steelman](./04-steelman.md) | Advocate vs Prosecutor per option (independent agents), cross-examination, verdict inputs |

## Out of scope

- Remote machines / other hosts' fleets (evolution note only)
- Controlling sessions (fleetops is read-only + jump-to-pane; no send-text, no kill)
- Non-Claude agents (Codex/Gemini) — sensor seam anticipates, nothing built
- Account-level limits/usage (that's tokenomics' domain)

## Cross-references

| Path | What |
|---|---|
| `docs/RESEARCH.md` | Short recon index (corrected 2026-07-10) |
| `specs/README.md` | Waves will be specced from 00-SYNTHESIS §First implementation steps |
| `/tui/tokenomics` | Pattern donor: runner.rs, paths.rs, MVU seams, store (future) |
| `/tui/bridge` | Pattern donor: hub + sources + ndjson codec |
