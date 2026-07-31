//! EXIF segment parser for JPEG (TIFF IFD)
//!
//! This module handles parsing of EXIF data in JPEG files.

#![allow(dead_code)]

use crate::core::{MetadataMap, TagValue};
use crate::error::ExifToolError;
use crate::tag_db::lookup_tag_name;

const EXIF_SIGNATURE: &[u8; 6] = b"Exif\0\0";

// TIFF 6.0, section 8: field type IDs.
const TIFF_TYPE_BYTE: u16 = 1;
const TIFF_TYPE_ASCII: u16 = 2;
const TIFF_TYPE_LONG: u16 = 4;
const TIFF_TYPE_RATIONAL: u16 = 5;

// TIFF/EP and EXIF GPS tag IDs.
const GPS_INFO_IFD_POINTER: u16 = 0x8825;
const GPS_LATITUDE_REF: u16 = 0x0001;
const GPS_LATITUDE: u16 = 0x0002;
const GPS_LONGITUDE_REF: u16 = 0x0003;
const GPS_LONGITUDE: u16 = 0x0004;
const GPS_ALTITUDE_REF: u16 = 0x0005;
const GPS_ALTITUDE: u16 = 0x0006;
const GPS_IMG_DIRECTION: u16 = 0x0011;

#[derive(Clone, Copy)]
enum ByteOrder {
    Little,
    Big,
}

impl ByteOrder {
    fn read_u16(self, data: &[u8], offset: usize) -> Option<u16> {
        let bytes: [u8; 2] = data.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
        Some(match self {
            Self::Little => u16::from_le_bytes(bytes),
            Self::Big => u16::from_be_bytes(bytes),
        })
    }

    fn read_u32(self, data: &[u8], offset: usize) -> Option<u32> {
        let bytes: [u8; 4] = data.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
        Some(match self {
            Self::Little => u32::from_le_bytes(bytes),
            Self::Big => u32::from_be_bytes(bytes),
        })
    }
}

struct IfdEntry {
    tag: u16,
    field_type: u16,
    count: u32,
    value_field_offset: usize,
}

/// Parse an EXIF APP1 payload.
///
/// `data` starts at the APP1 payload and therefore includes the `Exif\0\0`
/// signature. All TIFF offsets are interpreted relative to the TIFF header,
/// as required by TIFF 6.0 and the EXIF specification.
pub fn parse_exif_segment(
    data: &[u8],
    metadata: &mut MetadataMap,
) -> Result<(), ExifToolError> {
    let tiff = data
        .strip_prefix(EXIF_SIGNATURE)
        .ok_or_else(|| ExifToolError::parse_error("Invalid EXIF APP1 signature"))?;

    if tiff.len() < 8 {
        return Err(ExifToolError::parse_error(
            "EXIF APP1 TIFF header is truncated",
        ));
    }

    let byte_order = match tiff.get(0..2) {
        Some(b"II") => ByteOrder::Little,
        Some(b"MM") => ByteOrder::Big,
        _ => {
            return Err(ExifToolError::parse_error(
                "Invalid byte order in EXIF APP1 TIFF header",
            ));
        }
    };

    if byte_order.read_u16(tiff, 2) != Some(42) {
        return Err(ExifToolError::parse_error(
            "Invalid EXIF APP1 TIFF magic number",
        ));
    }

    let ifd0_offset = byte_order
        .read_u32(tiff, 4)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| ExifToolError::parse_error("Missing EXIF IFD0 offset"))?;

    let gps_ifd_offset = find_long_entry(
        tiff,
        ifd0_offset,
        byte_order,
        GPS_INFO_IFD_POINTER,
    );

    if let Some(gps_ifd_offset) = gps_ifd_offset {
        parse_gps_ifd(tiff, gps_ifd_offset, byte_order, metadata)?;
    }

    Ok(())
}

fn for_each_ifd_entry(
    data: &[u8],
    ifd_offset: usize,
    byte_order: ByteOrder,
    mut visit: impl FnMut(IfdEntry),
) -> Result<(), ExifToolError> {
    let entry_count = byte_order
        .read_u16(data, ifd_offset)
        .ok_or_else(|| ExifToolError::parse_error("Truncated EXIF IFD entry count"))?
        as usize;
    let entries_offset = ifd_offset
        .checked_add(2)
        .ok_or_else(|| ExifToolError::parse_error("EXIF IFD offset overflow"))?;
    let entries_size = entry_count
        .checked_mul(12)
        .ok_or_else(|| ExifToolError::parse_error("EXIF IFD entry count overflow"))?;
    let entries_end = entries_offset
        .checked_add(entries_size)
        .ok_or_else(|| ExifToolError::parse_error("EXIF IFD size overflow"))?;

    if entries_end > data.len() {
        return Err(ExifToolError::parse_error("Truncated EXIF IFD"));
    }

    for index in 0..entry_count {
        let entry_offset = entries_offset + index * 12;
        let Some(tag) = byte_order.read_u16(data, entry_offset) else {
            continue;
        };
        let Some(field_type) = byte_order.read_u16(data, entry_offset + 2) else {
            continue;
        };
        let Some(count) = byte_order.read_u32(data, entry_offset + 4) else {
            continue;
        };

        visit(IfdEntry {
            tag,
            field_type,
            count,
            value_field_offset: entry_offset + 8,
        });
    }

    Ok(())
}

