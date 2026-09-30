//! Source-backed Leica5 white-balance levels and their two EXIF composites.

use oxidex::core::operations::read_metadata;
use oxidex::core::tag_occurrence::ValueChannel;
use oxidex::core::{TagId, TagValue};
use oxidex::parsers::raw::{RawFormat, parse_raw_metadata};
use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernote_dispatcher::dispatch_makernote;
use oxidex::parsers::tiff::makernotes::leica::LeicaMakerNoteParser;
use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
use std::collections::HashMap;
use std::fs;
use std::process::Command;

#[path = "common/fixtures.rs"]
mod fixtures;

#[test]
#[ignore = "requires pinned combined-samples/Leica/LeicaX1.jpg"]
fn leica_x1_levels_and_balances_match_pinned_1359() {
    let path = fixtures::required_combined_fixture_path("Leica/LeicaX1.jpg");
    let metadata = read_metadata(&path).expect("LeicaX1.jpg parses");
    assert_eq!(
        metadata.get_string("Leica:WB_RGBLevels"),
        Some("0.5182186235 1 0.7231638418")
    );
    let rows: Vec<_> = metadata
        .project_occurrences(ValueChannel::PrintConv)
        .filter(|(key, _, _)| *key == "Leica:WB_RGBLevels")
        .collect();
    assert_eq!(rows.len(), 1);
    let (_, occurrence, printed) = &rows[0];
    assert_eq!(occurrence.id, TagId::Numeric(0x0413));
    assert_eq!(occurrence.group0.as_ref(), "MakerNotes");
    assert_eq!(occurrence.group1.as_ref(), "Leica");
    assert_eq!(occurrence.group2.as_deref(), Some("Camera"));
    assert_eq!(occurrence.origin.module, Some("Panasonic"));
    assert_eq!(occurrence.origin.table, Some("Leica5"));
    assert!(occurrence.origin.byte_range.is_some());
    assert_eq!(
        printed.as_ref(),
        &TagValue::String("0.5182186235 1 0.7231638418".into())
    );
    assert_eq!(
        occurrence.value,
        Some(TagValue::String("0.5182186235 1 0.7231638418".into()))
    );
    assert_eq!(
        occurrence.stored,
        Some(TagValue::Array(vec![
            TagValue::Rational {
                numerator: 256,
                denominator: 494
            },
            TagValue::Rational {
                numerator: 512,
                denominator: 512
            },
            TagValue::Rational {
                numerator: 256,
                denominator: 354
            },
        ]))
    );
    assert_eq!(
        metadata.get_string("Composite:RedBalance"),
        Some("0.518219")
    );
    assert_eq!(
        metadata.get_string("Composite:BlueBalance"),
        Some("0.723164")
    );
}

#[test]
#[ignore = "requires pinned combined-samples/Leica/LeicaX1.jpg"]
fn leica_x1_zero_denominators_match_pinned_1359() {
    let path = fixtures::required_combined_fixture_path("Leica/LeicaX1.jpg");
    let original = fs::read(path).expect("read LeicaX1.jpg");
    // The `-v3` pinned 13.59 oracle locates 0x0413's 24 value bytes here.
    const VALUES: usize = 0x043e;
    assert_eq!(
        &original[VALUES..VALUES + 24],
        &[
            0, 1, 0, 0, 238, 1, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 98, 1, 0, 0,
        ]
    );
    for (which, edit, expected_levels, expected_red, expected_blue) in [
        (
            "middle denominator zero",
            VALUES + 12..VALUES + 16,
            "0.5182186235 inf 0.7231638418",
            Some("0"),
            Some("0"),
        ),
        (
            "first numerator and denominator zero",
            VALUES..VALUES + 8,
            "undef 1 0.7231638418",
            Some("0"),
            Some("0.723164"),
        ),
        (
            "red denominator zero",
            VALUES + 4..VALUES + 8,
            "inf 1 0.7231638418",
            Some("Inf"),
            Some("0.723164"),
        ),
        (
            "blue denominator zero",
            VALUES + 20..VALUES + 24,
            "0.5182186235 1 inf",
            Some("0.518219"),
            Some("Inf"),
        ),
    ] {
        let mut data = original.clone();
        data[edit].fill(0);
        let file = tempfile::Builder::new()
            .suffix(".jpg")
            .tempfile()
            .expect("temporary JPEG");
        fs::write(file.path(), data).expect("write mutated JPEG");
        let metadata = read_metadata(file.path()).expect(which);
        assert_eq!(
            metadata.get_string("Leica:WB_RGBLevels"),
            Some(expected_levels),
            "{which}"
        );
        assert_eq!(
            metadata.get_string("Composite:RedBalance"),
            expected_red,
            "{which}"
        );
        assert_eq!(
            metadata.get_string("Composite:BlueBalance"),
            expected_blue,
            "{which}"
        );
    }
}

