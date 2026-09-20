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

The production-shaped durable build command is:

```bash
set -euo pipefail
tools/docs-local-deploy.sh --ref <candidate-sha> --full-report \
  --bench <candidate-benchmark-run-id> --build-only \
  --output "$EVIDENCE_DIR/docs-snapshot"
```

Replace the angle-bracket inputs with the frozen full SHA and verified run ID;
they are notation, not shell syntax. `--full-report` generates the actual
comparison pages; a stub cold build is insufficient. `--bench` must use an
explicit provenance-checked ID, with historical disposition if applicable.
Match the workflow's Node version (currently 24), environment/base URL,
dependency lock and generated report inputs. Save complete stdout/stderr and
exit statuses. In build-only mode the helper archives the ref into a temporary
snapshot, builds, copies Criterion reports into a durable output directory,
writes `snapshot-manifest.json`, and terminates. Require the manifest's exact
candidate SHA, source identity, configured base path, tree hash, dist hash and
complete rendered-route inventory. A `--worktree` snapshot deliberately records
`candidate_sha: null` plus a `source_identity` ending in `-worktree`; it is useful
for iterative testing but cannot be promoted as commit-bound release evidence.
Use `--ref` for the receipt-producing build. The
interactive default remains available for manual preview but is not the
receipt-producing instrument.

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

Install the lockfile and the pinned Playwright browser, then run the tracked
audit against the helper's own route inventory:

```bash
set -euo pipefail
(cd docs && npm ci --no-audit --no-fund && npx playwright install chromium)
node tools/docs/release-audit.mjs \
  --dist "$EVIDENCE_DIR/docs-snapshot/dist" \
  --inventory "$EVIDENCE_DIR/docs-snapshot/snapshot-manifest.json" \
  --representatives tools/docs/release-audit-representatives.json \
  --output "$EVIDENCE_DIR/browser-audit"
```

The command serves via ephemeral localhost HTTP, never `file://`, and writes
`automation-manifest.json`, `crawl.json`, `visual-matrix.json`, deterministic
screenshots and `server.log`. It verifies the dist aggregate hash against the
snapshot manifest before browser navigation and mounts the site at the
manifest's recorded base path, including non-root preview deployments.
It compares the supplied inventory to every rendered HTML file and exits
nonzero for route, asset, fragment, console, page, request, HTTP, interaction,
theme or missing-matrix-cell failures. Preserve all outputs without filtering
or rewriting them. A 200 fallback/404 page is not a valid route. Check base-path
handling, navigation, search and benchmark links, then rerun the whole audit
after fixes.

## Reproducible local browser automation and human review

Use Playwright when already available; equivalent browser automation is valid
if it produces the same evidence. Do not add a dependency merely to enforce a
tool preference. A missing browser, failed automation or incomplete human
review is unverified/blocked, never a pass. An HTTP-only crawler cannot prove
hydration, client navigation or responsive rendering.

Save an `automation_manifest` before the run with candidate SHA/tree, dist and
input hashes, localhost base URL, complete route list, representative route
list, browser/tool versions, script or replayable tool-call transcript path,
exact launch command/settings, UTC time and evidence directory. Resolve routes
from the rendered census, not links discovered from the home page. Record a
reproducible browser contract as follows:

1. Start the helper's HTTP preview and confirm its served content matches the
   recorded dist. Use isolated browser contexts with no inherited storage or
   extensions. In Playwright this is `browser.newContext(...)`; record browser
   engine/version, viewport, device scale factor and locale.
2. Before navigation attach `page.on('console', ...)` (error level),
   `page.on('pageerror', ...)`, `page.on('requestfailed', ...)`, and
   `page.on('response', ...)` for HTTP status >= 400. Save URL, resource type,
   failure/status and associated route. A 404 response does not necessarily
   produce `requestfailed`, so both checks are required.
3. For every inventory route, use `page.goto(url, {waitUntil: 'networkidle',
   timeout: 30000})` or the equivalent bounded settled-page wait. Verify the
   response, final URL, expected heading/content, hydrated navigation and
   absence of a fallback error page. Audit all referenced local assets and
   fragments, including ones not requested at this viewport. A timeout or
   failed request is recorded and investigated, not ignored to finish a run.
4. For each representative below, use contexts at 1440x1000 and 390x844 with
   `colorScheme: 'light'` and `'dark'`. Exercise the site's theme switch if
   needed and assert the actual root/computed theme, not just the emulation
   setting. Wait for fonts/images and layout to settle with a bounded wait;
   do not use arbitrary sleep as proof. Exercise mobile menu, search, a normal
   internal navigation, code blocks, and horizontal scrolling of wide tables.
