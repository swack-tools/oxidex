//! Nikon ColorBalance tag parser
//!
//! Parses Nikon MakerNote ColorBalance data structures which contain
//! white balance coefficients and color temperature information.
//!
//! # Overview
//!
//! Nikon cameras store white balance data in the ColorBalance (0x000C) and
//! ColorBalanceA (0x0097) MakerNote tags. This data includes:
//! - Red/Blue white balance multipliers for different lighting conditions
//! - Color temperature values (in Kelvin)
//! - Custom white balance settings
//!
//! # Data Format
//!
//! The ColorBalance data format varies by camera model, but commonly contains:
//! - Version identifier (4 bytes)
//! - WB_RBLevels pairs (red and blue multipliers, each 2 bytes)
//! - Multiple preset WB levels (Auto, Daylight, Cloudy, Shade, etc.)
//! - Color temperature values (2 bytes, in Kelvin)
//!
//! # Supported Tags
//!
//! | Tag Name | Description |
//! |----------|-------------|
//! | WB_RBLevels | Current red/blue white balance multipliers |
//! | WB_RBLevelsAuto | Auto white balance multipliers |
//! | WB_RBLevelsDaylight | Daylight preset multipliers |
//! | WB_RBLevelsCloudy | Cloudy preset multipliers |
//! | WB_RBLevelsShade | Shade preset multipliers |
//! | WB_RBLevelsTungsten | Tungsten/Incandescent preset multipliers |
//! | WB_RBLevelsFluorescent | Fluorescent preset multipliers |
//! | WB_RBLevelsFlash | Flash preset multipliers |
//! | ColorTemperature | Color temperature in Kelvin |
//! | ColorTemperatureAuto | Auto-detected color temperature |
//!
//! # Example
//!
//! ```ignore
//! use oxidex::core::MetadataMap;
//! use oxidex::parsers::tiff::makernotes::nikon::color_balance::parse_nikon_color_balance;
//!
//! // Raw ColorBalance data from MakerNote (example)
//! let data: &[u8] = &[/* color balance bytes */];
//! let is_big_endian = false;
//!
//! let metadata = parse_nikon_color_balance(data, is_big_endian);
//!
//! if let Some(rb_levels) = metadata.get_string("Nikon:WB_RBLevels") {
//!     println!("White Balance R/B: {}", rb_levels);
//! }
//! ```
//!
//! # References
//!
//! - ExifTool Nikon.pm: ColorBalance tag definitions
//! - Nikon NEF specification (unofficial documentation)

use crate::core::metadata_map::MetadataMap;
use crate::core::tag_value::TagValue;

// ============================================================================
// CONSTANTS
// ============================================================================

/// Minimum data size required for ColorBalance parsing (version + at least one RB pair)
const MIN_DATA_SIZE: usize = 8;

/// Size of a single WB_RBLevels pair in bytes (2 x u16)
const RB_PAIR_SIZE: usize = 4;

/// Offset to WB_RBLevels data (after version bytes)
const WB_DATA_OFFSET: usize = 4;

// ColorBalance data offsets for common Nikon format
// Note: These offsets vary by camera model; this implementation handles
// the most common format found in D-series cameras (D80, D90, D200, etc.)

/// Offset for WB_RBLevels (current white balance)
const OFFSET_WB_RBLEVELS: usize = 4;

/// Offset for WB_RBLevelsAuto
const OFFSET_WB_RBLEVELS_AUTO: usize = 8;

/// Offset for WB_RBLevelsDaylight
const OFFSET_WB_RBLEVELS_DAYLIGHT: usize = 12;

/// Offset for WB_RBLevelsCloudy
const OFFSET_WB_RBLEVELS_CLOUDY: usize = 16;

/// Offset for WB_RBLevelsShade
const OFFSET_WB_RBLEVELS_SHADE: usize = 20;

/// Offset for WB_RBLevelsTungsten (Incandescent)
const OFFSET_WB_RBLEVELS_TUNGSTEN: usize = 24;

/// Offset for WB_RBLevelsFluorescent
const OFFSET_WB_RBLEVELS_FLUORESCENT: usize = 28;

/// Offset for WB_RBLevelsFlash
const OFFSET_WB_RBLEVELS_FLASH: usize = 32;

