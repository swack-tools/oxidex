# Multi-Platform Release Workflow Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Create a unified release.yml workflow that builds, signs, and releases binaries for all platforms (Windows x64, Linux musl x64/aarch64, macOS aarch64 with signed binary and notarized DMG) triggered on git tags.

**Architecture:** Port Mac signing/notarization from mac-sign.yml into a new release.yml workflow. Build cross-platform binaries, sign macOS artifacts, create GitHub release with all artifacts using softprops/action-gh-release@v2.

**Tech Stack:** GitHub Actions, Rust cross-compilation, macOS codesign, Apple notarization, create-dmg

---

## Task 1: Create release.yml workflow skeleton

**Files:**
- Create: `.github/workflows/release.yml`

**Step 1: Create basic release workflow structure**

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

jobs:
  # Placeholder - will add jobs in next tasks
  build-linux:
    name: Build Linux (${{ matrix.target }})
    runs-on: ubuntu-latest
    strategy:
      matrix:
        target:
          - x86_64-unknown-linux-musl
          - aarch64-unknown-linux-musl
    steps:
      - run: echo "Placeholder"
```

**Step 2: Verify syntax**

Run: `cat .github/workflows/release.yml`
Expected: Valid YAML

**Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "feat: add release.yml workflow skeleton"
```

---

## Task 2: Add Linux build job to release.yml

**Files:**
- Modify: `.github/workflows/release.yml`

**Step 1: Implement Linux build matrix job**

Replace the placeholder with:

```yaml
  build-linux:
    name: Build Linux (${{ matrix.target }})
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      matrix:
        target:
          - x86_64-unknown-linux-musl
          - aarch64-unknown-linux-musl
    steps:
      - name: Checkout repository
        uses: actions/checkout@v4

      - name: Setup Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Install musl tools
        run: |
          sudo apt-get update -qq
          sudo apt-get install -y musl-tools musl-dev

      - name: Install cross-compilation tools for ARM64
        if: matrix.target == 'aarch64-unknown-linux-musl'
        run: |
          sudo apt-get install -y gcc-aarch64-linux-gnu

      - name: Build release binary
        run: cargo build --release --target ${{ matrix.target }}

      - name: Strip binary
        run: |
          if [ "${{ matrix.target }}" = "aarch64-unknown-linux-musl" ]; then
            aarch64-linux-gnu-strip target/${{ matrix.target }}/release/oxidex
          else
            strip target/${{ matrix.target }}/release/oxidex
          fi

      - name: Create artifact directory
        run: mkdir -p artifacts

      - name: Copy binary to artifacts
        run: |
          cp target/${{ matrix.target }}/release/oxidex artifacts/oxidex-${{ matrix.target }}

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: oxidex-${{ matrix.target }}
          path: artifacts/oxidex-${{ matrix.target }}
          retention-days: 1
```

**Step 2: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "feat: add Linux build job to release workflow"
```

---

## Task 3: Add Windows build job to release.yml

**Files:**
- Modify: `.github/workflows/release.yml`

**Step 1: Add Windows build job after Linux job**

```yaml
  build-windows:
    name: Build Windows (x86_64)
    runs-on: windows-latest
    steps:
      - name: Checkout repository
        uses: actions/checkout@v4

      - name: Setup Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Build release binary
        run: cargo build --release --target x86_64-pc-windows-msvc

      - name: Create artifact directory
        run: mkdir artifacts

      - name: Copy binary to artifacts
        run: |
          copy target\x86_64-pc-windows-msvc\release\oxidex.exe artifacts\oxidex-x86_64-pc-windows-msvc.exe

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: oxidex-x86_64-pc-windows-msvc
          path: artifacts/oxidex-x86_64-pc-windows-msvc.exe
          retention-days: 1
```

**Step 2: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "feat: add Windows build job to release workflow"
```

---

## Task 4: Port macOS signing and notarization to release.yml

**Files:**
- Read: `.github/workflows/mac-sign.yml` (for reference)
- Modify: `.github/workflows/release.yml`

**Step 1: Add macOS build and sign job**

