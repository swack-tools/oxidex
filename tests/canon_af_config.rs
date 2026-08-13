//! Regression coverage for Canon EOS R6 Mark II AFConfig MakerNote values.
//!
//! Values are pinned to ExifTool 13.59:
//! `exiftool -G1 -s -MakerNotes:AFConfigTool -MakerNotes:USMLensElectronicMF
//! -MakerNotes:AFStatusViewfinder -MakerNotes:InitialAFPointInServo
//! ...CanonEOS_R6m2.jpg`.

use oxidex::core::operations::read_metadata;
use std::path::Path;

const EOS_R6M2: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonEOS_R6m2.jpg";

#[test]
fn eos_r6m2_af_config_tool_matches_pinned_exiftool() {
    let path = Path::new(EOS_R6M2);
    if !path.is_file() {
        eprintln!("skipping: corpus fixture not present at {}", path.display());
        return;
    }

    let metadata = read_metadata(path).expect("parse Canon EOS R6 Mark II JPEG");
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
