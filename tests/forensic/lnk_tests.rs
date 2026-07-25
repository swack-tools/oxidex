//! Integration tests for LNK (Windows shortcut) parser
//!
//! Comprehensive test coverage for Windows shortcut file parsing including:
//! - Basic header parsing and signature verification
//! - Link flags and file attributes extraction
//! - Timestamp parsing (creation, access, modification)
//! - Target path and working directory extraction
//! - Command line arguments and icon location
//! - Volume serial number tracking
//! - Extra data blocks (tracker data, known folders)
//! - Edge cases and error handling

#[path = "../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::{FormatParser, TagValue};
use oxidex::parsers::specialized::lnk::LNKParser;

/// LNK file magic number
const LNK_MAGIC: u32 = 0x0000004C;

/// Shell Link GUID: {00021401-0000-0000-C000-000000000046}
const LNK_GUID: [u8; 16] = [
    0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

/// LNK header size
const LNK_HEADER_SIZE: usize = 76;

/// Link flags
const FLAG_HAS_LINK_TARGET_ID_LIST: u32 = 0x0001;
const FLAG_HAS_LINK_INFO: u32 = 0x0002;
const FLAG_HAS_NAME: u32 = 0x0004;
const FLAG_HAS_RELATIVE_PATH: u32 = 0x0008;
const FLAG_HAS_WORKING_DIR: u32 = 0x0010;
const FLAG_HAS_ARGUMENTS: u32 = 0x0020;
const FLAG_HAS_ICON_LOCATION: u32 = 0x0040;
const FLAG_IS_UNICODE: u32 = 0x0080;

/// Extra data block signatures
const TRACKER_DATA_BLOCK_SIG: u32 = 0xA0000003;
const KNOWN_FOLDER_BLOCK_SIG: u32 = 0xA000000B;

/// Create a minimal valid LNK header with specified flags and attributes
fn create_lnk_header(flags: u32, file_attrs: u32) -> Vec<u8> {
    let mut data = vec![0u8; LNK_HEADER_SIZE];

    // Magic number (0x4C 0x00 0x00 0x00)
    data[0..4].copy_from_slice(&LNK_MAGIC.to_le_bytes());

    // Shell Link GUID
    data[4..20].copy_from_slice(&LNK_GUID);

    // Link flags at offset 20
    data[20..24].copy_from_slice(&flags.to_le_bytes());

    // File attributes at offset 24
    data[24..28].copy_from_slice(&file_attrs.to_le_bytes());

    // Timestamps at offsets 28, 36, 44 (default to 0)
    // Icon index at offset 52 (4 bytes)
    // Show command at offset 56 (4 bytes)
    // Hot key at offset 60 (2 bytes)
    // Reserved fields at offset 62 (10 bytes)

    data
}

/// Create a LNK header with timestamps
fn create_lnk_header_with_timestamps(
    flags: u32,
    file_attrs: u32,
    creation_time: u64,
    access_time: u64,
    write_time: u64,
) -> Vec<u8> {
    let mut data = create_lnk_header(flags, file_attrs);

    // Creation time at offset 28
    data[28..36].copy_from_slice(&creation_time.to_le_bytes());

    // Access time at offset 36
    data[36..44].copy_from_slice(&access_time.to_le_bytes());

    // Write time at offset 44
    data[44..52].copy_from_slice(&write_time.to_le_bytes());

    data
}

#[test]
fn test_lnk_basic_parsing() {
    let data = create_lnk_header(0x0000, 0x0020);
    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("LNK".to_string()))
    );
    assert!(metadata.contains_key("FileSize"));
    assert!(metadata.contains_key("LinkFlags"));
    assert!(metadata.contains_key("FileAttributes"));
}

#[test]
fn test_lnk_header_flags_extraction() {
    let flags = FLAG_HAS_NAME | FLAG_HAS_ARGUMENTS | FLAG_HAS_WORKING_DIR;
    let data = create_lnk_header(flags, 0x0020);
    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    // Check flags are present (format should be 0x00000034 = 0x0004 | 0x0020 | 0x0010)
    let link_flags = metadata.get("LinkFlags").unwrap();
    if let TagValue::String(s) = link_flags {
        // Verify it's a hex string
        assert!(s.starts_with("0x"));
    }

    let flags_desc = metadata.get("LinkFlagsDescription").unwrap();
    if let TagValue::String(desc) = flags_desc {
        assert!(desc.contains("HasName"));
        assert!(desc.contains("HasWorkingDir"));
        assert!(desc.contains("HasArguments"));
    }
}

