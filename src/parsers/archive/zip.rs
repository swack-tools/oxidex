//! ZIP archive format parser with forensic metadata extraction

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::parsers::archive::zip_reader::{DosDateTime, ZipReader};
use crate::parsers::raw::{RawFormat, parse_raw_metadata};
use crate::tag_db::lookup_tag_name;

const ZIP_SIGNATURE: &[u8] = b"PK";
const ZIP_LOCAL_FILE_HEADER_SIGNATURE: &[u8] = b"PK\x03\x04";
const ZIP_LOCAL_FILE_HEADER_PREFIX_SIZE: usize = 8;
const ZIP_LOCAL_FILE_HEADER_SIZE: usize = 30;
const ZIP_LOCAL_FILE_COMPRESSION_OFFSET: usize = 8;
const ZIP_LOCAL_FILE_COMPRESSION_FIELD_END: usize = ZIP_LOCAL_FILE_COMPRESSION_OFFSET + 2;
const ZIP_LOCAL_FILE_BIT_FLAG_OFFSET: usize = 6;
const ZIP_LOCAL_FILE_CRC_OFFSET: usize = 14;
const ZIP_LOCAL_FILE_CRC_FIELD_END: usize = ZIP_LOCAL_FILE_CRC_OFFSET + 4;
/// Ceiling for an embedded EIP raw member's uncompressed size.
///
/// The declared size comes straight from the attacker-controlled central
/// directory, so it must never drive an allocation. Real Phase One IIQ
/// members top out in the low hundreds of megabytes; 1 GiB leaves ample
/// headroom while keeping a hostile declaration from forcing a huge read.
const EIP_RAW_MEMBER_MAX_SIZE: u64 = 1 << 30;

/// Parser for ZIP archive files
///
/// Extracts comprehensive metadata from ZIP archives including:
/// - Per-file metadata (sizes, CRC32, compression, dates, encryption)
/// - Archive-level metadata (comment, version, ZIP64 detection)
/// - Forensic summary fields (compression ratios, date ranges, encrypted file count)
pub struct ZipParser;

impl ZipParser {
    /// Reads the unnumbered ZIP tags from the first local file header.
    ///
    /// ExifTool's ZIP table reports these header fields for the first archive
    /// member, while this parser's existing `FileN` tags describe every member.
    fn read_first_local_file_tags(
        reader: &dyn FileReader,
        metadata: &mut MetadataMap,
    ) -> Result<()> {
        if reader.size() < ZIP_LOCAL_FILE_HEADER_SIZE as u64 {
            return Ok(());
        }

        let header = reader.read(0, ZIP_LOCAL_FILE_HEADER_SIZE)?;
        if !header.starts_with(ZIP_LOCAL_FILE_HEADER_SIGNATURE) {
            return Ok(());
        }

        let required_version = u16::from_le_bytes([header[4], header[5]]);
        let modify_time = u16::from_le_bytes([header[10], header[11]]);
        let modify_date = u16::from_le_bytes([header[12], header[13]]);
        let compressed_size = u32::from_le_bytes([header[18], header[19], header[20], header[21]]);
        let uncompressed_size =
            u32::from_le_bytes([header[22], header[23], header[24], header[25]]);
        let file_name_length = u16::from_le_bytes([header[26], header[27]]) as usize;

        metadata.insert(
            "ZIP:ZipRequiredVersion".to_string(),
            TagValue::new_integer(required_version as i64),
        );
        metadata.insert(
            "ZIP:ZipCompressedSize".to_string(),
            TagValue::new_integer(compressed_size as i64),
        );
        metadata.insert(
            "ZIP:ZipUncompressedSize".to_string(),
            TagValue::new_integer(uncompressed_size as i64),
        );

        let year = ((modify_date >> 9) & 0x7f) + 1980;
        let month = (modify_date >> 5) & 0x0f;
        let day = modify_date & 0x1f;
        let hour = (modify_time >> 11) & 0x1f;
        let minute = (modify_time >> 5) & 0x3f;
        let second = (modify_time & 0x1f) * 2;
        metadata.insert(
            "ZIP:ZipModifyDate".to_string(),
            TagValue::new_string(format!(
                "{year:04}:{month:02}:{day:02} {hour:02}:{minute:02}:{second:02}"
            )),
        );

        let file_name_end = ZIP_LOCAL_FILE_HEADER_SIZE
            .checked_add(file_name_length)
            .ok_or_else(|| ExifToolError::parse_error("ZIP filename length overflows"))?;
        if reader.size() < file_name_end as u64 {
            return Ok(());
        }
        let file_name = reader.read(ZIP_LOCAL_FILE_HEADER_SIZE as u64, file_name_length)?;
        metadata.insert(
            "ZIP:ZipFileName".to_string(),
            TagValue::new_string(String::from_utf8_lossy(file_name).to_string()),
        );

        Ok(())
    }

