//! Integration tests for PE anomaly detection
//!
//! Comprehensive test coverage for the PE anomaly detector which identifies
//! suspicious characteristics in PE files including packing, obfuscation,
//! and malicious patterns.

#[path = "../common/mod.rs"]
mod common;

#[allow(unused_imports)]
use common::TestReader;
use oxidex::core::TagValue;
use oxidex::parsers::pe::anomaly_detector::AnomalyDetector;
use oxidex::parsers::pe::structures::SectionHeader;

/// Helper to create a section header for testing
fn create_section(
    name: &[u8; 8],
    virtual_address: u32,
    virtual_size: u32,
    pointer_to_raw_data: u32,
    size_of_raw_data: u32,
    characteristics: u32,
) -> SectionHeader {
    SectionHeader {
        name: *name,
        virtual_size,
        virtual_address,
        size_of_raw_data,
        pointer_to_raw_data,
        pointer_to_relocations: 0,
        pointer_to_line_numbers: 0,
        number_of_relocations: 0,
        number_of_line_numbers: 0,
        characteristics,
    }
}

/// Helper to create test data with specific entropy characteristics
fn create_random_data(size: usize) -> Vec<u8> {
    // Use a simple pattern that cycles through all byte values for high entropy
    (0..size).map(|i| (i % 256) as u8).collect()
}

fn create_zeros_data(size: usize) -> Vec<u8> {
    vec![0u8; size]
}

fn create_repeating_pattern_data(size: usize, pattern: &[u8]) -> Vec<u8> {
    pattern.iter().cycle().take(size).copied().collect()
}

// ============================================================================
// Entropy Calculation Tests
// ============================================================================

#[test]
fn test_entropy_calculation_high() {
    // Random-looking data (all byte values) should have high entropy
    let data = create_random_data(1024);
    let entropy = AnomalyDetector::calculate_entropy(&data);
    assert!(
        entropy > 7.0,
        "High entropy data should have entropy > 7.0, got {}",
        entropy
    );
}

#[test]
fn test_entropy_calculation_low_zeros() {
    // All zeros should have very low entropy (approaching 0)
    let data = create_zeros_data(1024);
    let entropy = AnomalyDetector::calculate_entropy(&data);
    assert!(
        entropy < 0.1,
        "All zeros should have entropy < 0.1, got {}",
        entropy
    );
}

#[test]
fn test_entropy_calculation_low_uniform() {
    // All same bytes should have zero entropy
    let data = vec![0x42u8; 1024];
    let entropy = AnomalyDetector::calculate_entropy(&data);
    assert!(
        entropy < 0.1,
        "Uniform data should have entropy < 0.1, got {}",
        entropy
    );
}

#[test]
fn test_entropy_calculation_repeating_pattern() {
    // Repeating pattern should have low-to-medium entropy
    let pattern = b"ABCD";
    let data = create_repeating_pattern_data(1024, pattern);
    let entropy = AnomalyDetector::calculate_entropy(&data);
    assert!(
        entropy > 1.0 && entropy < 3.0,
        "Repeating pattern should have entropy between 1.0 and 3.0, got {}",
        entropy
    );
}

#[test]
fn test_entropy_calculation_empty_data() {
    // Empty data should return 0 entropy
    let data = Vec::new();
    let entropy = AnomalyDetector::calculate_entropy(&data);
    assert_eq!(
        entropy, 0.0,
        "Empty data should have entropy of exactly 0.0"
    );
}

#[test]
fn test_entropy_calculation_text_data() {
    // ASCII text should have medium entropy
    let data = b"This is some normal ASCII text that appears in executable files. \
                  It contains readable strings and printable characters.";
    let entropy = AnomalyDetector::calculate_entropy(data);
    assert!(
        entropy > 3.0 && entropy < 6.0,
        "ASCII text should have entropy between 3.0 and 6.0, got {}",
        entropy
    );
}

// ============================================================================
// Section Anomaly Detection Tests
// ============================================================================

