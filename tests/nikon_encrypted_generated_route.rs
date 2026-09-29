//! Required real-carrier proof for the Nikon encrypted generated binary route.

use oxidex::core::TagValue;
use oxidex::core::operations::read_metadata;
use oxidex::core::tag_occurrence::ValueChannel;
use oxidex::exiftool_tables::session::Session;
use oxidex::exiftool_tables::{Ctx, MemberValue};
use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::makernote_context::MakerNoteContext;
use oxidex::parsers::tiff::makernotes::nikon::NikonParser;
use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
use oxidex_tags::TagId;
use std::collections::HashMap;
use std::io::Write;

/// EXIF TIFF from the pinned 13.59 combined-samples Nikon/NikonD810.jpg
/// (source SHA-256 03ec78ef39ea83d4e81715dae96a24ffcf57df48a4d6091a7b07dca06ba10c57).
/// It ends after the complete 18,512-byte MakerNote; the removed image and
/// thumbnail data are irrelevant to the encrypted LensData carrier. The IFD0
/// next-directory pointer is cleared because its thumbnail was removed.
const D810_EXIF: &[u8] = include_bytes!("fixtures/nikon/d810-exif.nef");

#[test]
fn preview_ifd_base_public_api_keeps_six_arguments() {
    let mut decoder_context =
        oxidex::parsers::tiff::makernotes::nikon::binary_data::Ctx::new(None, None);
    let mut decoder_tags = HashMap::new();
    let (): () = oxidex::parsers::tiff::makernotes::nikon::encrypted::parse_lens_data(
        &[],
        0,
        None,
        ByteOrder::LittleEndian,
        &mut decoder_context,
        &mut decoder_tags,
    );
    let mut tags = HashMap::new();
    let mut value_forms = HashMap::new();
    let _ = oxidex::parsers::tiff::makernotes::nikon::parse_nikon_makernotes_with_preview_ifd_base(
        &[],
        ByteOrder::LittleEndian,
        None,
        0,
        &mut tags,
        &mut value_forms,
    );
}

fn d810_carrier(extension: &str) -> tempfile::NamedTempFile {
    d810_carrier_from(D810_EXIF, extension)
}

fn d810_carrier_from(exif: &[u8], extension: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::Builder::new()
        .suffix(extension)
        .tempfile()
        .expect("create D810 metadata carrier");
    if extension == ".jpg" {
        let app1_len = u16::try_from(exif.len() + 8).expect("EXIF fits in JPEG APP1");
        file.write_all(b"\xff\xd8\xff\xe1").unwrap();
        file.write_all(&app1_len.to_be_bytes()).unwrap();
        file.write_all(b"Exif\0\0").unwrap();
    }
    file.write_all(exif).unwrap();
    if extension == ".jpg" {
        file.write_all(b"\xff\xd9").unwrap();
    }
    file
}

