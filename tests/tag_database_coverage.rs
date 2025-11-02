//! Integration tests for tag database coverage

use exiftool_rs::tag_db::generated_tags::generated_tag_count;

#[test]
fn test_tag_database_has_minimum_coverage() {
    let count = generated_tag_count();

    // After adding all base format modules, expect at least 3000 tags
    assert!(
        count >= 3000,
        "Expected at least 3000 tags, found {}. Need to add more modules to build.rs",
        count
    );
}

#[test]
fn test_tag_database_target_coverage() {
    let count = generated_tag_count();

    // Ultimate target: 28,853 tags for full ExifTool parity
    // This test documents the gap
    let target = 28853;
    let coverage_percent = (count as f64 / target as f64) * 100.0;

    println!("Tag coverage: {}/{} ({:.1}%)", count, target, coverage_percent);

    // For now, we expect at least 10% coverage (2886 tags)
    assert!(
        count >= 2886,
        "Expected at least 10% coverage (2886 tags), found {} ({:.1}%)",
        count,
        coverage_percent
    );
}
