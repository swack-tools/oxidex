use oxidex::core::operations::read_metadata;
use std::path::Path;

const EXIFTOOL_JPEG: &str =
    "/tmp/oxidex-exiftool-cache/exiftool-partial/exiftool-13.59/t/images/ExifTool.jpg";

/// ExifTool 13.59 finds the `zmie` marker in an inner MIE trailer and reports
/// its required empty data block as MIE-Main:TrailerSignature.
#[test]
fn exiftool_jpeg_mie_trailer_signature_matches_exiftool() {
    let metadata = read_metadata(Path::new(EXIFTOOL_JPEG)).expect("ExifTool JPEG parses");

    assert_eq!(metadata.get_string("MIE-Main:TrailerSignature"), Some(""));
}
