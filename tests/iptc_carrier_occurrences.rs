//! Pinned ExifTool 13.59 selected IPTC output for source-derived PSD, TIFF,
//! EPS and PDF carriers. The saved native JSON uses `-j -a -G1:4 -s` so a
//! second physical IIM directory cannot disappear into a duplicate key.
use serde_json::{Map, Value};
use std::{path::Path, process::Command};

#[test]
fn carrier_blocks_preserve_source_types_lists_and_occurrences() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/iptc_carrier");
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/iptc_carrier/native-selected.json"))
            .expect("native selected output is JSON");
    for (file, tags) in expected.as_object().expect("file map") {
        let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(["-j", "-a", "-G1:4", "-s"])
            .arg(root.join(file))
            .output()
            .expect("run oxidex");
        assert!(
            output.status.success(),
            "{file}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let parsed: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|e| panic!("{file}: {e}: {}", String::from_utf8_lossy(&output.stdout)));
        let actual = parsed[0].as_object().expect("JSON object");
        let selected: Map<String, Value> = actual
            .iter()
            .filter(|(key, _)| {
                key.starts_with("IPTC:") || key.starts_with("IPTC2:") || key.starts_with("IPTC3:")
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        assert_eq!(Value::Object(selected), *tags, "{file}");
    }
}

fn json_fields(path: &Path, args: &[&str], suffix: &str) -> Map<String, Value> {
    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(path)
        .output()
        .expect("run oxidex");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("JSON output");
    parsed[0]
        .as_object()
        .expect("JSON object")
        .iter()
        .filter(|(key, _)| key.ends_with(suffix))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[test]
fn copy_family_is_projected_from_physical_directory_number() {
    let file =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/iptc_carrier/three-blocks.eps");
    let g14 = json_fields(&file, &["-j", "-a", "-G1:4", "-s"], "By-line");
    assert_eq!(g14.get("IPTC:By-line"), Some(&Value::from("Alice")));
    assert_eq!(g14.get("IPTC2:Copy1:By-line"), Some(&Value::from("Bob")));
    assert_eq!(g14.get("IPTC3:Copy2:By-line"), Some(&Value::from("Carol")));
    assert_eq!(g14.len(), 3);

    let g4 = json_fields(&file, &["-j", "-a", "-G4", "-s"], "By-line");
    assert_eq!(g4.get(":By-line"), Some(&Value::from("Alice")));
    assert_eq!(g4.get("Copy1:By-line"), Some(&Value::from("Bob")));
    assert_eq!(g4.get("Copy2:By-line"), Some(&Value::from("Carol")));
    assert_eq!(g4.len(), 3);

    let g1 = json_fields(&file, &["-j", "-a", "-G1", "-s"], "By-line");
    assert_eq!(g1.get("IPTC:By-line"), Some(&Value::from("Alice")));
    assert_eq!(g1.get("IPTC2:By-line"), Some(&Value::from("Bob")));
    assert_eq!(g1.get("IPTC3:By-line"), Some(&Value::from("Carol")));
    assert_eq!(g1.len(), 3);
    let default = json_fields(&file, &["-j", "-s"], "By-line");
    assert_eq!(default.get("IPTC:By-line"), Some(&Value::from("Alice")));
    assert_eq!(default.len(), 1);
}

#[path = "common/fixtures.rs"]
mod fixtures;

#[test]
fn flashpix_copy_family_uses_the_same_projection() {
    let Some(file) = fixtures::pinned_t_images_fixture_path("FlashPix.ppt") else {
        return;
    };
    let fields = json_fields(&file, &["-j", "-a", "-G1:4", "-s", "-CodePage"], "CodePage");
    let value = Value::from("Mac Roman (Western European)");
    assert_eq!(fields.get("FlashPix:CodePage"), Some(&value));
    assert_eq!(fields.get("FlashPix:Copy1:CodePage"), Some(&value));
    assert_eq!(fields.get("FlashPix:Copy2:CodePage"), Some(&value));
    assert_eq!(fields.len(), 3);
}

#[test]
fn carrier_value_conv_survives_numeric_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/iptc_carrier");
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/iptc_carrier/native-selected-n.json"))
            .expect("native numeric output is JSON");
    for (file, tags) in expected.as_object().expect("file map") {
        let actual = json_fields(
            &root.join(file),
            &["-j", "-a", "-G1:4", "-s", "--no-print-conv"],
            "",
        );
        let selected: Map<String, Value> = actual
            .into_iter()
            .filter(|(key, _)| {
                key.starts_with("IPTC:") || key.starts_with("IPTC2:") || key.starts_with("IPTC3:")
            })
            .collect();
        assert_eq!(
            Value::Object(selected),
            *tags,
            "{file} under --no-print-conv"
        );
    }
}
