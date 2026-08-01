//! AFCP (AXS File Concatenation Protocol) trailer parser
//! (`Image::ExifTool::AFCP::Main`)
//!
//! AFCP appends a block after the JPEG's EOI (and after any other trailers a
//! tool has chained on since). ExifTool's `ProcessAFCP` locates it by reading
//! a 12-byte footer at a fixed distance from EOF, where that distance
//! accounts for every other trailer chained on afterward. oxidex has no
//! general trailer-chain walker to reproduce that distance -- see
//! [`crate::parsers::trailer`] for why -- so instead this scans the file for
//! the footer's magic bytes directly: 4 bytes ("AXS!" for big-endian, "AXS*"
//! for little-endian) followed by a 4-byte absolute file offset back to the
//! AFCP block's start, then 4 unused bytes. A candidate is accepted only when
//! that offset points at an identical magic sequence -- the same round-trip
//! check `ProcessAFCP` itself relies on (AFCP.pm:90), and one coincidental
//! image bytes are vanishingly unlikely to satisfy.
//!
//! The AFCP block is a 12-byte header (magic, version, entry count, unused)
//! followed by `numEntries` 12-byte directory entries: a 4-byte ASCII tag, a
//! `size` int32u, and an `offset` int32u that (unlike FotoStation's
//! relative footers) is an ABSOLUTE file position.
//!
//! Two of the four tags in `%AFCP::Main` (AFCP.pm:21-61) are read here. `IPTC`
//! is a plain IIM block routed to `Image::ExifTool::IPTC::Main`, exactly like
//! the Photoshop 8BIM resource and FotoStation's own IPTC record, so it reuses
//! `extract_iptc_values_from_block` rather than re-decoding; `TEXT` is a
//! string reported as `AFCP:Text`. `Nail` and `PrVw` are `ThumbnailImage` and
//! `PreviewImage` -- embedded JPEGs behind a RawConv that skips a variable
//! amount of padding to find the SOI marker (AFCP.pm:44-49). Those belong to
//! oxidex's embedded-image handling rather than to this module, and no file in
//! the sample corpus carries them.
//!
//! Because the offsets are absolute, "any editing software that doesn't
//! recognize the AFCP trailer" (AFCP.pm:248-253) invalidates the whole block
//! by changing the file length ahead of it. ExifTool recovers by finding where
//! the header actually landed and shifting every offset by
//! `$fix = $actualPos - $startPos` (AFCP.pm:94-117); [`find_afcp_block`] does
//! the same.
//!
//! ExifTool treats AFCP's IPTC as a non-standard location for JPEG (the only
//! standard location is the APP13 Photoshop resource, `IPTC.pm`
//! `%isStandardIPTC`), so it is low priority: a name that also appears in
//! the standard block keeps the standard value, and only names the standard
//! block doesn't provide surface from AFCP. `parse_jpeg_metadata` reproduces
//! this by inserting AFCP's entries before `process_iptc_segments` runs, the
//! same position FotoStation's trailer IPTC uses and for the same reason.

use crate::core::{MetadataMap, TagValue};
use crate::parsers::jpeg::iptc_parser::extract_iptc_values_from_block;
use crate::parsers::trailer;

const MAGIC_BE: [u8; 4] = *b"AXS!";
const MAGIC_LE: [u8; 4] = *b"AXS*";
/// The three bytes both spellings share; the fourth selects the byte order.
const MAGIC_PREFIX: &[u8; 3] = b"AXS";
const FOOTER_LENGTH: usize = 12;
const BLOCK_HEADER_LENGTH: usize = 12;
const DIRECTORY_ENTRY_LENGTH: usize = 12;
const TAG_IPTC: [u8; 4] = *b"IPTC";
const TAG_TEXT: [u8; 4] = *b"TEXT";

/// `$size < 0x80000000` (AFCP.pm:165) -- a payload size with the high bit set
/// is rejected rather than read.
const MAX_PAYLOAD_SIZE: u64 = 0x8000_0000;

