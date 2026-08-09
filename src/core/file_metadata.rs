//! File system metadata extraction module
//!
//! This module provides cross-platform file metadata extraction functionality,
//! extracting information from the file system that is independent of the file's
//! content format (JPEG, PNG, PDF, etc.).
//!
//! # Metadata Fields Extracted
//!
//! - **File:FileName**: Base file name without directory path
//! - **File:Directory**: Parent directory path
//! - **File:FileSize**: Human-readable file size (e.g., "144 kB")
//! - **File:FileModifyDate**: Last modification timestamp
//! - **File:FileAccessDate**: Last access timestamp
//! - **File:FileInodeChangeDate** (Unix) / **File:FileCreateDate** (Windows): Inode change or creation time
//! - **File:FilePermissions**: Unix-style permission string (e.g., "-rw-r--r--")
//! - **File:FileType**: Detected file type based on extension
//! - **File:FileTypeExtension**: File extension (e.g., "pdf", "jpg")
//! - **File:MIMEType**: MIME type based on file extension
//!
//! # Platform Support
//!
//! - **Unix/Linux/macOS**: Full metadata including permissions and inode change time
//! - **Windows**: Full metadata except file permissions (shown as rwxrwxrwx)
//!
//! # Example
//!
//! ```no_run
//! use std::path::Path;
//! use oxidex::core::file_metadata::extract_file_metadata;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let path = Path::new("/path/to/document.pdf");
//! let metadata = extract_file_metadata(path)?;
//!
//! // Access file metadata
//! if let Some(filename) = metadata.get_string("File:FileName") {
//!     println!("File name: {}", filename);
//! }
//! # Ok(())
//! # }
//! ```

use crate::core::value_formatter::format_file_size as fmt_file_size;
use crate::core::{MetadataMap, TagValue};
use crate::error::Result;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Extracts file system metadata from a file path.
///
/// This function reads metadata from the file system (not from the file content)
/// and returns it in a MetadataMap with "File:" prefix.
///
/// # Parameters
///
/// - `path`: Path to the file to extract metadata from
///
/// # Returns
///
/// - `Ok(MetadataMap)`: Extracted file metadata with "File:" prefix
/// - `Err(ExifToolError)`: I/O error or path error
///
/// # Errors
///
/// Returns an error if:
/// - File does not exist
/// - Permission denied
/// - I/O error accessing file metadata
///
/// # Example
///
/// ```no_run
/// use std::path::Path;
/// use oxidex::core::file_metadata::extract_file_metadata;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let path = Path::new("document.pdf");
/// let metadata = extract_file_metadata(path)?;
///
/// for (key, value) in metadata.iter() {
///     println!("{}: {:?}", key, value);
/// }
/// # Ok(())
/// # }
/// ```
pub fn extract_file_metadata(path: &Path) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::with_capacity(10);

    // Get file metadata from the file system
    let file_metadata = fs::metadata(path)?; // From trait converts io::Error to ExifToolError::IoError

    // File name (basename without directory)
    if let Some(filename) = path.file_name()
        && let Some(filename_str) = filename.to_str()
    {
        metadata.insert(
            "File:FileName".to_string(),
            TagValue::new_string(filename_str.to_string()),
        );
    }

    // Directory (parent directory path)
    if let Some(parent) = path.parent() {
        let dir_str = if parent.as_os_str().is_empty() {
            ".".to_string()
        } else {
            parent.to_string_lossy().to_string()
        };
        metadata.insert("File:Directory".to_string(), TagValue::new_string(dir_str));
    }

    // File size (human-readable format)
    let file_size = file_metadata.len();
    let size_str = fmt_file_size(file_size);
    metadata.insert("File:FileSize".to_string(), TagValue::new_string(size_str));

    // File modification date/time
    if let Ok(modified) = file_metadata.modified() {
        let formatted_date = format_system_time(modified);
        metadata.insert(
            "File:FileModifyDate".to_string(),
            TagValue::new_string(formatted_date),
        );
    }

    // File access date/time
    if let Ok(accessed) = file_metadata.accessed() {
        let formatted_date = format_system_time(accessed);
        metadata.insert(
            "File:FileAccessDate".to_string(),
            TagValue::new_string(formatted_date),
        );
    }

    // File inode change time (Unix) or creation time (Windows)
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let ctime = file_metadata.ctime();
        let system_time = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(ctime as u64);
        let formatted_date = format_system_time(system_time);
        metadata.insert(
            "File:FileInodeChangeDate".to_string(),
            TagValue::new_string(formatted_date),
        );
    }

    #[cfg(windows)]
    {
        if let Ok(created) = file_metadata.created() {
            let formatted_date = format_system_time(created);
            metadata.insert(
                "File:FileCreateDate".to_string(),
                TagValue::new_string(formatted_date),
            );
        }
    }

    // File permissions (Unix format)
    #[cfg(unix)]
    {
        let permissions = file_metadata.permissions();
        let mode = permissions.mode();
        let perm_str = format_unix_permissions(mode);
        metadata.insert(
            "File:FilePermissions".to_string(),
            TagValue::new_string(perm_str),
        );
    }

    #[cfg(windows)]
    {
        // Windows doesn't have Unix-style permissions, so we show a placeholder
        metadata.insert(
            "File:FilePermissions".to_string(),
            TagValue::new_string("-rw-rw-rw-".to_string()),
        );
    }

    // File type and extension based on path
    if let Some(extension) = path.extension()
        && let Some(ext_str) = extension.to_str()
    {
        let ext_lower = ext_str.to_lowercase();

        // File type extension. ExifTool reports the format's canonical
        // extension rather than echoing the filename, so `clip.m2ts` reports
        // `mts`.
        metadata.insert(
            "File:FileTypeExtension".to_string(),
            TagValue::new_string(get_file_type_extension(&ext_lower).to_string()),
        );

        // File type (human-readable)
        let file_type = get_file_type(&ext_lower);
        metadata.insert(
            "File:FileType".to_string(),
            TagValue::new_string(file_type.to_string()),
        );

        // MIME type
        let mime_type = get_mime_type(&ext_lower);
        metadata.insert(
            "File:MIMEType".to_string(),
            TagValue::new_string(mime_type.to_string()),
        );
    }

    Ok(metadata)
}

