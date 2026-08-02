//! Integration tests for LNK (Windows shortcut) parser
//!
//! Every assertion here is written against `Image::ExifTool::LNK` -- the tag
//! names carry ExifTool's `LNK` group and the expected strings are the
//! `PrintConv` output ExifTool itself produces. Coverage:
//! - Header parsing and signature verification (`%LNK::Main`)
//! - `Flags` and `FileAttributes` BITMASK conversions
//! - FILETIME date tags (`CreateDate`, `AccessDate`, `ModifyDate`)
//! - `LinkInfo` volume and path tags
//! - String data (`Description`, `RelativePath`, `WorkingDirectory`,
//!   `CommandLineArguments`, `IconFileName`)
//! - Extra data blocks (`TrackerData`, `ConsoleData`)
//! - Edge cases and error handling

#[path = "../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::{FormatParser, TagValue};
use oxidex::parsers::specialized::lnk::LNKParser;

/// LNK file magic number (also the Shell Link Header size)
const LNK_MAGIC: u32 = 0x0000004C;

/// Shell Link GUID: {00021401-0000-0000-C000-000000000046}
const LNK_GUID: [u8; 16] = [
    0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

/// LNK header size
const LNK_HEADER_SIZE: usize = 76;

/// Link flags (`%LNK::Main` 0x14)
const FLAG_HAS_LINK_INFO: u32 = 0x0002;
const FLAG_HAS_DESCRIPTION: u32 = 0x0004;
const FLAG_HAS_RELATIVE_PATH: u32 = 0x0008;
const FLAG_HAS_WORKING_DIR: u32 = 0x0010;
const FLAG_HAS_ARGUMENTS: u32 = 0x0020;
const FLAG_HAS_ICON_FILE: u32 = 0x0040;
const FLAG_IS_UNICODE: u32 = 0x0080;

/// Extra data block signatures (LNK.pm:412)
const CONSOLE_DATA_SIG: u32 = 0xA0000002;
const TRACKER_DATA_BLOCK_SIG: u32 = 0xA0000003;
const KNOWN_FOLDER_BLOCK_SIG: u32 = 0xA000000B;

/// Create a minimal valid LNK header with specified flags and attributes
fn create_lnk_header(flags: u32, file_attrs: u32) -> Vec<u8> {
    let mut data = vec![0u8; LNK_HEADER_SIZE];

    // Magic number (0x4C 0x00 0x00 0x00) -- also the header size
    data[0..4].copy_from_slice(&LNK_MAGIC.to_le_bytes());

    // Shell Link GUID
    data[4..20].copy_from_slice(&LNK_GUID);

    // Link flags at offset 0x14
    data[20..24].copy_from_slice(&flags.to_le_bytes());

    // File attributes at offset 0x18
    data[24..28].copy_from_slice(&file_attrs.to_le_bytes());

    data
}

/// Create a LNK header with FILETIME timestamps
fn create_lnk_header_with_timestamps(
    flags: u32,
    file_attrs: u32,
    creation_time: u64,
    access_time: u64,
    write_time: u64,
) -> Vec<u8> {
    let mut data = create_lnk_header(flags, file_attrs);

    // CreateDate at 0x1c, AccessDate at 0x24, ModifyDate at 0x2c
    data[28..36].copy_from_slice(&creation_time.to_le_bytes());
    data[36..44].copy_from_slice(&access_time.to_le_bytes());
    data[44..52].copy_from_slice(&write_time.to_le_bytes());

    data
}

/// Appends a length-prefixed string data entry (LNK.pm:1786).
fn push_string_entry(data: &mut Vec<u8>, text: &str, unicode: bool) {
    if unicode {
        let units: Vec<u16> = text.encode_utf16().collect();
        data.extend_from_slice(&(units.len() as u16).to_le_bytes());
        for unit in units {
            data.extend_from_slice(&unit.to_le_bytes());
        }
    } else {
        data.extend_from_slice(&(text.len() as u16).to_le_bytes());
        data.extend_from_slice(text.as_bytes());
    }
}

fn expect_string(metadata: &oxidex::core::MetadataMap, tag: &str) -> String {
    match metadata.get(tag) {
        Some(TagValue::String(s)) => s.clone(),
        other => panic!("expected string for {tag}, got {other:?}"),
    }
}

/// ExifTool's local date/time form: `YYYY:MM:DD HH:MM:SS±HH:MM`
/// (`ConvertUnixTime($val, 1)`, ExifTool.pm:6808).
fn is_exiftool_local_datetime(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 25 {
        return false;
    }
    let digits_at = |idx: &[usize]| idx.iter().all(|&i| bytes[i].is_ascii_digit());
    digits_at(&[
        0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 23, 24,
    ]) && bytes[4] == b':'
        && bytes[7] == b':'
        && bytes[10] == b' '
        && bytes[13] == b':'
        && bytes[16] == b':'
        && (bytes[19] == b'+' || bytes[19] == b'-')
        && bytes[22] == b':'
}

#[test]
fn test_lnk_basic_parsing() {
    let data = create_lnk_header(0x0000, 0x0020);
    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    // No flags set -> DecodeBits renders "(none)" (ExifTool.pm:6405).
    assert_eq!(expect_string(&metadata, "LNK:Flags"), "(none)");
    assert_eq!(expect_string(&metadata, "LNK:FileAttributes"), "Archive");
    assert_eq!(expect_string(&metadata, "LNK:RunWindow"), "Hide");
    assert_eq!(expect_string(&metadata, "LNK:IconIndex"), "(none)");
    assert_eq!(expect_string(&metadata, "LNK:HotKey"), "(none)");
}

/// The parser must never emit a tag ExifTool does not name, and never an
/// ungrouped one.
#[test]
fn test_lnk_tags_use_exiftool_names() {
    let data = create_lnk_header(FLAG_HAS_DESCRIPTION | FLAG_HAS_ARGUMENTS, 0x0020);
    let reader = TestReader::new(data);
    let metadata = LNKParser.parse(&reader).expect("Failed to parse LNK");

    for (key, _) in metadata.iter() {
        assert!(key.starts_with("LNK:"), "ungrouped tag {key}");
    }
    // Names the previous implementation invented; ExifTool has none of them.
    for invented in [
        "FileType",
        "FileSize",
        "LinkFlags",
        "LinkFlagsDescription",
        "CreationTime",
        "AccessTime",
        "WriteTime",
        "Name",
        "IconLocation",
        "VolumeSerialNumber",
        "MACAddress",
        "DroidFileID",
        "DroidVolumeID",
        "KnownFolderID",
        "HasPropertyStore",
    ] {
        assert!(
            metadata.get(invented).is_none(),
            "{invented} is not an ExifTool tag"
        );
        assert!(
            metadata.get(&format!("LNK:{invented}")).is_none(),
            "LNK:{invented} is not an ExifTool tag"
        );
    }
}

#[test]
fn test_lnk_header_flags_extraction() {
    let flags = FLAG_HAS_DESCRIPTION | FLAG_HAS_ARGUMENTS | FLAG_HAS_WORKING_DIR;
    let data = create_lnk_header(flags, 0x0020);
    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    // %LNK::Main 0x14 BITMASK, bits reported in ascending order.
    assert_eq!(
        expect_string(&metadata, "LNK:Flags"),
        "Description, WorkingDir, CommandArgs"
    );
}

#[test]
fn test_lnk_file_attributes() {
    // Read-only + Hidden + Archive
    let file_attrs = 0x0001 | 0x0002 | 0x0020;
    let data = create_lnk_header(0x0000, file_attrs);
    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    // %fileAttributes (LNK.pm:30) -- ExifTool spells bit 0 "Read-only".
    assert_eq!(
        expect_string(&metadata, "LNK:FileAttributes"),
        "Read-only, Hidden, Archive"
    );
    // TargetFileAttributes belongs to the ItemID TargetInfo block, not here.
    assert!(metadata.get("LNK:TargetFileAttributes").is_none());
}

#[test]
fn test_lnk_timestamps() {
    // 2020-01-01 00:00:00 UTC as a FILETIME
    let timestamp = 132223104000000000u64;

    let data = create_lnk_header_with_timestamps(0x0000, 0x0020, timestamp, timestamp, timestamp);
    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    for tag in ["LNK:CreateDate", "LNK:AccessDate", "LNK:ModifyDate"] {
        let value = expect_string(&metadata, tag);
        assert!(
            is_exiftool_local_datetime(&value),
            "{tag} = {value:?} is not ExifTool's local date/time form"
        );
        // Local time for that instant lands on one of these three dates.
        let date = &value[..10];
        assert!(
            matches!(date, "2019:12:31" | "2020:01:01" | "2020:01:02"),
            "{tag} = {value:?}"
        );
    }
}

#[test]
fn test_lnk_zero_timestamps() {
    // `RawConv => '$val ? $val : undef'` (LNK.pm:53) drops zero dates.
    let data = create_lnk_header_with_timestamps(0x0000, 0x0020, 0, 0, 0);
    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert!(!metadata.contains_key("LNK:CreateDate"));
    assert!(!metadata.contains_key("LNK:AccessDate"));
    assert!(!metadata.contains_key("LNK:ModifyDate"));
}

#[test]
fn test_lnk_with_linkinfo_local_path() {
    let mut data = create_lnk_header(FLAG_HAS_LINK_INFO, 0x0020);
    data.resize(220, 0);

    // LinkInfo structure starts right after the header
    let link_info_offset = LNK_HEADER_SIZE;

    let link_info_size = 96u32;
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

    // VolumeID structure
    let vol_offset = link_info_offset + 28;
    let volume_id_size = 20u32;
    let drive_type = 3u32; // Fixed Disk
    let volume_serial = 0xABCD1234u32;
    let volume_label_offset = 16u32;

    data[vol_offset..vol_offset + 4].copy_from_slice(&volume_id_size.to_le_bytes());
    data[vol_offset + 4..vol_offset + 8].copy_from_slice(&drive_type.to_le_bytes());
    data[vol_offset + 8..vol_offset + 12].copy_from_slice(&volume_serial.to_le_bytes());
    data[vol_offset + 12..vol_offset + 16].copy_from_slice(&volume_label_offset.to_le_bytes());
    data[vol_offset + 16..vol_offset + 21].copy_from_slice(b"OS\0\0\0");

    // LocalBasePath (single-byte, header size < 0x24)
    let path_offset = link_info_offset + 52;
    let path = b"C:\\Windows\\System32\\notepad.exe\0";
    data[path_offset..path_offset + path.len()].copy_from_slice(path);

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert_eq!(expect_string(&metadata, "LNK:DriveType"), "Fixed Disk");
    // LNK.pm:1160 splits the serial into two hex quads.
    assert_eq!(
        expect_string(&metadata, "LNK:DriveSerialNumber"),
        "ABCD-1234"
    );
    assert_eq!(expect_string(&metadata, "LNK:VolumeLabel"), "OS");
    assert_eq!(
        expect_string(&metadata, "LNK:LocalBasePath"),
        "C:\\Windows\\System32\\notepad.exe"
    );
}

#[test]
fn test_lnk_with_working_directory() {
    let mut data = create_lnk_header(FLAG_HAS_WORKING_DIR, 0x0020);
    push_string_entry(&mut data, "C:\\Temp\\Dir", false);
    data.resize(200, 0);

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert_eq!(
        expect_string(&metadata, "LNK:WorkingDirectory"),
        "C:\\Temp\\Dir"
    );
}

#[test]
fn test_lnk_with_arguments() {
    let mut data = create_lnk_header(FLAG_HAS_ARGUMENTS, 0x0020);
    push_string_entry(&mut data, "-arg1 -verbose", false);
    data.resize(200, 0);

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert_eq!(
        expect_string(&metadata, "LNK:CommandLineArguments"),
        "-arg1 -verbose"
    );
}

#[test]
fn test_lnk_with_icon_file_name() {
    let mut data = create_lnk_header(FLAG_HAS_ICON_FILE, 0x0020);
    push_string_entry(&mut data, "C:\\icons\\app.ico", false);
    data.resize(200, 0);

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    // ExifTool names this IconFileName (%LNK::Main 0x30040), not IconLocation.
    assert_eq!(
        expect_string(&metadata, "LNK:IconFileName"),
        "C:\\icons\\app.ico"
    );
}

#[test]
fn test_lnk_with_relative_path() {
    let mut data = create_lnk_header(FLAG_HAS_RELATIVE_PATH, 0x0020);
    push_string_entry(&mut data, ".\\data\\file", false);
    data.resize(200, 0);

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert_eq!(
        expect_string(&metadata, "LNK:RelativePath"),
        ".\\data\\file"
    );
}

#[test]
fn test_lnk_with_unicode_strings() {
    let mut data = create_lnk_header(FLAG_HAS_DESCRIPTION | FLAG_IS_UNICODE, 0x0020);
    push_string_entry(&mut data, "Test\u{6587}\u{4ef6}", true);
    data.resize(200, 0);

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    // %LNK::Main 0x30004 is Description; the old parser called it "Name".
    assert_eq!(
        expect_string(&metadata, "LNK:Description"),
        "Test\u{6587}\u{4ef6}"
    );
}

#[test]
fn test_lnk_with_tracker_data_block() {
    let mut data = create_lnk_header(0x0000, 0x0020);
    data.resize(LNK_HEADER_SIZE + 100, 0);

    let tracker_offset = LNK_HEADER_SIZE;
    let block_size = 96u32;

    data[tracker_offset..tracker_offset + 4].copy_from_slice(&block_size.to_le_bytes());
    data[tracker_offset + 4..tracker_offset + 8]
        .copy_from_slice(&TRACKER_DATA_BLOCK_SIG.to_le_bytes());

    // %LNK::TrackerData 0x10 MachineID, Format => 'var_string'
    let machine_id = b"WORKSTATION1\0";
    data[tracker_offset + 16..tracker_offset + 16 + machine_id.len()].copy_from_slice(machine_id);

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert_eq!(expect_string(&metadata, "LNK:MachineID"), "WORKSTATION1");
}

#[test]
fn test_lnk_with_console_data_block() {
    let mut data = create_lnk_header(0x0000, 0x0020);
    let block_size = 0x8cu32;
    let base = data.len();
    data.resize(base + block_size as usize + 4, 0);

    data[base..base + 4].copy_from_slice(&block_size.to_le_bytes());
    data[base + 4..base + 8].copy_from_slice(&CONSOLE_DATA_SIG.to_le_bytes());
    data[base + 0x08..base + 0x0a].copy_from_slice(&7u16.to_le_bytes());
    data[base + 0x0a..base + 0x0c].copy_from_slice(&0xf5u16.to_le_bytes());
    data[base + 0x0c..base + 0x0e].copy_from_slice(&80u16.to_le_bytes());
    data[base + 0x0e..base + 0x10].copy_from_slice(&500u16.to_le_bytes());
    data[base + 0x24..base + 0x28].copy_from_slice(&0x30u32.to_le_bytes());
    data[base + 0x28..base + 0x2c].copy_from_slice(&400u32.to_le_bytes());
    for (i, unit) in "8514oem".encode_utf16().enumerate() {
        data[base + 0x2c + i * 2..base + 0x2e + i * 2].copy_from_slice(&unit.to_le_bytes());
    }
    data[base + 0x78..base + 0x7c].copy_from_slice(&1u32.to_le_bytes());

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    // %LNK::ConsoleData PrintConv forms (LNK.pm:1250)
    assert_eq!(expect_string(&metadata, "LNK:FillAttributes"), "0x07");
    assert_eq!(expect_string(&metadata, "LNK:PopupFillAttributes"), "0xf5");
    assert_eq!(expect_string(&metadata, "LNK:ScreenBufferSize"), "80 x 500");
    assert_eq!(expect_string(&metadata, "LNK:FontFamily"), "Modern");
    assert_eq!(
        metadata.get("LNK:FontWeight"),
        Some(&TagValue::Integer(400))
    );
    assert_eq!(expect_string(&metadata, "LNK:FontName"), "8514oem");
    assert_eq!(expect_string(&metadata, "LNK:FullScreen"), "No");
    assert_eq!(expect_string(&metadata, "LNK:InsertMode"), "Yes");
}

/// ExifTool decodes no tags from a KnownFolder block (`%LNK::UnknownData` is
/// empty), so the parser must stay silent rather than invent a GUID tag.
#[test]
fn test_lnk_with_known_folder_block() {
    let mut data = create_lnk_header(0x0000, 0x0020);
    data.resize(LNK_HEADER_SIZE + 32, 0);

    let folder_offset = LNK_HEADER_SIZE;
    let block_size = 28u32;

    data[folder_offset..folder_offset + 4].copy_from_slice(&block_size.to_le_bytes());
    data[folder_offset + 4..folder_offset + 8]
        .copy_from_slice(&KNOWN_FOLDER_BLOCK_SIG.to_le_bytes());
    data[folder_offset + 8..folder_offset + 24].copy_from_slice(&[0x11u8; 16]);

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert!(metadata.get("LNK:KnownFolderID").is_none());
    assert!(metadata.get("KnownFolderID").is_none());
}

#[test]
fn test_lnk_minimal_truncated() {
    // Minimal valid header, no optional structures
    let data = create_lnk_header(0x0000, 0x0000);
    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse minimal LNK");

    assert_eq!(expect_string(&metadata, "LNK:Flags"), "(none)");
    assert_eq!(expect_string(&metadata, "LNK:FileAttributes"), "(none)");
    assert_eq!(
        metadata.get("LNK:TargetFileSize"),
        Some(&TagValue::Integer(0))
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
    // String data is read in ExifTool's fixed order (LNK.pm:1786), one entry
    // per set flag.
    let flags =
        FLAG_HAS_DESCRIPTION | FLAG_HAS_WORKING_DIR | FLAG_HAS_ARGUMENTS | FLAG_HAS_ICON_FILE;
    let mut data = create_lnk_header(flags, 0x0020);

    push_string_entry(&mut data, "MyShortcut", false);
    push_string_entry(&mut data, "C:\\Work", false);
    push_string_entry(&mut data, "-v -debug", false);
    push_string_entry(&mut data, "app.ico", false);
    data.resize(400, 0);

    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert_eq!(expect_string(&metadata, "LNK:Description"), "MyShortcut");
    assert_eq!(expect_string(&metadata, "LNK:WorkingDirectory"), "C:\\Work");
    assert_eq!(
        expect_string(&metadata, "LNK:CommandLineArguments"),
        "-v -debug"
    );
    assert_eq!(expect_string(&metadata, "LNK:IconFileName"), "app.ico");
}

#[test]
fn test_lnk_system_hidden_attributes() {
    // Hidden + System
    let file_attrs = 0x0002 | 0x0004;
    let data = create_lnk_header(0x0000, file_attrs);
    let reader = TestReader::new(data);
    let parser = LNKParser;

    let metadata = parser.parse(&reader).expect("Failed to parse LNK");

    assert_eq!(
        expect_string(&metadata, "LNK:FileAttributes"),
        "Hidden, System"
    );
}
