//! RAR archive format parser
//!
//! Implements comprehensive metadata extraction from RAR archive files.
//! Supports both RAR4 and RAR5 formats with detailed archive properties.

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// RAR signature: "Rar!" (0x52 0x61 0x72 0x21)
const RAR_SIGNATURE: &[u8] = b"Rar!";

/// RAR5 signature (additional marker at offset 7)
const RAR5_MARKER: u8 = 0x01;

/// RAR4 block types
const RAR4_BLOCK_ARCHIVE: u8 = 0x73;
const RAR4_BLOCK_FILE: u8 = 0x74;

/// RAR4 archive header flags (offset 2 in header)
const RAR4_FLAG_VOLUME: u16 = 0x0001; // Bit 0: Multi-part archive
const RAR4_FLAG_COMMENT: u16 = 0x0002; // Bit 1: Archive comment present
const RAR4_FLAG_LOCK: u16 = 0x0004; // Bit 2: Archive lock
const RAR4_FLAG_SOLID: u16 = 0x0008; // Bit 3: Solid archive
const RAR4_FLAG_NEWNUMBERING: u16 = 0x0010; // Bit 4: New naming scheme
const RAR4_FLAG_AUTH: u16 = 0x0020; // Bit 5: Authenticity info
const RAR4_FLAG_RECOVERY: u16 = 0x0040; // Bit 6: Recovery record
const RAR4_FLAG_ENCRYPTED: u16 = 0x0080; // Bit 7: Block headers encrypted

/// RAR5 header types
const RAR5_HEADER_MAIN: u64 = 1;
const RAR5_HEADER_FILE: u64 = 2;

/// RAR5 main archive flags
const RAR5_FLAG_VOLUME: u64 = 0x0001;
const RAR5_FLAG_RECOVERY: u64 = 0x0002;
const RAR5_FLAG_LOCKED: u64 = 0x0004;
const RAR5_FLAG_SOLID: u64 = 0x0008;

/// RAR5 encryption flags
const RAR5_FLAG_ENCRYPTED: u64 = 0x0001;

/// RAR parser for extracting metadata from RAR archives
pub struct RARParser;

impl RARParser {
    /// Verifies RAR signature
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 7 {
            return Ok(false);
        }

