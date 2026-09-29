//! Regression coverage for Canon EOS R6 Mark II AFConfig MakerNote values.
//!
//! Values are pinned to ExifTool 13.59:
//! `exiftool -G1 -s -MakerNotes:AFConfigTool -MakerNotes:USMLensElectronicMF
//! -MakerNotes:AFStatusViewfinder -MakerNotes:InitialAFPointInServo
//! ...CanonEOS_R6m2.jpg`.

use oxidex::core::operations::read_metadata;
#[path = "common/fixtures.rs"]
mod fixtures;

const EOS_R6M2: &str = "Canon/CanonEOS_R6m2.jpg";

#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn eos_r6m2_af_config_tool_matches_pinned_exiftool() {
    let path = fixtures::required_combined_fixture_path(EOS_R6M2);

    let metadata = read_metadata(&path).expect("parse Canon EOS R6 Mark II JPEG");
    assert_eq!(metadata.get_string("Canon:AFConfigTool"), Some("Case A"));
    assert_eq!(
        metadata.get_string("Canon:USMLensElectronicMF"),
        Some("One-Shot -> Enabled (magnify)")
    );
    assert_eq!(
        metadata.get_string("Canon:AFStatusViewfinder"),
        Some("Show in Field of View")
    );
    assert_eq!(
        metadata.get_string("Canon:InitialAFPointInServo"),
        Some("Auto")
    );
}