/// Offset for ColorTemperature (if present)
const OFFSET_COLOR_TEMPERATURE: usize = 36;

/// Offset for ColorTemperatureAuto (if present)
const OFFSET_COLOR_TEMPERATURE_AUTO: usize = 38;

// ============================================================================
// BYTE ORDER HELPERS
// ============================================================================

/// Reads a u16 value from a byte slice at the specified offset.
///
/// # Arguments
/// * `data` - The byte slice to read from
/// * `offset` - The byte offset to start reading from
/// * `big_endian` - If true, interpret as big-endian; otherwise little-endian
///
/// # Returns
/// * `Some(u16)` - The parsed value if the offset is valid
/// * `None` - If the offset is out of bounds
#[inline]
fn read_u16(data: &[u8], offset: usize, big_endian: bool) -> Option<u16> {
    // Bounds check: ensure we have at least 2 bytes available
    if offset + 2 > data.len() {
        return None;
    }

    let bytes = [data[offset], data[offset + 1]];

    Some(if big_endian {
        u16::from_be_bytes(bytes)
    } else {
        u16::from_le_bytes(bytes)
    })
}

/// Reads a u32 value from a byte slice at the specified offset.
///
/// # Arguments
/// * `data` - The byte slice to read from
/// * `offset` - The byte offset to start reading from
/// * `big_endian` - If true, interpret as big-endian; otherwise little-endian
///
/// # Returns
/// * `Some(u32)` - The parsed value if the offset is valid
/// * `None` - If the offset is out of bounds
#[inline]
fn read_u32(data: &[u8], offset: usize, big_endian: bool) -> Option<u32> {
    // Bounds check: ensure we have at least 4 bytes available
    if offset + 4 > data.len() {
        return None;
    }

    let bytes = [
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ];

    Some(if big_endian {
        u32::from_be_bytes(bytes)
    } else {
        u32::from_le_bytes(bytes)
    })
}

// ============================================================================
// WHITE BALANCE EXTRACTION
// ============================================================================

/// Extracts a red/blue white balance level pair from the data.
///
/// White balance levels are stored as two consecutive u16 values:
/// - First u16: Red multiplier
/// - Second u16: Blue multiplier
///
/// The values are typically in the range 256-1024, with 256 representing
/// no adjustment and higher values indicating stronger compensation.
///
/// # Arguments
/// * `data` - The byte slice containing ColorBalance data
/// * `offset` - The byte offset to the start of the RB pair
/// * `big_endian` - If true, interpret as big-endian; otherwise little-endian
///
/// # Returns
/// * `Some((red, blue))` - The red and blue multiplier values
/// * `None` - If the data is too short
fn extract_rb_levels(data: &[u8], offset: usize, big_endian: bool) -> Option<(u16, u16)> {
    let red = read_u16(data, offset, big_endian)?;
    let blue = read_u16(data, offset + 2, big_endian)?;
    Some((red, blue))
}

/// Formats an RB levels pair as a human-readable string.
///
/// The output format matches ExifTool's WB_RBLevels format:
/// "red_value blue_value" (space-separated)
///
/// # Arguments
/// * `red` - Red channel multiplier
/// * `blue` - Blue channel multiplier
///
/// # Returns
/// A formatted string like "512 480"
fn format_rb_levels(red: u16, blue: u16) -> String {
    format!("{} {}", red, blue)
}

/// Calculates the normalized white balance multipliers.
///
/// Normalizes the red/blue values relative to a reference (typically 256).
/// This produces floating-point multipliers that can be used for
/// color correction calculations.
///
/// # Arguments
/// * `red` - Raw red channel multiplier
/// * `blue` - Raw blue channel multiplier
///
/// # Returns
/// Tuple of (normalized_red, normalized_blue) as f64 values
#[allow(dead_code)]
fn normalize_rb_levels(red: u16, blue: u16) -> (f64, f64) {
    // Nikon typically uses 256 as the reference point
    const REFERENCE: f64 = 256.0;

    let normalized_red = f64::from(red) / REFERENCE;
    let normalized_blue = f64::from(blue) / REFERENCE;

    (normalized_red, normalized_blue)
}

// ============================================================================
// VERSION DETECTION
// ============================================================================

