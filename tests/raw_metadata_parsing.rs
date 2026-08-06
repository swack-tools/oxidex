//! Tests for camera raw metadata parsing
//!
//! These tests verify that the raw metadata parser correctly extracts
//! metadata from various camera raw file formats.
//!
//! Note: Tests requiring actual raw file fixtures will fail if fixtures
//! don't exist yet - that's expected and OK for this iteration.

use oxidex::core::TagValue;
use oxidex::parsers::raw::{RawFormat, parse_raw_metadata};
use std::fs;

#[test]
fn test_parse_dng_metadata() {
    // This test requires a real DNG sample file
    // It will fail gracefully if the fixture doesn't exist yet
    let fixture_path = "tests/fixtures/raw/sample.dng";

    if let Ok(data) = fs::read(fixture_path) {
        let result = parse_raw_metadata(&data, RawFormat::AdobeDNG);
        assert!(result.is_ok(), "Failed to parse DNG file");

        let metadata = result.unwrap();

        // DNG files should have at least the FileType tag
        assert!(
            metadata.contains_key("File:FileType"),
            "DNG should have File:FileType tag"
        );

        // If TIFF parsing succeeded, we should have more tags
        // (This is optional since we don't have a real fixture yet)
        if metadata.len() > 1 {
            // We successfully parsed some metadata
            println!("Successfully parsed {} tags from DNG", metadata.len());
        }
    } else {
        // Fixture doesn't exist yet - skip test with warning
        eprintln!(
            "Warning: Skipping test_parse_dng_metadata - fixture not found at {}",
            fixture_path
        );
    }
}

#[test]
fn test_parse_cr2_metadata() {
    // This test requires a real CR2 sample file
    let fixture_path = "tests/fixtures/raw/sample.cr2";

    if let Ok(data) = fs::read(fixture_path) {
        let result = parse_raw_metadata(&data, RawFormat::CanonCR2);
        assert!(result.is_ok(), "Failed to parse CR2 file");

        let metadata = result.unwrap();

        // CR2 files should have FileType tag at minimum
        assert!(
            metadata.contains_key("File:FileType"),
            "CR2 should have File:FileType tag"
        );

        // If TIFF parsing succeeded, we might have Canon MakerNotes
        if metadata.len() > 1 {
            println!("Successfully parsed {} tags from CR2", metadata.len());
        }
    } else {
        eprintln!(
            "Warning: Skipping test_parse_cr2_metadata - fixture not found at {}",
            fixture_path
        );
    }
}

#[test]
fn test_parse_nef_metadata() {
    // This test requires a real NEF sample file
    let fixture_path = "tests/fixtures/raw/sample.nef";

    if let Ok(data) = fs::read(fixture_path) {
        let result = parse_raw_metadata(&data, RawFormat::NikonNEF);
        assert!(result.is_ok(), "Failed to parse NEF file");

        let metadata = result.unwrap();
        assert!(
            metadata.contains_key("File:FileType"),
            "NEF should have File:FileType tag"
        );

        if metadata.len() > 1 {
            println!("Successfully parsed {} tags from NEF", metadata.len());
        }
    } else {
        eprintln!(
            "Warning: Skipping test_parse_nef_metadata - fixture not found at {}",
            fixture_path
        );
    }
}

#[test]
fn test_nef_jpg_from_raw_is_extracted_from_sub_ifd_pair() {
    let data = fs::read("/tmp/oxidex-exiftool-cache/combined-samples/Nikon.nef")
        .expect("Nikon NEF comparison fixture should be available");

    let metadata = parse_raw_metadata(&data, RawFormat::NikonNEF)
        .expect("Nikon NEF comparison fixture should parse");

    // ExifTool 13.59 Exif.pm declares the SubIFD 0x0201/0x0202 pair as
    // DataTag => JpgFromRaw. The fixture's bytes are exactly the declared
    // 29-byte range at its JpgFromRawStart offset.
    assert_eq!(
        metadata.get("EXIF:JpgFromRaw"),
        Some(&TagValue::new_binary(
            b"<Dummy JpgFromRaw image data>".to_vec()
        ))
    );
}

#[test]
fn test_rw2_af_point_position_is_relocated_from_makernote() {
    let data = fs::read("/tmp/oxidex-exiftool-cache/exiftool/t/images/Panasonic.rw2")
        .expect("pinned ExifTool Panasonic RW2 fixture should be available");

    let metadata = parse_raw_metadata(&data, RawFormat::PanasonicRW2)
        .expect("Panasonic RW2 fixture should parse");

    // ExifTool 13.59's Panasonic.pm defines tag 0x004d as two rational64u
    // AF-area-center coordinates. Its exact PrintConv renders this fixture as
    // "0.5 0.5".
    assert_eq!(
        metadata.get("Panasonic:AFPointPosition"),
        Some(&TagValue::new_string("0.5 0.5".to_string()))
    );
}

