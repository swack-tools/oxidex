//! Integration tests for format detection
//!
//! These tests verify that the format detection system correctly identifies
//! camera raw formats and integrates them into the main FileFormat enum.

#[path = "../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::FileFormat;
use oxidex::core::format_dispatch::dispatch_format_parser;
use oxidex::parsers::detect_format;
use oxidex::parsers::raw::RawFormat;

#[test]
fn test_detect_canon_cr2() {
    // Canon CR2 has TIFF header + "CR\x02\x00" marker at offset 8
    let cr2_data = vec![
        0x49, 0x49, 0x2a, 0x00, // TIFF little-endian header
        0x10, 0x00, 0x00, 0x00, // IFD offset
        b'C', b'R', 0x02, 0x00, // CR2 signature
        0x00, 0x00, 0x00, 0x00,
    ];
    let reader = TestReader::new(cr2_data);
    let format = detect_format(&reader).expect("Should detect format");

    // Verify it's detected as a CameraRaw variant
    match format {
        FileFormat::CameraRaw(raw_format) => {
            assert_eq!(
                raw_format,
                RawFormat::CanonCR2,
                "Should detect as Canon CR2"
            );
        }
        _ => panic!("Expected CameraRaw(CanonCR2), got {:?}", format),
    }
}

#[test]
fn test_detect_canon_cr3() {
    // Canon CR3 uses ISO Base Media Format with "ftypcrx " marker
    let cr3_data = vec![
        0x00, 0x00, 0x00, 0x18, // Box size
        b'f', b't', b'y', b'p', // "ftyp"
        b'c', b'r', b'x', b' ', // "crx " (CR3 brand)
        0x00, 0x00, 0x00, 0x00,
    ];
    let reader = TestReader::new(cr3_data);
    let format = detect_format(&reader).expect("Should detect format");

    match format {
        FileFormat::CameraRaw(raw_format) => {
            assert_eq!(
                raw_format,
                RawFormat::CanonCR3,
                "Should detect as Canon CR3"
            );
        }
        _ => panic!("Expected CameraRaw(CanonCR3), got {:?}", format),
    }
}

#[test]
fn test_detect_nikon_nef() {
    // Nikon NEF is TIFF big-endian with .nef extension
    // Since we don't have filename in detect_format, this test will be skipped for now
    // or we need to modify the detection function to accept filename
    // For now, test with TIFF header and verify it doesn't break existing TIFF detection
    let nef_data = vec![
        0x4d, 0x4d, 0x00, 0x2a, // TIFF big-endian header
        0x00, 0x00, 0x00, 0x08, // IFD offset
    ];
    let reader = TestReader::new(nef_data);
    let format = detect_format(&reader).expect("Should detect format");

    // Without filename context, TIFF-based raw formats will be detected as TIFF
    // This is expected behavior - we need filename for disambiguation
    assert!(
        matches!(format, FileFormat::TIFF | FileFormat::CameraRaw(_)),
        "Should detect as TIFF or CameraRaw, got {:?}",
        format
    );
}

#[test]
fn test_detect_sony_arw() {
    // Sony ARW is TIFF little-endian with .arw extension
    let arw_data = vec![
        0x49, 0x49, 0x2a, 0x00, // TIFF little-endian header
        0x08, 0x00, 0x00, 0x00, // IFD offset
    ];
    let reader = TestReader::new(arw_data);
    let format = detect_format(&reader).expect("Should detect format");

    // Without filename context, will be detected as TIFF
    assert!(
        matches!(format, FileFormat::TIFF | FileFormat::CameraRaw(_)),
        "Should detect as TIFF or CameraRaw, got {:?}",
        format
    );
}

#[test]
fn test_detect_dng() {
    // DNG is TIFF-based with DNGVersion tag
    // Without full IFD parsing, will appear as TIFF
    let dng_data = vec![
        0x49, 0x49, 0x2a, 0x00, // TIFF little-endian header
        0x08, 0x00, 0x00, 0x00, // IFD offset
    ];
    let reader = TestReader::new(dng_data);
    let format = detect_format(&reader).expect("Should detect format");

    assert!(
        matches!(format, FileFormat::TIFF | FileFormat::CameraRaw(_)),
        "Should detect as TIFF or CameraRaw, got {:?}",
        format
    );
}

#[test]
fn test_detect_fujifilm_raf() {
    // Fujifilm RAF has distinctive "FUJIFILMCCD-RAW " signature
    let raf_data = vec![
        b'F', b'U', b'J', b'I', b'F', b'I', b'L', b'M', b'C', b'C', b'D', b'-', b'R', b'A', b'W',
        b' ', 0x00, 0x00, 0x00, 0x00,
    ];
    let reader = TestReader::new(raf_data);
    let format = detect_format(&reader).expect("Should detect format");

    match format {
        FileFormat::CameraRaw(raw_format) => {
            assert_eq!(
                raw_format,
                RawFormat::FujifilmRAF,
                "Should detect as Fujifilm RAF"
            );
        }
        _ => panic!("Expected CameraRaw(FujifilmRAF), got {:?}", format),
    }
}