/// A located AFCP block: where its header is, and `$fix` (AFCP.pm:117), the
/// amount every offset stored inside it is out by. `fix` is zero unless the
/// file was edited after the trailer was written.
struct Located {
    block_start: usize,
    fix: i64,
}

/// Extracts the tags a JPEG's AFCP trailer carries, if one is present.
///
/// # Arguments
///
/// * `file` - The complete file contents
///
/// # Returns
///
/// A metadata map keyed `IPTC:<Name>` for the trailer's IPTC record, plus
/// `AFCP:Text` for a `TEXT` entry; empty when the file carries no AFCP
/// trailer, or the trailer has no directory entry this module reads.
pub fn parse_afcp_trailer(file: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let Some(located) = find_afcp_block(file) else {
        return metadata;
    };
    for (tag, data) in afcp_entries(file, &located) {
        match tag {
            TAG_IPTC => {
                for (name, value) in extract_iptc_values_from_block(data) {
                    metadata.insert(name, value);
                }
            }
            TAG_TEXT => {
                metadata.insert(
                    "AFCP:Text",
                    TagValue::new_string(String::from_utf8_lossy(data).into_owned()),
                );
            }
            _ => {}
        }
    }
    metadata
}

/// Finds a validated AFCP block by locating its footer.
///
/// Scans backward from the end of the file for the magic sequence and accepts
/// a candidate only when the 4-byte offset that follows it also points at an
/// occurrence of the same magic. Scanning from the end mirrors `ProcessAFCP`
/// reading the footer at (close to) EOF, and it means the true footer -- which
/// sits after the block it points back to -- is found before that block's own
/// header could be mistaken for one.
///
/// When the offset does *not* round-trip, the file has been edited since the
/// trailer was written and every offset in it is stale by the same amount.
/// ExifTool scans for where the header actually is and records the difference
/// as `$fix` (AFCP.pm:94-117); the second search here does the same, working
/// back from the footer rather than forward from the start of the run of
/// trailers, which is the only thing ExifTool needs its chain for. A stray
/// magic sequence inside payload data is rejected because the directory it
/// would imply does not resolve.
fn find_afcp_block(file: &[u8]) -> Option<Located> {
    trailer::find_last(
        file,
        FOOTER_LENGTH,
        MAGIC_PREFIX,
        FOOTER_LENGTH,
        |file, end| {
            let footer = &file[end - FOOTER_LENGTH..end];
            let magic: [u8; 4] = footer[..4].try_into().ok()?;
            let big_endian = match magic {
                MAGIC_BE => true,
                MAGIC_LE => false,
                _ => return None,
            };
            let offset_bytes: [u8; 4] = footer[4..8].try_into().ok()?;
            let start_pos = read_u32(offset_bytes, big_endian) as usize;

            if block_magic_at(file, start_pos, magic, end) {
                return Some(Located {
                    block_start: start_pos,
                    fix: 0,
                });
            }

            let searchable = &file[..end - FOOTER_LENGTH];
            memchr::memmem::rfind_iter(searchable, &magic).find_map(|block_start| {
                if !block_magic_at(file, block_start, magic, end) {
                    return None;
                }
                let located = Located {
                    block_start,
                    fix: block_start as i64 - start_pos as i64,
                };
                // Only accept a shifted header whose whole directory resolves.
                (!afcp_entries(file, &located).is_empty()).then_some(located)
            })
        },
    )
}

/// Whether a complete block header carrying `magic` sits at `at`, with room
/// for it before the footer that ends at `end`.
fn block_magic_at(file: &[u8], at: usize, magic: [u8; 4], end: usize) -> bool {
    match file.get(at..at + BLOCK_HEADER_LENGTH) {
        Some(header) => header[..4] == magic && at + BLOCK_HEADER_LENGTH <= end - FOOTER_LENGTH,
        None => false,
    }
}

