//! JUMBF (JPEG Universal Metadata Box Format) parser for APP11 segments.
//!
//! JUMBF (ISO/IEC 19566-5) is the container the C2PA / CAI provenance data and
//! the JPEG XT box metadata ride in. Inside a JPEG it is carried by one or more
//! APP11 segments, each holding a numbered chunk of one box; the box only
//! becomes parseable once every chunk has been seen and concatenated.
//!
//! # APP11 chunk framing (ExifTool.pm, APP11 branch)
//!
//! An APP11 payload whose first four bytes match `JP..` and which is at least
//! 16 bytes long is a JUMBF chunk:
//!
//! ```text
//! offset  size  description
//! 0       4     "JP" + 2-byte box instance number
//! 4       4     sequence number                  (big-endian)
//! 8       4     box length   \  the first 8 bytes of the reassembled box
//! 12      4     box type     /  (4-character code, e.g. "jumb")
//! 16      ...   chunk payload
//! ```
//!
//! Everything is big-endian. Chunks are grouped by box type and indexed by
//! sequence number; once the accumulated payload plus the box header equals the
//! declared length, the box is complete and gets walked.
//!
//! Two quirks are reproduced from ExifTool:
//!
//! - a Microsoft encoder writes the length and type little-endian, which shows
//!   up as the type reading `bmuj`; the type is corrected to `jumb` and the
//!   length re-read little-endian;
//! - a declared length of 1 means the real length is a 64-bit value at offset
//!   16 and the box header is 16 bytes rather than 8.
//!
//! # Box contents
//!
//! Walking is the `Image::ExifTool::Jpeg2000::Main` box walk restricted to the
//! box types JUMBF uses:
//!
//! - `jumb` - superbox, contains a `jumd` plus content boxes;
//! - `jumd` - description box: content type UUID, toggles, optional label, ID
//!   and signature, and possibly a private box trailing the record;
//! - `json` / `cbor` - the payload formats C2PA stores claims and assertions
//!   in, flattened into tags by the key-path rules in
//!   `Image::ExifTool::JSON::ProcessTag`;
//! - `bfdb` / `bidb` / `c2sh` - binary description, binary data and salt hash,
//!   whose tag names come from the enclosing `jumd` label.
//!
//! All tags are reported under group 0 `JUMBF`, matching ExifTool's
//! `SET_GROUP0` in `Jpeg2000::ProcessJUMB`.
//!
//! # References
//!
//! - ISO/IEC 19566-5 - JPEG Universal Metadata Box Format
//! - `Image::ExifTool::Jpeg2000` (`ProcessJUMB`, `ProcessJUMD`, `%Main`)
//! - `Image::ExifTool::JSON` (`ProcessTag`, `FoundTag`), `Image::ExifTool::CBOR`
//! - <https://c2pa.org/specifications/>

use super::cbor::{self, CborValue};
use crate::core::{Instance, MetadataMap, TagValue};
use crate::error::Result;

/// Smallest APP11 payload that can carry a JUMBF chunk header
/// (`ExifTool.pm`: `length($$segDataPt) >= 16`).
const MIN_CHUNK_LEN: usize = 16;

/// Size of a plain (32-bit length) box header.
const BOX_HEADER_LEN: usize = 8;

/// Size of a box header that carries a 64-bit length.
const BOX_HEADER_LEN_64: usize = 16;

/// Shortest `jumd` record ExifTool will read: 16-byte type + toggles byte.
const MIN_JUMD_LEN: usize = 17;

/// Maximum box nesting accepted. C2PA manifests are a handful of levels deep;
/// this only stops a corrupt file from driving unbounded recursion.
const MAX_BOX_DEPTH: usize = 32;

/// Tag names ExifTool pre-defines in `%Image::ExifTool::CBOR::Main`, which win
/// over the name derived from the key path.
const CBOR_PREDEFINED_NAMES: &[(&str, &str)] = &[
    ("dc:title", "Title"),
    ("dc:format", "Format"),
    ("authorName", "AuthorName"),
    ("authorIdentifier", "AuthorIdentifier"),
    ("thumbnailUrl", "ThumbnailURL"),
];

/// Parses the JUMBF metadata carried by a JPEG's APP11 segments.
///
/// `app11_payloads` must be the payloads of every APP11 segment in the file, in
/// file order and with the marker and length field already stripped. Segments
/// that are not JUMBF chunks (JPEG-HDR, for instance) are ignored, and a box
/// whose chunks never complete is dropped rather than half-reported.
///
/// # Returns
///
/// A [`MetadataMap`] of `JUMBF:*` tags. An input with no JUMBF chunks yields an
/// empty map rather than an error.
pub fn parse_jumbf(app11_payloads: &[&[u8]]) -> Result<MetadataMap> {
    let mut collector = Collector::default();
    for boxed in assemble_boxes(app11_payloads) {
        collector.walk(&boxed, 0);
    }
    Ok(collector.metadata)
}

