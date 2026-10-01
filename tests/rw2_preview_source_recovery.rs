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
fn preview_make_after_app1_is_refused_without_losing_other_tags() {
    let mut data = source();
    let (tiff, ifd0) = preview_ifd0(&data);
    let make = entry(&data, ifd0, 0x010f);
    let app1_length = usize::from(u16::from_be_bytes(
        data[tiff - 8..tiff - 6].try_into().unwrap(),
    ));
    let app1_end = tiff - 8 + app1_length;
    let value_at = app1_end + 4;
    assert!(value_at + 11 <= data.len());
    data[make + 4..make + 8].copy_from_slice(&11u32.to_le_bytes());
    data[make + 8..make + 12]
        .copy_from_slice(&u32::try_from(value_at - tiff).unwrap().to_le_bytes());
    data[value_at..value_at + 11].copy_from_slice(b"PreviewFoo\0");

    // Pinned ExifTool warns "Bad offset for IFD0 Make" for the preview.
    // The outer RW2 Make remains Panasonic and other preview reads survive.
    let metadata = parse(&data);
    assert_eq!(metadata.get_string("IFD0:Make"), Some("Panasonic"));
    assert_eq!(
        metadata.get_string("InteropIFD:InteropIndex"),
        Some("R98 - DCF basic file (sRGB)")
    );
    assert_eq!(metadata.get_string("ExifIFD:ColorSpace"), Some("sRGB"));
}

#[test]
fn preview_ifd0_software_after_app1_is_refused() {
    let mut data = source();
    let (tiff, ifd0) = preview_ifd0(&data);
    let software = entry(&data, ifd0, 0x0131);
    let app1_length = usize::from(u16::from_be_bytes(
        data[tiff - 8..tiff - 6].try_into().unwrap(),
    ));
    let app1_end = tiff - 8 + app1_length;
    let value_at = app1_end + 4;
    assert!(value_at + 10 <= data.len());
    data[software + 4..software + 8].copy_from_slice(&10u32.to_le_bytes());
    data[software + 8..software + 12]
        .copy_from_slice(&u32::try_from(value_at - tiff).unwrap().to_le_bytes());
    data[value_at..value_at + 10].copy_from_slice(b"Preview-X\0");

    // Pinned ExifTool warns "Bad offset for IFD0 Software" and reports no
    // preview Software, despite the value existing in the enclosing JPEG.
    assert!(parse(&data).get("IFD0:Software").is_none());
}

#[test]
fn bad_first_preview_ifd0_entry_aborts_later_tags_and_ifd1() {
    let mut data = source();
    let (_, ifd0) = preview_ifd0(&data);
    data[ifd0 + 4..ifd0 + 6].copy_from_slice(&99u16.to_le_bytes());
    let exif_pointer = entry(&data, ifd0, 0x8769);
    data[exif_pointer..exif_pointer + 2].copy_from_slice(&0xC7FFu16.to_le_bytes());

    // Pinned ExifTool warns about IFD0 entry 0 and abandons the preview IFD.
    // The outer RW2 Model is still present, but no later preview value is.
    let metadata = parse(&data);
    assert_eq!(metadata.get_string("IFD0:Model"), Some("DMC-LX3"));
    assert!(metadata.get("IFD0:ResolutionUnit").is_none());
    assert!(metadata.get("IFD1:ThumbnailOffset").is_none());
}

#[test]
fn bad_first_preview_exififd_entry_aborts_later_exif_values() {
    let mut data = source();
    let (tiff, ifd0) = preview_ifd0(&data);
    let exif_pointer = entry(&data, ifd0, 0x8769);
    let exif_ifd = tiff + read_u32(&data, exif_pointer + 8) as usize;
    data[exif_ifd + 4..exif_ifd + 6].copy_from_slice(&99u16.to_le_bytes());

    // The ExifIFD abort does not retroactively discard IFD0 or IFD1.
    let metadata = parse(&data);
    assert_eq!(metadata.get_string("IFD0:ResolutionUnit"), Some("inches"));
    assert_eq!(metadata.get_integer("IFD1:ThumbnailOffset"), Some(11976));
    assert!(metadata.get("ExifIFD:ColorSpace").is_none());
    assert!(metadata.get("Panasonic:BatteryLevel").is_none());
}

