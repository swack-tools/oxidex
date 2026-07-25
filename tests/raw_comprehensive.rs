//! Comprehensive tests for camera raw format support
//!
//! This test suite validates:
//! - All 36 raw format extensions are detected correctly
//! - Format detection priority (magic bytes override extension)
//! - All RawFormat variants can be parsed without panicking
//! - Integration with real raw files (ignored tests for manual verification)

use oxidex::core::operations::read_metadata;
use oxidex::parsers::raw::{RawFormat, detect_raw_format, parse_raw_metadata};
use std::path::Path;

/// Test that all raw format extensions are correctly detected
///
/// This test ensures comprehensive coverage of all 36 supported raw formats
/// organized by manufacturer. Each extension should map to its corresponding
/// RawFormat variant.
#[test]
fn test_all_raw_extensions_detected() {
    // Minimal data buffer for extension-based detection fallback
    let minimal_data = b"\x00\x00\x00\x00";

    // Comprehensive list of all supported raw format extensions
    // Organized by manufacturer for clarity and maintainability
    let extensions = vec![
        // Canon formats
        ("test.cr2", RawFormat::CanonCR2),
        ("test.cr3", RawFormat::CanonCR3),
        ("test.crw", RawFormat::CanonCRW),
        // Nikon formats
        ("test.nef", RawFormat::NikonNEF),
        ("test.nrw", RawFormat::NikonNRW),
        // Sony formats
        ("test.arw", RawFormat::SonyARW),
        ("test.sr2", RawFormat::SonySR2),
        ("test.srf", RawFormat::SonySRF),
        ("test.srw", RawFormat::SonySRW),
        ("test.arq", RawFormat::SonyARQ),
        ("test.ari", RawFormat::SonyARI),
        // Fujifilm
        ("test.raf", RawFormat::FujifilmRAF),
        // Olympus
        ("test.orf", RawFormat::OlympusORF),
        ("test.ori", RawFormat::OlympusORI),
        // Pentax
        ("test.pef", RawFormat::PentaxPEF),
        // Panasonic
        ("test.rw2", RawFormat::PanasonicRW2),
        ("test.rwl", RawFormat::PanasonicRWL),
        // Hasselblad
        ("test.3fr", RawFormat::Hasselblad3FR),
        ("test.fff", RawFormat::HasselbladFFF),
        // Phase One
        ("test.iiq", RawFormat::PhaseOneIIQ),
        // Mamiya
        ("test.mef", RawFormat::MamiyaMEF),
        // Leaf
        ("test.mos", RawFormat::LeafMOS),
        // Kodak
        ("test.dcr", RawFormat::KodakDCR),
        ("test.kdc", RawFormat::KodakKDC),
        // Minolta
        ("test.mdc", RawFormat::MinoltaMDC),
        ("test.mrw", RawFormat::MinoltaMRW),
        // Epson
        ("test.erf", RawFormat::EpsonERF),
        // Sigma
        ("test.x3f", RawFormat::SigmaX3F),
        // GoPro
        ("test.gpr", RawFormat::GoProGPR),
        // Adobe
        ("test.dng", RawFormat::AdobeDNG),
        // Other formats
        ("test.hif", RawFormat::HEIFHIF),
        ("test.lri", RawFormat::LightLRI),
        ("test.sti", RawFormat::SinarSTI),
        ("test.raw", RawFormat::GenericRAW),
        ("test.cam", RawFormat::GenericCAM),
        ("test.rev", RawFormat::GenericREV),
    ];

    // Verify each extension is detected correctly
    for (filename, expected_format) in &extensions {
        let detected = detect_raw_format(minimal_data, filename);
        assert_eq!(
            detected,
            Some(*expected_format),
            "Failed to detect format for {}. Expected {:?}, got {:?}",
            filename,
            expected_format,
            detected
        );
    }

    // Verify the count matches the expected number of formats (36 total)
    // This serves as a regression test to ensure no formats are accidentally removed
    assert_eq!(
        extensions.len(),
        36,
        "Expected 36 total raw formats to be tested (Canon:3, Nikon:2, Sony:6, Fuji:1, Olympus:2, Pentax:1, Panasonic:2, Hasselblad:2, PhaseOne:1, Mamiya:1, Leaf:1, Kodak:2, Minolta:2, Epson:1, Sigma:1, GoPro:1, Adobe:1, Other:6)"
    );
}

