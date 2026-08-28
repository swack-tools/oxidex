//! Capture One EIP (Enhanced Image Package) parser.
//!
//! Transcribed from `Image::ExifTool::CaptureOne` in the pinned 13.59 tree
//! (`CaptureOne.pm`). An EIP is a ZIP container holding an image (IIQ, TIFF
//! or JPEG) plus Capture One Settings (`.cos`) files, which are XML documents
//! whose `<E K="name" V="value"/>` elements carry the real properties.
//!
//! ExifTool reaches this module from `ProcessZIP`: any archive with a member
//! matching `^CaptureOne/.*\.(cos|COS)$` is handed to `ProcessEIP`
//! (`ZIP.pm:619-623`), which
//!
//! 1. stamps every ZIP member with the eight `ZIP:Zip*` tags under its own
//!    sub-document number (`CaptureOne.pm:156-165`),
//! 2. picks the highest-sorting `manifest\d*.xml` member and reads the
//!    `<RawPath>`/`<SettingsPath>` file list out of it, requiring at least
//!    one non-`.cos` image before trusting it (`CaptureOne.pm:130-153`),
//! 3. parses each listed `.cos` member through the XMP reader with the
//!    `K`/`V` attribute swap (`CaptureOne.pm:37-58`), and
//! 4. re-enters the full metadata extractor on each listed image member
//!    (`CaptureOne.pm:190-191`).

use crate::core::{Instance, MetadataMap, ReadOptions, SHIM_DEFAULT_PRIORITY, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::BufferedReader;
use crate::parsers::archive::zip::record_zip_member_tags;
use crate::parsers::raw::{RawFormat, parse_raw_metadata};
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};
use std::io::{Cursor, Read};
use zip::ZipArchive;

/// Ceiling for an embedded member's declared uncompressed size.
///
/// The declared size comes straight from the attacker-controlled central
/// directory, so it must never drive an unbounded read. Real Phase One IIQ
/// members top out in the low hundreds of megabytes; 1 GiB leaves ample
/// headroom while keeping a hostile declaration from forcing a huge
/// allocation.
const EIP_MEMBER_MAX_SIZE: u64 = 1 << 30;

/// Parse a Capture One EIP file.
///
/// The counterpart of `ProcessEIP` (`CaptureOne.pm:120-197`). The identity
/// tags mirror its `$et->SetFileType('EIP')` (`CaptureOne.pm:126`); the MIME
/// type is `%mimeType`'s EIP row, which the generated
/// `crate::filetype` tables also carry.
pub fn parse_eip_metadata(
    reader: &dyn crate::core::FileReader,
) -> std::result::Result<MetadataMap, String> {
    parse(reader).map_err(|e| format!("EIP parse error: {}", e))
}

