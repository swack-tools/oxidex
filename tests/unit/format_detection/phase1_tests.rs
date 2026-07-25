#[path = "../../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::FileFormat;
use oxidex::parsers::detection::detect_format;

#[test]
fn test_detect_flac_by_magic() {
    let data = b"fLaC\x00\x00\x00\x22";
    let reader = TestReader::new(data.to_vec());
    let format = detect_format(&reader).unwrap();
    assert_eq!(format, FileFormat::FLAC);
}
