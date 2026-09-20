# GitHub release, artifact, and macOS verification

Use this reference only after the authorized tag push. Workflow configuration
is an expectation; the tag-triggered runs and downloaded artifacts are proof.
Continue using the unique durable `EVIDENCE_DIR` established by `gates.md`,
outside tracked repository content; do not redirect release evidence to
`/tmp` or delete the evidence tree after verification.

## Monitor exact-commit workflows

Resolve the repository and record the tag object before inspecting runs:

```bash
set -euo pipefail
REPO=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
git fetch origin --tags
test "$(git rev-parse "refs/tags/$TAG^{}")" = "$MAIN_SHA"
gh run list --workflow release.yml --commit "$MAIN_SHA" --event push \
  --json databaseId,url,headSha,headBranch,event,status,conclusion,workflowName \
  > "$EVIDENCE_DIR/release-runs.json"
gh run list --workflow docker.yml --commit "$MAIN_SHA" --event push \
  --json databaseId,url,headSha,headBranch,event,status,conclusion,workflowName \
  > "$EVIDENCE_DIR/docker-runs.json"
jq -e --arg tag "$TAG" --arg sha "$MAIN_SHA" --arg workflow "Release" '
  [.[] | select(
    .headBranch == $tag and .headSha == $sha and
    .workflowName == $workflow and .event == "push"
  )] | select(length == 1) | .[0]
' "$EVIDENCE_DIR/release-runs.json" > "$EVIDENCE_DIR/selected-release-run.json"
jq -e --arg tag "$TAG" --arg sha "$MAIN_SHA" --arg workflow "Docker" '
  [.[] | select(
    .headBranch == $tag and .headSha == $sha and
    .workflowName == $workflow and .event == "push"
  )] | select(length == 1) | .[0]
' "$EVIDENCE_DIR/docker-runs.json" > "$EVIDENCE_DIR/selected-docker-run.json"
RELEASE_RUN_ID=$(jq -er '.databaseId' "$EVIDENCE_DIR/selected-release-run.json")
DOCKER_RUN_ID=$(jq -er '.databaseId' "$EVIDENCE_DIR/selected-docker-run.json")
gh run watch "$RELEASE_RUN_ID" --exit-status
gh run view "$RELEASE_RUN_ID" --json \
  databaseId,url,headSha,headBranch,event,status,conclusion,workflowName \
  > "$EVIDENCE_DIR/final-release-run.json"
jq -e --argjson id "$RELEASE_RUN_ID" --arg tag "$TAG" \
  --arg sha "$MAIN_SHA" --arg workflow "Release" '
  select(
    .databaseId == $id and .headBranch == $tag and .headSha == $sha and
    .workflowName == $workflow and .event == "push" and
    .status == "completed" and .conclusion == "success"
  )
' "$EVIDENCE_DIR/final-release-run.json" > /dev/null
gh run watch "$DOCKER_RUN_ID" --exit-status
gh run view "$DOCKER_RUN_ID" --json \
  databaseId,url,headSha,headBranch,event,status,conclusion,workflowName \
  > "$EVIDENCE_DIR/final-docker-run.json"
jq -e --argjson id "$DOCKER_RUN_ID" --arg tag "$TAG" \
  --arg sha "$MAIN_SHA" --arg workflow "Docker" '
  select(
    .databaseId == $id and .headBranch == $tag and .headSha == $sha and
    .workflowName == $workflow and .event == "push" and
    .status == "completed" and .conclusion == "success"
  )
' "$EVIDENCE_DIR/final-docker-run.json" > /dev/null
```

The `jq -e` filters refuse zero or multiple matches; never select "latest"
silently. If a rerun creates multiple matching workflow runs, stop and record
an explicit unambiguous selection rule (for example a maintainer-named run ID)
before replacing the selected-run JSON. Require each selected run to name
`TAG`, `MAIN_SHA`, the expected workflow name, and the tag-push event. Record
its run ID, URL, status, conclusion, and expected skips. The pre-watch selection
record identifies the run but does not prove its outcome: only the fresh
`final-*-run.json` records may support a receipt, and both must pass the complete
identity plus `completed`/`success` assertions above before the receipt can
become `verified`. For a SemVer pre-release, the GitHub release must be a
prerelease, must not become Latest, stable docs must not be relabelled, and
Docker must publish only exact version tags (not `:latest`). For a stable
release, verify the stable behaviors separately.