fn parse(reader: &dyn crate::core::FileReader) -> Result<MetadataMap> {
    let size = reader.size() as usize;
    let file_data = reader.read(0, size)?;

    let mut metadata = MetadataMap::new();

    // Every archive member's ZIP:Zip* tags, one sub-document per member with
    // the first member winning the bare key -- the shared transcription of
    // `HandleMember` that OOXML and plain ZIP also use. ProcessEIP emits
    // these for *all* members, parsed or not (`CaptureOne.pm:157-165`).
    record_zip_member_tags(file_data, &mut metadata);

    let mut archive = ZipArchive::new(Cursor::new(file_data))
        .map_err(|e| ExifToolError::parse_error(format!("Failed to read EIP archive: {}", e)))?;

    // Member names in central-directory order (`$zip->members()`,
    // `CaptureOne.pm:157`). `ZipArchive::file_names()` iterates a hash map,
    // so the index walk is what preserves file order here. `name_for_index`
    // reads the parsed central directory directly and cannot fail for a
    // valid index -- unlike `by_index`, which errors on e.g. an encrypted
    // member and, filtered out, would shift every later position so that
    // `read_member(index)` read one member's bytes under another's name and
    // document number. Archive::Zip's `members()` likewise enumerates every
    // entry by name and only fails at `contents()` time, which is
    // `read_member` here.
    let member_names: Vec<String> = (0..archive.len())
        .map(|i| archive.name_for_index(i).unwrap_or_default().to_owned())
        .collect();

    // Choose the manifest: all members matching `^manifest\d*.xml$` (the `.`
    // is unescaped in the Perl pattern, so it matches any single character),
    // keeping the byte-wise greatest name -- `next if $file and $file gt $f`
    // keeps the candidate on ties (`CaptureOne.pm:131-139`).
    let mut manifest: Option<(usize, &str)> = None;
    for (index, name) in member_names.iter().enumerate() {
        if !is_manifest_name(name) {
            continue;
        }
        if let Some((_, best)) = manifest
            && best > name.as_str()
        {
            continue;
        }
        manifest = Some((index, name));
    }

    // File names to parse, from the chosen manifest's <RawPath> and
    // <SettingsPath> elements. A manifest without at least one image entry
    // is ignored entirely (`CaptureOne.pm:141-154`).
    let mut parse_files: Vec<String> = Vec::new();
    if let Some((index, _)) = manifest
        && let Some(buff) = read_member(&mut archive, index)
    {
        let text = String::from_utf8_lossy(&buff);
        let mut found_image = false;
        for path in manifest_paths(&text) {
            let Some(extension) = ascii_extension(&path) else {
                continue;
            };
            // `next unless $file =~ /\.(cos|iiq|jpe?g|tiff?)$/i`
            if !matches!(
                extension.as_str(),
                "cos" | "iiq" | "jpg" | "jpeg" | "tif" | "tiff"
            ) {
                continue;
            }
            if extension != "cos" {
                found_image = true;
            }
            parse_files.push(path);
        }
        if !found_image {
            parse_files.clear();
        }
    }

    // Walk every member; the document number increments for each one whether
    // or not it is parsed (`$$et{DOC_NUM} = ++$docNum`, `CaptureOne.pm:164`).
    for (index, name) in member_names.iter().enumerate() {
        let doc_num = index as u32 + 1;
        let selected = if parse_files.is_empty() {
            // Manifest missing or unusable: fall back to image files in the
            // root directory and `.cos` files under `CaptureOne/`
            // (`CaptureOne.pm:168-172`, case-insensitive).
            fallback_selects(name)
        } else {
            parse_files.iter().any(|f| f == name)
        };
        if !selected {
            continue;
        }
        let Some(buff) = read_member(&mut archive, index) else {
            // `$status and $et->Warn("Error extracting $file"), next`
            continue;
        };
        let extension = ascii_extension(name).unwrap_or_default();
        if extension == "cos" {
            parse_cos(&buff, Instance(doc_num), &mut metadata);
        } else {
            parse_image_member(&buff, &extension, Instance(doc_num), &mut metadata);
        }
    }

    Ok(metadata)
}

/// `^manifest\d*.xml$` (`CaptureOne.pm:131`), where the unescaped `.`
/// matches any single character: literal `manifest`, then digits, then one
/// arbitrary character, then literal `xml` at end of string. Case-sensitive.
fn is_manifest_name(name: &str) -> bool {
    let Some(middle) = name
        .strip_prefix("manifest")
        .and_then(|rest| rest.strip_suffix("xml"))
    else {
        return false;
    };
    let bytes = middle.as_bytes();
    let Some((_any_one_char, digits)) = bytes.split_last() else {
        return false;
    };
    digits.iter().all(u8::is_ascii_digit)
}

/// Every `<RawPath>` / `<SettingsPath>` element value in the manifest --
/// `m{<(RawPath|SettingsPath)>(.*?)</\1>}sg` (`CaptureOne.pm:145`).
fn manifest_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for open in ["<RawPath>", "<SettingsPath>"] {
        let close = match open {
            "<RawPath>" => "</RawPath>",
            _ => "</SettingsPath>",
        };
        let mut rest = text;
        while let Some(start) = rest.find(open) {
            let after = &rest[start + open.len()..];
            let Some(end) = after.find(close) else {
                break;
            };
            paths.push(after[..end].to_owned());
            rest = &after[end + close.len()..];
        }
    }
    paths
}

/// The lower-cased final extension of a path, if any.
fn ascii_extension(path: &str) -> Option<String> {
    path.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
}

