//! DJI debug metadata carried in JPEG APP7 (`Image::ExifTool::DJI::Info`).

use crate::core::tag_occurrence::shim_group_priority;
use crate::core::{Instance, MetadataMap, TagValue};
use crate::parsers::tiff::makernotes::dji::{DjiInfoValue, process_dji_info};

const HEADER: &[u8] = b"DJI-DBG\0";

/// Extracts every `DJI::Info` record from a DJI-DBG APP7 payload.
///
/// ExifTool.pm:8224-8230 (pinned 13.59) selects `DJI::Info` only when APP7
/// starts with `DJI-DBG\0`, starts the directory after that 8-byte header,
/// and forces family-0 group `APP7`; the table's own family-1 group is
/// `DJI`. `ProcessDJIInfo` then reads the contiguous bracketed records (see
/// [`process_dji_info`]): printable values are text with trailing NULs
/// removed, anything else stays binary.
pub fn parse_dji_dbg_app7(data: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let Some(records) = data.strip_prefix(HEADER) else {
        return metadata;
    };

    for (name, value) in process_dji_info(records) {
        let value = match value {
            DjiInfoValue::Text(text) => TagValue::String(text),
            DjiInfoValue::Binary(bytes) => TagValue::Binary(bytes),
        };
        metadata.insert_occurrence(
            format!("APP7:{name}"),
            value,
            shim_group_priority("APP7"),
            "DJI",
            Instance::default(),
        );
    }

    metadata
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(data: &mut Vec<u8>, id: &[u8], value: &[u8]) {
        data.push(b'[');
        data.extend_from_slice(id);
        data.push(b':');
        data.extend_from_slice(value);
        data.push(b']');
    }

    #[test]
    fn known_and_make_tag_info_records_are_all_extracted() {
        let mut data = HEADER.to_vec();
        record(&mut data, b"ae_dbg_info", &[1, b']', 2, 3]);
        record(&mut data, b"sensor_id", b"98JFN4G5S00G2S\0\0");
        record(&mut data, b"awb_dbg_data_v2", &[0; 16]);
        record(&mut data, b"scap_info", &[9; 4]);

        let metadata = parse_dji_dbg_app7(&data);

        // The `]` inside the first payload is not followed by `[`, so it
        // does not end the record (DJI.pm:971's lookahead).
        assert_eq!(
            metadata.get("APP7:AEDebugInfo"),
            Some(&TagValue::Binary(vec![1, b']', 2, 3]))
        );
        assert_eq!(metadata.get_string("APP7:SensorID"), Some("98JFN4G5S00G2S"));
        assert_eq!(
            metadata.get("APP7:Awb_Dbg_Data_V2"),
            Some(&TagValue::Binary(vec![0; 16]))
        );
        assert_eq!(
            metadata.get("APP7:Scap_Info"),
            Some(&TagValue::Binary(vec![9; 4]))
        );
        assert_eq!(metadata.len(), 4);
    }

    #[test]
    fn payload_without_header_is_not_dji_info() {
        let mut data = Vec::new();
        record(&mut data, b"sensor_id", b"X1");
        assert!(parse_dji_dbg_app7(&data).is_empty());
    }
}
