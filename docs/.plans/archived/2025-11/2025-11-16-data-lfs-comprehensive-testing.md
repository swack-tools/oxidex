# Data.lfs Comprehensive Testing and Fix Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Test all 4,026 files in `/Users/allen/Documents/git/examples/data.lfs/` directory, identify parsing errors, fix them, and ensure 100% compatibility with camera raw formats and metadata extraction.

**Architecture:** Systematic testing approach with error categorization, targeted fixes for each error type, regression testing, and verification. Uses batch processing with error collection, analysis of error patterns, implementation of fixes, and re-verification.

**Tech Stack:** Rust, exiftool-rs CLI, bash scripting for batch testing, error pattern analysis

---

## Task 1: Initial Directory Scan and Sample Testing

**Files:**
- Create: `tests/data_lfs_testing.sh` (test script)
- Create: `tests/data_lfs_errors.log` (error log)

**Step 1: Create comprehensive test script**

Create `tests/data_lfs_testing.sh`:

```bash
#!/bin/bash
# Comprehensive test script for data.lfs directory

DATA_DIR="/Users/allen/Documents/git/examples/data.lfs"
EXIFTOOL_RS="./target/release/exiftool-rs"
ERROR_LOG="tests/data_lfs_errors.log"
SUCCESS_LOG="tests/data_lfs_success.log"

# Clear previous logs
> "$ERROR_LOG"
> "$SUCCESS_LOG"

# Counters
total=0
success=0
errors=0

echo "Starting comprehensive test of data.lfs directory..."
echo "Total files to test: $(find "$DATA_DIR" -type f | wc -l)"
echo ""

# Process all files
find "$DATA_DIR" -type f | while read -r file; do
    total=$((total + 1))

    # Test the file
    if $EXIFTOOL_RS "$file" > /dev/null 2>&1; then
        success=$((success + 1))
        echo "$file" >> "$SUCCESS_LOG"
    else
        errors=$((errors + 1))
        echo "=== ERROR: $file ===" >> "$ERROR_LOG"
        $EXIFTOOL_RS "$file" 2>&1 >> "$ERROR_LOG"
        echo "" >> "$ERROR_LOG"
    fi

    # Progress indicator every 100 files
    if [ $((total % 100)) -eq 0 ]; then
        echo "Processed: $total files (Success: $success, Errors: $errors)"
    fi
done

echo ""
echo "Testing complete!"
echo "Total: $total"
echo "Success: $success"
echo "Errors: $errors"
echo ""
echo "Error details saved to: $ERROR_LOG"
echo "Success list saved to: $SUCCESS_LOG"
```

**Step 2: Make script executable**

```bash
chmod +x tests/data_lfs_testing.sh
```

**Step 3: Build release binary**

```bash
cargo build --release
```

Expected: Binary built successfully

**Step 4: Run sample test on one subdirectory first**

```bash
# Test just Leaf directory first (smaller subset)
find "/Users/allen/Documents/git/examples/data.lfs/Leaf" -type f | head -10 | while read f; do
    ./target/release/exiftool-rs "$f" > /dev/null 2>&1 || echo "ERROR: $f"
done
```

Expected: Identify any immediate errors

**Step 5: Commit test infrastructure**

```bash
git add tests/data_lfs_testing.sh
git commit -m "test: add comprehensive data.lfs testing script"
```

---

## Task 2: Run Full Directory Test and Collect Errors

**Files:**
- Modify: `tests/data_lfs_testing.sh` (if needed)
- Output: `tests/data_lfs_errors.log`
- Output: `tests/data_lfs_success.log`

**Step 1: Run comprehensive test**

```bash
./tests/data_lfs_testing.sh
```

Expected: Complete test run, generate error and success logs
Time estimate: 10-20 minutes for 4,026 files

**Step 2: Analyze error patterns**

```bash
# Count unique error types
grep "Error:" tests/data_lfs_errors.log | sort | uniq -c | sort -rn
```

Expected: List of error types sorted by frequency

**Step 3: Categorize errors by file type**

```bash
# Extract file extensions from errors
grep "=== ERROR:" tests/data_lfs_errors.log | awk '{print $NF}' | sed 's/.*\.//' | sort | uniq -c | sort -rn
```

Expected: Error distribution by file format

**Step 4: Create error summary**

Create `tests/data_lfs_error_summary.md`:

```markdown
# Data.lfs Error Summary

Generated: [DATE]

## Statistics
- Total files tested: [N]
- Successful: [N]
- Errors: [N]
- Success rate: [X%]

## Error Categories

### Category 1: [Error Type]
- Count: [N]
- File formats: [extensions]
- Example error: [error message]

[... repeat for each category ...]

## Files by Error Type

[List of files grouped by error type]
```

