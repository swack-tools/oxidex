//! JPEG APP segment parsers (APP2, APP12, APP14, COM, SOF)
//!
//! This module provides parsers for various JPEG application-specific segments:
//! - APP2: ICC Profile
//! - APP12: Picture Info (Ducky)
//! - APP14: Adobe segment
//! - COM: JPEG Comment
//! - SOF: Start of Frame (component information)

use crate::core::{MetadataMap, TagValue};
use crate::io::EndianReader;

/// Parse ICC Profile (APP2) segment
///
/// ICC Profile segments start with "ICC_PROFILE\0"
pub fn parse_icc_profile_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    if data.len() < 14 {
        return Err("ICC Profile segment too short".to_string());
    }

    // Check for ICC_PROFILE identifier
    if &data[0..12] != b"ICC_PROFILE\0" {
        return Err("Invalid ICC Profile identifier".to_string());
    }

    // Sequence number (1-based)
    let sequence = data[12];
    // Total number of APP2 segments
    let total = data[13];

    metadata.insert(
        "ICC_Profile:ProfileSequence".to_string(),
        TagValue::String(format!("{} of {}", sequence, total)),
    );

    // If this is the first segment, extract profile header info
    if sequence == 1 && data.len() >= 128 + 14 {
        let profile_data = &data[14..];

        // Profile size (bytes 0-3)
        if profile_data.len() >= 4 {
            let reader = EndianReader::big_endian(profile_data);
            let size = reader.u32_at(0).unwrap_or(0);
            metadata.insert(
                "ICC_Profile:ProfileSize".to_string(),
                TagValue::Integer(size as i64),
            );
        }

        // Profile version (bytes 8-11)
        if profile_data.len() >= 11 {
            let version_major = profile_data[8];
            let version_minor = (profile_data[9] >> 4) & 0x0F;
            metadata.insert(
                "ICC_Profile:ProfileVersion".to_string(),
                TagValue::String(format!("{}.{}", version_major, version_minor)),
            );
        }

        // Profile class (bytes 12-15)
        if profile_data.len() >= 16
            && let Ok(class) = std::str::from_utf8(&profile_data[12..16])
        {
            let class_desc = match class {
                "scnr" => "Input Device Profile",
                "mntr" => "Display Device Profile",
                "prtr" => "Output Device Profile",
                "link" => "DeviceLink Profile",
                "spac" => "ColorSpace Conversion Profile",
                "abst" => "Abstract Profile",
                "nmcl" => "Named Color Profile",
                _ => class,
            };
            metadata.insert(
                "ICC_Profile:ProfileClass".to_string(),
                TagValue::String(class_desc.to_string()),
            );
        }

        // Color space (bytes 16-19)
        if profile_data.len() >= 20
            && let Ok(color_space) = std::str::from_utf8(&profile_data[16..20])
        {
            metadata.insert(
                "ICC_Profile:ColorSpace".to_string(),
                TagValue::String(color_space.trim().to_string()),
            );
        }
    }

    Ok(())
}

/// Parse Picture Info (Ducky) segment (APP12)
///
/// Ducky segments start with "Ducky"
pub fn parse_ducky_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    if data.len() < 5 {
        return Err("Ducky segment too short".to_string());
    }

    if &data[0..5] != b"Ducky" {
        return Err("Invalid Ducky identifier".to_string());
    }

    // Ducky format: sequence of tag-length-value triplets
    let mut offset = 5;
    while offset + 4 <= data.len() {
        let header_reader = EndianReader::big_endian(&data[offset..]);
        let tag_id = header_reader.u16_at(0).unwrap_or(0);
        let length = header_reader.u16_at(2).unwrap_or(0) as usize;
        offset += 4;

        if offset + length > data.len() {
            break;
        }

        let value_data = &data[offset..offset + length];

        // Parse common Ducky tags
        match tag_id {
            0x0001 => {
                // Quality
                if length >= 4 {
                    let value_reader = EndianReader::big_endian(value_data);
                    let quality = value_reader.i32_at(0).unwrap_or(0);
                    metadata.insert(
                        "Ducky:Quality".to_string(),
                        TagValue::Integer(quality as i64),
                    );
                }
            }
            0x0002 => {
                // Comment
                if let Ok(comment) = std::str::from_utf8(value_data) {
                    metadata.insert(
                        "Ducky:Comment".to_string(),
                        TagValue::String(comment.to_string()),
                    );
                }
            }
            0x0003 => {
                // Copyright
                if let Ok(copyright) = std::str::from_utf8(value_data) {
                    metadata.insert(
                        "Ducky:Copyright".to_string(),
                        TagValue::String(copyright.to_string()),
                    );
                }
            }
            _ => {
                // Unknown tag
                metadata.insert(
                    format!("Ducky:Tag_{:04X}", tag_id),
                    TagValue::Binary(value_data.to_vec()),
                );
            }
        }

        offset += length;
    }

    Ok(())
}