/// Represents the ColorBalance data format version.
///
/// Different Nikon camera generations use different ColorBalance formats.
/// The version determines the data layout and which fields are available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorBalanceVersion {
    /// Type 1: Early DSLRs (D100, D1X, D1H)
    /// Simpler format with fewer presets
    Type1,

    /// Type 2: Mid-range DSLRs (D70, D80, D90, D200)
    /// Full preset support with color temperature
    Type2,

    /// Type 3: Modern DSLRs and mirrorless (D3, D4, D5, Z series)
    /// Extended format with additional data
    Type3,

    /// Unknown format - parse conservatively
    Unknown,
}

/// Detects the ColorBalance data format version from the version bytes.
///
/// The first 4 bytes of ColorBalance data typically contain a version
/// identifier that indicates the data format.
///
/// # Arguments
/// * `data` - The ColorBalance data bytes
/// * `big_endian` - Byte order for interpreting version bytes
///
/// # Returns
/// The detected `ColorBalanceVersion`
fn detect_version(data: &[u8], big_endian: bool) -> ColorBalanceVersion {
    if data.len() < 4 {
        return ColorBalanceVersion::Unknown;
    }

    // Read version identifier (first 4 bytes)
    let version = read_u32(data, 0, big_endian).unwrap_or(0);

    // Version detection based on common patterns
    // Note: These patterns are derived from analyzing Nikon NEF files
    match version {
        // Type 1: Version bytes typically start with 0x0100 or similar
        0x0100..=0x01FF => ColorBalanceVersion::Type1,

        // Type 2: Common in D80, D90, D200 era cameras
        0x0200..=0x02FF => ColorBalanceVersion::Type2,

        // Type 3: Modern cameras with extended data
        0x0300..=0x04FF => ColorBalanceVersion::Type3,

        // Fallback: Try to detect based on data size
        _ => {
            if data.len() >= 40 {
                ColorBalanceVersion::Type2 // Assume Type2 for reasonable data sizes
            } else if data.len() >= 12 {
                ColorBalanceVersion::Type1
            } else {
                ColorBalanceVersion::Unknown
            }
        }
    }
}

// ============================================================================
// MAIN PARSER
// ============================================================================

/// Parses Nikon ColorBalance data and extracts white balance metadata.
///
/// This function processes raw ColorBalance data from Nikon MakerNotes
/// and extracts white balance coefficients, presets, and color temperature
/// values into a structured `MetadataMap`.
///
/// # Arguments
///
/// * `data` - Raw bytes from the ColorBalance tag (0x000C or 0x0097)
/// * `byte_order` - Byte order flag: `true` for big-endian, `false` for little-endian
///
/// # Returns
///
/// A `MetadataMap` containing the extracted white balance tags. The map will
/// include only those tags that could be successfully parsed from the data.
///
/// # Tag Names
///
/// All tags are prefixed with "Nikon:" to follow the codebase convention:
/// - `Nikon:WB_RBLevels` - Current white balance red/blue multipliers
/// - `Nikon:WB_RBLevelsAuto` - Auto white balance multipliers
/// - `Nikon:WB_RBLevelsDaylight` - Daylight preset multipliers
/// - `Nikon:WB_RBLevelsCloudy` - Cloudy preset multipliers
/// - `Nikon:WB_RBLevelsShade` - Shade preset multipliers
/// - `Nikon:WB_RBLevelsTungsten` - Tungsten (incandescent) preset multipliers
/// - `Nikon:WB_RBLevelsFluorescent` - Fluorescent preset multipliers
/// - `Nikon:WB_RBLevelsFlash` - Flash preset multipliers
/// - `Nikon:ColorTemperature` - Color temperature in Kelvin
/// - `Nikon:ColorTemperatureAuto` - Auto-detected color temperature
/// - `Nikon:ColorBalanceVersion` - Detected format version
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::tiff::makernotes::nikon::color_balance::parse_nikon_color_balance;
///
/// // Sample ColorBalance data (Type 2 format, little-endian)
/// let data = vec![
///     0x00, 0x02, 0x00, 0x00,  // Version: 0x0200 (Type 2)
///     0x00, 0x02, 0xE0, 0x01,  // WB_RBLevels: R=512, B=480
///     0x00, 0x02, 0xE0, 0x01,  // WB_RBLevelsAuto: R=512, B=480
///     // ... additional data
/// ];
///
/// let metadata = parse_nikon_color_balance(&data, false);
/// assert!(metadata.contains_key("Nikon:WB_RBLevels"));
/// ```
///
/// # Notes
///
/// - The parser is defensive and will skip any fields it cannot parse
/// - Zero values for white balance levels are considered invalid and skipped
/// - Color temperature values outside the valid range (1000-25000K) are skipped
pub fn parse_nikon_color_balance(data: &[u8], byte_order: bool) -> MetadataMap {
    let mut metadata = MetadataMap::new();

    // Early return if data is too small to contain meaningful information
    if data.len() < MIN_DATA_SIZE {
        return metadata;
    }

    // Detect format version to determine parsing strategy
    let version = detect_version(data, byte_order);

    // Store version information for debugging/reference
    let version_str = match version {
        ColorBalanceVersion::Type1 => "Type1",
        ColorBalanceVersion::Type2 => "Type2",
        ColorBalanceVersion::Type3 => "Type3",
        ColorBalanceVersion::Unknown => "Unknown",
    };
    metadata.insert(
        "Nikon:ColorBalanceVersion",
        TagValue::new_string(version_str),
    );

    // Parse white balance presets based on detected version
    match version {
        ColorBalanceVersion::Type1 => {
            parse_type1_color_balance(data, byte_order, &mut metadata);
        }
        ColorBalanceVersion::Type2 | ColorBalanceVersion::Type3 => {
            parse_type2_color_balance(data, byte_order, &mut metadata);
        }
        ColorBalanceVersion::Unknown => {
            // For unknown formats, attempt conservative parsing
            // Only extract WB_RBLevels if data appears valid
            parse_minimal_color_balance(data, byte_order, &mut metadata);
        }
    }

    metadata
}

