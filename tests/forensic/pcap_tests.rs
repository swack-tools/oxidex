//! Comprehensive integration tests for PCAP/PCAP-NG parser
//!
//! Tests verify:
//! - PCAP Ethernet and WiFi link type detection
//! - Timestamp extraction and packet counting
//! - Byte order (big-endian/little-endian) detection
//! - Nanosecond precision timestamps
//! - Duration calculation from multi-packet captures
//!
//! Uses TestReader pattern with synthetic PCAP data.

#[path = "../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::{FormatParser, TagValue};
use oxidex::parsers::specialized::pcap::PCAPParser;

/// Helper function to create minimal PCAP files with packets
///
/// # Arguments
/// * `link_type` - PCAP link layer type (1=Ethernet, 105=WiFi, etc.)
/// * `packet_count` - Number of packets to add to the file
/// * `magic` - PCAP magic number as bytes (for byte order and precision tests)
/// * `little_endian` - Whether to use little-endian encoding for fields
///
/// # Returns
/// A complete PCAP file with global header and specified packets
fn create_pcap_with_packets(
    link_type: u32,
    packet_count: u32,
    magic: u32,
    little_endian: bool,
) -> Vec<u8> {
    let mut data = Vec::new();

    // Helper function to write u16 in the specified endianness
    let write_u16 = |data: &mut Vec<u8>, val: u16| {
        if little_endian {
            data.extend_from_slice(&val.to_le_bytes());
        } else {
            data.extend_from_slice(&val.to_be_bytes());
        }
    };

    // Helper function to write u32 in the specified endianness
    let write_u32 = |data: &mut Vec<u8>, val: u32| {
        if little_endian {
            data.extend_from_slice(&val.to_le_bytes());
        } else {
            data.extend_from_slice(&val.to_be_bytes());
        }
    };

    // Helper function to write i32 in the specified endianness
    let write_i32 = |data: &mut Vec<u8>, val: i32| {
        if little_endian {
            data.extend_from_slice(&val.to_le_bytes());
        } else {
            data.extend_from_slice(&val.to_be_bytes());
        }
    };

    // Write magic number as raw bytes (matches parser's byte pattern checks)
    if magic == 0xd4c3b2a1 {
        // Little-endian PCAP magic
        data.extend_from_slice(&[0xd4, 0xc3, 0xb2, 0xa1]);
    } else if magic == 0xa1b2c3d4 {
        // Big-endian PCAP magic
        data.extend_from_slice(&[0xa1, 0xb2, 0xc3, 0xd4]);
    } else if magic == 0x4d3cb2a1 {
        // Little-endian nanosecond PCAP magic
        data.extend_from_slice(&[0x4d, 0x3c, 0xb2, 0xa1]);
    } else if magic == 0xa1b23c4d {
        // Big-endian nanosecond PCAP magic
        data.extend_from_slice(&[0xa1, 0xb2, 0x3c, 0x4d]);
    } else {
        // Fallback: write as-is
        write_u32(&mut data, magic);
    }

    // Version major: 2
    write_u16(&mut data, 2);

    // Version minor: 4
    write_u16(&mut data, 4);

    // Thiszone (GMT offset, usually 0): 0
    write_i32(&mut data, 0);

    // Sigfigs (timestamp accuracy, usually 0): 0
    write_u32(&mut data, 0);

    // Snaplen (max packet length: 65535)
    write_u32(&mut data, 65535);

    // Link layer type
    write_u32(&mut data, link_type);

    // Add packets
    for packet_idx in 0..packet_count {
        // Packet timestamp (seconds since epoch)
        let ts_sec = 1609459200u32 + packet_idx; // Jan 1, 2021 + offset

        // Packet timestamp (microseconds)
        let ts_usec = 0u32;

        // Packet data: simple Ethernet frame (dest MAC + src MAC + EtherType)
        let packet_data = vec![
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // Broadcast destination
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, // Source MAC
            0x08, 0x00, // EtherType (IPv4)
        ];

        let packet_len = packet_data.len() as u32;

        // Write packet header
        write_u32(&mut data, ts_sec);
        write_u32(&mut data, ts_usec);
        write_u32(&mut data, packet_len);
        write_u32(&mut data, packet_len); // orig_len = incl_len

        // Write packet data
        data.extend_from_slice(&packet_data);
    }

    data
}

