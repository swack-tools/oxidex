# ExifTool Coverage Loop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A saved Workflow script, invocable by name (`exiftool-coverage-loop`), that finds oxidex/ExifTool tag-coverage gaps across all formats, fans out one subagent per format to fix and verify them in isolation, merges verified fixes back with a regression safety net, and repeats until two consecutive rounds close zero gaps.

**Architecture:** One JS Workflow script at `.claude/workflows/exiftool-coverage-loop.js` built incrementally: Find stage (one agent running `just compare-exiftool-full`, returning a schema-validated comparison report), Fix stage (one `isolation: 'worktree'` agent per format with gaps, fanned out via `parallel()`), Merge stage (a plain sequential `for` loop — not `parallel()` — merging verified branches back one at a time with a `cargo test --workspace` safety net), wrapped in a `while` loop tracking consecutive "dry" rounds (zero gaps closed).

**Tech Stack:** Workflow tool (JS orchestration script), the existing `src/bin/tag-comparison` Rust binary, `just compare-exiftool-full`, `cargo test --workspace`, git worktrees.

## Global Constraints

- Local commits only. No push, no PR, ever, from any agent this plan spawns.
- Gap scope is `missing_in_oxidex` + `value_differences` only. Never chase `extra_in_oxidex`.
- A round is "dry" iff it closes zero gaps. Two consecutive dry rounds stops the workflow. This is *not* the generic new-vs-seen loop-until-dry template.
- Every commit that lands on the shared branch has passed `cargo test --workspace` twice: once in the fix-agent's isolated worktree, once again after the sequential merge.
- Fix-agents run with `isolation: 'worktree'` — never plain `agent()` — because related formats (JPEG/TIFF/DNG/CR2/NEF/ARW) share EXIF/IFD parser code and concurrent fix-agents would otherwise corrupt each other's working tree.
- The merge stage is a plain sequential `for` loop, never `parallel()` — merging is a git operation and concurrent merges would race.
- The Workflow tool's own 1000-agent lifetime cap is a backstop, not a bug to work around. Hitting it mid-run isn't a failure; the user re-invokes the named workflow to resume.
- Full design rationale lives in `docs/plans/specs/2026-07-19-exiftool-coverage-loop-design.md` — consult it for anything not covered by a task below.

---

## Task 1: Make `compare-exiftool-full`'s combined-samples directory persistent

Today, `just compare-exiftool-full` builds its combined ExifTool-test-corpus-plus-camera-samples directory at `/tmp/exiftool-combined-$$` and deletes it via a `trap cleanup EXIT` the moment the recipe finishes. That's fine for a one-shot developer invocation, but this plan's Find and Fix stages both need to point the `tag-comparison` binary at that directory *after* the recipe has already exited — the Find stage builds it, then per-format Fix-stage agents (running later, in separate worktrees) need to re-run `tag-comparison --format X` against the exact same samples without re-downloading or re-extracting anything. Deleting it on exit makes that impossible.

**Files:**
- Modify: `justfile:683-793` (the `compare-exiftool-full` recipe only — leave `compare-exiftool-full-update` at `justfile:805+`, which is CI-only, untouched)

**Interfaces:**
- Produces: a stable path `${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}/combined-samples` that persists across separate `just compare-exiftool-full` invocations and separate shell processes, and a stable exiftool binary path `${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}/exiftool/exiftool` (already the existing behavior — unchanged). Task 2 (Find stage) and Task 3 (Fix stage) both reference these two paths directly.

- [ ] **Step 1: Change `COMBINED_DIR` to a stable path under the cache dir**

In `justfile`, inside the `compare-exiftool-full` recipe, change:

```sh
    CACHE_DIR="${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}"
    EXIFTOOL_DIR="$CACHE_DIR/exiftool"
    COMBINED_DIR="/tmp/exiftool-combined-$$"
    GCS_BUCKET="https://storage.googleapis.com/oxidex-samples/exiftool"

    cleanup() {
        echo "🧹 Cleaning up temp files..."
        rm -rf "$COMBINED_DIR"
    }
    trap cleanup EXIT

    mkdir -p "$CACHE_DIR"
```

to:

