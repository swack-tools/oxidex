export const meta = {
  name: 'exiftool-coverage-loop',
  description: 'Find oxidex/ExifTool tag-coverage gaps and fix them in a forever loop: shard large-gap formats across workers, PR each verified fix, dedup against open PRs, until dry',
  phases: [
    { title: 'Find', detail: 'run tag-comparison against the ExifTool test corpus + samples' },
    { title: 'Triage', detail: 'list open PRs from prior rounds to avoid duplicate work' },
    { title: 'Shard', detail: 'split large-gap formats into focused worker slices' },
    { title: 'Fix', detail: 'one isolated-worktree agent per slice, PRs its own verified fix' },
    { title: 'Audit', detail: 'review the run for process inefficiencies, PR the harness if warranted' },
  ],
}

const CACHE_DIR = (args && args.cacheDir) || '/tmp/oxidex-exiftool-cache'
const REPO_PATH = (args && args.repoPath) || '/home/allen/git/oxidex'
const REPO_SLUG = (args && args.repoSlug) || 'swack-tools/oxidex'
// GitHub rejects pushes whose commit author is a private, unverified email (GH007) --
// the repo owner's git config uses their private gmail, which triggers this on every
// push. We can't touch git config (global or repo) to fix it globally, so every commit
// a worker makes must override author/committer inline via `git -c` for that one
// command -- this is their public GitHub noreply address, safe to bake in.
const COMMIT_AUTHOR_NAME = (args && args.commitAuthorName) || 'swackhamer'
const COMMIT_AUTHOR_EMAIL = (args && args.commitAuthorEmail) || '619624+swackhamer@users.noreply.github.com'
const GIT_AUTHOR_OVERRIDE = `-c user.name="${COMMIT_AUTHOR_NAME}" -c user.email="${COMMIT_AUTHOR_EMAIL}"`
const SHARD_THRESHOLD = 25
const MAX_SHARDS = 6
const MAX_DRY_ROUNDS = 3
const MAX_ROUNDS = 25
const MAX_WORKITEMS_PER_ROUND = 50

const COMPARISON_REPORT_SCHEMA = {
  type: 'object',
  properties: {
    overall_coverage: { type: 'number' },
    total_regressions: { type: 'number' },
    summary: { type: 'string' },
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

function findGapsPrompt() {
  return `Run these steps in order from the oxidex repository at "${REPO_PATH}":\n` +
    `1. cd "${REPO_PATH}"\n` +
    `2. git checkout main && git pull --ff-only origin main -- this must succeed cleanly (fast-forward only); if it fails, STOP and report the failure in your summary rather than proceeding on a stale or diverged tree.\n` +
    `3. cat .exiftool-version -- confirm it reads "13.59". If it does not, STOP and report the mismatch; do not grade against a different pinned version.\n` +
    `4. EXIFTOOL_CACHE_DIR=${CACHE_DIR} just compare-exiftool-full -- this builds tag-comparison, downloads/reuses the cached pinned ExifTool 13.59 release plus its test corpus and camera samples, and writes comparison.json in the repo root.\n\n` +
    `Read comparison.json and return its contents as your structured output verbatim: the by_format map keyed by format name, each with missing_in_oxidex, value_differences, and regressions. If a format's missing_in_oxidex or value_differences array is large (roughly 50+ entries), truncate it to a representative sample and set the corresponding missing_in_oxidex_truncated / value_differences_truncated to true and missing_in_oxidex_total_count / value_differences_total_count to the real total count -- don't silently truncate without those markers. Also run \`pwd\` and \`git branch --show-current\` and report them as repo_path and repo_branch. Do not modify or commit anything -- this is a read-only discovery step (aside from the git pull in step 2).`
}

const OPEN_PRS_SCHEMA = {
  type: 'object',
  properties: {
    prs: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          number: { type: 'number' },
          headRefName: { type: 'string' },
          title: { type: 'string' },
          url: { type: 'string' },
        },
        required: ['number', 'headRefName', 'title', 'url'],
      },
    },
  },
  required: ['prs'],
}

function openPRsPrompt() {
  return `Run: gh pr list --repo ${REPO_SLUG} --state open --limit 200 --json number,headRefName,title,url\n` +
    `Parse the JSON output and return it as { prs: [...] }. This is read-only -- do not create, edit, or merge anything.`
}

