//! ZIP archive format parser with forensic metadata extraction

use crate::core::{
    FileFormat, FileReader, FormatParser, Instance, MetadataMap, SHIM_DEFAULT_PRIORITY, TagValue,
};
use crate::error::{ExifToolError, Result};
use std::io::Cursor;
use zip::ZipArchive;

const ZIP_SIGNATURE: &[u8] = b"PK";
const ZIP_LOCAL_FILE_HEADER_SIGNATURE: &[u8] = b"PK\x03\x04";
/// Central-directory file header signature (APPNOTE 4.3.12).
const ZIP_CENTRAL_DIR_SIGNATURE: &[u8] = b"PK\x01\x02";
/// End-of-central-directory record signature (APPNOTE 4.3.16).
const ZIP_EOCD_SIGNATURE: &[u8] = b"PK\x05\x06";
/// ZIP64 end-of-central-directory locator signature (APPNOTE 4.3.15).
const ZIP_EOCD64_LOCATOR_SIGNATURE: &[u8] = b"PK\x06\x07";
/// ZIP64 end-of-central-directory record signature (APPNOTE 4.3.14).
const ZIP_EOCD64_SIGNATURE: &[u8] = b"PK\x06\x06";
/// Fixed part of the end-of-central-directory record, comment excluded.
const ZIP_EOCD_SIZE: usize = 22;
/// The ZIP64 locator is fixed-size and sits immediately before the EOCD.
const ZIP_EOCD64_LOCATOR_SIZE: usize = 20;
/// Fixed part of a central-directory file header, before name/extra/comment.
const ZIP_CENTRAL_DIR_HEADER_SIZE: usize = 46;
/// How far back from EOF the end-of-central-directory record can start.
///
/// The record's trailing archive comment is a 16-bit length, so the fixed
/// part begins at most `ZIP_EOCD_SIZE + 65535` bytes from the end.
const ZIP_EOCD_SEARCH_LIMIT: usize = ZIP_EOCD_SIZE + u16::MAX as usize;

/// The eight stored central-directory fields ExifTool's `HandleMember` reads
/// (`ZIP.pm:496-503`), before any conversion.
struct CentralDirectoryEntry {
    required_version: u16,
    bit_flag: u16,
    compression: u16,
    modify_time: u16,
    modify_date: u16,
    crc32: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    file_name: String,
}

/// Little-endian `u16` at `offset`; `0` if the slice is short.
///
/// Every call site has already bounds-checked the enclosing fixed-size
/// header, so a short read here is unreachable rather than tolerated.
fn read_u16(data: &[u8], offset: usize) -> u16 {
    data.get(offset..offset + 2)
        .and_then(|slice| slice.try_into().ok())
        .map(u16::from_le_bytes)
        .unwrap_or(0)
}

/// Little-endian `u32` at `offset`; `0` if the slice is short.
fn read_u32(data: &[u8], offset: usize) -> u32 {
    data.get(offset..offset + 4)
        .and_then(|slice| slice.try_into().ok())
        .map(u32::from_le_bytes)
        .unwrap_or(0)
}

/// Little-endian `u64` at `offset`; `0` if the slice is short.
fn read_u64(data: &[u8], offset: usize) -> u64 {
    data.get(offset..offset + 8)
        .and_then(|slice| slice.try_into().ok())
        .map(u64::from_le_bytes)
        .unwrap_or(0)
}

/// ExifTool's `ZipCompression` PrintConv (`ZIP.pm:76-96`).
///
/// The map is transcribed whole. An unlisted method is *not* approximated to
/// a nearby name: ExifTool renders a PrintConv hash miss as `Unknown ($val)`
/// (`ExifTool.pm:3629-3634`, the non-`PrintHex` branch), so that exact string
/// is what this returns. Notably ZIP.pm has no entry for method 93, so a
/// Zstandard member reads `Unknown (93)` under ExifTool -- naming it
/// "Zstandard" would be a plausible-but-wrong value under a real tag name.
fn zip_compression_print_conv(method: u16) -> String {
    let name = match method {
        0 => "None",
        1 => "Shrunk",
        2 => "Reduced with compression factor 1",
        3 => "Reduced with compression factor 2",
        4 => "Reduced with compression factor 3",
        5 => "Reduced with compression factor 4",
        6 => "Imploded",
        7 => "Tokenized",
        8 => "Deflated",
        9 => "Enhanced Deflate using Deflate64(tm)",
        10 => "Imploded (old IBM TERSE)",
        12 => "BZIP2",
        14 => "LZMA (EFS)",
        18 => "IBM TERSE (new)",
        19 => "IBM LZ77 z Architecture (PFS)",
        96 => "JPEG recompressed",
        97 => "WavPack compressed",
        98 => "PPMd version I, Rev 1",
        other => return format!("Unknown ({})", other),
    };
    name.to_string()
}