/// Parses Type 1 ColorBalance format (early Nikon DSLRs).
///
/// Type 1 format is simpler and contains fewer presets:
/// - Version (4 bytes)
/// - WB_RBLevels (4 bytes)
/// - WB_RBLevelsAuto (4 bytes) - may not be present
fn parse_type1_color_balance(data: &[u8], big_endian: bool, metadata: &mut MetadataMap) {
    // WB_RBLevels (current white balance)
    if let Some((red, blue)) = extract_rb_levels(data, OFFSET_WB_RBLEVELS, big_endian)
        && is_valid_rb_level(red)
        && is_valid_rb_level(blue)
    {
        metadata.insert(
            "Nikon:WB_RBLevels",
            TagValue::new_string(format_rb_levels(red, blue)),
        );
    }

    // WB_RBLevelsAuto (if data is long enough)
    if data.len() >= OFFSET_WB_RBLEVELS_AUTO + RB_PAIR_SIZE
        && let Some((red, blue)) = extract_rb_levels(data, OFFSET_WB_RBLEVELS_AUTO, big_endian)
        && is_valid_rb_level(red)
        && is_valid_rb_level(blue)
    {
        metadata.insert(
            "Nikon:WB_RBLevelsAuto",
            TagValue::new_string(format_rb_levels(red, blue)),
        );
    }
}

