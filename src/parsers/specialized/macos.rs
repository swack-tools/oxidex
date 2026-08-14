//! MacOS `._` sidecar (AppleDouble) metadata parser.
//!
//! On filesystems that cannot store MacOS forks natively, MacOS writes a
//! companion file whose name begins with `._`. ExifTool 13.59 reads these
//! through `MacOS::ProcessMacOS` (MacOS.pm:700-726): a 26-byte AppleDouble
//! header, an entry table of `(id, offset, length)` triples, and then
//! `MacOS::Main` (MacOS.pm:31-50), which declares exactly two entry IDs --
//! `2` (the resource fork) and `9` (the extended-attribute block).
//!
//! Entry 9 is handed to `ProcessATTR` (MacOS.pm:655-693), which walks a
//! per-attribute directory and names each value through
//! `ReadXAttrValue` (MacOS.pm:545-581) against `MacOS::XAttr`
//! (MacOS.pm:245-348). That is the whole of what this parser implements.
//!
//! # Deliberately absent
//!
//! * **Entry 2, the resource fork** (MacOS.pm:39-42, `SubDirectory` into
//!   `RSRC::Main`). The corpus's only `._` file carries a 286-byte resource
//!   fork from which the pinned oracle reports *no* tags, so there is nothing
//!   here to measure a `RSRC::Main` port against, and an unmeasured one is
//!   what `AGENTS.md` calls a coverage lie.
//! * **`com.apple.FinderInfo`** (MacOS.pm:256-291). Its value is built by a
//!   `ValueConv` `unpack` feeding a `PrintConv` that re-splits the result and
//!   runs `DecodeBits` over a fabricated 32-bit word. Both halves are fully
//!   specified in the Perl, but no file in the corpus carries the attribute,
//!   so a transcription of them could not be checked against the oracle even
//!   once. Following the MRC precedent in `AGENTS.md`, the tag is omitted
//!   rather than emitted unverified.
//! * **`MacOS::MDItem`** (MacOS.pm:53-243) and the rest of `MacOS::XAttr`'s
//!   write path. `MDItem` tags come from shelling out to `mdls`
//!   (MacOS.pm's `ExtractMDItemTags`), not from any file, and ExifTool only
//!   runs it on MacOS.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/MacOS.pm`, `lib/Image/ExifTool/PLIST.pm`

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::io::timestamp::unix_time_to_local_exif_datetime;
use crate::parsers::tiff::makernotes::shared::binary_plist::{PlistValue, parse as parse_plist};

/// MacOS.pm:706, `$hdr =~ /^\0\x05\x16\x07\0(.)\0\0Mac OS X        /s`.
const HEADER_LEN: usize = 26;
const MAGIC_PREFIX: &[u8] = b"\0\x05\x16\x07\0";
const MAGIC_SUFFIX: &[u8] = b"\0\0Mac OS X        ";
/// MacOS.pm:710, `$ver == 2 or $et->Warn(...), return 1`.
const SUPPORTED_VERSION: u8 = 2;
/// MacOS.pm:39-49: the two entry IDs `MacOS::Main` declares.
const ENTRY_ATTR: u32 = 9;
/// MacOS.pm:721, `$len > 100000000 and $et->Warn('Record size too large')`.
const MAX_RECORD: u32 = 100_000_000;
/// MacOS.pm:576: the length past which a plain string becomes binary.
const MAX_TEXT_LEN: usize = 200;
/// PLIST.pm:279, `$val + 11323 * 24 * 3600` -- the CFDate epoch (2001-01-01)
/// expressed as an offset from the Unix epoch.
const CFDATE_EPOCH_OFFSET: f64 = 11323.0 * 24.0 * 3600.0;

