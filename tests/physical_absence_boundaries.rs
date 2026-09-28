//! CLI regressions for physical Exif::Main copies the ordinary reader omits.
//! The synthetic fixtures and pinned-13.59 before/after probes are documented
//! in the PR 980 physical-absence repair evidence.

use std::process::Command;

fn run_write(original: &[u8], extension: &str, argument: &str) -> (std::process::Output, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(format!("input.{extension}"));
    std::fs::write(&file, original).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(["-overwrite_original", argument])
        .arg(&file)
        .output()
        .unwrap();
    (result, std::fs::read(file).unwrap())
}

fn assert_refused_unchanged(original: &[u8], extension: &str, argument: &str) {
    let (result, after) = run_write(original, extension, argument);
    assert!(
        !result.status.success(),
        "unsafe {argument} was reported successful: {} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(after, original, "unsafe {argument} changed the file");
}

#[test]
fn bare_subifd_tag_removal_is_not_a_false_no_op() {
    let original = include_bytes!("fixtures/physical_absence/subifd_blacklevel.jpg");
    assert_refused_unchanged(original, "jpg", "-BlackLevelRepeatDim=");
    // The same bounded child tree proves a different EXIF tag absent.
    let (result, after) = run_write(original, "jpg", "-CalibrationIlluminant1=");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(after, original);
}

#[test]
fn bare_sony_sr2_private_write_is_atomic() {
    let original = include_bytes!("fixtures/physical_absence/sony_sr2_private.arw");
    assert_refused_unchanged(original, "arw", "-ColorSpace#=2");
}