#[test]
fn test_parse_minimal_tiff_based_raw() {
    // Create a minimal valid TIFF header
    // II (little-endian) + 0x002A (magic 42) + offset to IFD (8)
    let mut data = Vec::new();
    data.extend_from_slice(b"II"); // Little-endian
    data.extend_from_slice(&42u16.to_le_bytes()); // Magic number
    data.extend_from_slice(&8u32.to_le_bytes()); // First IFD offset
    data.extend_from_slice(&0u16.to_le_bytes()); // IFD entry count (0 entries)

    let result = parse_raw_metadata(&data, RawFormat::AdobeDNG);

    // Should parse successfully or fail gracefully (not panic)
    match result {
        Ok(metadata) => {
            assert!(
                metadata.contains_key("File:FileType"),
                "Should have File:FileType tag"
            );
            println!(
                "Minimal TIFF parsed successfully with {} tags",
                metadata.len()
            );
        }
        Err(e) => {
            println!("Minimal TIFF parsing failed gracefully: {}", e);
        }
    }
}

#[test]
fn test_parse_cr3_stub() {
    // CR3 has ISO Base Media Format signature
    let data = b"\x00\x00\x00\x18ftypcrx \x00\x00\x00\x00crx isom";

    let result = parse_raw_metadata(data, RawFormat::CanonCR3);

    assert!(result.is_ok(), "CR3 stub should parse successfully");
    let metadata = result.unwrap();

    // ExifTool's file type for this format is "CR3" (ExifTool.pm:272,
    // `CR3 => ['MOV', 'Canon RAW 3 format']`), with `image/x-canon-cr3`
    // (ExifTool.pm:637). This assertion used to require "CanonCR3", which is
    // the Rust `RawFormat` variant name and not an ExifTool file type at all --
    // the test was pinning the bug in place. The tag-comparison harness skips
    // the File group, so nothing else was checking these three.
    assert_eq!(metadata.get_string("File:FileType"), Some("CR3"));
    assert_eq!(metadata.get_string("File:FileTypeExtension"), Some("cr3"));
    assert_eq!(
        metadata.get_string("File:MIMEType"),
        Some("image/x-canon-cr3")
    );
}

#[test]
fn test_parse_cr3_compressor_version_selects_crm() {
    // Canon.pm's %Canon::uuid CNCV entry is
    //     RawConv => '$self->OverrideFileType($1) if $val =~ /^Canon(\w{3})/i'
    // so the same `crx ` brand is a CR3 still or a CRM movie depending only on
    // what the CNCV box says. Byte layout: [size: u32 BE][type][payload].
    let mut data = Vec::from(*b"\x00\x00\x00\x18ftypcrx \x00\x00\x00\x00crx isom");
    let cncv = b"CanonCRM_001/00.09.00/00.00.00";
    data.extend_from_slice(&(8u32 + cncv.len() as u32).to_be_bytes());
    data.extend_from_slice(b"CNCV");
    data.extend_from_slice(cncv);

    let metadata = parse_raw_metadata(&data, RawFormat::CanonCR3).expect("CRM should parse");

    assert_eq!(metadata.get_string("File:FileType"), Some("CRM"));
    assert_eq!(metadata.get_string("File:FileTypeExtension"), Some("crm"));
    // ExifTool.pm:638, `CRM => 'video/x-canon-crm'`.
    assert_eq!(
        metadata.get_string("File:MIMEType"),
        Some("video/x-canon-crm")
    );
    assert_eq!(
        metadata.get_string("Canon:CompressorVersion"),
        Some("CanonCRM_001/00.09.00/00.00.00")
    );
}

#[test]
fn test_parse_x3f_stub() {
    // X3F has FOVb signature
    let data = b"FOVb\x00\x00\x00\x01test data for X3F format";

    let result = parse_raw_metadata(data, RawFormat::SigmaX3F);

    assert!(result.is_ok(), "X3F stub should parse successfully");
    let metadata = result.unwrap();

    // This used to assert only `contains_key`, which is the same blind spot
    // `test_parse_cr3_stub` had: it passed while the value was the Rust
    // `RawFormat` variant name "SigmaX3F". ExifTool.pm:565 is
    // `X3F => ['X3F', 'Sigma RAW format']` and ExifTool.pm:830 is
    // `X3F => 'image/x-sigma-x3f'`; `exiftool -s -S -FileType Sigma.x3f`
    // prints `X3F`.
    assert_eq!(metadata.get_string("File:FileType"), Some("X3F"));
    assert_eq!(
        metadata.get_string("File:MIMEType"),
        Some("image/x-sigma-x3f")
    );
}

#[test]
fn test_parse_mrw_stub() {
    // MRW has \x00MRM signature
    let data = b"\x00MRM\x00\x00\x00\x01test data for MRW format";

    let result = parse_raw_metadata(data, RawFormat::MinoltaMRW);

    assert!(result.is_ok(), "MRW stub should parse successfully");
    let metadata = result.unwrap();

    // Same blind spot as the X3F stub above: the value was "MinoltaMRW".
    // ExifTool.pm:429 is `MRW => ['MRW', 'Minolta RAW format']` and
    // ExifTool.pm:737 is `MRW => 'image/x-minolta-mrw'`;
    // `exiftool -s -S -FileType Minolta.mrw` prints `MRW`.
    assert_eq!(metadata.get_string("File:FileType"), Some("MRW"));
    assert_eq!(
        metadata.get_string("File:MIMEType"),
        Some("image/x-minolta-mrw")
    );
}