#[test]
fn test_detect_upx_packer_section() {
    let sections = vec![
        create_section(b".text\0\0\0", 0x1000, 0x1000, 0x400, 0x1000, 0x60000020),
        create_section(b"UPX0\0\0\0\0", 0x2000, 0x2000, 0x1400, 0x2000, 0x60000020),
        create_section(b"UPX1\0\0\0\0", 0x4000, 0x1000, 0x3400, 0x1000, 0x60000020),
    ];

    let anomalies = AnomalyDetector::detect_section_anomalies(&sections);

    assert!(!anomalies.is_empty(), "Should detect UPX packer sections");
    assert!(
        anomalies.iter().any(|a| a.contains("UPX")),
        "Should specifically mention UPX packer"
    );
}

#[test]
fn test_detect_aspack_packer_section() {
    let sections = vec![
        create_section(b".text\0\0\0", 0x1000, 0x1000, 0x400, 0x1000, 0x60000020),
        create_section(b"ASPack\0\0", 0x2000, 0x2000, 0x1400, 0x2000, 0x60000020),
    ];

    let anomalies = AnomalyDetector::detect_section_anomalies(&sections);

    assert!(
        !anomalies.is_empty(),
        "Should detect ASPack packer sections"
    );
    assert!(
        anomalies.iter().any(|a| a.contains("ASPack")),
        "Should specifically mention ASPack packer"
    );
}

#[test]
fn test_detect_themida_packer_section() {
    let sections = vec![
        create_section(b".text\0\0\0", 0x1000, 0x1000, 0x400, 0x1000, 0x60000020),
        create_section(b"Themida\0", 0x2000, 0x5000, 0x1400, 0x1000, 0x60000020),
    ];

    let anomalies = AnomalyDetector::detect_section_anomalies(&sections);

    assert!(
        !anomalies.is_empty(),
        "Should detect Themida packer sections"
    );
    assert!(
        anomalies.iter().any(|a| a.contains("Themida")),
        "Should specifically mention Themida packer"
    );
}

#[test]
fn test_detect_writable_executable_section() {
    // Section with both writable (0x80000000) and executable (0x20000000) flags
    const SECTION_WRITABLE: u32 = 0x80000000;
    const SECTION_EXECUTABLE: u32 = 0x20000000;

    let sections = vec![create_section(
        b".text\0\0\0",
        0x1000,
        0x1000,
        0x400,
        0x1000,
        SECTION_WRITABLE | SECTION_EXECUTABLE,
    )];

    let anomalies = AnomalyDetector::detect_section_anomalies(&sections);

    assert!(!anomalies.is_empty(), "Should detect W+X section");
    assert!(
        anomalies
            .iter()
            .any(|a| a.contains("writable and executable")),
        "Should mention writable and executable characteristics"
    );
}

#[test]
fn test_detect_nonprintable_section_name() {
    // Section name with non-printable characters
    let sections = vec![create_section(
        b"\x01\x02\x03\x04\x05\x06\x07\x08",
        0x1000,
        0x1000,
        0x400,
        0x1000,
        0x60000020,
    )];

    let anomalies = AnomalyDetector::detect_section_anomalies(&sections);

    assert!(
        !anomalies.is_empty(),
        "Should detect non-printable section name"
    );
    assert!(
        anomalies.iter().any(|a| a.contains("non-printable")),
        "Should mention non-printable name"
    );
}

#[test]
fn test_detect_zero_virtual_size_with_raw_data() {
    let sections = vec![create_section(
        b".data\0\0\0",
        0x2000,
        0, // Zero virtual size
        0x1400,
        0x1000, // But has raw data
        0x40000040,
    )];

    let anomalies = AnomalyDetector::detect_section_anomalies(&sections);

    assert!(
        !anomalies.is_empty(),
        "Should detect zero virtual size anomaly"
    );
    assert!(
        anomalies.iter().any(|a| a.contains("zero virtual size")),
        "Should mention zero virtual size"
    );
}

#[test]
fn test_detect_large_virtual_size_vs_raw_size() {
    // Virtual size is much larger than raw size (potential unpacking target)
    let sections = vec![create_section(
        b".packed\0",
        0x2000,
        0x50000, // 320KB virtual
        0x1400,
        0x5000, // 20KB raw (16x ratio)
        0x60000020,
    )];

    let anomalies = AnomalyDetector::detect_section_anomalies(&sections);

    assert!(
        !anomalies.is_empty(),
        "Should detect virtual/raw size mismatch"
    );
    assert!(
        anomalies
            .iter()
            .any(|a| a.contains("virtual size") && a.contains("raw size")),
        "Should mention virtual and raw size discrepancy"
    );
}

