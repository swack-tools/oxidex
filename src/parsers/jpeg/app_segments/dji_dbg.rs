//! DJI debug metadata carried in JPEG APP7 (`Image::ExifTool::DJI::Info`).

use crate::core::{MetadataMap, TagValue};

const HEADER: &[u8] = b"DJI-DBG\0";

/// Extracts known `DJI::Info` records from a DJI-DBG APP7 payload.
///
/// ExifTool selects `DJI::Info` only when APP7 starts with `DJI-DBG\0`, then
/// `ProcessDJIInfo` accepts contiguous bracketed records. Its printable-value
/// path removes only trailing NUL bytes; non-printable values remain binary.
pub fn parse_dji_dbg_app7(data: &[u8]) -> MetadataMap {
    data.strip_prefix(HEADER)
        .map_or_else(MetadataMap::new, |records| {
            parse_dji_info_records_in_group(records, "APP7")
        })
}

/// Extracts the bracketed record stream used by `DJI::Info`.
///
/// ExifTool uses the same table for an APP7 `DJI-DBG\0` payload and for the
/// `MakerNoteDJIInfo` EXIF value.  The latter starts directly with `[`.
pub fn parse_dji_info_records(records: &[u8]) -> MetadataMap {
    parse_dji_info_records_in_group(records, "APP7")
}

/// Extracts the bracketed `DJI::Info` stream under its ExifTool group.
pub fn parse_dji_info_records_in_group(records: &[u8], group: &str) -> MetadataMap {
    let mut metadata = MetadataMap::new();

    let mut offset = 0;
    while records.get(offset) == Some(&b'[') {
        let mut candidate = offset + 1;
        let end = loop {
            let Some(relative) = records[candidate..].iter().position(|byte| *byte == b']') else {
                break None;
            };
            candidate += relative;
            if records.get(candidate + 1).is_none_or(|byte| *byte == b'[') {
                break Some(candidate);
            }
            candidate += 1;
        };
        let Some(end) = end else { break };

        let record = &records[offset + 1..end];
        let mut parts = record.splitn(2, |byte| *byte == b':');
        let Some((name, value)) =
            parts
                .next()
                .zip(parts.next())
                .and_then(|(name, value)| match name {
                    b"sensor_id" => Some(("SensorID", value)),
                    b"GimbalDegree(Y,P,R)" => Some(("GimbalDegree", value)),
                    b"FlightDegree(Y,P,R)" => Some(("FlightDegree", value)),
                    b"FlightSpeed(X,Y,Z)" => Some(("FlightSpeed", value)),
                    b"ae_dbg_info" => Some(("AEDebugInfo", value)),
                    b"ae_histogram_info" => Some(("AEHistogramInfo", value)),
                    b"ae_local_histogram" => Some(("AELocalHistogram", value)),
                    b"ae_liveview_histogram_info" => Some(("AELiveViewHistogramInfo", value)),
                    b"ae_liveview_local_histogram" => Some(("AELiveViewLocalHistogram", value)),
                    b"awb_dbg_info" => Some(("AWBDebugInfo", value)),
                    b"af_dbg_info" => Some(("AFDebugInfo", value)),
                    b"hiso" => Some(("Histogram", value)),
                    b"xidiri" => Some(("Xidiri", value)),
                    b"adj_dbg_info" => Some(("ADJDebugInfo", value)),
                    b"hyperlapse_dbg_info" => Some(("HyperlapsDebugInfo", value)),
                    _ => None,
                })
        else {
            offset = end + 1;
            continue;
        };

        {
            let printable = value
                .iter()
                .rposition(|byte| *byte != 0)
                .map(|last| &value[..=last]);
            if printable.is_some_and(|value| {
                !value.is_empty() && value.iter().all(|byte| (0x20..=0x7e).contains(byte))
            }) {
                let value = printable.expect("checked above");
                metadata.insert(
                    format!("{group}:{name}"),
                    TagValue::String(String::from_utf8_lossy(value).into_owned()),
                );
            } else {
                metadata.insert(format!("{group}:{name}"), TagValue::Binary(value.to_vec()));
            }
        }
        offset = end + 1;
    }

    metadata
}