/// Reads the AFCP directory, returning each entry's 4-byte tag alongside the
/// raw bytes it points at (an ABSOLUTE file offset, unlike FotoStation's
/// records, shifted by `fix` when an edit invalidated them).
///
/// Mirrors AFCP.pm:160-203: an entry whose size has the high bit set, or whose
/// payload does not lie entirely within the file, is a "Bad AFCP directory"
/// and ends the walk rather than being skipped.
fn afcp_entries<'a>(file: &'a [u8], located: &Located) -> Vec<([u8; 4], &'a [u8])> {
    let block_start = located.block_start;
    let mut out = Vec::new();
    let big_endian = file[block_start..block_start + 4] == MAGIC_BE;
    let Some(num_entries) = file
        .get(block_start + 6..block_start + 8)
        .and_then(|b| <[u8; 2]>::try_from(b).ok())
        .map(|b| read_u16(b, big_endian))
    else {
        return out;
    };

    let dir_start = block_start + BLOCK_HEADER_LENGTH;
    // "Error reading AFCP directory" (AFCP.pm:146) -- a directory that runs
    // off the end of the file yields nothing at all.
    let Some(dir) = file.get(dir_start..dir_start + DIRECTORY_ENTRY_LENGTH * num_entries as usize)
    else {
        return out;
    };

    for entry in dir.chunks_exact(DIRECTORY_ENTRY_LENGTH) {
        let tag: [u8; 4] = entry[..4].try_into().expect("chunk is 12 bytes");
        let size_bytes: [u8; 4] = entry[4..8].try_into().expect("chunk is 12 bytes");
        let offset_bytes: [u8; 4] = entry[8..12].try_into().expect("chunk is 12 bytes");
        let size = read_u32(size_bytes, big_endian) as u64;
        let offset = read_u32(offset_bytes, big_endian) as i64;
        let Some(start) = offset.checked_add(located.fix).filter(|s| *s >= 0) else {
            break;
        };
        let Some(data) = (size < MAX_PAYLOAD_SIZE)
            .then(|| file.get(start as usize..start as usize + size as usize))
            .flatten()
        else {
            break;
        };
        out.push((tag, data));
    }

    out
}

fn read_u16(bytes: [u8; 2], big_endian: bool) -> u16 {
    if big_endian {
        u16::from_be_bytes(bytes)
    } else {
        u16::from_le_bytes(bytes)
    }
}

