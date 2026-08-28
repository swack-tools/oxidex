//! Apple Property List (PLIST) parser, mirroring ExifTool's `PLIST.pm`.
//!
//! ExifTool reads both encodings of a plist through one tag table,
//! `%Image::ExifTool::PLIST::Main` (PLIST.pm, pinned 13.59):
//!
//! * **XML** plists go through the XMP parser with `PLIST.pm`'s `FoundTag`
//!   as the found-proc: `<key>` values build a `/`-joined tag ID path, and
//!   every value element (`string`, `integer`, `real`, `date`, `data`,
//!   `true`, `false`) stores one tag. Family 1 group: `XML`.
//! * **Binary** plists (`bplist00`) are walked object-by-object by
//!   `ProcessBinaryPLIST`/`ExtractObject`: dict keys build the same
//!   `/`-joined IDs. Family 1 group: `PLIST`.
//!
//! Tag IDs the table does not declare get a generated name
//! (`s/([^A-Za-z])([a-z])/$1\u$2/g`, strip illegal characters, `ucfirst`),
//! exactly as ExifTool generates them on the fly. Declared IDs use the
//! table's name -- that is where `slowMotion/regions/timeRange/start/value`
//! becomes `SlowMotionRegionsStartTimeValue` in an Apple `.aae` sidecar.
//!
//! The `adjustmentData` entry is a `SubDirectory` back into the same table
//! (`CompressedPLIST`): its base64 payload is itself a plist (raw-deflated
//! unless it already starts with `bplist00`), and its tags land in the
//! `PLIST` family-1 group.

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// Binary plist magic (`bplist00`; `bplist01` is read identically).
const BPLIST_MAGIC: &[u8] = b"bplist0";

/// XML plist identifiers.
const XML_DECLARATION: &[u8] = b"<?xml";
const PLIST_TAG: &[u8] = b"<plist";
const DOCTYPE_PLIST: &[u8] = b"<!DOCTYPE plist";

