//! BPG (Better Portable Graphics) format parser
//!
//! BPG file structure (<http://bellard.org/bpg/>, mirrored by ExifTool's
//! BPG.pm):
//! - Signature: 4 bytes (0x42 0x50 0x47 0xFB)
//! - Bytes 4-5: one big-endian 16-bit field carrying every header flag:
//!   `pixel_format` (0xE000), `alpha1` (0x1000), `bit_depth_minus_8`
//!   (0x0F00), `color_space` (0x00F0), `extension_present` (0x0008),
//!   `alpha2` (0x0004), `limited_range` (0x0002), `animation` (0x0001)
//! - ImageWidth, ImageHeight, ImageLength: ue7 variable-length integers
//! - If `extension_present`: ue7 extension block length, then a sequence of
//!   `<type byte><ue7 length><payload>` extensions (1=EXIF, 2=ICC_Profile,
//!   3=XMP, 4=ThumbnailBPG, 5=AnimationControl)

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::parsers::image::embedded::{
    parse_embedded_exif, parse_embedded_icc, parse_embedded_xmp,
};

const BPG_SIGNATURE: &[u8] = &[0x42, 0x50, 0x47, 0xFB];

/// Longest possible BPG header: signature(4) + flags(2) + three ue7 values
/// of at most 5 bytes each. ExifTool reads the same 21 bytes.
const BPG_MAX_HEADER_LEN: usize = 21;

/// Upper bound on the extension block, matching ExifTool's own guard
/// against a corrupt length sending us off to read the whole disk.
const BPG_MAX_EXTENSION_LEN: u64 = 10_000_000;

/// Extension type codes (ExifTool's `BPG::Extensions` table).
const EXT_EXIF: u8 = 1;
const EXT_ICC_PROFILE: u8 = 2;
const EXT_XMP: u8 = 3;

/// Parser for BPG (Better Portable Graphics) image files
///
/// Extracts the BPG header (dimensions, pixel format, colour space, bit
/// depth, alpha and flags) plus any EXIF, ICC profile and XMP carried in
/// the file's extension block.
pub struct BPGParser;

/// One decoded ue7 integer plus the number of bytes it occupied.
struct Ue7 {
    value: u64,
    len: usize,
}

/// Reads a ue7 variable-length integer.
///
/// ue7 is big-endian: each byte contributes its low 7 bits to the *bottom*
/// of the accumulator (`val = (val << 7) | (byte & 0x7f)`), and the high bit
/// marks continuation. Decoding it little-endian silently yields the right
/// answer for any value below 128 and garbage above, which is why small
/// test images can look correct while real ones do not.
fn read_ue7(data: &[u8], pos: usize) -> Option<Ue7> {
    let mut value: u64 = 0;
    for i in 0..5 {
        let byte = *data.get(pos + i)?;
        value = (value << 7) | u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            // Bits 32-34 set in the 5th byte would overflow 32 bits.
            if i == 4 && byte & 0x70 != 0 {
                return None;
            }
            return Some(Ue7 { value, len: i + 1 });
        }
        // A leading 0x80 is a non-canonical encoding of zero.
        if i == 0 && byte == 0x80 {
            return None;
        }
    }
    None
}