fn read_u32(bytes: [u8; 4], big_endian: bool) -> u32 {
    if big_endian {
        u32::from_be_bytes(bytes)
    } else {
        u32::from_le_bytes(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real IPTC IIM block from `combined-samples/AFCP.jpg` (263 bytes at
    /// file offset 0x11f), byte for byte, per `exiftool -v3`.
    fn afcp_jpg_iptc_block() -> &'static [u8] {
        b"\x1c\x02\x00\x00\x02\x00\x02\x1c\x02\x0f\x00\x01p\x1c\x027\x00\x0820051223\
\x1c\x02\x05\x00\x0bobject name\x1c\x02\x0a\x00\x012\x1c\x02\x14\x00\x08supp cat\
\x1c\x02\x19\x00\x07keyword\x1c\x02(\x00\x14special instructions\x1c\x02P\x00\x06byline\
\x1c\x02U\x00\x0cbyline title\x1c\x02Z\x00\x04city\x1c\x02e\x00\x0ccountry name\
\x1c\x02g\x00\x03otr\x1c\x02i\x00\x08headline\x1c\x02n\x00\x06credit\x1c\x02s\x00\x06source\
\x1c\x02t\x00\x0bcopy freely\x1c\x02x\x00\x12ExifTool AFCP test\
\x1c\x02z\x00\x0ecaption writer\x1c\x02_\x00\x05state"
    }

    /// Builds a synthetic JPEG-with-AFCP-trailer file: `[preamble][AFCP
    /// block header][directory entries][entry payloads][footer]`. Mirrors
    /// the real `AFCP.jpg` layout (block header `vers=1, numEntries=2`, an
    /// `IPTC` entry and one entry with an unrecognized tag) but with a short
    /// placeholder payload for the unrecognized entry instead of AFCP.jpg's
    /// 548-byte one.
    fn build_afcp_file(preamble: &[u8], iptc_payload: &[u8], unknown_payload: &[u8]) -> Vec<u8> {
        let block_start = preamble.len();
        let dir_start = block_start + BLOCK_HEADER_LENGTH;
        let iptc_offset = dir_start + 2 * DIRECTORY_ENTRY_LENGTH;
        let unknown_offset = iptc_offset + iptc_payload.len();

        let mut file = preamble.to_vec();
        file.extend_from_slice(b"AXS!"); // magic (big-endian)
        file.extend_from_slice(&1u16.to_be_bytes()); // version
        file.extend_from_slice(&2u16.to_be_bytes()); // numEntries
        file.extend_from_slice(&[0u8; 4]); // unused checksum

        file.extend_from_slice(b"IPTC");
        file.extend_from_slice(&(iptc_payload.len() as u32).to_be_bytes());
        file.extend_from_slice(&(iptc_offset as u32).to_be_bytes());

        file.extend_from_slice(b"%SCC"); // an AFCP tag oxidex doesn't wire
        file.extend_from_slice(&(unknown_payload.len() as u32).to_be_bytes());
        file.extend_from_slice(&(unknown_offset as u32).to_be_bytes());

        file.extend_from_slice(iptc_payload);
        file.extend_from_slice(unknown_payload);

        file.extend_from_slice(b"AXS!"); // footer magic
        file.extend_from_slice(&(block_start as u32).to_be_bytes()); // startPos
        file.extend_from_slice(&[0u8; 4]); // unused checksum
        file
    }

    /// Every assertion comes from `exiftool -a -G1 -s combined-samples/
    /// AFCP.jpg` (ExifTool 13.59): this file's only IPTC source is its AFCP
    /// trailer, so these are also the values a bare `-j` reports.
    #[test]
    fn test_afcp_jpg_trailer_matches_exiftool() {
        let file = build_afcp_file(
            b"\xff\xd8\xff\xd9",
            afcp_jpg_iptc_block(),
            b"not an IPTC payload",
        );

        let m = parse_afcp_trailer(&file);
        assert_eq!(m.get_string("IPTC:ApplicationRecordVersion"), Some("2"));
        assert_eq!(m.get_string("IPTC:Category"), Some("p"));
        assert_eq!(m.get_string("IPTC:DateCreated"), Some("2005:12:23"));
        assert_eq!(m.get_string("IPTC:ObjectName"), Some("object name"));
        assert_eq!(m.get_string("IPTC:Urgency"), Some("2"));
        assert_eq!(
            m.get_string("IPTC:SupplementalCategories"),
            Some("supp cat")
        );
        assert_eq!(m.get_string("IPTC:Keywords"), Some("keyword"));
        assert_eq!(
            m.get_string("IPTC:SpecialInstructions"),
            Some("special instructions")
        );
        assert_eq!(m.get_string("IPTC:By-line"), Some("byline"));
        assert_eq!(m.get_string("IPTC:By-lineTitle"), Some("byline title"));
        assert_eq!(m.get_string("IPTC:City"), Some("city"));
        assert_eq!(
            m.get_string("IPTC:Country-PrimaryLocationName"),
            Some("country name")
        );
        assert_eq!(
            m.get_string("IPTC:OriginalTransmissionReference"),
            Some("otr")
        );
        assert_eq!(m.get_string("IPTC:Headline"), Some("headline"));
        assert_eq!(m.get_string("IPTC:Credit"), Some("credit"));
        assert_eq!(m.get_string("IPTC:Source"), Some("source"));
        assert_eq!(m.get_string("IPTC:CopyrightNotice"), Some("copy freely"));
        assert_eq!(
            m.get_string("IPTC:Caption-Abstract"),
            Some("ExifTool AFCP test")
        );
        assert_eq!(m.get_string("IPTC:Writer-Editor"), Some("caption writer"));
        assert_eq!(m.get_string("IPTC:Province-State"), Some("state"));
        // The `%SCC` entry isn't in `%AFCP::Main`, so ExifTool ignores it too.
        assert_eq!(m.len(), 20);
    }

    #[test]
    fn test_footer_not_at_absolute_eof_is_still_found() {
        // Real files chain other trailers (FotoStation, Vivo, ...) after
        // AFCP, so the footer is rarely the file's last 12 bytes.
        let mut file = build_afcp_file(b"\xff\xd8\xff\xd9", afcp_jpg_iptc_block(), b"placeholder");
        file.extend_from_slice(b"\x00\x00\x00\x00trailing trailer data, not AFCP");

        let m = parse_afcp_trailer(&file);
        assert_eq!(m.get_string("IPTC:ObjectName"), Some("object name"));
    }

    #[test]
    fn test_little_endian_byte_order() {
        let block_start = 4usize;
        let iptc_payload = afcp_jpg_iptc_block();
        let dir_start = block_start + BLOCK_HEADER_LENGTH;
        let iptc_offset = dir_start + DIRECTORY_ENTRY_LENGTH;

        let mut file = b"\xff\xd8\xff\xd9".to_vec();
        file.extend_from_slice(b"AXS*");
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&[0u8; 4]);
        file.extend_from_slice(b"IPTC");
        file.extend_from_slice(&(iptc_payload.len() as u32).to_le_bytes());
        file.extend_from_slice(&(iptc_offset as u32).to_le_bytes());
        file.extend_from_slice(iptc_payload);
        file.extend_from_slice(b"AXS*");
        file.extend_from_slice(&(block_start as u32).to_le_bytes());
        file.extend_from_slice(&[0u8; 4]);

        let m = parse_afcp_trailer(&file);
        assert_eq!(m.get_string("IPTC:ObjectName"), Some("object name"));
    }

    #[test]
    fn test_file_without_afcp_trailer_yields_nothing() {
        assert!(parse_afcp_trailer(b"\xff\xd8\xff\xd9 no trailer here").is_empty());
        assert!(parse_afcp_trailer(b"").is_empty());
    }

    #[test]
    fn test_magic_bytes_without_a_valid_round_trip_are_ignored() {
        // "AXS!" appears, but the offset it names doesn't itself start with
        // "AXS!" -- exactly the case ExifTool's own footer validation
        // (re-reading the block header) is designed to reject.
        let mut file = vec![0u8; 40];
        file[28..32].copy_from_slice(b"AXS!");
        file[32..36].copy_from_slice(&0u32.to_be_bytes()); // points at zero bytes
        assert!(parse_afcp_trailer(&file).is_empty());
    }

    /// Builds a big-endian AFCP trailer holding `entries`, laid out exactly as
    /// `ProcessAFCP` writes one (AFCP.pm:212-213).
    ///
    /// `pad` bytes go between the image and the trailer *after* the offsets
    /// have been computed -- what an editor that does not know about AFCP does
    /// to a file.
    fn build_with_pad(preamble: &[u8], entries: &[(&[u8; 4], &[u8])], pad: usize) -> Vec<u8> {
        let block_start = preamble.len() as u32;
        let value_pos =
            block_start + (BLOCK_HEADER_LENGTH + DIRECTORY_ENTRY_LENGTH * entries.len()) as u32;
        let (mut dir, mut values) = (Vec::new(), Vec::new());
        for (tag, payload) in entries {
            dir.extend_from_slice(*tag);
            dir.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            dir.extend_from_slice(&(value_pos + values.len() as u32).to_be_bytes());
            values.extend_from_slice(payload);
        }

        let mut file = preamble.to_vec();
        file.extend(std::iter::repeat_n(0u8, pad));
        file.extend_from_slice(b"AXS!");
        file.extend_from_slice(&1u16.to_be_bytes()); // version
        file.extend_from_slice(&(entries.len() as u16).to_be_bytes());
        file.extend_from_slice(&[0u8; 4]);
        file.extend_from_slice(&dir);
        file.extend_from_slice(&values);
        file.extend_from_slice(b"AXS!");
        file.extend_from_slice(&block_start.to_be_bytes());
        file.extend_from_slice(&[0u8; 4]);
        file
    }

    #[test]
    fn test_text_entry() {
        // `TEXT => 'Text'` (AFCP.pm:37); ExifTool 13.59 reports it as
        // `[AFCP] Text` on a trailer built exactly like this one.
        let file = build_with_pad(b"\xff\xd8\xff\xd9", &[(b"TEXT", b"hello afcp text")], 0);
        assert_eq!(
            parse_afcp_trailer(&file).get_string("AFCP:Text"),
            Some("hello afcp text")
        );
    }

    #[test]
    fn test_offsets_invalidated_by_an_edit_are_fixed_up() {
        // Padding inserted ahead of the trailer leaves every stored offset
        // seven bytes short. ExifTool 13.59 still reads such a file, via
        // `$fix = $actualPos - $startPos` (AFCP.pm:117).
        let file = build_with_pad(b"\xff\xd8\xff\xd9", &[(b"IPTC", afcp_jpg_iptc_block())], 7);
        let m = parse_afcp_trailer(&file);
        assert_eq!(m.get_string("IPTC:ObjectName"), Some("object name"));
        assert_eq!(
            m.get_string("IPTC:Caption-Abstract"),
            Some("ExifTool AFCP test")
        );
        assert_eq!(m.len(), 20);
    }

    #[test]
    fn test_repeatable_datasets_keep_every_value() {
        // 2:25 Keywords is `Flags => 'List'`, so ExifTool joins the three
        // records rather than keeping the last. Inserting the decoded pairs
        // one at a time would report only "third".
        let iptc = b"\x1c\x02\x19\x00\x05first\x1c\x02\x19\x00\x06second\x1c\x02\x19\x00\x05third";
        let file = build_with_pad(b"\xff\xd8\xff\xd9", &[(b"IPTC", iptc)], 0);
        assert_eq!(
            parse_afcp_trailer(&file).get("IPTC:Keywords"),
            Some(&TagValue::Array(vec![
                TagValue::new_string("first".to_string()),
                TagValue::new_string("second".to_string()),
                TagValue::new_string("third".to_string()),
            ]))
        );
    }

    #[test]
    fn test_payload_running_past_the_end_of_the_file_is_rejected() {
        let image = b"\xff\xd8\xff\xd9";
        // Entry 0's size field sits 4 bytes into the directory, which starts
        // 12 bytes into the block.
        let size_at = image.len() + BLOCK_HEADER_LENGTH + 4;

        let mut file = build_with_pad(image, &[(b"IPTC", afcp_jpg_iptc_block())], 0);
        file[size_at..size_at + 4].copy_from_slice(&0xffffu32.to_be_bytes());
        assert!(parse_afcp_trailer(&file).is_empty());

        // A size with the high bit set is rejected before it is read at all
        // (`$size < 0x80000000`, AFCP.pm:165).
        let mut file = build_with_pad(image, &[(b"IPTC", afcp_jpg_iptc_block())], 0);
        file[size_at..size_at + 4].copy_from_slice(&0x8000_0000u32.to_be_bytes());
        assert!(parse_afcp_trailer(&file).is_empty());
    }

    #[test]
    fn test_directory_running_past_the_end_of_the_file_yields_nothing() {
        // "Error reading AFCP directory" (AFCP.pm:146).
        let image = b"\xff\xd8\xff\xd9";
        let mut file = build_with_pad(image, &[(b"IPTC", afcp_jpg_iptc_block())], 0);
        let count_at = image.len() + 6; // entry count, 6 bytes into the header
        file[count_at..count_at + 2].copy_from_slice(&9999u16.to_be_bytes());
        assert!(parse_afcp_trailer(&file).is_empty());
    }
}