fn find_long_entry(
    data: &[u8],
    ifd_offset: usize,
    byte_order: ByteOrder,
    wanted_tag: u16,
) -> Option<usize> {
    let mut result = None;
    if for_each_ifd_entry(data, ifd_offset, byte_order, |entry| {
        if entry.tag == wanted_tag
            && entry.field_type == TIFF_TYPE_LONG
            && entry.count == 1
        {
            result = byte_order
                .read_u32(data, entry.value_field_offset)
                .and_then(|value| usize::try_from(value).ok());
        }
    })
    .is_err()
    {
        return None;
    }
    result
}

fn parse_gps_ifd(
    data: &[u8],
    ifd_offset: usize,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) -> Result<(), ExifToolError> {
    let mut latitude_ref = None;
    let mut latitude = None;
    let mut longitude_ref = None;
    let mut longitude = None;
    let mut altitude_ref = None;
    let mut altitude = None;
    let mut image_direction = None;

    for_each_ifd_entry(data, ifd_offset, byte_order, |entry| match entry.tag {
        GPS_LATITUDE_REF if entry.field_type == TIFF_TYPE_ASCII => {
            latitude_ref = read_inline_ascii(data, &entry);
        }
        GPS_LATITUDE if entry.field_type == TIFF_TYPE_RATIONAL && entry.count == 3 => {
            latitude = read_rationals(data, &entry, byte_order, 3);
        }
        GPS_LONGITUDE_REF if entry.field_type == TIFF_TYPE_ASCII => {
            longitude_ref = read_inline_ascii(data, &entry);
        }
        GPS_LONGITUDE if entry.field_type == TIFF_TYPE_RATIONAL && entry.count == 3 => {
            longitude = read_rationals(data, &entry, byte_order, 3);
        }
        GPS_ALTITUDE_REF if entry.field_type == TIFF_TYPE_BYTE && entry.count == 1 => {
            altitude_ref = data.get(entry.value_field_offset).copied();
        }
        GPS_ALTITUDE if entry.field_type == TIFF_TYPE_RATIONAL && entry.count == 1 => {
            altitude = read_rationals(data, &entry, byte_order, 1)
                .and_then(|values| values.first().copied());
        }
        GPS_IMG_DIRECTION if entry.field_type == TIFF_TYPE_RATIONAL && entry.count == 1 => {
            image_direction = read_rationals(data, &entry, byte_order, 1)
                .and_then(|values| values.first().copied());
        }
        _ => {}
    })?;

    if let Some(reference) = latitude_ref.as_deref().and_then(latitude_ref_name) {
        metadata.insert(
            lookup_tag_name(GPS_LATITUDE_REF, "GPS"),
            TagValue::String(reference.to_string()),
        );
    }
    if let Some(values) = latitude.as_deref() {
        metadata.insert(
            lookup_tag_name(GPS_LATITUDE, "GPS"),
            TagValue::String(format_coordinate(values, latitude_ref.as_deref())),
        );
    }
    if let Some(reference) = longitude_ref.as_deref().and_then(longitude_ref_name) {
        metadata.insert(
            lookup_tag_name(GPS_LONGITUDE_REF, "GPS"),
            TagValue::String(reference.to_string()),
        );
    }
    if let Some(values) = longitude.as_deref() {
        metadata.insert(
            lookup_tag_name(GPS_LONGITUDE, "GPS"),
            TagValue::String(format_coordinate(values, longitude_ref.as_deref())),
        );
    }
    if let Some(value) = altitude {
        let signed_value = if altitude_ref == Some(1) {
            -value
        } else {
            value
        };
        metadata.insert(
            lookup_tag_name(GPS_ALTITUDE, "GPS"),
            TagValue::String(format!("{signed_value:.2} m")),
        );
    }
    if let Some(value) = image_direction {
        metadata.insert(
            lookup_tag_name(GPS_IMG_DIRECTION, "GPS"),
            TagValue::String(format!("{value:.1}")),
        );
    }

    Ok(())
}

fn read_inline_ascii(data: &[u8], entry: &IfdEntry) -> Option<String> {
    let count = usize::try_from(entry.count).ok()?;
    if count == 0 || count > 4 {
        return None;
    }
    let bytes = data.get(entry.value_field_offset..entry.value_field_offset.checked_add(count)?)?;
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    std::str::from_utf8(bytes).ok().map(str::to_string)
}

