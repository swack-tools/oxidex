//! APP13 Photoshop Image Resource Block (IRB) parser
//!
//! JPEG APP13 segments (marker 0xFFED) carry Adobe Photoshop metadata as a
//! run of Image Resource Blocks ("8BIM"). This module extracts the resources
//! ExifTool reports by default from `%Image::ExifTool::Photoshop::Main`;
//! resource 0x0404 (IPTCData) is left to the dedicated IPTC parser, and every
//! resource ExifTool marks `Unknown => 1` is skipped so oxidex does not emit
//! tags ExifTool hides.
//!
//! # Photoshop IRB Format
//!
//! The APP13 Photoshop segment has the following structure:
//! - Signature: "Photoshop 3.0\0" (14 bytes)
//! - Image Resource Blocks: Sequence of 8BIM blocks
//!
//! Each 8BIM block contains:
//! - Signature: "8BIM" (4 bytes)
//! - Resource ID: 2 bytes (big-endian)
//! - Resource Name: Pascal string (1 byte length + data), padded to even length
//! - Data Size: 4 bytes (big-endian)
//! - Data: variable length, padded to even length
//!
//! # Binary sub-directories
//!
//! Several resources are `ProcessBinaryData` tables. In those tables the
//! numeric keys are INDICES in units of the table's default `FORMAT`, not
//! byte offsets: `Photoshop::Resolution` declares `FORMAT => 'int16u'` (so
//! index 2 is byte 4) and `Photoshop::JPEG_Quality` declares
//! `FORMAT => 'int16s'`, while `SliceInfo`, `VersionInfo` and
//! `PrintScaleInfo` have no `FORMAT` and therefore default to `int8u`, where
//! index and byte offset coincide. `var_ustr32` entries shift every later
//! index by the byte length of the decoded string (ExifTool.pm:10012).

use super::perl_number;
use crate::core::{MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use nom::{
    IResult,
    bytes::complete::{tag, take},
    number::complete::{be_u16, be_u32, u8 as nom_u8},
};

// Constants
const PHOTOSHOP_SIGNATURE: &[u8] = b"Photoshop 3.0\0";
const EIGHTBIM_SIGNATURE: &[u8] = b"8BIM";

// Resource IDs handled here (ids from Photoshop.pm's %Photoshop::Main)
const RES_JPEG_QUALITY: u16 = 0x0406;
const RES_RESOLUTION_INFO: u16 = 0x03ED;
const RES_COPYRIGHT_FLAG: u16 = 0x040A;
const RES_URL: u16 = 0x040B;
const RES_GLOBAL_ANGLE: u16 = 0x040D;
const RES_GLOBAL_ALTITUDE: u16 = 0x0419;
const RES_SLICE_INFO: u16 = 0x041A;
const RES_URL_LIST: u16 = 0x041E;
const RES_VERSION_INFO: u16 = 0x0421;
const RES_IPTC_DIGEST: u16 = 0x0425;
const RES_PRINT_SCALE_INFO: u16 = 0x0426;

/// Represents an Adobe Photoshop Image Resource Block
#[derive(Debug, Clone, PartialEq)]
struct ImageResourceBlock<'a> {
    /// Resource ID (e.g., 0x03ED for Resolution Info)
    id: u16,
    /// Resource name (Pascal string)
    #[allow(dead_code)]
    name: &'a [u8],
    /// Resource data payload
    data: &'a [u8],
}

/// Parses a single Adobe Photoshop Image Resource Block (8BIM).
///
/// # Format
/// - Signature: "8BIM" (4 bytes)
/// - ID: 2 bytes (big-endian)
/// - Name: Pascal string (1 byte length + data), padded to even length
/// - Size: 4 bytes (big-endian)
/// - Data: variable length, padded to even length
fn parse_image_resource_block(input: &[u8]) -> IResult<&[u8], ImageResourceBlock<'_>> {
    // Parse 8BIM signature
    let (input, _) = tag(EIGHTBIM_SIGNATURE)(input)?;

    // Parse resource ID (2 bytes, big-endian)
    let (input, id) = be_u16(input)?;

    // Parse Pascal string name (1 byte length + data)
    let (input, name_length) = nom_u8(input)?;
    let (input, name) = take(name_length as usize)(input)?;

    // Pascal string must be padded to even length (including length byte)
    let total_name_length = 1 + name_length as usize;
    let (input, _) = if total_name_length % 2 == 1 {
        take(1usize)(input)?
    } else {
        (input, &b""[..])
    };

    // Parse data size (4 bytes, big-endian)
    let (input, data_size) = be_u32(input)?;

    // Parse data
    let (input, data) = take(data_size as usize)(input)?;

    // Data must be padded to even length
    let (input, _) = if data_size % 2 == 1 {
        take(1usize)(input)?
    } else {
        (input, &b""[..])
    };

    Ok((input, ImageResourceBlock { id, name, data }))
}

