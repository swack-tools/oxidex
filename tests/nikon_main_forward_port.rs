use oxidex::cli::tag_resolution::{family1_label, resolve_requested_tags, resolved_display_value};
use oxidex::core::TagValue;
use oxidex::core::operations::read_metadata;
use oxidex::parsers::tiff::ifd_parser::ByteOrder;

#[path = "common/fixtures.rs"]
mod fixtures;

fn put_u16(bytes: &mut [u8], at: usize, value: u16, order: ByteOrder) {
    let encoded = match order {
        ByteOrder::LittleEndian => value.to_le_bytes(),
        ByteOrder::BigEndian => value.to_be_bytes(),
    };
    bytes[at..at + 2].copy_from_slice(&encoded);
}

fn put_u32(bytes: &mut [u8], at: usize, value: u32, order: ByteOrder) {
    let encoded = match order {
        ByteOrder::LittleEndian => value.to_le_bytes(),
        ByteOrder::BigEndian => value.to_be_bytes(),
    };
    bytes[at..at + 4].copy_from_slice(&encoded);
}

/// Builds a complete TIFF/EXIF carrier so the assertions exercise the real
/// MakerNote dispatcher and occurrence bridge, not a Nikon-local helper.
fn tiff_with_nikon_0x56(record: &[u8], order: ByteOrder) -> tempfile::NamedTempFile {
    const EXIF_IFD: usize = 38;
    const MAKE: usize = 56;
    const MAKERNOTE: usize = 74;
    const EMBEDDED_TIFF: usize = MAKERNOTE + 10;
    const RECORD: usize = EMBEDDED_TIFF + 26;

    let mut tiff = vec![0_u8; RECORD + record.len()];
    tiff[..2].copy_from_slice(match order {
        ByteOrder::LittleEndian => b"II",
        ByteOrder::BigEndian => b"MM",
    });
    put_u16(&mut tiff, 2, 42, order);
    put_u32(&mut tiff, 4, 8, order);
    put_u16(&mut tiff, 8, 2, order);

    put_u16(&mut tiff, 10, 0x010f, order);
    put_u16(&mut tiff, 12, 2, order);
    put_u32(&mut tiff, 14, 18, order);
    put_u32(&mut tiff, 18, MAKE as u32, order);
    put_u16(&mut tiff, 22, 0x8769, order);
    put_u16(&mut tiff, 24, 4, order);
    put_u32(&mut tiff, 26, 1, order);
    put_u32(&mut tiff, 30, EXIF_IFD as u32, order);

    put_u16(&mut tiff, EXIF_IFD, 1, order);
    put_u16(&mut tiff, EXIF_IFD + 2, 0x927c, order);
    put_u16(&mut tiff, EXIF_IFD + 4, 7, order);
    put_u32(
        &mut tiff,
        EXIF_IFD + 6,
        (RECORD + record.len() - MAKERNOTE) as u32,
        order,
    );
    put_u32(&mut tiff, EXIF_IFD + 10, MAKERNOTE as u32, order);

    tiff[MAKE..MAKERNOTE].copy_from_slice(b"NIKON CORPORATION\0");
    tiff[MAKERNOTE..EMBEDDED_TIFF].copy_from_slice(b"Nikon\0\x02\0\0\0");
    tiff[EMBEDDED_TIFF..EMBEDDED_TIFF + 2].copy_from_slice(match order {
        ByteOrder::LittleEndian => b"II",
        ByteOrder::BigEndian => b"MM",
    });
    put_u16(&mut tiff, EMBEDDED_TIFF + 2, 42, order);
    put_u32(&mut tiff, EMBEDDED_TIFF + 4, 8, order);
    put_u16(&mut tiff, EMBEDDED_TIFF + 8, 1, order);
    put_u16(&mut tiff, EMBEDDED_TIFF + 10, 0x0056, order);
    put_u16(&mut tiff, EMBEDDED_TIFF + 12, 7, order);
    put_u32(&mut tiff, EMBEDDED_TIFF + 14, record.len() as u32, order);
    put_u32(&mut tiff, EMBEDDED_TIFF + 18, 26, order);
    tiff[RECORD..].copy_from_slice(record);

    let file = tempfile::Builder::new()
        .suffix(".tif")
        .tempfile()
        .expect("create synthetic Nikon TIFF");
    std::fs::write(file.path(), tiff).expect("write synthetic Nikon TIFF");
    file
}

