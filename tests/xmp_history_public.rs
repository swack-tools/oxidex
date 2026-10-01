//! Public history output pinned against ExifTool 13.59's xmpMM History Seq.
//! The JPEG, PDF and EPS fixtures contain the same XMP packet. Native output
//! and fixture hashes are retained in the XMP recovery evidence receipt.

use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str, extension: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/xmp_priority/history_native")
        .join(format!("{name}.{extension}"))
}

fn oxidex(args: &[&str], path: &Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(path)
        .output()
        .expect("run oxidex");
    assert!(
        output.status.success(),
        "{args:?} {path:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 output")
}

fn history_json(path: &Path) -> Value {
    let output = oxidex(&["-j", "-a", "-G1", "-s"], path);
    let parsed: Value = serde_json::from_str(&output).expect("JSON output");
    let mut history = Map::new();
    for (key, value) in parsed[0].as_object().expect("one object") {
        if key.contains("History") {
            history.insert(key.clone(), value.clone());
        }
    }
    Value::Object(history)
}

#[test]
fn native_history_collision_keeps_declared_namespace_lookalike() {
    // ExifTool 13.59 -a -G1 -s: one flattened xmpMM event and the real
    // independent probe property. A fabricated numbered xmpMM alias must
    // neither appear as a copy nor win the bare History1Action request.
    let expected = json!({
        "XMP-probe:History1Action": "probe-action",
        "XMP-xmpMM:HistoryAction": "history-action",
        "XMP-xmpMM:HistoryWhen": "2024:01:02 03:04:05Z"
    });
    for extension in ["jpg", "pdf", "eps", "xmp"] {
        let path = fixture("collision", extension);
        assert_eq!(history_json(&path), expected, "{extension}");
        assert_eq!(
            oxidex(&["-s3", "-History1Action"], &path).trim(),
            "probe-action",
            "{extension} bare winner"
        );
    }
}

#[test]
fn native_history_without_collision_has_no_numbered_public_names() {
    let expected = json!({
        "XMP-xmpMM:HistoryAction": "history-action",
        "XMP-xmpMM:HistoryWhen": "2024:01:02 03:04:05Z"
    });
    for extension in ["jpg", "pdf", "eps"] {
        let path = fixture("no-collision", extension);
        assert_eq!(history_json(&path), expected, "{extension}");
        assert!(
            oxidex(&["-s3", "-History1Action"], &path).trim().is_empty(),
            "{extension} must not expose a synthetic alias"
        );
    }
}

#[test]
fn native_history_sequence_is_a_list_of_flattened_fields() {
    let expected = json!({
        "XMP-xmpMM:HistoryAction": ["history-action", "second-action"],
        "XMP-xmpMM:HistoryWhen": ["2024:01:02 03:04:05Z", "2024:02:03 04:05:06Z"]
    });
    for extension in ["jpg", "pdf", "eps"] {
        let path = fixture("multiple", extension);
        assert_eq!(history_json(&path), expected, "{extension}");
    }
}

#[test]
fn native_history_all_six_event_fields_keep_list_boundaries() {
    // XMP.pm %sResourceEvent declares exactly these six fields. The two
    // parameter values contain comma-space, so a joined/split surrogate
    // would produce four wrong elements instead of two native ones.
    let expected = json!({
        "XMP-xmpMM:HistoryAction": ["created", "saved"],
        "XMP-xmpMM:HistoryChanged": ["/metadata", "/content"],
        "XMP-xmpMM:HistoryInstanceID": ["uuid:one", "uuid:two"],
        "XMP-xmpMM:HistoryParameters": ["first, parameter", "second, parameter"],
        "XMP-xmpMM:HistorySoftwareAgent": ["Tool A", "Tool B"],
        "XMP-xmpMM:HistoryWhen": ["2024:01:02 03:04:05Z", "2024:02:03 04:05:06Z"]
    });
    for extension in ["jpg", "pdf", "eps"] {
        assert_eq!(
            history_json(&fixture("all-fields", extension)),
            expected,
            "{extension}"
        );
    }
}