fn u32_be(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

/// Extract MacOS `._` sidecar metadata.
pub fn parse_macos_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let header = reader
        .read(0, HEADER_LEN)
        .map_err(|error| error.to_string())?;
    if !header.starts_with(MAGIC_PREFIX) || &header[6..24] != MAGIC_SUFFIX {
        return Err("not a MacOS ._ sidecar file".to_string());
    }
    let mut metadata = MetadataMap::new();
    // MacOS.pm:710: an unsupported version is a warning and stops extraction,
    // but the file is still accepted.
    if header[5] != SUPPORTED_VERSION {
        return Ok(metadata);
    }

    // MacOS.pm:711-715, `SetByteOrder('MM')` then `Get16u(\$hdr, 0x18)`.
    let entries = u32::from(u16::from_be_bytes([header[0x18], header[0x19]]));
    let Ok(table) = reader.read(HEADER_LEN as u64, entries as usize * 12) else {
        // MacOS.pm:715, `$et->Warn('Truncated header'), return 1`.
        return Ok(metadata);
    };

    for index in 0..entries as usize {
        let pos = index * 12;
        let (Some(tag), Some(offset), Some(len)) = (
            u32_be(&table, pos),
            u32_be(&table, pos + 4),
            u32_be(&table, pos + 8),
        ) else {
            break;
        };
        if len > MAX_RECORD {
            break;
        }
        let Ok(record) = reader.read(u64::from(offset), len as usize) else {
            // MacOS.pm:722, `$et->Warn('Truncated record'), last`.
            break;
        };
        // MacOS.pm:39-49: only these two IDs have a table entry, and
        // `HandleTag` on an unlisted ID does nothing. Entry 2 (RSRC) is
        // deliberately not implemented -- see the module docs.
        if tag == ENTRY_ATTR {
            process_attr(record, offset, &mut metadata);
        }
    }
    Ok(metadata)
}

/// `ProcessATTR` (MacOS.pm:655-693).
fn process_attr(data: &[u8], data_pos: u32, metadata: &mut MetadataMap) {
    // MacOS.pm:662, `$dataLen >= 58 and $$dataPt =~ /^.{34}ATTR/s`.
    if data.len() < 58 || data.get(34..38) != Some(b"ATTR".as_slice()) {
        return;
    }
    let Some(entries) = u32_be(data, 66) else {
        return;
    };

    let mut pos = 70usize;
    for _ in 0..entries {
        // MacOS.pm:671.
        if pos + 12 > data.len() {
            break;
        }
        let (Some(offset), Some(len)) = (u32_be(data, pos), u32_be(data, pos + 4)) else {
            break;
        };
        let name_len = usize::from(data[pos + 10]);
        // MacOS.pm:675.
        if pos + 11 + name_len > data.len() {
            break;
        }
        // MacOS.pm:676: the ATTR block stores *absolute* file offsets, so
        // each one is rebased onto this record.
        let Some(offset) = offset.checked_sub(data_pos) else {
            // MacOS.pm:677 means to abort here. Its `$off < 0 or $off >
            // $dataLen and ...` parses as `$off < 0 or ($off > $dataLen and
            // ...)`, so Perl actually falls through on a negative offset and
            // then indexes from the end of the buffer. That is a precedence
            // slip, not a decoding rule, and reproducing it would be
            // inventing semantics -- so this stops instead.
            break;
        };
        let offset = offset as usize;
        if offset > data.len() {
            break;
        }

        // MacOS.pm:678-681.
        let raw_name = &data[pos + 11..pos + 11 + name_len];
        let name_end = raw_name
            .iter()
            .rposition(|byte| *byte != 0)
            .map_or(0, |last| last + 1);
        let mut attribute = String::from_utf8_lossy(&raw_name[..name_end]).into_owned();
        // MacOS.pm:681: the random suffix on a kMDLabel attribute is dropped
        // so the ID matches the table entry.
        if attribute.starts_with("com.apple.metadata:kMDLabel_") {
            attribute = "com.apple.metadata:kMDLabel".to_string();
        }

        // MacOS.pm:682.
        if offset + len as usize > data.len() {
            break;
        }
        let value = &data[offset..offset + len as usize];
        if let Some((name, tag_value)) = read_xattr_value(&attribute, value) {
            metadata.insert(format!("MacOS:{name}"), tag_value);
        }

        // MacOS.pm:690, `$pos += (11 + $n + 3) & -4`.
        pos += (11 + name_len + 3) & !3;
    }
}