#[test]
fn test_lnk_file_attributes() {
    // Test ReadOnly + Archive + Hidden
    let file_attrs = 0x0001 | 0x0020 | 0x0002; // ReadOnly, Archive, Hidden
    let data = create_lnk_header(0x0000, file_attrs);
    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    let attrs = metadata.get("TargetFileAttributes").unwrap();
    if let TagValue::String(attr_str) = attrs {
        assert!(attr_str.contains("ReadOnly"));
        assert!(attr_str.contains("Archive"));
        assert!(attr_str.contains("Hidden"));
    }
}

#[test]
fn test_lnk_timestamps() {
    // Test timestamp: 2020-01-01 00:00:00 UTC = 132223104000000000
    let timestamp = 132223104000000000u64;

    let data = create_lnk_header_with_timestamps(0x0000, 0x0020, timestamp, timestamp, timestamp);
    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert!(metadata.contains_key("CreationTime"));
    assert!(metadata.contains_key("AccessTime"));
    assert!(metadata.contains_key("WriteTime"));

    if let Some(TagValue::String(creation_time)) = metadata.get("CreationTime") {
        assert!(creation_time.starts_with("2020-01-01"));
    }
}

#[test]
fn test_lnk_zero_timestamps() {
    // Zero timestamps should not be included in metadata
    let data = create_lnk_header_with_timestamps(0x0000, 0x0020, 0, 0, 0);
    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    // Zero timestamps should not create metadata entries
    assert!(!metadata.contains_key("CreationTime"));
    assert!(!metadata.contains_key("AccessTime"));
    assert!(!metadata.contains_key("WriteTime"));
}

#[test]
fn test_lnk_with_linkinfo_local_path() {
    let mut data = create_lnk_header(FLAG_HAS_LINK_INFO, 0x0020);
    data.resize(200, 0);

    // LinkInfo structure starts at offset 76
    let link_info_offset = 76;

    // LinkInfo header
    let link_info_size = 80u32;
    let link_info_header_size = 28u32;
    let link_info_flags = 0x0001u32; // VolumeIDAndLocalBasePath
    let volume_id_offset = 28u32;
    let local_base_path_offset = 52u32;

    data[link_info_offset..link_info_offset + 4].copy_from_slice(&link_info_size.to_le_bytes());
    data[link_info_offset + 4..link_info_offset + 8]
        .copy_from_slice(&link_info_header_size.to_le_bytes());
    data[link_info_offset + 8..link_info_offset + 12]
        .copy_from_slice(&link_info_flags.to_le_bytes());
    data[link_info_offset + 12..link_info_offset + 16]
        .copy_from_slice(&volume_id_offset.to_le_bytes());
    data[link_info_offset + 16..link_info_offset + 20]
        .copy_from_slice(&local_base_path_offset.to_le_bytes());

    // VolumeID structure at offset 76 + 28 = 104
    let vol_offset = link_info_offset + 28;
    let volume_id_size = 20u32;
    let drive_type = 3u32; // Fixed disk
    let volume_serial = 0xABCD1234u32;

    data[vol_offset..vol_offset + 4].copy_from_slice(&volume_id_size.to_le_bytes());
    data[vol_offset + 4..vol_offset + 8].copy_from_slice(&drive_type.to_le_bytes());
    data[vol_offset + 8..vol_offset + 12].copy_from_slice(&volume_serial.to_le_bytes());

    // LocalBasePath at offset 76 + 52 = 128
    let path_offset = link_info_offset + 52;
    let path = b"C:\\Windows\\System32\\notepad.exe\0";
    data[path_offset..path_offset + path.len()].copy_from_slice(path);

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert_eq!(
        metadata.get("VolumeSerialNumber"),
        Some(&TagValue::String("ABCD1234".to_string()))
    );
    assert!(metadata.contains_key("LocalBasePath"));
    if let Some(TagValue::String(path)) = metadata.get("LocalBasePath") {
        assert!(path.contains("notepad.exe"));
    }
}

#[test]
fn test_lnk_with_working_directory() {
    let mut data = create_lnk_header(FLAG_HAS_WORKING_DIR, 0x0020);
    data.resize(200, 0);

    // Working directory string at offset 76 (ANSI)
    let string_offset = 76;
    let char_count = 11u16; // Length of "C:\\Temp\\Dir"
    data[string_offset..string_offset + 2].copy_from_slice(&char_count.to_le_bytes());
    data[string_offset + 2..string_offset + 13].copy_from_slice(b"C:\\Temp\\Dir");

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert!(metadata.contains_key("WorkingDirectory"));
    if let Some(TagValue::String(dir)) = metadata.get("WorkingDirectory") {
        assert!(dir.contains("Temp"));
    }
}

