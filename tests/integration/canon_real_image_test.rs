//! Real-world Canon MakerNote extraction test
//!
//! This integration test validates Canon MakerNote extraction from actual Canon camera files.
//! The test gracefully skips if no sample file is available.
//!
//! # Setup Instructions
//!
//! To run this test with a real Canon image:
//!
//! 1. **Option A - Download from RAW Image Repository:**
//!    - Visit: https://raw.pixls.us/
//!    - Search for Canon camera models (e.g., "Canon EOS", "Canon PowerShot")
//!    - Download any Canon JPEG or CR2 (RAW) file
//!
//! 2. **Option B - Use Your Own Canon Image:**
//!    - Use any JPEG or CR2 file from a Canon camera
//!    - Even older Canon cameras should work (they use the same MakerNote structure)
//!
//! 3. **Place the file at:** `tests/fixtures/canon_sample.jpg`
//!    - If using CR2, rename it to `.jpg` for consistency (filename doesn't affect parsing)
//!
//! # Expected Behavior
//!
//! - **Without sample file:** Test prints skip message and returns early (not a failure)
//! - **With sample file:** Test extracts and prints all Canon: prefixed tags for manual verification
//!
//! # Example Canon Tags
//!
//! ## Phase 1 Tags (Simple Values)
//! - `Canon:CanonModelID` - e.g., "0x80000001"
//! - `Canon:FirmwareVersion` - e.g., "Firmware Version 1.0.0"
//! - `Canon:OwnerName` - e.g., "John Doe"
//! - `Canon:SerialNumber` - e.g., "012345678901"
//! - `Canon:ImageType` - e.g., "IMG:EOS R5"
//! - `Canon:FileNumber` - e.g., "1234567"
//!
//! ## Phase 2 Tags (Array Values - Camera Settings)
//! - `Canon:MacroMode` - e.g., "Normal" (Phase 2)
//! - `Canon:Quality` - e.g., "Fine" (Phase 2)
//! - `Canon:FlashMode` - e.g., "Off" (Phase 2)
//! - `Canon:DriveMode` - e.g., "Single" (Phase 2)
//! - `Canon:FocusMode` - e.g., "One-shot AF" (Phase 2)
//! - `Canon:MeteringMode` - e.g., "Evaluative" (Phase 2)
//! - `Canon:ExposureMode` - e.g., "Manual" (Phase 2)
//! - `Canon:ISO` - e.g., "100" (Phase 2)
//!
//! ## Phase 2 Tags (Shot Info)
//! - `Canon:AutoISO` - e.g., "100" (Phase 2)
//! - `Canon:BaseISO` - e.g., "100" (Phase 2)
//! - `Canon:MeasuredEV` - e.g., "128" (Phase 2)
//! - `Canon:TargetAperture` - e.g., "160" (Phase 2)
//! - `Canon:TargetShutterSpeed` - e.g., "96" (Phase 2)
//! - `Canon:SubjectDistance` - e.g., "1000 mm" (Phase 2)
//!
//! ## Phase 2 Tags (Focal Length)
//! - `Canon:FocalType` - e.g., "2" (Phase 2)
//! - `Canon:FocalLength` - e.g., "50 mm" (Phase 2)

use exiftool_rs::core::operations::read_metadata;
use std::path::Path;

/// Test Canon MakerNote extraction from a real Canon camera image.
///
/// This test validates that:
/// 1. Canon MakerNote data can be successfully parsed from real-world files
/// 2. Canon tags are properly extracted and prefixed with "Canon:"
/// 3. The extracted data can be displayed for manual verification
///
/// # Test Behavior
///
/// If `tests/fixtures/canon_sample.jpg` doesn't exist, the test prints helpful
/// instructions and returns early (skip). This ensures the test doesn't fail
/// in CI/CD environments where sample files may not be available.
#[test]
fn test_canon_real_image() {
    let sample_path = Path::new("tests/fixtures/canon_sample.jpg");

    // Gracefully skip if no sample file exists
    if !sample_path.exists() {
        eprintln!("\n=== Canon Real Image Test: SKIPPED ===");
        eprintln!("No Canon sample file found at: {:?}", sample_path);
        eprintln!("");
        eprintln!("To run this test, obtain a Canon image:");
        eprintln!("  1. Download from: https://raw.pixls.us/ (search for Canon)");
        eprintln!("  2. Or use your own Canon JPEG/CR2/CR3 file");
        eprintln!("  3. Place at: tests/fixtures/canon_sample.jpg");
        eprintln!("");
        eprintln!("This is not a test failure - the test is simply skipped.");
        eprintln!("=======================================\n");
        return;
    }

    // Read metadata from the Canon sample file
    let metadata =
        read_metadata(sample_path).expect("Failed to read metadata from Canon sample file");

    // Verify that at least some Canon tags were extracted
    let canon_tags: Vec<_> = metadata
        .keys()
        .filter(|k| k.starts_with("Canon:"))
        .collect();

    assert!(
        !canon_tags.is_empty(),
        "Expected Canon MakerNote tags to be present in the sample file"
    );

    // Print all extracted Canon tags for manual verification
    eprintln!(
        "\n=== Extracted Canon Tags ({} total) ===",
        canon_tags.len()
    );

    // Sort tags for consistent output
    let mut sorted_tags: Vec<_> = metadata
        .iter()
        .filter(|(k, _)| k.starts_with("Canon:"))
        .collect();
    sorted_tags.sort_by_key(|(k, _)| *k);

    for (key, value) in sorted_tags {
        eprintln!("{}: {:?}", key, value);
    }

    eprintln!("=========================================\n");

    // Additional verification: check for common Canon tags
    // These should be present in most Canon images
    eprintln!("=== Verification Summary ===");

    if metadata.contains_key("Canon:CanonModelID") {
        eprintln!("✓ Canon:CanonModelID found");
    } else {
        eprintln!("⚠ Canon:CanonModelID not found (may not be in all Canon images)");
    }

    if metadata.contains_key("Canon:CanonFirmwareVersion") {
        eprintln!("✓ Canon:CanonFirmwareVersion found");
    } else {
        eprintln!("⚠ Canon:CanonFirmwareVersion not found (may not be in all Canon images)");
    }

    if metadata.contains_key("Canon:CanonImageType") {
        eprintln!("✓ Canon:CanonImageType found");
    } else {
        eprintln!("⚠ Canon:CanonImageType not found (may not be in all Canon images)");
    }

    eprintln!("============================\n");
}
