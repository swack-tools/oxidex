# Fix-Throughput Engine — Design Spec

**Date:** 2026-07-25
**Status:** Phase 1 partially shipped (PR #116); Phases 2–4 proposed
**Goal:** Minimize total wall-clock time to close all open ExifTool tag gaps
**Baseline at authoring:** 4,333 open gaps; 17.2 gaps/hour produced; 0.24 gaps/hour published

---

## 1. Why this spec exists

The fleet has never been model-limited. Every throughput investigation this
session that started from "the model isn't producing good fixes" was wrong,
and the evidence said so repeatedly:

- 25,919 fixer calls produced only 205 reviewer calls and 7 landed fixes.
  The instinct was "89% of failures are `no diff in model response`, so fix
  the prompt." But the fleet's single most productive 4-hour window ran at
  that *same* no-diff rate. The no-diff rate is loud, not binding.
- The next instinct was "wire up the dormant T3 bulk table-port tier."
  Adversarial verification killed it: `attempt_table_port` emits 4 of 8
  required evidence trailers, so **100% of its commits quarantine on
  arrival**, and its 12,000-char table-source cap shows only 27.5% of the
  flagship example table while the acceptance gate demands ≥80% exact.

What the measurements actually show is that **the fleet produces fixes far
faster than it publishes them**:

| Stage | Rate |
|---|---|
| Gaps produced by workers | 6.7–17.2 /hour |
| Gaps published to `main` | **0.24 /hour** |

That is a ~96% loss between "a correct fix exists on disk" and "the gap is
closed on `main`." No amount of prompt, model, or context work improves the
headline metric while that ratio holds. This spec therefore orders all work
by **where output is destroyed**, not by where effort is spent.

### Method note

Findings here come from a 41-agent parallel analysis of the live system in
which every proposed change was handed to an independent adversarial
verifier instructed to refute it. **33 of 35 candidate levers were
refuted**, including both of the author's own leading hypotheses. Only
claims that survived that pass, or that were directly measured afterward,
appear below. Where a number is an estimate rather than a measurement, it
says so.

---

## 2. Success metric

**Primary:** total wall-clock hours to reach zero open gaps.

**Tracking metric:** gaps published to `main` per hour, measured as landed
`fix(...)` commits' closed-gap counts over elapsed time. Production-side
rates (`fixed` events in `lessons.jsonl`) are explicitly *not* the headline
metric — optimizing them is what produced a pipeline that generates work it
cannot ship.

**Time-to-zero at various published rates (4,333 gaps):**

| Published rate | Time to zero |
|---|---|
| 0.24 /h (baseline) | 752 days |
| 6.7 /h | 27 days |
| 17.2 /h | 10.5 days |
| 50 /h | 3.6 days |
| 100 /h | 1.8 days |

The first row is why publish-path work outranks everything else.

---

## 3. Constraints and decisions

Decided with the user before design:

1. **Tiered verification, applied to every squad.** Cheap mechanically
   verifiable fixes take a fast lane; novel or computed logic takes the
   full lane. No squad is exempt, and no correctness guarantee is deleted —
   gates move to the layer that can run them once instead of repeatedly.
2. **Optimize time-to-zero**, not per-attempt conversion.
3. **Incremental and always-live.** The fleet keeps running throughout.
   Every claimed gain is proven from logs, never assumed.

Invariants that must not be broken by any change here:

- **No-discard invariant (M5).** No ref reset may discard commits not
  contained in `origin/main`.
- **Consume handshake (M2/M5).** A worker branch head is not reset until a
  merger records it consumed or quarantined.
- **Detached-HEAD publish.** `squad/*` is only ever fast-forwarded to a
  fully validated state.
- **Fail-safe review.** An unparseable or missing verdict must still reject.
- **No stale-report fall-through.** A failed comparison must never fall
  back to an on-disk report from a previous round.

---

## 4. Architecture: where output is destroyed

```
gap discovery → [T1 per-tag fixer] → build → targeted test → workspace test
     → LLM review → commit → squad merger → squad branch → sweep PR → main
                    ▲                        ▲                    ▲
                    │                        │                    │
                 §5.2 40% loss           §5.1 ~96% loss       §5.4 latency
              (verdict parsed            (daemon death,        (batch cadence)
               fail-closed)               validator gates)
```

Each numbered section below corresponds to one loss site, ordered by
measured magnitude.

---

## 5. Phase 1 — Publish path (SHIPPED, PR #116)

### 5.1 Merger crash containment + supervision

**Defect.** `run_batch_check` documents *"never raises — log loudly and
report via `ok`"*, but called `comparison_fn` with no `try/except`, and that
bottoms out in `find_tag_gaps.run_format_comparison`'s
`subprocess.run(..., check=True)`. Any non-zero exit — an operator `pkill`,
an OOM kill, a diff that compiles under `--bin oxidex` but breaks the
`tag-comparison-binary` feature path — raised straight through and killed
the daemon. **Nothing respawned a merger.**

**Observed impact.** 7 of 14 mergers died on this one line within seconds of
each other, stranding 68% of worker slots with no publish path for over an
hour. ~12 fix commits carrying ~27 tags accumulated unpublished on worker
branches. 6 of the 10 `fixed` events in that window came from squads whose
merger was dead.

**Fix.** Catch `CalledProcessError` per format, record it as a check failure,
and route to the existing "hold publication" path. Two properties are
load-bearing:

- It must **not** fall through to a stale report. `/tmp` accumulates
  `tagcmp-*.json` for days; reusing one would hand a previous round's
  verdicts to the publication gate, converting a loud crash into a silent
  false "clean".
- One format failing must not abort sibling formats in the same check.

**Supervision.** Containment is not immortality. `scripts/supervise_mergers.sh`
polls every 30s and restarts any squad missing its merger. It is deliberately
stateless and safe to run redundantly — mergers already hold a per-squad
flock with stale-heartbeat takeover, so a duplicate spawn loses the race and
exits. **Verified live** by killing a merger outright and observing
automatic recovery within one poll.

### 5.2 Trailing reviewer verdicts

**Defect.** `build_review_prompt` instructs the reviewer to *"answer each
checklist item briefly, THEN give your verdict"* — but
`extract_review_verdict_full` matched a verdict only on the **first** line.
A model that obeyed the prompt was scored `unparseable review verdict` and
fail-safed to **REJECT**.

**Observed impact.** 7 of 209 reviewer replies (3.3%) were APPROVE verdicts
inverted to REJECT, mapping to 5 distinct tags of which ~4 never landed —
against 10 tags delivered in the same window, roughly a **40% loss**. Review
runs *after* the build and *after* the gap-closed recheck, so each inversion
discarded an already-built, already-verified fix via `git_checkout_clean`.

**Fix.** Rescan bottom-up for the last line stating a verdict, tolerating a
`Verdict:` / `**Final Verdict:**` label and markdown emphasis. Bottom-up is
deliberate: checklist bodies routinely mention "approve"/"reject" while
discussing criteria, so the *last* such line is the model's conclusion. A
reply with no verdict line anywhere still rejects — the fail-safe posture is
unchanged, it is simply no longer triggered by correct answers.

### 5.3 Expected effect

Phase 1 does not make the fleet produce more. It stops the fleet from
throwing away what it already produces. Expected published rate moves from
0.24 gaps/h toward the 6.7–17.2 gaps/h already being generated — a
**~28–70x** improvement in the headline metric with no change to the model,
prompt, or safety bar.

**This must be measured before Phase 2 begins** (see §9). If published rate
does not move, the remaining loss is in the validator, and Phase 2 changes
target.

---

## 6. Phase 2 — Validator and quarantine (proposed)

Phase 1 increases the number of commits reaching the validator. The
quarantine ledger is currently ~96% rejection (48 entries vs 2 work-done
consumes), so the bottleneck is expected to move here. Phase 2 is
deliberately **not** pre-committed to a fix — it begins with measurement.

### 6.1 Quarantine is permanent; transient failures are indistinguishable

`load_quarantine` folds the ledger by patch-id and every later poll skips a
quarantined patch-id **without retry**. That is correct for a genuine defect
and wrong for a transient one. Two real transient classes were observed:

- `printconv-unverifiable` caused purely by `--perl-lib` being unset, so no
  verification could occur at all. (Mitigated operationally by setting it;
  not yet enforced in code.)
- Wafer truncation returning HTTP 200 with a truncation sentinel in the
  body, so transport-level retry never fires.

**Proposed.** Classify rejection flags as `permanent` (fabricated PrintConv,
wrong tag ID, test regression) vs `transient` (tooling unavailable,
truncated response, comparison crash). Only permanent flags write a
no-retry quarantine entry; transient flags schedule a bounded retry with
backoff. Requires a flag-taxonomy table in `validate_fix_commit.py` and a
retry counter in the ledger entry (the `attempt`/`backoff_seconds` fields
already exist and are currently advisory only).

### 6.2 Enforce `--perl-lib` rather than silently degrading

A merger started without `--perl-lib` cannot verify any PrintConv and flags
everything `printconv-unverifiable`. This silently converts a correctness
gate into a rejection generator. **Proposed:** fail fast at merger startup
if the configured Perl lib is missing or unreadable, rather than degrading.

### 6.3 Bounded retry on truncation sentinel

Detect the `[Wafer: response was truncated` sentinel in `review_verdict` and
retry once before parsing. Small (~2 recovered reviews per 63 at current
config) but trivial, and it compounds with §5.2 — they cover disjoint
thirds of the same unparseable-verdict population and must be applied in
order: retry the truncation first, then run the bottom-up verdict scan on
the possibly-retried reply.

---

## 7. Phase 3 — Work selection (proposed)

Only after Phases 1–2 have two clean days of measurement. These multiply
*production*, which is not the constraint until publishing is fixed.

### 7.1 Suppress the FLIR tarpit

FLIR/InfiRay is a measured tarpit: **0 fixes out of 657 attempts**, yet 40 of
the last 120 diffs (33%) target `flir_parser.rs`, from 6 different squads —
which also generates same-file cross-squad merge conflicts.

**Proposed.** A soft cap of N live claims per canonical module, reusing the
existing `resolve_canonical_table` + `claim_conflicts` machinery, or an
explicit module blacklist until a parser scaffold lands. This recovers up to
~33% of diff-producing capacity.

**Explicitly rejected alternative:** partitioning tags strictly by squad
ownership. Applied retroactively to the measured window it would have
blocked 6 of 9 fixes and 24 of 28 gaps — a 3x *reduction* — because 86% of
successful work was legitimately cross-squad. The proven seams (APP12, Exif)
are mined by many squads at once, and that is a feature.

### 7.2 Difficulty-tiered gap ordering

`tag_gap = active[0]` takes gaps in list order, so the ~501 `value_differences`
(where oxidex already emits the tag and only the value is wrong — dozens are
literally int→label) sit permanently at the tail and are unreachable. A
priority sort on `(difficulty_tier, fail_count, index)` makes the cheapest
wins reachable first, which directly serves time-to-zero.

### 7.3 Gap-weighted format assignment

Round-robin format assignment leaves ~2 of 20 slots idling every round, and
makes ~460 gaps unreachable. Gap-weighted apportionment in
`squad_worker_formats` is a straight ~10% capacity gain.

---

## 8. Phase 4 — Bulk table-port, if and only if prerequisites land (proposed)

The dormant T3 tier is the only mechanism with a step-change ceiling, but it
is **not** "wire up a flag," and the honest assessment is far narrower than
it first appears.

**Three hard blockers, in order:**

1. **No driver exists.** `run_table_job` appears only in a docstring.
   `attempt_table_port` is a bare inner loop with no claim/release,
   heartbeat, state persistence, worktree refresh, or lesson-emission
   analogue. This is writing a driver, not adding an argument.
2. **Output is 100% unpublishable.** `attempt_table_port` emits exactly
   `{Format, Table, Worker, Verified}` (verified at its `git_commit_fn`
   call). `REQUIRED_TRAILERS` is
   `{Format, Tag, Sample, Exiftool-Value, Oxidex-Value, Perl-Ref, Verified, Worker}`
   — so it supplies 3 of the 8 required and is missing **5**
   (`Tag`, `Sample`, `Exiftool-Value`, `Oxidex-Value`, `Perl-Ref`); `Table`
   itself is no longer required at all after PR #114. Every table-port
   commit therefore quarantines on arrival. The validator must be taught a
   distinct table-port evidence shape, and `attempt_table_port` must emit
   per-member evidence rather than table-level evidence.
3. **The source cap defeats the acceptance gate.** `DEFAULT_MAX_TABLE_SOURCE_CHARS`
   is 12,000 while `evaluate_table_port_gate` demands ≥80% of members exact
   *and* zero present-but-wrong. Measured visibility: `CanonCustom::Functions2`
   27.5%, `NikonCustom::SettingsD3` 39.6%, `Exif::Main` 8%. You cannot port
   80% of a table exactly from 27% of its ground truth, and guessing the
   remainder trips the zero-wrong clause.

**Realistic addressable pool:** of 4,333 gaps, 378 are in groups with no
table at all (T3-impossible), 913 sit in tables that truncate, and **383
(8.8%) both resolve and fit** — not the 38.6% a naive grouping suggests.

**The one honest pilot:** `Sony::AFStatus79` — 95 gaps, 9,256 chars, **98.9%
of source visible**. It is the only top-5 group whose table fits the cap.

**Gate for proceeding:** fix blockers 1–3, run the `Sony::AFStatus79` pilot,
and measure. T3 has executed exactly **zero** times in 46,709 model calls, so
its per-table success rate is genuinely unknown and the acceptance gate is
binary with no partial credit. Do not fund a fleet-wide T3 rollout on an
unmeasured tier.

---

## 9. Measurement plan

Every phase gate is a measurement, not a judgment call.

**Headline (published rate):**
```
gaps published/hour = Σ(closed-gap counts on fix(...) commits reachable
                        from origin/main in window) ÷ window hours
```
Source: `git log origin/main`, cross-referenced with `Verified: recheck-pass
gaps=N->M` trailers.

**Funnel conversion**, per stage, from existing logs:

| Stage | Source | Current |
|---|---|---|
| attempts → diffs | `lessons.jsonl` `build_failed` vs others | ~11% |
| diffs → reviews | `manifest.log` `phase=reviewer` count | 0.8% |
| reviews → approvals | verdict distribution | ~60% (was ~40% pre-§5.2) |
| approvals → consumed | `squad-status/*.json` `status=consumed` | measure |
| consumed → main | `git log origin/main` | measure |

**Health invariants** (alert, not optimize):

- 14/14 mergers alive — `pgrep`, now supervised
- Dispatcher alive; worker count == configured slots
- Quarantine growth rate; ratio of transient to permanent flags
- Swap delta across checks (host stability under load)

**Phase gates:**

- Phase 1 → 2: published rate measured over ≥4 clean hours. If it has not
  risen substantially toward the production rate, the residual loss is in
  the validator — re-derive before building §6.
- Phase 2 → 3: quarantine rejection ratio falls below 50%, or is shown to be
  dominated by genuine defects.
- Phase 3 → 4: production rate is the binding constraint again (published
  ≈ produced), justifying step-change work.

---

## 10. What not to do

Each of these looked attractive and was refuted by measurement:

- **Do not optimize the "no diff in model response" rate as a throughput
  play.** It is 89% of failures but not the cap — the most productive window
  ran at the same rate.
- **Do not lower reviewer `reasoning_effort` for speed.** The reviewer is
  2.8% of model wall-time (9.9h of 348.6h); the fixer is 87.4%. Eliminating
  reviewer latency entirely buys <3% while weakening the only gate against
  sample-gaming.
- **Do not share `CARGO_TARGET_DIR` across workers.** Measured as a 15x
  regression on the hottest compile path; per-worktree `target/` is
  deliberate.
- **Do not partition tags strictly by squad.** Retroactively blocks 86% of
  successful gaps (§7.1).
- **Do not refactor `find_tag_gaps.run_format_comparison` to return
  `(ok, output)`.** Touches 6 production + 8 test call sites; per-consumer
  `try/except` gets the same containment.
- **Do not wire T3 as-is.** 100% of its output quarantines (§8).
- **Do not add a database (e.g. SurrealDB) for lesson storage.** The
  knowledge layer is injected into prompts as text; a query layer adds a
  new always-on failure dependency for 20+ concurrent workers without
  addressing any measured loss. Every root cause found this session was an
  infrastructure defect, not a knowledge-representation problem.

---

## 11. Risks

| Risk | Mitigation |
|---|---|
| §5.2 lenience approves a bad fix | Bottom-up scan still requires an explicit verdict keyword; no-verdict still rejects. PrintConv byte-verification and workspace tests are unchanged. |
| Phase 1 shifts load to the validator and quarantine grows | Expected and explicitly measured at the Phase 1→2 gate; §6 is the response. |
| Supervisor masks a crash-looping merger | Supervisor logs every restart; a squad restarting repeatedly is visible in `merger-supervisor.log` and should be investigated, not ignored. |
| Single-model monoculture (all DeepSeek-V4-Pro) correlates fixer and reviewer blind spots | Reviewer reject-rate tracked in §9; revert to a mixed pool if approval rate rises without a matching rise in landed, verified tags. |
| Transient/permanent flag taxonomy (§6.1) misclassifies a real defect as transient | Retry is bounded; a patch-id exceeding the retry budget becomes permanent. |