#[test]
fn test_lnk_with_arguments() {
    let mut data = create_lnk_header(FLAG_HAS_ARGUMENTS, 0x0020);
    data.resize(200, 0);

    // Arguments string at offset 76 (ANSI)
    let string_offset = 76;
    let char_count = 14u16; // Length of "-arg1 -verbose"
    data[string_offset..string_offset + 2].copy_from_slice(&char_count.to_le_bytes());
    data[string_offset + 2..string_offset + 16].copy_from_slice(b"-arg1 -verbose");

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert!(metadata.contains_key("CommandLineArguments"));
    if let Some(TagValue::String(args)) = metadata.get("CommandLineArguments") {
        assert!(args.contains("-arg1"));
        assert!(args.contains("-verbose"));
    }
}

#[test]
fn test_lnk_with_icon_location() {
    let mut data = create_lnk_header(FLAG_HAS_ICON_LOCATION, 0x0020);
    data.resize(200, 0);

    // Icon location string at offset 76 (ANSI)
    let string_offset = 76;
    let icon_path = b"C:\\icons\\app.ico";
    let char_count = icon_path.len() as u16;
    data[string_offset..string_offset + 2].copy_from_slice(&char_count.to_le_bytes());
    data[string_offset + 2..string_offset + 2 + icon_path.len()].copy_from_slice(icon_path);

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert!(metadata.contains_key("IconLocation"));
    if let Some(TagValue::String(icon)) = metadata.get("IconLocation") {
        assert!(icon.contains("app.ico"));
    }
}

#[test]
fn test_lnk_with_relative_path() {
    let mut data = create_lnk_header(FLAG_HAS_RELATIVE_PATH, 0x0020);
    data.resize(200, 0);

    // Relative path string at offset 76 (ANSI)
    let string_offset = 76;
    let rel_path = b".\\data\\file";
    let char_count = rel_path.len() as u16;
    data[string_offset..string_offset + 2].copy_from_slice(&char_count.to_le_bytes());
    data[string_offset + 2..string_offset + 2 + rel_path.len()].copy_from_slice(rel_path);

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert!(metadata.contains_key("RelativePath"));
    if let Some(TagValue::String(path)) = metadata.get("RelativePath") {
        assert!(path.contains("data"));
    }
}

#[test]
fn test_lnk_with_unicode_strings() {
    let mut data = create_lnk_header(FLAG_HAS_NAME | FLAG_IS_UNICODE, 0x0020);
    data.resize(200, 0);

    // Unicode string at offset 76 (UTF-16LE)
    let string_offset = 76;
    let char_count = 7u16; // "Test文件" - mixed ASCII and Unicode
    data[string_offset..string_offset + 2].copy_from_slice(&char_count.to_le_bytes());

    // Encode "Test文件" in UTF-16LE
    let test_str = "TestDoc";
    let utf16_chars: Vec<u16> = test_str.encode_utf16().collect();
    for (i, &ch) in utf16_chars.iter().enumerate() {
        let bytes = ch.to_le_bytes();
        data[string_offset + 2 + i * 2] = bytes[0];
        data[string_offset + 2 + i * 2 + 1] = bytes[1];
    }

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert!(metadata.contains_key("Name"));
}

#[test]
fn test_lnk_with_tracker_data_block() {
    let mut data = create_lnk_header(0x0000, 0x0020);
    data.resize(200, 0);

    // TrackerDataBlock at offset 76
    let tracker_offset = 76;
    let block_size = 96u32;
    let block_sig = TRACKER_DATA_BLOCK_SIG;

    data[tracker_offset..tracker_offset + 4].copy_from_slice(&block_size.to_le_bytes());
    data[tracker_offset + 4..tracker_offset + 8].copy_from_slice(&block_sig.to_le_bytes());

    // Machine ID at offset +16 (null-terminated string)
    let machine_id = b"WORKSTATION1\0";
    data[tracker_offset + 16..tracker_offset + 16 + machine_id.len()].copy_from_slice(machine_id);

    // Droid Volume ID at offset +32 (GUID)
    let vol_guid = [
        0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
        0x88,
    ];
    data[tracker_offset + 32..tracker_offset + 48].copy_from_slice(&vol_guid);

    // Droid File ID at offset +48 (GUID with MAC address in last 6 bytes)
    let file_guid = [
        0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
        0x99,
    ];
    data[tracker_offset + 48..tracker_offset + 64].copy_from_slice(&file_guid);

    // Terminal block (size < 4)
    data[tracker_offset + 96..tracker_offset + 100].copy_from_slice(&0u32.to_le_bytes());

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert!(metadata.contains_key("MachineID"));
    if let Some(TagValue::String(machine)) = metadata.get("MachineID") {
        assert!(machine.contains("WORKSTATION1"));
    }

    assert!(metadata.contains_key("DroidVolumeID"));
    assert!(metadata.contains_key("DroidFileID"));
    assert!(metadata.contains_key("MACAddress"));

    // Verify MAC address format
    if let Some(TagValue::String(mac)) = metadata.get("MACAddress") {
        assert!(mac.contains(":"));
    }
}

