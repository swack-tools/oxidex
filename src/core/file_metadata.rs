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

        // File type extension
        metadata.insert(
            "File:FileTypeExtension".to_string(),
            TagValue::new_string(ext_lower.clone()),
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

/// Formats a SystemTime to ExifTool-compatible date format.
///
/// Format: YYYY:MM:DD HH:MM:SS+HH:MM
///
/// Example: 2025:10:17 16:07:59-05:00
fn format_system_time(time: SystemTime) -> String {
    use std::time::UNIX_EPOCH;

    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = duration.as_secs();

    // Convert to local time with timezone offset
    // This is a simplified version - a full implementation would use chrono or time crate
    let datetime = UNIX_EPOCH + std::time::Duration::from_secs(secs);

    // For now, use a basic conversion
    // In a production system, we'd use chrono or time crate for proper timezone handling
    let _datetime = datetime; // Suppress unused variable warning
    let epoch_secs = secs as i64;

    // Calculate date/time components
    const SECS_PER_DAY: i64 = 86400;
    const DAYS_IN_YEAR: i64 = 365;
    const DAYS_IN_4_YEARS: i64 = DAYS_IN_YEAR * 4 + 1;

    let mut days = epoch_secs / SECS_PER_DAY;
    let day_secs = epoch_secs % SECS_PER_DAY;

    // Calculate year (simplified, starting from 1970)
    let mut year = 1970;
    while days >= DAYS_IN_4_YEARS {
        days -= DAYS_IN_4_YEARS;
        year += 4;
    }
    while days >= DAYS_IN_YEAR {
        let leap = if year % 4 == 0 { 1 } else { 0 };
        let year_days = DAYS_IN_YEAR + leap;
        if days < year_days {
            break;
        }
        days -= year_days;
        year += 1;
    }

    // Calculate month and day (simplified)
    const DAYS_IN_MONTH: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1;
    let leap = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
        1
    } else {
        0
    };

    for (i, &days_in_month) in DAYS_IN_MONTH.iter().enumerate() {
        let month_days = days_in_month + if i == 1 { leap } else { 0 };
        if days < month_days {
            month = i as i64 + 1;
            break;
        }
        days -= month_days;
    }
    let day = days + 1;

    // Calculate hours, minutes, seconds
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;

    // Get timezone offset (simplified - just use system's current offset)
    // In production, we'd properly handle timezone using chrono or time crate
    let tz_offset = get_timezone_offset();

    format!(
        "{}:{:02}:{:02} {:02}:{:02}:{:02}{}",
        year, month, day, hours, minutes, seconds, tz_offset
    )
}

/// Gets the current system timezone offset as a string.
///
/// Returns format: +HH:MM or -HH:MM
///
/// This is a simplified implementation. In production, use chrono or time crate.
fn get_timezone_offset() -> String {
    // Try to get system timezone offset
    // This is a placeholder - proper implementation would use chrono or time crate
    #[cfg(unix)]
    {
        use std::process::Command;
        if let Ok(output) = Command::new("date").arg("+%z").output()
            && let Ok(tz_str) = String::from_utf8(output.stdout)
        {
            let trimmed = tz_str.trim();
            if trimmed.len() >= 5 {
                // Format is +HHMM or -HHMM, convert to +HH:MM or -HH:MM
                let sign = &trimmed[0..1];
                let hours = &trimmed[1..3];
                let mins = &trimmed[3..5];
                return format!("{}{}:{}", sign, hours, mins);
            }
        }
    }

    // Default to +00:00 if we can't determine timezone
    "+00:00".to_string()
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
        "avi" => "AVI",
        "mkv" => "MKV",
        "flv" => "FLV",
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
        "avi" => "video/x-msvideo",
        "mkv" => "video/x-matroska",
        "flv" => "video/x-flv",
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
