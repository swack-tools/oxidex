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
    // Ground truth for later stages to verify they're operating in the same
    // location as this (known-good, non-isolated) agent -- see mergePrompt,
    // which had a real incident where a merge agent wandered into the wrong
    // worktree/branch entirely and silently merged there instead.
    repo_path: { type: 'string' },
    repo_branch: { type: 'string' },
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
          // For large formats the relaying agent truncates these arrays
          // rather than writing thousands of entries into its own
          // structured-output call, and adds these markers when it does.
          // Consumers needing the complete list must re-derive it directly
          // (e.g. by re-running tag-comparison --format X themselves)
          // rather than trusting missing_in_oxidex/value_differences here
          // to be exhaustive.
          missing_in_oxidex_truncated: { type: 'boolean' },
          missing_in_oxidex_total_count: { type: 'number' },
          value_differences_truncated: { type: 'boolean' },
          value_differences_total_count: { type: 'number' },
        },
        required: ['format', 'missing_in_oxidex', 'value_differences', 'regressions'],
      },
    },
  },
  required: ['by_format', 'repo_path', 'repo_branch'],
}

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
  // The find-stage report's inline missing_in_oxidex/value_differences arrays may be
  // truncated for large formats (see COMPARISON_REPORT_SCHEMA's _truncated/_total_count
  // fields) -- they're illustrative here, not authoritative. The agent re-derives its own
  // complete, current gap list directly from tag-comparison before doing any work.
  const approxCount = (group.missing_in_oxidex_total_count ?? (group.missing_in_oxidex || []).length) +
    (group.value_differences_total_count ?? (group.value_differences || []).length)
  const sampleMissing = (group.missing_in_oxidex || []).slice(0, 10)
    .map(t => `  - ${t.family}:${t.name} = ${t.value}`).join('\n') || '  (none in the inline sample)'
  const sampleDiffs = (group.value_differences || []).slice(0, 10)
    .map(d => `  - ${d.tag_key}: exiftool="${d.exiftool_value}" oxidex="${d.oxidex_value}"`).join('\n') || '  (none in the inline sample)'

  return `You are working in the oxidex repository (a Rust ExifTool reimplementation), on format "${group.format}". ` +
    `The find stage reported roughly ${approxCount} coverage gaps for this format. A few examples (this inline ` +
    `list may be truncated for large formats, so treat it as illustrative, not authoritative):\n\n` +
    `Missing entirely, a sample:\n${sampleMissing}\n\n` +
    `Value differences, a sample:\n${sampleDiffs}\n\n` +
    `Before doing anything else, get your OWN complete, current gap list for this format:\n` +
    `1. cargo build --release --bin tag-comparison --features tag-comparison-binary (if not already built)\n` +
    `2. ./target/release/tag-comparison --exiftool ${CACHE_DIR}/exiftool/exiftool ` +
    `--samples ${CACHE_DIR}/combined-samples --format ${group.format} ` +
    `-o /tmp/tagcmp-${group.format}-start.json --markdown-dir /tmp/tagcmp-${group.format}-start-md\n` +
    `Read /tmp/tagcmp-${group.format}-start.json -- its missing_in_oxidex and value_differences arrays for ` +
    `"${group.format}" are the complete, authoritative gap list (this file comes straight from the comparison ` +
    `tool, not through an agent relay that may truncate it).\n\n` +
    `Find the relevant parser code yourself (grep src/parsers and src/core for "${group.format}" and tag names ` +
    `from that file -- there is no static format-to-file map to hand you). Implement as many of these gaps as ` +
    `you can correctly verify in this pass. You do not need to close all of them -- large formats won't close ` +
    `in one round, and that's expected; whatever remains will resurface next round. For value differences, ` +
    `use judgment: only "fix" genuine bugs, not benign formatting differences. oxidex already runs its own ` +
    `format_for_exiftool/normalize_tag_family layer before this comparison runs, so gross PrintConv-vs-raw ` +
    `noise is already filtered out -- don't chase incidental ExifTool quirks that aren't part of the tag's ` +
    `documented semantics.\n\n` +
    `When you believe you've made progress:\n` +
    `1. cargo build --release --bin oxidex\n` +
    `2. Re-run: ./target/release/tag-comparison --exiftool ${CACHE_DIR}/exiftool/exiftool ` +
    `--samples ${CACHE_DIR}/combined-samples --format ${group.format} ` +
    `-o /tmp/tagcmp-${group.format}-end.json --markdown-dir /tmp/tagcmp-${group.format}-end-md\n` +
    `3. Read /tmp/tagcmp-${group.format}-end.json and confirm the combined missing_in_oxidex + ` +
    `value_differences count for "${group.format}" is strictly lower than in the "-start.json" file from ` +
    `step 2 above, and that regressions is empty.\n` +
    `4. cargo test --workspace\n\n` +
    `If both checks pass, commit on your current git branch with a descriptive message. Report: format -- ` +
    `use exactly the string "${group.format}" verbatim, not a slug or description of your own choosing, since ` +
    `the caller matches on it programmatically -- verified (true only if you committed after both checks ` +
    `passed), gapsClosed (the count reduction between the start and end files you confirmed), branch (run ` +
    `"git branch --show-current" and report it if verified, else null), and a one-paragraph summary. If you ` +
    `cannot verify a real, regression-free improvement, do NOT commit -- run "git checkout -- ." and ` +
    `"git clean -fd" to leave your worktree clean, and report verified: false, gapsClosed: 0, branch: null.`
}

function gapGroupsFrom(report, onlyFormats) {
  return Object.values(report.by_format || {})
    .filter(f => (f.missing_in_oxidex && f.missing_in_oxidex.length) || (f.value_differences && f.value_differences.length))
    .filter(f => !onlyFormats || onlyFormats.includes(f.format))
}