/// Parses Type 2/Type 3 ColorBalance format (modern Nikon cameras).
///
/// Type 2/3 format includes full preset support:
/// - Version (4 bytes)
/// - WB_RBLevels (4 bytes)
/// - Multiple preset RB levels (4 bytes each)
/// - Color temperature values (2 bytes each)
fn parse_type2_color_balance(data: &[u8], big_endian: bool, metadata: &mut MetadataMap) {
    // Define preset offsets and their corresponding tag names
    let presets: &[(usize, &str)] = &[
        (OFFSET_WB_RBLEVELS, "Nikon:WB_RBLevels"),
        (OFFSET_WB_RBLEVELS_AUTO, "Nikon:WB_RBLevelsAuto"),
        (OFFSET_WB_RBLEVELS_DAYLIGHT, "Nikon:WB_RBLevelsDaylight"),
        (OFFSET_WB_RBLEVELS_CLOUDY, "Nikon:WB_RBLevelsCloudy"),
        (OFFSET_WB_RBLEVELS_SHADE, "Nikon:WB_RBLevelsShade"),
        (OFFSET_WB_RBLEVELS_TUNGSTEN, "Nikon:WB_RBLevelsTungsten"),
        (
            OFFSET_WB_RBLEVELS_FLUORESCENT,
            "Nikon:WB_RBLevelsFluorescent",
        ),
        (OFFSET_WB_RBLEVELS_FLASH, "Nikon:WB_RBLevelsFlash"),
    ];

    // Parse each preset that fits within the data
    for (offset, tag_name) in presets {
        if data.len() >= *offset + RB_PAIR_SIZE
            && let Some((red, blue)) = extract_rb_levels(data, *offset, big_endian)
            && is_valid_rb_level(red)
            && is_valid_rb_level(blue)
        {
            metadata.insert(*tag_name, TagValue::new_string(format_rb_levels(red, blue)));
        }
    }

    // Parse color temperature values if present
    if data.len() >= OFFSET_COLOR_TEMPERATURE + 2
        && let Some(temp) = read_u16(data, OFFSET_COLOR_TEMPERATURE, big_endian)
        && is_valid_color_temperature(temp)
    {
        metadata.insert(
            "Nikon:ColorTemperature",
            TagValue::new_integer(i64::from(temp)),
        );
    }

    if data.len() >= OFFSET_COLOR_TEMPERATURE_AUTO + 2
        && let Some(temp) = read_u16(data, OFFSET_COLOR_TEMPERATURE_AUTO, big_endian)
        && is_valid_color_temperature(temp)
    {
        metadata.insert(
            "Nikon:ColorTemperatureAuto",
            TagValue::new_integer(i64::from(temp)),
        );
    }
}

/// Parses ColorBalance data conservatively for unknown formats.
///
/// This function attempts to extract only the most basic white balance
/// data when the format version is not recognized.
fn parse_minimal_color_balance(data: &[u8], big_endian: bool, metadata: &mut MetadataMap) {
    // Only try to extract the primary WB_RBLevels
    if data.len() >= WB_DATA_OFFSET + RB_PAIR_SIZE
        && let Some((red, blue)) = extract_rb_levels(data, WB_DATA_OFFSET, big_endian)
        && is_valid_rb_level(red)
        && is_valid_rb_level(blue)
    {
        metadata.insert(
            "Nikon:WB_RBLevels",
            TagValue::new_string(format_rb_levels(red, blue)),
        );
    }
}

// ============================================================================
// VALIDATION HELPERS
// ============================================================================

/// Validates a white balance red/blue level value.
///
/// Valid values are non-zero and within a reasonable range.
/// Nikon typically uses values in the range 100-2000.
///
/// # Arguments
/// * `value` - The red or blue multiplier value to validate
///
/// # Returns
/// `true` if the value appears to be a valid white balance level
#[inline]
fn is_valid_rb_level(value: u16) -> bool {
    // Zero is never valid (indicates missing/unset data)
    // Very high values (>10000) are likely garbage data
    value > 0 && value < 10000
}

/// Validates a color temperature value in Kelvin.
///
/// Valid color temperatures for photography range from approximately
/// 1000K (candlelight) to 25000K (clear blue sky in shade).
///
/// # Arguments
/// * `temp` - The color temperature value in Kelvin
///
/// # Returns
/// `true` if the value is within the valid color temperature range
#[inline]
fn is_valid_color_temperature(temp: u16) -> bool {
    // Practical photography range: 1000K to 25000K
    // 0 typically indicates "not set" or "auto"
    (1000..=25000).contains(&temp)
}