/// How `%Image::ExifTool::PLIST::Main` answers one tag ID.
enum KnownTag {
    /// A plain rename: `'slowMotion/rate' => 'SlowMotionRate'`.
    Name(&'static str),
    /// The two slow-motion flag tags, whose `PrintConv` is a `BITMASK`
    /// (`0 => 'Valid', 1 => 'Has been rounded', 2 => 'Positive infinity',
    /// 3 => 'Negative infinity', 4 => 'Indefinite'`).
    SlowMotionFlags(&'static str),
    /// A declared tag whose conversion this parser refuses to approximate;
    /// see `known_tag` for the per-entry PLIST.pm citations. The tag is
    /// omitted rather than emitted with a raw (wrong) value under the real
    /// ExifTool name.
    Omitted,
}

/// `%Image::ExifTool::PLIST::Main`'s declared tag IDs (PLIST.pm). Everything
/// else gets the generated-name treatment, same as ExifTool.
fn known_tag(tag: &str) -> Option<KnownTag> {
    use KnownTag::*;
    Some(match tag {
        // slow motion stuff found in AAE files
        "slowMotion/regions/timeRange/start/flags" => {
            SlowMotionFlags("SlowMotionRegionsStartTimeFlags")
        }
        "slowMotion/regions/timeRange/start/value" => Name("SlowMotionRegionsStartTimeValue"),
        "slowMotion/regions/timeRange/start/timescale" => Name("SlowMotionRegionsStartTimeScale"),
        "slowMotion/regions/timeRange/start/epoch" => Name("SlowMotionRegionsStartTimeEpoch"),
        "slowMotion/regions/timeRange/duration/flags" => {
            SlowMotionFlags("SlowMotionRegionsDurationFlags")
        }
        "slowMotion/regions/timeRange/duration/value" => Name("SlowMotionRegionsDurationValue"),
        "slowMotion/regions/timeRange/duration/timescale" => {
            Name("SlowMotionRegionsDurationTimeScale")
        }
        "slowMotion/regions/timeRange/duration/epoch" => Name("SlowMotionRegionsDurationEpoch"),
        "slowMotion/regions" => Name("SlowMotionRegions"),
        "slowMotion/rate" => Name("SlowMotionRate"),
        // buried deep in live photo .mov file
        "SystemVersion/ProductBuildVersion" => Name("ProductBuildVersion"),
        "SystemVersion/ProductName" => Name("ProductName"),
        "SystemVersion/ProductVersion" => Name("ProductVersion"),
        "FrameworkVersions/CoreMotion" => Name("CoreMotionVersion"),
        "FrameworkVersions/CMCaptureCore" => Name("CMCaptureCoreVersion"),
        "FrameworkVersions/H16ISPServices" => Name("H16ISPServicesVersion"),
        "FrameworkVersions/CoreMedia" => Name("CoreMediaVersion"),
        // tags found in PLIST information of QuickTime iTunesInfo iTunMOVI
        // atoms; the name generation below would also strip the `//name`
        // suffix, but these carry explicit table names.
        "cast//name" => Name("Cast"),
        "directors//name" => Name("Directors"),
        "producers//name" => Name("Producers"),
        "screenwriters//name" => Name("Screenwriters"),
        "codirectors//name" => Name("Codirectors"),
        "studio//name" => Name("Studio"),
        // MODD tags with conversions this parser does not reproduce. Their
        // generated names would collide with the real ExifTool names while
        // carrying unconverted values, so they are omitted and counted:
        // * `MetaDataList//DateTimeOriginal` -- Sony stores a "real": days
        //   since Dec 31 1899; ValueConv `ConvertUnixTime(($val - 25569) *
        //   24 * 3600)` (PLIST.pm).
        // * `MetaDataList//Duration` -- PrintConv `ConvertDuration($val)`
        //   on a float (PLIST.pm).
        // * `MetaDataList//Geolocation/Latitude` / `Longitude` -- PrintConv
        //   `Image::ExifTool::GPS::ToDMS(...)` (PLIST.pm).
        "MetaDataList//DateTimeOriginal"
        | "MetaDataList//Duration"
        | "MetaDataList//Geolocation/Latitude"
        | "MetaDataList//Geolocation/Longitude" => Omitted,
        "MetaDataList//Geolocation/MapDatum" => Name("GPSMapDatum"),
        _ => return None,
    })
}

/// One extracted plist value, before it becomes a `TagValue`.
#[derive(Debug, Clone, PartialEq)]
enum PlistValue {
    String(String),
    Integer(i64),
    Real(f64),
    /// Already rendered through the date conversion the source encoding
    /// calls for (`ConvertXMPDate` for XML, `ConvertUnixTime(...,1)` for
    /// binary).
    Date(String),
    Data(Vec<u8>),
    /// An array/set object's non-dict members (binary), or nothing at all
    /// (`SlowMotionRegions` is an array of dicts, which ExifTool skips,
    /// leaving `[]`).
    Array(Vec<PlistValue>),
    /// A dict whose entries were already stored as their own tags.
    DictMarker,
}

impl PlistValue {
    fn to_tag_value(&self) -> Option<TagValue> {
        Some(match self {
            PlistValue::String(s) | PlistValue::Date(s) => TagValue::String(s.clone()),
            PlistValue::Integer(i) => TagValue::Integer(*i),
            PlistValue::Real(f) => TagValue::Float(*f),
            PlistValue::Data(bytes) => TagValue::Binary(bytes.clone()),
            PlistValue::Array(items) => TagValue::Array(
                items
                    .iter()
                    .filter_map(PlistValue::to_tag_value)
                    .collect::<Vec<_>>(),
            ),
            PlistValue::DictMarker => return None,
        })
    }
}

/// Generate a tag name from an undeclared tag ID, exactly as both of
/// `PLIST.pm`'s add-on-the-fly sites do:
///
/// ```perl
/// $name =~ s/([^A-Za-z])([a-z])/$1\u$2/g; # capitalize words
/// $name =~ tr/-_a-zA-Z0-9//dc;            # remove illegal characters
/// ```
///
/// then `ucfirst`. The binary-side site additionally prefixes `Tag` when the
/// result is too short or starts with a digit or dash.
fn generate_tag_name(tag: &str, binary_side: bool) -> String {
    let mut name = String::with_capacity(tag.len());
    let mut prev_non_alpha = false;
    for (index, ch) in tag.chars().enumerate() {
        let mapped = if prev_non_alpha && ch.is_ascii_lowercase() && index > 0 {
            ch.to_ascii_uppercase()
        } else {
            ch
        };
        prev_non_alpha = !ch.is_ascii_alphabetic();
        if mapped == '-' || mapped == '_' || mapped.is_ascii_alphanumeric() {
            name.push(mapped);
        }
    }
    if binary_side
        && (name.len() < 2
            || name.starts_with('-')
            || name.starts_with(|c: char| c.is_ascii_digit()))
    {
        let mut prefixed = String::from("Tag");
        let mut chars = name.chars();
        if let Some(first) = chars.next() {
            prefixed.extend(first.to_uppercase());
            prefixed.push_str(chars.as_str());
        }
        return prefixed;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => name,
    }
}

/// The slow-motion flags `PrintConv` -- `{ BITMASK => { 0 => 'Valid', ... } }`
/// with no exact keys, so every value goes through `DecodeBits`.
fn slow_motion_flags(value: i64) -> String {
    crate::exiftool_tables::decode_bits(
        value,
        &[
            (0, "Valid"),
            (1, "Has been rounded"),
            (2, "Positive infinity"),
            (3, "Negative infinity"),
            (4, "Indefinite"),
        ],
    )
}

/// Accumulates tags for one family-1 group (`XML` or `PLIST`), reproducing
/// ExifTool's consecutive-same-ID list behaviour: PLIST tags are created
/// with `List => 1`, but `PLIST.pm` deletes the list linkage whenever the
/// next stored tag has a different ID, so only back-to-back repeats join
/// into one list.
struct PlistStore<'m> {
    metadata: &'m mut MetadataMap,
    group: &'static str,
    last_tag: Option<String>,
}

impl<'m> PlistStore<'m> {
    fn new(metadata: &'m mut MetadataMap, group: &'static str) -> Self {
        Self {
            metadata,
            group,
            last_tag: None,
        }
    }

    fn store(&mut self, tag: &str, value: PlistValue, binary_side: bool) {
        let name = match known_tag(tag) {
            Some(KnownTag::Name(name)) => name.to_string(),
            Some(KnownTag::SlowMotionFlags(name)) => {
                let rendered = match &value {
                    PlistValue::Integer(i) => slow_motion_flags(*i),
                    // A non-integer under a BITMASK PrintConv would render
                    // as-is in ExifTool; pass the raw value through.
                    other => {
                        let key = format!("{}:{name}", self.group);
                        if let Some(tag_value) = other.to_tag_value() {
                            self.insert(&key, tag_value, tag);
                        }
                        return;
                    }
                };
                let key = format!("{}:{name}", self.group);
                self.insert(&key, TagValue::String(rendered), tag);
                return;
            }
            Some(KnownTag::Omitted) => return,
            None => generate_tag_name(tag, binary_side),
        };
        if name.is_empty() {
            return;
        }
        let key = format!("{}:{name}", self.group);
        if let Some(tag_value) = value.to_tag_value() {
            self.insert(&key, tag_value, tag);
        }
    }