```bash
set -euo pipefail
gh api "repos/$REPO/releases/tags/$TAG" \
  | jq -S '{html_url,tag_name,target_commitish,draft,prerelease,assets:
      [.assets[] | {name,size,url:.browser_download_url}]}' \
  | tee "$EVIDENCE_DIR/github-release.json"
gh api "repos/$REPO/releases/latest" --jq '.tag_name'
```

The Git tag peel, not `target_commitish` text alone, proves the released commit.

## Expected asset matrix

Derive the current expectation from the reviewed `release.yml`; update the
matrix if that workflow intentionally changes. With the release version in
`VERSION`, the current workflow expects:

| Asset | Platform / proof |
| --- | --- |
| `oxidex-x86_64-unknown-linux-musl` | Linux x86_64 static binary |
| `oxidex-aarch64-unknown-linux-musl` | Linux arm64 static binary |
| `oxidex-x86_64-pc-windows-gnu.exe` | Windows x86_64 binary |
| `oxidex-aarch64-apple-darwin` | Signed macOS arm64 binary |
| `oxidex-v${VERSION}.dmg` | Notarized and stapled macOS DMG containing the signed executable |

Require no missing, zero-byte, or unexpected assets. If checksums are
advertised or emitted, verify them; current absence of a checksum asset must be
recorded as `unverified`/not provided, never invented.

```bash
set -euo pipefail
mkdir -p "$EVIDENCE_DIR/assets"
gh release download "$TAG" --dir "$EVIDENCE_DIR/assets"
shasum -a 256 "$EVIDENCE_DIR"/assets/* | sort \
  | tee "$EVIDENCE_DIR/assets.sha256"
```

Record each asset's release URL, byte size, SHA-256, observed name, and status
in `artifacts`. Inspect the Docker manifest for both architectures when Docker
publication is in scope:

```bash
set -euo pipefail
docker buildx imagetools inspect "swackhamer/oxidex:$VERSION" \
  | tee "$EVIDENCE_DIR/docker-manifest.txt"
```

## Verify the downloaded macOS artifacts

Run these on macOS against the actual GitHub release downloads, not the CI
workspace artifact and not a locally rebuilt binary. First bind the downloads
to the selected release run. The current workflow uploads the signed binary
as `oxidex-aarch64-apple-darwin` and the stapled image as `oxidex-dmg`, then
copies those same files into the release. Preserve the API artifact manifest
(IDs, names, digests, expiry and workflow SHA), the run downloads, and their
file hashes. The API `digest`, when present, describes the artifact archive,
not the extracted executable; do not compare those unlike hashes.

```bash
set -euo pipefail
gh api --paginate --slurp "repos/$REPO/actions/runs/$RELEASE_RUN_ID/artifacts" \
  > "$EVIDENCE_DIR/release-run-artifact-pages.json"
jq -e --argjson run "$RELEASE_RUN_ID" --arg sha "$MAIN_SHA" '
  [.[] | .artifacts[] | select(
    .name == "oxidex-aarch64-apple-darwin" or .name == "oxidex-dmg"
  )] | select(length == 2)
  | select((map(.name) | unique | length) == 2)
  | select(all(.[]; .expired == false and
      .workflow_run.id == $run and .workflow_run.head_sha == $sha))
' "$EVIDENCE_DIR/release-run-artifact-pages.json" \
  > "$EVIDENCE_DIR/macos-run-artifact-manifest.json"
RUN_ARTIFACT_DIR="$EVIDENCE_DIR/release-run-$RELEASE_RUN_ID-artifacts"
test ! -e "$RUN_ARTIFACT_DIR"
mkdir "$RUN_ARTIFACT_DIR"
gh run download "$RELEASE_RUN_ID" --repo "$REPO" \
  --name oxidex-aarch64-apple-darwin --name oxidex-dmg --dir "$RUN_ARTIFACT_DIR"
MAC_BIN="$EVIDENCE_DIR/assets/oxidex-aarch64-apple-darwin"
DMG="$EVIDENCE_DIR/assets/oxidex-v${VERSION}.dmg"
RUN_MAC_BIN="$RUN_ARTIFACT_DIR/oxidex-aarch64-apple-darwin/oxidex-aarch64-apple-darwin"
RUN_DMG="$RUN_ARTIFACT_DIR/oxidex-dmg/oxidex-v${VERSION}.dmg"
test -s "$MAC_BIN" && test -s "$DMG" && test -s "$RUN_MAC_BIN" && test -s "$RUN_DMG"
shasum -a 256 "$MAC_BIN" "$RUN_MAC_BIN" "$DMG" "$RUN_DMG" \
  | tee "$EVIDENCE_DIR/macos-run-and-release.sha256"
cmp "$MAC_BIN" "$RUN_MAC_BIN"
cmp "$DMG" "$RUN_DMG"
```

