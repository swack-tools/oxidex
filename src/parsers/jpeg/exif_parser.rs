//! EXIF segment parser for JPEG (TIFF IFD)
//!
//! This module handles parsing of EXIF data in JPEG files.

#![allow(dead_code)]

use crate::core::{MetadataMap, TagValue};
use crate::error::ExifToolError;

#[derive(Clone, Copy)]
enum TiffByteOrder {
    Little,
    Big,
}

fn checked_slice(
    data: &[u8],
    offset: usize,
    length: usize,
) -> Result<&[u8], ExifToolError> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| ExifToolError::parse_error("EXIF offset overflow"))?;
    data.get(offset..end)
        .ok_or_else(|| ExifToolError::parse_error_at("Truncated EXIF data", offset))
}

fn read_u16(
    data: &[u8],
    offset: usize,
    byte_order: TiffByteOrder,
) -> Result<u16, ExifToolError> {
    let bytes = checked_slice(data, offset, 2)?;
    Ok(match byte_order {
        TiffByteOrder::Little => u16::from_le_bytes([bytes[0], bytes[1]]),
        TiffByteOrder::Big => u16::from_be_bytes([bytes[0], bytes[1]]),
    })
}

fn read_u32(
    data: &[u8],
    offset: usize,
    byte_order: TiffByteOrder,
) -> Result<u32, ExifToolError> {
    let bytes = checked_slice(data, offset, 4)?;
    Ok(match byte_order {
        TiffByteOrder::Little => {
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        }
        TiffByteOrder::Big => {
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        }
    })
}

fn tiff_offset(value: u32) -> Result<usize, ExifToolError> {
    usize::try_from(value)
        .map_err(|_| ExifToolError::parse_error("TIFF offset does not fit in usize"))
}

fn ifd_entry_offset(ifd_offset: usize, index: usize) -> Result<usize, ExifToolError> {
    index
        .checked_mul(12)
        .and_then(|size| size.checked_add(2))
        .and_then(|size| ifd_offset.checked_add(size))
        .ok_or_else(|| ExifToolError::parse_error("IFD entry offset overflow"))
}

fn find_ifd_entry(
    tiff: &[u8],
    ifd_offset: usize,
    wanted_tag: u16,
    byte_order: TiffByteOrder,
) -> Result<Option<(usize, u16, u32)>, ExifToolError> {
    let entry_count = read_u16(tiff, ifd_offset, byte_order)? as usize;

    for index in 0..entry_count {
        let entry_offset = ifd_entry_offset(ifd_offset, index)?;
        checked_slice(tiff, entry_offset, 12)?;

        if read_u16(tiff, entry_offset, byte_order)? == wanted_tag {
            let field_type = read_u16(tiff, entry_offset + 2, byte_order)?;
            let count = read_u32(tiff, entry_offset + 4, byte_order)?;
            return Ok(Some((entry_offset, field_type, count)));
        }
    }

    Ok(None)
}

fn entry_data<'a>(
    tiff: &'a [u8],
    entry_offset: usize,
    field_type: u16,
    count: u32,
    byte_order: TiffByteOrder,
) -> Result<&'a [u8], ExifToolError> {
    // TIFF 6.0 field type sizes: BYTE=1, ASCII=1, SHORT=2, LONG=4,
    // RATIONAL=8, SBYTE=1, UNDEFINED=1, SSHORT=2, SLONG=4, SRATIONAL=8,
    // FLOAT=4, DOUBLE=8.
    let type_size = match field_type {
        1 | 2 | 6 | 7 => 1usize,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => {
            return Err(ExifToolError::parse_error(format!(
                "Unsupported TIFF field type {}",
                field_type
            )));
        }
    };
    let count = usize::try_from(count)
        .map_err(|_| ExifToolError::parse_error("TIFF value count does not fit in usize"))?;
    let size = type_size
        .checked_mul(count)
        .ok_or_else(|| ExifToolError::parse_error("TIFF value size overflow"))?;

    if size <= 4 {
        checked_slice(tiff, entry_offset + 8, size)
    } else {
        let offset = tiff_offset(read_u32(tiff, entry_offset + 8, byte_order)?)?;
        checked_slice(tiff, offset, size)
    }
}

fn read_ascii_entry(
    tiff: &[u8],
    entry_offset: usize,
    field_type: u16,
    count: u32,
    byte_order: TiffByteOrder,
) -> Result<String, ExifToolError> {
    if field_type != 2 {
        return Err(ExifToolError::parse_error(format!(
            "GPS ASCII tag has TIFF field type {}",
            field_type
        )));
    }

    let bytes = entry_data(tiff, entry_offset, field_type, count, byte_order)?;
    let end = bytes.iter().position(|&byte| byte == 0).unwrap_or(bytes.len());
    let text = std::str::from_utf8(&bytes[..end])
        .map_err(|_| ExifToolError::parse_error("GPS ASCII tag is not valid UTF-8"))?;
    Ok(text.to_string())
}

