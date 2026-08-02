//! Samsung "STMN" MakerNotes -- ExifTool's `MakerNoteSamsung1a` / `MakerNoteSamsung1b`.
//!
//! Samsung's compact cameras of the Digimax/KENOX/NV/L/PL/WB era do not write a
//! TIFF IFD into `MakerNote`. They write a flat binary record that begins with
//! the ASCII signature `STMN` followed by a three-digit version. ExifTool
//! selects it purely on that signature (MakerNotes.pm:950-964); the `Make`
//! string is never consulted:
//!
//! ```text
//!     {
//!         Name => 'MakerNoteSamsung1a',
//!         # Samsung STMN maker notes WITHOUT PreviewImage
//!         Condition => '$$valPt =~ /^STMN\d{3}.\0{4}/s',
//!         Binary => 1,
//!         Notes => 'Samsung "STMN" maker notes without PreviewImage',
//!     },
//!     {
//!         Name => 'MakerNoteSamsung1b',
//!         # Samsung STMN maker notes WITH PreviewImage
//!         Condition => '$$valPt =~ /^STMN\d{3}/',
//!         SubDirectory => {
//!             TagTable => 'Image::ExifTool::Samsung::Main',
//!         },
//!     },
//! ```
//!
//! The two entries are tried in that order, so the `1a` condition wins whenever
//! it matches. Its extra `.\0{4}` covers byte 7 plus the four bytes at 8..12 --
//! which is exactly `PreviewImageStart` -- so "1a" means "an STMN record whose
//! preview offset is zero". `1a` carries `Binary => 1` and *no* `SubDirectory`,
//! so ExifTool descends into nothing and reports no tags for those files at
//! all. Only `1b` reaches `Samsung::Main`.
//!
//! `Samsung::Main` itself is a `ProcessBinaryData` table, transcribed into
//! [`crate::exiftool_tables`] as `Samsung::Main` -- `FORMAT => 'int32u'`,
//! `FIRST_ENTRY => 0`, with `MakerNoteVersion` overridden to `undef[8]`. Index
//! 2 and index 3 therefore land at byte 8 and byte 12. The layout is read from
//! that generated table rather than restated here.
//!
//! Two fields of `Samsung::Main` are deliberately not produced:
//!
//! * `SamsungIFD` (index 11) is `Condition`-gated on `/^[^\0]\0\0\0/` and
//!   descends into `Image::ExifTool::Samsung::IFD`, whose own `NOTES` state
//!   "no tags in this IFD are known". ExifTool emits nothing from it without
//!   `-u`, so there is nothing to match.
//! * `PreviewImage` is not in this table at all -- ExifTool builds it as a
//!   Composite from the `PreviewImageStart`/`PreviewImageLength` pair, which
//!   oxidex's own composite table already declares.

use std::collections::HashMap;

use crate::exiftool_tables::{DecodedValue, decode_binary_table, find_table};
use crate::io::ByteOrder as IoByteOrder;
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// Length of the `STMN` signature plus its three version digits.
const SIGNATURE_LEN: usize = 7;

/// Byte range covered by `PreviewImageStart`, which is what separates the
/// `1a` and `1b` conditions.
const PREVIEW_START: std::ops::Range<usize> = 8..12;

/// ExifTool's `MakerNoteSamsung1b` condition, `$$valPt =~ /^STMN\d{3}/`.
///
/// Note that `\d` here is Perl's ASCII-digit class under ExifTool's default
/// (non-unicode) semantics on a byte string, so this is an exact translation.
#[must_use]
pub fn is_stmn(data: &[u8]) -> bool {
    data.len() >= SIGNATURE_LEN
        && &data[0..4] == b"STMN"
        && data[4..SIGNATURE_LEN].iter().all(u8::is_ascii_digit)
}

/// ExifTool's `MakerNoteSamsung1a` condition, `$$valPt =~ /^STMN\d{3}.\0{4}/s`,
/// evaluated on a block already known to satisfy [`is_stmn`].
///
/// The `.` consumes byte 7 and `\0{4}` the four bytes of `PreviewImageStart`.
/// A block shorter than 12 bytes cannot match the regex, so it stays `1b` --
/// and `decode_binary_table` then simply yields no `PreviewImageStart`, which
/// is what ExifTool's `ProcessBinaryData` does with a short block too.
fn is_binary_only(data: &[u8]) -> bool {
    data.len() >= PREVIEW_START.end && data[PREVIEW_START].iter().all(|byte| *byte == 0)
}

const fn io_order(order: ByteOrder) -> IoByteOrder {
    match order {
        ByteOrder::LittleEndian => IoByteOrder::Little,
        ByteOrder::BigEndian => IoByteOrder::Big,
    }
}

