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
//! ## Test Corpus Status
//!
//! Currently testing with available fixtures:
//! - 2 JPEG files (with EXIF, with EXIF+XMP)
//! - 1 TIFF file
//!
//! TODO: Expand test corpus to meet 10+ diverse images requirement:
//! - PNG with text chunks
//! - PNG with eXIf chunk
//! - Additional TIFF variants (multi-page, big-endian)
//! - JPEG with GPS metadata
//! - Files with maker notes

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
        .arg("-json")
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
            p.iter().all(|(key, pv)| {
                r.get(key)
                    .map(|rv| values_match(pv, rv))
                    .unwrap_or(false)
            })
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
    let perl_data: Vec<HashMap<String, Value>> = serde_json::from_str(perl_json)
        .map_err(|e| format!("Failed to parse Perl ExifTool JSON: {}\nOutput:\n{}", e, perl_json))?;

    let rust_data: Vec<HashMap<String, Value>> = serde_json::from_str(rust_json)
        .map_err(|e| format!("Failed to parse ExifTool-RS JSON: {}\nOutput:\n{}", e, rust_json))?;

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
        // Skip metadata fields that aren't actual tags
        if key == "SourceFile" || key == "File:FileName" || key == "File:Directory" {
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
        if key == "SourceFile" || key == "File:FileName" || key == "File:Directory" {
            continue;
        }
        if !perl_tags.contains_key(key) {
            eprintln!("Warning: ExifTool-RS has additional tag not in Perl ExifTool: {}", key);
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
    let perl_json = get_perl_exiftool_output(test_file)
        .expect("Failed to get Perl ExifTool output");
    let rust_json = get_exiftool_rs_output(test_file)
        .expect("Failed to get ExifTool-RS output");

    // Compare outputs
    let report = compare_json_outputs(&perl_json, &rust_json)
        .expect("Failed to compare JSON outputs");

    // Print results
    println!("\n=== JPEG with EXIF Comparison Results ===");
    println!("Match rate: {:.2}%", report.match_rate);
    println!("Matched: {}/{} tags", report.matched_tags, report.total_tags);

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

    // Assert 95% match rate threshold
    assert!(
        report.match_rate >= 95.0,
        "Match rate {:.2}% below 95% threshold. {} mismatches out of {} tags.",
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

    let perl_json = get_perl_exiftool_output(test_file)
        .expect("Failed to get Perl ExifTool output");
    let rust_json = get_exiftool_rs_output(test_file)
        .expect("Failed to get ExifTool-RS output");

    let report = compare_json_outputs(&perl_json, &rust_json)
        .expect("Failed to compare JSON outputs");

    println!("\n=== JPEG with EXIF+XMP Comparison Results ===");
    println!("Match rate: {:.2}%", report.match_rate);
    println!("Matched: {}/{} tags", report.matched_tags, report.total_tags);

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
        report.match_rate >= 95.0,
        "Match rate {:.2}% below 95% threshold. {} mismatches out of {} tags.",
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

    let test_file = Path::new("tests/fixtures/tiff/sample.tif");
    assert!(
        test_file.exists(),
        "Test fixture not found: {:?}",
        test_file
    );

    let perl_json = get_perl_exiftool_output(test_file)
        .expect("Failed to get Perl ExifTool output");
    let rust_json = get_exiftool_rs_output(test_file)
        .expect("Failed to get ExifTool-RS output");

    let report = compare_json_outputs(&perl_json, &rust_json)
        .expect("Failed to compare JSON outputs");

    println!("\n=== TIFF Comparison Results ===");
    println!("Match rate: {:.2}%", report.match_rate);
    println!("Matched: {}/{} tags", report.matched_tags, report.total_tags);

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
        report.match_rate >= 95.0,
        "Match rate {:.2}% below 95% threshold. {} mismatches out of {} tags.",
        report.match_rate,
        report.mismatches.len(),
        report.total_tags
    );
}

// ============================================================================
// Additional Test Cases - To Be Implemented When Fixtures Available
// ============================================================================

// TODO: Implement when PNG fixtures with text chunks are available
// #[test]
// #[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
// fn test_comparison_png_with_text() { ... }

// TODO: Implement when PNG fixtures with eXIf chunk are available
// #[test]
// #[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
// fn test_comparison_png_with_exif() { ... }

// TODO: Implement when multi-page TIFF fixtures are available
// #[test]
// #[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
// fn test_comparison_tiff_multipage() { ... }

// TODO: Implement when JPEG with GPS metadata is available
// #[test]
// #[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
// fn test_comparison_jpeg_with_gps() { ... }

// TODO: Implement when files with maker notes are available
// #[test]
// #[cfg_attr(not(feature = "exiftool-comparison"), ignore)]
// fn test_comparison_jpeg_with_maker_notes() { ... }
