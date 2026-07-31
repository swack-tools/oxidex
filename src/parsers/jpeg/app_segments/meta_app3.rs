//! APP3 "Meta" parser (ExifTool `Image::ExifTool::Kodak::Meta`)
//!
//! Kodak cameras (DC280, DC3400, DC5000, MC3, M580, Z950, Z981, ...) write a
//! second TIFF directory in the JPEG APP3 segment. The container is identical
//! to the APP1 "Exif" segment -- a 6-byte identifier followed by a TIFF header
//! and one IFD -- but the tag ids come from their own table.
//!
//! ExifTool dispatches on `/^(Meta|META|Exif)\0\0/` and parses the remainder
//! as TIFF with both the directory start and the offset base at byte 6
//! (ExifTool.pm:7990). Tags are reported in family-0 group `Meta`
//! (family 1 `MetaIFD`).
//!
//! Tag ids below were read from Kodak.pm directly. They are all 32-bit-safe
//! ids in the 0xc350..0xc46e range; the generated `oxidex-tags-*` YAML is not
//! a usable source for them because its ids are truncated to 16 bits.

use crate::core::{MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// The three APP3 identifiers ExifTool accepts for this directory.
const META_IDENTIFIERS: [&[u8]; 3] = [b"Meta\0\0", b"META\0\0", b"Exif\0\0"];

/// Byte offset of the TIFF header inside the APP3 payload, which is also the
/// base every IFD value offset is relative to.
const TIFF_BASE: usize = 6;

/// Maps a Kodak Meta tag id to ExifTool's tag name.
///
/// Ids ExifTool does not list are `Unknown` and stay hidden, so they are
/// skipped rather than emitted under a synthesised name.
fn meta_tag_name(id: u16) -> Option<&'static str> {
    Some(match id {
        0xc350 => "FilmProductCode",
        0xc351 => "ImageSourceEK",
        0xc352 => "CaptureConditionsPAR",
        0xc353 => "CameraOwner",
        0xc354 => "SerialNumber",
        0xc355 => "UserSelectGroupTitle",
        0xc356 => "DealerIDNumber",
        0xc357 => "CaptureDeviceFID",
        0xc358 => "EnvelopeNumber",
        0xc359 => "FrameNumber",
        0xc35a => "FilmCategory",
        0xc35b => "FilmGencode",
        0xc35c => "ModelAndVersion",
        0xc35d => "FilmSize",
        0xc35e => "SBA_RGBShifts",
        0xc35f => "SBAInputImageColorspace",
        0xc360 => "SBAInputImageBitDepth",
        0xc361 => "SBAExposureRecord",
        0xc362 => "UserAdjSBA_RGBShifts",
        0xc363 => "ImageRotationStatus",
        0xc364 => "RollGuidElements",
        0xc365 => "MetadataNumber",
        0xc366 => "EditTagArray",
        0xc367 => "Magnification",
        0xc36c => "NativeXResolution",
        0xc36d => "NativeYResolution",
        0xc37a => "NativeResolutionUnit",
        0xc418 => "SourceImageDirectory",
        0xc419 => "SourceImageFileName",
        0xc41a => "SourceImageVolumeName",
        0xc46c => "PrintQuality",
        0xc46e => "ImagePrintStatus",
        // 0xc36e/0xc36f are the KodakEffectsIFD/KodakBordersIFD sub-IFDs.
        // No sample in the corpus exercises them, so they are deliberately
        // not walked rather than parsed against an unverified layout.
        _ => return None,
    })
}

/// Tags carrying `Binary => 1` in Kodak.pm: ExifTool reports the LENGTH OF
/// THE CONVERTED VALUE, not the tag's raw byte count, so UserAdjSBA_RGBShifts
/// (int32s[3]) reports 5 bytes -- the length of the string "0 0 0".
/// Verified with `exiftool -b -UserAdjSBA_RGBShifts` on ExifTool.jpg.
fn is_binary_tag(id: u16) -> bool {
    matches!(id, 0xc361 | 0xc362)
}

/// Tags read through `Exif::ConvertExifText`, which strips the 8-byte EXIF
/// character-code prefix when one is present.
fn is_exif_text_tag(id: u16) -> bool {
    matches!(id, 0xc353 | 0xc354)
}

/// TIFF byte order of the directory.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Endian {
    Little,
    Big,
}

impl Endian {
    fn u16(self, bytes: &[u8]) -> u16 {
        let raw = [bytes[0], bytes[1]];
        match self {
            Endian::Little => u16::from_le_bytes(raw),
            Endian::Big => u16::from_be_bytes(raw),
        }
    }

