//! BMP (Bitmap) image format parser

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::exiftool_tables::binary_tables::BMP_MAIN;
use crate::io::EndianReader;

/// BMP signature: "BM" (0x42 0x4D)
const BMP_SIGNATURE: &[u8] = b"BM";

/// BMP parser for extracting metadata from Windows bitmap images
pub struct BMPParser;

impl BMPParser {
    /// Verifies BMP signature
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 2 {
            return Ok(false);
        }
        let header = reader.read(0, 2)?;
        Ok(header == BMP_SIGNATURE)
    }

    /// Reads image dimensions from BMP header
    /// Width at offset 18 (4 bytes), Height at offset 22 (4 bytes), both little-endian
    pub fn read_dimensions(reader: &dyn FileReader) -> Result<(i32, i32)> {
        if reader.size() < 26 {
            return Ok((0, 0));
        }
        let header_data = reader.read(18, 8)?;
        let endian_reader = EndianReader::little_endian(header_data);
        let width = endian_reader.i32_at(0).unwrap_or(0);
        let height = endian_reader.i32_at(4).unwrap_or(0);
        Ok((width, height))
    }

    /// Reads the DIB header size, which ExifTool reports as BMPVersion.
    pub fn read_bmp_version(reader: &dyn FileReader) -> Result<u32> {
        if reader.size() < 18 {
            return Ok(0);
        }
        let version = reader.read(14, 4)?;
        let endian_reader = EndianReader::little_endian(version);
        Ok(endian_reader.u32_at(0).unwrap_or(0))
    }

    /// Reads the number of color planes from the DIB header.
    pub fn read_planes(reader: &dyn FileReader) -> Result<u16> {
        if reader.size() < 18 {
            return Ok(0);
        }

        let dib_header = reader.read(14, 4)?;
        let dib_header_size = EndianReader::little_endian(dib_header)
            .u32_at(0)
            .unwrap_or(0);
        let offset = if matches!(dib_header_size, 12 | 16 | 64) {
            22
        } else {
            26
        };
        if reader.size() < offset + 2 {
            return Ok(0);
        }

        let planes = reader.read(offset, 2)?;
        let endian_reader = EndianReader::little_endian(planes);
        Ok(endian_reader.u16_at(0).unwrap_or(0))
    }

    /// Reads bit depth from BMP header (offset 28, 2 bytes)
    pub fn read_bit_depth(reader: &dyn FileReader) -> Result<u16> {
        if reader.size() < 30 {
            return Ok(0);
        }
        let bits = reader.read(28, 2)?;
        let endian_reader = EndianReader::little_endian(bits);
        Ok(endian_reader.u16_at(0).unwrap_or(0))
    }

    /// Reads compression method from BMP header (offset 30, 4 bytes)
    pub fn read_compression(reader: &dyn FileReader) -> Result<u32> {
        if reader.size() < 34 {
            return Ok(0);
        }
        let comp = reader.read(30, 4)?;
        let endian_reader = EndianReader::little_endian(comp);
        Ok(endian_reader.u32_at(0).unwrap_or(0))
    }

    /// Reads the compressed image size from the DIB header (offset 34, 4 bytes).
    pub fn read_image_length(reader: &dyn FileReader) -> Result<u32> {
        if reader.size() < 38 {
            return Ok(0);
        }
        let length = reader.read(34, 4)?;
        let endian_reader = EndianReader::little_endian(length);
        Ok(endian_reader.u32_at(0).unwrap_or(0))
    }

    /// Reads horizontal pixels per meter from the DIB header (offset 38, 4 bytes).
    pub fn read_pixels_per_meter_x(reader: &dyn FileReader) -> Result<u32> {
        if reader.size() < 42 {
            return Ok(0);
        }
        let pixels_per_meter = reader.read(38, 4)?;
        let endian_reader = EndianReader::little_endian(pixels_per_meter);
        Ok(endian_reader.u32_at(0).unwrap_or(0))
    }

    /// Reads vertical pixels per meter from the DIB header (offset 42, 4 bytes).
    pub fn read_pixels_per_meter_y(reader: &dyn FileReader) -> Result<u32> {
        if reader.size() < 46 {
            return Ok(0);
        }
        let pixels_per_meter = reader.read(42, 4)?;
        let endian_reader = EndianReader::little_endian(pixels_per_meter);
        Ok(endian_reader.u32_at(0).unwrap_or(0))
    }

    /// Reads horizontal resolution from BMP header (offset 38, 4 bytes)
    /// Returns pixels per meter
    pub fn read_h_resolution(reader: &dyn FileReader) -> Result<i32> {
        if reader.size() < 42 {
            return Ok(0);
        }
        let res = reader.read(38, 4)?;
        let endian_reader = EndianReader::little_endian(res);
        Ok(endian_reader.i32_at(0).unwrap_or(0))
    }

    /// Reads vertical resolution from BMP header (offset 42, 4 bytes)
    /// Returns pixels per meter
    pub fn read_v_resolution(reader: &dyn FileReader) -> Result<i32> {
        if reader.size() < 46 {
            return Ok(0);
        }
        let res = reader.read(42, 4)?;
        let endian_reader = EndianReader::little_endian(res);
        Ok(endian_reader.i32_at(0).unwrap_or(0))
    }

    /// Reads number of colors in palette (offset 46, 4 bytes)
    pub fn read_num_colors(reader: &dyn FileReader) -> Result<u32> {
        if reader.size() < 50 {
            return Ok(0);
        }
        let colors = reader.read(46, 4)?;
        let endian_reader = EndianReader::little_endian(colors);
        Ok(endian_reader.u32_at(0).unwrap_or(0))
    }

    /// Reads number of important colors (offset 50, 4 bytes)
    pub fn read_num_important_colors(reader: &dyn FileReader) -> Result<u32> {
        if reader.size() < 54 {
            return Ok(0);
        }
        let colors = reader.read(50, 4)?;
        let endian_reader = EndianReader::little_endian(colors);
        Ok(endian_reader.u32_at(0).unwrap_or(0))
    }
}

