//! Source-backed acceptance tests for the Pentax/Panasonic forward port.
//!
//! Values are from ExifTool 13.59's `Pentax.pm`, `Panasonic.pm`, and
//! `Olympus.pm`. The ignored carrier tests deliberately require the configured
//! combined corpus; Task 13's focused command runs them with
//! `--include-ignored`, so an absent fixture is a failure rather than a skip.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::TagValue;
use oxidex::core::operations::read_metadata;
use oxidex::core::tag_occurrence::ValueChannel;
use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::panasonic::parse_panasonic_makernotes;
use oxidex::parsers::tiff::makernotes::pentax::PentaxParser;
use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
use std::collections::HashMap;

fn pentax_block(tag: u16, field_type: u16, count: u32, trailer: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"PENTAX \0MM");
    out.extend_from_slice(&1_u16.to_be_bytes());
    out.extend_from_slice(&tag.to_be_bytes());
    out.extend_from_slice(&field_type.to_be_bytes());
    out.extend_from_slice(&count.to_be_bytes());
    out.extend_from_slice(&64_u32.to_be_bytes());
    out.extend_from_slice(&0_u32.to_be_bytes());
    out.resize(64, 0);
    out.extend_from_slice(trailer);
    out
}

#[test]
fn pentax_caf_point_info_decodes_source_bitmasks() {
    // CAFPointInfo bytes: reserved, 2x2 grid, then states 01,10,11,00.
    // DecodeAFPoints(mask 0x02) selects 2,3; mask 0x03 selects 1,2,3.
    // Keep the synthetic record out-of-line like real CAFPointInfo values;
    // TIFF stores an UNDEFINED value of four bytes or fewer inline.
    let data = pentax_block(0x0238, 7, 6, &[0, 0x22, 0x6c, 0, 0, 0]);
    let mut tags = HashMap::new();
    PentaxParser::default()
        .parse(&data, ByteOrder::LittleEndian, &mut tags)
        .expect("synthetic Pentax CAF MakerNote parses");

    assert_eq!(
        tags.get("Pentax:NumCAFPoints").map(String::as_str),
        Some("4")
    );
    assert_eq!(
        tags.get("Pentax:CAFGridSize").map(String::as_str),
        Some("2x2")
    );
    assert_eq!(
        tags.get("Pentax:CAFPointsInFocus").map(String::as_str),
        Some("2,3")
    );
    assert_eq!(
        tags.get("Pentax:CAFPointsSelected").map(String::as_str),
        Some("1,2,3")
    );
}

#[test]
fn pentax_external_flash_guide_number_decodes_fractional_field() {
    for (raw, expected) in [(0_u8, "n/a"), (6, "21"), (29, "14"), (31, "61")] {
        let mut record = [0_u8; 27];
        record[24] = raw;
        let data = pentax_block(0x0208, 7, 27, &record);
        let mut tags = HashMap::new();
        PentaxParser::default()
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .expect("synthetic Pentax FlashInfo MakerNote parses");
        assert_eq!(
            tags.get("Pentax:ExternalFlashGuideNumber")
                .map(String::as_str),
            Some(expected),
            "FlashInfo byte 24 raw {raw}"
        );
    }
}

#[test]
fn panasonic_mke_type2_uses_generated_little_endian_layout() {
    let mut tags = HashMap::new();
    parse_panasonic_makernotes(b"MKEM\0\0\x88\0", ByteOrder::BigEndian, &mut tags);

    assert_eq!(
        tags.get("Panasonic:MakerNoteType").map(String::as_str),
        Some("MKEM")
    );
    assert_eq!(tags.get("Panasonic:Gain").map(String::as_str), Some("136"));
}

#[test]
#[ignore = "requires configured ExifTool 13.59 combined Pentax carriers"]
fn pentax_real_carriers_prove_caf_guide_iso_and_city_semantics() {
    let q_s1 = fixtures::required_combined_fixture_path("Pentax/PentaxQ-S1.jpg");
    let q_s1 = read_metadata(&q_s1).expect("read required Pentax Q-S1 carrier");
    assert_eq!(q_s1.get_string("Pentax:CAFGridSize"), Some("7x7"));
    assert_eq!(q_s1.get_string("Pentax:CAFPointsInFocus"), Some("34"));
    assert_eq!(q_s1.get_string("Pentax:CAFPointsSelected"), Some("34"));
    assert_eq!(q_s1.get_string("Pentax:ISO"), Some("400"));
    assert_eq!(q_s1.get_string("Pentax:HometownCity"), Some("Tokyo"));
    assert_eq!(q_s1.get_string("Pentax:DestinationCity"), Some("Tokyo"));

    let k10d = fixtures::required_combined_fixture_path("Pentax/PentaxK10D.jpg");
    let k10d = read_metadata(&k10d).expect("read required Pentax K10D carrier");
    assert_eq!(
        k10d.get_string("Pentax:ExternalFlashGuideNumber"),
        Some("n/a")
    );
    assert_eq!(k10d.get_string("Pentax:ISO"), Some("100"));
    assert_eq!(k10d.get_string("Pentax:HometownCity"), Some("Tokyo"));
    assert_eq!(k10d.get_string("Pentax:DestinationCity"), Some("Paris"));
}

#[test]
#[ignore = "requires configured ExifTool 13.59 combined Panasonic carriers"]
fn panasonic_real_type2_carrier_proves_makernote_type_and_gain() {
    let path = fixtures::required_combined_fixture_path("Panasonic/PanasonicPV-DV401-K.jpg");
    let metadata = read_metadata(&path).expect("read required Panasonic Type2 carrier");
    assert_eq!(metadata.get_string("Panasonic:MakerNoteType"), Some("MKEM"));
    assert_eq!(metadata.get_integer("Panasonic:Gain"), Some(136));
}

#[test]
#[ignore = "requires configured ExifTool 13.59 combined Panasonic carriers"]
fn panasonic_lens_type_cascades_from_typed_inputs_in_normal_and_numeric_modes() {
    let path = fixtures::required_combined_fixture_path("Panasonic/PanasonicDC-GX800.jpg");
    let metadata = read_metadata(&path).expect("read required Panasonic lens carrier");

    let make_value = metadata
        .project_occurrences(ValueChannel::ValueConv)
        .find_map(|(key, _, value)| (key == "Panasonic:LensTypeMake").then(|| value.into_owned()))
        .expect("typed LensTypeMake occurrence");
    assert_eq!(make_value, TagValue::Integer(2));

    assert_eq!(
        metadata.get_string("Composite:LensType"),
        Some("Lumix G Vario 12-32mm F3.5-5.6 Asph. Mega OIS")
    );
    assert_eq!(
        metadata
            .without_print_conv()
            .get_string("Composite:LensType"),
        Some("2 20 10")
    );
}