    fn u32(self, bytes: &[u8]) -> u32 {
        let raw = [bytes[0], bytes[1], bytes[2], bytes[3]];
        match self {
            Endian::Little => u32::from_le_bytes(raw),
            Endian::Big => u32::from_be_bytes(raw),
        }
    }
}

/// Byte width of one element of each TIFF format code (0 for unknown codes).
fn format_size(format: u16) -> usize {
    match format {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => 0,
    }
}

/// Parses an APP3 "Meta" segment.
///
/// # Arguments
///
/// * `data` - Raw APP3 segment payload, including the 6-byte identifier
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Tags keyed `Meta:<Name>`
/// * `Err(ExifToolError)` - If the identifier or TIFF header is not valid
///
/// # Errors
///
/// Returns an error when the payload does not begin with one of ExifTool's
/// three accepted identifiers or the TIFF header is malformed.
pub fn parse_meta_app3(data: &[u8]) -> Result<MetadataMap> {
    if !META_IDENTIFIERS.iter().any(|id| data.starts_with(id)) {
        return Err(ExifToolError::parse_error(
            "APP3 segment is not a Kodak Meta directory",
        ));
    }
    let tiff = data
        .get(TIFF_BASE..)
        .filter(|t| t.len() >= 8)
        .ok_or_else(|| ExifToolError::parse_error("APP3 Meta segment has no TIFF header"))?;

    let endian = match &tiff[0..2] {
        b"II" => Endian::Little,
        b"MM" => Endian::Big,
        _ => {
            return Err(ExifToolError::parse_error(
                "APP3 Meta segment has an invalid TIFF byte order mark",
            ));
        }
    };

    let mut metadata = MetadataMap::new();
    let ifd_offset = endian.u32(&tiff[4..8]) as usize;
    read_ifd(tiff, ifd_offset, endian, &mut metadata);
    Ok(metadata)
}

/// Walks one IFD, inserting every entry whose id is in the Meta table.
fn read_ifd(tiff: &[u8], offset: usize, endian: Endian, metadata: &mut MetadataMap) {
    let Some(count_bytes) = tiff.get(offset..offset + 2) else {
        return;
    };
    let count = endian.u16(count_bytes) as usize;

    for i in 0..count {
        let entry_start = offset + 2 + i * 12;
        let Some(entry) = tiff.get(entry_start..entry_start + 12) else {
            return;
        };
        let id = endian.u16(&entry[0..2]);
        let format = endian.u16(&entry[2..4]);
        let element_count = endian.u32(&entry[4..8]) as usize;

        let Some(name) = meta_tag_name(id) else {
            continue;
        };
        let Some(size) = format_size(format).checked_mul(element_count) else {
            continue;
        };
        if size == 0 {
            continue;
        }

        // Values of four bytes or fewer live in the entry itself; anything
        // larger is at an offset relative to the TIFF header.
        let raw = if size <= 4 {
            entry.get(8..8 + size)
        } else {
            let value_offset = endian.u32(&entry[8..12]) as usize;
            tiff.get(value_offset..value_offset.saturating_add(size))
        };
        let Some(raw) = raw else {
            continue;
        };

        if let Some(value) = convert_value(id, format, element_count, raw, endian) {
            metadata.insert(format!("Meta:{}", name), value);
        }
    }
}