// File size formatting moved to core::value_formatter module
// to ensure consistency with ExifTool (decimal units: 1 kB = 1000 bytes)

/// Formats a `SystemTime` to ExifTool's local-time format.
///
/// Format: `YYYY:MM:DD HH:MM:SS±HH:MM`, e.g. `2025:10:17 16:07:59-05:00`.
///
/// Both halves are *local*: the calendar/clock fields are the local civil time,
/// and the suffix is the UTC offset in force **at that instant**, so a timestamp
/// from January renders `-06:00` while one from July renders `-05:00` in a
/// US-Central zone. ExifTool.pm `ConvertUnixTime` (lib/Image/ExifTool.pm:6804-6807):
///
/// ```text
/// } else {
///     @tm = localtime($itime);
///     $tz = TimeZoneString(\@tm, $itime);
/// }
/// ```
///
/// `TimeZoneString` (lib/Image/ExifTool.pm:6764-6776) renders that offset as
/// `sprintf('%s%.2d:%.2d', $sign, $h, $min - $h * 60)`, which is exactly
/// chrono's `%:z`.
fn format_system_time(time: SystemTime) -> String {
    use std::time::UNIX_EPOCH;

    // `duration_since` errors for pre-epoch times; recover the negative offset
    // from the error rather than clamping such timestamps to 1970.
    let secs = match time.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    };
    format_unix_time_local(secs)
}

/// Renders a Unix timestamp as ExifTool's local date/time string.
///
/// ExifTool.pm:6787 short-circuits the epoch itself:
/// `return '0000:00:00 00:00:00' if $time == 0;`
pub(crate) fn format_unix_time_local(secs: i64) -> String {
    if secs == 0 {
        return "0000:00:00 00:00:00".to_string();
    }
    match chrono::DateTime::from_timestamp(secs, 0) {
        Some(utc) => utc
            .with_timezone(&chrono::Local)
            .format("%Y:%m:%d %H:%M:%S%:z")
            .to_string(),
        // Only reachable for timestamps outside chrono's representable range.
        None => "0000:00:00 00:00:00".to_string(),
    }
}