/// The manifest-less selection rule:
/// `^([^/]+\.(iiq|jpe?g|tiff?)|CaptureOne/.*\.cos)$` case-insensitively
/// (`CaptureOne.pm:171`).
fn fallback_selects(name: &str) -> bool {
    let Some(extension) = ascii_extension(name) else {
        return false;
    };
    if extension == "cos" {
        // `CaptureOne/.*` -- the Perl match is under `/i`, so the directory
        // comparison is case-insensitive here, unlike the detection pattern.
        let mut chars = name.splitn(2, '/');
        let (Some(first), Some(_rest)) = (chars.next(), chars.next()) else {
            return false;
        };
        return first.eq_ignore_ascii_case("CaptureOne");
    }
    // `[^/]+\.` -- at least one character before the dot, none of them `/`
    // (the `!name.contains('/')` covers the stem as well as the rest).
    let stem_nonempty = name
        .rsplit_once('.')
        .is_some_and(|(stem, _)| !stem.is_empty());
    matches!(extension.as_str(), "iiq" | "jpg" | "jpeg" | "tif" | "tiff")
        && !name.contains('/')
        && stem_nonempty
}

/// Read one member's full contents, bounded by its declared size.
///
/// Returns `None` on any inconsistency -- ExifTool warns and skips the
/// member (`CaptureOne.pm:176`), it never fails the whole file.
fn read_member(archive: &mut ZipArchive<Cursor<&[u8]>>, index: usize) -> Option<Vec<u8>> {
    let member = archive.by_index(index).ok()?;
    let declared_size = member.size();
    if declared_size > EIP_MEMBER_MAX_SIZE {
        return None;
    }
    let mut data = Vec::new();
    // Reading one byte past the declared size catches a stream longer than
    // declared as well as one shorter.
    member.take(declared_size + 1).read_to_end(&mut data).ok()?;
    (data.len() as u64 == declared_size).then_some(data)
}

/// Parse one embedded image member and merge its tags.
///
/// ExifTool re-enters the whole extractor (`$et->ExtractInfo(\$buff,
/// { ReEntry => 1 })`, `CaptureOne.pm:191`), so the member's own format
/// parser runs and its IFD tags land in the file's map. The container's
/// identity stays the EIP's, so the member's `File:*` and `Composite:*`
/// projections are dropped before the merge: identity comes from the EIP
/// tables, and the Composite pass runs once, centrally, over the merged map
/// (`operations.rs` Step 6) -- which is also where the
/// `TIFF_TYPE =~ /^(CR2|Canon 1D RAW|IIQ|EIP)$/` ImageSize preference
/// (Exif.pm:4384-4390) already knows about EIP.
fn parse_image_member(
    data: &[u8],
    extension: &str,
    instance: Instance,
    metadata: &mut MetadataMap,
) {
    let parsed = match extension {
        "iiq" => parse_raw_metadata(data, RawFormat::PhaseOneIIQ),
        "jpg" | "jpeg" => {
            let reader = BufferedReader::from_bytes(data);
            crate::core::operations::parse_jpeg_metadata(&reader, &ReadOptions::default())
        }
        "tif" | "tiff" => {
            let reader = BufferedReader::from_bytes(data);
            crate::core::operations::parse_tiff_metadata(&reader)
        }
        _ => return,
    };
    let Ok(image_metadata) = parsed else {
        // ExifTool's re-entry failing produces no tags but no error either.
        return;
    };

    // The embedded TIFF header is what sets `File:ExifByteOrder` on the
    // re-entrant pass (`FoundTag('ExifByteOrder', ...)`, ExifTool.pm:8702);
    // the container itself starts `PK`, so `operations.rs`'s Step 5b
    // backstop cannot see it. First image wins, like every re-entry tag.
    if !metadata.contains_key("File:ExifByteOrder") {
        let order = match data.get(0..2) {
            Some(b"II") => Some("Little-endian (Intel, II)"),
            Some(b"MM") => Some("Big-endian (Motorola, MM)"),
            _ => None,
        };
        if let Some(order) = order {
            metadata.insert("File:ExifByteOrder", TagValue::new_string(order));
        }
    }

    // Copy the member parser's *winner* projection, key by key, under this
    // member's sub-document instance. Not `MetadataMap::merge`: merge
    // replays raw occurrences, and `remove()` only clears the winners
    // projection, so a merge would resurrect the member's own `File:*`
    // identity claims (`FileType: IIQ`) over the container's. ExifTool never
    // has those claims to begin with -- a `ReEntry` extraction skips
    // `SetFileType` -- and its Composite pass runs once over the combined
    // tag set, which is `operations.rs` Step 6 here. The instance keeps the
    // arbitration of `ExifTool.pm:9564`: a second image member cannot
    // displace the first one's values.
    for (key, value) in image_metadata.iter() {
        if key.starts_with("File:") || key.starts_with("Composite:") {
            continue;
        }
        let group1 = key.split(':').next().unwrap_or_default().to_owned();
        metadata.insert_occurrence(
            key.as_str(),
            value.clone(),
            SHIM_DEFAULT_PRIORITY,
            &group1,
            instance,
        );
    }
}