```sh
    CACHE_DIR="${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}"
    EXIFTOOL_DIR="$CACHE_DIR/exiftool"
    # Persistent, not ephemeral: the coverage-loop workflow re-runs
    # tag-comparison directly against this same path from separate agent
    # invocations after this recipe has already exited, so it must survive
    # past this shell's lifetime (unlike the old `/tmp/exiftool-combined-$$`
    # + `trap cleanup EXIT`, which deleted it on exit).
    COMBINED_DIR="$CACHE_DIR/combined-samples"
    GCS_BUCKET="https://storage.googleapis.com/oxidex-samples/exiftool"

    mkdir -p "$CACHE_DIR"
```

(This removes the `cleanup()`/`trap` block entirely — nothing else in this recipe needs cleanup on exit.)

- [ ] **Step 2: Verify the recipe still runs and the directory survives**

Run:
```bash
EXIFTOOL_CACHE_DIR=/tmp/oxidex-coverage-loop-test just compare-exiftool-full
ls -d /tmp/oxidex-coverage-loop-test/combined-samples
```
Expected: the recipe completes with `✅ Comprehensive comparison complete!`, and the `ls` command finds the directory (proving it wasn't deleted on exit).

- [ ] **Step 3: Verify re-running reuses the cache (no re-download) and doesn't wipe the directory**

Run:
```bash
time EXIFTOOL_CACHE_DIR=/tmp/oxidex-coverage-loop-test just compare-exiftool-full
```
Expected: output includes `✓ Using cached ExifTool <version>` and `(cached)` next to each manufacturer, and the whole run completes noticeably faster than Step 2's first run (no fresh downloads). The directory from Step 2 is still present throughout (nothing deletes it mid-run).

- [ ] **Step 4: Clean up the test cache dir and commit**

```bash
rm -rf /tmp/oxidex-coverage-loop-test
git add justfile
git commit -m "build: persist compare-exiftool-full's combined-samples dir across runs

The coverage-loop workflow's Find and Fix stages both need to point
tag-comparison at the same combined samples directory from separate
agent invocations after the recipe that built it has already exited.
Move it from an ephemeral \$\$-tmp dir (deleted via trap on exit) to a
stable path under the existing cache dir."
```

---

## Task 2: Find stage — schema, prompt, and a scoped-down live validation

**Files:**
- Create: `.claude/workflows/exiftool-coverage-loop.js`

**Interfaces:**
- Produces: `COMPARISON_REPORT_SCHEMA` (JSON Schema mirroring `ComparisonReport`/`FormatComparison` from `src/bin/tag-comparison/models/mod.rs`), `CACHE_DIR` (string constant), `findGapsPrompt()` (returns a prompt string, no args). Task 3 consumes `COMPARISON_REPORT_SCHEMA` and `CACHE_DIR`.

- [ ] **Step 1: Write the script with just the meta block and Find stage**

```js
export const meta = {
  name: 'exiftool-coverage-loop',
  description: 'Find oxidex/ExifTool tag-coverage gaps and fix them in a forever loop, one subagent per format, until two rounds close nothing',
  phases: [
    { title: 'Find', detail: 'run tag-comparison against the ExifTool test corpus + samples' },
    { title: 'Fix', detail: 'one isolated-worktree agent per format with gaps' },
    { title: 'Merge', detail: 'sequential merge-back with a regression safety net' },
  ],
}

const CACHE_DIR = (args && args.cacheDir) || '/tmp/oxidex-exiftool-cache'

const COMPARISON_REPORT_SCHEMA = {
  type: 'object',
  properties: {
    overall_coverage: { type: 'number' },
    total_regressions: { type: 'number' },
    summary: { type: 'string' },
    by_format: {
      type: 'object',
      additionalProperties: {
        type: 'object',
        properties: {
          format: { type: 'string' },
          files_tested: { type: 'number' },
          coverage_percentage: { type: 'number' },
          total_exiftool_tags: { type: 'number' },
          missing_in_oxidex: {
            type: 'array',
            items: {
              type: 'object',
              properties: {
                name: { type: 'string' },
                family: { type: 'string' },
                value: { type: 'string' },
                tag_id: { type: ['string', 'null'] },
                source_file: { type: ['string', 'null'] },
              },
              required: ['name', 'family', 'value'],
            },
          },
          value_differences: {
            type: 'array',
            items: {
              type: 'object',
              properties: {
                tag_key: { type: 'string' },
                exiftool_value: { type: 'string' },
                oxidex_value: { type: 'string' },
                source_file: { type: 'string' },
              },
              required: ['tag_key', 'exiftool_value', 'oxidex_value', 'source_file'],
            },
          },
          regressions: { type: 'array', items: { type: 'string' } },
        },
        required: ['format', 'missing_in_oxidex', 'value_differences', 'regressions'],
      },
    },
  },
  required: ['by_format'],
}

function findGapsPrompt() {
  return `Run \`EXIFTOOL_CACHE_DIR=${CACHE_DIR} just compare-exiftool-full\` from the oxidex repository root. ` +
    `This builds the tag-comparison binary, downloads or reuses a cached ExifTool release plus its t/images ` +
    `test corpus and camera sample set, and writes comparison.json in the repo root. Read comparison.json and ` +
    `return its contents as your structured output verbatim: the by_format map keyed by format name, each ` +
    `with missing_in_oxidex, value_differences, and regressions. Do not modify or commit anything -- this is ` +
    `a read-only discovery step.`
}