/// The declared entries of `%Image::ExifTool::MacOS::XAttr` (MacOS.pm:245-348)
/// that this parser implements, as `(attribute id, tag name)`.
///
/// `com.apple.FinderInfo` (MacOS.pm:256-291) is deliberately absent; see the
/// module docs.
const XATTR: &[(&str, &str)] = &[
    ("com.apple.quarantine", "XAttrQuarantine"), // MacOS.pm:292-311
    (
        "com.apple.metadata:com_apple_mail_dateReceived",
        "XAttrAppleMailDateReceived",
    ), // MacOS.pm:312-315
    (
        "com.apple.metadata:com_apple_mail_dateSent",
        "XAttrAppleMailDateSent",
    ), // MacOS.pm:316-319
    (
        "com.apple.metadata:com_apple_mail_isRemoteAttachment",
        "XAttrAppleMailIsRemoteAttachment",
    ), // MacOS.pm:320-322
    (
        "com.apple.metadata:kMDItemDownloadedDate",
        "XAttrMDItemDownloadedDate",
    ), // MacOS.pm:323-326
    (
        "com.apple.metadata:kMDItemFinderComment",
        "XAttrMDItemFinderComment",
    ), // MacOS.pm:327
    (
        "com.apple.metadata:kMDItemWhereFroms",
        "XAttrMDItemWhereFroms",
    ), // MacOS.pm:328-338
    ("com.apple.metadata:kMDLabel", "XAttrMDLabel"), // MacOS.pm:339
    ("com.apple.ResourceFork", "XAttrResourceFork"), // MacOS.pm:340
    ("com.apple.lastuseddate#PS", "XAttrLastUsedDate"), // MacOS.pm:341-347
];

/// The two `MacOS::XAttr` entries declaring `Binary => 1` (MacOS.pm:339-340).
const BINARY_TAGS: &[&str] = &["XAttrMDLabel", "XAttrResourceFork"];

/// `ReadXAttrValue` (MacOS.pm:545-581) plus the per-tag conversions
/// `MacOS::XAttr` declares. Returns `None` where ExifTool suppresses the tag.
fn read_xattr_value(attribute: &str, value: &[u8]) -> Option<(String, TagValue)> {
    let name = xattr_name(attribute);

    // MacOS.pm:341-346's `RawConv => 'ConvertUnixTime(unpack("V",$$val))'`.
    // It runs on the raw attribute bytes, ahead of the bplist branch.
    if name == "XAttrLastUsedDate" {
        let seconds = u32::from_le_bytes(value.get(..4)?.try_into().ok()?);
        return Some((name, TagValue::new_string(convert_unix_time_utc(seconds))));
    }

    // MacOS.pm:565-575: a `bplist0`-prefixed value is decoded, and a
    // dictionary top object suppresses the tag entirely.
    let decoded = if value.starts_with(b"bplist0") {
        match parse_plist(value) {
            Some(PlistValue::Dict(_)) | None => return None,
            Some(plist) => XAttrValue::Plist(plist),
        }
    } else {
        XAttrValue::Bytes(value)
    };

    // MacOS.pm:339-340's `Binary => 1`: ExifTool prints the placeholder for
    // these whatever the value decoded to.
    if BINARY_TAGS.contains(&name.as_str()) {
        return Some((name, TagValue::Binary(value.to_vec())));
    }

    let text = match decoded {
        XAttrValue::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        // MacOS.pm:565-570: a plist array stays an ARRAY ref, so the
        // null/length test at MacOS.pm:576 -- guarded by `not ref $val` --
        // never applies to it, and ExifTool reports it as a list.
        XAttrValue::Plist(PlistValue::Array(items)) => {
            let mut values = Vec::with_capacity(items.len());
            for item in &items {
                values.push(TagValue::new_string(scalar_text(item)?));
            }
            return Some((name, TagValue::Array(values)));
        }
        XAttrValue::Plist(plist) => scalar_text(&plist)?,
    };

    // MacOS.pm:576-579: a plain string with an embedded null, or longer than
    // 200 bytes, becomes binary.
    if text.as_bytes().contains(&0) || text.len() > MAX_TEXT_LEN {
        return Some((name, TagValue::Binary(text.into_bytes())));
    }

    if name == "XAttrQuarantine" {
        return Some((name, TagValue::new_string(quarantine(&text))));
    }
    Some((name, TagValue::new_string(text)))
}

enum XAttrValue<'a> {
    Bytes(&'a [u8]),
    Plist(PlistValue),
}

fn scalar_text(value: &PlistValue) -> Option<String> {
    match value {
        // PLIST.pm:275-280: a date renders through `ConvertUnixTime($val +
        // 11323 * 24 * 3600, 1)`, i.e. local time with a zone suffix.
        PlistValue::Date(seconds) => {
            let unix = seconds + CFDATE_EPOCH_OFFSET;
            Some(unix_time_to_local_exif_datetime(unix as i64))
        }
        other => other.scalar(),
    }
}

