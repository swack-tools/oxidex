# ExifTool Compatibility Report Design

**Date:** 2025-12-07
**Status:** Approved

## Overview

Automated comparison of OxiDex metadata extraction against ExifTool, published to GitHub Pages on every parser change. Tracks coverage, differences, and regressions over time.

## Trigger

GitHub Actions workflow runs on:
- Push to `main` with changes to `src/parsers/**`
- Manual dispatch

## ExifTool Version Management

Download ExifTool from the official versioned release, cached by version number.

```yaml
- name: Get ExifTool version
  id: exiftool-version
  run: |
    VERSION=$(curl -s https://exiftool.org/ver.txt)
    echo "version=$VERSION" >> $GITHUB_OUTPUT

- name: Cache ExifTool + test images
  uses: actions/cache@v4
  id: cache-exiftool
  with:
    path: ~/exiftool
    key: exiftool-${{ steps.exiftool-version.outputs.version }}

- name: Download ExifTool release
  if: steps.cache-exiftool.outputs.cache-hit != 'true'
  run: |
    VERSION=${{ steps.exiftool-version.outputs.version }}
    curl -L "https://github.com/exiftool/exiftool/archive/refs/tags/$VERSION.tar.gz" -o exiftool.tar.gz
    tar -xzf exiftool.tar.gz
    mv "exiftool-$VERSION" ~/exiftool
```

Both the ExifTool binary and test images come from the same versioned release, ensuring consistency.

## Test Files

Use all files from ExifTool's test suite (`t/images/` directory in the release). These are downloaded in CI, not committed to the repo.

Benefits:
- Comprehensive coverage (hundreds of test files)
- Tests against same files ExifTool uses
- No binary bloat in repo
- Samples match ExifTool version exactly

## Comparison Logic

### What We Compare

Tag name + value comparison for each test file:
- **Missing tags:** Present in ExifTool output, absent in OxiDex
- **Extra tags:** Present in OxiDex output, absent in ExifTool
- **Value differences:** Same tag name, different value
- **Regressions:** Tags OxiDex had in previous run but lost (tracked via baseline)

### Comparison Script

Location: `scripts/compare-tags/` (Rust binary in workspace)

Flow:
1. Load `baseline.json` (previous OxiDex results)
2. For each test file:
   - Run: `~/exiftool/exiftool -json -G1 -a <file>`
   - Run: `./target/release/oxidex --json <file>`
   - Normalize outputs (lowercase keys, trim whitespace)
   - Compare and categorize differences
   - Check against baseline for regressions
3. Generate markdown reports
4. Update `baseline.json` with current results
5. Exit 0 (always success - informational only)

### Output Normalization

- Key format: `Group:TagName` (e.g., `EXIF:Make`)
- Values converted to strings for comparison
- Binary data shown as `[binary data, N bytes]`

## Report Structure

Output location: `docs/reference/comparison/`

```
docs/reference/comparison/
├── index.md              # Summary with coverage % per format
├── baseline.json         # Previous OxiDex results for regression tracking
├── jpeg.md               # Detailed JPEG comparison
├── png.md                # Detailed PNG comparison
├── mp4.md                # Detailed MP4 comparison
└── ...                   # One page per format
```

### Summary Page (index.md)

```markdown
# ExifTool Compatibility Report

Generated: 2025-12-07 | ExifTool v13.43 | OxiDex v1.2.1

| Format | Files | Coverage | Missing | Extra | Regressions |
|--------|-------|----------|---------|-------|-------------|
| JPEG   | 45    | 94%      | 12      | 3     | 0           |
| PNG    | 12    | 87%      | 8       | 1     | 2           |
| MP4    | 8     | 72%      | 45      | 0     | 0           |
```

### Format Pages (e.g., jpeg.md)

Each format page contains:
- Coverage statistics (files tested, average coverage)
- Missing tags table (tag name, expected value from ExifTool)
- Extra tags table (tags OxiDex finds that ExifTool doesn't)
- Value differences table (tag, ExifTool value, OxiDex value)
- Regressions section (highlighted, tags lost since last run)

## Workflow Structure

New workflow: `.github/workflows/compare-exiftool.yml`

```yaml
name: ExifTool Compatibility Report

on:
  push:
    branches: [main]
    paths:
      - 'src/parsers/**'
  workflow_dispatch:

jobs:
  compare:
    runs-on: ubuntu-latest
    permissions:
      contents: write

    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Get ExifTool version
        id: exiftool-version
        run: |
          VERSION=$(curl -s https://exiftool.org/ver.txt)
          echo "version=$VERSION" >> $GITHUB_OUTPUT

      - name: Cache ExifTool + test images
        uses: actions/cache@v4
        id: cache-exiftool
        with:
          path: ~/exiftool
          key: exiftool-${{ steps.exiftool-version.outputs.version }}

      - name: Download ExifTool release
        if: steps.cache-exiftool.outputs.cache-hit != 'true'
        run: |
          VERSION=${{ steps.exiftool-version.outputs.version }}
          curl -L "https://github.com/exiftool/exiftool/archive/refs/tags/$VERSION.tar.gz" -o exiftool.tar.gz
          tar -xzf exiftool.tar.gz
          mv "exiftool-$VERSION" ~/exiftool

      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Build OxiDex
        run: cargo build --release

      - name: Build comparison tool
        run: cargo build --release -p compare-tags

      - name: Run comparison
        run: |
          ./target/release/compare-tags \
            --exiftool ~/exiftool/exiftool \
            --oxidex ./target/release/oxidex \
            --samples ~/exiftool/t/images \
            --baseline docs/reference/comparison/baseline.json \
            --output docs/reference/comparison

      - name: Commit reports
        run: |
          git config user.name "github-actions[bot]"
          git config user.email "github-actions[bot]@users.noreply.github.com"
          git add docs/reference/comparison/
          git diff --staged --quiet || git commit -m "docs: update ExifTool compatibility report"
          git push
```

## Deployment

The existing `deploy-docs.yml` workflow triggers on changes to `docs/**`. When this workflow commits updated reports to `docs/reference/comparison/`, it automatically triggers a docs rebuild and deploy.

## CI Behavior

- **Always green:** Workflow exits 0 regardless of coverage
- **Regressions documented:** Lost tags highlighted in report but don't fail CI
- **Informational:** Report provides visibility into compatibility status

## VitePress Integration

Add to sidebar in `docs/.vitepress/config.js`:

```js
{
  text: 'Compatibility',
  items: [
    { text: 'ExifTool Comparison', link: '/reference/comparison/' }
  ]
}
```

## Files to Create

1. `.github/workflows/compare-exiftool.yml` - Workflow definition
2. `scripts/compare-tags/Cargo.toml` - Comparison tool package
3. `scripts/compare-tags/src/main.rs` - Comparison logic
4. `docs/reference/comparison/index.md` - Initial placeholder (generated content)
5. `docs/reference/comparison/baseline.json` - Initial empty baseline
6. Update `docs/.vitepress/config.js` - Add sidebar entry