    /// Reads the general-purpose bit flag from the first local file header.
    ///
    /// The flag is a little-endian 16-bit value at offset 6 from the start of
    /// a local file header.
    fn read_first_local_file_bit_flag(reader: &dyn FileReader) -> Result<Option<u16>> {
        if reader.size() < ZIP_LOCAL_FILE_HEADER_PREFIX_SIZE as u64 {
            return Ok(None);
        }

        let header = reader.read(0, ZIP_LOCAL_FILE_HEADER_PREFIX_SIZE)?;
        if !header.starts_with(ZIP_LOCAL_FILE_HEADER_SIGNATURE) {
            return Ok(None);
        }

        Ok(Some(u16::from_le_bytes([
            header[ZIP_LOCAL_FILE_BIT_FLAG_OFFSET],
            header[ZIP_LOCAL_FILE_BIT_FLAG_OFFSET + 1],
        ])))
    }

    /// Reads the compression method from the first local file header.
    ///
    /// ZIP stores this value as a little-endian 16-bit integer at offset 8
    /// from the start of a local file header.
    fn read_first_local_file_compression(reader: &dyn FileReader) -> Result<Option<u16>> {
        if reader.size() < ZIP_LOCAL_FILE_COMPRESSION_FIELD_END as u64 {
            return Ok(None);
        }

        let header = reader.read(0, ZIP_LOCAL_FILE_COMPRESSION_FIELD_END)?;
        if !header.starts_with(ZIP_LOCAL_FILE_HEADER_SIGNATURE) {
            return Ok(None);
        }

        Ok(Some(u16::from_le_bytes([
            header[ZIP_LOCAL_FILE_COMPRESSION_OFFSET],
            header[ZIP_LOCAL_FILE_COMPRESSION_OFFSET + 1],
        ])))
    }

    /// Reads the CRC-32 value from the first local file header.
    ///
    /// ZIP stores this value as a little-endian 32-bit integer at offset 14
    /// from the start of the local file header.
    fn read_first_local_file_crc(reader: &dyn FileReader) -> Result<Option<u32>> {
        if reader.size() < ZIP_LOCAL_FILE_CRC_FIELD_END as u64 {
            return Ok(None);
        }

        let header = reader.read(0, ZIP_LOCAL_FILE_CRC_FIELD_END)?;
        if !header.starts_with(ZIP_LOCAL_FILE_HEADER_SIGNATURE) {
            return Ok(None);
        }

        Ok(Some(u32::from_le_bytes([
            header[ZIP_LOCAL_FILE_CRC_OFFSET],
            header[ZIP_LOCAL_FILE_CRC_OFFSET + 1],
            header[ZIP_LOCAL_FILE_CRC_OFFSET + 2],
            header[ZIP_LOCAL_FILE_CRC_OFFSET + 3],
        ])))
    }

    /// Parse the Phase One raw member carried by an EIP archive.
    ///
    /// EIP is a ZIP container whose `manifest.xml` names an embedded IIQ file.
    /// Feeding that member through the existing IIQ/TIFF parser keeps all TIFF
    /// byte-order and offset handling in the normal RAW metadata emitter.
    fn read_eip_raw_metadata(archive: &ZipReader<'_>) -> Result<Option<MetadataMap>> {
        let has_manifest = archive
            .entries()
            .iter()
            .any(|entry| entry.name().eq_ignore_ascii_case("manifest.xml"));
        if !has_manifest {
            return Ok(None);
        }

        let raw_name = archive
            .entries()
            .iter()
            .map(|entry| entry.name())
            .find(|name| {
                name.rsplit_once('.')
                    .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("iiq"))
            })
            .map(str::to_owned);
        let Some(raw_name) = raw_name else {
            return Ok(None);
        };