function gapGroupsFrom(report, onlyFormats) {
  return Object.values(report.by_format || {})
    .filter(f => (f.missing_in_oxidex && f.missing_in_oxidex.length) || (f.value_differences && f.value_differences.length))
    .filter(f => !onlyFormats || onlyFormats.includes(f.format))
}

function totalGapCount(group) {
  return (group.missing_in_oxidex_total_count ?? (group.missing_in_oxidex || []).length) +
    (group.value_differences_total_count ?? (group.value_differences || []).length)
}

const SHARD_PLAN_SCHEMA = {
  type: 'object',
  properties: {
    format: { type: 'string' },
    shards: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          label: { type: 'string' },
          focusHint: { type: 'string' },
          estimatedGapCount: { type: 'number' },
        },
        required: ['label', 'focusHint', 'estimatedGapCount'],
      },
    },
  },
  required: ['format', 'shards'],
}

function shardPlanPrompt(group) {
  return `You are working in the oxidex repository at "${REPO_PATH}" (read-only planning, make no changes) on format "${group.format}", which the find stage reported has a large number of coverage gaps (roughly ${totalGapCount(group)}).\n\n` +
    `1. cd "${REPO_PATH}"\n` +
    `2. Build if needed: cargo build --release --bin tag-comparison --features tag-comparison-binary\n` +
    `3. Run: ./target/release/tag-comparison --exiftool ${CACHE_DIR}/exiftool/exiftool --samples ${CACHE_DIR}/combined-samples --format ${group.format} -o /tmp/tagcmp-${group.format}-shardplan.json --markdown-dir /tmp/tagcmp-${group.format}-shardplan-md\n` +
    `4. Read /tmp/tagcmp-${group.format}-shardplan.json -- its missing_in_oxidex and value_differences arrays for "${group.format}" are the complete, current gap list.\n\n` +
    `Partition this gap list into ${MAX_SHARDS <= 2 ? 2 : `2-${MAX_SHARDS}`} shards along natural boundaries that map to genuinely separable code regions -- typically by maker/vendor (grep the "family" and tag names against src/parsers to see which module owns them), by source_file when different camera samples clearly exercise different code paths, or by a shared root-cause bug affecting a cluster of tags (e.g. several tags in one sub-IFD/segment misreading the same field). Do NOT split by arbitrary alphabetical or count-based chunks -- a shard should be something one focused worker can plausibly understand and fix without touching the same code another shard's worker is touching, so two shards should virtually never edit the same file. Small formats or ones with a single obvious cause can get just 1-2 shards; only split as finely as the gap list's actual structure supports.\n\n` +
    `For each shard, give a short label (used in the branch/PR name, so keep it terse and slug-friendly-ish), a focusHint that's specific enough for another agent with no other context to grep for and select the right tags/files (list actual tag names, source files, or a code-level description of the shared bug), and an estimatedGapCount.\n\n` +
    `Report: format ("${group.format}" verbatim) and shards (array of {label, focusHint, estimatedGapCount}). This is a planning step only -- do not modify, commit, or build anything beyond what's listed above.`
}

const FIX_RESULT_SCHEMA = {
  type: 'object',
  properties: {
    format: { type: 'string' },
    scope: { type: 'string' },
    verified: { type: 'boolean' },
    gapsClosed: { type: 'number' },
    branch: { type: ['string', 'null'] },
    prUrl: { type: ['string', 'null'] },
    summary: { type: 'string' },
  },
  required: ['format', 'verified', 'gapsClosed', 'summary'],
}