    fn insert(&mut self, key: &str, value: TagValue, tag: &str) {
        let consecutive = self.last_tag.as_deref() == Some(tag);
        self.last_tag = Some(tag.to_string());
        if consecutive {
            // Join with the existing value as a list, ExifTool-style.
            match self.metadata.remove(key) {
                Some(TagValue::Array(mut items)) => {
                    items.push(value);
                    self.metadata
                        .insert(key.to_string(), TagValue::Array(items));
                }
                Some(existing) => {
                    self.metadata
                        .insert(key.to_string(), TagValue::Array(vec![existing, value]));
                }
                None => {
                    self.metadata.insert(key.to_string(), value);
                }
            }
        } else if !self.metadata.contains_key(key) {
            self.metadata.insert(key.to_string(), value);
        }
    }
}

// ---------------------------------------------------------------------------
// XML plist
// ---------------------------------------------------------------------------

/// `Image::ExifTool::XMP::UnescapeXML`'s entity set: the five named XML
/// entities plus numeric character references.
fn unescape_xml(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[1..end];
        let replacement: Option<String> = match entity {
            "amp" => Some("&".to_string()),
            "lt" => Some("<".to_string()),
            "gt" => Some(">".to_string()),
            "quot" => Some("\"".to_string()),
            "apos" => Some("'".to_string()),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| {
                    entity
                        .strip_prefix('#')
                        .and_then(|dec| dec.parse::<u32>().ok())
                })
                .and_then(char::from_u32)
                .map(String::from),
        };
        match replacement {
            Some(s) => {
                out.push_str(&s);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `Image::ExifTool::XMP::ConvertXMPDate`:
///
/// ```perl
/// if ($val =~ /^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}:\d{2})(:\d{2})?\s*(\S*)$/) {
///     my $s = $5 || '';
///     $val = "$1:$2:$3 $4$s$6";
/// } elsif (not $unsure and $val =~ /^(\d{4})(-\d{2}){0,2}/) {
///     $val =~ tr/-/:/;
/// }
/// ```
fn convert_xmp_date(value: &str) -> String {
    let b = value.as_bytes();
    let full = (|| {
        if b.len() < 16 {
            return None;
        }
        let digits = |range: std::ops::Range<usize>| b[range].iter().all(u8::is_ascii_digit);
        if !(digits(0..4)
            && b[4] == b'-'
            && digits(5..7)
            && b[7] == b'-'
            && digits(8..10)
            && (b[10] == b'T' || b[10] == b' ')
            && digits(11..13)
            && b[13] == b':'
            && digits(14..16))
        {
            return None;
        }
        let mut index = 16;
        let mut seconds = "";
        if b.len() >= 19 && b[16] == b':' && digits(17..19) {
            seconds = &value[16..19];
            index = 19;
        }
        // `\s*(\S*)$`: optional whitespace, then a final run of non-space.
        let tail = value[index..].trim_start();
        if tail.contains(char::is_whitespace) {
            return None;
        }
        Some(format!(
            "{}:{}:{} {}{seconds}{tail}",
            &value[0..4],
            &value[5..7],
            &value[8..10],
            &value[11..16],
        ))
    })();
    if let Some(converted) = full {
        return converted;
    }
    // Partial dates: YYYY, YYYY-MM, YYYY-MM-DD (possibly with a suffix).
    if b.len() >= 4 && b[..4].iter().all(u8::is_ascii_digit) {
        let dash_ok = |start: usize| {
            b.len() >= start + 3 && b[start] == b'-' && digits_at(b, start + 1, start + 3)
        };
        if b.len() == 4 || dash_ok(4) {
            return value.replace('-', ":");
        }
    }
    value.to_string()
}

fn digits_at(bytes: &[u8], start: usize, end: usize) -> bool {
    bytes[start..end].iter().all(u8::is_ascii_digit)
}

/// Decode base64, ignoring anything outside the alphabet (whitespace, line
/// breaks) -- `Image::ExifTool::XMP::DecodeBase64`'s behaviour.
fn decode_base64(text: &str) -> Vec<u8> {
    fn value_of(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &byte in text.as_bytes() {
        let Some(v) = value_of(byte) else { continue };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// The XML plist walk. A deliberately small scanner rather than a full XML
/// parser: plists are a fixed, flat DTD (`plist`, `dict`, `array`, `key` and
/// seven value elements), and this mirrors how `PLIST.pm`'s `FoundTag`
/// consumes the XMP parser's property events.
fn parse_xml_plist(data: &[u8], store: &mut PlistStore<'_>) {
    let text = String::from_utf8_lossy(data);
    let mut props: Vec<&str> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    let mut content_start = 0usize;
    let mut position = 0usize;

    while let Some(open) = text[position..].find('<') {
        let open = position + open;
        // Comments, processing instructions and the DOCTYPE.
        if text[open..].starts_with("<!--") {
            match text[open..].find("-->") {
                Some(end) => {
                    position = open + end + 3;
                    continue;
                }
                None => return,
            }
        }
        if text[open..].starts_with("<?") || text[open..].starts_with("<!") {
            match text[open..].find('>') {
                Some(end) => {
                    position = open + end + 1;
                    continue;
                }
                None => return,
            }
        }
        let Some(close) = text[open..].find('>') else {
            return;
        };
        let close = open + close;
        let inner = &text[open + 1..close];
        if let Some(name) = inner.strip_prefix('/') {
            // Closing tag: a value element consumes the text since its
            // opening tag.
            let slot = name_slot(name.trim());
            if props.last().copied() == Some(slot) {
                if is_value_element(slot) || slot == "key" {
                    let raw = &text[content_start..open];
                    found_xml_tag(slot, &props, raw, &mut keys, store);
                }
                props.pop();
            }
            position = close + 1;
            content_start = position;
            continue;
        }
        let self_closing = inner.ends_with('/');
        let inner = inner.trim_end_matches('/');
        let name = inner.split_ascii_whitespace().next().unwrap_or("");
        if name.is_empty() {
            position = close + 1;
            continue;
        }
        let slot = name_slot(name);
        if self_closing {
            // `<true/>`, `<false/>`, `<data/>`, `<dict/>`, ...
            if is_value_element(slot) {
                props.push(slot);
                found_xml_tag(slot, &props, "", &mut keys, store);
                props.pop();
            }
            position = close + 1;
            content_start = position;
            continue;
        }
        props.push(slot);
        position = close + 1;
        content_start = position;
    }
}

/// Intern the element names we track so `props` can hold `&'static str`.
fn name_slot(name: &str) -> &'static str {
    match name {
        "plist" => "plist",
        "dict" => "dict",
        "array" => "array",
        "key" => "key",
        "string" => "string",
        "integer" => "integer",
        "real" => "real",
        "date" => "date",
        "data" => "data",
        "true" => "true",
        "false" => "false",
        _ => "other",
    }
}

fn is_value_element(name: &str) -> bool {
    matches!(
        name,
        "string" | "integer" | "real" | "date" | "data" | "true" | "false"
    )
}

/// `PLIST.pm`'s `FoundTag`, for one XML property event.
fn found_xml_tag(
    prop: &str,
    props: &[&str],
    raw: &str,
    keys: &mut Vec<String>,
    store: &mut PlistStore<'_>,
) {
    if prop == "key" {
        let val = unescape_xml(raw);
        // Top-level key should be plist/dict/key.
        if props.len() <= 3 {
            keys.clear();
            keys.push(val);
        } else {
            while keys.len() < props.len() - 3 {
                keys.push(String::new());
            }
            while keys.len() > props.len() - 2 {
                keys.pop();
            }
            let index = props.len() - 3;
            if index == keys.len() {
                keys.push(val);
            } else {
                keys[index] = val;
            }
        }
        return;
    }

    if keys.is_empty() {
        return; // can't store value if no associated key
    }
    let tag = keys.join("/");

    let value = match prop {
        "data" => {
            let trimmed: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
            let bytes = if !trimmed.is_empty()
                && trimmed.len().is_multiple_of(2)
                && trimmed
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                // MODD files use ASCII-hex encoded "data"...
                (0..trimmed.len())
                    .step_by(2)
                    .filter_map(|i| u8::from_str_radix(&trimmed[i..i + 2], 16).ok())
                    .collect()
            } else {
                // ...but the PLIST DTD specifies Base64 encoding.
                decode_base64(&trimmed)
            };
            // `adjustmentData` is a SubDirectory back into this same table:
            // process its contents (under the PLIST family-1 group) instead
            // of storing the blob.
            if tag == "adjustmentData" {
                process_adjustment_data(&bytes, store.metadata);
                store.last_tag = Some(tag);
                return;
            }
            PlistValue::Data(bytes)
        }
        "date" => PlistValue::Date(convert_xmp_date(unescape_xml(raw).trim())),
        "true" => PlistValue::String("True".to_string()),
        "false" => PlistValue::String("False".to_string()),
        _ => PlistValue::String(unescape_xml(raw)),
    };
    store.store(&tag, value, false);
}

/// The `CompressedPLIST` handling for `adjustmentData` (PLIST.pm): the
/// payload is raw-deflated unless it already starts with `bplist00`, and
/// then processed as a plist of its own.
fn process_adjustment_data(bytes: &[u8], metadata: &mut MetadataMap) {
    if bytes.starts_with(BPLIST_MAGIC) {
        let mut store = PlistStore::new(metadata, "PLIST");
        let _ = parse_binary_plist(bytes, &mut store);
        return;
    }
    use std::io::Read as _;
    let mut inflated = Vec::new();
    let mut decoder = flate2::read::DeflateDecoder::new(bytes);
    if decoder.read_to_end(&mut inflated).is_ok() {
        if inflated.starts_with(BPLIST_MAGIC) {
            let mut store = PlistStore::new(metadata, "PLIST");
            let _ = parse_binary_plist(&inflated, &mut store);
        } else if inflated.starts_with(b"<") {
            let mut store = PlistStore::new(metadata, "PLIST");
            parse_xml_plist(&inflated, &mut store);
        }
    }
}

// ---------------------------------------------------------------------------
// Binary plist
// ---------------------------------------------------------------------------

struct BinaryPlist<'a> {
    data: &'a [u8],
    offsets: Vec<usize>,
    ref_size: usize,
}

impl<'a> BinaryPlist<'a> {
    /// Read the big-endian unsigned integer of `size` bytes at `offset` --
    /// `%readProc`'s 1/2/3/4/8-byte forms.
    fn read_uint(&self, offset: usize, size: usize) -> Option<u64> {
        if !matches!(size, 1 | 2 | 3 | 4 | 8) {
            return None;
        }
        let bytes = self.data.get(offset..offset + size)?;
        let mut value: u64 = 0;
        for &b in bytes {
            value = (value << 8) | u64::from(b);
        }
        Some(value)
    }

    fn object_ref(&self, offset: usize, index: usize) -> Option<usize> {
        self.read_uint(offset + index * self.ref_size, self.ref_size)
            .map(|v| v as usize)
    }

    /// `ExtractObject` (PLIST.pm ref 2), at byte offset `at`. `parent` is
    /// the accumulated tag ID; dict members store their tags through
    /// `store` as they are found.
    fn extract(
        &self,
        at: usize,
        parent: Option<&str>,
        store: &mut PlistStore<'_>,
    ) -> Option<PlistValue> {
        let marker = *self.data.get(at)?;
        let object_type = marker >> 4;
        let size_nibble = (marker & 0x0f) as usize;
        let mut at = at + 1;
        match object_type {
            0 => {
                // null/bool/fill
                let value = match size_nibble {
                    0x00 => "<null>",
                    0x08 => "True",
                    0x09 => "False",
                    0x0f => "<fill>",
                    _ => return None,
                };
                Some(PlistValue::String(value.to_string()))
            }
            1 => {
                let size = 1usize << size_nibble;
                self.read_uint(at, size)
                    .map(|v| PlistValue::Integer(v as i64))
            }
            2 => {
                let size = 1usize << size_nibble;
                match size {
                    4 => {
                        let b = self.data.get(at..at + 4)?;
                        Some(PlistValue::Real(f64::from(f32::from_be_bytes([
                            b[0], b[1], b[2], b[3],
                        ]))))
                    }
                    8 => {
                        let b = self.data.get(at..at + 8)?;
                        Some(PlistValue::Real(f64::from_be_bytes(b.try_into().ok()?)))
                    }
                    _ => None,
                }
            }
            3 => {
                // Date: seconds since 2001-01-01, "11323 days from Unix time
                // zero"; `ConvertUnixTime($val + 11323 * 24 * 3600, 1)` --
                // local time with a timezone suffix.
                let size = 1usize << size_nibble;
                let seconds = match size {
                    8 => {
                        let b = self.data.get(at..at + 8)?;
                        f64::from_be_bytes(b.try_into().ok()?)
                    }
                    4 => {
                        let b = self.data.get(at..at + 4)?;
                        f64::from(f32::from_be_bytes([b[0], b[1], b[2], b[3]]))
                    }
                    _ => return None,
                };
                Some(PlistValue::Date(convert_mac_epoch(seconds)))
            }
            8 => {
                // UID
                let size = size_nibble + 1;
                if matches!(size, 1 | 2 | 3 | 4 | 8) {
                    self.read_uint(at, size)
                        .map(|v| PlistValue::Integer(v as i64))
                } else if size == 16 {
                    // ExifTool renders a 16-byte UID through ASF's GetGUID
                    // (mixed-endian GUID text); omitted here rather than
                    // approximated (PLIST.pm `ExtractObject`, type 8).
                    None
                } else {
                    let bytes = self.data.get(at..at + size)?;
                    let mut s = String::from("0x");
                    for b in bytes {
                        s.push_str(&format!("{b:02x}"));
                    }
                    Some(PlistValue::String(s))
                }
            }
            4 | 5 | 6 | 10 | 12 | 13 => {
                let mut size = size_nibble;
                if size == 0x0f {
                    // size is stored in an extra integer object
                    let extra = *self.data.get(at)?;
                    if extra >> 4 != 1 {
                        return None;
                    }
                    let int_size = 1usize << (extra & 0x0f);
                    size = self.read_uint(at + 1, int_size)? as usize;
                    at += 1 + int_size;
                }
                match object_type {
                    4 => {
                        // data
                        let bytes = self.data.get(at..at.checked_add(size)?)?;
                        Some(PlistValue::Data(bytes.to_vec()))
                    }
                    5 => {
                        // ASCII string
                        let bytes = self.data.get(at..at.checked_add(size)?)?;
                        Some(PlistValue::String(
                            String::from_utf8_lossy(bytes).into_owned(),
                        ))
                    }
                    6 => {
                        // UCS-2BE string
                        let byte_len = size.checked_mul(2)?;
                        let bytes = self.data.get(at..at.checked_add(byte_len)?)?;
                        let units: Vec<u16> = bytes
                            .chunks_exact(2)
                            .map(|c| u16::from_be_bytes([c[0], c[1]]))
                            .collect();
                        Some(PlistValue::String(String::from_utf16_lossy(&units)))
                    }
                    10 | 12 => self.extract_array(at, size, parent, store),
                    13 => self.extract_dict(at, size, parent, store),
                    _ => unreachable!(),
                }
            }
            _ => None,
        }
    }

    fn extract_array(
        &self,
        at: usize,
        count: usize,
        parent: Option<&str>,
        store: &mut PlistStore<'_>,
    ) -> Option<PlistValue> {
        let mut items = Vec::new();
        for index in 0..count {
            let reference = self.object_ref(at, index)?;
            let offset = *self.offsets.get(reference)?;
            match self.extract(offset, parent, store) {
                Some(PlistValue::DictMarker) | None => continue,
                Some(value) => items.push(value),
            }
        }
        Some(PlistValue::Array(items))
    }

    fn extract_dict(
        &self,
        at: usize,
        count: usize,
        parent: Option<&str>,
        store: &mut PlistStore<'_>,
    ) -> Option<PlistValue> {
        // prevent infinite recursion
        if parent.is_some_and(|p| p.len() > 1000) {
            return None;
        }
        for index in 0..count {
            let key_ref = self.object_ref(at, index)?;
            let value_ref = self.object_ref(at, count + index)?;
            let key_offset = *self.offsets.get(key_ref)?;
            let key = match self.extract(key_offset, None, store) {
                Some(PlistValue::String(k)) if !k.is_empty() => k,
                _ => continue, // silently ignore bad dict entries
            };
            let tag = match parent {
                Some(parent) => format!("{parent}/{key}"),
                None => key,
            };
            let value_offset = *self.offsets.get(value_ref)?;
            let Some(value) = self.extract(value_offset, Some(&tag), store) else {
                continue;
            };
            if matches!(value, PlistValue::DictMarker) {
                continue;
            }
            store.store(&tag, value, true);
        }
        Some(PlistValue::DictMarker)
    }
}

/// `ConvertUnixTime($mac_seconds + 11323 * 24 * 3600, 1)`: the Mac epoch
/// offset, then ExifTool's local-time rendering with a `±HH:MM` timezone
/// suffix (`format_unix_time_local`). Fractional seconds round to nearest
/// (`sprintf '%.0f'` semantics, ties-to-even).
fn convert_mac_epoch(mac_seconds: f64) -> String {
    let unix = mac_seconds + 11323.0 * 24.0 * 3600.0;
    let mut whole = unix.floor();
    let frac = unix - whole;
    let carry = format!("{frac:.0}");
    if carry.starts_with('1') {
        whole += 1.0;
    }
    crate::core::file_metadata::format_unix_time_local(whole as i64)
}

/// `ProcessBinaryPLIST` (PLIST.pm ref 2): trailer, offset table, then the
/// top object.
fn parse_binary_plist(data: &[u8], store: &mut PlistStore<'_>) -> Result<()> {
    if data.len() < 40 || !data.starts_with(BPLIST_MAGIC) {
        return Err(ExifToolError::parse_error(
            "File too small for binary plist format",
        ));
    }
    let trailer = &data[data.len() - 32..];
    let int_size = trailer[6] as usize;
    let ref_size = trailer[7] as usize;
    let num_objects = u64::from_be_bytes(trailer[8..16].try_into().unwrap()) as usize;
    let top_object = u64::from_be_bytes(trailer[16..24].try_into().unwrap()) as usize;
    let table_offset = u64::from_be_bytes(trailer[24..32].try_into().unwrap()) as usize;

    if top_object >= num_objects
        || !matches!(int_size, 1 | 2 | 3 | 4 | 8)
        || !matches!(ref_size, 1 | 2 | 3 | 4 | 8)
    {
        return Err(ExifToolError::parse_error("Invalid binary plist trailer"));
    }
    let table_len = int_size
        .checked_mul(num_objects)
        .ok_or_else(|| ExifToolError::parse_error("Binary plist offset table overflow"))?;
    let table = data
        .get(table_offset..table_offset + table_len)
        .ok_or_else(|| ExifToolError::parse_error("Binary plist offset table out of range"))?;

    let plist = BinaryPlist {
        data,
        offsets: Vec::new(),
        ref_size,
    };
    let mut offsets = Vec::with_capacity(num_objects);
    for index in 0..num_objects {
        let mut value: usize = 0;
        for &b in &table[index * int_size..(index + 1) * int_size] {
            value = (value << 8) | usize::from(b);
        }
        offsets.push(value);
    }
    let plist = BinaryPlist { offsets, ..plist };

    let top_offset = *plist
        .offsets
        .get(top_object)
        .ok_or_else(|| ExifToolError::parse_error("Binary plist top object out of range"))?;
    // The top object is normally a dict; whatever it is, extracting it emits
    // every reachable tag through `store`.
    let _ = plist.extract(top_offset, None, store);
    Ok(())
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// macOS Property List parser (both encodings), mirroring
/// `Image::ExifTool::PLIST::ProcessPLIST`.
pub struct PlistParser;

impl PlistParser {
    /// Signature check: binary plist magic, or an XML document containing a
    /// `<plist` root / plist DOCTYPE in its first 512 bytes.
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 8 {
            return Ok(false);
        }
        let header = reader.read(0, 8)?;
        if header.starts_with(BPLIST_MAGIC) {
            return Ok(true);
        }
        let check_size = reader.size().min(512) as usize;
        let data = reader.read(0, check_size)?;
        if data.starts_with(XML_DECLARATION)
            && (contains(data, PLIST_TAG) || contains(data, DOCTYPE_PLIST))
        {
            return Ok(true);
        }
        Ok(false)
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

impl FormatParser for PlistParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid plist signature"));
        }
        let data = reader.read(0, reader.size() as usize)?;
        let mut metadata = MetadataMap::new();

        if data.starts_with(BPLIST_MAGIC) {
            // The two encodings of a plist have different MIME types, and
            // only the XML one is in `%mimeType` -- `PLIST =>
            // 'application/xml'` (ExifTool.pm), whose own comment says the
            // binary format's 'application/x-plist' is recognized at run
            // time by `ProcessPLIST`:
            //
            //     $et->SetFileType('PLIST', 'application/x-plist');
            //
            // Only the MIME type is set here; the file type is left to the
            // identification layer, which already reports `PLIST` -- and
            // `AAE` for the Apple edit sidecars that are also plists.
            metadata.insert(
                "File:MIMEType".to_string(),
                TagValue::String("application/x-plist".to_string()),
            );
            let mut store = PlistStore::new(&mut metadata, "PLIST");
            // A bad trailer is ExifTool's "Error reading binary PLIST file":
            // the file is still typed and reported, just without content
            // tags -- so the error deliberately does not fail the read.
            let _ = parse_binary_plist(data, &mut store);
        } else {
            let mut store = PlistStore::new(&mut metadata, "XML");
            parse_xml_plist(data, &mut store);
        }
        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::Plist)
    }
}

/// Parses metadata from macOS Property List files (both encodings).
pub fn parse_plist_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    PlistParser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// The `t/images/PLIST.aae` sample verbatim: an XML plist whose
    /// `adjustmentData` carries a nested binary plist.
    const AAE_SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>adjustmentBaseVersion</key>
	<integer>0</integer>
	<key>adjustmentData</key>
	<data>
	YnBsaXN0MDDRAQJac2xvd01vdGlvbtIDBAUWV3JlZ2lvbnNUcmF0ZaEG0QcIWXRpbWVS
	YW5nZdIJCgsUVXN0YXJ0WGR1cmF0aW9u1AwNDg8QERITVWZsYWdzVXZhbHVlWXRpbWVz
	Y2FsZVVlcG9jaBABEwAAAAF3SSaQEjuaygAQANQMDQ4PEBUSExMAAAAG11VeoCI+AAAA
	CAsWGyMoKi03PEJLVFpganBye4CCi5QAAAAAAAABAQAAAAAAAAAXAAAAAAAAAAAAAAAA
	AAAAmQ==
	</data>
	<key>adjustmentEditorBundleID</key>
	<string></string>
	<key>adjustmentFormatIdentifier</key>
	<string>com.apple.video.slomo</string>
	<key>adjustmentFormatVersion</key>
	<string>1.1</string>
	<key>adjustmentRenderTypes</key>
	<integer>0</integer>
</dict>
</plist>"#;