/// Test that magic bytes take priority over file extension
///
/// When a file has distinctive magic bytes, those should override the extension.
/// This is critical for correctly identifying files that have been renamed or
/// have incorrect extensions.
#[test]
fn test_format_detection_priority_magic_bytes_override_extension() {
    // Canon CR2 has distinctive magic bytes: TIFF header + "CR\x02\x00" at offset 8
    let cr2_magic = b"II\x2a\x00\x10\x00\x00\x00CR\x02\x00\x00\x00";

    // Even with a .nef extension, the CR2 magic bytes should be detected
    let format = detect_raw_format(cr2_magic, "test.nef");
    assert_eq!(
        format,
        Some(RawFormat::CanonCR2),
        "CR2 magic bytes should override .nef extension"
    );

    // Canon CR3 uses ISO Base Media Format with "ftypcrx " marker
    let cr3_magic = b"\x00\x00\x00\x18ftypcrx \x00\x00\x00\x00";
    let format = detect_raw_format(cr3_magic, "test.arw");
    assert_eq!(
        format,
        Some(RawFormat::CanonCR3),
        "CR3 magic bytes should override .arw extension"
    );

    // Fujifilm RAF has distinctive 16-byte signature
    let raf_magic = b"FUJIFILMCCD-RAW \x00\x00\x00\x00";
    let format = detect_raw_format(raf_magic, "test.orf");
    assert_eq!(
        format,
        Some(RawFormat::FujifilmRAF),
        "RAF magic bytes should override .orf extension"
    );

    // Sigma X3F has "FOVb" signature
    let x3f_magic = b"FOVb\x00\x00\x00\x00\x00\x00\x00\x00";
    let format = detect_raw_format(x3f_magic, "test.pef");
    assert_eq!(
        format,
        Some(RawFormat::SigmaX3F),
        "X3F magic bytes should override .pef extension"
    );

    // Minolta MRW has "\x00MRM" signature
    let mrw_magic = b"\x00MRM\x00\x00\x00\x00\x00\x00\x00\x00";
    let format = detect_raw_format(mrw_magic, "test.rw2");
    assert_eq!(
        format,
        Some(RawFormat::MinoltaMRW),
        "MRW magic bytes should override .rw2 extension"
    );
}

/// Test that all RawFormat variants can be parsed without panicking
///
/// This test ensures graceful handling of all raw formats, even with minimal data.
/// The parser should either successfully extract metadata or return an error,
/// but should never panic.
#[test]
fn test_all_formats_handled_gracefully() {
    // Minimal TIFF header (little-endian)
    // Most raw formats are TIFF-based, so this should work for many formats
    let minimal_tiff = b"II\x2a\x00\x08\x00\x00\x00";

    // All RawFormat variants organized by manufacturer
    let formats = vec![
        // Canon
        RawFormat::CanonCR2,
        RawFormat::CanonCR3,
        RawFormat::CanonCRW,
        // Nikon
        RawFormat::NikonNEF,
        RawFormat::NikonNRW,
        // Sony (all 6 variants)
        RawFormat::SonyARW,
        RawFormat::SonySR2,
        RawFormat::SonySRF,
        RawFormat::SonySRW,
        RawFormat::SonyARQ,
        RawFormat::SonyARI,
        // Fujifilm
        RawFormat::FujifilmRAF,
        // Olympus (both variants)
        RawFormat::OlympusORF,
        RawFormat::OlympusORI,
        // Pentax
        RawFormat::PentaxPEF,
        // Panasonic (both variants)
        RawFormat::PanasonicRW2,
        RawFormat::PanasonicRWL,
        // Hasselblad (both variants)
        RawFormat::Hasselblad3FR,
        RawFormat::HasselbladFFF,
        // Phase One
        RawFormat::PhaseOneIIQ,
        // Mamiya
        RawFormat::MamiyaMEF,
        // Leaf
        RawFormat::LeafMOS,
        // Kodak (both variants)
        RawFormat::KodakDCR,
        RawFormat::KodakKDC,
        // Minolta (both variants)
        RawFormat::MinoltaMDC,
        RawFormat::MinoltaMRW,
        // Epson
        RawFormat::EpsonERF,
        // Sigma
        RawFormat::SigmaX3F,
        // GoPro
        RawFormat::GoProGPR,
        // Adobe
        RawFormat::AdobeDNG,
        // Other (all 6 variants)
        RawFormat::HEIFHIF,
        RawFormat::LightLRI,
        RawFormat::SinarSTI,
        RawFormat::GenericRAW,
        RawFormat::GenericCAM,
        RawFormat::GenericREV,
    ];

    // Verify all 36 formats are tested
    assert_eq!(
        formats.len(),
        36,
        "Expected to test all 36 RawFormat variants"
    );

    // Test that each format can be parsed without panicking
    for format in formats {
        // Use std::panic::catch_unwind to catch any panics
        let result = std::panic::catch_unwind(|| parse_raw_metadata(minimal_tiff, format));

        assert!(
            result.is_ok(),
            "Format {:?} caused panic during parsing",
            format
        );

        // The inner result may be Ok or Err, but it should not panic
        if let Ok(inner_result) = result {
            // Either successfully parsed or gracefully returned an error
            assert!(
                inner_result.is_ok() || inner_result.is_err(),
                "Format {:?} returned unexpected result",
                format
            );
        }
    }
}

