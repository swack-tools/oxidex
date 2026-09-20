# Candidate, gate, promotion, and tag procedure

Use this reference through the tag push. It contains commands that mutate
remote state; inspection and dry-run steps do not authorize the real mutation.

## 1. Preflight and freeze

Work in a dedicated release worktree and branch. Before edits or remote
commands:

```bash
set -euo pipefail
: "${VERSION:?Set the release version without a v prefix}"
: "${RELEASE_LOCK:?Set the absolute shared lock controller path}"
test -f "$RELEASE_LOCK"
TAG="v$VERSION"
PARITY_RECEIPT=/absolute/path/to/release-parity-receipt.json
DOCUMENTATION_RECEIPT=/absolute/path/to/documentation-release-receipt.json
FINALIZATION_RECEIPT=/absolute/path/to/release-finalization-receipt.json
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
CANDIDATE_CARGO_TARGET_DIR="$EVIDENCE_DIR/cargo-target-candidate"
test ! -e "$CANDIDATE_CARGO_TARGET_DIR"
mkdir "$CANDIDATE_CARGO_TARGET_DIR"
printf '%s\n' "$CANDIDATE_CARGO_TARGET_DIR" | tee "$EVIDENCE_DIR/candidate-cargo-target.txt"
git show --no-patch --format=fuller "$CANDIDATE_SHA" | tee "$EVIDENCE_DIR/candidate.txt"
git status --porcelain=v1 | tee "$EVIDENCE_DIR/status.txt"
```

Require a clean tree. Record the full SHA and evidence directory in every
receipt and log; `HEAD`, "tip", branch names, and abbreviated SHAs are not
release identities. Record `CANDIDATE_CARGO_TARGET_DIR` in the receipt and use
it only for this release run's candidate worktree; the shared lock permits
concurrent builds and does not provide artifact isolation.

## 2. Complete version inventory

Inventory Cargo workspace members rather than assuming every crate shares the
root version. Preserve intentional independent versions such as
`oxidex-tags-shared` and non-publishable `0.0.0` utility crates.

```bash
set -euo pipefail
cargo metadata --no-deps --format-version 1 \
  | jq -S '{workspace_members, packages: [.packages[] | {name, version, manifest_path}]}' \
  | tee "$EVIDENCE_DIR/workspace-versions.json"
```

Search every tracked text file for version literals independently of the new
`VERSION`. This includes older release/archive URLs, prose, packaging and
dependency pins; a search for only the requested version misses stale values.

```bash
set -euo pipefail
VERSION_LITERAL_RE='(^|[^[:alnum:]_])[vV]?[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?'
git grep -n -I -E "$VERSION_LITERAL_RE" -- . \
  ':(exclude)tools/ci/testdata/release_receipts/**' \
  | tee "$EVIDENCE_DIR/version-literals.txt"
```

Also inventory computed/field-based version sources and validate the tag:

```bash
set -euo pipefail
git grep -n -I -E '\[package\]|version[[:space:]]*=|VERSION|__version__' -- . \
  ':(exclude)tools/ci/testdata/release_receipts/**' \
  | tee "$EVIDENCE_DIR/version-fields.txt"
python3 tools/ci/release_version.py --tag "$TAG" --cargo-toml Cargo.toml \
  | tee "$EVIDENCE_DIR/release-version.txt"
```

Reconcile every hit in `version_inventory` against the packaging decision and
documentation receipt. Record path/line, literal, kind (OxiDex package,
user-facing release, independent package, dependency or unrelated numeric
literal), disposition (`current`, `historical`, `independent` or `excluded`),
reason and supporting evidence. Unresolved hits block completion. Current
OxiDex versions must equal `VERSION`; historical references need visible
historical context. Exclusions require an explicit scope reason. For example,
`packaging/homebrew/oxidex.rb` contains a `v0.1.0.tar.gz` archive URL and a
placeholder checksum: inventory it and decide whether that packaging is in
scope; neither automatic replacement nor a claim of a published formula is
justified by finding it. Keep dependency pins, including those in `Cargo.lock`,
separate from OxiDex release versions; preserve intentional independent crate
versions and retain the excluded/dependency hit list for audit.

