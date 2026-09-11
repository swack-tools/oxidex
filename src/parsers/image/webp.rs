//! WebP image format parser
//!
//! WebP uses RIFF container with chunks:
//! - VP8/VP8L/VP8X: Image data
//! - EXIF: TIFF/EXIF metadata
//! - ICCP: ICC color profile
//! - XMP: XMP metadata

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use crate::parsers::image::embedded::parse_embedded_exif;
use crate::parsers::xmp::rdf_parser::parse_xmp;

/// WebP signature: "RIFF" + size + "WEBP"
const RIFF_SIGNATURE: &[u8] = b"RIFF";
const WEBP_SIGNATURE: &[u8] = b"WEBP";

/// WebP parser
pub struct WebPParser;

impl WebPParser {
    /// Verifies WebP signature
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 12 {
            return Ok(false);
        }
        let header = reader.read(0, 12)?;
        Ok(&header[0..4] == RIFF_SIGNATURE && &header[8..12] == WEBP_SIGNATURE)
    }
}

impl FormatParser for WebPParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid WebP signature"));
        }

        let mut metadata = MetadataMap::new();

        metadata.insert("FileType".to_string(), TagValue::String("WebP".to_string()));

        // Parse RIFF chunks to find EXIF, XMP, and VP8X data
        parse_webp_chunks(reader, &mut metadata)?;

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::WebP)
    }
}

/// Parses metadata from WebP files.
///
/// This is a convenience wrapper around WebPParser that provides a functional API.
pub fn parse_webp_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = WebPParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

