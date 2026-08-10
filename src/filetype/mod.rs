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

/// Formats whose magic number [`tables::MAGIC`] files under a different name.
///
/// [`identify`] corroborates a header against the *formats* `%fileTypeLookup`
/// names, but `%magicNumber` is keyed by *file type*, and for one entry the two
/// disagree. `PFM => [['Font','PFM2'], 'Printer Font Metrics']` names the
/// processing format `PFM2` -- `Other.pm`'s `ProcessPFM2` -- while the pattern
/// that routine validates is filed under `PFM` (ExifTool.pm:1012):
///
/// ```text
///     PFM  => 'P[Ff]\x0a\d+ \d+\x0a[-+0-9.]+\x0a',
/// ```
///
/// ExifTool never has to reconcile the two, because a format with no
/// `%magicNumber` entry is not skipped by its pre-filter -- it is handed to its
/// module, and `ProcessPFM2` runs the same test itself (ExifTool.pm:3024-3030).
/// OxiDex has no module to load, so the pattern has to be reachable under the
/// name the format list actually uses, or the corroboration step declines a
/// file ExifTool identifies.
///
/// `pfm` is the only extension in [`tables::EXT_TO_TYPE`] whose format list
/// mixes a magic-bearing format with a magic-less one;
/// `magic_alias_reaches_every_format_in_a_mixed_list` fails if a regeneration
/// introduces another.
static MAGIC_ALIAS: &[(&str, &str)] = &[("PFM2", "PFM")];

/// The [`tables::MAGIC`] key that carries `format`'s pattern.
fn magic_key(format: &str) -> &str {
    MAGIC_ALIAS
        .iter()
        .find_map(|(from, to)| (*from == format).then_some(*to))
        .unwrap_or(format)
}

/// Whether the header satisfies the pattern filed under one specific key.
fn magic_matches(key: &str, header: &[u8]) -> bool {
    let head = &header[..header.len().min(HEADER_LEN)];
    COMPILED
        .iter()
        .any(|(k, re)| *k == key && re.is_match(head))
}

/// Whether `header` satisfies the magic number ExifTool files under `file_type`.
///
/// The identity tags and the parser dispatch are two different code paths
/// asking the same question, and when they answer it from two different
/// hand-written approximations they drift. They did: `File:FileType` read
/// this table and reported `DXF` for the `  0\r\n` group codes real AutoCAD
/// writers emit, while `detect_format` tested `starts_with("0\n")`, missed
/// them, and sent the file to the plain-text parser. Detection asks here
/// instead, so a format's magic number has exactly one definition.
#[must_use]
pub fn matches_magic(file_type: &str, header: &[u8]) -> bool {
    magic_matches(magic_key(file_type), header)
}

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
    // `.pfm` is two unrelated formats sharing one FileType. The Font module
    // claims Windows Printer Font Metrics and inherits its root's MIME type,
    // `application/x-font-type1`; `ProcessPFM2` claims Portable FloatMap HDR
    // images and hardcodes the MIME type `%mimeType` does not carry for either
    // (Other.pm:44):
    //
    // ```text
    //     $et->SetFileType('PFM', 'image/x-pfm');
    // ```
    //
    // That literal is the *only* source of `image/x-pfm` in ExifTool, which is
    // why the generated MIME table has no PFM row to look it up in. Both forms
    // report `FileType: PFM`, so the header is the only thing separating them.
    if id.file_type == "PFM" && magic_matches("PFM", header) {
        id.mime_type = Some("image/x-pfm");
    }
    // An ISO base-media file's `ftyp` major brand outranks its extension, and
    // for the HEIF family the two routinely disagree: `QuickTime.heic` is brand
    // `mif1`, which ExifTool reports as `HEIF` / `image/heif` / `heif` while
    // `%fileTypeLookup` maps the `.heic` extension to `HEIC`.
    //
    // ExifTool reads the brand and takes the FileType from the extension named
    // in its `%ftypLookup` description (QuickTime.pm:9993):
    //
    // ```text
    //     if ($ftypLookup{$type} and $ftypLookup{$type} =~ /\(\.(\w+)/) {
    //         $fileType = $1;
    //     ...
    //     $et->SetFileType($fileType, $mimeLookup{$fileType} || 'video/mp4');
    // ```
    //
    // Only the HEIF/AVIF brands are decoded here, because they are the ones
    // whose brand contradicts the extension. The rest of `%ftypLookup` resolves
    // to the same answer the extension already gave, and transcribing 200 rows
    // to confirm what is already correct would be the expensive way to change
    // nothing. `%useExt` is not consulted: its sole entry is `GLV => 'MP4'`,
    // which no brand here can produce.
    if let Some((file_type, mime)) = heif_family_brand(header) {
        id.file_type = Cow::Borrowed(file_type);
        id.extension = extension_for(file_type);
        id.mime_type = Some(mime);
    }
    id
}

