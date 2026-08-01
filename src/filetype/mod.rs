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
    ///
    /// A `Cow` because one format reports a subtype rather than a bare name:
    /// AIFF.pm:206 appends `" (multi-page)"` for a DjVu whose top-level FORM
    /// is `DJVM`.
    pub file_type: Cow<'static, str>,
    /// The root format whose module reads this file, e.g. `"TIFF"` for `CR2`.
    ///
    /// Equal to `file_type` for a root format. [`tables::MAGIC`] is keyed on
    /// the root, so this is the name a header match is compared against.
    pub root_type: &'static str,
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

fn identity(file_type: &'static str, root_type: &'static str) -> Identity {
    Identity {
        file_type: Cow::Borrowed(file_type),
        root_type,
        extension: extension_for(file_type),
        // ExifTool falls back to the root's MIME type when the sub-type
        // declares none: `$mimeType = $mimeType{$baseType} unless $mimeType`
        // (ExifTool.pm, SetFileType).
        mime_type: mime_for(file_type).or_else(|| mime_for(root_type)),
    }
}

/// Whether the header satisfies the magic number of `file_type` or `root_type`.
///
/// `None` means the question does not arise: neither name declares a magic
/// number, so nothing can contradict the extension.
///
/// This asks about *those two formats* rather than taking the first pattern in
/// the table that matches, which is what ExifTool does: it puts the
/// extension's own module at the head of the list it tries
/// (`ExtractInfo`/`GetFileType`) instead of letting an earlier, looser pattern
/// answer for it. Taking the first hit made identification order-dependent and
/// silently wrong for any format whose header another pattern also accepts --
/// `HTML.html` in the comparison corpus opens `<?xml`, which the `XMP` pattern
/// matches 39 entries earlier, so an HTML file was never identified as HTML.
///
/// `%magicNumber` is keyed on root formats, so a sub-type corroborates through
/// its root as well: a .cr2 header matches the TIFF pattern, and a .djvu
/// header matches AIFF's `AT&TFORM` alternative.
fn magic_accepts(file_type: &str, root_type: &str, header: &[u8]) -> Option<bool> {
    let head = &header[..header.len().min(HEADER_LEN)];
    let mut declared = false;
    for (format, re) in COMPILED.iter() {
        if *format != file_type && *format != root_type {
            continue;
        }
        declared = true;
        if re.is_match(head) {
            return Some(true);
        }
    }
    declared.then_some(false)
}

/// `FileType` refinements a module makes after its magic number matches.
///
/// ExifTool's magic number is only a pre-filter; the module that accepts the
/// file gets the last word on what to call it. DjVu is the one case where that
/// changes the reported string rather than just confirming it:
///
/// ```text
///     $et->SetFileType('DJVU');
///     ...
///     # modify FileType to indicate a multi-page document
///     $$et{VALUE}{FileType} .= " (multi-page)" if $buf2 eq 'DJVM' ...
/// ```
///
/// (AIFF.pm:202-206 -- DjVu files are recognised and walked by AIFF.pm.)
fn refine(mut id: Identity, header: &[u8]) -> Identity {
    if id.file_type == "DJVU" && header.get(12..16) == Some(b"DJVM") {
        id.file_type = Cow::Owned(format!("{} (multi-page)", id.file_type));
    }
    id
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
    let from_ext = ext.and_then(identify_by_extension)?;
    match magic_accepts(&from_ext.file_type, from_ext.root_type, header) {
        // Header and extension agree, or nothing contradicts the extension.
        Some(true) | None => Some(refine(from_ext, header)),
        // The extension names a format whose magic number this header fails.
        // ExifTool would settle it by parsing; we cannot, so we decline.
        Some(false) => None,
    }
}