/// Parse a Capture One Settings (COS) XML document.
///
/// COS files carry their properties as `<E K="name" V="value"/>` elements.
/// ExifTool routes them through the XMP reader with two overrides
/// (`ProcessCOS`, `CaptureOne.pm:98-111`): `HandleCOSAttrs` swaps the `K`/`V`
/// attribute pair in as the property name and value whenever the element has
/// no content of its own (`CaptureOne.pm:41-43`), and `FoundCOS` adds each
/// name to the table dynamically and records the value
/// (`CaptureOne.pm:66-92`).
///
/// All tags from one COS document share one sub-document number, so a name
/// repeated inside the document (`Rotation` appears in both the `<DL>` and
/// `<AL>` sections) resolves to the *last* occurrence -- the pinned oracle's
/// `-G -s -j -a` output on `t/images/CaptureOne.eip` reports
/// `XML:Rotation: 90`, the `<AL>` value. `Instance(doc_num)` reproduces both
/// halves of `ExifTool.pm:9564`'s guard: a same-instance duplicate wins the
/// key, a different COS document's duplicate does not displace the first.
fn parse_cos(data: &[u8], instance: Instance, metadata: &mut MetadataMap) {
    let mut reader = Reader::from_reader(data);
    let mut stack: Vec<Pending> = Vec::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => stack.push(Pending::capture(e)),
            Ok(Event::Empty(ref e)) => {
                let pending = Pending::capture(e);
                emit_cos(pending, instance, metadata);
            }
            Ok(Event::Text(ref t)) => {
                if let Some(top) = stack.last_mut()
                    && let Ok(text) = t.xml10_content()
                {
                    top.text.push_str(&text);
                }
            }
            Ok(Event::End(_)) => {
                if let Some(pending) = stack.pop() {
                    emit_cos(pending, instance, metadata);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
        buf.clear();
    }
}

/// One partially-read COS element: its name, its `K`/`V` attributes if
/// present, and whatever direct text content has accumulated so far.
struct Pending {
    name: String,
    k: Option<String>,
    v: Option<String>,
    text: String,
}

impl Pending {
    fn capture(e: &quick_xml::events::BytesStart<'_>) -> Pending {
        let mut pending = Pending {
            name: String::from_utf8_lossy(e.local_name().as_ref()).into_owned(),
            k: None,
            v: None,
            text: String::new(),
        };
        for attr in e.attributes().flatten() {
            let key = attr.key.as_ref();
            // `defined $$attrs{K} and defined $$attrs{V}`
            // (`CaptureOne.pm:41`): the attribute names are exactly `K`
            // and `V`.
            if key == b"K" || key == b"V" {
                let value = attr
                    .normalized_value(XmlVersion::Implicit1_0)
                    .map(|v| v.into_owned())
                    .unwrap_or_else(|_| String::from_utf8_lossy(&attr.value).into_owned());
                if key == b"K" {
                    pending.k = Some(value);
                } else {
                    pending.v = Some(value);
                }
            }
        }
        pending
    }
}

/// Record one COS property, following `HandleCOSAttrs` + `FoundCOS`.
fn emit_cos(pending: Pending, instance: Instance, metadata: &mut MetadataMap) {
    struct Property {
        name: String,
        value: String,
    }

    // `HandleCOSAttrs`: the K/V pair stands in for the property only when
    // the element carries no content of its own (`not length $$valPt`,
    // `CaptureOne.pm:41`).
    let property = if pending.text.is_empty()
        && let (Some(k), Some(v)) = (&pending.k, &pending.v)
    {
        Property {
            name: k.clone(),
            value: v.clone(),
        }
    } else if !pending.text.trim().is_empty() {
        Property {
            name: pending.name,
            value: pending.text,
        }
    } else {
        return;
    };

    // `FoundCOS`: `return 0 unless length $tag` (`CaptureOne.pm:75`).
    if property.name.is_empty() {
        return;
    }

    // The one statically-declared entry in `%CaptureOne::Main`:
    // `ColorCorrections => { ValueConv => '\$val', Hidden => 1 }`
    // (`CaptureOne.pm:29`). A scalar-reference ValueConv renders as
    // ExifTool's standard binary placeholder, sized after the UTF-8
    // decode and XML unescape.
    if property.name == "ColorCorrections" {
        metadata.insert_occurrence(
            "XML:ColorCorrections",
            TagValue::new_binary(property.value.into_bytes()),
            SHIM_DEFAULT_PRIORITY,
            "XML",
            instance,
        );
        return;
    }

    let name = sanitize_cos_name(&property.name);

    // `if ($name =~ /Date(?![a-z])/)` -- a dynamically added tag whose
    // sanitized name contains `Date` not followed by a lowercase letter is
    // treated as a date: `ValueConv =>
    // 'Image::ExifTool::XMP::ConvertXMPDate($val,1)'` (`CaptureOne.pm:79-82`).
    // The PrintConv is `ConvertDateTime`, which without `-d` returns its
    // argument unchanged.
    let value = if has_date_marker(&name) {
        convert_xmp_date_unsure(&property.value)
    } else {
        property.value
    };

    metadata.insert_occurrence(
        format!("XML:{}", name),
        TagValue::new_string(value),
        SHIM_DEFAULT_PRIORITY,
        "XML",
        instance,
    );
}

/// `AddTagToTable`'s name normalization (`ExifTool.pm:9254-9266`): strip
/// everything outside `[-_a-zA-Z0-9]`, capitalize the first letter, and
/// prefix `Tag` when what remains is shorter than two characters or does not
/// start with a letter.
fn sanitize_cos_name(raw: &str) -> String {
    let mut name: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if let Some(first) = name.chars().next()
        && first.is_ascii_lowercase()
    {
        name.replace_range(0..1, &first.to_ascii_uppercase().to_string());
    }
    if name.len() < 2 || !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        name = format!("Tag{}", name);
    }
    name
}