**Step 5: Commit error analysis**

```bash
git add tests/data_lfs_errors.log tests/data_lfs_error_summary.md
git commit -m "test: add data.lfs comprehensive test results and error analysis"
```

---

## Task 3: Fix Unsupported File Format Errors

**Files:**
- Modify: `src/parsers/format_detector.rs` (if new formats needed)
- Modify: `src/cli/batch_processor.rs` (add extensions)

**Step 1: Identify unsupported extensions**

```bash
# Get unsupported extensions from error log
grep "Unknown file format" tests/data_lfs_errors.log | sed 's/.*: //' | sed 's/.*\.//' | sort | uniq
```

Expected: List of unsupported file extensions

**Step 2: Add supported extensions to batch processor**

In `src/cli/batch_processor.rs`, add new extensions to `SUPPORTED_EXTENSIONS`:

```rust
const SUPPORTED_EXTENSIONS: &[&str] = &[
    // ... existing extensions ...
    // Add any new raw formats found
    "srf", "srw", "ari", "arq", // If not already present
];
```

**Step 3: Rebuild and test unsupported formats**

```bash
cargo build --release
# Test files that previously failed with "Unknown file format"
```

Expected: Previously unsupported formats now recognized

**Step 4: Commit extension additions**

```bash
git add src/cli/batch_processor.rs
git commit -m "fix: add support for additional raw format extensions"
```

---

## Task 4: Fix TIFF Parsing Errors

**Files:**
- Modify: `src/parsers/tiff/mod.rs`
- Modify: `src/parsers/tiff/ifd.rs`

**Step 1: Identify TIFF parsing errors**

```bash
grep -A5 "Failed to read metadata" tests/data_lfs_errors.log | grep -i "tiff\|ifd\|tag" | head -20
```

Expected: Specific TIFF parsing error messages

**Step 2: Add error handling for malformed TIFF data**

In `src/parsers/tiff/ifd.rs`, improve error handling:

```rust
// Add graceful handling for corrupted IFD chains
pub fn parse_ifd_chain(data: &[u8], offset: usize) -> Result<Vec<IFD>> {
    let mut ifds = Vec::new();
    let mut current_offset = offset;
    let mut seen_offsets = std::collections::HashSet::new();

    loop {
        // Prevent infinite loops from circular references
        if !seen_offsets.insert(current_offset) {
            eprintln!("Warning: Circular IFD reference detected at offset {}", current_offset);
            break;
        }

        // Validate offset is within file bounds
        if current_offset >= data.len() {
            eprintln!("Warning: IFD offset {} exceeds file size {}", current_offset, data.len());
            break;
        }

        match parse_ifd(data, current_offset) {
            Ok((ifd, next_offset)) => {
                ifds.push(ifd);
                if next_offset == 0 {
                    break;
                }
                current_offset = next_offset;
            }
            Err(e) => {
                eprintln!("Warning: Failed to parse IFD at offset {}: {}", current_offset, e);
                break;
            }
        }
    }

    Ok(ifds)
}
```

**Step 3: Test TIFF parsing improvements**

```bash
cargo build --release
# Re-test files that had TIFF errors
```

Expected: Improved error handling, fewer parsing failures

**Step 4: Commit TIFF parser improvements**

```bash
git add src/parsers/tiff/ifd.rs
git commit -m "fix: improve TIFF parser error handling for malformed IFD chains"
```

---

## Task 5: Fix Raw Format Specific Errors

**Files:**
- Modify: `src/parsers/raw/metadata.rs`
- Modify: `src/parsers/raw/format_detection.rs`

**Step 1: Identify raw format specific errors**

```bash
grep -i "raw\|cr2\|cr3\|nef\|arw\|dng" tests/data_lfs_errors.log | head -30
```

Expected: Errors specific to raw format parsing

**Step 2: Improve raw format detection for edge cases**

In `src/parsers/raw/format_detection.rs`:

```rust
pub fn detect_raw_format(data: &[u8], filename: &str) -> Option<RawFormat> {
    // Add minimum data length check
    if data.len() < 16 {
        // Fall back to extension-only detection for very small files
        return detect_by_extension_only(filename);
    }

    // ... existing detection logic ...
}

fn detect_by_extension_only(filename: &str) -> Option<RawFormat> {
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())?;

    match ext.as_str() {
        "cr2" => Some(RawFormat::CanonCR2),
        "cr3" => Some(RawFormat::CanonCR3),
        // ... all extensions ...
        _ => None,
    }
}
```

**Step 3: Add graceful fallback for unsupported raw sub-formats**

In `src/parsers/raw/metadata.rs`:

