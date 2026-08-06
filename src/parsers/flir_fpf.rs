//! FLIR Public image Format (`.fpf`) -- a standalone thermal-camera image,
//! distinct from the `FPXR` FlashPix segments JPEG carries.
//!
//! Reference: `Image::ExifTool::FLIR::ProcessFPF` (FLIR.pm:1599-1615) and the
//! `%Image::ExifTool::FLIR::FPF` table (FLIR.pm:900-971).
//!
//! `ProcessFPF` reads a fixed 892-byte header, confirms the
//! `FPF Public Image Format\0` magic, then reads the table's fields directly
//! out of that buffer with `ProcessBinaryData` (i.e. `index` is the absolute
//! byte offset, since the table's `FORMAT` is `int8u`). Byte order defaults to
//! little-endian and is toggled only if the low 16 bits of `FPFVersion`
//! (offset 0x20) come back zero under that assumption -- ExifTool's own
//! belt-and-suspenders check, since it says "I think these are always
//! little-endian".
//!
//! This module transcribes the table directly rather than reusing the
//! generated `crate::exiftool_tables::binary_tables::FLIR_FPF` (whose `Field`
//! type is shared with unrelated MakerNote tables and not yet wired to a
//! generic interpreter) -- every offset, format and `PrintConv` below is
//! checked against that generated table and against FLIR.pm.

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// `$raf->Read($buff, 892) == 892 and $buff =~ /^FPF Public Image Format\0/`
const HEADER_LEN: usize = 892;

/// `%Image::ExifTool::FLIR::FPF` group1.
const GROUP: &str = "FLIR";

fn read_u16(buf: &[u8], off: usize, little_endian: bool) -> Option<u16> {
    let b = buf.get(off..off + 2)?;
    Some(if little_endian {
        u16::from_le_bytes([b[0], b[1]])
    } else {
        u16::from_be_bytes([b[0], b[1]])
    })
}

fn read_u32(buf: &[u8], off: usize, little_endian: bool) -> Option<u32> {
    let b = buf.get(off..off + 4)?;
    Some(if little_endian {
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    } else {
        u32::from_be_bytes([b[0], b[1], b[2], b[3]])
    })
}

fn read_f32(buf: &[u8], off: usize, little_endian: bool) -> Option<f32> {
    read_u32(buf, off, little_endian).map(f32::from_bits)
}

fn read_str(buf: &[u8], off: usize, len: usize) -> Option<String> {
    let b = buf.get(off..off + len)?;
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    Some(String::from_utf8_lossy(&b[..end]).trim().to_string())
}

/// `sprintf("%.1f",$val)` / `sprintf("%.2f",$val)` PrintConvs shared by
/// several float fields (`%float1f`, `%float2f` in FLIR.pm).
fn insert_float(metadata: &mut MetadataMap, name: &str, val: f32, decimals: usize) {
    let s = match decimals {
        1 => format!("{val:.1}"),
        _ => format!("{val:.2}"),
    };
    metadata.insert(format!("{GROUP}:{name}"), TagValue::String(s));
}

/// `%floatKelvin`: `ValueConv => '$val - 273.15'`, `PrintConv =>
/// 'sprintf("%.1f C",$val)'` (FLIR.pm:39-43).
fn insert_kelvin(metadata: &mut MetadataMap, name: &str, val: f32) {
    let celsius = val as f64 - 273.15;
    metadata.insert(
        format!("{GROUP}:{name}"),
        TagValue::String(format!("{celsius:.1} C")),
    );
}

fn insert_str(metadata: &mut MetadataMap, name: &str, val: Option<String>) {
    if let Some(v) = val {
        metadata.insert(format!("{GROUP}:{name}"), TagValue::String(v));
    }
}

fn image_type_name(v: u16) -> Option<&'static str> {
    Some(match v {
        0 => "Temperature",
        1 => "Temperature Difference",
        2 => "Object Signal",
        3 => "Object Signal Difference",
        _ => return None,
    })
}

fn pixel_format_name(v: u16) -> Option<&'static str> {
    Some(match v {
        0 => "2-byte short integer",
        1 => "4-byte long integer",
        2 => "4-byte float",
        3 => "8-byte double",
        _ => return None,
    })
}

