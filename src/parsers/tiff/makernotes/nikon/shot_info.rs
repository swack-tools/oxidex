//! Nikon ShotInfo tag parser
//!
//! Parses Nikon MakerNote ShotInfo binary data to extract shooting parameters
//! such as shutter count, exposure settings, focus information, and
//! vibration reduction status.
//!
//! # ShotInfo Structure
//!
//! The ShotInfo tag (0x0091) contains camera-model-specific shooting data
//! stored as a binary blob. The structure varies by camera model, but
//! common fields include:
//!
//! - Shutter count
//! - Exposure time and aperture
//! - ISO speed
//! - Focus distance
//! - AF mode and area mode
//! - Vibration reduction status
//!
//! # Byte Order
//!
//! The `byte_order` parameter indicates:
//! - `true` = Big-endian (Motorola byte order)
//! - `false` = Little-endian (Intel byte order)
//!
//! # Example
//!
//! ```ignore
//! use oxidex::parsers::tiff::makernotes::nikon::shot_info::parse_nikon_shot_info;
//! use oxidex::core::MetadataMap;
//!
//! let data: &[u8] = &[/* ShotInfo binary data */];
//! let metadata = parse_nikon_shot_info(data, false); // Little-endian
//!
//! if let Some(value) = metadata.get_integer("Nikon:ShutterCount") {
//!     println!("Shutter count: {}", value);
//! }
//! ```

use crate::core::metadata_map::MetadataMap;
use crate::core::tag_value::TagValue;

// =============================================================================
// ShotInfo Field Offsets
// =============================================================================
// These offsets are based on common Nikon ShotInfo structures.
// Note: Actual offsets vary by camera model; these are typical values
// for modern Nikon DSLRs (D7xxx, D8xx, Z-series, etc.)

/// Offset to the ShotInfo version identifier (typically first 4 bytes)
const OFFSET_VERSION: usize = 0;

/// Offset to the shutter count value (u32)
const OFFSET_SHUTTER_COUNT: usize = 4;

/// Offset to exposure time numerator (u32, in microseconds or 1/10000s units)
const OFFSET_EXPOSURE_TIME: usize = 8;

/// Offset to f-number value (u16, multiplied by 10 or 100)
const OFFSET_FNUMBER: usize = 12;

/// Offset to ISO value (u16)
const OFFSET_ISO: usize = 14;

/// Offset to focus distance value (u16, in cm or encoded)
const OFFSET_FOCUS_DISTANCE: usize = 16;

/// Offset to AF mode byte
const OFFSET_AF_MODE: usize = 18;

/// Offset to AF area mode byte
const OFFSET_AF_AREA_MODE: usize = 19;

/// Offset to vibration reduction status byte
const OFFSET_VR_STATUS: usize = 20;

/// Minimum data length required for parsing (bytes)
/// This ensures we have enough data to extract the primary fields
const MIN_SHOT_INFO_LENGTH: usize = 21;

// =============================================================================
// AF Mode Decode Values
// =============================================================================

/// Decodes AF mode byte to human-readable string
///
/// # Arguments
/// * `value` - The raw AF mode byte value
///
/// # Returns
/// A static string describing the AF mode
fn decode_af_mode(value: u8) -> &'static str {
    match value {
        0 => "Manual",
        1 => "AF-S",
        2 => "AF-C",
        3 => "AF-A",
        4 => "AF-F", // Nikon mirrorless
        _ => "Unknown",
    }
}

/// Decodes AF area mode byte to human-readable string
///
/// # Arguments
/// * `value` - The raw AF area mode byte value
///
/// # Returns
/// A static string describing the AF area mode
fn decode_af_area_mode(value: u8) -> &'static str {
    match value {
        0 => "Single Area",
        1 => "Dynamic Area",
        2 => "Dynamic Area (9 points)",
        3 => "Dynamic Area (21 points)",
        4 => "Dynamic Area (51 points)",
        5 => "Group Area",
        6 => "Auto-area",
        7 => "3D-tracking",
        8 => "Wide Area (S)",
        9 => "Wide Area (L)",
        10 => "Pinpoint",
        _ => "Unknown",
    }
}