fn read_rationals(
    data: &[u8],
    entry: &IfdEntry,
    byte_order: ByteOrder,
    expected_count: usize,
) -> Option<Vec<f64>> {
    if usize::try_from(entry.count).ok()? != expected_count {
        return None;
    }
    let value_offset = byte_order
        .read_u32(data, entry.value_field_offset)
        .and_then(|value| usize::try_from(value).ok())?;
    let byte_count = expected_count.checked_mul(8)?;
    data.get(value_offset..value_offset.checked_add(byte_count)?)?;

    let mut values = Vec::with_capacity(expected_count);
    for index in 0..expected_count {
        let offset = value_offset.checked_add(index.checked_mul(8)?)?;
        let numerator = byte_order.read_u32(data, offset)?;
        let denominator = byte_order.read_u32(data, offset + 4)?;
        if denominator == 0 {
            return None;
        }
        values.push(numerator as f64 / denominator as f64);
    }
    Some(values)
}

fn latitude_ref_name(value: &str) -> Option<&'static str> {
    match value {
        "N" => Some("North"),
        "S" => Some("South"),
        _ => None,
    }
}

fn longitude_ref_name(value: &str) -> Option<&'static str> {
    match value {
        "E" => Some("East"),
        "W" => Some("West"),
        _ => None,
    }
}

fn format_coordinate(values: &[f64], reference: Option<&str>) -> String {
    let degrees = values.first().copied().unwrap_or(0.0);
    let minutes = values.get(1).copied().unwrap_or(0.0);
    let seconds = values.get(2).copied().unwrap_or(0.0);
    match reference {
        Some(reference) => {
            format!("{degrees:.0} deg {minutes:.0}' {seconds:.2}\" {reference}")
        }
        None => format!("{degrees:.0} deg {minutes:.0}' {seconds:.2}\""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_little_endian_gps_ifd() {
        let mut data = vec![0_u8; 230];
        data[0..6].copy_from_slice(EXIF_SIGNATURE);
        let tiff = &mut data[6..];
        tiff[0..2].copy_from_slice(b"II");
        tiff[2..4].copy_from_slice(&42_u16.to_le_bytes());
        tiff[4..8].copy_from_slice(&8_u32.to_le_bytes());

        tiff[8..10].copy_from_slice(&1_u16.to_le_bytes());
        tiff[10..12].copy_from_slice(&GPS_INFO_IFD_POINTER.to_le_bytes());
        tiff[12..14].copy_from_slice(&TIFF_TYPE_LONG.to_le_bytes());
        tiff[14..18].copy_from_slice(&1_u32.to_le_bytes());
        tiff[18..22].copy_from_slice(&32_u32.to_le_bytes());

        let gps = 32;
        tiff[gps..gps + 2].copy_from_slice(&7_u16.to_le_bytes());
        let entries = [
            (GPS_LATITUDE_REF, TIFF_TYPE_ASCII, 2, u32::from_le_bytes(*b"N\0\0\0")),
            (GPS_LATITUDE, TIFF_TYPE_RATIONAL, 3, 120),
            (GPS_LONGITUDE_REF, TIFF_TYPE_ASCII, 2, u32::from_le_bytes(*b"W\0\0\0")),
            (GPS_LONGITUDE, TIFF_TYPE_RATIONAL, 3, 144),
            (GPS_ALTITUDE_REF, TIFF_TYPE_BYTE, 1, 0),
            (GPS_ALTITUDE, TIFF_TYPE_RATIONAL, 1, 168),
            (GPS_IMG_DIRECTION, TIFF_TYPE_RATIONAL, 1, 176),
        ];
        for (index, (tag, field_type, count, value)) in entries.iter().enumerate() {
            let offset = gps + 2 + index * 12;
            tiff[offset..offset + 2].copy_from_slice(&tag.to_le_bytes());
            tiff[offset + 2..offset + 4].copy_from_slice(&field_type.to_le_bytes());
            tiff[offset + 4..offset + 8].copy_from_slice(&count.to_le_bytes());
            tiff[offset + 8..offset + 12].copy_from_slice(&value.to_le_bytes());
        }

        let rationals = [
            (120, 45, 1), (128, 0, 1), (136, 1858, 100),
            (144, 93, 1), (152, 27, 1), (160, 4119, 100),
            (168, 35283, 100), (176, 0, 1),
        ];
        for (offset, numerator, denominator) in rationals {
            tiff[offset..offset + 4].copy_from_slice(&numerator.to_le_bytes());
            tiff[offset + 4..offset + 8].copy_from_slice(&denominator.to_le_bytes());
        }

        let mut metadata = MetadataMap::new();
        assert!(parse_exif_segment(&data, &mut metadata).is_ok());
        assert_eq!(
            metadata.get(&lookup_tag_name(GPS_ALTITUDE, "GPS")),
            Some(&TagValue::String("352.83 m".to_string()))
        );
        assert_eq!(
            metadata.get(&lookup_tag_name(GPS_IMG_DIRECTION, "GPS")),
            Some(&TagValue::String("0.0".to_string()))
        );
        assert_eq!(
            metadata.get(&lookup_tag_name(GPS_LATITUDE, "GPS")),
            Some(&TagValue::String("45 deg 0' 18.58\" N".to_string()))
        );
        assert_eq!(
            metadata.get(&lookup_tag_name(GPS_LONGITUDE, "GPS")),
            Some(&TagValue::String("93 deg 27' 41.19\" W".to_string()))
        );
    }
}
