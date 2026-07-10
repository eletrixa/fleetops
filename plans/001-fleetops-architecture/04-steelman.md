# Steelman court

> Advocate and Prosecutor ran as independent subagents (disjoint prompts, no shared conclusions)
> on the evidence pack (01, 02, 03). Cross-examination and verdict inputs written by the synthesis
> pass. Uncited arguments were instructed to be treated as RHETORIC; none needed the marker.


## Option A — Sensor fusion, single process

### Advocate (strongest honest case)

# The case for Option A — Sensor fusion, single process

## The data is already there; every other option adds machinery to re-collect it

The deep dive's key finding (01 D1) is that Claude Code v2.1.x natively maintains `~/.claude/sessions/<pid>.json` — discovery, busy/idle, and a derived name, updating within seconds, zero installation. D2 dissolves the multi-account problem (one dir, all 6 accounts). D3 supplies tokens, context % (the exact statusline recipe, `statusline.mjs:92-116`), ai-titles, and pending AskUserQuestion. D7 gives liveness + account attribution; D6 gives the pane map in a measured 110 ms. That is the full CLAUDE.md feature list — name, status, tokens, context %, pane jump — from read-only native sources. Option A is not a compromise; it is the discovery that the product's data plane already exists.

## It is the only option that honors every constraint by construction

Fleetops is read-only over the fleet; writing Claude config dirs is Ask-first (01 Constraints). B and C's hook lane mutates 7 settings.json files that already show 5 distinct md5s of drift and a verified no-op hook today (01 D4) — a doctor-babysitting treadmill in the exact currency the maintainer is short on: ops attention (01 Constraints). C is worse: the six tokenomics staleness bugs were all the daemon/store/heartbeat class ("stale data shown as live", 01 pain points; 03 C explicitly flags this), plus the documented stale-binary footgun now applies to a daemon. And the negative results are decisive: Anthropic itself ships flat files, no SQLite anywhere in `~/.claude` (01 Negative results) — the ecosystem consensus C bets against.

## Prior art says hooks under-deliver; A must exist anyway

