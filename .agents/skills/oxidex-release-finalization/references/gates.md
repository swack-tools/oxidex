# Candidate, gate, promotion, and tag procedure

Use this reference through the tag push. It contains commands that mutate
remote state; inspection and dry-run steps do not authorize the real mutation.

## 1. Preflight and freeze

Work in a dedicated release worktree and branch. Before edits or remote
commands:

```bash
set -euo pipefail
VERSION=2.0.0-beta.1
TAG="v$VERSION"
PARITY_RECEIPT=/absolute/path/to/release-parity-receipt.json
DOCUMENTATION_RECEIPT=/absolute/path/to/documentation-release-receipt.json
tools/preflight.sh --upstream
git fetch origin main refactor/tag-machinery --tags
git status --short --branch
git rev-parse --show-toplevel
```

Create a new evidence directory under a user-provided durable root outside the
tracked repository. A repo-adjacent sibling is acceptable; `/tmp` is not. Use a
new run ID and refuse a pre-existing path rather than deleting or reusing it.

```bash
set -euo pipefail
REPO_ROOT=$(git rev-parse --show-toplevel)
EVIDENCE_ROOT=/absolute/durable/evidence/root
RUN_ID=20260918T120000Z-unique-suffix
case "$EVIDENCE_ROOT" in
  "$REPO_ROOT"|"$REPO_ROOT"/*|/tmp|/tmp/*)
    echo "refusing: evidence root must be durable and outside tracked repository content" >&2
    exit 1
    ;;
esac
EVIDENCE_DIR="$EVIDENCE_ROOT/oxidex-release-${TAG}-${RUN_ID}"
test ! -e "$EVIDENCE_DIR"
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
set -euo pipefail
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
set -euo pipefail
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
set -euo pipefail
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
set -euo pipefail
PR_URL=$(gh pr create --base main --head "$(git branch --show-current)" \
  --title "release: prepare v${VERSION}" --body-file "$EVIDENCE_DIR/pr-body.md")
test -n "$PR_URL"
printf '%s\n' "$PR_URL" | tee "$EVIDENCE_DIR/pr-url.txt"
PR=$(gh pr view "$PR_URL" --json number --jq '.number')
test -n "$PR"
gh pr view "$PR" --json url,baseRefName,headRefOid,reviewDecision,mergeStateStatus,statusCheckRollup
gh pr checks "$PR" --required
```

Require review approval and all required checks. After the authorized merge:

```bash
set -euo pipefail
git fetch origin main --tags
MAIN_SHA=$(gh pr view "$PR" --json mergedAt,mergeCommit --jq '.mergeCommit.oid')
test -n "$MAIN_SHA"
test "$(git rev-parse 'origin/main^{commit}')" = "$MAIN_SHA"
git merge-base --is-ancestor "$MAIN_SHA" origin/main
git show --no-patch --format=fuller "$MAIN_SHA" | tee "$EVIDENCE_DIR/main.txt"
```

Revalidate candidate-bound evidence:

```bash
set -euo pipefail
CANDIDATE_TREE=$(git rev-parse "$CANDIDATE_SHA^{tree}")
MAIN_TREE=$(git rev-parse "$MAIN_SHA^{tree}")
MAIN_EVIDENCE_DIR="$EVIDENCE_DIR/main-$MAIN_SHA"
POST_MERGE_WORKTREE="$EVIDENCE_ROOT/worktrees/oxidex-release-main-$MAIN_SHA"
MAIN_CARGO_TARGET_DIR="$EVIDENCE_ROOT/cargo-targets/oxidex-release-main-$MAIN_SHA"
test ! -e "$MAIN_EVIDENCE_DIR"
test ! -e "$POST_MERGE_WORKTREE"
test ! -e "$MAIN_CARGO_TARGET_DIR"
mkdir -p "$MAIN_EVIDENCE_DIR" "$(dirname "$POST_MERGE_WORKTREE")" \
  "$(dirname "$MAIN_CARGO_TARGET_DIR")"
git worktree add --detach "$POST_MERGE_WORKTREE" "$MAIN_SHA"
MAIN_HEAD=$(git -C "$POST_MERGE_WORKTREE" rev-parse 'HEAD^{commit}')
test "$MAIN_HEAD" = "$MAIN_SHA"
```

