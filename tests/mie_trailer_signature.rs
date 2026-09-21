#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::read_metadata;

/// The comparison report uses ExifTool's group-0 `MIE` family for this inner
/// `zmie` trailer (rather than the group-1 `MIE-Main` spelling).
#[test]
fn exiftool_jpeg_mie_trailer_signature_matches_exiftool() {
    let Some(path) = fixtures::pinned_t_images_fixture_path("ExifTool.jpg") else {
        eprintln!("skipping: pinned fixture ExifTool.jpg is absent");
        return;
    };

    let metadata = read_metadata(&path).expect("ExifTool JPEG parses");

    assert_eq!(metadata.get_string("MIE:TrailerSignature"), Some(""));
    assert_eq!(
        metadata.get_string("MIE:Copyright"),
        Some("© 2006 Phil Harvey")
    );
}
