//! Canon `FilterInfo` (MakerNote tag 0x4024) -- creative filter settings.
//!
//! This is the one Canon sub-table that is not a flat `BinaryData` array, which
//! is why PR #241 deliberately left it out of `binary_tables`. `%Canon::FilterInfo`
//! declares its own `PROCESS_PROC => \&ProcessFilters` (Canon.pm:9203) and its
//! keys are parameter ids that appear inside a self-describing record, not
//! indices into the record. The record is:
//!
//! ```text
//!   4 bytes  - (unused by ProcessFilters)
//!   4 bytes  - number of filters
//!   per filter:
//!     4 bytes  - filter number
//!     4 bytes  - filter data length, measured from the number-of-parameters word
//!     4 bytes  - number of parameters
//!     per parameter:
//!       4 bytes         - parameter id, which is the table key
//!       4 bytes         - value count
//!       4 * count bytes - int32s values
//! ```
//!
//! The table itself is transcribed by script alongside the CameraInfo tables;
//! see [`super::camera_info_tables`]. Only the walk lives here.

use std::collections::HashMap;

use super::camera_info::{Conv, print_conv};
use super::camera_info_tables::TBL_FILTERINFO_REF;
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// Fixed header before the first filter: four unused bytes then the count.
const HEADER_LEN: usize = 8;
/// Each filter opens with number, data length and parameter count.
const FILTER_HEADER_LEN: usize = 12;
/// Each parameter opens with its id and its value count.
const PARAM_HEADER_LEN: usize = 8;

/// Walks a `FilterInfo` record, a direct transliteration of
/// `Image::ExifTool::Canon::ProcessFilters` (Canon.pm:10810).
///
/// ExifTool abandons the record at the first structural inconsistency rather
/// than resynchronising, and so does this: a truncated or over-long filter ends
/// the walk with whatever was already read, because guessing where the next
/// filter starts would emit real tag names over the wrong bytes.
pub(crate) fn parse_filter_info(
    data: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    if data.len() < HEADER_LEN {
        return;
    }
    let le = matches!(byte_order, ByteOrder::LittleEndian);
    let read_u32 = |at: usize| -> Option<u32> {
        let b: [u8; 4] = data.get(at..at + 4)?.try_into().ok()?;
        Some(if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };

    let end = data.len();
    let Some(num_filters) = read_u32(4) else {
        return;
    };
    let mut pos = HEADER_LEN;

    for _ in 0..num_filters {
        if pos + FILTER_HEADER_LEN > end {
            return; // "Truncated data for filter N"
        }
        let (Some(size), Some(nparm)) = (read_u32(pos + 4), read_u32(pos + 8)) else {
            return;
        };
        // The length counts from the parameter-count word, so the next filter
        // starts at pos + 4 + size, not pos + 12 + size.
        let Some(next) = (pos + 4).checked_add(size as usize) else {
            return;
        };
        if next > end {
            return; // "Invalid size for filter N"
        }
        pos += FILTER_HEADER_LEN;

        for _ in 0..nparm {
            if pos + PARAM_HEADER_LEN > end {
                return; // "Truncated data for filter N param M"
            }
            let (Some(tag), Some(count)) = (read_u32(pos), read_u32(pos + 4)) else {
                return;
            };
            pos += PARAM_HEADER_LEN;
            let Some(value_len) = (count as usize).checked_mul(4) else {
                return;
            };
            if pos + value_len > end {
                return; // "Truncated value for filter N param M"
            }
            if let Some(field) = TBL_FILTERINFO_REF
                .fields
                .iter()
                .find(|f| f.idx == tag as i64)
            {
                // `ReadValue(..., 'int32s', $count, ...)` joins multiple values
                // with a single space.
                let values: Vec<String> = (0..count as usize)
                    .filter_map(|i| read_u32(pos + i * 4).map(|v| (v as i32).to_string()))
                    .collect();
                if values.len() == count as usize && !values.is_empty() {
                    let joined = values.join(" ");
                    let conv = if values.len() == 1 {
                        match joined.parse::<i64>() {
                            Ok(n) => Conv::Int(n),
                            Err(_) => Conv::Str(joined),
                        }
                    } else {
                        Conv::Str(joined)
                    };
                    tags.insert(format!("Canon:{}", field.name), print_conv(field.pc, conv));
                }
            }
            pos += value_len;
        }
        pos = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One filter carrying one parameter, laid out exactly as `ProcessFilters`
    /// reads it. `size` is measured from the parameter-count word, which is the
    /// detail that decides where the next filter begins.
    fn record(params: &[(u32, &[i32])]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&(params.len() as u32).to_le_bytes());
        for (id, values) in params {
            body.extend_from_slice(&id.to_le_bytes());
            body.extend_from_slice(&(values.len() as u32).to_le_bytes());
            for v in *values {
                body.extend_from_slice(&v.to_le_bytes());
            }
        }
        let mut out = Vec::new();
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes()); // one filter
        out.extend_from_slice(&1u32.to_le_bytes()); // filter number
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn reads_filter_parameters() {
        let mut tags = HashMap::new();
        parse_filter_info(
            &record(&[(0x101, &[-1]), (0x401, &[2]), (0x402, &[1]), (0x403, &[7])]),
            ByteOrder::LittleEndian,
            &mut tags,
        );
        // %filterConv: -1 is 'Off', anything else is "On ($val)".
        assert_eq!(
            tags.get("Canon:GrainyBWFilter").map(String::as_str),
            Some("Off")
        );
        assert_eq!(
            tags.get("Canon:MiniatureFilter").map(String::as_str),
            Some("On (2)")
        );
        assert_eq!(
            tags.get("Canon:MiniatureFilterOrientation")
                .map(String::as_str),
            Some("Vertical")
        );
        assert_eq!(
            tags.get("Canon:MiniatureFilterPosition")
                .map(String::as_str),
            Some("7")
        );
    }

    /// A filter whose declared length runs past the record must stop the walk,
    /// not be clamped: continuing would read the next parameter id out of
    /// whatever bytes happen to follow.
    #[test]
    fn truncated_record_stops_rather_than_guessing() {
        let mut good = record(&[(0x101, &[-1])]);
        // Overstate the filter's size by 64 bytes.
        let size = u32::from_le_bytes(good[12..16].try_into().unwrap());
        good[12..16].copy_from_slice(&(size + 64).to_le_bytes());
        let mut tags = HashMap::new();
        parse_filter_info(&good, ByteOrder::LittleEndian, &mut tags);
        assert!(tags.is_empty(), "{:?}", tags);
    }

    #[test]
    fn empty_and_short_records_are_ignored() {
        for len in 0..8 {
            let mut tags = HashMap::new();
            parse_filter_info(&vec![0u8; len], ByteOrder::LittleEndian, &mut tags);
            assert!(tags.is_empty());
        }
    }
}