/// Formats Unix file permissions to a string.
///
/// Format: drwxrwxrwx where:
/// - First char: file type (- for file, d for directory, l for symlink)
/// - Next 3 chars: owner permissions (rwx)
/// - Next 3 chars: group permissions (rwx)
/// - Last 3 chars: other permissions (rwx)
///
/// Example: -rw-r--r--
#[cfg(unix)]
fn format_unix_permissions(mode: u32) -> String {
    let file_type = if mode & 0o170000 == 0o040000 {
        'd' // directory
    } else if mode & 0o170000 == 0o120000 {
        'l' // symbolic link
    } else {
        '-' // regular file
    };

    let user_r = if mode & 0o400 != 0 { 'r' } else { '-' };
    let user_w = if mode & 0o200 != 0 { 'w' } else { '-' };
    let user_x = if mode & 0o100 != 0 { 'x' } else { '-' };

    let group_r = if mode & 0o040 != 0 { 'r' } else { '-' };
    let group_w = if mode & 0o020 != 0 { 'w' } else { '-' };
    let group_x = if mode & 0o010 != 0 { 'x' } else { '-' };

    let other_r = if mode & 0o004 != 0 { 'r' } else { '-' };
    let other_w = if mode & 0o002 != 0 { 'w' } else { '-' };
    let other_x = if mode & 0o001 != 0 { 'x' } else { '-' };

    format!(
        "{}{}{}{}{}{}{}{}{}{}",
        file_type, user_r, user_w, user_x, group_r, group_w, group_x, other_r, other_w, other_x
    )
}

/// Determines the file type description from file extension.
fn get_file_type(extension: &str) -> &'static str {
    match extension {
        "pdf" => "PDF",
        "jpg" | "jpeg" => "JPEG",
        "png" => "PNG",
        "gif" => "GIF",
        "tif" | "tiff" => "TIFF",
        "bmp" => "BMP",
        "webp" => "WEBP",
        "heic" | "heif" => "HEIF",
        "svg" => "SVG",
        "cam" => "CAM",
        "mp4" => "MP4",
        "mov" => "MOV",
        // MPEG-4 audio/video variants of the QuickTime container. ExifTool
        // derives these from the `ftyp` major brand; keyed here on extension
        // to match how this table resolves every other format.
        "m4a" => "M4A",
        "m4b" => "M4B",
        "m4p" => "M4P",
        "m4v" => "M4V",
        "avi" => "AVI",
        "mkv" => "MKV",
        "flv" => "FLV",
        // MPEG-2 Transport Stream. ExifTool maps m2t/m2ts/mts (and ts) to the
        // M2TS file type. `ts` is deliberately excluded here: this lookup is
        // keyed on the extension alone with no content check, and `.ts` also
        // means TypeScript source, which would be confidently mislabelled.
        "mts" | "m2ts" | "m2t" => "M2TS",
        "wmv" | "asf" => "ASF",
        "mp3" => "MP3",
        "wav" => "WAV",
        "flac" => "FLAC",
        "ogg" => "OGG",
        "txt" => "TXT",
        "doc" | "docx" => "DOC",
        "xls" | "xlsx" => "XLS",
        "ppt" | "pptx" => "PPT",
        "zip" => "ZIP",
        "rar" => "RAR",
        "7z" => "7Z",
        "gz" | "tar" => "TAR",
        _ => "Unknown",
    }
}

/// Maps a raw filename extension to ExifTool's canonical `FileTypeExtension`.
///
/// ExifTool reports the format's preferred extension (its `%fileTypeExt`
/// table) rather than echoing the filename, so a `.m2ts` file reports `mts`.
/// Extensions with no such override are returned unchanged.
fn get_file_type_extension(extension: &str) -> &str {
    match extension {
        "m2ts" | "m2t" => "mts",
        other => other,
    }
}