/// Big-endian readers over a resource payload; `None` when out of range so a
/// truncated resource drops its remaining tags instead of inventing values.
fn be_u16_at(data: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(off..off + 2)?.try_into().ok()?))
}

fn be_i16_at(data: &[u8], off: usize) -> Option<i16> {
    be_u16_at(data, off).map(|v| v as i16)
}

fn be_u32_at(data: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(off..off + 4)?.try_into().ok()?))
}

fn be_f32_at(data: &[u8], off: usize) -> Option<f32> {
    be_u32_at(data, off).map(f32::from_bits)
}

/// Formats a PrintConv hash miss the way ExifTool does, so an unrecognised
/// code reports itself instead of being rounded to a neighbouring label.
fn unknown_code(value: i64) -> TagValue {
    TagValue::String(format!("Unknown ({})", value))
}

/// Reads a `var_ustr32` value at `offset`: an int32u character count followed
/// by that many UTF-16BE code units.
///
/// Returns the decoded string and the byte length of the character data,
/// which is what ExifTool adds to `$varSize` to shift every later index.
fn read_var_ustr32(data: &[u8], offset: usize) -> Option<(String, usize)> {
    let chars = be_u32_at(data, offset)? as usize;
    let byte_len = chars.checked_mul(2)?;
    let start = offset + 4;
    let bytes = data.get(start..start.checked_add(byte_len)?)?;
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    let decoded = String::from_utf16_lossy(&units);
    // ExifTool truncates the decoded string at its first NUL.
    let decoded = match decoded.split_once('\0') {
        Some((head, _)) => head.to_string(),
        None => decoded,
    };
    Some((decoded, byte_len))
}

/// `%Photoshop::Resolution` (resource 0x03ED), `FORMAT => 'int16u'`.
///
/// Index 0 -> byte 0 (XResolution, int32u), index 2 -> byte 4
/// (DisplayedUnitsX), index 4 -> byte 8 (YResolution), index 6 -> byte 12
/// (DisplayedUnitsY).
fn parse_resolution_info(data: &[u8], metadata: &mut MetadataMap) {
    // ValueConv '$val / 0x10000', PrintConv 'int($val * 100 + 0.5) / 100'
    let resolution = |raw: u32| -> TagValue {
        let value = raw as f64 / 65536.0;
        TagValue::String(perl_number((value * 100.0 + 0.5).trunc() / 100.0))
    };
    let displayed_units = |raw: u16| -> TagValue {
        match raw {
            1 => TagValue::String("inches".to_string()),
            2 => TagValue::String("cm".to_string()),
            other => unknown_code(other as i64),
        }
    };

    if let Some(raw) = be_u32_at(data, 0) {
        metadata.insert("Photoshop:XResolution", resolution(raw));
    }
    if let Some(raw) = be_u16_at(data, 4) {
        metadata.insert("Photoshop:DisplayedUnitsX", displayed_units(raw));
    }
    if let Some(raw) = be_u32_at(data, 8) {
        metadata.insert("Photoshop:YResolution", resolution(raw));
    }
    if let Some(raw) = be_u16_at(data, 12) {
        metadata.insert("Photoshop:DisplayedUnitsY", displayed_units(raw));
    }
}

/// `%Photoshop::PrintScaleInfo` (resource 0x0426); no `FORMAT`, so indices
/// are byte offsets.
fn parse_print_scale_info(data: &[u8], metadata: &mut MetadataMap) {
    if let Some(style) = be_u16_at(data, 0) {
        let value = match style {
            0 => TagValue::String("Centered".to_string()),
            1 => TagValue::String("Size to Fit".to_string()),
            2 => TagValue::String("User Defined".to_string()),
            other => unknown_code(other as i64),
        };
        metadata.insert("Photoshop:PrintStyle", value);
    }
    if let (Some(x), Some(y)) = (be_f32_at(data, 2), be_f32_at(data, 6)) {
        metadata.insert(
            "Photoshop:PrintPosition",
            TagValue::String(format!(
                "{} {}",
                perl_number(x as f64),
                perl_number(y as f64)
            )),
        );
    }
    if let Some(scale) = be_f32_at(data, 10) {
        metadata.insert(
            "Photoshop:PrintScale",
            TagValue::String(perl_number(scale as f64)),
        );
    }
}