phase('Find')
const report = await agent(findGapsPrompt(), {
  label: 'find-gaps',
  schema: COMPARISON_REPORT_SCHEMA,
})

log(`find stage: ${Object.keys(report.by_format || {}).length} formats in report`)
return report
```

- [ ] **Step 2: Run it and confirm the Find stage produces a well-formed report**

Invoke:
```
Workflow({ scriptPath: "<the path this was saved under>" })
```
(or re-paste the script inline the first time — either way, note the returned `runId` for later resume). This is a real, non-trivial invocation: it downloads ExifTool plus camera samples on first run (several minutes) and rebuilds `tag-comparison`.

Expected: the workflow completes, `log()` reports some number of formats greater than zero (JPEG/PNG/TIFF/PDF/MP4 at minimum, since those have local fixtures ExifTool's own `t/images` corpus will also cover many more), and the returned report validates against `COMPARISON_REPORT_SCHEMA` (the harness retries automatically on schema mismatch — if it fails after retries, read `<transcriptDir>/journal.jsonl` to see the agent's actual raw output and adjust the schema or prompt).

- [ ] **Step 3: Commit**

```bash
git add .claude/workflows/exiftool-coverage-loop.js
git commit -m "feat: add Find stage for exiftool-coverage-loop workflow

Runs just compare-exiftool-full and returns the resulting
comparison.json as a schema-validated structured object."
```

---

## Task 3: Fix stage — gap grouping, prompt, and a single-format live validation

**Files:**
- Modify: `.claude/workflows/exiftool-coverage-loop.js`

**Interfaces:**
- Consumes: `COMPARISON_REPORT_SCHEMA`, `CACHE_DIR`, `findGapsPrompt()`, `report` (Task 2).
- Produces: `FIX_RESULT_SCHEMA` (`{format, verified, gapsClosed, branch, summary}`), `fixPrompt(group)` (takes one `by_format` entry, returns a prompt string), `gapGroups` (array of `by_format` entries with at least one gap). Task 4 consumes `FIX_RESULT_SCHEMA` and the `verified` filtering pattern shown here.

- [ ] **Step 1: Add gap grouping, the Fix schema/prompt, and an optional test-scoping filter**

Insert after the `COMPARISON_REPORT_SCHEMA` constant and before `findGapsPrompt()`:

```js
const FIX_RESULT_SCHEMA = {
  type: 'object',
  properties: {
    format: { type: 'string' },
    verified: { type: 'boolean' },
    gapsClosed: { type: 'number' },
    branch: { type: ['string', 'null'] },
    summary: { type: 'string' },
  },
  required: ['format', 'verified', 'gapsClosed', 'summary'],
}

