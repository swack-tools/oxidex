//! ExposureProgram value formatting
//!
//! This module provides formatting for the EXIF ExposureProgram tag (0x8822),
//! converting numeric values to human-readable strings that match ExifTool's
//! output format.
//!
//! The ExposureProgram tag indicates the class of exposure program used by
//! the camera when the image was shot, ranging from manual control to various
//! automatic and scene-specific modes.

/// Formats an ExposureProgram numeric value to its human-readable string representation.
///
/// This function converts the raw EXIF ExposureProgram tag value to the corresponding
/// string description, matching ExifTool's output format exactly. The ExposureProgram
/// tag (0x8822) indicates the exposure mode used when capturing the image.
///
/// # Arguments
///
/// * `value` - The numeric ExposureProgram value from the EXIF data
///
/// # Returns
///
/// A `String` containing the human-readable description of the exposure program.
/// For values outside the defined range (0-9), returns "Unknown ({value})".
///
/// # Value Mappings
///
/// | Value | Description                    |
/// |-------|--------------------------------|
/// | 0     | Not Defined                    |
/// | 1     | Manual                         |
/// | 2     | Program AE                     |
/// | 3     | Aperture-priority AE           |
/// | 4     | Shutter speed priority AE      |
/// | 5     | Creative (Slow speed)          |
/// | 6     | Action (High speed)            |
/// | 7     | Portrait                       |
/// | 8     | Landscape                      |
/// | 9     | Bulb                           |
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::exposure_program::format_exposure_program;
///
/// assert_eq!(format_exposure_program(0), "Not Defined");
/// assert_eq!(format_exposure_program(1), "Manual");
/// assert_eq!(format_exposure_program(2), "Program AE");
/// assert_eq!(format_exposure_program(3), "Aperture-priority AE");
/// assert_eq!(format_exposure_program(4), "Shutter speed priority AE");
/// assert_eq!(format_exposure_program(5), "Creative (Slow speed)");
/// assert_eq!(format_exposure_program(6), "Action (High speed)");
/// assert_eq!(format_exposure_program(7), "Portrait");
/// assert_eq!(format_exposure_program(8), "Landscape");
/// assert_eq!(format_exposure_program(9), "Bulb");
/// assert_eq!(format_exposure_program(99), "Unknown (99)");
/// ```
pub fn format_exposure_program(value: u32) -> String {
    match value {
        0 => "Not Defined".to_string(),
        1 => "Manual".to_string(),
        2 => "Program AE".to_string(),
        3 => "Aperture-priority AE".to_string(),
        4 => "Shutter speed priority AE".to_string(),
        5 => "Creative (Slow speed)".to_string(),
        6 => "Action (High speed)".to_string(),
        7 => "Portrait".to_string(),
        8 => "Landscape".to_string(),
        9 => "Bulb".to_string(),
        // For unknown values, return a descriptive string that includes the raw value
        // to aid in debugging and to ensure no information is lost
        _ => format!("Unknown ({})", value),
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Test all defined ExposureProgram values (0-9) match ExifTool output exactly
    #[test]
    fn test_all_defined_values() {
        assert_eq!(format_exposure_program(0), "Not Defined");
        assert_eq!(format_exposure_program(1), "Manual");
        assert_eq!(format_exposure_program(2), "Program AE");
        assert_eq!(format_exposure_program(3), "Aperture-priority AE");
        assert_eq!(format_exposure_program(4), "Shutter speed priority AE");
        assert_eq!(format_exposure_program(5), "Creative (Slow speed)");
        assert_eq!(format_exposure_program(6), "Action (High speed)");
        assert_eq!(format_exposure_program(7), "Portrait");
        assert_eq!(format_exposure_program(8), "Landscape");
        assert_eq!(format_exposure_program(9), "Bulb");
    }

    /// Test that unknown values return a formatted "Unknown (N)" string
    #[test]
    fn test_unknown_values() {
        // Values just outside the defined range
        assert_eq!(format_exposure_program(10), "Unknown (10)");
        assert_eq!(format_exposure_program(11), "Unknown (11)");

        // Larger unknown values
        assert_eq!(format_exposure_program(99), "Unknown (99)");
        assert_eq!(format_exposure_program(255), "Unknown (255)");
        assert_eq!(format_exposure_program(1000), "Unknown (1000)");

        // Maximum u32 value
        assert_eq!(
            format_exposure_program(u32::MAX),
            format!("Unknown ({})", u32::MAX)
        );
    }

    /// Test commonly encountered ExposureProgram values in real-world images
    #[test]
    fn test_common_real_world_values() {
        // Manual mode - commonly used by professional photographers
        assert_eq!(format_exposure_program(1), "Manual");

        // Program AE - the most common automatic mode
        assert_eq!(format_exposure_program(2), "Program AE");

        // Aperture priority - popular for controlling depth of field
        assert_eq!(format_exposure_program(3), "Aperture-priority AE");

        // Shutter priority - used for motion control
        assert_eq!(format_exposure_program(4), "Shutter speed priority AE");
    }

    /// Test that the function handles the boundary between defined and undefined values
    #[test]
    fn test_boundary_values() {
        // Last defined value
        assert_eq!(format_exposure_program(9), "Bulb");

        // First undefined value
        assert_eq!(format_exposure_program(10), "Unknown (10)");
    }
}
