//! Integration tests for active tag database coverage

use oxidex::core::{MetadataMap, TagValue, validate_tag_value, write_metadata};
use oxidex::tag_db::{generated_tags::generated_tag_count, get_tag_descriptor, tag_count};
use std::fs;
use tempfile::tempdir;

#[test]
fn test_tag_database_count_comes_from_active_registry() {
    assert_eq!(
        generated_tag_count(),
        tag_count(),
        "legacy generated count must reflect active registry count"
    );
    assert!(
        tag_count() >= 2886,
        "expected active registry to expose at least 10% ExifTool tag coverage"
    );
}

#[test]
fn test_core_tag_descriptors_are_reachable() {
    for tag in [
        "EXIF:Make",
        "EXIF:Model",
        "GPS:GPSLatitude",
        "XMP:Creator",
        "IPTC:ObjectName",
    ] {
        assert!(
            get_tag_descriptor(tag).is_some(),
            "expected active registry descriptor for {tag}"
        );
    }
}

#[test]
fn test_yaml_backed_descriptors_do_not_reject_parser_value_types() {
    let temp_dir = tempdir().expect("create temp directory");
    let png_path = temp_dir.path().join("sample.png");
    fs::copy("tests/fixtures/png/sample.png", &png_path).expect("copy PNG fixture");

    let descriptor =
        get_tag_descriptor("PNG:ImageWidth").expect("expected YAML-backed PNG descriptor");
    assert!(!descriptor.is_writable());
    validate_tag_value(descriptor, &TagValue::new_integer(640))
        .expect("public validation must share unreliable YAML type semantics");

    let mut metadata = MetadataMap::new();
    metadata.insert("PNG:ImageWidth".to_string(), TagValue::new_integer(640));
    write_metadata(&png_path, &metadata)
        .expect("untyped YAML descriptors must not reject parser-compatible integer values");

    let mut invalid = MetadataMap::new();
    invalid.insert("PNG:ImageWidth".to_string(), TagValue::new_rational(1, 0));
    let error = write_metadata(&png_path, &invalid).expect_err("zero denominator must be rejected");
    assert!(error.to_string().contains("denominator cannot be zero"));
}

#[test]
fn test_mixed_duplicate_yaml_descriptors_do_not_force_strict_type_validation() {
    let descriptor = get_tag_descriptor("Panasonic:WBRedLevel")
        .expect("expected YAML-backed duplicate maker descriptor");

    validate_tag_value(descriptor, &TagValue::new_rational(1, 2))
        .expect("mixed duplicate YAML types must not force fallback descriptor type");
}
