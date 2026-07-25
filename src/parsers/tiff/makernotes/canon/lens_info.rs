//! Canon LensInfo tag parser
//!
//! This module parses Canon MakerNote lens information tags, extracting detailed
//! lens metadata including type, model, serial number, and optical specifications.
//!
//! ## Supported Tags
//!
//! - **LensType**: Numeric ID mapped to lens name via Canon lens database
//! - **LensModel**: String identifier for the lens model
//! - **LensSerialNumber**: Lens serial number (when available)
//! - **MinFocalLength**: Minimum focal length in millimeters
//! - **MaxFocalLength**: Maximum focal length in millimeters
//! - **MinAperture**: Minimum aperture (largest f-number)
//! - **MaxAperture**: Maximum aperture (smallest f-number)
//!
//! ## Data Format
//!
//! Canon stores lens information in MakerNote sub-IFDs and array structures.
//! The LensType field uses a numeric ID that maps to lens names via the
//! Canon lens database (see `lens_data::canon`).
//!
//! ## Example
//!
//! ```ignore
//! use oxidex::parsers::tiff::makernotes::canon::lens_info::parse_canon_lens_info;
//! use oxidex::core::MetadataMap;
//!
//! let lens_data: &[u8] = &[/* raw lens info bytes */];
//! let byte_order = true; // true = little-endian, false = big-endian
//!
//! let metadata = parse_canon_lens_info(lens_data, byte_order);
//! if let Some(lens_type) = metadata.get_string("Canon:LensType") {
//!     println!("Lens: {}", lens_type);
//! }
//! ```

use crate::core::metadata_map::MetadataMap;
use crate::core::tag_value::TagValue;

// Re-export the Canon lens database lookup function
use super::super::lens_data::canon;

// ============================================================================
// CONSTANTS
// ============================================================================

/// Minimum valid data length for lens info parsing (in bytes).
/// Canon lens info requires at least enough bytes to read the lens type.
const MIN_LENS_INFO_LENGTH: usize = 2;

/// Offset for LensType in Canon LensInfo structure (typically at offset 0).
const LENS_TYPE_OFFSET: usize = 0;

/// Offset for MinFocalLength in Canon LensInfo structure (typically at offset 2).
const MIN_FOCAL_LENGTH_OFFSET: usize = 2;

/// Offset for MaxFocalLength in Canon LensInfo structure (typically at offset 4).
const MAX_FOCAL_LENGTH_OFFSET: usize = 4;

/// Offset for MinAperture (largest f-number) in Canon LensInfo structure.
/// Canon stores aperture as APEX value * 32.
const MIN_APERTURE_OFFSET: usize = 6;

/// Offset for MaxAperture (smallest f-number) in Canon LensInfo structure.
/// Canon stores aperture as APEX value * 32.
const MAX_APERTURE_OFFSET: usize = 8;

/// Offset for LensModel string in extended Canon LensInfo structure.
const LENS_MODEL_OFFSET: usize = 10;

/// Maximum length for lens model string to prevent buffer overruns.
const MAX_LENS_MODEL_LENGTH: usize = 64;

/// Offset for LensSerialNumber in extended Canon LensInfo structure.
/// Note: Serial number location varies by camera model; this is a common offset.
const LENS_SERIAL_NUMBER_OFFSET: usize = 74;

/// Maximum length for lens serial number string.
const MAX_SERIAL_NUMBER_LENGTH: usize = 16;

// ============================================================================
// BYTE READING UTILITIES
// ============================================================================

/// Reads a 16-bit unsigned integer from a byte slice at the specified offset.
///
/// # Arguments
/// * `data` - Source byte slice
/// * `offset` - Byte offset to read from
/// * `little_endian` - If true, read as little-endian; otherwise big-endian
///
/// # Returns
/// The parsed u16 value, or None if the offset is out of bounds
#[inline]
fn read_u16(data: &[u8], offset: usize, little_endian: bool) -> Option<u16> {
    if offset + 2 > data.len() {
        return None;
    }

    let bytes = [data[offset], data[offset + 1]];
    if little_endian {
        Some(u16::from_le_bytes(bytes))
    } else {
        Some(u16::from_be_bytes(bytes))
    }
}

