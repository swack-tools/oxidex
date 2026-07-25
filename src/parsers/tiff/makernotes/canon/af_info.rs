//! Canon AFInfo tag parser
//!
//! Parses Canon MakerNotes AFInfo block containing autofocus settings and
//! AF point information. This data structure varies by camera model but
//! generally contains information about:
//! - Number of AF points available
//! - Which AF points were selected
//! - Which AF points achieved focus
//! - AF area dimensions and positions
//!
//! The AFInfo block is a signed 16-bit integer array stored in the Canon
//! MakerNote IFD, typically at tag 0x0012 (AFInfo) or 0x0026 (AFInfo2).
//!
//! # Data Format
//!
//! The AFInfo array uses the following layout (indices are 0-based):
//! - [0]: Array length (number of i16 values including this one)
//! - [1]: Number of AF points available on the camera
//! - [2]: Number of valid/active AF points
//! - [3]: AF area mode (single point, zone, etc.)
//! - [4]: Number of AFAreaWidths entries
//! - [5+]: Variable-length arrays for widths, heights, positions
//!
//! # References
//!
//! Based on ExifTool's Canon.pm AFInfo and AFInfo2 tag definitions.
//! See: https://exiftool.org/TagNames/Canon.html#AFInfo

use crate::core::MetadataMap;
use crate::core::TagValue;
use crate::io::{ByteOrder, EndianReader};

// =============================================================================
// CONSTANTS - AFInfo Array Indices
// =============================================================================

/// Index for the array length field (number of i16 values in the array)
const AF_ARRAY_LENGTH: usize = 0;

/// Index for the total number of AF points supported by the camera
const AF_NUM_AF_POINTS: usize = 1;

/// Index for the number of valid/active AF points
const AF_VALID_AF_POINTS: usize = 2;

/// Index for the AF area mode (single point, zone, tracking, etc.)
const AF_AREA_MODE: usize = 3;

/// Index for the count of AF area width values that follow
const AF_AREA_WIDTH_COUNT: usize = 4;

// =============================================================================
// AF Area Mode Decoder
// =============================================================================

/// Decodes the AF area mode value into a human-readable string.
///
/// AF area mode determines how the camera selects focus points - whether
/// a single point is used, a zone of points, or automatic selection.
///
/// # Arguments
///
/// * `mode` - The raw AF area mode value from the AFInfo array
///
/// # Returns
///
/// A static string describing the AF area mode, or "Unknown" for
/// unrecognized values.
fn decode_af_area_mode(mode: i16) -> &'static str {
    match mode {
        0 => "Off (Manual Focus)",
        1 => "AF Point Expansion (surround)",
        2 => "Single-point AF",
        3 => "Multi-point AF",
        4 => "Single-point AF (Face Detect)",
        5 => "Multi-point AF (Face Detect)",
        6 => "AF Point Expansion (4 point)",
        7 => "Zone AF",
        8 => "AF Point Expansion (8 point)",
        9 => "Spot AF",
        10 => "AF Point Expansion (6 point)",
        11 => "Flexizone Multi (49 point)",
        12 => "Flexizone Multi (All points)",
        13 => "Flexizone Single",
        14 => "Large Zone AF (Horizontal)",
        15 => "Large Zone AF (Vertical)",
        _ => "Unknown",
    }
}

// =============================================================================
// PUBLIC API
// =============================================================================

