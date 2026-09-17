//! JPEG XL (JXL) image format parser
//!
//! JPEG XL supports two formats:
//! - Bare codestream: starts with 0xFF 0x0A
//! - Container format: ISOBMFF-based boxes starting with "JXL " signature
//!
//! Container boxes include: jxlc (codestream), jxlp (partial), Exif, xml (XMP)

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use crate::parsers::image::embedded::parse_embedded_exif_at;
use crate::parsers::xmp::rdf_parser::parse_xmp;

/// Bare codestream signature: 0xFF 0x0A
const JXL_CODESTREAM_SIGNATURE: &[u8] = &[0xFF, 0x0A];
/// Container signature: size (4) + "JXL " (4) + ftyp header
const JXL_CONTAINER_SIGNATURE: &[u8] = b"JXL ";

/// Reads bits LSB-first across a byte slice, per the JPEG XL bitstream spec.
struct JxlBitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8,
}

impl<'a> JxlBitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    fn get_bits(&mut self, n: u32) -> Option<u32> {
        let mut value: u32 = 0;
        for i in 0..n {
            let byte = *self.data.get(self.byte_pos)?;
            let bit = (byte >> self.bit_pos) & 1;
            value |= (bit as u32) << i;
            self.bit_pos += 1;
            if self.bit_pos == 8 {
                self.bit_pos = 0;
                self.byte_pos += 1;
            }
        }
        Some(value)
    }
}

/// Parser for JPEG XL (JXL) next-generation image files
///
/// Extracts metadata from JPEG XL format images including dimensions, bit depth,
/// color information, and embedded EXIF/XMP data.
pub struct JXLParser;