Expired/missing/ambiguous run artifacts or unequal bytes block provenance;
a freshly calculated download hash alone does not prove a build's SHA. If
retention has expired, require an independently verifiable retained manifest
or attestation binding these exact file hashes to this run and `MAIN_SHA`;
do not create that missing provenance from the release downloads themselves.
This comparison establishes the GitHub run-to-release chain, not a claim that
the current workflow emits a signed build attestation.

Verify on a Mac capable of running the asset's architecture. The reviewed
`just create-dmg` recipe places one regular executable at `/oxidex` inside the
image. Mount read-only in a new isolated directory, require that layout, and
compare the payload bytes before executing either copy. Run the following
block in Bash; its subshell confines cleanup traps to this verification.

```bash
set -euo pipefail
: "${EXPECTED_DEVELOPER_ID:?Set the full expected Developer ID Application authority}"
: "${EXPECTED_TEAM_IDENTIFIER:?Set the expected Apple team identifier}"
(
DMG_MOUNT=$(mktemp -d "$EVIDENCE_DIR/dmg-mount.XXXXXX")
cleanup_macos_mount() {
  local verify_exit=$?
  trap - EXIT HUP INT TERM
  if hdiutil detach "$DMG_MOUNT" >> "$EVIDENCE_DIR/macos-cleanup.log" 2>&1; then
    rmdir "$DMG_MOUNT" >> "$EVIDENCE_DIR/macos-cleanup.log" 2>&1 || verify_exit=1
  else
    printf 'Detach failed; inspect and clean up mount point: %s\n' "$DMG_MOUNT" \
      >> "$EVIDENCE_DIR/macos-cleanup.log"
    verify_exit=1
  fi
  printf '%s\n' "$verify_exit" > "$EVIDENCE_DIR/macos-verification.exit"
  exit "$verify_exit"
}
trap cleanup_macos_mount EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
printf '%s\n' "$DMG_MOUNT" > "$EVIDENCE_DIR/macos-mount-point.txt"
hdiutil attach -readonly -nobrowse -noautoopen -plist \
  -mountpoint "$DMG_MOUNT" "$DMG" > "$EVIDENCE_DIR/macos-attach.plist" \
  2> "$EVIDENCE_DIR/macos-attach.stderr"
find "$DMG_MOUNT" -type f -name oxidex -print > "$EVIDENCE_DIR/macos-payload-paths.txt"
test "$(wc -l < "$EVIDENCE_DIR/macos-payload-paths.txt")" -eq 1
IFS= read -r MAC_DMG_PAYLOAD < "$EVIDENCE_DIR/macos-payload-paths.txt"
test "$MAC_DMG_PAYLOAD" = "$DMG_MOUNT/oxidex"
test -x "$MAC_DMG_PAYLOAD"
MAC_BIN_SHA256=$(shasum -a 256 "$MAC_BIN" | awk '{print $1}')
DMG_PAYLOAD_SHA256=$(shasum -a 256 "$MAC_DMG_PAYLOAD" | awk '{print $1}')
shasum -a 256 "$MAC_BIN" "$MAC_DMG_PAYLOAD" \
  | tee "$EVIDENCE_DIR/macos-payload.sha256"
test "$MAC_BIN_SHA256" = "$DMG_PAYLOAD_SHA256"
chmod u+x "$MAC_BIN"
file "$MAC_BIN" "$DMG" | tee "$EVIDENCE_DIR/macos-file.txt"
codesign --verify --strict --verbose=4 "$MAC_BIN" \
  2>&1 | tee "$EVIDENCE_DIR/macos-codesign-verify.txt"
codesign --display --verbose=4 "$MAC_BIN" \
  2>&1 | tee "$EVIDENCE_DIR/macos-codesign-display.txt"
codesign --verify --strict --verbose=4 "$MAC_DMG_PAYLOAD" \
  2>&1 | tee "$EVIDENCE_DIR/macos-payload-codesign-verify.txt"
codesign --display --verbose=4 "$MAC_DMG_PAYLOAD" \
  2>&1 | tee "$EVIDENCE_DIR/macos-payload-codesign-display.txt"
for signature in \
  "$EVIDENCE_DIR/macos-codesign-display.txt" \
  "$EVIDENCE_DIR/macos-payload-codesign-display.txt"
do
  grep -Fqx "Authority=$EXPECTED_DEVELOPER_ID" "$signature"
  grep -Fqx "TeamIdentifier=$EXPECTED_TEAM_IDENTIFIER" "$signature"
done
spctl --assess --type execute --verbose=4 "$MAC_BIN" \
  2>&1 | tee "$EVIDENCE_DIR/macos-gatekeeper-binary.txt"
spctl --assess --type execute --verbose=4 "$MAC_DMG_PAYLOAD" \
  2>&1 | tee "$EVIDENCE_DIR/macos-gatekeeper-payload.txt"
spctl --assess --type open --context context:primary-signature --verbose=4 "$DMG" \
  2>&1 | tee "$EVIDENCE_DIR/macos-gatekeeper-dmg.txt"
xcrun stapler validate -v "$DMG" \
  2>&1 | tee "$EVIDENCE_DIR/macos-stapler.txt"
"$MAC_BIN" --version | tee "$EVIDENCE_DIR/macos-binary-version.txt"
"$MAC_DMG_PAYLOAD" --version | tee "$EVIDENCE_DIR/macos-payload-version.txt"
test "$(<"$EVIDENCE_DIR/macos-binary-version.txt")" = "oxidex $VERSION"
test "$(<"$EVIDENCE_DIR/macos-payload-version.txt")" = "oxidex $VERSION"
"$MAC_BIN" --help > "$EVIDENCE_DIR/macos-binary-help.txt" 2>&1
"$MAC_DMG_PAYLOAD" --help > "$EVIDENCE_DIR/macos-payload-help.txt" 2>&1
)
```