impl FormatParser for BMPParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid BMP signature"));
        }

        let mut metadata = MetadataMap::new();

        metadata.insert("FileType".to_string(), TagValue::String("BMP".to_string()));

        let bmp_version = Self::read_bmp_version(reader)?;
        if bmp_version >= 40 {
            let version = BMP_MAIN
                .fields
                .iter()
                .find(|field| field.name == "BMPVersion")
                .and_then(|field| field.print_conv.apply(i64::from(bmp_version)))
                .unwrap_or_else(|| bmp_version.to_string());
            metadata.insert("BMPVersion".to_string(), TagValue::String(version));

            let image_length = Self::read_image_length(reader)?;
            metadata.insert(
                "ImageLength".to_string(),
                TagValue::Integer(i64::from(image_length)),
            );

            let pixels_per_meter_x = Self::read_pixels_per_meter_x(reader)?;
            metadata.insert(
                "PixelsPerMeterX".to_string(),
                TagValue::Integer(i64::from(pixels_per_meter_x)),
            );

            let pixels_per_meter_y = Self::read_pixels_per_meter_y(reader)?;
            metadata.insert(
                "PixelsPerMeterY".to_string(),
                TagValue::Integer(i64::from(pixels_per_meter_y)),
            );
        }

        let (width, height) = Self::read_dimensions(reader)?;
        let abs_width = width.abs() as u64;
        let abs_height = height.abs() as u64;

        metadata.insert(
            "ImageWidth".to_string(),
            TagValue::String(abs_width.to_string()),
        );
        metadata.insert(
            "ImageHeight".to_string(),
            TagValue::String(abs_height.to_string()),
        );

        // Add BMP: prefixed versions for format-specific tagging
        metadata.insert("BMP:Width".to_string(), TagValue::Integer(abs_width as i64));
        metadata.insert(
            "BMP:Height".to_string(),
            TagValue::Integer(abs_height as i64),
        );

        let planes = Self::read_planes(reader)?;
        metadata.insert("Planes".to_string(), TagValue::Integer(planes as i64));

        let bit_depth = Self::read_bit_depth(reader)?;
        metadata.insert(
            "BitDepth".to_string(),
            TagValue::String(bit_depth.to_string()),
        );
        // Add BMP: prefixed version for format-specific tagging
        metadata.insert(
            "BMP:BitDepth".to_string(),
            TagValue::Integer(bit_depth as i64),
        );

        // Compression method
        let compression = Self::read_compression(reader)?;
        let compression_str = match compression {
            0 => "None",
            1 => "RLE 8-bit",
            2 => "RLE 4-bit",
            3 => "Bitfields",
            4 => "JPEG",
            5 => "PNG",
            _ => "Unknown",
        };
        metadata.insert(
            "Compression".to_string(),
            TagValue::String(compression_str.to_string()),
        );
        // Add BMP: prefixed version for format-specific tagging
        metadata.insert(
            "BMP:Compression".to_string(),
            TagValue::String(compression_str.to_string()),
        );

        // Resolution
        let h_res = Self::read_h_resolution(reader)?;
        let v_res = Self::read_v_resolution(reader)?;
        if h_res > 0 {
            metadata.insert(
                "XResolution".to_string(),
                TagValue::String(format!("{} pixels/meter", h_res)),
            );
            // Add BMP: prefixed version for format-specific tagging
            metadata.insert(
                "BMP:XResolution".to_string(),
                TagValue::String(format!("{} pixels/meter", h_res)),
            );
        }
        if v_res > 0 {
            metadata.insert(
                "YResolution".to_string(),
                TagValue::String(format!("{} pixels/meter", v_res)),
            );
            // Add BMP: prefixed version for format-specific tagging
            metadata.insert(
                "BMP:YResolution".to_string(),
                TagValue::String(format!("{} pixels/meter", v_res)),
            );
        }

        // Color palette information
        let num_colors = Self::read_num_colors(reader)?;
        if num_colors > 0 {
            metadata.insert(
                "NumColors".to_string(),
                TagValue::Integer(num_colors as i64),
            );
            // Add BMP: prefixed version for format-specific tagging
            metadata.insert(
                "BMP:ColorCount".to_string(),
                TagValue::Integer(num_colors as i64),
            );
        }

        // Calculate image size (file size - header size, approximately)
        // DIB header is typically at offset 14, and image data follows the color table
        let image_data_size = reader.size().saturating_sub(14);
        if image_data_size > 0 {
            metadata.insert(
                "BMP:ImageSize".to_string(),
                TagValue::Integer(image_data_size as i64),
            );
        }

        let important_colors = Self::read_num_important_colors(reader)?;
        if important_colors > 0 {
            metadata.insert(
                "NumImportantColors".to_string(),
                TagValue::Integer(important_colors as i64),
            );
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::BMP)
    }
}