#[test]
fn nikon_main_0x56_public_reader_preserves_print_and_value_forms() {
    // Pinned Nikon.pm: firmware ValueConv changes the first digit pair;
    // bit-packed type and PixelShiftActive retain raw numbers under -n.
    let file = tiff_with_nikon_0x56(
        &[b'A', b'1', b'2', b'3', 1, 0, 0, 0, 0, 0, 0, 0, 2, 1, 0, 0],
        ByteOrder::LittleEndian,
    );
    let metadata = read_metadata(file.path()).expect("synthetic Nikon TIFF parses");

    assert_eq!(
        metadata.get_string("Nikon:FirmwareVersion56"),
        Some("A12.3")
    );
    for (key, shown, raw) in [
        ("Nikon:BurstStartImageType", "Unknown (1)", "1"),
        ("Nikon:PixelShiftActive", "Unknown (2)", "2"),
    ] {
        let requested = resolve_requested_tags(&metadata, &[key.to_string()], false);
        let occurrence = requested
            .first()
            .map(|resolved| resolved.occurrence)
            .unwrap_or_else(|| panic!("{key} must be a canonical occurrence"));
        assert_eq!(family1_label(occurrence), "Nikon", "{key} group");
        assert_eq!(
            resolved_display_value(occurrence, false),
            TagValue::new_string(shown)
        );
        assert_eq!(
            resolved_display_value(occurrence, true),
            TagValue::new_string(raw)
        );
    }
}

#[test]
fn nikon_main_0x56_uses_embedded_big_endian_and_withholds_truncated_fields() {
    let file = tiff_with_nikon_0x56(
        &[
            b'0', b'1', b'0', b'0', 0xa0, 0x00, 0x00, 0x02, 0, 0, 0, 17, 1,
        ],
        ByteOrder::BigEndian,
    );
    let metadata = read_metadata(file.path()).expect("big-endian Nikon TIFF parses");
    assert_eq!(
        metadata.get_string("Nikon:FirmwareVersion56"),
        Some("01.00")
    );
    assert_eq!(metadata.get_string("Nikon:BurstStartSlotNumber"), Some("2"));
    assert_eq!(
        metadata.get_string("Nikon:BurstStartImageType"),
        Some("NEF")
    );
    assert_eq!(metadata.get_string("Nikon:BurstShotNumber"), Some("17"));
    assert_eq!(metadata.get_string("Nikon:PixelShiftActive"), Some("Yes"));

    let truncated = tiff_with_nikon_0x56(
        &[b'N', b'O', b'P', b'E', 0, 0, 0, 0, 1],
        ByteOrder::LittleEndian,
    );
    let metadata = read_metadata(truncated.path()).expect("truncated Nikon TIFF parses");
    assert_eq!(metadata.get_string("Nikon:FirmwareVersion56"), Some("NOPE"));
    assert!(metadata.get("Nikon:BurstStartSlotNumber").is_none());
    assert!(metadata.get("Nikon:PixelShiftActive").is_none());
}

#[test]
#[ignore = "requires the pinned ExifTool 13.59 combined-samples corpus"]
fn nikon_z8_0x56_corpus_carrier_is_required_and_matches_the_pinned_values() {
    let path = fixtures::required_combined_fixture_path("Nikon/NikonZ8.jpg");
    let metadata = read_metadata(&path).expect("Nikon Z8 parses");
    assert_eq!(
        metadata.get_string("Nikon:FirmwareVersion56"),
        Some("01.00")
    );
    assert_eq!(metadata.get_string("Nikon:PixelShiftActive"), Some("No"));
}
