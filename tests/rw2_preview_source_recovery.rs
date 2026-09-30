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
fn preview_a411_a412_use_exififd_identity_and_keep_panasonic_noise_reduction() {
    let original = source();
    let (tiff, ifd0) = preview_ifd0(&original);
    let pointer = entry(&original, ifd0, 0x8769);
    let exif_ifd = tiff + read_u32(&original, pointer + 8) as usize;
    let slot = entry(&original, exif_ifd, 0xA405);
    assert_eq!(read_u16(&original, slot + 2), 3);
    assert_eq!(read_u32(&original, slot + 4), 1);
    assert_eq!(read_u16(&original, slot + 8), 24);

    for (id, name) in [(0xA411u16, "ShadingCorrection"), (0xA412, "NoiseReduction")] {
        for (raw, expected) in [(0u16, "No"), (1, "Yes"), (2, "Unknown (2)")] {
            let mut data = original.clone();
            data[slot..slot + 2].copy_from_slice(&id.to_le_bytes());
            data[slot + 8..slot + 10].copy_from_slice(&raw.to_le_bytes());
            let metadata = parse(&data);
            assert_eq!(
                metadata.get_string(&format!("ExifIFD:{name}")),
                Some(expected),
                "{name}={raw}"
            );
            assert_eq!(
                metadata.get_string("Panasonic:NoiseReduction"),
                Some("Standard"),
                "maker-note NoiseReduction must remain distinct"
            );
        }
    }
}
