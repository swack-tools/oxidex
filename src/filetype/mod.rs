//! File-type identification driven by ExifTool's own magic-number table.
//!
//! OxiDex has its own signature detector for the formats it can parse. This
//! module is narrower and complementary: it answers "what *is* this file?" for
//! everything ExifTool recognises, including the 43 formats in the comparison
//! corpus that OxiDex produced no output for at all.
//!
//! Those files were not scoring badly, they were scoring zero -- no
//! `FileType`, no `FileTypeExtension`, no `MIMEType`. Identifying a file is
//! cheap and independent of being able to parse it, so this runs as a fallback
//! and fills in the three identity tags without claiming to understand the
//! contents.
//!
//! [`tables`] is generated from `%magicNumber`, `%fileTypeLookup` and
//! `%mimeType`, so it cannot drift from ExifTool.

pub mod tables;

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::bytes::Regex;

/// How ExifTool sizes the header it tests magic numbers against.
const HEADER_LEN: usize = 1024;

/// Compiled magic patterns, in ExifTool's test order.
///
/// Patterns that fail to compile are dropped rather than panicking: a bad
/// pattern should cost one format's identification, not the whole binary. The
/// `all_magic_patterns_compile` test asserts the set is in fact complete, so a
/// regression surfaces in CI instead of silently degrading detection.
static COMPILED: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    tables::MAGIC
        .iter()
        .filter_map(|(t, p)| Regex::new(p).ok().map(|r| (*t, r)))
        .collect()
});

/// What a file was identified as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// ExifTool's `FileType`, e.g. `"AIFF"`.
    pub file_type: &'static str,
    /// ExifTool's `FileTypeExtension`, lowercase.
    pub extension: Cow<'static, str>,
    /// ExifTool's `MIMEType`, if it declares one.
    pub mime_type: Option<&'static str>,
}

fn mime_for(file_type: &str) -> Option<&'static str> {
    tables::MIME_TYPE
        .binary_search_by_key(&file_type, |(t, _)| t)
        .ok()
        .map(|i| tables::MIME_TYPE[i].1)
}

/// Canonical extension for a file type, lowercase.
///
/// ExifTool reports the *preferred* extension, not the one on disk: a `.aif`
/// file reports `aiff`, and a DICOM file reports `dcm` rather than `dicom`.
/// The rule is `fileTypeExt{$fileType}` falling back to the type name, printed
/// lowercase.
///
/// Returns `Cow` because the fallback lowercases the type name and so
/// allocates, while the override table is already lowercase and can borrow.
fn extension_for(file_type: &str) -> Cow<'static, str> {
    tables::FILE_TYPE_EXT
        .binary_search_by_key(&file_type, |(t, _)| t)
        .ok()
        .map_or_else(
            || Cow::Owned(file_type.to_ascii_lowercase()),
            |i| Cow::Borrowed(tables::FILE_TYPE_EXT[i].1),
        )
}

fn identity(file_type: &'static str) -> Identity {
    Identity {
        file_type,
        extension: extension_for(file_type),
        mime_type: mime_for(file_type),
    }
}

/// First magic number matching this header, in ExifTool's test order.
///
/// This is `%magicNumber` alone, and `%magicNumber` is a *pre-filter*: ExifTool
/// follows a match by asking the format module to parse the file, and reports
/// "Unknown file type" when that fails. Several patterns are correspondingly
/// loose -- `Font` accepts any file starting `\0\x01` -- so this is not a
/// safe answer on its own. Use [`identify`].
fn magic_match(header: &[u8]) -> Option<&'static str> {
    let head = &header[..header.len().min(HEADER_LEN)];
    COMPILED
        .iter()
        .find(|(_, re)| re.is_match(head))
        .map(|(t, _)| *t)
}

/// Identify a file from its header and, when known, its filename extension.
///
/// Requires a recognised extension that agrees with the header. That is
/// deliberately stricter than matching magic alone: because OxiDex cannot parse
/// these formats, it cannot run ExifTool's confirming step, and an unconfirmed
/// loose pattern would put a confident wrong `FileType` on an arbitrary file --
/// ExifTool's `Font` magic number matches any file starting `\0\x01`.
///
/// The cost is under-claiming. A JPEG named `.dat`, or a file with no
/// extension, is not identified here even though ExifTool would identify it by
/// parsing. That is the intended trade: refusing to answer is recoverable,
/// mislabelling is not.
#[must_use]
pub fn identify(header: &[u8], ext: Option<&str>) -> Option<Identity> {
    let magic = magic_match(header);

    let from_ext = ext.and_then(identify_by_extension)?;
    match magic {
        // Header and extension agree: highest confidence.
        Some(m) if m == from_ext.file_type => Some(from_ext),
        // The extension names a type with no magic number of its own, so
        // nothing contradicts it.
        None if !has_magic(from_ext.file_type) => Some(from_ext),
        // They disagree, or the header matched something else. ExifTool would
        // settle this by parsing; we cannot, so we decline.
        _ => None,
    }
}

fn has_magic(file_type: &str) -> bool {
    tables::MAGIC.iter().any(|(t, _)| *t == file_type)
}

