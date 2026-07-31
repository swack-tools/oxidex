//! FLIF (Free Lossless Image Format) parser
//!
//! FLIF format structure (<http://flif.info/>, mirrored by ExifTool's
//! FLIF.pm):
//! - Magic: 4 bytes "FLIF"
//! - Byte 4: image type, an ASCII character in `0x30..=0x6f` encoding
//!   colour channels, interlacing and animation together
//! - Byte 5: bit-depth code, ASCII '0' (custom), '1' (8-bit) or '2' (16-bit)
//! - Width and height: varints, each stored one less than its true value
//! - Frame count: varint stored two less than its true value, present only
//!   when the image-type character is greater than 'H'
//! - Metadata chunks: `<4-byte name><varint length><DEFLATE payload>` for
//!   iCCP, eXif and eXmp, terminated by the image chunk whose first byte is
//!   below 0x20 and which encodes the `Encoding` tag

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::parsers::image::embedded::{
    parse_embedded_exif, parse_embedded_icc, parse_embedded_xmp,
};

const FLIF_SIGNATURE: &[u8] = b"FLIF";

/// Guard against a corrupt chunk length making us allocate the world.
const FLIF_MAX_CHUNK_LEN: u64 = 10_000_000;

/// Parser for FLIF (Free Lossless Image Format) files
///
/// Extracts the FLIF header (image type, bit depth, dimensions, animation
/// frame count and encoding) plus any ICC profile, EXIF or XMP carried in
/// the file's DEFLATE-compressed metadata chunks.
pub struct FLIFParser;

impl FLIFParser {
    /// Verifies the FLIF file signature ("FLIF")
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 4 {
            return Ok(false);
        }
        let header = reader.read(0, 4)?;
        Ok(header == FLIF_SIGNATURE)
    }
}

impl FormatParser for FLIFParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid FLIF signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("FLIF".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        // The whole file is read once: chunk payloads are DEFLATE streams
        // whose inflated size is unknown up front, so slicing them out of
        // one buffer is both simpler and cheaper than seeking.
        let size = reader.size() as usize;
        let data = reader.read(0, size)?;

        parse_flif_header(data, &mut metadata)?;

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::FLIF)
    }
}

/// Decoded image-type character.
struct ImageType {
    /// ExifTool's printed `ImageType` string.
    label: String,
    /// Animated images carry an extra frame-count varint.
    animated: bool,
}

/// Decodes the image-type character (ExifTool FLIF tag 0).
///
/// The character is a single opaque code, not a bit-field: ExifTool spells
/// out all twelve valid values, so an unrecognised one reports its own
/// character rather than being folded onto a neighbouring label.
fn decode_image_type(code: u8) -> ImageType {
    let label = match code {
        b'1' => "Grayscale (non-interlaced)",
        b'3' => "RGB (non-interlaced)",
        b'4' => "RGBA (non-interlaced)",
        b'A' => "Grayscale (interlaced)",
        b'C' => "RGB (interlaced)",
        b'D' => "RGBA (interlaced)",
        b'Q' => "Grayscale Animation (non-interlaced)",
        b'S' => "RGB Animation (non-interlaced)",
        b'T' => "RGBA Animation (non-interlaced)",
        b'a' => "Grayscale Animation (interlaced)",
        b'c' => "RGB Animation (interlaced)",
        b'd' => "RGBA Animation (interlaced)",
        _ => "",
    };
    ImageType {
        label: if label.is_empty() {
            format!("Unknown ({})", code as char)
        } else {
            label.to_string()
        },
        // Animated types are exactly those whose character sorts above 'H'.
        animated: code > b'H',
    }
}

