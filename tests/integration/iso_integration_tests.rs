use oxidex::core::TagValue;
use oxidex::io::buffered_reader::BufferedReader;
use oxidex::parsers::archive::iso::parse_iso_metadata;
use std::path::Path;

/// ExifTool 13.59 derives this from the ISO primary-volume descriptor's
/// `VolumeBlockCount * VolumeBlockSize`, then applies `ConvertFileSize`.
///
/// This fails if the composite is omitted, if either descriptor field uses the
/// wrong width or byte order, or if the byte count is rendered differently.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn iso_fixture_reports_volume_size() {
    let path = Path::new("/tmp/oxidex-exiftool-cache/exiftool/t/images/ISO.iso");
    let reader = BufferedReader::new(path).expect("Failed to open pinned ISO fixture");
    let metadata = parse_iso_metadata(&reader).expect("Failed to parse pinned ISO fixture");

    assert_eq!(
        metadata
            .get("ISO:VolumeSize")
            .expect("OxiDex missing ISO:VolumeSize"),
        &TagValue::String("391 MB".to_string())
    );
}