/// Creates a minimal valid PCAP-NG file with Interface Description Block
fn create_pcapng_with_idb(link_type: u16, interface_name: Option<&str>) -> Vec<u8> {
    let mut data = Vec::new();

    // Section Header Block (SHB)
    // Block type: 0x0a0d0d0a (magic, little-endian encoding)
    data.extend_from_slice(&0x0a0d0d0au32.to_le_bytes());

    // SHB length: 28 bytes (basic SHB without options)
    data.extend_from_slice(&28u32.to_le_bytes());

    // Byte order magic (0x1a2b3c4d = little-endian)
    data.extend_from_slice(&0x1a2b3c4du32.to_le_bytes());

    // Version major (1)
    data.extend_from_slice(&1u16.to_le_bytes());

    // Version minor (0)
    data.extend_from_slice(&0u16.to_le_bytes());

    // Section length (64-bit, -1 = unknown)
    data.extend_from_slice(&0xffffffffffffffffu64.to_le_bytes());

    // SHB length again (at end)
    data.extend_from_slice(&28u32.to_le_bytes());

    // Interface Description Block (IDB)
    // Block type: 0x00000001
    data.extend_from_slice(&1u32.to_le_bytes());

    // Calculate IDB length
    let mut idb_data = Vec::new();

    // Link type (2 bytes)
    idb_data.extend_from_slice(&link_type.to_le_bytes());

    // Reserved (2 bytes)
    idb_data.extend_from_slice(&0u16.to_le_bytes());

    // Snaplen (4 bytes): 65535
    idb_data.extend_from_slice(&65535u32.to_le_bytes());

    // Options
    if let Some(name) = interface_name {
        // Option code: 2 (if_name)
        idb_data.extend_from_slice(&2u16.to_le_bytes());

        // Option length (rounded to 4-byte boundary)
        let name_len = name.len();
        let padded_len = ((name_len + 3) / 4) * 4;
        idb_data.extend_from_slice(&(name_len as u16).to_le_bytes());

        // Interface name value
        idb_data.extend_from_slice(name.as_bytes());

        // Padding
        for _ in 0..(padded_len - name_len) {
            idb_data.push(0);
        }
    }

    // End of options (option code 0, length 0)
    idb_data.extend_from_slice(&0u16.to_le_bytes());
    idb_data.extend_from_slice(&0u16.to_le_bytes());

    // IDB block length
    let idb_length = 12u32 + idb_data.len() as u32; // 4 (type) + 4 (length) + data + 4 (length repeat)
    data.extend_from_slice(&idb_length.to_le_bytes());

    // IDB data
    data.extend_from_slice(&idb_data);

    // IDB block length again (at end)
    data.extend_from_slice(&idb_length.to_le_bytes());

    data
}

// ============================================================================
// PCAP Tests
// ============================================================================

/// Test 1: PCAP with Ethernet link type
///
/// Verifies:
/// - LinkTypeName is "Ethernet"
/// - PacketCount is correct
/// - Basic PCAP parsing works
#[test]
fn test_pcap_ethernet() {
    let data = create_pcap_with_packets(
        1,          // Link type: Ethernet
        5,          // 5 packets
        0xd4c3b2a1, // Little-endian PCAP magic
        true,       // Little-endian
    );

    let reader = TestReader::new(data);
    let parser = PCAPParser;
    let metadata = parser.parse(&reader).expect("Failed to parse PCAP");

    // Verify link type name
    assert_eq!(
        metadata.get("PCAP:LinkTypeName"),
        Some(&TagValue::String("Ethernet".to_string())),
        "LinkTypeName should be 'Ethernet'"
    );

    // Verify link type number
    assert_eq!(
        metadata.get("PCAP:LinkType"),
        Some(&TagValue::String("1".to_string())),
        "LinkType should be '1'"
    );

    // Verify packet count
    assert_eq!(
        metadata.get("PCAP:PacketCount"),
        Some(&TagValue::String("5".to_string())),
        "PacketCount should be '5'"
    );

    // Verify version
    assert_eq!(
        metadata.get("PCAP:Version"),
        Some(&TagValue::String("2.4".to_string())),
        "Version should be '2.4'"
    );
}