#[test]
#[ignore = "requires pinned combined-samples/Leica/LeicaX1.jpg"]
fn leica_x1_overlapping_wb_offset_is_not_reported() {
    let path = fixtures::required_combined_fixture_path("Leica/LeicaX1.jpg");
    let mut bytes = fs::read(path).unwrap();
    assert_eq!(
        &bytes[0x3b2..0x3be],
        &[0x13, 0x04, 5, 0, 3, 0, 0, 0, 0x86, 1, 0, 0]
    );
    bytes[0x3ba..0x3be].copy_from_slice(&8u32.to_le_bytes());
    let file = tempfile::Builder::new().suffix(".jpg").tempfile().unwrap();
    fs::write(file.path(), bytes).unwrap();
    let metadata = read_metadata(file.path()).unwrap();
    assert!(!metadata.contains_key("Leica:WB_RGBLevels"));
    assert!(!metadata.contains_key("Composite:RedBalance"));
    assert!(!metadata.contains_key("Composite:BlueBalance"));
}

// A minimal TIFF/DNG with a physically located Leica MakerNote. Leica5's
// 0x0413 offset is payload-relative; Leica8's is TIFF-relative and outside
// the declared MakerNote. The same bytes exercise both routing contracts.
fn leica_dng(layout: u8, wb_offset: u32) -> Vec<u8> {
    let note_at = 128usize;
    let value_at = if layout == 0x06 {
        note_at + wb_offset as usize
    } else {
        wb_offset as usize
    };
    let note_len = if layout == 0x06 { 50u32 } else { 26u32 };
    let mut bytes = vec![0u8; 256];
    bytes[..8].copy_from_slice(b"II\x2a\0\x08\0\0\0");
    bytes[8..10].copy_from_slice(&3u16.to_le_bytes());
    let entry = |bytes: &mut [u8], at: usize, id: u16, kind: u16, count: u32, value: u32| {
        bytes[at..at + 2].copy_from_slice(&id.to_le_bytes());
        bytes[at + 2..at + 4].copy_from_slice(&kind.to_le_bytes());
        bytes[at + 4..at + 8].copy_from_slice(&count.to_le_bytes());
        bytes[at + 8..at + 12].copy_from_slice(&value.to_le_bytes());
    };
    entry(&mut bytes, 10, 0x010f, 2, 16, 64);
    entry(&mut bytes, 22, 0x8769, 4, 1, 96);
    entry(&mut bytes, 34, 0xc612, 1, 4, 0x0000_0401);
    bytes[64..80].copy_from_slice(b"Leica Camera AG\0");
    bytes[96..98].copy_from_slice(&1u16.to_le_bytes());
    entry(&mut bytes, 98, 0x927c, 7, note_len, note_at as u32);
    bytes[note_at..note_at + 8].copy_from_slice(&[b'L', b'E', b'I', b'C', b'A', 0, layout, 0]);
    bytes[note_at + 8..note_at + 10].copy_from_slice(&1u16.to_le_bytes());
    entry(&mut bytes, note_at + 10, 0x0413, 5, 3, wb_offset);
    if value_at >= note_at + 26 {
        for (index, (n, d)) in [(256u32, 494u32), (512, 512), (256, 354)]
            .into_iter()
            .enumerate()
        {
            bytes[value_at + index * 8..value_at + index * 8 + 4].copy_from_slice(&n.to_le_bytes());
            bytes[value_at + index * 8 + 4..value_at + index * 8 + 8]
                .copy_from_slice(&d.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn leica_makernote_entry_array_boundary_is_read_in_raw_tiff_and_jpeg() {
    for note_at in [110usize, 111, 113, 114, 128] {
        for extension in ["dng", "tiff", "jpg"] {
            let mut bytes = leica_dng(0x08, 200);
            let note = bytes[128..154].to_vec();
            bytes[110..154].fill(0);
            bytes[note_at..note_at + note.len()].copy_from_slice(&note);
            bytes[106..110].copy_from_slice(&(note_at as u32).to_le_bytes());
            if extension != "dng" {
                // Remove DNGVersion while retaining the physical IFD layout.
                bytes[34..36].copy_from_slice(&0x0100u16.to_le_bytes());
            }
            if extension == "jpg" {
                let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
                jpeg.extend_from_slice(&((bytes.len() + 8) as u16).to_be_bytes());
                jpeg.extend_from_slice(b"Exif\0\0");
                jpeg.extend_from_slice(&bytes);
                jpeg.extend_from_slice(&[0xff, 0xd9]);
                bytes = jpeg;
            }
            let file = tempfile::Builder::new()
                .suffix(&format!(".{extension}"))
                .tempfile()
                .unwrap();
            fs::write(file.path(), bytes).unwrap();
            let metadata = read_metadata(file.path()).unwrap();
            assert_eq!(
                metadata.get_string("Leica:WB_RGBLevels"),
                Some("0.5182186235 1 0.7231638418"),
                "{extension}: MakerNote starts at {note_at}"
            );
            assert_eq!(
                metadata.get_string("Composite:RedBalance"),
                Some("0.518219")
            );
            assert_eq!(
                metadata.get_string("Composite:BlueBalance"),
                Some("0.723164")
            );
        }
    }
}

#[test]
fn leica_dng_located_levels_have_public_and_typed_values() {
    for (layout, wb_offset) in [(0x06, 26u32), (0x08, 200u32)] {
        let bytes = leica_dng(layout, wb_offset);
        let raw = parse_raw_metadata(&bytes, RawFormat::AdobeDNG).expect("RAW reader");
        let file = tempfile::Builder::new().suffix(".dng").tempfile().unwrap();
        fs::write(file.path(), bytes).unwrap();
        let public = read_metadata(file.path()).expect("public DNG reader");
        for metadata in [&raw, &public] {
            assert_eq!(
                metadata.get_string("Leica:WB_RGBLevels"),
                Some("0.5182186235 1 0.7231638418"),
                "layout {layout:#x}"
            );
            let rows: Vec<_> = metadata
                .project_occurrences(ValueChannel::PrintConv)
                .filter(|(key, _, _)| *key == "Leica:WB_RGBLevels")
                .collect();
            assert_eq!(rows.len(), 1, "layout {layout:#x}");
            assert_eq!(rows[0].1.id, TagId::Numeric(0x0413));
            assert_eq!(rows[0].1.group1.as_ref(), "Leica");
            assert_eq!(rows[0].1.origin.module, Some("Panasonic"));
            assert_eq!(rows[0].1.origin.table, Some("Leica5"));
            assert_eq!(
                rows[0].1.origin.byte_range,
                Some(
                    (value_at_for(layout, wb_offset) as u64)
                        ..(value_at_for(layout, wb_offset) as u64 + 24)
                )
            );
            assert_eq!(
                rows[0].1.stored,
                Some(TagValue::Array(vec![
                    TagValue::Rational {
                        numerator: 256,
                        denominator: 494
                    },
                    TagValue::Rational {
                        numerator: 512,
                        denominator: 512
                    },
                    TagValue::Rational {
                        numerator: 256,
                        denominator: 354
                    },
                ]))
            );
        }
        assert_eq!(public.get_string("Composite:RedBalance"), Some("0.518219"));
        assert_eq!(public.get_string("Composite:BlueBalance"), Some("0.723164"));
    }
}

fn value_at_for(layout: u8, offset: u32) -> usize {
    if layout == 0x06 {
        128 + offset as usize
    } else {
        offset as usize
    }
}

#[test]
fn leica_dng_rejects_wb_offsets_into_declaring_directory() {
    for (layout, offset) in [(0x06, 8u32), (0x08, 136u32)] {
        let bytes = leica_dng(layout, offset);
        let metadata = parse_raw_metadata(&bytes, RawFormat::AdobeDNG).expect("RAW reader");
        assert!(
            !metadata.contains_key("Leica:WB_RGBLevels"),
            "layout {layout:#x}"
        );
        assert!(!metadata.contains_key("Composite:RedBalance"));
        assert!(!metadata.contains_key("Composite:BlueBalance"));
        let file = tempfile::Builder::new().suffix(".dng").tempfile().unwrap();
        fs::write(file.path(), bytes).unwrap();
        let public = read_metadata(file.path()).expect("public DNG reader");
        assert!(!public.contains_key("Leica:WB_RGBLevels"));
        assert!(!public.contains_key("Composite:RedBalance"));
        assert!(!public.contains_key("Composite:BlueBalance"));
    }
}

#[test]
fn leica_dng_preserves_infinite_red_and_blue_balances() {
    for (which, denominator, expected_red, expected_blue) in [
        ("red", 4usize, "Inf", "0.723164"),
        ("blue", 20usize, "0.518219", "Inf"),
    ] {
        let mut bytes = leica_dng(0x08, 200);
        bytes[200 + denominator..200 + denominator + 4].fill(0);
        let file = tempfile::Builder::new().suffix(".dng").tempfile().unwrap();
        fs::write(file.path(), bytes).unwrap();
        let metadata = read_metadata(file.path()).expect(which);
        assert_eq!(
            metadata.get_string("Composite:RedBalance"),
            Some(expected_red)
        );
        assert_eq!(
            metadata.get_string("Composite:BlueBalance"),
            Some(expected_blue)
        );
    }
    let mut bytes = leica_dng(0x08, 200);
    bytes[204..208].fill(0);
    bytes[212..216].fill(0);
    let file = tempfile::Builder::new().suffix(".dng").tempfile().unwrap();
    fs::write(file.path(), bytes).unwrap();
    let metadata = read_metadata(file.path()).expect("red and green infinity");
    assert_eq!(metadata.get_string("Composite:RedBalance"), Some("NaN"));
    assert_eq!(metadata.get_string("Composite:BlueBalance"), Some("0"));
}

#[test]
fn leica_dng_reads_long_wb_levels_with_stored_type() {
    let mut bytes = leica_dng(0x08, 200);
    bytes[140..142].copy_from_slice(&4u16.to_le_bytes());
    for (index, value) in [256u32, 512, 128].into_iter().enumerate() {
        bytes[200 + index * 4..204 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    let metadata = parse_raw_metadata(&bytes, RawFormat::AdobeDNG).unwrap();
    assert_eq!(
        metadata.get_string("Leica:WB_RGBLevels"),
        Some("256 512 128")
    );
    let occurrence = metadata
        .project_occurrences(ValueChannel::PrintConv)
        .find(|(key, _, _)| *key == "Leica:WB_RGBLevels")
        .unwrap()
        .1;
    assert_eq!(
        occurrence.stored,
        Some(TagValue::Array(vec![
            TagValue::Integer(256),
            TagValue::Integer(512),
            TagValue::Integer(128),
        ]))
    );
    assert_eq!(occurrence.origin.byte_range, Some(200..212));
    assert_eq!(occurrence.priority, 0);
    let file = tempfile::Builder::new().suffix(".dng").tempfile().unwrap();
    fs::write(file.path(), bytes).unwrap();
    let public = read_metadata(file.path()).unwrap();
    assert_eq!(public.get_string("Composite:RedBalance"), Some("0.5"));
    assert_eq!(public.get_string("Composite:BlueBalance"), Some("0.25"));
}

#[test]
fn leica_dng_reads_inline_byte_wb_levels_with_exact_origin() {
    let mut bytes = leica_dng(0x08, 200);
    bytes[140..142].copy_from_slice(&1u16.to_le_bytes());
    bytes[146..150].copy_from_slice(&[2, 4, 1, 0]);
    let metadata = parse_raw_metadata(&bytes, RawFormat::AdobeDNG).unwrap();
    assert_eq!(metadata.get_string("Leica:WB_RGBLevels"), Some("2 4 1"));
    let occurrence = metadata
        .project_occurrences(ValueChannel::PrintConv)
        .find(|(key, _, _)| *key == "Leica:WB_RGBLevels")
        .unwrap()
        .1;
    assert_eq!(occurrence.origin.byte_range, Some(146..149));
    assert_eq!(occurrence.priority, 0);
    assert_eq!(
        occurrence.stored,
        Some(TagValue::Array(vec![
            TagValue::Integer(2),
            TagValue::Integer(4),
            TagValue::Integer(1),
        ]))
    );
}

#[test]
fn detached_leica8_reads_inline_wb_but_not_tiff_relative_values() {
    let mut bytes = leica_dng(0x08, 200);
    set_leica_entry(&mut bytes, 0x0413, 1, 3, 0x0001_0402);
    let mut tags = HashMap::new();
    dispatch_makernote(
        "Leica Camera AG",
        &bytes[128..154],
        ByteOrder::LittleEndian,
        &mut tags,
    )
    .unwrap();
    assert_eq!(
        tags.get("Leica:WB_RGBLevels").map(String::as_str),
        Some("2 4 1")
    );

    // The public parser trait also accepts a standalone MakerNote payload.
    tags.clear();
    LeicaMakerNoteParser
        .parse(&bytes[128..154], ByteOrder::LittleEndian, &mut tags)
        .unwrap();
    assert_eq!(
        tags.get("Leica:WB_RGBLevels").map(String::as_str),
        Some("2 4 1")
    );

    for (field_type, count, value, expected) in [
        (5, 0, 200, ""),
        (4, 1, 256, "256"),
        (2, 3, 0x0032_2031, "1 2"),
    ] {
        set_leica_entry(&mut bytes, 0x0413, field_type, count, value);
        tags.clear();
        LeicaMakerNoteParser
            .parse(&bytes[128..154], ByteOrder::LittleEndian, &mut tags)
            .unwrap();
        assert_eq!(
            tags.get("Leica:WB_RGBLevels").map(String::as_str),
            Some(expected),
            "type {field_type}, count {count}"
        );
    }

    set_leica_entry(&mut bytes, 0x0413, 5, 3, 200);
    tags.clear();
    dispatch_makernote(
        "Leica Camera AG",
        &bytes[128..154],
        ByteOrder::LittleEndian,
        &mut tags,
    )
    .unwrap();
    assert!(!tags.contains_key("Leica:WB_RGBLevels"));
    LeicaMakerNoteParser
        .parse(&bytes[128..154], ByteOrder::LittleEndian, &mut tags)
        .unwrap();
    assert!(!tags.contains_key("Leica:WB_RGBLevels"));
}

#[test]
fn leica_dng_rejects_stored_offsets_into_tiff_header() {
    for offset in 0..8 {
        let bytes = leica_dng(0x08, offset);
        let metadata = parse_raw_metadata(&bytes, RawFormat::AdobeDNG).unwrap();
        assert!(
            !metadata.contains_key("Leica:WB_RGBLevels"),
            "offset {offset}"
        );
        assert!(
            !metadata.contains_key("Composite:RedBalance"),
            "offset {offset}"
        );
        assert!(
            !metadata.contains_key("Composite:BlueBalance"),
            "offset {offset}"
        );
    }
}

#[test]
fn leica5_wb_duplicate_keeps_first_priority_zero_entry() {
    let mut bytes = leica_dng(0x08, 200);
    bytes[102..106].copy_from_slice(&38u32.to_le_bytes());
    bytes[136..138].copy_from_slice(&2u16.to_le_bytes());
    bytes[150..152].copy_from_slice(&0x0413u16.to_le_bytes());
    bytes[152..154].copy_from_slice(&5u16.to_le_bytes());
    bytes[154..158].copy_from_slice(&3u32.to_le_bytes());
    bytes[158..162].copy_from_slice(&224u32.to_le_bytes());
    for (index, (num, den)) in [(128u32, 256u32), (1, 1), (128, 256)]
        .into_iter()
        .enumerate()
    {
        let at = 224 + index * 8;
        bytes[at..at + 4].copy_from_slice(&num.to_le_bytes());
        bytes[at + 4..at + 8].copy_from_slice(&den.to_le_bytes());
    }
    let metadata = parse_raw_metadata(&bytes, RawFormat::AdobeDNG).unwrap();
    let rows: Vec<_> = metadata
        .project_occurrences(ValueChannel::PrintConv)
        .filter(|(key, _, _)| *key == "Leica:WB_RGBLevels")
        .collect();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|(_, occurrence, _)| occurrence.priority == 0)
    );
    assert_eq!(
        metadata.get_string("Leica:WB_RGBLevels"),
        Some("0.5182186235 1 0.7231638418")
    );
    let file = tempfile::Builder::new().suffix(".dng").tempfile().unwrap();
    fs::write(file.path(), bytes).unwrap();
    let public = read_metadata(file.path()).unwrap();
    assert_eq!(public.get_string("Composite:RedBalance"), Some("0.518219"));
    assert_eq!(public.get_string("Composite:BlueBalance"), Some("0.723164"));
}

#[test]
fn leica_dng_double_balance_keeps_finite_value_on_print_overflow() {
    let mut bytes = leica_dng(0x08, 200);
    bytes[140..142].copy_from_slice(&12u16.to_le_bytes());
    for (index, value) in [1e308f64, 1.0, 1.0].into_iter().enumerate() {
        bytes[200 + index * 8..208 + index * 8].copy_from_slice(&value.to_le_bytes());
    }
    let file = tempfile::Builder::new().suffix(".dng").tempfile().unwrap();
    fs::write(file.path(), bytes).unwrap();
    let metadata = read_metadata(file.path()).unwrap();
    assert_eq!(
        metadata.get_string("Leica:WB_RGBLevels"),
        Some("1e+308 1 1")
    );
    assert_eq!(metadata.get_string("Composite:RedBalance"), Some("Inf"));
    assert_eq!(metadata.get_string("Composite:BlueBalance"), Some("1"));
}

#[test]
fn leica_dng_signed_rational_retains_negative_zero() {
    let mut bytes = leica_dng(0x08, 200);
    bytes[140..142].copy_from_slice(&10u16.to_le_bytes());
    for (index, (numerator, denominator)) in [(0i32, -1i32), (1, 1), (1, 4)].into_iter().enumerate()
    {
        let at = 200 + index * 8;
        bytes[at..at + 4].copy_from_slice(&numerator.to_le_bytes());
        bytes[at + 4..at + 8].copy_from_slice(&denominator.to_le_bytes());
    }
    let raw = parse_raw_metadata(&bytes, RawFormat::AdobeDNG).unwrap();
    assert_eq!(raw.get_string("Leica:WB_RGBLevels"), Some("-0 1 0.25"));
    let occurrence = raw
        .project_occurrences(ValueChannel::PrintConv)
        .find(|(key, _, _)| *key == "Leica:WB_RGBLevels")
        .unwrap()
        .1;
    assert_eq!(
        occurrence.stored,
        Some(TagValue::Array(vec![
            TagValue::Rational {
                numerator: 0,
                denominator: -1
            },
            TagValue::Rational {
                numerator: 1,
                denominator: 1
            },
            TagValue::Rational {
                numerator: 1,
                denominator: 4
            },
        ]))
    );
}

fn set_leica_entry(bytes: &mut [u8], tag: u16, field_type: u16, count: u32, value: u32) {
    bytes[138..140].copy_from_slice(&tag.to_le_bytes());
    bytes[140..142].copy_from_slice(&field_type.to_le_bytes());
    bytes[142..146].copy_from_slice(&count.to_le_bytes());
    bytes[146..150].copy_from_slice(&value.to_le_bytes());
}

fn leica_dng_with_prefix(layout: u8, prefix: &[(u16, u16, u32, u32)]) -> Vec<u8> {
    let mut bytes = leica_dng(layout, 64);
    bytes.resize(512, 0);
    let entries = prefix.len() + 1;
    let note_len = 8 + 2 + 12 * entries + 4;
    bytes[102..106].copy_from_slice(&(note_len as u32).to_le_bytes());
    bytes[136..138].copy_from_slice(&(entries as u16).to_le_bytes());
    for (index, &(tag, field_type, count, value)) in prefix.iter().enumerate() {
        let at = 138 + 12 * index;
        bytes[at..at + 12].copy_from_slice(
            &[
                tag.to_le_bytes().as_slice(),
                field_type.to_le_bytes().as_slice(),
                count.to_le_bytes().as_slice(),
                value.to_le_bytes().as_slice(),
            ]
            .concat(),
        );
    }
    let at = 138 + 12 * prefix.len();
    bytes[at..at + 12].copy_from_slice(
        &[
            0x0413u16.to_le_bytes().as_slice(),
            1u16.to_le_bytes().as_slice(),
            3u32.to_le_bytes().as_slice(),
            0x0001_0402u32.to_le_bytes().as_slice(),
        ]
        .concat(),
    );
    bytes
}

#[test]
fn leica_wb_respects_process_exif_directory_refusal_budget() {
    for layout in [0x06, 0x08] {
        let cases = vec![
            ("first format 14", vec![(0x0305, 14, 1, 123456)], false),
            ("first format zero", vec![(0x0305, 0, 1, 123456)], false),
            ("first accepted", vec![(0x03ff, 1, 1, 1)], true),
            (
                "ten bad formats",
                std::iter::once((0x03ff, 1, 1, 1))
                    .chain((0..10).map(|i| (0x0500 + i, 14, 1, 1)))
                    .collect(),
                true,
            ),
            (
                "eleven bad formats",
                std::iter::once((0x03ff, 1, 1, 1))
                    .chain((0..11).map(|i| (0x0500 + i, 14, 1, 1)))
                    .collect(),
                false,
            ),
            (
                "eleven zero formats",
                std::iter::once((0x03ff, 1, 1, 1))
                    .chain((0..11).map(|i| (0x0500 + i, 0, 1, 1)))
                    .collect(),
                true,
            ),
            (
                "ten bad offsets",
                std::iter::once((0x03ff, 1, 1, 1))
                    .chain((0..10).map(|i| (0x0500 + i, 2, 8, 0)))
                    .collect(),
                true,
            ),
            (
                "eleven bad offsets",
                std::iter::once((0x03ff, 1, 1, 1))
                    .chain((0..11).map(|i| (0x0500 + i, 2, 8, 0)))
                    .collect(),
                false,
            ),
        ];
        for (case, prefix, expected) in cases {
            let bytes = leica_dng_with_prefix(layout, &prefix);
            let file = tempfile::Builder::new().suffix(".dng").tempfile().unwrap();
            fs::write(file.path(), bytes).unwrap();
            let metadata = read_metadata(file.path()).unwrap();
            assert_eq!(
                metadata.contains_key("Leica:WB_RGBLevels"),
                expected,
                "layout {layout}: {case}"
            );
            assert_eq!(
                metadata.contains_key("Composite:RedBalance"),
                expected,
                "layout {layout}: {case}"
            );
            assert_eq!(
                metadata.contains_key("Composite:BlueBalance"),
                expected,
                "layout {layout}: {case}"
            );
        }

        // Exif.pm exempts a first bad entry when Model already starts ILCE.
        // Put Model before ExifIFD so the source has seen it on entry.
        let mut bytes = leica_dng_with_prefix(layout, &[(0x0305, 14, 1, 123456)]);
        bytes[8..10].copy_from_slice(&4u16.to_le_bytes());
        bytes[22..34].copy_from_slice(
            &[
                0x0110u16.to_le_bytes().as_slice(),
                2u16.to_le_bytes().as_slice(),
                5u32.to_le_bytes().as_slice(),
                80u32.to_le_bytes().as_slice(),
            ]
            .concat(),
        );
        bytes[34..46].copy_from_slice(
            &[
                0x8769u16.to_le_bytes().as_slice(),
                4u16.to_le_bytes().as_slice(),
                1u32.to_le_bytes().as_slice(),
                96u32.to_le_bytes().as_slice(),
            ]
            .concat(),
        );
        bytes[46..58].copy_from_slice(
            &[
                0xc612u16.to_le_bytes().as_slice(),
                1u16.to_le_bytes().as_slice(),
                4u32.to_le_bytes().as_slice(),
                0x0000_0401u32.to_le_bytes().as_slice(),
            ]
            .concat(),
        );
        bytes[80..85].copy_from_slice(b"ILCE\0");
        let file = tempfile::Builder::new().suffix(".dng").tempfile().unwrap();
        fs::write(file.path(), bytes).unwrap();
        let metadata = read_metadata(file.path()).unwrap();
        assert!(
            metadata.contains_key("Leica:WB_RGBLevels"),
            "layout {layout}: ILCE exception"
        );
    }
}

#[test]
fn leica_wb_zero_count_and_singletons_keep_readvalue_shapes() {
    for (field_type, count, value, printed, stored) in [
        (5, 0, 200, "", TagValue::String(String::new())),
        (4, 1, 256, "256", TagValue::Integer(256)),
        (
            5,
            1,
            200,
            "0.5182186235",
            TagValue::Rational {
                numerator: 256,
                denominator: 494,
            },
        ),
    ] {
        let mut bytes = leica_dng(0x08, 200);
        set_leica_entry(&mut bytes, 0x0413, field_type, count, value);
        let metadata = parse_raw_metadata(&bytes, RawFormat::AdobeDNG).unwrap();
        assert_eq!(metadata.get_string("Leica:WB_RGBLevels"), Some(printed));
        let row = metadata
            .project_occurrences(ValueChannel::PrintConv)
            .find(|(key, _, _)| *key == "Leica:WB_RGBLevels")
            .unwrap()
            .1;
        assert_eq!(row.stored, Some(stored));
    }
}

#[test]
fn leica_wb_string_and_undefined_fields_keep_perl_composite_input() {
    let cases = [
        (2u16, &b"1 2 3\0"[..], "1 2 3", Some("0.5"), Some("1.5")),
        (7, &b"1 2 3\0"[..], "1 2 3", Some("0.5"), Some("1.5")),
        (129, &b"\xff2 3\0?"[..], "?2 3?", None, None),
        (129, &b"1 2\0 3\0"[..], "1 2 3", Some("0.5"), Some("1.5")),
        (
            7,
            &b"1\x002 3 4"[..],
            "12 3 4",
            Some("0.333333"),
            Some("1.333333"),
        ),
        (
            129,
            &b"1\x002 3 4"[..],
            "12 3 4",
            Some("0.333333"),
            Some("1.333333"),
        ),
    ];
    for (field_type, payload, printed, red, blue) in cases {
        let mut bytes = leica_dng(0x08, 200);
        set_leica_entry(&mut bytes, 0x0413, field_type, payload.len() as u32, 200);
        bytes[200..200 + payload.len()].copy_from_slice(payload);
        let file = tempfile::Builder::new().suffix(".dng").tempfile().unwrap();
        fs::write(file.path(), bytes).unwrap();
        let metadata = read_metadata(file.path()).unwrap();
        if field_type == 129 && oxidex::exiftool_oracle::repo_pin() == "11.78" {
            assert!(!metadata.contains_key("Leica:WB_RGBLevels"));
            assert!(!metadata.contains_key("Composite:RedBalance"));
            assert!(!metadata.contains_key("Composite:BlueBalance"));
            for numeric in [false, true] {
                let mut command = Command::new(env!("CARGO_BIN_EXE_oxidex"));
                command.arg("-j");
                if numeric {
                    command.arg("--no-print-conv");
                }
                let output = command.arg(file.path()).output().unwrap();
                assert!(output.status.success());
                let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                assert!(json[0].get("Leica:WB_RGBLevels").is_none());
                assert!(json[0].get("Composite:RedBalance").is_none());
                assert!(json[0].get("Composite:BlueBalance").is_none());
            }
            continue;
        }
        assert_eq!(metadata.get_string("Leica:WB_RGBLevels"), Some(printed));
        assert_eq!(
            metadata
                .iter()
                .find(|(key, _)| key.as_str() == "Leica:WB_RGBLevels")
                .and_then(|(_, value)| value.as_string()),
            Some(printed)
        );
        let public = metadata
            .project_occurrences(ValueChannel::PrintConv)
            .find(|(key, _, _)| *key == "Leica:WB_RGBLevels")
            .and_then(|(_, _, value)| value.as_string().map(str::to_owned));
        assert_eq!(
            public.map(|text| text.replace('\0', "")).as_deref(),
            Some(printed)
        );
        assert_eq!(metadata.get_string("Composite:RedBalance"), red);
        assert_eq!(metadata.get_string("Composite:BlueBalance"), blue);
        for numeric in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_oxidex"));
            command.arg("-j");
            if numeric {
                command.arg("--no-print-conv");
            }
            let output = command.arg(file.path()).output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(json[0]["Leica:WB_RGBLevels"].as_str(), Some(printed));
        }
    }
}

#[test]
fn leica_wb_inline_nul_keeps_public_json_string_type() {
    for field_type in [7u16, 129] {
        let mut bytes = leica_dng(0x08, 200);
        set_leica_entry(&mut bytes, 0x0413, field_type, 2, 0x0031);
        let file = tempfile::Builder::new().suffix(".dng").tempfile().unwrap();
        fs::write(file.path(), bytes).unwrap();
        for numeric in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_oxidex"));
            command.arg("-j");
            if numeric {
                command.arg("--no-print-conv");
            }
            let output = command.arg(file.path()).output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            if field_type == 129 && oxidex::exiftool_oracle::repo_pin() == "11.78" {
                assert!(
                    !json[0]
                        .as_object()
                        .unwrap()
                        .contains_key("Leica:WB_RGBLevels")
                );
            } else {
                assert_eq!(json[0]["Leica:WB_RGBLevels"].as_str(), Some("1"));
            }
        }
    }
}

#[test]
fn leica_non_wb_values_reject_header_and_declaring_ifd_offsets() {
    for (layout, invalid_offsets, valid_offset) in [
        (0x06u8, vec![0, 1, 7, 8], 72),
        (0x08u8, vec![0, 1, 7, 136, 138], 200),
    ] {
        for (tag, field_type, count, name) in [
            (0x040au16, 3u16, 4u32, "Leica:FocusDistance"),
            (0x0303, 2, 8, "Leica:LensType"),
            (0x0408, 2, 8, "Leica:OriginalDirectory"),
        ] {
            for offset in &invalid_offsets {
                let mut bytes = leica_dng(layout, valid_offset);
                set_leica_entry(&mut bytes, tag, field_type, count, *offset);
                let metadata = parse_raw_metadata(&bytes, RawFormat::AdobeDNG).unwrap();
                assert!(
                    !metadata.contains_key(name),
                    "layout {layout:#x} tag {tag:#x} offset {offset}"
                );
            }
            let mut bytes = leica_dng(layout, valid_offset);
            set_leica_entry(&mut bytes, tag, field_type, count, valid_offset);
            let start = if layout == 0x06 {
                128 + valid_offset as usize
            } else {
                valid_offset as usize
            };
            let payload: &[u8; 8] = if tag == 0x040a {
                &[0x0a, 0x06, 0, 0, 0, 0, 0, 0]
            } else {
                b"VALID\0  "
            };
            bytes[start..start + 8].copy_from_slice(payload);
            let metadata = parse_raw_metadata(&bytes, RawFormat::AdobeDNG).unwrap();
            assert!(
                metadata.contains_key(name),
                "layout {layout:#x} tag {tag:#x} valid offset"
            );
            if tag == 0x040a {
                assert_eq!(metadata.get_string(name), Some("1.546 m"));
            }
        }
    }
}

#[test]
fn leica_wb_refuses_non_apple_types_rejected_by_process_exif() {
    for field_type in [14u16, 15, 16, 17, 18] {
        let mut bytes = leica_dng(0x08, 200);
        set_leica_entry(&mut bytes, 0x0413, field_type, 3, 200);
        let metadata = parse_raw_metadata(&bytes, RawFormat::AdobeDNG).unwrap();
        assert!(
            !metadata.contains_key("Leica:WB_RGBLevels"),
            "type {field_type}"
        );
    }
}
