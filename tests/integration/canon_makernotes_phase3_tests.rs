//! Integration tests for Canon MakerNotes Phase 3 features
//!
//! Tests lens database, AFInfo, and FileInfo array parsing.
//!
//! Every expected lens name below is the literal right-hand side of a line in
//! ExifTool's `%canonLensTypes` (Canon.pm), quoted in the comment beside it.
//! The names these tests used to assert came from a hand-written table that
//! agreed with `%canonLensTypes` on 22 of its 146 entries; because the tests
//! were written from the same table, they passed the whole time.

const CANON_EOS_M10: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonEOS_M10.jpg";
const CANON_EOS_R6M2: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonEOS_R6m2.jpg";

/// Canon.pm FileInfo key 6 uses `RawConv => '$val <= 0 ? undef : $val'` and
/// its `canonQuality` table renders the real EOS M10 corpus value 3 as Fine.
#[test]
fn eos_m10_reports_file_info_raw_jpg_quality() {
    use oxidex::core::operations::read_metadata;
    use std::path::Path;

    if !Path::new(CANON_EOS_M10).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(CANON_EOS_M10)).expect("EOS M10 parses");
    assert_eq!(metadata.get_string("Canon:RawJpgQuality"), Some("Fine"));
}

/// Canon.pm CameraSettings key 52 is `HDR-PQ`; pinned ExifTool 13.59 reports
/// the real EOS R6 Mark II corpus value as Off.
#[test]
fn eos_r6m2_reports_camera_settings_hdr_pq() {
    use oxidex::core::operations::read_metadata;
    use std::path::Path;

    if !Path::new(CANON_EOS_R6M2).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(CANON_EOS_R6M2)).expect("EOS R6 Mark II parses");
    assert_eq!(metadata.get_string("Canon:HDR-PQ"), Some("Off"));
}

#[test]
fn test_canon_lens_database_integration() {
    // This test verifies that lens IDs from real Canon JPEG files
    // are correctly mapped to lens names using the lens database
    //
    // Note: This test will use synthetic test data since we don't have
    // real Canon files with known lens IDs in the test fixtures.
    // In production, this would be tested with real Canon images.

    // For now, verify that the lens database module compiles and links
    use oxidex::parsers::tiff::makernotes::canon_lens_database::lookup_lens_name;

    // Canon.pm:561  `4156 => 'Canon EF 50mm f/1.8 STM',`
    assert_eq!(
        lookup_lens_name(4156),
        Some("Canon EF 50mm f/1.8 STM".to_string())
    );
    // Canon.pm:480  `368 => 'Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens',`
    assert_eq!(
        lookup_lens_name(368),
        Some("Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string())
    );
    // Canon.pm:583  `61182 => 'Canon RF 50mm F1.2L USM or other Canon RF Lens',`
    // Every RF lens reports this one id; ExifTool files the individual models
    // under 61182.1-61182.68 and only resolves them for Composite:LensID.
    assert_eq!(
        lookup_lens_name(61182),
        Some("Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string())
    );
    // Ids the table does not carry must stay absent rather than borrow a
    // neighbour's name. 61183-61193 were eleven invented RF entries.
    assert_eq!(lookup_lens_name(61183), None);
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
    use oxidex::parsers::tiff::makernotes::canon_lens_database::lookup_lens_name;

    // Test coverage of major lens categories
    let test_lenses = vec![
        // (id, name, Canon.pm line the name is copied from)
        (4156, "Canon EF 50mm f/1.8 STM", 561),
        (
            368,
            "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens",
            480,
        ),
        (61182, "Canon RF 50mm F1.2L USM or other Canon RF Lens", 583),
        (186, "Canon EF 70-200mm f/4L USM", 390),
        (50, "Canon EF-S 18-200mm f/3.5-5.6 IS", 201),
        (4142, "Canon EF-S 18-135mm f/3.5-5.6 IS STM", 547),
        (65535, "n/a", 652),
    ];

    for (lens_id, expected_name, pm_line) in test_lenses {
        let result = lookup_lens_name(lens_id);
        assert!(
            result.is_some(),
            "Lens ID {} should be in database (Canon.pm:{})",
            lens_id,
            pm_line
        );
        assert_eq!(
            result.unwrap(),
            expected_name,
            "Lens ID {} disagrees with Canon.pm:{}",
            lens_id,
            pm_line
        );
    }
}
