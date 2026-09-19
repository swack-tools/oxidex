# Release checklist

This page routes the release; the project skills contain the executable
procedures and receipt schemas. A green `just ci` alone is not release proof.

## 1. Freeze and measure the candidate

Record the full candidate SHA and tree in a durable evidence directory. Run
the skills in this order:

1. `exiftool-parity` produces a verified parity receipt from the pinned oracle
   and named instruments.
2. `oxidex-release-documentation` consumes that receipt and produces a
   verified documentation receipt for the same candidate SHA and tree.
3. `oxidex-release-finalization` consumes both receipts, inventories every
   version string, runs the release gates, and audits the publication
   workflows and expected artifacts.

Missing, blocked, partial, stale, or SHA-mismatched receipts stop the release.
The documentation audit must use the exact candidate's production-equivalent
local deploy, real generated inputs, every rendered route and asset, browser
automation, responsive light/dark screenshots, and recorded human visual
review. That evidence plus a passing Pages pipeline/settings audit is
sufficient for documentation verification; live Pages deployment is optional
operational confirmation.

## 2. Promote through `main`

Open and review a release PR whose base is `main`; never push directly to the
protected branch. After merge, freeze the exact `main` commit as `MAIN_SHA` and
require its CI and release gates to pass. If its tree differs from the audited
candidate, regenerate the parity receipt and documentation receipt against
`MAIN_SHA` before continuing.

Run the signed-tag dry run against that exact commit:

```bash
OXIDEX_TAG_DRY_RUN=1 just tag "$VERSION" "$MAIN_SHA"
```

Then stop for separate explicit maintainer authorization of the exact version,
tag, and `MAIN_SHA`. Preparing or merging the PR does not authorize the real
signed tag. Existing remote tags are immutable; do not move, delete, or reuse
one.

## 3. Verify publication

After the authorized signed tag is pushed, follow
`oxidex-release-finalization` until its receipt records:

- the tag resolving to the gated exact `main` commit;
- successful tag-bound GitHub Actions runs;
- the GitHub release's prerelease/latest classification, expected assets, and
  checksums;
- downloaded macOS artifact hashes plus the actual code signature, Gatekeeper
  assessment, and stapled notarization ticket; and
- any selected package or container publication results.

Workflow YAML, secret names, a dry run, or a green build do not prove that the
GitHub release exists or that Apple accepted the downloadable artifact.
