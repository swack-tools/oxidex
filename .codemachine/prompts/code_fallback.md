# Code Refinement Task

The previous code submission did not pass verification. You must fix the following issues and resubmit your work.

---

## Original Task Description

The code changes attempted to improve metadata tag handling across multiple writers:
- Fixed PDF date tag names (CreationDate vs CreateDate, ModDate vs ModifyDate)
- Enhanced JPEG writer to use full TIFF structure with sub-IFDs
- Enhanced PNG writer to accept all TIFF-compatible tag prefixes
- Enhanced TIFF writer with sub-IFD pointer support

---

## Issues Detected

### 1. QuickTime/MP4 Tag Prefix Breaking Changes (Critical)
**Test Failures:** 6 tests
- `mp4_tests::test_parse_sample_mp4_metadata`
- `mp4_tests::test_parse_mp4_extracts_multiple_tags`
- `mp4_tests::test_parse_mp4_genre_tag`
- `mp4_tests::test_parse_mp4_copyright_tag`
- `mp4_tests::test_parse_mp4_with_quicktime_user_data`
- `mp4_tests::test_parse_mp4_atom_hierarchy`

**Root Cause:**
Tests expect tags like `ItemList:Album`, `ItemList:Artist`, `ItemList:Genre` but the code is now returning them with `iTunes:` prefix instead.

**Impact:**
This appears to be an UNINTENDED change from a previous iteration. The current git diff does NOT show any QuickTime parser changes, which means this was changed earlier and is now causing cascading failures.

**Evidence:**
```
Expected: ItemList:Album
Found: iTunes:Album
```

**Fix Required:**
You need to examine `src/parsers/quicktime/metadata_extractor.rs` and `src/parsers/quicktime/mod.rs` to see if the prefix was changed from `ItemList:` to `iTunes:`. If so, you have TWO options:

**Option A (Recommended):** Revert the prefix back to `ItemList:` to maintain compatibility with existing tests and documentation.

**Option B:** Update ALL test expectations to use `iTunes:` prefix. This would require changes in:
- `tests/integration/mp4_tests.rs` (all assertions)
- Any documentation referencing these tags
- Mark as a breaking change

### 2. Date Shift Tests Failing (7 tests)
**Test Failures:**
- `date_shift_tests::test_shift_dates_add_one_day`
- `date_shift_tests::test_shift_dates_add_hours_and_minutes`
- `date_shift_tests::test_shift_dates_complex_offset`
- `date_shift_tests::test_shift_dates_set_absolute`
- `date_shift_tests::test_shift_dates_subtract_one_month`
- `date_shift_tests::test_shift_dates_preserves_other_tags`
- `date_shift_tests::test_shift_specific_tag_only`

**Root Cause:**
Tests are looking for `ExifIFD:DateTimeOriginal` tag but it's not present in the test fixture metadata. This could be because:
1. The test fixture doesn't have this tag
2. The TIFF/EXIF parser is not extracting this tag correctly
3. The tag name has changed

**Fix Required:**
1. First, verify what tags ARE present in the test fixture:
   ```rust
   // Add debug output to see actual tags in test
   println!("Available tags: {:#?}", metadata.keys());
   ```
2. Check if `DateTimeOriginal` is being parsed with a different prefix (e.g., `IFD0:DateTimeOriginal`)
3. Update the date shift logic to look for the correct tag name
4. OR ensure the test fixture actually contains the expected tag

### 3. Rename Tests Failing (8 tests)
**Test Failures:**
- `rename_tests::test_build_new_filename_simple_tag`
- `rename_tests::test_build_new_filename_with_date_format`
- `rename_tests::test_build_new_filename_with_extension`
- `rename_tests::test_rename_file_dry_run`
- `rename_tests::test_rename_file_actual_rename`
- `rename_tests::test_rename_file_collision_detection`
- `rename_tests::test_rename_preserves_file_in_same_directory`
- `rename_tests::test_rename_sanitizes_invalid_characters`