/// Decodes vibration reduction status byte to human-readable string
///
/// # Arguments
/// * `value` - The raw VR status byte value
///
/// # Returns
/// A static string describing the VR status
fn decode_vr_status(value: u8) -> &'static str {
    match value {
        0 => "Off",
        1 => "On",
        2 => "On (Active)",
        3 => "On (Normal)",
        4 => "On (Sport)",
        _ => "Unknown",
    }
}

// =============================================================================
// Byte Reading Utilities
// =============================================================================

/// Reads a u16 value from the data slice at the given offset
///
/// # Arguments
/// * `data` - The byte slice to read from
/// * `offset` - The byte offset to start reading
/// * `big_endian` - Whether to use big-endian byte order
///
/// # Returns
/// The u16 value if the offset is valid, None otherwise
#[inline]
fn read_u16(data: &[u8], offset: usize, big_endian: bool) -> Option<u16> {
    if offset + 2 > data.len() {
        return None;
    }

    let bytes: [u8; 2] = [data[offset], data[offset + 1]];
    Some(if big_endian {
        u16::from_be_bytes(bytes)
    } else {
        u16::from_le_bytes(bytes)
    })
}

/// Reads a u32 value from the data slice at the given offset
///
/// # Arguments
/// * `data` - The byte slice to read from
/// * `offset` - The byte offset to start reading
/// * `big_endian` - Whether to use big-endian byte order
///
/// # Returns
/// The u32 value if the offset is valid, None otherwise
#[inline]
fn read_u32(data: &[u8], offset: usize, big_endian: bool) -> Option<u32> {
    if offset + 4 > data.len() {
        return None;
    }

    let bytes: [u8; 4] = [
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

// =============================================================================
// Main Parser Function
// =============================================================================

/// Parses Nikon ShotInfo binary data into a MetadataMap
///
/// Extracts shooting parameters from Nikon MakerNote ShotInfo tag data.
/// The ShotInfo structure contains camera-specific shooting metadata
/// including shutter count, exposure settings, focus information, and
/// vibration reduction status.
///
/// # Arguments
///
/// * `data` - Raw ShotInfo binary data extracted from the MakerNote
/// * `byte_order` - Byte order flag: `true` for big-endian, `false` for little-endian
///
/// # Returns
///
/// A `MetadataMap` containing extracted ShotInfo fields with the "Nikon:" prefix.
/// If the data is too short or invalid, returns an empty or partially-filled map.
///
/// # Extracted Tags
///
/// The function attempts to extract the following tags (when data is available):
///
/// | Tag Name                  | Type    | Description                        |
/// |---------------------------|---------|-----------------------------------|
/// | `Nikon:ShutterCount`      | Integer | Total actuations of the shutter    |
/// | `Nikon:ExposureTime`      | Float   | Exposure time in seconds           |
/// | `Nikon:FNumber`           | Float   | Aperture f-number                  |
/// | `Nikon:ISO`               | Integer | ISO sensitivity                    |
/// | `Nikon:FocusDistance`     | Float   | Focus distance in meters           |
/// | `Nikon:AFMode`            | String  | Autofocus mode (AF-S, AF-C, etc.)  |
/// | `Nikon:AFAreaMode`        | String  | AF area selection mode             |
/// | `Nikon:VibrationReduction`| String  | VR/IBIS status (On/Off)            |
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::tiff::makernotes::nikon::shot_info::parse_nikon_shot_info;
///
/// // Example ShotInfo data (simplified)
/// let shot_info_data = vec![
///     0x30, 0x31, 0x30, 0x30, // Version "0100"
///     0x00, 0x00, 0x27, 0x10, // Shutter count: 10000
///     0x00, 0x00, 0x03, 0xE8, // Exposure: 1000 (1/1000s)
///     0x00, 0x50,             // FNumber: 80 (f/8.0)
///     0x01, 0x90,             // ISO: 400
///     0x00, 0xC8,             // Focus distance: 200 (2m)
///     0x02,                   // AF Mode: AF-C
///     0x01,                   // AF Area Mode: Dynamic Area
///     0x01,                   // VR: On
/// ];
///
/// let metadata = parse_nikon_shot_info(&shot_info_data, false);
/// assert_eq!(metadata.get_integer("Nikon:ShutterCount"), Some(10000));
/// ```
///
/// # Notes
///
/// - The actual structure of ShotInfo varies significantly between Nikon camera
///   models. This parser uses offsets typical of modern Nikon DSLRs.
/// - Some fields may be encoded differently on different cameras (e.g., focus
///   distance units, exposure time representation).
/// - Fields that cannot be parsed (due to insufficient data or invalid values)
///   are silently omitted from the output.
pub fn parse_nikon_shot_info(data: &[u8], byte_order: bool) -> MetadataMap {
    let mut map = MetadataMap::new();

    // Return empty map if data is too short
    if data.len() < MIN_SHOT_INFO_LENGTH {
        return map;
    }

    // Extract version string (first 4 bytes as ASCII)
    if data.len() >= 4 {
        let version_bytes = &data[OFFSET_VERSION..OFFSET_VERSION + 4];
        // Check if version bytes are printable ASCII
        if version_bytes
            .iter()
            .all(|&b| b.is_ascii_graphic() || b == b' ')
        {
            let version = String::from_utf8_lossy(version_bytes).trim().to_string();
            if !version.is_empty() {
                map.insert("Nikon:ShotInfoVersion", TagValue::new_string(version));
            }
        }
    }

    // Extract shutter count (u32 at offset 4)
    if let Some(shutter_count) = read_u32(data, OFFSET_SHUTTER_COUNT, byte_order) {
        // Validate: shutter count should be reasonable (0 to 10 million)
        if shutter_count > 0 && shutter_count < 10_000_000 {
            map.insert(
                "Nikon:ShutterCount",
                TagValue::new_integer(shutter_count as i64),
            );
        }
    }

    // Extract exposure time
    // The value is typically stored as 1/10000s or microseconds
    if let Some(exposure_raw) = read_u32(data, OFFSET_EXPOSURE_TIME, byte_order)
        && exposure_raw > 0
        && exposure_raw < 1_000_000_000
    {
        // Convert to seconds (assuming 1/10000s units)
        let exposure_seconds = exposure_raw as f64 / 10000.0;
        map.insert("Nikon:ExposureTime", TagValue::new_float(exposure_seconds));
    }

    // Extract f-number (aperture)
    // Typically stored as f-number * 10 (e.g., f/2.8 = 28)
    if let Some(fnumber_raw) = read_u16(data, OFFSET_FNUMBER, byte_order)
        && fnumber_raw > 0
        && fnumber_raw < 1000
    {
        let fnumber = fnumber_raw as f64 / 10.0;
        map.insert("Nikon:FNumber", TagValue::new_float(fnumber));
    }

    // Extract ISO
    if let Some(iso) = read_u16(data, OFFSET_ISO, byte_order) {
        // Validate: ISO should be in reasonable range (50+)
        // Note: u16 max is 65535, but that's still a valid ISO value
        if iso >= 50 {
            map.insert("Nikon:ISO", TagValue::new_integer(iso as i64));
        }
    }

    // Extract focus distance (typically in cm)
    if let Some(focus_raw) = read_u16(data, OFFSET_FOCUS_DISTANCE, byte_order)
        && focus_raw > 0
        && focus_raw < 65535
    {
        // Convert to meters (assuming cm units)
        let focus_meters = focus_raw as f64 / 100.0;
        map.insert("Nikon:FocusDistance", TagValue::new_float(focus_meters));
    }

    // Extract AF mode
    if OFFSET_AF_MODE < data.len() {
        let af_mode = data[OFFSET_AF_MODE];
        let af_mode_str = decode_af_mode(af_mode);
        map.insert("Nikon:AFMode", TagValue::new_string(af_mode_str));
    }

    // Extract AF area mode
    if OFFSET_AF_AREA_MODE < data.len() {
        let af_area_mode = data[OFFSET_AF_AREA_MODE];
        let af_area_mode_str = decode_af_area_mode(af_area_mode);
        map.insert("Nikon:AFAreaMode", TagValue::new_string(af_area_mode_str));
    }

    // Extract vibration reduction status
    if OFFSET_VR_STATUS < data.len() {
        let vr_status = data[OFFSET_VR_STATUS];
        let vr_status_str = decode_vr_status(vr_status);
        map.insert(
            "Nikon:VibrationReduction",
            TagValue::new_string(vr_status_str),
        );
    }

    map
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates test ShotInfo data with known values
    fn create_test_shot_info() -> Vec<u8> {
        let mut data = vec![0u8; 24];

        // Version string "0100" at offset 0
        data[0] = b'0';
        data[1] = b'1';
        data[2] = b'0';
        data[3] = b'0';

        // Shutter count: 10000 at offset 4 (little-endian)
        let shutter_count: u32 = 10000;
        data[4..8].copy_from_slice(&shutter_count.to_le_bytes());

        // Exposure time: 1000 (1/10s) at offset 8 (little-endian)
        let exposure: u32 = 1000;
        data[8..12].copy_from_slice(&exposure.to_le_bytes());

        // FNumber: 56 (f/5.6) at offset 12 (little-endian)
        let fnumber: u16 = 56;
        data[12..14].copy_from_slice(&fnumber.to_le_bytes());

        // ISO: 400 at offset 14 (little-endian)
        let iso: u16 = 400;
        data[14..16].copy_from_slice(&iso.to_le_bytes());

        // Focus distance: 250 (2.5m) at offset 16 (little-endian)
        let focus: u16 = 250;
        data[16..18].copy_from_slice(&focus.to_le_bytes());

        // AF Mode: 2 (AF-C) at offset 18
        data[18] = 2;

        // AF Area Mode: 3 (Dynamic Area 21 points) at offset 19
        data[19] = 3;

        // VR Status: 1 (On) at offset 20
        data[20] = 1;

        data
    }

    #[test]
    fn test_parse_shot_info_little_endian() {
        let data = create_test_shot_info();
        let result = parse_nikon_shot_info(&data, false);

        // Verify version
        assert_eq!(result.get_string("Nikon:ShotInfoVersion"), Some("0100"));

        // Verify shutter count
        assert_eq!(result.get_integer("Nikon:ShutterCount"), Some(10000));

        // Verify exposure time (1000 / 10000 = 0.1 seconds)
        let exposure = result.get_float("Nikon:ExposureTime");
        assert!(exposure.is_some());
        assert!((exposure.unwrap() - 0.1).abs() < 0.001);

        // Verify f-number (56 / 10 = 5.6)
        let fnumber = result.get_float("Nikon:FNumber");
        assert!(fnumber.is_some());
        assert!((fnumber.unwrap() - 5.6).abs() < 0.001);

        // Verify ISO
        assert_eq!(result.get_integer("Nikon:ISO"), Some(400));

        // Verify focus distance (250 / 100 = 2.5 meters)
        let focus = result.get_float("Nikon:FocusDistance");
        assert!(focus.is_some());
        assert!((focus.unwrap() - 2.5).abs() < 0.001);

        // Verify AF mode
        assert_eq!(result.get_string("Nikon:AFMode"), Some("AF-C"));

        // Verify AF area mode
        assert_eq!(
            result.get_string("Nikon:AFAreaMode"),
            Some("Dynamic Area (21 points)")
        );

        // Verify VR status
        assert_eq!(result.get_string("Nikon:VibrationReduction"), Some("On"));
    }

    #[test]
    fn test_parse_shot_info_big_endian() {
        let mut data = vec![0u8; 24];

        // Version string "0200"
        data[0] = b'0';
        data[1] = b'2';
        data[2] = b'0';
        data[3] = b'0';

        // Shutter count: 50000 (big-endian)
        let shutter_count: u32 = 50000;
        data[4..8].copy_from_slice(&shutter_count.to_be_bytes());

        // Exposure time: 2000 (big-endian)
        let exposure: u32 = 2000;
        data[8..12].copy_from_slice(&exposure.to_be_bytes());

        // FNumber: 28 (f/2.8) (big-endian)
        let fnumber: u16 = 28;
        data[12..14].copy_from_slice(&fnumber.to_be_bytes());

        // ISO: 800 (big-endian)
        let iso: u16 = 800;
        data[14..16].copy_from_slice(&iso.to_be_bytes());

        // Focus distance: 100 (1m) (big-endian)
        let focus: u16 = 100;
        data[16..18].copy_from_slice(&focus.to_be_bytes());

        // AF Mode: 1 (AF-S)
        data[18] = 1;

        // AF Area Mode: 0 (Single Area)
        data[19] = 0;

        // VR Status: 3 (On Normal)
        data[20] = 3;

        let result = parse_nikon_shot_info(&data, true);

        assert_eq!(result.get_string("Nikon:ShotInfoVersion"), Some("0200"));
        assert_eq!(result.get_integer("Nikon:ShutterCount"), Some(50000));
        assert_eq!(result.get_integer("Nikon:ISO"), Some(800));
        assert_eq!(result.get_string("Nikon:AFMode"), Some("AF-S"));
        assert_eq!(result.get_string("Nikon:AFAreaMode"), Some("Single Area"));
        assert_eq!(
            result.get_string("Nikon:VibrationReduction"),
            Some("On (Normal)")
        );
    }

    #[test]
    fn test_parse_shot_info_empty_data() {
        let data: Vec<u8> = vec![];
        let result = parse_nikon_shot_info(&data, false);

        // Empty data should return empty map
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_shot_info_short_data() {
        // Data shorter than minimum required length
        let data = vec![0x30, 0x31, 0x30, 0x30, 0x00, 0x00];
        let result = parse_nikon_shot_info(&data, false);

        // Should return empty map due to insufficient data
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_shot_info_invalid_shutter_count() {
        let mut data = create_test_shot_info();

        // Set shutter count to 0 (invalid)
        data[4..8].copy_from_slice(&0u32.to_le_bytes());

        let result = parse_nikon_shot_info(&data, false);

        // Shutter count should not be present (invalid value)
        assert!(result.get_integer("Nikon:ShutterCount").is_none());
    }

    #[test]
    fn test_parse_shot_info_invalid_iso() {
        let mut data = create_test_shot_info();

        // Set ISO to 10 (below minimum valid range of 50)
        data[14..16].copy_from_slice(&10u16.to_le_bytes());

        let result = parse_nikon_shot_info(&data, false);

        // ISO should not be present (invalid value)
        assert!(result.get_integer("Nikon:ISO").is_none());
    }

    #[test]
    fn test_decode_af_mode() {
        assert_eq!(decode_af_mode(0), "Manual");
        assert_eq!(decode_af_mode(1), "AF-S");
        assert_eq!(decode_af_mode(2), "AF-C");
        assert_eq!(decode_af_mode(3), "AF-A");
        assert_eq!(decode_af_mode(4), "AF-F");
        assert_eq!(decode_af_mode(255), "Unknown");
    }

    #[test]
    fn test_decode_af_area_mode() {
        assert_eq!(decode_af_area_mode(0), "Single Area");
        assert_eq!(decode_af_area_mode(1), "Dynamic Area");
        assert_eq!(decode_af_area_mode(6), "Auto-area");
        assert_eq!(decode_af_area_mode(7), "3D-tracking");
        assert_eq!(decode_af_area_mode(255), "Unknown");
    }

    #[test]
    fn test_decode_vr_status() {
        assert_eq!(decode_vr_status(0), "Off");
        assert_eq!(decode_vr_status(1), "On");
        assert_eq!(decode_vr_status(2), "On (Active)");
        assert_eq!(decode_vr_status(3), "On (Normal)");
        assert_eq!(decode_vr_status(4), "On (Sport)");
        assert_eq!(decode_vr_status(255), "Unknown");
    }

    #[test]
    fn test_read_u16_little_endian() {
        let data = vec![0x10, 0x27]; // 10000 in little-endian
        assert_eq!(read_u16(&data, 0, false), Some(10000));
    }

    #[test]
    fn test_read_u16_big_endian() {
        let data = vec![0x27, 0x10]; // 10000 in big-endian
        assert_eq!(read_u16(&data, 0, true), Some(10000));
    }

    #[test]
    fn test_read_u32_little_endian() {
        let data = vec![0x10, 0x27, 0x00, 0x00]; // 10000 in little-endian
        assert_eq!(read_u32(&data, 0, false), Some(10000));
    }

    #[test]
    fn test_read_u32_big_endian() {
        let data = vec![0x00, 0x00, 0x27, 0x10]; // 10000 in big-endian
        assert_eq!(read_u32(&data, 0, true), Some(10000));
    }

    #[test]
    fn test_read_u16_out_of_bounds() {
        let data = vec![0x10];
        assert_eq!(read_u16(&data, 0, false), None);
        assert_eq!(read_u16(&data, 1, false), None);
    }

    #[test]
    fn test_read_u32_out_of_bounds() {
        let data = vec![0x10, 0x27, 0x00];
        assert_eq!(read_u32(&data, 0, false), None);
    }

    #[test]
    fn test_version_non_ascii() {
        let mut data = create_test_shot_info();

        // Set version to non-printable bytes
        data[0] = 0x00;
        data[1] = 0x00;
        data[2] = 0xFF;
        data[3] = 0xFF;

        let result = parse_nikon_shot_info(&data, false);

        // Version should not be present (non-printable)
        assert!(result.get_string("Nikon:ShotInfoVersion").is_none());
    }

    #[test]
    fn test_all_af_modes() {
        for mode in 0..=4 {
            let mut data = create_test_shot_info();
            data[18] = mode;
            let result = parse_nikon_shot_info(&data, false);
            assert!(result.get_string("Nikon:AFMode").is_some());
        }
    }

    #[test]
    fn test_all_af_area_modes() {
        for mode in 0..=10 {
            let mut data = create_test_shot_info();
            data[19] = mode;
            let result = parse_nikon_shot_info(&data, false);
            assert!(result.get_string("Nikon:AFAreaMode").is_some());
        }
    }

    #[test]
    fn test_all_vr_statuses() {
        for status in 0..=4 {
            let mut data = create_test_shot_info();
            data[20] = status;
            let result = parse_nikon_shot_info(&data, false);
            assert!(result.get_string("Nikon:VibrationReduction").is_some());
        }
    }

    #[test]
    fn test_extreme_values() {
        let mut data = vec![0u8; 24];

        // Version
        data[0..4].copy_from_slice(b"TEST");

        // Maximum valid shutter count (just under 10 million)
        let max_shutter: u32 = 9_999_999;
        data[4..8].copy_from_slice(&max_shutter.to_le_bytes());

        // Maximum valid ISO (u16 max is 65535)
        let max_iso: u16 = 51200; // Common high ISO value within u16 range
        data[14..16].copy_from_slice(&max_iso.to_le_bytes());

        // AF mode and area mode
        data[18] = 0;
        data[19] = 0;
        data[20] = 0;

        let result = parse_nikon_shot_info(&data, false);

        assert_eq!(result.get_integer("Nikon:ShutterCount"), Some(9_999_999));
        assert_eq!(result.get_integer("Nikon:ISO"), Some(51200));
    }
}
