use crate::fixtures::pinned_fixture_path;
use oxidex::core::TagValue;
use oxidex::core::operations::read_metadata;
use std::process::Command;

#[test]
fn wpg_record_list_binary_output_uses_raw_ids_and_newlines() {
    let file = tempfile::Builder::new().suffix(".wpg").tempfile().unwrap();
    let mut bytes = vec![0; 16];
    bytes[..4].copy_from_slice(b"\xffWPC");
    bytes[4..8].copy_from_slice(&16u32.to_le_bytes());
    bytes[10] = 1;
    bytes.extend_from_slice(&[0x0f, 0, 0x10, 0, 0, 0]);
    std::fs::write(file.path(), bytes).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(["-b", "-Records"])
        .arg(file.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"15\n16");
}

/// ExifTool 13.59 emits WPG version 1 record types in file order and collapses
/// only adjacent duplicate types. This fails if the WPG parser is not wired
/// into the production read path, if a record length is decoded incorrectly,
/// or if the collapse/print conversion differs from ExifTool's `Records` tag.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn wpg_fixture_reports_records() {
    let Some(path) = pinned_fixture_path("WPG.wpg") else {
        return;
    };
    let metadata = read_metadata(&path).expect("read pinned WPG fixture");

    assert_eq!(
        metadata
            .get("WPG:Records")
            .expect("OxiDex missing WPG:Records"),
        &TagValue::Array(vec![
            TagValue::String("Start WPG (Type 1)".to_string()),
            TagValue::String("Start WPG (Type 2)".to_string()),
            TagValue::String("Fill Attributes".to_string()),
            TagValue::String("Line Attributes".to_string()),
            TagValue::String("Polygon x 5".to_string()),
            TagValue::String("Fill Attributes".to_string()),
            TagValue::String("Line Attributes".to_string()),
            TagValue::String("End WPG".to_string()),
        ])
    );
}
