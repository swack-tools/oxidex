# ExifTool Coverage Loop — Design

## Context

OxiDex already has two different tools that diff its own tag extraction
against real ExifTool output:

- `scripts/jpeg_tag_matrix.py` — JPEG-only, but rigorous: writes every
  ExifTool-writable JPEG tag with `exiftool`, reads it back with oxidex, then
  writes with oxidex and reads back with both tools. Full read *and* write
  parity, wired into CI (`.github/workflows/jpeg-tag-matrix.yml`) with a
  ratcheting baseline (`docs/reference/jpeg-tag-baseline.json`).
- `src/bin/tag-comparison` — general across all ~38 format parsers, but
  read-only: links the oxidex crate directly, extracts tags from whatever
  sample files exist for a format, extracts the same files with `exiftool`,
  and diffs the two tag sets per format. It runs today (`just
  compare-exiftool-full`) against ExifTool's own test corpus (`t/images`,
  spanning nearly every format ExifTool supports) plus a cached camera
  sample set (~7,100 files, 109 manufacturers), but has never been run in CI
  and has no committed baseline yet.

Separately, `src/bin/sync_tags.rs` (merged in commit `3965f80`, just prior to
this design) is **Phase A of a four-phase plan** (agreed with the user in an
earlier session) to make ExifTool's own `-listx` output the ground truth for
oxidex's tag database:

- **A** (done): standalone `exiftool -listx`-based sync tool, replacing the
  broken Perl-regex generator.
- **B**: CI workflow that invokes the sync tool and opens a PR on drift.
- **C**: rewire the write path to consume YAML-derived type data.
- **D**: *"generalize read/write coverage verification across all formats,
  wired into CI as a regression gate"* — i.e., turn `tag-comparison` into
  what `jpeg_tag_matrix.py` already is for JPEG, for every format.

This document does not implement Phase D itself. It specifies something
adjacent and one step further: an autonomous loop that uses `tag-comparison`
(as it exists today, read-only) to find coverage gaps and spins up
subagents to close them, verify the fix, and commit — repeatedly, across
however many rounds it takes to run out of fixable gaps. Phase D (turning
`tag-comparison` into a true write-verified harness per format) remains a
separate, future project; this loop works with the coverage tool as it
exists now.

## Goal

A saved Workflow script, invoked by name (`exiftool-coverage-loop`), that:

1. Finds coverage gaps between oxidex and ExifTool across all formats.
2. Fans out one subagent per format with gaps, each implementing and
   verifying a fix in an isolated git worktree.
3. Merges verified fixes back into the working branch, running the full
   test suite as a last-mile regression check.
4. Repeats ("rounds") until two consecutive rounds close zero gaps, then
   stops.