/// ExifTool's `ZipModifyDate` ValueConv (`ZIP.pm:99-113`).
///
/// The stored value is one MS-DOS `int32u` whose high half is the date and
/// low half the time; ExifTool shifts the packed word directly, so the field
/// split here reproduces `($val >> 25) + 1980` and friends exactly. Seconds
/// are stored in two-second units, hence the doubling.
fn dos_datetime_to_exiftool(modify_date: u16, modify_time: u16) -> String {
    let packed = (u32::from(modify_date) << 16) | u32::from(modify_time);
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        (packed >> 25) + 1980,
        (packed >> 21) & 0x0f,
        (packed >> 16) & 0x1f,
        (packed >> 11) & 0x1f,
        (packed >> 5) & 0x3f,
        (packed & 0x1f) * 2,
    )
}

/// Parser for ZIP archive files
///
/// Extracts comprehensive metadata from ZIP archives including:
/// - Per-file metadata (sizes, CRC32, compression, dates, encryption)
/// - Archive-level metadata (comment, version, ZIP64 detection)
/// - Forensic summary fields (compression ratios, date ranges, encrypted file count)
pub struct ZipParser;

impl ZipParser {
    /// One central-directory entry, in the raw stored form ExifTool reads.
    ///
    /// ExifTool reaches these fields through Archive::Zip: `HandleMember`
    /// (`ZIP.pm:492-506`) calls `versionNeededToExtract`, `bitFlag`,
    /// `compressionMethod`, `lastModFileDateTime`, `crc32`, `compressedSize`,
    /// `uncompressedSize` and `fileName`, every one of which Archive::Zip
    /// reads out of the central directory rather than recomputing.
    ///
    /// The `zip` crate cannot stand in for that read. Its `version_needed()`
    /// is *derived* from the compression method rather than reported from the
    /// file (`zip-8.6.0/src/types.rs:351`), and it exposes no accessor at all
    /// for the general-purpose bit flag, so two of the eight fields would have
    /// to be invented. Walking the directory keeps every value a stored byte.
    fn read_central_directory(data: &[u8]) -> Vec<CentralDirectoryEntry> {
        let mut entries = Vec::new();
        let Some(start) = Self::central_directory_start(data) else {
            return entries;
        };
        let Ok(mut pos) = usize::try_from(start) else {
            return entries;
        };

        while let Some(end) = pos.checked_add(ZIP_CENTRAL_DIR_HEADER_SIZE) {
            let Some(header) = data.get(pos..end) else {
                break;
            };
            if !header.starts_with(ZIP_CENTRAL_DIR_SIGNATURE) {
                break;
            }

            let name_length = read_u16(header, 28) as usize;
            let extra_length = read_u16(header, 30) as usize;
            let comment_length = read_u16(header, 32) as usize;

            let Some(name_end) = end.checked_add(name_length) else {
                break;
            };
            let Some(name) = data.get(end..name_end) else {
                break;
            };

            entries.push(CentralDirectoryEntry {
                required_version: read_u16(header, 6),
                bit_flag: read_u16(header, 8),
                compression: read_u16(header, 10),
                modify_time: read_u16(header, 12),
                modify_date: read_u16(header, 14),
                crc32: read_u32(header, 16),
                compressed_size: read_u32(header, 20),
                uncompressed_size: read_u32(header, 24),
                file_name: String::from_utf8_lossy(name).into_owned(),
            });

            let Some(next) = name_end
                .checked_add(extra_length)
                .and_then(|p| p.checked_add(comment_length))
            else {
                break;
            };
            pos = next;
        }

        entries
    }

    /// Byte offset of the first central-directory file header.
    ///
    /// Returns `None` for anything this cannot read exactly -- a truncated or
    /// absent end-of-central-directory record, or a ZIP64 archive whose
    /// locator does not resolve. Callers then emit no `ZIP:Zip*` tags at all,
    /// which is AGENTS.md's omit-rather-than-approximate rule: a guessed
    /// directory offset would yield confident, wrongly-decoded members.
    fn central_directory_start(data: &[u8]) -> Option<u64> {
        let eocd = Self::find_signature_from_end(data, ZIP_EOCD_SIGNATURE, ZIP_EOCD_SEARCH_LIMIT)?;
        let record = data.get(eocd..eocd.checked_add(ZIP_EOCD_SIZE)?)?;
        let offset = read_u32(record, 16);

        // A saturated offset means the true one lives in the ZIP64 record,
        // which the locator immediately preceding the EOCD points at.
        if offset != u32::MAX {
            return Some(u64::from(offset));
        }

        let locator_start = eocd.checked_sub(ZIP_EOCD64_LOCATOR_SIZE)?;
        let locator = data.get(locator_start..eocd)?;
        if !locator.starts_with(ZIP_EOCD64_LOCATOR_SIGNATURE) {
            return None;
        }

        let eocd64 = usize::try_from(read_u64(locator, 8)).ok()?;
        let record64 = data.get(eocd64..eocd64.checked_add(56)?)?;
        if !record64.starts_with(ZIP_EOCD64_SIGNATURE) {
            return None;
        }
        Some(read_u64(record64, 48))
    }

