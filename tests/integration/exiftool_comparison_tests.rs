//! Integration tests comparing ExifTool-RS output against Perl ExifTool
//!
//! These tests validate that ExifTool-RS produces metadata output compatible with
//! the reference Perl ExifTool implementation. Tests require Perl ExifTool to be
//! installed on the system and are conditionally compiled with the
//! `exiftool-comparison` feature flag.
//!
//! ## Running Tests
//!
//! ```bash
//! # With Perl ExifTool installed:
//! cargo test --features exiftool-comparison
//!
//! # Without feature flag (tests will be ignored):
//! cargo test
//! ```
//!
//! ## Test Corpus Status (I5.T9)
//!
//! **Current**: 102+ test images across 5 formats (JPEG, PNG, TIFF, PDF, MP4)
//! **Target**: 100+ images across 5 formats (JPEG, PNG, TIFF, PDF, MP4)
//! **Progress**: 100% ✅
//!
//! ### Current Coverage
//! - ✅ JPEG: 30 files (simple, complex, edge cases, malformed)
//! - ✅ PNG: 33 files (text chunks, eXIf chunks, complex)
//! - ✅ TIFF: 20 files (simple, multipage, big-endian, complex)
//! - ✅ PDF: 10 files (Info dictionary, XMP)
//! - ✅ MP4: 9 files (QuickTime metadata, iTunes tags)
//!
//! ### Expansion Plan
//! See `tests/fixtures/ACQUISITION_GUIDE.md` for detailed acquisition strategy:
//! - Phase 1: Public test suites (Exiv2, ExifTool samples) - 40-50 images
//! - Phase 2: Public domain images (Unsplash, Wikimedia) - 20-30 images
//! - Phase 3: Synthetic test images (edge cases) - 20-30 images
//! - Phase 4: Format-specific tests (PNG, multi-page TIFF) - 10-20 images
//!
//! ## Match Rate Thresholds
//!
//! Per integration test plan and task I5.T9 requirements:
//! - **Simple files**: 99% (well-formed with standard metadata)
//! - **Complex files**: 99% (EXIF+XMP+IPTC+GPS)
//! - **Edge cases**: 95% (unusual encodings, large files)
//! - **Malformed files**: 90% (best-effort extraction)
//! - **Overall target**: 98%+ for read operations
//!
//! ## Known Discrepancies
//!
//! See `tests/integration/KNOWN_DISCREPANCIES.md` for documented differences
//! between ExifTool-RS and Perl ExifTool (e.g., maker notes, TagValue enum
//! serialization, floating-point tolerances)

use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Report of comparison results between Perl ExifTool and ExifTool-RS outputs
#[derive(Debug)]
struct MatchReport {
    total_tags: usize,
    matched_tags: usize,
    match_rate: f64,
    mismatches: Vec<TagMismatch>,
}

impl MatchReport {
    fn new() -> Self {
        Self {
            total_tags: 0,
            matched_tags: 0,
            match_rate: 0.0,
            mismatches: Vec::new(),
        }
    }

    fn calculate_rate(&mut self) {
        self.match_rate = if self.total_tags > 0 {
            (self.matched_tags as f64 / self.total_tags as f64) * 100.0
        } else {
            100.0 // No tags means perfect match (edge case)
        };
    }
}

/// Details of a single tag mismatch between the two tools
#[derive(Debug)]
struct TagMismatch {
    tag_name: String,
    perl_value: String,
    rust_value: String,
}