function fixPrompt(group) {
  const missingList = (group.missing_in_oxidex || [])
    .map(t => `  - ${t.family}:${t.name} = ${t.value} (sample: ${t.source_file || 'n/a'})`)
    .join('\n') || '  (none)'
  const diffList = (group.value_differences || [])
    .map(d => `  - ${d.tag_key}: exiftool="${d.exiftool_value}" oxidex="${d.oxidex_value}" (sample: ${d.source_file})`)
    .join('\n') || '  (none)'
  const gapCount = (group.missing_in_oxidex || []).length + (group.value_differences || []).length

  return `You are working in the oxidex repository (a Rust ExifTool reimplementation), on format "${group.format}". ` +
    `The tag-comparison tool (src/bin/tag-comparison) found these gaps against real ExifTool for this format:\n\n` +
    `Missing entirely (ExifTool extracts it, oxidex doesn't):\n${missingList}\n\n` +
    `Value differences (both extract it, values disagree):\n${diffList}\n\n` +
    `Find the relevant parser code yourself (grep src/parsers and src/core for "${group.format}" and the tag ` +
    `names above -- there is no static format-to-file map to hand you). Implement as many of these gaps as ` +
    `you can correctly verify in this pass. You do not need to close all of them -- large formats won't close ` +
    `in one round, and that's expected; whatever remains will resurface next round. For value differences, ` +
    `use judgment: only "fix" genuine bugs, not benign formatting differences. oxidex already runs its own ` +
    `format_for_exiftool/normalize_tag_family layer before this comparison runs, so gross PrintConv-vs-raw ` +
    `noise is already filtered out -- don't chase incidental ExifTool quirks that aren't part of the tag's ` +
    `documented semantics.\n\n` +
    `When you believe you've made progress:\n` +
    `1. cargo build --release --bin oxidex\n` +
    `2. cargo build --release --bin tag-comparison --features tag-comparison-binary\n` +
    `3. ./target/release/tag-comparison --exiftool ${CACHE_DIR}/exiftool/exiftool ` +
    `--samples ${CACHE_DIR}/combined-samples --format ${group.format} ` +
    `-o /tmp/tagcmp-${group.format}.json --markdown-dir /tmp/tagcmp-${group.format}-md\n` +
    `4. Read /tmp/tagcmp-${group.format}.json and confirm the combined ` +
    `missing_in_oxidex + value_differences count for "${group.format}" is strictly lower than ${gapCount} ` +
    `(the count you started with) and that regressions is empty.\n` +
    `5. cargo test --workspace\n\n` +
    `If both checks 4 and 5 pass, commit your changes on your current git branch with a descriptive commit ` +
    `message. Report: format ("${group.format}"), verified (true only if you committed after both checks ` +
    `passed), gapsClosed (the count reduction you confirmed in step 4), branch (run "git branch --show-current" ` +
    `and report that name if verified, else null), and a one-paragraph summary. If you cannot verify a real, ` +
    `regression-free improvement, do NOT commit -- run "git checkout -- ." and "git clean -fd" to leave your ` +
    `worktree clean, and report verified: false, gapsClosed: 0, branch: null.`
}

function gapGroupsFrom(report, onlyFormats) {
  return Object.values(report.by_format || {})
    .filter(f => (f.missing_in_oxidex && f.missing_in_oxidex.length) || (f.value_differences && f.value_differences.length))
    .filter(f => !onlyFormats || onlyFormats.includes(f.format))
}
```

Then replace the script's trailing `log(...)` / `return report` pair (from Task 2) with:

```js
const gapGroups = gapGroupsFrom(report, args && args.onlyFormats)
log(`found gaps in ${gapGroups.length} formats${args && args.onlyFormats ? ` (scoped to ${args.onlyFormats.join(', ')})` : ''}`)

if (gapGroups.length === 0) {
  log('no gaps to fix')
  return { report, fixResults: [] }
}

phase('Fix')
const fixResults = await parallel(gapGroups.map(g => () =>
  agent(fixPrompt(g), {
    label: `fix-${g.format}`,
    phase: 'Fix',
    isolation: 'worktree',
    schema: FIX_RESULT_SCHEMA,
  })
))