/// Decode an `STMN` MakerNote into `Samsung:`-prefixed tags.
///
/// `byte_order` is the enclosing TIFF header's order, which is what ExifTool
/// uses: the `MakerNoteSamsung1b` `SubDirectory` declares no `ByteOrder` of its
/// own, so `ProcessBinaryData` runs under the order already in effect. The
/// sample corpus exercises both -- 38 of the 50 STMN files are little-endian
/// and 12 are big-endian, and `exiftool -v3` labels the directory accordingly.
pub fn parse(data: &[u8], byte_order: ByteOrder, tags: &mut HashMap<String, String>) {
    if !is_stmn(data) || is_binary_only(data) {
        return;
    }
    let Some(table) = find_table("Samsung", "Main") else {
        return;
    };
    for decoded in decode_binary_table(table, data, io_order(byte_order)) {
        let rendered = match (decoded.field.name, &decoded.raw) {
            // `undef[8]`. The version is NUL-padded to the full eight bytes on
            // the models that spell it with seven characters ("STMN100",
            // "STMN010"); ExifTool carries the padding in the value and drops
            // it at the output layer (exiftool:3819, `tr/\0//d`). Refuse a
            // block that is not text rather than render one approximately.
            ("MakerNoteVersion", DecodedValue::Undefined(bytes)) => {
                match std::str::from_utf8(bytes) {
                    Ok(text) => text.trim_end_matches('\0').to_string(),
                    Err(_) => continue,
                }
            }
            ("PreviewImageStart" | "PreviewImageLength", DecodedValue::Integer(value)) => {
                value.to_string()
            }
            _ => continue,
        };
        tags.insert(format!("Samsung:{}", decoded.field.name), rendered);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bytes 0..16 of `Samsung/SamsungNV10.jpg`'s MakerNote, from
    /// `exiftool -v3`: big-endian, "STMN0102", start 0x00437650, length 0xe7d9.
    const NV10: [u8; 16] = [
        0x53, 0x54, 0x4d, 0x4e, 0x30, 0x31, 0x30, 0x32, 0x00, 0x43, 0x76, 0x50, 0x00, 0x00, 0xe7,
        0xd9,
    ];

    /// Bytes 0..16 of `Samsung/SamsungES10.jpg`'s MakerNote: little-endian, a
    /// NUL-padded "STMN100", start 0x00392aff, length 0x0001322a.
    const ES10: [u8; 16] = [
        0x53, 0x54, 0x4d, 0x4e, 0x31, 0x30, 0x30, 0x00, 0xff, 0x2a, 0x39, 0x00, 0x2a, 0x32, 0x01,
        0x00,
    ];

    fn run(data: &[u8], order: ByteOrder) -> HashMap<String, String> {
        let mut tags = HashMap::new();
        parse(data, order, &mut tags);
        tags
    }

    #[test]
    fn big_endian_record_matches_exiftool() {
        let tags = run(&NV10, ByteOrder::BigEndian);
        assert_eq!(tags["Samsung:MakerNoteVersion"], "STMN0102");
        assert_eq!(tags["Samsung:PreviewImageStart"], "4421200");
        assert_eq!(tags["Samsung:PreviewImageLength"], "59353");
    }

    #[test]
    fn little_endian_record_matches_exiftool() {
        let tags = run(&ES10, ByteOrder::LittleEndian);
        // ExifTool prints "STMN100": the eighth byte is NUL padding.
        assert_eq!(tags["Samsung:MakerNoteVersion"], "STMN100");
        assert_eq!(tags["Samsung:PreviewImageStart"], "3746559");
        assert_eq!(tags["Samsung:PreviewImageLength"], "78378");
    }

    #[test]
    fn byte_order_is_not_guessed() {
        // Reading the big-endian NV10 record as little-endian must not happen
        // to produce ExifTool's numbers -- if it did, the corpus could not
        // distinguish a correct decoder from a lucky one.
        let tags = run(&NV10, ByteOrder::LittleEndian);
        assert_ne!(tags["Samsung:PreviewImageStart"], "4421200");
    }

    #[test]
    fn samsung1a_yields_nothing() {
        // `MakerNoteSamsung1a` is `Binary => 1` with no SubDirectory: a zero
        // PreviewImageStart means ExifTool reports no tags whatsoever.
        let mut data = ES10;
        data[8..12].fill(0);
        assert!(run(&data, ByteOrder::LittleEndian).is_empty());
    }

    #[test]
    fn signature_is_required() {
        assert!(!is_stmn(b"STMNabc\0"));
        assert!(!is_stmn(b"Samsung\0"));
        assert!(!is_stmn(b"STMN"));
        assert!(is_stmn(b"STMN010"));
        assert!(is_stmn(b"STMN0102"));
    }

    #[test]
    fn short_block_yields_only_what_fits() {
        // Eight bytes carry the version and nothing else. ExifTool's
        // ProcessBinaryData stops at the end of the block rather than reading
        // past it, and neither `1a` (which needs 12 bytes) nor the offset pair
        // can apply.
        let tags = run(&ES10[..8], ByteOrder::LittleEndian);
        assert_eq!(tags["Samsung:MakerNoteVersion"], "STMN100");
        assert!(!tags.contains_key("Samsung:PreviewImageStart"));
        assert!(!tags.contains_key("Samsung:PreviewImageLength"));
    }

    #[test]
    fn generated_table_carries_the_layout() {
        // The offsets this decoder relies on come from the transcription, not
        // from this file. FORMAT => 'int32u' with FIRST_ENTRY => 0 puts index
        // 2 at byte 8 and index 3 at byte 12.
        let table = find_table("Samsung", "Main").expect("Samsung::Main");
        let offset = |name: &str| {
            table
                .fields
                .iter()
                .find(|f| f.name == name)
                .map(|f| table.byte_offset(f))
                .expect(name)
        };
        assert_eq!(offset("MakerNoteVersion"), 0);
        assert_eq!(offset("PreviewImageStart"), 8);
        assert_eq!(offset("PreviewImageLength"), 12);
    }
}