/// Parse Adobe segment (APP14)
///
/// Adobe segments start with "Adobe"
pub fn parse_adobe_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    if data.len() < 12 {
        return Err("Adobe segment too short".to_string());
    }

    if &data[0..5] != b"Adobe" {
        return Err("Invalid Adobe identifier".to_string());
    }

    let reader = EndianReader::big_endian(data);

    // DCT Encode Version (2 bytes at offset 5)
    let dct_encode_version = reader.u16_at(5).unwrap_or(0);
    metadata.insert(
        "Adobe:DCTEncodeVersion".to_string(),
        TagValue::Integer(dct_encode_version as i64),
    );

    // APP14 Flags0 (2 bytes at offset 7)
    let flags0 = reader.u16_at(7).unwrap_or(0);
    metadata.insert(
        "Adobe:APP14Flags0".to_string(),
        TagValue::Integer(flags0 as i64),
    );

    // APP14 Flags1 (2 bytes at offset 9)
    let flags1 = reader.u16_at(9).unwrap_or(0);
    metadata.insert(
        "Adobe:APP14Flags1".to_string(),
        TagValue::Integer(flags1 as i64),
    );

    // Color Transform (1 byte at offset 11)
    let color_transform = data[11];
    let transform_desc = match color_transform {
        0 => "Unknown (RGB or CMYK)",
        1 => "YCbCr",
        2 => "YCCK",
        _ => "Unknown",
    };
    metadata.insert(
        "Adobe:ColorTransform".to_string(),
        TagValue::String(transform_desc.to_string()),
    );

    Ok(())
}

/// Parse JPEG Comment segment (COM, marker 0xFFFE)
///
/// ExifTool exposes COM data as the File:Comment tag and strips trailing NUL
/// bytes ("some dumb softwares add null terminators" — ExifTool.pm COM handler).
pub fn parse_comment_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    let end = data.iter().rposition(|&b| b != 0).map_or(0, |p| p + 1);
    let trimmed = &data[..end];
    match std::str::from_utf8(trimmed) {
        Ok(comment) => {
            metadata.insert(
                "File:Comment".to_string(),
                TagValue::String(comment.to_string()),
            );
        }
        Err(_) => {
            metadata.insert(
                "File:Comment".to_string(),
                TagValue::Binary(trimmed.to_vec()),
            );
        }
    }
    Ok(())
}

/// Parse JFIF APP0 segment with extended fields
///
/// This parser handles both JFIF and JFXX (JFIF extension) segments.
pub fn parse_app0_extended(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    if data.len() < 5 {
        return Err("APP0 segment too short".to_string());
    }

    // Check JFIF identifier
    if &data[0..5] == b"JFIF\x00" {
        // Already parsed by jfif_parser, but we can add JFXX extension support
        // Check if there's more data after standard JFIF header (14 bytes)
        if data.len() >= 14 {
            let thumbnail_width = data[12];
            let thumbnail_height = data[13];

            // If there's thumbnail data, note it
            if thumbnail_width > 0 && thumbnail_height > 0 {
                metadata.insert(
                    "JFIF:HasThumbnail".to_string(),
                    TagValue::String("Yes".to_string()),
                );
            }
        }
        return Ok(());
    }

    // Check for JFXX extension
    if &data[0..5] == b"JFXX\x00" {
        return parse_jfxx_segment(&data[5..], metadata);
    }

    Err("Not a JFIF/JFXX segment".to_string())
}

