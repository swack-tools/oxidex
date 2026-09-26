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
use crate::core::tag_occurrence::intern;
use crate::core::{Instance, Provenance, TagOccurrence, TagValue};
use crate::exiftool_tables::Ctx;
use crate::exiftool_tables::session::Session;
use crate::exiftool_tables::{Dir, find_table, process_binary_data};
use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext;
use std::collections::HashMap;

use super::shared::MakerNoteParser;
use super::shared::engine_value::engine_value_text;

/// MakerNotes.pm:275-287, after the two earlier KDK variants: both Type2
/// signatures are independent of EXIF Make. The first has eight arbitrary
/// bytes before `Eastman Kodak`; the second is the exact byte-shaped header.
pub(crate) fn is_type2(data: &[u8]) -> bool {
    if data.get(8..21) == Some(b"Eastman Kodak") {
        return true;
    }
    let Some(header) = data.get(..12) else {
        return false;
    };
    header[0] == 1
        && header[1] == 0
        && (header[2] == 0 || header[2] == 1)
        && header[3..6] == [0, 0, 0]
        && header[6..8] == [4, 0]
        && header[8..12].iter().all(u8::is_ascii_alphabetic)
}

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
    /// Kodak.pm:85-91: `int8u[4]`, formatted as hh:mm:ss.hh.
    pub(super) const TIME_CREATED: usize = 0x14;
    /// Kodak.pm:225-230: `int16u`, `ValueConv => '$val / 100'`.
    pub const TOTAL_ZOOM: usize = 0x62;
    /// Kodak.pm:231-235: `int16u`, zero is `Off`.
    pub(super) const DATE_TIME_STAMP: usize = 0x64;
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

    fn type2_rows(
        &self,
        data: &[u8],
        cond_ctx: &mut Ctx<'_>,
    ) -> Vec<crate::exiftool_tables::Emitted> {
        let mut rows = Vec::new();
        if let Some(table) = find_table("Kodak", "Type2") {
            // MakerNotes.pm:286 pins Kodak::Type2 to BigEndian, regardless of
            // the enclosing TIFF order. The generated table owns all offsets.
            process_binary_data(
                table,
                Dir::whole(data, ByteOrder::BigEndian.to_io_byte_order()),
                cond_ctx,
                &mut rows,
            );
        }
        rows
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

        // TimeCreated: Kodak.pm's `%.2d:%.2d:%.2d.%.2d` ValueConv.
        if let Some(bytes) = record.get(field_offset::TIME_CREATED..field_offset::TIME_CREATED + 4)
        {
            tags.insert(
                "Kodak:TimeCreated".to_string(),
                format!(
                    "{:02}:{:02}:{:02}.{:02}",
                    bytes[0], bytes[1], bytes[2], bytes[3]
                ),
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

        if let Some(v) = reader.u16_at(field_offset::DATE_TIME_STAMP) {
            tags.insert(
                "Kodak:DateTimeStamp".to_string(),
                if v == 0 {
                    "Off".to_string()
                } else {
                    format!("Mode {v}")
                },
            );
        }
    }

    fn main_occurrences(&self, ctx: &MakerNoteContext<'_>) -> Vec<(String, TagOccurrence)> {
        let data = ctx.payload();
        let order = if data.starts_with(b"KDK INFO") {
            ByteOrder::BigEndian
        } else if data.starts_with(b"KDK") {
            ByteOrder::LittleEndian
        } else {
            return Vec::new();
        };
        let Some(record) = data.get(KODAK_MAIN_START..) else {
            return Vec::new();
        };
        let reader = EndianReader::new(record, order.to_io_byte_order());
        let mut rows = Vec::new();

        if let Some(bytes) = record.get(field_offset::TIME_CREATED..field_offset::TIME_CREATED + 4)
        {
            let raw = TagValue::new_string(format!(
                "{} {} {} {}",
                bytes[0], bytes[1], bytes[2], bytes[3]
            ));
            let stored = TagValue::Array(
                bytes
                    .iter()
                    .map(|byte| TagValue::Integer(i64::from(*byte)))
                    .collect(),
            );
            let value = TagValue::new_string(format!(
                "{:02}:{:02}:{:02}.{:02}",
                bytes[0], bytes[1], bytes[2], bytes[3]
            ));
            rows.push((
                "Kodak:TimeCreated".to_string(),
                kodak_occurrence(
                    0x0014,
                    "TimeCreated",
                    "Time",
                    stored,
                    raw,
                    value.clone(),
                    value,
                    ctx.payload_base() + (KODAK_MAIN_START + field_offset::TIME_CREATED) as u64,
                    4,
                ),
            ));
        }

        if let Some(v) = reader.u16_at(field_offset::DATE_TIME_STAMP) {
            let raw = TagValue::Integer(i64::from(v));
            rows.push((
                "Kodak:DateTimeStamp".to_string(),
                kodak_occurrence(
                    0x0064,
                    "DateTimeStamp",
                    "Camera",
                    raw.clone(),
                    raw.clone(),
                    raw,
                    TagValue::new_string(if v == 0 {
                        "Off".to_string()
                    } else {
                        format!("Mode {v}")
                    }),
                    ctx.payload_base() + (KODAK_MAIN_START + field_offset::DATE_TIME_STAMP) as u64,
                    2,
                ),
            ));
        }
        rows
    }
}

#[allow(clippy::too_many_arguments)]
fn kodak_occurrence(
    id: u16,
    name: &'static str,
    group2: &'static str,
    stored: TagValue,
    raw: TagValue,
    value: TagValue,
    print: TagValue,
    byte_start: u64,
    byte_len: u64,
) -> TagOccurrence {
    TagOccurrence {
        id: crate::core::TagId::Numeric(id),
        name: intern(name),
        group0: intern("MakerNotes"),
        group1: intern("Kodak"),
        group2: Some(intern(group2)),
        instance: Instance::default(),
        stored: Some(stored),
        raw,
        value: Some(value),
        print: Some(print),
        priority: 1,
        is_list: false,
        order: 0,
        origin: Provenance {
            module: Some("Kodak"),
            table: Some("Main"),
            byte_range: Some(byte_start..byte_start + byte_len),
        },
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
        if is_type2(data) {
            let mut members = HashMap::new();
            let mut cond_ctx = Ctx::new(&mut members);
            for row in self.type2_rows(data, &mut cond_ctx) {
                if let Some(text) = engine_value_text(&row.value) {
                    tags.insert(format!("{}:{}", row.group1, row.name), text);
                }
            }
            return Ok(());
        }
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

    fn parse_with_context_and_values_and_session_and_occurrences(
        &self,
        ctx: &MakerNoteContext<'_>,
        byte_order: ByteOrder,
        model: Option<&str>,
        _session: &mut Session,
        _cond_ctx: &mut Ctx<'_>,
        tags: &mut HashMap<String, String>,
        _value_forms: &mut HashMap<String, String>,
        occurrences: &mut Vec<(String, TagOccurrence)>,
    ) -> Result<(), String> {
        if is_type2(ctx.payload()) {
            for row in self.type2_rows(ctx.payload(), _cond_ctx) {
                let key = format!("{}:{}", row.group1, row.name);
                let value = row.value_conv.clone().unwrap_or_else(|| row.value.clone());
                occurrences.push((
                    key,
                    TagOccurrence {
                        id: row.source_id,
                        name: intern(row.name),
                        group0: intern(row.group0),
                        group1: intern(row.group1),
                        group2: (!row.group2.is_empty()).then(|| intern(row.group2)),
                        instance: Instance::default(),
                        raw: row.stored.clone(),
                        value: Some(value),
                        print: Some(row.value),
                        stored: Some(row.stored),
                        priority: u8::from(!(row.low_priority || row.avoid)),
                        is_list: row.is_list,
                        order: 0,
                        origin: Provenance {
                            module: Some(row.module),
                            table: Some(row.table),
                            byte_range: None,
                        },
                    },
                ));
            }
            return Ok(());
        }
        self.parse_with_model(ctx.payload(), byte_order, model, tags)?;
        let rows = self.main_occurrences(ctx);
        if rows.iter().any(|(key, _)| key == "Kodak:TimeCreated") {
            tags.remove("Kodak:TimeCreated");
        }
        if rows.iter().any(|(key, _)| key == "Kodak:DateTimeStamp") {
            tags.remove("Kodak:DateTimeStamp");
        }
        occurrences.extend(rows);
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
        record[0x14..0x18].copy_from_slice(&[10, 22, 28, 62]);
        record[0x62..0x64].copy_from_slice(&140u16.to_be_bytes());
        record[0x64..0x66].copy_from_slice(&0u16.to_be_bytes());
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
        assert_eq!(
            tags.get("Kodak:TimeCreated"),
            Some(&"10:22:28.62".to_string())
        );
        assert_eq!(tags.get("Kodak:DateTimeStamp"), Some(&"Off".to_string()));
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