All checks and cleanup must exit zero. The trap detaches only the new mount
and removes only its empty mount-point directory; downloads and evidence are
retained. A failed attach may leave an empty directory; a failed detach leaves
the mount for explicit diagnosis, never recursive deletion or forced detach.
The command block requires the exact expected Developer ID authority and
TeamIdentifier in both executable signatures. Also inspect the display output
for hardened runtime and a trusted timestamp without recording private keys or
credentials. Set `macos_verification.status` to
`verified` only when the manifest/run SHA, run-to-release comparison, payload
hash match, expected version, signature/Gatekeeper/ticket checks and cleanup
are all evidenced. Record the host OS/architecture and every command's result.
The raw binary has no stapled DMG ticket of its own: do not claim it inherits
the image's notarization evidence without the payload hash match, and report
its own Gatekeeper assessment separately even when the bytes match.

| Evidence | Allowed claim |
| --- | --- |
| YAML mentions `codesign` or `notarytool` | Workflow path exists; credentials and artifact remain unverified. |
| CI signing/notarization step is green | CI reports success; released download remains unverified. |
| `codesign` passes only | Signature verified; Gatekeeper and notarization still unverified. |
| Run provenance and payload hash match, expected version/basic invocation, both executable signature/Gatekeeper checks, DMG `spctl` and `stapler validate` all pass | These exact release downloads are verified on the recorded Mac; the notarized DMG contains the byte-identical released binary. |

If any check fails, preserve the command output and hashes, set the release
receipt to blocked, and use the immutable-tag recovery table in `gates.md`.
Do not replace a failed asset under the same release and call the old evidence
valid.

## Final receipt decision

Set the final receipt to `verified` only if all of the following agree:

- the signed tag peels to `MAIN_SHA` and is reachable from `main`;
- both required workflow runs for `MAIN_SHA` concluded successfully;
- GitHub prerelease/latest and Docker tags match the release classification;
- the exact expected asset set exists and its hashes are recorded;
- downloaded macOS artifacts match the selected run manifest/provenance, the
  DMG payload matches the raw executable, both report the release version and
  pass basic invocation, their exact Developer ID and TeamIdentifier match the
  recorded expectations, and signature, Gatekeeper and DMG stapled-ticket
  checks pass with successful mount cleanup;
- every gate/workflow/artifact entry carries durable evidence.

Otherwise preserve the most specific non-verified status and name one safe
`next_action`; never summarize partial success as a completed release.

After populating `FINALIZATION_RECEIPT` from the durable evidence above, run
the terminal gate. A hand-edited `status: verified` is not a completion signal:

```bash
set -euo pipefail
python3 tools/ci/validate_release_receipt.py --kind finalization \
  --receipt "$FINALIZATION_RECEIPT" --version "$VERSION" \
  --candidate-sha "$CANDIDATE_SHA"
```

Only exit zero from this exact command permits the release to be reported as
verified. Preserve a nonzero result and its field-level diagnostics as a
blocked final receipt.
