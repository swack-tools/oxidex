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
const CANON_SAMPLE: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Canon.jpg";
const CANON_EOS_1DS: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonEOS-1DS.jpg";
const CANON_SX20IS: &str =
    "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonPowerShotSX20IS.jpg";
const CANON_A560: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonPowerShotA560.jpg";
const CANON_EOS_5D_M3: &str =
    "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonEOS5D_MarkIII.jpg";
const CANON_EOS_D60: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonEOS_D60.jpg";
const CANON_EOS_1D: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonEOS-1D.jpg";
const CANON_REBEL_T1I: &str =
    "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonEOS_REBEL_T1i.jpg";
const CANON_A1300: &str =
    "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonPowerShotA1300.jpg";

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

/// Pinned ExifTool 13.59 ground truth for Canon's direct MakerNote fields and
/// its two plain BinaryData records.  These files deliberately cover both the
/// model-gated D60 ColorBalance variant and the normal ColorBalance variant.
#[test]
fn canon_direct_and_binary_makernote_fields_match_pinned_exiftool() {
    use oxidex::core::operations::read_metadata;
    use std::path::Path;

    let cases = [
        (CANON_SAMPLE, "Canon:CanonFileLength", "4480822"),
        (CANON_SAMPLE, "Canon:WB_RGGBBlackLevels", "124 123 124 123"),
        (CANON_EOS_1DS, "Canon:RawDataLength", "0"),
        (CANON_SX20IS, "Canon:SuperMacro", "Off"),
        (CANON_A560, "Canon:FaceWidth", "35"),
        (CANON_EOS_5D_M3, "Canon:InternalSerialNumber2", "AD0010003"),
        (CANON_EOS_D60, "Canon:BlackLevels", "128 128 128 128"),
    ];

    for (path, tag, expected) in cases {
        if !Path::new(path).is_file() {
            return;
        }
        let metadata = read_metadata(Path::new(path)).expect("Canon fixture parses");
        assert_eq!(metadata.get_string(tag), Some(expected), "{path}: {tag}");
    }
}

/// Pinned ExifTool 13.59's nontrivial Canon direct-field behavior: the EOS-1D
/// AF grid has padding bits, `%longBin` measures decoded number text rather
/// than source bytes, and DustRemovalData is unconditionally binary.
#[test]
fn canon_legacy_direct_fields_match_pinned_exiftool() {
    use oxidex::core::operations::read_metadata;
    use std::path::Path;

    let cases = [
        (
            CANON_EOS_1D,
            "Canon:AFPointsInFocus1D",
            "Auto (B6,B7,C6,C7,C8,D6,D7,D8)",
        ),
        (
            CANON_EOS_1D,
            "Canon:ToneCurveTable",
            "(Binary data 1679 bytes, use -b option to extract)",
        ),
        (
            CANON_EOS_1D,
            "Canon:SharpnessTable",
            "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        ),
        (
            CANON_EOS_1D,
            "Canon:SharpnessFreqTable",
            "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        ),
        (
            CANON_EOS_1D,
            "Canon:WhiteBalanceTable",
            "(Binary data 2217 bytes, use -b option to extract)",
        ),
        (
            CANON_EOS_1D,
            "Canon:ToneCurveMatching",
            "(Binary data 95 bytes, use -b option to extract)",
        ),
        (
            CANON_EOS_1D,
            "Canon:WhiteBalanceMatching",
            "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        ),
        (
            CANON_REBEL_T1I,
            "Canon:DustRemovalData",
            "(Binary data 1024 bytes, use -b option to extract)",
        ),
        (CANON_A1300, "Canon:Categories", "(none)"),
        (CANON_EOS_R6M2, "Canon:AutoAFPointSelEOSiTRAF", "Enable"),
    ];

    for (path, tag, expected) in cases {
        if !Path::new(path).is_file() {
            return;
        }
        let metadata = read_metadata(Path::new(path)).expect("Canon fixture parses");
        assert_eq!(metadata.get_string(tag), Some(expected), "{path}: {tag}");
    }
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
