# Code Refinement Task

The previous code submission did not pass verification. The integration tests pass successfully with **14/14 tests at 100% match rate**, which meets the acceptance criteria. However, **8 unit tests are failing** due to namespace prefix changes and parser enhancements. You must fix these unit test failures and resubmit your work.

---

## Original Task Description

**Task I5.T9**: Expand integration test suite from I3.T10 to cover all supported formats and operations. Test corpus: 100+ images across JPEG (various EXIF/XMP combinations), TIFF (multi-page, big/little-endian), PNG (text, eXIf), PDF (Info, XMP), MP4 (iTunes, keys/ilst). Test operations: read, write, copy, rename, date shift. Compare against ExifTool for all operations.

**Acceptance threshold**: 98%+ tag value match for reads, successful round-trip for writes. Run as part of CI on every commit (with feature flag). Document test results in CI badge.

---

## Issues Detected

### Integration Test Results: ✅ PASSING
- **14/14 tests passing** with 100% match rate
- All formats covered: JPEG, TIFF, PNG, PDF, MP4
- All operations working: read, write, copy, rename, date shift
- Test corpus: 104 images (exceeds 100+ requirement)
- CI integration: ✅ Working
- README badges: ✅ Present

### Unit Test Failures: ❌ 8 TESTS FAILING

**Root Cause:** The recent code changes modified namespace prefixes in the QuickTime/MP4 parser:
- Changed `iTunes:` → `ItemList:` (to match Perl ExifTool conventions)
- Changed `QuickTime:` → `UserData:` (to match Perl ExifTool conventions)

Additionally, the PNG parser was enhanced to extract IHDR chunk metadata (ImageWidth, ImageHeight, BitDepth, etc.), which changes the expected tag counts in unit tests.

**Failing Tests:**

1. **core::operations::tests::test_raw_bytes_to_tag_value_binary**
   - Location: `src/core/operations.rs:1397`
   - Error: `assertion failed: value.is_binary()`
   - Issue: The `raw_bytes_to_tag_value()` function is not returning a Binary TagValue for UNDEFINED (type 7) field types

2. **parsers::pdf::tests::test_parse_pdf_with_info_dict**
   - Location: `src/parsers/pdf/mod.rs` (test function)
   - Error: Likely failing on tag count or specific tag extraction
   - Issue: PDF parser may have been modified or test expectations are incorrect

3. **parsers::png::tests::test_parse_minimal_png**
   - Location: `src/parsers/png/mod.rs:823`
   - Error: `assertion left == right failed: left: 7, right: 0`
   - Issue: Minimal PNG now extracts 7 tags from IHDR chunk (ImageWidth, ImageHeight, BitDepth, ColorType, Compression, Filter, Interlace) instead of 0

4. **parsers::png::tests::test_parse_png_with_text_chunk**
   - Location: `src/parsers/png/mod.rs:848`
   - Error: `assertion left == right failed: left: 8, right: 1`
   - Issue: PNG with tEXt chunk now has 7 IHDR tags + 1 tEXt tag = 8 tags (test expects 1)

5. **parsers::png::tests::test_parse_png_with_itxt_chunk**
   - Location: `src/parsers/png/mod.rs:883`
   - Error: `assertion left == right failed: left: 8, right: 1`
   - Issue: PNG with iTXt chunk now has 7 IHDR tags + 1 iTXt tag = 8 tags (test expects 1)

6. **parsers::png::tests::test_parse_png_with_exif_chunk**
   - Location: `src/parsers/png/mod.rs:924`
   - Error: `assertion left == right failed: left: 9, right: 1`
   - Issue: PNG with eXIf chunk now has 7 IHDR tags + more EXIF tags (test expects 1)

7. **parsers::quicktime::tests::test_parse_quicktime_user_data**
   - Location: `src/parsers/quicktime/mod.rs:323`
   - Error: `assertion failed: metadata.contains_key("QuickTime:Title")`
   - Issue: Tag is now named `UserData:Title` instead of `QuickTime:Title`

8. **parsers::quicktime::tests::test_parse_itunes_metadata**
   - Location: `src/parsers/quicktime/mod.rs:341`
   - Error: `assertion failed: metadata.contains_key("iTunes:Artist")`
   - Issue: Tag is now named `ItemList:Artist` instead of `iTunes:Artist`

---

## Best Approach to Fix

You must update the unit tests to match the new behavior of the parsers. The parser implementations are CORRECT (they now match Perl ExifTool conventions), so the tests need to be updated to reflect the new reality.

### Fix 1: Update QuickTime/MP4 Unit Tests

**File**: `src/parsers/quicktime/mod.rs`

**Action**: Update the test functions to use the new namespace prefixes:

**Test: `test_parse_quicktime_user_data` (line 323)**
```rust
// OLD (incorrect):
assert!(metadata.contains_key("QuickTime:Title"));
if let Some(title) = metadata.get_string("QuickTime:Title") {
    assert_eq!(title, "Test Title");
}

// NEW (correct):
assert!(metadata.contains_key("UserData:Title"));
if let Some(title) = metadata.get_string("UserData:Title") {
    assert_eq!(title, "Test Title");
}
```

**Test: `test_parse_itunes_metadata` (line 341)**
```rust
// OLD (incorrect):
assert!(metadata.contains_key("iTunes:Artist"));
if let Some(artist) = metadata.get_string("iTunes:Artist") {
    assert_eq!(artist, "Artist Name");
}

// NEW (correct):
assert!(metadata.contains_key("ItemList:Artist"));
if let Some(artist) = metadata.get_string("ItemList:Artist") {
    assert_eq!(artist, "Artist Name");
}
```

### Fix 2: Update PNG Unit Tests

