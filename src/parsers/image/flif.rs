//! FLIF (Free Lossless Image Format) parser
//!
//! FLIF format structure (per ExifTool's `FLIF.pm` and <http://flif.info/>):
//! - Magic: 4 bytes "FLIF"
//! - Image type: 1 byte, a printable character in `0x30..=0x6f` naming the
//!   channel count, interlacing and animation together
//! - Bit depth: 1 byte, `'0'` (custom), `'1'` (8-bit) or `'2'` (16-bit)
//! - Width, height: varint encoding, each stored one less than its value
//! - Frame count: varint (only when the image type char is greater than `'H'`),
//!   stored two less than its value
//! - Metadata chunks: `iCCP`, `eXif`, `eXmp`, each a 4-byte name, a varint
//!   length, and raw-DEFLATE-compressed content
//! - Image data, introduced by a byte below 0x20 that names the encoding

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::buffered_reader::BufferedReader;
use crate::io::{ByteOrder as EndianByteOrder, EndianReader};
use crate::parsers::tiff::ifd_parser::{ByteOrder, parse_ifd};
use crate::parsers::xmp::rdf_parser::parse_xmp;
use crate::tag_db::lookup_tag_name;

const FLIF_SIGNATURE: &[u8] = b"FLIF";

/// Largest metadata chunk we will inflate, so a corrupt varint length cannot
/// make us allocate without bound.
const MAX_METADATA_CHUNK: u64 = 16 * 1024 * 1024;

/// Parser for FLIF (Free Lossless Image Format) files
///
/// Extracts metadata from FLIF format images including dimensions, color type, bit depth,
/// interlacing, animation, and embedded EXIF data.
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

        // Parse FLIF header and metadata chunks
        parse_flif_header(reader, &mut metadata)?;

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::FLIF)
    }
}

/// Describes a FLIF image type character.
///
/// The character encodes the channel count, interlacing and animation in one
/// value; ExifTool spells the combinations out in `FLIF::Main`'s tag 0.
fn image_type_name(image_type: u8) -> Option<&'static str> {
    Some(match image_type {
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
        _ => return None,
    })
}

/// Parse FLIF header and extract metadata
fn parse_flif_header(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
    if reader.size() < 6 {
        return Err(ExifToolError::parse_error("FLIF file too short"));
    }

    let header = reader.read(4, 2)?;
    let (image_type, bit_depth_code) = (header[0], header[1]);

    // ExifTool's gate is /^FLIF([0-\x6f])([0-2])/: the two header bytes are
    // printable characters, not a packed bitfield. Reading them as a bitfield
    // made every real FLIF file (image type '3' = RGB) decode to an
    // out-of-range channel count and fail before a single tag was emitted.
    if !(0x30..=0x6f).contains(&image_type) || !(b'0'..=b'2').contains(&bit_depth_code) {
        return Err(ExifToolError::parse_error("Invalid FLIF header"));
    }

    // A character with no name is still reported, as the raw value -- that is
    // what an ExifTool PrintConv does when a value is not in its table.
    let type_name = image_type_name(image_type)
        .map(str::to_string)
        .unwrap_or_else(|| (image_type as char).to_string());
    metadata.insert("ImageType".to_string(), TagValue::String(type_name));

    metadata.insert(
        "BitDepth".to_string(),
        match bit_depth_code {
            b'1' => TagValue::Integer(8),
            b'2' => TagValue::Integer(16),
            _ => TagValue::String("Custom".to_string()),
        },
    );

    // Width and height are each stored one less than their value.
    let mut offset = 6u64;
    let (width, width_bytes) = read_varint(reader, offset)?;
    offset += width_bytes;
    let (height, height_bytes) = read_varint(reader, offset)?;
    offset += height_bytes;

    metadata.insert(
        "ImageWidth".to_string(),
        TagValue::Integer(width as i64 + 1),
    );
    metadata.insert(
        "ImageHeight".to_string(),
        TagValue::Integer(height as i64 + 1),
    );

    // Animated types carry a frame count, stored two less than its value.
    if image_type > b'H' {
        let (frames, frame_bytes) = read_varint(reader, offset)?;
        offset += frame_bytes;
        metadata.insert(
            "AnimationFrames".to_string(),
            TagValue::Integer(frames as i64 + 2),
        );
    }

    // Look for metadata chunks
    parse_flif_metadata_chunks(reader, offset, metadata)?;

    Ok(())
}