#[test]
fn test_detect_sigma_x3f() {
    // Sigma X3F has "FOVb" signature
    let x3f_data = vec![b'F', b'O', b'V', b'b', 0x00, 0x00, 0x00, 0x00];
    let reader = TestReader::new(x3f_data);
    let format = detect_format(&reader).expect("Should detect format");

    match format {
        FileFormat::CameraRaw(raw_format) => {
            assert_eq!(
                raw_format,
                RawFormat::SigmaX3F,
                "Should detect as Sigma X3F"
            );
        }
        _ => panic!("Expected CameraRaw(SigmaX3F), got {:?}", format),
    }
}

#[test]
fn test_detect_minolta_mrw() {
    // Minolta MRW has "\x00MRM" signature
    let mrw_data = vec![0x00, b'M', b'R', b'M', 0x00, 0x00, 0x00, 0x00];
    let reader = TestReader::new(mrw_data);
    let format = detect_format(&reader).expect("Should detect format");

    match format {
        FileFormat::CameraRaw(raw_format) => {
            assert_eq!(
                raw_format,
                RawFormat::MinoltaMRW,
                "Should detect as Minolta MRW"
            );
        }
        _ => panic!("Expected CameraRaw(MinoltaMRW), got {:?}", format),
    }
}

#[test]
fn test_existing_formats_still_work() {
    // Verify that existing format detection still works

    // JPEG
    let jpeg_data = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    let reader = TestReader::new(jpeg_data);
    assert_eq!(detect_format(&reader).unwrap(), FileFormat::JPEG);

    // PNG
    let png_data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let reader = TestReader::new(png_data);
    assert_eq!(detect_format(&reader).unwrap(), FileFormat::PNG);

    // PDF
    let pdf_data = vec![0x25, 0x50, 0x44, 0x46, 0x2D, 0x31, 0x2E, 0x34];
    let reader = TestReader::new(pdf_data);
    assert_eq!(detect_format(&reader).unwrap(), FileFormat::PDF);
}

#[test]
fn test_detect_dpx_magic_bytes() {
    for magic in [b"SDPX", b"XPDS"] {
        let reader = TestReader::new(magic.to_vec());
        assert_eq!(
            detect_format(&reader).unwrap().name(),
            "DPX",
            "{magic:?} must select the DPX parser"
        );
    }
}

#[test]
fn test_parse_dpx_direct_table_fields() {
    let mut data = vec![0; 2080];
    data[..4].copy_from_slice(b"SDPX");
    data[8..12].copy_from_slice(b"V2.0");
    data[16..20].copy_from_slice(&2080_u32.to_be_bytes());
    data[20..24].copy_from_slice(&1_u32.to_be_bytes());
    data[24..28].copy_from_slice(&1664_u32.to_be_bytes());
    data[28..32].copy_from_slice(&384_u32.to_be_bytes());
    data[36..50].copy_from_slice(b"frame0001.dpx\0");
    data[160..172].copy_from_slice(b"OxiDex Test\0");
    data[768..770].copy_from_slice(&0_u16.to_be_bytes());
    data[770..772].copy_from_slice(&1_u16.to_be_bytes());
    data[772..776].copy_from_slice(&1920_u32.to_be_bytes());
    data[776..780].copy_from_slice(&1080_u32.to_be_bytes());
    data[800..804].copy_from_slice(&[50, 6, 6, 10]);
    data[820..839].copy_from_slice(b"Synthetic DPX test\0");

    let metadata = dispatch_format_parser(&TestReader::new(data), FileFormat::DPX)
        .expect("DPX header should parse");

    assert_eq!(metadata.get_string("FileType"), Some("DPX"));
    // `image/x-dpx` reaches output as `File:MIMEType`, from the generated MIME
    // table via `add_identity_tags`. The parser's ungrouped copy was a second
    // answer that `normalize_identity_tags` dropped on every read, since it
    // never promotes a parser's MIMEType.
    assert!(metadata.get_string("MIMEType").is_none());
    assert_eq!(metadata.get_string("ByteOrder"), Some("Big-endian"));
    assert_eq!(metadata.get_string("HeaderVersion"), Some("V2.0"));
    assert_eq!(metadata.get_integer("DPXFileSize"), Some(2080));
    assert_eq!(metadata.get_string("DittoKey"), Some("New"));
    assert_eq!(metadata.get_string("ImageFileName"), Some("frame0001.dpx"));
    assert_eq!(metadata.get_string("Creator"), Some("OxiDex Test"));
    assert_eq!(
        metadata.get_string("Orientation"),
        Some("Horizontal (normal)")
    );
    assert_eq!(metadata.get_integer("ImageElements"), Some(1));
    assert_eq!(metadata.get_integer("ImageWidth"), Some(1920));
    assert_eq!(metadata.get_integer("ImageHeight"), Some(1080));
    assert_eq!(
        metadata.get_string("ComponentsConfiguration"),
        Some("R, G, B")
    );
    assert_eq!(
        metadata.get_string("TransferCharacteristic"),
        Some("ITU-R 709-4")
    );
    assert_eq!(
        metadata.get_string("ColorimetricSpecification"),
        Some("ITU-R 709-4")
    );
    assert_eq!(metadata.get_integer("BitDepth"), Some(10));
    assert_eq!(
        metadata.get_string("ImageDescription"),
        Some("Synthetic DPX test")
    );
}
