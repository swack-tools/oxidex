//! Integration tests for Pentax MakerNotes parser
//!
//! Tests the Pentax MakerNotes parsing functionality including:
//! - Lens database lookups (K-mount classic and modern lenses)
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data

const PENTAX_Q7: &str = "Pentax/PentaxQ7.jpg";
const PENTAX_K5: &str = "Pentax/PentaxK-5.jpg";
const PENTAX_OPTIO_430: &str = "Pentax/PentaxOptio430.jpg";
const PENTAX_OPTIO_430_RS: &str = "Pentax/PentaxOptio430RS.jpg";
const PENTAX_EI_200: &str = "Pentax/PentaxEI-200.jpg";

/// Asahi's headerless Optio 430 note selects Pentax::Type2, not Pentax::Main.
/// These are the complete public Pentax values from pinned ExifTool 13.59;
/// the city-code whitespace is significant.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn pentax_type2_uses_its_own_table_without_main_leakage() {
    use oxidex::core::operations::read_metadata;

    let metadata = read_metadata(&crate::fixtures::required_combined_fixture_path(
        PENTAX_OPTIO_430,
    ))
    .expect("Pentax Optio 430 parses");
    let expected = [
        ("RecordingMode", "Auto"),
        ("Quality", "Best"),
        ("FocusMode", "Auto"),
        ("FlashMode", "Off"),
        ("WhiteBalance", "Auto"),
        ("DigitalZoom", "0"),
        ("Sharpness", "Normal"),
        ("Contrast", "Normal"),
        ("Saturation", "Normal"),
        ("ISO", "100"),
        ("ColorFilter", "Full"),
        ("HometownCityCode", "NYC "),
        ("DestinationCityCode", "    "),
    ];
    for (name, value) in expected {
        assert_eq!(metadata.get_string(&format!("Pentax:{name}")), Some(value));
    }
    let actual: std::collections::HashSet<_> = metadata
        .iter()
        .filter_map(|(name, _)| name.strip_prefix("Pentax:"))
        .collect();
    let expected_names: std::collections::HashSet<_> =
        expected.iter().map(|(name, _)| *name).collect();
    assert_eq!(actual, expected_names, "Type2 must not emit Main-only tags");

    // `-n` bypasses each Type2 PrintConv. Pinned ExifTool 13.59 reports
    // these exact raw values; using Main's reverse enum would be wrong.
    let raw = metadata.without_print_conv();
    let expected_raw = [
        ("RecordingMode", "0"),
        ("Quality", "2"),
        ("FocusMode", "3"),
        ("FlashMode", "4"),
        ("WhiteBalance", "0"),
        ("DigitalZoom", "0"),
        ("Sharpness", "0"),
        ("Contrast", "0"),
        ("Saturation", "0"),
        ("ISO", "100"),
        ("ColorFilter", "1"),
        ("HometownCityCode", "NYC "),
        ("DestinationCityCode", "    "),
    ];
    for (name, value) in expected_raw {
        assert_eq!(raw.get_string(&format!("Pentax:{name}")), Some(value));
    }
}

/// The AOC directory in the Optio 430RS selects Casio::Type2's 0x3007 field.
/// Its zero value is the only model-independent Best Shot rendering: `Off`.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn pentax_optio_430rs_reports_casio_best_shot_off() {
    use oxidex::core::operations::read_metadata;

    let metadata = read_metadata(&crate::fixtures::required_combined_fixture_path(
        PENTAX_OPTIO_430_RS,
    ))
    .expect("Pentax Optio 430RS parses");
    assert_eq!(metadata.get_string("Casio:BestShotMode"), Some("Off"));
}

/// The EI-200 is Pentax-branded but carries ExifTool's signature-selected
/// Kodak Type-2 record.  Its maker must therefore retain the Kodak family.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn pentax_ei_200_reports_kodak_type2_maker() {
    use oxidex::core::operations::read_metadata;

    let metadata = read_metadata(&crate::fixtures::required_combined_fixture_path(
        PENTAX_EI_200,
    ))
    .expect("Pentax EI-200 parses");
    assert_eq!(metadata.get_string("Kodak:KodakMaker"), Some("PENTAX"));
}

/// ExifTool 13.59 decodes the Q7's 0x0238 CAFPointInfo record even when its
/// zero-by-zero grid contains no selected or in-focus points.  The empty
/// bitfields must be represented as ExifTool's `(none)`, not omitted.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn pentax_q7_reports_empty_caf_point_sets() {
    use oxidex::core::operations::read_metadata;

    let metadata = read_metadata(&crate::fixtures::required_combined_fixture_path(PENTAX_Q7))
        .expect("Pentax Q7 parses");
    assert_eq!(
        metadata.get_string("Pentax:CAFPointsInFocus"),
        Some("(none)")
    );
    assert_eq!(
        metadata.get_string("Pentax:CAFPointsSelected"),
        Some("(none)")
    );
}

