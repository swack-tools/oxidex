use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::pentax::PentaxParser;
use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
use std::collections::HashMap;

/// `Pentax::CAFPointInfo` stores four two-bit point states in each byte.
/// `10` marks an in-focus point and `11` a selected point; the point numbers
/// are one-based and most-significant pair first.
#[test]
fn pentax_caf_point_info_decodes_focus_and_selected_points() {
    let mut data = b"PENTAX \0II".to_vec();
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&0x0238u16.to_le_bytes());
    data.extend_from_slice(&7u16.to_le_bytes());
    data.extend_from_slice(&3u32.to_le_bytes());
    // `undef[3]` fits inline in the little-endian IFD value word.
    data.extend_from_slice(&0x00b2_2200u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());

    let mut tags = HashMap::new();
    PentaxParser::default()
        .parse(&data, ByteOrder::LittleEndian, &mut tags)
        .expect("synthetic Pentax CAF MakerNote parses");

    assert_eq!(tags.get("Pentax:NumCAFPoints"), Some(&"4".to_string()));
    assert_eq!(tags.get("Pentax:CAFGridSize"), Some(&"2x2".to_string()));
    assert_eq!(tags.get("Pentax:CAFPointsInFocus"), Some(&"1,4".to_string()));
    assert_eq!(tags.get("Pentax:CAFPointsSelected"), Some(&"2".to_string()));
}