#[test]
fn prior_outer_ilce_model_allows_bad_first_preview_entry_and_ifd1() {
    let mut data = source();
    let outer_ifd = read_u32(&data, 4) as usize;
    let model = entry(&data, outer_ifd, 0x0110);
    let jpeg = entry(&data, outer_ifd, 0x002e);
    assert!(model > jpeg);
    assert_eq!(read_u32(&data, model + 4), 8);
    let model_value = read_u32(&data, model + 8) as usize;
    data[model_value..model_value + 8].copy_from_slice(b"ILCE-T\0\0");
    let model_entry = data[model..model + 12].to_vec();
    let jpeg_entry = data[jpeg..jpeg + 12].to_vec();
    data[model..model + 12].copy_from_slice(&jpeg_entry);
    data[jpeg..jpeg + 12].copy_from_slice(&model_entry);
    let (_, ifd0) = preview_ifd0(&data);
    data[ifd0 + 4..ifd0 + 6].copy_from_slice(&99u16.to_le_bytes());

    // Model was known when JpgFromRaw was entered. Exif.pm's Sony ILCE
    // exception skips the bad first entry and continues through IFD1.
    let metadata = parse(&data);
    assert_eq!(metadata.get_string("IFD0:Model"), Some("ILCE-T"));
    assert_eq!(metadata.get_string("IFD0:ResolutionUnit"), Some("inches"));
    assert_eq!(metadata.get_string("ExifIFD:ColorSpace"), Some("sRGB"));
    assert_eq!(metadata.get_integer("IFD1:ThumbnailOffset"), Some(11976));
}

#[test]
fn late_outer_ilce_model_does_not_revive_bad_preview_ifd0() {
    let mut data = source();
    let outer_ifd = read_u32(&data, 4) as usize;
    let model = entry(&data, outer_ifd, 0x0110);
    assert_eq!(read_u32(&data, model + 4), 8);
    let model_value = read_u32(&data, model + 8) as usize;
    data[model_value..model_value + 8].copy_from_slice(b"ILCE-T\0\0");
    let (_, ifd0) = preview_ifd0(&data);
    data[ifd0 + 4..ifd0 + 6].copy_from_slice(&99u16.to_le_bytes());

    let metadata = parse(&data);
    assert_eq!(metadata.get_string("IFD0:Model"), Some("ILCE-T"));
    assert!(metadata.get("IFD0:ResolutionUnit").is_none());
    assert!(metadata.get("IFD1:ThumbnailOffset").is_none());
}