/// Parses metadata from BMP files.
///
/// This is a convenience wrapper around BMPParser that provides a functional API.
pub fn parse_bmp_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = BMPParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn parses_direct_v3_fields_from_pinned_exiftool_bmp_sample() {
        // Header values from ExifTool 13.59's t/images/BMP.bmp fixture.
        let mut bmp = vec![0_u8; 54];
        bmp[..2].copy_from_slice(BMP_SIGNATURE);
        bmp[14..18].copy_from_slice(&40_u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&8_i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&8_i32.to_le_bytes());
        bmp[26..28].copy_from_slice(&1_u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&8_u16.to_le_bytes());
        bmp[34..38].copy_from_slice(&64_u32.to_le_bytes());
        bmp[38..42].copy_from_slice(&2835_i32.to_le_bytes());
        bmp[42..46].copy_from_slice(&2835_i32.to_le_bytes());

        let metadata = BMPParser.parse(&TestReader::new(bmp)).unwrap();

        assert_eq!(
            metadata.get("BMPVersion"),
            Some(&TagValue::String("Windows V3".to_string()))
        );
        assert_eq!(metadata.get("ImageLength"), Some(&TagValue::Integer(64)));
        assert_eq!(
            metadata.get("PixelsPerMeterX"),
            Some(&TagValue::Integer(2835))
        );
        assert_eq!(
            metadata.get("PixelsPerMeterY"),
            Some(&TagValue::Integer(2835))
        );
    }
}
