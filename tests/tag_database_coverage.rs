//! Integration tests for active tag database coverage

use oxidex::core::{TagValue, read_metadata, validate_tag_value, write_metadata};
use oxidex::error::ExifToolError;
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
    let before = fs::read(&png_path).expect("read PNG fixture");

    let descriptor =
        get_tag_descriptor("PNG:ImageWidth").expect("expected source-backed PNG descriptor");
    assert!(!descriptor.is_writable());
    // PNG.pm declares the IHDR width as int32u. The parser's integer value
    // agrees with that type even though the PNG writer cannot change it.
    validate_tag_value(descriptor, &TagValue::new_integer(641))
        .expect("source-typed PNG width must accept the parser's integer value");

    // Validation accepts the integer; the write is then refused because no
    // writer writes a PNG's IHDR width -- which `Ok(())` used to hide.
    let mut metadata = read_metadata(&png_path).expect("read PNG");
    metadata.insert("PNG:ImageWidth".to_string(), TagValue::new_integer(641));
    let error = write_metadata(&png_path, &metadata)
        .expect_err("a width no writer writes must not be reported written");
    assert!(
        matches!(error, ExifToolError::TagsNotWritten { .. }),
        "source-typed PNG width must not reject parser-compatible integer values: {error:?}"
    );
    assert_eq!(
        fs::read(&png_path).unwrap(),
        before,
        "refused write changed the file"
    );

    let mut invalid = read_metadata(&png_path).expect("read PNG");
    invalid.insert("PNG:ImageWidth".to_string(), TagValue::new_rational(1, 0));
    let error = write_metadata(&png_path, &invalid).expect_err("rational width must be rejected");
    assert!(
        error
            .to_string()
            .contains("Type mismatch: expected Integer but got Rational"),
        "source-typed PNG width should reject a rational value: {error:?}"
    );
    assert_eq!(fs::read(&png_path).unwrap(), before);

    // An unknown YAML type still checks intrinsic rational validity.
    let untyped = get_tag_descriptor("PNG:ImageHeader")
        .expect("expected source-declared untyped PNG descriptor");
    let error = validate_tag_value(untyped, &TagValue::new_rational(1, 0))
        .expect_err("zero denominator must be rejected for an untyped descriptor");
    assert!(error.to_string().contains("denominator cannot be zero"));
}

#[test]
fn test_mixed_duplicate_yaml_descriptors_do_not_force_strict_type_validation() {
    let descriptor = get_tag_descriptor("Panasonic:WBRedLevel")
        .expect("expected YAML-backed duplicate maker descriptor");

    validate_tag_value(descriptor, &TagValue::new_rational(1, 2))
        .expect("mixed duplicate YAML types must not force fallback descriptor type");
}