/// Renders one IFD entry the way ExifTool does for this table.
fn convert_value(
    id: u16,
    format: u16,
    element_count: usize,
    raw: &[u8],
    endian: Endian,
) -> Option<TagValue> {
    let rendered = match format {
        // string
        2 => trim_at_nul(&String::from_utf8_lossy(raw)),
        // undef / int8u / int8s carrying text-like payloads
        7 => {
            if is_exif_text_tag(id) {
                convert_exif_text(raw)
            } else {
                trim_at_nul(&String::from_utf8_lossy(raw))
            }
        }
        _ => {
            let size = format_size(format);
            let mut parts = Vec::with_capacity(element_count);
            for i in 0..element_count {
                let chunk = raw.get(i * size..(i + 1) * size)?;
                parts.push(match format {
                    1 => (chunk[0] as i64).to_string(),
                    6 => (chunk[0] as i8 as i64).to_string(),
                    3 => (endian.u16(chunk) as i64).to_string(),
                    8 => (endian.u16(chunk) as i16 as i64).to_string(),
                    4 => (endian.u32(chunk) as i64).to_string(),
                    9 => (endian.u32(chunk) as i32 as i64).to_string(),
                    5 | 10 => {
                        let numerator = endian.u32(&chunk[0..4]);
                        let denominator = endian.u32(&chunk[4..8]);
                        if format == 5 {
                            format!("{}/{}", numerator, denominator)
                        } else {
                            format!("{}/{}", numerator as i32, denominator as i32)
                        }
                    }
                    11 => format!("{}", f32::from_bits(endian.u32(chunk))),
                    12 => {
                        let hi = endian.u32(&chunk[0..4]) as u64;
                        let lo = endian.u32(&chunk[4..8]) as u64;
                        let bits = match endian {
                            Endian::Little => (lo << 32) | hi,
                            Endian::Big => (hi << 32) | lo,
                        };
                        format!("{}", f64::from_bits(bits))
                    }
                    _ => return None,
                });
            }
            parts.join(" ")
        }
    };

    if !is_binary_tag(id) {
        return Some(TagValue::String(rendered));
    }
    // `Binary => 1`: ExifTool hides the value behind a byte count. For an
    // `undef` tag that is the raw payload; for a numeric tag it is the
    // rendered string, which is why "0 0 0" reports as 5 bytes.
    Some(TagValue::Binary(if format == 7 {
        raw.to_vec()
    } else {
        rendered.into_bytes()
    }))
}

/// Truncates at the first NUL, matching ExifTool's handling of TIFF strings.
fn trim_at_nul(text: &str) -> String {
    match text.split_once('\0') {
        Some((head, _)) => head.to_string(),
        None => text.to_string(),
    }
}