Q2 (02): hooks do NOT fully solve needs-input — no WaitingForInput hook (#13024), idle_prompt is a buggy 60 s timer (#12048/#8320), AskUserQuestion uncovered, headless unreliable (#40506). Even hook-first claude-control keeps a heuristic fallback and overrides hooks against live CPU (02). B's own safety property concedes the point: "degraded mode = Option A exactly... never load-bearing" (03 B). So A is the mandatory core of B regardless — building it first risks nothing and defers the hook lane until its marginal value (permission prompts + instant transitions only) is actually felt.

## Fit, testability, delivery

Fusion is the robust pattern the survey found ("the robust tools hybridize", 02 Taxonomy) — four independent lanes degrade gracefully, unlike D's single undocumented glyph with "no fallback" (03 D). The dead-PID guard (D1: 20/36 stale files, cured by /proc+procStart) is fusion working. Architecturally it reuses bridge's proven hub ("many async lanes → one stream", 01 Siblings) and tokenomics' MVU seams; every parser is pure with fixtures, tokenomics-grade loop speed, no daemon-lifecycle tests (03 A). Cost: ~3–4 waves to full board, one process, ~0 attention (03 A); exit cost ≈ zero because sensors are plug-in tasks — hooks or SQLite bolt on additively (03 A Evolution).

## Concessions, honestly

Permission-waits are invisible (D3: never hit JSONL) and transitions lag ~1–2 s (03 A ceiling). But only launch-owning wrappers detect permissions deterministically, and wrapping breaks the maintainer's plain-wezterm workflow (02 wrapper PAIN); hooks are the partial patch, addable later. 1–2 s matches human glance cadence — Stargx ships with admitted lag (02). A1/A2 fragility is shared by every option in the field (02 agentic-era note); fusion is precisely the mitigation.

**Strongest point:** Claude Code already natively writes everything fleetops needs (sessions/*.json + JSONL + /proc + wezterm CLI), so Option A ships the full feature set read-only with zero installation, zero drift surface, and zero daemon — while B's own spec admits its degraded mode is exactly A, making A the mandatory, risk-free core to build first.

**Concessions:**
- Permission-wait is invisible to A (not in JSONL) — conceded; reframed: only launch-owning wrappers solve it deterministically and they break the workflow; hooks cover it partially and bolt on later as one more sensor.
- Status transitions lag ~1–2 s behind hooks — conceded; reframed: glance-cadence monitoring tolerates it (Stargx ships with admitted lag), and instant transitions are B's only other marginal gain.
- Everything rides undocumented internals (A1/A2) — conceded; reframed: every option and every surveyed tool shares this risk; A's four-lane fusion is the standard mitigation (degraded modes, tolerant parsers).
- No history/sparklines without a store — conceded; reframed: not in the CLAUDE.md feature list, tokenomics already owns per-account history, and SQLite is an additive later step per 03 A's evolution path.

### Prosecutor (strongest honest attack)

# The case against Option A

## 1. It cannot see the one state the tool exists for

Fleetops' domain vocabulary demands `NeedsDecision` (03 preamble), because a session blocked on a permission prompt is the single most expensive attention leak for a solo operator. Option A structurally cannot emit it: "permission prompts are NOT written to JSONL at all" (01 D3), confirmed as a negative result ("permission prompts invisible to JSONL, period", 01) and by the case-study verdict: "'permission needed' = Notification hook only" (02 Q2). A has no hooks. Worse, per A6 the native `status:'busy'` was never verified to distinguish waiting-on-permission — so the blocked session will most plausibly render as **Working**. That is not stale-shown-as-live; it is *wrong*-shown-as-live: the maintainer waits on a session that is waiting on him. The tool fails silently in exactly its highest-stakes state.

## 2. It is the tokenomics bug class, industrialized

All six tokenomics staleness bugs were "stale data shown as live," several in overlay merges and idle-frozen values (01 pain points). Option A's entire architecture is a merge of four laggy, mutually contradicting sensors: `sessions/*.json` where **20 of 36 files referenced dead PIDs** — Anthropic's own push lane already has the stale-file disease (01 D1); JSONL tails whose "status lag" is documented lane-wide PAIN (02 lane D); `/proc`, 2-state only and confounded by idle-at-100%-CPU (#19393, 01 D7); and wezterm glyphs whose format is untraced (A2). The status fold must arbitrate these with no push signal to ground-truth against — precisely the merge class tokenomics paid six bugs to learn, now with four inputs instead of two.

## 3. Four undocumented internals, no doctor

A rides A1 (sessions schema), A2 (glyph format), and JSONL churn simultaneously — 9 CLI versions in 30 days, undocumented line types, officially "internal, changes between versions" (01 D3). The pack's own mitigation for internals is "per-version `doctor` verification" (02 agentic-era note) — but `doctor` appears only in Options B/C. Zero-install means zero verification: when a release shifts a schema, A's breakage manifests as silently wrong statuses, the same silent-breakage PAIN indicted for scraping (02). With agents as the main developers, each silent drift invites threshold-tweaking "fixes" — the ccmanager "perpetual curation treadmill" (02) internalized, minus ccmanager's ownership of the PTY.

## 4. Prior art convergently rejects this shape

"The robust tools hybridize" (02 taxonomy). craftzdog explicitly rejected polling for hook stamps; claude-control is hook-first with heuristics only as fallback (02). The closest pure-pull analog, Stargx, "README admits inherent lag" (02). No surveyed tool succeeds with A's exact shape.

## 5. The evolution path is a confession

A's own text concedes the ceiling and plans to "add Option B's hook lane" later (03 A); meanwhile "degraded mode = Option A exactly" (03 B). A is therefore not an alternative — it is B minus its only lane covering permission prompts, sold separately for a savings of ~1 wave (03 B cost). You will build B anyway; choosing A merely schedules a period of shipping a board that lies about blocked sessions.

## Verdict

A saves one wave and a settings-edit Ask (01 D4) by deleting the only deterministic signal for the tool's core job, while quadrupling the undocumented surface with no verifier. That trade is indefensible under "ops attention is the scarcest resource" (01 constraints).

**Strongest point:** Option A structurally cannot represent NeedsDecision — permission prompts appear in no source it reads (01 D3, 02 Q2) — and per A6 a permission-blocked session will render as Working, so the tool silently fails in exactly the state that costs the operator the most attention.

**Attacks rejected as unfair:**
- Rejected 'JSONL parse cost will melt the CPU': #22041/V8-thrash pain is Node-centric; Rust byte-offset tailing plus compaction-appends-in-place (01 D3) largely defuses it — only initial-scan care is needed, an implementation detail not an architecture flaw.
- Rejected 'won't scale past 17 sessions': A4 says ×10 is still trivial for inotify limits; a scaling attack would be a strawman.
- Rejected 'zero-install is marketing': it genuinely is zero-install and read-only-compliant (01 constraints); the attack must target what that purchase costs, not deny the purchase.
- Rejected 'no SQLite means no history/multi-client': solo user, no stated sparkline requirement — that need is speculative and 03 C's own text lists the daemon as the attention hazard.
- Conceded as fair-to-A: question detection via pending AskUserQuestion tool_use does work from JSONL (01 D3, 02 Q1) — the prosecution rests on permission prompts, not questions.


## Option B — A + async hook push

### Advocate (strongest honest case)

# The case for Option B — the hybrid that earns its one extra wave

**The product is an attention router, and A cannot route the most expensive attention event.** Fleetops exists because ops attention is the scarce resource, and the costliest state is a session silently blocked on a human. Permission prompts are *never written to JSONL* (01 D3, 01 negative results), so Option A's own ceiling admits "permission-wait invisible" (03 A). The case-study sweep answered this definitively: without owning the launch, "permission needed = Notification hook only" (02 Q2). B is the only non-wrapper candidate that delivers `NeedsDecision` at all — and wrappers are disqualified because they break the maintainer's plain-wezterm workflow (02 wrapper PAIN). D is worse still: Working/Idle only, no NeedsDecision/NeedsAnswer, riding an undocumented glyph with no fallback (03 D, A2).

**B's safety property is architectural, not aspirational: degraded mode = Option A exactly** (03 B). Every documented hook pathology — idle_prompt's 60s timer and false positives (#12048/#8320), no AskUserQuestion hook (#13024), headless unreliability (#40506) (01 D4, 02 Q2) — degrades B to A's board, never below it. The push lane is additive, never load-bearing. Compare C, where the daemon *is* load-bearing: all six tokenomics staleness bugs were exactly the daemon/heartbeat class (01 pain points, 03 C), the stale-binary footgun now applies to a background process, and SQLite buys no freshness — the TUI polls anyway (03 C). Meanwhile the ecosystem's negative evidence is loud: zero SQLite anywhere in `~/.claude` (01 negative results); craftzdog's lane-C author explicitly praised "no separate database to get out of sync" (02).

**Prior art converges on exactly B's shape.** The robust tools hybridize (02 taxonomy); claude-control = hooks + heuristic fallback (02); Anthropic's own agent-teams answer is hooks + machine-readable state files (02). And B embodies all three mitigations the agentic-era note prescribes for undocumented-internals risk: tolerant parsers, per-version `doctor` verification, degraded modes (02 agentic note, A3).

**Cheap, testable, evolvable.** Cost is A + ~1 wave (03 B); `async:true` hooks cost 2.2 ms with no UX latency or error spam (01 D4); `session_title` arrives in hook stdin — semantic names without transcript reads (01 D4). The hook script is tested by executing against fixture stdin; the fold merge reuses tokenomics' proven overlay-merge pattern, table-tested (03 B). Evolution: SubagentStart/Stop → per-agent rows; exit = delete hooks via doctor, A remains (03 B).

**Concessions.** (1) B writes into 7 settings.json — an Ask-first boundary — and drift is real (5 distinct md5s today, 01 D4). Reframe: doctor is idempotent, opt-in, user-approved; and the *existing* status hook is already a verified no-op on every session (01 D4) — the fleet needs a doctor anyway. (2) Hooks miss AskUserQuestion (#13024). Reframe: B keeps A's JSONL pending-tool_use lane for `NeedsAnswer` (01 D3), so the two lanes are complementary — precisely the stated ceiling of the non-wrapper design space (02 Q2). (3) The events mailbox can accumulate stale files, as Anthropic's own sessions dir proves (20/36 dead PIDs, 01 D1). Reframe: it's a mailbox, not a store; the fold applies freshness + PID-liveness provenance per the tokenomics staleness lesson (03 preamble).

B is the honest maximum of the non-wrapper design space: A's zero-risk floor, the ecosystem's proven push lane on top, and one process to babysit.

**Strongest point:** Option B is the only candidate that can show the single most attention-expensive state — a session blocked on a permission prompt (invisible to JSONL, hence to A and D) — while its degraded mode is exactly Option A, so the hook lane's documented flakiness can never make it worse than the runner-up.

**Concessions:**
- B is the only option that mutates Claude config dirs (7 settings.json, Ask-first boundary) and inherits real drift (5 md5s today) — reframed: doctor is idempotent, opt-in, user-approved, and the fleet already carries a verified no-op hook that needs cleanup regardless
- Hooks do not cover AskUserQuestion (#13024 open), idle_prompt is buggy (60s, #12048/#8320), headless claude -p unreliable (#40506) — reframed: B never leans on hooks alone; JSONL pending-tool_use covers NeedsAnswer and degraded mode is exactly A
- The events mailbox can accumulate stale files, as Anthropic's own sessions dir demonstrates (20/36 dead PIDs) — reframed: it is a mailbox keyed by session_id, not a store, and the status fold applies freshness + PID-liveness guards per the tokenomics never-show-stale-as-live lesson
- Hook stdin schema is binary-grepped, not documented (A3) — reframed: doctor re-verifies per CC version, and a parse break only degrades to A

### Prosecutor (strongest honest attack)

## The case against Option B: a permanent tax for a hollow payload

**1. B's flagship capability targets an event class this fleet has already suppressed.** The only thing hooks deliver that no other lane can — permission-prompt visibility ("permission prompts are NOT written to JSONL at all", 01 D3; "'permission needed' = Notification hook only", 02 Q2) — is nearly worthless here, because the fleet's own discovery command is `pgrep -af 'claude --dangerously'` → all 17 live sessions run with `--dangerously-skip-permissions` (01 D7). the maintainer's launch mode has already deleted most NeedsDecision events. And the other needs-input gap stays open under B: no hook covers AskUserQuestion (#13024 open, "no current workarounds fully cover" it, 02 Q2; 01 D4), so NeedsAnswer still comes from A's JSONL pending-tool_use lane. B's unique payload shrinks to shaving ~1–2 s of transition latency (03 A ceiling) plus `idle_prompt` — a hardcoded 60 s timer with documented false positives and no-fires (#12048, #8320; 01 D4).

**2. The lane goes dark exactly where the fleet is heading.** Agents are the main developers; headless `claude -p` sessions are where hooks are unreliable (#40506, #38651; 01 D4, 02 PAIN). The push lane covers interactive sessions best and agent sessions worst — inverted coverage for this user.

**3. It converts CC's release cadence into a recurring chore against the scarcest resource.** Hook stdin schema is binary-grepped, not documented (A3), so 03 B itself budgets "attention: doctor re-verify after CC updates" — at 9 CLI versions in 30 days (01 D3), that's a verification loop every ~3 days. Drift is verified reality: the 7 settings.json show 5 distinct md5s today (01 D4). Worse, settings.json writes are Ask-first (01 Constraints), so agents cannot self-heal the lane; every repair blocks on the maintainer. Ops attention is the decreed scarce resource (01 Constraints) — B is the only option that structurally spends it forever.

**4. It re-creates the dominant bug class with higher authority.** Tokenomics' six staleness bugs were all "stale shown as live," including a **stale authoritative overlay winning the merge** (01 pain) — and B's fold is literally "hook evt > native evt" (03 B diagram). A crashed session never fires SessionEnd; its high-priority mailbox file asserts "working" until liveness scavenging wins. Anthropic's own hook-push plane demonstrates the failure at scale: 20 of 36 sessions/*.json reference dead PIDs (01 D1). And `async: true` swallows errors (01 D4: no error spam = no error visibility), so a half-broken hook writes wrong events silently — degraded mode is *not* cleanly A; it's A plus confident lies.

**5. There is a corpse of exactly this architecture already in this home directory.** `claude-wezterm-status.sh` is installed in all 7 settings.json and is a **verified no-op on every session** — undetected until this recon (01 D4). Installed-everywhere hook, silently dead: that is B's steady state, observed.

**6. It is deferrable by A's own design.** 03 A's evolution path: "add Option B's hook lane as one more sensor (additive)... Exit cost ≈ zero." Choosing B now pays the doctor/merge/drift machinery before measuring whether A's 1–2 s lag ever hurts. Build A; earn B with evidence.

**Strongest point:** B's only hook-exclusive payload — permission-prompt detection — targets an event class the fleet itself suppresses (all 17 sessions launched `--dangerously-skip-permissions`, 01 D7), while permanently taxing the scarcest resource with doctor re-verification across 7 drifting settings.json at a 9-releases-per-month CC cadence.

**Attacks rejected as unfair:**
- Rejected: 'hooks add latency/UX cost to sessions' — unfair; async:true measured at ~2.2 ms spawn (01 D4).
- Rejected: 'B has no fallback when hooks break cleanly' — strawman; degraded-mode-equals-A is explicit and real for clean absence (03 B); the honest attack is only about half-broken/stale events, not absence.
- Rejected: 'B adds daemon/SQLite/migration burden' — that is Option C's cost; B stays one process with no store (03 B).
- Rejected: 'events mailbox grows unbounded' — trivial to prune; not load-bearing.
- Conceded as B's genuine strength: if the maintainer ever runs permission-gated sessions, hooks are the only non-wrapper lane that sees them (02 Q2) — the attack is that today's fleet makes that capability idle, not that it is fake.


## Option C — Tokenomics twin (collector + SQLite)

### Advocate (strongest honest case)

# The case for Option C — the tokenomics twin

## 1. It is the only option that is actually the decreed shape

The project decree is "same shape as tokenomics" (fleetops CLAUDE.md). Options A/B borrow tokenomics' *code seams*; only C replicates its *topology*. And 01's pattern table is effectively a build manual for C specifically: collector↔TUI via WAL (1 writer, N readers, no IPC), single-writer discipline, `user_version` migrations, heartbeat, retention prune — every row cites a source file to port verbatim (01 §Tokenomics patterns, `collector.rs:20-26`, `store.rs`). For agentic development, where AI agents are the main developers, this is decisive: agents are excellent at porting a proven sibling with 127 tests and insta snapshots (01 Tests row) and breed bugs in novel designs. A/B's in-memory sensor-fusion state is the novel part of this project; C makes the hard part (state lifecycle, staleness) a port, not an invention.

## 2. Best test story, and TDD is decreed

03 itself grades C "the most proven test story of the four" (tempfile SQLite, FakeAdapter sensors, bounded-time collector tests). Spec-driven TDD is a constraint (01 §Constraints); the option that makes every wave's red-green loop cheapest compounds over the whole build.

## 3. The store quarantines the one certain risk

02's closing note: the risky surface is not the tech, it's undocumented Claude Code internals — 9 CLI versions in 30 days, schema churn per release (01 D3, 02 §JSONL PAIN). C confines every fragile parser inside the collector; the schema is the stable seam. When a CC release breaks a parser, you fix one small process while the TUI keeps rendering last-known-good with honest freshness stamps — provenance and freshness as *columns* makes the tokenomics staleness lesson structural (01 §pain points; 03 status vocabulary), and future clients (waybar glance — cheaper attention than opening a TUI; 03 C evolution) inherit it for free.

## 4. Parse cost is documented ecosystem pain; C amortizes it

Transcripts reach 22.3 MB (01 D3); CC itself hangs at 99% CPU on large session files (#22041) and ccusage_go exists solely to cut −87% memory/−92% CPU (02 §JSONL dashboards). C parses each JSONL line once, ever (byte-offset tailing survives `/compact`, 01 D3) into cheap rows; A/B re-derive fleet state on every TUI start.

## 5. History is in-domain, not gold-plating

Ops attention is the scarce resource (01 §Constraints), and needs-input moments fire whether or not the TUI is open. Only C answers "what did I miss": B's mailbox is last-event-per-session, and permission prompts never hit JSONL (01 D3, 02 Q2) — a missed NeedsDecision is unrecoverable in A/B. Plus tokens are the brief, and `output_tokens` undercounts ~2× mid-stream (01 D3) — sparkline trends are more honest than any instant read.

## Conceded, and reframed

- **Two processes = daemon babysitting; the six staleness bugs were exactly this class** (01 §pain, 03). True — but that tuition is paid: the fixes are existing code, and a heartbeat row rendered in the TUI makes collector death loud, never silent.
- **No SQLite anywhere in `~/.claude`; craftzdog rejected "a database to get out of sync"** (01 §negatives, 02). Those are stateless status-stampers with no token/history brief; and C's DB never claims authority over live status — provenance+freshness make it a cache with an honesty label.
- **SQLite adds zero freshness; TUI polls 1 s anyway** (03). Fully conceded. Instantaneity comes from the hook lane C ingests, exactly as in B.
- **A+2–3 waves; high exit cost** (03). The extra waves are ports, not designs; the "public contract" has one consumer on one machine, with additive `user_version` migrations as the proven escape hatch (01 §patterns).

**Strongest point:** C is the only option where the one certain future event — a Claude Code internals break — degrades a single replaceable collector process instead of blinding the whole board, while the fleet's history, including needs-input moments that fired while the maintainer wasn't looking, survives.

**Concessions:**
- Two-process topology invites the daemon-babysitting bug class that produced tokenomics' six staleness bugs — reframed: the fixes are paid-for portable code, and a visible heartbeat row makes collector death loud
- Ecosystem evidence points away from SQLite (none in ~/.claude; craftzdog explicitly rejected a database) — reframed: those tools are stateless status-stampers without fleetops' token/history brief, and C's store is a freshness-labeled cache, never live-status authority
- SQLite adds no freshness — the TUI polls at 1 s either way; fully conceded, instant transitions come from the hook lane, same as Option B
- Higher build cost (A + 2-3 waves) and high exit cost (schema as contract) — reframed: the waves are verbatim ports cheap for AI agents, and the contract has exactly one consumer on one machine with additive migrations as the escape hatch

### Prosecutor (strongest honest attack)

# The case against Option C — "Tokenomics twin"

## 1. It reproduces the exact topology that generated every tokenomics bug

Tokenomics shipped **six staleness bugs, all of the class "stale data shown as live"**: heartbeat written but never read, idle values frozen, stale overlay winning the merge, plus the loop-freeze (01 §pain points). Every one of those bugs lives in the collector→store→reader gap — the gap Option C imports wholesale. 03 itself concedes it: "Attention: heartbeat/daemon babysitting — the six tokenomics staleness bugs were all *this* class" (03 C). For a project whose decreed domain rule is "never show stale as live" (03 preamble), choosing the architecture with the worst verified track record on precisely that invariant is indefensible. A and B have no store-freshness layer to lie: state is rebuilt from the sources each tick.

## 2. The store buys zero freshness — the actual product

Fleetops' product is a *live* board: status incl. needs-input, context %, jump-to-pane (fleetops CLAUDE.md). On the two KEY questions — needs-input without hooks, and hooks' ceiling (02 Q1/Q2) — C is byte-identical to B. SQLite moves nothing: "SQLite adds no freshness (no cross-process change notification — TUI polls anyway)" (03 C). You pay a daemon and get the same 1 s tick as A.

## 3. What it does buy is speculative, and available later for free

History/sparklines and waybar/web readers appear nowhere in the requirements. Both A and B list "add SQLite history" as an additive evolution step (03 A/B evolution paths), so C front-loads 2–3 extra waves (03 C cost) for optionality that costs nothing to defer — while its own exit cost is "high (store schema is the public contract)" (03 C). Worse, the history it persists is built on numbers 01 flags as untrustworthy: `usage.output_tokens` undercounts ~2× (01 D3). Durable storage confers false authority on approximate data.

## 4. Persistence is the wrong response to schema churn

Every source is an undocumented internal that changes per release — 9 CLI versions in 30 days, unknown line types, A1/A3/A5 all live (01 D3, assumption log). A/B mis-parse → restart re-derives clean state. C mis-parses → wrong rows are *durably written*, and "migrations forever" (03 C) turns each upstream churn into a schema event. The ecosystem's verdict is unanimous: **zero SQLite in `~/.claude`** — Anthropic's own state plane is flat files (01 negative results); no surveyed tool in any of the five lanes runs a DB daemon, and the hook-lane author explicitly built "no separate database to get out of sync" (02 lane C, craftzdog).

## 5. Two binaries in an agentic shop doubles a documented footgun

The **stale installed binary** footgun is already on tokenomics' paid-lessons list (01 §pain). With AI agents as the main developers and read-only-over-fleet discipline, C means every wave must keep collector and TUI version-locked, restart a daemon correctly, and verify a heartbeat — a new thing demanding exactly the ops attention fleetops exists to reclaim ("ops surface must be near-zero", 01 Constraints).

## 6. The "twin" framing is a category error

Tokenomics is per-account limits, where windowed history *is* the product. Fleetops is per-session liveness (fleetops CLAUDE.md) — freshness is the product; history is decoration. Symmetry of shape is not symmetry of domain.

**Verdict:** C is B plus a daemon, a schema contract, and the tokenomics bug class — minus nothing you can't add in one later wave.

**Strongest point:** Option C pays two processes, migrations forever, and daemon babysitting to buy unrequested history, while reproducing exactly the collector→store→reader topology that produced all six tokenomics stale-shown-as-live bugs — and since SQLite gives no cross-process change notification, the live board it exists to serve is no fresher than Option A's.

**Attacks rejected as unfair:**
- Rejected 'SQLite WAL is flaky on WSL2' — unfair: the store lives on ext4 where WAL is proven in tokenomics (01 D8, 01 patterns table).
- Rejected 'the daemon will be unstable' — no evidence; tokenomics' collector runs; the honest attack is the staleness/attention class, not raw crash rate.
- Rejected 'C's test story is weak' — 03 explicitly calls it the most proven test story of the four; conceded to C.
- Rejected 'history is worthless' — history is legitimately valuable someday; the fair attack is that it's deferrable at zero cost from A/B, not that it's useless.


## Option D — wezterm lens

### Advocate (strongest honest case)

# The case for Option D — wezterm lens

## The scarce resource is attention, and D is the only option that consumes none

The constraint list is explicit: "solo user, one machine; ops surface must be near-zero (attention is the scarce resource)" (01 §Constraints). Tokenomics paid for six staleness bugs, all one class — "stale data shown as live" — plus the stale-binary and daemon footguns (01 §Tokenomics pain). Every rival grows that class: C re-buys the daemon/heartbeat babysitting outright (03 C: "the six tokenomics staleness bugs were all *this* class"); B adds a hook lane into 7 settings.json files that already show 5 distinct md5s of drift and whose *existing* hook is a verified no-op today (01 D4) — proof that hook infrastructure rots silently on this exact machine. Even Anthropic's own state files rot: 20 of 36 `sessions/*.json` reference dead PIDs (01 D1). D has nearly no state to go stale: one 1–2 s poll of `wezterm cli list` (median 110 ms, verified — 01 D6). A pane that's gone is gone. No PID-reuse guards, no dead-file GC, no doctor, no installer, no Ask-first settings mutation — the purest reading of the read-only-over-fleet Never-rule (01 §Constraints).

## The rivals' headline advantage is smaller than advertised

The reason to build A/B/C is NeedsDecision/NeedsAnswer. But 02's Q2 verdict is that *nobody* gets it cleanly without owning the launch: no WaitingForInput hook (#13024), `idle_prompt` is a buggy 60 s timer (#12048/#8320), AskUserQuestion uncovered, permission prompts never hit JSONL (01 D3), and wrappers break the plain-wezterm workflow (02 §wrapper PAIN). So A/B ship 3–5 waves of parsers over internals officially "internal, changes between versions" (01 D3; 9 CLI versions in 30 days) to lift the ceiling only partway. D delivers what Claude itself pushes at transition time — the OSC-stamped glyph+title users can't disable (01 D6, #31107): Working vs not-Working, semantic name, pane jump. For 17 sessions on a monitor, "not working → glance, one keypress to jump" is the actual job.

## Delivery, cost, testability

~1 wave (03 D) vs 3–7. Test surface: one pure wezterm-JSON parser + glyph classifier (03 D) instead of fixture treadmills for four undocumented formats (01 A1/A3/A5). No JSONL tailing fleet-wide dodges the documented CPU-pain class (99%-CPU hangs #22041, 22 MB files — 01 D3, 02 lane D PAIN); the selected session's transcript is read on demand only. Naming is free — no $0.17/day Haiku lane (01 D9).

## D is a real option, not a dead end

"Grows into A by adding sensors — nothing thrown away except its simplicity" (03 D). The runner seam, MVU shell, wezterm sensor, and jump verb are wave 1 of A *anyway*. Choosing D defers the A1–A6 bets (01 §Assumption log) until a week of real use shows which gap actually costs attention — evidence-priced escalation instead of speculative parser fleets.

## Concessions

Overview lacks tokens/context % (detail-pane only; tokenomics already owns the budget question per-account, and token figures are approximate regardless — 01 D3 undercount). Status is Working/Idle only. Glyph format is assumption A2 with no fallback for *status* — though name×cwd matching and jump survive a format change (01 A2 impact). Sessions outside wezterm are invisible — but the decreed domain is "one wezterm window with many tabs/panes"; the pane IS the row (03 D). Each gap has a named, additive upgrade (A's sensors), none requires undoing D.

D is "the honest baseline every other option must beat" (03 D). On this evidence, it hasn't been beaten yet — it's been out-featured on paper by options that pay in the one currency the maintainer can't spare.

**Strongest point:** Option D is a strict, one-wave subset of Option A that delivers the core job (which panes need me + jump there) from the only externally-supported interface, deferring every undocumented-internals bet (A1–A6) until real usage proves which gap is worth its permanent attention cost.

**Concessions:**
- No tokens/context % in the overview — deferred to on-demand detail pane; reframed: tokenomics sibling owns budgets per-account and JSONL token figures are approximate anyway (01 D3)
- Status is Working/Idle only, no NeedsDecision/NeedsAnswer — reframed: per 02 Q2 no non-wrapper option delivers these reliably either; idle-means-glance plus one-keypress jump covers the attention loop
- Glyph convention is undocumented (A2) with no status fallback if it changes — reframed: name×cwd mapping and pane-jump survive, and the fix is adding A's sensors, not a rewrite
- Sessions outside wezterm are invisible — reframed: the decreed domain is one wezterm window of panes (CLAUDE.md), so the pane IS the row (03 D)

### Prosecutor (strongest honest attack)

# The case against Option D — "wezterm lens"

## 1. It cannot show the one state fleetops exists for

Fleetops' scarce resource is ops attention; the payoff is knowing which of 17 sessions needs the maintainer. Option D's own ceiling admits **"Working/Idle only (no NeedsDecision/NeedsAnswer)"** (03 D). Permission prompts never hit JSONL (01 D3, 02 Q1), and the pane glyph is a binary busy/idle signal — so a session blocked on a permission prompt renders indistinguishable from idle. On a 17-session fleet (01 D7), "needs decision" displayed as "idle" means the maintainer goes back to manually scanning panes — the exact failure the tool was commissioned to eliminate. A monitoring board that is blind to the highest-value alert is not a baseline; it's a placebo.

## 2. It bets everything on A2 — with no fallback

The glyph/title convention is undocumented; the emitting code was never traced, and #31107 only proves users can't disable it, not that the format is stable (01 D6, A2). The pack is explicit: **"no fallback — when it changes, the board is blind"** (03 D). This is the claude-squad brittle floor by another name: "single UI-copy change breaks it silently" (02 lane A). Worse, it fails as a *staleness lie*: tokenomics' six paid-for bugs were all "stale data shown as live" (01 pain points), and a silently mis-parsed glyph shows *wrong* status as live, with no provenance/freshness signal to contradict it — violating the decreed status vocabulary that every status carry provenance + freshness (03 preamble). Even Claude's own title generation broke on a version bump once (#29335, 01 D9).

## 3. Broken identity model = migration dead-end

D has "no Session aggregate — the pane IS the row" (03 D). But identity truth is sessionId + PID/procStart (01 D1), and account attribution across 6 accounts requires `/proc/<pid>/environ` (01 D2) — which D never reads. Sessions outside wezterm (headless `claude -p`, 02 PAIN) are invisible entirely. The claimed evolution "grows into A" is generous: growing into A means replacing the domain's identity keystone (pane → Session aggregate), plus fuzzy pane↔transcript matching (cwd×title, 01 D6) for the detail pane — ambiguous when two sessions share a cwd, and the on-demand read can hit a 22.3 MB transcript (01 D3), the documented CPU-pain class (#22041, 02 lane D).

## 4. The savings are smaller than advertised

D costs ~1 wave vs A's 3–4 (03) — but D's wave already builds the wezterm poller, runner seam, glyph parser, and MVU shell, which *are* A's pane sensor. What D skips is exactly the sessions-file reader and JSONL tail (01 D1/D3) — the components that deliver the CLAUDE.md-specified columns: tokens and context % are absent from D's overview (03 D ceiling). You save two waves by not building the product.

## Verdict

D is defensible only as a throwaway spike. As an architecture, it couples the entire board to the single weakest assumption in the log, structurally omits NeedsDecision/NeedsAnswer, and repeats the stale-shown-as-live bug class tokenomics already paid to learn.

**Strongest point:** Option D bets the whole board on the fallback-less, undocumented glyph convention (A2) while being structurally unable to show NeedsDecision/NeedsAnswer — the single state fleetops exists to surface on a 17-session fleet.

**Attacks rejected as unfair:**
- Did not attack wezterm CLI latency — measured fast (median 110 ms, 01 D6); 1–2 s polling is genuinely fine.
- Did not claim the glyph format has already broken — A2 is an unverified risk, not an observed failure; the attack is on the absence of a fallback, not on a fabricated breakage.
- Did not attack D's read-only posture or setup cost — zero installation is real and honors the fleet constraint.
- Did not count the shared Rust/ratatui/MVU shell against D — every option pays that cost equally.

---

## Cross-examination

### Option A
- **Best advocate point:** Claude Code natively writes everything fleetops needs; A ships the full feature set read-only, zero install — and B's own spec admits its degraded mode is exactly A.
  → **Rebuttal:** "Everything" overstates: permission prompts exist in no source A reads (01 D3), and native `status:'busy'` is unverified during a permission wait (A6). A ships the full *observable-without-installation* feature set — a real but narrower claim.
- **Best prosecutor point:** A structurally cannot represent NeedsDecision; a permission-blocked session most plausibly renders as Working — wrong-shown-as-live in the highest-stakes state.
  → **Rebuttal:** On *this* fleet the attack mostly evaporates: all 17 live sessions run `--dangerously-skip-permissions` (01 D7), so the permission-prompt class is suppressed by the operator's own launch mode; the frequent needs-input event is AskUserQuestion, which A detects via JSONL pending-tool_use (01 D3). Residual exposure (occasional non-bypass session) is bounded by a stall detector (busy + no transcript append > N min → `Stalled?` with honest freshness) and by the hook lane remaining a one-wave additive upgrade.

### Option B
- **Best advocate point:** B is the only non-wrapper candidate that can show a permission-blocked session at all, and its degraded mode is exactly A — flakiness can never make it worse than the runner-up.
  → **Rebuttal:** "Never worse than A" is not quite architectural: `async:true` swallows hook errors (01 D4), so a half-broken hook writes *wrong* events with top merge priority — degraded mode is A plus confident lies unless the fold liveness-checks every hook event, which reintroduces the merge complexity B was meant to simplify.
- **Best prosecutor point:** B's only hook-exclusive payload targets an event class the fleet suppresses (17/17 `--dangerously-skip-permissions`), while permanently taxing the scarcest resource (doctor re-verify across 7 drifting settings.json at 9 CC releases/month).
  → **Rebuttal:** Partially fair, but the tax is overstated: hooks are configured once in the *shared* helper pattern (one script, 7 one-line references), `doctor` verification is agent-runnable read-only (only the *fix* is Ask-first), and if the maintainer's workflow ever includes non-bypass sessions or agent-teams hooks, the lane's value returns. The correct conclusion is "defer with a named trigger", not "never".

### Option C
- **Best advocate point:** C is the only option where a Claude Code internals break degrades one replaceable collector while history — including needs-input moments that fired while the maintainer wasn't looking — survives.
  → **Rebuttal:** The quarantine argument applies equally to A's sensor tasks (a broken parser degrades one sensor, the hub renders last-known-good with freshness stamps — same property, no daemon); and "missed needs-input history" is served by a scrollable event log in-process, not necessarily a durable store.
- **Best prosecutor point:** C pays two processes, migrations forever, and daemon babysitting to buy unrequested history, while reproducing the exact topology that produced all six tokenomics stale-shown-as-live bugs — and SQLite gives the live board zero freshness.
  → **Rebuttal:** The tuition-is-paid counter (fixes exist as portable code) is real but cuts the other way: the fixes are portable *because* the topology invites the bugs. No effective rebuttal on freshness — conceded by C's own advocate.

### Option D
- **Best advocate point:** D is a strict one-wave subset of A delivering the core job (which panes need me + jump) from the only externally-supported interface, deferring every A1–A6 bet until real usage prices the gaps.
  → **Rebuttal:** The subset claim is the strongest thing about D — but the deferred bets include the product's specified columns (tokens, context %, NeedsAnswer). D defers the requirements, not just the risk. As wave 1 *of A*, the argument is excellent; as a stopping point, it isn't.
- **Best prosecutor point:** D bets the whole board on the fallback-less undocumented glyph (A2) while structurally unable to show NeedsDecision/NeedsAnswer.
  → **Rebuttal:** None adequate for D-as-architecture. For D-as-first-wave, the bet is hedged: A's other sensors arrive in the following waves.

## Verdict inputs

- **A**: The court established A as the mandatory core of the design space — every option either contains it (B, C) or grows into it (D). Its real ceiling (permission prompts) is largely suppressed on this fleet by `--dangerously-skip-permissions`; its frequent-case needs-input signal (AskUserQuestion pending tool_use) works pull-only. Prosecution's strongest surviving demands: provenance+freshness discipline in the fold, a read-only `doctor` self-check, and a stall detector. These are design requirements, not reasons to reject.
- **B**: Correct end-state *if* the permission class returns to relevance; today its unique payload is ~1–2 s latency and a buggy idle_prompt, bought with a permanent Ask-first-gated maintenance loop. Defer behind a named trigger.
- **C**: Loses on every axis it was nominated for except test-story maturity; the store buys no freshness for a freshness product, and history is additive later. Rejected for v1.
- **D**: Rejected as an architecture (identity model, missing product columns, single fallback-less assumption), embraced as A's pane sensor and the natural first vertical slice.
