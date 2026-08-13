//! Canon CIFF records embedded in JPEG APP0 segments.
//!
//! Canon's early PowerShot cameras put a complete CIFF (`HEAPJPGM`)
//! directory in a second APP0 record. CIFF is not TIFF: each directory is a
//! block whose final four bytes point back to a table of ten-byte entries.

use crate::core::{MetadataMap, TagValue};
use std::collections::HashSet;

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

impl Endian {
    fn u16(self, bytes: &[u8]) -> Option<u16> {
        let bytes: [u8; 2] = bytes.get(..2)?.try_into().ok()?;
        Some(match self {
            Self::Little => u16::from_le_bytes(bytes),
            Self::Big => u16::from_be_bytes(bytes),
        })
    }

    fn u32(self, bytes: &[u8]) -> Option<u32> {
        let bytes: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
        Some(match self {
            Self::Little => u32::from_le_bytes(bytes),
            Self::Big => u32::from_be_bytes(bytes),
        })
    }

    fn i32(self, bytes: &[u8]) -> Option<i32> {
        self.u32(bytes).map(|value| value as i32)
    }

    fn f32(self, bytes: &[u8]) -> Option<f32> {
        self.u32(bytes).map(f32::from_bits)
    }
}

/// Parses an APP0 payload when it contains the CIFF `HEAPJPGM` signature.
pub fn parse_ciff_app0(data: &[u8]) -> MetadataMap {
    let (endian, header_len) = match data.get(..14) {
        Some(header) if &header[..2] == b"II" && &header[6..14] == b"HEAPJPGM" => {
            (Endian::Little, Endian::Little.u32(&header[2..6]))
        }
        Some(header) if &header[..2] == b"MM" && &header[6..14] == b"HEAPJPGM" => {
            (Endian::Big, Endian::Big.u32(&header[2..6]))
        }
        _ => return MetadataMap::new(),
    };
    let Some(header_len) = header_len.map(|value| value as usize) else {
        return MetadataMap::new();
    };
    if header_len >= data.len() {
        return MetadataMap::new();
    }

    let mut metadata = MetadataMap::new();
    let mut visited = HashSet::new();
    parse_directory(
        data,
        endian,
        header_len,
        data.len() - header_len,
        None,
        0,
        &mut visited,
        &mut metadata,
    );
    metadata
}

#[allow(clippy::too_many_arguments)]
fn parse_directory(
    data: &[u8],
    endian: Endian,
    block_start: usize,
    block_len: usize,
    parent_tag: Option<u16>,
    depth: usize,
    visited: &mut HashSet<(usize, usize)>,
    metadata: &mut MetadataMap,
) {
    if depth > 16 || block_len < 6 || block_start.checked_add(block_len).is_none() {
        return;
    }
    let Some(block_end) = block_start.checked_add(block_len) else {
        return;
    };
    if block_end > data.len() || !visited.insert((block_start, block_len)) {
        return;
    }
    let Some(directory_rel) = endian.u32(&data[block_end - 4..block_end]) else {
        return;
    };
    let Some(directory_start) = block_start.checked_add(directory_rel as usize) else {
        return;
    };
    let Some(count) = endian.u16(data.get(directory_start..).unwrap_or_default()) else {
        return;
    };
    let Some(entries_end) = directory_start.checked_add(2 + count as usize * 10) else {
        return;
    };
    if entries_end > block_end {
        return;
    }

    for index in 0..count as usize {
        let entry_start = directory_start + 2 + index * 10;
        let entry = &data[entry_start..entry_start + 10];
        let Some(raw_tag) = endian.u16(entry) else {
            continue;
        };
        let Some(size) = endian.u32(&entry[2..6]).map(|value| value as usize) else {
            continue;
        };
        let Some(value_offset) = endian.u32(&entry[6..10]).map(|value| value as usize) else {
            continue;
        };
        if raw_tag & 0x8000 != 0 {
            continue;
        }
        let tag = raw_tag & 0x3fff;
        let tag_type = (raw_tag >> 8) & 0x38;
        let in_directory = raw_tag & 0x4000 != 0;

        if matches!(tag_type, 0x28 | 0x30) && !in_directory {
            let Some(child_start) = block_start.checked_add(value_offset) else {
                continue;
            };
            parse_directory(
                data,
                endian,
                child_start,
                size,
                Some(tag),
                depth + 1,
                visited,
                metadata,
            );
            continue;
        }

        let value = if in_directory {
            &entry[2..10]
        } else {
            let Some(start) = block_start.checked_add(value_offset) else {
                continue;
            };
            let Some(end) = start.checked_add(size) else {
                continue;
            };
            let Some(value) = data.get(start..end) else {
                continue;
            };
            value
        };
        insert_ciff_value(tag, parent_tag, value, endian, metadata);
    }
}