function fixPrompt(group, workItem) {
  const approxCount = totalGapCount(group)
  const sampleMissing = (group.missing_in_oxidex || []).slice(0, 10)
    .map(t => `  - ${t.family}:${t.name} = ${t.value}`).join('\n') || '  (none in the inline sample)'
  const sampleDiffs = (group.value_differences || []).slice(0, 10)
    .map(d => `  - ${d.tag_key}: exiftool="${d.exiftool_value}" oxidex="${d.oxidex_value}"`).join('\n') || '  (none in the inline sample)'
  const scopeLine = workItem.focusHint
    ? `Your scope for this pass is narrower than the whole format -- focus specifically on: ${workItem.focusHint}\n` +
      `Other workers may be assigned other slices of this same format's gap list in this same round; stay inside your scope so you don't collide with their edits. If you find your scope was mis-drawn (e.g. the tags you were assigned actually require touching the same code as an unrelated tag outside your scope), use judgment, note it in your summary, but still only commit changes you can verify.\n\n`
    : ''

  return `You are working in the oxidex repository (a Rust ExifTool reimplementation) at "${REPO_PATH}", on format "${group.format}"` +
    `${workItem.label ? ` (assigned slice: "${workItem.label}")` : ''}. ` +
    `The find stage reported roughly ${approxCount} coverage gaps for this format overall. A few examples (this inline list may be truncated for large formats, so treat it as illustrative, not authoritative):\n\n` +
    `Missing entirely, a sample:\n${sampleMissing}\n\n` +
    `Value differences, a sample:\n${sampleDiffs}\n\n` +
    scopeLine +
    `Before doing anything else, get your OWN complete, current gap list for this format:\n` +
    `1. cargo build --release --bin tag-comparison --features tag-comparison-binary (if not already built)\n` +
    `2. ./target/release/tag-comparison --exiftool ${CACHE_DIR}/exiftool/exiftool ` +
    `--samples ${CACHE_DIR}/combined-samples --format ${group.format} ` +
    `-o /tmp/tagcmp-${group.format}-${workItem.slug}-start.json --markdown-dir /tmp/tagcmp-${group.format}-${workItem.slug}-start-md\n` +
    `Read /tmp/tagcmp-${group.format}-${workItem.slug}-start.json -- its missing_in_oxidex and value_differences arrays for "${group.format}" are the complete, authoritative gap list (this file comes straight from the comparison tool, not through an agent relay that may truncate it). ${workItem.focusHint ? 'Filter it down to your assigned scope described above.' : ''}\n\n` +
    `Find the relevant parser code yourself (grep src/parsers and src/core for "${group.format}" and tag names from that file -- there is no static format-to-file map to hand you). ` +
    `Check src/exiftool_tables::find_table(module, table) first per AGENTS.md -- ExifTool's real byte layout is often already transcribed there, which is the cheap way to close a gap versus re-deriving a binary record by hand. ` +
    `Implement as many of your assigned gaps as you can correctly verify in this pass. You do not need to close all of them -- large formats won't close in one round, and that's expected; whatever remains will resurface next round. For value differences, use judgment: only "fix" genuine bugs, not benign formatting differences. oxidex already runs its own format_for_exiftool/normalize_tag_family layer before this comparison runs, so gross PrintConv-vs-raw noise is already filtered out -- don't chase incidental ExifTool quirks that aren't part of the tag's documented semantics. Never approximate or guess a conversion -- per AGENTS.md, a plausible-but-wrong value is worse than an absent tag; omit rather than fabricate.\n\n` +
    `When you believe you've made progress:\n` +
    `1. cargo build --release --bin oxidex\n` +
    `2. Re-run: ./target/release/tag-comparison --exiftool ${CACHE_DIR}/exiftool/exiftool ` +
    `--samples ${CACHE_DIR}/combined-samples --format ${group.format} ` +
    `-o /tmp/tagcmp-${group.format}-${workItem.slug}-end.json --markdown-dir /tmp/tagcmp-${group.format}-${workItem.slug}-end-md\n` +
    `3. Read /tmp/tagcmp-${group.format}-${workItem.slug}-end.json and confirm the combined missing_in_oxidex + value_differences count for "${group.format}" is strictly lower than in the "-start.json" file, and that regressions is empty.\n` +
    `4. cargo test --workspace\n\n` +
    `If both checks pass, commit on your current git branch with a descriptive message using ` +
    `git ${GIT_AUTHOR_OVERRIDE} commit -m "..." -- the repo owner's default git config email is private/unverified on ` +
    `GitHub and a plain "git commit" will produce a commit that GitHub REJECTS on push (GH007) with no useful error until ` +
    `then, so you MUST pass that -c override on the commit command itself, not via "git config". Then:\n` +
    `5. git push -u origin "$(git branch --show-current)"\n` +
    `6. gh pr create --repo ${REPO_SLUG} --base main --head "$(git branch --show-current)" ` +
    `--title "fix(${group.format.toLowerCase()}): ${workItem.label || 'coverage'} tag fixes" ` +
    `--body "Automated ExifTool tag-coverage fix, format ${group.format}${workItem.label ? ` (scope: ${workItem.label})` : ''}. Verified in an isolated worktree: before/after tag-comparison run against pinned ExifTool 13.59 confirmed a strict reduction in missing/differing tags with zero regressions, plus cargo test --workspace passing. Please review before merging -- this was not merged automatically." ` +
    `-- this prints the PR URL, capture it.\n\n` +
    `Report: format -- use exactly the string "${group.format}" verbatim, not a slug or description of your own choosing, since the caller matches on it programmatically -- scope ("${workItem.label || 'all'}"), verified (true only if you committed, pushed, AND opened the PR successfully after both checks passed), gapsClosed (the count reduction between the start and end files you confirmed), branch (run "git branch --show-current" and report it if verified, else null), prUrl (the URL gh pr create printed, or null), and a one-paragraph summary. If you cannot verify a real, regression-free improvement, do NOT commit, push, or open a PR -- run "git checkout -- ." and "git clean -fd" to leave your worktree clean, and report verified: false, gapsClosed: 0, branch: null, prUrl: null.`
}