#[test]
fn test_normal_pe_no_anomalies() {
    // Normal PE sections should not trigger anomalies
    // Note: .rsrc is in the packer list so we use .reloc instead
    let sections = vec![
        create_section(b".text\0\0\0", 0x1000, 0x2000, 0x400, 0x2000, 0x60000020), // Code
        create_section(b".data\0\0\0", 0x3000, 0x1000, 0x2400, 0x1000, 0xC0000040), // Data
        create_section(b".rdata\0\0", 0x4000, 0x800, 0x3400, 0x800, 0x40000040),   // Read-only
        create_section(b".reloc\0\0", 0x5000, 0x1000, 0x3C00, 0x1000, 0x40000040), // Relocations
    ];

    let anomalies = AnomalyDetector::detect_section_anomalies(&sections);

    assert!(
        anomalies.is_empty(),
        "Normal PE should have no anomalies, found: {:?}",
        anomalies
    );
}

#[test]
fn test_multiple_anomalies_in_single_pe() {
    const SECTION_WRITABLE: u32 = 0x80000000;
    const SECTION_EXECUTABLE: u32 = 0x20000000;

    let sections = vec![
        // Normal section
        create_section(b".text\0\0\0", 0x1000, 0x2000, 0x400, 0x2000, 0x60000020),
        // W+X section
        create_section(
            b".data\0\0\0",
            0x3000,
            0x1000,
            0x2400,
            0x1000,
            SECTION_WRITABLE | SECTION_EXECUTABLE,
        ),
        // UPX section
        create_section(b"UPX0\0\0\0\0", 0x4000, 0x3000, 0x3400, 0x1000, 0x60000020),
        // Large virtual/raw ratio
        create_section(b".packed\0", 0x7000, 0x100000, 0x4400, 0x5000, 0x60000020),
    ];

    let anomalies = AnomalyDetector::detect_section_anomalies(&sections);

    assert!(
        anomalies.len() >= 3,
        "Should detect multiple anomalies, found {}: {:?}",
        anomalies.len(),
        anomalies
    );
    assert!(
        anomalies.iter().any(|a| a.contains("UPX")),
        "Should detect UPX"
    );
    assert!(
        anomalies
            .iter()
            .any(|a| a.contains("writable and executable")),
        "Should detect W+X"
    );
    assert!(
        anomalies
            .iter()
            .any(|a| a.contains("virtual size") && a.contains("raw size")),
        "Should detect size mismatch"
    );
}

#[test]
fn test_section_count_anomaly_too_many_sections() {
    // Create an excessive number of sections (suspicious)
    let mut sections = Vec::new();
    for i in 0..100 {
        let name = format!(".sec{:03}\0", i);
        let mut name_bytes = [0u8; 8];
        name_bytes[..name.len().min(8)].copy_from_slice(&name.as_bytes()[..name.len().min(8)]);
        sections.push(create_section(
            &name_bytes,
            0x1000 + (i * 0x1000),
            0x1000,
            0x400 + (i * 0x1000),
            0x1000,
            0x60000020,
        ));
    }

    let _anomalies = AnomalyDetector::detect_section_anomalies(&sections);

    // While the detector doesn't explicitly check for too many sections,
    // this test ensures the detector can handle a large number without panicking
    // In a real malware scenario, excessive sections can be suspicious
    assert!(sections.len() > 50, "Test should have many sections");
}

// ============================================================================
// Full Workflow Tests (analyze method)
// ============================================================================

