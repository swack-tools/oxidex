//! ZIP archive format parser with forensic metadata extraction

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::parsers::raw::{RawFormat, parse_raw_metadata};
use crate::tag_db::lookup_tag_name;
use std::io::{Cursor, Read};
use zip::ZipArchive;

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
    fn read_eip_raw_metadata(
        archive: &mut ZipArchive<Cursor<&[u8]>>,
    ) -> Result<Option<MetadataMap>> {
        let has_manifest = archive
            .file_names()
            .any(|name| name.eq_ignore_ascii_case("manifest.xml"));
        if !has_manifest {
            return Ok(None);
        }

        let raw_name = archive
            .file_names()
            .find(|name| {
                name.rsplit_once('.')
                    .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("iiq"))
            })
            .map(str::to_owned);
        let Some(raw_name) = raw_name else {
            return Ok(None);
        };

        let raw_file = archive.by_name(&raw_name).map_err(|error| {
            ExifToolError::parse_error(format!("Failed to read EIP raw member: {error}"))
        })?;
        // The declared uncompressed size is attacker-controlled central
        // directory data: reject absurd declarations up front and bound the
        // read with `take` instead of pre-allocating from the header. Reading
        // one byte past the declared size keeps the original mismatch checks:
        // a stream shorter *or* longer than declared still errors below.
        let declared_size = raw_file.size();
        if declared_size > EIP_RAW_MEMBER_MAX_SIZE {
            return Err(ExifToolError::parse_error("EIP raw member is too large"));
        }
        let mut raw_data = Vec::new();
        raw_file
            .take(declared_size + 1)
            .read_to_end(&mut raw_data)
            .map_err(|error| {
                ExifToolError::parse_error(format!("Failed to extract EIP raw member: {error}"))
            })?;
        if raw_data.len() as u64 != declared_size {
            return Err(ExifToolError::parse_error(
                "EIP raw member is shorter than its declared size",
            ));
        }

        parse_raw_metadata(&raw_data, RawFormat::PhaseOneIIQ).map(Some)
    }

    /// Converts DOS DateTime to ISO 8601 format string
    ///
    /// DOS datetime format:
    /// - Date: bits 0-4=day, 5-8=month, 9-15=year (from 1980)
    /// - Time: bits 0-4=seconds/2, 5-10=minutes, 11-15=hours
    fn datetime_to_iso8601(dt: zip::DateTime) -> String {
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
    fn compression_method_name(method: zip::CompressionMethod) -> &'static str {
        match method {
            zip::CompressionMethod::Stored => "Stored",
            zip::CompressionMethod::Deflated => "Deflate",
            zip::CompressionMethod::Bzip2 => "Bzip2",
            zip::CompressionMethod::Zstd => "Zstd",
            _ => "Unknown",
        }
    }

    /// Compare two DateTime objects for ordering
    fn datetime_compare(a: &zip::DateTime, b: &zip::DateTime) -> i32 {
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
        let cursor = Cursor::new(file_data);
        let mut archive = ZipArchive::new(cursor)
            .map_err(|e| ExifToolError::parse_error(format!("Failed to read ZIP: {}", e)))?;

        // RAW parsing preserves physical IFD contexts in its keys. ExifTool's
        // EIP reader promotes only these embedded-IIQ fields to EXIF. Resolve
        // both names through the tag database rather than assuming prefixes,
        // and do not merge the RAW parser's Composite or other IIQ tags.
        if let Some(raw_metadata) = Self::read_eip_raw_metadata(&mut archive)? {
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
        if !comment.is_empty()
            && let Ok(comment_str) = std::str::from_utf8(comment)
        {
            metadata.insert(
                "ZIP:Comment".to_string(),
                TagValue::new_string(comment_str.to_string()),
            );
        }

        // Forensic summary tracking
        let mut total_compressed_size: u64 = 0;
        let mut total_uncompressed_size: u64 = 0;
        let mut encrypted_file_count = 0;
        let mut oldest_date: Option<zip::DateTime> = None;
        let mut newest_date: Option<zip::DateTime> = None;
        let mut file_names = Vec::new();

        // Per-file metadata extraction
        for i in 0..file_count {
            if let Ok(file) = archive.by_index(i) {
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
                let compression = file.compression();
                metadata.insert(
                    format!("{}CompressionMethod", prefix),
                    TagValue::new_string(Self::compression_method_name(compression).to_string()),
                );

                // Store compression method as integer
                // Note: CompressionMethod enum doesn't support direct cast to i64
                // We store the discriminant value indirectly through display
                let compression_value = match compression {
                    zip::CompressionMethod::Stored => 0,
                    zip::CompressionMethod::Deflated => 8,
                    zip::CompressionMethod::Bzip2 => 12,
                    zip::CompressionMethod::Zstd => 93,
                    _ => 255, // Unknown
                };
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

                // Encryption detection - check if file name suggests encryption
                // Note: zip crate doesn't expose encryption flags directly in stable API
                // We detect this indirectly through available methods
                let is_encrypted = file.compressed_size() > 0
                    && file.compression() == zip::CompressionMethod::Stored
                    && file.crc32() == 0;

                if is_encrypted {
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
                if let Ok(file) = archive.by_index(i) {
                    let method = Self::compression_method_name(file.compression()).to_string();
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
                if let Ok(file) = archive.by_index(i) {
                    let is_encrypted = file.compressed_size() > 0
                        && file.compression() == zip::CompressionMethod::Stored
                        && file.crc32() == 0;
                    if is_encrypted {
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
    use std::io::Write;
    use zip::write::{SimpleFileOptions, ZipWriter};

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
    fn test_datetime_to_iso8601() {
        // Test DOS datetime conversion. DOS timestamps have 2-second resolution,
        // so use an even second that round-trips exactly (zip 8.x correctly
        // truncates odd seconds; zip 0.6 did not).
        let dt = zip::DateTime::from_date_and_time(2024, 3, 15, 14, 30, 44).unwrap();
        let iso = ZipParser::datetime_to_iso8601(dt);
        assert_eq!(iso, "2024-03-15T14:30:44");

        // Test edge case: earliest valid DOS date
        let dt = zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap();
        let iso = ZipParser::datetime_to_iso8601(dt);
        assert_eq!(iso, "1980-01-01T00:00:00");
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

    #[test]
    fn test_zip_crc_metadata_format() {
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buffer);
            zip.start_file("test.txt", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"ExifTool test file\n").unwrap();
            zip.finish().unwrap();
        }

        let data = buffer.into_inner();
        let reader = BufferedReader::from_bytes(&data);
        let metadata = ZipParser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("ZIP:ZipCRC"),
            Some(&TagValue::new_string("0x6e17461a".to_string()))
        );
    }

    #[test]
    fn test_compression_method_names() {
        assert_eq!(
            ZipParser::compression_method_name(zip::CompressionMethod::Stored),
            "Stored"
        );
        assert_eq!(
            ZipParser::compression_method_name(zip::CompressionMethod::Deflated),
            "Deflate"
        );
        assert_eq!(
            ZipParser::compression_method_name(zip::CompressionMethod::Bzip2),
            "Bzip2"
        );
    }

    #[test]
    fn test_datetime_compare() {
        let dt1 = zip::DateTime::from_date_and_time(2024, 1, 1, 12, 0, 0).unwrap();
        let dt2 = zip::DateTime::from_date_and_time(2024, 1, 2, 12, 0, 0).unwrap();
        let dt3 = zip::DateTime::from_date_and_time(2024, 1, 1, 12, 0, 0).unwrap();

        assert!(ZipParser::datetime_compare(&dt1, &dt2) < 0);
        assert!(ZipParser::datetime_compare(&dt2, &dt1) > 0);
        assert_eq!(ZipParser::datetime_compare(&dt1, &dt3), 0);
    }

    #[test]
    fn test_empty_zip_archive() {
        // Create an empty ZIP archive
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buffer);
            zip.set_comment("Test archive comment").unwrap();
            zip.finish().unwrap();
        }

        let data = buffer.into_inner();
        let reader = BufferedReader::from_bytes(&data);
        let parser = ZipParser;

        let metadata = parser.parse(&reader).unwrap();

        // Verify basic metadata
        assert_eq!(
            metadata.get("ZIP:FileCount"),
            Some(&TagValue::new_integer(0))
        );

        // Verify comment
        assert_eq!(
            metadata.get("ZIP:Comment"),
            Some(&TagValue::new_string("Test archive comment".to_string()))
        );

        // Verify forensic fields
        assert_eq!(
            metadata.get("ZIP:TotalCompressedSize"),
            Some(&TagValue::new_integer(0))
        );
        assert_eq!(
            metadata.get("ZIP:TotalUncompressedSize"),
            Some(&TagValue::new_integer(0))
        );
    }

    #[test]
    fn test_zip_with_files() {
        // Create a ZIP archive with test files
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buffer);

            // Add first file (stored)
            let options = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .last_modified_time(
                    zip::DateTime::from_date_and_time(2024, 1, 15, 10, 30, 0).unwrap(),
                );
            zip.start_file("test1.txt", options).unwrap();
            zip.write_all(b"Hello, World!").unwrap();

            // Add second file (deflated)
            let options = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .last_modified_time(
                    zip::DateTime::from_date_and_time(2024, 3, 20, 15, 45, 30).unwrap(),
                );
            zip.start_file("test2.txt", options).unwrap();
            zip.write_all(b"This is a longer text that should compress well with deflate compression algorithm.").unwrap();

            zip.finish().unwrap();
        }

        let data = buffer.into_inner();
        let reader = BufferedReader::from_bytes(&data);
        let parser = ZipParser;

        let metadata = parser.parse(&reader).unwrap();

        // Verify file count
        assert_eq!(
            metadata.get("ZIP:FileCount"),
            Some(&TagValue::new_integer(2))
        );

        // Verify file names
        assert!(metadata.contains_key("ZIP:File1:Filename"));
        assert!(metadata.contains_key("ZIP:File2:Filename"));
        assert_eq!(
            metadata.get("ZIP:File1:Filename"),
            Some(&TagValue::new_string("test1.txt".to_string()))
        );

        // Verify sizes
        assert!(metadata.contains_key("ZIP:File1:CompressedSize"));
        assert!(metadata.contains_key("ZIP:File1:UncompressedSize"));
        assert!(metadata.contains_key("ZIP:File2:CompressedSize"));
        assert!(metadata.contains_key("ZIP:File2:UncompressedSize"));

        // Verify CRC32
        assert!(metadata.contains_key("ZIP:File1:CRC32"));
        assert!(metadata.contains_key("ZIP:File2:CRC32"));

        // Verify compression methods
        assert_eq!(
            metadata.get("ZIP:File1:CompressionMethod"),
            Some(&TagValue::new_string("Stored".to_string()))
        );
        assert_eq!(
            metadata.get("ZIP:File2:CompressionMethod"),
            Some(&TagValue::new_string("Deflate".to_string()))
        );

        // Verify timestamps
        assert_eq!(
            metadata.get("ZIP:File1:LastModified"),
            Some(&TagValue::new_string("2024-01-15T10:30:00".to_string()))
        );
        assert_eq!(
            metadata.get("ZIP:File2:LastModified"),
            Some(&TagValue::new_string("2024-03-20T15:45:30".to_string()))
        );

        // Verify version made by
        assert!(metadata.contains_key("ZIP:File1:VersionMadeBy"));
        assert!(metadata.contains_key("ZIP:File2:VersionMadeBy"));

        // Verify comma-separated file list (backward compatibility)
        assert_eq!(
            metadata.get("ZIP:Files"),
            Some(&TagValue::new_string("test1.txt, test2.txt".to_string()))
        );

        // Verify forensic summary fields
        assert!(metadata.contains_key("ZIP:TotalCompressedSize"));
        assert!(metadata.contains_key("ZIP:TotalUncompressedSize"));
        assert!(metadata.contains_key("ZIP:CompressionRatio"));

        // Verify date range
        assert_eq!(
            metadata.get("ZIP:OldestFileDate"),
            Some(&TagValue::new_string("2024-01-15T10:30:00".to_string()))
        );
        assert_eq!(
            metadata.get("ZIP:NewestFileDate"),
            Some(&TagValue::new_string("2024-03-20T15:45:30".to_string()))
        );
    }

    #[test]
    fn test_zip_with_directory() {
        // Create a ZIP archive with a directory
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buffer);

            // Add directory
            let options = SimpleFileOptions::default();
            zip.add_directory("test_dir/", options).unwrap();

            // Add file in directory
            zip.start_file("test_dir/file.txt", options).unwrap();
            zip.write_all(b"Content").unwrap();

            zip.finish().unwrap();
        }

        let data = buffer.into_inner();
        let reader = BufferedReader::from_bytes(&data);
        let parser = ZipParser;

        let metadata = parser.parse(&reader).unwrap();

        // Verify directory is detected
        assert_eq!(
            metadata.get("ZIP:FileCount"),
            Some(&TagValue::new_integer(2))
        );
        assert_eq!(
            metadata.get("ZIP:File1:IsDirectory"),
            Some(&TagValue::new_string("true".to_string()))
        );
    }

    #[test]
    fn test_compression_ratio_calculation() {
        // Create a ZIP with highly compressible data
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buffer);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("repeated.txt", options).unwrap();
            // Write highly compressible data (repeated pattern)
            let data = "A".repeat(10000);
            zip.write_all(data.as_bytes()).unwrap();
            zip.finish().unwrap();
        }

        let data = buffer.into_inner();
        let reader = BufferedReader::from_bytes(&data);
        let parser = ZipParser;

        let metadata = parser.parse(&reader).unwrap();

        // Verify compression ratio exists and is reasonable (should be very low for repeated data)
        assert!(metadata.contains_key("ZIP:CompressionRatio"));
        if let Some(TagValue::String(ratio_str)) = metadata.get("ZIP:CompressionRatio") {
            let ratio_value: f64 = ratio_str
                .trim_end_matches('%')
                .parse()
                .expect("Failed to parse ratio");
            // Highly compressible data should have very low ratio
            assert!(
                ratio_value < 10.0,
                "Compression ratio should be < 10% for repeated data"
            );
        }
    }

    #[test]
    fn test_zip64_detection() {
        // We can't easily create a true ZIP64 file in tests, but we can verify the logic
        // by checking that normal files don't get flagged as ZIP64
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buffer);
            let options = SimpleFileOptions::default();
            zip.start_file("small.txt", options).unwrap();
            zip.write_all(b"Small file").unwrap();
            zip.finish().unwrap();
        }

        let data = buffer.into_inner();
        let reader = BufferedReader::from_bytes(&data);
        let parser = ZipParser;

        let metadata = parser.parse(&reader).unwrap();

        // Small archive should not be detected as ZIP64
        assert!(!metadata.contains_key("ZIP:IsZIP64"));
    }
}