5. Save a full-page screenshot for every representative route/viewport/theme cell via
   `page.screenshot({path, fullPage: true})` or equivalent; capture additional
   screenshots for opened navigation/search and any clipped/overflow state.
   Use deterministic filenames keyed by route, width and theme. Record a
   per-sample result containing route, viewport, theme, screenshot path/hash,
   interaction results and console/network failure arrays (explicitly empty
   when observed). Every missing matrix cell is an unresolved requirement.
6. Preserve machine-readable crawl/sample results and tool logs. The run must
   exit nonzero or report failed status for missing routes/assets, unexpected
   errors, timeouts or failed assertions. Document any benign exception with
   its cause and evidence; silently filtering an error is not a pass.
7. Present the complete screenshot manifest/contact sheet and full-resolution
   images for human screenshot review. Record reviewer, UTC review time,
   reviewed manifest hash, findings and disposition in `visual_review.human_review`.
   Automated image capture or an agent claiming the page looks fine does not
   substitute for this human review. If not yet reviewed, keep it unverified.
8. Fix findings, rebuild at the new frozen SHA, rerun the exhaustive crawl and
   affected browser matrix, and obtain review of the updated screenshots.

Browser review must cover home, guide, install, migration, changelog,
reference, parity, status, performance, and at least one wide-table page.
Discover the actual routes from the census. At **1440px** desktop and **390px**
mobile widths, inspect every representative in both light and **dark theme**.
For each route/width/theme, save screenshot paths, viewport dimensions, page
load/interaction result, console errors, failed network requests and findings
in `visual_review.samples`; record zero errors explicitly when observed.

Human review checks overflow/table scrolling, readable text/contrast, navigation
menus, search, code blocks, headings, badges and benchmark charts. The
representative matrix complements the exhaustive browser route/asset crawl;
neither substitutes for the other.

## Pages configuration and pipeline

These are read-only checks, not deployment operations:

```bash
gh api repos/swack-tools/oxidex/pages \
  --jq '{status,cname,https_enforced,build_type,source}'
actionlint -config-file .github/actionlint.yaml \
  .github/workflows/deploy-docs.yml .github/workflows/release.yml
python3 -m unittest tools.ci.test_release_workflow -v
```

Persist the API responses with timestamps. Require actual `build_type` to be
`workflow`; record `cname`, HTTPS and source settings. Inspect candidate YAML
for triggers/path filters, concurrency, generated comparison and benchmark
handoff/provenance, build/copy steps, upload/deploy-pages actions, permissions,
`github-pages` environment and custom domain/base/HTTPS agreement. Record syntax
and repository-test results; existing tests do not replace inspection of
untested handoffs. A source-branch
setting or legacy `gh-pages` commit does not establish workflow-mode publishing.
`release.yml` has a stable-only `update-docs` job that mutates `gh-pages`; a
prerelease skips that job, and even a completed job does not prove this site's
active Pages deployment. Record the discrepancy instead of changing settings
under audit authority.

Separate `local_build`, `visual_review`, `pages_pipeline`, and optional
`live_deployment` statuses. The Pages API proves current configuration; syntax,
tests and static inspection prove the audited pipeline contract. A pipeline
defect blocks documentation verification. The absence of an actual run for an
unmerged candidate is not a defect. Check whether triggers/path filters cover
the intended release changes, rather than requiring a run as a prerequisite.
A manual dispatch is an external action needing existing task scope.

## Optional post-merge live deployment confirmation

The exact-candidate production-equivalent local build, complete local browser
audit, human screenshot review and successful Pages pipeline/settings audit
are sufficient for overall documentation `status: verified`. An actual
deployment URL or run is not required. Keep
`required_for_documentation_verification: false` in `live_deployment`; use
`not_run` when not attempted or `unverified` for incomplete optional evidence.
Do not put the absence of optional live evidence into required `unresolved`.

After merge, freeze `MAIN_SHA` and compare its tree with `candidate_tree`. If
different, rerun the same local production audit against that exact commit
before tag authorization. For an identical tree, retain original candidate
provenance and record SHA/tree equivalence in `main_equivalence`.

When post-merge operational confirmation is in scope, inspect candidate runs:

```bash
gh run list --workflow deploy-docs.yml --branch main \
  --json databaseId,headSha,status,conclusion,url
```

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
cache-propagation retries; preserve mismatches as operational findings. If
deployment/API identity or artifact evidence is unavailable, mark the optional
section unverified, not passed. This does not invalidate proven local quality.
If these checks expose a real documentation/pipeline defect, move the overall
receipt to blocked until that defect is resolved. Completing optional checks
may verify `live_deployment.status`; it is not the gate for overall verification.