/// Parse FLIF metadata chunks (iCCP, eXif, eXmp)
///
/// Chunk content is raw-DEFLATE compressed. Reading it as a plain byte range
/// -- and its length as a 4-byte big-endian integer rather than a varint --
/// mis-framed the very first chunk, so the working ICC, EXIF and XMP parsers
/// this routes to never saw a byte of any FLIF file's metadata.
fn parse_flif_metadata_chunks(
    reader: &dyn FileReader,
    mut offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let file_size = reader.size();

    while offset < file_size {
        // A byte below 0x20 starts the image data and names the encoding, so
        // it is decided by the first byte alone -- read that before insisting
        // on a full four-byte chunk name.
        let Ok(first) = reader.read(offset, 1).map(|byte| byte[0]) else {
            break;
        };
        if first < 0x20 {
            // ExifTool emits tag 5 for whatever this byte is; only 0 has a
            // PrintConv name, and anything else prints as the raw number.
            metadata.insert(
                "Encoding".to_string(),
                match first {
                    0 => TagValue::String("FLIF16".to_string()),
                    other => TagValue::Integer(other as i64),
                },
            );
            break;
        }

        let Ok(Ok(chunk_type)) = reader.read(offset, 4).map(<[u8; 4]>::try_from) else {
            break;
        };

        offset += 4;
        let Ok((chunk_size, size_bytes)) = read_varint(reader, offset) else {
            break;
        };
        offset += size_bytes;

        let chunk_size = chunk_size as u64;
        if chunk_size > MAX_METADATA_CHUNK || offset + chunk_size > file_size {
            break;
        }

        if matches!(&chunk_type, b"iCCP" | b"eXif" | b"eXmp")
            && let Ok(compressed) = reader.read(offset, chunk_size as usize)
            && let Some(inflated) = raw_inflate(compressed)
        {
            match &chunk_type {
                b"eXif" => parse_flif_exif(&inflated, metadata),
                b"eXmp" => {
                    if let Ok(xmp_tags) = parse_xmp(&inflated) {
                        for (tag_name, value) in xmp_tags {
                            metadata.insert(tag_name, TagValue::String(value));
                        }
                    }
                }
                b"iCCP" => {
                    if let Ok(icc_tags) = crate::parsers::icc::parse_icc_profile_data(&inflated) {
                        // `parse_icc_profile_data` returns bare names; the
                        // `ICC_Profile:` family is added by whoever embeds
                        // the profile, the same way `parse_icc_file` does it
                        // for a standalone .icc.
                        for (tag_name, value) in icc_tags {
                            metadata.insert(format!("ICC_Profile:{}", tag_name), value);
                        }
                    }
                }
                _ => {}
            }
        }

        offset += chunk_size;
    }

    Ok(())
}

/// Inflate a raw DEFLATE stream (no zlib or gzip wrapper), as FLIF stores its
/// metadata chunks.
fn raw_inflate(compressed: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;

    let mut decoder = flate2::read::DeflateDecoder::new(compressed);
    let mut inflated = Vec::new();
    decoder.read_to_end(&mut inflated).ok()?;
    Some(inflated)
}

/// Parse EXIF data from an inflated FLIF eXif chunk
fn parse_flif_exif(exif_data: &[u8], metadata: &mut MetadataMap) {
    // The chunk keeps JPEG's "Exif\0\0" introducer ahead of the TIFF header.
    let tiff_data = match exif_data.strip_prefix(b"Exif\0\0") {
        Some(rest) => rest,
        None => exif_data,
    };

    if tiff_data.len() < 8 {
        return;
    }

    let byte_order = match &tiff_data[0..2] {
        b"II" => ByteOrder::LittleEndian,
        b"MM" => ByteOrder::BigEndian,
        _ => return,
    };

    let endian_order = match byte_order {
        ByteOrder::LittleEndian => EndianByteOrder::Little,
        ByteOrder::BigEndian => EndianByteOrder::Big,
    };
    let tiff_reader = EndianReader::new(tiff_data, endian_order);

    // Verify TIFF magic (0x002A)
    if tiff_reader.u16_at(2).unwrap_or(0) != 0x002A {
        return;
    }

    metadata.insert(
        "ExifByteOrder".to_string(),
        TagValue::String(
            match byte_order {
                ByteOrder::LittleEndian => "Little-endian (Intel, II)",
                ByteOrder::BigEndian => "Big-endian (Motorola, MM)",
            }
            .to_string(),
        ),
    );

    let ifd_offset = tiff_reader.u32_at(4).unwrap_or(0);
    let ifd_reader = BufferedReader::from_bytes(tiff_data);

    let Ok(ifd0_tags) = parse_ifd(&ifd_reader, ifd_offset as u64, byte_order) else {
        return;
    };

    for (tag_id, field_type, value_count, raw_bytes) in &ifd0_tags {
        let tag_value = raw_bytes_to_tag_value(raw_bytes, *field_type, *value_count, byte_order);
        metadata.insert(lookup_tag_name(*tag_id, "IFD0"), tag_value);

        // Follow the ExifIFD (0x8769) and GPS (0x8825) pointers.
        let sub_ifd = match tag_id {
            0x8769 => "ExifIFD",
            0x8825 => "GPS",
            _ => continue,
        };
        if raw_bytes.len() < 4 {
            continue;
        }
        let pointer = EndianReader::new(raw_bytes, endian_order)
            .u32_at(0)
            .unwrap_or(0);
        if let Ok(sub_tags) = parse_ifd(&ifd_reader, pointer as u64, byte_order) {
            for (sub_id, sub_type, sub_count, sub_bytes) in &sub_tags {
                let value = raw_bytes_to_tag_value(sub_bytes, *sub_type, *sub_count, byte_order);
                metadata.insert(lookup_tag_name(*sub_id, sub_ifd), value);
            }
        }
    }
}

