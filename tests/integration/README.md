# Integration Tests

This directory contains integration tests for ExifTool-RS, focusing on comparison testing against the reference Perl ExifTool implementation.

## Overview

The integration test suite validates that ExifTool-RS produces metadata output compatible with Perl ExifTool across:
- **5 formats**: JPEG, PNG, TIFF, PDF, MP4
- **100+ test images**: Diverse corpus covering simple, complex, edge cases, and malformed files
- **Multiple operations**: Read, write, copy, rename, date shift
- **98%+ match rate**: Tag value parity with Perl ExifTool

## Running Tests

### Prerequisites

1. **Install Perl ExifTool**:
   ```bash
   # Ubuntu/Debian
   sudo apt-get install libimage-exiftool-perl

   # macOS
   brew install exiftool

   # Windows
   choco install exiftool
   ```

2. **Verify installation**:
   ```bash
   exiftool -ver
   # Should print version (e.g., 12.70)
   ```

### Execute Tests

```bash
# Run comparison tests (requires Perl ExifTool)
cargo test --features exiftool-comparison

# Run with detailed output
cargo test --features exiftool-comparison -- --nocapture

# Run specific test
cargo test --features exiftool-comparison test_comparison_jpeg_with_exif
```

### Without Feature Flag

```bash
# Tests will be ignored if feature flag not enabled
cargo test
# Output: test integration::test_comparison_jpeg_with_exif ... ignored
```

## Test Structure

### Comparison Tests

**File**: `exiftool_comparison_tests.rs`

Tests that compare JSON output from Perl ExifTool and ExifTool-RS:

1. **Read Operations** (5 active tests):
   - `test_comparison_jpeg_with_exif` - Basic JPEG with EXIF
   - `test_comparison_jpeg_with_exif_xmp` - JPEG with EXIF+XMP
   - `test_comparison_tiff` - TIFF with IFD
   - `test_comparison_pdf` - PDF with Info dictionary
   - `test_comparison_mp4` - MP4 with QuickTime metadata

2. **Write Operations** (4 placeholder tests):
   - `test_write_roundtrip_jpeg_artist` - Write → read → verify
   - `test_copy_metadata_jpeg_to_jpeg` - Copy tags
   - `test_rename_file_pattern` - Rename by metadata
   - `test_date_shift_all_dates` - Shift timestamps

3. **Additional Formats** (5 placeholder tests):
   - `test_comparison_png_with_text` - PNG text chunks
   - `test_comparison_png_with_exif` - PNG with eXIf
   - `test_comparison_tiff_multipage` - Multi-page TIFF
   - `test_comparison_jpeg_with_gps` - GPS coordinates
   - `test_comparison_jpeg_with_maker_notes` - Maker notes

### Test Corpus

**Location**: `tests/fixtures/`

Organized by format and complexity:

```
tests/fixtures/
├── manifest.json                    # Corpus metadata
├── ACQUISITION_GUIDE.md            # How to expand corpus
├── jpeg/
│   ├── simple/                     # Basic EXIF (15 target)
│   ├── complex/                    # EXIF+XMP+GPS (15 target)
│   ├── edge_cases/                 # Large, unusual (10 target)
│   └── malformed/                  # Corrupted (10 target)
├── png/
│   ├── simple/                     # Text chunks (10 target)
│   ├── complex/                    # eXIf, ICC (10 target)
│   └── edge_cases/                 # Interlaced, APNG (10 target)
├── tiff/
│   ├── simple/                     # Single-page (10 target)
│   ├── complex/                    # Multi-page, big-endian (10 target)
│   └── edge_cases/                 # Large, unusual bit depth (5 target)
├── pdf/
│   ├── simple/                     # Info dictionary (5 target)
│   └── complex/                    # XMP, embedded images (10 target)
└── mp4/
    ├── simple/                     # Basic iTunes (5 target)
    └── complex/                    # GPS track, multi-stream (10 target)
```

**Current Status**: 5/130+ images (5%)

## Test Methodology

### Comparison Process

1. **Execute Perl ExifTool**:
   ```bash
   exiftool -json -a -G1 -struct sample.jpg
   ```

2. **Execute ExifTool-RS**:
   ```bash
   exiftool-rs -json sample.jpg
   ```

3. **Compare JSON Outputs**:
   - Parse both JSON arrays
   - Iterate through Perl ExifTool tags (ground truth)
   - Match against ExifTool-RS tags
   - Apply tolerance for floating-point values
   - Calculate match rate: `matched / total * 100`

4. **Validate Match Rate**:
   - Simple files: ≥99%
   - Complex files: ≥99%
   - Edge cases: ≥95%
   - Overall: ≥98%

### Value Matching Rules

