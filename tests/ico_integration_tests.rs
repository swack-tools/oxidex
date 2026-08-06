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