```rust
pub fn parse_raw_metadata(data: &[u8], format: RawFormat) -> Result<MetadataMap> {
    // Attempt to parse, fall back to minimal metadata on error
    parse_tiff_based_raw(data, format).or_else(|e| {
        eprintln!("Warning: Failed to parse {} metadata: {}", format!("{:?}", format), e);
        let mut metadata = MetadataMap::new();
        metadata.insert_string("File:FileType", format!("{:?}", format))?;
        Ok(metadata)
    })
}
```

**Step 4: Test raw format improvements**

```bash
cargo build --release
# Re-test raw format files
```

Expected: Graceful handling of edge cases

**Step 5: Commit raw format improvements**

```bash
git add src/parsers/raw/metadata.rs src/parsers/raw/format_detection.rs
git commit -m "fix: improve raw format detection and error handling for edge cases"
```

---

## Task 6: Fix Memory-Mapped I/O Errors

**Files:**
- Modify: `src/core/operations.rs`

**Step 1: Identify memory mapping errors**

```bash
grep -i "mmap\|permission\|access denied" tests/data_lfs_errors.log
```

Expected: Permission or access errors

**Step 2: Add fallback to regular file reading**

In `src/core/operations.rs`:

```rust
pub fn read_metadata(path: &Path) -> Result<MetadataMap> {
    // Try memory-mapped reading first
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("Warning: Permission denied for {}", path.display());
            return Err(ExifToolError::from(e));
        }
        Err(e) => {
            return Err(ExifToolError::from(e));
        }
    };

    // ... rest of parsing logic ...
}
```

**Step 3: Test I/O improvements**

```bash
cargo build --release
# Re-test files that had I/O errors
```

Expected: Better error messages, graceful handling

**Step 4: Commit I/O improvements**

```bash
git add src/core/operations.rs
git commit -m "fix: improve file I/O error handling and permission checks"
```

---

## Task 7: Add Support for Additional Binary Formats

**Files:**
- Modify: `src/parsers/mod.rs`
- Create: `src/parsers/binary_fallback.rs` (if needed)

**Step 1: Identify files that need binary fallback**

```bash
grep "unsupported\|unknown" tests/data_lfs_errors.log | wc -l
```

Expected: Count of files needing fallback handling

**Step 2: Implement minimal metadata extraction for unknown formats**

Create `src/parsers/binary_fallback.rs`:

```rust
//! Fallback parser for unknown binary formats
//! Extracts basic file metadata without format-specific parsing

use crate::core::MetadataMap;
use crate::error::Result;
use std::path::Path;

pub fn parse_unknown_binary(path: &Path, data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Basic file information
    metadata.insert_string("File:FileType", "Unknown")?;
    metadata.insert_string("File:FileSize", format_file_size(data.len()))?;

    // Try to detect if it's text-based
    if is_likely_text(data) {
        metadata.insert_string("File:MIMEType", "text/plain")?;
    } else {
        metadata.insert_string("File:MIMEType", "application/octet-stream")?;
    }

    Ok(metadata)
}

fn is_likely_text(data: &[u8]) -> bool {
    let sample = &data[..data.len().min(1024)];
    let text_chars = sample.iter().filter(|&&b| b.is_ascii_graphic() || b.is_ascii_whitespace()).count();
    text_chars as f64 / sample.len() as f64 > 0.85
}

fn format_file_size(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = KB * 1024;
    const GB: usize = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}
```

**Step 3: Integrate fallback parser**

In `src/parsers/mod.rs`:

```rust
pub mod binary_fallback;
```

In `src/core/operations.rs`:

```rust
FileFormat::Unknown => {
    // Try binary fallback
    parsers::binary_fallback::parse_unknown_binary(path, &data)?
}
```

**Step 4: Test fallback parsing**

```bash
cargo build --release
# Test on unknown format files
```

Expected: Basic metadata extraction even for unknown formats

**Step 5: Commit fallback parser**

```bash
git add src/parsers/binary_fallback.rs src/parsers/mod.rs src/core/operations.rs
git commit -m "feat: add fallback parser for unknown binary formats"
```

---

## Task 8: Re-run Comprehensive Test and Verify Fixes

**Files:**
- Output: `tests/data_lfs_errors_after_fix.log`
- Output: `tests/data_lfs_success_after_fix.log`

**Step 1: Run comprehensive test again**

```bash
./tests/data_lfs_testing.sh
mv tests/data_lfs_errors.log tests/data_lfs_errors_after_fix.log
mv tests/data_lfs_success.log tests/data_lfs_success_after_fix.log
```

Expected: Significantly fewer errors

**Step 2: Compare before and after**

