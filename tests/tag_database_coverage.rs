//! Integration tests for active tag database coverage

use oxidex::tag_db::{generated_tags::generated_tag_count, get_tag_descriptor, tag_count};

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