// ============================================================================
// UNIT TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // BYTE ORDER TESTS
    // ========================================================================

    #[test]
    fn test_read_u16_little_endian() {
        let data = [0x34, 0x12]; // 0x1234 in little-endian
        assert_eq!(read_u16(&data, 0, false), Some(0x1234));
    }

    #[test]
    fn test_read_u16_big_endian() {
        let data = [0x12, 0x34]; // 0x1234 in big-endian
        assert_eq!(read_u16(&data, 0, true), Some(0x1234));
    }

    #[test]
    fn test_read_u16_out_of_bounds() {
        let data = [0x12];
        assert_eq!(read_u16(&data, 0, false), None);
        assert_eq!(read_u16(&data, 1, false), None);
    }

    #[test]
    fn test_read_u32_little_endian() {
        let data = [0x78, 0x56, 0x34, 0x12]; // 0x12345678 in little-endian
        assert_eq!(read_u32(&data, 0, false), Some(0x12345678));
    }

    #[test]
    fn test_read_u32_big_endian() {
        let data = [0x12, 0x34, 0x56, 0x78]; // 0x12345678 in big-endian
        assert_eq!(read_u32(&data, 0, true), Some(0x12345678));
    }

    #[test]
    fn test_read_u32_out_of_bounds() {
        let data = [0x12, 0x34, 0x56];
        assert_eq!(read_u32(&data, 0, false), None);
    }

    // ========================================================================
    // RB LEVELS EXTRACTION TESTS
    // ========================================================================

    #[test]
    fn test_extract_rb_levels_little_endian() {
        // Red=512 (0x0200), Blue=480 (0x01E0)
        let data = [0x00, 0x02, 0xE0, 0x01];
        let result = extract_rb_levels(&data, 0, false);
        assert_eq!(result, Some((512, 480)));
    }

    #[test]
    fn test_extract_rb_levels_big_endian() {
        // Red=512 (0x0200), Blue=480 (0x01E0) in big-endian
        let data = [0x02, 0x00, 0x01, 0xE0];
        let result = extract_rb_levels(&data, 0, true);
        assert_eq!(result, Some((512, 480)));
    }

    #[test]
    fn test_extract_rb_levels_with_offset() {
        let data = [0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xE0, 0x01];
        let result = extract_rb_levels(&data, 4, false);
        assert_eq!(result, Some((512, 480)));
    }

    #[test]
    fn test_extract_rb_levels_insufficient_data() {
        let data = [0x00, 0x02, 0xE0]; // Missing one byte
        assert_eq!(extract_rb_levels(&data, 0, false), None);
    }

    #[test]
    fn test_format_rb_levels() {
        assert_eq!(format_rb_levels(512, 480), "512 480");
        assert_eq!(format_rb_levels(256, 256), "256 256");
        assert_eq!(format_rb_levels(1024, 768), "1024 768");
    }

    // ========================================================================
    // VALIDATION TESTS
    // ========================================================================

    #[test]
    fn test_is_valid_rb_level() {
        // Valid values
        assert!(is_valid_rb_level(256));
        assert!(is_valid_rb_level(512));
        assert!(is_valid_rb_level(1024));
        assert!(is_valid_rb_level(9999));

        // Invalid values
        assert!(!is_valid_rb_level(0));
        assert!(!is_valid_rb_level(10000));
        assert!(!is_valid_rb_level(65535));
    }

    #[test]
    fn test_is_valid_color_temperature() {
        // Valid temperatures
        assert!(is_valid_color_temperature(1000)); // Minimum
        assert!(is_valid_color_temperature(2700)); // Warm white
        assert!(is_valid_color_temperature(5500)); // Daylight
        assert!(is_valid_color_temperature(6500)); // Cloudy
        assert!(is_valid_color_temperature(25000)); // Maximum

        // Invalid temperatures
        assert!(!is_valid_color_temperature(0)); // Unset
        assert!(!is_valid_color_temperature(999)); // Below range
        assert!(!is_valid_color_temperature(25001)); // Above range
    }

    // ========================================================================
    // VERSION DETECTION TESTS
    // ========================================================================

    #[test]
    fn test_detect_version_type1() {
        let data = [0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]; // Version 0x0100
        assert_eq!(detect_version(&data, false), ColorBalanceVersion::Type1);
    }

    #[test]
    fn test_detect_version_type2() {
        let data = [0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]; // Version 0x0200
        assert_eq!(detect_version(&data, false), ColorBalanceVersion::Type2);
    }

    #[test]
    fn test_detect_version_type3() {
        let data = [0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]; // Version 0x0300
        assert_eq!(detect_version(&data, false), ColorBalanceVersion::Type3);
    }

    #[test]
    fn test_detect_version_unknown() {
        let data = [0x00, 0x00]; // Too short
        assert_eq!(detect_version(&data, false), ColorBalanceVersion::Unknown);
    }

    #[test]
    fn test_detect_version_fallback_by_size() {
        // Unknown version but large enough data should default to Type2
        let data = vec![0xFF; 50];
        assert_eq!(detect_version(&data, false), ColorBalanceVersion::Type2);
    }

    // ========================================================================
    // MAIN PARSER TESTS
    // ========================================================================

    #[test]
    fn test_parse_empty_data() {
        let data: &[u8] = &[];
        let result = parse_nikon_color_balance(data, false);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_too_small_data() {
        let data = [0x00, 0x01, 0x00]; // Less than MIN_DATA_SIZE
        let result = parse_nikon_color_balance(&data, false);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_type2_full_data() {
        // Construct Type 2 ColorBalance data
        let mut data = vec![0u8; 40];

        // Version: 0x0200 (Type 2) - little-endian
        data[0] = 0x00;
        data[1] = 0x02;
        data[2] = 0x00;
        data[3] = 0x00;

        // WB_RBLevels at offset 4: R=512, B=480
        data[4] = 0x00;
        data[5] = 0x02;
        data[6] = 0xE0;
        data[7] = 0x01;

        // WB_RBLevelsAuto at offset 8: R=520, B=490
        data[8] = 0x08;
        data[9] = 0x02;
        data[10] = 0xEA;
        data[11] = 0x01;

        // WB_RBLevelsDaylight at offset 12: R=500, B=470
        data[12] = 0xF4;
        data[13] = 0x01;
        data[14] = 0xD6;
        data[15] = 0x01;

        // ColorTemperature at offset 36: 5500K
        data[36] = 0x7C;
        data[37] = 0x15;

        let result = parse_nikon_color_balance(&data, false);

        assert_eq!(
            result.get_string("Nikon:ColorBalanceVersion"),
            Some("Type2")
        );
        assert_eq!(result.get_string("Nikon:WB_RBLevels"), Some("512 480"));
        assert_eq!(result.get_string("Nikon:WB_RBLevelsAuto"), Some("520 490"));
        assert_eq!(
            result.get_string("Nikon:WB_RBLevelsDaylight"),
            Some("500 470")
        );
        assert_eq!(result.get_integer("Nikon:ColorTemperature"), Some(5500));
    }

    #[test]
    fn test_parse_type1_data() {
        // Construct Type 1 ColorBalance data
        let mut data = vec![0u8; 12];

        // Version: 0x0100 (Type 1) - little-endian
        data[0] = 0x00;
        data[1] = 0x01;
        data[2] = 0x00;
        data[3] = 0x00;

        // WB_RBLevels at offset 4: R=256, B=256
        data[4] = 0x00;
        data[5] = 0x01;
        data[6] = 0x00;
        data[7] = 0x01;

        // WB_RBLevelsAuto at offset 8: R=260, B=252
        data[8] = 0x04;
        data[9] = 0x01;
        data[10] = 0xFC;
        data[11] = 0x00;

        let result = parse_nikon_color_balance(&data, false);

        assert_eq!(
            result.get_string("Nikon:ColorBalanceVersion"),
            Some("Type1")
        );
        assert_eq!(result.get_string("Nikon:WB_RBLevels"), Some("256 256"));
        assert_eq!(result.get_string("Nikon:WB_RBLevelsAuto"), Some("260 252"));
    }

    #[test]
    fn test_parse_big_endian_data() {
        // Construct Type 2 ColorBalance data in big-endian
        let mut data = vec![0u8; 12];

        // Version: 0x0200 (Type 2) - big-endian
        data[0] = 0x00;
        data[1] = 0x00;
        data[2] = 0x02;
        data[3] = 0x00;

        // WB_RBLevels at offset 4: R=512 (0x0200), B=480 (0x01E0) - big-endian
        data[4] = 0x02;
        data[5] = 0x00;
        data[6] = 0x01;
        data[7] = 0xE0;

        let result = parse_nikon_color_balance(&data, true);

        assert_eq!(result.get_string("Nikon:WB_RBLevels"), Some("512 480"));
    }

    #[test]
    fn test_parse_skips_zero_rb_levels() {
        let mut data = vec![0u8; 12];

        // Version: 0x0100 (Type 1)
        data[0] = 0x00;
        data[1] = 0x01;
        data[2] = 0x00;
        data[3] = 0x00;

        // WB_RBLevels: R=0, B=0 (invalid - should be skipped)
        data[4] = 0x00;
        data[5] = 0x00;
        data[6] = 0x00;
        data[7] = 0x00;

        let result = parse_nikon_color_balance(&data, false);

        // Should have version but not WB_RBLevels
        assert!(result.contains_key("Nikon:ColorBalanceVersion"));
        assert!(!result.contains_key("Nikon:WB_RBLevels"));
    }

    #[test]
    fn test_parse_skips_invalid_color_temperature() {
        let mut data = vec![0u8; 40];

        // Version: 0x0200 (Type 2)
        data[0] = 0x00;
        data[1] = 0x02;
        data[2] = 0x00;
        data[3] = 0x00;

        // Valid WB_RBLevels
        data[4] = 0x00;
        data[5] = 0x02;
        data[6] = 0xE0;
        data[7] = 0x01;

        // Invalid ColorTemperature: 0 (below minimum)
        data[36] = 0x00;
        data[37] = 0x00;

        let result = parse_nikon_color_balance(&data, false);

        assert!(result.contains_key("Nikon:WB_RBLevels"));
        assert!(!result.contains_key("Nikon:ColorTemperature"));
    }

    #[test]
    fn test_parse_all_presets() {
        // Test that all 8 presets can be parsed
        let mut data = vec![0u8; 40];

        // Version: 0x0200 (Type 2)
        data[1] = 0x02;

        // Set different values for each preset to verify correct parsing
        let preset_values: &[(usize, u16, u16)] = &[
            (4, 512, 480),  // WB_RBLevels
            (8, 520, 490),  // WB_RBLevelsAuto
            (12, 500, 470), // WB_RBLevelsDaylight
            (16, 530, 500), // WB_RBLevelsCloudy
            (20, 540, 510), // WB_RBLevelsShade
            (24, 450, 550), // WB_RBLevelsTungsten
            (28, 470, 530), // WB_RBLevelsFluorescent
            (32, 515, 485), // WB_RBLevelsFlash
        ];

        for (offset, red, blue) in preset_values {
            let red_bytes = red.to_le_bytes();
            let blue_bytes = blue.to_le_bytes();
            data[*offset] = red_bytes[0];
            data[*offset + 1] = red_bytes[1];
            data[*offset + 2] = blue_bytes[0];
            data[*offset + 3] = blue_bytes[1];
        }

        let result = parse_nikon_color_balance(&data, false);

        assert_eq!(result.get_string("Nikon:WB_RBLevels"), Some("512 480"));
        assert_eq!(result.get_string("Nikon:WB_RBLevelsAuto"), Some("520 490"));
        assert_eq!(
            result.get_string("Nikon:WB_RBLevelsDaylight"),
            Some("500 470")
        );
        assert_eq!(
            result.get_string("Nikon:WB_RBLevelsCloudy"),
            Some("530 500")
        );
        assert_eq!(result.get_string("Nikon:WB_RBLevelsShade"), Some("540 510"));
        assert_eq!(
            result.get_string("Nikon:WB_RBLevelsTungsten"),
            Some("450 550")
        );
        assert_eq!(
            result.get_string("Nikon:WB_RBLevelsFluorescent"),
            Some("470 530")
        );
        assert_eq!(result.get_string("Nikon:WB_RBLevelsFlash"), Some("515 485"));
    }

    // ========================================================================
    // NORMALIZATION TESTS
    // ========================================================================

    #[test]
    fn test_normalize_rb_levels() {
        let (red, blue) = normalize_rb_levels(512, 480);

        // 512 / 256 = 2.0
        assert!((red - 2.0).abs() < f64::EPSILON);

        // 480 / 256 = 1.875
        assert!((blue - 1.875).abs() < f64::EPSILON);
    }

    #[test]
    fn test_normalize_rb_levels_unity() {
        let (red, blue) = normalize_rb_levels(256, 256);

        // Both should be 1.0 (no adjustment)
        assert!((red - 1.0).abs() < f64::EPSILON);
        assert!((blue - 1.0).abs() < f64::EPSILON);
    }
}
