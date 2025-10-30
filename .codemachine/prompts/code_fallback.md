# Code Refinement Task

The previous code submission did not pass verification. You must fix the following issues and resubmit your work.

---

## Original Task Description

**Task I5.T9: Comprehensive Integration Testing Against ExifTool**

Expand integration test suite from I3.T10 to cover all supported formats and operations. Test corpus: 100+ images across JPEG (various EXIF/XMP combinations), TIFF (multi-page, big/little-endian), PNG (text, eXIf), PDF (Info, XMP), MP4 (iTunes, keys/ilst). Test operations: read, write, copy, rename, date shift. Compare against ExifTool for all operations. Acceptance threshold: 98%+ tag value match for reads, successful round-trip for writes. Run as part of CI on every commit (with feature flag). Document test results in CI badge.

**Acceptance Criteria:**
- Test corpus contains 100+ diverse images ✅ **PASSED** (104 images)
- Tests cover all supported formats (JPEG, TIFF, PNG, PDF, MP4) ✅ **PASSED**
- Tests cover all operations (read, write, copy, rename, date shift) ✅ **PASSED**
- 98%+ tag match rate achieved for reads ❌ **FAILED**
- Round-trip tests pass (write → read → verify) ✅ **PASSED**
- CI runs tests on every commit (with ExifTool installed in CI environment) ✅ **PASSED**
- README shows test results badge (pass/fail) ✅ **PASSED**

---

## Issues Detected

### Critical Test Failures (7 out of 14 ExifTool comparison tests failing)

**1. Format Read Test Failures - Match Rate Below 98% Threshold:**

*   **`test_comparison_mp4`**: Match rate **73.33%** (22/30 tags matched, 8 mismatches)
    - Missing tags: `UserData:Title`, `ItemList:ContentCreateDate`, `ItemList:Comment`, `ItemList:Copyright`, `ItemList:Artist`, `ItemList:Album`, `ItemList:Genre`, `ItemList:Title`
    - Root cause: ExifTool-RS is not parsing QuickTime/iTunes metadata tags from MP4 files correctly

*   **`test_comparison_png_with_exif`**: Match rate **42.11%** (significantly below threshold)
    - Root cause: ExifTool-RS is returning tag IDs as raw hex codes (e.g., `EXIF:0x0128`, `EXIF:0x010F`) instead of human-readable tag names that Perl ExifTool returns
    - Additional tags reported that Perl doesn't have: `EXIF:0x0128`, `EXIF:0x010F`, `EXIF:0x8769`, etc.

*   **`test_comparison_pdf`**: Match rate **90.91%** (below 98% threshold)
    - Need detailed analysis of which PDF tags are mismatching

*   **`test_comparison_jpeg_with_gps`**: Match rate **87.50%** (below 98% threshold)
    - GPS tag parsing or formatting issues

*   **`test_comparison_tiff`**: Match rate **68.18%** (30/44 tags matched)
    - Missing tag: `IFD0:YCbCrPositioning` (Perl: "Centered", Rust: MISSING)
    - Root cause: Similar to PNG issue - TIFF parser returning raw hex tag IDs instead of names

*   **`test_comparison_tiff_big_endian`**: Match rate **82.35%**
    - Byte order handling issues in TIFF parser

*   **`test_comparison_tiff_multipage`**: Match rate **76.92%**
    - Multi-IFD parsing issues

**2. Code Formatting Issues:**

*   **11 files have formatting violations** that cause `cargo fmt --all -- --check` to fail
*   Affected files include:
    - `src/cli/batch_processor.rs:437`
    - `src/core/operations.rs:217, 227`
    - `tests/integration/exiftool_comparison_tests.rs:1045, 1055, 1072, 1095`
    - `tests/integration/jpeg_write_tests.rs:126`
    - `tests/integration/rename_tests.rs:116, 308`
    - And others (see full diff in verification output)
*   Issues: Long lines not properly wrapped, method chains not properly formatted

---

## Best Approach to Fix

### Phase 1: Fix Tag Name Resolution (Highest Priority)

**Problem**: The parsers are returning raw tag IDs (hex codes like `0x010F`) instead of human-readable tag names. The `lookup_tag_name()` function is likely not finding matches in the tag database.

**Files to modify:**
1. **`src/tag_db/generated_tags.rs`**: Verify tag database contains correct mappings for all standard EXIF/TIFF tags
2. **`src/parsers/png/mod.rs`**: Ensure PNG eXIf chunk parser correctly maps tag IDs to names
3. **`src/parsers/tiff/mod.rs`** (likely exists): Ensure TIFF parser correctly maps tag IDs to names
4. **`src/core/operations.rs:217, 227`**: Review how `lookup_tag_name()` is being called - ensure correct IFD names ("IFD0", "ExifIFD", "GPS", etc.) are passed

