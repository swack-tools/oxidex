# Fleet Knowledge & Scaling: Squads, Job Tiers, Evidence-Bearing Merges

**Date:** 2026-07-24
**Supersedes/extends:** `2026-07-23-throughput-tags-per-hour-design.md` (F1–F6 all landed; this builds on them).
**Files under change:** `scripts/model_fix_loop.py`, `scripts/parallel_model_fix_loop.py`,
`scripts/find_tag_gaps.py`, `scripts/log_sweep_review.py`, `scripts/watch_parallel_fix.py`,
new `scripts/attribute_gaps.py`, `scripts/squad_merge_loop.py`, `scripts/overlord_sweep.py`,
`scripts/validate_fix_commit.py`, `scripts/distill_lessons.py`, `scripts/squads.toml`,
`config.toml`, `config.example.toml`, tests.

This spec merges three critiqued designs (knowledge layer / squad topology / merge
pipeline) into one architecture. Every high-severity critique is resolved in-line or
deferred with a reason; the traceability table in Appendix A maps each one to its
disposition.

---

## 0. Goals and constraints

Goals (the five user requirements):

1. **Shared knowledge**: every lesson (human verdict, wrong-value recheck, reviewer
   rejection) reaches every worker in minutes, not "same-format-only or never".
2. **Grouping**: many workers on one format (JPEG's 3012 gaps) learning together,
   without making worktree merges harder.
3. **Scale** 20 → 50 → 100 — honestly, against the real binding constraints.
4. **Merge probability**: maximize P(a fixed gap flows cleanly through the ~5-minute
   overlord sweep into a PR). Observed today: ~3% merge yield (2 of 65 worker_done).
5. **Close all ~4363 gaps** with cheaper paths than ~37 calls/tag: whole-table ports
   and foundation unlocks, not just per-tag grinding.

Hard constraints (verified, not assumptions):

- **One host**: 10 cores, 32 GB RAM, 162 GB free disk. 22 worktrees already cost 62 GB.
- **One shared API budget**: `governor_calls_per_minute = 30` fleet-wide (flock bucket,
  `_governor_locked` at `model_fix_loop.py:712`). 429 storms already observed at ~20
  workers. Fleet throughput ceiling = calls/min ÷ calls-per-landed-tag, **independent
  of worker count** once the bucket saturates (~30–45 in-flight calls).
- **One human**: tag-fix PR latency 16–23 h; 29 verdicts ever (62% approval). Human
  attention is the scarcest resource and must be spent only where machines cannot decide.
- **ExifTool Perl is the only ground truth**. The generated `oxidex-tags-*` YAML has
  no PrintConvs and known duplicate-ID defects; it is scaffolding, never an oracle.
- **The fleet never stops**: every rollout phase must coexist with running workers,
  except one explicitly declared worker-drain cutover (Phase 3).
- Gap mass is module-shaped, not format-shaped: top 20 ExifTool `.pm` modules = 82.7%
  of all 4363 gaps; Canon.pm alone spans three of today's format workers.

## 1. Current state: shared vs siloed, and the confirmed races

Shared today (all under `OXIDEX_HOME=~/.oxidex/logs/`, worktree-independent):

| Store | Locking | Confirmed defect |
|---|---|---|
| `rate-governor.json` | flock (only store with real locking) | none |
| `model-fix-tag-state.json` (226 KB) | **none** | plain `write_text` (`save_tag_state`, `model_fix_loop.py:2666`): torn read → `load_tag_state` (:2656) returns `{}` → next save wipes all 96 entries; whole-dict lost updates every load→save window; `run_tag_loop` blacklist-exhaustion path saves `state = {}` erasing **every** format (fleet runs without `--blacklist-full`); claims never heartbeated, `claim_stale_seconds=1800` < realistic attempt |
| `format-memory/<FMT>.md` | none | `summarize_format_memory` (:1418) read → minutes-long model call → blind `write_text`; also silently failing (bare `except`), XMP.md is 10 KB of duplicated 429 noise fed back into prompts |
| `sweep-review-history.jsonl` / `landed-tags.log` | append-only (safe) | filtered **same-format-only** at prompt build; landed set re-read only at round start; no tombstone for reverts |
| `model-fix-diffs/`, `model-fix-requests/` | append manifests | second-resolution filenames with **no worker id** — same-second cross-worker overwrites of the only durable attempt record |
| `/tmp/tagcmp-<FMT>.json` | none | fixed per-format path; two same-format processes (live right now: duplicate CR2 workers) clobber each other's recheck — `tag_still_open` can judge worker A by worker B's build |

Siloed today: `KNOWN_PITFALLS` frozen in source (`model_fix_loop.py:1342`, 4 of 11
rejection lessons, propagates only at round-boundary `checkout -B` from main);
format-memory keyed per container format (a Canon.pm lesson learned on JPEG never
reaches the CR2/DNG workers); sweep reviews filtered to same format.

Pipeline defects: reviewer sees **only tag names + the diff** (`build_review_prompt`
:1725) — no exiftool values, no Perl; the live 4096-token prompt cap head-truncates
away exactly the learning sections (sweep reviews, memory, attempts); double emission
escapes the gap-count recheck (`extra_in_oxidex` is computed but unused); round-end
merge is a barrier serialized behind the slowest worker (round 4's commits died in the
00:40 restart); local main accumulated 10 sweep-merge commits then had to be
hard-reset to origin (squash-merge divergence), leaving `model-fix-gif` 103 commits
ahead as a dirty-dup time bomb; two dispatcher instances and orphaned duplicate CR2
workers are running concurrently with zero mutual exclusion.

## 2. The knowledge layer

### K1 — `lessons.jsonl`: one append-only event ledger

New file `~/.oxidex/logs/lessons.jsonl` + `append_lesson(event: dict)` in
`model_fix_loop.py`. **Atomicity contract** (stricter than the PIPE_BUF hand-wave the
critiques shot down): open with `os.open(path, O_APPEND|O_CREAT|O_WRONLY)`, one
`os.write` per line, line clamped to **2000 bytes**; readers skip malformed lines
(never degrade to `{}`); the file is never rotated or rewritten (readers seek to a
tail window, see K3).

Schema: `{ts, worker, format, module, table, tag_key, event, reason, evidence,
checklist_id, fingerprint_scoped, fingerprint_generic}`. Event enum:
`build_failed | gap_not_closed | wrong_value | test_regressed | duplicate |
review_rejected | critique | fixed | machine_accepted | human_accepted |
human_rejected | structural | infra`. 429/timeout reasons are logged as
`event=infra` and **excluded from every knowledge query** — this kills the XMP.md
noise-loop class at the source. Two fingerprints (resolves the dead-multiplier
critique): `fingerprint_scoped = sha1(event, module, checklist_id or norm-reason)`
and `fingerprint_generic = sha1(event, checklist_id or norm-reason)` — the generic
one is what makes "same mistake in Canon.pm and Nikon.pm" clusterable.

Writers (exact call sites): `critique_and_continue` after each failed round;
`fix_gap` on recheck failure (`wrong_value` with `{exiftool_value, oxidex_value}`),
duplicate, review rejection (carrying the checklist id, §6), and `fixed`;
`log_sweep_review.py` mirrors every human verdict (`--lesson "free text"` optional
flag for the generalizable takeaway); the squad merger and overlord sweep (§4) log
machine verdicts. `append_format_memory_note` (:1390) is **retired** — worker memory
notes become lesson events, so no worker ever appends to a file the distiller
rewrites (resolves the distiller-vs-appender race by construction).

### K2 — `GLOBAL-PITFALLS.md` replaces the code constant

`~/.oxidex/logs/knowledge/GLOBAL-PITFALLS.md`, read by new `load_global_pitfalls()`
at every `build_prompt` (fresh read, falls back to the `KNOWN_PITFALLS` constant when
missing — tests stay hermetic). Because it lives in shared OXIDEX_HOME and is read at
prompt-build time, a new lesson reaches all workers on their **next prompt** — zero
script redeploys, zero round-boundary git propagation.

Curation discipline (resolves the unbounded-uncurated critique): hard cap **3000
chars / 12 bullets**; written only by the distiller (K3) or a human, always via
tempfile + `os.replace`, previous version copied to `knowledge/history/`. Raw
`--lesson` text goes to lessons.jsonl, never straight into this file. Seeded at
rollout with all 11 human-rejection lessons: the 4 existing bullets plus — match
exact tag IDs/table indices against the Perl shown; copy PrintConv strings
byte-for-byte, never paraphrase; `rg` the tag name and edit the existing reachable
emitter (a second emission path escapes the gap count); never invent a fixture that
asserts your own output; verify against `exiftool` text output, not `-j`.

### K3 — Module-keyed playbooks + a **deterministic** distiller

`~/.oxidex/logs/knowledge/modules/<Module>.md` (Canon.md serves JPEG+CR2+DNG workers;
Exif.md serves nine). Written **only** by `scripts/distill_lessons.py`; workers only
read (at `build_prompt`, replacing `load_format_memory`; format-name fallback key
when module attribution is ambiguous).

The distiller is deliberately dumber than the critiqued design (resolves both the
over-engineering and the 429-starvation critiques): **no model calls**. It groups
non-infra events by `(module, checklist_id | event)`, counts occurrences and distinct
modules via `fingerprint_generic`, and renders newest-first bullets with counts,
capped 4000 chars/file: `"wrong_value ×7 (Canon.pm, Minolta.pm): PrintConv strings
must match Perl byte-for-byte — last: JPEG:MakerNotes:AELButton 2026-07-24"`. Any
event class with ≥3 occurrences across ≥2 modules is promoted as a candidate bullet
for GLOBAL-PITFALLS.md — written only if the file's content hash changes.

Singleton discipline (resolves orphan-lock-holder): lock file
`knowledge/distiller.lock` carries `{pid, script_git_sha, heartbeat_ts}`; a launcher
finding a stale heartbeat (>10 min) or a script-sha mismatch SIGTERMs the holder and
takes over. Runs every 15 min from the dispatcher loop (also cron-safe). Cursor file
advances **only past newline-terminated lines and only after outputs are replaced**;
re-processing is idempotent by event line hash. `summarize_format_memory` (:1418) and
its call site are deleted; the 22 existing `format-memory/*.md` files are distilled
once (dropping 429 noise) into the new playbooks and archived.

### K4 — Sweep reviews go global; verdict classes

`load_recent_sweep_reviews` (:1459) drops the same-format filter: up to 4 same-module
entries + up to 4 most-recent **global rejections** (rejections generalize; all 29
verdicts fit in a prompt). New verdict vocabulary in `sweep-review-history.jsonl`:
`human_accepted | human_rejected | machine_accepted | machine_rejected | reverted`.
Machine entries (auto-logged by merger/sweep, §M6) are deduped by
`(patch_id, reason)` so a re-polled failure cannot flood the window and evict human
verdicts (resolves the eviction critique), and prompt selection always prefers human
entries over machine ones. `machine_accepted` is **never** presented as
human-equivalent training signal (resolves the poisoned-signal critique).

### K5 — Reviewer evidence plumbing (the checklist itself is §6)

`build_review_prompt` (:1725) gains, threaded from `fix_gap` where all of it is in
scope:

1. **`perl_block`** — the same Perl reference snippets the fixer saw.
2. **`live_evidence`** — NOT from the comparison JSON (whose `matched_tags` carries
   no values — resolves the unimplementable-recheck-evidence critique): `fix_gap`
   shells out to `exiftool` and the oxidex CLI on the gap's `source_file` for just
   the target tags and renders `exiftool=<v> oxidex=<v> (post-fix)` per tag.
3. **`emission_scan`** — `rg -n` for each tag name scoped to the **format's parser
   subtree** (e.g. `src/parsers/tiff/makernotes/`), plus the pre/post occurrence
   counts from `detect_duplicate_tag_insertion` (:628) machinery — not a repo-wide
   grep that fills with other manufacturers' hits.
4. The C1–C5 checklist (§6) with a mandatory `UNVERIFIABLE:<id>` outcome: when the
   relevant Perl table doesn't fit the reviewer budget, the reviewer must say so, and
   `UNVERIFIABLE` on C1/C2 routes the commit to the human judgment queue instead of
   silently passing (resolves the vacuous-checklist critique).

Reviewer prompt budget is independent of the fixer cap: `reviewer_max_prompt_tokens
= 8192`.

## 3. Fleet topology: squads, job tiers, claims

**Verdict on the "whole fleet on one format" proposal (requirement 2): adapted,
not adopted.** Pointing all N workers at JPEG and sharding by *tag* is rejected:
JPEG's 3012 gaps concentrate in ~25 manufacturer `.pm` modules whose Rust emitters
are shared files (`canon.rs`, `nikon.rs`, registry const blocks), so tag-sharded
workers would collide on the same emitter regions — exactly the merge pain
requirement 2 forbids — and makernote knowledge clusters by manufacturer module,
not by container format, so tag-neighbors sharded arbitrarily learn nothing from
each other. Module squads deliver the intended effect anyway: 13 of the 14 squads
own JPEG gaps (see the "of which JPEG" column in S2, reconciling to 3012), so most
of the fleet *is* working JPEG simultaneously — but sharded along module
boundaries, where each shared emitter file has exactly one owning squad and
squad-mates share a playbook (K3) without ever sharing a worktree.

### S1 — Gap-attribution index

New `scripts/attribute_gaps.py`: maps every gap in `comparison.json` to its ExifTool
`.pm` module **and `%table` name** by indexing quoted tag names across the Perl lib
(resolved exactly as `resolve_exiftool_perl_lib_dir` does), using the per-bucket
priority lists validated in the gap census. Output
`~/.oxidex/logs/gap-attribution.json` (written tempfile + `os.replace`):
`{tag_key: {module, table, squad, formats[], sample_dirs[]}}` + a squads summary.
Regenerated by the dispatcher once per round after its full comparison. Known
few-percent name-collision noise is acceptable because attribution is **advisory
routing** everywhere (claims, memory keys, warn-only ownership) and **never a gate**
— except T3 table membership, which is parsed from the Perl `%table` source itself,
not the name index.

`find_tag_gaps.py` changes: `run_format_comparison` (:168) gains `out_suffix` and
`sample_dirs` parameters. **Every recheck writes
`/tmp/tagcmp-<FMT>-<worker_id>.json`** — worker-unique, killing the shared-fixed-path
race for good (workers, mergers with suffix `<squad>-staging`, and the sweep with
`sweep` are all isolated). `sample_dirs` lets a Canon-squad JPEG recheck scan Canon/
+ relevant root files (~300 files, ~5 s) instead of the 4085-file corpus (~57 s).
Scoped rechecks are regression-blind outside the shard **by design**; the
compensating full-corpus checks live at staging-batch and sweep time (§M2, §M4).

### S2 — Squad table and worker identity

Squads own ExifTool modules and therefore **all container formats those modules
serve** — the shared Rust emitter files get exactly one owning squad, which is what
keeps merges structurally conflict-free (the observed pattern already: 4 concurrent
worker commits, 4 disjoint files). Manifest `scripts/squads.toml` (checked in,
warn-only — see YAGNI for why enforcement is deferred):

| Squad | Modules | Formats | Gaps | of which JPEG |
|---|---|---|---|---|
| canon | Canon, CanonCustom, CanonRaw, CanonVRD, CR3-QuickTime | JPEG, CR2, CR3, DNG | 917 | 527 |
| nikon | Nikon, NikonCustom, NikonSettings, NikonCapture | JPEG, NEF | 613 | 497 |
| sony-minolta | Sony, Minolta, MinoltaRaw | JPEG, MRW | 518 | 443 |
| xmp | XMP.pm/XMP2.pl | JPEG, XMP, DNG, PDF, PSD, CR2 | 382 | 236 |
| exif-core | Exif.pm IFD wiring | 9 formats | 284 | 36 |
| olympus | Olympus | JPEG | 231 | 231 |
| pentax-samsung | Pentax, Samsung.pm | JPEG | 215 | 215 |
| panasonic-leica | Panasonic, PanasonicRaw | JPEG, RW2 | 183 | 107 |
| mobile | Google, GoPro, Apple, DJI, Qualcomm | JPEG | 185 | 185 |
| thermal | FLIR, InfiRay | JPEG | 158 | 158 |
| sigma-c2pa | Sigma, SigmaRaw, Jpeg2000/JUMBF | X3F, JPEG | 167 | 85 |
| ps-docs | Photoshop, IPTC, PhotoMechanic, FotoStation, PDF.pm | PSD, PDF, JPEG | 138 | 56 |
| standards-appn | ICC_Profile, JPEG.pm APPn, APP12, Meta, MPF | JPEG, NEF | 135 | 110 |
| tail | FlashPix/OLE, Kodak/Sanyo/Ricoh/Casio, FujiFilm, small formats | rest | 221 | ~126 |

JPEG reconciliation (this is where requirement (e)'s "decompose the 3012" lives):
the JPEG column sums to **exactly 3012** (527+497+443+236+36+231+215+107+185+158+
85+56+110+126). Component derivations from the census: canon 527 = Canon.pm 285 +
CanonCustom 164 + CanonVRD 43 + CanonRaw.pm 35; nikon 497 = Nikon 223 +
NikonCustom 236 + NikonSettings 38; sony-minolta 443 = Sony 344 + Minolta-JPEG 99;
pentax-samsung 215 = Pentax 194 (incl. Samsung-dir rebadges) + Samsung.pm 21;
panasonic-leica 107 = Panasonic-JPEG 90 + PanasonicRaw-JPEG 17; mobile 185 =
Google 58 + GoPro 48 + Apple 33 + DJI 32 + Qualcomm 14; thermal 158 = FLIR 77 +
InfiRay 81; sigma-c2pa 85 = JUMBF/C2PA 74 + Sigma.pm-JPEG 11; ps-docs 56 =
Photoshop 22 + IPTC 17 + FotoStation 10 + PhotoMechanic 7; standards-appn 110 =
ICC 21 + JPEG.pm-APPn 33 + APP12 35 + Meta 16 + MPF 5; tail ~126 = FujiFilm 36 +
FlashPix-FPXR 15 + Sanyo 15 + Ricoh 12 + Kodak ~9 + Casio 7 + XML 7 + misc APPn
~25. The Gaps column sums to 4347 vs the census 4363 — the ~16 residual is
dynamic-named/unattributable tags; `gap-attribution.json` re-derives both columns
every round, so these are snapshot values (2026-07-24), not config.

Slot allocation is a **formula, not three hardcoded tables** (the 100-worker table
was fiction on this host): each round the dispatcher assigns
`slots_i = max(1, round(total_slots × open_gaps_i / Σ open_gaps))` from live
attribution counts; a dried-up squad's slots flow to the largest backlog
automatically, exactly like `discover_formats` drops empty formats today.
Reconciliation rule (the `max(1,·)` floor overshoots with 14 squads): while
`Σ slots_i > total_slots`, decrement the squad with the lowest gaps-per-slot among
those holding >1 slot; while under, increment the highest. Worked examples against
the census snapshot above, so operators can sanity-check dispatcher output (±1 slot
of these rows for the same counts; larger divergence = attribution drift or a
formula bug):

**`total_slots = 20`** (today's host):

| canon | nikon | sony-minolta | xmp | other 10 squads | Σ |
|---|---|---|---|---|---|
| 4 | 3 | 2 | 1\* | 1 each | 20 |

\*raw rounding gives xmp 2 (Σ = 21); xmp has the lowest gaps-per-slot of the
multi-slot squads (382/2 = 191 vs sony-minolta's 259), so it yields the slot.

**`lanes = 50`** (Phase 5 gate; logical claim lanes over ≤24 process slots, §5):

| canon | nikon | sony-minolta | xmp | exif-core | olympus | tail | pentax-samsung | panasonic-leica | mobile | thermal | sigma-c2pa | ps-docs | standards-appn | Σ |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 11 | 7 | 6 | 4 | 3 | 3 | 3 | 2 | 2 | 2 | 2 | 2 | 2 | 1\* | 50 |

\*raw rounding gives standards-appn 2 (Σ = 51); it has the lowest gaps-per-lane
(135/2 = 68), so it yields.

**100**: no table — deferred to the second host (§5 sketch, §8).

**Worker identity at >1 worker per squad** (this was unspecified in the critiqued
design and is now explicit): worktree `~/.oxidex/worktrees/parallel-fix/
model-fix-<squad>-<n>`, branch `model-fix-parallel-<squad>-<n>`, `--worker-id <squad>-<n>`.
The worker id flows into: the claim record, `/tmp/tagcmp-*` suffix,
`tag-fix-prompts/process-<id>-prompt.log`, and **all `model-fix-diffs/` /
`model-fix-requests/` filenames** (`{ts}-{worker}-{phase}-…`), ending the same-second
artifact overwrites. Worktrees are per-slot and reused across rounds
(`create_worktree` reuse path unchanged).

### S3 — Job tiers

- **T1 tag / T2 sibling-cluster**: unchanged (F2 machinery, `max_cluster_tags = 6`).
  No jobs file for these — workers derive gaps fresh and claim, as today (the
  T1/T2 planner from the critiqued design is YAGNI'd; see §8).
- **T3 TABLE-PORT**: one job = one ExifTool `%table` (e.g.
  `%Image::ExifTool::Canon::CameraSettings`). Prompt embeds (i) the **full** Perl
  table source, (ii) the `oxidex-tags-*` YAML id→name skeleton as scaffolding
  (never as value ground truth), (iii) the registry precedent
  (`src/parsers/tiff/makernotes/registries/canon.rs` `CAMERA_SETTINGS_SCHEMA`).
  Config: `[table_job] max_prompt_tokens = 16384, max_repair_rounds = 8, model =`
  strongest in the phase-routing table. **Acceptance gate (three clauses — the
  critiqued two-clause gate shipped wrong values)**: (a) ≥80% of members close with
  exact values; (b) zero regressions of previously-matching tags; (c) **zero members
  present-but-wrong** — a member that cannot be made exact is removed from emission
  (commented out with a `// TODO(tag_key)` marker) before commit, never emitted
  wrong. Absent members re-queue as T1, and `plan_table_jobs` excludes member tags
  of claimed/pending tables from T1 selection (cross-tier exclusion, §S4). Cost
  target: 5–10 calls/tag vs ~37.
- **T4 FOUNDATION-UNLOCK**: seven jobs pre-seeded from the census (do not wait for
  auto-escalation, which is deferred): CR3 QuickTime-box→Canon CMT dispatch (~207
  gaps), FLIR FFF record parser (90 incl. DJI thermal), JUMBF/C2PA box walker (74),
  PDF embedded EXIF/IPTC/Photoshop streams (77), Sony-A100→Minolta.pm dispatch (70),
  NikonCapture NX-edit walker (61), OLE/FlashPix property sets (52; the dispatch arm
  already exists at `src/core/format_dispatch.rs`). ~630 gaps behind 7 jobs. Seeds
  live in `scripts/foundation_jobs.toml` (checked in, human-curated); routed to the
  strongest model with table-job budgets. Tags behind a pending foundation get
  `held_by_foundation=<job>` in tag-state, **cleared automatically** when the
  foundation job's commit lands on origin/main (the janitor in §M5 checks and
  clears — resolves the no-release-path critique), after which members re-enter as
  T1/T3 work.

### S4 — Claim protocol (safe at N workers per squad)

All in `model_fix_loop.py`, mirroring the proven `_governor_locked` pattern:

1. `_state_locked(path, mutate_fn)` — flock on `model-fix-tag-state.lock`; every
   read-modify-write in `run_tag_loop` (claim, result, blacklist) moves inside a
   `mutate_fn` closure. Critical sections are milliseconds; even 100 claimants is
   <10 acquisitions/sec.
2. `save_tag_state` (:2666) → `NamedTemporaryFile` in-dir + `os.replace`.
   `load_tag_state` (:2656) distinguishes missing-file from parse-failure; parse
   failure logs and **raises** rather than returning `{}` (no more silent wipe).
3. Blacklist-exhaustion reset (`run_tag_loop` `state = {}` path): scoped by an
   **explicit key list** captured at claim-filter time — never a prefix match
   (squads are not key prefixes; resolves the unimplementable-prefix critique) and
   never `{}`.
4. **Time-based heartbeat** (not call-based — call-based starves in governor queues
   and cargo runs, exactly when claims must survive): a daemon thread touches
   `claimed_at` every `heartbeat_seconds = 60` while an attempt is in flight.
   `claim_stale_seconds` rises to 7200; with a heartbeat, stale now means *dead*,
   not *slow*.
5. Claim keys are **canonical**: T1/T2 claims resolve through the attribution index
   to `(module, table)` and the claim record stores both `tag_key` and
   `canonical_table`; a T3 claim on a table excludes T1 claims on its members and
   vice versa (checked inside the same `mutate_fn` — resolves the tier-aliasing
   critique).

### S5 — Worker lifecycle

`max_tags_per_process` **stays 1**: one job, one commit, one process exit. This is a
deliberate simplification versus the critiqued multi-job worker — it keeps the
`refresh_worktree` (`:582`) ff-only contract valid (a worker never holds local
commits while refreshing), avoids the mid-round jobs-file store entirely, and makes
"commit == job == independently mergeable unit" an invariant the merge pipeline
relies on. Workers branch from **their squad's staging branch** (`--base-ref
squad/<squad>`), not local main. Between processes the dispatcher's `checkout -B`
re-anchors to the squad branch tip — but only after the consume handshake (§M2)
confirms the previous commit was consumed.

## 4. Merge pipeline

### M1 — Commit contract: evidence trailers + `validate_fix_commit.py`

`git_commit` (`model_fix_loop.py:577`) gains a trailers dict, emitted via repeated
`-m` blocks. From data already in scope in `fix_gap`: `Format:`, one `Tag: <family>:<name>`
per cluster member, `Sample: <source_file>`, `Exiftool-Value:`
(sanitized: single line, ≤200 chars), `Oxidex-Value:` (post-fix, from the live
re-extraction of K5), `Perl-Ref: <pm-file>:<line>`, `Verified: recheck-pass
gaps=<before>-><after>`, `Worker: <worker_id>`, `Table: <canonical_table>`.
Patch-id is recorded by the merger at consume time.

`scripts/validate_fix_commit.py` re-verifies mechanically — and, unlike the
critiqued single-sample version, it is built to catch rejection class (a):

- **Multi-sample**: runs the tag-comparison binary against **every** cached sample
  file whose exiftool output contains the target tag (a grep over
  `/tmp/oxidex-exiftool-cache` per-file outputs), not just the one `Sample:`.
- **PrintConv-vs-Perl**: extracts every quoted value string the diff adds inside
  map-like structures (`=>` tables, `const_decoder` arrays) and requires each to
  appear **byte-identical** in the `Perl-Ref` module source. Any miss → the commit
  is flagged to the human queue, never auto-shipped.
- Asserts the diff touches only files consistent with the squad's ownership globs
  (warn-only: violations flag, not revert), and computes/records patch-id.

### M2 — Squad staging branches + merger daemon (`scripts/squad_merge_loop.py`)

One staging worktree per squad (`~/.oxidex/worktrees/squad-staging/<squad>`, branch
`squad/<squad>` **cut from origin/main**). One merger process per squad; lock file
carries `{pid, script_git_sha, heartbeat}` with stale/mismatch takeover (same rule
as the distiller), and `stop_parallel_fix.py` learns to reap merger pgids.

Poll every `merger_poll_seconds = 120` for new commits on the squad's worker
branches. Per candidate commit, **on a detached HEAD** (resolves the
published-ref-not-append-only critique — the public `squad/<squad>` ref only ever
fast-forwards to fully validated states):

1. Per-commit patch-id novelty vs `origin/main ∪ squad/<squad>` (`git cherry`),
   consulting the **quarantine ledger** (`~/.oxidex/logs/quarantine.jsonl`,
   patch-id-keyed) — quarantined patch-ids are skipped without retry (resolves the
   bisect-livelock and no-rejection-memory critiques; every merger rejection is
   recorded there with reason + backoff, and auto-logged `machine_rejected` once,
   deduped).
2. `validate_fix_commit.py`.
3. `git cherry-pick` onto the detached head; targeted `cargo test --lib <fmt>`
   (`cargo_test_targeted`, :841).
4. **Pre/post multiset recheck** (M3) scoped to the commit's sample dirs.
5. On success: fast-forward `squad/<squad>` to the validated head; append a green
   stamp to `~/.oxidex/logs/squad-status/<squad>.json` via tempfile+`os.replace`
   (readers treat parse failure as "no news"); mark the worker branch head
   **consumed** in the same file.

**Consume handshake** (resolves the lost-commit-window critique):
`create_worktree` in `parallel_model_fix_loop.py` refuses to `checkout -B` a worker
branch whose head SHA is not recorded consumed/rejected in squad-status; it skips
the reset that round and the merger picks the commit up on its next poll.

**Batch full-format check**: every `merger_batch_commits = 10` commits or
`merger_batch_seconds = 900`, the merger runs a **full-corpus** comparison for each
of its formats on the staging tip vs the last batch baseline — this is the
compensating control for sample-scoped per-commit rechecks (cross-shard regressions
and within-squad combination effects, including merge-created double emission,
surface here at latest, *before* the overlord ever sees the branch — resolves the
merge-created-double-emission critique at the correct tier). Delta assertion is the
**inequality** form (M4).

The dispatcher's own `merge_branch` phase (`parallel_model_fix_loop.py:171`) is
retired behind `--legacy-merge` once all squads have mergers. Workspace tests
disappear from the per-commit path entirely; they run at sweep time (M4) and are
additionally gated by the build semaphore (§5).

### M3 — Deterministic double-emission gate

`find_tag_gaps.py` compare output gains two fields:

- `duplicate_emissions`: any tag key oxidex emits more than once for one sample
  file (no baseline needed).
- `new_oxidex_only`: oxidex-only keys present post-run but absent pre-run, where
  **pre and post are always the same worktree, same tree modulo the change under
  test** — worker recheck diffs pre-fix vs post-fix in the worker's worktree;
  merger diffs staging-tip-before vs after the cherry-pick; sweep diffs
  origin/main-base vs merged sweep branch. Lineage is intra-worktree by
  construction, so the fragile cross-tier baseline problem from the critiques never
  arises.

Consumed at three checkpoints: `fix_gap`'s recheck (extends `tag_still_open`,
:2558 — cheapest catch), merger validation + batch check, overlord post-merge
recheck. This finally gives rejection class (b) — 3 of 11 human rejections, the
class that escapes the gap count — a deterministic gate that works for clusters and
registry/dynamic-name emitters (where the literal-string backstop
`detect_duplicate_tag_insertion` is blind).

### M4 — Overlord runbook (`scripts/overlord_sweep.py`)

The human-driven session runs this every ~5 min (honest cadence: 5 min for
non-JPEG-touching sweeps, up to ~15 min when JPEG rechecks and a workspace test are
in the window — the claim that everything fits in 5 minutes is not made).

MECHANICAL: (1) preflight (singleton locks healthy, no stale merger heartbeats);
(2) collect green stamps newer than `sweep-state.json` (atomic read; parse failure
= no news); (3) cut a **fresh** `sweep/tags-<date>-<n>` from origin/main — never
reuse (ends the pr40 force-push spiral); (4) merge each green squad head **at its
stamped SHA** (append-only refs make this well-defined); a cross-squad conflict is
a hard error → flag, since ownership makes it near-impossible; (5) post-merge
semantic recheck for the union of touched formats: **assert measured gap delta ≥
Σ Verified trailers, `duplicate_emissions` empty, `new_oxidex_only` empty** —
over-delivery (an Exif.pm fix closing gaps in nine formats, a table port closing
unenumerated siblings) is logged as bonus yield, never a failure (resolves the
strict-equality-drops-good-work critique); an unexplained **negative** component or
any duplicate emission triggers mechanical bisection by squad subsets, the
offending patch-id goes to the quarantine ledger (consulted by mergers too, so it
cannot re-enter — no livelock), and a `machine_rejected` entry is logged once;
(6) one `cargo test --workspace` on the final sweep branch; (7)
`log_sweep_review.py --from-range` auto-logs `machine_accepted` for merged commits;
(8) `gh pr create`, PR body carrying the per-tag evidence table (Tag /
Exiftool-Value / Oxidex-Value / Sample count) generated from trailers — this is
what turns human PR review from O(re-derive) into O(scan) and is the concrete
answer to the human-bandwidth critique.

JUDGMENT queue (held in staging, dashboard shows queue age; drain-SLA before any
scale-up): commits that (a) add/edit **value-map / PrintConv-like tables** —
always, this is the class-(a) firewall: wrong-value fixes can never auto-ship,
which the critiqued design got wrong; (b) add a new file or new top-level
`fn parse_`; (c) touch tests/fixtures; (d) carry reviewer `UNVERIFIABLE` outcomes
or a PrintConv-vs-Perl mismatch from `validate_fix_commit`; (e) touch commons files
(`src/core/format_dispatch.rs`, `src/parsers/tiff/makernotes/shared/`). Everything
else ships mechanically with `machine_accepted` — but the sweep-review store keeps
the verdict classes distinct (K4), so the learning loop is not poisoned.

### M5 — Branch lifecycle and git hygiene

- **Local `main` is a mirror**: the dispatcher fast-forwards it from origin/main at
  round start and **never merges into it**. All integration state lives on squad
  staging branches and sweep branches. There is nothing on local main to lose, so
  the reset-loses-work critique dissolves structurally. Invariant, enforced in
  code: no ref reset may discard commits not contained in origin/main, an open
  sweep PR, or a squad staging branch.
- **Squad branch re-cut rule** (the lifecycle the critiques found missing): when a
  squad's PR merges to origin/main, the merger re-cuts `squad/<squad>` from
  origin/main and re-cherry-picks only patch-id-novel unconsumed commits. Patch-id
  is recorded in trailers-adjacent stores (squad-status, quarantine,
  sweep-review, landed-tags) from day one because cherry-pick and rebase-merge
  rewrite SHAs.
- **PR merge method**: rebase-merge for tag-fix PRs (`gh pr merge --rebase`) so each
  one-concern commit is individually revertable on origin/main. If repo policy
  forces squash: cap PRs at one squad and ≤10 commits, embed all Tag:/Patch-Id
  trailers in the squash body — required, not optional. Infra PRs keep squash.
- **Tombstones**: `log_sweep_review.py --revert <sha>` appends
  `REVERTED <FORMAT>:<tag>` to `landed-tags.log`; `load_landed_tags` (:2638) honors
  tombstones so reverted tags re-enter the pool (fixes the permanent-suppression
  hazard from the 00:57:41 backfill).
- **Janitor** (dispatcher round step): auto-reset any worktree whose merge-base
  with origin/main is >3 days old **and** whose commits are all consumed or
  quarantined (retires model-fix-gif-class time bombs without destroying unswept
  work); clear `held_by_foundation` flags whose foundation commit is on
  origin/main; rotate `dashboard.log` (186 MB today) and prune
  `model-fix-requests/` beyond 14 days.

### M6 — Auto-generated sweep-review entries

`log_sweep_review.py --from-commit/--from-range` parses trailers
(`git interpret-trailers --parse`) and writes entries without human typing; verdict
class per M4; dedup by `(patch_id, reason)`. Human verdicts remain a strict
superset signal: the human reviews the judgment queue and any sampled fraction of
machine-accepted commits, and their verdicts overwrite machine ones for the same
patch-id.

## 5. Scaling path 20 → 50 → 100, with the binding constraints stated

Throughput identity: `tags/hour = OK-calls/hour ÷ calls-per-landed-tag × P(land)`.
The configured 30 calls/min is a ceiling, not the supply: *measured* sustained
supply is ~70–75 OK calls/hour (24 h average — the provider TPM binds below the
config), which at 37 calls/tag is ~45–48 tags/day *best case* — **worker count
does not appear in this equation once the bucket saturates (~30–45 in-flight
calls ≈ 24–32 busy workers)**. The critiqued designs' 100-worker
tables were fiction on this host (10 cores / 32 GB / 162 GB free; 50 concurrent
cargo builds would swap and out-disk). The honest levers, in order:

1. **calls-per-landed-tag** (T3/T4: 5–10 vs 37) — the only 4–8× lever available
   today. This is how "scale" actually happens.
2. **P(land)** (knowledge layer + evidence pipeline lifting the 62% approval and
   the ~3% merge yield).
3. **governor budget** — raise `governor_calls_per_minute` only when the provider
   account allows; measured `consecutive_limited` in `rate-governor.json` gates
   this, and the dispatcher shrinks spawn counts when it rises.
4. **worker slots — last**. Slots stay ≤ `max_parallel = 24` on this host. A
   **build semaphore** (flock, `build_semaphore = 5`) caps concurrent
   `cargo build/test` across all workers+mergers so 10 cores are never
   oversubscribed by linking.

Definitions: "50 workers" = 50 **logical claim lanes** multiplexed over ≤24 process
slots (a slot picks up the next claim when one finishes) — pure claim-layer
arithmetic, no new processes. Gate to enable: governor ≥60/min granted AND measured
T3 calls-per-tag <10 AND ≥100 GB disk headroom. "100 workers" = a second host;
**deferred** (§8) — nothing in this spec depends on it.

**Second-host sketch (and an honesty note on "worker-count-invariant"):** every
coordination primitive in this spec is single-host — flock on
`model-fix-tag-state.lock` / `rate-governor.lock` / merger and distiller locks,
`O_APPEND` writes to `lessons.jsonl`, tempfile+`os.replace` in a local OXIDEX_HOME
— and neither flock nor O_APPEND atomicity holds over NFS, so a second host must
**never** mount the first host's OXIDEX_HOME. The layers are therefore
worker-count-invariant only *within one host*. Across hosts the mechanism is
**partition, don't share**: split `squads.toml` by host (e.g. host A:
canon+nikon+sony-minolta+xmp ≈ 56% of gaps; host B: the rest), each host running
its own complete stack — OXIDEX_HOME, claim store, mergers, distiller, build
semaphore — over its disjoint squad set. Exactly three cross-host interfaces:
(a) **git via origin** (worker/squad branches push; the sweep host pulls and runs
the single `overlord_sweep.py`); (b) a **statically split governor budget** — each
host's bucket is configured to a fixed share of the granted account rate, no
shared bucket, so a 60/min grant becomes 30+30; (c) **knowledge sync** — a cron
rsync of `GLOBAL-PITFALLS.md` + module playbooks plus append-merge of
`lessons.jsonl` deltas (minutes of latency is fine for advisory content; nothing
gates on it). Claim contention across hosts is impossible by construction
(disjoint squads), so no distributed locks are needed; a small coordination
service (one sqlite/HTTP process) is warranted only if a single squad ever has to
span hosts (§9 Q4).

### Quantified path to all ~4363 (requirement 5, assembled)

Bucketing the census by cheapest viable tier. The per-squad plain-vs-encrypted-vs-
dynamic `%table` classification comes from `gap-attribution.json`; the T3 share is
the dominant unknown and is carried at ±30% until the Phase 4 canon pilot measures
real calls-per-tag:

| Path | Gaps (est.) | Calls/gap (est.) | Total calls |
|---|---|---|---|
| T4 foundations — the 7 seeded jobs (S3) | ~630 | ~3–5 (≈200–400/job) | ~2–3k |
| T3 table ports — plain fixed `%tables`: manufacturer makernotes, Canon/NikonCustom bitfield tables, XMP namespaces, IRB/APPn records | ~2,400 ±30% | 5–10 | ~12–24k |
| T1/T2 residual grind — singletons, small formats, post-T3 stragglers | ~1,000 | 20–37 (knowledge layer targets ≤25) | ~20–37k |
| Deferred (§8) — encrypted Sony 0x94xx / Nikon blocks (~250), dynamic names (~50), misc | ~330 | — | excluded |
| **Total scheduled** | **~4,030** | — | **~34–64k** |

Against the all-T1 baseline (4,363 × 37 ≈ **161k** successful calls), the tier mix
is a 2.5–4.7× reduction. Duration at the *measured* supply (~70–75 OK calls/hr,
governor at 30/min): **~19–36 days** of continuous operation vs ~90 days all-T1.
If governor 60/min is granted and the provider sustains ~2× (~150 OK calls/hr):
**~10–18 days**. Two caveats stated rather than hidden: (1) T4 jobs *unlock*
rather than close some of their gaps (CR3 dispatch opens Canon CMT tables that
then ride T3), so the T4/T3 boundary is fuzzy — the table double-counts nothing
but the split will shift as foundations land; (2) at the implied 100–200
closures/day, the **human PR gate becomes the binding constraint** even with
O(scan) evidence tables (M4) — open question 3 is the release valve. Re-estimate
this whole table after Phase 4 from measured per-tier calls-per-landed-tag.

KPI: calls-per-landed-tag per tier, from `manifest.log` + `cache-stats.log`,
surfaced on `watch_parallel_fix.py`.

## 6. Prompt changes

### Fixer

- `KNOWN_PITFALLS` constant becomes the fallback for `load_global_pitfalls()` (K2);
  file seeded with the full 11-lesson taxonomy.
- `max_prompt_tokens` 4096 → **8192** (not the critiqued 12288 — TPM economics), with
  **graduated per-section truncation** replacing head-keeping: reserved budgets —
  learning block (pitfalls excerpt + module playbook + sweep reviews + lessons
  tail) `learning_budget_tokens = 1200`; gap lists and Perl NOTES keep existing
  caps; parser-file section is elastic **with a floor** `parser_floor_tokens =
  2000` (never squeezed to zero — resolves the inverted-starvation critique);
  overflow shrinks sections in priority order (attempts → samples → neighbor →
  perl_block excess → parser above floor) rather than deleting the tail. Section
  order otherwise unchanged (the cross-worker cache-prefix reorder is YAGNI'd).
- Lessons tail: `build_prompt` reads the last `lessons_tail_kb = 256` KB of
  lessons.jsonl via seek (bounded, no full scan), filters same-module then
  same-format non-infra events, max 8 entries.

### Reviewer checklist (maps 1:1 to the human rejection taxonomy)

`C1` exact tag ID / table index matches the Perl shown (class a). `C2` PrintConv
strings byte-identical, not paraphrased (class a). `C3` the diff edits an emitter
found in the emission scan rather than adding a second path (class b). `C4` any
new/changed test asserts values from a real corpus sample, not a fixture invented
in this diff (class c). `C5` no hardcoded sample-specific values. Reply stays
`APPROVE` / `REJECT: <Cn> <reason>` / `UNVERIFIABLE: <Cn>` — `extract_review_verdict`
gains the third verdict; C1/C2 UNVERIFIABLE → human queue (K5). Checklist ids flow
into lessons.jsonl as the clusterable fingerprint key.

## 7. Rollout (the fleet never stops, except the declared Phase 3 worker drain)

**Phase 0 — stop the bleeding (hours; independent small PRs).** Kill the duplicate
dispatcher; singleton flock + orphan reaping (persist spawned pgids, killpg
leftovers at startup); `_state_locked` + `os.replace` + scoped blacklist reset +
heartbeat thread (S4 items 1–4); worker-id in tagcmp paths and artifact filenames;
delete model-fix-gif and the stale sweep worktrees; local-main-is-a-mirror rule.
*Acceptance:* concurrent-writer test on tag-state passes; two dispatchers cannot
start; a synthetic torn tag-state file raises instead of wiping; no `/tmp/tagcmp`
filename collisions in a 20-worker round.

**Phase 1 — knowledge + evidence, topology-independent (1–2 days).** lessons.jsonl
writers; GLOBAL-PITFALLS.md seeded + loader; global sweep-review tiers + verdict
classes; graduated truncation + 8192 budget; reviewer enrichment (K5/§6);
double-emission fields in `find_tag_gaps.py` + `tag_still_open` multiset check;
commit trailers + `validate_fix_commit.py` + tombstones; deterministic distiller +
one-time format-memory migration; delete `summarize_format_memory`. The per-format
fleet keeps running throughout. *Acceptance:* tag-fix-prompt previews show the
learning block surviving on large-context tags; a seeded double-emission fixture
fails the recheck; trailer round-trip test green; distiller lock takeover test.

**Phase 2 — attribution + merger pilot (days).** `attribute_gaps.py` +
`squads.toml` (warn-only); merger daemons piloted on **formats wholly owned by one
squad** (NEF→nikon, CR2/DNG→canon, MRW→sony-minolta, RW2→panasonic-leica), consuming
the existing per-format worker branches — no commit→squad router needed because
routing is by whole-format ownership during the pilot; JPEG stays on legacy
round-end merge (resolves the pilot-strands-JPEG-commits critique). Consume
handshake wired into `create_worktree`. *Acceptance:* one day with zero lost worker
commits (every head consumed or quarantined), green stamps flowing, quarantine
dedup verified.

**Phase 3 — declared cutover (~1 day).** Stop spawning per-format workers; drain
in-flight (≤1 round); legacy merge consumes remaining branches once; delete
per-format worktrees (frees ~55 GB); dispatcher switches to squad spawning
(`--squad`, per-slot worktrees, `--base-ref squad/<squad>`); mergers for all
squads; overlord switches to `overlord_sweep.py`; `--legacy-merge` retired. This is
the one stop-the-world moment and it is for workers only — mergers, overlord, and
the knowledge layer run through it. *Acceptance:* one full cycle where each active
squad lands ≥1 commit through staging → sweep → PR; JPEG-touching squads produce
commits where the single JPEG worker produced ~1/round.

**Phase 4 — job tiers (week 2).** Seed the 7 foundation jobs; pilot T3 on the
canon squad with the three-clause gate; measure calls-per-landed-tag per tier and
human approval vs the 62% baseline before fleet-wide T3. *Acceptance:* one table
port lands through the full pipeline with zero present-but-wrong members; KPI
dashboards live.

**Phase 5 — scale gate.** Slots → 24 as build-semaphore headroom allows. 50 lanes
only when: governor ≥60/min granted, T3 <10 calls/tag measured, ≥100 GB disk,
judgment-queue drain SLA met. 100: deferred.

## 8. Explicitly cut / deferred (YAGNI, with reasons)

- **Model-based distillation, scoring formulas, recency half-lives, log_lesson.py
  CLI** — 29 verdicts and 16 landed tags do not justify the apparatus; the
  deterministic distiller + curated pitfalls file captures ~80% of the value with
  none of the 429-starvation or poisoning surface. Revisit when lessons.jsonl
  exceeds ~5k non-infra events.
- **T1/T2 jobs.jsonl planner** — workers already derive gaps fresh and claim
  atomically; a second store would go stale mid-round and need reconciliation.
  Only T3/T4 get explicit (human-curated, checked-in) job seeds.
- **fail_reason auto-escalation (≥5-same-reason rule)** — the seven foundation jobs
  were found by hand in an afternoon; the rule costs an extra critique call per
  failure for marginal yield. Defer until the seeds are exhausted.
- **Ownership enforcement + commons file leases** — attribution noise would
  auto-revert correct fixes, and the lease TTL reproduces the stale-claim bug at
  file granularity on the highest-blast-radius files. Ownership stays warn-only
  (violations flag to the judgment queue); commons edits are always
  human-reviewed. Revisit with measured misattribution <1%.
- **Multi-job worker processes** — breaks the ff-only refresh contract and drags in
  the jobs-file store; one-job-per-process is the invariant the merge pipeline
  leans on.
- **Per-commit cherry-pick splitting of mixed novel/dup branches** — moot under
  one-commit-per-job; the branch-level gate stays until it demonstrably misfires.
- **Cross-worker cache-prefix prompt reorder** — ~10% cold-call input savings,
  invalidated by every pitfalls write; not worth the migration diff while 429s and
  calls-per-tag dominate cost.
- **100-worker scale on this host** — physically impossible (10 cores / 32 GB /
  162 GB free vs ~280–400 GB of worktrees and dozens of concurrent link steps);
  deferred to a second host using the host-partitioned design sketched in §5
  (disjoint squad sets per host, statically split governor budget, git + rsync as
  the only cross-host channels). The claim/merge/knowledge layers are
  worker-count-invariant only *within* one host — flock, O_APPEND, and local
  OXIDEX_HOME semantics do not survive NFS, so no store in this spec may ever be
  pointed at a network filesystem. *(This is the explicit disposition of
  high-severity critiques D1-H4, D2-H4, D3-H6.)*
- **Encrypted-block ports (Sony 0x94xx, Nikon encrypted lens/shot info)** — 
  legitimate ~37-calls/tag T1 grind or dedicated future design; not blocking.
- Carried from the previous spec: hex-window byte slicing; two-tags-in-flight
  pipelining.

## 9. Open questions

1. Can the provider account's rate limit support `governor_calls_per_minute = 60`?
   This gates the 50-lane step; if not, T3/T4 efficiency is the only lever.
2. Is rebase-merge acceptable repo policy for tag-fix PRs? If squash is forced, the
   trailer-in-squash-body fallback becomes mandatory (M5).
3. Should machine-accepted structural commits ever bypass the PR-level human
   entirely once machine-vs-human verdict agreement is measured ≥95% over ≥100
   commits? (Today: no; everything still rides a human-merged PR.)
4. Second host for the 100-lane step — buy, borrow, or wait for T3/T4 to make it
   unnecessary? If a single squad (canon) ever outgrows one host, the
   partition-by-squad sketch (§5) no longer suffices and a small coordination
   service must replace flock — deliberately not designed here.
5. Should the existing `landed-tags.log` backfill (16 entries stamped 00:57:41) be
   re-verified against origin/main content now, or is the janitor's ongoing check
   sufficient?
6. PrintConv-vs-Perl byte-checking assumes value strings are extractable from the
   diff by pattern; how often do computed PrintConvs (sprintf/expressions) force
   the human queue, and is that rate acceptable?

## Testing (hermetic, house style)

- `_state_locked`: two processes claiming under contention in a tempdir (multiprocessing);
  torn-file read raises; scoped reset deletes exactly the captured key list;
  heartbeat thread with fake clock keeps a slow claim alive past 1800 s.
- `tag_still_open` multiset truth table incl. `duplicate_emissions` and
  `new_oxidex_only` cases; pre/post lineage fixture (same worktree, injected
  comparison results).
- Trailer build/parse round-trip against a real `git` tempdir repo;
  `validate_fix_commit` with a fake comparison runner: multi-sample pass,
  PrintConv byte-mismatch → flagged, ownership violation → warn.
- Merger: validate-on-detached-HEAD then ff (published ref never contains a
  dropped commit); quarantine skip; rejection-ledger dedup + backoff; consume
  handshake blocks `checkout -B` on unconsumed heads.
- Overlord: delta inequality (over-delivery passes, negative component fails,
  duplicate emission fails); fresh-branch naming; sweep-state atomic read
  (parse failure = no news).
- Knowledge: `load_global_pitfalls` fallback; O_APPEND lesson writes with a
  malformed-line-tolerant reader; distiller cursor advances only past complete
  lines and only after `os.replace`; lock takeover on stale heartbeat / sha
  mismatch; two-tier sweep-review selection prefers human verdicts.
- Prompt: graduated-truncation property test — section budgets sum ≤ cap, parser
  floor honored, learning block present at 8192 with a 60 KB parser file.
- Full discover suite green throughout (466 baseline).

---

## Appendix A — High-severity critique traceability

| # | Critique (design-lens) | Disposition |
|---|---|---|
| D1-H1 | Module-keyed memory recreates the distiller-vs-appender lost-update race | **Resolved** (K1/K3): workers never append to distiller-rewritten files; notes become ledger events; playbooks are distiller-only outputs |
| D1-H2 / D1-H6, D2-H2 | `/tmp/tagcmp-<FMT>` fixed-path collision corrupts `tag_still_open` under same-format workers | **Resolved** (S1, Phase 0): per-worker `out_suffix` on every recheck; merger/sweep suffixes isolated |
| D1-H3 | Double emission has no deterministic gate; `extra_in_oxidex` unused | **Resolved** (M3): `duplicate_emissions` + intra-worktree pre/post `new_oxidex_only` at three checkpoints |
| D1-H4, D2-H4, D3-H6 | Scale posture ignores governor/TPM/host CPU/disk | **Resolved** (§5) + **deferred** (100 workers, §8 with reason): worker count demoted below calls-per-tag, governor, P(land); build semaphore; slots ≤24; lanes-vs-slots split |
| D1-H5, D3-H9 | Requirement 5 (cheaper paths) unanswered; strict delta==sum rejects cross-format fixes | **Resolved** (S3, M4): T3/T4 tiers + 7 seeded foundations; delta assertion is ≥ with over-delivery as bonus |
| D2-H1 | Round-start hard reset of local main loses unswept work | **Resolved** (M5): local main is a mirror, never an integration point; explicit no-discard invariant; squad re-cut rule after PR merge |
| D2-H3 | Merge-created double emission invisible between squad merge and human | **Resolved** (M2/M3): merger validates the merged tree per commit + full-format batch check before green-stamping |
| D2-H5 | Human review bandwidth unscaled; T3 multiplies burden | **Resolved** (M1/M4): multi-sample + PrintConv-vs-Perl mechanical checks, per-tag evidence tables in PR bodies (review becomes O(scan)), value-map commits always human-flagged, machine/human verdict classes kept distinct. Residual: open question 3 |
| D3-H1 / D3-H8 | Class-(a) wrong values pass all mechanical gates and flag heuristics; auto-accept poisons training signal | **Resolved** (M1, M4, K4): value-map-touching commits can never auto-ship; PrintConv byte-check; multi-sample validation; `machine_accepted` is a distinct verdict class |
| D3-H2 | Tag-state races armed by >1 worker per format | **Resolved** (S4, Phase 0): flock + `os.replace` + raise-on-torn-read + time-based heartbeat + canonical claim keys with cross-tier exclusion |
| D3-H3 | Worker identity unspecified at >1 worker/format (shared worktree/branch, unsuffixed tagcmp) | **Resolved** (S2): per-slot worktrees/branches/worker-ids threaded through claims, tagcmp paths, prompt logs, artifact filenames |
| D3-H4 | Staging branch not append-only; drops rewrite published refs | **Resolved** (M2): validate on detached HEAD, ff-only publication, quarantine ledger instead of ref rewrites |
| D3-H5 | Lost-commit window between async poll and round-boundary `checkout -B` | **Resolved** (M2/S5): consume handshake in `create_worktree` |
| D3-H7 | Migration incoherence (per-format workers vs squad topology; pilot strands JPEG) | **Resolved** (Phase 2/3): pilot only on wholly-owned formats; declared stop-the-world worker drain at cutover; no mixed claim-space window |
