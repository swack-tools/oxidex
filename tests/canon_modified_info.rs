//! Regression coverage for the model-gated Canon ModifiedInfo fields.
//!
//! Expected values are from pinned ExifTool 13.59:
//! `exiftool -G1 -s -Canon:ModifiedSharpness -Canon:ModifiedDigitalGain
//! CanonEOS-1D.jpg`.

use oxidex::core::operations::read_metadata;
use std::path::Path;

const EOS_1D: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonEOS-1D.jpg";

#[test]
fn eos_1d_modified_info_matches_pinned_exiftool() {
    let path = Path::new(EOS_1D);
    if !path.is_file() {
        eprintln!("skipping: corpus fixture not present at {}", path.display());
        return;
    }

    let metadata = read_metadata(path).expect("parse Canon EOS-1D JPEG");
    assert_eq!(metadata.get_string("Canon:ModifiedSharpness"), Some("0"));
    assert_eq!(metadata.get_string("Canon:ModifiedDigitalGain"), Some("0"));
}