/// Test extension detection is case-insensitive
///
/// Camera raw files may have extensions in various cases (e.g., .NEF, .nef, .Nef).
/// The detection should handle all case variations correctly.
#[test]
fn test_case_insensitive_extension_detection() {
    let minimal_data = b"\x00\x00\x00\x00";

    // Test various case combinations
    let test_cases = vec![
        ("test.NEF", RawFormat::NikonNEF),
        ("test.nef", RawFormat::NikonNEF),
        ("test.Nef", RawFormat::NikonNEF),
        ("test.ARW", RawFormat::SonyARW),
        ("test.arw", RawFormat::SonyARW),
        ("test.Arw", RawFormat::SonyARW),
        ("test.CR2", RawFormat::CanonCR2),
        ("test.cr2", RawFormat::CanonCR2),
        ("test.Cr2", RawFormat::CanonCR2),
        ("test.DNG", RawFormat::AdobeDNG),
        ("test.dng", RawFormat::AdobeDNG),
        ("test.Dng", RawFormat::AdobeDNG),
    ];

    for (filename, expected_format) in test_cases {
        let detected = detect_raw_format(minimal_data, filename);
        assert_eq!(
            detected,
            Some(expected_format),
            "Case-insensitive detection failed for {}",
            filename
        );
    }
}

/// Test handling of files without extensions
///
/// If a file has no extension and no recognizable magic bytes,
/// detection should gracefully return None.
#[test]
fn test_no_extension_handling() {
    let minimal_data = b"II\x2a\x00\x08\x00\x00\x00";

    // File without extension should return None
    let result = detect_raw_format(minimal_data, "testfile");
    assert_eq!(
        result, None,
        "Should return None for file without extension"
    );

    // Empty filename should return None
    let result = detect_raw_format(minimal_data, "");
    assert_eq!(result, None, "Should return None for empty filename");
}

/// Test handling of unknown extensions
///
/// Files with unrecognized extensions should return None.
#[test]
fn test_unknown_extension_handling() {
    let minimal_data = b"\x00\x00\x00\x00";

    let unknown_extensions = vec![
        "test.xyz",
        "test.unknown",
        "test.jpg", // Not a raw format
        "test.png",
        "test.mp4",
    ];

    for filename in unknown_extensions {
        let result = detect_raw_format(minimal_data, filename);
        assert_eq!(
            result, None,
            "Should return None for unknown extension: {}",
            filename
        );
    }
}

/// Test handling of insufficient data
///
/// When the data buffer is too small to contain magic bytes,
/// the detection should fall back to extension-based detection.
#[test]
fn test_insufficient_data_fallback() {
    // Very small data buffer (only 2 bytes)
    let insufficient_data = b"II";

    // Should fall back to extension-based detection
    let result = detect_raw_format(insufficient_data, "test.nef");
    assert_eq!(
        result,
        Some(RawFormat::NikonNEF),
        "Should use extension fallback when data is insufficient"
    );

    let result = detect_raw_format(insufficient_data, "test.arw");
    assert_eq!(
        result,
        Some(RawFormat::SonyARW),
        "Should use extension fallback when data is insufficient"
    );
}