const AUDIT_SCHEMA = {
  type: 'object',
  properties: {
    findings: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          description: { type: 'string' },
          severity: { type: 'string' },
        },
        required: ['description', 'severity'],
      },
    },
    prOpened: { type: 'boolean' },
    prUrl: { type: ['string', 'null'] },
    summary: { type: 'string' },
  },
  required: ['findings', 'prOpened', 'summary'],
}

function auditPrompt(roundLog) {
  return `You are reviewing the execution log of an automated ExifTool tag-coverage-gap-closing loop that just ran against the oxidex repository at "${REPO_PATH}". Here is the round-by-round summary as JSON:\n\n${JSON.stringify(roundLog, null, 2)}\n\n` +
    `Look for concrete, actionable inefficiency patterns in how the loop itself operates -- NOT in the tag fixes -- for example: formats repeatedly failing verification across multiple rounds (suggesting the fix prompt or sharding gave a worker an impossible or wrongly-scoped task), shards that turned out to overlap and collide, rounds that closed suspiciously few gaps relative to workers spawned, or any sign from the summaries that workers cut corners (approximated a conversion instead of omitting it, skipped the pinned-oracle check, etc -- these would violate AGENTS.md).\n\n` +
    `If you find something concrete and actionable that you can fix in the workflow script itself, read "${REPO_PATH}/.claude/workflows/exiftool-coverage-loop.js", make the targeted improvement on a fresh branch off current main (cd "${REPO_PATH}" && git checkout main && git pull --ff-only origin main && git checkout -b <branch>), commit using git ${GIT_AUTHOR_OVERRIDE} commit -m "..." (the repo owner's default git config email is private/unverified on GitHub, so a plain "git commit" produces a commit GitHub REJECTS on push (GH007) -- pass that -c override on the commit command itself, never via "git config"), push, and open a PR (gh pr create --repo ${REPO_SLUG} --base main --head <branch> --title "chore(coverage-loop): <short description>" --body "<what was inefficient and why this fixes it>") explaining the inefficiency and the fix. If nothing concrete and actionable turned up, do NOT open a PR -- just report your findings, even if the finding is "no significant inefficiency observed."\n\n` +
    `Report: findings (array of {description, severity: "low"|"medium"|"high"}), prOpened (bool), prUrl (string or null), summary (one paragraph).`
}

const roundLog = []
let dryRounds = 0
let round = 0