/// Parse RIFF chunks in WebP file
fn parse_webp_chunks(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
    let file_size = reader.size();

    // Skip RIFF header (12 bytes: "RIFF" + size + "WEBP")
    let mut offset = 12u64;

    while offset + 8 <= file_size {
        // Read chunk header: FourCC (4 bytes) + size (4 bytes, little-endian)
        let chunk_header = reader.read(offset, 8)?;
        let chunk_type = &chunk_header[0..4];
        // WebP/RIFF uses little-endian byte order
        let header_reader = EndianReader::little_endian(chunk_header);
        let chunk_size = header_reader.u32_at(4).unwrap_or(0) as u64;

        // Move past header
        let chunk_data_offset = offset + 8;

        match chunk_type {
            b"VP8X" => {
                // A VP8X chunk is what makes a WebP an *extended* WebP, and
                // ExifTool renames the file on seeing one (RIFF.pm:2106):
                //
                //     $et->OverrideFileType('Extended WEBP',undef,'webp')
                //         if $tag eq 'VP8X' and $type eq 'WEBP';
                //
                // The rename keys on the chunk's presence, not on its
                // contents, so it happens here rather than inside the size
                // check below. Into the `File` group to outrank
                // `%fileTypeLookup`, which answers plain `WEBP` for the
                // extension. Extension and MIME are unchanged -- ExifTool
                // passes `undef` and `'webp'`.
                metadata.insert(
                    "File:FileType".to_string(),
                    TagValue::String("Extended WEBP".to_string()),
                );

                // Extended WebP header with flags
                if chunk_size >= 10 {
                    let vp8x_data = reader.read(chunk_data_offset, 10)?;
                    let vp8x_reader = EndianReader::little_endian(vp8x_data);
                    let flags = vp8x_reader.u8_at(0).unwrap_or(0);

                    // Extract image dimensions (24-bit values, little-endian)
                    let width = (vp8x_reader.u32_at(4).unwrap_or(0) & 0x00FFFFFF) + 1;
                    let height = (vp8x_reader.u32_at(7).unwrap_or(0) & 0x00FFFFFF) + 1;

                    metadata.insert(
                        "WebP:ImageWidth".to_string(),
                        TagValue::Integer(width as i64),
                    );
                    metadata.insert(
                        "WebP:ImageHeight".to_string(),
                        TagValue::Integer(height as i64),
                    );

                    // Build WebP_Flags string like ExifTool
                    // VP8X flags (from ExifTool RIFF.pm):
                    //   Bit 1 (0x02) = Animation
                    //   Bit 2 (0x04) = XMP
                    //   Bit 3 (0x08) = EXIF
                    //   Bit 4 (0x10) = Alpha
                    //   Bit 5 (0x20) = ICC Profile
                    // ExifTool outputs in order: XMP, EXIF, Alpha, Animation
                    // (ICC is not included in WebP_Flags, it's a separate Has tag)
                    let mut flag_parts = Vec::new();
                    if flags & 0x04 != 0 {
                        flag_parts.push("XMP");
                    }
                    if flags & 0x08 != 0 {
                        flag_parts.push("EXIF");
                    }
                    if flags & 0x10 != 0 {
                        flag_parts.push("Alpha");
                    }
                    if flags & 0x02 != 0 {
                        flag_parts.push("Animation");
                    }
                    if !flag_parts.is_empty() {
                        metadata.insert(
                            "WebP:WebP_Flags".to_string(),
                            TagValue::String(flag_parts.join(", ")),
                        );
                    }

                    // Keep individual flags for compatibility
                    // Note: Bit 5 (0x20) = ICC, Bit 4 (0x10) = Alpha
                    if flags & 0x20 != 0 {
                        metadata.insert(
                            "WebP:HasICCP".to_string(),
                            TagValue::String("Yes".to_string()),
                        );
                    }
                    if flags & 0x10 != 0 {
                        metadata.insert(
                            "WebP:HasAlpha".to_string(),
                            TagValue::String("Yes".to_string()),
                        );
                    }
                    if flags & 0x08 != 0 {
                        metadata.insert(
                            "WebP:HasEXIF".to_string(),
                            TagValue::String("Yes".to_string()),
                        );
                    }
                    if flags & 0x04 != 0 {
                        metadata.insert(
                            "WebP:HasXMP".to_string(),
                            TagValue::String("Yes".to_string()),
                        );
                    }
                    if flags & 0x02 != 0 {
                        metadata.insert(
                            "WebP:IsAnimation".to_string(),
                            TagValue::String("Yes".to_string()),
                        );
                    }
                }
            }
            b"VP8 " => {
                // Lossy VP8 bitstream - extract dimensions from frame header
                if chunk_size >= 10 {
                    let vp8_data = reader.read(chunk_data_offset, 10)?;
                    let vp8_reader = EndianReader::little_endian(vp8_data);
                    // VP8 frame header starts with 3-byte frame tag
                    let frame_tag = vp8_reader.u8_at(0).unwrap_or(1);
                    // Check if this is a keyframe
                    if frame_tag & 0x01 == 0 {
                        // Extract VP8 version (bits 1-3 of frame tag)
                        let version = (frame_tag >> 1) & 0x07;
                        let version_str = match version {
                            0 => "0 (bicubic reconstruction, normal loop)",
                            1 => "1 (bilinear reconstruction, simple loop)",
                            2 => "2 (bilinear reconstruction, no loop)",
                            3 => "3 (no reconstruction, no loop)",
                            _ => "Unknown",
                        };
                        metadata.insert(
                            "WebP:VP8Version".to_string(),
                            TagValue::String(version_str.to_string()),
                        );

                        // Keyframe - dimensions at bytes 6-9
                        let width_data = vp8_reader.u16_at(6).unwrap_or(0);
                        let height_data = vp8_reader.u16_at(8).unwrap_or(0);
                        let width = width_data & 0x3FFF;
                        let height = height_data & 0x3FFF;

                        // Extract scale factors (upper 2 bits)
                        let horizontal_scale = (width_data >> 14) & 0x03;
                        let vertical_scale = (height_data >> 14) & 0x03;

                        metadata.insert(
                            "WebP:HorizontalScale".to_string(),
                            TagValue::Integer(horizontal_scale as i64),
                        );
                        metadata.insert(
                            "WebP:VerticalScale".to_string(),
                            TagValue::Integer(vertical_scale as i64),
                        );

                        if !metadata.contains_key("WebP:ImageWidth") {
                            metadata.insert(
                                "WebP:ImageWidth".to_string(),
                                TagValue::Integer(width as i64),
                            );
                            metadata.insert(
                                "WebP:ImageHeight".to_string(),
                                TagValue::Integer(height as i64),
                            );
                        }
                    }
                }
            }
            b"VP8L" => {
                // Lossless VP8L bitstream
                if chunk_size >= 5 {
                    let vp8l_data = reader.read(chunk_data_offset, 5)?;
                    let vp8l_reader = EndianReader::little_endian(vp8l_data);
                    // Check signature byte (0x2F)
                    if vp8l_reader.u8_at(0).unwrap_or(0) == 0x2F {
                        // Dimensions are packed in bytes 1-4
                        let bits = vp8l_reader.u32_at(1).unwrap_or(0);
                        let width = (bits & 0x3FFF) + 1;
                        let height = ((bits >> 14) & 0x3FFF) + 1;

                        if !metadata.contains_key("WebP:ImageWidth") {
                            metadata.insert(
                                "WebP:ImageWidth".to_string(),
                                TagValue::Integer(width as i64),
                            );
                            metadata.insert(
                                "WebP:ImageHeight".to_string(),
                                TagValue::Integer(height as i64),
                            );
                        }
                    }
                }
            }
            b"EXIF" => {
                // EXIF metadata - contains TIFF/EXIF data
                if chunk_size > 0 && chunk_data_offset + chunk_size <= file_size {
                    let exif_data = reader.read(chunk_data_offset, chunk_size as usize)?;
                    if parse_webp_exif(exif_data, chunk_data_offset, metadata).is_err() {
                        // Silently ignore EXIF parsing errors
                    }
                }
            }
            b"XMP " => {
                // XMP metadata - XML format
                if chunk_size > 0 && chunk_data_offset + chunk_size <= file_size {
                    let xmp_data = reader.read(chunk_data_offset, chunk_size as usize)?;

                    // Parse the XMP and extract metadata
                    if let Ok(xmp_props) = parse_xmp(xmp_data) {
                        for (name, value) in xmp_props {
                            metadata.insert(name, TagValue::String(value));
                        }
                    }
                }
            }
            b"ICCP" => {
                // ICC color profile
                metadata.insert(
                    "WebP:ICCProfileSize".to_string(),
                    TagValue::Integer(chunk_size as i64),
                );
            }
            b"ALPH" => {
                // Alpha chunk - contains alpha compression info
                // WebP spec byte layout: |Rsv|P|F|C| where:
                //   C (Compression): bits 1-0
                //   F (Filtering): bits 3-2
                //   P (Preprocessing): bits 5-4
                //   Rsv (Reserved): bits 7-6
                //
                // Note: ExifTool's RIFF.pm has a bug - all three tags use Mask 0x03
                // without BitShift, so they all extract the same bits 0-1 from the byte.
                // We match ExifTool's buggy behavior for compatibility.
                if chunk_size >= 1 {
                    let alph_data = reader.read(chunk_data_offset, 1)?;
                    let flags = alph_data[0];

                    // All three use bits 0-1 to match ExifTool's bug
                    let value = flags & 0x03;

                    let preprocessing_str = match value {
                        0 => "none",
                        1 => "Level Reduction",
                        _ => "Unknown",
                    };
                    metadata.insert(
                        "RIFF:AlphaPreprocessing".to_string(),
                        TagValue::String(preprocessing_str.to_string()),
                    );

                    let filtering_str = match value {
                        0 => "none",
                        1 => "Horizontal",
                        2 => "Vertical",
                        3 => "Gradient",
                        _ => "Unknown",
                    };
                    metadata.insert(
                        "RIFF:AlphaFiltering".to_string(),
                        TagValue::String(filtering_str.to_string()),
                    );

                    let compression_str = match value {
                        0 => "none",
                        1 => "Lossless",
                        _ => "Unknown",
                    };
                    metadata.insert(
                        "RIFF:AlphaCompression".to_string(),
                        TagValue::String(compression_str.to_string()),
                    );
                }
            }
            b"ANIM" => {
                // Animation control chunk
                if chunk_size >= 6 {
                    let anim_data = reader.read(chunk_data_offset, 6)?;
                    let anim_reader = EndianReader::little_endian(anim_data);

                    // Background color (4 bytes ARGB)
                    let bg_color = anim_reader.u32_at(0).unwrap_or(0);

                    // Loop count (2 bytes) - 0 means infinite
                    let loop_count = anim_reader.u16_at(4).unwrap_or(0);

                    metadata.insert(
                        "WebP:AnimationBackgroundColor".to_string(),
                        TagValue::String(format!("0x{:08X}", bg_color)),
                    );

                    if loop_count == 0 {
                        metadata.insert(
                            "WebP:AnimationLoopCount".to_string(),
                            TagValue::String("Infinite".to_string()),
                        );
                    } else {
                        metadata.insert(
                            "WebP:AnimationLoopCount".to_string(),
                            TagValue::Integer(loop_count as i64),
                        );
                    }
                }
            }
            b"ANMF" => {
                // Animation frame chunk - just count them
                if !metadata.contains_key("WebP:AnimationFrameCount") {
                    metadata.insert("WebP:AnimationFrameCount".to_string(), TagValue::Integer(1));
                } else if let Some(TagValue::Integer(count)) =
                    metadata.get("WebP:AnimationFrameCount")
                {
                    metadata.insert(
                        "WebP:AnimationFrameCount".to_string(),
                        TagValue::Integer(count + 1),
                    );
                }
            }
            _ => {
                // Skip unknown chunks
            }
        }

        // Move to next chunk (chunks are padded to even byte boundary)
        offset = chunk_data_offset + chunk_size;
        if !chunk_size.is_multiple_of(2) {
            offset += 1; // Padding byte
        }
    }

    Ok(())
}

