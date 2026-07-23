//! JPEG APP segment parsers (APP0, APP8, APP11, APP12, COM, SOF)
//!
//! This module provides parsers for various JPEG application-specific segments:
//! - APP0: JFIF/JFXX
//! - APP8: SPIFF
//! - APP11: JPEG-HDR
//! - APP12: Picture Info (Ducky)
//! - COM: JPEG Comment
//! - SOF: Start of Frame (component information)

use crate::core::{MetadataMap, TagValue};
use crate::io::EndianReader;

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

    // AVI1 (Motion JPEG) APP0 segment
    //
    // ExifTool exposes the byte after the "AVI1\0" signature as
    // APP0:InterleavedField.
    if data.len() >= 6 && &data[0..5] == b"AVI1\0" {
        let interleaved = data[5];
        let interleaved_str = match interleaved {
            0 => "Not Interleaved",
            1 => "Odd",
            2 => "Even",
            _ => "Unknown",
        };
        metadata.insert(
            "APP0:InterleavedField".to_string(),
            TagValue::String(interleaved_str.to_string()),
        );
        return Ok(());
    }

    // OCAD APP0 segment
    //
    // OCAD writes an ASCII revision header of the form
    // "Ocad$Rev: <decimal revision> $", followed by padding and other data.
    const OCAD_REVISION_PREFIX: &[u8] = b"Ocad$Rev: ";
    if data.starts_with(OCAD_REVISION_PREFIX) {
        let revision_data = &data[OCAD_REVISION_PREFIX.len()..];
        let digit_count = revision_data
            .iter()
            .position(|byte| !byte.is_ascii_digit())
            .unwrap_or(revision_data.len());

        if digit_count == 0 {
            return Err("OCAD APP0 segment has no revision number".to_string());
        }

        let revision = std::str::from_utf8(&revision_data[..digit_count])
            .map_err(|_| "OCAD revision is not valid ASCII".to_string())?
            .parse::<i64>()
            .map_err(|_| "OCAD revision is outside the supported integer range".to_string())?;

        metadata.insert("APP0:OcadRevision".to_string(), TagValue::Integer(revision));
        return Ok(());
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
/// SPIFF (Still Picture Interchange File Format, ISO/IEC 10918-3) stores basic
/// image parameters in the first APP8 segment. ExifTool processes APP8 as
/// SPIFF only when the payload starts with "SPIFF\0" AND is exactly 32 bytes;
/// real-world v1.2 samples carry 2 pad bytes after ColorComponents that the
/// spec does not mention, and the offsets below follow those samples
/// (ExifTool JPEG.pm %SPIFF table).
pub fn parse_spiff_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    if data.len() != 32 {
        return Err(format!(
            "APP8 SPIFF payload must be 32 bytes, got {}",
            data.len()
        ));
    }
    if &data[0..6] != b"SPIFF\0" {
        return Err("Invalid SPIFF identifier".to_string());
    }

    // Offsets are relative to the byte after the 6-byte identifier.
    let body = &data[6..];
    let reader = EndianReader::big_endian(body);

    metadata.insert(
        "SPIFF:SPIFFVersion".to_string(),
        TagValue::String(format!("{}.{}", body[0], body[1])),
    );

    let profile_id = match body[2] {
        0 => "Not Specified".to_string(),
        1 => "Continuous-tone Base".to_string(),
        2 => "Continuous-tone Progressive".to_string(),
        3 => "Bi-level Facsimile".to_string(),
        4 => "Continuous-tone Facsimile".to_string(),
        other => format!("Unknown ({})", other),
    };
    metadata.insert("SPIFF:ProfileID".to_string(), TagValue::String(profile_id));

    metadata.insert(
        "SPIFF:ColorComponents".to_string(),
        TagValue::Integer(body[3] as i64),
    );

    metadata.insert(
        "SPIFF:ImageHeight".to_string(),
        TagValue::Integer(reader.u32_at(6).unwrap_or(0) as i64),
    );
    metadata.insert(
        "SPIFF:ImageWidth".to_string(),
        TagValue::Integer(reader.u32_at(10).unwrap_or(0) as i64),
    );

    let color_space = match body[14] {
        0 => "Bi-level".to_string(),
        1 => "YCbCr, ITU-R BT 709, video".to_string(),
        2 => "No color space specified".to_string(),
        3 => "YCbCr, ITU-R BT 601-1, RGB".to_string(),
        4 => "YCbCr, ITU-R BT 601-1, video".to_string(),
        8 => "Gray-scale".to_string(),
        9 => "PhotoYCC".to_string(),
        10 => "RGB".to_string(),
        11 => "CMY".to_string(),
        12 => "CMYK".to_string(),
        13 => "YCCK".to_string(),
        14 => "CIELab".to_string(),
        other => format!("Unknown ({})", other),
    };
    metadata.insert(
        "SPIFF:ColorSpace".to_string(),
        TagValue::String(color_space),
    );

    metadata.insert(
        "SPIFF:BitsPerSample".to_string(),
        TagValue::Integer(body[15] as i64),
    );

    let compression = match body[16] {
        0 => "Uncompressed, interleaved, 8 bits per sample".to_string(),
        1 => "Modified Huffman".to_string(),
        2 => "Modified READ".to_string(),
        3 => "Modified Modified READ".to_string(),
        4 => "JBIG".to_string(),
        5 => "JPEG".to_string(),
        other => format!("Unknown ({})", other),
    };
    metadata.insert(
        "SPIFF:Compression".to_string(),
        TagValue::String(compression),
    );

    let resolution_unit = match body[17] {
        0 => "None".to_string(),
        1 => "inches".to_string(),
        2 => "cm".to_string(),
        other => format!("Unknown ({})", other),
    };
    metadata.insert(
        "SPIFF:ResolutionUnit".to_string(),
        TagValue::String(resolution_unit),
    );

    metadata.insert(
        "SPIFF:YResolution".to_string(),
        TagValue::Integer(reader.u32_at(18).unwrap_or(0) as i64),
    );
    metadata.insert(
        "SPIFF:XResolution".to_string(),
        TagValue::Integer(reader.u32_at(22).unwrap_or(0) as i64),
    );

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
    if let Ok(hdr_metadata) = crate::parsers::jpeg::app_segments::parse_app11_jpeg_hdr(data) {
        for (key, value) in hdr_metadata {
            metadata.insert(key, value);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_parse_app0_ocad_revision() {
        // Exact APP0 payload layout used by SonyDCR-DVD710.jpg.
        let data = b"Ocad$Rev: 14797 $\0\0\0\0\0\0\0\x01\x10";
        let mut metadata = MetadataMap::new();

        let result = parse_app0_extended(data, &mut metadata);

        assert!(result.is_ok());
        assert_eq!(metadata.get_integer("APP0:OcadRevision"), Some(14797));
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

    /// Builds the 32-byte APP8 SPIFF payload ExifTool recognizes
    /// (identifier + version + profile + components + 2 pad bytes +
    /// dimensions + colorspace/bps/compression/unit + resolutions).
    fn spiff_payload_32() -> Vec<u8> {
        let mut p = b"SPIFF\0".to_vec();
        p.extend_from_slice(&[1, 0]); // version 1.0
        p.push(1); // ProfileID: Continuous-tone Base
        p.push(3); // 3 color components
        p.extend_from_slice(&[0, 0]); // pad bytes seen in real v1.2 samples
        p.extend_from_slice(&480u32.to_be_bytes()); // height
        p.extend_from_slice(&640u32.to_be_bytes()); // width
        p.extend_from_slice(&[3, 8, 5, 1]); // BT601 RGB, 8 bits, JPEG, inches
        p.extend_from_slice(&72u32.to_be_bytes()); // Y resolution
        p.extend_from_slice(&72u32.to_be_bytes()); // X resolution
        assert_eq!(p.len(), 32);
        p
    }

    #[test]
    fn test_parse_spiff_segment_full() {
        let mut metadata = MetadataMap::new();
        let result = parse_spiff_segment(&spiff_payload_32(), &mut metadata);
        assert!(result.is_ok());
        assert_eq!(metadata.get_string("SPIFF:SPIFFVersion"), Some("1.0"));
        assert_eq!(
            metadata.get_string("SPIFF:ProfileID"),
            Some("Continuous-tone Base")
        );
        assert_eq!(metadata.get_integer("SPIFF:ColorComponents"), Some(3));
        assert_eq!(metadata.get_integer("SPIFF:ImageHeight"), Some(480));
        assert_eq!(metadata.get_integer("SPIFF:ImageWidth"), Some(640));
        assert_eq!(
            metadata.get_string("SPIFF:ColorSpace"),
            Some("YCbCr, ITU-R BT 601-1, RGB")
        );
        assert_eq!(metadata.get_integer("SPIFF:BitsPerSample"), Some(8));
        assert_eq!(metadata.get_string("SPIFF:Compression"), Some("JPEG"));
        assert_eq!(metadata.get_string("SPIFF:ResolutionUnit"), Some("inches"));
        assert_eq!(metadata.get_integer("SPIFF:YResolution"), Some(72));
        assert_eq!(metadata.get_integer("SPIFF:XResolution"), Some(72));
    }

    #[test]
    fn test_parse_spiff_segment_rejects_non_32_byte_payload() {
        // ExifTool only recognizes 32-byte SPIFF payloads; a 30-byte
        // spec-shaped payload must extract nothing.
        let mut payload = spiff_payload_32();
        payload.truncate(30);
        let mut metadata = MetadataMap::new();
        assert!(parse_spiff_segment(&payload, &mut metadata).is_err());
        assert!(metadata.get("SPIFF:SPIFFVersion").is_none());
    }

    #[test]
    fn test_parse_olympus_app12_f_number() {
        let data = b"[picture info]\r\nFNumber=11.0\r\n";

        let metadata = crate::parsers::jpeg::app_segments::parse_app12_olympus(data)
            .expect("valid Picture Info APP12 data should parse");

        assert!(
            metadata.get("APP12:FNumber").is_some(),
            "FNumber should be exposed in ExifTool's APP12 group"
        );
        assert_eq!(
            metadata.get("APP12:FNumber"),
            metadata.get("Olympus:FNumber"),
            "APP12 FNumber should retain the parsed decimal value"
        );
    }

    #[test]
    fn test_parse_picture_info_image_size() {
        // Picture Info names this source field Resolution, while ExifTool
        // exposes it as APP12:ImageSize.
        let data = b"[picture info]\r\nResolution=1280x960\r\n";

        let metadata = crate::parsers::jpeg::app_segments::parse_app12_olympus(data)
            .expect("valid Picture Info APP12 data should parse");

        assert_eq!(metadata.get_string("APP12:ImageSize"), Some("1280x960"));
    }

    #[test]
    fn test_parse_olympus_app12_flash() {
        let data = b"[picture info]\r\nFlash=Off\r\n";

        let metadata = crate::parsers::jpeg::app_segments::parse_app12_olympus(data)
            .expect("valid Picture Info APP12 data should parse");

        assert_eq!(
            metadata.get_string("APP12:Flash"),
            Some("Off"),
            "Flash should be exposed in ExifTool's APP12 group"
        );
    }

    #[test]
    fn test_parse_legacy_agfa_app12_id() {
        // Identifier-less Agfa Picture Info is recognized by the generic
        // Olympus/Picture Info parser used by APP12 dispatch.
        let data = b"Type=SR84\r\nVersion=v84-71\r\nID=AGFA DIGITAL CAMERA\r\n";

        let metadata = crate::parsers::jpeg::app_segments::parse_app12_olympus(data)
            .expect("valid legacy Picture Info APP12 data should parse");

        assert_eq!(metadata.get_string("APP12:ID"), Some("AGFA DIGITAL CAMERA"));
    }

    #[test]
    fn test_parse_olympus_app12_fcs7() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.\0\r\n[diag info]\r\nFCS6=3\r\nFCS7=3\r\n";

        let metadata = crate::parsers::jpeg::app_segments::parse_app12_olympus(data)
            .expect("valid Olympus Picture Info APP12 data should parse");

        assert_eq!(metadata.get_integer("APP12:FCS6"), Some(3));
        assert_eq!(
            metadata.get_integer("APP12:FCS7"),
            Some(3),
            "FCS7 should be exposed in ExifTool's APP12 group"
        );
    }

    #[test]
    fn test_parse_olympus_app12_imbb() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.\0\r\n[diag info]\r\nIMbb=35761\r\n";

        let metadata = crate::parsers::jpeg::app_segments::parse_app12_olympus(data)
            .expect("valid Olympus Picture Info APP12 data should parse");

        assert_eq!(
            metadata.get_integer("APP12:IMbb"),
            Some(35761),
            "IMbb should be exposed in ExifTool's APP12 group"
        );
    }

    #[test]
    fn test_parse_olympus_app12_imbg() {
        // Diagnostic data from OlympusD620L.jpg.
        let data = b"OLYMPUS OPTICAL CO.,LTD.\0\r\n[diag info]\r\nIMbg=33709\r\n";

        let metadata = crate::parsers::jpeg::app_segments::parse_app12_olympus(data)
            .expect("valid Olympus Picture Info APP12 data should parse");

        assert_eq!(metadata.get_integer("APP12:IMbg"), Some(33709));
    }

    #[test]
    fn test_parse_olympus_app12_imgb() {
        // Diagnostic data from OlympusD620L.jpg.
        let data = b"OLYMPUS OPTICAL CO.,LTD.\0\r\n[diag info]\r\nIMgb=33346\r\n";

        let metadata = crate::parsers::jpeg::app_segments::parse_app12_olympus(data)
            .expect("valid Olympus Picture Info APP12 data should parse");

        assert_eq!(
            metadata.get_integer("APP12:IMgb"),
            Some(33346),
            "IMgb should be exposed in ExifTool's APP12 group"
        );
    }

    #[test]
    fn test_parse_olympus_app12_imgr() {
        // Diagnostic data from OlympusD620L.jpg.
        let data = b"OLYMPUS OPTICAL CO.,LTD.\0\r\n[diag info]\r\nIMgr=33122\r\n";

        let metadata = crate::parsers::jpeg::app_segments::parse_app12_olympus(data)
            .expect("valid Olympus Picture Info APP12 data should parse");

        assert_eq!(
            metadata.get_integer("APP12:IMgr"),
            Some(33122),
            "IMgr should be exposed in ExifTool's APP12 group"
        );
    }

    #[test]
    fn test_parse_olympus_app12_imrg() {
        // Diagnostic data from OlympusD620L.jpg.
        let data = b"OLYMPUS OPTICAL CO.,LTD.\0\r\n[diag info]\r\nIMrg=33975\r\n";

        let metadata = crate::parsers::jpeg::app_segments::parse_app12_olympus(data)
            .expect("valid Olympus Picture Info APP12 data should parse");

        assert_eq!(
            metadata.get_integer("APP12:IMrg"),
            Some(33975),
            "IMrg should be exposed in ExifTool's APP12 group"
        );
    }

    #[test]
    fn test_parse_olympus_app12_imbr() {
        // Diagnostic data from OlympusD620L.jpg.
        let data = b"OLYMPUS OPTICAL CO.,LTD.\0\r\n[diag info]\r\nIMbr=32929\r\n";

        let metadata = crate::parsers::jpeg::app_segments::parse_app12_olympus(data)
            .expect("valid Olympus Picture Info APP12 data should parse");

        assert_eq!(
            metadata.get_integer("APP12:IMbr"),
            Some(32929),
            "IMbr should be exposed in ExifTool's APP12 group"
        );
    }

    #[test]
    fn test_parse_olympus_app12_imrb() {
        // Diagnostic data from OlympusD620L.jpg.
        let data = b"OLYMPUS OPTICAL CO.,LTD.\0\r\n[diag info]\r\nIMrb=32721\r\n";

        let metadata = crate::parsers::jpeg::app_segments::parse_app12_olympus(data)
            .expect("valid Olympus Picture Info APP12 data should parse");

        assert_eq!(
            metadata.get_integer("APP12:IMrb"),
            Some(32721),
            "IMrb should be exposed in ExifTool's APP12 group"
        );
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
}