// ---------------------------------------------------------------------------
// APP11 chunk reassembly
// ---------------------------------------------------------------------------

/// A box whose chunks are still arriving.
struct PendingBox {
    /// Box type as written after the Microsoft byte-order fix.
    box_type: [u8; 4],
    /// Declared total length of the reassembled box, header included.
    declared_len: u64,
    /// 8 or 16, depending on whether the box uses a 64-bit length.
    header_len: usize,
    /// The box header bytes to prepend when the box completes.
    header: Vec<u8>,
    /// Chunk payloads by sequence number. Index 0 is pre-seeded empty because
    /// ExifTool seeds the list with `[ '' ]` and real chunks are usually
    /// numbered from 1.
    chunks: Vec<Option<Vec<u8>>>,
}

/// Reassembles complete JUMBF boxes from a file's APP11 payloads.
fn assemble_boxes(payloads: &[&[u8]]) -> Vec<Vec<u8>> {
    let mut pending: Vec<PendingBox> = Vec::new();
    let mut complete: Vec<Vec<u8>> = Vec::new();

    for payload in payloads {
        let payload = *payload;
        if payload.len() < MIN_CHUNK_LEN || !payload.starts_with(b"JP") {
            continue;
        }

        let seq = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]) as usize;
        let mut declared_len = u64::from(u32::from_be_bytes([
            payload[8],
            payload[9],
            payload[10],
            payload[11],
        ]));
        let mut box_type = [payload[12], payload[13], payload[14], payload[15]];

        // Microsoft writes the length and type little-endian, so "jumb" arrives
        // reversed. Correct both, exactly as ExifTool rewrites the header.
        if &box_type == b"bmuj" {
            box_type = *b"jumb";
            declared_len = u64::from(u32::from_le_bytes([
                payload[8],
                payload[9],
                payload[10],
                payload[11],
            ]));
        }

        // A declared length of 1 means the true length is the 64-bit value that
        // follows the type, and the box header is 16 bytes instead of 8.
        let header_len = if declared_len == 1 && payload.len() >= 24 {
            declared_len = u64::from_be_bytes([
                payload[16],
                payload[17],
                payload[18],
                payload[19],
                payload[20],
                payload[21],
                payload[22],
                payload[23],
            ]);
            BOX_HEADER_LEN_64
        } else {
            BOX_HEADER_LEN
        };

        if declared_len < header_len as u64 {
            continue; // "Invalid JUMBF segment"
        }

        // The reassembled box starts with the corrected header taken from this
        // segment, followed by every chunk payload in sequence order. A 64-bit
        // box keeps the literal 1 in its 32-bit length field and carries the
        // real length after the type, which is how the box walk recognises it.
        let mut header = Vec::with_capacity(header_len);
        if header_len == BOX_HEADER_LEN_64 {
            header.extend_from_slice(&1u32.to_be_bytes());
            header.extend_from_slice(&box_type);
            header.extend_from_slice(&declared_len.to_be_bytes());
        } else {
            header.extend_from_slice(&(declared_len as u32).to_be_bytes());
            header.extend_from_slice(&box_type);
        }

        let slot = match pending.iter().position(|p| p.box_type == box_type) {
            Some(i) => i,
            None => {
                pending.push(PendingBox {
                    box_type,
                    declared_len,
                    header_len,
                    header: header.clone(),
                    chunks: vec![Some(Vec::new())],
                });
                pending.len() - 1
            }
        };

        {
            let entry = &mut pending[slot];
            entry.declared_len = declared_len;
            entry.header_len = header_len;
            entry.header = header;
            if entry.chunks.len() <= seq {
                entry.chunks.resize_with(seq + 1, || None);
            }
            // A repeated, non-empty sequence number is a malformed file; keep
            // the first copy rather than overwriting it.
            if entry.chunks[seq].as_ref().is_some_and(|c| !c.is_empty()) {
                continue;
            }
            entry.chunks[seq] = Some(payload[BOX_HEADER_LEN + header_len..].to_vec());
        }

        // The box is complete once every sequence slot is filled and the total
        // matches the declared length.
        let entry = &pending[slot];
        let mut size = entry.header_len as u64;
        let mut filled = true;
        for chunk in &entry.chunks {
            match chunk {
                Some(c) => size += c.len() as u64,
                None => {
                    filled = false;
                    break;
                }
            }
        }
        if filled && size == entry.declared_len {
            let entry = pending.remove(slot);
            let mut buffer = entry.header;
            for chunk in entry.chunks.into_iter().flatten() {
                buffer.extend_from_slice(&chunk);
            }
            complete.push(buffer);
        }
    }

    complete
}

// ---------------------------------------------------------------------------
// Box walk
// ---------------------------------------------------------------------------

