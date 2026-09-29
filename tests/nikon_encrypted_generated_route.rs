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