```yaml
  build-macos:
    name: Build and Sign macOS (aarch64)
    runs-on: warp-macos-15-arm64-6x
    permissions:
      contents: write
    steps:
      - name: Checkout code
        uses: actions/checkout@v4

      - name: Extract version from tag
        id: version
        run: |
          VERSION=${GITHUB_REF#refs/tags/v}
          echo "version=$VERSION" >> $GITHUB_OUTPUT
          echo "Version: $VERSION"

      - name: Install dependencies
        run: |
          brew install just create-dmg

      - name: Import signing certificate
        env:
          BUILD_CERTIFICATE_BASE64: ${{ secrets.BUILD_CERTIFICATE_BASE64 }}
          P12_PASSWORD: ${{ secrets.P12_PASSWORD }}
          KEYCHAIN_PASSWORD: ${{ secrets.KEYCHAIN_PASSWORD }}
        run: |
          CERTIFICATE_PATH=$RUNNER_TEMP/build_certificate.p12
          KEYCHAIN_PATH=$RUNNER_TEMP/app-signing.keychain-db

          echo -n "$BUILD_CERTIFICATE_BASE64" | base64 --decode -o $CERTIFICATE_PATH

          security create-keychain -p "$KEYCHAIN_PASSWORD" $KEYCHAIN_PATH
          security set-keychain-settings -lut 21600 $KEYCHAIN_PATH
          security unlock-keychain -p "$KEYCHAIN_PASSWORD" $KEYCHAIN_PATH

          security import $CERTIFICATE_PATH -P "$P12_PASSWORD" -A -t cert -f pkcs12 -k $KEYCHAIN_PATH
          security list-keychain -d user -s $KEYCHAIN_PATH
          security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KEYCHAIN_PASSWORD" $KEYCHAIN_PATH

          echo "Available signing identities:"
          security find-identity -v -p codesigning $KEYCHAIN_PATH

      - name: Build release
        env:
          DEVELOPMENT_TEAM: ${{ secrets.DEVELOPMENT_TEAM }}
        run: |
          just build-release

      - name: Sign binary
        env:
          KEYCHAIN_PASSWORD: ${{ secrets.KEYCHAIN_PASSWORD }}
        run: |
          APP_PATH="./target/release/oxidex"
          KEYCHAIN_PATH=$RUNNER_TEMP/app-signing.keychain-db

          security unlock-keychain -p "$KEYCHAIN_PASSWORD" $KEYCHAIN_PATH

          IDENTITY=$(security find-identity -v -p codesigning $KEYCHAIN_PATH | grep "Developer ID Application" | head -1 | grep -o '".*"' | tr -d '"')

          if [ -z "$IDENTITY" ]; then
            echo "Error: No Developer ID Application certificate found"
            exit 1
          fi

          echo "Found signing identity: $IDENTITY"
          echo "Signing binary with Developer ID certificate..."
          codesign --sign "$IDENTITY" \
            --timestamp \
            --options runtime \
            --verbose \
            --keychain $KEYCHAIN_PATH \
            "$APP_PATH"

          echo "Verifying signature..."
          codesign --verify --verbose "$APP_PATH"

          echo "Displaying signature details..."
          codesign --display --verbose=4 "$APP_PATH"

          echo "✓ Binary signed successfully"

      - name: Create DMG
        run: |
          just create-dmg v${{ steps.version.outputs.version }}

      - name: Notarize DMG
        env:
          NOTARIZATION_APPLE_ID: ${{ secrets.NOTARIZATION_APPLE_ID }}
          NOTARIZATION_PASSWORD: ${{ secrets.NOTARIZATION_PASSWORD }}
          NOTARIZATION_TEAM_ID: ${{ secrets.NOTARIZATION_TEAM_ID }}
        run: |
          DMG_PATH="dist/oxidex-v${{ steps.version.outputs.version }}.dmg"

          echo "Submitting DMG for notarization..."
          SUBMISSION_OUTPUT=$(xcrun notarytool submit "$DMG_PATH" \
            --apple-id "$NOTARIZATION_APPLE_ID" \
            --password "$NOTARIZATION_PASSWORD" \
            --team-id "$NOTARIZATION_TEAM_ID" \
            --wait 2>&1)

          echo "$SUBMISSION_OUTPUT"

          SUBMISSION_ID=$(echo "$SUBMISSION_OUTPUT" | grep -E "^\s+id:" | head -1 | awk '{print $2}' | tr -d '[:space:]')

          echo "Extracted submission ID: $SUBMISSION_ID"

          if echo "$SUBMISSION_OUTPUT" | grep -q "status: Accepted"; then
            echo "✓ Notarization accepted"
          else
            echo "❌ Notarization failed, fetching log..."
            xcrun notarytool log "$SUBMISSION_ID" developer_log.json \
              --apple-id "$NOTARIZATION_APPLE_ID" \
              --password "$NOTARIZATION_PASSWORD" \
              --team-id "$NOTARIZATION_TEAM_ID"

            echo "Notarization error details:"
            cat developer_log.json
            exit 1
          fi

          echo "Stapling notarization ticket to DMG..."
          xcrun stapler staple "$DMG_PATH"

          echo "Verifying DMG notarization..."
          xcrun stapler validate "$DMG_PATH"

          echo "✓ DMG notarized and stapled successfully"

      - name: Create artifact directory
        run: mkdir -p artifacts

      - name: Copy signed binary and DMG to artifacts
        run: |
          cp target/release/oxidex artifacts/oxidex-aarch64-apple-darwin
          cp dist/oxidex-v${{ steps.version.outputs.version }}.dmg artifacts/

      - name: Upload signed binary artifact
        uses: actions/upload-artifact@v4
        with:
          name: oxidex-aarch64-apple-darwin
          path: artifacts/oxidex-aarch64-apple-darwin
          retention-days: 1

      - name: Upload DMG artifact
        uses: actions/upload-artifact@v4
        with:
          name: oxidex-dmg
          path: artifacts/oxidex-v${{ steps.version.outputs.version }}.dmg
          retention-days: 1

      - name: Clean up keychain
        if: always()
        run: |
          security delete-keychain $RUNNER_TEMP/app-signing.keychain-db || true
```