#[test]
fn preview_exif_value_after_app1_is_refused() {
    let mut data = source();
    let (tiff, ifd0) = preview_ifd0(&data);
    let exif_pointer = entry(&data, ifd0, 0x8769);
    let exif_ifd = tiff + read_u32(&data, exif_pointer + 8) as usize;
    let slot = entry(&data, exif_ifd, 0xA405);
    let app1_length = usize::from(u16::from_be_bytes(
        data[tiff - 8..tiff - 6].try_into().unwrap(),
    ));
    let app1_end = tiff - 8 + app1_length;
    let value_at = app1_end + 4;
    assert!(value_at + 6 <= data.len());
    data[slot..slot + 2].copy_from_slice(&0xA411u16.to_le_bytes());
    data[slot + 2..slot + 4].copy_from_slice(&3u16.to_le_bytes());
    data[slot + 4..slot + 8].copy_from_slice(&3u32.to_le_bytes());
    data[slot + 8..slot + 12]
        .copy_from_slice(&u32::try_from(value_at - tiff).unwrap().to_le_bytes());
    data[value_at..value_at + 6].copy_from_slice(&[1, 0, 0, 0, 0, 0]);

    // The pinned native reader warns "Bad offset for ExifIFD
    // ShadingCorrection" and emits no A411. Ordinary preview values stay
    // within the APP1 payload even when the enclosing JPEG has more bytes.
    assert!(parse(&data).get("ExifIFD:ShadingCorrection").is_none());
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
fn preview_ilce_model_keeps_a411_after_bad_first_exif_entry() {
    let mut data = source();
    let (tiff, ifd0) = preview_ifd0(&data);
    let model = entry(&data, ifd0, 0x0110);
    assert_eq!(read_u16(&data, model + 2), 2);
    let model_at = tiff + read_u32(&data, model + 8) as usize;

    let exif_pointer = entry(&data, ifd0, 0x8769);
    let exif_ifd = tiff + read_u32(&data, exif_pointer + 8) as usize;
    data[exif_ifd + 4..exif_ifd + 6].copy_from_slice(&99u16.to_le_bytes());
    let shading = entry(&data, exif_ifd, 0xa405);
    data[shading..shading + 2].copy_from_slice(&0xa411u16.to_le_bytes());
    data[shading + 2..shading + 4].copy_from_slice(&3u16.to_le_bytes());
    data[shading + 4..shading + 8].copy_from_slice(&1u32.to_le_bytes());
    data[shading + 8..shading + 12].copy_from_slice(&1u32.to_le_bytes());

    // Pinned ExifTool stops at the bad first ExifIFD entry for DMC-LX3.
    assert!(parse(&data).get("ExifIFD:ShadingCorrection").is_none());

    // With a previously established ILCE Model, ExifTool skips the bad entry
    // and reports the reached later A411 as Yes.
    data[model + 4..model + 8].copy_from_slice(&7u32.to_le_bytes());
    data[model_at..model_at + 7].copy_from_slice(b"ILCE-T\0");
    let metadata = parse(&data);
    assert_eq!(
        metadata.get_string("ExifIFD:ShadingCorrection"),
        Some("Yes")
    );
}

#[test]
fn preview_model_after_exif_pointer_does_not_enable_ilce_exception() {
    let mut data = source();
    let (tiff, ifd0) = preview_ifd0(&data);
    let model = entry(&data, ifd0, 0x0110);
    let model_at = tiff + read_u32(&data, model + 8) as usize;
    data[model + 4..model + 8].copy_from_slice(&7u32.to_le_bytes());
    data[model_at..model_at + 7].copy_from_slice(b"ILCE-T\0");
    let exif_pointer = entry(&data, ifd0, 0x8769);
    let exif_ifd = tiff + read_u32(&data, exif_pointer + 8) as usize;
    data[exif_ifd + 4..exif_ifd + 6].copy_from_slice(&99u16.to_le_bytes());
    let shading = entry(&data, exif_ifd, 0xA405);
    data[shading..shading + 12].copy_from_slice(&[0x11, 0xA4, 3, 0, 1, 0, 0, 0, 1, 0, 0, 0]);
    // Move Model after ExifOffset without changing its value. ExifTool enters
    // ExifIFD before it learns ILCE-T, stops at its bad first entry, and
    // reports no ShadingCorrection (pinned source-derived RW2 oracle).
    let later = entry(&data, ifd0, 0xC4A5);
    for byte in 0..12 {
        data.swap(model + byte, later + byte);
    }
    assert!(parse(&data).get("ExifIFD:ShadingCorrection").is_none());
}

#[test]
fn preview_exif_pointer_does_not_revisit_prior_gps_directory() {
    let mut data = source();
    let (tiff, ifd0) = preview_ifd0(&data);
    let exif_pointer = entry(&data, ifd0, 0x8769);
    let exif_ifd = tiff + read_u32(&data, exif_pointer + 8) as usize;
    let shading = entry(&data, exif_ifd, 0xA405);
    data[shading..shading + 12].copy_from_slice(&[0x11, 0xA4, 3, 0, 1, 0, 0, 0, 1, 0, 0, 0]);
    // The earlier GPSInfo entry claims this physical directory first.
    let gps = entry(&data, ifd0, 0x0213);
    let pointer_bytes = data[exif_pointer..exif_pointer + 12].to_vec();
    data[gps..gps + 12].copy_from_slice(&pointer_bytes);
    data[gps..gps + 2].copy_from_slice(&0x8825u16.to_le_bytes());
    // ExifTool warns about the repeated directory and emits no ExifIFD A411.
    assert!(parse(&data).get("ExifIFD:ShadingCorrection").is_none());
}

#[test]
fn preview_interop_reached_after_bad_entry_blocks_ifd1_alias() {
    let mut data = source();
    let (tiff, ifd0) = preview_ifd0(&data);
    let model = entry(&data, ifd0, 0x0110);
    let model_at = tiff + read_u32(&data, model + 8) as usize;
    data[model + 4..model + 8].copy_from_slice(&7u32.to_le_bytes());
    data[model_at..model_at + 7].copy_from_slice(b"ILCE-T\0");
    let exif_pointer = entry(&data, ifd0, 0x8769);
    let exif_ifd = tiff + read_u32(&data, exif_pointer + 8) as usize;
    data[exif_ifd + 4..exif_ifd + 6].copy_from_slice(&99u16.to_le_bytes());
    let shading = entry(&data, exif_ifd, 0xA405);
    data[shading..shading + 12].copy_from_slice(&[0x11, 0xA4, 3, 0, 1, 0, 0, 0, 1, 0, 0, 0]);
    let interop = entry(&data, exif_ifd, 0xA005);
    let count = usize::from(read_u16(&data, ifd0));
    let thumbnail_ifd = read_u32(&data, ifd0 + 2 + count * 12);
    data[interop + 8..interop + 12].copy_from_slice(&thumbnail_ifd.to_le_bytes());
    // ExifTool visits this location as InteropIFD, then refuses the IFD1
    // link to the same address. It still reaches A411 after the ILCE skip.
    let metadata = parse(&data);
    assert_eq!(
        metadata.get_string("ExifIFD:ShadingCorrection"),
        Some("Yes")
    );
    assert!(metadata.get("IFD1:ThumbnailOffset").is_none());
    assert!(metadata.get("IFD1:ThumbnailLength").is_none());
}

#[test]
fn preview_ifd0_chain_uses_app1_boundary_not_jpeg_tail() {
    let mut data = source();
    let outer_ifd0 = read_u32(&data, 4) as usize;
    let jpg_from_raw = entry(&data, outer_ifd0, 0x002e);
    assert_eq!(read_u16(&data, jpg_from_raw + 2), 7);

    // This APP1 ends after a complete IFD0 ResolutionUnit entry. The next
    // four JPEG bytes are EOI plus padding: ff d9 00 00. If mistaken for a
    // little-endian IFD link, they point at 0xd9ff, where a directory-shaped
    // trailer deliberately carries thumbnail tags. Pinned ExifTool reads
    // ResolutionUnit but no IFD1 thumbnail from this source-derived RW2.
    let mut tiff = b"II*\0".to_vec();
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x0128u16.to_le_bytes());
    tiff.extend_from_slice(&3u16.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&2u32.to_le_bytes());
    assert_eq!(tiff.len(), 22);
    let mut preview = vec![0xff, 0xd8, 0xff, 0xe1];
    preview.extend_from_slice(&u16::try_from(8 + tiff.len()).unwrap().to_be_bytes());
    preview.extend_from_slice(b"Exif\0\0");
    preview.extend_from_slice(&tiff);
    preview.extend_from_slice(&[0xff, 0xd9, 0, 0]);
    preview.resize(12 + 0xd9ff, 0);
    preview.extend_from_slice(&2u16.to_le_bytes());
    for (tag, value) in [(0x0201u16, 120u32), (0x0202, 4)] {
        preview.extend_from_slice(&tag.to_le_bytes());
        preview.extend_from_slice(&4u16.to_le_bytes());
        preview.extend_from_slice(&1u32.to_le_bytes());
        preview.extend_from_slice(&value.to_le_bytes());
    }
    preview.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(preview.len(), 55849);
    let preview_offset = u32::try_from(data.len()).unwrap();
    data[jpg_from_raw + 4..jpg_from_raw + 8]
        .copy_from_slice(&u32::try_from(preview.len()).unwrap().to_le_bytes());
    data[jpg_from_raw + 8..jpg_from_raw + 12].copy_from_slice(&preview_offset.to_le_bytes());
    data.extend_from_slice(&preview);

    let metadata = parse(&data);
    assert_eq!(metadata.get_string("IFD0:ResolutionUnit"), Some("inches"));
    assert!(metadata.get("IFD1:ThumbnailOffset").is_none());
    assert!(metadata.get("IFD1:ThumbnailLength").is_none());
}