log(`${fixResults.filter(Boolean).filter(r => r.verified).length}/${fixResults.filter(Boolean).length} fix attempts verified`)
return { report, fixResults }
```

- [ ] **Step 2: Validate against a single, small, real format**

Pick a format from Task 2's report with a modest gap count (check the report's `by_format` entries for one with, say, under 20 combined `missing_in_oxidex`/`value_differences` -- avoid picking a huge one like QuickTime for this validation run). Invoke:
```
Workflow({ scriptPath: "<path>", args: { onlyFormats: ["<CHOSEN_FORMAT>"] } })
```
Expected: exactly one `fix-<FORMAT>` agent runs (visible in `/workflows`), and it returns a `FIX_RESULT_SCHEMA`-shaped object. Read `<transcriptDir>/journal.jsonl` to check what it actually did. Two acceptable outcomes:
- `verified: true` with a `branch` name -- confirm that branch exists (`git branch --list "<branch>"`) and has a real commit (`git log <branch> -1`).
- `verified: false` -- confirm the worktree was left clean (`git worktree list` shows no dirty state) rather than lingering with uncommitted changes.

- [ ] **Step 3: Commit**

```bash
git add .claude/workflows/exiftool-coverage-loop.js
git commit -m "feat: add Fix stage for exiftool-coverage-loop workflow

Groups comparison-report gaps by format and fans out one
isolation:'worktree' agent per format to implement and
self-verify a fix, gated on cargo test --workspace passing."
```

---

## Task 4: Merge stage, round loop, dry-stop logic, and full-loop validation

**Files:**
- Modify: `.claude/workflows/exiftool-coverage-loop.js`

**Interfaces:**
- Consumes: `FIX_RESULT_SCHEMA`, `fixResults`, `gapGroupsFrom()`, `findGapsPrompt()` (Tasks 2-3).
- Produces: `MERGE_RESULT_SCHEMA`, `mergePrompt(result)`, the outer `while` round loop with `dryRounds` tracking. This is the last task -- nothing downstream depends on this file further.

- [ ] **Step 1: Replace the script body with the full round loop**

Keep everything above `phase('Find')` from Tasks 2-3 (the `meta` block, `CACHE_DIR`, both schemas, `findGapsPrompt`, `fixPrompt`, `gapGroupsFrom`) unchanged. Add, right after `gapGroupsFrom`:

```js
const MERGE_RESULT_SCHEMA = {
  type: 'object',
  properties: {
    format: { type: 'string' },
    success: { type: 'boolean' },
    summary: { type: 'string' },
  },
  required: ['format', 'success', 'summary'],
}

function mergePrompt(result) {
  return `You are in the oxidex repository's main (non-worktree) working tree. A subagent working in git ` +
    `branch "${result.branch}" verified a coverage fix for format "${result.format}": ${result.summary}\n\n` +
    `1. Run: git merge --no-ff "${result.branch}" -m "merge: ${result.format} coverage fix"\n` +
    `   If it conflicts, run "git merge --abort", report success: false, and explain the conflict in summary.\n` +
    `2. If the merge succeeded, run: cargo test --workspace\n` +
    `3. If tests fail, run "git reset --hard HEAD~1" to undo only the merge commit you just made, report ` +
    `success: false, and explain the regression in summary.\n` +
    `4. If tests pass, the merge stands. Report success: true.\n\n` +
    `Report: format ("${result.format}"), success (bool), summary.`
}
```

Then replace the entire body from `phase('Find')` onward with:

```js
const MAX_DRY_ROUNDS = 2
let dryRounds = 0
let round = 0

