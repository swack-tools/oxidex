//! WordPerfect Graphics (WPG) record-list extraction.
//!
//! This deliberately implements only ExifTool's version-1 `Records` tag from
//! `Image::ExifTool::WPG::ProcessWPG`: record headers begin at the header's
//! declared offset, have a one-byte type and WPG variable-length payload size,
//! and adjacent equal types are collapsed in the displayed list.

use crate::core::{FileReader, MetadataMap, TagValue};

const WPG_SIGNATURE: &[u8] = b"\xFFWPC";
const WPG_HEADER_LEN: usize = 16;

/// Reads WPG's `ReadVarInt`: either one byte, or `0xff` followed by a
/// little-endian u16, optionally extended by another little-endian u16.
fn read_var_int(data: &[u8], cursor: &mut usize) -> Option<usize> {
    let first = *data.get(*cursor)?;
    *cursor += 1;
    if first != 0xff {
        return Some(usize::from(first));
    }

    let low = u16::from_le_bytes([*data.get(*cursor)?, *data.get(*cursor + 1)?]);
    *cursor += 2;
    if low & 0x8000 == 0 {
        return Some(usize::from(low));
    }

    let high = u16::from_le_bytes([*data.get(*cursor)?, *data.get(*cursor + 1)?]);
    *cursor += 2;
    Some(((usize::from(low & 0x7fff)) << 16) | usize::from(high))
}

/// ExifTool's `%Image::ExifTool::WPG::Main` `Records` PrintConv table.
fn record_name(record_type: u8) -> String {
    let name = match record_type {
        0x01 => "Fill Attributes",
        0x02 => "Line Attributes",
        0x03 => "Marker Attributes",
        0x04 => "Polymarker",
        0x05 => "Line",
        0x06 => "Polyline",
        0x07 => "Rectangle",
        0x08 => "Polygon",
        0x09 => "Ellipse",
        0x0a => "Reserved",
        0x0b => "Bitmap (Type 1)",
        0x0c => "Graphics Text (Type 1)",
        0x0d => "Graphics Text Attributes",
        0x0e => "Color Map",
        0x0f => "Start WPG (Type 1)",
        0x10 => "End WPG",
        0x11 => "PostScript Data (Type 1)",
        0x12 => "Output Attributes",
        0x13 => "Curved Polyline",
        0x14 => "Bitmap (Type 2)",
        0x15 => "Start Figure",
        0x16 => "Start Chart",
        0x17 => "PlanPerfect Data",
        0x18 => "Graphics Text (Type 2)",
        0x19 => "Start WPG (Type 2)",
        0x1a => "Graphics Text (Type 3)",
        0x1b => "PostScript Data (Type 2)",
        _ => return format!("Unknown (0x{record_type:02x})"),
    };
    name.to_string()
}

fn format_records(record_types: &[u8]) -> Vec<String> {
    let mut records = Vec::new();
    let mut index = 0;

    while index < record_types.len() {
        let record_type = record_types[index];
        let mut count = 1;
        while index + count < record_types.len() && record_types[index + count] == record_type {
            count += 1;
        }

        let mut name = record_name(record_type);
        if count > 1 {
            name.push_str(&format!(" x {count}"));
        }
        records.push(name);
        index += count;
    }

    records
}

/// Parses only WPG v1's ExifTool-compatible `WPG:Records` list.
pub fn parse_wpg_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    if reader.size() < WPG_HEADER_LEN as u64 {
        return Err("WPG file too short".to_string());
    }

    let header = reader
        .read(0, WPG_HEADER_LEN)
        .map_err(|error| error.to_string())?;
    if header[0..4] != *WPG_SIGNATURE {
        return Err("Invalid WPG signature".to_string());
    }

    let mut metadata = MetadataMap::new();
    // `Records` is a WPG 1.0 tag. WPG 2.0 has the separate `RecordsV2` tag,
    // which is intentionally out of this single-tag change.
    if header[10] != 1 {
        return Ok(metadata);
    }

    let declared_offset = u64::from(u32::from_le_bytes([
        header[4], header[5], header[6], header[7],
    ]));
    let records_offset = declared_offset.max(WPG_HEADER_LEN as u64);
    if records_offset >= reader.size() {
        return Ok(metadata);
    }

    let remaining = usize::try_from(reader.size() - records_offset)
        .map_err(|_| "WPG file is too large to parse".to_string())?;
    let data = reader
        .read(records_offset, remaining)
        .map_err(|error| error.to_string())?;

    let mut cursor = 0;
    let mut record_types = Vec::new();
    while let Some(&record_type) = data.get(cursor) {
        cursor += 1;
        let Some(length) = read_var_int(&data, &mut cursor) else {
            break;
        };
        let Some(next) = cursor.checked_add(length) else {
            break;
        };
        if next > data.len() {
            break;
        }
        cursor = next;
        if record_type == 0 {
            break;
        }
        record_types.push(record_type);
    }

    if !record_types.is_empty() {
        metadata.insert(
            "WPG:Records".to_string(),
            TagValue::Array(
                format_records(&record_types)
                    .into_iter()
                    .map(TagValue::String)
                    .collect(),
            ),
        );
    }
    Ok(metadata)
}
