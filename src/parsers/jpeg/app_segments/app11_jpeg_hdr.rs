//! JPEG-HDR APP11 segment parser
//!
//! This module provides comprehensive parsing for JPEG-HDR metadata stored in APP11 segments.
//! JPEG-HDR is a backward-compatible extension to JPEG for storing High Dynamic Range (HDR)
//! images. The format stores a tone-mapped base image in standard JPEG format along with
//! metadata in APP11 segments that enable HDR reconstruction.
//!
//! # Format Overview
//!
//! JPEG-HDR segments contain (compatible with ExifTool output):
//! - JPEG-HDRVersion - Format version (1 byte)
//! - CorrectionMethod - Method used for HDR correction (1 byte)
//! - Alpha - Tone mapping exposure coefficient (4 bytes float)
//! - Beta - Tone mapping contrast coefficient (4 bytes float)
//! - Ln0 - Lower luminance bound in log space (4 bytes float)
//! - Ln1 - Upper luminance bound in log space (4 bytes float)
//! - S2n - Signal-to-noise ratio estimate (4 bytes float)
//! - RatioImage - Optional embedded ratio image data (remaining bytes)
//!
//! # Segment Structure
//!
//! JPEG-HDR segments begin with one of these identifiers:
//! - `HDR_RI` (6 bytes) - HDR Ratio Image segment containing reconstruction data
//! - `JPEG-HDR` (8 bytes) - Generic JPEG-HDR parameter segment
//!
//! After the identifier, the binary structure is:
//! ```text
//! Offset  Size  Description
//! 0       1     Version (JPEG-HDRVersion)
//! 1       1     CorrectionMethod
//! 2       4     Alpha (float32, big-endian)
//! 6       4     Beta (float32, big-endian)
//! 10      4     Ln0 (float32, big-endian)
//! 14      4     Ln1 (float32, big-endian)
//! 18      4     S2n (float32, big-endian)
//! 22      N     RatioImage data (optional)
//! ```
//!
//! # ExifTool Compatibility
//!
//! Tags are output with the `APP11` family prefix to match ExifTool's output format.
//! For example: `APP11:Alpha`, `APP11:JPEG-HDRVersion`, etc.
//!
//! # References
//!
//! - Ward, G. & Simmons, M. (2004). "JPEG-HDR: A Backwards-Compatible, High Dynamic Range
//!   Extension to JPEG"

use crate::core::{MetadataMap, TagValue};
use crate::error::Result;
use crate::io::EndianReader;

/// Minimum segment size required for parsing (identifier + basic header)
const MIN_SEGMENT_SIZE: usize = 6;

/// HDR Ratio Image identifier ("HDR_RI")
const HDR_RI_IDENTIFIER: &[u8] = b"HDR_RI";

/// Generic JPEG-HDR identifier
const JPEG_HDR_IDENTIFIER: &[u8] = b"JPEG-HDR";

/// Header size after identifier: version(1) + correction(1) + 5 floats(20) = 22 bytes
const HDR_HEADER_SIZE: usize = 22;

/// Correction method identifiers used in JPEG-HDR tone mapping
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionMethod {
    /// No correction applied
    None,
    /// Multiplicative correction
    Multiplicative,
    /// Additive correction
    Additive,
    /// Logarithmic correction (most common for HDR)
    Logarithmic,
    /// Gamma correction
    Gamma,
    /// Unknown or proprietary method
    Unknown(u8),
}

impl CorrectionMethod {
    /// Converts a byte value to a CorrectionMethod enum variant
    ///
    /// # Arguments
    ///
    /// * `value` - The raw byte value from the segment
    ///
    /// # Returns
    ///
    /// The corresponding CorrectionMethod variant
    fn from_byte(value: u8) -> Self {
        match value {
            0 => CorrectionMethod::None,
            1 => CorrectionMethod::Multiplicative,
            2 => CorrectionMethod::Additive,
            3 => CorrectionMethod::Logarithmic,
            4 => CorrectionMethod::Gamma,
            other => CorrectionMethod::Unknown(other),
        }
    }

    /// Returns a human-readable description of the correction method
    fn description(&self) -> String {
        match self {
            CorrectionMethod::None => "None".to_string(),
            CorrectionMethod::Multiplicative => "Multiplicative".to_string(),
            CorrectionMethod::Additive => "Additive".to_string(),
            CorrectionMethod::Logarithmic => "Logarithmic".to_string(),
            CorrectionMethod::Gamma => "Gamma".to_string(),
            CorrectionMethod::Unknown(v) => format!("Unknown ({})", v),
        }
    }
}

