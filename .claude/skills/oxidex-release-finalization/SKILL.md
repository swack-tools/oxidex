---
name: oxidex-release-finalization
description: Use when preparing, promoting, tagging, publishing, or validating an OxiDex release or release candidate.
---

# OxiDex Release Finalization

## Core boundary

Finalize only an immutable, receipt-backed release candidate. A deadline, a
pre-release label, or green integration CI does not authorize bypassing the
reviewed PR to `main`. The release tag must name the exact `main` commit that
passed the release gates.

Never infer publication or Apple verification from workflow YAML, secret
names, a dry run, or a green build. Verify the released macOS download with
its code signature, Gatekeeper assessment, and stapled notarization ticket.

## Required inputs and output

Require the release version, expected tag, full candidate SHA, a verified
parity receipt, a verified documentation receipt, and explicit packaging
decisions. Refuse a missing, stale, partial, blocked, or SHA-mismatched
receipt. Copy `templates/release-finalization-receipt.json` to a unique
evidence directory and update it as facts become available; never turn an
unavailable check into a pass.

The receipt is `verified` only after every required gate, exact-commit
workflow, expected artifact, and macOS check is proved. Otherwise set
`status` to `blocked` or `unverified` and make `next_action` the next safe
operation.

## Workflow

1. Read [references/gates.md](references/gates.md). Run preflight, freeze the
   candidate SHA, inventory every workspace/user-facing version, validate
   receipt compatibility, and run the locked gates plus workflow tests.
2. Promote through a reviewed PR whose base is `main`. After merge, freeze
   the exact `main` commit, confirm the candidate/main trees, revalidate all
   commit-bound evidence, and require successful CI for that exact SHA.
3. Run the signed-tag dry run against that SHA. Then stop and request explicit maintainer
   authorization for the exact `(version, tag, main SHA)` tuple.
   Authorization to prepare, merge, or dry-run is not tag-push authorization.
4. Only after that authorization, create and push the signed tag at the same
   SHA. Existing remote tags are immutable: do not move, delete, or reuse one.
5. Read [references/github-release-and-macos.md](references/github-release-and-macos.md).
   Monitor both tag workflows, reconcile the expected asset matrix, download
   the release artifacts, hash them, and verify the actual macOS artifacts.

## Pressure red flags

- "It is only a prerelease, so `main` can wait."
- "The workflow runs `codesign`/`notarytool`, so Apple verification is done."
- "The earlier approval also covers pushing the tag."
- "The tag can be repointed if a late check fails."

All four mean stop. Preserve the blocked receipt and follow the recovery table
in the references; speed does not widen release authority.

## Common rationalizations

| Shortcut | Required response |
| --- | --- |
| Integration CI is green | Open and complete the reviewed PR to `main`; revalidate the merge SHA. |
| The YAML contains Apple commands | Record workflow-path validation only; verify the released download. |
| The deadline is near | Keep the release blocked until exact-SHA evidence and authorization exist. |
| A pushed tag is wrong | Preserve it, stop publication, and use the immutable-tag recovery path. |
