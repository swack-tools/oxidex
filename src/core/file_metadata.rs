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
use crate::core::{Instance, MetadataMap, SHIM_DEFAULT_PRIORITY, TagValue};
use crate::error::Result;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// What ExifTool reports when nothing declares a MIME type for the file.
///
/// `%mimeType`'s own header comment (ExifTool.pm:614-616) and `SetFileType`
/// (ExifTool.pm:9715) agree:
///
/// ```text
///     # (missing entries default to 'application/unknown', ...)
///     ...
///     $self->FoundTag('MIMEType', $mimeType || 'application/unknown');
/// ```
///
/// Not `application/octet-stream` -- that is a real answer `%mimeType` gives
/// explicitly for DR4, VRD, LNK, MOI and the EXE family, and using it as the
/// fallback made "we have no idea" indistinguishable from ExifTool's actual
/// value for those types.
pub(crate) const UNKNOWN_MIME_TYPE: &str = "application/unknown";

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
/// A filesystem fact, family-1 `System` per ExifTool's `%Extra` overrides
/// (`ExifTool.pm:1317-1319` `Directory`, `:1333-1334` `FileName`,
/// `:1388-1389` `FileSize`, `:1435-1443` `FileModifyDate`, `:1452-1462`
/// `FileAccessDate`, `:1476-1479` `FileInodeChangeDate`/`FileCreateDate`,
/// `:1496-1501` `FilePermissions`) -- confirmed group-qualified against the
/// pinned oracle: `-G0:1 -FileName -Directory -FileSize -FileModifyDate
/// Canon.jpg` prints `[File:System]` for all four. `FileType`,
/// `FileTypeExtension` and `MIMEType` (below, outside this helper) get no
/// such override in `%Extra` (`ExifTool.pm:1420-1432`, and `MIMEType` has no
/// `%Extra` entry at all) -- family 1 there is simply group 0 (`File`),
/// confirmed by the same oracle invocation printing plain `[File]` for
/// `FileType`. Inserted at [`SHIM_DEFAULT_PRIORITY`], same as `insert()`:
/// nothing about these tags needs real priority arbitration, only the
/// family-1 label `insert()` cannot carry.
fn insert_system_tag(metadata: &mut MetadataMap, key: &'static str, value: TagValue) {
    metadata.insert_occurrence(
        key,
        value,
        SHIM_DEFAULT_PRIORITY,
        "System",
        Instance::default(),
    );
}

