//! Canon ColorData tag parser
//!
//! This module parses Canon-specific ColorData MakerNote tags containing
//! white balance and color calibration information. ColorData is embedded
//! within Canon MakerNotes and contains critical color temperature values
//! used for accurate color reproduction.
//!
//! # Overview
//!
//! Canon cameras store color calibration data in a binary blob within the
//! MakerNotes. This data includes:
//!
//! - Color temperatures for various lighting conditions (AsShot, Daylight, Shade, etc.)
//! - White balance shift values for amber-blue (AB) and green-magenta (GM) axes
//!
//! # Data Structure
//!
//! The ColorData structure varies by camera model, but typically contains:
//! - Header bytes with version information
//! - Array of signed 16-bit color temperature values
//! - White balance shift parameters
//!
//! # Supported Tags
//!
//! | Offset | Tag Name | Description |
//! |--------|----------|-------------|
//! | 63-64 | ColorTempAsShot | Color temperature as shot |
//! | 65-66 | ColorTempAuto | Auto white balance temperature |
//! | 79-80 | ColorTempDaylight | Daylight preset temperature |
//! | 83-84 | ColorTempShade | Shade preset temperature |
//! | 87-88 | ColorTempCloudy | Cloudy preset temperature |
//! | 91-92 | ColorTempTungsten | Tungsten preset temperature |
//! | 95-96 | ColorTempFluorescent | Fluorescent preset temperature |
//! | 99-100 | ColorTempFlash | Flash preset temperature |
//! | 186-187 | WBShiftAB | White balance shift amber-blue |
//! | 188-189 | WBShiftGM | White balance shift green-magenta |
//!
//! # Example
//!
//! ```ignore
//! use oxidex::parsers::tiff::makernotes::canon::color_data::parse_canon_color_data;
//!
//! let color_data: &[u8] = /* ... */;
//! let is_little_endian = true;
//! let metadata = parse_canon_color_data(color_data, is_little_endian);
//!
//! if let Some(temp) = metadata.get_integer("Canon:ColorTempAsShot") {
//!     println!("As-shot color temperature: {}K", temp);
//! }
//! ```

use crate::core::metadata_map::MetadataMap;
use crate::core::tag_value::TagValue;
use crate::io::{ByteOrder, EndianReader};

// ============================================================================
// CONSTANTS - ColorData Byte Offsets
// ============================================================================

/// Byte offset for ColorTempAsShot (as-shot color temperature in Kelvin)
const OFFSET_COLOR_TEMP_AS_SHOT: usize = 63;

/// Byte offset for ColorTempAuto (auto white balance color temperature)
const OFFSET_COLOR_TEMP_AUTO: usize = 65;

/// Byte offset for ColorTempDaylight (~5200K daylight preset)
const OFFSET_COLOR_TEMP_DAYLIGHT: usize = 79;

/// Byte offset for ColorTempShade (~7000K shade preset)
const OFFSET_COLOR_TEMP_SHADE: usize = 83;

/// Byte offset for ColorTempCloudy (~6000K cloudy preset)
const OFFSET_COLOR_TEMP_CLOUDY: usize = 87;

/// Byte offset for ColorTempTungsten (~3200K tungsten preset)
const OFFSET_COLOR_TEMP_TUNGSTEN: usize = 91;

/// Byte offset for ColorTempFluorescent (~4000K fluorescent preset)
const OFFSET_COLOR_TEMP_FLUORESCENT: usize = 95;

/// Byte offset for ColorTempFlash (~6500K flash preset)
const OFFSET_COLOR_TEMP_FLASH: usize = 99;

/// Byte offset for WBShiftAB (white balance shift amber-blue axis)
/// Positive values shift toward amber, negative toward blue
const OFFSET_WB_SHIFT_AB: usize = 186;

/// Byte offset for WBShiftGM (white balance shift green-magenta axis)
/// Positive values shift toward green, negative toward magenta
const OFFSET_WB_SHIFT_GM: usize = 188;

/// Minimum data size required to parse color temperature values
/// This covers at least through ColorTempFlash at offset 99 + 2 bytes
const MIN_COLOR_TEMP_SIZE: usize = 102;