        let header = reader.read(0, 7)?;
        Ok(header.starts_with(RAR_SIGNATURE))
    }

    /// Detects RAR version (4.x or 5.0)
    pub fn detect_version(reader: &dyn FileReader) -> Result<&'static str> {
        if reader.size() < 8 {
            return Ok("Unknown");
        }

        let header = reader.read(0, 8)?;
        if header.len() >= 7 && &header[0..4] == RAR_SIGNATURE {
            // RAR5 has 0x01 at offset 6 (seventh byte of the 8-byte signature)
            if header.len() >= 7 && header[6] == RAR5_MARKER {
                Ok("5.0")
            } else {
                Ok("4.x")
            }
        } else {
            Ok("Unknown")
        }
    }

    /// Parses RAR4 archive header and extracts metadata
    fn parse_rar4_metadata(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
        // RAR4 structure: Signature (7 bytes) + Archive Header Block
        // Archive header starts at offset 7
        if reader.size() < 20 {
            return Ok(()); // Not enough data for full header
        }

        let header_data = reader.read(7, 13)?;
        if header_data.len() < 7 {
            return Ok(());
        }

        // Check if this is archive header (type 0x73)
        if header_data[2] != RAR4_BLOCK_ARCHIVE {
            return Ok(()); // Not an archive header
        }

        // Extract flags at offset 3-4 (little-endian u16)
        if header_data.len() >= 5 {
            let reader = EndianReader::little_endian(header_data);
            let flags = reader.u16_at(3).unwrap_or(0);

            metadata.insert(
                "IsVolume".to_string(),
                TagValue::String((flags & RAR4_FLAG_VOLUME != 0).to_string()),
            );
            metadata.insert(
                "IsSolid".to_string(),
                TagValue::String((flags & RAR4_FLAG_SOLID != 0).to_string()),
            );
            metadata.insert(
                "HasRecoveryRecord".to_string(),
                TagValue::String((flags & RAR4_FLAG_RECOVERY != 0).to_string()),
            );
            metadata.insert(
                "IsEncrypted".to_string(),
                TagValue::String((flags & RAR4_FLAG_ENCRYPTED != 0).to_string()),
            );
            metadata.insert(
                "HasComment".to_string(),
                TagValue::String((flags & RAR4_FLAG_COMMENT != 0).to_string()),
            );
            metadata.insert(
                "IsLocked".to_string(),
                TagValue::String((flags & RAR4_FLAG_LOCK != 0).to_string()),
            );
        }

        // Count file entries by scanning for file header blocks (0x74)
        if let Ok(count) = Self::count_rar4_files(reader) {
            metadata.insert("FileCount".to_string(), TagValue::String(count.to_string()));
        }

        Ok(())
    }

    /// Counts file entries in RAR4 archive
    fn count_rar4_files(reader: &dyn FileReader) -> Result<u32> {
        let mut offset = 7u64;
        let mut file_count = 0u32;
        let max_offset = reader.size().min(1024 * 1024); // Limit scan to 1MB

        while offset + 7 < max_offset {
            let block_header = reader.read(offset, 7)?;
            if block_header.len() < 7 {
                break;
            }

            let r = EndianReader::little_endian(block_header);
            let block_type = block_header[2];
            let _block_flags = r.u16_at(3).unwrap_or(0);
            let block_size = r.u16_at(5).unwrap_or(0);

            if block_type == RAR4_BLOCK_FILE {
                file_count += 1;
            }

            // Advance to next block
            if block_size == 0 {
                break;
            }
            offset += block_size as u64;

            // Safety limit: stop after 10000 files
            if file_count >= 10000 {
                break;
            }
        }

        Ok(file_count)
    }

    /// Parses RAR5 archive header and extracts metadata
    fn parse_rar5_metadata(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
        // RAR5 structure: Signature (8 bytes) + Main Archive Header
        if reader.size() < 20 {
            return Ok(());
        }

        let header_data = reader.read(8, 32)?;
        if header_data.len() < 10 {
            return Ok(());
        }

        // Parse variable-length header
        let (_header_crc, pos) = Self::read_rar5_u32(header_data, 0)?;
        let (_header_size, pos) = Self::read_rar5_vint(header_data, pos)?;
        let (header_type, pos) = Self::read_rar5_vint(header_data, pos)?;
        let (header_flags, _) = Self::read_rar5_vint(header_data, pos)?;

        if header_type == RAR5_HEADER_MAIN {
            metadata.insert(
                "IsVolume".to_string(),
                TagValue::String((header_flags & RAR5_FLAG_VOLUME != 0).to_string()),
            );
            metadata.insert(
                "IsSolid".to_string(),
                TagValue::String((header_flags & RAR5_FLAG_SOLID != 0).to_string()),
            );
            metadata.insert(
                "HasRecoveryRecord".to_string(),
                TagValue::String((header_flags & RAR5_FLAG_RECOVERY != 0).to_string()),
            );
            metadata.insert(
                "IsLocked".to_string(),
                TagValue::String((header_flags & RAR5_FLAG_LOCKED != 0).to_string()),
            );

            // Check encryption flag
            let is_encrypted = (header_flags & RAR5_FLAG_ENCRYPTED) != 0;
            metadata.insert(
                "IsEncrypted".to_string(),
                TagValue::String(is_encrypted.to_string()),
            );
        }

        // Count file entries in RAR5
        if let Ok(count) = Self::count_rar5_files(reader) {
            metadata.insert("FileCount".to_string(), TagValue::String(count.to_string()));
        }

        Ok(())
    }

    /// Reads a 32-bit little-endian integer from RAR5 data
    fn read_rar5_u32(data: &[u8], offset: usize) -> Result<(u32, usize)> {
        let reader = EndianReader::little_endian(data);
        let value = reader
            .u32_at(offset)
            .ok_or_else(|| ExifToolError::parse_error("Unexpected end of RAR5 header"))?;
        Ok((value, offset + 4))
    }

    /// Reads a variable-length integer from RAR5 data
    fn read_rar5_vint(data: &[u8], offset: usize) -> Result<(u64, usize)> {
        if offset >= data.len() {
            return Err(ExifToolError::parse_error("Unexpected end of RAR5 vint"));
        }

        let mut result = 0u64;
        let mut shift = 0;
        let mut pos = offset;

        loop {
            if pos >= data.len() {
                return Err(ExifToolError::parse_error("Incomplete RAR5 vint"));
            }

            let byte = data[pos];
            pos += 1;

            result |= ((byte & 0x7F) as u64) << shift;
            shift += 7;

            if (byte & 0x80) == 0 {
                break;
            }

            if shift >= 64 {
                return Err(ExifToolError::parse_error("RAR5 vint overflow"));
            }
        }

        Ok((result, pos))
    }

    /// Counts file entries in RAR5 archive
    fn count_rar5_files(reader: &dyn FileReader) -> Result<u32> {
        let mut offset = 8u64;
        let mut file_count = 0u32;
        let max_offset = reader.size().min(1024 * 1024); // Limit scan to 1MB

        while offset + 10 < max_offset {
            let block_data = reader.read(offset, 20)?;
            if block_data.len() < 10 {
                break;
            }

            // Try to parse header
            if let Ok((_, pos1)) = Self::read_rar5_u32(block_data, 0)
                && let Ok((header_size, pos2)) = Self::read_rar5_vint(block_data, pos1)
                && let Ok((header_type, _)) = Self::read_rar5_vint(block_data, pos2)
            {
                if header_type == RAR5_HEADER_FILE {
                    file_count += 1;
                }

                // Advance to next block
                if header_size == 0 || header_size > 1024 * 1024 {
                    break;
                }
                offset += header_size;

                // Safety limit
                if file_count >= 10000 {
                    break;
                }
                continue;
            }
            break;
        }

        Ok(file_count)
    }
}