/// Parse JFXX (JFIF extension) segment
fn parse_jfxx_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    if data.is_empty() {
        return Err("JFXX segment empty".to_string());
    }

    let extension_code = data[0];
    let ext_type = match extension_code {
        0x10 => "Thumbnail JPEG",
        0x11 => "Thumbnail 1 byte/pixel",
        0x13 => "Thumbnail 3 bytes/pixel",
        _ => "Unknown",
    };

    metadata.insert(
        "JFIF:ThumbnailType".to_string(),
        TagValue::String(ext_type.to_string()),
    );

    Ok(())
}

/// Parse SOF (Start of Frame) segment for component information
pub fn parse_sof_segment(
    marker: u16,
    data: &[u8],
    metadata: &mut MetadataMap,
) -> Result<(), String> {
    if data.len() < 6 {
        return Err("SOF segment too short".to_string());
    }

    let reader = EndianReader::big_endian(data);

    // Sample precision (1 byte)
    let precision = data[0];
    metadata.insert(
        "File:BitsPerSample".to_string(),
        TagValue::Integer(precision as i64),
    );

    // Image height (2 bytes)
    let height = reader.u16_at(1).unwrap_or(0);
    metadata.insert(
        "File:ImageHeight".to_string(),
        TagValue::Integer(height as i64),
    );

    // Image width (2 bytes)
    let width = reader.u16_at(3).unwrap_or(0);
    metadata.insert(
        "File:ImageWidth".to_string(),
        TagValue::Integer(width as i64),
    );

    // Number of components (1 byte)
    let num_components = data[5];
    metadata.insert(
        "File:ColorComponents".to_string(),
        TagValue::Integer(num_components as i64),
    );
    // Also add JPEG: prefixed version for format-specific tagging
    metadata.insert(
        "JPEG:ColorComponents".to_string(),
        TagValue::Integer(num_components as i64),
    );

    // Also add JPEG: prefixed versions for format-specific tagging
    metadata.insert("JPEG:Width".to_string(), TagValue::Integer(width as i64));
    metadata.insert("JPEG:Height".to_string(), TagValue::Integer(height as i64));

    // Encoding process - match ExifTool's format with coding suffix
    let encoding = match marker {
        0xFFC0 => "Baseline DCT, Huffman coding",
        0xFFC1 => "Extended Sequential DCT, Huffman coding",
        0xFFC2 => "Progressive DCT, Huffman coding",
        0xFFC3 => "Lossless, Huffman coding",
        0xFFC5 => "Differential Sequential DCT, Huffman coding",
        0xFFC6 => "Differential Progressive DCT, Huffman coding",
        0xFFC7 => "Differential Lossless, Huffman coding",
        0xFFC9 => "Extended Sequential DCT, Arithmetic coding",
        0xFFCA => "Progressive DCT, Arithmetic coding",
        0xFFCB => "Lossless, Arithmetic coding",
        0xFFCD => "Differential Sequential DCT, Arithmetic coding",
        0xFFCE => "Differential Progressive DCT, Arithmetic coding",
        0xFFCF => "Differential Lossless, Arithmetic coding",
        _ => "Unknown",
    };
    metadata.insert(
        "File:EncodingProcess".to_string(),
        TagValue::String(encoding.to_string()),
    );

    // Parse component details to extract YCbCrSubSampling
    let mut offset = 6;

    // First component (Y) determines the subsampling base
    if offset + 3 <= data.len() && num_components >= 3 {
        let sampling_factors = data[offset + 1];
        let y_h_sampling = (sampling_factors >> 4) & 0x0F;
        let y_v_sampling = sampling_factors & 0x0F;

        // Format as "YCbCr4:2:0 (h v)" matching ExifTool's output
        let subsampling_name = match (y_h_sampling, y_v_sampling) {
            (2, 2) => format!("YCbCr4:2:0 ({} {})", y_h_sampling, y_v_sampling),
            (2, 1) => format!("YCbCr4:2:2 ({} {})", y_h_sampling, y_v_sampling),
            (1, 1) => format!("YCbCr4:4:4 ({} {})", y_h_sampling, y_v_sampling),
            _ => format!("YCbCr ({} {})", y_h_sampling, y_v_sampling),
        };

        metadata.insert(
            "File:YCbCrSubSampling".to_string(),
            TagValue::String(subsampling_name),
        );
    }

    // Collect sampling factors for JPEG:SamplingFactors tag
    let mut sampling_factors_vec = Vec::new();

    // Also keep JPEG: prefixed tags for component details
    offset = 6;
    for i in 0..num_components {
        if offset + 3 > data.len() {
            break;
        }

        let component_id = data[offset];
        let sampling_factors = data[offset + 1];
        let h_sampling = (sampling_factors >> 4) & 0x0F;
        let v_sampling = sampling_factors & 0x0F;
        let _quant_table = data[offset + 2];

        // Component name
        let component_name = match component_id {
            1 => "Y",
            2 => "Cb",
            3 => "Cr",
            4 => "I",
            5 => "Q",
            _ => "Unknown",
        };

        metadata.insert(
            format!("JPEG:ComponentID_{}", i + 1),
            TagValue::String(component_name.to_string()),
        );

        let subsampling_str = format!("{}x{}", h_sampling, v_sampling);
        metadata.insert(
            format!("JPEG:YCbCrSubSampling_{}", i + 1),
            TagValue::String(subsampling_str.clone()),
        );

        // Collect sampling factors for the combined tag
        sampling_factors_vec.push(subsampling_str);

        offset += 3;
    }

    // Add combined JPEG:SamplingFactors tag (comma-separated)
    if !sampling_factors_vec.is_empty() {
        metadata.insert(
            "JPEG:SamplingFactors".to_string(),
            TagValue::String(sampling_factors_vec.join(", ")),
        );
    }

    Ok(())
}

