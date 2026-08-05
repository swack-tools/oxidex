//! Kodak MakerNote parser
//!
//! Parses Kodak digital camera-specific EXIF MakerNote tags.
//!
//! ## Tag Structure
//!
//! `Kodak::Main` (`Kodak.pm:36-227`) is `PROCESS_PROC =>
//! \&Image::ExifTool::ProcessBinaryData`, a fixed-offset byte record, *not*
//! a TIFF IFD -- `MakerNotes.pm:254-272` even marks the tag itself
//! `NotIFD => 1`. Two signed variants both route to it:
//!
//! * `MakerNoteKodak1a`: payload starts `"KDK INFO"`, `Start => '$valuePtr +
//!   8'`, `ByteOrder => 'BigEndian'`.
//! * `MakerNoteKodak1b`: payload starts `"KDK"` (but not `"KDK INFO"`),
//!   same `Start`, `ByteOrder => 'LittleEndian'`.
//!
//! `exiftool_tables::find_table("Kodak","Main")` already carries this
//! table's real field offsets (verified against `Kodak.pm` and against
//! `combined-samples/Kodak.jpg`'s actual bytes), but its `FIRST_ENTRY => 8`
//! is *not* a byte-offset shift -- `ExifTool.pm`'s only use of
//! `FIRST_ENTRY` is to bound the synthetic-tag range `-U` walks
//! (`ExifTool.pm:9901-9906`), and Kodak.pm's own tag keys (`0x00`, `0x09`,
//! `0x0c`, ...) already equal the fields' byte offsets in the
//! `Start`-shifted record directly (`KodakModel` at key `0x00` sits at file
//! offset `$valuePtr+8+0`, verified against the sample's raw hex).
//! `exiftool_tables::BinaryTable::byte_offset` computes `(index -
//! first_entry) * format_size`, which would shift every field here by -8
//! bytes -- untested by anything else in this crate, since every other
//! transcribed table happens to have `first_entry: 0`. Rather than call it
//! and risk that shift, this reads each field's offset directly from
//! `field.index`, the same number `Kodak.pm` declares.

#![allow(dead_code)]

use crate::core::formatters::numeric_precision::perl_number;
use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

use super::shared::MakerNoteParser;

/// `MakerNotes.pm:255`, `:265`: both Kodak1a and Kodak1b `Start
/// => '$valuePtr + 8'`, past the signature + 2-byte pad.
const KODAK_MAIN_START: usize = 8;

/// Kodak.pm:1-227 field names this parser reads, each verified against
/// `combined-samples/Kodak.jpg` (`exiftool -G1 -s -a`, 13.59 pinned oracle).
/// Offsets are relative to the `Start`-shifted record (i.e. `field.index`
/// straight from `exiftool_tables::find_table("Kodak","Main")` -- see the
/// module doc comment for why this doesn't go through
/// `BinaryTable::byte_offset`).
mod field_offset {
    /// Kodak.pm:52-55: `string[8]`.
    pub const KODAK_MODEL: usize = 0x00;
    /// Kodak.pm:65-68: `int16u`.
    pub const KODAK_IMAGE_WIDTH: usize = 0x0c;
    /// Kodak.pm:69-72: `int16u`.
    pub const KODAK_IMAGE_HEIGHT: usize = 0x0e;
    /// Kodak.pm:73-77: `int16u`.
    pub const YEAR_CREATED: usize = 0x10;
    /// Kodak.pm:78-84: `int8u[2]`.
    pub const MONTH_DAY_CREATED: usize = 0x12;
    /// Kodak.pm:225-230: `int16u`, `ValueConv => '$val / 100'`.
    pub const TOTAL_ZOOM: usize = 0x62;
}

/// Kodak MakerNote parser implementation
pub struct KodakParser;

impl Default for KodakParser {
    fn default() -> Self {
        Self::new()
    }
}

impl KodakParser {
    /// Creates a new Kodak parser instance
    pub fn new() -> Self {
        KodakParser
    }