/// `Image::ExifTool::Exif::ConvertExifText`: drops the 8-byte EXIF character
/// code when the value carries one, then trims the trailing NUL padding.
fn convert_exif_text(raw: &[u8]) -> String {
    const CHARACTER_CODES: [&[u8; 8]; 4] = [
        b"ASCII\0\0\0",
        b"UNICODE\0",
        b"JIS\0\0\0\0\0",
        b"\0\0\0\0\0\0\0\0",
    ];
    let body = CHARACTER_CODES
        .iter()
        .find(|code| raw.starts_with(**code))
        .map_or(raw, |code| &raw[code.len()..]);
    String::from_utf8_lossy(body)
        .trim_end_matches('\0')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a little-endian APP3 Meta payload from `(id, format, count,
    /// value)` entries, placing oversized values after the directory.
    fn meta_segment(entries: &[(u16, u16, u32, Vec<u8>)]) -> Vec<u8> {
        let mut tiff = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        tiff.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        let directory_end = 8 + 2 + entries.len() * 12 + 4;
        let mut overflow: Vec<u8> = Vec::new();
        for (id, format, count, value) in entries {
            tiff.extend_from_slice(&id.to_le_bytes());
            tiff.extend_from_slice(&format.to_le_bytes());
            tiff.extend_from_slice(&count.to_le_bytes());
            if value.len() <= 4 {
                let mut padded = value.clone();
                padded.resize(4, 0);
                tiff.extend_from_slice(&padded);
            } else {
                let offset = (directory_end + overflow.len()) as u32;
                tiff.extend_from_slice(&offset.to_le_bytes());
                overflow.extend_from_slice(value);
            }
        }
        tiff.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
        tiff.extend_from_slice(&overflow);

        let mut segment = b"Meta\0\0".to_vec();
        segment.extend_from_slice(&tiff);
        segment
    }

    fn le16s(values: &[u16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    fn le32s(values: &[i32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    /// The 16 entries carried by combined-samples/ExifTool.jpg. Expected
    /// values come from `exiftool -json -G combined-samples/ExifTool.jpg`
    /// (ExifTool 13.55).
    #[test]
    fn test_exiftool_jpg_meta_directory_matches_exiftool() {
        let segment = meta_segment(&[
            (0xc350, 2, 3, b"37\0".to_vec()),
            (0xc351, 3, 1, le16s(&[1])),
            (0xc352, 3, 1, le16s(&[1])),
            (0xc359, 3, 1, le16s(&[0])),
            (0xc35a, 3, 1, le16s(&[2])),
            (0xc35b, 2, 2, b"2\0".to_vec()),
            (0xc35c, 2, 10, b"Version 9\0".to_vec()),
            (0xc35d, 3, 1, le16s(&[1])),
            (0xc35e, 9, 3, le32s(&[0, 0, 0])),
            (0xc35f, 3, 1, le16s(&[3])),
            (0xc360, 3, 3, le16s(&[12, 12, 12])),
            (0xc361, 7, 368, vec![0u8; 368]),
            (0xc362, 9, 3, le32s(&[0, 0, 0])),
            (0xc363, 3, 1, le16s(&[0])),
            (0xc364, 7, 32, b"00000000000000000000000000000000".to_vec()),
            (0xc365, 7, 4, b"0100".to_vec()),
        ]);
        let m = parse_meta_app3(&segment).unwrap();

        assert_eq!(m.get_string("Meta:FilmProductCode"), Some("37"));
        assert_eq!(m.get_string("Meta:ImageSourceEK"), Some("1"));
        assert_eq!(m.get_string("Meta:CaptureConditionsPAR"), Some("1"));
        assert_eq!(m.get_string("Meta:FrameNumber"), Some("0"));
        assert_eq!(m.get_string("Meta:FilmCategory"), Some("2"));
        assert_eq!(m.get_string("Meta:FilmGencode"), Some("2"));
        assert_eq!(m.get_string("Meta:ModelAndVersion"), Some("Version 9"));
        assert_eq!(m.get_string("Meta:FilmSize"), Some("1"));
        assert_eq!(m.get_string("Meta:SBA_RGBShifts"), Some("0 0 0"));
        assert_eq!(m.get_string("Meta:SBAInputImageColorspace"), Some("3"));
        assert_eq!(m.get_string("Meta:SBAInputImageBitDepth"), Some("12 12 12"));
        assert_eq!(m.get_string("Meta:ImageRotationStatus"), Some("0"));
        assert_eq!(
            m.get_string("Meta:RollGuidElements"),
            Some("00000000000000000000000000000000")
        );
        assert_eq!(m.get_string("Meta:MetadataNumber"), Some("0100"));

        // `exiftool -b -SBAExposureRecord` returns 368 raw bytes, while
        // `exiftool -b -UserAdjSBA_RGBShifts` returns the 5-byte string
        // "0 0 0" -- the CONVERTED value, not the 12 raw bytes.
        let TagValue::Binary(record) = m.get("Meta:SBAExposureRecord").unwrap() else {
            panic!("SBAExposureRecord should be binary");
        };
        assert_eq!(record.len(), 368);
        let TagValue::Binary(shifts) = m.get("Meta:UserAdjSBA_RGBShifts").unwrap() else {
            panic!("UserAdjSBA_RGBShifts should be binary");
        };
        assert_eq!(shifts, b"0 0 0");

        assert_eq!(m.len(), 16);
    }

    #[test]
    fn test_unlisted_ids_are_skipped() {
        // 0xc36b and 0xc46f are commented out in Kodak.pm, so ExifTool
        // treats them as unknown and hides them.
        let segment = meta_segment(&[
            (0xc36b, 2, 4, b"1.0\0".to_vec()),
            (0xc46f, 3, 1, le16s(&[1])),
            (0xc350, 2, 3, b"37\0".to_vec()),
        ]);
        let m = parse_meta_app3(&segment).unwrap();
        assert_eq!(m.get_string("Meta:FilmProductCode"), Some("37"));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn test_all_three_identifiers_accepted() {
        for identifier in ["Meta\0\0", "META\0\0", "Exif\0\0"] {
            let mut segment = meta_segment(&[(0xc350, 2, 3, b"37\0".to_vec())]);
            segment[..6].copy_from_slice(identifier.as_bytes());
            let m = parse_meta_app3(&segment).unwrap();
            assert_eq!(m.get_string("Meta:FilmProductCode"), Some("37"));
        }
    }

    #[test]
    fn test_non_meta_identifier_rejected() {
        let mut segment = meta_segment(&[(0xc350, 2, 3, b"37\0".to_vec())]);
        segment[..6].copy_from_slice(b"Stim\0\0");
        assert!(parse_meta_app3(&segment).is_err());
    }

    #[test]
    fn test_convert_exif_text_strips_character_code() {
        assert_eq!(convert_exif_text(b"ASCII\0\0\0Kodak\0"), "Kodak");
        assert_eq!(convert_exif_text(b"\0\0\0\0\0\0\0\0Kodak"), "Kodak");
        assert_eq!(convert_exif_text(b"Kodak"), "Kodak");
    }

    #[test]
    fn test_truncated_directory_is_not_fatal() {
        let mut segment = meta_segment(&[
            (0xc350, 2, 3, b"37\0".to_vec()),
            (0xc351, 3, 1, le16s(&[1])),
        ]);
        segment.truncate(segment.len() - 14);
        let m = parse_meta_app3(&segment).unwrap();
        assert_eq!(m.get_string("Meta:FilmProductCode"), Some("37"));
    }
}
