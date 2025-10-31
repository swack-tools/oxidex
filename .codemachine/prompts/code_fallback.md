# Code Refinement Task

The previous code submission did not pass verification. You must fix the following issues and resubmit your work.

---

## Original Task Description

The code changes attempted to improve metadata tag handling, including:
- Fixing PDF Keywords parsing (keeping as string instead of array)
- Changing QuickTime iTunes tag prefix from `ItemList:` to `iTunes:`
- Enhancing TIFF writer to properly handle sub-IFDs (ExifIFD, GPS IFD)

---

## Issues Detected

### 1. PNG EXIF Writing Broken (Critical)
**Test Failures:**
- `png_write_tests::test_write_exif_chunk` - FAILED
- `png_write_tests::test_mixed_metadata_types` - FAILED

**Root Cause:**
The `serialize_exif_chunk()` function in `src/writers/png_writer.rs` only accepts tags with `EXIF:` prefix:
```rust
if tag_name.starts_with("EXIF:") {
    exif_metadata.insert(tag_name, tag_value.clone());
}
```

But tests (and the TIFF parser) use prefixes like `IFD0:`, `IFD1:`, `ExifIFD:`, `GPS:` for EXIF tags.

**Expected Behavior:**
The function should accept all TIFF-writable tag prefixes: `IFD0:`, `IFD1:`, `ExifIFD:`, `GPS:`, `EXIF:`, `InteropIFD:`, `MakerNotes:`

### 2. QuickTime Tag Prefix Inconsistency
**Test Failures:**
- `mp4_tests::test_parse_sample_mp4_metadata` - FAILED
- `mp4_tests::test_parse_mp4_*` - Multiple failures

**Root Cause:**
Changed tag prefix from `ItemList:` to `iTunes:` in `src/parsers/quicktime/metadata_extractor.rs`, but this breaks compatibility with existing test fixtures and expectations.

**Impact:**
All tests expecting `ItemList:Album`, `ItemList:Artist`, etc. now fail because tags are returned as `iTunes:Album`, `iTunes:Artist`.

### 3. PDF Metadata Parsing Issues
**Test Failure:**
- `pdf_tests::test_parse_sample_pdf_metadata` - FAILED: "PDF:CreationDate not found"

**Root Cause:**
Unclear - the PDF parser changes only affected Keywords handling. Need to investigate why CreationDate is missing.

### 4. TIFF Writer Sub-IFD Issues
**Test Failure:**
- `write_operations_tests::test_write_metadata_with_integer_tags` - FAILED

**Root Cause:**
The new `serialize_ifd_with_pointers()` function and sub-IFD logic in `reconstruct_tiff_structure()` may have issues with:
- Incorrect offset calculations
- Missing constants (EXIF_IFD_POINTER, GPS_INFO_IFD_POINTER need to be defined)
- Tag filtering logic separating IFD0 vs ExifIFD vs GPS tags

### 5. Date Shift Tests Failing
**Test Failures:**
- All `date_shift_tests::*` tests failing

**Root Cause:**
Tests expect `ExifIFD:DateTimeOriginal` but this tag may not be present in test metadata or the tag name has changed.

### 6. Rename Tests Failing
**Test Failures:**
- All `rename_tests::*` tests failing

**Root Cause:**
Same as #5 - looking for `ExifIFD:DateTimeOriginal` tag that's not found.

---

## Best Approach to Fix

### Priority 1: Fix PNG EXIF Writer (Critical - Breaks Core Functionality)

**File:** `src/writers/png_writer.rs`
**Function:** `serialize_exif_chunk()`

**Fix:**
```rust
fn serialize_exif_chunk(metadata: &MetadataMap) -> Result<Vec<u8>> {
    // Filter only TIFF-writable EXIF tags
    let mut exif_metadata = MetadataMap::new();
    for (tag_name, tag_value) in metadata.iter() {
        // Accept all TIFF-compatible prefixes
        let is_tiff_writable = tag_name.starts_with("IFD0:")
            || tag_name.starts_with("IFD1:")
            || tag_name.starts_with("ExifIFD:")
            || tag_name.starts_with("GPS:")
            || tag_name.starts_with("EXIF:")
            || tag_name.starts_with("InteropIFD:")
            || tag_name.starts_with("MakerNotes:");

        if is_tiff_writable {
            exif_metadata.insert(tag_name, tag_value.clone());
        }
    }

    // Rest of the function remains the same...
}
```

