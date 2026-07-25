use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

const OXIDEX_BIN: &str = env!(
    "CARGO_BIN_EXE_oxidex",
    "oxidex binary not found. Run `cargo build` first."
);

/// Helper function to run the oxidex CLI command
fn run_oxidex_command(args: &[&str], input_file: &Path) -> (String, String, i32) {
    let mut command_args = args.to_vec();
    command_args.push(input_file.to_str().unwrap());

    let output = Command::new(OXIDEX_BIN)
        .args(&command_args)
        .output()
        .expect("Failed to execute oxidex command");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    (stdout, stderr, exit_code)
}

/// Helper function to run the oxidex CLI command and read metadata (JSON output)
fn read_metadata_json(file: &Path) -> serde_json::Value {
    let (stdout, _, exit_code) = run_oxidex_command(&["-j"], file);
    assert_eq!(exit_code, 0, "Failed to read metadata in JSON format.");
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("Failed to parse JSON output");

    // oxidex -j returns an array of objects [{...}]
    if let Some(array) = json.as_array() {
        if let Some(first) = array.first() {
            return first.clone();
        }
    }

    // Fallback (should not happen if output format is correct)
    json
}

#[test]
/// Test `oxidex -all=` to remove all metadata
fn test_cli_remove_all_metadata() {
    let temp_dir = tempdir().expect("Failed to create temporary directory");
    let test_file = temp_dir.path().join("sample_with_exif.jpg");
    fs::copy("tests/fixtures/jpeg/sample_with_exif.jpg", &test_file)
        .expect("Failed to copy test file");

    // Remove all metadata
    let (stdout, stderr, exit_code) = run_oxidex_command(&["-all="], &test_file);
    assert_eq!(exit_code, 0, "stdout: {}\nstderr: {}", stdout, stderr);
    assert!(stdout.contains("1 image files updated"));

    // Verify metadata is empty or minimal after removal
    let metadata = read_metadata_json(&test_file);
    // ExifTool preserves some basic structural tags even after -all=, so we check for common EXIF tags
    assert!(metadata.get("IFD0:Make").is_none());
    assert!(metadata.get("IFD0:Model").is_none());
    assert!(metadata.get("EXIF:DateTimeOriginal").is_none());
    // There might be some very basic file system info or similar, but the core EXIF/XMP/IPTC should be gone
    assert!(metadata.as_object().map_or(true, |obj| obj.len() < 15)); // Expect few tags (mostly File:* system tags)
}

#[test]
/// Test `oxidex -TAG=` to delete a specific tag
fn test_cli_delete_specific_tag() {
    let temp_dir = tempdir().expect("Failed to create temporary directory");
    let test_file = temp_dir.path().join("sample_with_exif.jpg");
    fs::copy("tests/fixtures/jpeg/sample_with_exif.jpg", &test_file)
        .expect("Failed to copy test file");

    // Verify IFD0:Make exists initially
    let initial_metadata = read_metadata_json(&test_file);
    assert!(initial_metadata.get("IFD0:Make").is_some());

    // Delete IFD0:Make tag
    let (stdout, stderr, exit_code) = run_oxidex_command(&["-IFD0:Make="], &test_file);
    assert_eq!(exit_code, 0, "stdout: {}\nstderr: {}", stdout, stderr);
    assert!(stdout.contains("1 image files updated"));

    // Verify IFD0:Make is gone and other tags remain
    let final_metadata = read_metadata_json(&test_file);
    assert!(final_metadata.get("IFD0:Make").is_none());
    assert!(final_metadata.get("IFD0:Model").is_some()); // Other tag should still exist
}

#[test]
/// Test `oxidex -TAG -TAG` for specific tag extraction
fn test_cli_specific_tag_extraction() {
    let temp_dir = tempdir().expect("Failed to create temporary directory");
    let test_file = temp_dir.path().join("sample_with_exif.jpg");
    fs::copy("tests/fixtures/jpeg/sample_with_exif.jpg", &test_file)
        .expect("Failed to copy test file");

    // Extract only IFD0:Make and IFD0:Model
    let (stdout, stderr, exit_code) =
        run_oxidex_command(&["-IFD0:Make", "-IFD0:Model"], &test_file);
    assert_eq!(exit_code, 0, "stdout: {}\nstderr: {}", stdout, stderr);

    // Verify output contains only specified tags (human-readable format)
    assert!(stdout.contains("IFD0:Make"));
    assert!(stdout.contains("IFD0:Model"));
    assert!(!stdout.contains("IFD0:ModifyDate")); // Should not contain other tags
    assert!(!stdout.contains("Found metadata tag(s)")); // Should not contain general header
    assert_eq!(
        stdout
            .lines()
            .filter(|&line| !line.trim().is_empty())
            .count(),
        2
    ); // Only 2 relevant lines
}

#[test]
/// Test `oxidex -s` for short output format
fn test_cli_short_format_output() {
    let temp_dir = tempdir().expect("Failed to create temporary directory");
    let test_file = temp_dir.path().join("sample_with_exif_xmp.jpg");
    fs::copy("tests/fixtures/jpeg/sample_with_exif_xmp.jpg", &test_file)
        .expect("Failed to copy test file");

    // Run oxidex with short format flag
    let (stdout, stderr, exit_code) = run_oxidex_command(&["-s"], &test_file);
    assert_eq!(exit_code, 0, "stdout: {}\nstderr: {}", stdout, stderr);

    // Verify output format: "TagName: Value" (no family prefix, shortened names for some tags)
    // and long values are truncated.
    // We expect some common tags to be present in short format.
    assert!(stdout.contains("Make:"));
    assert!(stdout.contains("Model:"));
    assert!(stdout.contains("Creator:"));
    // Check for truncation if an XMP tag with long value exists
    assert!(!stdout.contains("IFD0:")); // No family prefix
    assert!(!stdout.contains("Found metadata tag(s):")); // No header
}
