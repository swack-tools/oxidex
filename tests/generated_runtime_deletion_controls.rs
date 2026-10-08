//! Structural retention control for the historical Task18 deletion ledger.
//! Runtime generated-off attribution still requires authenticated receipts.
use serde_json::Value;
use std::fs;

#[test]
fn historical_changes_remain_unqualified_until_prior_approval_is_proven() {
    let ledger: Value = serde_json::from_str(
        &fs::read_to_string("docs/reference/generated-runtime-deletion-ledger.json")
            .expect("Task18 ledger must exist"),
    )
    .expect("Task18 ledger must be JSON");
    let changes = ledger["historical_changes"]
        .as_array()
        .expect("historical rows must be listed");
    assert_eq!(changes.len(), 5);
    assert_eq!(changes.iter().filter(|row| row["literal_deleted"] == true).count(), 3);
    assert_eq!(changes.iter().filter(|row| row["literal_deleted"] == false).count(), 2);
    assert!(changes.iter().all(|row| row["qualification"] == "unqualified-no-pre-deletion-appendix"));
    assert!(ledger["approved_finite_appendix"].is_null());
    assert!(ledger["controller_reconciliation_manifest"].is_null());
    let retained: i64 = ledger["retained_groups"]
        .as_array()
        .expect("retained groups")
        .iter()
        .map(|row| row["count"].as_i64().expect("integer KEEP count"))
        .sum();
    assert_eq!(retained, 49);
}

#[test]
fn structural_and_decline_fallback_paths_stay_present() {
    let exif = fs::read_to_string("src/core/exif_dir_engine.rs").expect("Exif directory engine");
    let tiff = fs::read_to_string("src/core/tiff_helpers.rs").expect("TIFF helpers");
    let keyed = fs::read_to_string("src/exiftool_tables/keyed_engine.rs").expect("keyed walker");
    let serial = fs::read_to_string("src/exiftool_tables/serial_engine.rs").expect("serial walker");
    for required in ["fn keep_hand_on_decline", "IFD0_HAND_ON_DECLINE", "fn take_ifd0", "fn finish_ifd0"] {
        assert!(exif.contains(required), "required Exif traversal/fallback absent: {required}");
    }
    assert!(tiff.contains("IFD1_RESIDUAL_IDS"));
    assert!(tiff.contains("fn parse_ifd1_with_session"));
    assert!(exif.contains("IFD0_HAND_KEPT"));
    assert!(keyed.contains("fn process_keyed_directory"));
    assert!(serial.contains("fn process_serial_directory"));
}