while (dryRounds < MAX_DRY_ROUNDS && round < MAX_ROUNDS) {
  round++
  log(`--- round ${round} (dry streak: ${dryRounds}/${MAX_DRY_ROUNDS}) ---`)

  phase('Find')
  const report = await agent(findGapsPrompt(), {
    label: `find-gaps-round-${round}`,
    schema: COMPARISON_REPORT_SCHEMA,
  })

  if (!report) {
    throw new Error(`round ${round}: Find stage failed -- aborting without counting it as dry`)
  }

  let gapGroups = gapGroupsFrom(report, args && args.onlyFormats)
  log(`round ${round}: found gaps in ${gapGroups.length} formats (overall coverage ${report.overall_coverage}%)`)

  if (gapGroups.length === 0) {
    log(`round ${round}: zero gaps -- full coverage reached`)
    roundLog.push({ round, gapFormats: 0, workItems: 0, verified: 0, prsOpened: 0, skippedInFlight: 0 })
    dryRounds++
    continue
  }

  phase('Triage')
  const openPrsResult = await agent(openPRsPrompt(), {
    label: `open-prs-round-${round}`,
    schema: OPEN_PRS_SCHEMA,
  })
  const openPrs = (openPrsResult && openPrsResult.prs) || []
  const inFlightFormats = new Set()
  for (const g of gapGroups) {
    const marker = `fix(${g.format.toLowerCase()})`
    if (openPrs.some(pr => pr.title.toLowerCase().includes(marker))) {
      inFlightFormats.add(g.format)
    }
  }
  if (inFlightFormats.size) {
    log(`round ${round}: skipping ${inFlightFormats.size} format(s) with an open unmerged PR already: ${[...inFlightFormats].join(', ')}`)
  }
  gapGroups = gapGroups.filter(g => !inFlightFormats.has(g.format))

  if (gapGroups.length === 0) {
    log(`round ${round}: everything with a gap already has an open PR awaiting review -- nothing new to assign this round`)
    roundLog.push({ round, gapFormats: 0, workItems: 0, verified: 0, prsOpened: 0, skippedInFlight: inFlightFormats.size })
    dryRounds++
    continue
  }

  phase('Shard')
  const shardedPerFormat = await parallel(gapGroups.map(g => async () => {
    if (totalGapCount(g) <= SHARD_THRESHOLD) {
      return { group: g, items: [{ label: null, focusHint: null, slug: 'all' }] }
    }
    const plan = await agent(shardPlanPrompt(g), {
      label: `shard-plan-${g.format}`,
      phase: 'Shard',
      schema: SHARD_PLAN_SCHEMA,
    })
    const shards = (plan && plan.shards && plan.shards.length) ? plan.shards.slice(0, MAX_SHARDS) : [{ label: null, focusHint: null, estimatedGapCount: totalGapCount(g) }]
    log(`round ${round}: ${g.format} sharded into ${shards.length} slice(s)`)
    return {
      group: g,
      items: shards.map((s, i) => ({
        label: s.label || null,
        focusHint: s.focusHint || null,
        slug: (s.label || `shard${i}`).toLowerCase().replace(/[^a-z0-9]+/g, '-').slice(0, 30) || `shard${i}`,
      })),
    }
  }))

  let workItems = shardedPerFormat.filter(Boolean).flatMap(({ group, items }) => items.map(item => ({ group, item })))
  if (workItems.length > MAX_WORKITEMS_PER_ROUND) {
    log(`round ${round}: ${workItems.length} work items planned, capping at ${MAX_WORKITEMS_PER_ROUND} (dropped ${workItems.length - MAX_WORKITEMS_PER_ROUND} -- they resurface next round)`)
    workItems = workItems.slice(0, MAX_WORKITEMS_PER_ROUND)
  }
  log(`round ${round}: dispatching ${workItems.length} fix worker(s)`)

  phase('Fix')
  const fixResults = await parallel(workItems.map(({ group, item }) => () =>
    agent(fixPrompt(group, item), {
      label: `fix-${group.format}${item.label ? '-' + item.slug : ''}`,
      phase: 'Fix',
      isolation: 'worktree',
      schema: FIX_RESULT_SCHEMA,
    }).then(r => {
      if (r) log(`round ${round}: fix-${group.format}${item.label ? '/' + item.label : ''} -- ${r.verified ? `verified, closed ${r.gapsClosed}, PR ${r.prUrl}` : 'not verified, no PR'}`)
      return r ? { ...r, format: group.format } : r
    })
  ))

  const verified = fixResults.filter(Boolean).filter(r => r.verified)
  log(`round ${round}: ${verified.length}/${fixResults.filter(Boolean).length} fix attempts verified and PR'd`)

  roundLog.push({
    round,
    gapFormats: gapGroups.length,
    workItems: workItems.length,
    verified: verified.length,
    prsOpened: verified.filter(r => r.prUrl).length,
    skippedInFlight: inFlightFormats.size,
  })

  dryRounds = verified.length === 0 ? dryRounds + 1 : 0
}

const stoppedReason = dryRounds >= MAX_DRY_ROUNDS ? `${dryRounds} consecutive dry rounds` : `hit the ${MAX_ROUNDS}-round safety cap`
log(`stopped after ${round} rounds (${stoppedReason})`)

phase('Audit')
const audit = await agent(auditPrompt(roundLog), {
  label: 'process-audit',
  schema: AUDIT_SCHEMA,
})
if (audit) {
  log(`audit: ${audit.summary}`)
  if (audit.prOpened) log(`audit opened a process-improvement PR: ${audit.prUrl}`)
}

return { rounds: round, stoppedReason, roundLog, audit }
