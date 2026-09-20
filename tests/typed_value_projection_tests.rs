//! Consumer-level acceptance matrix for typed occurrence projection.

use oxidex::cli::tag_resolution::resolved_display_value;
use oxidex::core::tag_occurrence::ValueChannel;
use oxidex::core::{Instance, MetadataMap, Provenance, TagId, TagOccurrence, TagValue};
use oxidex::ffi::{
    ExifToolValueChannel, exiftool_create, exiftool_destroy, exiftool_get_tag_string,
    exiftool_get_tag_string_in_channel, exiftool_read_file,
};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::ffi::{CStr, CString};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct Fixture {
    cases: Vec<Case>,
    intentional_display_reparse: Vec<ReparseAllowance>,
}

#[derive(Debug, Deserialize)]
struct ReparseAllowance {
    id: String,
    source: String,
}

#[derive(Debug, Deserialize)]
struct Case {
    kind: String,
    key: String,
    raw: TagValue,
    value: TagValue,
    print: TagValue,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!(
        "../tools/exiftool-tables/fixtures/typed_value_projection.json"
    ))
    .expect("typed projection fixture must be valid JSON")
}

fn occurrence(case: &Case) -> TagOccurrence {
    let (group0, name) = case.key.split_once(':').expect("fixture key has a group");
    TagOccurrence {
        id: TagId::Named(case.key.clone()),
        name: Arc::from(name),
        group0: Arc::from(group0),
        group1: Arc::from("Fixture"),
        group2: None,
        instance: Instance::default(),
        raw: case.raw.clone(),
        value: Some(case.value.clone()),
        print: Some(case.print.clone()),
        stored: Some(case.raw.clone()),
        priority: 1,
        is_list: false,
        order: 0,
        origin: Provenance::default(),
    }
}

#[test]
fn fixture_backed_matrix_selects_print_and_value_channels_from_one_occurrence() {
    let fixture = fixture();
    let required = [
        "integer",
        "float",
        "rational",
        "enum",
        "date_time",
        "bytes",
        "binary_placeholder",
        "list",
        "undefined_suppressed",
        "units",
        "negative_zero",
    ];
    for kind in required {
        assert!(
            fixture.cases.iter().any(|case| case.kind == kind),
            "missing {kind}"
        );
    }
    let distinct = fixture
        .cases
        .iter()
        .find(|case| case.raw != case.value && case.value != case.print)
        .expect("fixture must carry an occurrence with distinct raw, ValueConv, and PrintConv");
    assert_eq!(distinct.kind, "rational");

    for case in &fixture.cases {
        let occurrence = occurrence(case);
        assert_eq!(
            resolved_display_value(&occurrence, false),
            case.print,
            "normal output must use PrintConv for {}",
            case.kind
        );
        assert_eq!(
            resolved_display_value(&occurrence, true),
            case.value,
            "--no-print-conv must use ValueConv for {}",
            case.kind
        );
        assert_eq!(
            occurrence.project(ValueChannel::PrintConv).as_ref(),
            &case.print,
            "fixture PrintConv must remain canonical for {}",
            case.kind
        );
    }
}

#[test]
fn duplicate_groups_remain_distinct_before_consumer_projection() {
    let mut metadata = MetadataMap::new();
    metadata.insert("IFD0:Label", TagValue::new_string("first"));
    metadata.insert("ExifIFD:Label", TagValue::new_string("second"));

    let labels: Vec<_> = metadata
        .project_occurrences(ValueChannel::PrintConv)
        .map(|(key, _, value)| (key.to_owned(), value.into_owned()))
        .collect();
    assert_eq!(
        labels,
        vec![
            ("IFD0:Label".to_string(), TagValue::new_string("first")),
            ("ExifIFD:Label".to_string(), TagValue::new_string("second")),
        ]
    );
}

