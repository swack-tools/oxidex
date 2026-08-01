//! EXIF Flash (0x9209) bitmap decoding.
//!
//! `Flash` is the one EXIF enum that is not a flat lookup: ExifTool renders it
//! from five bit fields (fired / strobe return / mode / function present /
//! red-eye), which is why it is written out longhand here rather than living in
//! a table with the others.
//!
//! The flat enums that used to sit alongside it -- ColorSpace, Contrast,
//! CustomRendered, ExposureMode, GainControl, LightSource, MeteringMode,
//! Saturation, SceneCaptureType, SensingMethod, Sharpness,
//! SubjectDistanceRange, WhiteBalance and Orientation -- were fourteen
//! `LazyLock<HashMap>` tables reachable only through `decode_exif_enum`, which
//! nothing ever called. `core::formatters::exif_enums` holds the versions the
//! product actually uses. Both are gone; `decode_flash` is what was live.

// =============================================================================
// Flash Bitmap Decoding
// =============================================================================

/// Decode Flash value (tag 0x9209) - bitmap decoding
///
/// The Flash tag is a complex bitmap where different bits indicate different
/// aspects of the flash status. This function decodes all bits and returns
/// a human-readable string matching ExifTool's output format.
///
/// # Bitmap Structure
///
/// | Bits  | Description                                      |
/// |-------|--------------------------------------------------|
/// | 0     | Flash fired (0 = No, 1 = Yes)                    |
/// | 1-2   | Return detection (0 = No strobe, 2 = Not detected, 3 = Detected) |
/// | 3-4   | Flash mode (0 = Unknown, 1 = On, 2 = Off, 3 = Auto) |
/// | 5     | Flash function (0 = Present, 1 = No flash function) |
/// | 6     | Red-eye reduction (0 = No, 1 = Yes)              |
///
/// # Examples
///
/// ```
/// use oxidex::core::exif_enums::decode_flash;
///
/// assert_eq!(decode_flash(0), "No Flash");
/// assert_eq!(decode_flash(1), "Fired");
/// assert_eq!(decode_flash(0x18), "Auto, Did not fire"); // auto mode, not fired
/// assert_eq!(decode_flash(0x19), "Auto, Fired"); // auto mode, fired
/// ```
pub fn decode_flash(value: u32) -> String {
    // Extract individual bit fields from the flash bitmap
    let fired = (value & 0x01) != 0; // Bit 0: flash fired
    let return_val = (value >> 1) & 0x03; // Bits 1-2: strobe return detection
    let mode = (value >> 3) & 0x03; // Bits 3-4: flash mode
    let function = (value >> 5) & 0x01; // Bit 5: flash function present
    let red_eye = (value >> 6) & 0x01; // Bit 6: red-eye reduction

    // Special case: no flash function
    if function == 1 {
        return "No flash function".to_string();
    }

    let mut parts = Vec::new();

    // Flash mode first (if known), then fired status
    // This matches ExifTool's format: "Mode, Fired/Did not fire"
    match mode {
        1 => {
            // Compulsory flash mode (On)
            parts.push("On");
            if fired {
                parts.push("Fired");
            } else {
                parts.push("Did not fire");
            }
        }
        2 => {
            // Compulsory suppression mode (Off)
            parts.push("Off");
            if fired {
                parts.push("Fired");
            } else {
                parts.push("Did not fire");
            }
        }
        3 => {
            // Auto mode
            parts.push("Auto");
            if fired {
                parts.push("Fired");
            } else {
                parts.push("Did not fire");
            }
        }
        _ => {
            // Unknown mode (0) - just show fired status
            if fired {
                parts.push("Fired");
            } else {
                parts.push("No Flash");
            }
        }
    }

    // Red-eye reduction mode
    if red_eye == 1 {
        parts.push("Red-eye reduction");
    }

    // Strobe return detection status (only meaningful if flash was fired)
    match return_val {
        2 => parts.push("Return not detected"),
        3 => parts.push("Return detected"),
        _ => {} // 0 = no strobe return detection function, 1 = reserved
    }

    parts.join(", ")
}

// =============================================================================
// Master Decode Function
// =============================================================================

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flash_decoding() {
        // Basic states (unknown mode)
        assert_eq!(decode_flash(0), "No Flash");
        assert_eq!(decode_flash(1), "Fired");

        // Flash with auto mode (bits 3-4 = 0b11 = 3, shifted left 3 = 0x18)
        // 0x18 = 0b00011000 = not fired + auto mode (bits 3-4)
        assert_eq!(decode_flash(0x18), "Auto, Did not fire");
        // 0x19 = 0b00011001 = fired (bit 0) + auto mode (bits 3-4)
        assert_eq!(decode_flash(0x19), "Auto, Fired");

        // Flash off mode (bits 3-4 = 0b10 = 2, shifted left 3 = 0x10)
        // 0x10 = 0b00010000 = not fired + off mode
        assert_eq!(decode_flash(0x10), "Off, Did not fire");
        // 0x14 = 0b00010100 = not fired + off mode + return not detected
        assert_eq!(decode_flash(0x14), "Off, Did not fire, Return not detected");

        // Flash on mode (bits 3-4 = 0b01 = 1, shifted left 3 = 0x08)
        // 0x08 = 0b00001000 = not fired + on mode
        assert_eq!(decode_flash(0x08), "On, Did not fire");
        // 0x09 = 0b00001001 = fired + on mode
        assert_eq!(decode_flash(0x09), "On, Fired");

        // Return detected (bits 1-2 = 0b11 = 3) - unknown mode
        // 0x07 = 0b00000111 = fired + return detected
        assert_eq!(decode_flash(0x07), "Fired, Return detected");

        // Return not detected (bits 1-2 = 0b10 = 2) - unknown mode
        // 0x05 = 0b00000101 = fired + return not detected
        assert_eq!(decode_flash(0x05), "Fired, Return not detected");

        // No flash function (bit 5)
        // 0x20 = 0b00100000 = no flash function
        assert_eq!(decode_flash(0x20), "No flash function");

        // Red-eye reduction (bit 6) - unknown mode
        // 0x41 = 0b01000001 = fired + red-eye reduction
        assert_eq!(decode_flash(0x41), "Fired, Red-eye reduction");

        // Complex: auto + fired + red-eye
        // 0x59 = 0b01011001 = fired + auto + red-eye
        assert_eq!(decode_flash(0x59), "Auto, Fired, Red-eye reduction");

        // Complex: auto + fired + return detected
        // 0x1F = 0b00011111 = fired + auto + return detected
        assert_eq!(decode_flash(0x1F), "Auto, Fired, Return detected");

        // Complex: on + red-eye
        // 0x49 = 0b01001001 = fired + on + red-eye
        assert_eq!(decode_flash(0x49), "On, Fired, Red-eye reduction");
    }
}
