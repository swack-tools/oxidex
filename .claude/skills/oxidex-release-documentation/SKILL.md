---
name: oxidex-release-documentation
description: Use when release-candidate documentation, benchmark claims, changelog, or GitHub Pages must be audited for factual accuracy and build/browser readiness; not for an isolated prose or typo edit.
---

# OxiDex Release Documentation

Approve documentation only from complete, commit-bound evidence. A green build
or sidebar tour does not cover every rendered route. This skill does not
authorize merge, deployment, Pages-setting changes, tagging, or publication.

## Inputs and output

Require a version, full candidate SHA/tree, verified parity receipt, committed
docs/generated content, workflow files, Pages settings, and an explicit
benchmark disposition. Benchmark artifacts are required only when a current or
historical performance claim is retained; a release with no performance claim
uses `not_applicable` with a reason.

Copy [`documentation-release-receipt.json`](templates/documentation-release-receipt.json)
to a unique durable evidence directory outside tracked content. Keep unknown
facts `unverified`, known failures `blocked`, and blockers in `unresolved`.

## Ordered audit

1. Run preflight in a clean isolated worktree and freeze the candidate. Validate
   the parity receipt's schema, status, SHA/tree, oracle, instruments, scope,
   and hashes. Missing or mismatched parity evidence blocks parity claims.
2. Read [`factuality-ledger.md`](references/factuality-ledger.md). Reconcile all
   committed Markdown, generated pages, and rendered routes, including unlinked
   pages and copied reports. Classify each as `current`, `historical`, or
   `excluded` with evidence.
3. Reconcile changelog, version, installation, migration, platform, status,
   parity, and performance statements against the exact candidate. Read
   [`benchmark-policy.md`](references/benchmark-policy.md) and bind every
   retained benchmark claim to its actual commit and artifact.
4. Read [`github-pages-audit.md`](references/github-pages-audit.md). Run the
   tracked production-equivalent build and browser audit. Crawl every rendered
   route, local asset, and fragment; separately capture every representative
   route/viewport/theme cell. Preserve manifests, screenshots, console/page/
   request findings, server logs, and human screenshot review.
5. Audit Pages API `build_type`, workflow syntax/tests, triggers, permissions,
   generated/benchmark handoffs, artifact/deploy actions, domain/base, and
   HTTPS. A legacy `gh-pages` update or workflow YAML is not deployment proof.
6. Bind approval to candidate SHA/tree. If an authorized merge changes the
   tree, rerun against `MAIN_SHA` before tag authorization; otherwise record
   explicit tree equivalence.

## Approval contract

Set `status: verified` only after exact-candidate factuality, source/generated/
rendered reconciliation, production-equivalent local build, exhaustive route/
asset/fragment browser checks, the complete representative visual matrix,
human screenshot review, and Pages pipeline/settings audit all pass with no
required unresolved items. This approves documentation quality before tag
authorization, not the release itself.

Live deployment confirmation is optional post-merge evidence. It may remain
`not_run` or `unverified` with
`required_for_documentation_verification: false`. If an optional check exposes
a real defect, block verification until it is resolved.