/// Identify by filename extension, for formats with no distinctive header.
#[must_use]
pub fn identify_by_extension(ext: &str) -> Option<Identity> {
    let lower = ext.to_ascii_lowercase();
    tables::EXT_TO_TYPE
        .binary_search_by_key(&lower.as_str(), |(e, _, _)| e)
        .ok()
        .map(|i| identity(tables::EXT_TO_TYPE[i].1, tables::EXT_TO_TYPE[i].2))
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

    /// `%fileTypeLookup`'s first array element is the *root* format -- the
    /// module that reads the file -- and the reported `FileType` is the key.
    /// Recording the root instead mislabelled 162 of ExifTool 13.30's 350
    /// extensions, turning a DjVu image into AIFF audio (`DJVU => ['AIFF']`)
    /// and every TIFF-based raw into plain TIFF.
    #[test]
    fn sub_types_report_their_own_name_not_their_root() {
        for (ext, want_type, want_root) in [
            ("cr2", "CR2", "TIFF"),
            ("nef", "NEF", "TIFF"),
            ("djvu", "DJVU", "AIFF"),
            ("ttf", "TTF", "Font"),
            ("j2c", "J2C", "JP2"),
            ("csv", "CSV", "TXT"),
            ("wmv", "WMV", "ASF"),
            // A root format is its own root.
            ("jpg", "JPEG", "JPEG"),
        ] {
            let id = identify_by_extension(ext).unwrap();
            assert_eq!(id.file_type, want_type, "FileType for .{ext}");
            assert_eq!(id.root_type, want_root, "root for .{ext}");
        }
    }

    /// An extension that reaches its entry through an alias only keeps the
    /// landing key when that key is a real sub-type -- i.e. when its root is
    /// itself a format in the table. `DCM => 'DICM'` and `DICM => ['DICOM']`
    /// land on "DICM", which ExifTool never prints; the root DICOM is absent
    /// from the table because it is a bare module name, and that is the answer.
    #[test]
    fn alias_hops_to_a_bare_module_name_report_the_module() {
        assert_eq!(identify_by_extension("dcm").unwrap().file_type, "DICOM");
        assert_eq!(identify_by_extension("dcm").unwrap().extension, "dcm");

        // The hop still keeps a genuine sub-type: DJVU's root AIFF *is* a
        // format in the table, so .djv stays DJVU rather than collapsing to
        // AIFF -- as do .azw (MOBI), .j2k (J2C) and .3gp2 (3G2).
        for (ext, want) in [
            ("djv", "DJVU"),
            ("azw", "MOBI"),
            ("j2k", "J2C"),
            ("3gp2", "3G2"),
        ] {
            assert_eq!(identify_by_extension(ext).unwrap().file_type, want);
        }
    }

    /// An upper-cased spelling of the root is not a sub-type: every
    /// `%fileTypeLookup` key is upper-case because they are extensions, and
    /// `VCARD => ['VCard', ...]` is the same word twice. ExifTool reports its
    /// own spelling.
    #[test]
    fn root_spelling_wins_over_the_upper_cased_extension_key() {
        assert_eq!(identify_by_extension("vcf").unwrap().file_type, "VCard");
        assert_eq!(
            identify_by_extension("torrent").unwrap().file_type,
            "Torrent"
        );
        assert_eq!(identify_by_extension("macos").unwrap().file_type, "MacOS");
    }

    /// A sub-type is corroborated through its root, because `%magicNumber` is
    /// keyed on roots: a .cr2's header matches the TIFF pattern, and a .djvu's
    /// matches AIFF's `AT&TFORM` alternative. Requiring the header to name the
    /// sub-type itself would have declined every one of them.
    #[test]
    fn header_corroborates_a_sub_type_through_its_root() {
        // DJVM is the multi-page form, and AIFF.pm:206 says so in the
        // FileType; the single-page DJVU form keeps the bare name.
        let djvu = identify(b"AT&TFORM\x00\x00\x03\x96DJVM", Some("djvu")).unwrap();
        assert_eq!(djvu.file_type, "DJVU (multi-page)");
        assert_eq!(djvu.mime_type, Some("image/vnd.djvu"));
        assert_eq!(
            identify(b"AT&TFORM\x00\x00\x03\x96DJVU", Some("djvu"))
                .unwrap()
                .file_type,
            "DJVU"
        );

        let cr2 = identify(b"II\x2a\x00\x10\x00\x00\x00CR\x02\x00", Some("cr2")).unwrap();
        assert_eq!(cr2.file_type, "CR2");
        assert_eq!(cr2.mime_type, Some("image/x-canon-cr2"));

        // A header that contradicts both the sub-type and its root is still
        // refused rather than guessed.
        assert!(identify(b"BM\x00\x00", Some("cr2")).is_none());
    }

    /// Corroboration asks the extension's *own* formats, not whichever pattern
    /// comes first in the table.
    ///
    /// `HTML.html` in the comparison corpus opens with an XML declaration. The
    /// `XMP` magic number (`\s*<`) matches that and sits 39 entries ahead of
    /// `HTML` in ExifTool's test order, so "first pattern that matches"
    /// answered XMP, disagreed with the extension, and identified nothing at
    /// all. ExifTool tries the extension's own module first.
    #[test]
    fn corroboration_asks_the_extensions_own_format_not_the_first_hit() {
        let html = identify(
            b"<?xml version=\"1.0\"?>\n<!DOCTYPE html PUBLIC",
            Some("html"),
        )
        .expect("html is identified");
        assert_eq!(html.file_type, "HTML");
        assert_eq!(html.mime_type, Some("text/html"));
        // A real XMP sidecar still identifies as XMP, not as HTML.
        assert_eq!(
            identify(b"<?xpacket begin=\"\"?><x:xmpmeta", Some("xmp"))
                .unwrap()
                .file_type,
            "XMP"
        );
    }

    /// ExifTool falls back to the root's MIME type when the sub-type declares
    /// none: `$mimeType = $mimeType{$baseType} unless $mimeType`.
    #[test]
    fn mime_type_falls_back_to_the_root() {
        // PFB has no %mimeType entry of its own; Font does.
        assert_eq!(mime_for("PFB"), None);
        assert_eq!(
            identify_by_extension("pfb").unwrap().mime_type,
            mime_for("Font")
        );
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