/// Accumulates tags while walking a reassembled JUMBF box.
#[derive(Default)]
struct Collector {
    metadata: MetadataMap,
    /// The tag-name prefix taken from the innermost `jumd` label, which renames
    /// the `bfdb` / `bidb` / `c2sh` tags of the surrounding superbox.
    label: Option<String>,
}

impl Collector {
    /// Records a tag under group `JUMBF`, with the first value seen winning
    /// the default (non-`-a`) view.
    ///
    /// ExifTool reports one copy of a duplicated tag name unless `-a` is
    /// given, and that copy is the first one extracted -- so a later box
    /// repeating a name (every C2PA assertion carries its own `alg` or
    /// `JUMDType`/`JUMDLabel`, for instance) must not displace the value
    /// already reported. Previously this used a `contains_key` guard that
    /// simply skipped `insert()` for every repeat, which kept the right
    /// winner but never recorded the later occurrences at all -- a file
    /// with N boxes sharing a tag name had 1 `JUMDType`/`JUMDLabel`
    /// occurrence instead of N, invisible to `-a` (Stage 4's duplicate-loss
    /// scan, `tools/exiftool-tables/duplicate_loss_scan.py`, is what caught
    /// it: `ExifTool.jpg` and `XMP.svg` each carry six JUMBF boxes but
    /// oxidex exposed only one `JUMDType`/`JUMDLabel`). `insert_occurrence`
    /// with `Priority => 0` reproduces the same "first wins" default this
    /// module has always had (`TagSink::record`'s priority-0 promotion,
    /// `ExifTool.pm:9541-9551` -- the same rule JPEG COM's `Comment` uses,
    /// `jpeg_helpers::process_com_segments`) while still recording every
    /// occurrence, so `-a` can see them all.
    fn emit(&mut self, name: &str, value: TagValue) {
        let key = format!("JUMBF:{}", name);
        self.metadata
            .insert_occurrence(key, value, 0, "JUMBF", Instance::default());
    }

