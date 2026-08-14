use oxidex::core::TagValue;
use oxidex::io::buffered_reader::BufferedReader;
use oxidex::parsers::archive::iso::parse_iso_metadata;
use std::path::Path;

/// The two primary-volume-descriptor fields `Image::ExifTool::ISO::Composite::
/// VolumeSize` (ISO.pm:119-126) multiplies -- `VolumeBlockCount *
/// VolumeBlockSize` under `ConvertFileSize`.
///
/// This test asserts the *inputs*, because the product is not this parser's
/// tag: ExifTool reports it as `Composite:VolumeSize` and has no
/// `ISO:VolumeSize`. The derivation is pinned by
/// `composite::compute::tests::iso_volume_size_multiplies_the_block_geometry`,
/// and the end-to-end result was checked against the pinned oracle on this
/// same fixture (`Composite:VolumeSize` = "391 MB" from both tools). This
/// still fails if either descriptor field uses the wrong width or byte order.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn iso_fixture_reports_the_volume_size_inputs() {
    let path = Path::new("/tmp/oxidex-exiftool-cache/exiftool/t/images/ISO.iso");
    let reader = BufferedReader::new(path).expect("Failed to open pinned ISO fixture");
    let metadata = parse_iso_metadata(&reader).expect("Failed to parse pinned ISO fixture");

    assert_eq!(
        metadata
            .get("ISO:VolumeBlockCount")
            .expect("OxiDex missing ISO:VolumeBlockCount"),
        &TagValue::String("190976".to_string())
    );
    assert_eq!(
        metadata
            .get("ISO:VolumeBlockSize")
            .expect("OxiDex missing ISO:VolumeBlockSize"),
        &TagValue::String("2048".to_string())
    );
}