/// `/Date(?![a-z])/` over the sanitized name (`CaptureOne.pm:79`).
fn has_date_marker(name: &str) -> bool {
    let bytes = name.as_bytes();
    let mut from = 0;
    while let Some(pos) = name[from..].find("Date") {
        let after = from + pos + "Date".len();
        if bytes.get(after).is_none_or(|b| !b.is_ascii_lowercase()) {
            return true;
        }
        from = after;
    }
    false
}

/// `Image::ExifTool::XMP::ConvertXMPDate($val, 1)` (`XMP.pm:3383-3394`).
///
/// With `$unsure` set -- which is how `FoundCOS` calls it -- only a value
/// matching the full XMP datetime shape
/// `^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}:\d{2})(:\d{2})?\s*(\S*)$` is rewritten
/// to EXIF form; the bare-date `tr/-/:/` branch at `XMP.pm:3390-3392` is
/// explicitly skipped (`not $unsure`), and anything else passes through
/// unchanged.
fn convert_xmp_date_unsure(val: &str) -> String {
    let b = val.as_bytes();
    let digits = |from: usize, n: usize| {
        b.get(from..from + n)
            .is_some_and(|s| s.iter().all(u8::is_ascii_digit))
    };
    let matches_shape = b.len() >= 16
        && digits(0, 4)
        && b[4] == b'-'
        && digits(5, 2)
        && b[7] == b'-'
        && digits(8, 2)
        && (b[10] == b'T' || b[10] == b' ')
        && digits(11, 2)
        && b[13] == b':'
        && digits(14, 2);
    if !matches_shape {
        return val.to_string();
    }
    let mut rest = &val[16..];
    let mut seconds = "";
    if rest.len() >= 3
        && rest.as_bytes()[0] == b':'
        && rest.as_bytes()[1].is_ascii_digit()
        && rest.as_bytes()[2].is_ascii_digit()
    {
        seconds = &rest[..3];
        rest = &rest[3..];
    }
    // `\s*(\S*)$`: optional whitespace, then a trailing run with no interior
    // whitespace.
    let trailing = rest.trim_start();
    if trailing.chars().any(char::is_whitespace) {
        return val.to_string();
    }
    format!(
        "{}:{}:{} {}{}{}",
        &val[0..4],
        &val[5..7],
        &val[8..10],
        &val[11..16],
        seconds,
        trailing
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `^manifest\d*.xml$` with the `.` unescaped (`CaptureOne.pm:131`).
    #[test]
    fn manifest_pattern_matches_like_the_perl_regex() {
        assert!(is_manifest_name("manifest.xml"));
        assert!(is_manifest_name("manifest50.xml"));
        assert!(is_manifest_name("manifest5Xxml")); // the unescaped dot
        assert!(!is_manifest_name("Manifest.xml")); // case-sensitive
        assert!(!is_manifest_name("manifest.xml.bak")); // anchored at $
        assert!(!is_manifest_name("manifestxml")); // `.` needs one char
        assert!(!is_manifest_name("amanifest.xml")); // anchored at ^
    }

    /// `next if $file and $file gt $f` keeps the byte-wise greatest name:
    /// `manifest50.xml` sorts above `manifest.xml` because `5` (0x35) is
    /// greater than `.` (0x2e).
    #[test]
    fn manifest_choice_prefers_the_greatest_name() {
        let names = ["manifest.xml", "manifest50.xml"];
        let mut best: Option<&str> = None;
        for name in names {
            if let Some(b) = best
                && b > name
            {
                continue;
            }
            best = Some(name);
        }
        assert_eq!(best, Some("manifest50.xml"));
    }

    /// The fallback member filter,
    /// `^([^/]+\.(iiq|jpe?g|tiff?)|CaptureOne/.*\.cos)$/i`
    /// (`CaptureOne.pm:171`).
    #[test]
    fn fallback_selection_mirrors_the_perl_pattern() {
        assert!(fallback_selects("0.IIQ"));
        assert!(fallback_selects("image.Jpeg"));
        assert!(fallback_selects("scan.tif"));
        assert!(fallback_selects("CaptureOne/Settings50/0.IIQ.cos"));
        assert!(fallback_selects("captureone/x.cos")); // /i on the whole match
        assert!(!fallback_selects("sub/dir/image.iiq")); // images: root only
        assert!(!fallback_selects("Other/0.IIQ.cos")); // cos: CaptureOne/ only
        assert!(!fallback_selects("manifest.xml"));
        assert!(!fallback_selects(".iiq")); // `[^/]+` needs a non-empty stem
    }

    /// `AddTagToTable`'s normalization (`ExifTool.pm:9254-9266`).
    #[test]
    fn cos_names_are_sanitized_like_add_tag_to_table() {
        assert_eq!(sanitize_cos_name("Basic_Rating"), "Basic_Rating");
        assert_eq!(sanitize_cos_name("rating"), "Rating");
        assert_eq!(sanitize_cos_name("a b"), "Ab");
        assert_eq!(sanitize_cos_name("x"), "TagX");
        assert_eq!(sanitize_cos_name("1shot"), "Tag1shot");
        assert_eq!(sanitize_cos_name("-dash"), "Tag-dash");
    }

    /// `/Date(?![a-z])/` (`CaptureOne.pm:79`).
    #[test]
    fn date_marker_requires_no_following_lowercase() {
        assert!(has_date_marker("CaptureDate"));
        assert!(has_date_marker("DateTime"));
        assert!(has_date_marker("Date"));
        assert!(!has_date_marker("Dateline")); // followed by lowercase
        assert!(!has_date_marker("Update2")); // "date" != "Date"
    }

    /// `ConvertXMPDate($val, 1)`: full datetime rewritten, everything else
    /// untouched -- including the bare date that the `$unsure == 0` branch
    /// would have rewritten (`XMP.pm:3390-3392`).
    #[test]
    fn xmp_date_conversion_in_unsure_mode() {
        assert_eq!(
            convert_xmp_date_unsure("2009-11-03T19:55:32Z"),
            "2009:11:03 19:55:32Z"
        );
        assert_eq!(
            convert_xmp_date_unsure("2009-11-03 19:55+01:00"),
            "2009:11:03 19:55+01:00"
        );
        assert_eq!(convert_xmp_date_unsure("2009-11-03"), "2009-11-03");
        assert_eq!(convert_xmp_date_unsure("not a date"), "not a date");
    }

    /// The K/V attribute swap and its guards, end to end on a COS fragment:
    /// the last same-document duplicate wins the key (the oracle reports
    /// `XML:Rotation: 90` for `t/images/CaptureOne.eip`), values are
    /// XML-unescaped (`UnescapeXML`, `CaptureOne.pm:89`), and
    /// `ColorCorrections` becomes a binary placeholder sized in bytes
    /// (`CaptureOne.pm:29`).
    #[test]
    fn cos_properties_are_swapped_in_from_k_and_v() {
        let cos = br#"<?xml version="1.0"?>
<IMG>
  <E K="UUID" V="133ACDE8"/>
  <VAR>
    <DL>
      <E K="Rotation" V="0"/>
      <E K="FilmCurve" V="A &amp; B"/>
      <E K="ColorCorrections" V="1,1,1,0"/>
    </DL>
    <AL>
      <E K="Rotation" V="90"/>
    </AL>
  </VAR>
</IMG>"#;
        let mut metadata = MetadataMap::new();
        parse_cos(cos, Instance(5), &mut metadata);

        assert_eq!(
            metadata.get("XML:UUID"),
            Some(&TagValue::new_string("133ACDE8"))
        );
        assert_eq!(
            metadata.get("XML:Rotation"),
            Some(&TagValue::new_string("90")),
            "the later same-document occurrence must win the key"
        );
        assert_eq!(
            metadata.get("XML:FilmCurve"),
            Some(&TagValue::new_string("A & B"))
        );
        assert_eq!(
            metadata.get("XML:ColorCorrections"),
            Some(&TagValue::new_binary(b"1,1,1,0".to_vec()))
        );
        // Structural elements never become tags.
        assert!(metadata.get("XML:IMG").is_none());
        assert!(metadata.get("XML:VAR").is_none());
        assert!(metadata.get("XML:DL").is_none());
    }

    /// A member `by_index` refuses (here: AES-encrypted, no password) must
    /// not shift later members' positions: names come from `name_for_index`,
    /// so member *i* of the walk is always archive entry *i*. With the old
    /// `filter_map(by_index(..).ok())` enumeration the encrypted first entry
    /// vanished from the list, `read_member` then read entry 0 (the locked
    /// member) under the COS member's name, and the COS tags were silently
    /// lost. Archive::Zip enumerates every entry by name and only fails at
    /// `contents()` time (`CaptureOne.pm:157,175-176`), which is what this
    /// pins.
    #[test]
    fn an_unreadable_member_does_not_shift_the_walk() {
        use std::io::Write;
        use zip::write::{SimpleFileOptions, ZipWriter};

        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "locked.bin",
                SimpleFileOptions::default().with_aes_encryption(zip::AesMode::Aes256, "pw"),
            )
            .unwrap();
        writer.write_all(b"sealed").unwrap();
        writer
            .start_file("CaptureOne/x.cos", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(br#"<IMG><E K="Rotation" V="90"/></IMG>"#)
            .unwrap();
        let bytes = writer.finish().unwrap().into_inner();

        let reader = BufferedReader::from_bytes(&bytes);
        let metadata = parse(&reader).expect("EIP parse");
        assert_eq!(
            metadata.get("XML:Rotation"),
            Some(&TagValue::new_string("90")),
            "the COS member after an unreadable entry must still be read as itself"
        );
    }

    /// A duplicate arriving from a *different* COS document must not
    /// displace the first document's winner -- the other half of
    /// `ExifTool.pm:9564`'s guard.
    #[test]
    fn a_later_cos_document_does_not_displace_the_first() {
        let mut metadata = MetadataMap::new();
        parse_cos(
            br#"<IMG><E K="Rotation" V="0"/></IMG>"#,
            Instance(4),
            &mut metadata,
        );
        parse_cos(
            br#"<IMG><E K="Rotation" V="90"/></IMG>"#,
            Instance(5),
            &mut metadata,
        );
        assert_eq!(
            metadata.get("XML:Rotation"),
            Some(&TagValue::new_string("0"))
        );
    }
}