/// Reads a `.fpf` file: FLIR's standalone thermal-image format.
///
/// # Errors
///
/// Returns `ParseError` when the file is shorter than the 892-byte header or
/// the magic does not match (mirrors `ProcessFPF` returning 0 on either).
pub fn parse_fpf_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    let size = reader.size();
    if size < HEADER_LEN as u64 {
        return Err(ExifToolError::parse_error("FPF header truncated"));
    }
    let buf = reader
        .read(0, HEADER_LEN)
        .map_err(|e| ExifToolError::parse_error(format!("FPF read failed: {e}")))?;
    if !buf.starts_with(b"FPF Public Image Format\0") {
        return Err(ExifToolError::parse_error("Not an FPF file"));
    }

    // SetByteOrder('II'); ToggleByteOrder() unless Get32u(\$buff, 0x20) & 0xffff;
    let le_version = read_u32(buf, 0x20, true).unwrap_or(0);
    let little_endian = (le_version & 0xffff) != 0;

    let mut metadata = MetadataMap::new();

    if let Some(v) = read_u32(buf, 0x20, little_endian) {
        metadata.insert(format!("{GROUP}:FPFVersion"), TagValue::Integer(v as i64));
    }
    if let Some(v) = read_u32(buf, 0x24, little_endian) {
        metadata.insert(
            format!("{GROUP}:ImageDataOffset"),
            TagValue::Integer(v as i64),
        );
    }
    if let Some(v) = read_u16(buf, 0x28, little_endian) {
        if let Some(label) = image_type_name(v) {
            metadata.insert(
                format!("{GROUP}:ImageType"),
                TagValue::String(label.to_string()),
            );
        }
    }
    if let Some(v) = read_u16(buf, 0x2a, little_endian) {
        if let Some(label) = pixel_format_name(v) {
            metadata.insert(
                format!("{GROUP}:ImagePixelFormat"),
                TagValue::String(label.to_string()),
            );
        }
    }
    if let Some(v) = read_u16(buf, 0x2c, little_endian) {
        metadata.insert(format!("{GROUP}:ImageWidth"), TagValue::Integer(v as i64));
    }
    if let Some(v) = read_u16(buf, 0x2e, little_endian) {
        metadata.insert(format!("{GROUP}:ImageHeight"), TagValue::Integer(v as i64));
    }
    if let Some(v) = read_u32(buf, 0x30, little_endian) {
        metadata.insert(
            format!("{GROUP}:ExternalTriggerCount"),
            TagValue::Integer(v as i64),
        );
    }
    if let Some(v) = read_u32(buf, 0x34, little_endian) {
        metadata.insert(
            format!("{GROUP}:SequenceFrameNumber"),
            TagValue::Integer(v as i64),
        );
    }

    insert_str(&mut metadata, "CameraModel", read_str(buf, 0x78, 32));
    insert_str(&mut metadata, "CameraPartNumber", read_str(buf, 0x98, 32));
    insert_str(&mut metadata, "CameraSerialNumber", read_str(buf, 0xb8, 32));

    if let Some(v) = read_f32(buf, 0xd8, little_endian) {
        insert_kelvin(&mut metadata, "CameraTemperatureRangeMin", v);
    }
    if let Some(v) = read_f32(buf, 0xdc, little_endian) {
        insert_kelvin(&mut metadata, "CameraTemperatureRangeMax", v);
    }

    insert_str(&mut metadata, "LensModel", read_str(buf, 0xe0, 32));
    insert_str(&mut metadata, "LensPartNumber", read_str(buf, 0x100, 32));
    insert_str(&mut metadata, "LensSerialNumber", read_str(buf, 0x120, 32));
    insert_str(&mut metadata, "FilterModel", read_str(buf, 0x140, 32));
    insert_str(&mut metadata, "FilterPartNumber", read_str(buf, 0x150, 32));
    insert_str(
        &mut metadata,
        "FilterSerialNumber",
        read_str(buf, 0x180, 32),
    );

    if let Some(v) = read_f32(buf, 0x1e0, little_endian) {
        insert_float(&mut metadata, "Emissivity", v, 2);
    }
    if let Some(v) = read_f32(buf, 0x1e4, little_endian) {
        metadata.insert(
            format!("{GROUP}:ObjectDistance"),
            TagValue::String(format!("{:.2} m", v)),
        );
    }
    if let Some(v) = read_f32(buf, 0x1e8, little_endian) {
        insert_kelvin(&mut metadata, "ReflectedApparentTemperature", v);
    }
    if let Some(v) = read_f32(buf, 0x1ec, little_endian) {
        insert_kelvin(&mut metadata, "AtmosphericTemperature", v);
    }
    if let Some(v) = read_f32(buf, 0x1f0, little_endian) {
        // `PrintConv => 'sprintf("%.1f %%",$val*100)'` (FLIR.pm:945).
        metadata.insert(
            format!("{GROUP}:RelativeHumidity"),
            TagValue::String(format!("{:.1} %", v as f64 * 100.0)),
        );
    }
    if let Some(v) = read_f32(buf, 0x1f4, little_endian) {
        insert_float(&mut metadata, "ComputedAtmosphericTrans", v, 2);
    }
    if let Some(v) = read_f32(buf, 0x1f8, little_endian) {
        insert_float(&mut metadata, "EstimatedAtmosphericTrans", v, 2);
    }
    if let Some(v) = read_f32(buf, 0x1fc, little_endian) {
        insert_kelvin(&mut metadata, "ReferenceTemperature", v);
    }
    if let Some(v) = read_f32(buf, 0x200, little_endian) {
        insert_kelvin(&mut metadata, "IRWindowTemperature", v);
    }
    if let Some(v) = read_f32(buf, 0x204, little_endian) {
        insert_float(&mut metadata, "IRWindowTransmission", v, 2);
    }

    // FLIR.pm FPF: `int32u[7]`, ValueConv =>
    // `sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2d.%.3d", split(" ",$val))`.
    // This format has no timezone or locale conversion.
    if let (
        Some(year),
        Some(month),
        Some(day),
        Some(hour),
        Some(minute),
        Some(second),
        Some(millis),
    ) = (
        read_u32(buf, 0x248, little_endian),
        read_u32(buf, 0x24c, little_endian),
        read_u32(buf, 0x250, little_endian),
        read_u32(buf, 0x254, little_endian),
        read_u32(buf, 0x258, little_endian),
        read_u32(buf, 0x25c, little_endian),
        read_u32(buf, 0x260, little_endian),
    ) {
        metadata.insert(
            format!("{GROUP}:DateTimeOriginal"),
            TagValue::String(format!(
                "{year:04}:{month:02}:{day:02} {hour:02}:{minute:02}:{second:02}.{millis:03}"
            )),
        );
    }

    if let Some(v) = read_f32(buf, 0x2a4, little_endian) {
        insert_float(&mut metadata, "CameraScaleMin", v, 1);
    }
    if let Some(v) = read_f32(buf, 0x2a8, little_endian) {
        insert_float(&mut metadata, "CameraScaleMax", v, 1);
    }
    if let Some(v) = read_f32(buf, 0x2ac, little_endian) {
        insert_float(&mut metadata, "CalculatedScaleMin", v, 1);
    }
    if let Some(v) = read_f32(buf, 0x2b0, little_endian) {
        insert_float(&mut metadata, "CalculatedScaleMax", v, 1);
    }
    if let Some(v) = read_f32(buf, 0x2b4, little_endian) {
        insert_float(&mut metadata, "ActualScaleMin", v, 1);
    }
    if let Some(v) = read_f32(buf, 0x2b8, little_endian) {
        insert_float(&mut metadata, "ActualScaleMax", v, 1);
    }

    if metadata.is_empty() {
        return Err(ExifToolError::parse_error("No FPF tags decoded"));
    }

    if let Some(id) = crate::filetype::identify_by_extension("fpf") {
        metadata.insert("File:FileType", TagValue::new_string(id.file_type.as_ref()));
        metadata.insert(
            "File:FileTypeExtension",
            TagValue::new_string(id.extension.as_ref()),
        );
        if let Some(mime) = id.mime_type {
            metadata.insert("File:MIMEType", TagValue::new_string(mime));
        }
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SliceReader(Vec<u8>);

    impl FileReader for SliceReader {
        fn read(&self, offset: u64, length: usize) -> std::io::Result<&[u8]> {
            let start = offset as usize;
            let end = start
                .checked_add(length)
                .filter(|e| *e <= self.0.len())
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "past end of file")
                })?;
            Ok(&self.0[start..end])
        }

        fn size(&self) -> u64 {
            self.0.len() as u64
        }
    }

    /// Cross-checked against the pinned ExifTool 13.59 oracle on the same
    /// synthetic buffer (matching `FLIR:*` output).
    fn sample_buffer() -> Vec<u8> {
        let mut buf = vec![0u8; HEADER_LEN + 100];
        buf[0..24].copy_from_slice(b"FPF Public Image Format\0");
        buf[0x20..0x24].copy_from_slice(&2u32.to_le_bytes());
        buf[0x24..0x28].copy_from_slice(&900u32.to_le_bytes());
        buf[0x28..0x2a].copy_from_slice(&0u16.to_le_bytes());
        buf[0x2a..0x2c].copy_from_slice(&2u16.to_le_bytes());
        buf[0x2c..0x2e].copy_from_slice(&320u16.to_le_bytes());
        buf[0x2e..0x30].copy_from_slice(&240u16.to_le_bytes());
        buf[0x30..0x34].copy_from_slice(&5u32.to_le_bytes());
        buf[0x34..0x38].copy_from_slice(&7u32.to_le_bytes());
        buf[0x78..0x78 + 9].copy_from_slice(b"FLIR T650");
        buf[0xd8..0xdc].copy_from_slice(&(-40.0f32).to_le_bytes());
        buf[0xdc..0xe0].copy_from_slice(&150.0f32.to_le_bytes());
        buf[0x1e0..0x1e4].copy_from_slice(&0.95f32.to_le_bytes());
        buf[0x1e4..0x1e8].copy_from_slice(&3.0f32.to_le_bytes());
        buf[0x1f0..0x1f4].copy_from_slice(&0.5f32.to_le_bytes());
        buf
    }

    #[test]
    fn decodes_header_fields_matching_exiftool() {
        let reader = SliceReader(sample_buffer());
        let metadata = parse_fpf_metadata(&reader).expect("valid FPF file");

        assert_eq!(metadata.get_integer("FLIR:FPFVersion"), Some(2));
        assert_eq!(metadata.get_string("FLIR:ImageType"), Some("Temperature"));
        assert_eq!(
            metadata.get_string("FLIR:ImagePixelFormat"),
            Some("4-byte float")
        );
        assert_eq!(metadata.get_integer("FLIR:ImageWidth"), Some(320));
        assert_eq!(metadata.get_string("FLIR:CameraModel"), Some("FLIR T650"));
        // -40 K -> -313.1 C, matching ExifTool's `%floatKelvin` conversion.
        assert_eq!(
            metadata.get_string("FLIR:CameraTemperatureRangeMin"),
            Some("-313.1 C")
        );
        assert_eq!(metadata.get_string("FLIR:ObjectDistance"), Some("3.00 m"));
        // 0.5 * 100 -> "50.0 %".
        assert_eq!(metadata.get_string("FLIR:RelativeHumidity"), Some("50.0 %"));
        assert_eq!(metadata.get_string("File:FileType"), Some("FPF"));
    }

    #[test]
    fn preserves_empty_fixed_strings_and_decodes_fpf_datetime_original() {
        let mut buf = sample_buffer();
        // FLIR.pm FPF: DateTimeOriginal is int32u[7] at 0x248, formatted as
        // YYYY:MM:DD HH:MM:SS.mmm. The zero-filled lens/filter fields are
        // emitted by ExifTool as empty strings rather than omitted.
        for (offset, value) in [
            (0x248, 2013u32),
            (0x24c, 2),
            (0x250, 22),
            (0x254, 11),
            (0x258, 19),
            (0x25c, 20),
            (0x260, 891),
        ] {
            buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }

        let metadata = parse_fpf_metadata(&SliceReader(buf)).expect("valid FPF file");

        assert_eq!(
            metadata.get_string("FLIR:DateTimeOriginal"),
            Some("2013:02:22 11:19:20.891")
        );
        for name in [
            "LensPartNumber",
            "LensSerialNumber",
            "FilterModel",
            "FilterPartNumber",
            "FilterSerialNumber",
        ] {
            assert_eq!(metadata.get_string(&format!("FLIR:{name}")), Some(""));
        }
    }

    #[test]
    fn rejects_wrong_magic() {
        let mut buf = vec![0u8; HEADER_LEN];
        buf[0..3].copy_from_slice(b"XXX");
        let reader = SliceReader(buf);
        assert!(parse_fpf_metadata(&reader).is_err());
    }

    #[test]
    fn rejects_truncated_file() {
        let reader = SliceReader(vec![0u8; 10]);
        assert!(parse_fpf_metadata(&reader).is_err());
    }
}
