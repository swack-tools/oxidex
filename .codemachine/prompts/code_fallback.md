# Code Refinement Task

The previous code submission did not pass verification. You must fix the following issues and resubmit your work.

---

## Original Task Description

**Task: v1.0.0 Release Preparation - Final Verification**

Complete all 55 development tasks across 5 iterations for the ExifTool-RS project v1.0.0 stable release. The codebase should be production-ready with:
- All format parsers and writers working correctly
- Comprehensive test coverage passing
- No linting errors
- Complete documentation
- Cross-platform binaries ready for distribution

---

## Issues Detected

The verification shows good progress (34 remaining failures, down from 70). All 387 unit tests pass and no linting errors exist. However, **34 integration tests are still failing** due to the following issues:

### **Issue #1: MP4 Tag Family Naming Inconsistency**

*   **Root Cause:** The MP4 parser in `src/parsers/mp4_parser.rs` outputs tags with "ItemList:" prefix (e.g., "ItemList:Title", "ItemList:Artist"), but integration tests expect "iTunes:" prefix (e.g., "iTunes:Title", "iTunes:Artist").
*   **Impact:** All MP4 integration tests fail because expected tag names don't match actual parsed tag names.
*   **Affected Tests:**
    - `mp4_tests::test_parse_sample_mp4_metadata`
    - `mp4_tests::test_parse_mp4_extracts_multiple_tags`
    - `mp4_tests::test_parse_mp4_copyright_tag`
    - `mp4_tests::test_parse_mp4_genre_tag`
    - `mp4_tests::test_parse_mp4_with_quicktime_user_data`
    - `mp4_tests::test_mp4_both_itunes_and_quicktime_metadata`
*   **Evidence:** Test output shows `ItemList:Title: String("Test Video 1")` but test expects `metadata.contains_key("iTunes:Title")`.

### **Issue #2: PNG EXIF Write/Read Tag Name Mismatch**

*   **Root Cause:** When writing PNG metadata, tags are provided with human-readable names (e.g., "IFD0:Make", "IFD0:Model"), but the PNG parser reads EXIF data back as hex tag IDs (e.g., "EXIF:0x010F", "EXIF:0x0110").
*   **Impact:** PNG write tests fail because tags cannot be read back with the same names they were written with.
*   **Affected Tests:**
    - `png_write_tests::test_write_exif_chunk` (expects "EXIF:0x010F" for Make, got None)
    - `png_write_tests::test_mixed_metadata_types` (expects "TestMake", got None)
*   **Evidence:** Test comment at line 263 says "PNG parser returns EXIF tags with hex notation (EXIF:0x010F) not names (EXIF:Make)".

### **Issue #3: TIFF/JPEG Write Not Persisting ExifIFD Tags**

*   **Root Cause:** The TIFF writer's tag filtering logic (line 277-283 in `src/writers/tiff_writer.rs`) now accepts IFD0/IFD1/ExifIFD/GPS tags, but there may be an issue with how ExifIFD tags are being serialized or written to the file structure.
*   **Impact:** Tags written with "ExifIFD:" prefix (e.g., "ExifIFD:ISO", "ExifIFD:DateTimeOriginal") are not being read back after write operations.
*   **Affected Tests:**
    - `write_operations_tests::test_write_metadata_with_integer_tags` (writes "ExifIFD:ISO", reads back None)
    - All rename_tests (depend on "ExifIFD:DateTimeOriginal" being present)
    - All date_shift_tests (depend on ExifIFD date/time tags)
*   **Evidence:** Test writes `metadata.insert("ExifIFD:ISO", TagValue::new_integer(400))` but `assert_eq!(updated_metadata.get_integer("ExifIFD:ISO"), Some(400))` fails with `left: None, right: Some(400)`.

### **Issue #4: PDF Sample Fixture Data Issues**

*   **Root Cause:** The `tests/fixtures/pdf/sample.pdf` file may not contain the expected metadata fields that tests are looking for.
*   **Impact:** PDF integration tests fail when trying to read or verify metadata.
*   **Affected Tests:**
    - `pdf_tests::test_parse_sample_pdf_metadata`
    - `pdf_write_tests::test_write_to_sample_fixture`
    - `pdf_write_tests::test_write_multiple_field_modifications`
    - `pdf_write_tests::test_write_with_long_values`

### **Issue #5: PNG Sample Fixture Data Issues**

*   **Root Cause:** The `tests/fixtures/png/sample.png` file may be missing expected metadata chunks.
*   **Impact:** PNG tests fail when expecting metadata that doesn't exist in the fixture.
*   **Affected Tests:**
    - `png_tests::test_png_with_text_chunks`
    - `png_tests::test_png_with_exif_chunk`
    - `png_tests::test_png_with_mixed_metadata`
    - `png_tests::test_png_empty_metadata`

---

## Best Approach to Fix