fn insert_ciff_value(
    tag: u16,
    parent_tag: Option<u16>,
    value: &[u8],
    endian: Endian,
    metadata: &mut MetadataMap,
) {
    let integer = |offset| {
        endian
            .u32(value.get(offset..).unwrap_or_default())
            .map(|v| v as i64)
    };
    let signed = |offset| {
        endian
            .i32(value.get(offset..).unwrap_or_default())
            .map(|v| v as i64)
    };
    let float = |offset| {
        endian
            .f32(value.get(offset..).unwrap_or_default())
            .map(|v| v as f64)
    };
    let short = |offset| {
        endian
            .u16(value.get(offset..).unwrap_or_default())
            .map(|v| v as i64)
    };
    let text = || {
        String::from_utf8_lossy(value)
            .trim_end_matches('\0')
            .to_string()
    };
    let insert_integer = |metadata: &mut MetadataMap, name, value| {
        if let Some(value) = value {
            metadata.insert(format!("CIFF:{name}"), TagValue::Integer(value));
        }
    };
    let insert_float = |metadata: &mut MetadataMap, name, value| {
        if let Some(value) = value {
            metadata.insert(format!("CIFF:{name}"), TagValue::Float(value));
        }
    };

    match tag {
        0x1803 => {
            if let Some(format) = integer(0) {
                let format = match format as u32 {
                    0x0001_0000 => "JPEG (lossy)".to_string(),
                    0x0001_0002 => "JPEG (non-quantization)".to_string(),
                    0x0001_0003 => "JPEG (lossy/non-quantization toggled)".to_string(),
                    0x0002_0001 => "CRW".to_string(),
                    other => format!("Unknown (0x{other:08x})"),
                };
                metadata.insert("CIFF:FileFormat".to_string(), TagValue::String(format));
            }
            insert_float(metadata, "TargetCompressionRatio", float(4));
        }
        0x1810 => {
            insert_integer(metadata, "ImageWidth", integer(0));
            insert_integer(metadata, "ImageHeight", integer(4));
            insert_float(metadata, "PixelAspectRatio", float(8));
            insert_integer(metadata, "Rotation", signed(12));
            insert_integer(metadata, "ComponentBitDepth", integer(16));
            insert_integer(metadata, "ColorBitDepth", integer(20));
            insert_integer(metadata, "ColorBW", integer(24));
        }
        0x100a => {
            if let Some(value) = short(0) {
                let value = match value {
                    0 => "Real-world Subject".to_string(),
                    1 => "Written Document".to_string(),
                    other => format!("Unknown ({other})"),
                };
                metadata.insert("CIFF:TargetImageType".to_string(), TagValue::String(value));
            }
        }
        0x1804 => insert_integer(metadata, "RecordID", integer(0)),
        0x1817 => insert_integer(metadata, "FileNumber", integer(0)),
        0x180e => {
            if let Some(timestamp) = integer(0)
                && let Some(datetime) = chrono::DateTime::from_timestamp(timestamp, 0)
            {
                metadata.insert(
                    "CIFF:DateTimeOriginal".to_string(),
                    TagValue::String(datetime.format("%Y:%m:%d %H:%M:%S").to_string()),
                );
            }
            insert_integer(metadata, "TimeZoneCode", signed(4));
            insert_integer(metadata, "TimeZoneInfo", integer(8));
        }
        0x0816 => {
            metadata.insert(
                "CIFF:OriginalFileName".to_string(),
                TagValue::String(text()),
            );
        }
        0x0817 => {
            metadata.insert(
                "CIFF:ThumbnailFileName".to_string(),
                TagValue::String(text()),
            );
        }
        0x1010 => {
            if let Some(value) = short(0) {
                let value = match value {
                    0 => "Single Shot".to_string(),
                    2 => "Continuous Shooting".to_string(),
                    other => format!("Unknown ({other})"),
                };
                metadata.insert(
                    "CIFF:ShutterReleaseMethod".to_string(),
                    TagValue::String(value),
                );
            }
        }
        0x1011 => {
            if let Some(value) = short(0) {
                let value = match value {
                    0 => "Priority on shutter".to_string(),
                    1 => "Priority on focus".to_string(),
                    other => format!("Unknown ({other})"),
                };
                metadata.insert(
                    "CIFF:ShutterReleaseTiming".to_string(),
                    TagValue::String(value),
                );
            }
        }
        0x1813 => {
            insert_float(metadata, "FlashGuideNumber", float(0));
            insert_float(metadata, "FlashThreshold", float(4));
        }
        0x1818 => {
            insert_float(metadata, "ExposureCompensation", float(0));
            if let Some(apex) = float(4) {
                let seconds = if apex.abs() < 100.0 {
                    2f64.powf(-apex)
                } else {
                    0.0
                };
                let shutter = if seconds > 0.0 && seconds < 1.0 {
                    format!("1/{}", (1.0 / seconds).round() as i64)
                } else {
                    format_number(seconds)
                };
                metadata.insert(
                    "CIFF:ShutterSpeedValue".to_string(),
                    TagValue::String(shutter),
                );
            }
            if let Some(apex) = float(8) {
                let aperture = 2f64.powf(apex / 2.0);
                metadata.insert(
                    "CIFF:ApertureValue".to_string(),
                    TagValue::Float((aperture * 10.0).round() / 10.0),
                );
            }
        }
        0x1807 => {
            if let Some(distance) = float(0) {
                metadata.insert(
                    "CIFF:TargetDistanceSetting".to_string(),
                    TagValue::String(format!("{} mm", format_number(distance))),
                );
            }
        }
        0x1814 => insert_float(metadata, "MeasuredEV", float(0).map(|value| value + 5.0)),
        0x0805 if parent_tag == Some(0x2804) => {
            metadata.insert(
                "CIFF:CanonFileDescription".to_string(),
                TagValue::String(text()),
            );
        }
        0x0815 => {
            metadata.insert("CIFF:CanonImageType".to_string(), TagValue::String(text()));
        }
        0x0810 => {
            metadata.insert("CIFF:OwnerName".to_string(), TagValue::String(text()));
        }
        0x080a => {
            let make_len = value
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(value.len());
            metadata.insert(
                "CIFF:Make".to_string(),
                TagValue::String(String::from_utf8_lossy(&value[..make_len]).into_owned()),
            );
            let model_start = make_len.saturating_add(1);
            if let Some(model) = value.get(model_start..) {
                metadata.insert(
                    "CIFF:Model".to_string(),
                    TagValue::String(
                        String::from_utf8_lossy(model)
                            .trim_end_matches('\0')
                            .to_string(),
                    ),
                );
            }
        }
        0x101c => insert_integer(metadata, "BaseISO", short(0)),
        0x080d => {
            metadata.insert(
                "CIFF:ROMOperationMode".to_string(),
                TagValue::String(text()),
            );
        }
        0x080b => {
            metadata.insert(
                "CIFF:CanonFirmwareVersion".to_string(),
                TagValue::String(text()),
            );
        }
        0x1029 => {
            if let Some(value) = short(0) {
                let value = match value {
                    1 => "Fixed".to_string(),
                    2 => "Zoom".to_string(),
                    other => format!("Unknown ({other})"),
                };
                metadata.insert("CIFF:FocalType".to_string(), TagValue::String(value));
            }
            if let Some(value) = short(2) {
                metadata.insert(
                    "CIFF:FocalLength".to_string(),
                    TagValue::String(format!("{value} mm")),
                );
            }
            for (offset, name) in [(4, "FocalPlaneXSize"), (6, "FocalPlaneYSize")] {
                if let Some(value) = short(offset) {
                    metadata.insert(
                        format!("CIFF:{name}"),
                        TagValue::String(format!("{:.2} mm", value as f64 * 0.0254)),
                    );
                }
            }
        }
        _ => {}
    };
}

fn format_number(value: f64) -> String {
    let rendered = format!("{value:.10}");
    rendered
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::parse_ciff_app0;

    #[test]
    fn rejects_non_ciff_app0_data() {
        assert!(parse_ciff_app0(b"JFIF\0\x01\x02").is_empty());
    }
}