    /// Walks a sequence of JUMBF boxes.
    fn walk(&mut self, data: &[u8], depth: usize) {
        if depth > MAX_BOX_DEPTH {
            return;
        }

        let mut pos = 0usize;
        while pos + BOX_HEADER_LEN <= data.len() {
            let mut len = u64::from(u32::from_be_bytes([
                data[pos],
                data[pos + 1],
                data[pos + 2],
                data[pos + 3],
            ]));
            let box_type = [data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]];

            let header_len = if len == 1 {
                if pos + BOX_HEADER_LEN_64 > data.len() {
                    break;
                }
                let mut wide = [0u8; 8];
                wide.copy_from_slice(&data[pos + 8..pos + 16]);
                len = u64::from_be_bytes(wide);
                BOX_HEADER_LEN_64
            } else {
                BOX_HEADER_LEN
            };

            // A zero length means the box runs to the end of this directory.
            if len == 0 {
                len = (data.len() - pos) as u64;
            }

            let Ok(len) = usize::try_from(len) else { break };
            if len < header_len || pos + len > data.len() {
                break;
            }

            let body = &data[pos + header_len..pos + len];
            self.process_box(&box_type, body, depth);
            pos += len;
        }
    }

    /// Dispatches one box by its 4-character type code.
    fn process_box(&mut self, box_type: &[u8; 4], body: &[u8], depth: usize) {
        match box_type {
            b"jumb" => {
                self.walk(body, depth + 1);
                // ExifTool drops the label when a superbox finishes, so a
                // sibling box cannot inherit a nested box's label.
                self.label = None;
            }
            b"jumd" => self.process_description(body, depth),
            b"json" => self.process_json(body),
            b"cbor" => self.process_cbor(body),
            b"bfdb" => {
                // ValueConv drops the leading toggles byte, strips trailing
                // NULs and turns the separator between the MIME type and the
                // optional file name into ", ".
                let Some(rest) = body.get(1..) else { return };
                let trimmed: &[u8] = match rest.iter().rposition(|&b| b != 0) {
                    Some(end) => &rest[..=end],
                    None => &[],
                };
                let text = String::from_utf8_lossy(trimmed).replacen('\0', ", ", 1);
                let name = self.suffixed_name("Type", "BinaryDataType");
                self.emit(&name, TagValue::String(text));
            }
            b"bidb" => {
                let name = self.suffixed_name("Data", "BinaryData");
                self.emit(&name, TagValue::Binary(body.to_vec()));
            }
            b"c2sh" => {
                let name = self.suffixed_name("Salt", "C2PASaltHash");
                self.emit(&name, TagValue::String(to_hex(body)));
            }
            _ => {}
        }
    }

    /// Builds the name of a label-renamed tag, or its default when the
    /// enclosing description box carried no label.
    fn suffixed_name(&self, suffix: &str, default: &str) -> String {
        match &self.label {
            Some(label) => format!("{}{}", label, suffix),
            None => default.to_string(),
        }
    }

    /// Reads a `jumd` description box (`Jpeg2000::ProcessJUMD`).
    fn process_description(&mut self, body: &[u8], depth: usize) {
        self.label = None;
        if body.len() < MIN_JUMD_LEN {
            return; // "Truncated JUMD directory"
        }

        self.emit("JUMDType", TagValue::String(format_jumd_type(&body[0..16])));

        let toggles = body[16];
        let mut pos = MIN_JUMD_LEN;

        if toggles & 0x02 != 0 {
            let Some(nul) = body[pos..].iter().position(|&b| b == 0) else {
                return; // "Missing JUMD label terminator"
            };
            let label = String::from_utf8_lossy(&body[pos..pos + nul]).into_owned();
            pos += nul + 1;
            if !label.is_empty() {
                self.label = Some(label_to_tag_name(&label));
            }
            self.emit("JUMDLabel", TagValue::String(label));
        }

        if toggles & 0x04 != 0 {
            let Some(id) = body.get(pos..pos + 4) else {
                return; // "Missing JUMD ID"
            };
            let id = u32::from_be_bytes([id[0], id[1], id[2], id[3]]);
            self.emit("JUMDID", TagValue::Integer(i64::from(id)));
            pos += 4;
        }

        if toggles & 0x08 != 0 {
            let Some(sig) = body.get(pos..pos + 32) else {
                return; // "Missing JUMD signature"
            };
            self.emit("JUMDSignature", TagValue::String(to_hex(sig)));
            pos += 32;
        }

        // A private box (a `c2sh` salt, in practice) may follow the record.
        if body.len() - pos >= BOX_HEADER_LEN {
            self.walk(&body[pos..], depth + 1);
        }
    }

    /// Flattens a `json` box.
    fn process_json(&mut self, body: &[u8]) {
        // Some writers pad the record out with NULs, which is not valid JSON.
        let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(trim_trailing_nuls(body))
        else {
            return;
        };
        let value = json_to_value(&parsed);
        self.flatten(&value, &[]);
    }

    /// Flattens a `cbor` box.
    fn process_cbor(&mut self, body: &[u8]) {
        let mut pos = 0usize;
        let Some(value) = cbor::read_value(body, &mut pos, 0) else {
            return;
        };
        self.flatten(&value, CBOR_PREDEFINED_NAMES);
    }

    /// Applies `JSON::ProcessTag` to a decoded box and emits the result.
    ///
    /// A top-level map contributes one tag path per key; a top-level array is
    /// keyed `Item0`, `Item1`, ... which is how a COSE_Sign1 signature (a
    /// four-element array) surfaces. Anything else carries no tag.
    fn flatten(&mut self, value: &CborValue, predefined: &[(&str, &str)]) {
        let mut bag = BoxTags::default();
        match value {
            CborValue::Map(entries) => {
                for (key, item) in entries {
                    process_tag(&mut bag, key, item, false, predefined);
                }
            }
            CborValue::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    process_tag(&mut bag, &format!("Item{}", index), item, false, predefined);
                }
            }
            _ => {}
        }
        for (name, values, is_list) in bag.entries {
            let value = collapse(values, is_list);
            self.emit(&name, value);
        }
    }
}

/// Tags collected from a single `json` or `cbor` box, in emission order.
///
/// Accumulation is per box on purpose: ExifTool's list tags gather the repeats
/// found while reading one box, and a name repeated by a *later* box is an
/// ordinary duplicate that the first-wins rule drops. Merging the two would
/// report, for example, eight exclusion offsets for a Pixel JPEG that has two
/// four-exclusion assertions, where ExifTool reports four.
#[derive(Default)]
struct BoxTags {
    entries: Vec<(String, Vec<TagValue>, bool)>,
}

impl BoxTags {
    fn push(&mut self, name: String, value: TagValue, is_list: bool) {
        match self
            .entries
            .iter_mut()
            .find(|(existing, _, _)| *existing == name)
        {
            Some((_, values, list)) => {
                values.push(value);
                *list |= is_list;
            }
            None => self.entries.push((name, vec![value], is_list)),
        }
    }
}

/// Turns the values collected for one tag into a single [`TagValue`].
///
/// ExifTool prints a one-element list as a bare scalar, and reports only the
/// first of several same-named non-list values.
fn collapse(values: Vec<TagValue>, is_list: bool) -> TagValue {
    let mut values = values;
    if is_list && values.len() > 1 {
        TagValue::Array(values)
    } else if values.is_empty() {
        TagValue::String(String::new())
    } else {
        values.swap_remove(0)
    }
}

