# GitHub release, artifact, and macOS verification

Use this reference only after the authorized tag push. Workflow configuration
is an expectation; the tag-triggered runs and downloaded artifacts are proof.

## Monitor exact-commit workflows

Resolve the repository and record the tag object before inspecting runs:

```bash
REPO=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
git fetch origin --tags
test "$(git rev-parse "refs/tags/$TAG^{}")" = "$MAIN_SHA"
gh run list --workflow release.yml --commit "$MAIN_SHA" --event push \
  --json databaseId,url,headSha,event,status,conclusion,workflowName
gh run list --workflow docker.yml --commit "$MAIN_SHA" --event push \
  --json databaseId,url,headSha,event,status,conclusion,workflowName
gh run watch "$RELEASE_RUN_ID" --exit-status
gh run watch "$DOCKER_RUN_ID" --exit-status
```

Require each selected run to name `MAIN_SHA` and the tag-push event. Record its
run ID, URL, status, conclusion, and expected skips. For a SemVer pre-release,
the GitHub release must be a prerelease, must not become Latest, stable docs
must not be relabelled, and Docker must publish only exact version tags (not
`:latest`). For a stable release, verify the stable behaviors separately.

```bash
gh api "repos/$REPO/releases/tags/$TAG" \
  | jq -S '{html_url,tag_name,target_commitish,draft,prerelease,assets:
      [.assets[] | {name,size,url:.browser_download_url}]}' \
  | tee "$EVIDENCE_DIR/github-release.json"
gh api "repos/$REPO/releases/latest" --jq '.tag_name'
```

The Git tag peel, not `target_commitish` text alone, proves the released commit.

## Expected asset matrix

Derive the current expectation from the reviewed `release.yml`; update the
matrix if that workflow intentionally changes. For v2.0.0-beta.1 it is:

| Asset | Platform / proof |
| --- | --- |
| `oxidex-x86_64-unknown-linux-musl` | Linux x86_64 static binary |
| `oxidex-aarch64-unknown-linux-musl` | Linux arm64 static binary |
| `oxidex-x86_64-pc-windows-gnu.exe` | Windows x86_64 binary |
| `oxidex-aarch64-apple-darwin` | Signed macOS arm64 binary |
| `oxidex-v2.0.0-beta.1.dmg` | Signed, notarized, stapled macOS disk image |

Require no missing, zero-byte, or unexpected assets. If checksums are
advertised or emitted, verify them; current absence of a checksum asset must be
recorded as `unverified`/not provided, never invented.

```bash
mkdir -p "$EVIDENCE_DIR/assets"
gh release download "$TAG" --dir "$EVIDENCE_DIR/assets"
shasum -a 256 "$EVIDENCE_DIR"/assets/* | sort \
  | tee "$EVIDENCE_DIR/assets.sha256"
```

Record each asset's release URL, byte size, SHA-256, observed name, and status
in `artifacts`. Inspect the Docker manifest for both architectures when Docker
publication is in scope:

```bash
docker buildx imagetools inspect "swackhamer/oxidex:$VERSION" \
  | tee "$EVIDENCE_DIR/docker-manifest.txt"
```

## Verify the downloaded macOS artifacts

Run these on macOS against the actual GitHub release downloads, not the CI
workspace artifact and not a locally rebuilt binary.

```bash
MAC_BIN="$EVIDENCE_DIR/assets/oxidex-aarch64-apple-darwin"
DMG="$EVIDENCE_DIR/assets/oxidex-v${VERSION}.dmg"
file "$MAC_BIN" "$DMG" | tee "$EVIDENCE_DIR/macos-file.txt"
codesign --verify --strict --verbose=4 "$MAC_BIN" \
  2>&1 | tee "$EVIDENCE_DIR/macos-codesign-verify.txt"
codesign --display --verbose=4 "$MAC_BIN" \
  2>&1 | tee "$EVIDENCE_DIR/macos-codesign-display.txt"
spctl --assess --type open --context context:primary-signature --verbose=4 "$DMG" \
  2>&1 | tee "$EVIDENCE_DIR/macos-gatekeeper-dmg.txt"
xcrun stapler validate -v "$DMG" \
  2>&1 | tee "$EVIDENCE_DIR/macos-stapler.txt"
```

All commands must exit zero. Inspect the display output for the expected
Developer ID identity, hardened runtime, timestamp, and TeamIdentifier without
recording private keys or credentials. Set `macos_verification.status` to
`verified` only when the hashes bind these results to the release assets.

| Evidence | Allowed claim |
| --- | --- |
| YAML mentions `codesign` or `notarytool` | Workflow path exists; credentials and artifact remain unverified. |
| CI signing/notarization step is green | CI reports success; released download remains unverified. |
| `codesign` passes only | Signature verified; Gatekeeper and notarization still unverified. |
| `codesign`, the DMG `spctl` assessment, and `stapler validate` pass on hashed downloads | Released macOS assets are Apple verified for this receipt. |

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
- downloaded macOS artifacts pass signature, Gatekeeper, and stapled-ticket
  verification;
- every gate/workflow/artifact entry carries durable evidence.

Otherwise preserve the most specific non-verified status and name one safe
`next_action`; never summarize partial success as a completed release.