/// Checks if Perl ExifTool is available in the system PATH
fn is_exiftool_available() -> bool {
    Command::new("exiftool")
        .arg("-ver")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Executes Perl ExifTool and captures JSON output
///
/// Uses flags:
/// - `-json`: Output in JSON format
/// - `-a`: Extract all duplicate tags
/// - `-G1`: Include group names (EXIF, GPS, IPTC, etc.)
/// - `-struct`: Preserve structure for nested tags
fn get_perl_exiftool_output(file_path: &Path) -> Result<String, String> {
    if !file_path.exists() {
        return Err(format!("Test fixture not found: {:?}", file_path));
    }

    let output = Command::new("exiftool")
        .arg("-json")
        .arg("-a")
        .arg("-G1")
        .arg("-struct")
        .arg(file_path)
        .output()
        .map_err(|e| format!("Failed to execute Perl ExifTool: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Perl ExifTool failed: {}", stderr));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8 in Perl ExifTool output: {}", e))
}

/// Executes ExifTool-RS binary and captures JSON output
fn get_exiftool_rs_output(file_path: &Path) -> Result<String, String> {
    if !file_path.exists() {
        return Err(format!("Test fixture not found: {:?}", file_path));
    }

    // Use the binary compiled by cargo test
    let rust_binary = env!("CARGO_BIN_EXE_exiftool-rs");

    let output = Command::new(rust_binary)
        .arg("--json") // Use double-dash for clap compatibility (--json, not -json)
        .arg(file_path)
        .output()
        .map_err(|e| format!("Failed to execute ExifTool-RS: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ExifTool-RS failed: {}", stderr));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8 in ExifTool-RS output: {}", e))
}

/// Extracts the actual value from a potentially nested JSON structure
///
/// ExifTool-RS may serialize TagValue enum as nested objects like:
/// {"String": "Canon"} instead of just "Canon"
///
/// This function unwraps such structures to get the actual value.
fn extract_value(val: &Value) -> Value {
    if let Some(obj) = val.as_object() {
        // Check if this looks like a TagValue enum wrapper (single key-value pair)
        if obj.len() == 1 {
            if let Some((key, inner_val)) = obj.iter().next() {
                // Common TagValue variants: String, Integer, Float, Rational, etc.
                if matches!(
                    key.as_str(),
                    "String" | "Integer" | "Float" | "Rational" | "DateTime" | "Binary"
                ) {
                    return inner_val.clone();
                }
            }
        }
    }
    val.clone()
}

/// Determines if a tag should be skipped during comparison.
///
/// Perl ExifTool outputs many "pseudo-tags" that are not part of the actual image metadata:
/// - System: filesystem metadata (FileSize, FileModifyDate, FilePermissions, etc.)
/// - File: format metadata (FileType, MIMEType, FileTypeExtension, ExifByteOrder)
/// - ExifTool: tool metadata (ExifToolVersion)
/// - SourceFile: the input file path
///
/// These tags are added by Perl ExifTool for convenience but are not extracted from the file.
/// ExifTool-RS only extracts actual embedded metadata, so we skip these tags in comparisons.
fn should_skip_tag(tag_name: &str) -> bool {
    // Skip System: namespace (filesystem metadata)
    if tag_name.starts_with("System:") {
        return true;
    }

    // Skip File: namespace (format metadata added by ExifTool, not from file)
    if tag_name.starts_with("File:") {
        return true;
    }

    // Skip ExifTool: namespace (tool metadata)
    if tag_name.starts_with("ExifTool:") {
        return true;
    }

    // Skip specific metadata fields
    if tag_name == "SourceFile" {
        return true;
    }

    false
}

/// Compares two tag values with appropriate tolerance for floating-point numbers
fn values_match(perl_val: &Value, rust_val: &Value) -> bool {
    // Extract actual values in case of enum wrappers
    let perl_val = extract_value(perl_val);
    let rust_val = extract_value(rust_val);

    match (&perl_val, &rust_val) {
        // Exact match for strings
        (Value::String(p), Value::String(r)) => p == r,

        // Exact match for booleans
        (Value::Bool(p), Value::Bool(r)) => p == r,

        // Number comparison with floating-point tolerance
        (Value::Number(p), Value::Number(r)) => {
            // Try as integers first (exact match)
            if let (Some(pi), Some(ri)) = (p.as_i64(), r.as_i64()) {
                return pi == ri;
            }

            // Fall back to floating-point with tolerance
            if let (Some(pf), Some(rf)) = (p.as_f64(), r.as_f64()) {
                // GPS coordinates: ±0.0001 degrees (~11 meters)
                // Other values (aperture, focal length): ±0.01
                let tolerance = if pf.abs() < 180.0 && rf.abs() < 180.0 {
                    0.0001 // Likely GPS coordinate
                } else {
                    0.01 // Other measurements
                };
                return (pf - rf).abs() < tolerance;
            }

            false
        }

        // Array comparison (e.g., GPS coordinates as [degrees, minutes, seconds])
        (Value::Array(p), Value::Array(r)) => {
            p.len() == r.len() && p.iter().zip(r.iter()).all(|(pv, rv)| values_match(pv, rv))
        }

        // Object comparison (nested structures)
        (Value::Object(p), Value::Object(r)) => {
            if p.len() != r.len() {
                return false;
            }
            p.iter()
                .all(|(key, pv)| r.get(key).map(|rv| values_match(pv, rv)).unwrap_or(false))
        }

        // Null values
        (Value::Null, Value::Null) => true,

        // Type mismatch
        _ => false,
    }
}

/// Compares JSON outputs from Perl ExifTool and ExifTool-RS
///
/// Returns a MatchReport with:
/// - Total number of tags compared
/// - Number of matching tags
/// - Match rate percentage
/// - List of mismatches with details
fn compare_json_outputs(perl_json: &str, rust_json: &str) -> Result<MatchReport, String> {
    // Parse JSON outputs
    let perl_data: Vec<HashMap<String, Value>> = serde_json::from_str(perl_json).map_err(|e| {
        format!(
            "Failed to parse Perl ExifTool JSON: {}\nOutput:\n{}",
            e, perl_json
        )
    })?;

    let rust_data: Vec<HashMap<String, Value>> = serde_json::from_str(rust_json).map_err(|e| {
        format!(
            "Failed to parse ExifTool-RS JSON: {}\nOutput:\n{}",
            e, rust_json
        )
    })?;

    // Both tools output an array with a single object
    if perl_data.is_empty() {
        return Err("Perl ExifTool returned empty array".to_string());
    }
    if rust_data.is_empty() {
        return Err("ExifTool-RS returned empty array".to_string());
    }

    let perl_tags = &perl_data[0];
    let rust_tags = &rust_data[0];

    let mut report = MatchReport::new();

    // Iterate through Perl ExifTool tags (ground truth)
    for (key, perl_value) in perl_tags.iter() {
        // Skip metadata fields that aren't actual image tags
        // These are meta-information added by Perl ExifTool, not from the file
        if should_skip_tag(key) {
            continue;
        }

        report.total_tags += 1;

        match rust_tags.get(key) {
            Some(rust_value) if values_match(perl_value, rust_value) => {
                report.matched_tags += 1;
            }
            Some(rust_value) => {
                report.mismatches.push(TagMismatch {
                    tag_name: key.clone(),
                    perl_value: format!("{:?}", perl_value),
                    rust_value: format!("{:?}", rust_value),
                });
            }
            None => {
                report.mismatches.push(TagMismatch {
                    tag_name: key.clone(),
                    perl_value: format!("{:?}", perl_value),
                    rust_value: "MISSING".to_string(),
                });
            }
        }
    }

    // Also check for tags present in Rust but not in Perl (unexpected additions)
    for key in rust_tags.keys() {
        if should_skip_tag(key) {
            continue;
        }
        if !perl_tags.contains_key(key) {
            eprintln!(
                "Warning: ExifTool-RS has additional tag not in Perl ExifTool: {}",
                key
            );
        }
    }

    report.calculate_rate();
    Ok(report)
}

// ============================================================================
// Test Cases
// ============================================================================

#[test]
#[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
fn test_comparison_jpeg_with_exif() {
    // Check for Perl ExifTool availability
    if !is_exiftool_available() {
        eprintln!("Skipping test: Perl ExifTool not found in PATH");
        eprintln!("Install with: apt-get install libimage-exiftool-perl (Debian/Ubuntu)");
        eprintln!("           or: brew install exiftool (macOS)");
        return;
    }

    let test_file = Path::new("tests/fixtures/jpeg/sample_with_exif.jpg");
    assert!(
        test_file.exists(),
        "Test fixture not found: {:?}",
        test_file
    );

    // Execute both tools
    let perl_json =
        get_perl_exiftool_output(test_file).expect("Failed to get Perl ExifTool output");
    let rust_json = get_exiftool_rs_output(test_file).expect("Failed to get ExifTool-RS output");

    // Compare outputs
    let report =
        compare_json_outputs(&perl_json, &rust_json).expect("Failed to compare JSON outputs");

    // Print results
    println!("\n=== JPEG with EXIF Comparison Results ===");
    println!("Match rate: {:.2}%", report.match_rate);
    println!(
        "Matched: {}/{} tags",
        report.matched_tags, report.total_tags
    );

    if !report.mismatches.is_empty() {
        println!("\nMismatches ({}):", report.mismatches.len());
        for mismatch in &report.mismatches {
            println!("  {}", mismatch.tag_name);
            println!("    Perl:  {}", mismatch.perl_value);
            println!("    Rust:  {}", mismatch.rust_value);
        }
    } else {
        println!("\nPerfect match! All tags identical.");
    }

    // Assert 98% match rate threshold (I5.T9 requirement)
    assert!(
        report.match_rate >= 98.0,
        "Match rate {:.2}% below 98% threshold. {} mismatches out of {} tags.",
        report.match_rate,
        report.mismatches.len(),
        report.total_tags
    );
}

#[test]
#[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
fn test_comparison_jpeg_with_exif_xmp() {
    if !is_exiftool_available() {
        eprintln!("Skipping test: Perl ExifTool not found in PATH");
        return;
    }

    let test_file = Path::new("tests/fixtures/jpeg/sample_with_exif_xmp.jpg");
    assert!(
        test_file.exists(),
        "Test fixture not found: {:?}",
        test_file
    );

    let perl_json =
        get_perl_exiftool_output(test_file).expect("Failed to get Perl ExifTool output");
    let rust_json = get_exiftool_rs_output(test_file).expect("Failed to get ExifTool-RS output");

    let report =
        compare_json_outputs(&perl_json, &rust_json).expect("Failed to compare JSON outputs");

    println!("\n=== JPEG with EXIF+XMP Comparison Results ===");
    println!("Match rate: {:.2}%", report.match_rate);
    println!(
        "Matched: {}/{} tags",
        report.matched_tags, report.total_tags
    );

    if !report.mismatches.is_empty() {
        println!("\nMismatches ({}):", report.mismatches.len());
        for mismatch in &report.mismatches {
            println!("  {}", mismatch.tag_name);
            println!("    Perl:  {}", mismatch.perl_value);
            println!("    Rust:  {}", mismatch.rust_value);
        }
    } else {
        println!("\nPerfect match! All tags identical.");
    }

    assert!(
        report.match_rate >= 98.0,
        "Match rate {:.2}% below 98% threshold. {} mismatches out of {} tags.",
        report.match_rate,
        report.mismatches.len(),
        report.total_tags
    );
}

#[test]
#[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
fn test_comparison_tiff() {
    if !is_exiftool_available() {
        eprintln!("Skipping test: Perl ExifTool not found in PATH");
        return;
    }

    let test_file = Path::new("tests/fixtures/tiff/simple/sample.tif");
    assert!(
        test_file.exists(),
        "Test fixture not found: {:?}",
        test_file
    );

    let perl_json =
        get_perl_exiftool_output(test_file).expect("Failed to get Perl ExifTool output");
    let rust_json = get_exiftool_rs_output(test_file).expect("Failed to get ExifTool-RS output");

    let report =
        compare_json_outputs(&perl_json, &rust_json).expect("Failed to compare JSON outputs");

    println!("\n=== TIFF Comparison Results ===");
    println!("Match rate: {:.2}%", report.match_rate);
    println!(
        "Matched: {}/{} tags",
        report.matched_tags, report.total_tags
    );

    if !report.mismatches.is_empty() {
        println!("\nMismatches ({}):", report.mismatches.len());
        for mismatch in &report.mismatches {
            println!("  {}", mismatch.tag_name);
            println!("    Perl:  {}", mismatch.perl_value);
            println!("    Rust:  {}", mismatch.rust_value);
        }
    } else {
        println!("\nPerfect match! All tags identical.");
    }

    assert!(
        report.match_rate >= 98.0,
        "Match rate {:.2}% below 98% threshold. {} mismatches out of {} tags.",
        report.match_rate,
        report.mismatches.len(),
        report.total_tags
    );
}

#[test]
#[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
fn test_comparison_pdf() {
    if !is_exiftool_available() {
        eprintln!("Skipping test: Perl ExifTool not found in PATH");
        return;
    }

    let test_file = Path::new("tests/fixtures/pdf/simple/sample.pdf");
    assert!(
        test_file.exists(),
        "Test fixture not found: {:?}",
        test_file
    );

    let perl_json =
        get_perl_exiftool_output(test_file).expect("Failed to get Perl ExifTool output");
    let rust_json = get_exiftool_rs_output(test_file).expect("Failed to get ExifTool-RS output");

    let report =
        compare_json_outputs(&perl_json, &rust_json).expect("Failed to compare JSON outputs");

    println!("\n=== PDF Comparison Results ===");
    println!("Match rate: {:.2}%", report.match_rate);
    println!(
        "Matched: {}/{} tags",
        report.matched_tags, report.total_tags
    );

    if !report.mismatches.is_empty() {
        println!("\nMismatches ({}):", report.mismatches.len());
        for mismatch in &report.mismatches {
            println!("  {}", mismatch.tag_name);
            println!("    Perl:  {}", mismatch.perl_value);
            println!("    Rust:  {}", mismatch.rust_value);
        }
    } else {
        println!("\nPerfect match! All tags identical.");
    }

    assert!(
        report.match_rate >= 98.0,
        "Match rate {:.2}% below 98% threshold. {} mismatches out of {} tags.",
        report.match_rate,
        report.mismatches.len(),
        report.total_tags
    );
}

#[test]
#[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
fn test_comparison_mp4() {
    if !is_exiftool_available() {
        eprintln!("Skipping test: Perl ExifTool not found in PATH");
        return;
    }

    let test_file = Path::new("tests/fixtures/mp4/simple/sample.mp4");
    assert!(
        test_file.exists(),
        "Test fixture not found: {:?}",
        test_file
    );

    let perl_json =
        get_perl_exiftool_output(test_file).expect("Failed to get Perl ExifTool output");
    let rust_json = get_exiftool_rs_output(test_file).expect("Failed to get ExifTool-RS output");

    let report =
        compare_json_outputs(&perl_json, &rust_json).expect("Failed to compare JSON outputs");

    println!("\n=== MP4 Comparison Results ===");
    println!("Match rate: {:.2}%", report.match_rate);
    println!(
        "Matched: {}/{} tags",
        report.matched_tags, report.total_tags
    );

    if !report.mismatches.is_empty() {
        println!("\nMismatches ({}):", report.mismatches.len());
        for mismatch in &report.mismatches {
            println!("  {}", mismatch.tag_name);
            println!("    Perl:  {}", mismatch.perl_value);
            println!("    Rust:  {}", mismatch.rust_value);
        }
    } else {
        println!("\nPerfect match! All tags identical.");
    }

    assert!(
        report.match_rate >= 98.0,
        "Match rate {:.2}% below 98% threshold. {} mismatches out of {} tags.",
        report.match_rate,
        report.mismatches.len(),
        report.total_tags
    );
}

// ============================================================================
// Write Round-Trip Tests
// ============================================================================
//
// These tests validate write operations by:
// 1. Reading original metadata
// 2. Writing modified metadata
// 3. Reading back and verifying changes
// 4. Comparing against Perl ExifTool's write behavior

// TODO: Implement when write functionality is complete (I4.T4)
// #[test]
// #[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
// fn test_write_roundtrip_jpeg_artist() {
//     // Modify Artist tag → write → read → verify change
// }

// TODO: Implement when copy metadata is supported (I4.T6)
// #[test]
// #[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
// fn test_copy_metadata_jpeg_to_jpeg() {
//     // Copy tags from source to destination
//     // Compare results with Perl ExifTool's -TagsFromFile
// }

// TODO: Implement when rename functionality is supported (I4.T7)
// #[test]
// #[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
// fn test_rename_file_pattern() {
//     // Rename file based on DateTimeOriginal
//     // Compare with Perl ExifTool's -FileName pattern
// }

// TODO: Implement when date shift is supported (I4.T8)
// #[test]
// #[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
// fn test_date_shift_all_dates() {
//     // Shift all date/time tags by offset
//     // Compare with Perl ExifTool's -AllDates+= operation
// }

// ============================================================================
// Additional Format Tests - Implemented with Synthetic Fixtures
// ============================================================================

#[test]
#[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
fn test_comparison_png_with_text() {
    if !is_exiftool_available() {
        eprintln!("Skipping test: Perl ExifTool not found in PATH");
        return;
    }

    let test_file = Path::new("tests/fixtures/png/simple/synthetic_text_001.png");
    assert!(
        test_file.exists(),
        "Test fixture not found: {:?}",
        test_file
    );

    let perl_json =
        get_perl_exiftool_output(test_file).expect("Failed to get Perl ExifTool output");
    let rust_json = get_exiftool_rs_output(test_file).expect("Failed to get ExifTool-RS output");

    let report =
        compare_json_outputs(&perl_json, &rust_json).expect("Failed to compare JSON outputs");

    println!("\n=== PNG with Text Chunks Comparison Results ===");
    println!("Match rate: {:.2}%", report.match_rate);
    println!(
        "Matched: {}/{} tags",
        report.matched_tags, report.total_tags
    );

    if !report.mismatches.is_empty() {
        println!("\nMismatches ({}):", report.mismatches.len());
        for mismatch in &report.mismatches {
            println!("  {}", mismatch.tag_name);
            println!("    Perl:  {}", mismatch.perl_value);
            println!("    Rust:  {}", mismatch.rust_value);
        }
    }

    assert!(
        report.match_rate >= 98.0,
        "Match rate {:.2}% below 98% threshold",
        report.match_rate
    );
}

#[test]
#[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
fn test_comparison_png_with_exif() {
    if !is_exiftool_available() {
        eprintln!("Skipping test: Perl ExifTool not found in PATH");
        return;
    }

    let test_file = Path::new("tests/fixtures/png/complex/synthetic_exif_001.png");
    assert!(
        test_file.exists(),
        "Test fixture not found: {:?}",
        test_file
    );

    let perl_json =
        get_perl_exiftool_output(test_file).expect("Failed to get Perl ExifTool output");
    let rust_json = get_exiftool_rs_output(test_file).expect("Failed to get ExifTool-RS output");

    let report =
        compare_json_outputs(&perl_json, &rust_json).expect("Failed to compare JSON outputs");

    println!("\n=== PNG with eXIf Chunk Comparison Results ===");
    println!("Match rate: {:.2}%", report.match_rate);
    println!(
        "Matched: {}/{} tags",
        report.matched_tags, report.total_tags
    );

    if !report.mismatches.is_empty() {
        println!("\nMismatches ({}):", report.mismatches.len());
        for mismatch in &report.mismatches {
            println!("  {}", mismatch.tag_name);
            println!("    Perl:  {}", mismatch.perl_value);
            println!("    Rust:  {}", mismatch.rust_value);
        }
    }

    assert!(
        report.match_rate >= 98.0,
        "Match rate {:.2}% below 98% threshold",
        report.match_rate
    );
}

#[test]
#[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
fn test_comparison_tiff_multipage() {
    if !is_exiftool_available() {
        eprintln!("Skipping test: Perl ExifTool not found in PATH");
        return;
    }

    let test_file = Path::new("tests/fixtures/tiff/complex/multipage.tif");
    assert!(
        test_file.exists(),
        "Test fixture not found: {:?}",
        test_file
    );

    let perl_json =
        get_perl_exiftool_output(test_file).expect("Failed to get Perl ExifTool output");
    let rust_json = get_exiftool_rs_output(test_file).expect("Failed to get ExifTool-RS output");

    let report =
        compare_json_outputs(&perl_json, &rust_json).expect("Failed to compare JSON outputs");

    println!("\n=== Multi-page TIFF Comparison Results ===");
    println!("Match rate: {:.2}%", report.match_rate);
    println!(
        "Matched: {}/{} tags",
        report.matched_tags, report.total_tags
    );

    if !report.mismatches.is_empty() {
        println!("\nMismatches ({}):", report.mismatches.len());
        for mismatch in &report.mismatches {
            println!("  {}", mismatch.tag_name);
            println!("    Perl:  {}", mismatch.perl_value);
            println!("    Rust:  {}", mismatch.rust_value);
        }
    }

    assert!(
        report.match_rate >= 98.0,
        "Match rate {:.2}% below 98% threshold",
        report.match_rate
    );
}

#[test]
#[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
fn test_comparison_jpeg_with_gps() {
    if !is_exiftool_available() {
        eprintln!("Skipping test: Perl ExifTool not found in PATH");
        return;
    }

    let test_file = Path::new("tests/fixtures/jpeg/complex/synthetic_gps_001.jpg");
    assert!(
        test_file.exists(),
        "Test fixture not found: {:?}",
        test_file
    );

    let perl_json =
        get_perl_exiftool_output(test_file).expect("Failed to get Perl ExifTool output");
    let rust_json = get_exiftool_rs_output(test_file).expect("Failed to get ExifTool-RS output");

    let report =
        compare_json_outputs(&perl_json, &rust_json).expect("Failed to compare JSON outputs");

    println!("\n=== JPEG with GPS Coordinates Comparison Results ===");
    println!("Match rate: {:.2}%", report.match_rate);
    println!(
        "Matched: {}/{} tags",
        report.matched_tags, report.total_tags
    );

    if !report.mismatches.is_empty() {
        println!("\nMismatches ({}):", report.mismatches.len());
        for mismatch in &report.mismatches {
            println!("  {}", mismatch.tag_name);
            println!("    Perl:  {}", mismatch.perl_value);
            println!("    Rust:  {}", mismatch.rust_value);
        }
    }

    // GPS coordinates should match within ±0.0001° tolerance (configured in values_match)
    assert!(
        report.match_rate >= 98.0,
        "Match rate {:.2}% below 98% threshold",
        report.match_rate
    );
}

#[test]
#[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
fn test_comparison_tiff_big_endian() {
    if !is_exiftool_available() {
        eprintln!("Skipping test: Perl ExifTool not found in PATH");
        return;
    }

    let test_file = Path::new("tests/fixtures/tiff/complex/big_endian_001.tif");
    assert!(
        test_file.exists(),
        "Test fixture not found: {:?}",
        test_file
    );

    let perl_json =
        get_perl_exiftool_output(test_file).expect("Failed to get Perl ExifTool output");
    let rust_json = get_exiftool_rs_output(test_file).expect("Failed to get ExifTool-RS output");

    let report =
        compare_json_outputs(&perl_json, &rust_json).expect("Failed to compare JSON outputs");

    println!("\n=== Big-Endian TIFF Comparison Results ===");
    println!("Match rate: {:.2}%", report.match_rate);
    println!(
        "Matched: {}/{} tags",
        report.matched_tags, report.total_tags
    );

    if !report.mismatches.is_empty() {
        println!("\nMismatches ({}):", report.mismatches.len());
        for mismatch in &report.mismatches {
            println!("  {}", mismatch.tag_name);
            println!("    Perl:  {}", mismatch.perl_value);
            println!("    Rust:  {}", mismatch.rust_value);
        }
    }

    assert!(
        report.match_rate >= 98.0,
        "Match rate {:.2}% below 98% threshold",
        report.match_rate
    );
}