**File**: `src/parsers/png/mod.rs`

**Action**: Update test expectations to account for IHDR chunk extraction (7 additional tags)

**Test: `test_parse_minimal_png` (line 823)**
```rust
// OLD (incorrect):
assert_eq!(metadata.len(), 0);

// NEW (correct):
// Minimal PNG now extracts IHDR chunk metadata (7 tags)
assert_eq!(metadata.len(), 7);
// Verify IHDR tags are present
assert!(metadata.contains_key("PNG:ImageWidth"));
assert!(metadata.contains_key("PNG:ImageHeight"));
assert!(metadata.contains_key("PNG:BitDepth"));
assert!(metadata.contains_key("PNG:ColorType"));
assert!(metadata.contains_key("PNG:Compression"));
assert!(metadata.contains_key("PNG:Filter"));
assert!(metadata.contains_key("PNG:Interlace"));
```

**Test: `test_parse_png_with_text_chunk` (line 848)**
```rust
// OLD (incorrect):
assert_eq!(metadata.len(), 1);

// NEW (correct):
// 7 IHDR tags + 1 tEXt tag = 8 total
assert_eq!(metadata.len(), 8);
assert_eq!(metadata.get_string("PNG:tEXt:Author"), Some("John Doe"));
```

**Test: `test_parse_png_with_itxt_chunk` (line 883)**
```rust
// OLD (incorrect):
assert_eq!(metadata.len(), 1);

// NEW (correct):
// 7 IHDR tags + 1 iTXt tag = 8 total
assert_eq!(metadata.len(), 8);
// Verify iTXt tag is present (check the assertion that follows this line)
```

**Test: `test_parse_png_with_exif_chunk` (line 924)**
```rust
// OLD (incorrect):
assert_eq!(metadata.len(), 1);

// NEW (correct):
// 7 IHDR tags + N EXIF tags = 9+ total
// Count may vary depending on how many EXIF tags are in the test data
assert!(metadata.len() >= 8, "Expected at least 8 tags (7 IHDR + 1+ EXIF), got {}", metadata.len());
// Verify specific EXIF tag is present (check the assertion that follows this line)
```

### Fix 3: Fix Core Operations Binary Test

**File**: `src/core/operations.rs`

**Test: `test_raw_bytes_to_tag_value_binary` (line 1397)**

The test is checking that `raw_bytes_to_tag_value()` returns a Binary TagValue for UNDEFINED (type 7) field types.

**Action**: Review the `raw_bytes_to_tag_value()` function to ensure it correctly handles type 7 (UNDEFINED) as binary data.

**Expected behavior**: When field_type is 7 (UNDEFINED), the function should return `TagValue::Binary(bytes.to_vec())`.

**Check the function implementation** around line 700-800 in `src/core/operations.rs` and ensure type 7 is handled correctly:

```rust
pub fn raw_bytes_to_tag_value(
    bytes: &[u8],
    field_type: u16,
    count: u32,
    _tag_id: u16,
    byte_order: ByteOrder,
) -> TagValue {
    match field_type {
        7 => {
            // Type 7 = UNDEFINED (should return binary)
            TagValue::Binary(bytes.to_vec())
        }
        // ... other cases ...
    }
}
```

If the function is trying to interpret type 7 as ASCII/UTF-8 string, change it to return Binary instead.

### Fix 4: Investigate PDF Test Failure

**File**: `src/parsers/pdf/mod.rs`

**Test: `test_parse_pdf_with_info_dict`**

**Action**: Run the test individually to see the exact error message:

```bash
cargo test --lib test_parse_pdf_with_info_dict -- --nocapture
```

Then fix based on the error:
- If it's a tag count issue, verify how many tags the PDF parser extracts
- If it's a missing tag, check that all Info dictionary fields are being extracted
- If it's a tag name issue, verify the namespace prefix is correct (`PDF:` prefix)

---

## Testing Instructions

After making the fixes, run:

```bash
# Run all unit tests
cargo test --lib

# Run integration tests (should still pass)
cargo test --release --features exiftool-comparison exiftool_comparison_tests

# Run linting
cargo clippy --all-features -- -D warnings
```

**Success criteria**:
- All unit tests pass: `cargo test --lib` shows 387 passed, 0 failed
- All integration tests still pass: 14/14 tests with 100% match rate
- No linting errors from clippy

---

## Files to Modify

1. **src/parsers/quicktime/mod.rs** - Update 2 unit tests to use new namespace prefixes
2. **src/parsers/png/mod.rs** - Update 4 unit tests to expect IHDR tag extraction
3. **src/core/operations.rs** - Fix binary tag value handling for type 7 (UNDEFINED)
4. **src/parsers/pdf/mod.rs** - Fix PDF test (investigate failure first)

---

## Important Notes

- **DO NOT change the parser implementations** - they are correct and match Perl ExifTool
- **DO NOT change the integration tests** - they are passing with 100% match rate
- **ONLY update the unit tests** to match the new parser behavior
- The namespace changes (`iTunes:` → `ItemList:`, `QuickTime:` → `UserData:`) are correct and intentional
- The PNG IHDR extraction is correct and intentional (provides more metadata)

---

## Expected Outcome

After this fix:
- **387/387 unit tests passing** (100%)
- **14/14 integration tests passing** (100%)
- No linting errors
- All acceptance criteria met:
  - ✅ Test corpus: 104 images
  - ✅ Format coverage: JPEG, TIFF, PNG, PDF, MP4
  - ✅ Operation coverage: read, write, copy, rename, date shift
  - ✅ Match rate: 100% for all read tests (exceeds 98% requirement)
  - ✅ Round-trip tests: passing
  - ✅ CI integration: configured
  - ✅ README badges: present
  - ✅ No linting errors
