//! AFCP (AXS File Concatenation Protocol) trailer parser -- IPTC only
//! (`Image::ExifTool::AFCP::Main`)
//!
//! AFCP appends a block after the JPEG's EOI (and after any other trailers a
//! tool has chained on since). ExifTool's `ProcessAFCP` locates it by reading
//! a 12-byte footer at a fixed distance from EOF, where that distance
//! accounts for every other trailer chained on afterward. oxidex has no
//! general trailer-chain walker to reproduce that distance, so instead this
//! scans the file for the footer's magic bytes directly: 4 bytes ("AXS!" for
//! big-endian, "AXS*" for little-endian) followed by a 4-byte absolute file
//! offset back to the AFCP block's start, then 4 unused bytes. A candidate is
//! accepted only when that offset points at an identical magic sequence --
//! the same round-trip check `ProcessAFCP` itself relies on, and one
//! coincidental image bytes are vanishingly unlikely to satisfy.
//!
//! The AFCP block is a 12-byte header (magic, version, entry count, unused)
//! followed by `numEntries` 12-byte directory entries: a 4-byte ASCII tag, a
//! `size` int32u, and an `offset` int32u that (unlike FotoStation's
//! relative footers) is an ABSOLUTE file position. Only the `IPTC` entry is
//! wired here -- its payload is a plain IIM block routed to
//! `Image::ExifTool::IPTC::Main`, exactly like the Photoshop 8BIM resource
//! and FotoStation's own IPTC record, so it reuses `extract_iptc_from_block`
//! rather than re-decoding. `TEXT`/`Nail`/`PrVw` entries exist in
//! `%AFCP::Main` too but are out of scope here.
//!
//! ExifTool treats AFCP's IPTC as a non-standard location for JPEG (the only
//! standard location is the APP13 Photoshop resource, `IPTC.pm`
//! `%isStandardIPTC`), so it is low priority: a name that also appears in
//! the standard block keeps the standard value, and only names the standard
//! block doesn't provide surface from AFCP. `parse_jpeg_metadata` reproduces
//! this by inserting AFCP's entries before `process_iptc_segments` runs, the
//! same position FotoStation's trailer IPTC uses and for the same reason.

use crate::core::{MetadataMap, TagValue};
use crate::parsers::jpeg::iptc_parser::extract_iptc_from_block;

const MAGIC_BE: [u8; 4] = *b"AXS!";
const MAGIC_LE: [u8; 4] = *b"AXS*";
const FOOTER_LENGTH: usize = 12;
const BLOCK_HEADER_LENGTH: usize = 12;
const DIRECTORY_ENTRY_LENGTH: usize = 12;
const TAG_IPTC: [u8; 4] = *b"IPTC";

/// Extracts the IPTC entry from a JPEG's AFCP trailer, if one is present.
///
/// # Arguments
///
/// * `file` - The complete file contents
///
/// # Returns
///
/// A metadata map keyed `IPTC:<Name>`; empty when the file carries no AFCP
/// trailer, or the trailer has no `IPTC` directory entry.
pub fn parse_afcp_trailer(file: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let Some(block_start) = find_afcp_block(file) else {
        return metadata;
    };
    for (tag, data) in afcp_entries(file, block_start) {
        if tag == TAG_IPTC {
            for (name, value) in extract_iptc_from_block(data) {
                metadata.insert(name, TagValue::new_string(value));
            }
        }
    }
    metadata
}

/// Finds the absolute start offset of a validated AFCP block by locating its
/// footer.
///
/// Scans backward from the end of the file for the magic sequence and
/// accepts a candidate only when the 4-byte offset that follows it also
/// points at an occurrence of the same magic. Scanning from the end mirrors
/// `ProcessAFCP` reading the footer at (close to) EOF, and it means the true
/// footer -- which sits after the block it points back to -- is found before
/// that block's own header could be mistaken for one.
fn find_afcp_block(file: &[u8]) -> Option<usize> {
    if file.len() < FOOTER_LENGTH {
        return None;
    }
    (0..=file.len() - FOOTER_LENGTH)
        .rev()
        .find_map(|pos| validate_footer(file, pos))
}

/// Validates a candidate footer at `pos`, returning the AFCP block's start
/// offset when the footer's `startPos` round-trips to an identical magic.
fn validate_footer(file: &[u8], pos: usize) -> Option<usize> {
    let magic: [u8; 4] = file[pos..pos + 4].try_into().ok()?;
    let big_endian = if magic == MAGIC_BE {
        true
    } else if magic == MAGIC_LE {
        false
    } else {
        return None;
    };
    let offset_bytes: [u8; 4] = file.get(pos + 4..pos + 8)?.try_into().ok()?;
    let start_pos = read_u32(offset_bytes, big_endian) as usize;
    let block_magic: [u8; 4] = file.get(start_pos..start_pos + 4)?.try_into().ok()?;
    (block_magic == magic).then_some(start_pos)
}

/// Reads the AFCP directory at `block_start`, returning each entry's 4-byte
/// tag alongside the raw bytes it points at (an ABSOLUTE file offset, unlike
/// FotoStation's records).
fn afcp_entries(file: &[u8], block_start: usize) -> Vec<([u8; 4], &[u8])> {
    let big_endian = file[block_start..block_start + 4] == MAGIC_BE;
    let Some(num_entries) = file
        .get(block_start + 6..block_start + 8)
        .and_then(|b| <[u8; 2]>::try_from(b).ok())
        .map(|b| read_u16(b, big_endian))
    else {
        return Vec::new();
    };

    let dir_start = block_start + BLOCK_HEADER_LENGTH;
    (0..num_entries as usize)
        .filter_map(|index| {
            let entry = dir_start + index * DIRECTORY_ENTRY_LENGTH;
            let tag: [u8; 4] = file.get(entry..entry + 4)?.try_into().ok()?;
            let size_bytes: [u8; 4] = file.get(entry + 4..entry + 8)?.try_into().ok()?;
            let offset_bytes: [u8; 4] = file.get(entry + 8..entry + 12)?.try_into().ok()?;
            let size = read_u32(size_bytes, big_endian) as usize;
            let offset = read_u32(offset_bytes, big_endian) as usize;
            let data = file.get(offset..offset.checked_add(size)?)?;
            Some((tag, data))
        })
        .collect()
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
}