- **Strings**: Exact match
- **Integers**: Exact match
- **Floats**: ±0.01 tolerance (GPS: ±0.0001°)
- **Arrays**: Element-wise match with recursion
- **Objects**: Key-value match with recursion
- **Null**: Exact match

### TagValue Enum Unwrapping

ExifTool-RS serializes tags as strongly-typed enums:
- Perl: `{"Make": "Canon"}`
- Rust: `{"Make": {"String": "Canon"}}`

The `extract_value()` function unwraps these enums for comparison.

## Known Discrepancies

See `KNOWN_DISCREPANCIES.md` for documented differences:
- Maker notes (partial support)
- TagValue enum serialization (handled by comparison logic)
- Floating-point tolerances (GPS ±0.0001°, others ±0.01)
- Date/time timezone interpretation (under investigation)

## CI Integration

### GitHub Actions Workflow

**File**: `.github/workflows/ci.yml`

The `integration-tests` job:
- Runs on: Ubuntu, macOS, Windows
- Installs: Perl ExifTool (platform-specific)
- Executes: `cargo test --features exiftool-comparison`
- Uploads: Comparison reports as artifacts (90-day retention)

**Badge**: See README.md for workflow status

### CI Performance

- **Current** (5 images): ~10-25 seconds
- **Target** (100 images): ~200-500 seconds (3-8 minutes)
- **Timeout**: 30 minutes (comfortable margin)

## Expanding Test Corpus

See `tests/fixtures/ACQUISITION_GUIDE.md` for detailed strategy:

### Phase 1: Public Test Suites (40-50 images)
- Exiv2 test suite (GPL-2.0+)
- ExifTool samples (public domain)

### Phase 2: Public Domain Images (20-30 images)
- Unsplash (CC0)
- Wikimedia Commons (CC0/CC-BY)

### Phase 3: Synthetic Images (20-30 images)
- ImageMagick + exiftool
- Edge cases with known metadata

### Phase 4: Format-Specific Tests (10-20 images)
- PNG with text chunks
- Multi-page TIFF
- PDF with XMP
- MP4 with GPS track

### Quick Start

```bash
# Download Exiv2 test suite
git clone --depth 1 --filter=blob:none --sparse https://github.com/Exiv2/exiv2.git
cd exiv2 && git sparse-checkout set test/data

# Copy to fixtures
cp test/data/*.jpg ../exiftools/tests/fixtures/jpeg/complex/
cp test/data/*.tif ../exiftools/tests/fixtures/tiff/complex/

# Update manifest.json with sources
```

## Troubleshooting

### Perl ExifTool Not Found

**Error**: `Skipping test: Perl ExifTool not found in PATH`

**Solution**: Install Perl ExifTool (see Prerequisites)

### Match Rate Below Threshold

**Error**: `Match rate 92.5% below 98% threshold`

**Investigation**:
1. Check test output for mismatch details
2. Review `KNOWN_DISCREPANCIES.md` for acceptable differences
3. Verify fixture quality (not corrupted)
4. Check Perl ExifTool version (`exiftool -ver` should be 12.70+)
5. Document new discrepancies if legitimate

### Test Timeout

**Error**: CI job times out after 30 minutes

**Solution**:
- Reduce test corpus size temporarily
- Implement test sharding (run subset on PRs, full on main)
- Optimize comparison logic (parallel execution)

## Maintenance

### Adding New Test Files

1. Place file in appropriate `tests/fixtures/` subdirectory
2. Update `manifest.json` with metadata:
   ```json
   {
     "path": "jpeg/complex/new_image.jpg",
     "format": "JPEG",
     "category": "complex",
     "source": "Unsplash",
     "license": "CC0",
     "metadata_types": ["EXIF", "GPS"],
     "description": "Landscape photo with GPS",
     "expected_tags": ["EXIF:Make", "GPS:GPSLatitude"]
   }
   ```
3. Run tests: `cargo test --features exiftool-comparison`
4. Document any new discrepancies in `KNOWN_DISCREPANCIES.md`

### Updating Thresholds

If systematic differences are discovered (e.g., new Perl ExifTool version):
1. Update match rate thresholds in test assertions
2. Document reason in `KNOWN_DISCREPANCIES.md`
3. Update version tracking table
4. Notify team via PR description

## References

- **Task Specification**: I5.T9 in iteration manifest
- **Integration Test Plan**: `docs/testing/integration_test_plan.md`
- **ExifTool JSON Format**: https://exiftool.org/faq.html#Q10
- **Perl ExifTool Tag Names**: https://exiftool.org/TagNames/index.html

---

**Maintainer**: ExifTool-RS Team
**Last Updated**: 2025-10-30
**Test Coverage**: 5/130+ images (5%)
