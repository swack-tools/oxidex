# The OxiDex AI Harness

OxiDex is a Rust reimplementation of ExifTool. The **AI harness** — referred to internally as
"the fleet" — is an autonomous system that runs continuously against the repository to find
metadata tags that real ExifTool reports but OxiDex does not, and to write the parser code that
closes those gaps.

This document describes what the harness is, how it works end to end, and what it has actually
produced. Every number below is followed by the file it was counted from and the window it
covers, so it can be reproduced. Where a figure cannot be derived from available records, it is
labelled **not measured** rather than estimated.

::: tip Reach for transcription first
The harness is **not** the default route to coverage. Most of the remaining gap
is data that already exists in machine-readable form in ExifTool's own tables,
and generating from it is deterministic, verifiable, and roughly five orders of
magnitude cheaper — 27,747 tag entries extracted in 1.3 seconds, against this
harness's ~112 model calls per delivered tag.

See [Transcription](./TRANSCRIPTION.md). Before starting harness work on a gap,
confirm the knowledge is not already written down somewhere; if it is, that is
transcription work and belongs there instead.
:::

::: warning A note on numbers
This project has a documented history of inflated claims. The README once advertised 32,677 tags
and "complete parity" when the real figures were 16,677 and ~58%; that had to be corrected in
PR #199. The measurement traps that produced those errors are described in
[Counting rules](#counting-rules-and-the-traps-they-avoid), and every headline figure here is
derived under them.
:::

---

## What it is

The harness is a dispatcher process that runs a pool of parallel worker processes. Each worker
takes one format (JPEG, NEF, CR2, …), asks a large language model to write a patch that makes
OxiDex extract a tag it currently misses, and then tries very hard to disprove that the patch
worked. Only a patch that survives every check gets committed.

The two programs are:

| Component | File | Size |
| --- | --- | --- |
| Dispatcher | `scripts/parallel_model_fix_loop.py` | 2,960 lines |
| Worker | `scripts/model_fix_loop.py` | ~8,200 lines |

The dispatcher holds a singleton `flock` at `~/.oxidex/logs/dispatcher.lock`, allocates worker
slots across formats in proportion to each format's open-gap count, and spawns each worker in its
own process group and its own git worktree. Per-tag assignment does not happen in the dispatcher —
each worker claims individual tags under a separate lock, so two workers never work the same tag.

## What it is for

Finding and filling missing tags. The ground truth is real ExifTool: a comparison harness runs
both binaries over the same sample file and diffs the tag sets. A tag ExifTool emits that OxiDex
does not is a **gap**. A tag both emit with different values is a **value difference**. Both are
work items.

The harness exists because this work is large, repetitive, and individually small — most fixes
are a handful of lines wiring one tag id to one field in one table — but each one still requires
reading ExifTool's Perl source to get the tag id, type, and conversion right.

---

## How it works, end to end

One attempt on one tag passes through the following stages. The order matters, and it is not the
obvious one: the cheap checks run first, the two expensive ones (the reviewer model call and the
full workspace test suite) run last, and **the commit is the very last step of all**.

Line references are to `scripts/model_fix_loop.py`.

### Before any model call is spent

| Stage | Where | Notes |
| --- | --- | --- |
| Claim the tag under `flock` | `run_tag_loop.claim`, 7116–7210 | Also filters blacklisted, already-landed, and cross-tier tags |
| **Reachability check** | 7248–7278, `default_format_reachable` 4863–4935 | Runs OxiDex on the sample; if the parser produces no format-specific output at all, the tag is skipped with **zero** model calls |
| Snapshot the ExifTool baseline | 8054 | The "before" side of the later gap comparison |

The reachability check exists because of a real failure class: a parser that was written but is
never actually invoked. Every model call spent on such a format is wasted, because no patch to
that parser can change the output.

### Per repair round

The worker runs up to `max_repair_rounds` (default 5) rounds. Each round:

| # | Gate | Where | Rejects with |
| --- | --- | --- | --- |
| 1 | Model call, extract a unified diff | `attempt_build` 4447–4628 | `no diff in model response` |
| 2 | **`git apply`** tolerance ladder | 4631, `git_apply_with_rung` 1103 | resend; on exhaustion 4656 |
| 3 | **`cargo build`** | 4643 | resend; on exhaustion 4656 |
| 4 | **Compare against ExifTool** (fresh comparison run) | `recheck_fn`, 5439 | produces data, not a verdict |
| 5 | **Structural gate** — no *new* OxiDex-only tags introduced | 5459–5469 | `introduced new oxidex-only tag(s): …` |
| 6 | **Gap-count gate** | 5471–5526 | `gap count did not decrease` |
| 7 | **Targeted tests** (`cargo test --lib` for the format) | 5528–5537 | `targeted tests (…) regressed` |
| 8 | **Duplicate-insertion check** | 5539–5545, `detect_duplicate_tag_insertion` 1238 | `duplicate: a handler for … already exists elsewhere` |
| 9 | Evidence gathering (live re-extraction + emission scan) | 5550–5557 | *never rejects* — wrapped in `try/except`, degrades to empty |
| 10 | **Reviewer model call** (APPROVE / REJECT / UNVERIFIABLE) | 5559–5562 | `rejected by review: …` |
| 11 | **Full `cargo test --workspace`** | 5564–5578 | `cargo test --workspace regressed` |
| 12 | Build evidence trailers | `_build_fix_gap_trailers` 5130 | — |
| 13 | **`git commit`** | 5610–5614 | — |

Two ordering decisions are deliberate and worth naming:

- **Gate 8 precedes gate 10.** The duplicate check is cheap and local; the reviewer call costs a
  model request. The code comment at 5259–5269 states the intent directly: run the duplicate
  check "BEFORE spending a reviewer call on it."
- **Gate 11 follows gate 10.** The full workspace suite is the single most expensive check, so it
  runs only on a patch that has already convinced the reviewer.

Gate 9 is intentionally not a gate. Evidence gathering feeds the reviewer better context, but a
failure to collect it must not sink an otherwise good patch, so both collectors degrade to an
empty string.

### The `git apply` tolerance ladder

Model-authored diffs frequently have slightly wrong line numbers or whitespace. Rather than
discard them, gate 2 retries progressively looser, stopping at the first rung that applies:

```text
1. exact                       git apply --recount -
2. ignore-whitespace           git apply --recount --ignore-whitespace -
3. context1                    git apply --recount -C1 -
4. context1-ignore-whitespace  git apply --recount -C1 --ignore-whitespace -
5. 3way                        git apply --3way --recount -
```

`rung=None` in the log means every rung was tried and none applied. The recorded error is the
*strict* rung's stderr, because that one names the context git actually searched for.

Rung 5 is special-cased: `--3way` implies `--index`, so `_restore_after_three_way` (1062–1100)
un-stages afterwards on both success and failure. Without it, a staged change would survive the
next round's `git checkout -- .` and silently leak into an unrelated attempt.

**Measured, all diffs carrying a rung field (5,247 of 7,886 rows in
`~/.oxidex/logs/model-fix-diffs/manifest.log`; the remaining 2,639 predate the field):**

| Rung | Diffs | Share of applied |
| --- | ---: | ---: |
| `exact` | 2,018 | 74.4% |
| `context1` | 547 | 20.2% |
| `ignore-whitespace` | 78 | 2.9% |
| `context1-ignore-whitespace` | 71 | 2.6% |
| **applied, total** | **2,714** | |
| `None` (all rungs failed) | 2,533 | |

The looser rungs rescue **696 diffs, 25.6% of everything that applied**. The ladder earns its
keep — a strict-only gate would have discarded a quarter of all working patches.

### Model selection

There is no fixed model. `pick_model_fn` defaults to `random.choice` over the configured pool and
is re-drawn **before every individual call** (`attempt_build` 4445), so a single repair
conversation can span several models and even several providers. There is no within-call
failover: `call_model` retries the one model it was handed, and only when the whole ladder is
spent does the attempt fail — at which point the next draw is a fresh uniform pick. As
`config.toml` puts it, the pool *is* the retry.

A fleet-wide rate governor sits inside the retry loop. A rate-limit response sets a **global**
cooldown that pauses every worker, growing exponentially per consecutive limited outcome (30s,
60s, 120s, 240s, capped at 300s). This is why `max_retries` is deliberately pinned at 1: at 3, a
single 429 ladder fires four limited reports and parks the entire fleet near the cap.

---

## Why the commit being last matters

`git_commit_fn` is called exactly once in `fix_gap`, at line 5610, after every gate above. The
commit is also what *stages* anything at all:

```python
subprocess.run(["git", "add", "-A"], cwd=repo_root, check=True)
argv = ["git", "commit", "-m", message]
```

`git_apply_with_rung` (1117–1119) applies to the **working tree only** — no `--index`, no
`--cached`. Until line 5610 executes, a candidate fix exists solely as unstaged, uncommitted
working-tree modifications inside that worker's worktree.

**So an interrupted run loses exactly the work that already passed its gates.** Concretely, a
killed worker loses:

- the applied edit itself (unstaged working-tree content);
- every gate result — `built`, `diff`, `remaining`, `post_match`, `live_evidence`, `approved`,
  and the entire model conversation — all process-local Python locals, never persisted;
- the `cargo build` and the full `cargo test --workspace` runs, which are the two most expensive
  things the harness does.

Three mechanisms then destroy the working tree: the dispatcher's shutdown handler SIGKILLs every
worker process group (868–880), so there is no chance to commit; the next round's
`clean_worktree` runs `git checkout -- .` and `git clean -fd`; and `reap_orphan_worker_pgids`
kills any survivor at the next dispatcher startup.

The **only** durable residue is the raw diff text. `logging_git_apply` (7811–7831) writes every
diff — applied or rejected — to `~/.oxidex/logs/model-fix-diffs/` before returning, and its own
comment calls this "the only durable record of what was tried." Re-landing such a diff means
re-running apply, build, comparison, both test suites, and the reviewer call from scratch.

There is **no checkpointing of a passed-gates-but-uncommitted candidate anywhere.** The tag-state
file is only written *after* `fix_gap` returns, so a killed worker records nothing; its claim
simply goes stale after two hours and becomes re-claimable. The claim heartbeat thread is a
daemon thread specifically so that a dying worker cannot keep its own claim alive.

This is the harness's sharpest design cost. The cheapest possible improvement is not a faster
model — it is a checkpoint between gate 11 and gate 13.

---

## Results

### Counting rules, and the traps they avoid

Four distinct numbers in this repository look like "tags closed" and only one of them is.

**1. `Tag:` trailers are tags *assigned*, not tags *delivered*.** Each worker is handed a cluster
of up to `max_cluster_tags = 6` tags and emits a `Tag:` trailer for each. It commits if it closes
*any* of them. Commit `2efe807e` (PR #185) carries **12** `Tag:` trailers and delivered **2**
tags. Counting trailers overstates by up to 6x on individual commits and by 1.8x overall.

**2. `wire N missing tags` in the commit subject *is* the delivered count.** It is computed at
line 5579 as `closed = gap["gap_count"] - remaining`, where `remaining` comes from re-running the
full ExifTool comparison after the patch. It is a measurement, not a claim.

**3. `Verified: recheck-pass gaps=A->B` is the same measurement, recorded independently.** Across
all 23 measured fix blocks, `A - B` equals `wire N` **exactly, with zero mismatches** — the two
records corroborate each other.

**4. `N tag fix(es)` in a sweep commit subject counts *worker commits*, not tags.** Sweep #180's
subject says "3 tag fix(es)" and it delivered 14 tags.

There is also a **double-counting trap**. Summing `wire N` naively across all commit bodies gives
228 — and that figure is wrong. GitHub squash-merges write one `* <subject>` bullet per
constituent commit, and stacked PRs re-list their parent's commits. Commit `b74ec52c` (PR #41)
lists **11** `wire 1 missing tags` bullets while changing **zero** files under `src/` — proof
those bullets belong to a parent branch, not to that commit. After de-duplicating identical fix
blocks, 132 raw blocks collapse to 40.

### Gaps closed

**67 tag gaps closed with measured evidence**, across 23 fix blocks, 13 formats, over
2026-07-25 → 2026-07-30.

| | Count |
| --- | ---: |
| Tags **assigned** to those fixes (`Tag:` trailers) | 119 |
| Distinct tag keys named | 103 |
| Tags **delivered** (`wire N`, corroborated by `gaps=A->B`) | **67** |
| Delivery rate | **56%** |

Formats touched: CR2, ELF, HEIC, JPEG, NEF, PDF, PE, PSD, RAR, RW2, TTF, X3F, XMP.

*Method: parsed every `fix(<fmt>): wire N missing tags` block out of `git log origin/main`
commit bodies, keeping only blocks carrying a `Verified: recheck-pass gaps=A->B` trailer, then
de-duplicated on (format, wire count, pool, tag list, closed count, worker) to remove stacked-PR
re-listing.*

**Not measured: the pre-2026-07-25 era.** Fix commits before that date predate the evidence
trailers, so there is no per-commit measurement to read, and the stacked-PR re-listing above
makes their bullets impossible to de-duplicate reliably. Aggressive de-duplication yields 36
tags; the raw bullet count yields 93. The true figure is somewhere in that range and **cannot be
narrowed from the available records**. It is excluded from the 67 above rather than estimated.

### Current coverage

Three coverage figures are in circulation and they measure different things. All three are
correct; none of them substitutes for another.

| Figure | Value | What it measures |
| --- | --- | --- |
| Repo-wide tag count | **16,677 tags, 58% ExifTool parity** | Tags known to the OxiDex tag database, across 140+ formats (README, corrected in PR #199) |
| Live comparison snapshot | **738 of 1,511 tags = 48.8%** | 13 formats, one sample file each, `/tmp/fin_*.json` generated 2026-07-31T01:19Z |
| Full-corpus audit (JPEG) | **60.8% of tag instances per file**, median file **84.3%** | 4,085 JPEGs, PR #203 |

Per-format, from the 2026-07-31 snapshot:

| Format | Matched | Missing | Value diffs | ExifTool total | Coverage |
| --- | ---: | ---: | ---: | ---: | ---: |
| MachO | 6 | 0 | 0 | 6 | 100.0% |
| ISO | 13 | 1 | 0 | 14 | 92.9% |
| TTF | 22 | 4 | 0 | 26 | 84.6% |
| NEF | 141 | 38 | 25 | 204 | 69.1% |
| X3F | 29 | 13 | 0 | 42 | 69.0% |
| PSD | 62 | 28 | 1 | 91 | 68.1% |
| CR2 | 102 | 78 | 11 | 191 | 53.4% |
| PDF | 2 | 2 | 0 | 4 | 50.0% |
| XMP | 15 | 6 | 10 | 31 | 48.4% |
| RW2 | 68 | 85 | 0 | 153 | 44.4% |
| DNG | 107 | 141 | 17 | 265 | 40.4% |
| JPEG | 133 | 224 | 13 | 370 | 35.9% |
| MRW | 38 | 72 | 4 | 114 | 33.3% |
| **Total** | **738** | **692** | **81** | **1,511** | **48.8%** |

::: danger Coverage percentages are key-set statements, not per-file statements
The comparison harness collapses every tag to one entry per `family:name` across the whole
corpus, so presence is unioned and values are compared across different files. **Adding sample
files can lower a percentage with no code change at all.** Measured cost on CR2 (PR #203): 118
tags match when two samples are measured separately, 94 when measured together.

JPEG is the clearest illustration — the same parser scores 21.9% (corpus-collapsed), 35.9%
(this single sample), 60.8% (real per-file mean), or 84.3% (median file) depending only on how
you count. Always read the absolute counts beside the percentage.
:::

### Models tried

Eight distinct model identifiers appear in `~/.oxidex/logs/model-fix-requests/manifest.log` over
2026-07-22 → 2026-07-30. Note that `DeepSeek-V4-Pro` and `deepseek/deepseek-v4-pro` are the same
model reached through two different providers, which use different id casing.

| Model id | Calls | Notes |
| --- | ---: | --- |
| `gpt-5.6-sol` | 18,607 | Current sole pool member |
| `deepseek/deepseek-v4-pro` | 19,555 | OpenRouter id form |
| `gpt-5.6-terra` | 16,271 | Prior incumbent |
| `Kimi-K2.6` | 5,213 | |
| `gpt-5.5` | 2,792 | |
| `DeepSeek-V4-Pro` | 2,675 | Same model, different provider/casing |
| `GLM-5.2` | 2,169 | |
| `openrouter/free` | 13 | Meta-endpoint, not a model — routes at random per request across ~15 free backends |

`openrouter/free` deserves the caveat. It is OpenRouter's Free Models Router: each request lands
on a randomly chosen backend from roughly fifteen, filtered to those supporting the features the
request uses. You do not get to choose, and you get a different model every call. It was adopted
because per-request availability was excellent (30 concurrent requests in 12.1s, zero 429s) but
it was barely exercised — 13 calls — before the pool moved on.

### Per-model patch apply rate

How often a model's diff applied cleanly to the working tree. This is the purest available
measure of patch quality: it only counts calls that actually produced a diff, so it is not
contaminated by provider outages.

| Model | Diffs produced | Applied | Rejected | **Apply rate** |
| --- | ---: | ---: | ---: | ---: |
| `gpt-5.6-sol` | 649 | 519 | 130 | **80.0%** |
| `gpt-5.6-terra` | 71 | 54 | 17 | **76.1%** |
| `gpt-5.5` | 518 | 332 | 186 | **64.1%** |
| `Kimi-K2.6` | 598 | 327 | 271 | **54.7%** |
| `deepseek/deepseek-v4-pro` | 4,880 | 2,308 | 2,572 | **47.3%** |
| `DeepSeek-V4-Pro` | 668 | 300 | 368 | **44.9%** |
| `GLM-5.2` | 134 | 60 | 74 | **44.8%** |
| `openrouter/free` | 4 | 1 | 3 | **25.0%** |
| **All** | **7,522** | **3,901** | **3,621** | **51.9%** |

*Method: `~/.oxidex/logs/model-fix-diffs/manifest.log` records `applied=True|False` per diff but
carries no `model=` field, so each diff row was joined to the nearest preceding successful
`phase=fixer` call for the same worker in `~/.oxidex/logs/model-fix-requests/manifest.log`,
within a 30-minute window. 7,522 of 7,886 diff rows resolved; 358 predate the `worker=` field and
6 had no matching call.*

The small-sample rows are weak evidence. `gpt-5.6-terra` (71 diffs) and `openrouter/free` (4
diffs) should not be ranked against `deepseek/deepseek-v4-pro` (4,880 diffs) as though the
numbers carry equal weight.

### Per-model tag attribution

::: warning The `(via …)` string is the pool roster, not the producing model
Line 5611–5612 builds the commit subject as:

```python
f"fix({fmt.lower()}): wire {closed} missing tags "
f"(via {'/'.join(m['name'] for m in config['models'])})",
```

That joins **every member of the configured pool**, not the model that produced the diff. Since
`pick_model_fn` re-draws per call, a fix committed under a six-member pool could have come from
any of the six — and `_build_fix_gap_trailers` takes no model argument, so there is no per-commit
record of the actual producer either. Attribution is therefore **unambiguous only for
single-member pools**.
:::

Both columns count as valid attribution: a tag the model produced directly, and a tag a human or
agent then corrected before landing, are both attributable to the model that produced the
original patch. They are shown separately so the distinction stays visible.

| Pool roster in commit | Attribution | Tags direct | Tags hand-modified | **Total** | Assigned |
| --- | --- | ---: | ---: | ---: | ---: |
| `deepseek/deepseek-v4-pro` | unambiguous | 30 | 25 | **55** | 88 |
| `Kimi-K2.6` / `DeepSeek-V4-Pro` | ambiguous, 2 models | 8 | 0 | **8** | 22 |
| `gpt-5.5` / `GLM-5.2` / `Kimi-K2.6` / `DeepSeek-V4-Pro` | ambiguous, 4 distinct models | 3 | 0 | **3** | 3 |
| `gpt-5.6-sol` | unambiguous | 1 | 0 | **1** | 6 |
| **Total** | | **42** | **25** | **67** | **119** |

"Hand-modified" is the set of tags landed through a commit that explicitly records human or agent
correction of the model's patch:

- `5b5f2dfb` — "land 4 hand-verified fixes from the patch archive (13 real gaps closed)"
- `4e3b9515` — "correct fabricated enum constants that passed the recheck"
- `86518989` — validator-flagged APP12 tags, re-verified by hand
- `872b8d89` — validator flag was a false positive, corrected by hand

That last pair matters: the harness's own validator produced **both** false positives (`872b8d89`)
and false negatives — `4e3b9515` caught fabricated enum constants that had passed the automated
recheck. The gate stack is strong but not sound, which is why the hand-modified column is not
zero.

**Not measured: per-model attribution for the 11 tags landed under multi-model pools.** The
commit record does not identify which pool member produced each diff, and the request manifest
cannot be joined to a commit (it records calls, not commits). This would require adding the
producing model to `_build_fix_gap_trailers` going forward.

### Failure taxonomy

Failures split into two layers that must not be added together: **transport** failures, where the
API call itself did not return, and **semantic** failures, where a reply arrived but the proposed
fix did not survive the gates.

#### Transport layer

From `~/.oxidex/logs/model-fix-requests/manifest.log`, 2026-07-22 → 2026-07-30. This file is the
authoritative record of call outcomes: each line ends in `OK`, contains `ERROR=`, or is a
` RETRY ` line. **RETRY lines are counted separately — they are neither a success nor a terminal
failure, and folding them into either skews the rate.**

Totals: **67,296 terminal outcomes — 34,643 OK, 32,653 ERROR** — plus 9,613 RETRY lines.

| Failure | Count |
| --- | ---: |
| **429 rate limit** | 27,662 |
| 403 Forbidden | 4,504 |
| Model returned an empty reply | 144 |
| DNS resolution failure | 130 |
| Read operation timed out | 77 |
| Connection reset by peer | 57 |
| Malformed / non-JSON response body | 24 |
| Missing `choices` in response | 16 |
| 503 Service Unavailable | 15 |
| Remote closed connection without response | 9 |
| 502 Bad Gateway | 4 |
| **`deadline_seconds` exceeded while streaming** | 3 |
| 409 Conflict | 2 |
| Other socket errors | 5 |

::: danger Do not read all-time per-model failure rates as model quality
They are artifacts of two provider outage days, not model behaviour. Per day:

| Model | Date | Calls | Failure rate |
| --- | --- | ---: | ---: |
| `gpt-5.6-sol` | 2026-07-23 | 15,271 | **90.6%** |
| `gpt-5.6-terra` | 2026-07-23 | 14,902 | **93.3%** |
| `deepseek/deepseek-v4-pro` | 2026-07-28 | 8,414 | **53.9%** |
| `gpt-5.6-sol` | 2026-07-30 | 2,877 | **0.2%** |
| `deepseek/deepseek-v4-pro` | 2026-07-27 | 9,716 | 0.5% |
| `GLM-5.2` | all | 2,169 | 0.1% |
| `Kimi-K2.6` | all | 5,213 | 0.3% |
| `gpt-5.5` | all | 2,792 | 0.5% |

On 2026-07-23 **both** `gpt-5.6` variants failed at 90–93% simultaneously — that is a
provider-wide outage, and it alone accounts for essentially all 27,662 rate-limit errors. The
2026-07-28 `deepseek` figure is the 4,504 403s, also provider-side. The same `gpt-5.6-sol` that
looks like a 75% failure model all-time ran 2,877 calls at **0.2%** on 2026-07-30.

A flat all-time table would rank models by which days they happened to be on shift. Report per
day, or separate transport failures from patch quality, or do not rank at all.
:::

#### Semantic layer

From `~/.oxidex/logs/lessons.jsonl`, 14,546 records. Each row is one recorded outcome with an
`event` classification:

| Event | Count | Meaning |
| --- | ---: | --- |
| `infra` | 4,814 | Model call failed — the transport failures above, charged to no tag |
| `critique` | 4,417 | A failed attempt was analysed to inform the next round |
| `build_failed` | 3,196 | Patch applied but `cargo build` failed |
| `gap_not_closed` | 826 | Built and ran, but the gap count did not decrease |
| `structural` | 386 | Structural lesson recorded about a format |
| `review_rejected` | 360 | Reviewer model rejected the patch |
| `wrong_value` | 226 | Tag emitted, but the value disagrees with ExifTool |
| `fixed` | 171 | Gate stack passed, commit created |
| `test_regressed` | 139 | An existing test broke |
| `duplicate` | 11 | A handler for that tag already existed elsewhere |

By reason string:

| Reason | Count |
| --- | ---: |
| **No diff in model response** | 2,172 |
| **Gap count did not decrease** | 833 |
| No working fix after repair attempt (apply/build exhaustion) | 800 |
| **Patch did not apply** (rejected diffs, from the diffs manifest) | 3,687 |

"No diff in model response" at 2,172 is the largest single controllable failure — a model
replying in prose, or with a mangled fence, when a unified diff was required. It has four
sub-forms in the source: a plain failure to emit one, and exhaustion of the request budget, the
verify budget, or the patch-chunking safety limit.

Note that `fixed` = 171 here but only 67 tags landed on `main` with evidence. The two count
different things: `lessons.jsonl` records every worker-local commit including those on branches
that were never swept into `main`, superseded, or discarded. It is not a substitute for the git
record.

---

## Reproducing these numbers

All figures come from four sources. The two manifests are authoritative for call and patch
outcomes; the git log is authoritative for what actually landed.

```text
~/.oxidex/logs/model-fix-requests/manifest.log   call outcomes (OK / ERROR= / RETRY)
~/.oxidex/logs/model-fix-diffs/manifest.log      patch apply outcomes (applied=True|False)
~/.oxidex/logs/lessons.jsonl                     semantic outcome per attempt
git log origin/main                              what landed, with evidence trailers
/tmp/fin_*.json                                  per-format coverage snapshots
```

Manifest line shapes:

```text
<ts> phase=fixer worker=<FMT> tier=T1 model=<model> prompt_chars=N elapsed=Xs reply_chars=N OK
<ts> phase=fixer worker=<FMT> tier=T1 model=<model> prompt_chars=N elapsed=Xs ERROR=<reason>
<ts> phase=fixer worker=<FMT> tier=T1 model=<model> RETRY <message>

<ts> worker=<FMT> applied=True|False rung=<rung> file=<name> apply_msg='<msg>'
```

Counting call outcomes correctly — note the three-way split:

```bash
awk '$0 !~ /RETRY/ { total++
       if ($0 ~ / OK$/) ok++
       else if ($0 ~ /ERROR=/) err++ }
     END { printf "calls=%d OK=%d ERROR=%d OK%%=%.1f\n", total, ok, err, 100*ok/total }' \
  ~/.oxidex/logs/model-fix-requests/manifest.log
```

Patch apply rate:

```bash
grep -oE 'applied=(True|False)' ~/.oxidex/logs/model-fix-diffs/manifest.log | sort | uniq -c
```

### Known-bad measurement approaches

Do not use any of these. Each has produced a wrong answer in this project before:

- **`pgrep -fc` on worker processes, or pairing request files against response files.** Both give
  wrong failure rates. The manifest is the only reliable source.
- **`failrate.py` in the session scratchpad.** PR #204 changed the log format and its parser
  stopped matching `OK` lines; it now reports 100% failure in every window.
- **Commit trailer counts.** They report tags *assigned*, not tags *delivered*.
- **Naively summing `wire N` across all commit bodies.** Squash-merge bullets re-list parent-branch
  commits; this yields 228 instead of the de-duplicated figure.
- **A coverage percentage without its absolute counts.** The denominator moves when samples are
  added.

---

## Summary of what is *not* measured

Stated explicitly, because an honest gap is more useful than a plausible invention:

| Figure | Why it cannot be derived |
| --- | --- |
| Tags closed before 2026-07-25 | Predates evidence trailers; stacked-PR bullets cannot be de-duplicated. Bounded to 36–93, not narrowable |
| Per-model attribution for 11 tags | Landed under multi-model pools; the commit records the pool roster, not the producer |
| Which of the ~15 `openrouter/free` backends served any given call | The router does not report the chosen backend, and it re-routes per request |
| Cost per landed tag | Token usage is logged to `cache-stats.log` only when the provider reports it; several providers do not |
| Wall-clock time per landed tag | Per-call `elapsed` is recorded, but build and test time is not attributed to a tag |
| Whether the 1,000/day free-tier cap binds in practice | Not measurable without spending the budget |

---

## See also

- [`CORPUS_AUDIT.md`](https://github.com/swack-tools/oxidex/blob/main/CORPUS_AUDIT.md) — the
  full-corpus coverage audit that established the per-file JPEG figures used above.