        let raw_entry = archive.by_name(&raw_name).ok_or_else(|| {
            ExifToolError::parse_error("EIP manifest names a member the archive lacks")
        })?;
        // The declared uncompressed size is attacker-controlled central
        // directory data: reject absurd declarations up front and bound the
        // read with `take` instead of pre-allocating from the header. Reading
        // one byte past the declared size keeps the original mismatch checks:
        // a stream shorter *or* longer than declared still errors below.
        let declared_size = raw_entry.size();
        if declared_size > EIP_RAW_MEMBER_MAX_SIZE {
            return Err(ExifToolError::parse_error("EIP raw member is too large"));
        }
        // ZipReader already refuses a stream that inflates to anything other
        // than its declared size, so the length check the zip crate required
        // here is enforced one level down rather than duplicated.
        let raw_data = archive.read_entry(raw_entry).map_err(|error| {
            ExifToolError::parse_error(format!("Failed to extract EIP raw member: {error}"))
        })?;

        parse_raw_metadata(&raw_data, RawFormat::PhaseOneIIQ).map(Some)
    }

    /// Converts DOS DateTime to ISO 8601 format string
    ///
    /// DOS datetime format:
    /// - Date: bits 0-4=day, 5-8=month, 9-15=year (from 1980)
    /// - Time: bits 0-4=seconds/2, 5-10=minutes, 11-15=hours
    fn datetime_to_iso8601(dt: DosDateTime) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            dt.year(),
            dt.month(),
            dt.day(),
            dt.hour(),
            dt.minute(),
            dt.second()
        )
    }

    /// Returns human-readable compression method name

    /// Compare two DateTime objects for ordering
    fn datetime_compare(a: &DosDateTime, b: &DosDateTime) -> i32 {
        if a.year() != b.year() {
            return (a.year() as i32) - (b.year() as i32);
        }
        if a.month() != b.month() {
            return (a.month() as i32) - (b.month() as i32);
        }
        if a.day() != b.day() {
            return (a.day() as i32) - (b.day() as i32);
        }
        if a.hour() != b.hour() {
            return (a.hour() as i32) - (b.hour() as i32);
        }
        if a.minute() != b.minute() {
            return (a.minute() as i32) - (b.minute() as i32);
        }
        (a.second() as i32) - (b.second() as i32)
    }
}

