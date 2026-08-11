//! Olympus Digital Speech Standard (DSS) metadata parser.
//!
//! ExifTool routes standalone DSS files through `Olympus::DSS`, whose
//! generated binary table declares the EndTime field. This parser intentionally
//! reads only that assigned field.

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::exiftool_tables::{
    Acknowledged, DecodedValue, PerlCitation, RawAccess, decode_binary_table, find_table,
};
use crate::io::ByteOrder;

const DSS_SIGNATURE: &[u8] = b"\x02dss";
const DS2_SIGNATURE: &[u8] = b"\x03ds2";
const DSS_PROCESS_PROBE_LEN: u64 = 69;
const DSS_EXIFTOOL_READ_LEN: u64 = 898;

/// Extract Olympus DSS `EndTime` using ExifTool's declared `Olympus::DSS`
/// binary layout and date conversion.
pub fn parse_dss_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    if reader.size() < DSS_PROCESS_PROBE_LEN {
        return Err("DSS file is too short for the Olympus DSS header".to_string());
    }

    let read_len = reader.size().min(DSS_EXIFTOOL_READ_LEN) as usize;
    let data = reader
        .read(0, read_len)
        .map_err(|error| error.to_string())?;
    if !data.starts_with(DSS_SIGNATURE) && !data.starts_with(DS2_SIGNATURE) {
        return Err("invalid DSS signature".to_string());
    }

    // Olympus.pm's `EndTime` carries a `ValueConv` this schema does not
    // reproduce (12-digit `YYMMDDhhmmss` -> `20YY:MM:DD hh:mm:ss`), so
    // `Field::omitted.value_conv` is set; `format_dss_datetime` below is the
    // hand-verified equivalent, and this citation is RawAccess's required
    // acknowledgment.
    const END_TIME_CITATION: PerlCitation = PerlCitation {
        module: "Olympus",
        table: "DSS",
        tag: "EndTime",
        lines: "ValueConv, Olympus.pm",
    };

    let table = find_table("Olympus", "DSS").ok_or("missing Olympus::DSS table")?;
    let decode = decode_binary_table(table, &data, ByteOrder::Little);
    let end_time = decode
        .fields()
        .iter()
        .find(|decoded| decoded.field.name == "EndTime")
        .and_then(|decoded| RawAccess::new(decoded, Acknowledged::VALUE_CONV, &END_TIME_CITATION))
        .and_then(|access| match access.raw() {
            DecodedValue::String(value) => Some(value.clone()),
            _ => None,
        });

    let mut metadata = MetadataMap::new();
    if let Some(end_time) = end_time {
        metadata.insert(
            "Olympus:EndTime".to_string(),
            TagValue::new_string(format_dss_datetime(&end_time)),
        );
    }
    Ok(metadata)
}

/// Mirrors `Olympus.pm`'s `ValueConv`: transform a 12-digit `YYMMDDhhmmss`
/// string into `20YY:MM:DD hh:mm:ss`; otherwise leave the raw string intact.
fn format_dss_datetime(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() == 12 && bytes.iter().all(u8::is_ascii_digit) {
        format!(
            "20{}:{}:{} {}:{}:{}",
            &value[0..2],
            &value[2..4],
            &value[4..6],
            &value[6..8],
            &value[8..10],
            &value[10..12],
        )
    } else {
        value.to_string()
    }
}