/// `%Photoshop::SliceInfo` (resource 0x041A); no `FORMAT`, so indices are
/// byte offsets, and NumSlices sits after the variable-length group name.
fn parse_slice_info(data: &[u8], metadata: &mut MetadataMap) {
    let mut var_size = 0usize;
    if let Some((name, byte_len)) = read_var_ustr32(data, 20) {
        metadata.insert("Photoshop:SlicesGroupName", TagValue::String(name));
        var_size = byte_len;
    }
    if let Some(count) = be_u32_at(data, 24 + var_size) {
        metadata.insert("Photoshop:NumSlices", TagValue::Integer(count as i64));
    }
}

/// `%Photoshop::VersionInfo` (resource 0x0421); no `FORMAT`, so indices are
/// byte offsets, and ReaderName sits after the variable-length WriterName.
fn parse_version_info(data: &[u8], metadata: &mut MetadataMap) {
    if let Some(&flag) = data.get(4) {
        let value = match flag {
            0 => TagValue::String("No".to_string()),
            1 => TagValue::String("Yes".to_string()),
            other => unknown_code(other as i64),
        };
        metadata.insert("Photoshop:HasRealMergedData", value);
    }
    let mut var_size = 0usize;
    if let Some((writer, byte_len)) = read_var_ustr32(data, 5) {
        metadata.insert("Photoshop:WriterName", TagValue::String(writer));
        var_size = byte_len;
    }
    if let Some((reader, _)) = read_var_ustr32(data, 9 + var_size) {
        metadata.insert("Photoshop:ReaderName", TagValue::String(reader));
    }
}

/// `%Photoshop::JPEG_Quality` (resource 0x0406), `FORMAT => 'int16s'`.
///
/// Index 0 -> byte 0, index 1 -> byte 2, index 2 -> byte 4.
fn parse_jpeg_quality(data: &[u8], metadata: &mut MetadataMap) {
    if let Some(quality) = be_i16_at(data, 0) {
        // PrintConv => '$val + 4'
        metadata.insert(
            "Photoshop:PhotoshopQuality",
            TagValue::Integer(quality as i64 + 4),
        );
    }
    let Some(format) = be_i16_at(data, 2) else {
        return;
    };
    let format_value = match format {
        0x0000 => TagValue::String("Standard".to_string()),
        0x0001 => TagValue::String("Optimized".to_string()),
        0x0101 => TagValue::String("Progressive".to_string()),
        other => unknown_code(other as i64),
    };
    metadata.insert("Photoshop:PhotoshopFormat", format_value);

    // ProgressiveScans has Condition '$$self{PhotoshopFormat} == 0x0101'
    if format != 0x0101 {
        return;
    }
    if let Some(scans) = be_i16_at(data, 4) {
        let value = match scans {
            1 => TagValue::String("3 Scans".to_string()),
            2 => TagValue::String("4 Scans".to_string()),
            3 => TagValue::String("5 Scans".to_string()),
            other => unknown_code(other as i64),
        };
        metadata.insert("Photoshop:ProgressiveScans", value);
    }
}

/// Resource 0x041E: an int32u entry count, then per entry a skipped word and
/// ID followed by a `var_ustr32` URL.
///
/// ExifTool declares this `List => 1`; oxidex joins the entries with a space
/// (an empty list becomes an empty string), matching how the rest of the
/// codebase renders ExifTool list values.
fn parse_url_list(data: &[u8]) -> Option<TagValue> {
    let count = be_u32_at(data, 0)?;
    let mut urls: Vec<String> = Vec::new();
    let mut pos = 4usize;
    for _ in 0..count {
        pos += 8; // skip the word and ID preceding each URL
        let Some((url, byte_len)) = read_var_ustr32(data, pos) else {
            break;
        };
        urls.push(url);
        pos += 4 + byte_len;
    }
    Some(TagValue::String(urls.join(" ")))
}