impl BPGParser {
    /// Verifies the BPG file signature (0x42 0x50 0x47 0xFB)
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 4 {
            return Ok(false);
        }
        let header = reader.read(0, 4)?;
        Ok(header == BPG_SIGNATURE)
    }

    /// Decodes the fixed header and returns the offset of the extension
    /// length field, or `None` when no extension block is present.
    fn parse_header(
        data: &[u8],
        metadata: &mut MetadataMap,
    ) -> std::result::Result<Option<u64>, String> {
        if data.len() < 6 {
            return Err("BPG file too short".to_string());
        }
        if &data[0..4] != BPG_SIGNATURE {
            return Err("Invalid BPG signature".to_string());
        }

        // Bytes 4-5 are ONE big-endian 16-bit field; every flag below is a
        // mask over it (ExifTool tags 4 .. 4.4).
        let flags_word = u16::from_be_bytes([data[4], data[5]]);

        let pixel_format = (flags_word & 0xE000) >> 13;
        metadata.insert(
            "BPG:PixelFormat".to_string(),
            TagValue::String(match pixel_format {
                0 => "Grayscale".to_string(),
                1 => "4:2:0 (chroma at 0.5, 0.5)".to_string(),
                2 => "4:2:2 (chroma at 0.5, 0)".to_string(),
                3 => "4:4:4".to_string(),
                4 => "4:2:0 (chroma at 0, 0.5)".to_string(),
                5 => "4:2:2 (chroma at 0, 0)".to_string(),
                // Unknown codes report themselves rather than being
                // rounded onto a neighbouring label.
                other => format!("Unknown ({})", other),
            }),
        );

        // Alpha is a two-bit combination spread across the word (0x1004).
        let alpha = flags_word & 0x1004;
        metadata.insert(
            "BPG:Alpha".to_string(),
            TagValue::String(match alpha {
                0x0000 => "No Alpha Plane".to_string(),
                0x1000 => "Alpha Exists (color not premultiplied)".to_string(),
                0x1004 => "Alpha Exists (color premultiplied)".to_string(),
                0x0004 => "Alpha Exists (W color component)".to_string(),
                other => format!("Unknown (0x{:04x})", other),
            }),
        );

        let bit_depth = i64::from((flags_word & 0x0F00) >> 8) + 8;
        metadata.insert("BPG:BitDepth".to_string(), TagValue::Integer(bit_depth));

        let color_space = (flags_word & 0x00F0) >> 4;
        metadata.insert(
            "BPG:ColorSpace".to_string(),
            TagValue::String(match color_space {
                0 => "YCbCr (BT 601)".to_string(),
                1 => "RGB".to_string(),
                2 => "YCgCo".to_string(),
                3 => "YCbCr (BT 709)".to_string(),
                4 => "YCbCr (BT 2020)".to_string(),
                5 => "BT 2020 Constant Luminance".to_string(),
                other => format!("Unknown ({})", other),
            }),
        );

        // ExifTool's Flags tag is a BITMASK over 0x000b.
        let mut flag_names: Vec<&str> = Vec::new();
        if flags_word & 0x0001 != 0 {
            flag_names.push("Animation");
        }
        if flags_word & 0x0002 != 0 {
            flag_names.push("Limited Range");
        }
        let extension_present = flags_word & 0x0008 != 0;
        if extension_present {
            flag_names.push("Extension Present");
        }
        metadata.insert(
            "BPG:Flags".to_string(),
            TagValue::String(if flag_names.is_empty() {
                "(none)".to_string()
            } else {
                flag_names.join(", ")
            }),
        );

        let mut pos = 6usize;
        let width = read_ue7(data, pos).ok_or("Invalid BPG ImageWidth")?;
        pos += width.len;
        let height = read_ue7(data, pos).ok_or("Invalid BPG ImageHeight")?;
        pos += height.len;
        let image_length = read_ue7(data, pos).ok_or("Invalid BPG ImageLength")?;
        pos += image_length.len;

        metadata.insert(
            "BPG:ImageWidth".to_string(),
            TagValue::Integer(width.value as i64),
        );
        metadata.insert(
            "BPG:ImageHeight".to_string(),
            TagValue::Integer(height.value as i64),
        );
        metadata.insert(
            "BPG:ImageLength".to_string(),
            TagValue::Integer(image_length.value as i64),
        );

        Ok(extension_present.then_some(pos as u64))
    }

    /// Walks the extension block, dispatching each payload to the shared
    /// EXIF/ICC/XMP decoders.
    fn parse_extensions(data: &[u8], metadata: &mut MetadataMap) {
        let mut pos = 0usize;
        while pos < data.len() {
            let ext_type = data[pos];
            pos += 1;
            let Some(len) = read_ue7(data, pos) else {
                break;
            };
            pos += len.len;
            let Ok(len) = usize::try_from(len.value) else {
                break;
            };
            let Some(end) = pos.checked_add(len).filter(|end| *end <= data.len()) else {
                break;
            };
            let mut payload = &data[pos..end];

            if ext_type == EXT_EXIF {
                // libbpg copies the padding byte that follows the
                // "Exif\0" APP1 header into the extension, so the TIFF
                // header can start one byte in. ExifTool skips it with a
                // minor warning; we do the same silently.
                if payload.len() > 3
                    && (payload[1..3] == *b"II" || payload[1..3] == *b"MM")
                    && payload[0..2] != *b"II"
                    && payload[0..2] != *b"MM"
                {
                    payload = &payload[1..];
                }
                parse_embedded_exif(payload, metadata);
            } else if ext_type == EXT_ICC_PROFILE {
                parse_embedded_icc(payload, metadata);
            } else if ext_type == EXT_XMP {
                parse_embedded_xmp(payload, metadata);
            }

            pos = end;
        }
    }
}

impl FormatParser for BPGParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid BPG signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("BPG".to_string()));

        let file_size = reader.size();
        let header_len = (file_size as usize).min(BPG_MAX_HEADER_LEN);
        let header_data = reader.read(0, header_len)?;

        // The header is decoded from its own small buffer; the extension
        // block is then read separately at its real offset. Reading a
        // fixed prefix and hoping the extensions fit inside it silently
        // drops every embedded EXIF/ICC/XMP block larger than the guess.
        let ext_len_offset = match Self::parse_header(header_data, &mut metadata) {
            Ok(offset) => offset,
            Err(_) => return Ok(metadata),
        };

        if let Some(ext_len_offset) = ext_len_offset {
            // ue7 is at most 5 bytes, and may sit right at EOF.
            let avail = file_size.saturating_sub(ext_len_offset).min(5) as usize;
            if avail == 0 {
                return Ok(metadata);
            }
            let len_bytes = reader.read(ext_len_offset, avail)?;
            if let Some(ext_len) = read_ue7(len_bytes, 0) {
                let data_offset = ext_len_offset + ext_len.len as u64;
                if ext_len.value > 0
                    && ext_len.value <= BPG_MAX_EXTENSION_LEN
                    && data_offset.saturating_add(ext_len.value) <= file_size
                    && let Ok(ext_data) = reader.read(data_offset, ext_len.value as usize)
                {
                    Self::parse_extensions(ext_data, &mut metadata);
                }
            }
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::BPG)
    }
}

