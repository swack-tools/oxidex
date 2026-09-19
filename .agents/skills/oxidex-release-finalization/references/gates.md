# Candidate, gate, promotion, and tag procedure

Use this reference through the tag push. It contains commands that mutate
remote state; inspection and dry-run steps do not authorize the real mutation.

## 1. Preflight and freeze

Work in a dedicated release worktree and branch. Before edits or remote
commands:

```bash
VERSION=2.0.0-beta.1
TAG="v$VERSION"
PARITY_RECEIPT=/absolute/path/to/release-parity-receipt.json
DOCUMENTATION_RECEIPT=/absolute/path/to/documentation-release-receipt.json
tools/preflight.sh --upstream
git fetch origin main refactor/tag-machinery --tags
git status --short --branch
git rev-parse --show-toplevel
```

Create a new evidence directory outside tracked paths and capture the immutable
candidate identity. Replace the example evidence path with a unique timestamped
path for the run.

```bash
EVIDENCE_DIR=/tmp/oxidex-release-v2.0.0-beta.1-20260918T120000Z
mkdir -p "$EVIDENCE_DIR"
CANDIDATE_SHA=$(git rev-parse --verify 'HEAD^{commit}')
git show --no-patch --format=fuller "$CANDIDATE_SHA" | tee "$EVIDENCE_DIR/candidate.txt"
git status --porcelain=v1 | tee "$EVIDENCE_DIR/status.txt"
```

Require a clean tree. Record the full SHA and evidence directory in every
receipt and log; `HEAD`, "tip", branch names, and abbreviated SHAs are not
release identities.

## 2. Complete version inventory

Inventory Cargo workspace members rather than assuming every crate shares the
root version. Preserve intentional independent versions such as
`oxidex-tags-shared` and non-publishable `0.0.0` utility crates.

```bash
cargo metadata --no-deps --format-version 1 \
  | jq -S '{workspace_members, packages: [.packages[] | {name, version, manifest_path}]}' \
  | tee "$EVIDENCE_DIR/workspace-versions.json"
VERSION_RE=${VERSION//./\\.}
rg -n --hidden -g '!target' -g '!.git' \
  "$VERSION_RE|v$VERSION_RE|\[package\]|^version\s*=|VERSION|__version__" \
  Cargo.toml Cargo.lock oxidex-tags* packaging bindings src docs CHANGELOG.md README.md \
  .github justfile | tee "$EVIDENCE_DIR/version-references.txt"
python3 tools/ci/release_version.py --tag "$TAG" --cargo-toml Cargo.toml \
  | tee "$EVIDENCE_DIR/release-version.txt"
```

Review the results against the packaging decision and documentation receipt.
Do not silently normalize independent package versions or count dependency
versions in `Cargo.lock` as release versions.

## 3. Receipt compatibility

Parse both JSON receipts before trusting them. The parity receipt binds the
candidate in `oxidex_sha`; the documentation receipt binds it in
`candidate_sha`.

```bash
jq -e --arg sha "$CANDIDATE_SHA" '
  .schema_version == 1 and .oxidex_sha == $sha and .status == "verified"
  and (.refusals | length) == 0
' "$PARITY_RECEIPT"
jq -e --arg sha "$CANDIDATE_SHA" --arg version "$VERSION" '
  .schema_version == 1 and .candidate_sha == $sha and .version == $version
  and .status == "verified" and (.unresolved | length) == 0
' "$DOCUMENTATION_RECEIPT"
```

If the current receipt schema uses a later documented version, validate that
schema's equivalent identity/status fields instead of weakening the check.

| Observation | Decision |
| --- | --- |
| Receipt missing or invalid JSON | Block. |
| Candidate SHA differs | Block and regenerate for the frozen candidate. |
| `blocked`, `unverified`, partial, refusals, or unresolved claims | Block; preserve the stated limitation. |
| Version/tag or packaging decision differs | Block and reconcile documentation plus inventory. |
| Receipt is verified and bound to the frozen candidate | Accept provisionally; post-merge tree/SHA checks still apply. |

Store the receipt paths and SHA-256 hashes in the finalization receipt.

## 4. Locked local gates and workflow audit

Use the shared host lock and dedicated target directory for Cargo work. Keep
each command, exit code, candidate SHA, log path, and tool version in `gates`.

```bash
CARGO_TARGET_DIR=/Users/allen/git/codex-release-engineering-skills-target \
python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
  "$EVIDENCE_DIR/ci-standard.log" -- just ci-standard
python3 -m unittest tools.ci.test_release_workflow -v \
  2>&1 | tee "$EVIDENCE_DIR/release-workflow-tests.log"
actionlint -config-file .github/actionlint.yaml \
  .github/workflows/release.yml .github/workflows/docker.yml \
  2>&1 | tee "$EVIDENCE_DIR/actionlint.log"
```