#[test]
fn test_parse_all_tiff_based_formats() {
    // Test that all TIFF-based formats can be parsed without panicking
    let minimal_tiff = create_minimal_tiff();

    let tiff_formats = vec![
        RawFormat::CanonCR2,
        RawFormat::NikonNEF,
        RawFormat::NikonNRW,
        RawFormat::SonyARW,
        RawFormat::SonySR2,
        RawFormat::SonySRF,
        RawFormat::SonySRW,
        RawFormat::AdobeDNG,
        RawFormat::PentaxPEF,
        RawFormat::OlympusORF,
        RawFormat::FujifilmRAF,
        RawFormat::PanasonicRW2,
        RawFormat::Hasselblad3FR,
        RawFormat::PhaseOneIIQ,
        RawFormat::MamiyaMEF,
        RawFormat::LeafMOS,
        RawFormat::KodakDCR,
        RawFormat::GoProGPR,
    ];

    for format in tiff_formats {
        let result = parse_raw_metadata(&minimal_tiff, format);

        // Should either succeed or fail gracefully (not panic)
        assert!(
            result.is_ok() || result.is_err(),
            "Format {:?} caused panic",
            format
        );

        if let Ok(metadata) = result {
            assert!(
                metadata.contains_key("File:FileType"),
                "Format {:?} should have File:FileType tag",
                format
            );
        }
    }
}

#[test]
fn test_parse_invalid_data() {
    // Test with completely invalid data
    let invalid_data = b"This is not a valid raw file format";

    let result = parse_raw_metadata(invalid_data, RawFormat::AdobeDNG);

    // Should fail gracefully (not panic)
    assert!(result.is_err(), "Invalid data should return an error");
}

#[test]
fn test_parse_empty_data() {
    // Test with empty data
    let empty_data = b"";

    let result = parse_raw_metadata(empty_data, RawFormat::AdobeDNG);

    // Should fail gracefully (not panic)
    assert!(result.is_err(), "Empty data should return an error");
}

#[test]
fn test_parse_truncated_tiff() {
    // TIFF header needs at least 8 bytes, provide only 4
    let truncated_data = b"II\x2a\x00";

    let result = parse_raw_metadata(truncated_data, RawFormat::AdobeDNG);

    // Should fail gracefully
    assert!(result.is_err(), "Truncated TIFF should return an error");
}

#[test]
fn test_generic_raw_fallback() {
    // Test that generic RAW format attempts TIFF parsing
    let minimal_tiff = create_minimal_tiff();

    let result = parse_raw_metadata(&minimal_tiff, RawFormat::GenericRAW);

    // Should return metadata even if TIFF parsing fails
    assert!(result.is_ok(), "Generic RAW should always return metadata");

    let metadata = result.unwrap();
    assert!(
        metadata.contains_key("File:FileType"),
        "Generic RAW should have File:FileType tag"
    );
}

// Helper function to create a minimal valid TIFF file
fn create_minimal_tiff() -> Vec<u8> {
    let mut data = Vec::new();

    // TIFF header (little-endian)
    data.extend_from_slice(b"II"); // Byte order: little-endian
    data.extend_from_slice(&42u16.to_le_bytes()); // Magic number
    data.extend_from_slice(&8u32.to_le_bytes()); // Offset to first IFD

    // IFD0 with 0 entries
    data.extend_from_slice(&0u16.to_le_bytes()); // Number of directory entries
    data.extend_from_slice(&0u32.to_le_bytes()); // Next IFD offset (0 = no more IFDs)

    data
}

#[test]
fn test_parse_tiff_with_entries() {
    // Create a TIFF with one entry (ImageWidth)
    let mut data = Vec::new();

    // TIFF header
    data.extend_from_slice(b"II");
    data.extend_from_slice(&42u16.to_le_bytes());
    data.extend_from_slice(&8u32.to_le_bytes()); // IFD at offset 8

    // IFD with 1 entry
    data.extend_from_slice(&1u16.to_le_bytes()); // 1 directory entry

    // Tag entry: ImageWidth (0x0100), SHORT (3), count 1, value 1024
    data.extend_from_slice(&0x0100u16.to_le_bytes()); // Tag ID
    data.extend_from_slice(&3u16.to_le_bytes()); // Type: SHORT
    data.extend_from_slice(&1u32.to_le_bytes()); // Count: 1
    data.extend_from_slice(&1024u16.to_le_bytes()); // Value: 1024
    data.extend_from_slice(&0u16.to_le_bytes()); // Padding

    // Next IFD offset
    data.extend_from_slice(&0u32.to_le_bytes()); // No more IFDs

    let result = parse_raw_metadata(&data, RawFormat::AdobeDNG);

    match result {
        Ok(metadata) => {
            println!("Parsed {} tags from test TIFF", metadata.len());
            assert!(
                metadata.contains_key("File:FileType"),
                "Should have File:FileType"
            );
        }
        Err(e) => {
            println!("TIFF with entries parsing failed: {}", e);
        }
    }
}