**Root Cause:**
Same as issue #2 - tests expect `ExifIFD:DateTimeOriginal` but it's not found.

**Error Message:**
```
ParseError { message: "Tag 'ExifIFD:DateTimeOriginal' not found in metadata" }
```

**Fix Required:**
Same fix as issue #2 - identify the correct tag name being used by the parser and update the tests accordingly.

### 4. PDF Write Test Failing
**Test Failure:**
- `pdf_write_tests::test_write_to_sample_fixture`

**Root Cause:**
The test expects `PDF:Title` to be "Modified Sample Title" after writing, but it's returning `None`.

**Assertion:**
```rust
assertion `left == right` failed
  left: None
 right: Some("Modified Sample Title")
```

**Fix Required:**
1. Check if PDF writer is correctly writing the Title field to the Info dictionary
2. Verify the PDF parser can read back the written title
3. Check if there's a mismatch between write and read operations

### 5. TIFF Write Test Failing
**Test Failure:**
- `write_operations_tests::test_write_metadata_with_integer_tags`

**Root Cause:**
The test expects an integer tag to be written and read back, but it's returning `None`.

**Assertion:**
```rust
assertion `left == right` failed
  left: None
 right: Some(1)
```

**Fix Required:**
1. Check if the TIFF writer's new sub-IFD logic is correctly handling integer tags
2. Verify offset calculations in `reconstruct_tiff_structure()` are correct
3. The git diff shows this function was modified - there may be an issue with the two-pass approach or pointer entry serialization
4. Add debug logging to trace:
   - Which IFD the tag is being written to (IFD0 vs ExifIFD vs GPS)
   - The calculated offsets
   - The actual bytes being written

### 6. JPEG XMP and ExifTool Comparison Tests Failing
**Test Failures:**
- `jpeg_tests::test_jpeg_xmp_extraction_end_to_end`
- `exiftool_comparison_tests::test_comparison_mp4`
- `exiftool_comparison_tests::test_comparison_pdf`

**Root Cause:**
These are likely cascading failures from the issues above (QuickTime prefix changes for MP4, PDF write issues for PDF).

**Fix Required:**
These should pass once issues #1, #4 are fixed.

---

## Best Approach to Fix

### Step 1: Fix QuickTime Tag Prefix (Priority 1 - Blocks 6 tests)

**Files to check:**
- `src/parsers/quicktime/metadata_extractor.rs`
- `src/parsers/quicktime/mod.rs`

**Action:**
Search for where tags are being prefixed with `iTunes:` and change back to `ItemList:`:

```bash
grep -r "iTunes:" src/parsers/quicktime/
```

Change any instances of:
```rust
format!("iTunes:{}", tag_name)
```
To:
```rust
format!("ItemList:{}", tag_name)
```

### Step 2: Fix Date/Rename Tag Name Issues (Priority 2 - Blocks 15 tests)

**Investigation steps:**

1. **Find what tags are actually in the test fixture:**
   ```rust
   // In test setup
   let metadata = read_metadata(&path).unwrap();
   for (key, value) in metadata.iter() {
       if key.contains("DateTime") || key.contains("Date") {
           println!("Date tag: {} = {:?}", key, value);
       }
   }
   ```

2. **Check TIFF/EXIF parser output:**
   - Look at `src/parsers/tiff/ifd_parser.rs`
   - Verify how `DateTimeOriginal` is being prefixed
   - Check if it's `IFD0:DateTimeOriginal`, `EXIF:DateTimeOriginal`, or `ExifIFD:DateTimeOriginal`

3. **Update date shift and rename operations:**
   - Once you know the correct tag name, update:
     - `src/core/date_shift.rs` - `SHIFTABLE_DATE_TAGS` constant
     - `tests/integration/date_shift_tests.rs` - test expectations
     - `tests/integration/rename_tests.rs` - test patterns