/// Parse APP8 (SPIFF) segment
///
/// APP8 segments (marker 0xFFE8) contain SPIFF (Still Picture Interchange File Format) metadata.
/// SPIFF is used primarily by lossless JPEG implementations and includes compression and
/// color space information.
pub fn parse_spiff_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    if data.len() < 6 {
        return Err("APP8 SPIFF segment too short".to_string());
    }

    // Check SPIFF identifier (6 bytes: "SPIFF\0")
    if &data[0..6] != b"SPIFF\0" {
        return Err("Invalid SPIFF identifier".to_string());
    }

    // SPIFF structure after identifier:
    // Byte 6: SPIFF version major
    // Byte 7: SPIFF version minor
    // Byte 8: Profile ID
    // Byte 9: Components
    // Bytes 10-13: Height (big-endian u32)
    // Bytes 14-17: Width (big-endian u32)

    if data.len() >= 8 {
        let version_major = data[6];
        let version_minor = data[7];
        metadata.insert(
            "APP8:SPIFFVersion".to_string(),
            TagValue::String(format!("{}.{}", version_major, version_minor)),
        );
    }

    if data.len() >= 9 {
        let profile_id = data[8];
        let profile_name = match profile_id {
            0 => "Baseline",
            1 => "Progressive",
            2 => "Lossless",
            _ => "Unknown",
        };
        metadata.insert(
            "APP8:SPIFFProfile".to_string(),
            TagValue::String(profile_name.to_string()),
        );
    }

    if data.len() >= 10 {
        let components = data[9];
        metadata.insert(
            "APP8:SPIFFComponents".to_string(),
            TagValue::Integer(components as i64),
        );
    }

    if data.len() >= 18 {
        let reader = EndianReader::big_endian(data);
        let height = reader.u32_at(10).unwrap_or(0);
        let width = reader.u32_at(14).unwrap_or(0);
        metadata.insert(
            "APP8:SPIFFHeight".to_string(),
            TagValue::Integer(height as i64),
        );
        metadata.insert(
            "APP8:SPIFFWidth".to_string(),
            TagValue::Integer(width as i64),
        );
    }

    Ok(())
}