### Priority 2: Revert QuickTime Tag Prefix Change

**Files:**
- `src/parsers/quicktime/metadata_extractor.rs`
- `src/parsers/quicktime/mod.rs`
- `tests/integration/png_tests.rs` (if modified)

**Fix:**
Revert all `iTunes:` prefix changes back to `ItemList:` to maintain compatibility. The original ExifTool uses `ItemList:` prefix for iTunes metadata tags.

**Alternatively**, if you want to keep `iTunes:` prefix:
- Update ALL test expectations to use `iTunes:` instead of `ItemList:`
- Update test fixtures
- Document this as a breaking change

**Recommendation:** Revert to `ItemList:` for v1.0.0 compatibility.

### Priority 3: Fix TIFF Writer Sub-IFD Implementation

**File:** `src/writers/tiff_writer.rs`

**Missing Constants:**
Add these constants at the top of the file:
```rust
// IFD pointer tag IDs (from TIFF/EXIF specification)
const EXIF_IFD_POINTER: u16 = 0x8769;  // ExifOffset
const GPS_INFO_IFD_POINTER: u16 = 0x8825;  // GPSInfo
```

**Fix Offset Calculations:**
The current implementation's offset calculations in `reconstruct_tiff_structure()` are complex and may be incorrect. You need to:
1. Calculate IFD0 size first (without pointers)
2. Add space for pointer entries
3. Recalculate with correct total size
4. Serialize each IFD with correct offsets

**Simplify or Debug:**
Add debug logging to verify:
- IFD0 byte size calculation
- ExifIFD offset value
- GPS IFD offset value
- Whether tags are being correctly categorized into IFD0 vs ExifIFD vs GPS

### Priority 4: Investigate PDF CreationDate Issue

**File:** `src/parsers/pdf/info_parser.rs` or `src/parsers/pdf/mod.rs`

**Debug Steps:**
1. Check if the test fixture actually contains a CreationDate field
2. Verify the PDF parser is correctly extracting the Info dictionary
3. Check if date parsing logic was accidentally modified

**Likely Fix:**
The PDF parser changes only touched Keywords handling. The CreationDate failure might be a pre-existing issue or test environment problem. Check the actual PDF test file.

### Priority 5: Fix Date/Rename Test Tag Names

**Files:**
- `tests/integration/date_shift_tests.rs`
- `tests/integration/rename_tests.rs`

**Analysis Needed:**
Check test fixture metadata to see what tag names are actually present. The tests expect `ExifIFD:DateTimeOriginal` but:
- Is this tag present in the test image?
- Has the tag name changed due to parser updates?
- Is the tag being read with a different prefix?

**Possible Fix:**
Update test assertions to use correct tag names, OR ensure test fixtures have the expected tags.

---

## Testing Strategy

After making fixes, run tests in this order:

1. **Unit tests first:**
   ```bash
   cargo test --lib
   ```

2. **PNG write tests:**
   ```bash
   cargo test png_write_tests::
   ```

3. **QuickTime tests:**
   ```bash
   cargo test mp4_tests::
   ```

4. **All integration tests:**
   ```bash
   cargo test --test integration
   ```

5. **Verify no linting errors:**
   ```bash
   cargo clippy --all-targets --all-features -- -D warnings
   ```

---

## Implementation Order

1. **Fix PNG writer** (Priority 1) - 10 minutes
2. **Revert QuickTime prefixes** (Priority 2) - 5 minutes
3. **Add TIFF constants and debug** (Priority 3) - 15 minutes
4. **Investigate PDF/Date/Rename issues** (Priority 4 & 5) - 20 minutes
5. **Run full test suite and iterate** - 30 minutes

**Total estimated time: ~1.5 hours**

---

## Success Criteria

- All 122 integration tests pass
- `cargo clippy` shows no warnings
- `cargo test --all-features` exits with code 0
- No regressions in existing functionality