/// `ReadXAttrValue`'s tag naming (MacOS.pm:549-563):
///
/// ```text
/// if ($tag =~ /^com\.apple\.(.*)$/) {
///     ($name = $1) =~ s/^metadata:_?k//;
///     $name =~ s/^metadata:(com_)?//;
/// } else {
///     $name = $tag;
/// }
/// $name =~ s/[.:_]([a-z])/\U$1/g;
/// $name = 'XAttr' . ucfirst $name;
/// ```
///
/// then `AddTagToTable`'s `tr/-_a-zA-Z0-9//dc` (ExifTool.pm:9256), which is
/// what strips the `:` out of `XAttrOrgExiftoolMetadata:TestTag`.
fn xattr_name(attribute: &str) -> String {
    if let Some(name) = lookup_declared(attribute) {
        return name.to_string();
    }
    let mut name = match attribute.strip_prefix("com.apple.") {
        Some(rest) => {
            let rest = rest
                .strip_prefix("metadata:_k")
                .or_else(|| rest.strip_prefix("metadata:k"))
                .unwrap_or(rest);
            let rest = rest
                .strip_prefix("metadata:com_")
                .or_else(|| rest.strip_prefix("metadata:"))
                .unwrap_or(rest);
            rest.to_string()
        }
        None => attribute.to_string(),
    };

    // `s/[.:_]([a-z])/\U$1/g`.
    let mut out = String::with_capacity(name.len());
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        if matches!(c, '.' | ':' | '_')
            && let Some(next) = chars.peek().copied()
            && next.is_ascii_lowercase()
        {
            chars.next();
            out.extend(next.to_uppercase());
            continue;
        }
        out.push(c);
    }
    name = out;

    // `'XAttr' . ucfirst $name`.
    let mut chars = name.chars();
    let mut result = String::from("XAttr");
    if let Some(first) = chars.next() {
        result.extend(first.to_uppercase());
    }
    result.push_str(chars.as_str());

    // ExifTool.pm:9256, `tr/-_a-zA-Z0-9//dc`.
    result
        .chars()
        .filter(|c| *c == '-' || *c == '_' || c.is_ascii_alphanumeric())
        .collect()
}

fn lookup_declared(attribute: &str) -> Option<&'static str> {
    XATTR
        .iter()
        .find(|(id, _)| *id == attribute)
        .map(|(_, name)| *name)
}

/// MacOS.pm:303-309's `PrintConv`:
///
/// ```text
/// my @a = split /;/, $val;
/// $a[0] = 'Flags=' . $a[0];
/// $a[1] = 'set at ' . ConvertUnixTime(hex $a[1]);
/// $a[2] = 'by ' . $a[2];
/// return join ' ', @a;
/// ```
///
/// Perl's `split` drops trailing empty fields, which is why a value ending in
/// `;` still produces exactly three parts.
fn quarantine(value: &str) -> String {
    let mut parts: Vec<String> = value.split(';').map(str::to_string).collect();
    while parts.last().is_some_and(String::is_empty) {
        parts.pop();
    }
    if let Some(flags) = parts.first_mut() {
        *flags = format!("Flags={flags}");
    }
    if let Some(at) = parts.get_mut(1) {
        // `hex $a[1]` -- Perl's `hex` reads leading hex digits and yields 0
        // for anything it cannot read.
        let seconds = u32::from_str_radix(
            at.trim_start_matches("0x")
                .trim_end_matches(|c: char| !c.is_ascii_hexdigit()),
            16,
        )
        .unwrap_or(0);
        *at = format!("set at {}", convert_unix_time_utc(seconds));
    }
    if let Some(by) = parts.get_mut(2) {
        *by = format!("by {by}");
    }
    parts.join(" ")
}

/// `ConvertUnixTime($val)` with no `$toLocal` (ExifTool.pm:6798-6800): UTC,
/// and no time-zone suffix.
fn convert_unix_time_utc(seconds: u32) -> String {
    if seconds == 0 {
        // ExifTool.pm:6787.
        return "0000:00:00 00:00:00".to_string();
    }
    match chrono::DateTime::from_timestamp(i64::from(seconds), 0) {
        Some(utc) => utc.format("%Y:%m:%d %H:%M:%S").to_string(),
        None => "0000:00:00 00:00:00".to_string(),
    }
}
