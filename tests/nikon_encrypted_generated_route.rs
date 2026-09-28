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

#[path = "common/fixtures.rs"]
mod fixtures;

#[test]
fn d810_encrypted_lens_data_has_generated_occurrences() {
    let path = fixtures::required_combined_fixture_path("Nikon/NikonD810.jpg");
    let metadata = read_metadata(&path).expect("read required Nikon D810 carrier");
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
fn encrypted_field_respects_cli_request_and_numeric_projection() {
    let path = fixtures::required_combined_fixture_path("Nikon/NikonD810.jpg");
    let run = |numeric: bool| {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_oxidex"));
        command.args(["-G1", "-s", "-Nikon:ExitPupilPosition"]);
        if numeric {
            command.arg("--no-print-conv");
        }
        let output = command.arg(&path).output().expect("run oxidex request");
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
    let path = fixtures::required_combined_fixture_path("Nikon/NikonD810.jpg");
    let file = std::fs::read(&path).expect("read required D810 carrier bytes");
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
        assert_eq!(
            rows.iter()
                .filter(|(candidate, _)| candidate == &key)
                .count(),
            usize::from(!silenced),
            "{key}: only the generated route may report"
        );
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
