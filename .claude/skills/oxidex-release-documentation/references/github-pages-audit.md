# Exhaustive GitHub Pages audit

## Freeze inputs and reproduce production

Read `AGENTS.md`, `CLAUDE.md`, `docs/.vitepress/config.mts`,
`docs/package.json`, `tools/docs-local-deploy.sh`,
`.github/workflows/deploy-docs.yml`, and `.github/workflows/release.yml` at the
candidate commit. Resolve actual config filenames if they change. Run
`bash tools/preflight.sh --upstream --github` before remote checks. Use a clean
isolated candidate worktree and durable evidence directory; record the full
SHA, tree hash, tool versions, lockfile hash, UTC time and exact commands.
Do not run a heavy comparison/build outside the shared measurement lock
documented in `docs/AUTOGENERATION-PLAN.md`.

The production-shaped command is:

```text
tools/docs-local-deploy.sh --ref <candidate-sha> --full-report \
  --bench <candidate-benchmark-run-id> --no-open --keep
```

Replace the angle-bracket inputs with the frozen full SHA and verified run ID;
they are notation, not shell syntax. `--full-report` generates the actual
comparison pages; a stub cold build is insufficient. `--bench` must use an
explicit provenance-checked ID, with historical disposition if applicable.
Match the workflow's Node version (currently 24), environment/base URL,
dependency lock and generated report inputs. Save complete stdout/stderr and
exit statuses. The helper archives the ref into a temporary snapshot, builds,
copies Criterion reports and remains serving until interrupted. Record its
printed source/dist paths, keep the process running for review, then retain
the snapshot and copy evidence to durable storage; `--keep` alone is not a
durability guarantee. A server intentionally stopped after review is not a
failed build; record the build result separately from server termination.

Check the helper against the workflow at this SHA. It is a reproduction aid,
not proof of equivalence: verify generated inputs, report counts and hashes,
successful downloads, and all publish steps. Its archive may lack `.git` for
instruments needing provenance; record/refuse instrument failures and reproduce
the same steps in a detached candidate worktree under the lock rather than
bypassing instrument checks. Record any deviation and evidence of equivalence.

## Reconcile committed, generated and rendered sets

The full census includes `docs/**/*.md`, generated comparison Markdown and
every `docs/.vitepress/dist/**/*.html`. From the candidate worktree and the
helper's printed snapshot path, save these inventories (variables must be
absolute verified paths; `CANDIDATE_SHA` is already frozen):

```bash
set -euo pipefail
git ls-tree -r --name-only "$CANDIDATE_SHA" -- docs \
  | rg '\.md$' > "$EVIDENCE_DIR/committed-markdown.txt"
rg --files --hidden --no-ignore "$SNAPSHOT_SRC/docs/reference/comparison" \
  -g '*.md' > "$EVIDENCE_DIR/generated-comparison.txt"
rg --files --hidden --no-ignore "$SNAPSHOT_SRC/docs/.vitepress/dist" \
  -g '*.html' > "$EVIDENCE_DIR/rendered-html.txt"
```

Treat an unexpectedly empty inventory or failed command as blocked. Also
inventory any additional generated Markdown found in the snapshot. Normalize
source-relative paths, then reconcile against actual VitePress routing rules
(`base`, `cleanUrls`, rewrites, exclusions and index pages). Include unlinked
Markdown, copied `/benchmarks/` HTML, framework pages and source-only files.
Every source needs a route or justified exclusion; every HTML file needs its
origin and classification. Record counts and unresolved set differences in
the receipt. Sidebar links and a crawler starting only at home cannot discover
all published files. Resolve every difference before approval.

Serve via HTTP using the helper, never `file://`: root-relative assets and
clean URLs require a server. Crawl **every rendered route** from the HTML
inventory and every referenced local asset/link, including CSS/JS resources,
fonts/images, downloads and fragment targets. Verify status, content type,
expected content and fragment existence; a 200 fallback/404 page is not a
valid route. Save machine-readable per-URL outcomes, final redirects, broken
links, asset failures and unresolved external links. Check base-path handling,
navigation, search, and benchmark links. Run the whole crawl again after fixes.

## Responsive visual evidence