All post-merge reruns execute from `POST_MERGE_WORKTREE`, never from the
candidate worktree. Before each receipt or gate command, assert its `HEAD` is
still `MAIN_SHA`. Use `MAIN_EVIDENCE_DIR` for outputs and
`MAIN_CARGO_TARGET_DIR` for Cargo products. Do not remove these directories as
part of the release procedure; they are durable evidence.

```bash
set -euo pipefail
(
  cd "$POST_MERGE_WORKTREE"
  test "$(git rev-parse 'HEAD^{commit}')" = "$MAIN_SHA"
  CARGO_TARGET_DIR="$MAIN_CARGO_TARGET_DIR" \
  python3 /Users/allen/oxidex-ops/evidence/20260917-group1-batch2/locked.py --shared \
    "$MAIN_EVIDENCE_DIR/ci-standard.log" -- just ci-standard
  python3 -m unittest tools.ci.test_release_workflow -v \
    2>&1 | tee "$MAIN_EVIDENCE_DIR/release-workflow-tests.log"
  actionlint -config-file .github/actionlint.yaml \
    .github/workflows/release.yml .github/workflows/docker.yml \
    2>&1 | tee "$MAIN_EVIDENCE_DIR/actionlint.log"
)
```

If the trees differ, regenerate both the parity and documentation receipts
from this same detached worktree, writing them under `MAIN_EVIDENCE_DIR`, then
validate their SHA fields against `MAIN_SHA` before rerunning every release
gate above. If the trees match, preserve the candidate receipts only with the
recorded tree-equivalence proof; the exact-`main` gate and CI evidence still
comes from this detached worktree and `MAIN_SHA`.

| Candidate/main result | Required action |
| --- | --- |
| Trees differ | Invalidate both receipts; rerun them and every release gate from `POST_MERGE_WORKTREE` after asserting `HEAD == MAIN_SHA`. |
| Trees match but commit SHAs differ | Preserve candidate receipts with the tree-equivalence proof; rerun commit-sensitive gates from `POST_MERGE_WORKTREE` and require CI for `MAIN_SHA`. |
| Required CI is absent, pending, skipped unexpectedly, or failed | Block. |
| Exact `main` commit has successful required CI and compatible evidence | Proceed to tag dry run. |

Record the exact workflow run URLs and head SHA. A branch-level green badge or
the candidate PR checks are not proof for the merge commit.

## 6. Signed-tag dry run and authorization boundary

First prove the tag does not already exist locally or remotely. An existing tag
is a stop condition, not something to overwrite.

```bash
set -euo pipefail
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
set -euo pipefail
just tag "$VERSION" "$MAIN_SHA" 2>&1 | tee "$EVIDENCE_DIR/tag-push.log"
git fetch origin "refs/tags/$TAG:refs/tags/$TAG"
test "$(git rev-parse "refs/tags/$TAG^{}")" = "$MAIN_SHA"
if [ "$(git config --get gpg.format)" = "ssh" ]; then
  SIGNING_KEY=$(git config --get user.signingkey)
  [ -f "$SIGNING_KEY" ] && SIGNING_KEY=$(<"$SIGNING_KEY")
  ALLOWED_SIGNERS="$EVIDENCE_DIR/tag-allowed-signers"
  printf '%s %s\n' "$(git config --get user.email)" "$SIGNING_KEY" > "$ALLOWED_SIGNERS"
  VERIFY=(git -c gpg.ssh.allowedSignersFile="$ALLOWED_SIGNERS" tag -v "$TAG")
else
  VERIFY=(git tag -v "$TAG")
fi
"${VERIFY[@]}" 2>&1 | tee "$EVIDENCE_DIR/tag-verification.log"
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