/// Minimum data size required to parse WB shift values
/// This covers through WBShiftGM at offset 188 + 2 bytes
const MIN_WB_SHIFT_SIZE: usize = 190;

// ============================================================================
// TAG NAME CONSTANTS
// ============================================================================

/// Tag prefix for Canon MakerNote tags
const TAG_PREFIX: &str = "Canon";

/// Tag name for as-shot color temperature
const TAG_COLOR_TEMP_AS_SHOT: &str = "ColorTempAsShot";

/// Tag name for auto white balance color temperature
const TAG_COLOR_TEMP_AUTO: &str = "ColorTempAuto";

/// Tag name for daylight color temperature preset
const TAG_COLOR_TEMP_DAYLIGHT: &str = "ColorTempDaylight";

/// Tag name for shade color temperature preset
const TAG_COLOR_TEMP_SHADE: &str = "ColorTempShade";

/// Tag name for cloudy color temperature preset
const TAG_COLOR_TEMP_CLOUDY: &str = "ColorTempCloudy";

/// Tag name for tungsten color temperature preset
const TAG_COLOR_TEMP_TUNGSTEN: &str = "ColorTempTungsten";

/// Tag name for fluorescent color temperature preset
const TAG_COLOR_TEMP_FLUORESCENT: &str = "ColorTempFluorescent";

/// Tag name for flash color temperature preset
const TAG_COLOR_TEMP_FLASH: &str = "ColorTempFlash";

/// Tag name for white balance shift on amber-blue axis
const TAG_WB_SHIFT_AB: &str = "WBShiftAB";

/// Tag name for white balance shift on green-magenta axis
const TAG_WB_SHIFT_GM: &str = "WBShiftGM";

// ============================================================================
// PUBLIC API
// ============================================================================

/// Parses Canon ColorData binary data and extracts white balance and color
/// calibration metadata.
///
/// This function processes the binary ColorData blob found in Canon MakerNotes
/// and extracts color temperature values for various lighting presets, as well
/// as white balance shift adjustments.
///
/// # Arguments
///
/// * `data` - Raw byte slice containing the Canon ColorData structure.
///   Must be at least 102 bytes for color temperatures, or 190 bytes
///   to include WB shift values.
/// * `byte_order` - Byte order flag: `true` for little-endian (Intel),
///   `false` for big-endian (Motorola). Most Canon cameras
///   use little-endian.
///
/// # Returns
///
/// A `MetadataMap` containing the parsed tags. Each tag is prefixed with
/// "Canon:" and contains an integer value. Tags are only included if:
/// - The data buffer is large enough to contain the value
/// - The parsed value is within a valid range (color temps: 1000-15000K)
///
/// # Tag Format
///
/// All tags use the format `Canon:TagName` with integer values:
/// - Color temperature tags: values in Kelvin (e.g., 5200)
/// - WB shift tags: signed values typically in range -9 to +9
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::tiff::makernotes::canon::color_data::parse_canon_color_data;
///
/// // Parse ColorData from a Canon image (little-endian)
/// let metadata = parse_canon_color_data(&color_data_bytes, true);
///
/// // Access specific values
/// if let Some(daylight) = metadata.get_integer("Canon:ColorTempDaylight") {
///     assert!(daylight >= 5000 && daylight <= 5500);
/// }
/// ```
///
/// # Notes
///
/// - The exact byte offsets may vary between Canon camera models and firmware
///   versions. This implementation uses common offsets found in many EOS cameras.
/// - Invalid or out-of-range values are silently skipped rather than causing errors.
/// - Color temperature values outside the 1000-15000K range are considered invalid
///   and are not included in the output.
pub fn parse_canon_color_data(data: &[u8], byte_order: bool) -> MetadataMap {
    let mut metadata = MetadataMap::new();

    // Early return if data is too small for any useful parsing
    if data.len() < MIN_COLOR_TEMP_SIZE {
        return metadata;
    }

    // Convert boolean byte_order flag to EndianReader's ByteOrder enum
    // true = little-endian (common for Canon), false = big-endian
    let order = if byte_order {
        ByteOrder::Little
    } else {
        ByteOrder::Big
    };
    let reader = EndianReader::new(data, order);

    // Parse color temperature values
    // These are signed 16-bit integers representing Kelvin values
    parse_color_temp(
        &reader,
        OFFSET_COLOR_TEMP_AS_SHOT,
        TAG_COLOR_TEMP_AS_SHOT,
        &mut metadata,
    );
    parse_color_temp(
        &reader,
        OFFSET_COLOR_TEMP_AUTO,
        TAG_COLOR_TEMP_AUTO,
        &mut metadata,
    );
    parse_color_temp(
        &reader,
        OFFSET_COLOR_TEMP_DAYLIGHT,
        TAG_COLOR_TEMP_DAYLIGHT,
        &mut metadata,
    );
    parse_color_temp(
        &reader,
        OFFSET_COLOR_TEMP_SHADE,
        TAG_COLOR_TEMP_SHADE,
        &mut metadata,
    );
    parse_color_temp(
        &reader,
        OFFSET_COLOR_TEMP_CLOUDY,
        TAG_COLOR_TEMP_CLOUDY,
        &mut metadata,
    );
    parse_color_temp(
        &reader,
        OFFSET_COLOR_TEMP_TUNGSTEN,
        TAG_COLOR_TEMP_TUNGSTEN,
        &mut metadata,
    );
    parse_color_temp(
        &reader,
        OFFSET_COLOR_TEMP_FLUORESCENT,
        TAG_COLOR_TEMP_FLUORESCENT,
        &mut metadata,
    );
    parse_color_temp(
        &reader,
        OFFSET_COLOR_TEMP_FLASH,
        TAG_COLOR_TEMP_FLASH,
        &mut metadata,
    );

    // Parse WB shift values if data is large enough
    // These are smaller signed values (-9 to +9 typical range)
    if data.len() >= MIN_WB_SHIFT_SIZE {
        parse_wb_shift(&reader, OFFSET_WB_SHIFT_AB, TAG_WB_SHIFT_AB, &mut metadata);
        parse_wb_shift(&reader, OFFSET_WB_SHIFT_GM, TAG_WB_SHIFT_GM, &mut metadata);
    }

    metadata
}

