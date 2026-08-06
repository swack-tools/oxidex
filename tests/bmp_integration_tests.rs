use oxidex::io::buffered_reader::BufferedReader;
use oxidex::parsers::image::bmp::parse_bmp_metadata;
use std::path::Path;

const BMP_FIXTURE: &str = "/tmp/oxidex-exiftool-cache/exiftool/t/images/BMP.bmp";

#[test]
fn os2_bmp_extracts_planes_from_the_os2_dib_layout() {
    let mut bmp = vec![0; 14];
    bmp[0..2].copy_from_slice(b"BM");
    bmp.extend_from_slice(&12u32.to_le_bytes());
    bmp.extend_from_slice(&2u16.to_le_bytes());
    bmp.extend_from_slice(&3u16.to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&8u16.to_le_bytes());

    let reader = BufferedReader::from_bytes(&bmp);
    let metadata = parse_bmp_metadata(&reader).expect("parse synthetic OS/2 BMP");

    assert_eq!(metadata.get_integer("Planes"), Some(1));
}

#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn bmp_fixture_extracts_planes_from_the_dib_header() {
    let reader = BufferedReader::new(Path::new(BMP_FIXTURE)).expect("open pinned BMP fixture");
    let metadata = parse_bmp_metadata(&reader).expect("parse pinned BMP fixture");

    assert_eq!(metadata.get_integer("Planes"), Some(1));
}
