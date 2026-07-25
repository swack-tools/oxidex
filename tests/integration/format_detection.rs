//! Integration tests for format detection
//!
//! These tests verify that the format detection system correctly identifies
//! camera raw formats and integrates them into the main FileFormat enum.

#[path = "../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::FileFormat;
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
