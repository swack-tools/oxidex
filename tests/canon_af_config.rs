//! Regression coverage for Canon EOS R6 Mark II AFConfig MakerNote values.
//!
//! Values are pinned to ExifTool 13.59:
//! `exiftool -G1 -s -MakerNotes:AFConfigTool ...CanonEOS_R6m2.jpg`.

use oxidex::core::operations::read_metadata;
use std::path::Path;

const EOS_R6M2: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonEOS_R6m2.jpg";

#[test]
fn eos_r6m2_af_config_tool_matches_pinned_exiftool() {
    let path = Path::new(EOS_R6M2);
    assert!(
        path.is_file(),
        "missing required corpus fixture: {}",
        path.display()
    );

    let metadata = read_metadata(path).expect("parse Canon EOS R6 Mark II JPEG");
    assert_eq!(metadata.get_string("Canon:AFConfigTool"), Some("Case A"));
}