fn d810_distinct_lens_fstops(standalone_last: bool) -> Vec<u8> {
    let mut exif = D810_EXIF.to_vec();
    let nikon = exif
        .windows(10)
        .position(|window| window == b"Nikon\0\x02\x11\0\0")
        .expect("pinned D810 MakerNote");
    let tiff = nikon + 10;
    assert_eq!(&exif[tiff..tiff + 2], b"II");
    let ifd = tiff + u32::from_le_bytes(exif[tiff + 4..tiff + 8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(exif[ifd..ifd + 2].try_into().unwrap()) as usize;
    let entry = |id: u16, data: &[u8]| {
        (0..count)
            .map(|index| ifd + 2 + index * 12)
            .find(|&position| {
                u16::from_le_bytes(data[position..position + 2].try_into().unwrap()) == id
            })
            .expect("pinned Nikon physical entry")
    };
    let standalone = entry(0x008b, &exif);
    let encrypted = entry(0x0098, &exif);
    assert!(standalone < encrypted, "pinned D810 starts in sorted order");
    assert_eq!(&exif[standalone + 8..standalone + 12], &[72, 1, 12, 0]);
    exif[standalone + 8] = 84; // 0x008b: 84 * 1/12 = 7.00, versus 0x0098's 6.00.
    if standalone_last {
        for offset in 0..12 {
            exif.swap(standalone + offset, encrypted + offset);
        }
    }
    exif
}

fn d810_plaintext_lens_fstops(version: &[u8; 4], standalone_last: bool) -> Vec<u8> {
    let mut exif = d810_distinct_lens_fstops(standalone_last);
    let nikon = exif
        .windows(10)
        .position(|bytes| bytes == b"Nikon\0\x02\x11\0\0")
        .unwrap();
    let tiff = nikon + 10;
    let ifd = tiff + u32::from_le_bytes(exif[tiff + 4..tiff + 8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(exif[ifd..ifd + 2].try_into().unwrap()) as usize;
    let lens = (0..count)
        .map(|i| ifd + 2 + i * 12)
        .find(|&at| u16::from_le_bytes(exif[at..at + 2].try_into().unwrap()) == 0x0098)
        .unwrap();
    let offset = tiff + u32::from_le_bytes(exif[lens + 8..lens + 12].try_into().unwrap()) as usize;
    exif[offset..offset + 4].copy_from_slice(version);
    exif[offset + if version == b"0100" { 7 } else { 12 }] = 64;
    exif
}

#[test]
fn plaintext_lens_fstops_keeps_physical_order_and_numeric_value() {
    for version in [b"0100", b"0101"] {
        for standalone_last in [false, true] {
            for extension in [".jpg", ".nef", ".nrw"] {
                let exif = d810_plaintext_lens_fstops(version, standalone_last);
                let carrier = d810_carrier_from(&exif, extension);
                let metadata = read_metadata(carrier.path()).unwrap();
                let rows: Vec<_> = metadata
                    .project_occurrences(ValueChannel::PrintConv)
                    .filter(|(key, _, _)| *key == "Nikon:LensFStops")
                    .collect();
                assert_eq!(rows.len(), 2, "{version:?} {extension} {standalone_last}");
                let expected = if standalone_last {
                    ["5.33", "7.00"]
                } else {
                    ["7.00", "5.33"]
                };
                for ((_, row, value), print) in rows.iter().zip(expected) {
                    assert_eq!(value.as_ref(), &TagValue::String(print.to_owned()));
                    if row.origin.table != Some("Main") {
                        assert_eq!(
                            row.origin.table,
                            Some(if version == b"0100" {
                                "LensData00"
                            } else {
                                "LensData01"
                            })
                        );
                        assert_eq!(
                            row.id,
                            TagId::Numeric(if version == b"0100" { 7 } else { 12 })
                        );
                        assert_eq!(row.raw, TagValue::String("5.33".to_string()));
                        assert_eq!(row.stored, Some(TagValue::Integer(64)));
                        assert_eq!(row.value, Some(TagValue::Float(64.0 / 12.0)));
                    }
                }
                assert_eq!(metadata.get_string("Nikon:LensFStops"), Some(expected[1]));
                let run = |flags: &[&str]| {
                    let output = std::process::Command::new(env!("CARGO_BIN_EXE_oxidex"))
                        .args(flags)
                        .args(["-s3", "-LensFStops"])
                        .arg(carrier.path())
                        .output()
                        .unwrap();
                    assert!(output.status.success(), "{output:?}");
                    String::from_utf8(output.stdout)
                        .unwrap()
                        .lines()
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                };
                assert_eq!(run(&["-a"]), expected);
                let numeric = if standalone_last {
                    ["5.33333333333333", "7"]
                } else {
                    ["7", "5.33333333333333"]
                };
                assert_eq!(run(&["-a", "--no-print-conv"]), numeric);
            }
        }
    }
}

fn d810_mixed_plaintext_and_encrypted(version: &[u8; 4], plaintext_last: bool) -> Vec<u8> {
    let mut exif = D810_EXIF.to_vec();
    let nikon = exif
        .windows(10)
        .position(|bytes| bytes == b"Nikon\0\x02\x11\0\0")
        .unwrap();
    let tiff = nikon + 10;
    let ifd = tiff + u32::from_le_bytes(exif[tiff + 4..tiff + 8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(exif[ifd..ifd + 2].try_into().unwrap()) as usize;
    let entry = |id: u16, data: &[u8]| {
        (0..count)
            .map(|i| ifd + 2 + i * 12)
            .find(|&at| u16::from_le_bytes(data[at..at + 2].try_into().unwrap()) == id)
            .unwrap()
    };
    let earlier = entry(0x0097, &exif);
    let later = entry(0x0098, &exif);
    let encrypted_at =
        tiff + u32::from_le_bytes(exif[later + 8..later + 12].try_into().unwrap()) as usize;
    let spare_at =
        tiff + u32::from_le_bytes(exif[earlier + 8..earlier + 12].try_into().unwrap()) as usize;
    let encrypted = exif[encrypted_at..encrypted_at + 33].to_vec();
    assert_eq!(&encrypted[..4], b"0204");
    exif[spare_at..spare_at + 33].copy_from_slice(&encrypted);
    exif[earlier..earlier + 2].copy_from_slice(&0x0098u16.to_le_bytes());
    exif[earlier + 4..earlier + 8].copy_from_slice(&33u32.to_le_bytes());
    exif[encrypted_at..encrypted_at + 4].copy_from_slice(version);
    exif[encrypted_at + if version == b"0100" { 7 } else { 12 }] = 64;
    if !plaintext_last {
        for byte in 0..12 {
            exif.swap(earlier + byte, later + byte);
        }
    }
    exif
}

fn d810_mixed_encrypted(version: &[u8; 4], older_last: bool) -> Vec<u8> {
    let mut exif = D810_EXIF.to_vec();
    let nikon = exif
        .windows(10)
        .position(|bytes| bytes == b"Nikon\0\x02\x11\0\0")
        .unwrap();
    let tiff = nikon + 10;
    let ifd = tiff + u32::from_le_bytes(exif[tiff + 4..tiff + 8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(exif[ifd..ifd + 2].try_into().unwrap()) as usize;
    let entry = |id: u16, data: &[u8]| {
        (0..count)
            .map(|i| ifd + 2 + i * 12)
            .find(|&at| u16::from_le_bytes(data[at..at + 2].try_into().unwrap()) == id)
            .unwrap()
    };
    let earlier = entry(0x0097, &exif);
    let later = entry(0x0098, &exif);
    let source =
        tiff + u32::from_le_bytes(exif[later + 8..later + 12].try_into().unwrap()) as usize;
    let spare =
        tiff + u32::from_le_bytes(exif[earlier + 8..earlier + 12].try_into().unwrap()) as usize;
    let encrypted = exif[source..source + 33].to_vec();
    assert_eq!(&encrypted[..4], b"0204");
    exif[spare..spare + 33].copy_from_slice(&encrypted);
    exif[spare..spare + 4].copy_from_slice(version);
    exif[earlier..earlier + 2].copy_from_slice(&0x0098u16.to_le_bytes());
    exif[earlier + 4..earlier + 8].copy_from_slice(&33u32.to_le_bytes());
    if older_last {
        for byte in 0..12 {
            exif.swap(earlier + byte, later + byte);
        }
    }
    exif
}

#[test]
fn older_encrypted_lens_data_and_generated_0204_keep_both_physical_owners() {
    for version in [b"0201", b"0202", b"0203"] {
        for older_last in [false, true] {
            let exif = d810_mixed_encrypted(version, older_last);
            let carrier = d810_carrier_from(&exif, ".nef");
            let metadata = read_metadata(carrier.path()).unwrap();
            for (name, older_id, older_print, new_id, new_print) in [
                ("ExitPupilPosition", 4, "97.5 mm", 4, "97.5 mm"),
                ("AFAperture", 5, "2.8", 5, "2.8"),
                ("FocusPosition", 8, "0x04", 8, "0x04"),
                ("LensFStops", 12, "12.25", 13, "6.00"),
            ] {
                let key = format!("Nikon:{name}");
                let rows: Vec<_> = metadata
                    .project_occurrences(ValueChannel::PrintConv)
                    .filter(|(candidate, _, _)| *candidate == key)
                    .collect();
                let lens_rows: Vec<_> = rows
                    .iter()
                    .filter(|(_, row, _)| {
                        matches!(row.origin.table, Some("LensData01" | "LensData0204"))
                    })
                    .collect();
                assert_eq!(lens_rows.len(), 2, "{version:?} {older_last} {name}");
                let (first_table, first_id, first_print, second_table, second_id, second_print) =
                    if older_last {
                        (
                            "LensData0204",
                            new_id,
                            new_print,
                            "LensData01",
                            older_id,
                            older_print,
                        )
                    } else {
                        (
                            "LensData01",
                            older_id,
                            older_print,
                            "LensData0204",
                            new_id,
                            new_print,
                        )
                    };
                for (slot, table, id, printed) in [
                    (&lens_rows[0], first_table, first_id, first_print),
                    (&lens_rows[1], second_table, second_id, second_print),
                ] {
                    assert_eq!(slot.1.origin.table, Some(table));
                    assert_eq!(slot.1.id, TagId::Numeric(id));
                    assert_eq!(slot.2.as_ref(), &TagValue::String(printed.to_owned()));
                    assert!(slot.1.stored.is_some());
                    assert!(slot.1.value.is_some());
                }
                assert_eq!(rows.len(), 2 + usize::from(name == "LensFStops"));
            }
        }
    }
}

#[test]
fn conditional_0800_old_lens_data_survives_generated_0204() {
    for version in [b"0800", b"0801", b"0802"] {
        for older_last in [false, true] {
            let exif = d810_mixed_encrypted(version, older_last);
            let carrier = d810_carrier_from(&exif, ".nef");
            let metadata = read_metadata(carrier.path()).unwrap();
            for (name, old_id, old_print) in [
                ("ExitPupilPosition", 4, "97.5 mm"),
                ("AFAperture", 5, "2.8"),
                ("LensFStops", 14, "4.58"),
            ] {
                let key = format!("Nikon:{name}");
                let rows: Vec<_> = metadata
                    .project_occurrences(ValueChannel::PrintConv)
                    .filter(|(candidate, row, _)| {
                        *candidate == key && row.origin.table == Some("LensData0800")
                    })
                    .collect();
                assert_eq!(rows.len(), 1, "{version:?} {older_last} {name}");
                assert_eq!(rows[0].1.id, TagId::Numeric(old_id));
                assert_eq!(rows[0].2.as_ref(), &TagValue::String(old_print.to_owned()));
                assert!(rows[0].1.stored.is_some());
                assert!(rows[0].1.value.is_some());
            }
            let focus: Vec<_> = metadata
                .project_occurrences(ValueChannel::PrintConv)
                .filter(|(key, row, _)| {
                    *key == "Nikon:FocusPosition" && row.origin.table == Some("LensData0800")
                })
                .collect();
            assert!(focus.is_empty(), "0800 has no hand FocusPosition row");
        }
    }
}

#[test]
fn truncated_plaintext_field_does_not_invent_lens_fstops() {
    for (version, cut) in [(b"0100", 7u32), (b"0101", 12u32)] {
        let mut exif = d810_plaintext_lens_fstops(version, true);
        let nikon = exif
            .windows(10)
            .position(|bytes| bytes == b"Nikon\0\x02\x11\0\0")
            .unwrap();
        let tiff = nikon + 10;
        let ifd = tiff + u32::from_le_bytes(exif[tiff + 4..tiff + 8].try_into().unwrap()) as usize;
        let count = u16::from_le_bytes(exif[ifd..ifd + 2].try_into().unwrap()) as usize;
        let lens = (0..count)
            .map(|i| ifd + 2 + i * 12)
            .find(|&at| u16::from_le_bytes(exif[at..at + 2].try_into().unwrap()) == 0x0098)
            .unwrap();
        exif[lens + 4..lens + 8].copy_from_slice(&cut.to_le_bytes());
        let carrier = d810_carrier_from(&exif, ".nef");
        let metadata = read_metadata(carrier.path()).unwrap();
        let rows: Vec<_> = metadata
            .project_occurrences(ValueChannel::PrintConv)
            .filter(|(key, _, _)| *key == "Nikon:LensFStops")
            .collect();
        assert_eq!(rows.len(), 1, "{version:?}");
        assert_eq!(rows[0].1.origin.table, Some("Main"));
        assert_eq!(rows[0].2.as_ref(), &TagValue::String("7.00".to_owned()));
    }
}

#[test]
fn plaintext_and_generated_overlap_keep_both_physical_owners() {
    for version in [b"0100", b"0101"] {
        for plaintext_last in [false, true] {
            let exif = d810_mixed_plaintext_and_encrypted(version, plaintext_last);
            let carrier = d810_carrier_from(&exif, ".nef");
            let metadata = read_metadata(carrier.path()).unwrap();
            let rows: Vec<_> = metadata
                .project_occurrences(ValueChannel::PrintConv)
                .filter(|(key, _, _)| *key == "Nikon:LensFStops")
                .collect();
            assert_eq!(rows.len(), 3, "{version:?} plaintext_last={plaintext_last}");
            let expected = if plaintext_last {
                ["6.00", "6.00", "5.33"]
            } else {
                ["6.00", "5.33", "6.00"]
            };
            for ((_, _, value), printed) in rows.iter().zip(expected) {
                assert_eq!(value.as_ref(), &TagValue::String(printed.to_owned()));
            }
            let plaintext: Vec<_> = rows
                .iter()
                .filter(|(_, row, _)| {
                    row.origin.table
                        == Some(if version == b"0100" {
                            "LensData00"
                        } else {
                            "LensData01"
                        })
                })
                .collect();
            assert_eq!(plaintext.len(), 1);
            assert_eq!(plaintext[0].1.stored, Some(TagValue::Integer(64)));
            assert_eq!(plaintext[0].1.value, Some(TagValue::Float(64.0 / 12.0)));
            if version == b"0101" {
                for (name, field_id, stored, printed, value) in [
                    (
                        "AFAperture",
                        5,
                        6,
                        "1.2",
                        TagValue::Float(2.0_f64.powf(6.0 / 24.0)),
                    ),
                    (
                        "ExitPupilPosition",
                        4,
                        46,
                        "44.5 mm",
                        TagValue::Float(2048.0 / 46.0),
                    ),
                    ("FocusPosition", 8, 133, "0x85", TagValue::Integer(133)),
                ] {
                    let matches: Vec<_> = metadata
                        .project_occurrences(ValueChannel::PrintConv)
                        .filter(|(key, row, _)| {
                            *key == format!("Nikon:{name}")
                                && row.origin.table == Some("LensData01")
                        })
                        .collect();
                    assert_eq!(matches.len(), 1, "{name}");
                    assert_eq!(matches[0].1.id, TagId::Numeric(field_id));
                    assert_eq!(matches[0].1.value, Some(value));
                    assert_eq!(matches[0].1.stored, Some(TagValue::Integer(stored)));
                    assert_eq!(matches[0].1.raw, TagValue::String(printed.to_owned()));
                }
            }
            assert_eq!(metadata.get_string("Nikon:LensFStops"), Some(expected[2]));
        }
    }
}

#[test]
fn standalone_lens_fstops_survives_generated_ownership_in_physical_order() {
    for extension in [".jpg", ".nef", ".nrw"] {
        for standalone_last in [false, true] {
            let exif = d810_distinct_lens_fstops(standalone_last);
            let carrier = d810_carrier_from(&exif, extension);
            let metadata = read_metadata(carrier.path()).expect("read pinned D810 variant");
            let rows: Vec<_> = metadata
                .project_occurrences(ValueChannel::PrintConv)
                .filter(|(key, _, _)| *key == "Nikon:LensFStops")
                .collect();
            assert_eq!(
                rows.len(),
                2,
                "{extension} standalone_last={standalone_last}"
            );
            let expected = if standalone_last {
                [(13, "LensData0204", "6.00"), (0x008b, "Main", "7.00")]
            } else {
                [(0x008b, "Main", "7.00"), (13, "LensData0204", "6.00")]
            };
            for (row, (id, table, printed)) in rows.iter().zip(expected) {
                assert_eq!(row.1.id, TagId::Numeric(id));
                assert_eq!(row.1.origin.table, Some(table));
                assert_eq!(row.2.as_ref(), &TagValue::String(printed.to_owned()));
            }
            assert_eq!(
                metadata.get_string("Nikon:LensFStops"),
                Some(if standalone_last { "7.00" } else { "6.00" })
            );
        }
    }
}

#[test]
fn standalone_lens_fstops_cli_matches_native_order_winner_and_silence() {
    for extension in [".jpg", ".nef", ".nrw"] {
        for standalone_last in [false, true] {
            let exif = d810_distinct_lens_fstops(standalone_last);
            let carrier = d810_carrier_from(&exif, extension);
            let run = |all: bool, numeric: bool, silence: bool| {
                let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_oxidex"));
                if all {
                    command.arg("-a");
                }
                if numeric {
                    command.arg("--no-print-conv");
                }
                if silence {
                    command.env("OXIDEX_GENSHARE_SILENCE", "engine");
                }
                let output = command
                    .args(["-s3", "-LensFStops"])
                    .arg(carrier.path())
                    .output()
                    .expect("run D810 LensFStops CLI");
                assert!(output.status.success(), "{extension}: {output:?}");
                String::from_utf8(output.stdout)
                    .unwrap()
                    .lines()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            };
            let ordered = if standalone_last {
                ["6.00", "7.00"]
            } else {
                ["7.00", "6.00"]
            };
            let numeric = if standalone_last {
                ["6", "7"]
            } else {
                ["7", "6"]
            };
            assert_eq!(run(true, false, false), ordered, "{extension} -a order");
            assert_eq!(
                run(true, true, false),
                numeric,
                "{extension} -a numeric order"
            );
            assert_eq!(run(false, false, false), [ordered[1]], "{extension} winner");
            if extension != ".jpg" {
                assert_eq!(
                    run(false, false, true),
                    ["7.00"],
                    "{extension} standalone under silence"
                );
            }
        }
    }
}

#[test]
fn d810_encrypted_lens_data_has_generated_occurrences() {
    let carrier = d810_carrier(".jpg");
    let metadata = read_metadata(carrier.path()).expect("read pinned Nikon D810 EXIF carrier");
    for (name, source_index, stored, native_value, printed) in [
        ("ExitPupilPosition", 4, 21, 2048.0 / 21.0, "97.5 mm"),
        ("AFAperture", 5, 36, 2.0_f64.powf(36.0 / 24.0), "2.8"),
        ("FocusPosition", 8, 4, 4.0, "0x04"),
        ("LensFStops", 13, 72, 6.0, "6.00"),
    ] {
        let key = format!("Nikon:{name}");
        let rows: Vec<_> = metadata
            .project_occurrences(ValueChannel::PrintConv)
            .filter(|(candidate, row, _)| {
                *candidate == key && row.origin.table == Some("LensData0204")
            })
            .collect();
        assert_eq!(rows.len(), 1, "{key}: exactly one generated occurrence");
        let (_, row, print) = &rows[0];
        assert_eq!(row.id, TagId::Numeric(source_index), "{key}: source index");
        assert_eq!(row.group0.as_ref(), "MakerNotes");
        assert_eq!(row.group1.as_ref(), "Nikon");
        assert_eq!(row.group2.as_deref(), Some("Camera"));
        assert_eq!(row.origin.module, Some("Nikon"));
        assert_eq!(row.stored, Some(TagValue::Integer(stored)));
        let value = match row.value.as_ref() {
            Some(TagValue::Float(value)) => *value,
            Some(TagValue::Integer(value)) => *value as f64,
            other => panic!("{key}: expected typed ValueConv, got {other:?}"),
        };
        assert!((value - native_value).abs() < 1e-10, "{key}: ValueConv");
        assert_eq!(row.raw, TagValue::String(printed.to_string()));
        assert_eq!(print.as_ref(), &TagValue::String(printed.to_string()));
        assert_eq!(metadata.get_string(&key), Some(printed));
    }

    // The route is narrow: an uncredited field is still provided by the
    // established hand reader and carries no false generated attribution.
    assert_eq!(metadata.get_string("Nikon:MinFocalLength"), Some("24.5 mm"));
    assert!(
        metadata
            .project_occurrences(ValueChannel::PrintConv)
            .all(|(key, row, _)| key != "Nikon:MinFocalLength"
                || row.origin.table != Some("LensData0204"))
    );

    let mut credited: Vec<_> = metadata
        .project_occurrences(ValueChannel::PrintConv)
        .filter(|(_, row, _)| row.origin.table == Some("LensData0204"))
        .map(|(_, row, _)| row.name.to_string())
        .collect();
    credited.sort();
    assert_eq!(
        credited,
        [
            "AFAperture",
            "ExitPupilPosition",
            "FocusPosition",
            "LensFStops"
        ]
    );
}

#[test]
fn nef_and_nrw_lens_data_use_the_same_generated_owner_as_jpeg() {
    for extension in [".nef", ".nrw"] {
        let carrier = d810_carrier(extension);
        let metadata = read_metadata(carrier.path()).expect("read pinned D810 RAW carrier");
        for (name, printed) in [
            ("ExitPupilPosition", "97.5 mm"),
            ("AFAperture", "2.8"),
            ("FocusPosition", "0x04"),
            ("LensFStops", "6.00"),
        ] {
            let key = format!("Nikon:{name}");
            let rows: Vec<_> = metadata
                .project_occurrences(ValueChannel::PrintConv)
                .filter(|(candidate, _, _)| *candidate == key)
                .collect();
            let expected_count = if name == "LensFStops" { 2 } else { 1 };
            assert_eq!(
                rows.len(),
                expected_count,
                "{extension} {key}: physical occurrences"
            );
            let generated = rows
                .iter()
                .find(|(_, row, _)| row.origin.table == Some("LensData0204"))
                .expect("generated LensData occurrence");
            assert_eq!(generated.1.origin.module, Some("Nikon"));
            assert_eq!(generated.2.as_ref(), &TagValue::String(printed.to_owned()));
            if name == "LensFStops" {
                assert!(
                    rows.iter()
                        .any(|(_, row, _)| row.id == TagId::Numeric(0x008b)
                            && row.origin.table == Some("Main"))
                );
            }
            assert_eq!(metadata.get_string(&key), Some(printed));
        }
        assert_eq!(metadata.get_string("Nikon:MinFocalLength"), Some("24.5 mm"));
    }
}

#[test]
fn engine_silence_suppresses_raw_generated_fields_without_hiding_residuals() {
    for extension in [".nef", ".nrw"] {
        let carrier = d810_carrier(extension);
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args([
                "-j",
                "-G1",
                "-s",
                "-Nikon:ExitPupilPosition",
                "-Nikon:AFAperture",
                "-Nikon:FocusPosition",
                "-Nikon:LensFStops",
                "-Nikon:MinFocalLength",
            ])
            .arg(carrier.path())
            .env("OXIDEX_GENSHARE_SILENCE", "engine")
            .output()
            .expect("run RAW engine knockout");
        assert!(output.status.success(), "{extension}: {output:?}");
        let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let row = &rows[0];
        for name in ["ExitPupilPosition", "AFAperture", "FocusPosition"] {
            assert!(
                row.get(format!("Nikon:{name}")).is_none(),
                "{extension} {name}: no hand fallback"
            );
        }
        assert_eq!(row["Nikon:LensFStops"], 6.0, "standalone 0x008b survives");
        assert_eq!(row["Nikon:MinFocalLength"], "24.5 mm");
    }
}

#[test]
fn encrypted_field_respects_cli_request_and_numeric_projection() {
    let carrier = d810_carrier(".jpg");
    let run = |numeric: bool| {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_oxidex"));
        command.args(["-G1", "-s", "-Nikon:ExitPupilPosition"]);
        if numeric {
            command.arg("--no-print-conv");
        }
        let output = command
            .arg(carrier.path())
            .output()
            .expect("run oxidex request");
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).expect("utf8 CLI output")
    };
    let print = run(false);
    assert!(print.contains("ExitPupilPosition"));
    assert!(print.contains("97.5 mm"));
    assert!(
        !print.contains("AFAperture"),
        "request leaked another field"
    );
    let numeric = run(true);
    assert!(numeric.contains("97.5238095238095"));
    assert!(!numeric.contains("AFAperture"));
}

#[test]
fn generated_0204_silence_preserves_separate_0201_hand_occurrences() {
    let silenced = std::env::var("OXIDEX_GENSHARE_SILENCE")
        .ok()
        .is_some_and(|tokens| tokens.split(',').any(|token| token == "engine"));
    for older_last in [false, true] {
        let exif = d810_mixed_encrypted(b"0201", older_last);
        let marker = b"Nikon\0\x02\x11\0\0";
        let offset = exif
            .windows(marker.len())
            .position(|bytes| bytes == marker)
            .unwrap();
        let context = MakerNoteContext::detached(&exif[offset..]);
        let mut session = Session::new();
        let mut members: HashMap<&'static str, MemberValue> = HashMap::new();
        let mut condition = Ctx::new(&mut members);
        let mut tags = HashMap::new();
        let mut value_forms = HashMap::new();
        let mut rows = Vec::new();
        NikonParser
            .parse_with_context_and_values_and_session_and_occurrences(
                &context,
                ByteOrder::LittleEndian,
                Some("NIKON D810"),
                &mut session,
                &mut condition,
                &mut tags,
                &mut value_forms,
                &mut rows,
            )
            .unwrap();
        for name in [
            "ExitPupilPosition",
            "AFAperture",
            "FocusPosition",
            "LensFStops",
        ] {
            let key = format!("Nikon:{name}");
            let older: Vec<_> = rows
                .iter()
                .filter(|(candidate, row)| {
                    candidate == &key && row.origin.table == Some("LensData01")
                })
                .collect();
            let generated: Vec<_> = rows
                .iter()
                .filter(|(candidate, row)| {
                    candidate == &key && row.origin.table == Some("LensData0204")
                })
                .collect();
            assert_eq!(older.len(), 1, "{name} older_last={older_last}");
            assert_eq!(
                generated.len(),
                usize::from(!silenced),
                "{name} older_last={older_last}"
            );
        }
    }
    if !silenced {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "generated_0204_silence_preserves_separate_0201_hand_occurrences",
            ])
            .env("OXIDEX_GENSHARE_SILENCE", "engine")
            .status()
            .unwrap();
        assert!(status.success(), "engine knockout failed: {status}");
    }
}

#[test]
fn engine_silence_does_not_resurrect_hand_copies_on_real_d810() {
    let carrier = d810_carrier(".jpg");
    let file = std::fs::read(carrier.path()).expect("read pinned D810 carrier bytes");
    let marker = b"Nikon\0\x02\x11\0\0";
    let offset = file
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("D810 Nikon MakerNote signature");
    // Isolate the real MakerNote carrier so the process-wide engine token
    // does not suppress the enclosing EXIF IFD before it reaches Nikon.
    let context = MakerNoteContext::detached(&file[offset..]);
    let mut session = Session::new();
    let mut members: HashMap<&'static str, MemberValue> = HashMap::new();
    let mut condition = Ctx::new(&mut members);
    let mut tags = HashMap::new();
    let mut value_forms = HashMap::new();
    let mut rows = Vec::new();
    NikonParser
        .parse_with_context_and_values_and_session_and_occurrences(
            &context,
            ByteOrder::LittleEndian,
            Some("NIKON D810"),
            &mut session,
            &mut condition,
            &mut tags,
            &mut value_forms,
            &mut rows,
        )
        .expect("parse D810 Nikon MakerNote");

    let credited = [
        "ExitPupilPosition",
        "AFAperture",
        "FocusPosition",
        "LensFStops",
    ];
    let silenced = std::env::var("OXIDEX_GENSHARE_SILENCE")
        .ok()
        .is_some_and(|tokens| tokens.split(',').any(|token| token == "engine"));
    for name in credited {
        let key = format!("Nikon:{name}");
        assert!(!tags.contains_key(&key), "{key}: no hand copy");
        assert!(!value_forms.contains_key(&key), "{key}: no hand ValueConv");
        let selected: Vec<_> = rows
            .iter()
            .filter(|(candidate, _)| candidate == &key)
            .collect();
        let expected = usize::from(!silenced) + usize::from(name == "LensFStops");
        assert_eq!(
            selected.len(),
            expected,
            "{key}: generated plus physical standalone"
        );
        if name == "LensFStops" {
            assert!(selected.iter().any(
                |(_, row)| row.id == TagId::Numeric(0x008b) && row.origin.table == Some("Main")
            ));
        }
    }
    assert_eq!(
        tags.get("Nikon:MinFocalLength").map(String::as_str),
        Some("24.5 mm")
    );

    if !silenced {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "engine_silence_does_not_resurrect_hand_copies_on_real_d810",
            ])
            .env("OXIDEX_GENSHARE_SILENCE", "engine")
            .status()
            .expect("run real-carrier engine knockout in isolated process");
        assert!(status.success(), "engine knockout failed: {status}");
    }
}