**Likely fix:**
```rust
// In date_shift.rs
const SHIFTABLE_DATE_TAGS: &[&str] = &[
    "IFD0:DateTimeOriginal",  // Change from "ExifIFD:DateTimeOriginal"
    "IFD0:CreateDate",
    // ... rest of tags
];
```

### Step 3: Fix PDF Write Issue (Priority 3 - Blocks 2 tests)

**File:** `src/writers/pdf_writer.rs`

**Debug steps:**

1. Add logging to see if Title is being written:
   ```rust
   println!("Writing PDF Title: {:?}", metadata.get("PDF:Title"));
   ```

2. Check if the PDF Info dictionary is being correctly updated

3. Verify the file is being properly closed/flushed before reading back

**Potential issue:**
The PDF writer may not be correctly serializing the Info dictionary, or the byte offsets for the updated dictionary may be wrong.

### Step 4: Fix TIFF Write Integer Tag Issue (Priority 4 - Blocks 1 test)

**File:** `src/writers/tiff_writer.rs`

**Debug the new `reconstruct_tiff_structure()` function:**

The git diff shows significant changes to offset calculation logic. The issue is likely in:

1. **The two-pass approach:** The code serializes IFD0 with placeholder offsets, calculates size, then re-serializes with correct offsets. This may have an off-by-one error.

2. **Pointer entry handling:** Check `serialize_ifd_with_pointers()` to ensure:
   - Pointer entries are correctly inserted
   - The entry count is updated to include pointers
   - Offsets account for the additional pointer entries

3. **Tag categorization:** Verify integer tags are going to the right IFD:
   ```rust
   // Add debug output
   println!("Tag {} going to: {}", tag_name,
       if tag_name.starts_with("ExifIFD:") { "ExifIFD" }
       else if tag_name.starts_with("GPS:") { "GPS" }
       else { "IFD0" });
   ```

**Specific area to check:**
```rust
// This calculation may be wrong
let ifd0_temp = serialize_ifd_with_pointers(&ifd0_metadata, byte_order, ifd0_start_offset, &pointer_entries)?;
let ifd0_size = ifd0_temp.len() as u64;

// The actual size when re-serialized with real offsets might differ!
```

### Step 5: Verify All Tests Pass

After each fix, run:

```bash
# Run specific test suite
cargo test date_shift_tests::
cargo test rename_tests::
cargo test mp4_tests::
cargo test pdf_write_tests::
cargo test write_operations_tests::

# Run full integration suite
cargo test --test integration

# Verify no clippy warnings
cargo clippy --all-targets --all-features -- -D warnings
```

---

## Implementation Order

1. **Fix QuickTime prefix** (10 minutes) - Revert `iTunes:` back to `ItemList:`
2. **Investigate and fix date tag names** (20 minutes) - Find correct tag name and update code
3. **Fix PDF write** (15 minutes) - Debug and fix PDF Info dict serialization
4. **Fix TIFF integer write** (25 minutes) - Debug offset calculations in reconstruct_tiff_structure
5. **Run full test suite** (10 minutes) - Verify all 122 tests pass

**Total estimated time: ~1.5 hours**

---

## Success Criteria

- ✅ All 122 integration tests pass
- ✅ `cargo clippy --all-targets --all-features -- -D warnings` shows no warnings
- ✅ `cargo test --all-features` exits with code 0
- ✅ No regressions in existing functionality
- ✅ Git diff shows only intentional changes

---

## Critical Notes

1. **Do NOT change test expectations unless absolutely necessary** - The tests represent the expected behavior. Fix the code, not the tests.

2. **The QuickTime prefix issue is a regression** - This was likely changed in a previous task and needs to be reverted.

3. **Tag name consistency is critical** - Whatever prefix the parser uses must match what the rest of the system expects. Check the tag database and parser output.

4. **TIFF writer changes are complex** - The new sub-IFD logic needs careful review. Consider adding comprehensive logging to debug offset calculations.

5. **Test fixtures are authoritative** - If tests fail because tags aren't found, first verify what tags the test fixture actually contains before changing the code.
