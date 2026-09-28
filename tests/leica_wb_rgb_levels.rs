//! Source-backed Leica5 white-balance levels and their two EXIF composites.

use oxidex::core::operations::read_metadata;
use oxidex::core::tag_occurrence::ValueChannel;
use oxidex::core::{TagId, TagValue};
use oxidex::parsers::raw::{RawFormat, parse_raw_metadata};
use std::fs;

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