/// Parsed JPEG-HDR parameters
///
/// This structure holds all the HDR-related parameters extracted from an APP11 segment.
/// These parameters are used to reconstruct the full HDR image from the tone-mapped base.
#[derive(Debug, Clone, Default)]
pub struct JpegHdrParameters {
    /// JPEG-HDR format version (major.minor)
    pub version: Option<(u8, u8)>,
    /// Alpha coefficient for tone mapping (exposure adjustment)
    pub alpha: Option<f32>,
    /// Beta coefficient for tone mapping (contrast adjustment)
    pub beta: Option<f32>,
    /// Method used for HDR correction/reconstruction
    pub correction_method: Option<CorrectionMethod>,
    /// Lower luminance bound (Ln0) in log space
    pub ln0: Option<f32>,
    /// Upper luminance bound (Ln1) in log space
    pub ln1: Option<f32>,
    /// Signal-to-noise ratio estimate
    pub s2n: Option<f32>,
    /// Size of ratio image data in bytes (if present)
    pub ratio_image_size: Option<usize>,
    /// Indicates if this segment contains ratio image data
    pub has_ratio_image: bool,
}

/// Parses a JPEG-HDR APP11 segment and returns extracted metadata.
///
/// This function analyzes APP11 segment data to extract JPEG-HDR metadata including
/// version information, tone mapping parameters, and ratio image indicators.
///
/// # Arguments
///
/// * `data` - Raw APP11 segment data (excluding the APP11 marker and length bytes)
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully parsed metadata with JPEG-HDR tags
/// * `Err` - If the segment format is not recognized or parsing fails
///
/// # Supported Identifiers
///
/// - `HDR_RI` - HDR Ratio Image segment containing reconstruction data
/// - `JPEG-HDR` - Generic JPEG-HDR parameter segment
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::jpeg::app_segments::app11_jpeg_hdr::parse_app11_jpeg_hdr;
///
/// let segment_data = &[/* APP11 segment bytes */];
/// match parse_app11_jpeg_hdr(segment_data) {
///     Ok(metadata) => {
///         if let Some(version) = metadata.get_string("JPEG-HDR:Version") {
///             println!("JPEG-HDR Version: {}", version);
///         }
///     }
///     Err(e) => eprintln!("Failed to parse JPEG-HDR segment: {}", e),
/// }
/// ```
pub fn parse_app11_jpeg_hdr(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Validate minimum segment size
    if data.len() < MIN_SEGMENT_SIZE {
        return Err(crate::error::ExifToolError::parse_error(
            "APP11 segment too short for JPEG-HDR parsing",
        ));
    }

    // Detect segment type and dispatch to appropriate parser
    if data.len() >= HDR_RI_IDENTIFIER.len()
        && &data[..HDR_RI_IDENTIFIER.len()] == HDR_RI_IDENTIFIER
    {
        // HDR Ratio Image segment
        parse_hdr_ratio_image_segment(&data[HDR_RI_IDENTIFIER.len()..], &mut metadata)?;
    } else if data.len() >= JPEG_HDR_IDENTIFIER.len()
        && &data[..JPEG_HDR_IDENTIFIER.len()] == JPEG_HDR_IDENTIFIER
    {
        // Generic JPEG-HDR parameter segment
        parse_jpeg_hdr_parameter_segment(&data[JPEG_HDR_IDENTIFIER.len()..], &mut metadata)?;
    } else {
        return Err(crate::error::ExifToolError::parse_error(
            "Not a recognized JPEG-HDR segment (expected HDR_RI or JPEG-HDR identifier)",
        ));
    }

    Ok(metadata)
}

