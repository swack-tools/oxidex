mod common;

use common::TestReader;
use oxidex::io::buffered_reader::BufferedReader;
use oxidex::parsers::image::ico::parse_ico_metadata;
use std::path::Path;

const ICO_FIXTURE: &str = "/tmp/oxidex-exiftool-cache/exiftool/t/images/ICO.ico";

#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn extracts_bits_per_pixel_from_the_ico_directory_entry() {
    let reader = BufferedReader::new(Path::new(ICO_FIXTURE)).expect("open ICO fixture");
    let metadata = parse_ico_metadata(&reader).expect("parse ICO fixture");

    assert_eq!(metadata.get_string("BitsPerPixel"), Some("1"));
}

#[test]
fn does_not_treat_cursor_hotspot_y_as_bits_per_pixel() {
    let reader = TestReader::from_slice(&[
        0x00, 0x00, 0x02, 0x00, // reserved, cursor type
        0x01, 0x00, // one directory entry
        0x10, 0x10, 0x00, 0x00, // width, height, colors, reserved
        0x03, 0x00, // hotspot X
        0x07, 0x00, // hotspot Y (not bits per pixel)
        0x00, 0x00, 0x00, 0x00, // image length
        0x00, 0x00, 0x00, 0x00, // image offset
    ]);

    let metadata = parse_ico_metadata(&reader).expect("parse synthetic CUR");

    assert!(!metadata.contains_key("BitsPerPixel"));
}