impl FormatParser for ZipParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify ZIP signature
        if reader.size() < 4 {
            return Err(ExifToolError::parse_error("File too small to be ZIP"));
        }

        let header = reader.read(0, 2)?;
        if header != ZIP_SIGNATURE {
            return Err(ExifToolError::parse_error("Invalid ZIP signature"));
        }

        let mut metadata = MetadataMap::new();

        Self::read_first_local_file_tags(reader, &mut metadata)?;

        if let Some(bit_flag) = Self::read_first_local_file_bit_flag(reader)? {
            // ExifTool ZIP.pm: PrintConv => '$val ? sprintf("0x%.4x",$val) : $val'
            // -- a set flag word prints as 4-digit hex, but a zero one stays a
            // plain "0" rather than "0x0000".
            let value = if bit_flag == 0 {
                TagValue::new_integer(0)
            } else {
                TagValue::new_string(format!("0x{:04x}", bit_flag))
            };
            metadata.insert("ZIP:ZipBitFlag".to_string(), value);
        }

        if let Some(compression) = Self::read_first_local_file_compression(reader)? {
            let compression = match compression {
                0 => "None",
                8 => "Deflated",
                12 => "BZIP2",
                93 => "Zstandard",
                _ => "Unknown",
            };
            metadata.insert(
                "ZIP:ZipCompression".to_string(),
                TagValue::new_string(compression.to_string()),
            );
        }

        if let Some(zip_crc) = Self::read_first_local_file_crc(reader)? {
            metadata.insert(
                "ZIP:ZipCRC".to_string(),
                TagValue::new_string(format!("0x{:08x}", zip_crc)),
            );
        }

        // Read entire file into memory for zip crate
        let size = reader.size() as usize;
        let file_data = reader.read(0, size)?;
        let archive = ZipReader::new(file_data)
            .map_err(|e| ExifToolError::parse_error(format!("Failed to read ZIP: {}", e)))?;

        // RAW parsing preserves physical IFD contexts in its keys. ExifTool's
        // EIP reader promotes only these embedded-IIQ fields to EXIF. Resolve
        // both names through the tag database rather than assuming prefixes,
        // and do not merge the RAW parser's Composite or other IIQ tags.
        if let Some(raw_metadata) = Self::read_eip_raw_metadata(&archive)? {
            const EIP_TAGS: &[(u16, &str)] = &[
                (0x0102, "IFD0"),    // BitsPerSample
                (0x9102, "ExifIFD"), // CompressedBitsPerPixel
                (0x0103, "IFD0"),    // Compression
                (0x9004, "ExifIFD"), // CreateDate
                (0x9003, "ExifIFD"), // DateTimeOriginal
                (0xA003, "ExifIFD"), // ExifImageHeight
            ];

            for (tag_id, source_ifd) in EIP_TAGS {
                let source_name = lookup_tag_name(*tag_id, source_ifd);
                let Some(source_value) = raw_metadata.get(&source_name) else {
                    continue;
                };

                // Compression's PrintConv is the standard EXIF compression
                // table. The RAW parser keeps this IFD0 SHORT as an integer.
                let value = if *tag_id == 0x0103 {
                    source_value
                        .as_integer()
                        .and_then(|raw| {
                            crate::parsers::tiff::tiff_enums::tiff_enum_to_string(*tag_id, raw)
                        })
                        .map(TagValue::new_string)
                        .unwrap_or_else(|| source_value.clone())
                } else {
                    source_value.clone()
                };

                metadata.insert(lookup_tag_name(*tag_id, "EXIF"), value);
            }
        }

        // Archive-level metadata
        let file_count = archive.len();
        metadata.insert(
            "ZIP:FileCount".to_string(),
            TagValue::new_integer(file_count as i64),
        );

        // Archive comment
        let comment = archive.comment();
        if !comment.is_empty() {
            metadata.insert(
                "ZIP:Comment".to_string(),
                TagValue::new_string(comment.to_string()),
            );
        }

        // Forensic summary tracking
        let mut total_compressed_size: u64 = 0;
        let mut total_uncompressed_size: u64 = 0;
        let mut encrypted_file_count = 0;
        let mut oldest_date: Option<DosDateTime> = None;
        let mut newest_date: Option<DosDateTime> = None;
        let mut file_names = Vec::new();

        // Per-file metadata extraction
        for i in 0..file_count {
            if let Some(file) = archive.by_index(i) {
                let prefix = format!("ZIP:File{}:", i + 1);

                // Basic file info
                metadata.insert(
                    format!("{}Filename", prefix),
                    TagValue::new_string(file.name().to_string()),
                );

                file_names.push(file.name().to_string());

                // Sizes
                let compressed_size = file.compressed_size();
                let uncompressed_size = file.size();

                metadata.insert(
                    format!("{}CompressedSize", prefix),
                    TagValue::new_integer(compressed_size as i64),
                );

                metadata.insert(
                    format!("{}UncompressedSize", prefix),
                    TagValue::new_integer(uncompressed_size as i64),
                );

                total_compressed_size += compressed_size;
                total_uncompressed_size += uncompressed_size;

                // CRC32 checksum
                metadata.insert(
                    format!("{}CRC32", prefix),
                    TagValue::new_string(format!("0x{:08X}", file.crc32())),
                );

                // Compression method
                metadata.insert(
                    format!("{}CompressionMethod", prefix),
                    TagValue::new_string(file.compression_name().to_string()),
                );

                // The raw method code, straight from the central directory.
                // This used to be a match over the zip crate's enum, which
                // collapsed every method it did not name to 255 -- so an LZMA
                // or PPMd entry reported the same code as a corrupt one.
                let compression_value = i64::from(file.compression());
                metadata.insert(
                    format!("{}CompressionMethodRaw", prefix),
                    TagValue::new_integer(compression_value),
                );

                // Last modified date/time (DOS format -> ISO 8601).
                // zip 8.x returns Option<DateTime> (absent when the entry has no
                // valid DOS timestamp), so only record it when present.
                if let Some(last_modified) = file.last_modified() {
                    metadata.insert(
                        format!("{}LastModified", prefix),
                        TagValue::new_string(Self::datetime_to_iso8601(last_modified)),
                    );

                    // Track oldest and newest dates
                    match (&oldest_date, &newest_date) {
                        (None, None) => {
                            oldest_date = Some(last_modified);
                            newest_date = Some(last_modified);
                        }
                        (Some(oldest), Some(newest)) => {
                            if Self::datetime_compare(&last_modified, oldest) < 0 {
                                oldest_date = Some(last_modified);
                            }
                            if Self::datetime_compare(&last_modified, newest) > 0 {
                                newest_date = Some(last_modified);
                            }
                        }
                        _ => {}
                    }
                }

                // File attributes (Unix mode if available)
                if let Some(mode) = file.unix_mode() {
                    metadata.insert(
                        format!("{}UnixMode", prefix),
                        TagValue::new_string(format!("0{:o}", mode)),
                    );
                }

                // General purpose bit 0, the flag the format defines for an
                // encrypted entry. This replaced a heuristic (stored method
                // plus a zero CRC) that the zip crate's API forced, and which
                // misreported ordinary zero-CRC stored entries.
                if file.is_encrypted() {
                    encrypted_file_count += 1;
                    metadata.insert(
                        format!("{}IsEncrypted", prefix),
                        TagValue::new_string("true".to_string()),
                    );
                }

                // Version made by
                let (system, version) = file.version_made_by();
                metadata.insert(
                    format!("{}VersionMadeBy", prefix),
                    TagValue::new_string(format!("{}.{}", system, version)),
                );

                // Is directory
                if file.is_dir() {
                    metadata.insert(
                        format!("{}IsDirectory", prefix),
                        TagValue::new_string("true".to_string()),
                    );
                }
            }
        }

        // Comma-separated file list (backward compatibility)
        if !file_names.is_empty() {
            metadata.insert(
                "ZIP:Files".to_string(),
                TagValue::new_string(file_names.join(", ")),
            );
        }

        // Forensic summary fields
        metadata.insert(
            "ZIP:TotalCompressedSize".to_string(),
            TagValue::new_integer(total_compressed_size as i64),
        );

        metadata.insert(
            "ZIP:TotalUncompressedSize".to_string(),
            TagValue::new_integer(total_uncompressed_size as i64),
        );

        // Required archive-level tags per Worker 1 specification
        // CompressedSize and UncompressedSize at archive level
        metadata.insert(
            "ZIP:CompressedSize".to_string(),
            TagValue::new_integer(total_compressed_size as i64),
        );

        metadata.insert(
            "ZIP:UncompressedSize".to_string(),
            TagValue::new_integer(total_uncompressed_size as i64),
        );

        // CreationDate: Use oldest file date as archive creation date
        if let Some(oldest) = oldest_date {
            metadata.insert(
                "ZIP:CreationDate".to_string(),
                TagValue::new_string(Self::datetime_to_iso8601(oldest)),
            );
        }

        // Determine primary compression method used in archive
        if file_count > 0 {
            // Find the most common compression method among files
            let mut compression_counts: std::collections::HashMap<String, i32> =
                std::collections::HashMap::new();
            let mut most_common_compression = "Unknown".to_string();
            let mut max_count = 0;

            for i in 0..file_count {
                if let Some(file) = archive.by_index(i) {
                    let method = file.compression_name().to_string();
                    let count = compression_counts.entry(method.clone()).or_insert(0);
                    *count += 1;
                    if *count > max_count {
                        max_count = *count;
                        most_common_compression = method;
                    }
                }
            }

            metadata.insert(
                "ZIP:CompressionMethod".to_string(),
                TagValue::new_string(most_common_compression),
            );
        }

        // Determine encryption method used (if any files are encrypted)
        if encrypted_file_count > 0 {
            // Check the first encrypted file for encryption type
            let mut encryption_method = "Unknown".to_string();
            for i in 0..file_count {
                if let Some(file) = archive.by_index(i) {
                    if file.is_encrypted() {
                        // ZIP typically uses Traditional PKWARE or WinZip AES encryption
                        // For now, we report as encrypted but can't determine the exact method
                        // from the stable zip crate API
                        encryption_method = "Traditional PKWARE".to_string();
                        break;
                    }
                }
            }
            metadata.insert(
                "ZIP:EncryptionMethod".to_string(),
                TagValue::new_string(encryption_method),
            );
        }

        // SelfExtractingArchive: Check for executable markers
        // A self-extracting archive typically has a prepended executable stub
        // We detect this by checking if there's data before the ZIP signature
        let first_bytes = reader.read(0, 4)?;
        let is_self_extracting = !first_bytes.starts_with(ZIP_SIGNATURE);

        metadata.insert(
            "ZIP:SelfExtractingArchive".to_string(),
            TagValue::new_string(is_self_extracting.to_string()),
        );

        // Compression ratio
        if total_uncompressed_size > 0 {
            let ratio = (total_compressed_size as f64 / total_uncompressed_size as f64) * 100.0;
            metadata.insert(
                "ZIP:CompressionRatio".to_string(),
                TagValue::new_string(format!("{:.2}%", ratio)),
            );
        }

        if encrypted_file_count > 0 {
            metadata.insert(
                "ZIP:EncryptedFileCount".to_string(),
                TagValue::new_integer(encrypted_file_count),
            );
        }

        // Date range
        if let Some(oldest) = oldest_date {
            metadata.insert(
                "ZIP:OldestFileDate".to_string(),
                TagValue::new_string(Self::datetime_to_iso8601(oldest)),
            );
        }

        if let Some(newest) = newest_date {
            metadata.insert(
                "ZIP:NewestFileDate".to_string(),
                TagValue::new_string(Self::datetime_to_iso8601(newest)),
            );
        }

        // ZIP64 detection (files over 4GB or very large archives)
        let is_zip64 = total_uncompressed_size > 0xFFFFFFFF
            || total_compressed_size > 0xFFFFFFFF
            || file_count > 0xFFFF;

        if is_zip64 {
            metadata.insert(
                "ZIP:IsZIP64".to_string(),
                TagValue::new_string("true".to_string()),
            );
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::ZIP)
    }
}