/// Identify by filename extension, for formats with no distinctive header.
#[must_use]
pub fn identify_by_extension(ext: &str) -> Option<Identity> {
    let lower = ext.to_ascii_lowercase();
    tables::EXT_TO_TYPE
        .binary_search_by_key(&lower.as_str(), |(e, _)| e)
        .ok()
        .map(|i| identity(tables::EXT_TO_TYPE[i].1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_magic_patterns_compile() {
        // The runtime drops uncompilable patterns silently so one bad entry
        // cannot break the binary; this is what stops that being invisible.
        let bad: Vec<&str> = tables::MAGIC
            .iter()
            .filter(|(_, p)| Regex::new(p).is_err())
            .map(|(t, _)| *t)
            .collect();
        assert!(bad.is_empty(), "magic patterns failed to compile: {bad:?}");
        assert_eq!(COMPILED.len(), tables::MAGIC.len());
    }

    #[test]
    fn tables_are_sorted_for_binary_search() {
        assert!(tables::MIME_TYPE.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(tables::EXT_TO_TYPE.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn identifies_common_headers() {
        assert_eq!(
            identify(b"BM\x00\x00", Some("bmp")).unwrap().file_type,
            "BMP"
        );
        assert_eq!(
            identify(b"\xff\xd8\xff\xe0", Some("jpg"))
                .unwrap()
                .file_type,
            "JPEG"
        );
        assert_eq!(identify(b"%PDF-1.4", Some("pdf")).unwrap().file_type, "PDF");
        assert_eq!(
            identify(b"\x89PNG\r\n\x1a\n", Some("png"))
                .unwrap()
                .file_type,
            "PNG"
        );
    }

    #[test]
    fn identifies_formats_oxidex_cannot_parse() {
        // These are exactly the files that previously produced no output at
        // all. Identification does not depend on being able to parse them.
        assert_eq!(
            identify(b"FORM\x00\x00\x00\x10AIFF", Some("aif"))
                .unwrap()
                .file_type,
            "AIFF"
        );
        assert_eq!(identify(b"SDPX", Some("dpx")).unwrap().file_type, "DPX");
        assert_eq!(identify(b"FWS\x05", Some("swf")).unwrap().file_type, "SWF");
    }

    #[test]
    fn extension_uses_exiftool_override_not_the_type_name() {
        // DICOM's canonical extension is dcm, not "dicom"; JPEG's is jpg.
        // These come from a lexical hash in ExifTool.pm that is not visible in
        // the symbol table, so a regression here means the extractor stopped
        // finding it.
        assert_eq!(extension_for("DICOM"), "dcm");
        assert_eq!(extension_for("JPEG"), "jpg");
        assert_eq!(extension_for("GZIP"), "gz");
        // No override: lowercase the type name.
        assert_eq!(extension_for("AIFF"), "aiff");
        assert_eq!(extension_for("BMP"), "bmp");
    }

    #[test]
    fn reports_mime_types() {
        assert_eq!(
            identify(b"BM\x00\x00", Some("bmp")).unwrap().mime_type,
            Some("image/bmp")
        );
        assert_eq!(
            identify(b"\xff\xd8\xff\xe0", Some("jpg"))
                .unwrap()
                .mime_type,
            Some("image/jpeg")
        );
    }

    #[test]
    fn unknown_content_is_not_guessed() {
        // ExifTool reports "Unknown file type" for these bytes even though
        // its Font magic number matches them, because the Font module then
        // fails to parse. Requiring the extension to agree reproduces that.
        assert!(identify(b"\x00\x01\x02\x03 not any known format", Some("bin")).is_none());
        assert!(identify(b"", Some("bin")).is_none());
        // No extension means no corroboration, so nothing is claimed even
        // when a magic number matches.
        assert!(identify(b"\x89PNG\r\n\x1a\n", None).is_none());
        // A header that contradicts the extension is refused, not guessed.
        assert!(identify(b"BM\x00\x00", Some("png")).is_none());
    }

    #[test]
    fn extension_lookup_resolves_aliases() {
        assert_eq!(identify_by_extension("jpg").unwrap().file_type, "JPEG");
        assert_eq!(identify_by_extension("JPG").unwrap().file_type, "JPEG");
        assert!(identify_by_extension("nosuchext").is_none());
    }

    #[test]
    fn identification_does_not_mask_corruption() {
        // The read path falls back to identification only for
        // UnsupportedFormat. A malformed file in a format OxiDex *does* parse
        // must stay an error: reporting a corrupt document as a successful
        // read with three identity tags is worse than failing outright.
        // Guarded here so the distinction is not quietly widened later.
        use crate::error::ExifToolError;
        assert!(super::super::core::operations::is_unsupported(
            &ExifToolError::unsupported_format("no parser")
        ));
        assert!(!super::super::core::operations::is_unsupported(
            &ExifToolError::ParseError {
                message: "bad sector shift".to_string(),
                offset: Some(30),
            }
        ));
    }

    #[test]
    fn header_is_bounded() {
        // A huge buffer must not be scanned in full; only the first 1 KiB is
        // ever examined, matching ExifTool.
        let mut big = vec![0u8; 1 << 20];
        big[..2].copy_from_slice(b"BM");
        assert_eq!(identify(&big, Some("bmp")).unwrap().file_type, "BMP");
    }
}