/// FileType and MIME type for an ISO base-media `ftyp` major brand, for the
/// HEIF/AVIF family only.
///
/// Transcribed from `%ftypLookup` (QuickTime.pm:227-235) paired with
/// `%mimeLookup` (QuickTime.pm:104-127) in the pinned 13.59 tree. A brand this
/// does not name returns `None`, leaving the extension's answer in place --
/// the rule the generator follows, and the reason no MP4/MOV brand is guessed
/// at here.
fn heif_family_brand(header: &[u8]) -> Option<(&'static str, &'static str)> {
    // `[size:4]["ftyp"]["brand":4]`, so the brand is bytes 8..12 and the atom
    // must declare at least 12 bytes (QuickTime.pm's `$size >= 12`).
    if header.get(4..8) != Some(b"ftyp") {
        return None;
    }
    let size = u32::from_be_bytes(header.get(0..4)?.try_into().ok()?);
    if size < 12 {
        return None;
    }
    Some(match header.get(8..12)? {
        b"heic" => ("HEIC", "image/heic"),
        b"hevc" => ("HEICS", "image/heic-sequence"),
        b"mif1" | b"heix" => ("HEIF", "image/heif"),
        b"msf1" => ("HEIFS", "image/heif-sequence"),
        b"avif" | b"avis" | b"avio" | b"miaf" => ("AVIF", "image/avif"),
        _ => return None,
    })
}