/// `Image::ExifTool::JSON::ProcessTag` - expands a decoded structure into flat
/// tag paths.
fn process_tag(
    bag: &mut BoxTags,
    tag: &str,
    value: &CborValue,
    is_list: bool,
    predefined: &[(&str, &str)],
) {
    match value {
        CborValue::Map(entries) => {
            for (key, item) in entries {
                let mut path = String::from(tag);
                // ExifTool inserts an underline between a path ending in a
                // digit and a key starting with one, so the two numbers cannot
                // run together into a different name.
                if starts_with_digit(key) && ends_with_digit(tag) {
                    path.push('_');
                }
                path.push_str(&ucfirst(key));
                let path = uppercase_after_non_alpha(&path);
                process_tag(bag, &path, item, is_list, predefined);
            }
        }
        CborValue::Array(items) => {
            for item in items {
                process_tag(bag, tag, item, true, predefined);
            }
        }
        _ => found_tag(bag, tag, value, is_list, predefined),
    }
}

/// `Image::ExifTool::JSON::FoundTag` - turns a completed key path into a tag
/// name and records the value.
fn found_tag(
    bag: &mut BoxTags,
    tag: &str,
    value: &CborValue,
    is_list: bool,
    predefined: &[(&str, &str)],
) {
    let name = match predefined.iter().find(|(key, _)| *key == tag) {
        Some((_, name)) => (*name).to_string(),
        None => {
            // Colons are not legal in a tag name, so an "exif:Make" style key
            // becomes "Exif_Make".
            let mut name = tag.replace(':', "_");
            if name.len() >= 4 && name.as_bytes()[..4].eq_ignore_ascii_case(b"c2pa") {
                name.replace_range(..4, "C2PA");
            }
            make_tag_name(&name)
        }
    };
    bag.push(name, tag_value(value), is_list);
}

/// Converts a decoded JSON/CBOR scalar to a [`TagValue`].
fn tag_value(value: &CborValue) -> TagValue {
    match value {
        CborValue::Int(i) => TagValue::Integer(*i),
        CborValue::Float(f) => TagValue::Float(*f),
        CborValue::Bytes(b) => TagValue::Binary(b.clone()),
        CborValue::Text(s) => TagValue::String(s.clone()),
        // ExifTool spells the CBOR booleans "False"/"True" internally but its
        // JSON writer lowercases them, and that lowercase spelling is what the
        // value is compared against.
        CborValue::Simple(s) if s == "False" => TagValue::String("false".to_string()),
        CborValue::Simple(s) if s == "True" => TagValue::String("true".to_string()),
        CborValue::Simple(s) => TagValue::String(s.clone()),
        // Structures never reach here: process_tag expands them first.
        CborValue::Array(_) | CborValue::Map(_) => TagValue::String(String::new()),
    }
}

// ---------------------------------------------------------------------------
// Name construction
// ---------------------------------------------------------------------------

/// `Image::ExifTool::MakeTagName`.
fn make_tag_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let cleaned = ucfirst(&cleaned);
    let first = cleaned.chars().next();
    if cleaned.chars().count() < 2 || matches!(first, Some(c) if c == '-' || c.is_ascii_digit()) {
        format!("Tag{}", cleaned)
    } else {
        cleaned
    }
}

/// Turns a `jumd` label into the tag-name prefix ExifTool derives from it
/// (`Jpeg2000::ProcessJUMD`), so `c2pa.thumbnail.claim.jpeg` becomes
/// `C2PAThumbnailClaimJpeg`.
fn label_to_tag_name(label: &str) -> String {
    // Capitalise the letter following each illegal character, then drop the
    // illegal characters themselves.
    let mut capitalised = String::with_capacity(label.len());
    let chars: Vec<char> = label.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let legal = c.is_ascii_alphanumeric() || c == '-' || c == '_';
        if !legal && i + 1 < chars.len() && chars[i + 1].is_ascii_lowercase() {
            capitalised.push(c);
            capitalised.push(chars[i + 1].to_ascii_uppercase());
            i += 2;
        } else {
            capitalised.push(c);
            i += 1;
        }
    }

    let mut name: String = capitalised
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if let Some(at) = name.find("__") {
        name.replace_range(at..at + 2, "_");
    }
    let mut name = ucfirst(&name);
    if let Some(at) = name.find("C2pa") {
        name.replace_range(at..at + 4, "C2PA");
    }
    if name.chars().count() < 2 {
        name = format!("Tag{}", name);
    }
    name
}

/// `PrintConv` of the `jumd` type field: `6361636200110010...` renders as
/// `(cacb)-0011-0010-800000aa00389b71` when the first four bytes are printable.
fn format_jumd_type(bytes: &[u8]) -> String {
    let hex = to_hex(bytes);
    if hex.len() != 32 {
        return hex;
    }
    let first = &bytes[0..4];
    let head = if first.iter().all(|b| b.is_ascii_alphanumeric()) {
        format!("({})", String::from_utf8_lossy(first))
    } else {
        hex[0..8].to_string()
    };
    format!("{}-{}-{}-{}", head, &hex[8..12], &hex[12..16], &hex[16..32])
}

