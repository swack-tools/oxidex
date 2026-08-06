use oxidex::io::buffered_reader::BufferedReader;
use oxidex::parsers::image::bmp::parse_bmp_metadata;
use std::path::Path;

const BMP_FIXTURE: &str = "/tmp/oxidex-exiftool-cache/exiftool/t/images/BMP.bmp";

#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn bmp_fixture_extracts_planes_from_the_dib_header() {
    let reader = BufferedReader::new(Path::new(BMP_FIXTURE)).expect("open pinned BMP fixture");
    let metadata = parse_bmp_metadata(&reader).expect("parse pinned BMP fixture");

    assert_eq!(metadata.get_integer("Planes"), Some(1));
}