/// Parses an HDR Ratio Image (HDR_RI) segment.
///
/// HDR_RI segments contain the ratio image data used to reconstruct the full HDR
/// image from the tone-mapped base JPEG. The format (after the "HDR_RI" identifier) is:
///
/// ```text
/// Offset  Size  Description
/// 0       1     JPEG-HDRVersion (version number, single byte)
/// 1       1     CorrectionMethod (correction method code)
/// 2       4     Alpha (float32, big-endian, exposure coefficient)
/// 6       4     Beta (float32, big-endian, contrast coefficient)
/// 10      4     Ln0 (float32, big-endian, luminance lower bound)
/// 14      4     Ln1 (float32, big-endian, luminance upper bound)
/// 18      4     S2n (float32, big-endian, signal-to-noise)
/// 22      N     RatioImage data (optional binary image data)
/// ```
///
/// # Arguments
///
/// * `data` - Segment data after the "HDR_RI" identifier
/// * `metadata` - MetadataMap to populate with extracted values
///
/// # ExifTool Compatibility
///
/// Tags are named to match ExifTool's APP11 output format:
/// - APP11:JPEG-HDRVersion
/// - APP11:CorrectionMethod
/// - APP11:Alpha
/// - APP11:Beta
/// - APP11:Ln0
/// - APP11:Ln1
/// - APP11:S2n
/// - APP11:RatioImage
fn parse_hdr_ratio_image_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    if data.is_empty() {
        // Empty segment after identifier - still valid but no parameters
        return Ok(());
    }

    let reader = EndianReader::big_endian(data);

    // Parse JPEG-HDRVersion (1 byte at offset 0)
    // ExifTool reports this as an integer (e.g., "11")
    if !data.is_empty() {
        let version = reader.u8_at(0).unwrap_or(0);
        metadata.insert(
            "APP11:JPEG-HDRVersion".to_string(),
            TagValue::Integer(version as i64),
        );
    }

    // Parse CorrectionMethod (1 byte at offset 1)
    // ExifTool reports this as the raw numeric value (e.g., "0")
    if data.len() >= 2 {
        let correction_byte = reader.u8_at(1).unwrap_or(0);
        metadata.insert(
            "APP11:CorrectionMethod".to_string(),
            TagValue::Integer(correction_byte as i64),
        );
    }

    // Parse Alpha - exposure coefficient (4 bytes float32 at offset 2)
    if data.len() >= 6
        && let Some(alpha) = reader.f32_at(2)
        && alpha.is_finite()
    {
        metadata.insert("APP11:Alpha".to_string(), TagValue::Float(alpha as f64));
    }

    // Parse Beta - contrast coefficient (4 bytes float32 at offset 6)
    if data.len() >= 10
        && let Some(beta) = reader.f32_at(6)
        && beta.is_finite()
    {
        metadata.insert("APP11:Beta".to_string(), TagValue::Float(beta as f64));
    }

    // Parse Ln0 - lower luminance bound (4 bytes float32 at offset 10)
    if data.len() >= 14
        && let Some(ln0) = reader.f32_at(10)
        && ln0.is_finite()
    {
        metadata.insert("APP11:Ln0".to_string(), TagValue::Float(ln0 as f64));
    }

    // Parse Ln1 - upper luminance bound (4 bytes float32 at offset 14)
    if data.len() >= 18
        && let Some(ln1) = reader.f32_at(14)
        && ln1.is_finite()
    {
        metadata.insert("APP11:Ln1".to_string(), TagValue::Float(ln1 as f64));
    }

    // Parse S2n - signal-to-noise estimate (4 bytes float32 at offset 18)
    if data.len() >= 22
        && let Some(s2n) = reader.f32_at(18)
        && s2n.is_finite()
    {
        metadata.insert("APP11:S2n".to_string(), TagValue::Float(s2n as f64));
    }

    // Check for ratio image data (anything after the 22-byte header)
    if data.len() > HDR_HEADER_SIZE {
        let ratio_image_size = data.len() - HDR_HEADER_SIZE;
        // ExifTool reports this as "(Binary data N bytes, use -b option to extract)"
        metadata.insert(
            "APP11:RatioImage".to_string(),
            TagValue::String(format!(
                "(Binary data {} bytes, use -b option to extract)",
                ratio_image_size
            )),
        );
    }

    Ok(())
}

