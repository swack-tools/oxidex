//! DJI debug metadata carried in JPEG APP7 (`Image::ExifTool::DJI::Info`).

use crate::core::{MetadataMap, TagValue};

const HEADER: &[u8] = b"DJI-DBG\0";

/// Extracts the known `sensor_id` record from a DJI-DBG APP7 payload.
///
/// ExifTool selects `DJI::Info` only when APP7 starts with `DJI-DBG\0`, then
/// `ProcessDJIInfo` accepts contiguous bracketed records. Its printable-value
/// path removes only trailing NUL bytes; non-printable values remain binary.
pub fn parse_dji_dbg_app7(data: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let Some(records) = data.strip_prefix(HEADER) else {
        return metadata;
    };

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
        if let Some(value) = record.strip_prefix(b"sensor_id:") {
            let printable = value
                .iter()
                .rposition(|byte| *byte != 0)
                .map(|last| &value[..=last]);
            if printable.is_some_and(|value| {
                !value.is_empty() && value.iter().all(|byte| (0x20..=0x7e).contains(byte))
            }) {
                let value = printable.expect("checked above");
                metadata.insert(
                    "APP7:SensorID",
                    TagValue::String(String::from_utf8_lossy(value).into_owned()),
                );
            } else {
                metadata.insert("APP7:SensorID", TagValue::Binary(value.to_vec()));
            }
        }
        offset = end + 1;
    }

    metadata
}