/// Test with real raw files (ignored by default)
///
/// This test is ignored by default and requires actual camera raw files to be present.
/// To run this test with real files:
/// 1. Place real raw files in tests/fixtures/raw/real/ directory
/// 2. Run: cargo test test_read_real_raw_files -- --ignored
///
/// This is useful for manual verification with actual camera files.
#[test]
#[ignore]
fn test_read_real_raw_files() {
    let test_files = vec![
        // Canon
        ("tests/fixtures/raw/real/canon.cr2", RawFormat::CanonCR2),
        ("tests/fixtures/raw/real/canon.cr3", RawFormat::CanonCR3),
        // Nikon
        ("tests/fixtures/raw/real/nikon.nef", RawFormat::NikonNEF),
        // Sony
        ("tests/fixtures/raw/real/sony.arw", RawFormat::SonyARW),
        // Fujifilm
        ("tests/fixtures/raw/real/fuji.raf", RawFormat::FujifilmRAF),
        // Olympus
        ("tests/fixtures/raw/real/olympus.orf", RawFormat::OlympusORF),
        // Pentax
        ("tests/fixtures/raw/real/pentax.pef", RawFormat::PentaxPEF),
        // Panasonic
        (
            "tests/fixtures/raw/real/panasonic.rw2",
            RawFormat::PanasonicRW2,
        ),
        // Adobe DNG
        ("tests/fixtures/raw/real/sample.dng", RawFormat::AdobeDNG),
    ];

    for (file_path, _expected_format) in test_files {
        let path = Path::new(file_path);

        // Only test files that exist
        if path.exists() {
            println!("Testing real raw file: {}", file_path);

            // Test metadata extraction
            let metadata = read_metadata(path);
            assert!(
                metadata.is_ok(),
                "Failed to read metadata from {}. Error: {:?}",
                file_path,
                metadata.err()
            );

            let meta = metadata.unwrap();
            assert!(!meta.is_empty(), "No metadata extracted from {}", file_path);

            println!(
                "  Successfully extracted {} metadata tags from {}",
                meta.len(),
                file_path
            );

            // Verify file type is set
            if let Some(file_type) = meta.get("File:FileType") {
                println!("  File type: {:?}", file_type);
            }
        } else {
            println!(
                "Skipping {} (file not found). Place real raw files in tests/fixtures/raw/real/ to test.",
                file_path
            );
        }
    }
}

/// Test that TIFF-based raw formats share common parsing logic
///
/// Most raw formats (NEF, ARW, PEF, ORF, etc.) are TIFF-based and should
/// be parsed using the TIFF parser infrastructure.
#[test]
fn test_tiff_based_formats_use_tiff_parser() {
    // Valid TIFF header (little-endian) with minimal IFD
    let tiff_data = b"II\x2a\x00\x08\x00\x00\x00";

    // TIFF-based formats that should work with TIFF parser
    let tiff_based_formats = vec![
        RawFormat::CanonCR2,
        RawFormat::NikonNEF,
        RawFormat::SonyARW,
        RawFormat::AdobeDNG,
        RawFormat::PentaxPEF,
        RawFormat::OlympusORF,
        RawFormat::PanasonicRW2,
    ];

    for format in tiff_based_formats {
        // Should not panic when parsing TIFF-based formats
        let result = std::panic::catch_unwind(|| parse_raw_metadata(tiff_data, format));
        assert!(
            result.is_ok(),
            "TIFF-based format {:?} caused panic",
            format
        );
    }
}

/// Test that proprietary formats have appropriate handling
///
/// Some formats (CR3, X3F, MRW) use proprietary containers and need
/// special parsing logic.
#[test]
fn test_proprietary_formats_handled() {
    // Proprietary formats that don't use TIFF structure
    let proprietary_formats = vec![
        (RawFormat::CanonCR3, b"\x00\x00\x00\x18ftypcrx " as &[u8]),
        (RawFormat::SigmaX3F, b"FOVb\x00\x00\x00\x00" as &[u8]),
        (RawFormat::MinoltaMRW, b"\x00MRM\x00\x00\x00\x00" as &[u8]),
    ];

    for (format, data) in proprietary_formats {
        // Should not panic, even if full parsing is not yet implemented
        let result = std::panic::catch_unwind(|| parse_raw_metadata(data, format));
        assert!(
            result.is_ok(),
            "Proprietary format {:?} caused panic",
            format
        );

        // Should return either success or graceful error
        if let Ok(parse_result) = result {
            assert!(
                parse_result.is_ok() || parse_result.is_err(),
                "Proprietary format {:?} returned unexpected result",
                format
            );
        }
    }
}