/// Parses a generic JPEG-HDR parameter segment.
///
/// This handles segments that begin with the "JPEG-HDR" identifier and contain
/// HDR parameters. The structure is the same as HDR_RI but with a different
/// identifier prefix.
///
/// # Format
///
/// After the "JPEG-HDR" identifier (8 bytes), the structure is:
///
/// ```text
/// Offset  Size  Description
/// 0       1     JPEG-HDRVersion
/// 1       1     CorrectionMethod
/// 2       4     Alpha (float32, big-endian)
/// 6       4     Beta (float32, big-endian)
/// 10      4     Ln0 (float32, big-endian)
/// 14      4     Ln1 (float32, big-endian)
/// 18      4     S2n (float32, big-endian)
/// 22      N     RatioImage data (optional)
/// ```
///
/// # Arguments
///
/// * `data` - Segment data after the "JPEG-HDR" identifier
/// * `metadata` - MetadataMap to populate with extracted values
///
/// # ExifTool Compatibility
///
/// Tags use the APP11 family prefix to match ExifTool's output.
fn parse_jpeg_hdr_parameter_segment(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }

    let reader = EndianReader::big_endian(data);

    // Parse JPEG-HDRVersion (1 byte at offset 0)
    if !data.is_empty() {
        let version = reader.u8_at(0).unwrap_or(0);
        metadata.insert(
            "APP11:JPEG-HDRVersion".to_string(),
            TagValue::Integer(version as i64),
        );
    }

    // Parse CorrectionMethod (1 byte at offset 1)
    if data.len() >= 2 {
        let correction_byte = reader.u8_at(1).unwrap_or(0);
        metadata.insert(
            "APP11:CorrectionMethod".to_string(),
            TagValue::Integer(correction_byte as i64),
        );
    }

    // Parse Alpha coefficient (4 bytes float32 at offset 2)
    if data.len() >= 6
        && let Some(alpha) = reader.f32_at(2)
        && alpha.is_finite()
    {
        metadata.insert("APP11:Alpha".to_string(), TagValue::Float(alpha as f64));
    }

    // Parse Beta coefficient (4 bytes float32 at offset 6)
    if data.len() >= 10
        && let Some(beta) = reader.f32_at(6)
        && beta.is_finite()
    {
        metadata.insert("APP11:Beta".to_string(), TagValue::Float(beta as f64));
    }

    // Parse Ln0 - lower luminance bound (4 bytes float32 at offset 10)
    if data.len() >= 14
        && let Some(ln0) = reader.f32_at(10)
        && ln0.is_finite()
    {
        metadata.insert("APP11:Ln0".to_string(), TagValue::Float(ln0 as f64));
    }

    // Parse Ln1 - upper luminance bound (4 bytes float32 at offset 14)
    if data.len() >= 18
        && let Some(ln1) = reader.f32_at(14)
        && ln1.is_finite()
    {
        metadata.insert("APP11:Ln1".to_string(), TagValue::Float(ln1 as f64));
    }

    // Parse S2n - signal-to-noise estimate (4 bytes float32 at offset 18)
    if data.len() >= 22
        && let Some(s2n) = reader.f32_at(18)
        && s2n.is_finite()
    {
        metadata.insert("APP11:S2n".to_string(), TagValue::Float(s2n as f64));
    }

    // Check for ratio image data (anything after the 22-byte header)
    if data.len() > HDR_HEADER_SIZE {
        let ratio_image_size = data.len() - HDR_HEADER_SIZE;
        metadata.insert(
            "APP11:RatioImage".to_string(),
            TagValue::String(format!(
                "(Binary data {} bytes, use -b option to extract)",
                ratio_image_size
            )),
        );
    }

    Ok(())
}

