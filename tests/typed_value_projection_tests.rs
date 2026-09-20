//! Consumer-level acceptance matrix for typed occurrence projection.

use oxidex::cli::tag_resolution::resolved_display_value;
use oxidex::core::tag_occurrence::ValueChannel;
use oxidex::core::{Instance, MetadataMap, Provenance, TagId, TagOccurrence, TagValue};
use oxidex::ffi::{
    EXIFTOOL_VALUE_CHANNEL_VALUE_CONV, ExifToolHandle, exiftool_create, exiftool_destroy,
    exiftool_get_last_error, exiftool_get_tag_count, exiftool_get_tag_name_at,
    exiftool_get_tag_string, exiftool_get_tag_string_in_channel, exiftool_read_file,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, CString};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct Fixture {
    cases: Vec<Case>,
    composite_parse_classifications: Vec<ParseClassification>,
}

#[derive(Debug, Deserialize)]
struct ParseClassification {
    id: String,
    parse_count: usize,
    input_channel: String,
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
        "../tools/exiftool-tables/testdata/typed_value_projection.json"
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

fn verify_composite_parse_classifications(source: &str, fixture: &Fixture) -> Result<(), String> {
    let mut allowed = BTreeMap::new();
    for entry in &fixture.composite_parse_classifications {
        if entry.parse_count == 0 {
            return Err(format!("{} has an empty parse count", entry.id));
        }
        if !matches!(entry.input_channel.as_str(), "ValueConv" | "PrintConv") {
            return Err(format!("{} has an unknown input channel", entry.id));
        }
        let source_is_exact = entry.source.contains("Image/ExifTool")
            && entry.source.contains(':')
            && entry.source.bytes().any(|byte| byte.is_ascii_digit());
        if !source_is_exact {
            return Err(format!(
                "{} requires an exact ExifTool source location",
                entry.id
            ));
        }
        if allowed.insert(entry.id.as_str(), entry).is_some() {
            return Err(format!("duplicate parse classification {}", entry.id));
        }
    }

    let mut observed = BTreeSet::new();
    for (line_number, line) in source.lines().enumerate() {
        let parse_count = line.matches(".parse").count();
        if parse_count == 0 {
            continue;
        }
        let marker = line
            .split("typed-value-projection: reparse ")
            .nth(1)
            .and_then(|marker| marker.split_whitespace().next())
            .ok_or_else(|| format!("unclassified .parse at source line {}", line_number + 1))?;
        let entry = allowed
            .get(marker)
            .ok_or_else(|| format!("unlisted stringify/reparse id {marker}"))?;
        if parse_count != entry.parse_count {
            return Err(format!(
                "{} has {parse_count} parse calls in source but fixture expects {}",
                entry.id, entry.parse_count
            ));
        }
        if !observed.insert(marker) {
            return Err(format!(
                "parse classification id {marker} appears on multiple source lines"
            ));
        }
    }

    let classified = allowed.keys().copied().collect::<BTreeSet<_>>();
    if observed != classified {
        return Err(format!(
            "classification fixture and source disagree: observed {observed:?}, classified {classified:?}"
        ));
    }
    Ok(())
}

#[test]
fn composite_parse_sites_match_the_source_cited_classifications() {
    let fixture = fixture();
    verify_composite_parse_classifications(include_str!("../src/composite/compute.rs"), &fixture)
        .expect("every marked display reparse must be source-cited in the fixture");
}

#[test]
fn composite_parse_verifier_rejects_an_unmarked_site() {
    let fixture = fixture();
    let source = format!(
        "{}\nlet unexpected = printed.parse::<f64>();",
        include_str!("../src/composite/compute.rs")
    );
    assert!(verify_composite_parse_classifications(&source, &fixture).is_err());
}

fn assert_channel_header_compiles(header: &str) {
    let source = format!(
        "#include \"{header}\"\n\
         int main(void) {{\n\
             ExifToolValueChannel channel = EXIFTOOL_VALUE_CHANNEL_VALUE_CONV;\n\
             return channel == EXIFTOOL_VALUE_CHANNEL_STORED ||\n\
                    channel == EXIFTOOL_VALUE_CHANNEL_PRINT_CONV;\n\
         }}\n"
    );
    let mut child = Command::new("cc")
        .args(["-x", "c", "-fsyntax-only", "-I.", "-"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn C header syntax check");
    child
        .stdin
        .take()
        .expect("C compiler stdin")
        .write_all(source.as_bytes())
        .expect("write C header syntax probe");
    assert!(
        child
            .wait()
            .expect("wait for C header syntax check")
            .success(),
        "{header} must expose the channel type and all named constants"
    );
}

#[test]
fn both_public_c_headers_expose_the_named_channel_surface() {
    assert_channel_header_compiles("api/oxidex.h");
    assert_channel_header_compiles("include/oxidex.h");
}

#[test]
fn ffi_read_only_accessors_are_safe_to_call_concurrently_on_one_handle() {
    let file = tempfile::NamedTempFile::new().expect("creates JPEG fixture");
    std::fs::write(file.path(), interop_index_jpeg()).expect("writes JPEG fixture");
    let handle = exiftool_create();
    assert!(!handle.is_null());
    let path = CString::new(file.path().to_string_lossy().as_bytes()).expect("valid path");
    assert_eq!(exiftool_read_file(handle, path.as_ptr()), 0);

    let handle_address = handle as usize;
    let threads = (0..8)
        .map(|_| {
            std::thread::spawn(move || {
                let handle = handle_address as *const ExifToolHandle;
                let tag = CString::new("InteropIFD:InteropIndex").unwrap();
                for _ in 0..256 {
                    assert!(exiftool_get_tag_count(handle) > 0);
                    let name = exiftool_get_tag_name_at(handle, 0);
                    assert!(!name.is_null());
                    let value = exiftool_get_tag_string_in_channel(
                        handle,
                        tag.as_ptr(),
                        EXIFTOOL_VALUE_CHANNEL_VALUE_CONV,
                    );
                    assert!(!value.is_null());
                    assert_eq!(unsafe { CStr::from_ptr(value) }.to_str().unwrap(), "R98");
                }
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().expect("read-only FFI accessor thread");
    }
    exiftool_destroy(handle);
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
            EXIFTOOL_VALUE_CHANNEL_VALUE_CONV,
        ))
    };
    assert_eq!(value.to_str().unwrap(), "R98");

    let invalid = exiftool_get_tag_string_in_channel(handle, tag.as_ptr(), 99);
    assert!(
        invalid.is_null(),
        "unknown channel integers must be rejected"
    );
    let error = unsafe { CStr::from_ptr(exiftool_get_last_error()) };
    assert!(
        error
            .to_str()
            .unwrap()
            .contains("Unknown value channel: 99"),
        "unknown integers must be rejected before channel conversion"
    );
    exiftool_destroy(handle);
}
