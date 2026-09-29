//! Regression coverage for the model-gated Canon ModifiedInfo fields.
//!
//! Expected values are from pinned ExifTool 13.59:
//! `exiftool -G1 -s -Canon:ModifiedSharpness -Canon:ModifiedDigitalGain
//! CanonEOS-1D.jpg`.

use oxidex::core::operations::read_metadata;
#[path = "common/fixtures.rs"]
mod fixtures;

const EOS_1D: &str = "Canon/CanonEOS-1D.jpg";

#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn eos_1d_modified_info_matches_pinned_exiftool() {
    let path = fixtures::required_combined_fixture_path(EOS_1D);

    let metadata = read_metadata(&path).expect("parse Canon EOS-1D JPEG");
    assert_eq!(metadata.get_string("Canon:ModifiedSharpness"), Some("0"));
    assert_eq!(metadata.get_string("Canon:ModifiedDigitalGain"), Some("0"));
}