/// Perl's `ucfirst`.
fn ucfirst(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `s/([^a-zA-Z])([a-z])/$1\U$2/g` - capitalises a lower-case letter that
/// follows a non-letter, which is what turns `claim_generator_infoName` into
/// `claim_Generator_InfoName`.
fn uppercase_after_non_alpha(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if !c.is_ascii_alphabetic() && i + 1 < chars.len() && chars[i + 1].is_ascii_lowercase() {
            out.push(c);
            out.push(chars[i + 1].to_ascii_uppercase());
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn starts_with_digit(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn ends_with_digit(s: &str) -> bool {
    s.chars().next_back().is_some_and(|c| c.is_ascii_digit())
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

fn trim_trailing_nuls(bytes: &[u8]) -> &[u8] {
    match bytes.iter().rposition(|&b| b != 0) {
        Some(end) => &bytes[..=end],
        None => &[],
    }
}

/// Maps a parsed JSON document onto the same value model the CBOR reader
/// produces, so one flattening implementation serves both box types.
///
/// Object keys arrive in `serde_json`'s sorted order rather than document
/// order. That only matters when two different keys flatten onto the same tag
/// name, which the C2PA and schema.org records in use do not do.
fn json_to_value(value: &serde_json::Value) -> CborValue {
    match value {
        serde_json::Value::Null => CborValue::Simple("null".to_string()),
        serde_json::Value::Bool(b) => {
            CborValue::Simple(if *b { "True" } else { "False" }.to_string())
        }
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => CborValue::Int(i),
            None => CborValue::Float(n.as_f64().unwrap_or_default()),
        },
        serde_json::Value::String(s) => CborValue::Text(s.clone()),
        serde_json::Value::Array(items) => {
            CborValue::Array(items.iter().map(json_to_value).collect())
        }
        serde_json::Value::Object(entries) => CborValue::Map(
            entries
                .iter()
                .map(|(k, v)| (k.clone(), json_to_value(v)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wraps `body` in a JUMBF box header.
    fn make_box(box_type: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&((BOX_HEADER_LEN + body.len()) as u32).to_be_bytes());
        out.extend_from_slice(box_type);
        out.extend_from_slice(body);
        out
    }

    /// Builds a single-chunk APP11 payload carrying `boxed`.
    fn make_app11(boxed: &[u8], seq: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"JP\x00\x01");
        out.extend_from_slice(&seq.to_be_bytes());
        out.extend_from_slice(boxed);
        out
    }

    fn jumd(type_code: &[u8; 4], label: Option<&str>) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(type_code);
        body.extend_from_slice(&[
            0x00, 0x11, 0x00, 0x10, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
        ]);
        body.push(if label.is_some() { 0x03 } else { 0x01 });
        if let Some(label) = label {
            body.extend_from_slice(label.as_bytes());
            body.push(0);
        }
        make_box(b"jumd", &body)
    }

    #[test]
    fn assembles_a_single_chunk_box() {
        let boxed = make_box(b"jumb", &jumd(b"cacb", Some("cai")));
        let payload = make_app11(&boxed, 1);
        let boxes = assemble_boxes(&[&payload]);
        assert_eq!(boxes, vec![boxed]);
    }

    #[test]
    fn assembles_a_box_split_across_two_segments() {
        let boxed = make_box(b"jumb", &jumd(b"cacb", Some("cai")));
        let split = boxed.len() / 2;
        let mut first = Vec::from(&b"JP\x00\x01"[..]);
        first.extend_from_slice(&1u32.to_be_bytes());
        first.extend_from_slice(&boxed[..BOX_HEADER_LEN]);
        first.extend_from_slice(&boxed[BOX_HEADER_LEN..split]);
        let mut second = Vec::from(&b"JP\x00\x01"[..]);
        second.extend_from_slice(&2u32.to_be_bytes());
        second.extend_from_slice(&boxed[..BOX_HEADER_LEN]);
        second.extend_from_slice(&boxed[split..]);

        let boxes = assemble_boxes(&[&first, &second]);
        assert_eq!(boxes, vec![boxed]);
    }

    #[test]
    fn drops_an_incomplete_box() {
        let boxed = make_box(b"jumb", &jumd(b"cacb", Some("cai")));
        let mut first = Vec::from(&b"JP\x00\x01"[..]);
        first.extend_from_slice(&1u32.to_be_bytes());
        first.extend_from_slice(&boxed[..BOX_HEADER_LEN]);
        first.extend_from_slice(&boxed[BOX_HEADER_LEN..boxed.len() / 2]);
        assert!(assemble_boxes(&[&first]).is_empty());
    }

    #[test]
    fn corrects_the_microsoft_byte_order_bug() {
        let boxed = make_box(b"jumb", &jumd(b"cacb", Some("cai")));
        let mut payload = make_app11(&boxed, 1);
        // Rewrite the length little-endian and reverse the type, the way the
        // buggy encoder does.
        let len = (boxed.len() as u32).to_le_bytes();
        payload[8..12].copy_from_slice(&len);
        payload[12..16].copy_from_slice(b"bmuj");
        assert_eq!(assemble_boxes(&[&payload]), vec![boxed]);
    }

    #[test]
    fn ignores_non_jumbf_app11_payloads() {
        let hdr = b"HDR_RI ver=11\n ln0=0.0 ln1=1.0";
        assert!(assemble_boxes(&[&hdr[..]]).is_empty());
    }

    #[test]
    fn reads_description_type_and_label() {
        let boxed = make_box(b"jumb", &jumd(b"cacb", Some("cai")));
        let payload = make_app11(&boxed, 1);
        let metadata = parse_jumbf(&[&payload]).expect("parse");
        assert_eq!(
            metadata.get_string("JUMBF:JUMDType").as_deref(),
            Some("(cacb)-0011-0010-800000aa00389b71")
        );
        assert_eq!(
            metadata.get_string("JUMBF:JUMDLabel").as_deref(),
            Some("cai")
        );
    }

    #[test]
    fn flattens_a_json_box() {
        let mut body = jumd(b"json", Some("cai.location.broad"));
        body.extend_from_slice(&make_box(b"json", br#"{"location": "Salem, Oregon"}"#));
        let boxed = make_box(b"jumb", &body);
        let payload = make_app11(&boxed, 1);
        let metadata = parse_jumbf(&[&payload]).expect("parse");
        assert_eq!(
            metadata.get_string("JUMBF:Location").as_deref(),
            Some("Salem, Oregon")
        );
    }

    #[test]
    fn flattens_nested_json_structures() {
        let json = br#"{"@context":"http://schema.org/","author":[{"@type":"Person","name":"Jim Fisher"}],"copyrightHolder":{"name":"Jim Fisher"},"copyrightYear":2025}"#;
        let mut body = jumd(b"json", Some("stds.schema-org.CreativeWork"));
        body.extend_from_slice(&make_box(b"json", json));
        let payload = make_app11(&make_box(b"jumb", &body), 1);
        let metadata = parse_jumbf(&[&payload]).expect("parse");
        assert_eq!(
            metadata.get_string("JUMBF:Context").as_deref(),
            Some("http://schema.org/")
        );
        assert_eq!(
            metadata.get_string("JUMBF:AuthorType").as_deref(),
            Some("Person")
        );
        assert_eq!(
            metadata.get_string("JUMBF:AuthorName").as_deref(),
            Some("Jim Fisher")
        );
        assert_eq!(
            metadata.get_string("JUMBF:CopyrightHolderName").as_deref(),
            Some("Jim Fisher")
        );
        assert_eq!(metadata.get_integer("JUMBF:CopyrightYear"), Some(2025));
    }

    #[test]
    fn colons_in_json_keys_become_underlines() {
        let json = br#"{"@context":{"exif":"http://ns.adobe.com/exif/1.0/"},"exif:Make":"LEICA CAMERA AG"}"#;
        let mut body = jumd(b"json", Some("stds.exif"));
        body.extend_from_slice(&make_box(b"json", json));
        let payload = make_app11(&make_box(b"jumb", &body), 1);
        let metadata = parse_jumbf(&[&payload]).expect("parse");
        assert_eq!(
            metadata.get_string("JUMBF:Exif_Make").as_deref(),
            Some("LEICA CAMERA AG")
        );
        assert_eq!(
            metadata.get_string("JUMBF:ContextExif").as_deref(),
            Some("http://ns.adobe.com/exif/1.0/")
        );
    }

    #[test]
    fn flattens_a_cbor_box_into_a_list() {
        // {"exclusions": [{"start": 6, "length": 7831}, {"start": 7855, "length": 6239}]}
        let cbor: &[u8] = b"\xa1\x6aexclusions\x82\
\xa2\x65start\x06\x66length\x19\x1e\x97\
\xa2\x65start\x19\x1e\xaf\x66length\x19\x18\x5f";
        let mut body = jumd(b"cbor", Some("c2pa.hash.data"));
        body.extend_from_slice(&make_box(b"cbor", cbor));
        let payload = make_app11(&make_box(b"jumb", &body), 1);
        let metadata = parse_jumbf(&[&payload]).expect("parse");
        assert_eq!(
            metadata.get("JUMBF:ExclusionsStart"),
            Some(&TagValue::Array(vec![
                TagValue::Integer(6),
                TagValue::Integer(7855)
            ]))
        );
        assert_eq!(
            metadata.get("JUMBF:ExclusionsLength"),
            Some(&TagValue::Array(vec![
                TagValue::Integer(7831),
                TagValue::Integer(6239)
            ]))
        );
    }

    #[test]
    fn a_one_element_list_collapses_to_a_scalar() {
        // {"actions": [{"action": "c2pa.created"}]}
        let cbor: &[u8] = b"\xa1\x67actions\x81\xa1\x66action\x6cc2pa.created";
        let mut body = jumd(b"cbor", Some("c2pa.actions.v2"));
        body.extend_from_slice(&make_box(b"cbor", cbor));
        let payload = make_app11(&make_box(b"jumb", &body), 1);
        let metadata = parse_jumbf(&[&payload]).expect("parse");
        assert_eq!(
            metadata.get("JUMBF:ActionsAction"),
            Some(&TagValue::String("c2pa.created".to_string()))
        );
    }

    #[test]
    fn a_later_box_does_not_displace_an_earlier_value() {
        let cbor_first: &[u8] = b"\xa1\x63alg\x66sha256";
        let cbor_second: &[u8] = b"\xa1\x63alg\x64sha1";
        let mut inner = jumd(b"cbor", Some("first"));
        inner.extend_from_slice(&make_box(b"cbor", cbor_first));
        let mut outer = jumd(b"c2as", Some("c2pa.assertions"));
        outer.extend_from_slice(&make_box(b"jumb", &inner));
        let mut inner2 = jumd(b"cbor", Some("second"));
        inner2.extend_from_slice(&make_box(b"cbor", cbor_second));
        outer.extend_from_slice(&make_box(b"jumb", &inner2));
        let payload = make_app11(&make_box(b"jumb", &outer), 1);
        let metadata = parse_jumbf(&[&payload]).expect("parse");
        assert_eq!(metadata.get_string("JUMBF:Alg").as_deref(), Some("sha256"));
    }

    #[test]
    fn binary_boxes_take_their_name_from_the_label() {
        let mut body = jumd(b"\x40\xcb\x0c\x32", Some("c2pa.thumbnail.claim.jpeg"));
        body.extend_from_slice(&make_box(b"bfdb", b"\x00image/jpeg\x00"));
        body.extend_from_slice(&make_box(b"bidb", &[0xff, 0xd8, 0xff, 0xe0]));
        let payload = make_app11(&make_box(b"jumb", &body), 1);
        let metadata = parse_jumbf(&[&payload]).expect("parse");
        assert_eq!(
            metadata
                .get_string("JUMBF:C2PAThumbnailClaimJpegType")
                .as_deref(),
            Some("image/jpeg")
        );
        assert_eq!(
            metadata.get("JUMBF:C2PAThumbnailClaimJpegData"),
            Some(&TagValue::Binary(vec![0xff, 0xd8, 0xff, 0xe0]))
        );
    }

    #[test]
    fn a_top_level_cbor_array_is_keyed_by_item_index() {
        // tag(18) [ h'0102', {}, null, h'0304' ] - a COSE_Sign1 skeleton.
        let cbor: &[u8] = b"\xd2\x84\x42\x01\x02\xa0\xf6\x42\x03\x04";
        let mut body = jumd(b"c2cs", Some("c2pa.signature"));
        body.extend_from_slice(&make_box(b"cbor", cbor));
        let payload = make_app11(&make_box(b"jumb", &body), 1);
        let metadata = parse_jumbf(&[&payload]).expect("parse");
        assert_eq!(
            metadata.get("JUMBF:Item0"),
            Some(&TagValue::Binary(vec![0x01, 0x02]))
        );
        assert_eq!(metadata.get_string("JUMBF:Item2").as_deref(), Some("null"));
        assert_eq!(
            metadata.get("JUMBF:Item3"),
            Some(&TagValue::Binary(vec![0x03, 0x04]))
        );
    }

    #[test]
    fn label_becomes_a_tag_name_prefix() {
        assert_eq!(
            label_to_tag_name("c2pa.thumbnail.claim.jpeg"),
            "C2PAThumbnailClaimJpeg"
        );
        assert_eq!(label_to_tag_name("cai.rights"), "CaiRights");
        assert_eq!(
            label_to_tag_name("c2pa.hash.data.part__1"),
            "C2PAHashDataPart_1"
        );
    }

    #[test]
    fn key_paths_capitalise_after_separators() {
        assert_eq!(
            uppercase_after_non_alpha("claim_generator_infoName"),
            "claim_Generator_InfoName"
        );
        assert_eq!(uppercase_after_non_alpha("author@type"), "author@Type");
    }

    #[test]
    fn tag_names_drop_illegal_characters() {
        assert_eq!(make_tag_name("@ContextExif"), "ContextExif");
        assert_eq!(make_tag_name("exif_Make"), "Exif_Make");
        assert_eq!(make_tag_name("1"), "Tag1");
    }

    #[test]
    fn empty_input_yields_no_tags() {
        assert_eq!(parse_jumbf(&[]).expect("parse").len(), 0);
        let short: &[u8] = b"JP\x00\x01";
        assert_eq!(parse_jumbf(&[short]).expect("parse").len(), 0);
    }
}