/// Reads a null-terminated ASCII string from a byte slice.
///
/// # Arguments
/// * `data` - Source byte slice
/// * `offset` - Starting byte offset
/// * `max_length` - Maximum number of bytes to read
///
/// # Returns
/// The parsed string, or None if the offset is out of bounds or string is empty
fn read_ascii_string(data: &[u8], offset: usize, max_length: usize) -> Option<String> {
    if offset >= data.len() {
        return None;
    }

    let end = std::cmp::min(offset + max_length, data.len());
    let slice = &data[offset..end];

    // Find null terminator or end of slice
    let string_end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());

    if string_end == 0 {
        return None;
    }

    // Convert to string, filtering out non-printable characters
    let string: String = slice[..string_end]
        .iter()
        .filter(|&&b| (0x20..0x7F).contains(&b)) // Printable ASCII range
        .map(|&b| b as char)
        .collect();

    if string.is_empty() {
        None
    } else {
        Some(string.trim().to_string())
    }
}

// ============================================================================
// APERTURE CONVERSION
// ============================================================================

/// Converts Canon's APEX aperture value to an f-number.
///
/// Canon stores aperture values as APEX (Additive System of Photographic Exposure)
/// values multiplied by 32. The formula to convert to f-number is:
///
/// f-number = sqrt(2) ^ (APEX / 32)
///
/// # Arguments
/// * `apex_value` - Canon's raw aperture value (APEX * 32)
///
/// # Returns
/// The f-number as a floating-point value, or None if the value is invalid
fn apex_to_fnumber(apex_value: u16) -> Option<f64> {
    if apex_value == 0 || apex_value == 0xFFFF {
        return None;
    }

    // APEX value is stored as actual_apex * 32
    let apex = apex_value as f64 / 32.0;

    // f-number = 2^(APEX/2) = sqrt(2)^APEX
    let f_number = 2.0_f64.powf(apex / 2.0);

    // Sanity check: f-numbers should be in reasonable range (f/0.7 to f/90)
    if !(0.7..=90.0).contains(&f_number) {
        return None;
    }

    Some(f_number)
}

/// Formats an f-number for display with appropriate precision.
///
/// # Arguments
/// * `f_number` - The f-number value
///
/// # Returns
/// A formatted string like "f/2.8" or "f/22"
fn format_fnumber(f_number: f64) -> String {
    // Use one decimal place if needed, otherwise whole number
    if (f_number - f_number.round()).abs() < 0.05 {
        format!("f/{:.0}", f_number)
    } else {
        format!("f/{:.1}", f_number)
    }
}

// ============================================================================
// LENS TYPE LOOKUP
// ============================================================================

/// Looks up the lens name from a Canon lens type ID.
///
/// This function queries the Canon lens database to find the human-readable
/// name for a given lens type ID.
///
/// # Arguments
/// * `lens_type_id` - Canon's numeric lens type identifier
///
/// # Returns
/// The lens name if found in the database, or a formatted ID string otherwise
fn lookup_lens_type(lens_type_id: u16) -> String {
    // Check for special values that indicate no lens or unknown
    if lens_type_id == 0 || lens_type_id == 0xFFFF {
        return "Unknown".to_string();
    }

    // Look up in Canon lens database
    match canon::lookup(lens_type_id) {
        Some(name) => name.to_string(),
        None => {
            // Return numeric ID if not found in database
            // This helps with identifying new/unlisted lenses
            format!("Unknown (ID: {})", lens_type_id)
        }
    }
}

// ============================================================================
// MAIN PARSER FUNCTION
// ============================================================================