impl FormatParser for RARParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify signature
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid RAR signature"));
        }

        let mut metadata = MetadataMap::new();

        // Detect version
        let version = Self::detect_version(reader)?;
        metadata.insert("FileType".to_string(), TagValue::String("RAR".to_string()));
        metadata.insert(
            "RARVersion".to_string(),
            TagValue::String(version.to_string()),
        );
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        // Parse format-specific metadata
        match version {
            "5.0" => {
                Self::parse_rar5_metadata(reader, &mut metadata)?;
            }
            "4.x" => {
                Self::parse_rar4_metadata(reader, &mut metadata)?;
            }
            _ => {
                // Unknown version, skip detailed parsing
            }
        }

        // Extract additional Worker 3 specification tags
        // These may already be present but ensure they follow the RAR: naming convention

        // RAR:FileCount - extract file count if not already set
        if !metadata.contains_key("RAR:FileCount") {
            if let Some(TagValue::String(count_str)) = metadata.get("FileCount") {
                if let Ok(count) = count_str.parse::<i64>() {
                    metadata.insert("RAR:FileCount".to_string(), TagValue::new_integer(count));
                }
            }
        }

        // RAR:SolidArchive - extract from IsSolid tag and standardize
        if let Some(TagValue::String(is_solid)) = metadata.get("IsSolid") {
            metadata.insert(
                "RAR:SolidArchive".to_string(),
                TagValue::new_string(is_solid.clone()),
            );
        }

        // RAR:CompressionMethod - set to default for RAR
        // RAR uses various compression algorithms; we report as "RAR" for now
        if !metadata.contains_key("RAR:CompressionMethod") {
            metadata.insert(
                "RAR:CompressionMethod".to_string(),
                TagValue::new_string("RAR".to_string()),
            );
        }

        // RAR:EncryptionMethod - extract from IsEncrypted
        if let Some(TagValue::String(is_encrypted)) = metadata.get("IsEncrypted") {
            if is_encrypted == "true" {
                metadata.insert(
                    "RAR:EncryptionMethod".to_string(),
                    TagValue::new_string("AES-256".to_string()),
                );
            }
        }

        // RAR:CreateDate - we'll use a placeholder for now
        // RAR archives don't typically store a creation date in headers
        metadata.insert(
            "RAR:CreateDate".to_string(),
            TagValue::new_string("Unknown".to_string()),
        );

        // RAR:ModifyDate - same placeholder
        metadata.insert(
            "RAR:ModifyDate".to_string(),
            TagValue::new_string("Unknown".to_string()),
        );

        // No RAR:CompressedSize / RAR:UncompressedSize here.
        //
        // Until 2026-07-26 this emitted a hardcoded 0 for both, which collided
        // with the real values parse_rar_metadata() puts under the ZIP: keys:
        //   $ oxidex ZIP.rar | rg -i compressedsize
        //   RAR:CompressedSize: 0        <- fabricated placeholder
        //   ZIP:CompressedSize: 5        <- parsed from the RAR5 file header
        //   $ exiftool -G1 ZIP.rar
        //   [ZIP]  Compressed Size  : 5
        // ExifTool 13.55 reports RAR-family archives in the ZIP group (there is
        // no RAR group), so the ZIP: keys are the ExifTool-correct ones and the
        // RAR: placeholders were both wrong and a source of prefix-stripped
        // duplicate tags that make the comparison harness non-deterministic.
        // Emitting nothing when the size is unknown is an honest gap; emitting 0
        // is a fabrication that reads as a real value.

        // RAR:HeaderCRC - placeholder
        metadata.insert(
            "RAR:HeaderCRC".to_string(),
            TagValue::new_string("Unknown".to_string()),
        );

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::RAR)
    }
}