impl JXLParser {
    /// Verifies the JPEG XL file signature (supports both bare codestream and container formats)
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 2 {
            return Ok(false);
        }
        let header = reader.read(0, 2)?;
        if header == JXL_CODESTREAM_SIGNATURE {
            return Ok(true);
        }
        if reader.size() >= 12 {
            let header_long = reader.read(0, 12)?;
            // Container format: first 4 bytes are size, next 4 are "JXL "
            if &header_long[4..8] == JXL_CONTAINER_SIGNATURE {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Checks if file is container format (ISOBMFF-based)
    fn is_container_format(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 12 {
            return Ok(false);
        }
        let header = reader.read(0, 12)?;
        Ok(&header[4..8] == JXL_CONTAINER_SIGNATURE)
    }

    /// Parse bare codestream header for dimensions
    ///
    /// Mirrors ExifTool's `ProcessJXLCodestream` (lib/Image/ExifTool/Jpeg2000.pm):
    /// the SizeHeader is a bitstream read LSB-first, byte by byte, per the
    /// JPEG XL spec (ISO/IEC 18181-1).
    fn parse_codestream_header(
        data: &[u8],
        metadata: &mut MetadataMap,
    ) -> std::result::Result<(), String> {
        if data.len() < 2 || data[0] != 0xFF || data[1] != 0x0A {
            return Err("Invalid codestream header".to_string());
        }

        let mut bits = JxlBitReader::new(&data[2..]);

        let dims = (|| -> Option<(u32, u32)> {
            let small = bits.get_bits(1)? != 0;
            let y = if small {
                (bits.get_bits(5)? + 1) * 8
            } else {
                let sel = bits.get_bits(2)?;
                let nbits = [9u32, 13, 18, 30][sel as usize];
                bits.get_bits(nbits)? + 1
            };
            let ratio = bits.get_bits(3)?;
            let x = if ratio == 0 {
                if small {
                    (bits.get_bits(5)? + 1) * 8
                } else {
                    let sel = bits.get_bits(2)?;
                    let nbits = [9u32, 13, 18, 30][sel as usize];
                    bits.get_bits(nbits)? + 1
                }
            } else {
                const RATIOS: [(u32, u32); 7] =
                    [(1, 1), (12, 10), (4, 3), (3, 2), (16, 9), (5, 4), (2, 1)];
                let (rn, rd) = RATIOS[(ratio - 1) as usize];
                (y * rn) / rd
            };
            Some((x, y))
        })();

        if let Some((width, height)) = dims {
            metadata.insert(
                "File:ImageWidth".to_string(),
                TagValue::Integer(width as i64),
            );
            metadata.insert(
                "File:ImageHeight".to_string(),
                TagValue::Integer(height as i64),
            );
        }

        Ok(())
    }

    /// Decode ISOBMFF brand code to human-readable name
    fn decode_brand(brand: &[u8]) -> String {
        match brand {
            b"jxl " => "JPEG XL Image (.JXL)".to_string(),
            b"avif" => "AV1 Image File Format".to_string(),
            b"heic" => "HEIC Image".to_string(),
            b"mif1" => "HEIF Image".to_string(),
            b"msf1" => "HEIF Image Sequence".to_string(),
            b"mp41" => "MP4 v1".to_string(),
            b"mp42" => "MP4 v2".to_string(),
            b"isom" => "ISO Base Media".to_string(),
            b"jp2 " => "JPEG 2000 Image (.JP2)".to_string(),
            _ => {
                // Return the 4-char code as string
                String::from_utf8_lossy(brand).trim().to_string()
            }
        }
    }

    /// Parse ISOBMFF container format boxes
    fn parse_container_boxes(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
        let file_size = reader.size() as usize;
        let mut offset = 0usize;

        while offset + 8 <= file_size {
            let header = reader.read(offset as u64, 8)?;
            // ISOBMFF uses big-endian byte order
            let header_reader = EndianReader::big_endian(header);
            let box_size = header_reader.u32_at(0).unwrap_or(0) as usize;
            let box_type = std::str::from_utf8(&header[4..8]).unwrap_or("????");

            if box_size == 0 {
                break; // Box extends to end of file
            }
            if box_size < 8 || offset + box_size > file_size {
                break;
            }

            match box_type {
                "ftyp" => {
                    // File Type box - contains brand information
                    // Format: major_brand (4) + minor_version (4) + compatible_brands (4 each)
                    if box_size >= 16 {
                        let ftyp_data = reader.read((offset + 8) as u64, box_size - 8)?;
                        let ftyp_reader = EndianReader::big_endian(&*ftyp_data);

                        // Major brand (4 bytes)
                        if ftyp_data.len() >= 4 {
                            let major_brand = Self::decode_brand(&ftyp_data[0..4]);
                            metadata.insert(
                                "Jpeg2000:MajorBrand".to_string(),
                                TagValue::new_string(major_brand),
                            );
                        }

                        // Minor version (4 bytes as version number)
                        if ftyp_data.len() >= 8 {
                            let minor = ftyp_reader.u32_at(4).unwrap_or(0);
                            // Format as X.X.X (major.minor.patch from 32-bit value)
                            let major_ver = (minor >> 24) & 0xFF;
                            let minor_ver = (minor >> 16) & 0xFF;
                            let patch_ver = minor & 0xFFFF;
                            metadata.insert(
                                "Jpeg2000:MinorVersion".to_string(),
                                TagValue::new_string(format!(
                                    "{}.{}.{}",
                                    major_ver, minor_ver, patch_ver
                                )),
                            );
                        }

                        // Compatible brands (remaining 4-byte chunks).
                        //
                        // Model: Jpeg2000.pm:574-579 / QuickTime.pm:1045-1050
                        // `CompatibleBrands`' ValueConv --
                        // `my @a=($val=~/.{4}/sg); @a=grep(!/\0/,@a); \@a` --
                        // splits the remainder of the ftyp box into 4-byte
                        // chunks, drops any chunk containing a null byte, and
                        // returns the rest as a genuine list (`List => 1`),
                        // not a stringified one.
                        if ftyp_data.len() > 8 {
                            let mut brands: Vec<TagValue> = Vec::new();
                            let mut brand_offset = 8;
                            while brand_offset + 4 <= ftyp_data.len() {
                                let brand = &ftyp_data[brand_offset..brand_offset + 4];
                                if !brand.contains(&0u8) {
                                    brands.push(TagValue::new_string(
                                        String::from_utf8_lossy(brand).to_string(),
                                    ));
                                }
                                brand_offset += 4;
                            }
                            if !brands.is_empty() {
                                metadata.insert(
                                    "Jpeg2000:CompatibleBrands".to_string(),
                                    TagValue::Array(brands),
                                );
                            }
                        }
                    }
                }
                "jxlc" | "jxlp" => {
                    // Codestream box - parse for dimensions
                    let content_offset = if box_type == "jxlp" { 12 } else { 8 };
                    if offset + content_offset < file_size {
                        let content_size = box_size.saturating_sub(content_offset);
                        let max_read = content_size.min(64); // Only need header
                        if max_read > 0 {
                            let content =
                                reader.read((offset + content_offset) as u64, max_read)?;
                            let _ = Self::parse_codestream_header(content, metadata);
                        }
                    }
                }
                "Exif" => {
                    // Jpeg2000.pm 13.59:469-476: the box is a big-endian
                    // offset word followed by the TIFF header at
                    // `4 + (length > 4 ? unpack("N") : 0)` (:474), and the
                    // block is handed to ProcessTIFF with Exif::Main, so it
                    // decodes exactly as a JPEG APP1 does. Its `Base` is the
                    // TIFF header's own file position (:1245 `Base => $base +
                    // $dataPos + $subdirStart`, the Start expression included):
                    // box start + 8 (box header) + 4 (offset word) + offset.
                    if box_size > 12 {
                        let exif_data = reader.read((offset + 8) as u64, box_size - 8)?;
                        let tiff_offset = if exif_data.len() > 4 {
                            u32::from_be_bytes([
                                exif_data[0],
                                exif_data[1],
                                exif_data[2],
                                exif_data[3],
                            ]) as usize
                        } else {
                            0
                        };
                        if let Some(tiff_data) = exif_data.get(4 + tiff_offset..) {
                            let tiff_base = (offset + 8 + 4 + tiff_offset) as u64;
                            parse_embedded_exif_at(tiff_data, tiff_base, metadata);
                        }
                    }
                }
                "xml " => {
                    // XMP box
                    if box_size > 8 {
                        let xmp_data = reader.read((offset + 8) as u64, box_size - 8)?;
                        if let Ok(xmp_str) = std::str::from_utf8(xmp_data) {
                            // Extract basic XMP metadata
                            Self::parse_xmp_data(xmp_str, metadata);
                        }
                    }
                }
                "jxll" => {
                    // Level box - indicates feature level
                    if box_size >= 12 {
                        let level_data = reader.read((offset + 8) as u64, 4)?;
                        let level = level_data[0];
                        metadata.insert("JXLLevel".to_string(), TagValue::Integer(level as i64));
                    }
                }
                _ => {}
            }

            offset += box_size;
        }

        Ok(())
    }

    /// Extract metadata from XMP using the proper RDF parser
    fn parse_xmp_data(xmp: &str, metadata: &mut MetadataMap) {
        if let Ok(xmp_tags) = parse_xmp(xmp.as_bytes()) {
            for (tag_name, value) in xmp_tags {
                metadata.insert(tag_name, TagValue::String(value));
            }
        }
    }
}

impl FormatParser for JXLParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid JXL signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("JXL".to_string()));

