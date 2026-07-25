#[path = "../../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::FormatParser;
use oxidex::parsers::audio::flac::FlacParser;

#[test]
fn test_flac_magic_bytes() {
    let data = b"fLaC\x00\x00\x00\x22..."; // Mock FLAC file
    let reader = TestReader::new(data.to_vec());
    let parser = FlacParser;
    let result = parser.parse(&reader);

    // Should succeed with valid magic bytes
    assert!(result.is_ok());
}

#[test]
fn test_flac_invalid_magic() {
    let data = b"INVALID";
    let reader = TestReader::new(data.to_vec());
    let parser = FlacParser;
    let result = parser.parse(&reader);

    // Should fail with invalid magic bytes
    assert!(result.is_err());
}