/// Test 2: PCAP with IEEE 802.11 (WiFi) link type
///
/// Verifies:
/// - LinkTypeName is "IEEE 802.11 (WiFi)"
/// - Link type 105 is correctly detected
#[test]
fn test_pcap_wifi() {
    let data = create_pcap_with_packets(
        105,        // Link type: IEEE 802.11 (WiFi)
        3,          // 3 packets
        0xd4c3b2a1, // Little-endian PCAP magic
        true,       // Little-endian
    );

    let reader = TestReader::new(data);
    let parser = PCAPParser;
    let metadata = parser.parse(&reader).expect("Failed to parse PCAP");

    // Verify link type name
    assert_eq!(
        metadata.get("PCAP:LinkTypeName"),
        Some(&TagValue::String("IEEE 802.11 (WiFi)".to_string())),
        "LinkTypeName should be 'IEEE 802.11 (WiFi)'"
    );

    // Verify link type number
    assert_eq!(
        metadata.get("PCAP:LinkType"),
        Some(&TagValue::String("105".to_string())),
        "LinkType should be '105'"
    );

    // Verify packet count
    assert_eq!(
        metadata.get("PCAP:PacketCount"),
        Some(&TagValue::String("3".to_string())),
        "PacketCount should be '3'"
    );
}

/// Test 3: PCAP timestamp extraction
///
/// Verifies:
/// - FirstPacketTime is extracted
/// - LastPacketTime is extracted
/// - Timestamps are in ISO 8601 format
#[test]
fn test_pcap_timestamps() {
    let data = create_pcap_with_packets(
        1,          // Ethernet
        3,          // 3 packets with different timestamps
        0xd4c3b2a1, // Little-endian PCAP magic
        true,       // Little-endian
    );

    let reader = TestReader::new(data);
    let parser = PCAPParser;
    let metadata = parser.parse(&reader).expect("Failed to parse PCAP");

    // Verify first packet time exists
    assert!(
        metadata.contains_key("PCAP:FirstPacketTime"),
        "FirstPacketTime should exist"
    );

    // Verify last packet time exists
    assert!(
        metadata.contains_key("PCAP:LastPacketTime"),
        "LastPacketTime should exist"
    );

    // Verify timestamps are strings (ISO format check via simple pattern)
    if let Some(TagValue::String(first)) = metadata.get("PCAP:FirstPacketTime") {
        assert!(
            first.contains("T"),
            "FirstPacketTime should be in ISO format"
        );
        assert!(first.contains("Z"), "FirstPacketTime should end with Z");
    }

    if let Some(TagValue::String(last)) = metadata.get("PCAP:LastPacketTime") {
        assert!(last.contains("T"), "LastPacketTime should be in ISO format");
        assert!(last.contains("Z"), "LastPacketTime should end with Z");
    }
}

/// Test 4: PCAP duration calculation
///
/// Verifies:
/// - Duration tag exists for multi-packet captures
/// - Duration is calculated correctly from first and last packet timestamps
#[test]
fn test_pcap_duration() {
    let data = create_pcap_with_packets(
        1,          // Ethernet
        10,         // 10 packets spanning multiple seconds
        0xd4c3b2a1, // Little-endian PCAP magic
        true,       // Little-endian
    );

    let reader = TestReader::new(data);
    let parser = PCAPParser;
    let metadata = parser.parse(&reader).expect("Failed to parse PCAP");

    // Verify duration exists (should span 9 seconds for 10 packets with 1s interval)
    assert!(
        metadata.contains_key("PCAP:Duration"),
        "Duration should exist for multi-packet capture"
    );

    // Verify duration format
    if let Some(TagValue::String(duration)) = metadata.get("PCAP:Duration") {
        assert!(!duration.is_empty(), "Duration should not be empty");
        // Duration format should contain 'second' or 's'
        assert!(
            duration.contains("second") || duration.contains("s"),
            "Duration should specify time unit"
        );
    }
}

/// Test 5: PCAP big-endian byte order detection
///
/// Verifies:
/// - ByteOrder is "Big-endian" for 0xa1b2c3d4 magic
/// - Parser correctly handles big-endian encoded data
#[test]
fn test_pcap_big_endian() {
    let data = create_pcap_with_packets(
        1,          // Ethernet
        2,          // 2 packets
        0xa1b2c3d4, // Big-endian PCAP magic
        false,      // Big-endian
    );

    let reader = TestReader::new(data);
    let parser = PCAPParser;
    let metadata = parser.parse(&reader).expect("Failed to parse PCAP");

    // Verify byte order
    assert_eq!(
        metadata.get("PCAP:ByteOrder"),
        Some(&TagValue::String("Big-endian".to_string())),
        "ByteOrder should be 'Big-endian' for 0xa1b2c3d4 magic"
    );

    // Verify other fields are correctly parsed
    assert_eq!(
        metadata.get("PCAP:LinkTypeName"),
        Some(&TagValue::String("Ethernet".to_string())),
        "LinkTypeName should still be parsed correctly in big-endian"
    );
}

