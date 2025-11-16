//! End-to-end integration tests for IPTC metadata extraction

use exiftool_rs::core::operations::read_metadata;
use std::path::Path;

#[test]
fn test_iptc_extraction_from_real_file() {
    // This test requires a real IPTC sample file
    // Download from: https://www.iptc.org/std/photometadata/examples/
    // For now, we'll skip if file doesn't exist

    let sample_path = Path::new("tests/fixtures/iptc_sample.jpg");

    if !sample_path.exists() {
        eprintln!("Skipping test: IPTC sample file not found at {:?}", sample_path);
        return;
    }

    let metadata = read_metadata(sample_path)
        .expect("Failed to read metadata from IPTC sample");

    // Verify IPTC tags were extracted
    assert!(
        metadata.contains_key("IPTC:ObjectName")
            || metadata.contains_key("IPTC:By-line")
            || metadata.contains_key("IPTC:Caption-Abstract"),
        "Expected at least one IPTC tag to be present"
    );
}
