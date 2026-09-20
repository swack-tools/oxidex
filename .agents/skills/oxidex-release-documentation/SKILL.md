---
name: oxidex-release-documentation
description: Use when auditing OxiDex documentation, benchmark claims, or GitHub Pages readiness for a release candidate, including approval requests based on a green VitePress build or sidebar review.
---

# OxiDex Release Documentation

Approve documentation only from a complete, commit-bound evidence receipt.
A green build and a sidebar tour do not cover every rendered route. This
skill audits release readiness; it does not authorize a merge, deployment,
Pages setting change, or release publication.

## Inputs and output

Require version, full candidate SHA, parity receipt, benchmark artifacts,
workflow files, live Pages settings, committed docs, and generated content.
Copy `templates/documentation-release-receipt.json` into a unique durable
evidence directory outside tracked content. Populate all sections, retaining
unknown facts as `unverified` and blockers in `unresolved`. A template is not
evidence. Record command, exit status, UTC time, full SHA, and evidence path.

## Ordered audit

1. Read repository instructions, run preflight, and freeze the candidate SHA
   in an isolated clean worktree. Ingest the parity receipt: check its actual
   schema, status, SHA, oracle pin/capability, instruments, and artifact hashes.
   Missing, partial, or mismatched parity evidence blocks parity claims.
2. Read [factuality-ledger.md](references/factuality-ledger.md). Build the claim
   ledger, then inventory every committed Markdown source, generated comparison
   page, and rendered HTML route. Reconcile all three sets, including unlinked
   pages and copied benchmark reports. Give each page a `current`, `historical`,
   or `excluded` disposition and evidence; exclusions require a build reason.
3. Resolve stale current content. Check changelog, versions, installation,
   migration, platform support, status, parity and performance statements
   against candidate sources and receipts. Historical claims need visible
   dates/commits and must not be repeated as current release claims.
4. Read [benchmark-policy.md](references/benchmark-policy.md). Verify the exact
   candidate commit for each current benchmark claim. Record measured SHA,
   profile, machine, corpus, oracle, artifact identity and disposition. An older
   fallback may remain only as visibly historical, never as candidate results.
5. Read [github-pages-audit.md](references/github-pages-audit.md). Use the
   tracked `tools/docs-local-deploy.sh --build-only --output` and
   `tools/docs/release-audit.mjs` commands with real comparison output and
   candidate benchmark inputs. Their snapshot, crawl and visual manifests must
   cover every rendered route, local asset and fragment plus the tracked
   desktop/mobile light/dark representative matrix. Preserve screenshots,
   console/page/request findings and server logs, then obtain human review of
   the complete matrix.
6. Inspect Pages API `build_type`, `deploy-docs.yml`, and `release.yml`.
   Validate syntax/tests, triggers/path filters, permissions, generated-report
   and benchmark handoff, artifact/deploy actions, domain/base/HTTPS and current
   `workflow` mode settings. A pipeline defect blocks verification; a `gh-pages`
   update is not proof of deployment.
7. Bind the verified local audit to the exact candidate SHA and tree. After an
   authorized merge, compare the final `MAIN_SHA` tree: if it differs, rerun the
   same local audit against it before tag authorization. For an identical tree,
   preserve candidate evidence and record the SHA/tree equivalence explicitly.

## Approval contract

Set overall `status: verified` after the exact candidate's production-equivalent
local deploy, exhaustive factuality/route/asset audit, automated responsive
browser checks, human screenshot review and Pages pipeline audit all pass,
with no unresolved required checks. This is sufficient to approve documentation
quality before deployment. It does not authorize a merge, tag or publication.

Actual live deployment is optional post-merge operational confirmation, not a
documentation-quality prerequisite. `live_deployment` may be `not_run` or
`unverified` with `required_for_documentation_verification: false`. Do not put
an unrequested live confirmation into required `unresolved` items. If optional
checks discover a real documentation or pipeline defect, record that defect
and block verification until resolved.

A known unmet requirement is `blocked`; evidence not yet collected is
`unverified`. Preserve completed evidence and name the remaining work.

| Shortcut | Required evidence |
| --- | --- |
| Sidebar pages look fine | Reconciled source/generated/rendered census |
| Cold build is green | Production inputs, route/asset crawl, responsive screenshots |
| Older benchmark fallback succeeded | Historical label and actual measured commit |
| Release workflow updates `gh-pages` | Workflow-mode Pages API and pipeline audit |