You MUST address these issues in the following order:

### **Step 1: Fix MP4 Tag Family Names (CRITICAL)**

**File:** `src/parsers/mp4_parser.rs`

**Problem:** Parser outputs "ItemList:Title" but should output "iTunes:Title" for compatibility with ExifTool naming conventions.

**Required fix:**
- Locate the code that adds ItemList metadata tags (likely around handling `ilst` atom)
- Change the tag family prefix from "ItemList:" to "iTunes:" for all iTunes metadata tags
- Common iTunes tags: Title, Artist, Album, Year, Comment, Genre, Encoder, Copyright

**Example:**
```rust
// Change from:
metadata.insert("ItemList:Title", value);
// To:
metadata.insert("iTunes:Title", value);
```

### **Step 2: Fix PNG EXIF Tag Naming Consistency**

**File:** `src/parsers/png_parser.rs`

**Problem:** PNG parser returns hex tag IDs ("EXIF:0x010F") instead of human-readable names ("IFD0:Make").

**Required fix:**
- When parsing EXIF data from PNG eXIf chunks, use the tag registry to resolve tag IDs to human-readable names
- Instead of formatting tags as "EXIF:0x{:04X}", look up the tag descriptor and use its canonical name
- If tag ID is 0x010F (271 decimal), output "IFD0:Make" not "EXIF:0x010F"
- Apply this to all EXIF tags parsed from PNG files

**Recommended approach:**
```rust
// After reading tag ID from EXIF data:
if let Some(descriptor) = get_tag_descriptor_by_id(tag_id) {
    let tag_name = descriptor.name(); // Returns "IFD0:Make"
    metadata.insert(tag_name, value);
} else {
    // Fallback to hex notation for unknown tags
    metadata.insert(&format!("EXIF:0x{:04X}", tag_id), value);
}
```

### **Step 3: Fix TIFF Writer ExifIFD Serialization**

**File:** `src/writers/tiff_writer.rs`

**Problem:** Tags with "ExifIFD:" prefix are not being written to EXIF IFD structure properly.

**Analysis needed:**
1. Check if the `serialize_ifd()` function is being called separately for IFD0 and ExifIFD
2. Verify that tags starting with "ExifIFD:" are being separated from "IFD0:" tags
3. Ensure the EXIF IFD pointer (tag 0x8769) is being added to IFD0 with the correct offset
4. Confirm ExifIFD tags are being written to a separate IFD structure

**Required fix:**
- The TIFF writer needs to build TWO IFD structures: one for IFD0 tags and one for ExifIFD tags
- Tags starting with "IFD0:" go into the main IFD0
- Tags starting with "ExifIFD:" go into a separate EXIF IFD
- IFD0 must contain tag 0x8769 (ExifOffset) pointing to the location of the EXIF IFD
- Same logic applies for GPS IFD (tag 0x8825) if GPS tags are present

### **Step 4: Regenerate/Verify PDF and PNG Sample Fixtures**

**Required actions:**

1. **For PDF sample fixture:**
   - Use a tool to add Info dictionary metadata to `tests/fixtures/pdf/sample.pdf`
   - Required fields: Title, Author, Subject, Creator, Producer, CreationDate, ModDate
   - Example using exiftool: `exiftool -Title="Test PDF" -Author="Test Author" -Subject="Test Subject" tests/fixtures/pdf/sample.pdf`

2. **For PNG sample fixture:**
   - Ensure `tests/fixtures/png/sample.png` has appropriate metadata chunks
   - Add tEXt chunks: Title, Author, Description
   - OR add eXIf chunk with basic EXIF data (Make, Model, etc.)
   - You can create this programmatically or use a tool

3. **Verify fixture contents:**
   - After creating/modifying fixtures, read them with the parser and print all tags
   - Ensure the tags match what the tests expect

### **Step 5: Re-run Tests and Verify**

After making the above fixes:

1. Run `cargo clippy --all-targets --all-features -- -D warnings` (should still pass with no warnings)
2. Run `cargo test --lib --all-features` (should still show 387 passing)
3. Run `cargo test --test integration --all-features` to verify all integration tests now pass
4. If any tests still fail, analyze the specific failure and iterate

---

## Expected Outcome

After completing all fixes:
- ✅ All 387 unit tests pass (currently passing)
- ✅ All 122 integration tests pass (currently 88 passing, 34 failing)
- ✅ No linting errors (currently passing)
- ✅ MP4 tests pass with correct "iTunes:" tag family
- ✅ PNG write/read roundtrip works with consistent tag names
- ✅ TIFF/JPEG writes correctly persist ExifIFD tags
- ✅ PDF and PNG sample fixtures contain expected metadata

**Priority:** Focus on Steps 1-3 first as they fix code issues. Step 4 (fixtures) can be done in parallel or after if fixture data is genuinely missing.