/// Standalone function for parsing RAR metadata
///
/// This function provides a convenient interface for parsing RAR archive metadata
/// by instantiating the RARParser and calling its parse method.
///
/// # Arguments
///
/// * `reader` - A FileReader providing access to the RAR file data
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error description
pub fn parse_rar_metadata(
    reader: &dyn crate::core::FileReader,
) -> std::result::Result<MetadataMap, String> {
    let parser = RARParser;
    let mut metadata = parser
        .parse(reader)
        .map_err(|e| format!("RAR parse error: {}", e))?;

    if let Some(entry) =
        rar5_first_file_entry(reader).map_err(|e| format!("RAR parse error: {}", e))?
    {
        if let Some(size) = entry.compressed_size {
            metadata.insert("ZIP:CompressedSize".to_string(), TagValue::Integer(size));
        }
        if let Some(size) = entry.uncompressed_size {
            metadata.insert("ZIP:UncompressedSize".to_string(), TagValue::Integer(size));
        }
        if let Some(name) = entry.file_name {
            metadata.insert("ZIP:ArchivedFileName".to_string(), TagValue::String(name));
        }
        if let Some(os_byte) = entry.host_os {
            // No PrintConv match in ExifTool means it prints the raw value,
            // so an unmapped byte becomes its own number rather than a
            // stand-in string -- see rar5_host_os.
            let os = rar5_host_os(os_byte)
                .map(str::to_string)
                .unwrap_or_else(|| os_byte.to_string());
            metadata.insert("ZIP:OperatingSystem".to_string(), TagValue::String(os));
        }
        let version =
            RARParser::detect_version(reader).map_err(|e| format!("RAR parse error: {}", e))?;
        if version == "5.0" {
            metadata.insert(
                "ZIP:FileVersion".to_string(),
                TagValue::String("RAR v5".to_string()),
            );
        }
    }

    Ok(metadata)
}