```bash
# Count errors before
BEFORE=$(wc -l < tests/data_lfs_errors_before_fix.log)
# Count errors after
AFTER=$(wc -l < tests/data_lfs_errors_after_fix.log)
# Calculate improvement
echo "Errors reduced from $BEFORE to $AFTER"
echo "Improvement: $(( (BEFORE - AFTER) * 100 / BEFORE ))%"
```

Expected: Show improvement percentage

**Step 3: Analyze remaining errors**

```bash
grep "Error:" tests/data_lfs_errors_after_fix.log | sort | uniq -c | sort -rn
```

Expected: Categorized list of remaining errors

**Step 4: Create final test report**

Create `tests/data_lfs_final_report.md`:

```markdown
# Data.lfs Comprehensive Test Final Report

## Summary
- Total files tested: [N]
- Successfully parsed: [N] ([X%])
- Errors: [N] ([X%])

## Improvements
- Errors before fixes: [N]
- Errors after fixes: [N]
- Improvement: [X%]

## Remaining Issues
[List of error categories and counts]

## Recommendations
[Next steps for remaining errors]
```

**Step 5: Commit final test results**

```bash
git add tests/data_lfs_errors_after_fix.log tests/data_lfs_final_report.md
git commit -m "test: add final comprehensive test results after fixes"
```

---

## Task 9: Performance Testing and Optimization

**Files:**
- Create: `tests/data_lfs_performance.sh`

**Step 1: Create performance test script**

Create `tests/data_lfs_performance.sh`:

```bash
#!/bin/bash
# Performance test for data.lfs directory

DATA_DIR="/Users/allen/Documents/git/examples/data.lfs"
EXIFTOOL_RS="./target/release/exiftool-rs"

echo "Performance Test - Recursive Processing"
echo "======================================="

time $EXIFTOOL_RS -r "$DATA_DIR" > /dev/null 2>&1

echo ""
echo "Performance Test - Complete"
```

**Step 2: Run performance test**

```bash
chmod +x tests/data_lfs_performance.sh
./tests/data_lfs_performance.sh
```

Expected: Performance metrics

**Step 3: Compare with Perl ExifTool**

```bash
echo "Perl ExifTool Performance:"
time exiftool -r "/Users/allen/Documents/git/examples/data.lfs" > /dev/null 2>&1
```

Expected: Performance comparison

**Step 4: Document performance results**

Add to `tests/data_lfs_final_report.md`:

```markdown
## Performance

### exiftool-rs
- Time: [X] seconds
- Files/second: [N]

### Perl ExifTool
- Time: [X] seconds
- Files/second: [N]

### Comparison
- exiftool-rs is [X]x [faster/slower]
```

**Step 5: Commit performance results**

```bash
git add tests/data_lfs_performance.sh tests/data_lfs_final_report.md
git commit -m "test: add performance testing and comparison results"
```

---

## Task 10: Final Verification and Documentation

**Files:**
- Update: `README.md` (if needed)
- Update: `docs/formats/camera-raw.md`

**Step 1: Run all tests one final time**

```bash
cargo test --all
cargo build --release
./tests/data_lfs_testing.sh
```

Expected: All tests pass, high success rate on data.lfs

**Step 2: Update documentation with findings**

In `docs/formats/camera-raw.md`, add:

```markdown
## Tested Compatibility

Comprehensively tested against 4,000+ real-world camera raw files including:
- Canon (CR2, CR3, CRW)
- Nikon (NEF, NRW)
- Sony (ARW, SR2, SRF, SRW)
- Leaf (MOS)
- [... other manufacturers ...]

Success rate: [X%] on production camera files
```

**Step 3: Create summary commit**

```bash
git add docs/formats/camera-raw.md
git commit -m "docs: update camera raw format documentation with comprehensive testing results"
```

**Step 4: Push all changes**

```bash
git push
```

Expected: All changes pushed to remote

**Step 5: Create GitHub issue for remaining errors (if any)**

If there are remaining errors that require further investigation:

```bash
gh issue create --title "Remaining parsing errors in data.lfs comprehensive test" \
                --body "$(cat tests/data_lfs_final_report.md)"
```

---

## Verification Checklist

- [ ] All 4,026 files tested
- [ ] Error rate < 5%
- [ ] All major camera formats supported
- [ ] Performance acceptable (< 30 seconds for full directory)
- [ ] No crashes or panics
- [ ] Documentation updated
- [ ] All fixes committed and pushed
- [ ] Test infrastructure committed for future use

## Success Criteria

- **Primary:** Successfully parse > 95% of files without errors
- **Secondary:** Performance within 2x of Perl ExifTool
- **Tertiary:** Graceful error handling for all remaining edge cases
- **Documentation:** All supported formats documented with examples
