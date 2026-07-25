use oxidex::parsers::raw::RawFormat;
use oxidex::parsers::raw::detect_raw_format;

#[test]
fn test_detect_canon_cr2() {
    let magic_bytes = b"II\x2a\x00\x10\x00\x00\x00CR\x02\x00";
    let format = detect_raw_format(magic_bytes, "test.cr2");
    assert_eq!(format, Some(RawFormat::CanonCR2));
}

#[test]
fn test_detect_canon_cr3() {
    let magic_bytes = b"\x00\x00\x00\x18ftypcrx ";
    let format = detect_raw_format(magic_bytes, "test.cr3");
    assert_eq!(format, Some(RawFormat::CanonCR3));
}

#[test]
fn test_detect_nikon_nef() {
    let magic_bytes = b"MM\x00\x2a\x00\x00\x00\x08";
    let format = detect_raw_format(magic_bytes, "test.nef");
    assert_eq!(format, Some(RawFormat::NikonNEF));
}

#[test]
fn test_detect_sony_arw() {
    let magic_bytes = b"II\x2a\x00\x08\x00\x00\x00";
    let format = detect_raw_format(magic_bytes, "test.arw");
    assert_eq!(format, Some(RawFormat::SonyARW));
}

#[test]
fn test_detect_dng() {
    // DNG files have TIFF magic + DNG version tag
    let magic_bytes = b"II\x2a\x00\x08\x00\x00\x00";
    let format = detect_raw_format(magic_bytes, "test.dng");
    assert_eq!(format, Some(RawFormat::AdobeDNG));
}