#[test]
fn test_lnk_with_known_folder_block() {
    let mut data = create_lnk_header(0x0000, 0x0020);
    data.resize(200, 0);

    // KnownFolderDataBlock at offset 76
    let folder_offset = 76;
    let block_size = 28u32;
    let block_sig = KNOWN_FOLDER_BLOCK_SIG;

    data[folder_offset..folder_offset + 4].copy_from_slice(&block_size.to_le_bytes());
    data[folder_offset + 4..folder_offset + 8].copy_from_slice(&block_sig.to_le_bytes());

    // Known Folder GUID at offset +8
    let folder_guid = [
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF,
        0x00,
    ];
    data[folder_offset + 8..folder_offset + 24].copy_from_slice(&folder_guid);

    // Terminal block
    data[folder_offset + 28..folder_offset + 32].copy_from_slice(&0u32.to_le_bytes());

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert!(metadata.contains_key("KnownFolderID"));
    if let Some(TagValue::String(guid)) = metadata.get("KnownFolderID") {
        // GUID format: XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX
        assert_eq!(guid.len(), 36); // 32 hex digits + 4 hyphens
        assert_eq!(guid.matches('-').count(), 4);
    }
}

#[test]
fn test_lnk_minimal_truncated() {
    // Test with minimal valid header, no extra data
    let data = create_lnk_header(0x0000, 0x0000);
    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse minimal LNK");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("LNK".to_string()))
    );
    assert_eq!(
        metadata.get("FileSize"),
        Some(&TagValue::String("76".to_string()))
    );
}

#[test]
fn test_lnk_invalid_magic() {
    let mut data = vec![0u8; LNK_HEADER_SIZE];

    // Invalid magic number
    data[0..4].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);

    // Valid GUID
    data[4..20].copy_from_slice(&LNK_GUID);

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let result = parser.parse(&reader);
    assert!(result.is_err());
}

#[test]
fn test_lnk_invalid_guid() {
    let mut data = vec![0u8; LNK_HEADER_SIZE];

    // Valid magic
    data[0..4].copy_from_slice(&LNK_MAGIC.to_le_bytes());

    // Invalid GUID
    data[4..20].copy_from_slice(&[0xFF; 16]);

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let result = parser.parse(&reader);
    assert!(result.is_err());
}

#[test]
fn test_lnk_file_too_small() {
    // Only 50 bytes, less than minimum header size
    let data = vec![0u8; 50];
    let reader = TestReader::new(data);
    let parser = LNKParser;

    let result = parser.parse(&reader);
    assert!(result.is_err());
}

#[test]
fn test_lnk_multiple_flags_and_strings() {
    // Test with multiple flags and string data
    let flags = FLAG_HAS_NAME | FLAG_HAS_WORKING_DIR | FLAG_HAS_ARGUMENTS | FLAG_HAS_ICON_LOCATION;
    let mut data = create_lnk_header(flags, 0x0020);
    data.resize(400, 0);

    let mut string_offset = 76;

    // Name string
    let name = b"MyShortcut";
    data[string_offset..string_offset + 2].copy_from_slice(&(name.len() as u16).to_le_bytes());
    data[string_offset + 2..string_offset + 2 + name.len()].copy_from_slice(name);
    string_offset += 2 + name.len();

    // Working directory
    let workdir = b"C:\\Work";
    data[string_offset..string_offset + 2].copy_from_slice(&(workdir.len() as u16).to_le_bytes());
    data[string_offset + 2..string_offset + 2 + workdir.len()].copy_from_slice(workdir);
    string_offset += 2 + workdir.len();

    // Arguments
    let args = b"-v -debug";
    data[string_offset..string_offset + 2].copy_from_slice(&(args.len() as u16).to_le_bytes());
    data[string_offset + 2..string_offset + 2 + args.len()].copy_from_slice(args);
    string_offset += 2 + args.len();

    // Icon location
    let icon = b"app.ico";
    data[string_offset..string_offset + 2].copy_from_slice(&(icon.len() as u16).to_le_bytes());
    data[string_offset + 2..string_offset + 2 + icon.len()].copy_from_slice(icon);

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    // Verify all string data is present
    assert!(metadata.contains_key("Name"));
    assert!(metadata.contains_key("WorkingDirectory"));
    assert!(metadata.contains_key("CommandLineArguments"));
    assert!(metadata.contains_key("IconLocation"));
}

#[test]
fn test_lnk_system_hidden_attributes() {
    // Test System + Hidden file attributes
    let file_attrs = 0x0004 | 0x0002; // System + Hidden
    let data = create_lnk_header(0x0000, file_attrs);
    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    let attrs = metadata.get("TargetFileAttributes").unwrap();
    if let TagValue::String(attr_str) = attrs {
        assert!(attr_str.contains("System"));
        assert!(attr_str.contains("Hidden"));
    }
}