fn read_rationals(
    tiff: &[u8],
    entry_offset: usize,
    field_type: u16,
    count: u32,
    byte_order: TiffByteOrder,
) -> Result<Vec<f64>, ExifToolError> {
    if field_type != 5 {
        return Err(ExifToolError::parse_error(format!(
            "GPS rational tag has TIFF field type {}",
            field_type
        )));
    }

    let bytes = entry_data(tiff, entry_offset, field_type, count, byte_order)?;
    let count = usize::try_from(count)
        .map_err(|_| ExifToolError::parse_error("GPS rational count does not fit in usize"))?;
    let mut values = Vec::with_capacity(count);

    for index in 0..count {
        let offset = index
            .checked_mul(8)
            .ok_or_else(|| ExifToolError::parse_error("GPS rational offset overflow"))?;
        let numerator = read_u32(bytes, offset, byte_order)?;
        let denominator = read_u32(bytes, offset + 4, byte_order)?;
        if denominator == 0 {
            return Err(ExifToolError::parse_error(
                "GPS rational has a zero denominator",
            ));
        }
        values.push(numerator as f64 / denominator as f64);
    }

    Ok(values)
}

fn trim_decimal(value: f64, precision: usize) -> String {
    let mut text = format!("{value:.precision$}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

/// Parse GPS tags from an `Exif\0\0` JPEG APP1 payload.
///
/// All IFD offsets are relative to the TIFF header immediately following the
/// six-byte APP1 EXIF signature.
pub fn parse_app1_exif_segment(
    data: &[u8],
    metadata: &mut MetadataMap,
) -> Result<(), ExifToolError> {
    const EXIF_SIGNATURE: &[u8] = b"Exif\0\0";
    const GPS_INFO_IFD_POINTER: u16 = 0x8825;

    if !data.starts_with(EXIF_SIGNATURE) {
        return Err(ExifToolError::parse_error(
            "APP1 segment does not contain an EXIF payload",
        ));
    }

    let tiff = &data[EXIF_SIGNATURE.len()..];
    let byte_order = match checked_slice(tiff, 0, 2)? {
        b"II" => TiffByteOrder::Little,
        b"MM" => TiffByteOrder::Big,
        _ => return Err(ExifToolError::parse_error("Invalid TIFF byte order")),
    };

    if read_u16(tiff, 2, byte_order)? != 42 {
        return Err(ExifToolError::parse_error("Invalid TIFF magic number"));
    }

    let ifd0_offset = tiff_offset(read_u32(tiff, 4, byte_order)?)?;
    let Some((gps_pointer_entry, field_type, count)) =
        find_ifd_entry(tiff, ifd0_offset, GPS_INFO_IFD_POINTER, byte_order)?
    else {
        return Ok(());
    };

    if field_type != 4 || count != 1 {
        return Err(ExifToolError::parse_error(
            "GPSInfo IFD pointer is not a single TIFF LONG",
        ));
    }

    let gps_ifd_offset =
        tiff_offset(read_u32(tiff, gps_pointer_entry + 8, byte_order)?)?;

    let mut latitude_ref = None;
    let mut longitude_ref = None;
    let mut longitude = None;
    let mut speed = None;
    let mut map_datum = None;

    for (tag, destination) in [
        (0x0001u16, &mut latitude_ref),
        (0x0003u16, &mut longitude_ref),
        (0x0012u16, &mut map_datum),
    ] {
        if let Some((entry, field_type, count)) =
            find_ifd_entry(tiff, gps_ifd_offset, tag, byte_order)?
        {
            *destination = Some(read_ascii_entry(
                tiff, entry, field_type, count, byte_order,
            )?);
        }
    }

    if let Some((entry, field_type, count)) =
        find_ifd_entry(tiff, gps_ifd_offset, 0x0004, byte_order)?
    {
        let values = read_rationals(tiff, entry, field_type, count, byte_order)?;
        if values.len() == 3 {
            longitude = Some(values);
        }
    }

    if let Some((entry, field_type, count)) =
        find_ifd_entry(tiff, gps_ifd_offset, 0x000C, byte_order)?
    {
        let values = read_rationals(tiff, entry, field_type, count, byte_order)?;
        if let Some(value) = values.first() {
            speed = Some(*value);
        }
    }

    if let Some(reference) = latitude_ref {
        let display = match reference.as_str() {
            "N" => "North",
            "S" => "South",
            _ => reference.as_str(),
        };
        metadata.insert(
            "APP1:GPSLatitudeRef".to_string(),
            TagValue::String(display.to_string()),
        );
    }

    if let Some(reference) = longitude_ref.as_deref() {
        let display = match reference {
            "E" => "East",
            "W" => "West",
            _ => reference,
        };
        metadata.insert(
            "APP1:GPSLongitudeRef".to_string(),
            TagValue::String(display.to_string()),
        );
    }

    if let Some(values) = longitude {
        let direction = longitude_ref.as_deref().unwrap_or("");
        let display = format!(
            "{} deg {}' {}\" {}",
            trim_decimal(values[0], 6),
            trim_decimal(values[1], 6),
            trim_decimal(values[2], 2),
            direction
        );
        metadata.insert(
            "APP1:GPSLongitude".to_string(),
            TagValue::String(display.trim_end().to_string()),
        );
    }

    if let Some(datum) = map_datum {
        metadata.insert(
            "APP1:GPSMapDatum".to_string(),
            TagValue::String(datum),
        );
    }

    if let Some(speed) = speed {
        metadata.insert(
            "APP1:GPSSpeed".to_string(),
            TagValue::String(format!("{speed:.1}")),
        );
    }

    Ok(())
}