/// Extracts JPEG-HDR parameters into a structured format.
///
/// This is a convenience function that parses the segment data and returns
/// a structured `JpegHdrParameters` object instead of a MetadataMap.
///
/// # Arguments
///
/// * `data` - Raw APP11 segment data
///
/// # Returns
///
/// * `Ok(JpegHdrParameters)` - Structured HDR parameters
/// * `Err` - If parsing fails
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::jpeg::app_segments::app11_jpeg_hdr::extract_hdr_parameters;
///
/// let params = extract_hdr_parameters(segment_data)?;
/// if let Some(version) = params.version {
///     println!("JPEG-HDR version: {}", version);
/// }
/// if params.has_ratio_image {
///     println!("Contains ratio image data");
/// }
/// ```
pub fn extract_hdr_parameters(data: &[u8]) -> Result<JpegHdrParameters> {
    let metadata = parse_app11_jpeg_hdr(data)?;
    let mut params = JpegHdrParameters::default();

    // Extract version (now stored as single integer in APP11:JPEG-HDRVersion)
    if let Some(version) = metadata.get_integer("APP11:JPEG-HDRVersion") {
        params.version = Some((version as u8, 0));
    }

    // Extract floating-point parameters (using APP11 prefix)
    params.alpha = metadata.get_float("APP11:Alpha").map(|v| v as f32);
    params.beta = metadata.get_float("APP11:Beta").map(|v| v as f32);
    params.ln0 = metadata.get_float("APP11:Ln0").map(|v| v as f32);
    params.ln1 = metadata.get_float("APP11:Ln1").map(|v| v as f32);
    params.s2n = metadata.get_float("APP11:S2n").map(|v| v as f32);

    // Extract correction method (now stored as integer)
    if let Some(correction) = metadata.get_integer("APP11:CorrectionMethod") {
        params.correction_method = Some(CorrectionMethod::from_byte(correction as u8));
    }

    // Extract ratio image information
    // Ratio image size can be parsed from the RatioImage string if present
    if let Some(ratio_str) = metadata.get_string("APP11:RatioImage") {
        params.has_ratio_image = true;
        // Parse size from "(Binary data N bytes, use -b option to extract)"
        if let Some(size_start) = ratio_str.find("Binary data ") {
            let size_part = &ratio_str[size_start + 12..];
            if let Some(size_end) = size_part.find(' ')
                && let Ok(size) = size_part[..size_end].parse::<usize>()
            {
                params.ratio_image_size = Some(size);
            }
        }
    } else {
        params.has_ratio_image = false;
    }

    Ok(params)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a test segment with HDR_RI identifier.
    ///
    /// Creates a properly formatted JPEG-HDR segment with the structure:
    /// - HDR_RI identifier (6 bytes)
    /// - Version (1 byte)
    /// - CorrectionMethod (1 byte)
    /// - Alpha (4 bytes float32 big-endian)
    /// - Beta (4 bytes float32 big-endian)
    /// - Ln0 (4 bytes float32 big-endian)
    /// - Ln1 (4 bytes float32 big-endian)
    /// - S2n (4 bytes float32 big-endian)
    fn create_hdr_ri_segment(
        version: u8,
        correction: u8,
        alpha: f32,
        beta: f32,
        ln0: f32,
        ln1: f32,
        s2n: f32,
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(HDR_RI_IDENTIFIER);
        data.push(version);
        data.push(correction);
        data.extend_from_slice(&alpha.to_be_bytes());
        data.extend_from_slice(&beta.to_be_bytes());
        data.extend_from_slice(&ln0.to_be_bytes());
        data.extend_from_slice(&ln1.to_be_bytes());
        data.extend_from_slice(&s2n.to_be_bytes());
        data
    }

    /// Helper to create a test segment with JPEG-HDR identifier.
    fn create_jpeg_hdr_segment(
        version: u8,
        correction: u8,
        alpha: f32,
        beta: f32,
        ln0: f32,
        ln1: f32,
        s2n: f32,
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(JPEG_HDR_IDENTIFIER);
        data.push(version);
        data.push(correction);
        data.extend_from_slice(&alpha.to_be_bytes());
        data.extend_from_slice(&beta.to_be_bytes());
        data.extend_from_slice(&ln0.to_be_bytes());
        data.extend_from_slice(&ln1.to_be_bytes());
        data.extend_from_slice(&s2n.to_be_bytes());
        data
    }

    #[test]
    fn test_parse_hdr_ri_segment_with_full_parameters() {
        // Create a segment matching ExifTool baseline values:
        // Version=11, CorrectionMethod=0, Alpha=1.0, Beta=1.0,
        // Ln0=0.122262, Ln1=2.634655, S2n=2269.635
        let segment = create_hdr_ri_segment(
            11,       // JPEG-HDRVersion
            0,        // CorrectionMethod
            1.0,      // Alpha
            1.0,      // Beta
            0.122262, // Ln0
            2.634655, // Ln1
            2269.635, // S2n
        );

        let result = parse_app11_jpeg_hdr(&segment);
        assert!(result.is_ok(), "Parsing should succeed");

        let metadata = result.unwrap();

        // Verify JPEG-HDRVersion (integer)
        assert_eq!(
            metadata.get_integer("APP11:JPEG-HDRVersion"),
            Some(11),
            "JPEG-HDRVersion should be 11"
        );

        // Verify CorrectionMethod (integer)
        assert_eq!(
            metadata.get_integer("APP11:CorrectionMethod"),
            Some(0),
            "CorrectionMethod should be 0"
        );

        // Verify Alpha (float)
        let alpha = metadata.get_float("APP11:Alpha").unwrap();
        assert!(
            (alpha - 1.0).abs() < 0.001,
            "Alpha should be approximately 1.0"
        );

        // Verify Beta (float)
        let beta = metadata.get_float("APP11:Beta").unwrap();
        assert!(
            (beta - 1.0).abs() < 0.001,
            "Beta should be approximately 1.0"
        );

        // Verify Ln0 (float)
        let ln0 = metadata.get_float("APP11:Ln0").unwrap();
        assert!(
            (ln0 - 0.122262).abs() < 0.001,
            "Ln0 should be approximately 0.122262"
        );

        // Verify Ln1 (float)
        let ln1 = metadata.get_float("APP11:Ln1").unwrap();
        assert!(
            (ln1 - 2.634655).abs() < 0.001,
            "Ln1 should be approximately 2.634655"
        );

        // Verify S2n (float)
        let s2n = metadata.get_float("APP11:S2n").unwrap();
        assert!(
            (s2n - 2269.635).abs() < 1.0,
            "S2n should be approximately 2269.635"
        );

        // No RatioImage without extra data
        assert!(
            metadata.get_string("APP11:RatioImage").is_none(),
            "Should not have ratio image data"
        );
    }

    #[test]
    fn test_parse_hdr_ri_segment_with_ratio_image() {
        // Create segment with parameters and additional ratio image data (19 bytes like ExifTool baseline)
        let mut segment = create_hdr_ri_segment(11, 0, 1.0, 1.0, 0.122262, 2.634655, 2269.635);

        // Add 19 bytes of dummy ratio image data
        segment.extend_from_slice(&[
            0xDE, 0xAD, 0xBE, 0xEF, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x11, 0x22,
            0x33, 0x44, 0x55, 0x66, 0x77,
        ]);

        let result = parse_app11_jpeg_hdr(&segment);
        assert!(result.is_ok());

        let metadata = result.unwrap();

        // Verify RatioImage string format matches ExifTool
        let ratio_image = metadata.get_string("APP11:RatioImage");
        assert!(ratio_image.is_some(), "Should have ratio image data");
        assert!(
            ratio_image.unwrap().contains("19 bytes"),
            "RatioImage should indicate 19 bytes"
        );
    }

    #[test]
    fn test_parse_jpeg_hdr_parameter_segment() {
        let segment = create_jpeg_hdr_segment(
            11,       // Version
            0,        // CorrectionMethod
            1.0,      // Alpha
            1.0,      // Beta
            0.122262, // Ln0
            2.634655, // Ln1
            2269.635, // S2n
        );

        let result = parse_app11_jpeg_hdr(&segment);
        assert!(result.is_ok());

        let metadata = result.unwrap();

        // Verify using APP11 prefix
        assert_eq!(
            metadata.get_integer("APP11:JPEG-HDRVersion"),
            Some(11),
            "JPEG-HDRVersion should be 11"
        );

        assert_eq!(
            metadata.get_integer("APP11:CorrectionMethod"),
            Some(0),
            "CorrectionMethod should be 0"
        );
    }

    #[test]
    fn test_correction_method_parsing() {
        // Test each correction method
        let test_cases = [
            (0u8, "None"),
            (1u8, "Multiplicative"),
            (2u8, "Additive"),
            (3u8, "Logarithmic"),
            (4u8, "Gamma"),
            (99u8, "Unknown (99)"),
        ];

        for (byte_val, expected_desc) in test_cases {
            let method = CorrectionMethod::from_byte(byte_val);
            assert_eq!(
                method.description(),
                expected_desc,
                "CorrectionMethod {} should have description '{}'",
                byte_val,
                expected_desc
            );
        }
    }

    #[test]
    fn test_segment_too_short() {
        let short_data = &[0x48, 0x44, 0x52]; // "HDR" - too short
        let result = parse_app11_jpeg_hdr(short_data);

        assert!(result.is_err(), "Should fail for segment that is too short");
    }

    #[test]
    fn test_unrecognized_identifier() {
        let unknown_segment = b"UNKNOWN_IDENTIFIER_DATA";
        let result = parse_app11_jpeg_hdr(unknown_segment);

        assert!(
            result.is_err(),
            "Should fail for unrecognized segment identifier"
        );
    }

    #[test]
    fn test_minimal_hdr_ri_segment() {
        // Just the identifier and version/correction, no float parameters
        let mut minimal = Vec::new();
        minimal.extend_from_slice(HDR_RI_IDENTIFIER);
        minimal.push(11); // version
        minimal.push(0); // correction method

        let result = parse_app11_jpeg_hdr(&minimal);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(
            metadata.get_integer("APP11:JPEG-HDRVersion"),
            Some(11),
            "JPEG-HDRVersion should be 11"
        );
        assert_eq!(
            metadata.get_integer("APP11:CorrectionMethod"),
            Some(0),
            "CorrectionMethod should be 0"
        );
    }

    #[test]
    fn test_extract_hdr_parameters_structured() {
        // Test the structured parameter extraction
        let segment = create_hdr_ri_segment(
            11,   // Version
            3,    // Logarithmic
            1.2,  // Alpha
            0.95, // Beta
            -4.0, // Ln0
            9.0,  // Ln1
            75.0, // S2n
        );

        let result = extract_hdr_parameters(&segment);
        assert!(result.is_ok());

        let params = result.unwrap();

        assert_eq!(params.version, Some((11, 0)));
        assert_eq!(
            params.correction_method,
            Some(CorrectionMethod::Logarithmic)
        );

        // Check parameters with tolerance
        assert!(params.alpha.is_some());
        assert!((params.alpha.unwrap() - 1.2).abs() < 0.001);

        assert!(params.beta.is_some());
        assert!((params.beta.unwrap() - 0.95).abs() < 0.001);

        assert!(params.ln0.is_some());
        assert!((params.ln0.unwrap() - (-4.0)).abs() < 0.001);

        assert!(params.ln1.is_some());
        assert!((params.ln1.unwrap() - 9.0).abs() < 0.001);

        assert!(params.s2n.is_some());
        assert!((params.s2n.unwrap() - 75.0).abs() < 0.001);

        assert!(!params.has_ratio_image);
    }

    #[test]
    fn test_nan_and_infinity_handling() {
        // Create segment with NaN and infinity values - these should be filtered out
        // Format: HDR_RI + version(1) + correction(1) + Alpha(4) + Beta(4) + Ln0(4) + Ln1(4) + S2n(4)
        let mut segment = Vec::new();
        segment.extend_from_slice(HDR_RI_IDENTIFIER);
        segment.push(1); // version
        segment.push(0); // correction method

        // Add NaN for Alpha (offset 2)
        segment.extend_from_slice(&f32::NAN.to_be_bytes());
        // Add Infinity for Beta (offset 6)
        segment.extend_from_slice(&f32::INFINITY.to_be_bytes());
        // Add valid Ln0 (offset 10)
        segment.extend_from_slice(&1.5f32.to_be_bytes());

        let result = parse_app11_jpeg_hdr(&segment);
        assert!(result.is_ok());

        let metadata = result.unwrap();

        // NaN and Infinity values should NOT be included
        assert!(
            metadata.get_float("APP11:Alpha").is_none(),
            "NaN should be filtered out"
        );
        assert!(
            metadata.get_float("APP11:Beta").is_none(),
            "Infinity should be filtered out"
        );

        // Valid value should be included
        assert!(
            metadata.get_float("APP11:Ln0").is_some(),
            "Valid float should be included"
        );
    }

    #[test]
    fn test_empty_segment_after_identifier() {
        // Just the identifier with no data after
        let segment = HDR_RI_IDENTIFIER;
        let result = parse_app11_jpeg_hdr(segment);
        assert!(result.is_ok());

        // Empty data after identifier produces no tags
        let metadata = result.unwrap();
        assert!(
            metadata.get_integer("APP11:JPEG-HDRVersion").is_none(),
            "No version without data"
        );
    }

    #[test]
    fn test_partial_parameters() {
        // Segment with only some parameters present
        // Format: HDR_RI + version(1) + correction(1) + Alpha(4)
        let mut segment = Vec::new();
        segment.extend_from_slice(HDR_RI_IDENTIFIER);
        segment.push(11); // version
        segment.push(0); // correction method

        // Only Alpha (4 bytes)
        segment.extend_from_slice(&1.5f32.to_be_bytes());

        let result = parse_app11_jpeg_hdr(&segment);
        assert!(result.is_ok());

        let metadata = result.unwrap();

        assert!(
            metadata.get_float("APP11:Alpha").is_some(),
            "Alpha should be present"
        );
        assert!(
            metadata.get_float("APP11:Beta").is_none(),
            "Beta should not be present"
        );
        assert!(
            metadata.get_float("APP11:Ln0").is_none(),
            "Ln0 should not be present"
        );
    }
}