/// Extracts the packed-data size from the first RAR5 file block.
fn rar5_compressed_size(reader: &dyn FileReader) -> Result<Option<i64>> {
    const HEADER_FLAG_EXTRA_AREA: u64 = 0x0001;
    const HEADER_FLAG_DATA_AREA: u64 = 0x0002;
    const MAX_BLOCKS: usize = 10_000;

    let file_size = reader.size();
    if file_size < 8 {
        return Ok(None);
    }

    let signature = reader.read(0, 8)?;
    if signature != b"Rar!\x1a\x07\x01\x00" {
        return Ok(None);
    }

    let mut offset = 8u64;
    for _ in 0..MAX_BLOCKS {
        if offset >= file_size {
            break;
        }

        let remaining = file_size - offset;
        let prefix_len = remaining.min(64) as usize;
        if prefix_len < 6 {
            break;
        }
        let block = reader.read(offset, prefix_len)?;

        let (_, size_offset) = match RARParser::read_rar5_u32(block, 0) {
            Ok(value) => value,
            Err(_) => break,
        };
        let (header_size, header_offset) = match RARParser::read_rar5_vint(block, size_offset) {
            Ok(value) => value,
            Err(_) => break,
        };
        if header_size == 0 {
            break;
        }

        let header_prefix_size = match u64::try_from(header_offset) {
            Ok(value) => value,
            Err(_) => break,
        };
        let block_header_size = match header_prefix_size.checked_add(header_size) {
            Some(value) if value <= remaining => value,
            _ => break,
        };

        let (header_type, flags_offset) = match RARParser::read_rar5_vint(block, header_offset) {
            Ok(value) => value,
            Err(_) => break,
        };
        let (header_flags, mut field_offset) = match RARParser::read_rar5_vint(block, flags_offset)
        {
            Ok(value) => value,
            Err(_) => break,
        };

        if header_flags & HEADER_FLAG_EXTRA_AREA != 0 {
            let (_, next_offset) = match RARParser::read_rar5_vint(block, field_offset) {
                Ok(value) => value,
                Err(_) => break,
            };
            field_offset = next_offset;
        }

        let data_size = if header_flags & HEADER_FLAG_DATA_AREA != 0 {
            match RARParser::read_rar5_vint(block, field_offset) {
                Ok((value, _)) => value,
                Err(_) => break,
            }
        } else {
            0
        };

        if header_type == RAR5_HEADER_FILE {
            return Ok(i64::try_from(data_size).ok());
        }

        // The declared header size already includes the extra area.
        offset = match offset
            .checked_add(block_header_size)
            .and_then(|value| value.checked_add(data_size))
        {
            Some(value) if value > offset && value <= file_size => value,
            _ => break,
        };
    }

    Ok(None)
}

/// Fields extracted from the first RAR5 file entry.
struct Rar5FileEntry {
    compressed_size: Option<i64>,
    uncompressed_size: Option<i64>,
    host_os: Option<u8>,
    file_name: Option<String>,
}

/// Mapping from RAR5 host OS byte to the string seen in ExifTool output.
///
/// ExifTool's RAR5 table defines exactly two (ZIP.pm, the `OperatingSystem`
/// entry in the RAR5 tag table): `0 => 'Win32', 1 => 'Unix'`. Anything else
/// has no PrintConv, so ExifTool prints the raw number -- hence `None` here
/// rather than a catch-all string. An earlier revision mapped 2/3/4 to
/// MacOS/BeOS/OS-2 and everything else to "Unknown"; those come from the
/// RAR *4* host-OS numbering, and the two tables disagree (ZIP.pm's other
/// RAR table is 0=MS-DOS, 1=OS/2, 2=Win32, 3=Unix -- not a superset,
/// a different assignment). The recheck passed anyway because the sample
/// archive's host-OS byte is one of the two values that were correct.
///
/// Returning None matters more than it looks: substituting "Unknown" for an
/// unrecognized byte replaces data ExifTool would have shown as a number,
/// turning a visible unknown into an invisible one.
fn rar5_host_os(raw: u8) -> Option<&'static str> {
    match raw {
        0 => Some("Win32"),
        1 => Some("Unix"),
        _ => None,
    }
}