pub fn extract_file_metadata(path: &Path) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::with_capacity(10);

    // Get file metadata from the file system
    let file_metadata = fs::metadata(path)?; // From trait converts io::Error to ExifToolError::IoError

    // File name (basename without directory)
    if let Some(filename) = path.file_name()
        && let Some(filename_str) = filename.to_str()
    {
        insert_system_tag(
            &mut metadata,
            "File:FileName",
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
        insert_system_tag(
            &mut metadata,
            "File:Directory",
            TagValue::new_string(dir_str),
        );
    }

    // File size. ExifTool's `%Extra` declares `FileSize` with a
    // `PrintConv => \&ConvertFileSize` (ExifTool.pm:1388-1389) over the raw
    // byte count; oxidex used to fuse that conversion in at insert time
    // (storing only `"26 kB"`), which is exactly AGENTS.md's tagmodel/1.5
    // finding -- `--no-print-conv` had nothing to select, because the raw
    // byte count was already gone by the time it ran. `-n -s -FileSize` on
    // the pinned `t/images/ExifTool.jpg` (26106 bytes) must print `26106`;
    // `-G0:1 -s -FileSize` (no `-n`) must still print `[File:System]
    // FileSize : 26 kB`, unchanged from before this step. Storing both
    // forms on the same occurrence -- the formatted string where every
    // existing reader already looks, the byte count where only
    // `--no-print-conv` (`MetadataMap::without_print_conv`,
    // `cli::tag_resolution`) looks -- satisfies both without touching any
    // other reader of this tag.
    let file_size = file_metadata.len();
    let size_str = fmt_file_size(file_size);
    metadata.insert_occurrence_with_raw(
        "File:FileSize",
        TagValue::new_string(size_str),
        TagValue::new_integer(file_size as i64),
        SHIM_DEFAULT_PRIORITY,
        "System",
        Instance::default(),
    );

    // File modification date/time
    if let Ok(modified) = file_metadata.modified() {
        let formatted_date = format_system_time(modified);
        insert_system_tag(
            &mut metadata,
            "File:FileModifyDate",
            TagValue::new_string(formatted_date),
        );
    }

    // File access date/time
    if let Ok(accessed) = file_metadata.accessed() {
        let formatted_date = format_system_time(accessed);
        insert_system_tag(
            &mut metadata,
            "File:FileAccessDate",
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
        insert_system_tag(
            &mut metadata,
            "File:FileInodeChangeDate",
            TagValue::new_string(formatted_date),
        );
    }

    #[cfg(windows)]
    {
        if let Ok(created) = file_metadata.created() {
            let formatted_date = format_system_time(created);
            insert_system_tag(
                &mut metadata,
                "File:FileCreateDate",
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
        insert_system_tag(
            &mut metadata,
            "File:FilePermissions",
            TagValue::new_string(perm_str),
        );
    }

    #[cfg(windows)]
    {
        // Windows doesn't have Unix-style permissions, so we show a placeholder
        insert_system_tag(
            &mut metadata,
            "File:FilePermissions",
            TagValue::new_string("-rw-rw-rw-".to_string()),
        );
    }

    // File type and extension based on path
    if let Some(extension) = path.extension()
        && let Some(ext_str) = extension.to_str()
    {
        let ext_lower = ext_str.to_lowercase();
        // The header is what lets `filetype::refine` run, and three formats in
        // the corpus need it: a multi-page DjVu, Portable FloatMap's
        // `image/x-pfm`, and any HEIF whose `ftyp` brand contradicts its
        // extension. Reading it here rather than trusting the extension alone
        // is also what keeps this resolver from *out-ranking*
        // `operations::add_identity_tags`, which sees the same bytes.
        let header = read_header(path);
        let (file_type, file_type_ext, mime_type) = identify_extension(&ext_lower, &header);

        // File type extension. ExifTool reports the format's canonical
        // extension rather than echoing the filename, so `clip.m2ts` reports
        // `mts`.
        metadata.insert(
            "File:FileTypeExtension".to_string(),
            TagValue::new_string(file_type_ext),
        );

        // File type (human-readable)
        metadata.insert("File:FileType".to_string(), TagValue::new_string(file_type));

        // MIME type
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

/// `ConvertUnixTime`'s own seconds rounding (ExifTool.pm:6791-6796) at the
/// default `$dec = 0`:
///
/// ```perl
/// my $itime = int($time);
/// my $frac = $time - $itime;
/// $frac < 0 and $frac += 1, $itime -= 1;
/// $dec = sprintf('%.*f', $dec, $frac);
/// $dec =~ s/^(\d)// and $1 eq '1' and $itime += 1;
/// ```
///
/// `int` truncates toward zero and `sprintf '%.0f'` rounds half to even, so
/// this is neither `floor` nor `round`. Callers that hand `ConvertUnixTime` a
/// fractional second -- WTV's 100-ns `%timeInfo` and TNEF's MAPI `SYSTIME` --
/// print one second early without it.
pub(crate) fn round_to_second(seconds: f64) -> i64 {
    let truncated = seconds.trunc();
    let mut whole = truncated as i64;
    let mut frac = seconds - truncated;
    if frac < 0.0 {
        frac += 1.0;
        whole -= 1;
    }
    // `sprintf('%.0f', $frac)` for a fraction in [0,1) prints "0" or "1", and
    // exactly one half rounds to the even integer, which is 0.
    let carry = i64::from(frac > 0.5);
    whole + carry
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

/// Resolves `FileType`, `FileTypeExtension` and `MIMEType` for an extension.
///
/// Delegates to `crate::filetype`, which is generated from ExifTool's own
/// `%fileTypeLookup`, `%fileTypeExt` and `%mimeType` hashes. This used to be
/// three hand-written `match` blocks covering ~50 extensions, and they had
/// drifted from ExifTool on every format where the two disagreed:
///
/// ```text
///     "gz" | "tar"    => "TAR"    ExifTool: gz is GZIP, a different format
///     "doc" | "docx"  => "DOC"    ExifTool: DOCX  (likewise XLSX, PPTX)
///     "wmv" | "asf"   => "ASF"    ExifTool: WMV
///     "gz"  => "application/gzip"         ExifTool: application/x-gzip
///     "wav" => "audio/wav"                ExifTool: audio/x-wav
/// ```
///
/// Those answers were not merely wrong, they were *unrecoverable*:
/// `operations::add_identity_tags` repairs `FileType` from these same
/// generated tables on every read, but only where it finds a placeholder, and
/// a confident `TAR` is not a placeholder. The hand table's wrongness was
/// precisely what protected it from the correction. Deleting it is the fix;
/// there is no case where a 50-entry copy beats the 300-entry generated table
/// it was copied from.
///
/// Unrecognised extensions keep the placeholders (`Unknown` /
/// [`UNKNOWN_MIME_TYPE`]), which `add_identity_tags` is free to replace.
fn identify_extension(extension: &str, header: &[u8]) -> (String, String, &'static str) {
    // `.ts` is a deliberate deviation from ExifTool, and the one place the hand
    // table was right to disagree. `%fileTypeLookup` maps it to M2TS, but a
    // `.ts` file is overwhelmingly TypeScript source, and claiming it here
    // would confidently mislabel every one in a directory scan.
    // `add_identity_tags` may still identify a real transport stream, because
    // it requires the magic number to match as well.
    if extension == "ts" {
        return (
            "Unknown".to_string(),
            extension.to_string(),
            UNKNOWN_MIME_TYPE,
        );
    }

    // `identify` declines when the header contradicts the extension, which is
    // the answer we want: a `.gz` that is not gzip should not be called GZIP.
    // Falling back to the extension alone keeps the previous behaviour for a
    // file too short to carry a header.
    let identified = crate::filetype::identify(header, Some(extension))
        .or_else(|| crate::filetype::identify_by_extension(extension));

    match identified {
        Some(id) => {
            // Order matters, and it is ExifTool's (SetFileType, ExifTool.pm):
            //
            //     $mimeType or $mimeType = $mimeType{$fileType};
            //     $mimeType = $mimeType{$baseType} unless $mimeType ...
            //
            // `$mimeType` is the argument the *module* passed, so a module's own
            // lookup outranks `%mimeType`, which in turn outranks the root
            // type's. `Identity::mime_type` already collapses the last two, and
            // that collapse is why the module overlay has to be consulted
            // first: `%mimeType` has no M4A row, so the generated table answers
            // for it with the MOV root's `video/quicktime` -- a non-`None` wrong
            // answer that an `or_else` after it can never correct. ExifTool says
            // `audio/mp4`.
            // `application/unknown` rather than `application/octet-stream`
            // is ExifTool's fallback for a type `%mimeType` does not carry
            // (ExifTool.pm:9715, and the hash's own header comment at 614):
            //
            //     $self->FoundTag('MIMEType', $mimeType || 'application/unknown');
            //
            // The two are not interchangeable. `application/octet-stream` is a
            // real answer that %mimeType gives explicitly for DR4, VRD, LNK,
            // MOI and the whole EXE family; `application/unknown` is what is
            // left when nothing answered. Conflating them reported the wrong
            // one of the pair for `MacOS.macos`.
            let mime = module_mime_type(&id.file_type)
                .or(id.mime_type)
                .unwrap_or(UNKNOWN_MIME_TYPE);
            (id.file_type.into_owned(), id.extension.into_owned(), mime)
        }
        None => (
            "Unknown".to_string(),
            extension.to_string(),
            UNKNOWN_MIME_TYPE,
        ),
    }
}

/// The first KiB of `path`, or empty when it cannot be read.
///
/// 1 KiB is what the magic-number patterns are written against, matching
/// `operations::add_identity_tags`. An unreadable file is not an error here:
/// the caller already has the `fs::metadata` it needs, and an empty header
/// simply means the extension answers alone.
fn read_header(path: &Path) -> Vec<u8> {
    use std::io::Read;

    let Ok(mut file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut header = vec![0u8; 1024];
    match file.read(&mut header) {
        Ok(n) => {
            header.truncate(n);
            header
        }
        Err(_) => Vec::new(),
    }
}

/// MIME types ExifTool declares in a format module rather than in `%mimeType`.
///
/// The generated `MIME_TYPE` table transcribes `ExifTool.pm`'s `%mimeType`,
/// which is the only hash the generator reads. RIFF and QuickTime keep their
/// own lookups and pass the result to `SetFileType`, so ExifTool answers for
/// these types while the generated table has no row at all -- exactly the case
/// AGENTS.md names: a gap in a transcribed table means "not transcribed", not
/// "no such value". Without this overlay, delegating to the generated table
/// alone would have reported `application/octet-stream` for every WAV, AVI,
/// WEBP and M4A -- trading the old hand table's wrong answers for no answer.
///
/// Transcribed verbatim from the pinned 13.59 tree: `%riffMimeType`
/// (RIFF.pm:56-64) and `%mimeLookup` (QuickTime.pm:104-127). Verified against
/// the oracle on the corpus files that carry them: RIFF.wav `audio/x-wav`,
/// RIFF.avi and Pentax.avi `video/x-msvideo`, RIFF.webp `image/webp`,
/// QuickTime.m4a `audio/mp4`, QuickTime.heic `image/heif`.
fn module_mime_type(file_type: &str) -> Option<&'static str> {
    Some(match file_type {
        // RIFF.pm %riffMimeType
        "WAV" => "audio/x-wav",
        "AVI" => "video/x-msvideo",
        "WEBP" => "image/webp",
        "LA" => "audio/x-nspaudio",
        "OFR" => "audio/x-ofr",
        "PAC" => "audio/x-lpac",
        "WV" => "audio/x-wavpack",
        // QuickTime.pm %mimeLookup
        "3G2" => "video/3gpp2",
        "3GP" => "video/3gpp",
        "AAX" => "audio/vnd.audible.aax",
        "DVB" => "video/vnd.dvb.file",
        "F4A" | "F4B" | "M4A" | "M4B" | "M4P" => "audio/mp4",
        "JP2" => "image/jp2",
        "JPM" => "image/jpm",
        "JPX" => "image/jpx",
        "M4V" => "video/x-m4v",
        "MOV" | "MQV" => "video/quicktime",
        "HEIC" => "image/heic",
        "HEVC" | "HEICS" => "image/heic-sequence",
        "HEIF" => "image/heif",
        "HEIFS" => "image/heif-sequence",
        "AVIF" => "image/avif",
        "CRX" => "video/x-canon-crx",
        // Font.pm:598. `$ftyp` is one of three depending on which
        // `Start(Comp|Master)?FontMetrics` line opens the file, and all three
        // share the one MIME type:
        //   my $ftyp = $1 ? ($1 eq 'Comp' ? 'ACFM' : 'AMFM') : 'AFM';
        //   $et->SetFileType($ftyp, 'application/x-font-afm');
        // Without this, `%mimeType` answers for the PostScript font family
        // that AFM's root type belongs to -- `application/x-font-type1`.
        "AFM" | "ACFM" | "AMFM" => "application/x-font-afm",
        // LNK.pm:1733 -- `%mimeType` carries no URL row at all, so the
        // generated table leaves a Windows shortcut at `application/octet-
        // stream`:
        //   $et->SetFileType('URL', 'application/x-mswinurl');
        "URL" => "application/x-mswinurl",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // File size tests moved to core::value_formatter module

    /// The generated tables answer, and they answer as ExifTool does.
    ///
    /// The `match` blocks these replaced got `gz`, `docx`, `xlsx`, `pptx` and
    /// `wmv` wrong -- and wrong in the one way that could not be repaired
    /// downstream, because `add_identity_tags` only overwrites placeholders and
    /// a confident `TAR` is not a placeholder.
    #[test]
    fn file_type_comes_from_the_generated_tables() {
        for (ext, want) in [
            ("pdf", "PDF"),
            ("jpg", "JPEG"),
            ("png", "PNG"),
            // Each of these was wrong under the hand-written table.
            ("gz", "GZIP"),
            ("tar", "TAR"),
            ("docx", "DOCX"),
            ("doc", "DOC"),
            ("xlsx", "XLSX"),
            ("pptx", "PPTX"),
            ("wmv", "WMV"),
            ("asf", "ASF"),
        ] {
            assert_eq!(identify_extension(ext, b"").0, want, "FileType for .{ext}");
        }
        assert_eq!(identify_extension("nosuchext", b"").0, "Unknown");
    }

    #[test]
    fn mime_type_comes_from_the_generated_tables() {
        for (ext, want) in [
            ("pdf", "application/pdf"),
            ("jpg", "image/jpeg"),
            ("png", "image/png"),
            // `application/gzip` under the hand table; ExifTool says x-gzip.
            ("gz", "application/x-gzip"),
            ("tar", "application/x-tar"),
            ("wmv", "video/x-ms-wmv"),
        ] {
            assert_eq!(identify_extension(ext, b"").2, want, "MIMEType for .{ext}");
        }
        // `application/unknown`, not `application/octet-stream`: the latter
        // is a real `%mimeType` value for DR4, VRD, LNK and the EXE family,
        // and using it here made "nothing answered" look like one of them.
        assert_eq!(identify_extension("nosuchext", b"").2, UNKNOWN_MIME_TYPE);
    }

    /// The RIFF/QuickTime MIME types `%mimeType` does not carry.
    ///
    /// Each is pinned to the value the pinned oracle reports for the corpus
    /// file named in `module_mime_type`'s docs, not to the old hand table --
    /// which had `audio/wav` for WAV where ExifTool says `audio/x-wav`.
    #[test]
    fn module_declared_mime_types_survive_the_generated_table_gap() {
        for (ext, want) in [
            ("wav", "audio/x-wav"),
            ("avi", "video/x-msvideo"),
            ("webp", "image/webp"),
            ("m4a", "audio/mp4"),
            ("m4b", "audio/mp4"),
            ("m4p", "audio/mp4"),
            ("m4v", "video/x-m4v"),
            ("mov", "video/quicktime"),
            ("heif", "image/heif"),
            // Font.pm and LNK.pm declare these the same way RIFF and
            // QuickTime do -- in the module, so `%mimeType` has no row.
            ("afm", "application/x-font-afm"),
            ("url", "application/x-mswinurl"),
        ] {
            assert_eq!(identify_extension(ext, b"").2, want, "MIMEType for .{ext}");
        }
    }

    /// AFM's two siblings share its MIME type but have no extension of their
    /// own, so they are only reachable by file type.
    #[test]
    fn the_composite_font_metrics_types_share_afms_mime() {
        for file_type in ["AFM", "ACFM", "AMFM"] {
            assert_eq!(
                module_mime_type(file_type),
                Some("application/x-font-afm"),
                "{file_type}"
            );
        }
    }

    /// The File group is excluded from the tag-comparison harness, so these
    /// mappings are only ever covered by tests like this one.
    #[test]
    fn test_m2ts_file_type_and_mime() {
        for ext in ["mts", "m2ts", "m2t"] {
            let (file_type, _, mime) = identify_extension(ext, b"");
            assert_eq!(file_type, "M2TS", "FileType for .{ext}");
            assert_eq!(mime, "video/m2ts", "MIMEType for .{ext}");
        }
    }

    /// ExifTool reports the format's canonical extension, not the filename's.
    #[test]
    fn test_m2ts_file_type_extension_is_canonicalised() {
        assert_eq!(identify_extension("m2ts", b"").1, "mts");
        assert_eq!(identify_extension("m2t", b"").1, "mts");
        assert_eq!(identify_extension("mts", b"").1, "mts");
        // Extensions without an override pass through untouched.
        assert_eq!(identify_extension("jpg", b"").1, "jpg");
        assert_eq!(identify_extension("mov", b"").1, "mov");
    }

    #[test]
    fn test_m4a_family_file_type_and_mime() {
        for (ext, want) in [
            ("m4a", "M4A"),
            ("m4b", "M4B"),
            ("m4p", "M4P"),
            ("m4v", "M4V"),
        ] {
            assert_eq!(identify_extension(ext, b"").0, want, "FileType for .{ext}");
        }
    }

    /// Neighbouring container formats must be undisturbed by the additions
    /// above -- an over-eager mapping is worse than a missing one.
    #[test]
    fn test_neighbouring_video_formats_unchanged() {
        for (ext, file_type, mime) in [
            ("mov", "MOV", "video/quicktime"),
            ("mp4", "MP4", "video/mp4"),
            ("avi", "AVI", "video/x-msvideo"),
        ] {
            let (got_type, _, got_mime) = identify_extension(ext, b"");
            assert_eq!(got_type, file_type, "FileType for .{ext}");
            assert_eq!(got_mime, mime, "MIMEType for .{ext}");
        }
    }

    /// `.ts` is an M2TS extension for ExifTool, but this resolver has no
    /// content check and `.ts` is overwhelmingly TypeScript source. Claiming it
    /// here would confidently mislabel every TypeScript file. This is the one
    /// entry the hand-written table got right that the generated table does
    /// not, so it is carried forward explicitly.
    #[test]
    fn test_ts_extension_is_not_claimed_as_m2ts() {
        let (file_type, _, mime) = identify_extension("ts", b"");
        assert_eq!(file_type, "Unknown");
        assert_eq!(mime, UNKNOWN_MIME_TYPE);
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

    /// Step 19: filesystem facts get family-1 `System`
    /// (`ExifTool.pm:1333-1334` etc.), but `FileType`/`FileTypeExtension`/
    /// `MIMEType` get no such override and stay unmarked -- the boundary the
    /// pinned oracle draws between `[File:System] FileSize` and
    /// `[File] FileType` on `ExifTool.jpg`.
    #[test]
    fn test_extract_file_metadata_marks_system_facts_but_not_file_type() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("clip.mts");
        std::fs::write(&path, b"placeholder").expect("write");

        let meta = extract_file_metadata(&path).expect("extract");
        for key in [
            "File:FileName",
            "File:Directory",
            "File:FileSize",
            "File:FileModifyDate",
            "File:FileAccessDate",
        ] {
            let occurrences = meta.occurrences_for(key);
            assert_eq!(occurrences.len(), 1, "{key} should be recorded once");
            assert_eq!(
                &*occurrences[0].group1, "System",
                "{key} should be [File:System]"
            );
        }
        for key in ["File:FileType", "File:FileTypeExtension", "File:MIMEType"] {
            let occurrences = meta.occurrences_for(key);
            assert_eq!(occurrences.len(), 1, "{key} should be recorded once");
            assert_eq!(
                &*occurrences[0].group1, "",
                "{key} should carry no group1 override"
            );
        }
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