Each invocation runs entirely in the current git worktree, on the current
branch. It produces local commits only — no push, no PR. Re-invoking the
workflow later (e.g. after new commits land upstream, or to resume past the
Workflow tool's 1000-agent lifetime cap) picks up wherever the gap list
stands at that time.

## Non-goals

- Building a write-verification harness per format (Phase D, above) — the
  loop consumes `tag-comparison`'s existing read-only diff as-is.
- Auto-push or auto-PR. Commits land locally; pushing/opening a PR is a
  separate, manual step the user takes after reviewing the commit log.
- Chasing `extra_in_oxidex` (tags oxidex reports that ExifTool doesn't) —
  out of scope for a *coverage* loop; a false positive isn't a coverage gap.
- Fixing an entire format's backlog in a single round. Large formats (e.g.
  QuickTime, ~6,567 tags) will take many rounds; each round's fix-agent does
  as much as it can verify in one pass, and whatever remains resurfaces in
  the next round's find-stage automatically.

## Architecture

### Round structure

Each round runs three stages in sequence (`phase()` boundaries in the
Workflow script): **Find**, **Fix**, **Merge**. There is a hard barrier
between Find and Fix (the fix-agents need the complete per-format gap list
before starting), but Fix agents run concurrently with each other (subject
to the Workflow tool's own concurrency cap), and Merge runs as a plain
sequential loop after the Fix barrier.

```
while (dryRounds < 2) {
  phase('Find')
  report = agent(findGapsPrompt, {schema: COMPARISON_REPORT_SCHEMA})

  gapGroups = groupByFormat(report)   // plain JS, from missing_in_oxidex +
                                        // value_differences per format
  if (gapGroups.length === 0) { dryRounds++; continue }

  phase('Fix')
  fixResults = await parallel(gapGroups.map(g => () =>
    agent(fixPrompt(g), {isolation: 'worktree', schema: FIX_RESULT_SCHEMA})
  ))

  phase('Merge')
  closedCount = 0
  for (const r of fixResults.filter(Boolean).filter(r => r.verified)) {
    const merged = await agent(mergePrompt(r), {schema: MERGE_RESULT_SCHEMA})
    if (merged.success) closedCount += r.gapsClosed
    else log(`merge conflict/regression on ${r.format}, discarded`)
  }

  log(`round closed ${closedCount} gaps across ${fixResults.filter(r => r?.verified).length} formats`)
  dryRounds = closedCount === 0 ? dryRounds + 1 : 0
}
```

The dry-check is **"did this round close zero gaps,"** not the generic
loop-until-dry template's "did this round discover anything new." The gap
universe here is bounded and mostly static (it shrinks as fixes land, and
only grows if new parser code regresses or ExifTool's own tag list changes
between finds) — measuring "new vs. seen" would incorrectly call a round
dry just because it only worked through the top of a longer, already-known
backlog.

### Find stage

One agent runs `just compare-exiftool-full` (builds `tag-comparison`,
downloads/reuses a cached ExifTool release + `t/images` test corpus +
camera sample set, runs the comparison across all auto-detected formats)
and returns the resulting `comparison.json` report, validated against a
schema mirroring `ComparisonReport`/`FormatComparison`
(`src/bin/tag-comparison/models/mod.rs`): per format, `missing_in_oxidex`
(`TagInfo[]`: name, family, value, tag_id, source_file),
`value_differences` (`ValueDifference[]`: tag_key, exiftool_value,
oxidex_value, source_file), and `regressions` (baseline-ratchet regressions
vs. the last committed `docs/reference/comparison/baseline.json`, once one
exists).

`compare-exiftool-full` caches its ExifTool download and sample set in a
fixed directory (`EXIFTOOL_CACHE_DIR`, default `/tmp/oxidex-exiftool-cache`)
across runs, so repeated finds within one workflow invocation (and across
invocations, as long as `/tmp` survives) don't re-download gigabytes of
sample data every round.

### Fix stage

Grouping is by `format` (the top-level key in the comparison report). Every
format with at least one `missing_in_oxidex` or `value_differences` entry
gets one fix-agent, run with `isolation: 'worktree'` (parser code for
related formats — JPEG/TIFF/DNG/CR2/NEF/ARW all share EXIF/IFD parsing — can
overlap, so isolation prevents concurrent fix-agents from corrupting each
other's working tree).

Each fix-agent receives: the format name, its full `missing_in_oxidex` and
`value_differences` lists (tag names, expected values, sample file paths),
and instructions to:

1. Locate the relevant parser code itself (via grep/Explore — there's no
   static format→file map to hand it, and maintaining one would be brittle
   as parsers get refactored).
2. Implement as much of the backlog as it can verify in one pass. Large
   formats won't close in a single round; that's expected.
3. For `value_differences` specifically: use judgment on which are genuine
   bugs vs. benign formatting differences before "fixing" anything. The
   comparison tool already runs oxidex's own `format_for_exiftool`/
   `normalize_tag_family` layer before diffing (it links the oxidex crate
   directly rather than shelling out to the CLI), so gross PrintConv-vs-raw
   noise is already filtered — but rounding, date-format variants, and
   similar benign mismatches can still surface and must not be "fixed" by
   just making oxidex's formatter mimic incidental ExifTool quirks that
   aren't part of the documented tag semantics.
4. Verify: re-run `tag-comparison --format <FORMAT>` (reusing the cached
   sample set from the Find stage) and confirm `missing_in_oxidex` +
   `value_differences` count for that format decreased with zero new
   `regressions`, **and** run `cargo test --workspace` for regression
   safety.
5. Commit locally in its own worktree branch only if both checks pass.
   Otherwise, leave the worktree unchanged (git reset/clean before
   returning) so it auto-removes, and report `verified: false`.

Return schema includes: `format`, `verified` (bool), `gapsClosed` (count),
`branch` (worktree branch name, if verified), `summary`.

### Merge stage

A plain sequential `for` loop (not `parallel()`) over verified fix results —
sequential specifically to avoid concurrent-merge git races, since merging
is itself a git operation that must run inside an `agent()` call (Workflow
scripts have no direct filesystem/git access). For each: an agent merges the
worktree branch into the current working branch (fast-forward or a plain
merge commit) and re-runs `cargo test --workspace`. If that fails, the
agent rolls back the merge (`git reset --hard` to the pre-merge commit) and
reports failure — this is the safety net for fixes that are individually
correct but conflict semantically once combined (e.g. two formats' fixes
both touch shared IFD-parsing logic in incompatible ways).

### Stopping / resuming

Two consecutive dry rounds (zero gaps closed) stops the workflow. Separately,
the Workflow tool's own hard backstop — 1000 total agents across the
workflow's lifetime — bounds a single invocation regardless of dryness;
with roughly 38 formats and 3 agents per format-round (find is shared,
~1 fix + ~1 merge per format with gaps), a single invocation comfortably
covers a dozen-plus rounds before hitting that ceiling. Hitting it isn't a
failure — the user re-invokes the same named workflow to resume, and the
next Find stage picks up wherever the gap list currently stands.

## Error handling

If the Find stage's agent fails outright (network unavailable for the
ExifTool/sample download, `compare-exiftool-full` exits non-zero, or the
report fails schema validation after retries), the round aborts without
touching `dryRounds` and the workflow run ends with that error surfaced —
it does not count as a dry round, since "no gaps closed" and "couldn't even
check for gaps" are different failure modes and conflating them would let a
transient network blip masquerade as "coverage is complete."

## Testing / safety

- Every commit that lands on the working branch has already passed
  `cargo test --workspace` twice: once inside the fix-agent's isolated
  worktree, once again after merging into the shared branch.
- No push, no PR — the user reviews and pushes manually.
- Fix-agents that can't verify their own change leave no trace (worktree
  auto-cleaned) rather than committing something for the merge stage to
  discover is broken.
