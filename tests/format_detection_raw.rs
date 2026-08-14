//! Tests for camera raw format extension detection
//!
//! This test suite verifies that all major camera raw file extensions
//! are properly recognized as identifiable formats by the batch processor.
//!
//! `is_supported_file` used to be backed by a hand-maintained
//! `SUPPORTED_EXTENSIONS` allow-list. That was the defect: the list quietly
//! fell behind extensions OxiDex has real parsers for (MP3, ZIP, DOCX, TXT,
//! HTML, EPUB and more all dispatch to working parsers in
//! `crate::core::format_dispatch`), so `oxidex -r` silently skipped every
//! file with one of those extensions -- no error, no warning, no count. It
//! is now backed by `crate::filetype`'s extension table, generated from
//! ExifTool's own `%fileTypeLookup`, so it cannot drift the same way again.
//! These tests assert against that real table's contents, not against a
//! list an author has to remember to extend.

use std::path::Path;

/// Test that all camera raw extensions ExifTool actually recognizes are
/// identified by the batch processor.
///
/// Five entries from the original hand-maintained list -- `ari`, `mdc`,
/// `sti`, `cam`, `rev` -- are deliberately absent here: none of them appear
/// anywhere in the pinned ExifTool's `%fileTypeLookup`
/// (`lib/Image/ExifTool.pm:231`), so they were never real extensions to
/// begin with, and `crate::filetype`'s table (generated from that same
/// hash) correctly does not recognize them either. Asserting they pass
/// would just reintroduce the "make up an answer" failure mode this fix
/// exists to remove.
#[test]
fn test_raw_extensions_supported() {
    let raw_extensions = vec![
        // Canon
        "cr2", "cr3", "crw", // Nikon
        "nef", "nrw", // Sony
        "arw", "arq", "sr2", "srf", "srw", // Fujifilm
        "raf", // Olympus
        "orf", "ori", // Pentax
        "pef", // Panasonic
        "rw2", "rwl", // Hasselblad
        "3fr", "fff", // Phase One
        "iiq", // Mamiya
        "mef", // Kodak
        "mos", // Minolta
        "dcr", "kdc", // Epson
        "mrw", // Sigma
        "erf", // GoPro
        "x3f", // Adobe DNG
        "gpr", "dng", // HEIF
        "hif", // Sinar
        "lri", // Generic
        "raw",
    ];

    for ext in raw_extensions {
        let path_str = format!("test.{}", ext);
        let path = Path::new(&path_str);

        assert!(
            oxidex::cli::batch_processor::is_supported_file(path),
            "Extension '{}' not recognized by the batch processor",
            ext
        );
    }
}

/// Test that existing (non-raw) formats are still supported
///
/// `jfif` is deliberately absent: it was in the old hand-maintained list,
/// but `JFIF` never appears as a key in the pinned ExifTool's own
/// `%fileTypeLookup` (`lib/Image/ExifTool.pm:231`) -- it names the JPEG
/// APP0 segment ExifTool's `JFIF` tag table reads
/// (`lib/Image/ExifTool.pm:2197`), not a registered file extension.
/// `exiftool -r` (no `-ext`) does not process a bare `.jfif` file by
/// default either, so the old list's inclusion of it was itself an
/// unverified guess, and the canonical, generated table correctly declines
/// it too. A `.jfif` file passed to OxiDex directly is still read
/// correctly regardless -- single-file mode identifies by magic bytes, not
/// extension -- only the directory-walk fast pre-filter this test exercises
/// skips it, matching `GetFileType`'s own extension-only check.
#[test]
fn test_existing_formats_still_supported() {
    let existing_formats = vec![
        "jpg", "jpeg", "jpe", // JPEG
        "tif", "tiff", // TIFF
        "png",  // PNG
        "mp4", "m4v", "m4a", "m4b", "mov", // Video
        "pdf", // PDF
    ];

    for ext in existing_formats {
        let path_str = format!("test.{}", ext);
        let path = Path::new(&path_str);

        assert!(
            oxidex::cli::batch_processor::is_supported_file(path),
            "Extension '{}' should still be supported",
            ext
        );
    }
}

/// Formats the old hand-maintained `SUPPORTED_EXTENSIONS` list dropped even
/// though OxiDex has a real, dispatched parser for each of them
/// (`crate::core::format_dispatch::dispatch_format_parser`). These are
/// exactly the files `oxidex -r` used to skip in total silence; they must
/// be recognized now.
#[test]
fn test_previously_dropped_but_parseable_extensions_now_supported() {
    let dropped = vec![
        "mp3", "zip", "docx", "xlsx", "pptx", "txt", "html", "epub", "flac", "wav", "gif", "bmp",
        "webp", "ico",
    ];

    for ext in dropped {
        let path_str = format!("test.{}", ext);
        let path = Path::new(&path_str);

        assert!(
            oxidex::cli::batch_processor::is_supported_file(path),
            "Extension '{}' has a real parser but was not recognized -- \
             this is the exact defect the extension allow-list caused",
            ext
        );
    }
}

/// Test that extensions absent from ExifTool's own file type table are
/// properly declined -- and, in `batch_processor::collect_files`, counted
/// rather than silently dropped (see
/// `tests/integration/recursive_extension_coverage.rs`).
#[test]
fn test_unrecognized_extensions_rejected() {
    let unrecognized = vec!["qxzzq", "wibblezz", "notarealext", "zzznope"];

    for ext in unrecognized {
        let path_str = format!("test.{}", ext);
        let path = Path::new(&path_str);

        assert!(
            !oxidex::cli::batch_processor::is_supported_file(path),
            "Extension '{}' should NOT be recognized",
            ext
        );
    }
}

/// Test that extension matching is case-insensitive
#[test]
fn test_case_insensitive_matching() {
    let test_cases = vec![
        ("TEST.CR2", true),
        ("test.Cr2", true),
        ("test.NEF", true),
        ("test.Nef", true),
        ("test.JPG", true),
        ("test.TXT", true),
        ("test.QXZZQ", false),
    ];

    for (filename, should_support) in test_cases {
        let path = Path::new(filename);
        assert_eq!(
            oxidex::cli::batch_processor::is_supported_file(path),
            should_support,
            "File '{}' support status should be {}",
            filename,
            should_support
        );
    }
}