**Action Steps:**
- Add debug logging to `lookup_tag_name()` to see which tag IDs are failing to resolve
- Cross-reference with Perl ExifTool's tag database to ensure we have the same tag definitions
- For common EXIF tags like 0x010F (Make), 0x0110 (Model), 0x0128 (ResolutionUnit), these MUST be in the database
- Test with simple JPEG/TIFF files first to verify basic tag resolution works

### Phase 2: Fix MP4/QuickTime Parser

**Problem**: MP4 parser is missing 8 critical iTunes/QuickTime tags that Perl ExifTool extracts.

**Files to modify:**
1. **`src/parsers/mp4/mod.rs`** or **`src/parsers/quicktime/mod.rs`**

**Action Steps:**
- Implement parsing for QuickTime UserData atoms (contains `Title`)
- Implement parsing for iTunes metadata (ItemList `ilst` atom) containing:
  - `ContentCreateDate` (©day)
  - `Comment` (©cmt)
  - `Copyright` (cprt)
  - `Artist` (©ART)
  - `Album` (©alb)
  - `Genre` (gnre/©gen)
  - `Title` (©nam)
- Ensure atom parsing handles both 4-byte and UUID-based keys
- Map atom identifiers to standard ExifTool tag names (e.g., `ItemList:Artist`, `UserData:Title`)

### Phase 3: Fix TIFF-Specific Issues

**Files to modify:**
1. **`src/parsers/tiff/mod.rs`**

**Action Steps:**
- Fix `YCbCrPositioning` tag (0x0213) - ensure it's properly decoded (value 1 = "Centered", value 2 = "Co-sited")
- Review big-endian vs little-endian handling - 82% match rate suggests byte order issues
- For multi-page TIFF: ensure all IFDs are traversed and parsed (not just IFD0/IFD1)

### Phase 4: Fix PDF Tag Parsing

**Files to modify:**
1. **`src/parsers/pdf/mod.rs`**

**Action Steps:**
- Identify which tags are below 98% threshold by running test with verbose output
- Fix tag extraction for both PDF Info dictionary and XMP metadata streams
- Ensure date format parsing matches Perl ExifTool's output

### Phase 5: Fix GPS Tag Issues

**Files to modify:**
1. GPS tag parsing logic (likely in `src/parsers/jpeg/mod.rs` or `src/core/operations.rs`)

**Action Steps:**
- Identify which GPS tags are mismatching (run `test_comparison_jpeg_with_gps` with `--nocapture`)
- Fix GPS coordinate formatting (likely latitude/longitude conversion issues)
- Ensure GPS tag names match Perl ExifTool format (e.g., `GPS:GPSLatitude` vs `GPS:Latitude`)

### Phase 6: Fix Code Formatting

**Action**: Run `cargo fmt --all` to auto-fix all formatting issues. This should be done LAST after all code changes.

---

## Testing Strategy

After each phase, run the relevant integration tests:

```bash
# Test specific format
cargo test --features exiftool-comparison --test integration test_comparison_mp4 -- --nocapture

# Test all ExifTool comparison tests
cargo test --features exiftool-comparison --test integration exiftool_comparison_tests -- --nocapture

# Check formatting
cargo fmt --all -- --check

# Run clippy
cargo clippy --all-features -- -D warnings
```

**Success criteria**: All 14 ExifTool comparison tests must pass with match rates ≥ 98% for format read tests.

---

## Additional Context

- **Test Corpus**: Already contains 104 images (exceeds 100+ requirement) ✅
- **CI Integration**: Already configured in `.github/workflows/ci.yml` ✅
- **README Badges**: Already present ✅
- **Operation Tests**: Write, copy, rename, date shift tests are already passing ✅

The primary issue is **tag extraction and naming in the parsers**, not test infrastructure or corpus quality.

---

## Priority Order

1. **CRITICAL**: Fix tag name resolution (affects PNG, TIFF tests)
2. **CRITICAL**: Fix MP4/QuickTime parser (73% match rate)
3. **HIGH**: Fix TIFF multi-page and byte order issues
4. **MEDIUM**: Fix PDF parser (90% match rate - close but not passing)
5. **MEDIUM**: Fix GPS tag formatting
6. **LOW**: Run `cargo fmt --all` to fix formatting

Focus on getting match rates above 98% for all format read tests. The test infrastructure is solid - the parsers just need better tag extraction and naming logic.