/// Read a FLIF variable-length integer (base-128, high bit continues).
///
/// Returns (value, bytes_read).
fn read_varint(reader: &dyn FileReader, offset: u64) -> Result<(u32, u64)> {
    let mut value: u32 = 0;
    let mut consumed = 0u64;

    loop {
        // Five base-128 groups is the most a u32 can hold; more means the
        // stream is corrupt rather than merely long.
        if consumed >= 5 {
            return Err(ExifToolError::parse_error("FLIF varint too long"));
        }
        let byte = reader.read(offset + consumed, 1)?[0];
        consumed += 1;
        // Multiply rather than shift: `checked_shl` only rejects a shift
        // wider than the type, so it would discard the top seven bits of a
        // five-group varint and report success.
        value = value
            .checked_mul(128)
            .and_then(|shifted| shifted.checked_add((byte & 0x7F) as u32))
            .ok_or_else(|| ExifToolError::parse_error("FLIF varint overflow"))?;
        if byte & 0x80 == 0 {
            return Ok((value, consumed));
        }
    }
}

/// Convert raw EXIF bytes to TagValue
fn raw_bytes_to_tag_value(
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    byte_order: ByteOrder,
) -> TagValue {
    use crate::parsers::common::exif_types::ExifType;

    // Create EndianReader with appropriate byte order
    let endian_order = match byte_order {
        ByteOrder::LittleEndian => EndianByteOrder::Little,
        ByteOrder::BigEndian => EndianByteOrder::Big,
    };
    let reader = EndianReader::new(bytes, endian_order);

    if let Some(exif_type) = ExifType::from_u16(field_type) {
        match exif_type {
            ExifType::Byte if !bytes.is_empty() => {
                if value_count == 1 {
                    return TagValue::Integer(reader.u8_at(0).unwrap_or(0) as i64);
                }
                return TagValue::Binary(bytes.to_vec());
            }
            ExifType::Ascii => {
                let text = String::from_utf8_lossy(bytes);
                return TagValue::String(text.trim_end_matches('\0').to_string());
            }
            ExifType::Short if bytes.len() >= 2 => {
                if value_count == 1 {
                    let val = reader.u16_at(0).unwrap_or(0);
                    return TagValue::Integer(val as i64);
                }
            }
            ExifType::Long if bytes.len() >= 4 => {
                if value_count == 1 {
                    let val = reader.u32_at(0).unwrap_or(0);
                    return TagValue::Integer(val as i64);
                }
            }
            ExifType::Rational if bytes.len() >= 8 => {
                if value_count == 1 {
                    let numerator = reader.u32_at(0).unwrap_or(0) as i64;
                    let denominator = reader.u32_at(4).unwrap_or(0) as i64;
                    return TagValue::String(
                        crate::core::value_formatter::format_rational_as_decimal(
                            numerator,
                            denominator,
                        ),
                    );
                }
            }
            ExifType::SRational if bytes.len() >= 8 => {
                if value_count == 1 {
                    let numerator = reader.u32_at(0).unwrap_or(0) as i32 as i64;
                    let denominator = reader.u32_at(4).unwrap_or(0) as i32 as i64;
                    return TagValue::String(
                        crate::core::value_formatter::format_rational_as_decimal(
                            numerator,
                            denominator,
                        ),
                    );
                }
            }
            _ => {}
        }
    }

    TagValue::Binary(bytes.to_vec())
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
    use crate::test_support::TestReader;

    /// Encode a FLIF varint, the inverse of `read_varint`.
    fn encode_varint(mut value: u32) -> Vec<u8> {
        let mut bytes = vec![(value & 0x7F) as u8];
        value >>= 7;
        while value != 0 {
            bytes.insert(0, ((value & 0x7F) as u8) | 0x80);
            value >>= 7;
        }
        bytes
    }

    /// A minimal FLIF: 16x16 RGB, 8-bit, followed by the image-data byte.
    fn minimal_flif() -> Vec<u8> {
        let mut data = b"FLIF31".to_vec();
        data.extend_from_slice(&[0x0F, 0x0F]); // width-1, height-1
        data.push(0x00); // image data chunk (encoding FLIF16)
        data
    }

    /// The header bytes are printable characters, not a packed bitfield.
    /// Decoding '3' as a bitfield yielded an out-of-range channel count and
    /// aborted every real FLIF file before it produced a single tag.
    #[test]
    fn test_header_characters_decode_rather_than_bitfield() {
        let metadata = parse_flif_metadata(&TestReader::new(minimal_flif())).unwrap();
        assert_eq!(
            metadata.get_string("ImageType"),
            Some("RGB (non-interlaced)")
        );
        assert_eq!(metadata.get_integer("BitDepth"), Some(8));
        assert_eq!(metadata.get_integer("ImageWidth"), Some(16));
        assert_eq!(metadata.get_integer("ImageHeight"), Some(16));
        assert_eq!(metadata.get_string("Encoding"), Some("FLIF16"));
    }

    #[test]
    fn test_animated_header_reports_frame_count() {
        // 'S' is RGB animation, non-interlaced; frames are stored two less.
        let mut data = b"FLIFS1".to_vec();
        data.extend_from_slice(&[0x01, 0x01, 0x05]);
        data.push(0x00);

        let metadata = parse_flif_metadata(&TestReader::new(data)).unwrap();
        assert_eq!(
            metadata.get_string("ImageType"),
            Some("RGB Animation (non-interlaced)")
        );
        assert_eq!(metadata.get_integer("AnimationFrames"), Some(7));
    }

    /// Chunk lengths are base-128 varints, not 4-byte big-endian integers.
    #[test]
    fn test_varint_is_base128_continuation() {
        // 0x82 0x19 == (2 << 7) | 0x19 == 281, the length of a real file's
        // eXmp chunk; the old two-byte scheme decoded this as 641.
        let reader = TestReader::new(vec![0x82, 0x19]);
        assert_eq!(read_varint(&reader, 0).unwrap(), (281, 2));

        let reader = TestReader::new(vec![0x0F]);
        assert_eq!(read_varint(&reader, 0).unwrap(), (15, 1));

        for value in [0u32, 1, 127, 128, 281, 16383, 16384, 1_000_000] {
            let encoded = encode_varint(value);
            let reader = TestReader::new(encoded.clone());
            assert_eq!(
                read_varint(&reader, 0).unwrap(),
                (value, encoded.len() as u64)
            );
        }
    }

    /// Metadata chunks are raw-DEFLATE compressed, so the embedded XMP only
    /// reaches the XMP parser after inflation.
    #[test]
    fn test_compressed_xmp_chunk_reaches_the_xmp_parser() {
        use flate2::Compression;
        use flate2::write::DeflateEncoder;
        use std::io::Write;

        let xmp = br#"<?xpacket begin='' id=''?><x:xmpmeta xmlns:x='adobe:ns:meta/'>
<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>
<rdf:Description rdf:about='' xmlns:dc='http://purl.org/dc/elements/1.1/'>
<dc:creator><rdf:Seq><rdf:li>Phil Harvey</rdf:li></rdf:Seq></dc:creator>
</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end='w'?>"#;

        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(xmp).unwrap();
        let compressed = encoder.finish().unwrap();

        let mut data = b"FLIF31".to_vec();
        data.extend_from_slice(&[0x0F, 0x0F]);
        data.extend_from_slice(b"eXmp");
        data.extend_from_slice(&encode_varint(compressed.len() as u32));
        data.extend_from_slice(&compressed);
        data.push(0x00);

        let metadata = parse_flif_metadata(&TestReader::new(data)).unwrap();
        assert_eq!(metadata.get_string("XMP:Creator"), Some("Phil Harvey"));
    }

    /// A five-group varint carries 35 bits, so the accumulator has to reject
    /// the overflow rather than silently drop the top bits.
    #[test]
    fn test_varint_overflow_is_rejected() {
        let reader = TestReader::new(vec![0xFF, 0xFF, 0xFF, 0xFF, 0x7F]);
        assert!(read_varint(&reader, 0).is_err());

        // Six groups is refused on length alone.
        let reader = TestReader::new(vec![0x81, 0x80, 0x80, 0x80, 0x80, 0x00]);
        assert!(read_varint(&reader, 0).is_err());
    }

    /// Compress `payload` the way FLIF stores a metadata chunk and wrap it in
    /// a minimal 16x16 RGB file.
    fn flif_with_chunk(name: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        use flate2::Compression;
        use flate2::write::DeflateEncoder;
        use std::io::Write;

        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(payload).unwrap();
        let compressed = encoder.finish().unwrap();

        let mut data = b"FLIF31".to_vec();
        data.extend_from_slice(&[0x0F, 0x0F]);
        data.extend_from_slice(name);
        data.extend_from_slice(&encode_varint(compressed.len() as u32));
        data.extend_from_slice(&compressed);
        data.push(0x00);
        data
    }

    /// A big-endian TIFF whose IFD0 points at both an ExifIFD and a GPS IFD,
    /// prefixed with the "Exif\0\0" introducer the eXif chunk carries.
    fn exif_chunk_with_sub_ifds() -> Vec<u8> {
        fn entry(tag: u16, field_type: u16, count: u32, value: [u8; 4]) -> Vec<u8> {
            let mut bytes = tag.to_be_bytes().to_vec();
            bytes.extend_from_slice(&field_type.to_be_bytes());
            bytes.extend_from_slice(&count.to_be_bytes());
            bytes.extend_from_slice(&value);
            bytes
        }

        // IFD0 holds three entries, so it spans 8..50; ExifIFD and the GPS
        // IFD hold one each, at 50 and 68.
        const EXIF_IFD_OFFSET: u32 = 50;
        const GPS_IFD_OFFSET: u32 = 68;

        let mut tiff = b"MM\x00\x2a".to_vec();
        tiff.extend_from_slice(&8u32.to_be_bytes());

        tiff.extend_from_slice(&3u16.to_be_bytes());
        tiff.extend(entry(0x0112, 3, 1, [0x00, 0x01, 0x00, 0x00])); // Orientation
        tiff.extend(entry(0x8769, 4, 1, EXIF_IFD_OFFSET.to_be_bytes()));
        tiff.extend(entry(0x8825, 4, 1, GPS_IFD_OFFSET.to_be_bytes()));
        tiff.extend_from_slice(&0u32.to_be_bytes());
        assert_eq!(tiff.len(), EXIF_IFD_OFFSET as usize);

        tiff.extend_from_slice(&1u16.to_be_bytes());
        tiff.extend(entry(0xA002, 4, 1, 640u32.to_be_bytes())); // ExifImageWidth
        tiff.extend_from_slice(&0u32.to_be_bytes());
        assert_eq!(tiff.len(), GPS_IFD_OFFSET as usize);

        tiff.extend_from_slice(&1u16.to_be_bytes());
        tiff.extend(entry(0x0001, 2, 2, *b"N\0\0\0")); // GPSLatitudeRef
        tiff.extend_from_slice(&0u32.to_be_bytes());

        let mut chunk = b"Exif\0\0".to_vec();
        chunk.extend_from_slice(&tiff);
        chunk
    }

    /// IFD0's ExifIFD and GPS pointers have to be followed; stopping at IFD0
    /// drops every exposure and location tag a FLIF can carry.
    #[test]
    fn test_exif_chunk_sub_ifd_pointers_are_followed() {
        let data = flif_with_chunk(b"eXif", &exif_chunk_with_sub_ifds());
        let metadata = parse_flif_metadata(&TestReader::new(data)).unwrap();

        assert_eq!(
            metadata.get_string("ExifByteOrder"),
            Some("Big-endian (Motorola, MM)")
        );
        assert_eq!(metadata.get_integer("IFD0:Orientation"), Some(1));
        assert_eq!(metadata.get_integer("ExifIFD:ExifImageWidth"), Some(640));
        assert_eq!(metadata.get_string("GPS:GPSLatitudeRef"), Some("N"));
    }

    #[test]
    fn test_rejects_non_flif_header_bytes() {
        let mut data = b"FLIF".to_vec();
        data.extend_from_slice(&[0xFF, 0xFF, 0x00, 0x00]);
        assert!(parse_flif_metadata(&TestReader::new(data)).is_err());
    }
}