If `actionlint` or the lock controller is unavailable, block rather than
substituting an unrecorded command. Audit `release.yml` and `docker.yml` for
triggers, permissions, pinned actions, main ancestry, pre-release/latest
classification, packaging decisions, and the asset matrix in the companion
reference. Secret names may be checked; do not read or print secret values.

## 5. Reviewed promotion to `main`

Push the release branch and open a PR with `main` as the base. A direct push to
`main` is never part of this skill.

```bash
gh pr create --base main --head "$(git branch --show-current)" \
  --title "release: prepare v${VERSION}" --body-file "$EVIDENCE_DIR/pr-body.md"
gh pr view "$PR" --json url,baseRefName,headRefOid,reviewDecision,mergeStateStatus,statusCheckRollup
```

Require review approval and all required checks. After the authorized merge:

```bash
git fetch origin main --tags
MAIN_SHA=$(gh pr view "$PR" --json mergedAt,mergeCommit --jq '.mergeCommit.oid')
test -n "$MAIN_SHA"
test "$(git rev-parse 'origin/main^{commit}')" = "$MAIN_SHA"
git merge-base --is-ancestor "$MAIN_SHA" origin/main
git show --no-patch --format=fuller "$MAIN_SHA" | tee "$EVIDENCE_DIR/main.txt"
```

Revalidate candidate-bound evidence:

```bash
CANDIDATE_TREE=$(git rev-parse "$CANDIDATE_SHA^{tree}")
MAIN_TREE=$(git rev-parse "$MAIN_SHA^{tree}")
```

| Candidate/main result | Required action |
| --- | --- |
| Trees differ | Invalidate both receipts; rerun them and every release gate against `MAIN_SHA`. |
| Trees match but commit SHAs differ | Preserve candidate receipts with the tree-equivalence proof; rerun commit-sensitive gates and require CI for `MAIN_SHA`. |
| Required CI is absent, pending, skipped unexpectedly, or failed | Block. |
| Exact `main` commit has successful required CI and compatible evidence | Proceed to tag dry run. |

Record the exact workflow run URLs and head SHA. A branch-level green badge or
the candidate PR checks are not proof for the merge commit.

## 6. Signed-tag dry run and authorization boundary

First prove the tag does not already exist locally or remotely. An existing tag
is a stop condition, not something to overwrite.

```bash
git rev-parse -q --verify "refs/tags/$TAG" && exit 1 || true
if git ls-remote --exit-code --tags origin "refs/tags/$TAG" >/dev/null 2>&1; then
  echo "refusing: remote tag already exists" >&2
  exit 1
fi
OXIDEX_TAG_DRY_RUN=1 just tag "$VERSION" "$MAIN_SHA" \
  2>&1 | tee "$EVIDENCE_DIR/tag-dry-run.log"
```

Stop here. Ask for explicit maintainer authorization that names the exact
version, tag, full `MAIN_SHA`, dry-run result, and packaging decisions. Earlier
authorization to prepare or merge does not count. Record the authorization
reference without copying sensitive material.

Immediately before the authorized push, re-fetch and repeat the tag-absence,
main-ancestry, version, CI, and receipt checks. Then and only then:

```bash
just tag "$VERSION" "$MAIN_SHA" 2>&1 | tee "$EVIDENCE_DIR/tag-push.log"
git fetch origin "refs/tags/$TAG:refs/tags/$TAG"
test "$(git rev-parse "refs/tags/$TAG^{}")" = "$MAIN_SHA"
git tag -v "$TAG" 2>&1 | tee "$EVIDENCE_DIR/tag-verification.log"
```

Do not put the real push command in an unattended script or combine it with
the dry run; the pause is a release authority boundary.

## Immutable-tag recovery

| Failure point | Recovery |
| --- | --- |
| Before any remote tag exists | Fix the candidate through the reviewed process, rerun receipts/gates/dry run, and request fresh authorization. |
| Push rejected and remote tag is still absent | Diagnose, repeat pre-push checks, and request renewed authorization before retrying. |
| Remote tag exists at the intended SHA; workflow failure is transient | Preserve the tag and evidence; rerun only the same immutable workflow inputs, then verify outputs. |
| Remote tag exists at a wrong/unknown SHA | Stop publication and escalate as an incident. Do not move or delete the tag. |
| Code, workflow, metadata, or artifact content must change after push | Fix forward on `main` and issue a new version/tag; never replace history under the old tag. |