/// Parses Canon LensInfo data and extracts lens metadata.
///
/// This function takes raw Canon MakerNote lens information bytes and extracts
/// all available lens metadata including type, model, focal length range,
/// aperture range, and serial number.
///
/// # Arguments
/// * `data` - Raw byte slice containing Canon LensInfo data
/// * `byte_order` - Byte order: `true` for little-endian, `false` for big-endian
///
/// # Returns
/// A `MetadataMap` containing extracted lens tags with "Canon:" prefix.
/// Tags are only included if they contain valid, non-empty values.
///
/// # Tag Names
/// - `Canon:LensType` - Human-readable lens name or ID
/// - `Canon:LensModel` - Lens model string (if present)
/// - `Canon:LensSerialNumber` - Lens serial number (if present)
/// - `Canon:MinFocalLength` - Minimum focal length in mm
/// - `Canon:MaxFocalLength` - Maximum focal length in mm
/// - `Canon:MinAperture` - Minimum aperture (largest f-number)
/// - `Canon:MaxAperture` - Maximum aperture (smallest f-number)
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::tiff::makernotes::canon::lens_info::parse_canon_lens_info;
///
/// // Sample Canon LensInfo data (little-endian)
/// let data = vec![
///     0x01, 0x00,  // LensType: 1 (Canon EF 50mm f/1.8)
///     0x32, 0x00,  // MinFocalLength: 50mm
///     0x32, 0x00,  // MaxFocalLength: 50mm
///     0x30, 0x00,  // MinAperture: f/22 (APEX)
///     0x10, 0x00,  // MaxAperture: f/1.8 (APEX)
/// ];
///
/// let metadata = parse_canon_lens_info(&data, true);
/// assert!(metadata.contains_key("Canon:LensType"));
/// ```
pub fn parse_canon_lens_info(data: &[u8], byte_order: bool) -> MetadataMap {
    let mut metadata = MetadataMap::new();

    // Validate minimum data length
    if data.len() < MIN_LENS_INFO_LENGTH {
        return metadata;
    }

    // Parse LensType (offset 0)
    if let Some(lens_type_id) = read_u16(data, LENS_TYPE_OFFSET, byte_order) {
        let lens_name = lookup_lens_type(lens_type_id);
        metadata.insert("Canon:LensType", TagValue::new_string(lens_name));

        // Also store the raw lens ID for reference
        if lens_type_id != 0 && lens_type_id != 0xFFFF {
            metadata.insert(
                "Canon:LensTypeID",
                TagValue::new_integer(lens_type_id as i64),
            );
        }
    }

    // Parse MinFocalLength (offset 2)
    if let Some(min_focal) = read_u16(data, MIN_FOCAL_LENGTH_OFFSET, byte_order)
        && min_focal > 0
        && min_focal != 0xFFFF
    {
        metadata.insert(
            "Canon:MinFocalLength",
            TagValue::new_string(format!("{} mm", min_focal)),
        );
    }

    // Parse MaxFocalLength (offset 4)
    if let Some(max_focal) = read_u16(data, MAX_FOCAL_LENGTH_OFFSET, byte_order)
        && max_focal > 0
        && max_focal != 0xFFFF
    {
        metadata.insert(
            "Canon:MaxFocalLength",
            TagValue::new_string(format!("{} mm", max_focal)),
        );
    }

    // Parse MinAperture (offset 6) - largest f-number, smallest opening
    if let Some(min_aperture_raw) = read_u16(data, MIN_APERTURE_OFFSET, byte_order)
        && let Some(f_number) = apex_to_fnumber(min_aperture_raw)
    {
        metadata.insert(
            "Canon:MinAperture",
            TagValue::new_string(format_fnumber(f_number)),
        );
    }

    // Parse MaxAperture (offset 8) - smallest f-number, largest opening
    if let Some(max_aperture_raw) = read_u16(data, MAX_APERTURE_OFFSET, byte_order)
        && let Some(f_number) = apex_to_fnumber(max_aperture_raw)
    {
        metadata.insert(
            "Canon:MaxAperture",
            TagValue::new_string(format_fnumber(f_number)),
        );
    }

    // Parse LensModel string (offset 10, variable length)
    // Only attempt if data is long enough
    if data.len() > LENS_MODEL_OFFSET
        && let Some(lens_model) = read_ascii_string(data, LENS_MODEL_OFFSET, MAX_LENS_MODEL_LENGTH)
    {
        metadata.insert("Canon:LensModel", TagValue::new_string(lens_model));
    }

    // Parse LensSerialNumber (offset 74, variable length)
    // Only attempt if data is long enough
    if data.len() > LENS_SERIAL_NUMBER_OFFSET
        && let Some(serial) =
            read_ascii_string(data, LENS_SERIAL_NUMBER_OFFSET, MAX_SERIAL_NUMBER_LENGTH)
    {
        // Validate serial number format (should be alphanumeric)
        if serial.chars().all(|c| c.is_alphanumeric() || c == '-') {
            metadata.insert("Canon:LensSerialNumber", TagValue::new_string(serial));
        }
    }

    metadata
}