Browser review must cover home, guide, install, migration, changelog,
reference, parity, status, performance, and at least one wide-table page.
Discover the actual routes from the census. At **1440px** desktop and **390px**
mobile widths, inspect every representative in both light and **dark theme**.
For each route/width/theme, save screenshot paths, viewport dimensions, page
load/interaction result, console errors, failed network requests and findings
in `visual_review.samples`; record zero errors explicitly when observed.

Inspect overflow/table scrolling, readable text/contrast, navigation menus,
search, code blocks, headings, badges and benchmark charts. Capture and inspect
screenshots; merely generating screenshots is not a visual pass. Fix findings,
rebuild and repeat affected samples. The representative matrix complements the
exhaustive route crawl; neither substitutes for the other. If the browser is
unavailable, keep visual review `unverified` and report that blocker.

## Pages configuration and pipeline

These are read-only checks, not deployment operations:

```bash
gh api repos/swack-tools/oxidex/pages \
  --jq '{status,cname,https_enforced,build_type,source}'
gh run list --workflow deploy-docs.yml --branch main \
  --json databaseId,headSha,status,conclusion,url
```

Persist the API responses with timestamps. Require actual `build_type` to be
`workflow`; record `cname`, HTTPS and source settings. Inspect candidate YAML
for the comparison artifact, build/copy steps, upload/deploy-pages actions,
permissions, `github-pages` environment and path filters. A source-branch
setting or legacy `gh-pages` commit does not establish workflow-mode publishing.
`release.yml` has a stable-only `update-docs` job that mutates `gh-pages`; a
prerelease skips that job, and even a completed job does not prove this site's
active Pages deployment. Record the discrepancy instead of changing settings
under audit authority.

Separate `local_build`, `visual_review`, `pages_pipeline`, and
`live_deployment` statuses. The Pages API proves current configuration; YAML
proves wiring; run-list output identifies candidates, not successful delivery.
If workflow path filters prevented a run for the release SHA, record missing
evidence; a manual dispatch is an external action needing existing task scope.

## Verify the exact-commit live deployment

After an authorized merge, freeze the full `MAIN_SHA` from the merged PR and
compare candidate/main trees. A changed tree invalidates affected receipts;
even identical trees need commit-bound run and generated-input revalidation.
Select a `deploy-docs.yml` run on `main` whose `headSha == MAIN_SHA`, then save
its final record and completed publish-job logs:

```bash
set -euo pipefail
gh run view "$PAGES_RUN_ID" -R swack-tools/oxidex \
  --json databaseId,headSha,headBranch,workflowName,status,conclusion,url,jobs \
  > "$EVIDENCE_DIR/pages-run.json"
gh run view "$PAGES_RUN_ID" -R swack-tools/oxidex --log \
  > "$EVIDENCE_DIR/pages-run.log"
gh api "repos/swack-tools/oxidex/deployments?sha=$MAIN_SHA&environment=github-pages" \
  > "$EVIDENCE_DIR/pages-deployments.json"
```

Require completed/success, exact head SHA, expected workflow/branch, and a
successful deploy-pages step. Resolve the deployment ID and successful status
to that run, SHA, Pages artifact and environment URL; save deployment status
API output (`repos/swack-tools/oxidex/deployments/<id>/statuses`). Download and
hash the actual Pages artifact from that run, validating its archive paths
before extraction. Use its HTML/assets manifest to compare served content at
the final live URL, following redirects and recording response headers/hashes.
The production artifact is the hash source of truth: regeneration timestamps
can legitimately differ from local builds. Verify that its claims/generated
data match the approved dispositions and perform the live route/asset crawl.

Save `main_sha`, `run_id`, `deployment_id`, `artifact_id`, artifact hash,
environment URL, fetch time and per-route/asset content hashes in the receipt.
Check the final served version and release content, and repeat the representative
desktop/mobile light/dark browser matrix against the live site, recording console,
network and screenshot evidence separately from local review. Do not equate a
200 home page, DNS/TLS success or a green run with exact-commit content. Bound
cache-propagation retries; preserve mismatches as blocked. If deployment/API
identity or artifact evidence is unavailable, mark it unverified, not passed.

Before merge, report `status: ready_for_promotion` only when all candidate-local
checks and Pages pipeline checks pass; set `live_deployment.status: pending`.
After merge, rerun/confirm against the exact `MAIN_SHA`. Only after this live
proof may `live_deployment.status` and the same receipt's overall `status`
become `verified`, with `phase: main_live` and no unresolved requirements.
Tag authorization requires the fully verified receipt, not local readiness.