/// Extracts Photoshop metadata from APP13 segment data.
///
/// This function parses Photoshop Image Resource Blocks (8BIM) from APP13
/// segments and extracts the Photoshop-specific tags ExifTool reports by
/// default. IPTC (resource 0x0404) is handled by the IPTC parser instead.
///
/// # Parameters
///
/// - `data`: Raw APP13 segment data (must start with "Photoshop 3.0\0")
///
/// # Returns
///
/// `MetadataMap` containing extracted Photoshop tags, or error if parsing fails.
///
/// # Errors
///
/// Returns `ParseError` if the data doesn't start with the Photoshop signature.
pub fn parse_photoshop_irb(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Check for Photoshop signature
    if !data.starts_with(PHOTOSHOP_SIGNATURE) {
        return Err(ExifToolError::parse_error("Not a Photoshop IRB segment"));
    }

    // Skip past the Photoshop signature
    let mut current = &data[PHOTOSHOP_SIGNATURE.len()..];

    // Parse all 8BIM resource blocks
    while current.len() > 4 {
        // Check if this looks like a 8BIM block
        if !current.starts_with(EIGHTBIM_SIGNATURE) {
            break;
        }

        let Ok((remaining, block)) = parse_image_resource_block(current) else {
            // Failed to parse block, stop processing
            break;
        };

        match block.id {
            RES_RESOLUTION_INFO => parse_resolution_info(block.data, &mut metadata),
            RES_JPEG_QUALITY => parse_jpeg_quality(block.data, &mut metadata),
            RES_SLICE_INFO => parse_slice_info(block.data, &mut metadata),
            RES_VERSION_INFO => parse_version_info(block.data, &mut metadata),
            RES_PRINT_SCALE_INFO => parse_print_scale_info(block.data, &mut metadata),
            RES_COPYRIGHT_FLAG => {
                if let Some(&flag) = block.data.first() {
                    let value = match flag {
                        0 => TagValue::String("False".to_string()),
                        1 => TagValue::String("True".to_string()),
                        other => unknown_code(other as i64),
                    };
                    metadata.insert("Photoshop:CopyrightFlag", value);
                }
            }
            RES_URL => {
                // Writable => 'string': only the terminating NUL is dropped,
                // the padding spaces this resource often carries are kept.
                let text = String::from_utf8_lossy(block.data);
                metadata.insert(
                    "Photoshop:URL",
                    TagValue::String(text.trim_end_matches('\0').to_string()),
                );
            }
            RES_URL_LIST => {
                if let Some(value) = parse_url_list(block.data) {
                    metadata.insert("Photoshop:URL_List", value);
                }
            }
            RES_GLOBAL_ANGLE => {
                if let Some(angle) = be_u32_at(block.data, 0) {
                    metadata.insert("Photoshop:GlobalAngle", TagValue::Integer(angle as i64));
                }
            }
            RES_GLOBAL_ALTITUDE => {
                if let Some(altitude) = be_u32_at(block.data, 0) {
                    metadata.insert(
                        "Photoshop:GlobalAltitude",
                        TagValue::Integer(altitude as i64),
                    );
                }
            }
            RES_IPTC_DIGEST => {
                // ValueConv => 'unpack("H*", $val)'
                metadata.insert(
                    "Photoshop:IPTCDigest",
                    TagValue::String(
                        block
                            .data
                            .iter()
                            .map(|b| format!("{:02x}", b))
                            .collect::<String>(),
                    ),
                );
            }
            // Resource 0x0404 (IPTCData) is parsed by the IPTC parser, and
            // every other resource is either `Unknown => 1` in Photoshop.pm
            // or not yet ported; either way ExifTool shows nothing for it.
            _ => {}
        }

        current = remaining;
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds one 8BIM resource block with an empty resource name.
    fn irb_block(id: u16, data: &[u8]) -> Vec<u8> {
        let mut block = b"8BIM".to_vec();
        block.extend_from_slice(&id.to_be_bytes());
        block.push(0x00); // empty Pascal-string name
        block.push(0x00); // pad to even length
        block.extend_from_slice(&(data.len() as u32).to_be_bytes());
        block.extend_from_slice(data);
        if data.len() % 2 == 1 {
            block.push(0x00);
        }
        block
    }

    fn irb_segment(blocks: &[Vec<u8>]) -> Vec<u8> {
        let mut data = PHOTOSHOP_SIGNATURE.to_vec();
        for block in blocks {
            data.extend_from_slice(block);
        }
        data
    }

    /// The resources carried by combined-samples/ExifTool.jpg, byte for byte.
    /// Every expected value below comes from
    /// `exiftool -json -G combined-samples/ExifTool.jpg` (ExifTool 13.55).
    fn exiftool_jpg_segment() -> Vec<u8> {
        let mut version_info = vec![0x00, 0x00, 0x00, 0x01, 0x01];
        version_info.extend_from_slice(&15u32.to_be_bytes());
        for c in "Adobe Photoshop".encode_utf16() {
            version_info.extend_from_slice(&c.to_be_bytes());
        }
        version_info.extend_from_slice(&19u32.to_be_bytes());
        for c in "Adobe Photoshop 7.0".encode_utf16() {
            version_info.extend_from_slice(&c.to_be_bytes());
        }
        version_info.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);

        let mut slice_info = vec![0x00, 0x00, 0x00, 0x06];
        slice_info.extend_from_slice(&[0u8; 16]);
        slice_info.extend_from_slice(&4u32.to_be_bytes());
        for c in "IPTC".encode_utf16() {
            slice_info.extend_from_slice(&c.to_be_bytes());
        }
        slice_info.extend_from_slice(&1u32.to_be_bytes());

        irb_segment(&[
            irb_block(
                0x0425,
                &[
                    0x05, 0xad, 0x17, 0x70, 0xb1, 0xa9, 0x5f, 0x1f, 0x97, 0x88, 0xac, 0x99, 0x5f,
                    0xa6, 0x47, 0xda,
                ],
            ),
            irb_block(
                0x03ed,
                &[
                    0x00, 0x48, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x48, 0x00, 0x00, 0x00,
                    0x01, 0x00, 0x01,
                ],
            ),
            irb_block(
                0x0426,
                &[
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3f, 0x80, 0x00,
                    0x00,
                ],
            ),
            irb_block(0x040d, &[0x00, 0x00, 0x00, 0x1e]),
            irb_block(0x0419, &[0x00, 0x00, 0x00, 0x1e]),
            irb_block(0x040a, &[0x00]),
            irb_block(0x040b, b"https://exiftool.org/                    "),
            irb_block(0x041e, &[0x00, 0x00, 0x00, 0x00]),
            irb_block(0x041a, &slice_info),
            irb_block(0x0421, &version_info),
            irb_block(0x0406, &[0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x01]),
        ])
    }

    #[test]
    fn test_exiftool_jpg_resources_match_exiftool() {
        let m = parse_photoshop_irb(&exiftool_jpg_segment()).unwrap();

        assert_eq!(
            m.get_string("Photoshop:IPTCDigest"),
            Some("05ad1770b1a95f1f9788ac995fa647da")
        );
        assert_eq!(m.get_string("Photoshop:XResolution"), Some("72"));
        assert_eq!(m.get_string("Photoshop:DisplayedUnitsX"), Some("inches"));
        assert_eq!(m.get_string("Photoshop:YResolution"), Some("72"));
        assert_eq!(m.get_string("Photoshop:DisplayedUnitsY"), Some("inches"));
        assert_eq!(m.get_string("Photoshop:PrintStyle"), Some("Centered"));
        assert_eq!(m.get_string("Photoshop:PrintPosition"), Some("0 0"));
        assert_eq!(m.get_string("Photoshop:PrintScale"), Some("1"));
        assert_eq!(m.get_integer("Photoshop:GlobalAngle"), Some(30));
        assert_eq!(m.get_integer("Photoshop:GlobalAltitude"), Some(30));
        assert_eq!(m.get_string("Photoshop:CopyrightFlag"), Some("False"));
        assert_eq!(
            m.get_string("Photoshop:URL"),
            Some("https://exiftool.org/                    ")
        );
        assert_eq!(m.get_string("Photoshop:URL_List"), Some(""));
        assert_eq!(m.get_string("Photoshop:SlicesGroupName"), Some("IPTC"));
        assert_eq!(m.get_integer("Photoshop:NumSlices"), Some(1));
        assert_eq!(m.get_string("Photoshop:HasRealMergedData"), Some("Yes"));
        assert_eq!(
            m.get_string("Photoshop:WriterName"),
            Some("Adobe Photoshop")
        );
        assert_eq!(
            m.get_string("Photoshop:ReaderName"),
            Some("Adobe Photoshop 7.0")
        );
        assert_eq!(m.get_integer("Photoshop:PhotoshopQuality"), Some(7));
        assert_eq!(m.get_string("Photoshop:PhotoshopFormat"), Some("Standard"));
        // ProgressiveScans is conditional on PhotoshopFormat == 0x0101
        assert!(m.get("Photoshop:ProgressiveScans").is_none());
        assert_eq!(m.len(), 20);
    }

    #[test]
    fn test_hidden_and_iptc_resources_emit_nothing() {
        // 0x0404 belongs to the IPTC parser; 0x03f3/0x0408/0x2710 are all
        // `Unknown => 1` in Photoshop.pm, so ExifTool hides them by default.
        let segment = irb_segment(&[
            irb_block(0x0404, &[0x1c, 0x02, 0x00, 0x00, 0x02]),
            irb_block(0x03f3, &[0x00; 9]),
            irb_block(0x0408, &[0x00; 16]),
            irb_block(0x2710, &[0x00; 10]),
            irb_block(0x040d, &[0x00, 0x00, 0x00, 0x1e]),
        ]);
        let m = parse_photoshop_irb(&segment).unwrap();
        assert_eq!(m.get_integer("Photoshop:GlobalAngle"), Some(30));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn test_progressive_scans_only_when_format_is_progressive() {
        let segment = irb_segment(&[irb_block(
            0x0406,
            &[0x00, 0x08, 0x01, 0x01, 0x00, 0x02, 0x00],
        )]);
        let m = parse_photoshop_irb(&segment).unwrap();
        assert_eq!(m.get_integer("Photoshop:PhotoshopQuality"), Some(12));
        assert_eq!(
            m.get_string("Photoshop:PhotoshopFormat"),
            Some("Progressive")
        );
        assert_eq!(m.get_string("Photoshop:ProgressiveScans"), Some("4 Scans"));
    }

    #[test]
    fn test_unknown_enum_codes_report_themselves() {
        let segment = irb_segment(&[
            irb_block(0x040a, &[0x07]),
            irb_block(
                0x0426,
                &[
                    0x00, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00,
                ],
            ),
            irb_block(
                0x03ed,
                &[
                    0x00, 0x48, 0x00, 0x00, 0x00, 0x09, 0x00, 0x01, 0x00, 0x48, 0x00, 0x00, 0x00,
                    0x09, 0x00, 0x01,
                ],
            ),
        ]);
        let m = parse_photoshop_irb(&segment).unwrap();
        assert_eq!(m.get_string("Photoshop:CopyrightFlag"), Some("Unknown (7)"));
        assert_eq!(m.get_string("Photoshop:PrintStyle"), Some("Unknown (9)"));
        assert_eq!(
            m.get_string("Photoshop:DisplayedUnitsX"),
            Some("Unknown (9)")
        );
    }

    #[test]
    fn test_url_list_with_entries() {
        let mut data = 2u32.to_be_bytes().to_vec();
        for url in ["ab", "cd"] {
            data.extend_from_slice(&[0x00; 8]); // skipped word and ID
            data.extend_from_slice(&(url.len() as u32).to_be_bytes());
            for c in url.encode_utf16() {
                data.extend_from_slice(&c.to_be_bytes());
            }
        }
        let m = parse_photoshop_irb(&irb_segment(&[irb_block(0x041e, &data)])).unwrap();
        assert_eq!(m.get_string("Photoshop:URL_List"), Some("ab cd"));
    }

    #[test]
    fn test_truncated_resource_drops_later_tags() {
        // ResolutionInfo cut short after XResolution: the remaining indices
        // are absent rather than defaulted.
        let segment = irb_segment(&[irb_block(0x03ed, &[0x00, 0x48, 0x00, 0x00, 0x00, 0x01])]);
        let m = parse_photoshop_irb(&segment).unwrap();
        assert_eq!(m.get_string("Photoshop:XResolution"), Some("72"));
        assert_eq!(m.get_string("Photoshop:DisplayedUnitsX"), Some("inches"));
        assert!(m.get("Photoshop:YResolution").is_none());
        assert!(m.get("Photoshop:DisplayedUnitsY").is_none());
    }

    #[test]
    fn test_parse_image_resource_block_minimal() {
        let data = irb_block(0x040D, &[0x00, 0x00, 0x00, 0x1E]);
        let (remaining, block) = parse_image_resource_block(&data).unwrap();
        assert_eq!(block.id, 0x040D);
        assert_eq!(block.data, &[0x00, 0x00, 0x00, 0x1E]);
        assert!(remaining.is_empty());
    }

    #[test]
    fn test_parse_photoshop_irb_invalid_signature() {
        let data = b"NotPhotoshop";
        let result = parse_photoshop_irb(data);
        assert!(result.is_err());
    }
}