// ============================================================================
// PRIVATE HELPER FUNCTIONS
// ============================================================================

/// Parses a color temperature value at the given offset and inserts it into
/// the metadata map if valid.
///
/// Color temperature values are validated to be within the range 1000-15000K,
/// which covers the practical range from very warm (candlelight) to very cool
/// (blue sky) lighting conditions.
///
/// # Arguments
///
/// * `reader` - EndianReader positioned on the ColorData buffer
/// * `offset` - Byte offset of the i16 color temperature value
/// * `tag_name` - Name of the tag (without prefix)
/// * `metadata` - Mutable reference to the output MetadataMap
fn parse_color_temp(
    reader: &EndianReader,
    offset: usize,
    tag_name: &str,
    metadata: &mut MetadataMap,
) {
    if let Some(value) = reader.i16_at(offset) {
        // Validate color temperature range (1000K to 15000K)
        // Values outside this range are likely invalid or uninitialized data
        if (1000..=15000).contains(&value) {
            let full_tag_name = format!("{}:{}", TAG_PREFIX, tag_name);
            metadata.insert(full_tag_name, TagValue::new_integer(value as i64));
        }
    }
}

/// Parses a white balance shift value at the given offset and inserts it into
/// the metadata map.
///
/// WB shift values are signed integers that represent the user's manual
/// adjustment of white balance on either the amber-blue (AB) or green-magenta
/// (GM) axis. Typical values range from -9 to +9.
///
/// # Arguments
///
/// * `reader` - EndianReader positioned on the ColorData buffer
/// * `offset` - Byte offset of the i16 WB shift value
/// * `tag_name` - Name of the tag (without prefix)
/// * `metadata` - Mutable reference to the output MetadataMap
fn parse_wb_shift(
    reader: &EndianReader,
    offset: usize,
    tag_name: &str,
    metadata: &mut MetadataMap,
) {
    if let Some(value) = reader.i16_at(offset) {
        // WB shift values are typically small (-9 to +9) but we allow
        // the full i16 range since some cameras may use larger values
        // for fine-tuning. We only reject clearly invalid values (e.g., 0x7FFF).
        if value != i16::MAX && value != i16::MIN {
            let full_tag_name = format!("{}:{}", TAG_PREFIX, tag_name);
            metadata.insert(full_tag_name, TagValue::new_integer(value as i64));
        }
    }
}