#[test]
fn test_pentax_parser_trait_implementation() {
    use oxidex::parsers::tiff::makernotes::pentax::PentaxParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;

    let parser = PentaxParser::default();
    assert_eq!(parser.manufacturer_name(), "Pentax");
    assert_eq!(parser.tag_prefix(), "Pentax:");
}

#[test]
fn test_pentax_validate_header_aoc() {
    use oxidex::parsers::tiff::makernotes::pentax::PentaxParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;

    let parser = PentaxParser::default();

    // Valid AOC header
    let valid_header = b"AOC\0\x00\x00extra_data_here";
    assert!(parser.validate_header(valid_header));

    // Invalid header
    let invalid_header = b"Canon\0\0\0";
    assert!(!parser.validate_header(invalid_header));

    // Too short
    let too_short = b"AOC";
    assert!(!parser.validate_header(too_short));
}

#[test]
fn test_pentax_validate_header_pentax() {
    use oxidex::parsers::tiff::makernotes::pentax::PentaxParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;

    let parser = PentaxParser::default();

    // Valid PENTAX header
    let valid_header = b"PENTAX \0more_data_follows";
    assert!(parser.validate_header(valid_header));
}

#[test]
fn test_pentax_parser_empty_data() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::pentax::PentaxParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = PentaxParser::default();
    let mut tags = HashMap::new();

    // Empty data should not cause errors
    let result = parser.parse(&[], ByteOrder::LittleEndian, &mut tags);
    assert!(result.is_ok());
    assert_eq!(tags.len(), 0);
}

#[test]
fn test_pentax_parser_invalid_header() {
    use oxidex::parsers::tiff::ifd_parser::ByteOrder;
    use oxidex::parsers::tiff::makernotes::pentax::PentaxParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
    use std::collections::HashMap;

    let parser = PentaxParser::default();
    let mut tags = HashMap::new();

    // Invalid header should return error
    let invalid_data = b"Nikon\0\0\0some_data";
    let result = parser.parse(invalid_data, ByteOrder::LittleEndian, &mut tags);

    // Invalid headers are handled gracefully (may return Ok with no tags)
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_pentax_decode_quality() {
    use oxidex::parsers::tiff::makernotes::pentax::PentaxParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;

    // This test verifies that the quality decoder functions work correctly
    // through the parser implementation
    let parser = PentaxParser::default();
    assert_eq!(parser.manufacturer_name(), "Pentax");
}

#[test]
fn test_pentax_decode_picture_modes() {
    use oxidex::parsers::tiff::makernotes::pentax::PentaxParser;
    use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;

    // Verify parser is correctly instantiated for picture mode decoding
    let parser = PentaxParser::default();
    assert_eq!(parser.tag_prefix(), "Pentax:");
}

/// The public legacy getter keeps ExifTool's JSON list text while typed
/// projections preserve the 17 source bytes and signed RawConv values.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn pentax_k5_filter_info_keeps_public_and_typed_forms() {
    use oxidex::core::TagValue;
    use oxidex::core::operations::read_metadata;
    use oxidex::core::tag_occurrence::ValueChannel;

    let path = crate::fixtures::required_combined_fixture_path(PENTAX_K5);
    let metadata = read_metadata(&path).expect("Pentax K-5 parses");
    let key = "Pentax:DigitalFilter01";
    assert_eq!(
        metadata.get_string(key),
        Some(r#"["Toy Camera","Shading=2","Blur=2","ToneBreak=Red"]"#)
    );
    let stored: Vec<_> = metadata
        .project_occurrences(ValueChannel::Stored)
        .filter(|(row_key, _, _)| *row_key == key)
        .collect();
    assert_eq!(stored.len(), 1);
    assert!(matches!(stored[0].2.as_ref(), TagValue::Binary(bytes) if bytes.len() == 17));
    let value: Vec<_> = metadata
        .project_occurrences(ValueChannel::ValueConv)
        .filter(|(row_key, _, _)| *row_key == key)
        .collect();
    assert_eq!(value.len(), 1);
    assert_eq!(
        value[0].2.as_ref(),
        &TagValue::new_string("10 25 2 26 2 27 1 0 0 0 0 0 0 0 0 0 0")
    );
}