/// Scan RAR5 blocks to extract the first file entry's metadata.
fn rar5_first_file_entry(reader: &dyn FileReader) -> Result<Option<Rar5FileEntry>> {
    const HEADER_FLAG_EXTRA_AREA: u64 = 0x0001;
    const HEADER_FLAG_DATA_AREA: u64 = 0x0002;
    const MAX_BLOCKS: usize = 10_000;

    let file_size = reader.size();
    if file_size < 8 {
        return Ok(None);
    }

    let signature = reader.read(0, 8)?;
    if signature != b"Rar!\x1a\x07\x01\x00" {
        return Ok(None);
    }

    let mut offset = 8u64;
    for _ in 0..MAX_BLOCKS {
        if offset >= file_size {
            break;
        }
        let remaining = file_size - offset;
        let prefix_len = remaining.min(1024) as usize;
        if prefix_len < 6 {
            break;
        }
        let block = reader.read(offset, prefix_len)?;

        let (_, size_offset) = match RARParser::read_rar5_u32(block, 0) {
            Ok(value) => value,
            Err(_) => break,
        };
        let (header_size, header_offset) = match RARParser::read_rar5_vint(block, size_offset) {
            Ok(value) => value,
            Err(_) => break,
        };
        if header_size == 0 {
            break;
        }
        let header_prefix_size = match u64::try_from(header_offset) {
            Ok(value) => value,
            Err(_) => break,
        };
        let block_header_size = match header_prefix_size.checked_add(header_size) {
            Some(value) if value <= remaining => value,
            _ => break,
        };

        let (header_type, flags_offset) = match RARParser::read_rar5_vint(block, header_offset) {
            Ok(value) => value,
            Err(_) => break,
        };
        let (header_flags, mut field_offset) = match RARParser::read_rar5_vint(block, flags_offset)
        {
            Ok(value) => value,
            Err(_) => break,
        };

        if header_flags & HEADER_FLAG_EXTRA_AREA != 0 {
            let (_, next_offset) = match RARParser::read_rar5_vint(block, field_offset) {
                Ok(value) => value,
                Err(_) => break,
            };
            field_offset = next_offset;
        }

        let data_size = if header_flags & HEADER_FLAG_DATA_AREA != 0 {
            match RARParser::read_rar5_vint(block, field_offset) {
                Ok((value, next)) => {
                    // The data-size vint is part of the generic header, not the
                    // per-file record; advance field_offset so the
                    // file_flags/unpacked size fields are read from the correct position.
                    field_offset = next;
                    value
                }
                Err(_) => break,
            }
        } else {
            0
        };

        if header_type == RAR5_HEADER_FILE {
            // file flags vint
            let (file_flags, after_flags) = match RARParser::read_rar5_vint(block, field_offset) {
                Ok(v) => v,
                Err(_) => break,
            };
            // unpacked size vint
            let (unpacked_size, after_unpacked) =
                match RARParser::read_rar5_vint(block, after_flags) {
                    Ok(v) => v,
                    Err(_) => break,
                };
            // attributes vint (skip)
            let (_, after_attr) = match RARParser::read_rar5_vint(block, after_unpacked) {
                Ok(v) => v,
                Err(_) => break,
            };
            let mut pos = after_attr;
            // mtime if bit 0
            if file_flags & 0x01 != 0 {
                if pos + 4 > block.len() {
                    break;
                }
                pos += 4;
            }
            // data CRC if bit 1
            if file_flags & 0x02 != 0 {
                if pos + 4 > block.len() {
                    break;
                }
                pos += 4;
            }
            // compression info if bit 2
            if file_flags & 0x04 != 0 {
                let (_, after_comp) = match RARParser::read_rar5_vint(block, pos) {
                    Ok(v) => v,
                    Err(_) => break,
                };
                pos = after_comp;
            }
            // host OS vint
            let (host_os, after_os) = match RARParser::read_rar5_vint(block, pos) {
                Ok(v) => v,
                Err(_) => break,
            };
            pos = after_os;
            // file name length vint
            let (name_len, after_name_len) = match RARParser::read_rar5_vint(block, pos) {
                Ok(v) => v,
                Err(_) => break,
            };
            let name_len_usize = usize::try_from(name_len).unwrap_or(usize::MAX);
            if after_name_len + name_len_usize > block.len() {
                break;
            }
            let name_bytes = &block[after_name_len..after_name_len + name_len_usize];
            let file_name = String::from_utf8_lossy(name_bytes).into_owned();
            return Ok(Some(Rar5FileEntry {
                compressed_size: i64::try_from(data_size).ok(),
                uncompressed_size: i64::try_from(unpacked_size).ok(),
                host_os: Some(host_os as u8),
                file_name: Some(file_name),
            }));
        }

        // advance to next block
        offset = match offset
            .checked_add(block_header_size)
            .and_then(|value| value.checked_add(data_size))
        {
            Some(value) if value > offset && value <= file_size => value,
            _ => break,
        };
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;
    use crate::test_support::assert_no_divergent_prefixed_duplicates;

    /// ExifTool's own `ZIP.rar` sample, 74 bytes, embedded verbatim so this
    /// test does not depend on the sample corpus being present:
    /// `md5(ZIP.rar) = a19da0e9c47e4155119620dc369869cc`. It is a RAR5 archive
    /// holding one 5-byte file, `1.txt`.
    const ZIP_RAR_SAMPLE: &[u8] = &[
        0x52, 0x61, 0x72, 0x21, 0x1a, 0x07, 0x01, 0x00, 0x33, 0x92, 0xb5, 0xe5, 0x0a, 0x01, 0x05,
        0x06, 0x00, 0x05, 0x01, 0x01, 0x80, 0x80, 0x00, 0x64, 0xe8, 0xc3, 0x50, 0x21, 0x02, 0x03,
        0x0b, 0x85, 0x00, 0x04, 0x85, 0x00, 0x20, 0x82, 0x89, 0xd1, 0xf7, 0x80, 0x00, 0x00, 0x05,
        0x31, 0x2e, 0x74, 0x78, 0x74, 0x0a, 0x03, 0x02, 0x86, 0x76, 0xf3, 0x66, 0x60, 0x10, 0xd9,
        0x01, 0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x1d, 0x77, 0x56, 0x51, 0x03, 0x05, 0x04, 0x00,
    ];

    /// ExifTool 13.55 has no RAR group -- it reports RAR-family archives under
    /// ZIP -- and it reads the real size out of the file header:
    ///
    /// ```text
    /// $ exiftool -G1 -CompressedSize -UncompressedSize \
    ///     /tmp/oxidex-exiftool-cache/combined-samples/ZIP.rar
    /// [ZIP]           Compressed Size                 : 5
    /// [ZIP]           Uncompressed Size               : 5
    /// ```
    ///
    /// Until 2026-07-26 `RARParser::parse` unconditionally inserted a hardcoded
    /// `RAR:CompressedSize: 0` / `RAR:UncompressedSize: 0`, so this exact file
    /// produced `RAR:CompressedSize: 0` alongside `ZIP:CompressedSize: 5`. The
    /// comparison harness strips the group prefix before matching, so which of
    /// the two it scored was a coin flip.
    #[test]
    fn rar_emits_no_fabricated_size_placeholder_beside_the_zip_keys() {
        let reader = TestReader::from_slice(ZIP_RAR_SAMPLE);
        let metadata = parse_rar_metadata(&reader).unwrap();

        assert_eq!(
            metadata.get("ZIP:CompressedSize"),
            Some(&TagValue::Integer(5)),
            "the ZIP: keys are the ExifTool-named ones and carry the parsed size",
        );
        for fabricated in ["RAR:CompressedSize", "RAR:UncompressedSize"] {
            assert_eq!(
                metadata.get(fabricated),
                None,
                "{fabricated} was a hardcoded 0 placeholder that collided with \
                 the real ZIP: value; an absent tag is an honest gap, a 0 is a \
                 fabrication",
            );
        }

        assert_no_divergent_prefixed_duplicates(&metadata);
    }

    /// ExifTool's RAR5 OperatingSystem PrintConv is exactly {0: Win32,
    /// 1: Unix} (ZIP.pm). Literal bytes on purpose -- naming a constant
    /// here would assert the constant's own meaning back at itself.
    #[test]
    fn rar5_host_os_matches_exiftool_and_stops_there() {
        assert_eq!(rar5_host_os(0), Some("Win32"));
        assert_eq!(rar5_host_os(1), Some("Unix"));
        // 2/3/4 are RAR *4* host-OS numbers, not RAR5 ones. Mapping them
        // would print a stand-in where ExifTool prints the raw number.
        for raw in [2u8, 3, 4, 5, 255] {
            assert_eq!(
                rar5_host_os(raw),
                None,
                "RAR5 host OS {raw} has no ExifTool PrintConv; it must fall \
                 through to the raw value, not a substituted string",
            );
        }
    }

    #[test]
    fn test_rar_signature() {
        let mut data = b"Rar!".to_vec();
        data.extend_from_slice(&[0x1A, 0x07, 0x00]);
        let reader = TestReader::new(data);
        assert!(RARParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_rar5_detection() {
        let mut data = b"Rar!".to_vec();
        data.extend_from_slice(&[0x1A, 0x07, 0x01, 0x01]);
        let reader = TestReader::new(data);
        assert_eq!(RARParser::detect_version(&reader).unwrap(), "5.0");
    }

    #[test]
    fn test_rar4_metadata_extraction() {
        // Create minimal RAR4 archive with header
        let mut data = b"Rar!".to_vec();
        data.extend_from_slice(&[0x1A, 0x07, 0x00]); // RAR4 signature
        // Archive header block
        data.extend_from_slice(&[
            0x33, 0x92, // HEAD_CRC
            0x73, // HEAD_TYPE (archive)
            0x09, 0x00, // HEAD_FLAGS (solid + volume)
            0x0D, 0x00, // HEAD_SIZE
        ]);
        data.extend_from_slice(&[0x00; 6]); // Reserved

        let reader = TestReader::new(data);
        let parser = RARParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("FileType").unwrap(),
            &TagValue::String("RAR".to_string())
        );
        assert_eq!(
            metadata.get("RARVersion").unwrap(),
            &TagValue::String("4.x".to_string())
        );
        assert_eq!(
            metadata.get("IsSolid").unwrap(),
            &TagValue::String("true".to_string())
        );
        assert_eq!(
            metadata.get("IsVolume").unwrap(),
            &TagValue::String("true".to_string())
        );
    }

    #[test]
    fn test_rar5_vint_parsing() {
        // Test variable-length integer parsing
        let data = vec![0x80, 0x01]; // 128 in vint format
        let (value, pos) = RARParser::read_rar5_vint(&data, 0).unwrap();
        assert_eq!(value, 128);
        assert_eq!(pos, 2);

        let data = vec![0x7F]; // 127 in vint format
        let (value, pos) = RARParser::read_rar5_vint(&data, 0).unwrap();
        assert_eq!(value, 127);
        assert_eq!(pos, 1);
    }

    #[test]
    fn test_rar4_file_counting() {
        // Create RAR4 archive with 2 file headers
        let mut data = b"Rar!".to_vec();
        data.extend_from_slice(&[0x1A, 0x07, 0x00]); // RAR4 signature

        // Archive header
        data.extend_from_slice(&[
            0x33, 0x92, // CRC
            0x73, // Type: archive
            0x00, 0x00, // Flags
            0x0D, 0x00, // Size: 13 bytes
        ]);
        data.extend_from_slice(&[0x00; 6]); // Reserved

        // File header 1
        data.extend_from_slice(&[
            0x33, 0x92, // CRC
            0x74, // Type: file
            0x00, 0x00, // Flags
            0x20, 0x00, // Size: 32 bytes
        ]);
        data.extend_from_slice(&[0x00; 25]); // Rest of file header

        // File header 2
        data.extend_from_slice(&[
            0x33, 0x92, // CRC
            0x74, // Type: file
            0x00, 0x00, // Flags
            0x20, 0x00, // Size: 32 bytes
        ]);
        data.extend_from_slice(&[0x00; 25]); // Rest of file header

        let reader = TestReader::new(data);
        let count = RARParser::count_rar4_files(&reader).unwrap();
        assert_eq!(count, 2);
    }
}