**Step 2: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "feat: add macOS build, sign, and notarize job to release workflow"
```

---

## Task 5: Add GitHub release creation job

**Files:**
- Modify: `.github/workflows/release.yml`

**Step 1: Add release job that depends on all build jobs**

```yaml
  create-release:
    name: Create GitHub Release
    needs: [build-linux, build-windows, build-macos]
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - name: Checkout repository
        uses: actions/checkout@v4

      - name: Download all artifacts
        uses: actions/download-artifact@v4
        with:
          path: artifacts

      - name: Display artifact structure
        run: ls -R artifacts

      - name: Prepare release assets
        run: |
          mkdir -p release-assets
          # Linux x64
          cp artifacts/oxidex-x86_64-unknown-linux-musl/oxidex-x86_64-unknown-linux-musl release-assets/
          # Linux ARM64
          cp artifacts/oxidex-aarch64-unknown-linux-musl/oxidex-aarch64-unknown-linux-musl release-assets/
          # Windows x64
          cp artifacts/oxidex-x86_64-pc-windows-msvc/oxidex-x86_64-pc-windows-msvc.exe release-assets/
          # macOS signed binary
          cp artifacts/oxidex-aarch64-apple-darwin/oxidex-aarch64-apple-darwin release-assets/
          # macOS DMG
          cp artifacts/oxidex-dmg/*.dmg release-assets/

      - name: Extract version and create release notes
        id: version
        run: |
          VERSION=${GITHUB_REF#refs/tags/v}
          echo "version=$VERSION" >> $GITHUB_OUTPUT

          cat > release-notes.md << 'EOF'
          ## OxiDex v$VERSION

          ### Release Artifacts

          **Linux:**
          - `oxidex-x86_64-unknown-linux-musl` - Linux x86_64 (musl, static binary)
          - `oxidex-aarch64-unknown-linux-musl` - Linux ARM64 (musl, static binary)

          **Windows:**
          - `oxidex-x86_64-pc-windows-msvc.exe` - Windows x86_64

          **macOS:**
          - `oxidex-aarch64-apple-darwin` - Signed macOS ARM64 binary
          - `oxidex-v$VERSION.dmg` - Signed and notarized macOS DMG installer

          ### Installation

          **Linux/macOS:**
          ```bash
          # Download the appropriate binary
          chmod +x oxidex-*
          sudo mv oxidex-* /usr/local/bin/oxidex
          ```

          **Windows:**
          Download and run `oxidex-x86_64-pc-windows-msvc.exe`

          **macOS DMG:**
          1. Download and open the DMG file
          2. Drag OxiDex to Applications or desired location
          3. No security warnings - fully signed and notarized by Apple

          ### Note
          The macOS artifacts are signed with a Developer ID certificate and the DMG is notarized by Apple for secure installation.
          EOF

          # Replace $VERSION in the file
          sed -i "s/\$VERSION/$VERSION/g" release-notes.md

      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          name: Release v${{ steps.version.outputs.version }}
          body_path: release-notes.md
          draft: false
          prerelease: false
          files: |
            release-assets/*
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

**Step 2: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "feat: add GitHub release creation job with all artifacts"
```

---

## Task 6: Delete mac-sign.yml workflow

**Files:**
- Delete: `.github/workflows/mac-sign.yml`

**Step 1: Remove the old mac-sign workflow**

```bash
git rm .github/workflows/mac-sign.yml
```

**Step 2: Commit**

```bash
git commit -m "chore: remove mac-sign.yml (ported to release.yml)"
```

---

## Task 7: Push changes and create v1.1.1 tag

**Files:**
- N/A (git operations)

**Step 1: Push all commits**

```bash
git push origin main
```

Expected: All commits pushed successfully

**Step 2: Create and push v1.1.1 tag**

```bash
git tag -a v1.1.1 -m "Release v1.1.1

- Multi-platform release workflow
- Signed and notarized macOS binaries
- Automated GitHub release creation"

git push origin v1.1.1
```

Expected: Tag created and pushed, release workflow triggered

**Step 3: Monitor workflow**

```bash
gh run watch
```

Expected: release.yml workflow runs and completes successfully

---

## Task 8: Verify release artifacts

**Files:**
- N/A (verification)

**Step 1: Check GitHub release page**

```bash
gh release view v1.1.1
```

Expected output should show:
- Release v1.1.1
- 5 assets:
  - oxidex-x86_64-unknown-linux-musl
  - oxidex-aarch64-unknown-linux-musl
  - oxidex-x86_64-pc-windows-msvc.exe
  - oxidex-aarch64-apple-darwin
  - oxidex-v1.1.1.dmg

**Step 2: Download and verify each artifact exists**

```bash
gh release download v1.1.1 -D /tmp/release-test
ls -lh /tmp/release-test
```

Expected: All 5 files downloaded

**Step 3: Verify macOS binary is signed (if on macOS)**

```bash
codesign --verify --verbose /tmp/release-test/oxidex-aarch64-apple-darwin
codesign --display --verbose=4 /tmp/release-test/oxidex-aarch64-apple-darwin
```

Expected: Signature valid, shows Developer ID

**Step 4: Verify DMG is notarized (if on macOS)**

```bash
spctl --assess --verbose=4 --type install /tmp/release-test/oxidex-v1.1.1.dmg
```

Expected: "accepted" with source=Notarized Developer ID

---

## Debugging Steps (if workflow fails)

**Use superpowers:systematic-debugging skill:**

1. Check workflow logs:
   ```bash
   gh run view --log-failed
   ```

2. Common issues:
   - **Cross-compilation fails:** Ensure musl-tools and gcc-aarch64-linux-gnu installed
   - **macOS signing fails:** Verify secrets are set correctly
   - **Notarization fails:** Check Apple ID app-specific password
   - **Artifact upload fails:** Verify artifact paths match
   - **Release creation fails:** Ensure GITHUB_TOKEN has write permissions

3. If any step fails, use systematic-debugging to:
   - Identify root cause
   - Test hypothesis
   - Implement fix
   - Verify

---

## Success Criteria

✅ release.yml workflow exists and is properly configured
✅ All platform builds complete successfully
✅ macOS binary is signed with Developer ID certificate
✅ macOS DMG is notarized and stapled
✅ GitHub release v1.1.1 is created
✅ All 5 artifacts are uploaded to the release
✅ mac-sign.yml is deleted
✅ Verification confirms signatures and notarization

---

## Post-Implementation

After successful implementation:
1. Document the release process in README.md
2. Add badge for latest release
3. Update CONTRIBUTING.md with release instructions