/// Parse FLIF header and extract metadata
fn parse_flif_header(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    if data.len() < 6 {
        return Err(ExifToolError::parse_error("FLIF file too short"));
    }

    let type_code = data[4];
    let depth_code = data[5];
    // ExifTool refuses the file outright unless both header characters are
    // in range, so a malformed header is a parse error rather than a
    // silently mislabelled image.
    if !(0x30..=0x6f).contains(&type_code) || !(b'0'..=b'2').contains(&depth_code) {
        return Err(ExifToolError::parse_error("Invalid FLIF header"));
    }

    let image_type = decode_image_type(type_code);
    metadata.insert(
        "FLIF:ImageType".to_string(),
        TagValue::String(image_type.label),
    );
    metadata.insert(
        "FLIF:BitDepth".to_string(),
        TagValue::String(
            match depth_code {
                b'0' => "Custom",
                b'1' => "8",
                _ => "16",
            }
            .to_string(),
        ),
    );

    let mut pos = 6usize;
    // Width and height are each stored one less than their true value.
    let (width, len) = read_varint(data, pos).ok_or_else(invalid_varint)?;
    pos += len;
    let (height, len) = read_varint(data, pos).ok_or_else(invalid_varint)?;
    pos += len;
    metadata.insert(
        "FLIF:ImageWidth".to_string(),
        TagValue::Integer(i64::from(width) + 1),
    );
    metadata.insert(
        "FLIF:ImageHeight".to_string(),
        TagValue::Integer(i64::from(height) + 1),
    );

    if image_type.animated {
        // Frame count is stored two less than its true value.
        let (frames, len) = read_varint(data, pos).ok_or_else(invalid_varint)?;
        pos += len;
        metadata.insert(
            "FLIF:AnimationFrames".to_string(),
            TagValue::Integer(i64::from(frames) + 2),
        );
    }

    parse_flif_chunks(data, pos, metadata);

    Ok(())
}

fn invalid_varint() -> ExifToolError {
    ExifToolError::parse_error("Invalid FLIF varint")
}

/// Walks the metadata chunks that precede the image data.
fn parse_flif_chunks(data: &[u8], mut pos: usize, metadata: &mut MetadataMap) {
    while pos + 4 <= data.len() {
        let name = [data[pos], data[pos + 1], data[pos + 2], data[pos + 3]];

        // A first byte below 0x20 is the start of the image data, and
        // encodes the Encoding tag rather than naming a chunk.
        if name[0] < 0x20 {
            metadata.insert(
                "FLIF:Encoding".to_string(),
                TagValue::String(if name[0] == 0 {
                    "FLIF16".to_string()
                } else {
                    format!("Unknown ({})", name[0])
                }),
            );
            return;
        }

        pos += 4;

        let Some((len, len_bytes)) = read_varint(data, pos) else {
            return;
        };
        pos += len_bytes;
        if u64::from(len) > FLIF_MAX_CHUNK_LEN {
            return;
        }
        let Some(end) = pos.checked_add(len as usize).filter(|e| *e <= data.len()) else {
            return;
        };

        // Every FLIF metadata chunk is raw-DEFLATE compressed.
        if matches!(&name, b"iCCP" | b"eXif" | b"eXmp")
            && let Some(inflated) = raw_inflate(&data[pos..end])
        {
            match &name {
                b"iCCP" => {
                    parse_embedded_icc(&inflated, metadata);
                }
                b"eXif" => {
                    // ExifTool's SubDirectory declares Start => 6, skipping
                    // the "Exif\0\0" header that FLIF stores ahead of the
                    // TIFF block.
                    let tiff = if inflated.len() > 6 && inflated.starts_with(b"Exif\0\0") {
                        &inflated[6..]
                    } else {
                        &inflated[..]
                    };
                    parse_embedded_exif(tiff, metadata);
                }
                _ => {
                    parse_embedded_xmp(&inflated, metadata);
                }
            }
        }

        pos = end;
    }
}

/// Inflates a raw DEFLATE stream (no zlib or gzip wrapper).
fn raw_inflate(compressed: &[u8]) -> Option<Vec<u8>> {
    use flate2::read::DeflateDecoder;
    use std::io::Read;

    let mut out = Vec::new();
    let mut decoder = DeflateDecoder::new(compressed);
    // A truncated stream still yields everything decoded so far, which is
    // what ExifTool's rawinflate produces too; only a total failure with no
    // output is treated as "not decodable".
    match decoder.read_to_end(&mut out) {
        Ok(_) => Some(out),
        Err(_) if !out.is_empty() => Some(out),
        Err(_) => None,
    }
}