    /// Reads `Kodak::Main` (see the module doc comment) out of `record`,
    /// the `Start`-shifted bytes (i.e. `payload[8..]`), in `order`.
    fn parse_main_record(
        &self,
        record: &[u8],
        order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        let reader = EndianReader::new(record, order.to_io_byte_order());

        // KodakModel: string[8], truncated at the first NUL -- ExifTool's
        // ReadValue behavior for a `string[n]` (does not trim whitespace,
        // per binary_subdir.rs's note on the same rule).
        if let Some(bytes) = record.get(field_offset::KODAK_MODEL..field_offset::KODAK_MODEL + 8) {
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            if end > 0 {
                tags.insert(
                    "Kodak:KodakModel".to_string(),
                    String::from_utf8_lossy(&bytes[..end]).into_owned(),
                );
            }
        }

        if let Some(v) = reader.u16_at(field_offset::KODAK_IMAGE_WIDTH) {
            tags.insert("Kodak:KodakImageWidth".to_string(), v.to_string());
        }
        if let Some(v) = reader.u16_at(field_offset::KODAK_IMAGE_HEIGHT) {
            tags.insert("Kodak:KodakImageHeight".to_string(), v.to_string());
        }
        if let Some(v) = reader.u16_at(field_offset::YEAR_CREATED) {
            tags.insert("Kodak:YearCreated".to_string(), v.to_string());
        }

        // MonthDayCreated: int8u[2], ValueConv 'sprintf("%.2d:%.2d",split(" ",
        // $val))' -- month and day, zero-padded, colon-joined.
        if let Some(bytes) =
            record.get(field_offset::MONTH_DAY_CREATED..field_offset::MONTH_DAY_CREATED + 2)
        {
            tags.insert(
                "Kodak:MonthDayCreated".to_string(),
                format!("{:02}:{:02}", bytes[0], bytes[1]),
            );
        }

        // TotalZoom: int16u, ValueConv '$val / 100' (no PrintConv, so the
        // ValueConv'd number prints directly -- Perl's default number
        // stringification, which perl_number reproduces).
        if let Some(v) = reader.u16_at(field_offset::TOTAL_ZOOM) {
            tags.insert(
                "Kodak:TotalZoom".to_string(),
                perl_number(f64::from(v) / 100.0),
            );
        }
    }
}

impl MakerNoteParser for KodakParser {
    fn manufacturer_name(&self) -> &'static str {
        "Kodak"
    }

    fn tag_prefix(&self) -> &'static str {
        "Kodak:"
    }

    fn parse(
        &self,
        data: &[u8],
        _byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // Byte order is signature-determined for Kodak1a/1b (see the module
        // doc comment), not inherited from the enclosing TIFF -- ignore the
        // caller's `byte_order` the same way Casio Type2 and Sanyo resolve
        // their own.
        let order = if data.starts_with(b"KDK INFO") {
            ByteOrder::BigEndian
        } else if data.starts_with(b"KDK") {
            ByteOrder::LittleEndian
        } else {
            // Not a Kodak1a/1b payload (could be Type2/3/4/5/6 or another
            // vendor's rebrand) -- none of those are implemented here.
            return Ok(());
        };
        let Some(record) = data.get(KODAK_MAIN_START..) else {
            return Ok(());
        };
        self.parse_main_record(record, order, tags);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kodak_parser_trait() {
        let parser = KodakParser::new();
        assert_eq!(parser.manufacturer_name(), "Kodak");
        assert_eq!(parser.tag_prefix(), "Kodak:");
    }

    /// Bytes and expected values transcribed from `exiftool -v3` /
    /// `exiftool -G1 -s -a` on `combined-samples/Kodak.jpg`, whose
    /// MakerNote is `"KDK INFO"`-signed (big-endian).
    #[test]
    fn test_parse_kdk_info_main_record() {
        let parser = KodakParser::new();
        let mut data = b"KDK INFO".to_vec();
        let mut record = vec![0u8; 108];
        record[0x00..0x08].copy_from_slice(b"DX4900  ");
        record[0x0c..0x0e].copy_from_slice(&2448u16.to_be_bytes());
        record[0x0e..0x10].copy_from_slice(&1632u16.to_be_bytes());
        record[0x10..0x12].copy_from_slice(&2002u16.to_be_bytes());
        record[0x12] = 5;
        record[0x13] = 1;
        record[0x62..0x64].copy_from_slice(&140u16.to_be_bytes());
        data.extend_from_slice(&record);

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        // Real ExifTool output keeps the trailing padding: "DX4900  " (two
        // spaces), verified via `exiftool -j` on Kodak.jpg -- `string[n]`
        // is only truncated at the first NUL, and there isn't one here.
        assert_eq!(tags.get("Kodak:KodakModel"), Some(&"DX4900  ".to_string()));
        assert_eq!(tags.get("Kodak:KodakImageWidth"), Some(&"2448".to_string()));
        assert_eq!(
            tags.get("Kodak:KodakImageHeight"),
            Some(&"1632".to_string())
        );
        assert_eq!(tags.get("Kodak:YearCreated"), Some(&"2002".to_string()));
        assert_eq!(
            tags.get("Kodak:MonthDayCreated"),
            Some(&"05:01".to_string())
        );
        assert_eq!(tags.get("Kodak:TotalZoom"), Some(&"1.4".to_string()));
    }

    #[test]
    fn test_non_kodak1a1b_payload_is_a_no_op() {
        let parser = KodakParser::new();
        let data = vec![0u8; 32];
        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert!(tags.is_empty());
    }
}