    /// Pins the AAE decode against the pinned 13.59 oracle
    /// (`exiftool -G1 -s PLIST.aae`): five XML-group Adjustment* tags and
    /// ten PLIST-group slow-motion tags from the embedded binary plist.
    #[test]
    fn test_aae_sample_matches_oracle() {
        let reader = TestReader::new(AAE_SAMPLE.as_bytes().to_vec());
        let metadata = PlistParser.parse(&reader).unwrap();

        assert_eq!(metadata.get_string("XML:AdjustmentBaseVersion"), Some("0"));
        assert_eq!(
            metadata.get_string("XML:AdjustmentEditorBundleID"),
            Some("")
        );
        assert_eq!(
            metadata.get_string("XML:AdjustmentFormatIdentifier"),
            Some("com.apple.video.slomo")
        );
        assert_eq!(
            metadata.get_string("XML:AdjustmentFormatVersion"),
            Some("1.1")
        );
        assert_eq!(metadata.get_string("XML:AdjustmentRenderTypes"), Some("0"));

        assert_eq!(
            metadata.get_string("PLIST:SlowMotionRegionsStartTimeFlags"),
            Some("Valid")
        );
        assert_eq!(
            metadata.get("PLIST:SlowMotionRegionsStartTimeValue"),
            Some(&TagValue::Integer(6296250000))
        );
        assert_eq!(
            metadata.get("PLIST:SlowMotionRegionsStartTimeScale"),
            Some(&TagValue::Integer(1000000000))
        );
        assert_eq!(
            metadata.get("PLIST:SlowMotionRegionsStartTimeEpoch"),
            Some(&TagValue::Integer(0))
        );
        assert_eq!(
            metadata.get_string("PLIST:SlowMotionRegionsDurationFlags"),
            Some("Valid")
        );
        assert_eq!(
            metadata.get("PLIST:SlowMotionRegionsDurationValue"),
            Some(&TagValue::Integer(29382500000))
        );
        assert_eq!(
            metadata.get("PLIST:SlowMotionRegionsDurationTimeScale"),
            Some(&TagValue::Integer(1000000000))
        );
        assert_eq!(
            metadata.get("PLIST:SlowMotionRegionsDurationEpoch"),
            Some(&TagValue::Integer(0))
        );
        // The regions array holds one dict, which ExifTool skips when
        // collecting array members -- the tag is an empty list.
        assert_eq!(
            metadata.get("PLIST:SlowMotionRegions"),
            Some(&TagValue::Array(vec![]))
        );
        assert_eq!(
            metadata.get("PLIST:SlowMotionRate"),
            Some(&TagValue::Float(0.125))
        );
        // The subdirectory blob itself is not stored as a tag.
        assert_eq!(metadata.get("XML:AdjustmentData"), None);
    }