The final receipt's `version_inventory` must cover every Cargo workspace
manifest and bind each row to its actual `[package]` version declaration.
Record the complete `version-literals.txt` and `version-fields.txt`
reconciliation separately, including both scan SHA-256 values, tracked-file
and matching-line counts, reviewer, and zero unresolved entries, in
`version_reconciliation`. Both documented scans and the terminal validator
exclude only the validator's self-referential receipt fixtures. The terminal
validator recomputes both scans, loads the referenced reconciliation bytes,
and requires all counts and hashes to agree; a summary without both raw durable
scans and human reconciliation is not evidence.

## 3. Receipt compatibility

Parse both JSON receipts before trusting them. The parity receipt binds the
candidate in `oxidex_sha`; the documentation receipt binds it in
`candidate_sha`.

```bash
set -euo pipefail
python3 tools/ci/validate_release_receipt.py --kind parity \
  --receipt "$PARITY_RECEIPT" --version "$VERSION" \
  --candidate-sha "$CANDIDATE_SHA"
python3 tools/ci/validate_release_receipt.py --kind documentation \
  --receipt "$DOCUMENTATION_RECEIPT" --version "$VERSION" \
  --candidate-sha "$CANDIDATE_SHA"
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

Store each receipt's path, SHA-256, measured commit, and measured tree in the
finalization receipt. When candidate and `main` trees match, the measured
commit may remain the candidate SHA with that tree-equivalence proof. When the
trees differ, both upstream receipts must name the regenerated `MAIN_SHA` and
`MAIN_TREE`.

## 4. Locked local gates and workflow audit

Use the shared host lock and dedicated target directory for Cargo work. Keep
each command, exit code, candidate SHA, log path, and tool version in `gates`.

```bash
set -euo pipefail
CARGO_TARGET_DIR="$CANDIDATE_CARGO_TARGET_DIR" \
python3 "$RELEASE_LOCK" --shared \
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
gh pr view "$PR" --json url,baseRefName,headRefOid,reviewDecision,mergeStateStatus,statusCheckRollup \
  | tee "$EVIDENCE_DIR/pr-state.json"
gh pr checks "$PR" --required --json name,state,link \
  | tee "$EVIDENCE_DIR/required-checks.json"
REPO_OWNER=$(gh repo view --json owner --jq '.owner.login')
REPO_NAME=$(gh repo view --json name --jq '.name')
gh api graphql \
  -F owner="$REPO_OWNER" -F name="$REPO_NAME" -F number="$PR" \
  -f query='query($owner:String!,$name:String!,$number:Int!){
    repository(owner:$owner,name:$name){
      pullRequest(number:$number){
        reviewThreads(first: 100){
          pageInfo{hasNextPage}
          nodes{isResolved isOutdated comments(first:1){nodes{url body author{login}}}}
        }
      }
    }
  }' > "$EVIDENCE_DIR/review-threads.json"
python3 tools/ci/release_pr_gate.py \
  --pr-state "$EVIDENCE_DIR/pr-state.json" \
  --review-threads "$EVIDENCE_DIR/review-threads.json" \
  --required-checks "$EVIDENCE_DIR/required-checks.json" \
  --expected-head "$CANDIDATE_SHA" \
  --output "$EVIDENCE_DIR/reviewed-promotion.json"
jq -er '.unresolved_actionable_threads' "$EVIDENCE_DIR/reviewed-promotion.json" \
  | tee "$EVIDENCE_DIR/unresolved-actionable-review-threads.txt"
```

Require review approval, all required checks, a complete review-thread page,
and zero unresolved non-outdated review threads. The verified promotion JSON
also records SHA-256 identities for both raw inputs. A general review decision
does not prove that inline comments were resolved. After the authorized merge:

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
  python3 "$RELEASE_LOCK" --shared \
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
