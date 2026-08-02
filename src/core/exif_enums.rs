//! EXIF Flash (0x9209) decoding.
//!
//! `Flash` reads like a bitfield -- five fields (fired / strobe return / mode /
//! function present / red-eye) -- and this module used to render it that way.
//! ExifTool does not: Exif.pm 0x9209 is `PrintConv => \%flash`, a flat 27-key
//! hash, plus `Flags => 'PrintHex'`. The bits explain how those 27 codes were
//! chosen; they are not how the tag is printed, and the other 229 byte values
//! are not valid Flash codes. The table lives with the rest in
//! `core::formatters::exif_enums`; this module is the alias the tag-comparison
//! harness imports.
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

/// Decode Flash value (tag 0x9209).
///
/// Thin alias for [`crate::core::formatters::exif_enums::format_flash`], which
/// is `%Image::ExifTool::Exif::flash` -- a flat 27-key hash with a `PrintHex`
/// unknown form. Kept as a symbol because the tag-comparison harness calls it
/// by this name.
///
/// This function used to build the label from the Flash byte's bit fields
/// (fired / strobe return / mode / function present / red-eye). Those bits
/// explain how ExifTool's 27 codes were chosen, but ExifTool renders the tag by
/// hash lookup, and 229 of the 256 byte values are not valid Flash codes at
/// all. Scored against `%flash` over every input in 0..=255 the bitwise version
/// was wrong on 236: it invented `No Flash` for `0x02` (ExifTool:
/// `Unknown (0x2)`), collapsed every `0x20`-family code to `No flash function`
/// (so `0x38` printed a real label where ExifTool prints `Unknown (0x38)`), and
/// wrote `On, Fired, Return not detected` for `0x0d` where ExifTool writes
/// `On, Return not detected`.
///
/// # Examples
///
/// ```
/// use oxidex::core::exif_enums::decode_flash;
///
/// assert_eq!(decode_flash(0), "No Flash");
/// assert_eq!(decode_flash(1), "Fired");
/// assert_eq!(decode_flash(0x18), "Auto, Did not fire");
/// assert_eq!(decode_flash(0x19), "Auto, Fired");
/// // Not a Flash code -- ExifTool prints the byte in hex, it does not guess
/// assert_eq!(decode_flash(0x38), "Unknown (0x38)");
/// ```
pub fn decode_flash(value: u32) -> String {
    crate::core::formatters::exif_enums::format_flash(i64::from(value))
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

    /// Codes `%flash` names, asserted against the hash rather than against the
    /// bit layout.
    ///
    /// The version of this test that shipped with the bitwise decoder asserted
    /// `decode_flash(0x49) == "On, Fired, Red-eye reduction"` -- a value
    /// ExifTool never prints; the hash says `On, Red-eye reduction`. It read as
    /// a thorough test (13 assertions, each with a bit-layout comment) while
    /// enshrining the defect, and 23 files in the sample corpus reported that
    /// exact wrong string.
    #[test]
    fn test_flash_decoding() {
        assert_eq!(decode_flash(0x00), "No Flash");
        assert_eq!(decode_flash(0x01), "Fired");
        assert_eq!(decode_flash(0x05), "Fired, Return not detected");
        assert_eq!(decode_flash(0x07), "Fired, Return detected");
        assert_eq!(decode_flash(0x08), "On, Did not fire");
        assert_eq!(decode_flash(0x09), "On, Fired");
        assert_eq!(decode_flash(0x10), "Off, Did not fire");
        assert_eq!(decode_flash(0x14), "Off, Did not fire, Return not detected");
        assert_eq!(decode_flash(0x18), "Auto, Did not fire");
        assert_eq!(decode_flash(0x19), "Auto, Fired");
        assert_eq!(decode_flash(0x1f), "Auto, Fired, Return detected");
        assert_eq!(decode_flash(0x20), "No flash function");
        assert_eq!(decode_flash(0x41), "Fired, Red-eye reduction");
        assert_eq!(decode_flash(0x59), "Auto, Fired, Red-eye reduction");
    }

    /// The six codes the bitwise decoder mis-worded.
    ///
    /// Each of these is a key `%flash` does hold, where bit synthesis inserted a
    /// `Fired,` the hash does not carry.
    #[test]
    fn flash_codes_the_bitwise_decoder_misworded() {
        // was "On, Fired, Return not detected"
        assert_eq!(decode_flash(0x0d), "On, Return not detected");
        // was "On, Fired, Return detected"
        assert_eq!(decode_flash(0x0f), "On, Return detected");
        // was "On, Fired, Red-eye reduction"
        assert_eq!(decode_flash(0x49), "On, Red-eye reduction");
        // was "On, Fired, Red-eye reduction, Return not detected"
        assert_eq!(
            decode_flash(0x4d),
            "On, Red-eye reduction, Return not detected"
        );
        // was "On, Fired, Red-eye reduction, Return detected"
        assert_eq!(decode_flash(0x4f), "On, Red-eye reduction, Return detected");
        // was "Off, Did not fire, Red-eye reduction"
        assert_eq!(decode_flash(0x50), "Off, Red-eye reduction");
    }

    /// Codes `%flash` does not hold print in hex; they are not guessed at.
    ///
    /// The bitwise decoder answered every one of the 229 unnamed byte values
    /// with a plausible label. `0x38` and `0x28` both came back
    /// `No flash function` -- 60 files in the sample corpus, where ExifTool
    /// prints `Unknown (0x38)` and `Unknown (0x28)`.
    #[test]
    fn flash_codes_exiftool_does_not_name_print_in_hex() {
        assert_eq!(decode_flash(0x38), "Unknown (0x38)");
        assert_eq!(decode_flash(0x28), "Unknown (0x28)");
        assert_eq!(decode_flash(0x02), "Unknown (0x2)");
        assert_eq!(decode_flash(0x03), "Unknown (0x3)");
        assert_eq!(decode_flash(0x0a), "Unknown (0xa)");
        assert_eq!(decode_flash(0x11), "Unknown (0x11)");
        assert_eq!(decode_flash(0x21), "Unknown (0x21)");
        assert_eq!(decode_flash(0x60), "Unknown (0x60)");
        assert_eq!(decode_flash(0xff), "Unknown (0xff)");
    }

    /// Exactly 27 of the 256 byte values are Flash codes.
    #[test]
    fn flash_names_exactly_twenty_seven_of_two_hundred_fifty_six_bytes() {
        let named = (0u32..=255)
            .filter(|&v| !decode_flash(v).starts_with("Unknown ("))
            .count();
        assert_eq!(named, 27);
    }
}
