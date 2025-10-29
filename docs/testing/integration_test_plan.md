# Integration Test Plan

## 1. Overview

### 1.1 Purpose

This document defines the comprehensive integration testing strategy for ExifTool-RS. Integration tests validate end-to-end workflows, CLI operations, and behavioral parity with the reference Perl ExifTool implementation.

### 1.2 Scope

Integration tests complement unit tests (70% of suite) and property-based tests (20% of suite) by covering:

- **End-to-End Workflows**: Complete read → parse → extract → output pipelines
- **CLI Interface**: Command-line argument parsing and execution
- **Cross-Format Operations**: Batch processing across multiple file formats
- **Error Handling**: Real-world failure scenarios (missing files, corrupted metadata, permission errors)
- **ExifTool Parity**: Tag value comparison against Perl ExifTool (reference implementation)

### 1.3 Success Criteria

Integration tests are considered successful when:

1. **Functional Correctness**: 99%+ tag value match rate vs. Perl ExifTool for well-formed files
2. **Graceful Degradation**: Appropriate error handling for malformed files (no crashes/hangs)
3. **Performance**: Within 2x performance of Perl ExifTool for batch operations
4. **Cross-Platform**: Pass on Linux, macOS, and Windows
5. **Regression Prevention**: No degradation in match rate or performance across commits

---

## 2. Test Image Corpus Strategy

### 2.1 Corpus Size & Diversity Requirements

**Target**: 100+ images across all supported formats

**Diversity Matrix**:

| **Format** | **Simple** | **Complex** | **Edge Cases** | **Malformed** | **Total** |
|------------|-----------|-------------|----------------|---------------|-----------|
| JPEG       | 15        | 15          | 10             | 10            | 50        |
| PNG        | 10        | 10          | 5              | 5             | 30        |
| TIFF       | 8         | 8           | 4              | 5             | 25        |
| WebP       | 5         | 5           | 3              | 2             | 15        |
| HEIC       | 3         | 3           | 2              | 2             | 10        |
| **Total**  | **41**    | **41**      | **24**        | **24**        | **130**   |

**Complexity Definitions**:

- **Simple**: Single IFD, basic EXIF tags (Make, Model, DateTime)
- **Complex**: Multiple IFDs (EXIF, GPS, Interoperability), thumbnail images, maker notes
- **Edge Cases**: Large maker notes (>64KB), deeply nested IFDs (>8 levels), unusual tag values (empty strings, extreme GPS coordinates)
- **Malformed**: Truncated files, invalid magic bytes, corrupted IFD chains, decompression bombs

### 2.2 Image Sourcing Strategy

#### 2.2.1 Public Datasets

