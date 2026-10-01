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

#[test]
fn one_resource_keeps_duplicate_scalar_copies_without_changing_list_or_block_boundaries() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/iptc_carrier");
    let scalar = root.join("duplicate-scalar.psd");
    // Pinned ExifTool 13.59: the second ObjectName wins the unqualified
    // value, but -a -G4 still exposes the earlier physical record as Copy1.
    for numeric in [false, true] {
        let mut flags = vec!["-j", "-a", "-s", "-G1:4"];
        if numeric {
            flags.push("--no-print-conv");
        }
        let fields = json_fields(&scalar, &flags, "ObjectName");
        assert_eq!(
            fields.get("IPTC:Copy1:ObjectName"),
            Some(&Value::from("First"))
        );
        assert_eq!(fields.get("IPTC:ObjectName"), Some(&Value::from("Second")));
        assert_eq!(fields.len(), 2);
    }
    let g4 = json_fields(&scalar, &["-j", "-a", "-s", "-G4"], "ObjectName");
    assert_eq!(g4.get("Copy1:ObjectName"), Some(&Value::from("First")));
    assert_eq!(g4.get(":ObjectName"), Some(&Value::from("Second")));
    let g1 = json_fields(&scalar, &["-j", "-a", "-s", "-G1"], "ObjectName");
    assert_eq!(g1.get("IPTC:ObjectName"), Some(&Value::from("Second")));
    assert_eq!(g1.len(), 1);
    let default = json_fields(&scalar, &["-j", "-s"], "ObjectName");
    // OxiDex's existing default JSON convention retains the IPTC prefix;
    // pinned ExifTool omits it. The winning value must remain unchanged.
    assert_eq!(default.get("IPTC:ObjectName"), Some(&Value::from("Second")));
    assert_eq!(default.len(), 1);

    // List members stay inside one resource; separate resources still receive
    // distinct IPTC/IPTC2 directories rather than joining that list.
    let within = json_fields(
        &root.join("repeatable-byline.psd"),
        &["-j", "-a", "-G1:4", "-s"],
        "By-line",
    );
    assert_eq!(
        within.get("IPTC:By-line"),
        Some(&serde_json::json!(["Alice", "Bob"]))
    );
    let separate = json_fields(
        &root.join("two-iptc-resources.psd"),
        &["-j", "-a", "-G1:4", "-s"],
        "By-line",
    );
    assert_eq!(separate.get("IPTC:By-line"), Some(&Value::from("Alice")));
    assert_eq!(
        separate.get("IPTC2:Copy1:By-line"),
        Some(&Value::from("Bob"))
    );
}

#[test]
fn scalar_copies_follow_record_order_across_one_and_two_resources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/iptc_carrier");
    // Native 13.59 controls are saved in the IPTC recovery evidence's
    // root-scalar-boundary-controls/native-expectations.json.
    let three = json_fields(
        &root.join("three-scalars.psd"),
        &["-j", "-a", "-G1:4", "-s"],
        "ObjectName",
    );
    assert_eq!(
        three.get("IPTC:Copy1:ObjectName"),
        Some(&Value::from("First"))
    );
    assert_eq!(
        three.get("IPTC:Copy2:ObjectName"),
        Some(&Value::from("Second"))
    );
    assert_eq!(three.get("IPTC:ObjectName"), Some(&Value::from("Third")));
    assert_eq!(three.len(), 3);

    let mixed_file = root.join("mixed-list-scalars.psd");
    let mixed = json_fields(&mixed_file, &["-j", "-a", "-G1:4", "-s"], "ObjectName");
    assert_eq!(
        mixed.get("IPTC:Copy1:ObjectName"),
        Some(&Value::from("First"))
    );
    assert_eq!(mixed.get("IPTC:ObjectName"), Some(&Value::from("Second")));
    let byline = json_fields(&mixed_file, &["-j", "-a", "-G1:4", "-s"], "By-line");
    assert_eq!(
        byline.get("IPTC:By-line"),
        Some(&serde_json::json!(["Alice", "Bob"]))
    );
    assert_eq!(byline.len(), 1);

    let two = json_fields(
        &root.join("two-block-scalars.psd"),
        &["-j", "-a", "-G1:4", "-s"],
        "ObjectName",
    );
    assert_eq!(
        two.get("IPTC:Copy1:ObjectName"),
        Some(&Value::from("First"))
    );
    assert_eq!(two.get("IPTC:ObjectName"), Some(&Value::from("Second")));
    assert_eq!(
        two.get("IPTC2:Copy2:ObjectName"),
        Some(&Value::from("Third"))
    );
    assert_eq!(
        two.get("IPTC2:Copy3:ObjectName"),
        Some(&Value::from("Fourth"))
    );
    assert_eq!(two.len(), 4);
}

fn eps_grouped_text_order(group: &str, fixture: &str) -> Vec<String> {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/iptc_carrier")
        .join(fixture);
    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args([group, "-a", "-s"])
        .arg(file)
        .output()
        .expect("run oxidex");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (group, rest) = line.split_once(']')?;
            let (tag, value) = rest.split_once(':')?;
            let tag = tag.trim();
            matches!(tag, "By-line" | "ObjectName")
                .then(|| format!("{}:{tag}={}", group.trim_start_matches('['), value.trim()))
        })
        .collect()
}

#[test]
fn eps_grouped_text_sort_uses_resolved_copy_identity() {
    assert_eq!(
        eps_grouped_text_order("-G4", "eps-one-hex-sort.eps"),
        [":By-line=Alice", ":ObjectName=Second", "Copy1:By-line=Bob"]
    );
    assert_eq!(
        eps_grouped_text_order("-G1:4", "eps-one-hex-sort.eps"),
        [
            "IPTC:By-line=Alice",
            "IPTC2:Copy1:By-line=Bob",
            "IPTC2:ObjectName=Second",
        ]
    );
}

#[test]
fn eps_interleaved_list_keeps_first_source_position() {
    for (group, expected_group) in [("-G1", "IPTC"), ("-G1:4", "IPTC"), ("-G4", "")] {
        assert_eq!(
            eps_grouped_text_order(group, "eps-interleaved-list.eps"),
            [
                format!("{expected_group}:By-line=Alice, Bob"),
                format!("{expected_group}:ObjectName=Between"),
            ],
            "{group}"
        );
    }
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