/// Test 6: PCAP nanosecond timestamp precision
///
/// Verifies:
/// - TimestampPrecision is "Nanoseconds" for 0x4d3cb2a1 magic
/// - Parser correctly identifies nanosecond magic number
#[test]
fn test_pcap_nanosecond() {
    let data = create_pcap_with_packets(
        1,          // Ethernet
        2,          // 2 packets
        0x4d3cb2a1, // Little-endian PCAP nanosecond magic
        true,       // Little-endian
    );

    let reader = TestReader::new(data);
    let parser = PCAPParser;
    let metadata = parser.parse(&reader).expect("Failed to parse PCAP");

    // Verify timestamp precision
    assert_eq!(
        metadata.get("PCAP:TimestampPrecision"),
        Some(&TagValue::String("Nanoseconds".to_string())),
        "TimestampPrecision should be 'Nanoseconds' for 0x4d3cb2a1 magic"
    );

    // Verify other fields are still correctly parsed
    assert_eq!(
        metadata.get("PCAP:LinkTypeName"),
        Some(&TagValue::String("Ethernet".to_string())),
        "LinkTypeName should still be parsed correctly with nanosecond precision"
    );
}

// ============================================================================
// PCAP-NG Tests
// ============================================================================

/// Test 7: PCAP-NG with Ethernet Interface Description Block
///
/// Verifies:
/// - PCAP-NG format is correctly detected
/// - LinkTypeName is "Ethernet" for IDB link type 1
#[test]
fn test_pcapng_ethernet() {
    let data = create_pcapng_with_idb(1, Some("eth0"));

    let reader = TestReader::new(data);
    let parser = PCAPParser;
    let metadata = parser.parse(&reader).expect("Failed to parse PCAP-NG");

    // Verify PCAP-NG is detected
    assert!(
        metadata.contains_key("PCAPNG:LinkType"),
        "PCAPNG:LinkType should exist"
    );

    // Verify link type name
    assert_eq!(
        metadata.get("PCAPNG:LinkTypeName"),
        Some(&TagValue::String("Ethernet".to_string())),
        "PCAPNG LinkTypeName should be 'Ethernet'"
    );
}

/// Test 8: PCAP-NG with WiFi Interface Description Block
///
/// Verifies:
/// - LinkTypeName is "IEEE 802.11 (WiFi)" for IDB link type 105
#[test]
fn test_pcapng_wifi() {
    let data = create_pcapng_with_idb(105, Some("wlan0"));

    let reader = TestReader::new(data);
    let parser = PCAPParser;
    let metadata = parser.parse(&reader).expect("Failed to parse PCAP-NG");

    // Verify PCAP-NG is detected
    assert!(
        metadata.contains_key("PCAPNG:LinkType"),
        "PCAPNG:LinkType should exist"
    );

    // Verify link type name
    assert_eq!(
        metadata.get("PCAPNG:LinkTypeName"),
        Some(&TagValue::String("IEEE 802.11 (WiFi)".to_string())),
        "PCAPNG LinkTypeName should be 'IEEE 802.11 (WiFi)'"
    );
}

/// Test 9: PCAP-NG Interface Name option parsing
///
/// Verifies:
/// - InterfaceName option is correctly extracted from IDB
/// - Option parsing handles padded strings correctly
#[test]
fn test_pcapng_interface_name() {
    let data = create_pcapng_with_idb(1, Some("eth0"));

    let reader = TestReader::new(data);
    let parser = PCAPParser;
    let metadata = parser.parse(&reader).expect("Failed to parse PCAP-NG");

    // Verify interface name is extracted
    assert_eq!(
        metadata.get("PCAPNG:InterfaceName"),
        Some(&TagValue::String("eth0".to_string())),
        "InterfaceName should be extracted from IDB options"
    );
}

/// Test 10: PCAP-NG with various interface types
///
/// Verifies multiple common link types are correctly named
#[test]
fn test_pcapng_multiple_link_types() {
    // Test cases: (link_type, expected_name)
    let test_cases = vec![
        (0, "BSD Loopback"),
        (1, "Ethernet"),
        (9, "PPP"),
        (105, "IEEE 802.11 (WiFi)"),
        (113, "Linux Cooked Capture"),
    ];

    for (link_type, expected_name) in test_cases {
        let data = create_pcapng_with_idb(link_type, None);
        let reader = TestReader::new(data);
        let parser = PCAPParser;

        let metadata = parser
            .parse(&reader)
            .unwrap_or_else(|_| panic!("Failed to parse PCAP-NG with link type {}", link_type));

        assert_eq!(
            metadata.get("PCAPNG:LinkTypeName"),
            Some(&TagValue::String(expected_name.to_string())),
            "LinkTypeName should be '{}' for link type {}",
            expected_name,
            link_type
        );
    }
}

