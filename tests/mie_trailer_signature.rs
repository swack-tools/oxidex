use oxidex::core::operations::read_metadata;
use std::path::Path;

const EXIFTOOL_JPEG: &str = "/tmp/oxidex-exiftool-cache/exiftool/t/images/ExifTool.jpg";

/// The comparison report uses ExifTool's group-0 `MIE` family for this inner
/// `zmie` trailer (rather than the group-1 `MIE-Main` spelling).
#[test]
fn exiftool_jpeg_mie_trailer_signature_matches_exiftool() {
    if !Path::new(EXIFTOOL_JPEG).is_file() {
        eprintln!("skipping: pinned fixture not present at {EXIFTOOL_JPEG}");
        return;
    }

    let metadata = read_metadata(Path::new(EXIFTOOL_JPEG)).expect("ExifTool JPEG parses");

    assert_eq!(metadata.get_string("MIE:TrailerSignature"), Some(""));
    assert_eq!(
        metadata.get_string("MIE:Copyright"),
        Some("© 2006 Phil Harvey")
    );
}