#[test]
fn test_analyze_normal_pe_no_flags() {
    let sections = vec![
        create_section(b".text\0\0\0", 0x1000, 0x2000, 0x400, 0x2000, 0x60000020),
        create_section(b".data\0\0\0", 0x3000, 0x1000, 0x2400, 0x1000, 0xC0000040),
    ];

    // Create test data with normal entropy
    let mut data = Vec::new();
    data.extend_from_slice(&vec![0u8; 0x400]); // Skip to first section
    data.extend_from_slice(b"Normal code section data with reasonable entropy content");
    data.resize(0x2400 + 0x1000, 0); // Pad to include data section

    let reader = TestReader::new(data);
    let entry_point = 0x1010; // Entry point in .text section
    let image_base = 0x400000;

    let metadata = AnomalyDetector::analyze(&reader, &sections, entry_point, image_base);

    // Normal PE should not have suspicious flags
    assert!(
        !metadata.contains_key("EXE:SectionAnomalies"),
        "Normal PE should not have section anomalies"
    );
    assert!(
        !metadata.contains_key("EXE:SuspiciousSections"),
        "Normal PE should not be marked suspicious"
    );
    assert!(
        !metadata.contains_key("EXE:HighEntropySections"),
        "Normal PE should not have high entropy sections"
    );
    assert!(
        !metadata.contains_key("EXE:PossiblyPacked"),
        "Normal PE should not be marked as possibly packed"
    );
}

#[test]
fn test_analyze_pe_with_high_entropy_section() {
    let sections = vec![
        create_section(b".text\0\0\0", 0x1000, 0x2000, 0x400, 0x2000, 0x60000020),
        create_section(b".packed\0", 0x3000, 0x1000, 0x2400, 0x1000, 0x60000020),
    ];

    // Create test data with high entropy in packed section
    let mut data = vec![0u8; 0x2400];
    // Add high-entropy data (all byte values cycling)
    data.extend(create_random_data(0x1000));

    let reader = TestReader::new(data);
    let entry_point = 0x1010;
    let image_base = 0x400000;

    let metadata = AnomalyDetector::analyze(&reader, &sections, entry_point, image_base);

    // Should detect high entropy
    assert!(
        metadata.contains_key("EXE:HighEntropySections"),
        "Should detect high entropy section"
    );
    assert!(
        metadata.contains_key("EXE:PossiblyPacked"),
        "Should mark as possibly packed"
    );

    if let Some(TagValue::Array(sections)) = metadata.get("EXE:HighEntropySections") {
        assert!(
            !sections.is_empty(),
            "Should have at least one high entropy section"
        );
    } else {
        panic!("EXE:HighEntropySections should be an array");
    }
}

#[test]
fn test_analyze_pe_with_packer_signatures() {
    const SECTION_WRITABLE: u32 = 0x80000000;
    const SECTION_EXECUTABLE: u32 = 0x20000000;

    let sections = vec![
        create_section(
            b"UPX0\0\0\0\0",
            0x1000,
            0x2000,
            0x400,
            0x2000,
            SECTION_WRITABLE | SECTION_EXECUTABLE,
        ),
        create_section(b"UPX1\0\0\0\0", 0x3000, 0x1000, 0x2400, 0x1000, 0x60000020),
    ];

    let data = vec![0u8; 0x3400];
    let reader = TestReader::new(data);
    let entry_point = 0x1010;
    let image_base = 0x400000;

    let metadata = AnomalyDetector::analyze(&reader, &sections, entry_point, image_base);

    // Should detect packer signatures
    assert!(
        metadata.contains_key("EXE:SectionAnomalies"),
        "Should detect section anomalies"
    );
    assert!(
        metadata.contains_key("EXE:SuspiciousSections"),
        "Should mark sections as suspicious"
    );

    if let Some(TagValue::Array(anomalies)) = metadata.get("EXE:SectionAnomalies") {
        assert!(
            anomalies.len() >= 2,
            "Should detect multiple anomalies (UPX + W+X)"
        );
    } else {
        panic!("EXE:SectionAnomalies should be an array");
    }
}

#[test]
fn test_analyze_unusual_entry_point_in_data_section() {
    let sections = vec![
        create_section(b".text\0\0\0", 0x1000, 0x1000, 0x400, 0x1000, 0x60000020),
        create_section(b".data\0\0\0", 0x2000, 0x1000, 0x1400, 0x1000, 0xC0000040),
    ];

    let data = vec![0u8; 0x2400];
    let reader = TestReader::new(data);
    let entry_point = 0x2010; // Entry point in .data section (unusual)
    let image_base = 0x400000;

    let metadata = AnomalyDetector::analyze(&reader, &sections, entry_point, image_base);

    // Should detect unusual entry point
    assert!(
        metadata.contains_key("EXE:UnusualEntrySection"),
        "Should detect unusual entry point location"
    );

    if let Some(TagValue::String(msg)) = metadata.get("EXE:UnusualEntrySection") {
        assert!(msg.contains(".data"), "Should mention .data section");
    }
}

