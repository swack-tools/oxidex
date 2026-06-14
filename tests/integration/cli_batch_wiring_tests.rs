//! Integration tests for advertised CLI output options and batch output flags.
//!
//! These tests run the real `oxidex` binary to confirm that:
//! - ExifTool-style single-dash long options (e.g. `-json`) are normalized
//!   before lexopt parsing instead of being treated as clustered short flags.
//! - Batch directory processing honors output format flags (e.g. `-s` short
//!   format) by routing through the shared output formatters.

use std::process::Command;

fn oxidex(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .expect("run oxidex binary")
}

#[test]
fn single_dash_json_is_accepted() {
    let output = oxidex(&["-json", "tests/fixtures/jpeg/sample_with_exif.jpg"]);
    assert!(
        output.status.success(),
        "expected -json to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .trim_start()
            .starts_with('[')
    );
}

#[test]
fn batch_directory_honors_short_format() {
    let output = oxidex(&["-s", "tests/fixtures/jpeg/simple"]);
    assert!(
        output.status.success(),
        "expected batch -s to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The short formatter strips family prefixes (e.g. `IFD0:Make` -> `Make:`),
    // so assert that real EXIF-derived tags appear in their short form rather
    // than expecting the family-prefixed names emitted by the human formatter.
    assert!(stdout.contains("Make:") || stdout.contains("Model:"));
    // The single-file human-readable path prints a "Found N metadata tag(s):"
    // header; the batch path must not, confirming batch output routing.
    assert!(!stdout.contains("Found "));
}