fn verify_display_reparse_allowlist(source: &str, fixture: &Fixture) -> Result<(), String> {
    let allowed = fixture
        .intentional_display_reparse
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<BTreeSet<_>>();
    if fixture.intentional_display_reparse.iter().any(|entry| {
        let Some((path, line)) = entry.source.rsplit_once(':') else {
            return true;
        };
        path.trim().is_empty()
            || !path.contains("Image/ExifTool")
            || !line.bytes().any(|byte| byte.is_ascii_digit())
    }) {
        return Err(
            "every intentional display reparse requires an ExifTool source location".into(),
        );
    }

    let mut observed = BTreeSet::new();
    let lines = source.lines().collect::<Vec<_>>();
    for (line_number, line) in lines.iter().enumerate() {
        let Some(marker) = line
            .split("typed-value-projection: reparse ")
            .nth(1)
            .and_then(|marker| marker.split_whitespace().next())
        else {
            continue;
        };
        if !line.contains(".parse")
            && !lines
                .iter()
                .skip(line_number + 1)
                .take(6)
                .any(|next| next.contains(".parse"))
        {
            return Err(format!(
                "display reparse marker without a parse at source line {}",
                line_number + 1
            ));
        }
        if !allowed.contains(marker) {
            return Err(format!("unlisted display stringify/reparse id {marker}"));
        }
        observed.insert(marker);
    }

    if observed != allowed {
        return Err(format!(
            "allowlist and source disagree: observed {observed:?}, allowed {allowed:?}"
        ));
    }
    Ok(())
}

#[test]
fn composite_display_reparse_sites_match_the_source_cited_allowlist() {
    let fixture = fixture();
    verify_display_reparse_allowlist(include_str!("../src/composite/compute.rs"), &fixture)
        .expect("every marked display reparse must be source-cited in the fixture");
}

#[test]
fn composite_display_reparse_verifier_rejects_an_unlisted_site() {
    let fixture = fixture();
    let source = format!(
        "{}\nlet unexpected = printed.parse::<f64>(); // typed-value-projection: reparse unexpected",
        include_str!("../src/composite/compute.rs")
    );
    assert!(verify_display_reparse_allowlist(&source, &fixture).is_err());
}

fn interop_index_jpeg() -> Vec<u8> {
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1, 0x00, 0x46];
    jpeg.extend_from_slice(b"Exif\0\0II\x2a\0\x08\0\0\0");
    // IFD0 -> ExifIFD at TIFF offset 26.
    jpeg.extend_from_slice(&[1, 0, 0x69, 0x87, 4, 0, 1, 0, 0, 0, 26, 0, 0, 0, 0, 0, 0, 0]);
    // ExifIFD -> InteropIFD at TIFF offset 44.
    jpeg.extend_from_slice(&[1, 0, 0x05, 0xa0, 4, 0, 1, 0, 0, 0, 44, 0, 0, 0, 0, 0, 0, 0]);
    // InteropIFD: InteropIndex (ASCII[4]) = R98.
    jpeg.extend_from_slice(&[
        1, 0, 1, 0, 2, 0, 4, 0, 0, 0, b'R', b'9', b'8', 0, 0, 0, 0, 0,
    ]);
    jpeg.extend_from_slice(&[0xff, 0xd9]);
    jpeg
}

#[test]
fn ffi_explicit_channel_selects_value_conv_without_changing_legacy_print_default() {
    let file = tempfile::NamedTempFile::new().expect("creates JPEG fixture");
    std::fs::write(file.path(), interop_index_jpeg()).expect("writes JPEG fixture");
    let handle = exiftool_create();
    assert!(!handle.is_null());
    let path = CString::new(file.path().to_string_lossy().as_bytes()).expect("valid path");
    assert_eq!(exiftool_read_file(handle, path.as_ptr()), 0);

    let tag = CString::new("InteropIFD:InteropIndex").unwrap();
    let legacy = unsafe { CStr::from_ptr(exiftool_get_tag_string(handle, tag.as_ptr())) };
    assert_eq!(legacy.to_str().unwrap(), "R98 - DCF basic file (sRGB)");

    let value = unsafe {
        CStr::from_ptr(exiftool_get_tag_string_in_channel(
            handle,
            tag.as_ptr(),
            ExifToolValueChannel::ValueConv as i32,
        ))
    };
    assert_eq!(value.to_str().unwrap(), "R98");
    exiftool_destroy(handle);
}