/// Parses the WebP `EXIF` chunk.
///
/// RIFF.pm 13.59:557-577 declares the chunk as a SubDirectory of
/// Image::ExifTool::Exif::Main processed by ProcessTIFF, in two variants: the
/// TIFF header first (:559-564), or JPEG's `Exif\0\0` introducer ahead of it
/// (:566-572, `Start => 6`, with a minor "Improper EXIF header" warning). The
/// block is then exactly what a JPEG APP1 carries, so it goes through the
/// shared decoder rather than a WebP copy of it.
///
/// `chunk_data_offset` is the chunk payload's file position, which is the
/// `Base` ExifTool adds to the offsets it reports from the block
/// (`Pentax:PreviewImageStart`, `InteropIFD:OtherImageStart`) -- for both
/// variants: `Start => 6` moves the directory start, not the base, so the
/// introducer's six bytes are not part of the printed number (oracle: 2476
/// either way for the same block, chunk data at 60).
fn parse_webp_exif(
    exif_data: &[u8],
    chunk_data_offset: u64,
    metadata: &mut MetadataMap,
) -> Result<()> {
    if exif_data.len() < 8 {
        return Err(ExifToolError::parse_error("EXIF data too short"));
    }

    let tiff_data = match exif_data.strip_prefix(b"Exif\0\0") {
        Some(rest) => rest,
        None => exif_data,
    };

    if parse_embedded_exif(tiff_data, chunk_data_offset, metadata) {
        Ok(())
    } else {
        Err(ExifToolError::parse_error(
            "invalid TIFF header in EXIF chunk",
        ))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// A RIFF/WEBP container holding one chunk.
    fn webp(chunk_type: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(WEBP_SIGNATURE);
        body.extend_from_slice(chunk_type);
        body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        body.extend_from_slice(payload);
        if body.len() % 2 == 1 {
            body.push(0);
        }
        let mut data = Vec::from(RIFF_SIGNATURE);
        data.extend_from_slice(&(body.len() as u32).to_le_bytes());
        data.extend_from_slice(&body);
        data
    }

    #[test]
    fn a_vp8x_chunk_makes_it_an_extended_webp() {
        // 10 bytes: flags, 3 reserved, 24-bit width-1, 24-bit height-1.
        let payload = [0x10, 0, 0, 0, 0x3F, 0, 0, 0x2F, 0, 0];
        let metadata = WebPParser
            .parse(&TestReader::new(webp(b"VP8X", &payload)))
            .unwrap();
        assert_eq!(metadata.get_string("File:FileType"), Some("Extended WEBP"));
    }

    #[test]
    fn a_short_vp8x_still_renames_the_file() {
        // ExifTool's rename keys on the chunk tag, not on its contents, so a
        // VP8X too short to read dimensions from still makes the file
        // extended.
        let metadata = WebPParser
            .parse(&TestReader::new(webp(b"VP8X", &[0x10, 0])))
            .unwrap();
        assert_eq!(metadata.get_string("File:FileType"), Some("Extended WEBP"));
    }

    #[test]
    fn a_simple_webp_keeps_the_plain_name() {
        let metadata = WebPParser
            .parse(&TestReader::new(webp(b"VP8 ", &[0u8; 16])))
            .unwrap();
        // No `File:FileType` -- the identification layer's `WEBP` stands.
        assert_eq!(metadata.get_string("File:FileType"), None);
    }

    #[test]
    fn exif_chunk_preserves_scalar_tiff_types() {
        use crate::parsers::image::embedded::test_fixtures::tiff_with_entries;
        let tiff = tiff_with_entries(&[
            (0x0100, 1, &[65]),
            (0x0108, 6, &[255]),
            (0x010f, 7, b"Canon\0"),
            (0x010d, 7, b"Plan Scan \0"),
        ]);
        for prefix in [false, true] {
            let payload = if prefix {
                [b"Exif\0\0".as_slice(), &tiff].concat()
            } else {
                tiff.clone()
            };
            let file = webp(b"EXIF", &payload);
            let metadata = WebPParser
                .parse(&TestReader::new(file))
                .expect("WebP parses");
            assert_eq!(metadata.get_integer("IFD0:ImageWidth"), Some(65));
            assert_eq!(metadata.get_integer("IFD0:CellWidth"), Some(-1));
            assert_eq!(metadata.get_string("IFD0:Make"), Some("Canon\0"));
            assert_eq!(
                metadata.get_string("IFD0:DocumentName"),
                Some("Plan Scan \0")
            );
        }
    }

    /// Exif.pm 13.59:3849-3873 through the EXIF chunk, in both RIFF.pm
    /// variants (bare TIFF header, and `Exif\0\0` ahead of it): an all-NUL
    /// PanasonicTitle creates no tag, a NUL-padded PanasonicTitle2 prints
    /// exactly, and Make loses its trailing blank (Exif.pm:585) -- the rows
    /// the same TIFF block prints inside a JPEG.
    #[test]
    fn exif_chunk_panasonic_title_rawconv_matches_jpeg_path() {
        use crate::parsers::image::embedded::test_fixtures::{
            assert_block_a, assert_block_b, panasonic_title_block_a, panasonic_title_block_b,
        };

        let block = panasonic_title_block_a();
        let metadata = WebPParser
            .parse(&TestReader::new(webp(b"EXIF", &block)))
            .unwrap();
        assert_block_a(&metadata);

        let with_header = [b"Exif\0\0".as_slice(), &block].concat();
        let metadata = WebPParser
            .parse(&TestReader::new(webp(b"EXIF", &with_header)))
            .unwrap();
        assert_block_a(&metadata);

        let metadata = WebPParser
            .parse(&TestReader::new(webp(b"EXIF", &panasonic_title_block_b())))
            .unwrap();
        assert_block_b(&metadata);
    }

    /// RIFF.pm 13.59:557-577 / ExifTool.pm DoProcessTIFF: the `Base` added
    /// to the offsets reported from the EXIF chunk is the chunk data's file
    /// position (20 here: RIFF header 12 + chunk header 8), and `Start => 6`
    /// for the `Exif\0\0` variant moves the directory start only, so the
    /// oracle prints the same 1020 for both layouts (not 1026).
    #[test]
    fn exif_chunk_offsets_are_reported_from_the_chunk_data_position() {
        use crate::parsers::image::embedded::test_fixtures::{
            assert_other_image_start, interop_offset_block,
        };

        let block = interop_offset_block();
        let metadata = WebPParser
            .parse(&TestReader::new(webp(b"EXIF", &block)))
            .unwrap();
        assert_other_image_start(&metadata, 20);

        let with_header = [b"Exif\0\0".as_slice(), &block].concat();
        let metadata = WebPParser
            .parse(&TestReader::new(webp(b"EXIF", &with_header)))
            .unwrap();
        assert_other_image_start(&metadata, 20);
    }

    /// Pinned 13.59 on these complete WebP carriers: the same EF display
    /// label denotes distinct raw IDs; RF identity belongs to its own table.
    #[test]
    fn exif_chunk_canon_identity_survives_into_lens_id() {
        use crate::parsers::image::embedded::test_fixtures::canon_lens_tiff;

        const EF: &str = "Canon EF 300mm f/2.8L USM";
        const TAMRON: &str = "Tamron SP 15-30mm f/2.8 Di VC USD (A012)";
        const RF50: &str = "Canon RF 50mm F1.2L USM";
        const RFS: &str = "Canon RF-S 14-30mm F4-6.3 IS STM PZ";
        for (lens, rf, rf_label, expected) in [
            (129, None, None, EF),
            (136, None, None, TAMRON),
            (129, Some(257), Some(RF50), RF50),
            (136, Some(324), Some(RFS), RFS),
            (136, Some(0), Some("n/a"), TAMRON),
        ] {
            let block = canon_lens_tiff(lens, rf);
            for prefix in [b"".as_slice(), b"Exif\0\0".as_slice()] {
                let payload = [prefix, &block].concat();
                let mut map = WebPParser
                    .parse(&TestReader::new(webp(b"EXIF", &payload)))
                    .unwrap();
                assert_eq!(map.get_string("Canon:LensType"), Some(EF));
                let raw = lens.to_string();
                assert_eq!(map.value_form("Canon:LensType"), Some(raw.as_str()));
                assert_eq!(map.occurrences_for("Canon:LensType").len(), 1);
                assert_eq!(map.get_string("Canon:RFLensType"), rf_label);
                let raw_rf = rf.map(|n| n.to_string());
                assert_eq!(map.value_form("Canon:RFLensType"), raw_rf.as_deref());
                crate::composite::apply(&mut map);
                assert_eq!(
                    map.get_string("Composite:LensID"),
                    Some(expected),
                    "EF {lens}, RF {rf:?}, introducer bytes {}",
                    prefix.len()
                );
                assert!(!map.keys().any(|key| key.contains("RawLens")));
            }
        }
    }

    /// Seed a distinct occurrence, then traverse the real container chunk
    /// loop. A later losing raw ID must not mutate the winner; an incoming
    /// winner must retain its own value rather than the earlier occurrence's.
    #[test]
    fn exif_chunk_canon_identity_stays_with_its_occurrence() {
        use crate::core::Instance;
        use crate::parsers::image::embedded::test_fixtures::canon_lens_tiff;

        const EF: &str = "Canon EF 300mm f/2.8L USM";
        const TAMRON: &str = "Tamron SP 15-30mm f/2.8 Di VC USD (A012)";
        const RF50: &str = "Canon RF 50mm F1.2L USM";
        const RFS: &str = "Canon RF-S 14-30mm F4-6.3 IS STM PZ";
        for (key, seed_raw, seed_label, seed_priority, rf, incoming_raw, expected_raw, expected) in [
            ("Canon:LensType", "129", EF, 2, None, "136", "129", EF),
            ("Canon:LensType", "129", EF, 0, None, "136", "136", TAMRON),
            (
                "Canon:RFLensType",
                "257",
                RF50,
                2,
                Some(324),
                "324",
                "257",
                RF50,
            ),
            (
                "Canon:RFLensType",
                "257",
                RF50,
                0,
                Some(324),
                "324",
                "324",
                RFS,
            ),
        ] {
            let mut map = MetadataMap::new();
            map.insert_occurrence_with_raw(
                key,
                TagValue::new_string(seed_label),
                TagValue::new_string(seed_raw),
                seed_priority,
                "Canon",
                Instance(7),
            );
            let block = canon_lens_tiff(136, rf);
            parse_webp_chunks(&TestReader::new(webp(b"EXIF", &block)), &mut map).unwrap();
            let occurrences = map.occurrences_for(key);
            assert_eq!(occurrences.len(), 2);
            assert_eq!(
                occurrences[0].value.as_ref().and_then(TagValue::as_string),
                Some(seed_raw)
            );
            assert_eq!(occurrences[0].raw.as_string(), Some(seed_label));
            assert_eq!(occurrences[0].priority, seed_priority);
            assert_eq!(occurrences[0].group1.as_ref(), "Canon");
            assert_eq!(occurrences[0].instance, Instance(7));
            assert_eq!(
                occurrences[1].value.as_ref().and_then(TagValue::as_string),
                Some(incoming_raw)
            );
            assert_eq!(occurrences[1].priority, 1);
            assert_eq!(occurrences[1].group1.as_ref(), "");
            assert_eq!(occurrences[1].instance, Instance::default());
            assert!(occurrences[0].order < occurrences[1].order);
            assert_eq!(map.value_form(key), Some(expected_raw));
            let winner = map
                .winner_occurrences()
                .find(|(k, _)| k.as_str() == key)
                .unwrap()
                .1;
            let expected_instance = if seed_priority > 1 {
                Instance(7)
            } else {
                Instance::default()
            };
            assert_eq!(winner.instance, expected_instance);
            assert_eq!(
                winner.value.as_ref().and_then(TagValue::as_string),
                Some(expected_raw)
            );
            crate::composite::apply(&mut map);
            assert_eq!(map.get_string("Composite:LensID"), Some(expected));
        }
    }

    #[test]
    fn a_non_webp_riff_is_rejected() {
        let mut data = Vec::from(RIFF_SIGNATURE);
        data.extend_from_slice(&16u32.to_le_bytes());
        data.extend_from_slice(b"AVI LIST\0\0\0\0");
        assert!(WebPParser.parse(&TestReader::new(data)).is_err());
    }
}
