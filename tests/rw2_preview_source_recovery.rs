//! RW2 JpgFromRaw mutations measured against pinned ExifTool 13.59.
//! The source is its t/images/Panasonic.rw2; these changes touch only the
//! embedded preview TIFF. Oracle receipts live in the RW2 recovery brief.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::TagValue;
use oxidex::parsers::raw::{RawFormat, parse_raw_metadata};

fn source() -> Vec<u8> {
    std::fs::read(fixtures::required_t_images_fixture_path("Panasonic.rw2"))
        .expect("read pinned Panasonic.rw2")
}

fn read_u16(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(data[at..at + 2].try_into().unwrap())
}

fn read_u32(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

fn preview_ifd0(data: &[u8]) -> (usize, usize) {
    let tiff = data
        .windows(6)
        .position(|window| window == b"Exif\0\0")
        .expect("JpgFromRaw EXIF")
        + 6;
    assert_eq!(&data[tiff..tiff + 2], b"II");
    (tiff, tiff + read_u32(data, tiff + 4) as usize)
}

fn entry(data: &[u8], ifd: usize, tag: u16) -> usize {
    (0..usize::from(read_u16(data, ifd)))
        .map(|index| ifd + 2 + index * 12)
        .find(|at| read_u16(data, *at) == tag)
        .unwrap_or_else(|| panic!("missing preview entry {tag:#06x}"))
}

fn parse(data: &[u8]) -> oxidex::core::MetadataMap {
    parse_raw_metadata(data, RawFormat::PanasonicRW2).expect("parse mutated RW2")
}

#[test]
fn pointer_present_keeps_preview_ifd0_makernote_and_interop_reads() {
    let metadata = parse(&source());
    assert_eq!(metadata.get_integer("EXIF:XResolution"), Some(180));
    assert_eq!(metadata.get_string("IFD0:YResolution"), Some("180"));
    assert_eq!(metadata.get_string("IFD0:ResolutionUnit"), Some("inches"));
    assert_eq!(metadata.get_string("IFD0:Software"), Some("Ver.1.0"));
    assert_eq!(
        metadata.get_string("IFD0:ModifyDate"),
        Some("2008:08:06 15:21:56")
    );
    assert_eq!(
        metadata.get_string("IFD0:YCbCrPositioning"),
        Some("Co-sited")
    );
    assert_eq!(
        metadata.get_string("InteropIFD:InteropIndex"),
        Some("R98 - DCF basic file (sRGB)")
    );
    assert_eq!(metadata.get_string("Panasonic:BatteryLevel"), Some("Full"));
}

#[test]
fn preview_ifd0_print_im_and_thumbnail_survive_missing_or_invalid_exif_pointer() {
    let original = source();
    let (_, ifd0) = preview_ifd0(&original);
    let pointer = entry(&original, ifd0, 0x8769);
    assert_eq!(read_u16(&original, pointer + 2), 4);
    assert_eq!(read_u32(&original, pointer + 4), 1);

    for invalid in [false, true] {
        let mut data = original.clone();
        if invalid {
            data[pointer + 8..pointer + 12].copy_from_slice(&u32::MAX.to_le_bytes());
        } else {
            data[pointer..pointer + 2].copy_from_slice(&0xC7FFu16.to_le_bytes());
        }
        let metadata = parse(&data);
        assert_eq!(
            metadata.get_string("IFD0:ModifyDate"),
            Some("2008:08:06 15:21:56"),
            "invalid={invalid}"
        );
        assert_eq!(metadata.get_string("PrintIM:PrintIMVersion"), Some("0250"));
        assert_eq!(metadata.get_integer("IFD1:ThumbnailOffset"), Some(11976));
        assert_eq!(metadata.get_integer("IFD1:ThumbnailLength"), Some(28));
        assert_eq!(
            metadata.get("IFD1:ThumbnailImage"),
            Some(&TagValue::new_binary(
                b"<Dummy thumbnail image data>".to_vec()
            ))
        );
    }
}

#[test]
fn thumbnail_pointer_uses_declared_ifd0_count_when_an_entry_is_unreadable() {
    let mut data = source();
    let (_, ifd0) = preview_ifd0(&data);
    let pointer = entry(&data, ifd0, 0x8769);
    let print_im = entry(&data, ifd0, 0xC4A5);
    assert_eq!(read_u16(&data, print_im + 2), 7);
    data[pointer..pointer + 2].copy_from_slice(&0xC7FFu16.to_le_bytes());
    data[print_im + 2..print_im + 4].copy_from_slice(&0u16.to_le_bytes());

    let metadata = parse(&data);
    assert_eq!(
        metadata.get_string("IFD0:ModifyDate"),
        Some("2008:08:06 15:21:56")
    );
    assert_eq!(metadata.get_integer("IFD1:ThumbnailOffset"), Some(11976));
    assert_eq!(metadata.get_integer("IFD1:ThumbnailLength"), Some(28));
}

#[test]
fn looping_preview_ifd1_pointer_does_not_relabel_ifd0_tags() {
    let mut data = source();
    let (tiff, ifd0) = preview_ifd0(&data);
    let exif_pointer = entry(&data, ifd0, 0x8769);
    let print_im = entry(&data, ifd0, 0xC4A5);
    let count = usize::from(read_u16(&data, ifd0));
    let next_ifd_pointer = ifd0 + 2 + count * 12;
    assert_eq!(read_u32(&data, next_ifd_pointer), 10310);

    // Make IFD0 contain plausible thumbnail entries, then point its own
    // next-IFD link back to itself. Pinned ExifTool 13.59 refuses that
    // already-visited directory rather than calling these IFD1 tags.
    data[exif_pointer..exif_pointer + 2].copy_from_slice(&0x0201u16.to_le_bytes());
    data[print_im..print_im + 2].copy_from_slice(&0x0202u16.to_le_bytes());
    data[print_im + 2..print_im + 4].copy_from_slice(&4u16.to_le_bytes());
    data[print_im + 4..print_im + 8].copy_from_slice(&1u32.to_le_bytes());
    data[print_im + 8..print_im + 12].copy_from_slice(&28u32.to_le_bytes());
    data[next_ifd_pointer..next_ifd_pointer + 4]
        .copy_from_slice(&u32::try_from(ifd0 - tiff).unwrap().to_le_bytes());

    let metadata = parse(&data);
    assert_eq!(
        metadata.get_string("IFD0:ModifyDate"),
        Some("2008:08:06 15:21:56")
    );
    for key in [
        "IFD1:ThumbnailOffset",
        "IFD1:ThumbnailLength",
        "IFD1:ThumbnailImage",
    ] {
        assert!(metadata.get(key).is_none(), "unexpected {key}");
    }
}

#[test]
fn thumbnail_guard_follows_only_exif_pointer_shapes_processed_by_exiftool() {
    let original = source();
    let (_, ifd0) = preview_ifd0(&original);
    let pointer = entry(&original, ifd0, 0x8769);
    let count = usize::from(read_u16(&original, ifd0));
    let ifd1 = read_u32(&original, ifd0 + 2 + count * 12);

    // Each entry's bytes name the real IFD1. Only scalar pointers are
    // processed as ExifIFD by pinned ExifTool 13.59.
    for (field_type, value_count, thumbnail_expected) in [
        (3u16, 1u32, false),
        (3, 2, true),
        (4, 0, true),
        (4, 1, false),
        (4, 2, true),
        (8, 1, false),
        (9, 1, false),
        (9, 2, true),
        (13, 1, false),
        (13, 2, true),
    ] {
        let mut data = original.clone();
        data[pointer + 2..pointer + 4].copy_from_slice(&field_type.to_le_bytes());
        data[pointer + 4..pointer + 8].copy_from_slice(&value_count.to_le_bytes());
        data[pointer + 8..pointer + 12].copy_from_slice(&ifd1.to_le_bytes());
        let metadata = parse(&data);
        assert_eq!(
            metadata.get_integer("IFD1:ThumbnailOffset"),
            thumbnail_expected.then_some(11976),
            "Exif pointer type={field_type} count={value_count}"
        );
    }
}

#[test]
fn thumbnail_guard_uses_first_out_of_line_gps_or_interop_pointer() {
    let original = source();
    let (tiff, ifd0) = preview_ifd0(&original);
    let exif_pointer = entry(&original, ifd0, 0x8769);
    let exif_ifd = tiff + read_u32(&original, exif_pointer + 8) as usize;
    let interop_pointer = entry(&original, exif_ifd, 0xA005);
    let count = usize::from(read_u16(&original, ifd0));
    let array = ifd0 + 2 + count * 12;
    assert_eq!(read_u32(&original, array), 10310);
    let array_offset = u32::try_from(array - tiff).unwrap();

    for (is_gps, pointer) in [(true, exif_pointer), (false, interop_pointer)] {
        for (field_type, value_count) in [(3u16, 3u32), (8, 3), (4, 2), (9, 2), (13, 2)] {
            let mut data = original.clone();
            if is_gps {
                data[pointer..pointer + 2].copy_from_slice(&0x8825u16.to_le_bytes());
            }
            data[pointer + 2..pointer + 4].copy_from_slice(&field_type.to_le_bytes());
            data[pointer + 4..pointer + 8].copy_from_slice(&value_count.to_le_bytes());
            data[pointer + 8..pointer + 12].copy_from_slice(&array_offset.to_le_bytes());
            let metadata = parse(&data);
            assert_eq!(
                metadata.get_integer("IFD1:ThumbnailOffset"),
                None,
                "{} type={field_type} count={value_count}",
                if is_gps { "GPS" } else { "Interop" }
            );
        }
    }
}

#[test]
fn thumbnail_guard_ignores_negative_and_truncated_subdirectory_pointers() {
    let original = source();
    let (_, ifd0) = preview_ifd0(&original);
    let pointer = entry(&original, ifd0, 0x8769);
    for (field_type, value_count, stored) in [
        (8u16, 1u32, 0xffffu32),
        (9, 1, u32::MAX),
        (4, 2, u32::MAX),
        (3, 3, u32::MAX),
    ] {
        let mut data = original.clone();
        data[pointer + 2..pointer + 4].copy_from_slice(&field_type.to_le_bytes());
        data[pointer + 4..pointer + 8].copy_from_slice(&value_count.to_le_bytes());
        data[pointer + 8..pointer + 12].copy_from_slice(&stored.to_le_bytes());
        let metadata = parse(&data);
        assert_eq!(
            metadata.get_integer("IFD1:ThumbnailOffset"),
            Some(11976),
            "type={field_type} count={value_count} stored={stored}"
        );
    }
}

#[test]
fn thumbnail_guard_accepts_byte_subdirectory_pointer_when_ifd1_is_nearby() {
    let original = source();
    let (tiff, ifd0) = preview_ifd0(&original);
    let count = usize::from(read_u16(&original, ifd0));
    let next_pointer = ifd0 + 2 + count * 12;
    let ifd1 = tiff + read_u32(&original, next_pointer) as usize;
    let ifd1_len = 2 + usize::from(read_u16(&original, ifd1)) * 12 + 4;
    let exif_pointer = entry(&original, ifd0, 0x8769);
    let exif_ifd = tiff + read_u32(&original, exif_pointer + 8) as usize;
    let interop_pointer = entry(&original, exif_ifd, 0xA005);

    for (group, pointer) in [
        ("Exif", exif_pointer),
        ("GPS", exif_pointer),
        ("Interop", interop_pointer),
    ] {
        let mut data = original.clone();
        let original_ifd1 = data[ifd1..ifd1 + ifd1_len].to_vec();
        data[tiff + 170..tiff + 170 + ifd1_len].copy_from_slice(&original_ifd1);
        data[next_pointer..next_pointer + 4].copy_from_slice(&170u32.to_le_bytes());
        if group == "GPS" {
            data[pointer..pointer + 2].copy_from_slice(&0x8825u16.to_le_bytes());
        }
        data[pointer + 2..pointer + 4].copy_from_slice(&1u16.to_le_bytes());
        data[pointer + 4..pointer + 8].copy_from_slice(&1u32.to_le_bytes());
        data[pointer + 8..pointer + 12].copy_from_slice(&170u32.to_le_bytes());
        let metadata = parse(&data);
        assert_eq!(
            metadata.get_integer("IFD1:ThumbnailOffset"),
            None,
            "{group}"
        );
    }
}

#[test]
fn thumbnail_guard_accounts_for_gps_and_interop_pointer_shapes() {
    let original = source();
    let (tiff, ifd0) = preview_ifd0(&original);
    let exif_pointer = entry(&original, ifd0, 0x8769);
    let exif_ifd = tiff + read_u32(&original, exif_pointer + 8) as usize;
    let interop_pointer = entry(&original, exif_ifd, 0xA005);
    let count = usize::from(read_u16(&original, ifd0));
    let ifd1 = read_u32(&original, ifd0 + 2 + count * 12);

    for (is_gps, pointer) in [(true, exif_pointer), (false, interop_pointer)] {
        for (field_type, value_count, thumbnail_expected) in [
            (3u16, 1u32, false),
            (3, 2, false),
            (4, 0, true),
            (4, 1, false),
            (4, 2, true),
        ] {
            let mut data = original.clone();
            if is_gps {
                data[pointer..pointer + 2].copy_from_slice(&0x8825u16.to_le_bytes());
            }
            data[pointer + 2..pointer + 4].copy_from_slice(&field_type.to_le_bytes());
            data[pointer + 4..pointer + 8].copy_from_slice(&value_count.to_le_bytes());
            data[pointer + 8..pointer + 12].copy_from_slice(&ifd1.to_le_bytes());
            let metadata = parse(&data);
            assert_eq!(
                metadata.get_integer("IFD1:ThumbnailOffset"),
                thumbnail_expected.then_some(11976),
                "{} pointer type={field_type} count={value_count}",
                if is_gps { "GPS" } else { "Interop" }
            );
        }
    }
}

#[test]
fn preview_value_after_app1_remains_readable_through_jpeg() {
    let mut data = source();
    let (tiff, ifd0) = preview_ifd0(&data);
    let make = entry(&data, ifd0, 0x010f);
    let app1_length = usize::from(u16::from_be_bytes(
        data[tiff - 8..tiff - 6].try_into().unwrap(),
    ));
    let app1_end = tiff - 8 + app1_length;
    let value_at = app1_end + 4;
    assert!(value_at + 10 <= data.len());
    data[make + 8..make + 12]
        .copy_from_slice(&u32::try_from(value_at - tiff).unwrap().to_le_bytes());
    data[value_at..value_at + 10].copy_from_slice(b"Panasonic\0");

    let metadata = parse(&data);
    assert_eq!(metadata.get_string("IFD0:Make"), Some("Panasonic"));
    assert_eq!(
        metadata.get_string("InteropIFD:InteropIndex"),
        Some("R98 - DCF basic file (sRGB)")
    );
    assert_eq!(metadata.get_string("ExifIFD:ColorSpace"), Some("sRGB"));
}

#[test]
fn preview_a411_a412_use_exififd_identity_and_keep_panasonic_noise_reduction() {
    let original = source();
    let (tiff, ifd0) = preview_ifd0(&original);
    let pointer = entry(&original, ifd0, 0x8769);
    let exif_ifd = tiff + read_u32(&original, pointer + 8) as usize;
    let slot = entry(&original, exif_ifd, 0xA405);
    assert_eq!(read_u16(&original, slot + 2), 3);
    assert_eq!(read_u32(&original, slot + 4), 1);
    assert_eq!(read_u16(&original, slot + 8), 24);

    // Exif.pm's Writable int16u is a write format. Its ProcessExif reader
    // accepts every integral wire format, including type 13 (IFD).
    for (id, name) in [(0xA411u16, "ShadingCorrection"), (0xA412, "NoiseReduction")] {
        for field_type in [1u16, 3, 4, 6, 7, 8, 9, 13] {
            for (raw, expected) in [(0u32, "No"), (1, "Yes"), (2, "Unknown (2)")] {
                let mut data = original.clone();
                data[slot..slot + 2].copy_from_slice(&id.to_le_bytes());
                data[slot + 2..slot + 4].copy_from_slice(&field_type.to_le_bytes());
                data[slot + 8..slot + 12].copy_from_slice(&raw.to_le_bytes());
                let metadata = parse(&data);
                assert_eq!(
                    metadata.get_string(&format!("ExifIFD:{name}")),
                    Some(expected),
                    "{name} type={field_type} value={raw}"
                );
                assert_eq!(
                    metadata.get_string("Panasonic:NoiseReduction"),
                    Some("Standard"),
                    "maker-note NoiseReduction must remain distinct"
                );
            }
        }
    }
}

#[test]
fn preview_a411_uses_source_read_formats_beyond_integral_wires() {
    let original = source();
    let (tiff, ifd0) = preview_ifd0(&original);
    let pointer = entry(&original, ifd0, 0x8769);
    let exif_ifd = tiff + read_u32(&original, pointer + 8) as usize;
    let slot = entry(&original, exif_ifd, 0xA405);

    for (field_type, count, inline, expected) in [
        (2u16, 2u32, [b'1', 0, 0, 0], "Yes"),
        (2, 3, [b'0', b'1', 0, 0], "Unknown (01)"),
        (2, 3, [b'+', b'1', 0, 0], "Unknown (+1)"),
        (7, 2, [1, 0, 0, 0], "Unknown (\u{1})"),
        (11, 1, 1f32.to_le_bytes(), "Yes"),
        (129, 2, [b'1', 0, 0, 0], "Unknown (1)"),
        (3, 0, [0, 0, 0, 0], "Unknown ()"),
    ] {
        let mut data = original.clone();
        data[slot..slot + 2].copy_from_slice(&0xA411u16.to_le_bytes());
        data[slot + 2..slot + 4].copy_from_slice(&field_type.to_le_bytes());
        data[slot + 4..slot + 8].copy_from_slice(&count.to_le_bytes());
        data[slot + 8..slot + 12].copy_from_slice(&inline);
        let metadata = parse(&data);
        assert_eq!(
            metadata.get_string("ExifIFD:ShadingCorrection"),
            Some(expected),
            "type={field_type} count={count}"
        );
    }

    for (field_type, payload) in [
        (4u16, vec![1, 0, 0, 0, 0, 0, 0, 0]),
        (5, vec![1, 0, 0, 0, 1, 0, 0, 0]),
        (10, vec![1, 0, 0, 0, 1, 0, 0, 0]),
        (12, 1f64.to_le_bytes().to_vec()),
    ] {
        let mut data = original.clone();
        data[slot..slot + 2].copy_from_slice(&0xA411u16.to_le_bytes());
        data[slot + 2..slot + 4].copy_from_slice(&field_type.to_le_bytes());
        let count = if field_type == 4 { 2u32 } else { 1u32 };
        data[slot + 4..slot + 8].copy_from_slice(&count.to_le_bytes());
        data[slot + 8..slot + 12].copy_from_slice(&9000u32.to_le_bytes());
        data[tiff + 9000..tiff + 9000 + payload.len()].copy_from_slice(&payload);
        let metadata = parse(&data);
        assert_eq!(
            metadata.get_string("ExifIFD:ShadingCorrection"),
            Some(if field_type == 4 {
                "Unknown (1 0)"
            } else {
                "Yes"
            }),
            "type={field_type}"
        );
    }
}

#[test]
fn preview_a411_a412_arrays_use_generated_unknown_fallback() {
    let original = source();
    let (tiff, ifd0) = preview_ifd0(&original);
    let pointer = entry(&original, ifd0, 0x8769);
    let exif_ifd = tiff + read_u32(&original, pointer + 8) as usize;
    let slot = entry(&original, exif_ifd, 0xA405);
    for (id, name) in [(0xA411u16, "ShadingCorrection"), (0xA412, "NoiseReduction")] {
        let mut data = original.clone();
        data[slot..slot + 2].copy_from_slice(&id.to_le_bytes());
        data[slot + 4..slot + 8].copy_from_slice(&2u32.to_le_bytes());
        data[slot + 8..slot + 12].copy_from_slice(&[1, 0, 0, 0]);
        let metadata = parse(&data);
        assert_eq!(
            metadata.get_string(&format!("ExifIFD:{name}")),
            Some("Unknown (1 0)")
        );
    }
}
