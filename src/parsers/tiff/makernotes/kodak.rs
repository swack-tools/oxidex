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
//! Both walk the transcribed `exiftool_tables::find_table("Kodak","Main")`
//! over the `Start`-shifted record. Its `FIRST_ENTRY => 8` only bounds the
//! synthetic-tag range `-U` walks (`ExifTool.pm:9901-9906`); every field sits
//! at `index * increment` from the record start (`ProcessBinaryData`), which
//! is what the engine reads -- verified field by field against the pinned
//! oracle's `-G1 -a -s` on `t/images/Kodak.jpg`. The hand reader this
//! replaces read eight of the table's 25 fields, so `MeteringMode`,
//! `ExposureTime` and `FNumber` never reached the file and the ExifIFD copies
//! answered a bare `-MeteringMode` that ExifTool answers from `Kodak::Main`
//! (found later, at the MakerNote's position in the ExifIFD, at the same
//! default priority -- `ExifTool.pm:9564`).

#![allow(dead_code)]

use crate::core::tag_occurrence::intern;
use crate::core::{Instance, Provenance, TagOccurrence};
use crate::exiftool_tables::Ctx;
use crate::exiftool_tables::session::Session;
use crate::exiftool_tables::{Dir, find_table, process_binary_data};
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

    /// Walks `Kodak::Main` (Kodak.pm:36-227, `ProcessBinaryData`) over the
    /// `Start`-shifted record through the transcribed table. The byte order
    /// is the signature's (see the module doc comment), not the enclosing
    /// TIFF's. `None` for a payload that is neither Kodak1a nor Kodak1b.
    fn main_rows(
        &self,
        data: &[u8],
        cond_ctx: &mut Ctx<'_>,
    ) -> Option<Vec<crate::exiftool_tables::Emitted>> {
        let order = if data.starts_with(b"KDK INFO") {
            ByteOrder::BigEndian
        } else if data.starts_with(b"KDK") {
            ByteOrder::LittleEndian
        } else {
            return None;
        };
        let record = data.get(KODAK_MAIN_START..)?;
        let mut rows = Vec::new();
        if let Some(table) = find_table("Kodak", "Main") {
            process_binary_data(
                table,
                Dir::whole(record, order.to_io_byte_order()),
                cond_ctx,
                &mut rows,
            );
        }
        Some(rows)
    }
}

/// One engine row as the occurrence the MakerNote merge records, at the
/// row's own `FoundTag` priority (`PRIORITY => 0` / `Avoid`, ExifTool.pm:9469-9473).
fn engine_occurrence(row: crate::exiftool_tables::Emitted) -> (String, TagOccurrence) {
    let key = format!("{}:{}", row.group1, row.name);
    let value = row.value_conv.clone().unwrap_or_else(|| row.value.clone());
    (
        key,
        TagOccurrence {
            id: row.source_id,
            name: intern(row.name),
            group0: intern(row.group0),
            group1: intern(row.group1),
            group2: (!row.group2.is_empty()).then(|| intern(row.group2)),
            instance: Instance::default(),
            raw: row.value.clone(),
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
    )
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
        let mut members = HashMap::new();
        let mut cond_ctx = Ctx::new(&mut members);
        let rows = if is_type2(data) {
            self.type2_rows(data, &mut cond_ctx)
        } else {
            // Not a Kodak1a/1b payload (could be Type3/4/5/6 or another
            // vendor's rebrand) -- none of those are implemented here.
            self.main_rows(data, &mut cond_ctx).unwrap_or_default()
        };
        for row in rows {
            if let Some(text) = engine_value_text(&row.value) {
                tags.insert(format!("{}:{}", row.group1, row.name), text);
            }
        }
        Ok(())
    }

    fn parse_with_context_and_values_and_session_and_occurrences(
        &self,
        ctx: &MakerNoteContext<'_>,
        _byte_order: ByteOrder,
        _model: Option<&str>,
        _session: &mut Session,
        cond_ctx: &mut Ctx<'_>,
        _tags: &mut HashMap<String, String>,
        _value_forms: &mut HashMap<String, String>,
        occurrences: &mut Vec<(String, TagOccurrence)>,
    ) -> Result<(), String> {
        let rows = if is_type2(ctx.payload()) {
            self.type2_rows(ctx.payload(), cond_ctx)
        } else {
            self.main_rows(ctx.payload(), cond_ctx).unwrap_or_default()
        };
        occurrences.extend(rows.into_iter().map(engine_occurrence));
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

    /// `engine_occurrence`'s `raw` must carry the display form
    /// (`row.value`, PrintConv'd), matching every other generated adapter
    /// (`panasonic_generated_occurrence`) -- not `row.stored`, the file's
    /// typed source value. `MetadataMap::get`/`iter`/serialization all
    /// project through `raw` (`TagSink::get`, `core/tag_sink.rs`); only the
    /// CLI resolver explicitly re-selects `print`, so this bug was invisible
    /// there. `Kodak::Main`'s `MeteringMode` (index 28, `IntEnum(0 =>
    /// "Multi-segment", 1 => "Center-weighted average", 2 => "Spot")`) is
    /// stored as a plain integer, so a `raw: row.stored` regression would
    /// leave `TagValue::Integer(1)` where ExifTool's label belongs.
    #[test]
    fn test_engine_occurrence_raw_is_the_display_form_not_the_stored_form() {
        use crate::core::TagValue;

        let mut data = b"KDK INFO".to_vec();
        let mut record = vec![0u8; 108];
        record[28] = 1; // MeteringMode: Center-weighted average
        data.extend_from_slice(&record);

        let mut members = HashMap::new();
        let mut cond_ctx = Ctx::new(&mut members);
        let parser = KodakParser::new();
        let rows = parser
            .main_rows(&data, &mut cond_ctx)
            .expect("KDK INFO payload parses as Kodak::Main");
        let metering_row = rows
            .into_iter()
            .find(|row| row.name == "MeteringMode")
            .expect("Kodak::Main emits MeteringMode");
        assert_eq!(metering_row.stored, TagValue::Integer(1));

        let (key, occurrence) = engine_occurrence(metering_row);
        assert_eq!(key, "Kodak:MeteringMode");
        assert_eq!(
            occurrence.raw,
            TagValue::new_string("Center-weighted average"),
            "raw must be the PrintConv'd display form, not the stored integer"
        );
    }
}