// ============================================================================
// UNIT TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates test data with specified color temperature and WB shift values.
    /// Returns a buffer large enough for all values (MIN_WB_SHIFT_SIZE bytes).
    fn create_test_data(
        color_temp_as_shot: i16,
        color_temp_auto: i16,
        color_temp_daylight: i16,
        color_temp_shade: i16,
        color_temp_cloudy: i16,
        color_temp_tungsten: i16,
        color_temp_fluorescent: i16,
        color_temp_flash: i16,
        wb_shift_ab: i16,
        wb_shift_gm: i16,
        little_endian: bool,
    ) -> Vec<u8> {
        let mut data = vec![0u8; MIN_WB_SHIFT_SIZE];

        // Helper to write i16 at offset with proper byte order
        let write_i16 = |buf: &mut [u8], offset: usize, value: i16| {
            let bytes = if little_endian {
                value.to_le_bytes()
            } else {
                value.to_be_bytes()
            };
            buf[offset] = bytes[0];
            buf[offset + 1] = bytes[1];
        };

        write_i16(&mut data, OFFSET_COLOR_TEMP_AS_SHOT, color_temp_as_shot);
        write_i16(&mut data, OFFSET_COLOR_TEMP_AUTO, color_temp_auto);
        write_i16(&mut data, OFFSET_COLOR_TEMP_DAYLIGHT, color_temp_daylight);
        write_i16(&mut data, OFFSET_COLOR_TEMP_SHADE, color_temp_shade);
        write_i16(&mut data, OFFSET_COLOR_TEMP_CLOUDY, color_temp_cloudy);
        write_i16(&mut data, OFFSET_COLOR_TEMP_TUNGSTEN, color_temp_tungsten);
        write_i16(
            &mut data,
            OFFSET_COLOR_TEMP_FLUORESCENT,
            color_temp_fluorescent,
        );
        write_i16(&mut data, OFFSET_COLOR_TEMP_FLASH, color_temp_flash);
        write_i16(&mut data, OFFSET_WB_SHIFT_AB, wb_shift_ab);
        write_i16(&mut data, OFFSET_WB_SHIFT_GM, wb_shift_gm);

        data
    }

    #[test]
    fn test_parse_valid_color_temperatures_little_endian() {
        // Test with typical Canon color temperature values (little-endian)
        let data = create_test_data(
            5200, // AsShot
            5200, // Auto
            5200, // Daylight
            7000, // Shade
            6000, // Cloudy
            3200, // Tungsten
            4000, // Fluorescent
            6500, // Flash
            0,    // WBShiftAB
            0,    // WBShiftGM
            true, // little-endian
        );

        let metadata = parse_canon_color_data(&data, true);

        assert_eq!(metadata.get_integer("Canon:ColorTempAsShot"), Some(5200));
        assert_eq!(metadata.get_integer("Canon:ColorTempAuto"), Some(5200));
        assert_eq!(metadata.get_integer("Canon:ColorTempDaylight"), Some(5200));
        assert_eq!(metadata.get_integer("Canon:ColorTempShade"), Some(7000));
        assert_eq!(metadata.get_integer("Canon:ColorTempCloudy"), Some(6000));
        assert_eq!(metadata.get_integer("Canon:ColorTempTungsten"), Some(3200));
        assert_eq!(
            metadata.get_integer("Canon:ColorTempFluorescent"),
            Some(4000)
        );
        assert_eq!(metadata.get_integer("Canon:ColorTempFlash"), Some(6500));
    }

    #[test]
    fn test_parse_valid_color_temperatures_big_endian() {
        // Test with big-endian byte order
        let data = create_test_data(
            5500,  // AsShot
            5500,  // Auto
            5200,  // Daylight
            7500,  // Shade
            6500,  // Cloudy
            3000,  // Tungsten
            4500,  // Fluorescent
            7000,  // Flash
            0,     // WBShiftAB
            0,     // WBShiftGM
            false, // big-endian
        );

        let metadata = parse_canon_color_data(&data, false);

        assert_eq!(metadata.get_integer("Canon:ColorTempAsShot"), Some(5500));
        assert_eq!(metadata.get_integer("Canon:ColorTempDaylight"), Some(5200));
        assert_eq!(metadata.get_integer("Canon:ColorTempShade"), Some(7500));
        assert_eq!(metadata.get_integer("Canon:ColorTempTungsten"), Some(3000));
    }

    #[test]
    fn test_parse_wb_shift_values() {
        // Test WB shift parsing with various values
        let data = create_test_data(
            5200, // AsShot
            5200, // Auto
            5200, // Daylight
            7000, // Shade
            6000, // Cloudy
            3200, // Tungsten
            4000, // Fluorescent
            6500, // Flash
            3,    // WBShiftAB (shift toward amber)
            -2,   // WBShiftGM (shift toward magenta)
            true, // little-endian
        );

        let metadata = parse_canon_color_data(&data, true);

        assert_eq!(metadata.get_integer("Canon:WBShiftAB"), Some(3));
        assert_eq!(metadata.get_integer("Canon:WBShiftGM"), Some(-2));
    }

    #[test]
    fn test_parse_extreme_wb_shift_values() {
        // Test WB shift at boundary values
        let data = create_test_data(
            5200, 5200, 5200, 7000, 6000, 3200, 4000, 6500, 9,  // Maximum typical WBShiftAB
            -9, // Minimum typical WBShiftGM
            true,
        );

        let metadata = parse_canon_color_data(&data, true);

        assert_eq!(metadata.get_integer("Canon:WBShiftAB"), Some(9));
        assert_eq!(metadata.get_integer("Canon:WBShiftGM"), Some(-9));
    }

    #[test]
    fn test_invalid_color_temp_too_low() {
        // Color temperature below 1000K should be rejected
        let data = create_test_data(
            500, // Invalid - too low
            5200, 5200, 7000, 6000, 3200, 4000, 6500, 0, 0, true,
        );

        let metadata = parse_canon_color_data(&data, true);

        // AsShot should not be present due to invalid value
        assert!(metadata.get_integer("Canon:ColorTempAsShot").is_none());
        // Other valid values should still be parsed
        assert_eq!(metadata.get_integer("Canon:ColorTempAuto"), Some(5200));
    }

    #[test]
    fn test_invalid_color_temp_too_high() {
        // Color temperature above 15000K should be rejected
        let data = create_test_data(
            20000, // Invalid - too high
            5200, 5200, 7000, 6000, 3200, 4000, 6500, 0, 0, true,
        );

        let metadata = parse_canon_color_data(&data, true);

        assert!(metadata.get_integer("Canon:ColorTempAsShot").is_none());
        assert_eq!(metadata.get_integer("Canon:ColorTempAuto"), Some(5200));
    }

    #[test]
    fn test_boundary_color_temp_values() {
        // Test at exact boundary values (1000K and 15000K)
        let data = create_test_data(
            1000,  // Minimum valid
            15000, // Maximum valid
            5200, 7000, 6000, 3200, 4000, 6500, 0, 0, true,
        );

        let metadata = parse_canon_color_data(&data, true);

        assert_eq!(metadata.get_integer("Canon:ColorTempAsShot"), Some(1000));
        assert_eq!(metadata.get_integer("Canon:ColorTempAuto"), Some(15000));
    }

    #[test]
    fn test_empty_data() {
        // Empty data should return empty metadata
        let data: Vec<u8> = vec![];
        let metadata = parse_canon_color_data(&data, true);

        assert!(metadata.is_empty());
    }

    #[test]
    fn test_insufficient_data_for_color_temps() {
        // Data too small for color temps should return empty metadata
        let data = vec![0u8; 50]; // Less than MIN_COLOR_TEMP_SIZE
        let metadata = parse_canon_color_data(&data, true);

        assert!(metadata.is_empty());
    }

    #[test]
    fn test_sufficient_data_for_color_temps_but_not_wb_shift() {
        // Data large enough for color temps but not WB shift
        let mut data = vec![0u8; MIN_COLOR_TEMP_SIZE];

        // Write a valid color temperature at the AsShot offset (little-endian)
        let value: i16 = 5200;
        let bytes = value.to_le_bytes();
        data[OFFSET_COLOR_TEMP_AS_SHOT] = bytes[0];
        data[OFFSET_COLOR_TEMP_AS_SHOT + 1] = bytes[1];

        let metadata = parse_canon_color_data(&data, true);

        // Color temp should be parsed
        assert_eq!(metadata.get_integer("Canon:ColorTempAsShot"), Some(5200));
        // WB shift should not be present (data too small)
        assert!(metadata.get_integer("Canon:WBShiftAB").is_none());
        assert!(metadata.get_integer("Canon:WBShiftGM").is_none());
    }

    #[test]
    fn test_zero_values() {
        // Zero values for color temps are invalid (below 1000K threshold)
        let data = create_test_data(
            0, // Invalid
            0, // Invalid
            0, // Invalid
            0, // Invalid
            0, // Invalid
            0, // Invalid
            0, // Invalid
            0, // Invalid
            0, // Valid WB shift (no shift)
            0, // Valid WB shift (no shift)
            true,
        );

        let metadata = parse_canon_color_data(&data, true);

        // All color temps should be absent
        assert!(metadata.get_integer("Canon:ColorTempAsShot").is_none());
        assert!(metadata.get_integer("Canon:ColorTempDaylight").is_none());

        // Zero WB shift is valid (means no adjustment)
        assert_eq!(metadata.get_integer("Canon:WBShiftAB"), Some(0));
        assert_eq!(metadata.get_integer("Canon:WBShiftGM"), Some(0));
    }

    #[test]
    fn test_negative_color_temp() {
        // Negative color temperature is invalid
        let data = create_test_data(
            -5200, // Invalid
            5200, 5200, 7000, 6000, 3200, 4000, 6500, 0, 0, true,
        );

        let metadata = parse_canon_color_data(&data, true);

        assert!(metadata.get_integer("Canon:ColorTempAsShot").is_none());
        assert_eq!(metadata.get_integer("Canon:ColorTempAuto"), Some(5200));
    }

    #[test]
    fn test_tag_naming_format() {
        // Verify correct tag name format
        let data = create_test_data(5200, 5200, 5200, 7000, 6000, 3200, 4000, 6500, 1, 2, true);

        let metadata = parse_canon_color_data(&data, true);

        // Verify all expected tags are present with correct prefix format
        assert!(metadata.contains_key("Canon:ColorTempAsShot"));
        assert!(metadata.contains_key("Canon:ColorTempAuto"));
        assert!(metadata.contains_key("Canon:ColorTempDaylight"));
        assert!(metadata.contains_key("Canon:ColorTempShade"));
        assert!(metadata.contains_key("Canon:ColorTempCloudy"));
        assert!(metadata.contains_key("Canon:ColorTempTungsten"));
        assert!(metadata.contains_key("Canon:ColorTempFluorescent"));
        assert!(metadata.contains_key("Canon:ColorTempFlash"));
        assert!(metadata.contains_key("Canon:WBShiftAB"));
        assert!(metadata.contains_key("Canon:WBShiftGM"));
    }

    #[test]
    fn test_i16_max_wb_shift_rejected() {
        // i16::MAX as WB shift should be rejected (likely uninitialized data)
        let data = create_test_data(
            5200,
            5200,
            5200,
            7000,
            6000,
            3200,
            4000,
            6500,
            i16::MAX, // Should be rejected
            i16::MIN, // Should be rejected
            true,
        );

        let metadata = parse_canon_color_data(&data, true);

        assert!(metadata.get_integer("Canon:WBShiftAB").is_none());
        assert!(metadata.get_integer("Canon:WBShiftGM").is_none());
    }

    #[test]
    fn test_metadata_map_iteration() {
        // Verify that the metadata map can be iterated correctly
        let data = create_test_data(5200, 5200, 5200, 7000, 6000, 3200, 4000, 6500, 3, -2, true);

        let metadata = parse_canon_color_data(&data, true);

        // Should have 10 tags total (8 color temps + 2 WB shifts)
        assert_eq!(metadata.len(), 10);

        // Verify iteration works
        let mut count = 0;
        for (key, value) in metadata.iter() {
            assert!(key.starts_with("Canon:"));
            assert!(value.is_integer());
            count += 1;
        }
        assert_eq!(count, 10);
    }
}