// ============================================================================
// Edge Cases and Forensic Value Tests
// ============================================================================

/// Test 11: PCAP with zero packets
///
/// Verifies:
/// - Parser handles minimal PCAP file (header only)
/// - PacketCount is 0
#[test]
fn test_pcap_zero_packets() {
    let data = create_pcap_with_packets(
        1,          // Ethernet
        0,          // No packets
        0xd4c3b2a1, // Little-endian PCAP magic
        true,       // Little-endian
    );

    let reader = TestReader::new(data);
    let parser = PCAPParser;
    let metadata = parser.parse(&reader).expect("Failed to parse PCAP");

    assert_eq!(
        metadata.get("PCAP:PacketCount"),
        Some(&TagValue::String("0".to_string())),
        "PacketCount should be 0 for header-only PCAP"
    );
}

/// Test 12: PCAP large packet count
///
/// Verifies:
/// - Parser correctly counts large numbers of packets
/// - No overflow or panic
#[test]
fn test_pcap_large_packet_count() {
    let data = create_pcap_with_packets(
        1,          // Ethernet
        1000,       // 1000 packets
        0xd4c3b2a1, // Little-endian PCAP magic
        true,       // Little-endian
    );

    let reader = TestReader::new(data);
    let parser = PCAPParser;
    let metadata = parser.parse(&reader).expect("Failed to parse PCAP");

    assert_eq!(
        metadata.get("PCAP:PacketCount"),
        Some(&TagValue::String("1000".to_string())),
        "PacketCount should be 1000"
    );
}

/// Test 13: PCAP snaplen value extraction
///
/// Verifies:
/// - SnapLen field is correctly parsed and formatted
#[test]
fn test_pcap_snaplen() {
    let data = create_pcap_with_packets(
        1,          // Ethernet
        1,          // 1 packet
        0xd4c3b2a1, // Little-endian PCAP magic
        true,       // Little-endian
    );

    let reader = TestReader::new(data);
    let parser = PCAPParser;
    let metadata = parser.parse(&reader).expect("Failed to parse PCAP");

    // Verify snaplen is extracted (should be 65535 as set in helper)
    assert!(
        metadata.contains_key("PCAP:SnapLen"),
        "SnapLen should be extracted"
    );

    if let Some(TagValue::String(snaplen)) = metadata.get("PCAP:SnapLen") {
        assert!(
            snaplen.contains("65535"),
            "SnapLen should contain value 65535"
        );
        assert!(
            snaplen.contains("bytes"),
            "SnapLen should include unit 'bytes'"
        );
    }
}

/// Test 14: PCAP version parsing
///
/// Verifies:
/// - Version major and minor are correctly parsed
/// - Version format is "major.minor"
#[test]
fn test_pcap_version() {
    let data = create_pcap_with_packets(
        1,          // Ethernet
        1,          // 1 packet
        0xd4c3b2a1, // Little-endian PCAP magic
        true,       // Little-endian
    );

    let reader = TestReader::new(data);
    let parser = PCAPParser;
    let metadata = parser.parse(&reader).expect("Failed to parse PCAP");

    // Version should be 2.4
    assert_eq!(
        metadata.get("PCAP:Version"),
        Some(&TagValue::String("2.4".to_string())),
        "Version should be '2.4'"
    );

    assert_eq!(
        metadata.get("PCAP:VersionMajor"),
        Some(&TagValue::String("2".to_string())),
        "VersionMajor should be '2'"
    );

    assert_eq!(
        metadata.get("PCAP:VersionMinor"),
        Some(&TagValue::String("4".to_string())),
        "VersionMinor should be '4'"
    );
}

/// Test 15: PCAP timezone offset
///
/// Verifies:
/// - TimeZone tag is extracted (usually 0)
#[test]
fn test_pcap_timezone() {
    let data = create_pcap_with_packets(
        1,          // Ethernet
        1,          // 1 packet
        0xd4c3b2a1, // Little-endian PCAP magic
        true,       // Little-endian
    );

    let reader = TestReader::new(data);
    let parser = PCAPParser;
    let metadata = parser.parse(&reader).expect("Failed to parse PCAP");

    // TimeZone should exist
    assert!(
        metadata.contains_key("PCAP:TimeZone"),
        "TimeZone should be extracted"
    );

    if let Some(TagValue::String(tz)) = metadata.get("PCAP:TimeZone") {
        assert!(
            tz.contains("seconds"),
            "TimeZone should include unit 'seconds'"
        );
    }
}