        if Self::is_container_format(reader)? {
            // Container format (ISOBMFF-based)
            metadata.insert(
                "JXLFormat".to_string(),
                TagValue::String("Container".to_string()),
            );
            Self::parse_container_boxes(reader, &mut metadata)?;
        } else {
            // Bare codestream
            metadata.insert(
                "JXLFormat".to_string(),
                TagValue::String("Codestream".to_string()),
            );
            // ExifTool names the two encodings differently. Only the ISO BMFF
            // container is plain `JXL`; a bare codestream gets its own type
            // (Jpeg2000.pm:1628):
            //
            //     $et->SetFileType('JXL Codestream','image/jxl', 'jxl');
            //
            // Into the `File` group because it has to outrank
            // `%fileTypeLookup`, which answers `JXL` for the `.jxl` extension
            // whichever encoding is inside. The MIME type and extension are
            // the same either way, so only the type is set here.
            metadata.insert(
                "File:FileType".to_string(),
                TagValue::String("JXL Codestream".to_string()),
            );
            // Read codestream header (first 64 bytes should be enough)
            let header_size = (reader.size() as usize).min(64);
            let header = reader.read(0, header_size)?;
            let _ = Self::parse_codestream_header(header, &mut metadata);
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::JXL)
    }
}

/// Parses metadata from JPEG XL files.
///
/// This is a convenience wrapper around JXLParser that provides a functional API.
pub fn parse_jxl_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = JXLParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// The bare codestream: `%magicNumber{JXL}`'s first alternative.
    fn codestream() -> Vec<u8> {
        let mut data = vec![0xFF, 0x0A];
        data.extend_from_slice(&[0x08, 0x04, 0x8E, 0x81, 0x3C]);
        data.resize(64, 0);
        data
    }

    /// The ISO BMFF container: a `JXL ` signature box then an `ftyp` box, the
    /// second alternative of the same pattern.
    fn container() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"\0\0\0\x0cJXL \x0d\x0a\x87\x0a");
        data.extend_from_slice(b"\0\0\0\x14ftypjxl \0\0\0\0jxl ");
        data
    }

    #[test]
    fn a_bare_codestream_is_not_named_plain_jxl() {
        // Jpeg2000.pm:1628 gives the codestream its own file type;
        // `%fileTypeLookup` answers `JXL` for the extension either way, so the
        // distinction can only come from the content.
        let metadata = JXLParser.parse(&TestReader::new(codestream())).unwrap();
        assert_eq!(metadata.get_string("JXLFormat"), Some("Codestream"));
        assert_eq!(metadata.get_string("File:FileType"), Some("JXL Codestream"));
        // These SizeHeader bytes are ExifTool's own t/images/JXL.jxl sample,
        // which real ExifTool reports as File:ImageWidth=200, File:ImageHeight=130.
        assert_eq!(metadata.get_integer("File:ImageWidth"), Some(200));
        assert_eq!(metadata.get_integer("File:ImageHeight"), Some(130));
    }

    #[test]
    fn a_container_keeps_the_plain_jxl_name() {
        let metadata = JXLParser.parse(&TestReader::new(container())).unwrap();
        assert_eq!(metadata.get_string("JXLFormat"), Some("Container"));
        // No `File:FileType` -- the identification layer's `JXL` stands.
        assert_eq!(metadata.get_string("File:FileType"), None);
    }

    /// An `Exif` box: size, type, the big-endian offset word, `pad` bytes,
    /// then the TIFF block (Jpeg2000.pm 13.59:474).
    fn exif_box(tiff_offset: u32, pad: &[u8], tiff: &[u8]) -> Vec<u8> {
        let mut data = ((8 + 4 + pad.len() + tiff.len()) as u32)
            .to_be_bytes()
            .to_vec();
        data.extend_from_slice(b"Exif");
        data.extend_from_slice(&tiff_offset.to_be_bytes());
        data.extend_from_slice(pad);
        data.extend_from_slice(tiff);
        data
    }

    /// Exif.pm 13.59:3849-3873 through the Exif box: an all-NUL
    /// PanasonicTitle creates no tag, a NUL-padded PanasonicTitle2 prints
    /// exactly, and Make loses its trailing blank (Exif.pm:585) -- the rows
    /// the same TIFF block prints inside a JPEG. The second case carries a
    /// non-zero offset word, which Jpeg2000.pm:474 adds to the TIFF start.
    #[test]
    fn exif_box_panasonic_title_rawconv_matches_jpeg_path() {
        use crate::parsers::image::embedded::test_fixtures::{
            assert_block_a, assert_block_b, panasonic_title_block_a, panasonic_title_block_b,
        };

        let block = panasonic_title_block_a();
        let mut file = container();
        file.extend_from_slice(&exif_box(0, &[], &block));
        let metadata = JXLParser.parse(&TestReader::new(file)).unwrap();
        assert_block_a(&metadata);

        let mut file = container();
        file.extend_from_slice(&exif_box(6, &[0u8; 6], &block));
        let metadata = JXLParser.parse(&TestReader::new(file)).unwrap();
        assert_block_a(&metadata);

        let mut file = container();
        file.extend_from_slice(&exif_box(0, &[], &panasonic_title_block_b()));
        let metadata = JXLParser.parse(&TestReader::new(file)).unwrap();
        assert_block_b(&metadata);
    }

    /// Jpeg2000.pm 13.59:474 / :1245: the `Base` added to the offsets
    /// reported from the Exif box is the TIFF header's file position --
    /// signature box 12 + ftyp 20 + box header 8 + offset word 4 = 44, plus
    /// the offset word's value (50 for 6 bytes of padding); the oracle
    /// prints 1044 and 1050 for these two layouts.
    #[test]
    fn exif_box_offsets_are_reported_from_the_tiff_header_position() {
        use crate::parsers::image::embedded::test_fixtures::{
            assert_other_image_start, interop_offset_block,
        };

        let block = interop_offset_block();
        let mut file = container();
        file.extend_from_slice(&exif_box(0, &[], &block));
        let metadata = JXLParser.parse(&TestReader::new(file)).unwrap();
        assert_other_image_start(&metadata, 44);

        let mut file = container();
        file.extend_from_slice(&exif_box(6, &[0u8; 6], &block));
        let metadata = JXLParser.parse(&TestReader::new(file)).unwrap();
        assert_other_image_start(&metadata, 50);
    }

    #[test]
    fn a_non_jxl_file_is_rejected() {
        let err = JXLParser.parse(&TestReader::new(vec![0u8; 32]));
        assert!(err.is_err());
    }
}