/// Parse APP10 (ActivePhoto) segment
///
/// APP10 segments (marker 0xFFEA) contain Apple ActivePhoto metadata for Live Photos
/// and other dynamic content. The segment contains XML or binary metadata describing
/// motion and interaction capabilities.
pub fn parse_activephoto_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    if data.is_empty() {
        return Err("APP10 ActivePhoto segment is empty".to_string());
    }

    // Check for known identifier patterns
    if data.len() >= 20 && &data[0..7] == b"ActiveP" {
        // Apple ActivePhoto marker (may be followed by version/type data)
        metadata.insert(
            "APP10:Format".to_string(),
            TagValue::String("ActivePhoto".to_string()),
        );

        // Try to extract version if present
        if data.len() >= 10 {
            let version = data[7];
            metadata.insert(
                "APP10:Version".to_string(),
                TagValue::Integer(version as i64),
            );
        }
    } else {
        // Generic APP10 data
        metadata.insert(
            "APP10:DataSize".to_string(),
            TagValue::Integer(data.len() as i64),
        );
    }

    // Try to parse as text if possible
    if let Ok(text) = std::str::from_utf8(data) {
        if text.len() < 200 {
            // Only include as text if reasonably short
            metadata.insert("APP10:Data".to_string(), TagValue::String(text.to_string()));
        }
    }

    Ok(())
}

/// Parse APP11 (JPEG-HDR) segment
///
/// APP11 segments (marker 0xFFEB) contain HDR (High Dynamic Range) metadata.
/// This includes tone mapping information and exposure data for HDR images.
/// Note: More detailed HDR parsing is available in app_segments/app11_jpeg_hdr.rs
pub fn parse_jpeg_hdr_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    if data.is_empty() {
        return Err("APP11 JPEG-HDR segment is empty".to_string());
    }

    // Check for HDR_RI identifier (HDR Rendering Intent)
    if data.len() >= 6 && &data[0..6] == b"HDR_RI" {
        metadata.insert(
            "APP11:Format".to_string(),
            TagValue::String("HDR_RI".to_string()),
        );

        if data.len() >= 7 {
            let rendering_intent = data[6];
            let intent_name = match rendering_intent {
                0 => "Perceptual",
                1 => "Relative Colorimetric",
                2 => "Saturation",
                3 => "Absolute Colorimetric",
                _ => "Unknown",
            };
            metadata.insert(
                "APP11:RenderingIntent".to_string(),
                TagValue::String(intent_name.to_string()),
            );
        }
    } else {
        // Generic HDR data or other JPEG-HDR variant
        metadata.insert(
            "APP11:DataSize".to_string(),
            TagValue::Integer(data.len() as i64),
        );
    }

    // Delegate to specialized HDR parser if available and data looks valid
    let _ = crate::parsers::jpeg::app_segments::parse_app11_jpeg_hdr(data);

    Ok(())
}