1. **Exiv2 Test Suite** ([exiv2/exiv2 GitHub](https://github.com/Exiv2/exiv2/tree/main/tests/data))
   - License: GPL-compatible
   - Coverage: JPEG, TIFF, PNG with diverse EXIF/IPTC/XMP tags
   - Action: Download curated subset (30-40 images)

2. **Unsplash Free Images** ([unsplash.com](https://unsplash.com))
   - License: CC0 (public domain)
   - Coverage: Real-world photographs with GPS, camera settings, lens data
   - Action: Download 20-30 high-quality images from various cameras

3. **LibRaw Test Samples** ([libraw.org](https://www.libraw.org))
   - License: LGPL/CDDL
   - Coverage: RAW formats (CR2, NEF, ARW) - if supported
   - Action: Include 5-10 RAW samples for future format support

#### 2.2.2 Synthetic Generated Images

Use **ImageMagick** and **exiftool** to generate images with known metadata:

```bash
# Generate JPEG with specific EXIF
convert -size 640x480 xc:blue generated_simple.jpg
exiftool -Make="Canon" -Model="EOS 5D" -DateTimeOriginal="2024:01:15 14:30:00" generated_simple.jpg

# Generate TIFF with GPS coordinates
convert -size 1024x768 gradient:blue-red generated_gps.tif
exiftool -GPSLatitude="37.7749" -GPSLongitude="-122.4194" -GPSLatitudeRef="N" -GPSLongitudeRef="W" generated_gps.tif
```

**Generated Image Categories** (20-30 images):
- Minimal EXIF (1-2 tags)
- Complete EXIF (50+ standard tags)
- GPS-only metadata
- Unicode in tag values (Artist: "山田太郎", Copyright: "© 2024")
- Extreme values (exposure time: 1/8000s, ISO 102400)

#### 2.2.3 Malformed Samples for Security Testing

**Deliberately Crafted Files** (20-24 images):

| **Type** | **Description** | **Expected Behavior** |
|----------|-----------------|----------------------|
| Truncated header | File ends mid-IFD | `ParseError::UnexpectedEof` |
| Invalid magic bytes | `0xFF 0xD9` (JPEG EOI) at start | `UnsupportedFormat` |
| Circular IFD chain | IFD0 → IFD1 → IFD0 | Max depth limit (64), return `ParseError::MaxDepthExceeded` |
| Integer overflow | Tag count: `0xFFFFFFFF` | `ParseError::InvalidTagCount` |
| Decompression bomb | 10MB compressed → 10GB uncompressed | Reject if ratio > 100x |
| Path traversal in filename | `../../etc/passwd` | Sanitize with `canonicalize()` |
| XXE in XMP (XML) | `<!DOCTYPE foo [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>` | Reject external entities |

**Generation Tools**:
- **Radamsa**: Fuzzing tool for mutation-based corruption
- **Manual hex editing**: Precise control over malformed structures
- **Python PIL**: Programmatic generation of edge cases

### 2.3 Directory Structure

```
tests/fixtures/
├── jpeg/
│   ├── simple/              # 15 images: basic EXIF
│   ├── complex/             # 15 images: GPS + maker notes + thumbnails
│   ├── edge_cases/          # 10 images: unusual tag values, deep nesting
│   └── malformed/           # 10 images: security test cases
├── png/
│   ├── simple/              # 10 images: basic tEXt/iTXt chunks
│   ├── complex/             # 10 images: EXIF + XMP in PNG
│   ├── edge_cases/          # 5 images: animated PNG (APNG)
│   └── malformed/           # 5 images: corrupted chunks
├── tiff/
│   ├── simple/              # 8 images: single-page TIFF
│   ├── complex/             # 8 images: multi-page, BigTIFF
│   ├── edge_cases/          # 4 images: extremely large dimensions
│   └── malformed/           # 5 images: IFD corruption
├── webp/                    # 15 images (simple/complex/edge_cases/malformed)
├── heic/                    # 10 images (simple/complex/edge_cases/malformed)
└── README.md                # Corpus documentation and attributions
```

### 2.4 Corpus Metadata Tracking

Create `tests/fixtures/manifest.json` to track image provenance:

```json
{
  "version": "1.0.0",
  "images": [
    {
      "path": "jpeg/simple/canon_eos_5d.jpg",
      "source": "unsplash",
      "license": "CC0",
      "url": "https://unsplash.com/photos/xyz",
      "sha256": "a3c5f...",
      "tags_expected": 42,
      "formats": ["EXIF", "JFIF"]
    }
  ]
}
```

---

## 3. Validation Methodology

### 3.1 Comparison Approach

**Reference Implementation**: Perl ExifTool v12.70+ (latest stable)

**Comparison Strategy**:
1. Execute both tools on identical input files
2. Export metadata to JSON format for structured comparison
3. Parse JSON outputs and compute field-level match rate
4. Generate human-readable diff reports for mismatches

### 3.2 Tool Execution

#### 3.2.1 Perl ExifTool Command

```bash
exiftool -json -a -G1 -struct tests/fixtures/jpeg/simple/canon_eos_5d.jpg > perl_output.json
```

**Flags Explained**:
- `-json`: Output in JSON format
- `-a`: Extract duplicate tags (some formats allow tag repetition)
- `-G1`: Include group names (EXIF, GPS, IPTC, etc.)
- `-struct`: Preserve structure for nested tags (XMP, maker notes)

#### 3.2.2 ExifTool-RS Command

```bash
exiftool-rs -json tests/fixtures/jpeg/simple/canon_eos_5d.jpg > rust_output.json
```

**Expected JSON Format**:

```json
[
  {
    "SourceFile": "tests/fixtures/jpeg/simple/canon_eos_5d.jpg",
    "EXIF:Make": "Canon",
    "EXIF:Model": "Canon EOS 5D",
    "EXIF:DateTimeOriginal": "2024:01:15 14:30:00",
    "EXIF:FNumber": 2.8,
    "GPS:GPSLatitude": 37.7749,
    "GPS:GPSLongitude": -122.4194
  }
]
```

### 3.3 JSON Output Comparison

#### 3.3.1 Comparison Script

Implement `tests/integration/compare_with_exiftool.rs`:

```rust
use std::collections::HashMap;
use std::process::Command;
use serde_json::Value;

fn compare_json_outputs(perl_json: &str, rust_json: &str) -> MatchReport {
    let perl_data: Vec<HashMap<String, Value>> = serde_json::from_str(perl_json)?;
    let rust_data: Vec<HashMap<String, Value>> = serde_json::from_str(rust_json)?;

    let perl_tags = &perl_data[0];
    let rust_tags = &rust_data[0];

    let mut total_tags = 0;
    let mut matched_tags = 0;
    let mut mismatches = Vec::new();

    for (key, perl_value) in perl_tags.iter() {
        if key == "SourceFile" { continue; } // Skip metadata field

        total_tags += 1;

        match rust_tags.get(key) {
            Some(rust_value) if values_match(perl_value, rust_value) => {
                matched_tags += 1;
            }
            Some(rust_value) => {
                mismatches.push(Mismatch { key, perl_value, rust_value });
            }
            None => {
                mismatches.push(Mismatch { key, perl_value, rust_value: "MISSING" });
            }
        }
    }

    MatchReport {
        match_rate: (matched_tags as f64 / total_tags as f64) * 100.0,
        total_tags,
        matched_tags,
        mismatches,
    }
}

fn values_match(perl_val: &Value, rust_val: &Value) -> bool {
    match (perl_val, rust_val) {
        // Exact match for strings and integers
        (Value::String(p), Value::String(r)) => p == r,
        (Value::Number(p), Value::Number(r)) => p == r,

        // Floating-point tolerance for GPS coordinates
        (Value::Number(p), Value::Number(r)) => {
            if let (Some(pf), Some(rf)) = (p.as_f64(), r.as_f64()) {
                (pf - rf).abs() < 0.0001
            } else {
                false
            }
        }

        // Arrays (GPS coordinates, tag values)
        (Value::Array(p), Value::Array(r)) => {
            p.len() == r.len() && p.iter().zip(r.iter()).all(|(pv, rv)| values_match(pv, rv))
        }

        _ => false
    }
}
```

#### 3.3.2 Cross-Platform Considerations

**Path Separators**:
- Normalize paths before comparison: `path.replace('\\', '/')` on Windows
- Perl ExifTool uses forward slashes even on Windows

**Floating-Point Precision**:
- GPS coordinates: Tolerance of ±0.0001 degrees (~11 meters)
- Exposure time fractions: Compare as rational numbers (numerator/denominator)
- F-number, focal length: Tolerance of ±0.01

**Timezone Handling**:
- EXIF timestamps are localtime (no timezone)
- GPS timestamps are UTC
- Compare as strings, not parsed `DateTime` objects

**Vendor-Specific Tag Names**:
- Perl ExifTool: `"MakerNotes:CanonModelID"`
- ExifTool-RS: May use `"Canon:ModelID"` (shorter group name)
- Solution: Tag name normalization mapping

### 3.4 Match Rate Calculation

**Formula**:

```
Match Rate (%) = (Matched Tags / Total Tags in Reference) × 100
```

**Where**:
- **Matched Tags**: Tags where values are identical (or within tolerance)
- **Total Tags**: All tags extracted by Perl ExifTool (baseline)
- **Excluded**: Metadata fields (`SourceFile`, `ExifToolVersion`)

**Example**:

```
Perl ExifTool: 87 tags
ExifTool-RS:   85 tags (84 match Perl, 1 unique to Rust)
Match Rate = 84 / 87 × 100 = 96.6%
```

---

## 4. Acceptance Criteria & Thresholds

### 4.1 Pass/Fail Criteria

#### 4.1.1 Well-Formed Files

**Primary Criterion**: **99% tag value match rate**

For each image in `tests/fixtures/{format}/simple/` and `tests/fixtures/{format}/complex/`:

```
PASS: match_rate >= 99.0%
FAIL: match_rate < 99.0%
```

**Allowed Discrepancies (1% tolerance)**:

Valid reasons for mismatch (do not count as failures):

1. **Vendor-Specific Decoding**: Maker notes proprietary formats where documentation is unavailable
2. **Precision Differences**: Rational number representations (e.g., `1/125` vs `0.008`)
3. **Tag Name Variations**: Group naming differences (document mapping)
4. **Unsupported Tags**: Tags explicitly documented as "not yet implemented" in changelog

**Mismatch Handling**:
- Document all mismatches in `tests/integration/KNOWN_DISCREPANCIES.md`
- Each discrepancy requires:
  - Image path
  - Tag name
  - Expected value (Perl)
  - Actual value (Rust)
  - Explanation (why mismatch is acceptable OR issue tracker link)

#### 4.1.2 Edge Case Files

**Criterion**: **95% tag value match rate**

Edge cases (`tests/fixtures/{format}/edge_cases/`) may have:
- Unusual tag values (empty strings, extreme numbers)
- Deep IFD nesting requiring iterative parsing
- Large maker notes requiring chunked reading

**Acceptable**: Slightly lower match rate due to implementation trade-offs (e.g., max depth limits)

#### 4.1.3 Malformed Files

**Criterion**: **Graceful error handling (no crashes/hangs)**

For malformed files (`tests/fixtures/{format}/malformed/`):

```rust
#[test]
fn test_malformed_truncated_jpeg() {
    let result = exiftool_rs::extract_metadata("tests/fixtures/jpeg/malformed/truncated.jpg");

    // PASS: Returns specific error (no panic)
    assert!(result.is_err());

    // PASS: Error type is appropriate
    match result.unwrap_err() {
        ExifToolError::ParseError(ParseError::UnexpectedEof) => {},
        _ => panic!("Unexpected error type"),
    }
}
```

**Pass Criteria**:
- Returns `Err(ExifToolError::ParseError(..))` for corrupted structure
- Returns `Err(ExifToolError::UnsupportedFormat)` for invalid magic bytes
- Completes within 5 seconds (no infinite loops)
- No memory leaks (validate with Valgrind/AddressSanitizer)
- No panics (all errors are `Result<T, E>`)

### 4.2 Match Rate Thresholds

**Tiered Thresholds**:

| **Test Category** | **Minimum Match Rate** | **Target Match Rate** | **Action if Below Target** |
|-------------------|------------------------|----------------------|---------------------------|
| Simple files      | 99%                    | 100%                 | Investigate immediately, block merge |
| Complex files     | 99%                    | 99.5%                | Document discrepancy, issue tracker |
| Edge cases        | 95%                    | 98%                  | Best-effort improvement |
| Malformed files   | N/A                    | N/A                  | Graceful error only |

**CI/CD Enforcement**:

```yaml
# .github/workflows/integration_tests.yml
- name: Run ExifTool Comparison Tests
  run: cargo test --test compare_with_exiftool --features exiftool-comparison

- name: Check Match Rate
  run: |
    MATCH_RATE=$(jq '.match_rate' target/test-results/comparison_report.json)
    if (( $(echo "$MATCH_RATE < 99.0" | bc -l) )); then
      echo "FAIL: Match rate $MATCH_RATE% below 99% threshold"
      exit 1
    fi
```

### 4.3 Graceful Degradation for Malformed Files

**Definition**: Software handles invalid input without crashing, leaking memory, or exposing security vulnerabilities

**Requirements**:

1. **Error Recovery**: Parser backtracks and attempts to extract partial metadata
   ```rust
   // If IFD1 (thumbnail) is corrupted, still return IFD0 (main image) tags
   if let Err(e) = parse_ifd1(&mut reader) {
       warn!("IFD1 parsing failed: {}, continuing with IFD0", e);
   }
   ```

2. **Resource Limits**: Prevent denial-of-service attacks
   - Max file size: 1GB (configurable via `--max-file-size`)
   - Max IFD depth: 64 levels (prevent infinite recursion)
   - Max tag count per IFD: 10,000 (prevent memory exhaustion)
   - Max decompression ratio: 100x (prevent zip bombs)

3. **Security Guarantees**:
   - No buffer overflows (Rust ownership system)
   - No integer overflows (checked arithmetic: `size.checked_add(offset)?`)
   - No path traversal (sanitize filenames: `canonicalize()` + jail to working directory)
   - No XXE attacks (disable external entities in XML parser)

4. **Logging**: Informative error messages for debugging
   ```
   ERROR: Failed to parse TIFF IFD at offset 0x1A3C: invalid tag count 0xFFFFFFFF
   INFO: Extracted 42 tags from IFD0 before error, returning partial metadata
   ```

---

## 5. Regression Testing Infrastructure

### 5.1 Git LFS Setup

**Problem**: Binary test images (100+ files, ~500MB total) exceed GitHub repository size limits and slow down cloning.

**Solution**: Git Large File Storage (LFS) stores binary files externally while keeping lightweight pointers in Git history.

#### 5.1.1 Initial Setup (One-Time)

**Install Git LFS**:

```bash
# macOS
brew install git-lfs

# Ubuntu/Debian
sudo apt-get install git-lfs

# Windows
# Download installer from https://git-lfs.github.com/

# Initialize LFS
git lfs install
```

**Configure `.gitattributes`** (place in repository root):

```gitattributes
# Track binary image files with Git LFS
tests/fixtures/**/*.jpg filter=lfs diff=lfs merge=lfs -text
tests/fixtures/**/*.jpeg filter=lfs diff=lfs merge=lfs -text
tests/fixtures/**/*.png filter=lfs diff=lfs merge=lfs -text
tests/fixtures/**/*.tif filter=lfs diff=lfs merge=lfs -text
tests/fixtures/**/*.tiff filter=lfs diff=lfs merge=lfs -text
tests/fixtures/**/*.webp filter=lfs diff=lfs merge=lfs -text
tests/fixtures/**/*.heic filter=lfs diff=lfs merge=lfs -text
tests/fixtures/**/*.heif filter=lfs diff=lfs merge=lfs -text
tests/fixtures/**/*.avif filter=lfs diff=lfs merge=lfs -text

# Track test output binaries
tests/fixtures/**/*.bin filter=lfs diff=lfs merge=lfs -text
```

**Add Test Images**:

```bash
# Stage images for LFS tracking
git add tests/fixtures/**/*.jpg tests/fixtures/**/*.png tests/fixtures/**/*.tif

# Commit (LFS uploads to storage backend)
git commit -m "test: add integration test image corpus"

# Push to remote (uploads LFS objects)
git push origin main
```

#### 5.1.2 Storage Quotas & Management

**GitHub Free Tier**:
- Storage: 1GB free
- Bandwidth: 1GB/month free
- Overage: $5/month per 50GB storage, $5/month per 50GB bandwidth

**Mitigation Strategies**:

1. **Corpus Size Limits**:
   - Target: 500MB total for test corpus
   - Compress images at reasonable quality (JPEG: 85%, PNG: lossless but optimized)
   - Remove duplicate or redundant test cases

2. **Selective Checkout** (for developers):
   ```bash
   # Clone without downloading LFS files
   GIT_LFS_SKIP_SMUDGE=1 git clone https://github.com/yourorg/exiftool-rs.git

   # Download only specific format
   git lfs fetch --include="tests/fixtures/jpeg/**"
   git lfs checkout tests/fixtures/jpeg/
   ```

3. **CI/CD Optimization**:
   - Cache LFS files in GitHub Actions: `actions/cache@v3`
   - Only download files needed for changed code (e.g., if editing JPEG parser, skip PNG fixtures)

4. **Alternative Storage** (if GitHub quota exceeded):
   - Self-hosted LFS server (Gitea, GitLab with LFS support)
   - S3-backed LFS (using `git-lfs-s3`)

#### 5.1.3 Verification

**Check LFS Tracking**:

```bash
# List tracked files
git lfs ls-files

# Verify file is stored in LFS (not Git blob)
git lfs status

# Expected output:
# tests/fixtures/jpeg/simple/canon_eos_5d.jpg (LFS: a3c5f2... - 2.4 MB)
```

**Clone Test** (validate setup):

```bash
# Fresh clone
git clone https://github.com/yourorg/exiftool-rs.git test-clone
cd test-clone

# Verify LFS files are downloaded (not pointers)
file tests/fixtures/jpeg/simple/canon_eos_5d.jpg
# Expected: "JPEG image data" (not "ASCII text" which indicates LFS pointer)
```

### 5.2 CI/CD Integration

**GitHub Actions Workflow**: `.github/workflows/integration_tests.yml`

```yaml
name: Integration Tests

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  integration-tests:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
        rust: [stable]

    steps:
      - name: Checkout code
        uses: actions/checkout@v4
        with:
          lfs: true  # Enable LFS checkout

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: ${{ matrix.rust }}

      - name: Cache LFS files
        uses: actions/cache@v3
        with:
          path: .git/lfs
          key: lfs-${{ runner.os }}-${{ hashFiles('.gitattributes') }}

      - name: Install Perl ExifTool
        run: |
          if [ "$RUNNER_OS" == "Linux" ]; then
            sudo apt-get update && sudo apt-get install -y libimage-exiftool-perl
          elif [ "$RUNNER_OS" == "macOS" ]; then
            brew install exiftool
          elif [ "$RUNNER_OS" == "Windows" ]; then
            choco install exiftool
          fi
        shell: bash

      - name: Verify ExifTool Installation
        run: exiftool -ver

      - name: Run Integration Tests
        run: cargo test --test '*' --features exiftool-comparison
        env:
          RUST_BACKTRACE: 1

      - name: Run ExifTool Comparison Tests
        run: cargo test --test compare_with_exiftool --features exiftool-comparison -- --test-threads=1

      - name: Generate Comparison Report
        if: always()
        run: |
          cargo run --bin generate_comparison_report > comparison_report.md
          cat comparison_report.md >> $GITHUB_STEP_SUMMARY

      - name: Upload Test Results
        if: always()
        uses: actions/upload-artifact@v3
        with:
          name: test-results-${{ matrix.os }}
          path: target/test-results/

      - name: Check Match Rate Threshold
        run: |
          MATCH_RATE=$(jq -r '.match_rate' target/test-results/comparison_report.json)
          echo "Match rate: $MATCH_RATE%"
          if (( $(echo "$MATCH_RATE < 99.0" | bc -l) )); then
            echo "::error::Match rate $MATCH_RATE% below 99% threshold"
            exit 1
          fi
```

**Key Features**:

1. **LFS Checkout**: `lfs: true` in `actions/checkout` downloads binary files
2. **Cross-Platform**: Tests on Linux, macOS, Windows
3. **Caching**: LFS files cached to avoid re-download on every run
4. **Dependency Installation**: Perl ExifTool installed via package manager
5. **Failure Reporting**: Comparison report uploaded even if tests fail
6. **Threshold Enforcement**: CI fails if match rate < 99%

### 5.3 Baseline Management

**Problem**: As ExifTool-RS evolves, some tag values may intentionally differ from Perl ExifTool (e.g., better precision, bug fixes).

**Solution**: Version-controlled baseline of expected outputs.

#### 5.3.1 Baseline Generation

**Initial Baseline** (run once):

```bash
# Generate JSON outputs for all test images
cargo run --bin generate_baseline -- \
  --input tests/fixtures/ \
  --output tests/baselines/ \
  --exiftool-path $(which exiftool)

# Directory structure:
# tests/baselines/
# ├── jpeg/
# │   ├── simple/
# │   │   ├── canon_eos_5d.perl.json
# │   │   └── canon_eos_5d.rust.json
# │   └── complex/
# │       ├── nikon_d850.perl.json
# │       └── nikon_d850.rust.json
# └── baseline_metadata.json  # Match rates, discrepancies
```

**Baseline Metadata** (`tests/baselines/baseline_metadata.json`):

```json
{
  "version": "0.1.0",
  "exiftool_version": "12.70",
  "generated_at": "2024-01-15T10:30:00Z",
  "images": [
    {
      "path": "jpeg/simple/canon_eos_5d.jpg",
      "perl_tags": 42,
      "rust_tags": 42,
      "match_rate": 100.0,
      "discrepancies": []
    },
    {
      "path": "jpeg/complex/nikon_d850.jpg",
      "perl_tags": 187,
      "rust_tags": 185,
      "match_rate": 98.9,
      "discrepancies": [
        {
          "tag": "MakerNotes:LensID",
          "perl_value": "AF-S NIKKOR 24-70mm f/2.8E ED VR",
          "rust_value": "UNKNOWN (0x4B)",
          "reason": "Lens ID lookup table incomplete - issue #42"
        }
      ]
    }
  ],
  "overall_match_rate": 99.4
}
```

#### 5.3.2 Baseline Updates

**When to Update**:
- ExifTool-RS implements new tag decoder (improves match rate)
- Perl ExifTool releases new version with breaking changes
- Intentional divergence (e.g., fixing a Perl ExifTool bug)

**Update Process**:

```bash
# Regenerate baseline
cargo run --bin generate_baseline -- --update

# Review changes
git diff tests/baselines/baseline_metadata.json

# Commit with justification
git commit -m "test: update baseline for improved maker notes decoding

- Implemented Canon LensID lookup table (issue #42)
- Match rate improved from 99.4% to 99.8%
- 3 images now have 100% match rate
"
```

**Review Checklist**:
- [ ] Match rate did not decrease (unless intentional)
- [ ] Discrepancies are documented with issue links
- [ ] Changelog updated with breaking changes (if any)
- [ ] All reviewers approve baseline update

---

## 6. Test Categories

### 6.1 Format Coverage Tests

**Objective**: Ensure all supported file formats can be read, parsed, and have metadata extracted.

**Test Matrix**:

| **Format** | **Test File** | **Key Tags to Verify** | **Special Handling** |
|------------|---------------|------------------------|----------------------|
| JPEG       | `jpeg/simple/canon_eos_5d.jpg` | EXIF:Make, EXIF:Model, EXIF:DateTimeOriginal | APP1 segment (EXIF), APP0 (JFIF) |
| PNG        | `png/simple/screenshot.png` | PNG:tEXt:Author, PNG:tIME | tEXt, iTXt chunks |
| TIFF       | `tiff/simple/single_page.tif` | TIFF:ImageWidth, TIFF:BitsPerSample | IFD0 parsing |
| WebP       | `webp/simple/photo.webp` | EXIF:*, XMP:* | RIFF container, VP8 bitstream |
| HEIC       | `heic/simple/iphone_photo.heic` | EXIF:*, GPS:* | ISO Base Media File Format (BMFF) |

**Test Implementation**:

```rust
#[test]
fn test_format_jpeg_simple() {
    let metadata = extract_metadata("tests/fixtures/jpeg/simple/canon_eos_5d.jpg").unwrap();
    assert_eq!(metadata.get("EXIF:Make").unwrap().as_string(), "Canon");
    assert_eq!(metadata.get("EXIF:Model").unwrap().as_string(), "Canon EOS 5D");
    assert!(metadata.contains_key("EXIF:DateTimeOriginal"));
}

#[test]
fn test_format_png_text_chunks() {
    let metadata = extract_metadata("tests/fixtures/png/simple/screenshot.png").unwrap();
    assert!(metadata.contains_key("PNG:tEXt:Author"));
    assert!(metadata.get("PNG:tIME").is_some());
}
```

### 6.2 Tag Coverage Tests

**Objective**: Verify extraction of diverse tag types (strings, integers, rationals, GPS coordinates, dates).

**Tag Categories**:

| **Category** | **Example Tags** | **Test File** | **Validation** |
|--------------|-----------------|---------------|----------------|
| Basic EXIF | Make, Model, Software | `jpeg/simple/` | String equality |
| Numeric | ISO, FNumber, ExposureTime | `jpeg/complex/` | Rational number comparison |
| GPS | GPSLatitude, GPSLongitude, GPSAltitude | `jpeg/complex/gps.jpg` | Float tolerance (±0.0001°) |
| DateTime | DateTimeOriginal, CreateDate, ModifyDate | All formats | ISO 8601 parsing |
| Maker Notes | LensID, FocusMode, WhiteBalance | `jpeg/complex/maker_notes.jpg` | Vendor-specific decoding |
| XMP | XMP:Creator, XMP:Copyright | `png/complex/xmp.png` | XML namespace handling |
| IPTC | Keywords, Caption, ByLine | `jpeg/complex/iptc.jpg` | Text encoding (UTF-8) |

**Test Implementation**:

```rust
#[test]
fn test_tag_gps_coordinates() {
    let metadata = extract_metadata("tests/fixtures/jpeg/complex/gps.jpg").unwrap();

    let lat = metadata.get("GPS:GPSLatitude").unwrap().as_f64();
    let lon = metadata.get("GPS:GPSLongitude").unwrap().as_f64();

    // San Francisco coordinates
    assert!((lat - 37.7749).abs() < 0.0001);
    assert!((lon - (-122.4194)).abs() < 0.0001);
}

#[test]
fn test_tag_rational_numbers() {
    let metadata = extract_metadata("tests/fixtures/jpeg/complex/rationals.jpg").unwrap();

    // FNumber: 2.8 stored as 28/10
    let f_number = metadata.get("EXIF:FNumber").unwrap().as_rational();
    assert_eq!(f_number, (28, 10));

    // ExposureTime: 1/125 second
    let exposure = metadata.get("EXIF:ExposureTime").unwrap().as_rational();
    assert_eq!(exposure, (1, 125));
}
```

### 6.3 Error Handling Tests

**Objective**: Validate graceful degradation for invalid inputs.

**Error Scenarios**:

| **Error Type** | **Test File** | **Expected Error** | **Validation** |
|----------------|---------------|-------------------|----------------|
| Missing file | `nonexistent.jpg` | `IoError::NotFound` | `assert!(result.is_err())` |
| Unsupported format | `malformed/invalid_magic.dat` | `UnsupportedFormat` | Match error variant |
| Truncated file | `malformed/truncated.jpg` | `ParseError::UnexpectedEof` | Partial data handling |
| Corrupted IFD | `malformed/corrupt_ifd.tif` | `ParseError::InvalidTagCount` | IFD validation |
| Integer overflow | `malformed/overflow.tif` | `ParseError::IntegerOverflow` | Checked arithmetic |
| Decompression bomb | `malformed/zip_bomb.png` | `ParseError::DecompressionLimitExceeded` | Ratio check |

**Test Implementation**:

```rust
#[test]
fn test_error_missing_file() {
    let result = extract_metadata("tests/fixtures/nonexistent.jpg");
    assert!(matches!(result, Err(ExifToolError::IoError(io::ErrorKind::NotFound))));
}

#[test]
fn test_error_corrupted_ifd() {
    let result = extract_metadata("tests/fixtures/tiff/malformed/corrupt_ifd.tif");
    assert!(matches!(
        result,
        Err(ExifToolError::ParseError(ParseError::InvalidTagCount))
    ));
}

#[test]
#[timeout(5000)] // 5 second timeout
fn test_no_infinite_loop() {
    // Circular IFD chain should not hang
    let result = extract_metadata("tests/fixtures/malformed/circular_ifd.tif");
    assert!(result.is_err()); // Should error, not hang
}
```

### 6.4 Performance Benchmarks

**Objective**: Ensure ExifTool-RS is competitive with Perl ExifTool.

**Benchmark Categories**:

| **Benchmark** | **Description** | **Target** | **Tool** |
|---------------|-----------------|-----------|----------|
| Single file extraction | Extract all metadata from 1 JPEG | < 10ms | `criterion` |
| Batch processing | Process 1000 JPEGs | < 5 seconds | `hyperfine` |
| Large file handling | Extract from 50MB TIFF | < 500ms | `criterion` |
| Memory usage | Peak RSS during batch | < 100MB | `valgrind --tool=massif` |
| Cold start time | CLI launch + extraction | < 50ms | `hyperfine` |

**Benchmark Implementation**:

```rust
// benches/integration_benchmarks.rs
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};

fn bench_single_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_extraction");

    for format in ["jpeg", "png", "tiff"] {
        let file = format!("tests/fixtures/{}/simple/sample.{}", format, format);

        group.bench_with_input(
            BenchmarkId::new("exiftool-rs", format),
            &file,
            |b, path| {
                b.iter(|| extract_metadata(path).unwrap())
            }
        );
    }

    group.finish();
}

fn bench_batch_processing(c: &mut Criterion) {
    c.bench_function("batch_1000_jpegs", |b| {
        let files: Vec<_> = glob("tests/fixtures/jpeg/**/*.jpg").collect();
        b.iter(|| {
            for file in &files {
                let _ = extract_metadata(file);
            }
        })
    });
}

criterion_group!(benches, bench_single_extraction, bench_batch_processing);
criterion_main!(benches);
```

**CLI Comparison** (using `hyperfine`):

```bash
# Compare wall-clock time
hyperfine --warmup 3 \
  'exiftool tests/fixtures/jpeg/simple/*.jpg' \
  'exiftool-rs tests/fixtures/jpeg/simple/*.jpg'

# Expected output:
# Benchmark 1: exiftool ...
#   Time (mean ± σ):     120.5 ms ±   3.2 ms
# Benchmark 2: exiftool-rs ...
#   Time (mean ± σ):      58.3 ms ±   2.1 ms
# Summary: exiftool-rs is 2.07x faster
```

**Regression Detection**:

```yaml
# CI fails if performance degrades >10%
- name: Run Benchmarks
  run: cargo bench --bench integration_benchmarks -- --save-baseline main

- name: Compare with Baseline
  run: |
    cargo bench --bench integration_benchmarks -- --baseline main
    # criterion exits with error if >10% slower
```

---

## 7. Implementation Roadmap

### Phase 1: Infrastructure Setup (Week 1)

**Tasks**:
1. Configure Git LFS (`.gitattributes`, test clone)
2. Install Perl ExifTool in CI/CD (Linux, macOS, Windows)
3. Implement comparison script (`tests/integration/compare_with_exiftool.rs`)
4. Create baseline generation tool (`cargo run --bin generate_baseline`)

**Deliverables**:
- [ ] `.gitattributes` committed
- [ ] CI workflow runs successfully
- [ ] Comparison script produces JSON report
- [ ] Baseline metadata file generated

### Phase 2: Corpus Acquisition (Week 2-3)

**Tasks**:
1. Download Exiv2 test suite (30-40 images)
2. Download Unsplash images (20-30 images)
3. Generate synthetic images (20-30 images)
4. Create malformed samples (20-24 images)
5. Document provenance in `tests/fixtures/manifest.json`

**Deliverables**:
- [ ] 130+ images in `tests/fixtures/`
- [ ] Images committed via Git LFS
- [ ] Manifest with source attribution
- [ ] README documenting corpus

### Phase 3: Test Implementation (Week 4-5)

**Tasks**:
1. Write format coverage tests (5 formats × 4 categories)
2. Write tag coverage tests (7 tag types)
3. Write error handling tests (6 error scenarios)
4. Implement ExifTool comparison tests (`#[cfg(feature = "exiftool-comparison")]`)

**Deliverables**:
- [ ] `tests/integration/format_tests.rs`
- [ ] `tests/integration/tag_tests.rs`
- [ ] `tests/integration/error_tests.rs`
- [ ] `tests/integration/compare_with_exiftool.rs`

### Phase 4: Benchmarking (Week 6)

**Tasks**:
1. Implement criterion benchmarks (`benches/integration_benchmarks.rs`)
2. Run hyperfine CLI comparison
3. Configure CI regression detection
4. Document performance targets

**Deliverables**:
- [ ] Benchmark suite runs in CI
- [ ] Performance report in `docs/performance.md`
- [ ] Baseline performance locked

### Phase 5: Documentation & Maintenance (Week 7+)

**Tasks**:
1. Document known discrepancies in `KNOWN_DISCREPANCIES.md`
2. Create triage process for test failures
3. Establish baseline update policy
4. Monitor CI test runtime and optimize if >10 minutes

**Deliverables**:
- [ ] Discrepancy tracking document
- [ ] Runbook for test failures
- [ ] Baseline versioning policy

---

## 8. Appendices

### Appendix A: Tool Versions

**Reference Versions** (as of 2024-01-15):

| **Tool** | **Version** | **Source** |
|----------|-------------|-----------|
| Perl ExifTool | 12.70 | [exiftool.org](https://exiftool.org) |
| ImageMagick | 7.1.1 | [imagemagick.org](https://imagemagick.org) |
| Radamsa | 0.6 | [gitlab.com/akihe/radamsa](https://gitlab.com/akihe/radamsa) |
| Git LFS | 3.4.0 | [git-lfs.github.com](https://git-lfs.github.com) |

### Appendix B: Useful Resources

**EXIF Specifications**:
- [CIPA DC-008-2019](https://www.cipa.jp/std/documents/download_e.html?DC-008-Translation-2019-E) - EXIF 2.32 specification
- [TIFF 6.0 Specification](https://www.adobe.io/open/standards/TIFF.html)
- [PNG Specification](http://www.libpng.org/pub/png/spec/)

**Test Data Sources**:
- [Exiv2 Test Images](https://github.com/Exiv2/exiv2/tree/main/tests/data)
- [JPEG Test Suite](https://github.com/libjpeg-turbo/libjpeg-turbo/tree/main/testimages)
- [Sample EXIF Files](https://github.com/ianare/exif-samples)

**Fuzzing Resources**:
- [cargo-fuzz Book](https://rust-fuzz.github.io/book/cargo-fuzz.html)
- [OSS-Fuzz Integration](https://google.github.io/oss-fuzz/)

### Appendix C: Contact & Support

**Questions or Issues**:
- GitHub Issues: [https://github.com/yourorg/exiftool-rs/issues](https://github.com/yourorg/exiftool-rs/issues)
- Discussions: [https://github.com/yourorg/exiftool-rs/discussions](https://github.com/yourorg/exiftool-rs/discussions)
- Matrix Chat: `#exiftool-rs:matrix.org`

**Maintainers**:
- Test Infrastructure: @test-lead
- CI/CD: @devops-lead
- Performance: @performance-lead

---

**Document Version**: 1.0.0
**Last Updated**: 2024-01-15
**Next Review**: 2024-04-15 (quarterly)