    /// XML value handling against the oracle's `PLIST-xml.plist` output:
    /// booleans, escaped strings, dates, arrays and nested-dict key paths.
    #[test]
    fn test_xml_plist_values() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
    <key>TestArray</key>
    <array>
        <string>one</string>
        <string>two</string>
        <string>three</string>
    </array>
    <key>TestBoolean</key>
    <true/>
    <key>TestData</key>
    <data>VGhpcyBpcyBhIHRlc3Q=</data>
    <key>TestDate</key>
    <date>2013-02-22T12:49:10Z</date>
    <key>TestDict</key>
    <dict>
        <key>Author</key>
        <string>Phil</string>
        <key>When</key>
        <date>2000-01-02T08:04:05Z</date>
    </dict>
    <key>TestInteger</key>
    <integer>256</integer>
    <key>TestReal</key>
    <real>1.4</real>
    <key>TestString</key>
    <string>ExifTool PLIST test</string>
    <key>TestUnicode</key>
    <string>Ex&#xee;fT&#xf6;&#xf8;l PLIST t&#xe9;st</string>
</dict>
</plist>"#;
        let reader = TestReader::new(xml.as_bytes().to_vec());
        let metadata = PlistParser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("XML:TestArray"),
            Some(&TagValue::Array(vec![
                TagValue::String("one".into()),
                TagValue::String("two".into()),
                TagValue::String("three".into()),
            ]))
        );
        assert_eq!(metadata.get_string("XML:TestBoolean"), Some("True"));
        assert_eq!(
            metadata.get("XML:TestData"),
            Some(&TagValue::Binary(b"This is a test".to_vec()))
        );
        assert_eq!(
            metadata.get_string("XML:TestDate"),
            Some("2013:02:22 12:49:10Z")
        );
        assert_eq!(metadata.get_string("XML:TestDictAuthor"), Some("Phil"));
        assert_eq!(
            metadata.get_string("XML:TestDictWhen"),
            Some("2000:01:02 08:04:05Z")
        );
        assert_eq!(metadata.get_string("XML:TestInteger"), Some("256"));
        assert_eq!(metadata.get_string("XML:TestReal"), Some("1.4"));
        assert_eq!(
            metadata.get_string("XML:TestString"),
            Some("ExifTool PLIST test")
        );
        assert_eq!(
            metadata.get_string("XML:TestUnicode"),
            Some("Ex\u{ee}fT\u{f6}\u{f8}l PLIST t\u{e9}st")
        );
    }

    #[test]
    fn test_convert_xmp_date_forms() {
        assert_eq!(
            convert_xmp_date("2013-02-22T12:49:10Z"),
            "2013:02:22 12:49:10Z"
        );
        assert_eq!(
            convert_xmp_date("2013-02-22T12:49+05:30"),
            "2013:02:22 12:49+05:30"
        );
        assert_eq!(convert_xmp_date("2013-02-22"), "2013:02:22");
        assert_eq!(convert_xmp_date("2013-02"), "2013:02");
        assert_eq!(convert_xmp_date("2013"), "2013");
        assert_eq!(convert_xmp_date("not a date"), "not a date");
    }

    #[test]
    fn test_generate_tag_name_rules() {
        // capitalize after any non-letter, strip illegal characters, ucfirst
        assert_eq!(
            generate_tag_name("adjustmentBaseVersion", false),
            "AdjustmentBaseVersion"
        );
        assert_eq!(generate_tag_name("TestDict/When", false), "TestDictWhen");
        assert_eq!(generate_tag_name("a b.c", false), "ABC");
        // the binary-side 'Tag' prefix for short or digit-leading names
        // ("1x" first capitalizes to "1X": 'x' follows the non-letter '1')
        assert_eq!(generate_tag_name("1x", true), "Tag1X");
        assert_eq!(generate_tag_name("q", true), "TagQ");
    }

    #[test]
    fn test_unescape_xml_entities() {
        assert_eq!(
            unescape_xml("a &lt;b&gt; &amp; &quot;c&quot; &#65; &#x42;"),
            "a <b> & \"c\" A B"
        );
        assert_eq!(unescape_xml("no entities"), "no entities");
        assert_eq!(unescape_xml("&unknown; stays"), "&unknown; stays");
    }

    #[test]
    fn test_slow_motion_flags_bitmask() {
        assert_eq!(slow_motion_flags(1), "Valid");
        assert_eq!(slow_motion_flags(0), "(none)");
        assert_eq!(slow_motion_flags(3), "Valid, Has been rounded");
        // an unnamed bit renders as [n], as DecodeBits does
        assert_eq!(slow_motion_flags(1 << 5), "[5]");
    }

    #[test]
    fn test_verify_signature() {
        let mut binary = b"bplist00".to_vec();
        binary.extend(vec![0u8; 40]);
        assert!(PlistParser::verify_signature(&TestReader::new(binary)).unwrap());
        assert!(
            PlistParser::verify_signature(&TestReader::new(AAE_SAMPLE.as_bytes().to_vec()))
                .unwrap()
        );
        assert!(!PlistParser::verify_signature(&TestReader::new(vec![0u8; 64])).unwrap());
    }

    #[test]
    fn test_invalid_signature_is_an_error() {
        let reader = TestReader::new(vec![0u8; 100]);
        assert!(PlistParser.parse(&reader).is_err());
    }

    #[test]
    fn test_binary_plist_sets_mime_type() {
        // A tiny but complete binary plist: top object is the ASCII string
        // "hi" -- 8-byte header, one object, offset table, 32-byte trailer.
        let mut data = b"bplist00".to_vec();
        data.extend_from_slice(&[0x52, b'h', b'i']); // object 0 at offset 8
        let table_offset = data.len() as u64;
        data.push(8); // offset table: one 1-byte entry
        let mut trailer = vec![0u8; 32];
        trailer[6] = 1; // int size
        trailer[7] = 1; // ref size
        trailer[8..16].copy_from_slice(&1u64.to_be_bytes());
        trailer[16..24].copy_from_slice(&0u64.to_be_bytes());
        trailer[24..32].copy_from_slice(&table_offset.to_be_bytes());
        data.extend(trailer);

        let reader = TestReader::new(data);
        let metadata = PlistParser.parse(&reader).unwrap();
        assert_eq!(
            metadata.get_string("File:MIMEType"),
            Some("application/x-plist")
        );
    }

    #[test]
    fn test_supports_format() {
        assert!(PlistParser.supports_format(FileFormat::Plist));
        assert!(!PlistParser.supports_format(FileFormat::SQLite));
    }
}