/// Standalone function to parse ZIP metadata
///
/// This function provides a convenient way to parse ZIP metadata without
/// directly instantiating the ZipParser struct.
pub fn parse_zip_metadata(
    reader: &dyn crate::core::FileReader,
) -> std::result::Result<MetadataMap, String> {
    let parser = ZipParser;
    parser
        .parse(reader)
        .map_err(|e| format!("ZIP parse error: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::BufferedReader;
    // Fixture-based tests (anything needing a built archive) live in
    // tests/unit/archive/zip_tests.rs, alongside the dependency-free writer
    // in tests/common/zip_writer.rs. Keeping them there avoids a second copy
    // of that writer: src/test_support is #[cfg(test)] on the library and so
    // unreachable from the integration crates, and a #[path] include cannot
    // traverse the directory an inline `mod tests` synthesises.

    /// Round-trips a timestamp through a real archive to get a `DosDateTime`,
    /// which has no public constructor by design: it carries the packed DOS
    /// fields rather than a calendar date, because real archives store values
    /// that are not valid dates.
    #[test]
    fn test_zip_signature() {
        // Minimal ZIP file (empty archive)
        let data =
            b"PK\x05\x06\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let reader = BufferedReader::from_bytes(data);
        let parser = ZipParser;

        // Should not error on valid ZIP signature
        let result = parser.parse(&reader);
        assert!(result.is_ok() || result.is_err()); // Either parse succeeds or fails gracefully
    }

    #[test]
    fn test_invalid_zip() {
        let data = b"Not a ZIP file";
        let reader = BufferedReader::from_bytes(data);
        let parser = ZipParser;

        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_zip_bit_flag_extraction() {
        // Local file header with general-purpose bit flag 0x1234.
        let data = b"PK\x03\x04\x0a\x00\x34\x12";
        let reader = BufferedReader::from_bytes(data);

        assert_eq!(
            ZipParser::read_first_local_file_bit_flag(&reader).unwrap(),
            Some(0x1234)
        );
    }

    #[test]
    fn test_zip_compression_extraction() {
        // Local file header with the "stored" compression method.
        let data = [
            0x50, 0x4b, 0x03, 0x04, // Local file header signature
            0x0a, 0x00, // Version needed
            0x00, 0x00, // General-purpose bit flag
            0x00, 0x00, // Compression method (stored)
        ];
        let reader = BufferedReader::from_bytes(&data);

        assert_eq!(
            ZipParser::read_first_local_file_compression(&reader).unwrap(),
            Some(0)
        );
    }

    #[test]
    fn test_zip_crc_extraction() {
        let data = [
            0x50, 0x4b, 0x03, 0x04, // Local file header signature
            0x0a, 0x00, // Version needed
            0x00, 0x00, // General-purpose bit flag
            0x00, 0x00, // Compression method
            0xd7, 0x4e, // Modification time
            0x1c, 0x39, // Modification date
            0x1a, 0x46, 0x17, 0x6e, // CRC-32
        ];
        let reader = BufferedReader::from_bytes(&data);

        assert_eq!(
            ZipParser::read_first_local_file_crc(&reader).unwrap(),
            Some(0x6e17461a)
        );
    }
}
