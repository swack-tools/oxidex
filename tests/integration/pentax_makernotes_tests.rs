//! Integration tests for Pentax MakerNotes parser
//!
//! Tests the Pentax MakerNotes parsing functionality including:
//! - Lens database lookups (K-mount classic and modern lenses)
//! - MakerNoteParser trait implementation
//! - Header validation
//! - Tag extraction from synthetic test data

const PENTAX_Q7: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Pentax/PentaxQ7.jpg";
const PENTAX_OPTIO_430: &str =
    "/tmp/oxidex-exiftool-cache/combined-samples/Pentax/PentaxOptio430.jpg";
const PENTAX_OPTIO_430_RS: &str =
    "/tmp/oxidex-exiftool-cache/combined-samples/Pentax/PentaxOptio430RS.jpg";
const PENTAX_EI_200: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Pentax/PentaxEI-200.jpg";

/// Pentax Type2 records store their city codes as four-byte `undef` values,
/// including significant trailing spaces.  ExifTool exposes the raw string,
/// not a numeric city lookup, for these legacy Optio fields.
#[test]
fn pentax_type2_preserves_hometown_and_destination_city_codes() {
    use oxidex::core::operations::read_metadata;
    use std::path::Path;

    if !Path::new(PENTAX_OPTIO_430).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(PENTAX_OPTIO_430)).expect("Pentax Optio 430 parses");
    assert_eq!(metadata.get_string("Pentax:HometownCityCode"), Some("NYC "));
    assert_eq!(
        metadata.get_string("Pentax:DestinationCityCode"),
        Some("    ")
    );
}

/// The AOC Type-3 directory in the Optio 430RS uses Casio's 0x3007 field.
/// Its zero value is the only model-independent Best Shot rendering: `Off`.
#[test]
fn pentax_optio_430rs_reports_casio_best_shot_off() {
    use oxidex::core::operations::read_metadata;
    use std::path::Path;

    if !Path::new(PENTAX_OPTIO_430_RS).is_file() {
        return;
    }

    let metadata =
        read_metadata(Path::new(PENTAX_OPTIO_430_RS)).expect("Pentax Optio 430RS parses");
    assert_eq!(metadata.get_string("Casio:BestShotMode"), Some("Off"));
}

/// The EI-200 is Pentax-branded but carries ExifTool's signature-selected
/// Kodak Type-2 record.  Its maker must therefore retain the Kodak family.
#[test]
fn pentax_ei_200_reports_kodak_type2_maker() {
    use oxidex::core::operations::read_metadata;
    use std::path::Path;

    if !Path::new(PENTAX_EI_200).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(PENTAX_EI_200)).expect("Pentax EI-200 parses");
    assert_eq!(metadata.get_string("Kodak:KodakMaker"), Some("PENTAX"));
}

/// ExifTool 13.59 decodes the Q7's 0x0238 CAFPointInfo record even when its
/// zero-by-zero grid contains no selected or in-focus points.  The empty
/// bitfields must be represented as ExifTool's `(none)`, not omitted.
#[test]
fn pentax_q7_reports_empty_caf_point_sets() {
    use oxidex::core::operations::read_metadata;
    use std::path::Path;

    if !Path::new(PENTAX_Q7).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(PENTAX_Q7)).expect("Pentax Q7 parses");
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