/// Reads a FLIF varint: 7 bits per byte, most-significant group first, high
/// bit marks continuation. Returns the raw value and its encoded length --
/// the per-field `+1`/`+2` biases are applied by the caller because they
/// differ per field.
fn read_varint(data: &[u8], pos: usize) -> Option<(u32, usize)> {
    let mut value: u32 = 0;
    for i in 0..5 {
        let byte = *data.get(pos + i)?;
        value = value
            .checked_mul(128)?
            .checked_add(u32::from(byte & 0x7f))?;
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

/// Parses metadata from FLIF files.
///
/// This is a convenience wrapper around FLIFParser that provides a functional API.
pub fn parse_flif_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = FLIFParser;
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
    fn varints_are_big_endian_groups() {
        assert_eq!(read_varint(&[0x0f], 0), Some((15, 1)));
        assert_eq!(read_varint(&[0x81, 0x00], 0), Some((128, 2)));
        assert_eq!(read_varint(&[0x8e, 0x25], 0), Some((1829, 2)));
        assert_eq!(read_varint(&[0x81], 0), None);
    }

    #[test]
    fn image_type_char_is_a_code_not_a_bitfield() {
        assert_eq!(decode_image_type(b'3').label, "RGB (non-interlaced)");
        assert!(!decode_image_type(b'3').animated);
        assert_eq!(
            decode_image_type(b'S').label,
            "RGB Animation (non-interlaced)"
        );
        assert!(decode_image_type(b'S').animated);
        // Unrecognised codes report themselves.
        assert_eq!(decode_image_type(b'Z').label, "Unknown (Z)");
    }

    /// Header of the ExifTool distribution's FLIF.flif sample: 16x16 RGB.
    #[test]
    fn decodes_sample_header() {
        // "FLIF" '3' '1' 0x0f 0x0f then the image chunk marker.
        let data = b"FLIF31\x0f\x0f\x00\x00\x00\x00";
        let mut metadata = MetadataMap::new();
        parse_flif_header(data, &mut metadata).unwrap();

        assert_eq!(text(&metadata, "FLIF:ImageType"), "RGB (non-interlaced)");
        assert_eq!(text(&metadata, "FLIF:BitDepth"), "8");
        // 0x0f is stored one less than the real dimension.
        assert_eq!(text(&metadata, "FLIF:ImageWidth"), "16");
        assert_eq!(text(&metadata, "FLIF:ImageHeight"), "16");
        assert_eq!(text(&metadata, "FLIF:Encoding"), "FLIF16");
        assert!(metadata.get("FLIF:AnimationFrames").is_none());
    }

    #[test]
    fn animated_header_reads_frame_count() {
        // 'S' is an animated type, so a frame-count varint follows.
        let data = b"FLIFS1\x0f\x0f\x03\x00\x00\x00\x00";
        let mut metadata = MetadataMap::new();
        parse_flif_header(data, &mut metadata).unwrap();
        // 0x03 is stored two less than the real frame count.
        assert_eq!(text(&metadata, "FLIF:AnimationFrames"), "5");
    }

    #[test]
    fn rejects_malformed_header_characters() {
        let mut metadata = MetadataMap::new();
        // 0x7f is outside the valid image-type range.
        assert!(parse_flif_header(b"FLIF\x7f1\x0f\x0f\x00", &mut metadata).is_err());
    }

    #[test]
    fn inflates_deflate_chunk_payloads() {
        use flate2::Compression;
        use flate2::write::DeflateEncoder;
        use std::io::Write;

        let payload = b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"></x:xmpmeta>";
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(payload).unwrap();
        let compressed = encoder.finish().unwrap();
        assert_eq!(raw_inflate(&compressed).as_deref(), Some(&payload[..]));
    }

    #[test]
    fn rejects_non_flif_data() {
        let reader = BufferedReader::from_bytes(b"not a flif file");
        assert!(parse_flif_metadata(&reader).is_err());
    }
}