while (dryRounds < MAX_DRY_ROUNDS) {
  round++
  log(`--- round ${round} (dry streak: ${dryRounds}/${MAX_DRY_ROUNDS}) ---`)

  phase('Find')
  const report = await agent(findGapsPrompt(), {
    label: `find-gaps-round-${round}`,
    schema: COMPARISON_REPORT_SCHEMA,
  })

  if (!report) {
    // agent() returns null on a terminal failure after retries (e.g. the
    // sandbox blocks the curl calls just-compare-exiftool-full needs, or
    // the report never validates against the schema). This must abort
    // loudly, not silently count as a dry round -- "couldn't check for
    // gaps" and "checked and found none" are different failure modes.
    throw new Error(`round ${round}: Find stage failed -- aborting without counting it as dry`)
  }

  const gapGroups = gapGroupsFrom(report, args && args.onlyFormats)
  log(`round ${round}: found gaps in ${gapGroups.length} formats`)

  if (gapGroups.length === 0) {
    dryRounds++
    continue
  }

  phase('Fix')
  const fixResults = await parallel(gapGroups.map(g => () =>
    agent(fixPrompt(g), {
      label: `fix-${g.format}`,
      phase: 'Fix',
      isolation: 'worktree',
      schema: FIX_RESULT_SCHEMA,
    })
  ))

  const verified = fixResults.filter(Boolean).filter(r => r.verified)
  log(`round ${round}: ${verified.length}/${fixResults.filter(Boolean).length} fix attempts verified`)

  phase('Merge')
  let closedCount = 0
  for (const r of verified) {
    const merged = await agent(mergePrompt(r), {
      label: `merge-${r.format}`,
      phase: 'Merge',
      schema: MERGE_RESULT_SCHEMA,
    })
    if (merged && merged.success) {
      closedCount += r.gapsClosed
    } else {
      log(`round ${round}: merge discarded for ${r.format} (${merged ? merged.summary : 'merge agent failed'})`)
    }
  }

  log(`round ${round}: closed ${closedCount} gaps`)
  dryRounds = closedCount === 0 ? dryRounds + 1 : 0
}

log(`stopped after ${round} rounds (${dryRounds} consecutive dry rounds)`)
return { rounds: round }
```

- [ ] **Step 2: Validate the loop mechanics on a small scope**

Invoke with a format already known (from Task 3's validation) to have few enough gaps that a fix-agent could plausibly close all of them in one round, forcing an observable dry transition:
```
Workflow({ scriptPath: "<path>", args: { onlyFormats: ["<SAME_FORMAT_AS_TASK_3>"] } })
```
Expected: at least one round runs Find → Fix → Merge; if that format's gaps get fully closed, the *next* round's Find should report zero gaps for it, `dryRounds` increments, and after two such rounds the workflow logs `stopped after N rounds (2 consecutive dry rounds)` and returns. If gaps only partially close, run it again (or just let it keep going) to observe multiple non-dry rounds before dryness. Either way, confirm via `git log --oneline -10` that each closed gap produced exactly one merge commit on the working branch, and that `cargo test --workspace` passes on `HEAD` afterward.

- [ ] **Step 3: Commit**

```bash
git add .claude/workflows/exiftool-coverage-loop.js
git commit -m "feat: add Merge stage and round loop to exiftool-coverage-loop workflow

Sequentially merges verified per-format fixes with a
cargo-test-workspace safety net, wraps Find/Fix/Merge in a
round loop, and stops after two consecutive rounds close zero
gaps."
```

- [ ] **Step 4: Full, unscoped integration run**

Invoke without `onlyFormats`:
```
Workflow({ name: "exiftool-coverage-loop" })
```
(If name-based resolution fails, check whether `.claude/workflows/` expects a different filename/structure than a flat `<name>.js` and adjust Task 2's file location accordingly, then retry.) Let at least one full round run across every format with gaps. Confirm: the round completes, the log summary reports a real `closedCount`, `git log` shows one merge commit per successfully-closed format, and `cargo test --workspace` passes on `HEAD`. This is the acceptance test for the whole plan -- everything upstream was validated in isolation, this is the first time all three stages run together at full scale.

---

## Notes for whoever executes this plan

- Each Fix/Merge/Find agent invocation is a real, possibly multi-minute operation (network downloads on first run, full `cargo build --release` and `cargo test --workspace` cycles). Don't expect pytest-speed iteration; budget real wall-clock time per validation step.
- If a Find-stage agent's sandbox blocks outbound network access (curl to exiftool.org/GitHub), `just compare-exiftool-full` will fail loudly with a curl error -- per the design spec's error-handling section, that should abort the round without incrementing `dryRounds`, not be silently swallowed as "no gaps found."
- Full rationale for every decision here (why merge is sequential, why `missing_in_oxidex`/`value_differences` but not `extra_in_oxidex`, why dry means "closed zero" not "discovered nothing new") is in `docs/plans/specs/2026-07-19-exiftool-coverage-loop-design.md`. If a step in this plan seems to contradict that document, the spec wins -- flag it rather than silently picking one.