/// Parse APP15 (JPEG-LS) segment
///
/// APP15 segments (marker 0xFFEF) contain metadata for JPEG-LS (lossless JPEG)
/// compression. JPEG-LS is defined in ITU-T T.87 and provides better compression
/// than Huffman-based lossless JPEG.
pub fn parse_jpeg_ls_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    if data.is_empty() {
        return Err("APP15 JPEG-LS segment is empty".to_string());
    }

    // Check for JPEGLS identifier (if present)
    if data.len() >= 6 && &data[0..6] == b"JPEGLS" {
        metadata.insert(
            "APP15:Format".to_string(),
            TagValue::String("JPEG-LS".to_string()),
        );
    }

    // JPEG-LS specific parsing
    // Byte 0-1: Application-specific data (typically SOF or parameter markers)
    if data.len() >= 2 {
        let marker_byte1 = data[0];
        let marker_byte2 = data[1];

        // Check for common JPEG-LS markers
        if marker_byte1 == 0xFF {
            let marker_type = match marker_byte2 {
                0xF7 => "SOF-LS (Start of Frame for JPEG-LS)",
                0xF8 => "LSE (JPEG-LS Parameters Extension)",
                0xF9 => "RES (Reserved)",
                _ => "Unknown marker",
            };
            metadata.insert(
                "APP15:MarkerType".to_string(),
                TagValue::String(marker_type.to_string()),
            );
        }
    }

    // Record data size for diagnostic purposes
    metadata.insert(
        "APP15:DataSize".to_string(),
        TagValue::Integer(data.len() as i64),
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_icc_profile() {
        let mut data = Vec::new();
        data.extend_from_slice(b"ICC_PROFILE\0");
        data.push(1); // Sequence 1
        data.push(1); // Total 1

        // Minimal ICC profile header
        let mut profile_header = vec![0u8; 128];
        // Profile size
        profile_header[0..4].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
        // Version 4.0
        profile_header[8] = 0x04;
        profile_header[9] = 0x00;
        // Profile class "mntr" (display)
        profile_header[12..16].copy_from_slice(b"mntr");
        // Color space "RGB "
        profile_header[16..20].copy_from_slice(b"RGB ");

        data.extend_from_slice(&profile_header);

        let mut metadata = MetadataMap::new();
        let result = parse_icc_profile_segment(&data, &mut metadata);

        assert!(result.is_ok());
        assert_eq!(
            metadata.get_string("ICC_Profile:ProfileClass").as_deref(),
            Some("Display Device Profile")
        );
        assert_eq!(
            metadata.get_string("ICC_Profile:ColorSpace").as_deref(),
            Some("RGB")
        );
    }

    #[test]
    fn test_parse_adobe_segment() {
        let data = [
            b'A', b'd', b'o', b'b', b'e', // Identifier
            0x00, 0x64, // DCT Encode Version: 100
            0x00, 0x00, // Flags0
            0x00, 0x00, // Flags1
            0x01, // Color Transform: YCbCr
        ];

        let mut metadata = MetadataMap::new();
        let result = parse_adobe_segment(&data, &mut metadata);

        assert!(result.is_ok());
        assert_eq!(metadata.get_integer("Adobe:DCTEncodeVersion"), Some(100));
        assert_eq!(
            metadata.get_string("Adobe:ColorTransform").as_deref(),
            Some("YCbCr")
        );
    }

    #[test]
    fn test_parse_comment_segment() {
        let mut metadata = MetadataMap::new();
        // Trailing NULs are stripped, matching ExifTool's COM handler
        let result = parse_comment_segment(b"Hello JPEG\0\0", &mut metadata);
        assert!(result.is_ok());
        assert_eq!(metadata.get_string("File:Comment"), Some("Hello JPEG"));
    }

    #[test]
    fn test_parse_comment_segment_binary_fallback() {
        let mut metadata = MetadataMap::new();
        let result = parse_comment_segment(&[0xFF, 0xFE, 0x00, 0x41], &mut metadata);
        assert!(result.is_ok());
        assert_eq!(
            metadata.get("File:Comment"),
            Some(&TagValue::Binary(vec![0xFF, 0xFE, 0x00, 0x41]))
        );
    }

    #[test]
    fn test_parse_sof_baseline() {
        let data = [
            0x08, // Precision: 8 bits
            0x01, 0xE0, // Height: 480
            0x02, 0x80, // Width: 640
            0x03, // Components: 3 (YCbCr)
            // Component 1: Y
            0x01, 0x22, 0x00, // ID=1, sampling=2x2, quant=0
            // Component 2: Cb
            0x02, 0x11, 0x01, // ID=2, sampling=1x1, quant=1
            // Component 3: Cr
            0x03, 0x11, 0x01, // ID=3, sampling=1x1, quant=1
        ];

        let mut metadata = MetadataMap::new();
        let result = parse_sof_segment(0xFFC0, &data, &mut metadata);

        assert!(result.is_ok());
        assert_eq!(metadata.get_integer("File:ImageHeight"), Some(480));
        assert_eq!(metadata.get_integer("File:ImageWidth"), Some(640));
        assert_eq!(metadata.get_integer("File:BitsPerSample"), Some(8));
        assert_eq!(metadata.get_integer("File:ColorComponents"), Some(3));
        assert_eq!(
            metadata.get_string("File:EncodingProcess"),
            Some("Baseline DCT, Huffman coding")
        );
        assert_eq!(
            metadata.get_string("File:YCbCrSubSampling"),
            Some("YCbCr4:2:0 (2 2)")
        );
        assert_eq!(metadata.get_string("JPEG:ComponentID_1"), Some("Y"));
    }

    #[test]
    fn test_parse_spiff_segment() {
        let mut data = Vec::new();
        data.extend_from_slice(b"SPIFF\0");
        data.push(1); // Version major
        data.push(0); // Version minor
        data.push(0); // Profile: Baseline
        data.push(3); // Components: 3

        let mut metadata = MetadataMap::new();
        let result = parse_spiff_segment(&data, &mut metadata);

        assert!(result.is_ok());
        assert_eq!(
            metadata.get_string("APP8:SPIFFVersion"),
            Some("1.0"),
            "Should parse SPIFF version"
        );
        assert_eq!(
            metadata.get_string("APP8:SPIFFProfile"),
            Some("Baseline"),
            "Should identify Baseline profile"
        );
        assert_eq!(
            metadata.get_integer("APP8:SPIFFComponents"),
            Some(3),
            "Should parse components"
        );
    }

    #[test]
    fn test_parse_spiff_with_dimensions() {
        let mut data = Vec::new();
        data.extend_from_slice(b"SPIFF\0");
        data.push(1); // Version major
        data.push(0); // Version minor
        data.push(1); // Profile: Progressive
        data.push(3); // Components
        // Height: 2048 (0x00000800 big-endian)
        data.extend_from_slice(&[0x00, 0x00, 0x08, 0x00]);
        // Width: 1536 (0x00000600 big-endian)
        data.extend_from_slice(&[0x00, 0x00, 0x06, 0x00]);

        let mut metadata = MetadataMap::new();
        let result = parse_spiff_segment(&data, &mut metadata);

        assert!(result.is_ok());
        assert_eq!(
            metadata.get_integer("APP8:SPIFFHeight"),
            Some(2048),
            "Should parse height"
        );
        assert_eq!(
            metadata.get_integer("APP8:SPIFFWidth"),
            Some(1536),
            "Should parse width"
        );
    }

    #[test]
    fn test_parse_activephoto_segment() {
        let data = b"ActivePhoto metadata here";

        let mut metadata = MetadataMap::new();
        let result = parse_activephoto_segment(data, &mut metadata);

        assert!(result.is_ok());
        assert_eq!(
            metadata.get_string("APP10:Format"),
            Some("ActivePhoto"),
            "Should identify ActivePhoto format"
        );
    }

    #[test]
    fn test_parse_activephoto_empty() {
        let data = b"";

        let mut metadata = MetadataMap::new();
        let result = parse_activephoto_segment(data, &mut metadata);

        assert!(result.is_err(), "Should error on empty segment");
    }

    #[test]
    fn test_parse_jpeg_hdr_segment() {
        let data = b"HDR_RI\x01";

        let mut metadata = MetadataMap::new();
        let result = parse_jpeg_hdr_segment(data, &mut metadata);

        assert!(result.is_ok());
        assert_eq!(
            metadata.get_string("APP11:Format"),
            Some("HDR_RI"),
            "Should identify HDR_RI format"
        );
        assert_eq!(
            metadata.get_string("APP11:RenderingIntent"),
            Some("Relative Colorimetric"),
            "Should parse rendering intent"
        );
    }

    #[test]
    fn test_parse_jpeg_ls_segment() {
        let data = b"JPEGLS metadata";

        let mut metadata = MetadataMap::new();
        let result = parse_jpeg_ls_segment(data, &mut metadata);

        assert!(result.is_ok());
        assert_eq!(
            metadata.get_string("APP15:Format"),
            Some("JPEG-LS"),
            "Should identify JPEG-LS format"
        );
    }

    #[test]
    fn test_parse_jpeg_ls_with_marker() {
        let mut data = Vec::new();
        data.push(0xFF);
        data.push(0xF7); // SOF-LS marker
        data.extend_from_slice(b"remaining data");

        let mut metadata = MetadataMap::new();
        let result = parse_jpeg_ls_segment(&data, &mut metadata);

        assert!(result.is_ok());
        assert_eq!(
            metadata.get_string("APP15:MarkerType"),
            Some("SOF-LS (Start of Frame for JPEG-LS)"),
            "Should parse marker type"
        );
    }
}