#[test]
fn cyclic_preview_exififd_does_not_relabel_ifd0_shading_correction() {
    let mut data = source();
    let (tiff, ifd0) = preview_ifd0(&data);
    let exif_pointer = entry(&data, ifd0, 0x8769);
    let shading = entry(&data, ifd0, 0xc4a5);
    data[exif_pointer + 8..exif_pointer + 12]
        .copy_from_slice(&u32::try_from(ifd0 - tiff).unwrap().to_le_bytes());
    data[shading..shading + 2].copy_from_slice(&0xa411u16.to_le_bytes());
    data[shading + 2..shading + 4].copy_from_slice(&3u16.to_le_bytes());
    data[shading + 4..shading + 8].copy_from_slice(&1u32.to_le_bytes());
    data[shading + 8..shading + 12].copy_from_slice(&1u32.to_le_bytes());

    // Pinned ExifTool reports IFD0:ShadingCorrection=Yes, warns that the
    // ExifIFD pointer references prior IFD0, and reports no ExifIFD A411.
    let metadata = parse(&data);
    assert!(metadata.get("ExifIFD:ShadingCorrection").is_none());
    assert_eq!(metadata.get_integer("IFD1:ThumbnailOffset"), Some(11976));
}

#[test]
fn footerless_preview_exififd_keeps_reached_shading_correction() {
    let data = source();
    let outer_ifd0 = read_u32(&data, 4) as usize;
    let jpg_from_raw = entry(&data, outer_ifd0, 0x002e);
    assert_eq!(read_u16(&data, jpg_from_raw + 2), 7);

    // The APP1 TIFF ends exactly after A411's one complete entry. JPEG EOI
    // supplies only two bytes beyond it, so there is no four-byte next-IFD
    // footer for parse_ifd to read. Pinned ExifTool 13.59 still reports Yes.
    let mut tiff = b"II*\0".to_vec();
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x8769u16.to_le_bytes());
    tiff.extend_from_slice(&4u16.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&26u32.to_le_bytes());
    tiff.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(tiff.len(), 26);
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0xa411u16.to_le_bytes());
    tiff.extend_from_slice(&3u16.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    assert_eq!(tiff.len(), 40);

    let mut preview = vec![0xff, 0xd8, 0xff, 0xe1];
    preview.extend_from_slice(&u16::try_from(8 + tiff.len()).unwrap().to_be_bytes());
    preview.extend_from_slice(b"Exif\0\0");
    preview.extend_from_slice(&tiff);
    preview.extend_from_slice(&[0xff, 0xd9]);
    assert_eq!(preview.len(), 54);

    for trailer in [false, true] {
        let mut variant = data.clone();
        let mut jpg = preview.clone();
        if trailer {
            // ExifTool still uses the APP1 boundary when JPEG carries an
            // unrelated byte after EOI (pinned source-derived RW2 oracle).
            jpg.push(b'X');
        }
        let preview_offset = u32::try_from(variant.len()).unwrap();
        variant[jpg_from_raw + 4..jpg_from_raw + 8]
            .copy_from_slice(&u32::try_from(jpg.len()).unwrap().to_le_bytes());
        variant[jpg_from_raw + 8..jpg_from_raw + 12].copy_from_slice(&preview_offset.to_le_bytes());
        variant.extend_from_slice(&jpg);

        let metadata = parse(&variant);
        assert_eq!(
            metadata.get_string("ExifIFD:ShadingCorrection"),
            Some("Yes"),
            "trailer={trailer}"
        );
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

#[test]
fn preview_a411_preserves_embedded_nuls_and_zero_denominator_rationals() {
    let original = source();
    let (tiff, ifd0) = preview_ifd0(&original);
    let pointer = entry(&original, ifd0, 0x8769);
    let exif_ifd = tiff + read_u32(&original, pointer + 8) as usize;
    let slot = entry(&original, exif_ifd, 0xA405);

    for (field_type, count, inline, expected) in [
        (7u16, 2u32, [0, 1, 0, 0], "Unknown (\u{1})"),
        (129, 3, [b'1', 0, b'x', 0], "Unknown (1x)"),
    ] {
        let mut data = original.clone();
        data[slot..slot + 2].copy_from_slice(&0xA411u16.to_le_bytes());
        data[slot + 2..slot + 4].copy_from_slice(&field_type.to_le_bytes());
        data[slot + 4..slot + 8].copy_from_slice(&count.to_le_bytes());
        data[slot + 8..slot + 12].copy_from_slice(&inline);
        let metadata = parse(&data);
        assert_eq!(
            metadata.get_string("ExifIFD:ShadingCorrection"),
            Some(expected)
        );
    }

    for (numerator, expected) in [(1u32, "Unknown (inf)"), (0, "Unknown (undef)")] {
        for field_type in [5u16, 10] {
            let mut data = original.clone();
            data[slot..slot + 2].copy_from_slice(&0xA411u16.to_le_bytes());
            data[slot + 2..slot + 4].copy_from_slice(&field_type.to_le_bytes());
            data[slot + 4..slot + 8].copy_from_slice(&1u32.to_le_bytes());
            data[slot + 8..slot + 12].copy_from_slice(&9000u32.to_le_bytes());
            data[tiff + 9000..tiff + 9004].copy_from_slice(&numerator.to_le_bytes());
            data[tiff + 9004..tiff + 9008].copy_from_slice(&0u32.to_le_bytes());
            let metadata = parse(&data);
            assert_eq!(
                metadata.get_string("ExifIFD:ShadingCorrection"),
                Some(expected),
                "type={field_type} numerator={numerator}"
            );
        }
    }
}
