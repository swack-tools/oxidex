use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::nikon::sub_tables::parse_maker_notes_0x56;
use std::collections::HashMap;

/// Nikon.pm's `MakerNotes0x56` has a fixed Z-series record layout.  Its
/// firmware, packed burst fields, and independent pixel-shift flag are
/// source-derived fields; a missing parser must not collapse this carrier to
/// an opaque binary blob.
#[test]
fn nikon_maker_notes_0x56_decodes_burst_and_pixel_shift() {
    let data = [
        b'0', b'1', b'0', b'0', 0xa0, 0xe7, 0x30, 0x03, 1, 0, 0, 0, 1, 0, 0, 0,
    ];
    let mut tags = HashMap::new();

    parse_maker_notes_0x56(&data, ByteOrder::LittleEndian, &mut tags);

    assert_eq!(tags.get("Nikon:FirmwareVersion56"), Some(&"01.00".to_string()));
    assert_eq!(tags.get("Nikon:BurstStartSlotNumber"), Some(&"1".to_string()));
    assert_eq!(tags.get("Nikon:BurstStartFolderNumber"), Some(&"102".to_string()));
    assert_eq!(tags.get("Nikon:BurstStartImageNumber"), Some(&"1853".to_string()));
    assert_eq!(tags.get("Nikon:BurstStartImageType"), Some(&"JPG".to_string()));
    assert_eq!(tags.get("Nikon:BurstShotNumber"), Some(&"1".to_string()));
    assert_eq!(tags.get("Nikon:PixelShiftActive"), Some(&"Yes".to_string()));
}
