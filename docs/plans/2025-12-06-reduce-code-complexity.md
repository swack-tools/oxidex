# Reduce Codacy Code Complexity Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reduce cyclomatic complexity in src/ by refactoring repetitive patterns: bit-flag decoding, manual date calculations, and deep nesting.

**Architecture:** Extract shared utilities (macros, helper functions) to eliminate repeated conditional logic. Replace manual algorithms with battle-tested libraries. Use early returns to flatten nesting.

**Tech Stack:** Rust, chrono (already in Cargo.toml)

---

## Task 1: Create Bit-Flag Decoding Macro

**Files:**
- Create: `src/core/flag_utils.rs`
- Modify: `src/core/mod.rs`
- Test: `src/core/flag_utils.rs` (inline tests)

**Step 1: Write the failing test**

Add to `src/core/flag_utils.rs`:

```rust
//! Utilities for decoding bit flags into human-readable strings

/// Decodes bit flags into a vector of string labels.
///
/// # Arguments
/// * `value` - The bit field value to decode
/// * `flags` - Slice of (mask, label) tuples
///
/// # Example
/// ```
/// use oxidex::core::decode_flags;
/// let flags = decode_flags(0x2003, &[
///     (0x0001, "Flag A"),
///     (0x0002, "Flag B"),
///     (0x2000, "Flag C"),
/// ]);
/// assert_eq!(flags, vec!["Flag A", "Flag B", "Flag C"]);
/// ```
pub fn decode_flags(value: u32, flags: &[(u32, &str)]) -> Vec<&str> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_flags_multiple_set() {
        let result = decode_flags(0x2003, &[
            (0x0001, "No relocs"),
            (0x0002, "Executable"),
            (0x2000, "DLL"),
        ]);
        assert_eq!(result, vec!["No relocs", "Executable", "DLL"]);
    }

    #[test]
    fn test_decode_flags_none_set() {
        let result = decode_flags(0x0000, &[
            (0x0001, "Flag A"),
            (0x0002, "Flag B"),
        ]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_decode_flags_partial_match() {
        let result = decode_flags(0x0004, &[
            (0x0001, "Flag A"),
            (0x0004, "Flag C"),
            (0x0008, "Flag D"),
        ]);
        assert_eq!(result, vec!["Flag C"]);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib flag_utils`

Expected: FAIL with "not yet implemented"

**Step 3: Write minimal implementation**

Replace `todo!()` with:

```rust
pub fn decode_flags(value: u32, flags: &[(u32, &str)]) -> Vec<&str> {
    flags
        .iter()
        .filter(|(mask, _)| (value & mask) != 0)
        .map(|(_, label)| *label)
        .collect()
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --lib flag_utils`

Expected: PASS (3 tests)

**Step 5: Export from core module**

Modify `src/core/mod.rs` - add near other pub mod declarations:

```rust
pub mod flag_utils;
pub use flag_utils::decode_flags;
```

**Step 6: Run full test suite**

Run: `cargo test --workspace`

Expected: PASS

**Step 7: Commit**

```bash
git add src/core/flag_utils.rs src/core/mod.rs
git commit -m "feat(core): add decode_flags utility for bit-flag decoding"
```

---

## Task 2: Refactor PE Metadata Extractor to Use decode_flags

**Files:**
- Modify: `src/parsers/pe/metadata_extractor.rs:75-116`
- Modify: `src/parsers/pe/metadata_extractor.rs:256-299`

**Step 1: Run existing PE tests as baseline**

Run: `cargo test --workspace pe`

Expected: PASS (capture baseline)

**Step 2: Refactor COFF characteristics (lines 75-116)**

Replace lines 75-116 in `src/parsers/pe/metadata_extractor.rs`:

```rust
    // Decode characteristic bit flags into human-readable strings
    // Reference: Microsoft PE/COFF specification IMAGE_FILE_HEADER.Characteristics
    let mut flags = Vec::new();

    if (header.characteristics & 0x0001) != 0 {
        flags.push("No relocs");
    }
    if (header.characteristics & 0x0002) != 0 {
        flags.push("Executable");
    }
    if (header.characteristics & 0x0004) != 0 {
        flags.push("No line numbers");
    }
    if (header.characteristics & 0x0008) != 0 {
        flags.push("No symbols");
    }
    if (header.characteristics & 0x0020) != 0 {
        flags.push("Large address aware");
    }
    if (header.characteristics & 0x0100) != 0 {
        flags.push("32-bit");
    }
    if (header.characteristics & 0x0200) != 0 {
        flags.push("Bytes reversed lo");
    }
    if (header.characteristics & 0x1000) != 0 {
        flags.push("System file");
    }
    if (header.characteristics & 0x2000) != 0 {
        flags.push("DLL");
    }
    if (header.characteristics & 0x4000) != 0 {
        flags.push("Bytes reversed hi");
    }

    // Insert decoded characteristics as comma-separated string
    if !flags.is_empty() {
        metadata.insert(
            "PE:ImageFileCharacteristics".to_string(),
            TagValue::String(flags.join(", ")),
        );
    }
```

With:

```rust
    // Decode characteristic bit flags into human-readable strings
    // Reference: Microsoft PE/COFF specification IMAGE_FILE_HEADER.Characteristics
    use crate::core::decode_flags;

    const COFF_CHARACTERISTICS: &[(u32, &str)] = &[
        (0x0001, "No relocs"),
        (0x0002, "Executable"),
        (0x0004, "No line numbers"),
        (0x0008, "No symbols"),
        (0x0020, "Large address aware"),
        (0x0100, "32-bit"),
        (0x0200, "Bytes reversed lo"),
        (0x1000, "System file"),
        (0x2000, "DLL"),
        (0x4000, "Bytes reversed hi"),
    ];

    let flags = decode_flags(header.characteristics as u32, COFF_CHARACTERISTICS);

    // Insert decoded characteristics as comma-separated string
    if !flags.is_empty() {
        metadata.insert(
            "PE:ImageFileCharacteristics".to_string(),
            TagValue::String(flags.join(", ")),
        );
    }
```

**Step 3: Run PE tests to verify no regression**

Run: `cargo test --workspace pe`

Expected: PASS (same as baseline)

**Step 4: Refactor DLL characteristics (lines 256-299)**

Replace lines 256-299:

```rust
    // Decode DLL characteristic bit flags
    let mut dll_flags = Vec::new();

    // Reference: Microsoft PE/COFF specification IMAGE_OPTIONAL_HEADER.DllCharacteristics
    if (nt_header.dll_characteristics & 0x0020) != 0 {
        dll_flags.push("High entropy VA");
    }
    if (nt_header.dll_characteristics & 0x0040) != 0 {
        dll_flags.push("Dynamic base");
    }
    if (nt_header.dll_characteristics & 0x0080) != 0 {
        dll_flags.push("Force integrity");
    }
    if (nt_header.dll_characteristics & 0x0100) != 0 {
        dll_flags.push("NX compatible");
    }
    if (nt_header.dll_characteristics & 0x0200) != 0 {
        dll_flags.push("No isolation");
    }
    if (nt_header.dll_characteristics & 0x0400) != 0 {
        dll_flags.push("No SEH");
    }
    if (nt_header.dll_characteristics & 0x0800) != 0 {
        dll_flags.push("No bind");
    }
    if (nt_header.dll_characteristics & 0x1000) != 0 {
        dll_flags.push("AppContainer");
    }
    if (nt_header.dll_characteristics & 0x2000) != 0 {
        dll_flags.push("WDM driver");
    }
    if (nt_header.dll_characteristics & 0x4000) != 0 {
        dll_flags.push("Control flow guard");
    }
    if (nt_header.dll_characteristics & 0x8000) != 0 {
        dll_flags.push("Terminal server aware");
    }

    if !dll_flags.is_empty() {
        metadata.insert(
            "PE:DllCharacteristicsDecoded".to_string(),
            TagValue::String(dll_flags.join(", ")),
        );
    }
```

With:

```rust
    // Decode DLL characteristic bit flags
    // Reference: Microsoft PE/COFF specification IMAGE_OPTIONAL_HEADER.DllCharacteristics
    const DLL_CHARACTERISTICS: &[(u32, &str)] = &[
        (0x0020, "High entropy VA"),
        (0x0040, "Dynamic base"),
        (0x0080, "Force integrity"),
        (0x0100, "NX compatible"),
        (0x0200, "No isolation"),
        (0x0400, "No SEH"),
        (0x0800, "No bind"),
        (0x1000, "AppContainer"),
        (0x2000, "WDM driver"),
        (0x4000, "Control flow guard"),
        (0x8000, "Terminal server aware"),
    ];

    let dll_flags = decode_flags(nt_header.dll_characteristics as u32, DLL_CHARACTERISTICS);

    if !dll_flags.is_empty() {
        metadata.insert(
            "PE:DllCharacteristicsDecoded".to_string(),
            TagValue::String(dll_flags.join(", ")),
        );
    }
```

**Step 5: Run full test suite**

Run: `cargo test --workspace`

Expected: PASS

**Step 6: Run clippy**

Run: `cargo clippy`

Expected: No new warnings

**Step 7: Commit**

```bash
git add src/parsers/pe/metadata_extractor.rs
git commit -m "refactor(pe): use decode_flags for bit-flag decoding

Reduces cyclomatic complexity by replacing 21 if-statements with
declarative flag tables. No functional changes."
```

---

## Task 3: Replace Manual Date Calculation in LNK Parser with chrono

**Files:**
- Modify: `src/parsers/specialized/lnk.rs:67-86` (remove helper functions)
- Modify: `src/parsers/specialized/lnk.rs:96-156` (replace filetime_to_iso8601)
- Test: `tests/forensic/lnk_tests.rs`

**Step 1: Run existing LNK tests as baseline**

Run: `cargo test --workspace lnk`

Expected: PASS (capture baseline)

**Step 2: Remove manual date helper functions (lines 67-86)**

Delete these functions from `src/parsers/specialized/lnk.rs`:

```rust
/// Helper function to check if a year is a leap year
fn is_leap_year(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

/// Helper function to get the number of days in a month
fn get_days_in_month(month: u32, year: u64) -> u64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}
```

**Step 3: Replace filetime_to_iso8601 (lines 96-156)**

Replace the entire `filetime_to_iso8601` function:

```rust
    /// Converts Windows FILETIME (64-bit value) to ISO 8601 string
    ///
    /// FILETIME represents the number of 100-nanosecond intervals since 1601-01-01 00:00:00 UTC.
    /// Returns None if the timestamp is zero (not set) or invalid.
    fn filetime_to_iso8601(filetime: u64) -> Option<String> {
        if filetime == 0 {
            return None;
        }

        // Convert to Unix timestamp (seconds since 1970-01-01)
        let filetime_i64 = filetime as i64;
        let unix_nanos = (filetime_i64 - FILETIME_EPOCH_DIFF) * 100;

        if unix_nanos < 0 {
            return None;
        }

        let unix_secs = unix_nanos / 1_000_000_000;
        let subsec_nanos = (unix_nanos % 1_000_000_000) as u32;

        // Calculate date components from Unix timestamp
        let days_since_epoch = unix_secs / 86400;
        let remaining_secs = unix_secs % 86400;
        let hours = remaining_secs / 3600;
        let minutes = (remaining_secs % 3600) / 60;
        let seconds = remaining_secs % 60;
        let millis = subsec_nanos / 1_000_000;

        // Convert days since epoch to year/month/day
        let mut year = 1970;
        let mut days_left = days_since_epoch;

        // Handle negative years (before 1970)
        if days_left < 0 {
            return None;
        }

        loop {
            let days_in_year = if is_leap_year(year) { 366 } else { 365 };
            if days_left >= days_in_year {
                days_left -= days_in_year;
                year += 1;
            } else {
                break;
            }
        }

        let mut month = 1;
        for m in 1..=12 {
            let days_in_month = get_days_in_month(m, year) as i64;
            if days_left >= days_in_month {
                days_left -= days_in_month;
            } else {
                month = m;
                break;
            }
        }

        let day = days_left + 1;

        Some(format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            year, month, day, hours, minutes, seconds, millis
        ))
    }
```

With:

```rust
    /// Converts Windows FILETIME (64-bit value) to ISO 8601 string
    ///
    /// FILETIME represents the number of 100-nanosecond intervals since 1601-01-01 00:00:00 UTC.
    /// Returns None if the timestamp is zero (not set) or invalid.
    fn filetime_to_iso8601(filetime: u64) -> Option<String> {
        use chrono::Utc;

        if filetime == 0 {
            return None;
        }

        // FILETIME epoch is 1601-01-01, convert to Unix epoch (1970-01-01)
        let filetime_i64 = filetime as i64;
        let unix_nanos = (filetime_i64 - FILETIME_EPOCH_DIFF).checked_mul(100)?;

        if unix_nanos < 0 {
            return None;
        }

        let unix_secs = unix_nanos / 1_000_000_000;
        let subsec_nanos = (unix_nanos % 1_000_000_000) as u32;

        // Use timestamp_opt which returns LocalResult (can call .single() for strict validation)
        Utc.timestamp_opt(unix_secs, subsec_nanos)
            .single()
            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
    }
```

**Step 4: Run LNK tests to verify no regression**

Run: `cargo test --workspace lnk`

Expected: PASS (same as baseline)

**Step 5: Run full test suite**

Run: `cargo test --workspace`

Expected: PASS

**Step 6: Run clippy**

Run: `cargo clippy`

Expected: No new warnings

**Step 7: Commit**

```bash
git add src/parsers/specialized/lnk.rs
git commit -m "refactor(lnk): use chrono for FILETIME conversion

Replaces 60 lines of manual date calculation with chrono library.
Reduces cyclomatic complexity and eliminates custom leap year logic.
No functional changes - same output format maintained."
```

---

## Task 4: Refactor LNK Parser Flag Decoding

**Files:**
- Modify: `src/parsers/specialized/lnk.rs` (link flags and file attributes)

**Step 1: Find flag decoding in LNK parser**

Search for bit-flag if-chains in `src/parsers/specialized/lnk.rs` similar to PE parser.

**Step 2: Run existing LNK tests as baseline**

Run: `cargo test --workspace lnk`

Expected: PASS

**Step 3: Apply decode_flags to link flags**

Find any if-chains like:
```rust
if (flags & FLAG_HAS_LINK_TARGET_ID_LIST) != 0 { ... }
if (flags & FLAG_HAS_LINK_INFO) != 0 { ... }
```

Replace with decode_flags pattern if outputting to metadata as strings.

**Step 4: Run tests**

Run: `cargo test --workspace lnk`

Expected: PASS

**Step 5: Run clippy**

Run: `cargo clippy`

Expected: No new warnings

**Step 6: Commit**

```bash
git add src/parsers/specialized/lnk.rs
git commit -m "refactor(lnk): use decode_flags for link flag decoding"
```

---

## Task 5: Flatten QuickTime Atom Traversal with Early Returns

**Files:**
- Modify: `src/parsers/quicktime/metadata_extractor.rs:62-96`

**Step 1: Run existing QuickTime tests as baseline**

Run: `cargo test --workspace quicktime`

Expected: PASS

**Step 2: Extract track metadata extraction helper**

Add helper function before `extract_metadata`:

```rust
/// Extract metadata from a single track atom
///
/// Returns Err if required container atoms (mdia, minf, stbl) are missing.
/// Callers should ignore errors to preserve original behavior of skipping
/// incomplete tracks rather than failing the entire extraction.
fn extract_track_metadata(
    trak: &Atom,
    metadata: &mut MetadataMap,
    index: usize,
) -> Result<(), String> {
    // Extract track header - optional
    if let Some(tkhd) = trak.find_child("tkhd") {
        let _ = extract_track_header(&tkhd, metadata, index);
    }

    // Media container - required for further extraction
    // Uses ok_or_else() to convert Option to Result, enabling ? operator
    let mdia = trak
        .find_child("mdia")
        .ok_or_else(|| "missing mdia atom".to_string())?;

    // Extract media header - optional
    if let Some(mdhd) = mdia.find_child("mdhd") {
        let _ = extract_media_header(&mdhd, metadata, index);
    }

    // Media information - required for sample table access
    let minf = mdia
        .find_child("minf")
        .ok_or_else(|| "missing minf atom".to_string())?;

    // Sample table - required for sample descriptions
    let stbl = minf
        .find_child("stbl")
        .ok_or_else(|| "missing stbl atom".to_string())?;

    // Extract sample description - optional
    if let Some(stsd) = stbl.find_child("stsd") {
        let _ = extract_sample_description(&stsd, metadata, index);
    }

    Ok(())
}
```

**Step 3: Simplify main extract_metadata function**

Replace lines 68-96:

```rust
        // Extract track headers (tkhd) from all trak atoms
        if let Ok(children) = moov.parse_children() {
            let trak_atoms: Vec<_> = children
                .iter()
                .filter(|a| a.atom_type.matches("trak"))
                .collect();

            for (index, trak) in trak_atoms.iter().enumerate() {
                if let Some(tkhd) = trak.find_child("tkhd") {
                    let _ = extract_track_header(&tkhd, &mut metadata, index);
                }

                // Extract media header (mdhd) from trak→mdia→mdhd
                if let Some(mdia) = trak.find_child("mdia") {
                    if let Some(mdhd) = mdia.find_child("mdhd") {
                        let _ = extract_media_header(&mdhd, &mut metadata, index);
                    }

                    // Extract sample description (stsd) from trak→mdia→minf→stbl→stsd
                    if let Some(minf) = mdia.find_child("minf") {
                        if let Some(stbl) = minf.find_child("stbl") {
                            if let Some(stsd) = stbl.find_child("stsd") {
                                let _ = extract_sample_description(&stsd, &mut metadata, index);
                            }
                        }
                    }
                }
            }
        }
```

With:

```rust
        // Extract track headers (tkhd) from all trak atoms
        if let Ok(children) = moov.parse_children() {
            let trak_atoms: Vec<_> = children
                .iter()
                .filter(|a| a.atom_type.matches("trak"))
                .collect();

            for (index, trak) in trak_atoms.iter().enumerate() {
                // Ignore errors - missing atoms in a track should not prevent
                // processing other tracks (preserves original behavior)
                let _ = extract_track_metadata(trak, &mut metadata, index);
            }
        }
```

**Step 4: Run QuickTime tests to verify no regression**

Run: `cargo test --workspace quicktime`

Expected: PASS

**Step 5: Run full test suite**

Run: `cargo test --workspace`

Expected: PASS

**Step 6: Run clippy**

Run: `cargo clippy`

Expected: No new warnings

**Step 7: Commit**

```bash
git add src/parsers/quicktime/metadata_extractor.rs
git commit -m "refactor(quicktime): extract track metadata helper

Reduces nesting depth from 5 levels to 2 by extracting track
metadata extraction into a dedicated helper function with early
returns. No functional changes."
```

---

## Task 6: Final Verification and Cleanup

**Step 1: Run full test suite**

Run: `cargo test --workspace`

Expected: PASS

**Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`

Expected: No warnings

**Step 3: Run formatter**

Run: `cargo fmt`

Expected: No changes (or format as needed)

**Step 4: Verify complexity reduction**

Run Codacy analysis or count if-statements:

```bash
grep -r "if (" src/parsers/pe/metadata_extractor.rs | wc -l
grep -r "if (" src/parsers/specialized/lnk.rs | wc -l
```

Expected: Fewer if-statements than before

**Step 5: Final commit (if any formatting changes)**

```bash
git add -A
git commit -m "chore: format code"
```

---

## Summary

| Task | Complexity Reduction | Lines Changed |
|------|---------------------|---------------|
| 1. decode_flags utility | Enables reuse | +30 new |
| 2. PE bit-flag refactor | -21 if-statements | ~60 → ~20 |
| 3. LNK chrono refactor | -18 if-statements, -2 functions | ~70 → ~15 |
| 4. LNK flag decoding | ~10 if-statements | varies |
| 5. QuickTime flattening | -3 nesting levels | ~30 → ~15 |

**Total estimated complexity reduction: ~40%**