/// Parses Canon AFInfo data from raw bytes into a MetadataMap.
///
/// This function extracts autofocus information from the Canon AFInfo
/// binary structure, which contains details about AF points, selection
/// modes, and AF area geometry.
///
/// # Arguments
///
/// * `data` - Raw bytes of the AFInfo block (array of i16 values)
/// * `byte_order` - Byte order for parsing: `true` for big-endian,
///   `false` for little-endian
///
/// # Returns
///
/// A `MetadataMap` containing the parsed AF information with keys:
/// - `Canon:NumAFPoints` - Total AF points on camera
/// - `Canon:ValidAFPoints` - Number of active AF points
/// - `Canon:AFAreaMode` - AF selection mode (decoded)
/// - `Canon:AFPointsSelected` - Bitmask of selected points (if present)
/// - `Canon:AFPointsInFocus` - Bitmask of focused points (if present)
/// - `Canon:AFAreaWidths` - Comma-separated width values (if present)
/// - `Canon:AFAreaHeights` - Comma-separated height values (if present)
/// - `Canon:AFAreaXPositions` - Comma-separated X positions (if present)
/// - `Canon:AFAreaYPositions` - Comma-separated Y positions (if present)
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::tiff::makernotes::canon::af_info::parse_canon_af_info;
///
/// // Little-endian AFInfo data
/// let data = [0x0A, 0x00, 0x09, 0x00, 0x09, 0x00, 0x02, 0x00, /* ... */];
/// let metadata = parse_canon_af_info(&data, false);
///
/// if let Some(num_points) = metadata.get("Canon:NumAFPoints") {
///     println!("Camera has {} AF points", num_points);
/// }
/// ```
///
/// # Data Safety
///
/// This function performs bounds checking on all array accesses and
/// gracefully handles malformed or truncated data by returning a
/// partial result with whatever fields could be successfully parsed.
pub fn parse_canon_af_info(data: &[u8], byte_order: bool) -> MetadataMap {
    let mut metadata = MetadataMap::new();

    // Convert bool byte_order to our internal ByteOrder enum
    // true = big-endian, false = little-endian
    let order = if byte_order {
        ByteOrder::Big
    } else {
        ByteOrder::Little
    };

    let reader = EndianReader::new(data, order);

    // Minimum size check: need at least the array length field (2 bytes)
    if data.len() < 2 {
        return metadata;
    }

    // Read the array length field to validate data
    let array_length = match reader.i16_at(AF_ARRAY_LENGTH * 2) {
        Some(len) if len > 0 => len as usize,
        _ => return metadata,
    };

    // Validate that we have enough data for the declared array length
    let expected_bytes = array_length * 2;
    if data.len() < expected_bytes {
        // Proceed with available data, but cap our reads
    }

    // Helper closure to safely read i16 at array index
    let read_i16 = |index: usize| -> Option<i16> { reader.i16_at(index * 2) };

    // Parse NumAFPoints - total AF points on the camera
    if let Some(num_points) = read_i16(AF_NUM_AF_POINTS)
        && num_points > 0
        && num_points <= 1000
    {
        // Sanity check: no camera has >1000 AF points
        metadata.insert(
            "Canon:NumAFPoints",
            TagValue::new_integer(num_points as i64),
        );
    }

    // Parse ValidAFPoints - number of active/available AF points
    if let Some(valid_points) = read_i16(AF_VALID_AF_POINTS)
        && (0..=1000).contains(&valid_points)
    {
        metadata.insert(
            "Canon:ValidAFPoints",
            TagValue::new_integer(valid_points as i64),
        );
    }

    // Parse AFAreaMode - how AF points are selected
    if let Some(area_mode) = read_i16(AF_AREA_MODE) {
        let mode_string = decode_af_area_mode(area_mode);
        metadata.insert("Canon:AFAreaMode", TagValue::new_string(mode_string));
        // Also store the raw value for reference
        metadata.insert(
            "Canon:AFAreaModeRaw",
            TagValue::new_integer(area_mode as i64),
        );
    }

    // Parse AFAreaWidthCount - number of width entries
    let width_count = read_i16(AF_AREA_WIDTH_COUNT).unwrap_or(0).max(0) as usize;

    // Calculate dynamic array positions based on width_count
    // The structure after index 4 is:
    // - AFAreaWidths[width_count]
    // - AFAreaHeights[width_count]
    // - AFAreaXPositions[num_points]
    // - AFAreaYPositions[num_points]
    // - AFPointsSelected (bitmask)
    // - AFPointsInFocus (bitmask)

    let num_points = read_i16(AF_NUM_AF_POINTS).unwrap_or(0).max(0) as usize;

    // Starting index for variable-length arrays
    let widths_start = 5;
    let heights_start = widths_start + width_count;
    let x_positions_start = heights_start + width_count;
    let y_positions_start = x_positions_start + num_points;
    let selected_index = y_positions_start + num_points;
    let in_focus_index = selected_index + 1;

    // Parse AFAreaWidths
    if width_count > 0 && width_count <= 100 {
        let widths: Vec<i16> = (0..width_count)
            .filter_map(|i| read_i16(widths_start + i))
            .collect();

        if !widths.is_empty() {
            let widths_str = widths
                .iter()
                .map(|w| w.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            metadata.insert("Canon:AFAreaWidths", TagValue::new_string(widths_str));
        }
    }

    // Parse AFAreaHeights
    if width_count > 0 && width_count <= 100 {
        let heights: Vec<i16> = (0..width_count)
            .filter_map(|i| read_i16(heights_start + i))
            .collect();

        if !heights.is_empty() {
            let heights_str = heights
                .iter()
                .map(|h| h.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            metadata.insert("Canon:AFAreaHeights", TagValue::new_string(heights_str));
        }
    }

    // Parse AFAreaXPositions
    if num_points > 0 && num_points <= 1000 {
        let x_positions: Vec<i16> = (0..num_points)
            .filter_map(|i| read_i16(x_positions_start + i))
            .collect();

        if !x_positions.is_empty() {
            let x_str = x_positions
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            metadata.insert("Canon:AFAreaXPositions", TagValue::new_string(x_str));
        }
    }

    // Parse AFAreaYPositions
    if num_points > 0 && num_points <= 1000 {
        let y_positions: Vec<i16> = (0..num_points)
            .filter_map(|i| read_i16(y_positions_start + i))
            .collect();

        if !y_positions.is_empty() {
            let y_str = y_positions
                .iter()
                .map(|y| y.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            metadata.insert("Canon:AFAreaYPositions", TagValue::new_string(y_str));
        }
    }

    // Parse AFPointsSelected - bitmask of selected AF points
    if let Some(selected) = read_i16(selected_index) {
        metadata.insert(
            "Canon:AFPointsSelected",
            TagValue::new_integer(selected as i64),
        );
    }

    // Parse AFPointsInFocus - bitmask of AF points that achieved focus
    if let Some(in_focus) = read_i16(in_focus_index) {
        metadata.insert(
            "Canon:AFPointsInFocus",
            TagValue::new_integer(in_focus as i64),
        );
    }

    metadata
}

// =============================================================================
// UNIT TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a test AFInfo buffer with specified values.
    ///
    /// This helper builds a properly formatted little-endian AFInfo
    /// array for testing the parser.
    fn create_af_info_buffer(
        num_points: i16,
        valid_points: i16,
        area_mode: i16,
        width_count: i16,
    ) -> Vec<u8> {
        let mut data = Vec::new();

        // Calculate total array length
        // Base fields (5) + widths + heights + x_pos + y_pos + selected + in_focus
        let total_length = 5 + (width_count as i16 * 2) + (num_points * 2) + 2;

        // Write array length (index 0)
        data.extend_from_slice(&total_length.to_le_bytes());
        // Write num_points (index 1)
        data.extend_from_slice(&num_points.to_le_bytes());
        // Write valid_points (index 2)
        data.extend_from_slice(&valid_points.to_le_bytes());
        // Write area_mode (index 3)
        data.extend_from_slice(&area_mode.to_le_bytes());
        // Write width_count (index 4)
        data.extend_from_slice(&width_count.to_le_bytes());

        // Write dummy widths
        for i in 0..width_count {
            let width: i16 = 100 + i * 10;
            data.extend_from_slice(&width.to_le_bytes());
        }

        // Write dummy heights
        for i in 0..width_count {
            let height: i16 = 50 + i * 5;
            data.extend_from_slice(&height.to_le_bytes());
        }

        // Write dummy X positions
        for i in 0..num_points {
            let x: i16 = -500 + i * 100;
            data.extend_from_slice(&x.to_le_bytes());
        }

        // Write dummy Y positions
        for i in 0..num_points {
            let y: i16 = -250 + i * 50;
            data.extend_from_slice(&y.to_le_bytes());
        }

        // Write AFPointsSelected bitmask
        let selected: i16 = 0b0000_0000_0000_0101; // Points 0 and 2 selected
        data.extend_from_slice(&selected.to_le_bytes());

        // Write AFPointsInFocus bitmask
        let in_focus: i16 = 0b0000_0000_0000_0001; // Point 0 in focus
        data.extend_from_slice(&in_focus.to_le_bytes());

        data
    }

    #[test]
    fn test_parse_basic_af_info() {
        // Create test data with 9 AF points, single-point AF mode
        let data = create_af_info_buffer(9, 9, 2, 3);

        // Parse as little-endian (byte_order = false)
        let metadata = parse_canon_af_info(&data, false);

        // Verify NumAFPoints
        assert_eq!(
            metadata.get_integer("Canon:NumAFPoints"),
            Some(9),
            "Expected 9 AF points"
        );

        // Verify ValidAFPoints
        assert_eq!(
            metadata.get_integer("Canon:ValidAFPoints"),
            Some(9),
            "Expected 9 valid AF points"
        );

        // Verify AFAreaMode (2 = Single-point AF)
        assert_eq!(
            metadata.get_string("Canon:AFAreaMode"),
            Some("Single-point AF"),
            "Expected Single-point AF mode"
        );
    }

    #[test]
    fn test_parse_af_area_mode_decoding() {
        // Test various AF area modes
        let test_cases = vec![
            (0, "Off (Manual Focus)"),
            (2, "Single-point AF"),
            (3, "Multi-point AF"),
            (7, "Zone AF"),
            (9, "Spot AF"),
            (99, "Unknown"),
        ];

        for (mode_value, expected_mode) in test_cases {
            let data = create_af_info_buffer(9, 9, mode_value, 1);
            let metadata = parse_canon_af_info(&data, false);

            assert_eq!(
                metadata.get_string("Canon:AFAreaMode"),
                Some(expected_mode),
                "Mode {} should decode to '{}'",
                mode_value,
                expected_mode
            );
        }
    }

    #[test]
    fn test_parse_af_area_dimensions() {
        let data = create_af_info_buffer(5, 5, 2, 3);
        let metadata = parse_canon_af_info(&data, false);

        // Check that widths were parsed (3 width values: 100, 110, 120)
        let widths = metadata.get_string("Canon:AFAreaWidths");
        assert!(widths.is_some(), "AFAreaWidths should be present");
        assert_eq!(widths, Some("100 110 120"));

        // Check that heights were parsed (3 height values: 50, 55, 60)
        let heights = metadata.get_string("Canon:AFAreaHeights");
        assert!(heights.is_some(), "AFAreaHeights should be present");
        assert_eq!(heights, Some("50 55 60"));
    }

    #[test]
    fn test_parse_af_area_positions() {
        let data = create_af_info_buffer(5, 5, 2, 1);
        let metadata = parse_canon_af_info(&data, false);

        // Check X positions (5 values: -500, -400, -300, -200, -100)
        let x_pos = metadata.get_string("Canon:AFAreaXPositions");
        assert!(x_pos.is_some(), "AFAreaXPositions should be present");
        assert_eq!(x_pos, Some("-500 -400 -300 -200 -100"));

        // Check Y positions (5 values: -250, -200, -150, -100, -50)
        let y_pos = metadata.get_string("Canon:AFAreaYPositions");
        assert!(y_pos.is_some(), "AFAreaYPositions should be present");
        assert_eq!(y_pos, Some("-250 -200 -150 -100 -50"));
    }

    #[test]
    fn test_parse_af_points_bitmasks() {
        let data = create_af_info_buffer(9, 9, 2, 1);
        let metadata = parse_canon_af_info(&data, false);

        // Selected points bitmask (0b101 = 5)
        assert_eq!(
            metadata.get_integer("Canon:AFPointsSelected"),
            Some(5),
            "AFPointsSelected should be 5 (points 0 and 2)"
        );

        // In-focus points bitmask (0b1 = 1)
        assert_eq!(
            metadata.get_integer("Canon:AFPointsInFocus"),
            Some(1),
            "AFPointsInFocus should be 1 (point 0)"
        );
    }

    #[test]
    fn test_parse_empty_data() {
        let data: Vec<u8> = vec![];
        let metadata = parse_canon_af_info(&data, false);

        assert!(
            metadata.is_empty(),
            "Empty data should produce empty metadata"
        );
    }

    #[test]
    fn test_parse_minimal_data() {
        // Just the array length field (2 bytes) with value 1
        let data: Vec<u8> = vec![0x01, 0x00];
        let metadata = parse_canon_af_info(&data, false);

        // Should parse but have no useful data beyond length
        assert!(
            metadata.is_empty() || metadata.len() <= 1,
            "Minimal data should produce minimal or empty metadata"
        );
    }

    #[test]
    fn test_parse_big_endian() {
        // Create big-endian test data manually
        let mut data = Vec::new();

        // Array length = 10 (big-endian)
        data.extend_from_slice(&10i16.to_be_bytes());
        // NumAFPoints = 45 (big-endian)
        data.extend_from_slice(&45i16.to_be_bytes());
        // ValidAFPoints = 45 (big-endian)
        data.extend_from_slice(&45i16.to_be_bytes());
        // AFAreaMode = 7 (Zone AF, big-endian)
        data.extend_from_slice(&7i16.to_be_bytes());
        // WidthCount = 1 (big-endian)
        data.extend_from_slice(&1i16.to_be_bytes());
        // Width value
        data.extend_from_slice(&200i16.to_be_bytes());
        // Height value
        data.extend_from_slice(&150i16.to_be_bytes());
        // Pad to expected length
        for _ in 0..10 {
            data.extend_from_slice(&0i16.to_be_bytes());
        }

        // Parse as big-endian (byte_order = true)
        let metadata = parse_canon_af_info(&data, true);

        assert_eq!(
            metadata.get_integer("Canon:NumAFPoints"),
            Some(45),
            "Should parse 45 AF points in big-endian"
        );

        assert_eq!(
            metadata.get_string("Canon:AFAreaMode"),
            Some("Zone AF"),
            "Should decode Zone AF mode"
        );
    }

    #[test]
    fn test_parse_invalid_num_points() {
        // Create data with invalid (negative) num_points
        let mut data = Vec::new();
        data.extend_from_slice(&10i16.to_le_bytes()); // length
        data.extend_from_slice(&(-5i16).to_le_bytes()); // invalid num_points
        data.extend_from_slice(&5i16.to_le_bytes()); // valid_points
        data.extend_from_slice(&2i16.to_le_bytes()); // area_mode
        data.extend_from_slice(&0i16.to_le_bytes()); // width_count
        // Pad
        for _ in 0..5 {
            data.extend_from_slice(&0i16.to_le_bytes());
        }

        let metadata = parse_canon_af_info(&data, false);

        // NumAFPoints should not be present (negative value filtered)
        assert!(
            metadata.get_integer("Canon:NumAFPoints").is_none(),
            "Negative NumAFPoints should be filtered out"
        );

        // ValidAFPoints should still be present
        assert_eq!(
            metadata.get_integer("Canon:ValidAFPoints"),
            Some(5),
            "ValidAFPoints should still parse correctly"
        );
    }

    #[test]
    fn test_decode_af_area_mode_all_values() {
        // Test all known AF area mode values
        assert_eq!(decode_af_area_mode(0), "Off (Manual Focus)");
        assert_eq!(decode_af_area_mode(1), "AF Point Expansion (surround)");
        assert_eq!(decode_af_area_mode(2), "Single-point AF");
        assert_eq!(decode_af_area_mode(3), "Multi-point AF");
        assert_eq!(decode_af_area_mode(4), "Single-point AF (Face Detect)");
        assert_eq!(decode_af_area_mode(5), "Multi-point AF (Face Detect)");
        assert_eq!(decode_af_area_mode(6), "AF Point Expansion (4 point)");
        assert_eq!(decode_af_area_mode(7), "Zone AF");
        assert_eq!(decode_af_area_mode(8), "AF Point Expansion (8 point)");
        assert_eq!(decode_af_area_mode(9), "Spot AF");
        assert_eq!(decode_af_area_mode(10), "AF Point Expansion (6 point)");
        assert_eq!(decode_af_area_mode(11), "Flexizone Multi (49 point)");
        assert_eq!(decode_af_area_mode(12), "Flexizone Multi (All points)");
        assert_eq!(decode_af_area_mode(13), "Flexizone Single");
        assert_eq!(decode_af_area_mode(14), "Large Zone AF (Horizontal)");
        assert_eq!(decode_af_area_mode(15), "Large Zone AF (Vertical)");
        assert_eq!(decode_af_area_mode(100), "Unknown");
        assert_eq!(decode_af_area_mode(-1), "Unknown");
    }

    #[test]
    fn test_truncated_data_handling() {
        // Create data that claims to be longer than it actually is
        let mut data = Vec::new();
        data.extend_from_slice(&100i16.to_le_bytes()); // Claim 100 elements
        data.extend_from_slice(&9i16.to_le_bytes()); // NumAFPoints
        data.extend_from_slice(&9i16.to_le_bytes()); // ValidAFPoints
        // Data is truncated here

        let metadata = parse_canon_af_info(&data, false);

        // Should still parse available fields
        assert_eq!(
            metadata.get_integer("Canon:NumAFPoints"),
            Some(9),
            "Should parse NumAFPoints from truncated data"
        );
        assert_eq!(
            metadata.get_integer("Canon:ValidAFPoints"),
            Some(9),
            "Should parse ValidAFPoints from truncated data"
        );
    }

    #[test]
    fn test_af_info_with_zero_width_count() {
        // Some cameras may have 0 width entries
        let data = create_af_info_buffer(9, 9, 2, 0);
        let metadata = parse_canon_af_info(&data, false);

        // Should still parse basic fields
        assert_eq!(metadata.get_integer("Canon:NumAFPoints"), Some(9));

        // Width/Height arrays should be empty or not present
        assert!(
            metadata.get_string("Canon:AFAreaWidths").is_none()
                || metadata.get_string("Canon:AFAreaWidths") == Some(""),
            "Zero width count should result in no widths"
        );
    }
}