/// Whether the header satisfies the magic number of one of `formats`.
///
/// `None` means the question does not arise: not one of the formats declares a
/// magic number, so nothing can contradict the extension.
///
/// This asks about *specific* formats rather than taking the first pattern in
/// the table that matches, which is what ExifTool does: it puts the
/// extension's own module at the head of the list it tries
/// (`ExtractInfo`/`GetFileType`) instead of letting an earlier, looser pattern
/// answer for it. Taking the first hit made identification order-dependent and
/// silently wrong for any format whose header another pattern also accepts --
/// `HTML.html` opens `<?xml`, which the `XMP` pattern matches 39 entries
/// earlier, so an HTML file was never identified as HTML.
fn magic_accepts(formats: &[&str], header: &[u8]) -> Option<bool> {
    let head = &header[..header.len().min(HEADER_LEN)];
    let mut declared = false;
    for (key, re) in COMPILED.iter() {
        if !formats.iter().any(|f| magic_key(f) == *key) {
            continue;
        }
        declared = true;
        if re.is_match(head) {
            return Some(true);
        }
    }
    declared.then_some(false)
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
///
/// The one exception is text. When the extension is unrecognised entirely,
/// [`identify_text`] gets a look, because `TXT` and `XML` are the two types
/// whose confirming step OxiDex *can* run -- see its docs. `.gpx`, `.kml`,
/// `.igc` and `.log` are in no lookup table, in ExifTool either; ExifTool
/// identifies them purely by content, and without this they reported
/// `FileType: Unknown`.
#[must_use]
pub fn identify(header: &[u8], ext: Option<&str>) -> Option<Identity> {
    let Some((file_type, formats)) = ext.and_then(lookup_extension) else {
        return identify_text(header);
    };
    let root_type = *formats.first()?;
    // The magic-number table is keyed by *format*, not by file type: `.djvu`
    // is FileType DJVU processed by the AIFF module, so its header matches the
    // `AIFF` pattern. Corroborate against the formats the extension declares.
    match magic_accepts(formats, header) {
        // Header and extension agree, or nothing contradicts the extension.
        Some(true) | None => Some(refine(identity(file_type, root_type), header)),
        // The extension names a format whose magic number this header fails.
        // ExifTool would settle it by parsing; we cannot, so we decline.
        Some(false) => None,
    }
}

fn lookup_extension(ext: &str) -> Option<(&'static str, &'static [&'static str])> {
    let lower = ext.to_ascii_lowercase();
    tables::EXT_TO_TYPE
        .binary_search_by_key(&lower.as_str(), |(e, _, _)| e)
        .ok()
        .map(|i| {
            let (_, file_type, formats) = tables::EXT_TO_TYPE[i];
            (file_type, formats)
        })
}

/// Identify a file with no recognised extension as `XML` or `TXT`.
///
/// These are the two types [`identify`]'s "recognised extension" rule can
/// safely be relaxed for, and the reason is the rule's own: it exists because
/// OxiDex cannot run the confirming parse that ExifTool runs after a loose
/// magic number matches. For `TXT` and `XML` there is nothing left to confirm.
/// `%magicNumber{TXT}` *is* the whole test -- every byte in the tested buffer
/// is printable or whitespace -- and XML's confirming step is reading the first
/// tag, which is done here in full.
///
/// Order follows `@fileTypes` (ExifTool.pm:198-206), whose own comment is
/// "put types with weak file signatures at end of list to avoid false matches":
/// `XMP` sits near the front and `TXT` second from the end. That ordering is
/// load-bearing rather than cosmetic -- an XML document is printable text, so
/// it matches the `TXT` pattern too, and testing `TXT` first would report every
/// `.gpx` and `.kml` in the corpus as `TXT`.
///
/// Only `XML` and `TXT` are ever claimed. A header that turns out to be XMP,
/// RDF, SVG, PLIST, INX or RMD is *declined*, not guessed at: OxiDex has real
/// parsers for those reached by their own extensions, and a second, weaker
/// content-only route to the same types could only disagree with them.
#[must_use]
fn identify_text(header: &[u8]) -> Option<Identity> {
    // `%magicNumber{TXT}` ends `[...]*$` -- a star, so it matches the empty
    // string, and an empty buffer would be reported as a text file. ExifTool
    // never gets that far: it fails an empty file with `Error: File is empty`
    // and assigns no `FileType` at all. An empty buffer here also means "the
    // header could not be read", which is not evidence of anything.
    if header.is_empty() {
        return None;
    }
    if is_plain_xml(header) {
        // XML is absent from `%magicNumber` -- it is not a magic-number type at
        // all. ExifTool reaches it through XMP's pattern and then *names* it in
        // the XMP module (XMP.pm:4424), so there is no generated row to consult
        // and `identity()` cannot build this one.
        return Some(Identity {
            file_type: Cow::Borrowed("XML"),
            root_type: "XMP",
            extension: Cow::Borrowed("xml"),
            mime_type: Some("application/xml"),
        });
    }
    if magic_matches("TXT", header) {
        return Some(identity("TXT", "TXT"));
    }
    None
}

/// Whether the header is XML that carries no XMP, RDF, SVG or PLIST payload.
///
/// This is `ProcessXMP`'s type decision (XMP.pm:4360-4424) restricted to the
/// arm that yields `XML`:
///
/// ```text
///     if ($2 eq '<?xml') {
///         if (... '<?aid ')                 { $type = 'INX' }
///         elsif ($buf2 =~ /<x(mp)?:x[ma]pmeta/) { $hasXMP = 1 }
///         else {
///             if ($buf2 =~ /<!DOCTYPE\s+(\w+)/) { svg / plist / REDXIF / ... }
///             elsif ($buf2 =~ /<svg[\s>]/)      { $isSVG = 1 }
///             elsif ($buf2 =~ /<rdf:RDF/)       { $isRDF = 1 }
///             elsif ($buf2 =~ /<plist[\s>]/)    { $type  = 'PLIST' }
///         }
///         $isXML = 1;
///     }
///     ...
///     } elsif ($isXML and not $hasXMP and not $isRDF) { $type = 'XML' }
/// ```
///
/// Every branch that sets one of those flags is a branch this returns `false`
/// for, so the caller declines rather than mislabelling an XMP sidecar or an
/// SVG as a bare `XML` document.
///
/// UTF-16/32 XML is declined too. ExifTool decodes the buffer first and this
/// does not, so the honest answer for a byte string it cannot read as UTF-8 is
/// "no", not a guess. The leading nulls of a UTF-16BE document are excluded by
/// the BOM/whitespace skip below and never reach the `<?xml` test.
fn is_plain_xml(header: &[u8]) -> bool {
    // `\0{0,3}(\xfe\xff|\xff\xfe|\xef\xbb\xbf)?\0{0,3}\s*<` -- the XMP magic
    // number's own preamble. Only the UTF-8 BOM is stepped over; a UTF-16 BOM
    // means the document is not the UTF-8 this function goes on to read.
    let body = header
        .strip_prefix(b"\xef\xbb\xbf".as_slice())
        .unwrap_or(header);
    let body = match std::str::from_utf8(body) {
        Ok(text) => text,
        // A truncated multi-byte sequence at the 1 KiB boundary is normal, and
        // is not a reason to refuse to classify the tag that opens the file.
        Err(error) => match std::str::from_utf8(&body[..error.valid_up_to()]) {
            Ok(text) => text,
            Err(_) => return false,
        },
    };
    let body = body.trim_start();

    // `<svg` and `<rdf:RDF` are the other two openings `%magicNumber{XMP}`
    // admits; both set a flag that rules `XML` out, so neither is XML here.
    if !body.starts_with("<?xml") {
        return false;
    }
    // `$buf2` is ExifTool's own read-ahead buffer, so these searches are over
    // the same window: the header this function was handed.
    if body.contains("<x:xmpmeta")
        || body.contains("<xmp:xmpmeta")
        || body.contains("<x:xapmeta")
        || body.contains("<xmp:xapmeta")
        || body.contains("<rdf:RDF")
        || body.contains("<plist")
        || body.contains("<?aid ")
    {
        return false;
    }
    if let Some(rest) = body.split("<!DOCTYPE").nth(1) {
        // `<!DOCTYPE\s+(\w+)` -- only `fcpxml` continues on to be plain XML;
        // svg, plist and REDXIF each select a type, and anything else makes
        // ExifTool abandon the file entirely (`return 0`).
        let doctype = rest
            .trim_start()
            .split(|c: char| !c.is_alphanumeric() && c != '_');
        if doctype.into_iter().next() != Some("fcpxml") {
            return false;
        }
    }
    // `<svg[\s>]` anywhere in the buffer, per the elsif chain above.
    !body.contains("<svg ") && !body.contains("<svg>") && !body.contains("<svg\n")
}

/// `%mimeType` for one file type, with no root-type fallback.
///
/// Exposed for the parsers that name a file themselves. `SetFileType` resolves
/// the MIME type in three steps (ExifTool.pm:9704-9715):
///
/// ```text
///     $mimeType or $mimeType = $mimeType{$fileType};
///     $mimeType = $mimeType{$baseType} unless $mimeType or $baseType eq 'TIFF';
///     ...
///     $self->FoundTag('MIMEType', $mimeType || 'application/unknown');
/// ```
///
/// [`Identity::mime_type`] collapses the first two, which is right when the
/// extension resolved the type. A parser that names the file from its content
/// -- `ELF shared library`, `Mach-O executable` -- has a name `%mimeType` will
/// never carry, and has to ask for its base type's row itself.
#[must_use]
pub fn mime_for_type(file_type: &str) -> Option<&'static str> {
    mime_for(file_type)
}

/// Identify by filename extension, for formats with no distinctive header.
#[must_use]
pub fn identify_by_extension(ext: &str) -> Option<Identity> {
    lookup_extension(ext).and_then(|(file_type, formats)| {
        formats
            .first()
            .map(|root_type| identity(file_type, root_type))
    })
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
    fn file_type_is_the_lookup_key_not_the_processing_module() {
        // `%fileTypeLookup` maps an extension to [ module, description ], and
        // the module is not the FileType: DJVU => ['AIFF', 'DjVu image'] means
        // DjVu is *parsed by* AIFF.pm. GetFileType (ExifTool.pm:4219) returns
        // the resolved key. Emitting the module here made `.djvu` report
        // FileType AIFF, MIMEType audio/x-aiff -- and the AIFF magic number
        // `^(FORM....AIF[FC]|AT&TFORM)` matches a DjVu header, so the
        // corroboration step agreed with the wrong answer instead of catching
        // it.
        let djvu = identify(b"AT&TFORM\x00\x00\x03\x96DJVM", Some("djvu")).unwrap();
        // DJVM is the multi-page form; AIFF.pm:206 says so in the FileType.
        assert_eq!(djvu.file_type, "DJVU (multi-page)");
        assert_eq!(djvu.extension, "djvu");
        assert_eq!(djvu.mime_type, Some("image/vnd.djvu"));
        // The single-page form keeps the bare name.
        let single = identify(b"AT&TFORM\x00\x00\x03\x96DJVU", Some("djvu")).unwrap();
        assert_eq!(single.file_type, "DJVU");
        // Same shape, different modules.
        assert_eq!(identify_by_extension("avi").unwrap().file_type, "AVI");
        assert_eq!(identify_by_extension("dng").unwrap().file_type, "DNG");
        assert_eq!(identify_by_extension("heic").unwrap().file_type, "HEIC");
        // A string alias resolves to the target key, not to the module.
        assert_eq!(identify_by_extension("aif").unwrap().file_type, "AIFF");
    }

    #[test]
    fn corroboration_asks_the_extensions_own_format_not_the_first_hit() {
        // `HTML.html` in the comparison corpus opens with an XML declaration.
        // The `XMP` magic number (`\s*<`) matches that and sits 39 entries
        // ahead of `HTML` in ExifTool's test order, so "first pattern that
        // matches" answered XMP, disagreed with the extension, and identified
        // nothing at all. ExifTool tries the extension's own module first.
        let html = identify(
            b"<?xml version=\"1.0\"?>\n<!DOCTYPE html PUBLIC",
            Some("html"),
        )
        .expect("html is identified");
        assert_eq!(html.file_type, "HTML");
        assert_eq!(html.mime_type, Some("text/html"));
    }

    #[test]
    fn corroboration_is_against_the_module_not_the_file_type() {
        // An ARW is FileType ARW processed by TIFF.pm, so its header matches
        // the `TIFF` magic number. Comparing that against the file type would
        // reject every raw format; comparing against the module accepts it.
        let arw = identify(b"II*\x00\x08\x00\x00\x00", Some("arw")).unwrap();
        assert_eq!(arw.file_type, "ARW");
        // The header still has to agree with *something* the extension claims.
        assert!(identify(b"BM\x00\x00", Some("arw")).is_none());
    }

    #[test]
    fn pfm_is_two_formats_told_apart_by_the_header() {
        // `.pfm` resolves to one FileType and two MIME types. Both rows below
        // are what the pinned ExifTool 13.59 reports for the two `.pfm` files
        // in its own distribution, t/images/PFM.pfm and t/images/Font.pfm.
        let float = identify(
            b"PF\x0a512 768\x0a-1.000000\x0a\x00\x00\x00\x00",
            Some("pfm"),
        )
        .expect("a Portable FloatMap is identified");
        assert_eq!(float.file_type, "PFM");
        assert_eq!(float.extension, "pfm");
        assert_eq!(float.mime_type, Some("image/x-pfm"));

        // A Printer Font Metrics file opens with its version field, 0x0100
        // little-endian, which is what the Font module's magic number matches.
        // It takes the root module's MIME type, and must *not* pick up the
        // FloatMap one: adding a plain `PFM => image/x-pfm` row to the MIME
        // table is the intuitive fix for the case above and silently breaks
        // this one, because `identity` would then find it for both.
        let font = identify(b"\x00\x01\xf0\x00\x00\x00Copyright (c)", Some("pfm"))
            .expect("a Printer Font Metrics file is identified");
        assert_eq!(font.file_type, "PFM");
        assert_eq!(font.extension, "pfm");
        assert_eq!(font.mime_type, Some("application/x-font-type1"));
    }

    #[test]
    fn a_pfm_matching_neither_form_is_declined() {
        // ExifTool falls through to its plain-text fallback and reports TXT
        // for this, so answering PFM would be confidently wrong rather than
        // merely incomplete.
        assert!(identify(b"this is not a font and not a floatmap\n", Some("pfm")).is_none());
    }

    #[test]
    fn magic_alias_reaches_every_format_in_a_mixed_list() {
        // `magic_accepts` reads "some format here declares a magic number and
        // none of them matched" as a contradiction and declines. That is sound
        // only while every format in such a list is reachable in the magic
        // table: a magic-less one is a format ExifTool would still hand to its
        // module rather than skip, so declining on its behalf is a guess.
        //
        // `pfm`/`PFM2` is the only such entry in 13.59, and `MAGIC_ALIAS` makes
        // it reachable. This fails if a regeneration introduces another, rather
        // than letting it surface the way `.pfm` did -- as `FileType: Unknown`
        // on a file whose parser was working perfectly.
        let keys: std::collections::HashSet<&str> = tables::MAGIC.iter().map(|(t, _)| *t).collect();
        let unreachable: Vec<(&str, Vec<&str>)> = tables::EXT_TO_TYPE
            .iter()
            .filter(|(_, _, formats)| formats.iter().any(|f| keys.contains(magic_key(f))))
            .filter_map(|(ext, _, formats)| {
                let missing: Vec<&str> = formats
                    .iter()
                    .copied()
                    .filter(|f| !keys.contains(magic_key(f)))
                    .collect();
                (!missing.is_empty()).then_some((*ext, missing))
            })
            .collect();
        assert!(
            unreachable.is_empty(),
            "extension format lists mixing magic-bearing and unreachable formats: {unreachable:?}"
        );
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
        let djvu = identify(b"AT&TFORM\x00\x00\x03\x96DJVU", Some("djvu")).unwrap();
        assert_eq!(djvu.file_type, "DJVU");
        assert_eq!(djvu.mime_type, Some("image/vnd.djvu"));

        let cr2 = identify(b"II\x2a\x00\x10\x00\x00\x00CR\x02\x00", Some("cr2")).unwrap();
        assert_eq!(cr2.file_type, "CR2");
        assert_eq!(cr2.mime_type, Some("image/x-canon-cr2"));

        // A header that contradicts both the sub-type and its root is still
        // refused rather than guessed.
        assert!(identify(b"BM\x00\x00", Some("cr2")).is_none());
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

    /// The four corpus extensions no lookup table carries, in either tool.
    #[test]
    fn unlisted_text_extensions_identify_by_content() {
        let xml = br#"<?xml version="1.0"?><gpx version="1.0"></gpx>"#;
        for ext in ["gpx", "kml", "xml", "nosuchext"] {
            let id = identify(xml, Some(ext)).expect(ext);
            assert_eq!(id.file_type, "XML", "{ext}");
            assert_eq!(id.extension, "xml", "{ext}");
            assert_eq!(id.mime_type, Some("application/xml"), "{ext}");
        }

        let text = b"$PMGNTRK,4415.163,N,07631.126,W,00095,M,110833.53,A,,030409*68\r\n";
        for ext in ["log", "igc", "nosuchext"] {
            let id = identify(text, Some(ext)).expect(ext);
            assert_eq!(id.file_type, "TXT", "{ext}");
            assert_eq!(id.extension, "txt", "{ext}");
            assert_eq!(id.mime_type, Some("text/plain"), "{ext}");
        }
    }

    /// XML is tested before TXT, because XML *is* printable text.
    ///
    /// `@fileTypes` puts XMP near the front and TXT second from the end, under
    /// the comment "put types with weak file signatures at end of list to avoid
    /// false matches". Reversing the two here reports every `.gpx` as TXT.
    #[test]
    fn xml_is_tested_before_the_weaker_txt_pattern() {
        let xml = br#"<?xml version="1.0"?><kml></kml>"#;
        assert!(magic_matches("TXT", xml), "precondition: XML is also TXT");
        assert_eq!(identify(xml, Some("kml")).unwrap().file_type, "XML");
    }

    /// Everything `ProcessXMP` gives a name other than `XML` is declined.
    ///
    /// OxiDex reaches XMP, SVG and PLIST through their own extensions and real
    /// parsers; a second, weaker content-only route to the same types could
    /// only disagree with them, so this path answers `XML` or nothing.
    #[test]
    fn non_xml_markup_is_declined_rather_than_guessed_at() {
        for (label, body) in [
            (
                "xmpmeta",
                r#"<?xml version="1.0"?><x:xmpmeta xmlns:x="adobe:ns:meta/">"#,
            ),
            (
                "xapmeta",
                r#"<?xml version="1.0"?><x:xapmeta xmlns:x="adobe:ns:meta/">"#,
            ),
            (
                "bare rdf",
                r#"<?xml version="1.0"?><rdf:RDF xmlns:rdf="x"></rdf:RDF>"#,
            ),
            ("svg tag", r#"<?xml version="1.0"?><svg width="1"></svg>"#),
            (
                "svg doctype",
                r#"<?xml version="1.0"?><!DOCTYPE svg PUBLIC "x">"#,
            ),
            ("plist tag", r#"<?xml version="1.0"?><plist version="1.0">"#),
            (
                "plist doctype",
                r#"<?xml version="1.0"?><!DOCTYPE plist PUBLIC "x">"#,
            ),
            ("inx", "<?xml version=\"1.0\"?>\n<?aid style=\"50\"?>"),
            (
                "unknown doctype",
                r#"<?xml version="1.0"?><!DOCTYPE REDXIF><a/>"#,
            ),
        ] {
            assert!(
                !is_plain_xml(body.as_bytes()),
                "{label} must not be claimed as plain XML"
            );
        }

        // Final Cut Pro XML is the one DOCTYPE that stays plain XML.
        assert!(is_plain_xml(
            br#"<?xml version="1.0"?><!DOCTYPE fcpxml><fcpxml/>"#
        ));
    }

    /// A UTF-8 BOM is stepped over; a UTF-16 BOM means "not the UTF-8 we read".
    #[test]
    fn byte_order_marks_are_handled_conservatively() {
        let mut bom_xml = b"\xef\xbb\xbf".to_vec();
        bom_xml.extend_from_slice(br#"<?xml version="1.0"?><a/>"#);
        assert!(is_plain_xml(&bom_xml));

        // UTF-16LE `<?xml`. ExifTool decodes first and this does not, so the
        // honest answer is to decline rather than guess.
        let utf16 = b"\xff\xfe<\0?\0x\0m\0l\0";
        assert!(!is_plain_xml(utf16));
    }

    /// Binary content is not text, and a recognised extension still wins.
    #[test]
    fn binary_content_is_not_claimed_as_text() {
        // \x00 and \x01 are outside TXT's `[\x07-\x0d\x20-\x7e\x80-\xfe]`.
        assert!(identify(b"\x00\x01\x02\x03rest of file", Some("nosuchext")).is_none());
        assert!(identify(b"\x00\x01\x02\x03rest of file", None).is_none());

        // A known extension that agrees with its magic number is unaffected by
        // the fallback: it never reaches it.
        assert_eq!(identify(b"BM....", Some("bmp")).unwrap().file_type, "BMP");
    }

    /// An empty buffer is not a text file.
    ///
    /// `%magicNumber{TXT}` ends `[...]*$`, and the star matches the empty
    /// string -- so the pattern alone calls a zero-byte file TXT. ExifTool
    /// never consults it: an empty file fails with `Error: File is empty` and
    /// gets no `FileType` at all. `unknown_content_is_not_guessed` caught this
    /// when the content fallback was added.
    #[test]
    fn an_empty_buffer_is_not_a_text_file() {
        assert!(
            magic_matches("TXT", b""),
            "precondition: the pattern matches"
        );
        assert!(identify_text(b"").is_none());
        assert!(identify(b"", None).is_none());
        assert!(identify(b"", Some("nosuchext")).is_none());
    }
}
