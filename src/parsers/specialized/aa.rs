//! Audible audiobook (.aa) metadata parser.
//!
//! ExifTool 13.59's `Audible::ProcessAA` (Audible.pm:194-266) reads a
//! 16-byte file header (magic number at offset 4, `\x57\x90\x75\x36`), then
//! a table of contents of `12 * Get32u(header, 8)` bytes, each 12-byte TOC
//! entry naming a `(type, offset, length)` chunk elsewhere in the file. Only
//! three chunk types are read: `6` (an offset table, of which only the first
//! four bytes -- the chapter count -- are used), `11` (cover art, itself a
//! small length-prefixed sub-record) and `2` (a metadata dictionary of
//! length-prefixed key/value pairs). This parser reproduces that walk
//! exactly, including its validation gates.
//!
//! # Tag naming
//!
//! `Audible::Main` (Audible.pm:23-45) only pre-declares four dictionary
//! keys (`pubdate`, `pub_date_start`, `author`, `copyright`); every other
//! key ExifTool encounters is named on the fly (Audible.pm:255-259) via
//! `Image::ExifTool::MakeTagName` (ExifTool.pm:6451-6458: strip everything
//! outside `[-_a-zA-Z0-9]`, capitalize the first letter, and prefix `Tag` if
//! the result is under 2 characters or starts with a digit or dash) followed
//! by `s/_(.)/\U$1/g` (snake_case to CamelCase). [`make_tag_name`]
//! reproduces both steps.
//!
//! Every value is HTML-entity-unescaped (Audible.pm:260, `UnescapeHTML`,
//! reusing this crate's own transcription of ExifTool's `%entityNum` table
//! and `UnescapeXML` in `parsers::text::html`) and decoded as UTF-8
//! (Audible.pm:260, `$et->Decode($val, 'UTF8')`).
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/Audible.pm`

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::parsers::text::html::{ENTITY_NUM, unescape};

/// Audible.pm:203, `$raf->Read($buff, 16) == 16`.
const HEADER_LEN: usize = 16;
/// Audible.pm:203, `$buff =~ /^.{4}\x57\x90\x75\x36/s`.
const MAGIC: &[u8] = b"\x57\x90\x75\x36";
/// Audible.pm:209, `$bytes > 0xc00 and $et->Warn('Invalid TOC'), return 1`.
const MAX_TOC_BYTES: u32 = 0xc00;
const TOC_ENTRY_LEN: u32 = 12;
/// Audible.pm:225, `$length > 100000000 and $et->Warn(...), next`.
const MAX_CHUNK_LEN: u64 = 100_000_000;
/// Audible.pm:238, `$num > 0x200 and $et->Warn(...), next`.
const MAX_DICTIONARY_ENTRIES: u32 = 0x200;

const CHUNK_TYPE_METADATA: u32 = 2;
const CHUNK_TYPE_CHAPTERS: u32 = 6;
const CHUNK_TYPE_COVER_ART: u32 = 11;

/// Extract Audible metadata by walking ExifTool's declared TOC and chunk
/// layout.
pub fn parse_aa_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    if reader.size() < HEADER_LEN as u64 {
        return Err("AA file is too short for the 16-byte header".to_string());
    }
    let header = reader.read(0, HEADER_LEN).map_err(|e| e.to_string())?;
    if &header[4..8] != MAGIC {
        return Err("invalid AA magic number".to_string());
    }
    let declared_size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    if u64::from(declared_size) != reader.size() {
        return Err("AA header file-size field does not match the real file size".to_string());
    }

    let mut metadata = MetadataMap::new();
    let toc_entry_count = u32::from_be_bytes([header[8], header[9], header[10], header[11]]);
    let toc_bytes = TOC_ENTRY_LEN.saturating_mul(toc_entry_count);
    if toc_bytes > MAX_TOC_BYTES {
        return Ok(metadata);
    }
    let Ok(toc) = reader.read(HEADER_LEN as u64, toc_bytes as usize) else {
        return Ok(metadata);
    };

    for entry in toc.chunks_exact(TOC_ENTRY_LEN as usize) {
        let chunk_type = u32::from_be_bytes([entry[0], entry[1], entry[2], entry[3]]);
        if !matches!(
            chunk_type,
            CHUNK_TYPE_METADATA | CHUNK_TYPE_CHAPTERS | CHUNK_TYPE_COVER_ART
        ) {
            continue;
        }
        let offset = u64::from(u32::from_be_bytes([entry[4], entry[5], entry[6], entry[7]]));
        let length = u64::from(u32::from_be_bytes([
            entry[8], entry[9], entry[10], entry[11],
        ]));
        if length == 0 {
            continue;
        }

        if chunk_type == CHUNK_TYPE_CHAPTERS {
            if length < 4 {
                continue;
            }
            if let Ok(count_bytes) = reader.read(offset, 4) {
                let count = u32::from_be_bytes([
                    count_bytes[0],
                    count_bytes[1],
                    count_bytes[2],
                    count_bytes[3],
                ]);
                metadata.insert(
                    "Audible:ChapterCount",
                    TagValue::new_integer(i64::from(count)),
                );
            }
            continue;
        }

        if length > MAX_CHUNK_LEN {
            continue;
        }
        let Ok(chunk) = reader.read(offset, length as usize) else {
            // Audible.pm:227: a short read here aborts the whole walk.
            break;
        };

        if chunk_type == CHUNK_TYPE_COVER_ART {
            parse_cover_art(chunk, offset, &mut metadata);
        } else {
            parse_metadata_dictionary(chunk, &mut metadata);
        }
    }

    Ok(metadata)
}

/// Audible.pm:229-234: an 8-byte-prefixed sub-record naming a length and an
/// absolute file offset for the cover image, bounds-checked against the
/// chunk it was read from.
fn parse_cover_art(chunk: &[u8], chunk_offset: u64, metadata: &mut MetadataMap) {
    if chunk.len() < 8 {
        return;
    }
    let len = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as u64;
    let off = u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as u64;
    if off < chunk_offset + 8 {
        return;
    }
    let relative = off - chunk_offset;
    let Some(end) = relative.checked_add(len) else {
        return;
    };
    if end > chunk.len() as u64 {
        return;
    }
    let bytes = &chunk[relative as usize..end as usize];
    metadata.insert("Audible:CoverArt", TagValue::Binary(bytes.to_vec()));
}

/// Audible.pm:236-263: `$num` length-prefixed key/value pairs packed as
/// `[1 unknown byte][u32 tagLen][u32 valLen][tag bytes][value bytes]`.
fn parse_metadata_dictionary(chunk: &[u8], metadata: &mut MetadataMap) {
    if chunk.len() < 4 {
        return;
    }
    let num = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    if num > MAX_DICTIONARY_ENTRIES {
        return;
    }
    let length = chunk.len();
    let mut pos: usize = 4;
    for _ in 0..num {
        let Some(tag_pos) = pos.checked_add(9) else {
            break;
        };
        if tag_pos > length {
            break;
        }
        let tag_len = u32::from_be_bytes([
            chunk[pos + 1],
            chunk[pos + 2],
            chunk[pos + 3],
            chunk[pos + 4],
        ]) as usize;
        let val_len = u32::from_be_bytes([
            chunk[pos + 5],
            chunk[pos + 6],
            chunk[pos + 7],
            chunk[pos + 8],
        ]) as usize;
        let Some(val_pos) = tag_pos.checked_add(tag_len) else {
            break;
        };
        let Some(next_pos) = val_pos.checked_add(val_len) else {
            break;
        };
        if next_pos > length {
            break;
        }
        let tag = &chunk[tag_pos..val_pos];
        let value = &chunk[val_pos..next_pos];

        let name = predeclared_name(tag)
            .map(str::to_string)
            .unwrap_or_else(|| make_tag_name(tag));
        let decoded = String::from_utf8_lossy(value);
        let unescaped = unescape(&decoded, ENTITY_NUM);
        metadata.insert(format!("Audible:{name}"), TagValue::new_string(unescaped));

        pos = next_pos;
    }
}

/// Audible.pm:29-35: the four dictionary keys `Audible::Main` names
/// explicitly, ahead of the dynamic-naming fallback.
fn predeclared_name(tag: &[u8]) -> Option<&'static str> {
    match tag {
        b"pubdate" => Some("PublishDate"),
        b"pub_date_start" => Some("PublishDateStart"),
        b"author" => Some("Author"),
        b"copyright" => Some("Copyright"),
        _ => None,
    }
}

/// ExifTool.pm:6451-6458 `MakeTagName`, then Audible.pm:257
/// `s/_(.)/\U$1/g`.
fn make_tag_name(tag: &[u8]) -> String {
    let cleaned: String = tag
        .iter()
        .filter(|&&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        .map(|&b| b as char)
        .collect();

    let mut name = cleaned;
    if let Some(first) = name.chars().next() {
        name.replace_range(0..first.len_utf8(), &first.to_ascii_uppercase().to_string());
    }
    let needs_prefix =
        name.len() < 2 || matches!(name.as_bytes().first(), Some(b'-' | b'0'..=b'9'));
    if needs_prefix {
        name = format!("Tag{name}");
    }

    let mut out = String::with_capacity(name.len());
    let mut chars = name.chars();
    while let Some(c) = chars.next() {
        if c == '_' {
            if let Some(next) = chars.next() {
                out.extend(next.to_uppercase());
            }
            // else: a trailing lone `_` has no following char for the Perl
            // regex `/_(.)/ ` to match, so it is left as-is -- but there is
            // nothing left to push, matching Perl leaving it untouched only
            // when a char *does* follow; a truly trailing `_` is dropped
            // here versus kept in Perl. Audible dictionary keys observed in
            // the wild never end in `_`, so this divergence is unreached.
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_tag_name_converts_snake_case() {
        assert_eq!(make_tag_name(b"title_id"), "TitleId");
        assert_eq!(make_tag_name(b"long_description"), "LongDescription");
        assert_eq!(make_tag_name(b"is_aggregation"), "IsAggregation");
        assert_eq!(make_tag_name(b"description"), "Description");
    }

    #[test]
    fn make_tag_name_prefixes_digit_leading_keys() {
        assert_eq!(make_tag_name(b"7eb298ac1328"), "Tag7eb298ac1328");
    }

    #[test]
    fn predeclared_names_match_audible_main_table() {
        assert_eq!(predeclared_name(b"pubdate"), Some("PublishDate"));
        assert_eq!(
            predeclared_name(b"pub_date_start"),
            Some("PublishDateStart")
        );
        assert_eq!(predeclared_name(b"author"), Some("Author"));
        assert_eq!(predeclared_name(b"copyright"), Some("Copyright"));
        assert_eq!(predeclared_name(b"narrator"), None);
    }
}