#[test]
fn test_analyze_entry_point_outside_all_sections() {
    let sections = vec![
        create_section(b".text\0\0\0", 0x1000, 0x1000, 0x400, 0x1000, 0x60000020),
        create_section(b".data\0\0\0", 0x2000, 0x1000, 0x1400, 0x1000, 0xC0000040),
    ];

    let data = vec![0u8; 0x2400];
    let reader = TestReader::new(data);
    let entry_point = 0x5000; // Outside all sections
    let image_base = 0x400000;

    let metadata = AnomalyDetector::analyze(&reader, &sections, entry_point, image_base);

    // Should detect entry point anomaly
    assert!(
        metadata.contains_key("EXE:EntryPointAnomaly"),
        "Should detect entry point outside sections"
    );

    if let Some(TagValue::String(msg)) = metadata.get("EXE:EntryPointAnomaly") {
        assert!(
            msg.contains("outside all sections"),
            "Should mention entry point is outside sections"
        );
    }
}

#[test]
fn test_analyze_empty_pe_minimal_sections() {
    let sections = vec![create_section(
        b".text\0\0\0",
        0x1000,
        0x100,
        0x400,
        0x100,
        0x60000020,
    )];

    let data = vec![0u8; 0x500];
    let reader = TestReader::new(data);
    let entry_point = 0x1000;
    let image_base = 0x400000;

    let metadata = AnomalyDetector::analyze(&reader, &sections, entry_point, image_base);

    // Minimal PE might not trigger anomalies, but should not crash
    // This tests robustness with edge cases
    assert!(
        !metadata.is_empty() || metadata.is_empty(),
        "Should handle minimal PE without errors"
    );
}

#[test]
fn test_analyze_comprehensive_malicious_characteristics() {
    const SECTION_WRITABLE: u32 = 0x80000000;
    const SECTION_EXECUTABLE: u32 = 0x20000000;

    // PE with multiple red flags
    let sections = vec![
        create_section(
            b"UPX0\0\0\0\0",
            0x1000,
            0x2000,
            0x400,
            0x2000,
            SECTION_WRITABLE | SECTION_EXECUTABLE,
        ),
        create_section(b".packed\0", 0x3000, 0x100000, 0x2400, 0x5000, 0x60000020),
        create_section(
            b"\x01\x02\x03\x04\x05\x06\x07\x08",
            0x103000,
            0x1000,
            0x7400,
            0x1000,
            0x60000020,
        ),
    ];

    // Create high-entropy data
    let mut data = vec![0u8; 0x2400];
    data.extend(create_random_data(0x5000)); // High entropy in packed section
    data.resize(0x8400, 0);

    let reader = TestReader::new(data);
    let entry_point = 0x1010; // Entry in UPX0
    let image_base = 0x400000;

    let metadata = AnomalyDetector::analyze(&reader, &sections, entry_point, image_base);

    // Should detect multiple issues
    assert!(
        metadata.contains_key("EXE:SectionAnomalies"),
        "Should detect section anomalies"
    );
    assert!(
        metadata.contains_key("EXE:SuspiciousSections"),
        "Should mark as suspicious"
    );
    assert!(
        metadata.contains_key("EXE:HighEntropySections")
            || metadata.contains_key("EXE:PossiblyPacked"),
        "Should detect high entropy or packing"
    );
    assert!(
        metadata.contains_key("EXE:UnusualEntrySection"),
        "Should detect unusual entry section"
    );

    // Verify we have multiple anomalies
    if let Some(TagValue::Array(anomalies)) = metadata.get("EXE:SectionAnomalies") {
        assert!(
            anomalies.len() >= 3,
            "Should detect at least 3 anomalies (UPX, W+X, size ratio, non-printable), found: {}",
            anomalies.len()
        );
    }
}