// ============================================================================
// UNIT TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Test parsing with minimal valid data (just LensType)
    #[test]
    fn test_parse_minimal_lens_info() {
        // LensType = 1 (Canon EF 50mm f/1.8)
        let data = vec![0x01, 0x00];
        let metadata = parse_canon_lens_info(&data, true);

        assert!(metadata.contains_key("Canon:LensType"));
        assert_eq!(
            metadata.get_string("Canon:LensType"),
            Some("Canon EF 50mm f/1.8")
        );
    }

    /// Test parsing with empty/insufficient data
    #[test]
    fn test_parse_empty_data() {
        let data: Vec<u8> = vec![];
        let metadata = parse_canon_lens_info(&data, true);
        assert!(metadata.is_empty());

        let data = vec![0x01]; // Only 1 byte, need at least 2
        let metadata = parse_canon_lens_info(&data, true);
        assert!(metadata.is_empty());
    }

    /// Test parsing with complete lens info structure
    #[test]
    fn test_parse_complete_lens_info() {
        // Complete structure: type, min_focal, max_focal, min_aperture, max_aperture
        let mut data = vec![
            0x30, 0x00, // LensType: 48 (Canon EF 50mm f/1.8 II)
            0x32, 0x00, // MinFocalLength: 50mm
            0x32, 0x00, // MaxFocalLength: 50mm
            0xA0, 0x00, // MinAperture: APEX * 32 = 160, roughly f/22
            0x30, 0x00, // MaxAperture: APEX * 32 = 48, roughly f/1.8
        ];

        // Pad to lens model offset and add a model string
        while data.len() < LENS_MODEL_OFFSET {
            data.push(0x00);
        }
        data.extend_from_slice(b"EF50mm f/1.8 II\0");

        let metadata = parse_canon_lens_info(&data, true);

        assert!(metadata.contains_key("Canon:LensType"));
        assert!(metadata.contains_key("Canon:MinFocalLength"));
        assert!(metadata.contains_key("Canon:MaxFocalLength"));
        assert!(metadata.contains_key("Canon:LensModel"));

        assert_eq!(metadata.get_string("Canon:MinFocalLength"), Some("50 mm"));
        assert_eq!(metadata.get_string("Canon:MaxFocalLength"), Some("50 mm"));
        assert_eq!(
            metadata.get_string("Canon:LensModel"),
            Some("EF50mm f/1.8 II")
        );
    }

    /// Test parsing with big-endian byte order
    #[test]
    fn test_parse_big_endian() {
        // LensType = 1, big-endian
        let data = vec![0x00, 0x01, 0x00, 0x32, 0x00, 0x32];
        let metadata = parse_canon_lens_info(&data, false);

        assert!(metadata.contains_key("Canon:LensType"));
        assert_eq!(metadata.get_string("Canon:MinFocalLength"), Some("50 mm"));
    }

    /// Test unknown lens type ID
    #[test]
    fn test_unknown_lens_type() {
        // Use a lens ID that's unlikely to be in the database
        let data = vec![0xFE, 0xFF]; // 65534 - very unlikely to be a real lens
        let metadata = parse_canon_lens_info(&data, true);

        assert!(metadata.contains_key("Canon:LensType"));
        // Should contain "Unknown" in the lens type string
        let lens_type = metadata.get_string("Canon:LensType").unwrap();
        assert!(lens_type.contains("Unknown"));
    }

    /// Test zero lens type (no lens attached)
    #[test]
    fn test_zero_lens_type() {
        let data = vec![0x00, 0x00];
        let metadata = parse_canon_lens_info(&data, true);

        assert!(metadata.contains_key("Canon:LensType"));
        assert_eq!(metadata.get_string("Canon:LensType"), Some("Unknown"));
    }

    /// Test invalid aperture values are filtered
    #[test]
    fn test_invalid_aperture_filtered() {
        let data = vec![
            0x01, 0x00, // LensType
            0x32, 0x00, // MinFocalLength: 50mm
            0x32, 0x00, // MaxFocalLength: 50mm
            0xFF, 0xFF, // MinAperture: Invalid (0xFFFF)
            0x00, 0x00, // MaxAperture: Invalid (0)
        ];

        let metadata = parse_canon_lens_info(&data, true);

        // Aperture tags should not be present due to invalid values
        assert!(!metadata.contains_key("Canon:MinAperture"));
        assert!(!metadata.contains_key("Canon:MaxAperture"));
    }

    /// Test APEX to f-number conversion
    #[test]
    fn test_apex_conversion() {
        // f/1.4 = APEX 1, stored as 32
        assert!(apex_to_fnumber(32).is_some());
        let f14 = apex_to_fnumber(32).unwrap();
        assert!((f14 - 1.41).abs() < 0.1);

        // f/2.8 = APEX 3, stored as 96
        let f28 = apex_to_fnumber(96).unwrap();
        assert!((f28 - 2.83).abs() < 0.1);

        // f/5.6 = APEX 5, stored as 160
        let f56 = apex_to_fnumber(160).unwrap();
        assert!((f56 - 5.66).abs() < 0.1);

        // Invalid values
        assert!(apex_to_fnumber(0).is_none());
        assert!(apex_to_fnumber(0xFFFF).is_none());
    }

    /// Test f-number formatting
    #[test]
    fn test_fnumber_formatting() {
        assert_eq!(format_fnumber(2.8), "f/2.8");
        assert_eq!(format_fnumber(4.0), "f/4");
        assert_eq!(format_fnumber(5.6), "f/5.6");
        assert_eq!(format_fnumber(22.0), "f/22");
    }

    /// Test lens serial number parsing with valid serial
    #[test]
    fn test_lens_serial_number() {
        let mut data = vec![
            0x01, 0x00, // LensType
            0x32, 0x00, // MinFocalLength
            0x32, 0x00, // MaxFocalLength
            0x30, 0x00, // MinAperture
            0x30, 0x00, // MaxAperture
        ];

        // Pad to serial number offset
        while data.len() < LENS_SERIAL_NUMBER_OFFSET {
            data.push(0x00);
        }
        data.extend_from_slice(b"1234567890\0");

        let metadata = parse_canon_lens_info(&data, true);
        assert_eq!(
            metadata.get_string("Canon:LensSerialNumber"),
            Some("1234567890")
        );
    }

    /// Test that non-alphanumeric serial numbers are filtered
    #[test]
    fn test_invalid_serial_number_filtered() {
        let mut data = vec![0x01, 0x00];

        // Pad to serial number offset
        while data.len() < LENS_SERIAL_NUMBER_OFFSET {
            data.push(0x00);
        }
        // Invalid serial with special characters
        data.extend_from_slice(b"ABC@#$%\0");

        let metadata = parse_canon_lens_info(&data, true);
        assert!(!metadata.contains_key("Canon:LensSerialNumber"));
    }

    /// Test reading ASCII strings with non-printable characters
    #[test]
    fn test_read_ascii_string_filters_nonprintable() {
        let data = vec![0x41, 0x42, 0x01, 0x43, 0x00]; // "AB<non-print>C\0"
        let result = read_ascii_string(&data, 0, 10);
        assert_eq!(result, Some("ABC".to_string()));
    }

    /// Test reading ASCII string at invalid offset
    #[test]
    fn test_read_ascii_string_invalid_offset() {
        let data = vec![0x41, 0x42, 0x43];
        let result = read_ascii_string(&data, 10, 5);
        assert!(result.is_none());
    }

    /// Test known Canon lens IDs from database
    #[test]
    fn test_known_lens_ids() {
        // Test several known Canon lens IDs
        assert_eq!(lookup_lens_type(1), "Canon EF 50mm f/1.8");
        assert_eq!(lookup_lens_type(48), "Canon EF 50mm f/1.8 II");

        // Unknown should indicate it's unknown
        let unknown = lookup_lens_type(65000);
        assert!(unknown.contains("Unknown"));
    }

    /// Test LensTypeID is stored for valid lens types
    #[test]
    fn test_lens_type_id_stored() {
        let data = vec![0x30, 0x00]; // LensType = 48
        let metadata = parse_canon_lens_info(&data, true);

        assert!(metadata.contains_key("Canon:LensTypeID"));
        assert_eq!(metadata.get_integer("Canon:LensTypeID"), Some(48));
    }

    /// Test LensTypeID is not stored for invalid values
    #[test]
    fn test_lens_type_id_not_stored_for_invalid() {
        let data = vec![0x00, 0x00]; // LensType = 0
        let metadata = parse_canon_lens_info(&data, true);

        assert!(!metadata.contains_key("Canon:LensTypeID"));

        let data = vec![0xFF, 0xFF]; // LensType = 0xFFFF
        let metadata = parse_canon_lens_info(&data, true);

        assert!(!metadata.contains_key("Canon:LensTypeID"));
    }

    /// Test parsing zoom lens (different min/max focal lengths)
    #[test]
    fn test_zoom_lens_focal_lengths() {
        let data = vec![
            0x29, 0x00, // LensType: 41 (Canon EF 100-400mm f/4.5-5.6L IS USM)
            0x64, 0x00, // MinFocalLength: 100mm
            0x90, 0x01, // MaxFocalLength: 400mm
            0x00, 0x00, // MinAperture (placeholder)
            0x00, 0x00, // MaxAperture (placeholder)
        ];

        let metadata = parse_canon_lens_info(&data, true);

        assert_eq!(metadata.get_string("Canon:MinFocalLength"), Some("100 mm"));
        assert_eq!(metadata.get_string("Canon:MaxFocalLength"), Some("400 mm"));
    }
}