/// Parses metadata from BPG files.
///
/// This is a convenience wrapper around BPGParser that provides a functional API.
pub fn parse_bpg_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = BPGParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::buffered_reader::BufferedReader;

    /// Renders a tag the way a reader would see it, so expectations can be
    /// written against ExifTool's printed strings regardless of whether the
    /// parser stored a string or an integer.
    fn text(metadata: &MetadataMap, key: &str) -> String {
        match metadata.get(key) {
            Some(TagValue::String(s)) => s.clone(),
            Some(TagValue::Integer(i)) => i.to_string(),
            Some(other) => panic!("{key} is not a printable scalar: {other:?}"),
            None => panic!("missing tag {key}"),
        }
    }

    #[test]
    fn ue7_is_big_endian() {
        // 0x8e 0x25 -> (0x0e << 7) | 0x25 == 1829. Decoded little-endian
        // this would be 0x25 | (0x0e << 7) == the same here by accident of
        // symmetry, so use a case that distinguishes: 0x81 0x00 -> 128.
        assert_eq!(read_ue7(&[0x81, 0x00], 0).unwrap().value, 128);
        assert_eq!(read_ue7(&[0x81, 0x00], 0).unwrap().len, 2);
        assert_eq!(read_ue7(&[0x8e, 0x25], 0).unwrap().value, 1829);
        assert_eq!(read_ue7(&[0x10], 0).unwrap().value, 16);
        assert_eq!(read_ue7(&[0x00], 0).unwrap().value, 0);
        // Non-canonical leading 0x80 and truncated streams are rejected.
        assert!(read_ue7(&[0x80, 0x01], 0).is_none());
        assert!(read_ue7(&[0x81], 0).is_none());
    }

    /// Header from the ExifTool distribution's BPG.bpg sample.
    fn sample_header() -> Vec<u8> {
        vec![
            0x42, 0x50, 0x47, 0xfb, // "BPG\xfb"
            0x40, 0x08, // flags word
            0x10, // width 16
            0x10, // height 16
            0x00, // image length 0 (to EOF)
        ]
    }

    #[test]
    fn decodes_header_flag_word() {
        let mut metadata = MetadataMap::new();
        let ext_offset = BPGParser::parse_header(&sample_header(), &mut metadata).unwrap();

        assert_eq!(
            text(&metadata, "BPG:PixelFormat"),
            "4:2:2 (chroma at 0.5, 0)"
        );
        assert_eq!(text(&metadata, "BPG:Alpha"), "No Alpha Plane");
        assert_eq!(text(&metadata, "BPG:BitDepth"), "8");
        assert_eq!(text(&metadata, "BPG:ColorSpace"), "YCbCr (BT 601)");
        assert_eq!(text(&metadata, "BPG:Flags"), "Extension Present");
        assert_eq!(text(&metadata, "BPG:ImageWidth"), "16");
        assert_eq!(text(&metadata, "BPG:ImageHeight"), "16");
        assert_eq!(text(&metadata, "BPG:ImageLength"), "0");
        // Extension length field starts right after ImageLength.
        assert_eq!(ext_offset, Some(9));
    }

    #[test]
    fn header_without_extension_flag_reports_no_extension_block() {
        let mut header = sample_header();
        header[5] = 0x00; // clear extension-present bit
        let mut metadata = MetadataMap::new();
        let ext_offset = BPGParser::parse_header(&header, &mut metadata).unwrap();
        assert_eq!(ext_offset, None);
        assert_eq!(text(&metadata, "BPG:Flags"), "(none)");
    }

    #[test]
    fn extension_block_carries_exif() {
        // type=1 (EXIF), ue7 length, then a leading pad byte before "II"
        // exactly as libbpg writes it.
        let mut tiff = Vec::new();
        tiff.push(0x00); // libbpg padding byte
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&0x002Au16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x013Bu16.to_le_bytes()); // Artist
        tiff.extend_from_slice(&2u16.to_le_bytes());
        tiff.extend_from_slice(&4u32.to_le_bytes());
        tiff.extend_from_slice(b"Ph\0\0");
        tiff.extend_from_slice(&0u32.to_le_bytes());

        let mut ext = vec![EXT_EXIF];
        assert!(tiff.len() < 128, "length must fit a one-byte ue7");
        ext.push(tiff.len() as u8);
        ext.extend_from_slice(&tiff);

        let mut metadata = MetadataMap::new();
        BPGParser::parse_extensions(&ext, &mut metadata);
        assert!(
            metadata.keys().any(|k| k.ends_with(":Artist")),
            "expected Artist from the EXIF extension, got {:?}",
            metadata.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn rejects_non_bpg_data() {
        let reader = BufferedReader::from_bytes(b"not a bpg file at all");
        assert!(parse_bpg_metadata(&reader).is_err());
    }
}
