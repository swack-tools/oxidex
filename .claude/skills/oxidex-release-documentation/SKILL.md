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
5. Read [github-pages-audit.md](references/github-pages-audit.md). Reproduce the
   production build using `tools/docs-local-deploy.sh`, real comparison output
   and explicit benchmark inputs. Crawl every rendered route and referenced
   asset; inspect representative pages at desktop and mobile widths in light
   and dark theme, saving screenshots and console/network findings.
6. Inspect Pages API `build_type`, `deploy-docs.yml`, and `release.yml`.
   Require live `workflow` mode verification; a `gh-pages` update is not proof
   of deployment. Record pipeline evidence separately from local validation.
7. After an authorized merge, freeze the exact `main` SHA and verify that
   commit's successful Pages run, artifact, deployment identity, and live
   content hashes. Revalidate commit-bound evidence after any change. A
   pre-merge audit can finish locally while live deployment remains unverified.

## Approval contract

Use two phases of the same receipt:

- Before the PR to `main`, set `phase: candidate_local` and
  `status: ready_for_promotion` only after the exhaustive candidate-bound
  claims/pages/benchmark audit, production build, crawl, responsive visuals and
  Pages pipeline checks pass. Set `promotion_readiness.status: verified`, bind
  it to the candidate SHA, and leave `live_deployment.status: pending` with
  exact-main verification in `unresolved`. This is local readiness, not release
  approval, and does not authorize a merge.
- After merge, set `phase: main_live`, rerun/confirm against exact `MAIN_SHA`,
  and require successful `deploy-docs.yml`, live crawl, responsive visual checks
  and artifact/content identity. Upgrade the same receipt to `status: verified`
  only when all required checks pass and `unresolved` is empty. Require this
  full verification before tag authorization.

A known unmet requirement is `blocked`; evidence not yet collected is
`unverified`. Preserve completed evidence and name the remaining work.

| Shortcut | Required evidence |
| --- | --- |
| Sidebar pages look fine | Reconciled source/generated/rendered census |
| Cold build is green | Production inputs, route/asset crawl, responsive screenshots |
| Older benchmark fallback succeeded | Historical label and actual measured commit |
| Release workflow updates `gh-pages` | Workflow-mode Pages API and exact-commit live proof |