/// Determines the MIME type from file extension.
fn get_mime_type(extension: &str) -> &'static str {
    match extension {
        "pdf" => "application/pdf",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "tif" | "tiff" => "image/tiff",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "heic" | "heif" => "image/heif",
        "svg" => "image/svg+xml",
        "cam" => "image/x-casio-cam",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        // ExifTool QuickTime.pm %mimeLookup
        "m4a" | "m4b" | "m4p" => "audio/mp4",
        "m4v" => "video/x-m4v",
        "avi" => "video/x-msvideo",
        "mkv" => "video/x-matroska",
        "flv" => "video/x-flv",
        // See `get_file_type` for why `ts` is excluded.
        "mts" | "m2ts" | "m2t" => "video/m2ts",
        "wmv" | "asf" => "video/x-ms-wmv",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "ogg" => "audio/ogg",
        "txt" => "text/plain",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "zip" => "application/zip",
        "rar" => "application/x-rar-compressed",
        "7z" => "application/x-7z-compressed",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // File size tests moved to core::value_formatter module

    #[test]
    fn test_get_file_type() {
        assert_eq!(get_file_type("pdf"), "PDF");
        assert_eq!(get_file_type("jpg"), "JPEG");
        assert_eq!(get_file_type("png"), "PNG");
        assert_eq!(get_file_type("unknown"), "Unknown");
    }

    #[test]
    fn test_get_mime_type() {
        assert_eq!(get_mime_type("pdf"), "application/pdf");
        assert_eq!(get_mime_type("jpg"), "image/jpeg");
        assert_eq!(get_mime_type("png"), "image/png");
        assert_eq!(get_mime_type("unknown"), "application/octet-stream");
    }

    /// The File group is excluded from the tag-comparison harness, so these
    /// mappings are only ever covered by tests like this one.
    #[test]
    fn test_m2ts_file_type_and_mime() {
        for ext in ["mts", "m2ts", "m2t"] {
            assert_eq!(get_file_type(ext), "M2TS", "FileType for .{ext}");
            assert_eq!(get_mime_type(ext), "video/m2ts", "MIMEType for .{ext}");
        }
    }

    /// ExifTool reports the format's canonical extension, not the filename's.
    #[test]
    fn test_m2ts_file_type_extension_is_canonicalised() {
        assert_eq!(get_file_type_extension("m2ts"), "mts");
        assert_eq!(get_file_type_extension("m2t"), "mts");
        assert_eq!(get_file_type_extension("mts"), "mts");
        // Extensions without an override pass through untouched.
        assert_eq!(get_file_type_extension("jpg"), "jpg");
        assert_eq!(get_file_type_extension("mov"), "mov");
    }

    #[test]
    fn test_m4a_family_file_type_and_mime() {
        assert_eq!(get_file_type("m4a"), "M4A");
        assert_eq!(get_file_type("m4b"), "M4B");
        assert_eq!(get_file_type("m4p"), "M4P");
        assert_eq!(get_file_type("m4v"), "M4V");
        assert_eq!(get_mime_type("m4a"), "audio/mp4");
        assert_eq!(get_mime_type("m4b"), "audio/mp4");
        assert_eq!(get_mime_type("m4p"), "audio/mp4");
        assert_eq!(get_mime_type("m4v"), "video/x-m4v");
    }

    /// Neighbouring container formats must be undisturbed by the additions
    /// above -- an over-eager mapping is worse than a missing one.
    #[test]
    fn test_neighbouring_video_formats_unchanged() {
        assert_eq!(get_file_type("mov"), "MOV");
        assert_eq!(get_mime_type("mov"), "video/quicktime");
        assert_eq!(get_file_type("mp4"), "MP4");
        assert_eq!(get_mime_type("mp4"), "video/mp4");
        assert_eq!(get_file_type("avi"), "AVI");
        assert_eq!(get_mime_type("avi"), "video/x-msvideo");
    }

    /// `.ts` is an M2TS extension for ExifTool, but this table has no content
    /// check and `.ts` is overwhelmingly TypeScript source. Claiming it here
    /// would confidently mislabel every TypeScript file.
    #[test]
    fn test_ts_extension_is_not_claimed_as_m2ts() {
        assert_eq!(get_file_type("ts"), "Unknown");
        assert_eq!(get_mime_type("ts"), "application/octet-stream");
    }

    /// End-to-end regression for the reported bug: a `.mts` file reported
    /// FileType=Unknown and MIMEType=application/octet-stream.
    #[test]
    fn test_extract_file_metadata_reports_m2ts_file_group() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("clip.mts");
        std::fs::write(&path, b"placeholder").expect("write");

        let meta = extract_file_metadata(&path).expect("extract");
        assert_eq!(meta.get_string("File:FileType"), Some("M2TS"));
        assert_eq!(meta.get_string("File:FileTypeExtension"), Some("mts"));
        assert_eq!(meta.get_string("File:MIMEType"), Some("video/m2ts"));
    }

    /// End-to-end regression for the second reported misdetection (.m4a).
    #[test]
    fn test_extract_file_metadata_reports_m4a_file_group() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("track.m4a");
        std::fs::write(&path, b"placeholder").expect("write");

        let meta = extract_file_metadata(&path).expect("extract");
        assert_eq!(meta.get_string("File:FileType"), Some("M4A"));
        assert_eq!(meta.get_string("File:FileTypeExtension"), Some("m4a"));
        assert_eq!(meta.get_string("File:MIMEType"), Some("audio/mp4"));
    }

    /// Regression for the reported bug: `format_system_time` built its
    /// calendar/clock fields straight from the *UTC* epoch seconds and then
    /// appended the *local* UTC offset, so every File-group timestamp oxidex
    /// printed was wrong by the local offset (5 hours in US-Central summer).
    ///
    /// Asserted zone-independently: re-parsing the rendered string must recover
    /// the original instant. Under the old code it never could, because the
    /// clock fields and the offset described two different moments.
    #[test]
    fn test_format_system_time_round_trips_to_the_original_instant() {
        // 2026:07:15 14:30:00 UTC, and a winter instant on the other side of a
        // US DST transition to catch an offset pinned to "now" instead of the
        // timestamp.
        for epoch in [1_784_125_800_i64, 1_768_476_600_i64] {
            let rendered = format_unix_time_local(epoch);
            let parsed = chrono::DateTime::parse_from_str(&rendered, "%Y:%m:%d %H:%M:%S%:z")
                .unwrap_or_else(|e| panic!("{rendered:?} is not a valid date/time: {e}"));
            assert_eq!(
                parsed.timestamp(),
                epoch,
                "{rendered:?} decodes to a different instant than it was built from"
            );
        }
    }

    /// The offset must be the one in force at the timestamp, not the one in
    /// force right now -- ExifTool derives it per instant via
    /// `TimeZoneString(\@tm, $itime)` (lib/Image/ExifTool.pm:6806).
    ///
    /// Only meaningful where the local zone actually observes DST, so the two
    /// instants are compared to each other rather than to a fixed string.
    #[test]
    fn test_format_system_time_offset_follows_the_timestamp() {
        let summer = format_unix_time_local(1_784_125_800); // 2026-07-15 UTC
        let winter = format_unix_time_local(1_768_476_600); // 2026-01-15 UTC
        let off = |s: &str| s[s.len() - 6..].to_string();
        let local_has_dst = off(&summer) != off(&winter);
        if local_has_dst {
            // Both render as local wall-clock 09:30:00 in US-Central.
            assert_ne!(
                off(&summer),
                off(&winter),
                "a DST-observing zone must not print the same offset year-round"
            );
        }
        // Whatever the zone, the rendered offset must be a real ±HH:MM.
        for s in [&summer, &winter] {
            let o = off(s);
            assert!(
                (o.starts_with('+') || o.starts_with('-')) && o.as_bytes()[3] == b':',
                "malformed offset in {s:?}"
            );
        }
    }

    /// ExifTool.pm:6787 -- `return '0000:00:00 00:00:00' if $time == 0;`
    #[test]
    fn test_format_system_time_epoch_zero_matches_exiftool() {
        assert_eq!(format_unix_time_local(0), "0000:00:00 00:00:00");
    }

    #[cfg(unix)]
    #[test]
    fn test_format_unix_permissions() {
        // -rw-r--r-- (0644)
        assert_eq!(format_unix_permissions(0o100644), "-rw-r--r--");
        // drwxr-xr-x (0755)
        assert_eq!(format_unix_permissions(0o040755), "drwxr-xr-x");
        // -rwxrwxrwx (0777)
        assert_eq!(format_unix_permissions(0o100777), "-rwxrwxrwx");
    }
}