const MERGE_RESULT_SCHEMA = {
  type: 'object',
  properties: {
    format: { type: 'string' },
    success: { type: 'boolean' },
    summary: { type: 'string' },
  },
  required: ['format', 'success', 'summary'],
}

function mergePrompt(result, repoPath, repoBranch) {
  // A past incident: a merge agent wandered into a fix-agent's worktree directory
  // (e.g. to inspect the branch) and ran the merge from there instead of returning to
  // the shared main tree -- it silently merged into whatever branch that OTHER
  // directory happened to have checked out, reported success:true, and the real
  // target branch never received the commit at all. Verify location explicitly
  // before touching git, and never cd elsewhere to inspect the branch.
  return `Before doing anything else, run \`pwd\` and \`git branch --show-current\` and confirm the output ` +
    `is EXACTLY "${repoPath}" and "${repoBranch}". If either does not match -- including if you are inside ` +
    `any other directory such as a worktree for the branch being merged -- \`cd "${repoPath}"\` first and ` +
    `re-verify before proceeding. Do not run any git command in this task from any other directory. To ` +
    `inspect the branch being merged, use "git log ${result.branch} --oneline" or ` +
    `"git diff ${repoBranch}..${result.branch}" -- both work without cd-ing anywhere.\n\n` +
    `Once confirmed, you are in the oxidex repository's main working tree on "${repoBranch}". A subagent ` +
    `working in git branch "${result.branch}" verified a coverage fix for format "${result.format}": ` +
    `${result.summary}\n\n` +
    `1. Run: git merge --no-ff "${result.branch}" -m "merge: ${result.format} coverage fix"\n` +
    `   If it conflicts, run "git merge --abort", report success: false, and explain the conflict in summary.\n` +
    `2. If the merge succeeded, run: cargo test --workspace\n` +
    `3. If tests fail: before running any reset command, re-verify \`pwd\` and \`git branch --show-current\` ` +
    `STILL match "${repoPath}" and "${repoBranch}" exactly -- a long test run is exactly the kind of gap ` +
    `where you might have changed directories in between. If they no longer match, STOP -- do not run ` +
    `"git reset --hard" from anywhere -- report success: false and explain the location mismatch instead of ` +
    `guessing. If they still match, run "git reset --hard HEAD~1" to undo only the merge commit you just ` +
    `made, report success: false, and explain the regression in summary.\n` +
    `4. If tests pass, the merge stands. Report success: true.\n\n` +
    `Report: format ("${result.format}"), success (bool), summary (include the pwd/branch you verified in ` +
    `step 1 as part of the summary, for auditability).`
}

function findGapsPrompt() {
  return `Run \`EXIFTOOL_CACHE_DIR=${CACHE_DIR} just compare-exiftool-full\` from the oxidex repository root. ` +
    `This builds the tag-comparison binary, downloads or reuses a cached ExifTool release plus its t/images ` +
    `test corpus and camera sample set, and writes comparison.json in the repo root. Read comparison.json and ` +
    `return its contents as your structured output verbatim: the by_format map keyed by format name, each ` +
    `with missing_in_oxidex, value_differences, and regressions. If a format's missing_in_oxidex or ` +
    `value_differences array is large (roughly 50+ entries), truncate it to a representative sample and set ` +
    `the corresponding missing_in_oxidex_truncated / value_differences_truncated to true and ` +
    `missing_in_oxidex_total_count / value_differences_total_count to the real total count -- don't silently ` +
    `truncate without those markers, since downstream consumers rely on them to know the list isn't ` +
    `exhaustive. Also run \`pwd\` and \`git branch --show-current\` and report them as repo_path and ` +
    `repo_branch -- later stages use these to verify they're operating in the same location as you, since a ` +
    `past incident had a merge agent wander into an unrelated worktree/branch and silently merge there ` +
    `instead of here. Do not modify or commit anything -- this is a read-only discovery step.`
}

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
    // agent() returns null on a terminal failure after retries (e.g. the sandbox
    // blocks the curl calls just-compare-exiftool-full needs, or the report never
    // validates against the schema). This must abort loudly, not silently count as
    // a dry round -- "couldn't check for gaps" and "checked and found none" are
    // different failure modes.
    throw new Error(`round ${round}: Find stage failed -- aborting without counting it as dry`)
  }

  const gapGroups = gapGroupsFrom(report, args && args.onlyFormats)
  log(`round ${round}: found gaps in ${gapGroups.length} formats`)

  if (gapGroups.length === 0) {
    dryRounds++
    continue
  }

  phase('Fix')
  const rawFixResults = await parallel(gapGroups.map(g => () =>
    agent(fixPrompt(g), {
      label: `fix-${g.format}`,
      phase: 'Fix',
      isolation: 'worktree',
      schema: FIX_RESULT_SCHEMA,
    })
  ))

  // See the format-enforcement note where this pattern was introduced (Task 3):
  // parallel() preserves input order, so gapGroups[i] is the ground truth for
  // rawFixResults[i]'s format regardless of what the agent self-reports.
  const fixResults = rawFixResults.map((r, i) => {
    if (!r) return r
    if (r.format !== gapGroups[i].format) {
      log(`round ${round}: fix-${gapGroups[i].format} agent reported format "${r.format}", overriding to match the input group`)
    }
    return { ...r, format: gapGroups[i].format }
  })

  const verified = fixResults.filter(Boolean).filter(r => r.verified)
  log(`round ${round}: ${verified.length}/${fixResults.filter(Boolean).length} fix attempts verified`)

  phase('Merge')
  let closedCount = 0
  for (const r of verified) {
    const merged = await agent(mergePrompt(r, report.repo_path, report.repo_branch), {
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
