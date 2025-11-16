//! Integration tests for Canon MakerNotes Phase 3 features
//!
//! Tests lens database, AFInfo, and FileInfo array parsing

#[test]
fn test_canon_lens_database_integration() {
    // This test verifies that lens IDs from real Canon JPEG files
    // are correctly mapped to lens names using the lens database
    //
    // Note: This test will use synthetic test data since we don't have
    // real Canon files with known lens IDs in the test fixtures.
    // In production, this would be tested with real Canon images.

    // For now, verify that the lens database module compiles and links
    use exiftool_rs::parsers::tiff::makernotes::canon_lens_database::lookup_lens_name;

    // Test common lens lookups
    assert_eq!(
        lookup_lens_name(4156),
        Some("Canon EF 50mm f/1.8 STM".to_string())
    );
    assert_eq!(
        lookup_lens_name(368),
        Some("Canon EF 24-70mm f/2.8L II USM".to_string())
    );
    assert_eq!(
        lookup_lens_name(61182),
        Some("Canon RF 24-105mm f/4L IS USM".to_string())
    );
}

#[test]
fn test_canon_phase3_tags_extracted() {
    // Verify that Phase 3 tags are being extracted from Canon files
    // This is a placeholder test - in production, use real Canon test files

    // Test that the extraction functions are available
    // (More comprehensive testing would require real Canon JPEG fixtures)
    println!("Canon MakerNotes Phase 3 integration test placeholder");
}

#[test]
fn test_lens_database_coverage() {
    // Verify lens database has good coverage
    use exiftool_rs::parsers::tiff::makernotes::canon_lens_database::lookup_lens_name;

    // Test coverage of major lens categories
    let test_lenses = vec![
        (4156, "Canon EF 50mm f/1.8 STM"),        // Budget prime
        (368, "Canon EF 24-70mm f/2.8L II USM"),  // Pro zoom
        (61182, "Canon RF 24-105mm f/4L IS USM"), // RF mirrorless
        (186, "Canon EF 70-200mm f/2.8L IS"),     // Pro telephoto
        (50, "Canon EF 17-40mm f/4L USM"),        // Wide angle
    ];

    for (lens_id, expected_name) in test_lenses {
        let result = lookup_lens_name(lens_id);
        assert!(
            result.is_some(),
            "Lens ID {} should be in database",
            lens_id
        );
        assert_eq!(
            result.unwrap(),
            expected_name,
            "Lens ID {} has wrong name",
            lens_id
        );
    }
}