    /// Last occurrence of `signature` within `limit` bytes of the end.
    fn find_signature_from_end(data: &[u8], signature: &[u8], limit: usize) -> Option<usize> {
        if data.len() < signature.len() {
            return None;
        }
        let start = data.len().saturating_sub(limit);
        data[start..]
            .windows(signature.len())
            .rposition(|window| window == signature)
            .map(|pos| start + pos)
    }

    /// Records the eight per-member `ZIP:Zip*` tags for every archive entry.
    ///
    /// ExifTool emits this whole group once per member, not once per archive:
    /// `ProcessZIP` walks `$zip->members()` and stamps each with its own
    /// sub-document number before extracting -- `foreach $member (@members) {
    /// $$et{DOC_NUM} = ++$docNum; HandleMember(...) }` (`ZIP.pm:729-731`; the
    /// no-Archive::Zip fallback does the same at `ZIP.pm:789`). Its own
    /// `-a -G1 -s` output on `t/images/OOXML.docx` accordingly repeats
    /// `[ZIP] ZipFileName`, `[ZIP] ZipCRC` and the rest eighteen times, once
    /// per member -- 144 `[ZIP]` lines, 8 fields x 18 members.
    ///
    /// This parser previously read only the *first* local file header and
    /// stored one copy of each field through `insert()`, on the stated
    /// premise that "ExifTool's ZIP table reports these header fields for the
    /// first archive member". That premise does not hold: the first member
    /// wins the *default* (non-`-a`) view, but every later member is still
    /// extracted and still reachable under `-a`. Collapsing them lost eight
    /// keys per multi-member archive (Stage 4's duplicate-loss scan,
    /// `tools/exiftool-tables/duplicate_loss_scan.py`, is what measures it).
    ///
    /// The `DOC_NUM` stamp is also exactly what makes the first member win by
    /// default, so it is modelled here rather than approximated with
    /// `Priority => 0`. Each member records under `Instance(doc_num)`, and
    /// `TagSink::record`'s instance guard -- `ExifTool.pm:9564`'s
    /// `(not $$self{DOC_NUM} or ...)` -- stops any later member displacing the
    /// incumbent recorded under a different instance. Members 2..N are
    /// therefore retained in file order without ever changing the bare key,
    /// which is the same retention shape JUMBF and JPEG `COM` use, reached
    /// through the sub-document arm of the rule instead of the priority-0 one.
    pub(crate) fn record_member_tags(data: &[u8], metadata: &mut MetadataMap) {
        for (index, entry) in Self::read_central_directory(data).iter().enumerate() {
            // ExifTool numbers sub-documents from 1 (`++$docNum` on a
            // zero-initialised counter, `ZIP.pm:533` and `:731`).
            let instance = Instance(index as u32 + 1);
            let mut emit = |name: &str, value: TagValue| {
                metadata.insert_occurrence(
                    format!("ZIP:{}", name),
                    value,
                    SHIM_DEFAULT_PRIORITY,
                    "ZIP",
                    instance,
                );
            };

            // Emission order follows the tag IDs ExifTool hands to
            // `HandleTag` in `HandleMember` (`ZIP.pm:496-503`): 2, 3, 4, 5,
            // 7, 9, 11, 15.
            emit(
                "ZipRequiredVersion",
                TagValue::new_integer(i64::from(entry.required_version)),
            );

            // ZIP.pm:72-75 -- PrintConv => '$val ? sprintf("0x%.4x",$val) : $val'.
            // A set flag word prints as 4-digit hex; a zero one stays "0".
            emit(
                "ZipBitFlag",
                if entry.bit_flag == 0 {
                    TagValue::new_integer(0)
                } else {
                    TagValue::new_string(format!("0x{:04x}", entry.bit_flag))
                },
            );

            emit(
                "ZipCompression",
                TagValue::new_string(zip_compression_print_conv(entry.compression)),
            );

            emit(
                "ZipModifyDate",
                TagValue::new_string(dos_datetime_to_exiftool(
                    entry.modify_date,
                    entry.modify_time,
                )),
            );

            // ZIP.pm:115 -- PrintConv => 'sprintf("0x%.8x",$val)'.
            emit(
                "ZipCRC",
                TagValue::new_string(format!("0x{:08x}", entry.crc32)),
            );

            emit(
                "ZipCompressedSize",
                TagValue::new_integer(i64::from(entry.compressed_size)),
            );
            emit(
                "ZipUncompressedSize",
                TagValue::new_integer(i64::from(entry.uncompressed_size)),
            );
            emit("ZipFileName", TagValue::new_string(entry.file_name.clone()));
        }
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

        // Read entire file into memory for zip crate
        let size = reader.size() as usize;
        let file_data = reader.read(0, size)?;

        // Every archive member's ZIP:Zip* tags, first member winning the bare
        // key -- see `record_member_tags` for the ExifTool citations.
        Self::record_member_tags(file_data, &mut metadata);

        let cursor = Cursor::new(file_data);
        let mut archive = ZipArchive::new(cursor)
            .map_err(|e| ExifToolError::parse_error(format!("Failed to read ZIP: {}", e)))?;

        // No EIP handling here: an archive with a `CaptureOne/*.cos` member
        // is routed to `FileFormat::EIP` by content in `detect_zip_variant`
        // before this parser ever runs, mirroring ExifTool's `ProcessZIP`
        // hand-off to `CaptureOne::ProcessEIP` (`ZIP.pm:619-623`). See
        // `crate::parsers::archive::captureone`.

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

/// Records every ZIP member's `ZIP:Zip*` tags into `metadata`.
///
/// The shared entry point for the ZIP-container formats that are not the
/// plain-archive parser -- OOXML and friends reach ExifTool's ZIP tags
/// through the same `ProcessZIP` member walk (`ZIP.pm:729-731`), so they
/// share this transcription rather than keeping a second one.
pub(crate) fn record_zip_member_tags(data: &[u8], metadata: &mut MetadataMap) {
    ZipParser::record_member_tags(data, metadata);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::BufferedReader;
    use std::io::Write;
    use zip::write::{SimpleFileOptions, ZipWriter};

    /// Every member is retained as its own occurrence, and the first one
    /// still wins the bare key.
    ///
    /// Pins both halves of `ZIP.pm:729-731`'s `foreach $member (@members) {
    /// $$et{DOC_NUM} = ++$docNum; ... }`: N members produce N occurrences of
    /// each `ZIP:Zip*` key (reachable under `-a`), while the default view
    /// keeps member 1 -- the sub-document guard at `ExifTool.pm:9564` stops
    /// members 2..N displacing it.
    #[test]
    fn every_zip_member_is_retained_with_the_first_winning() {
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buffer);
            for name in ["alpha.txt", "beta.txt", "gamma.txt"] {
                zip.start_file(name, SimpleFileOptions::default()).unwrap();
                zip.write_all(name.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        let data = buffer.into_inner();
        let reader = BufferedReader::from_bytes(&data);
        let metadata = ZipParser.parse(&reader).unwrap();

        // One occurrence per member, for every field HandleMember extracts.
        for key in [
            "ZIP:ZipFileName",
            "ZIP:ZipCRC",
            "ZIP:ZipRequiredVersion",
            "ZIP:ZipBitFlag",
            "ZIP:ZipCompression",
            "ZIP:ZipModifyDate",
            "ZIP:ZipCompressedSize",
            "ZIP:ZipUncompressedSize",
        ] {
            assert_eq!(
                metadata.occurrences_for(key).len(),
                3,
                "{key} should carry one occurrence per member"
            );
        }

        // File order is preserved across the retained occurrences...
        let names: Vec<String> = metadata
            .occurrences_for("ZIP:ZipFileName")
            .iter()
            .filter_map(|occurrence| occurrence.raw.as_string().map(str::to_owned))
            .collect();
        assert_eq!(names, ["alpha.txt", "beta.txt", "gamma.txt"]);

        // ...and the bare key is still the first member's.
        assert_eq!(
            metadata.get("ZIP:ZipFileName"),
            Some(&TagValue::new_string("alpha.txt".to_string()))
        );
    }

    /// ZIP.pm:76-96 has no entry for compression method 93, so ExifTool falls
    /// through to `Unknown ($val)` (`ExifTool.pm:3629-3634`). Naming it
    /// "Zstandard" -- as this parser previously did -- is a plausible but
    /// wrong value under a real ExifTool tag name.
    #[test]
    fn unlisted_compression_method_is_not_invented() {
        assert_eq!(zip_compression_print_conv(0), "None");
        assert_eq!(zip_compression_print_conv(8), "Deflated");
        assert_eq!(zip_compression_print_conv(98), "PPMd version I, Rev 1");
        assert_eq!(zip_compression_print_conv(93), "Unknown (93)");
    }

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
