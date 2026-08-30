//! GPS Latitude/Longitude reference value formatter
//!
//! This module provides formatting functions to convert single-character GPS
//! reference values (or their ASCII byte equivalents) to human-readable strings
//! for ExifTool compatibility.
//!
//! # Background
//!
//! EXIF GPS metadata stores directional references as single ASCII characters:
//! - Latitude reference: "N" (North) or "S" (South)
//! - Longitude reference: "E" (East) or "W" (West)
//!
//! ExifTool displays these as full words ("North", "South", "East", "West"),
//! so this module provides the conversion to match that output format.
//!
//! # Supported Input Formats
//!
//! Both functions accept:
//! - Single character strings: "N", "S", "E", "W"
//! - ASCII byte values as strings: "78" (0x4E = 'N'), "83" (0x53 = 'S'), etc.
//!
//! A value the table does not name is wrapped in ExifTool's `Unknown (...)`
//! form rather than passed through, because that is what ExifTool prints.
//!
//! # Why `Unknown (...)`
//!
//! `GPSLatitudeRef` and `GPSDestLatitudeRef` are `PrintConv => \%printConvLatRef`
//! (`GPS.pm:74` and `GPS.pm:245`); `GPSLongitudeRef`/`GPSDestLongitudeRef` are
//! `PrintConv => \%printConvLonRef` (`GPS.pm:91` and `GPS.pm:258`). Those two
//! hashes (`GPS.pm:23-49`) hold `N => 'North'` / `S => 'South'` (resp.
//! `E => 'East'` / `W => 'West'`) plus an `OTHER` sub that opens with
//! `return undef unless $inv` -- i.e. it only ever fires on the *write* side.
//!
//! A HASH `PrintConv` that misses is resolved by `ExifTool.pm:3614-3635`:
//!
//! ```text
//! if (ref $conv eq 'HASH') {
//!     if (not defined($value = $$conv{$val})) {
//!         ...
//!         if ($$conv{OTHER}) { $value = &{$$conv{OTHER}}($val, undef, $conv); }
//!         if (not defined $value) {
//!             if ($$tagInfo{PrintHex} and ...) { $value = sprintf('Unknown (0x%x)',$val); }
//!             else                             { $value = "Unknown ($val)"; }
//!         }
//! ```
//!
//! `OTHER` is handed `$inv = undef` on read, so it returns undef, and neither
//! ref tag sets `PrintHex` -- the miss lands on `"Unknown ($val)"`, with the
//! raw value uninterpolated and *untrimmed*. Verified on the corpus with
//! ExifTool 13.59: 28 of the combined-samples files carry an empty or
//! whitespace `GPSLatitudeRef`/`GPSLongitudeRef` pair, and ExifTool prints
//! `Unknown ()` for the 27 empty ones and `Unknown ( )` for
//! `Samsung/SamsungSGH_G810.jpg`, whose value is a single space.
//!
//! # Examples
//!
//! ```
//! use oxidex::core::formatters::gps_lat_lon_ref::{format_gps_lat_ref, format_gps_lon_ref};
//!
//! // Character inputs
//! assert_eq!(format_gps_lat_ref("N"), "North");
//! assert_eq!(format_gps_lat_ref("S"), "South");
//! assert_eq!(format_gps_lon_ref("E"), "East");
//! assert_eq!(format_gps_lon_ref("W"), "West");
//!
//! // A value the PrintConv hash does not name takes ExifTool's miss form
//! assert_eq!(format_gps_lat_ref("X"), "Unknown (X)");
//! assert_eq!(format_gps_lat_ref(""), "Unknown ()");
//! ```

// -----------------------------------------------------------------------------
// ASCII byte values for GPS reference characters
// These are the decimal representations of the ASCII codes that may appear
// in raw EXIF data when bytes are interpreted as numeric strings.
// -----------------------------------------------------------------------------

/// ASCII code for 'N' (North) - 0x4E in hexadecimal
const ASCII_N: u8 = 0x4E; // 78 decimal

/// ASCII code for 'S' (South) - 0x53 in hexadecimal
const ASCII_S: u8 = 0x53; // 83 decimal

/// ASCII code for 'E' (East) - 0x45 in hexadecimal
const ASCII_E: u8 = 0x45; // 69 decimal

/// ASCII code for 'W' (West) - 0x57 in hexadecimal
const ASCII_W: u8 = 0x57; // 87 decimal

/// The four strings `%printConvLatRef` and `%printConvLonRef` produce.
///
/// ExifTool applies a `PrintConv` exactly once, at the point it prints. OxiDex
/// does not: two producers store the *already print-converted* text as the
/// occurrence's display value, and `resolved_display_value`
/// (`src/cli/tag_resolution.rs:294`) then hands that text straight back to
/// `exiftool_compat::format_tag_value`, which routes it here a second time:
///
/// * Composites. `src/composite/mod.rs:447-454` inserts `Computed::print` as
///   the display value (`xmp_gps_ref`, `src/composite/compute.rs:875-900`,
///   returns `Computed::new("N", "North")`), so `Composite:GPSLatitudeRef`
///   arrives as `North`. Seen on
///   `combined-samples/Samsung/SamsungGalaxyA55_5G.jpg`.
/// * The FLIR parser, `src/parsers/jpeg/flir_parser.rs:1294`, which expands
///   `FLIR.pm:629-637`'s `{ N => 'North', S => 'South' }` itself. Seen on
///   `combined-samples/DJI/DJI_XT2.jpg`.
///
/// Both files are correct today only because the miss branch used to be a
/// pass-through; wrapping unconditionally turned them into
/// `Unknown (North)` / `Unknown (East)` / `Unknown (West)`, verified by
/// building this change without the guard and running `oxidex -j` on both.
/// The double application is the real defect and it lives in those two
/// producers, not here -- this guard only keeps the miss form from making it
/// visible. It costs fidelity in exactly one place: a file whose *raw*
/// GPSLatitudeRef literally spelled `North` would print `North` where
/// ExifTool prints `Unknown (North)`. EXIF gives these tags `Count => 2`
/// (`GPS.pm:73`, `:86`), so such a value is malformed by construction and
/// appears nowhere in the corpus, whereas the two producers above are real
/// and measured.
const ALREADY_CONVERTED: [&str; 4] = ["North", "South", "East", "West"];

/// Whether `value` is one of this module's own outputs coming back around.
fn is_already_converted(value: &str) -> bool {
    ALREADY_CONVERTED.contains(&value)
}

// -----------------------------------------------------------------------------
// Public API
// -----------------------------------------------------------------------------

/// Formats a GPS latitude reference value to a human-readable direction.
///
/// Converts single-character latitude references or their ASCII byte values
/// to the corresponding cardinal direction name for ExifTool compatibility.
///
/// # Arguments
///
/// * `value` - The raw latitude reference value. Can be:
///   - A single character: "N" or "S"
///   - An ASCII byte value as a string: "78" (N) or "83" (S)
///
/// # Returns
///
/// - `"North"` for "N" or 0x4E (78)
/// - `"South"` for "S" or 0x53 (83)
/// - `"Unknown (<value>)"` for anything else, per `ExifTool.pm:3633`
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_lat_lon_ref::format_gps_lat_ref;
///
/// // Standard character inputs
/// assert_eq!(format_gps_lat_ref("N"), "North");
/// assert_eq!(format_gps_lat_ref("S"), "South");
///
/// // Handles whitespace trimming
/// assert_eq!(format_gps_lat_ref(" N "), "North");
///
/// // A miss takes ExifTool's `Unknown (...)` form, interpolating the raw
/// // value untouched (`ExifTool.pm:3633`, `$value = "Unknown ($val)"`).
/// assert_eq!(format_gps_lat_ref("E"), "Unknown (E)");
/// assert_eq!(format_gps_lat_ref("unknown"), "Unknown (unknown)");
/// ```
pub fn format_gps_lat_ref(value: &str) -> String {
    let trimmed = value.trim();

    // First, check for direct character match (most common case)
    match trimmed {
        "N" => return "North".to_string(),
        "S" => return "South".to_string(),
        _ => {}
    }

    // Check if the value is a single byte that matches our expected ASCII codes
    // This handles cases where raw bytes are passed as single-character strings
    if trimmed.len() == 1 {
        let byte = trimmed.as_bytes()[0];
        match byte {
            ASCII_N => return "North".to_string(),
            ASCII_S => return "South".to_string(),
            _ => {}
        }
    }

    // Check if the value is a numeric string representing an ASCII code
    // (e.g., "78" for 'N' or "83" for 'S')
    if let Ok(byte_val) = trimmed.parse::<u8>() {
        match byte_val {
            ASCII_N => return "North".to_string(),
            ASCII_S => return "South".to_string(),
            _ => {}
        }
    }

    // Already-converted input is returned as-is rather than wrapped. See
    // `ALREADY_CONVERTED` for why this layer sees its own output back.
    if is_already_converted(trimmed) {
        return trimmed.to_string();
    }

    // A HASH PrintConv that misses falls through to `"Unknown ($val)"`
    // (`ExifTool.pm:3627-3634`) -- `%printConvLatRef`'s `OTHER` sub returns
    // undef on the read side (`GPS.pm:28`, `return undef unless $inv`) and
    // GPSLatitudeRef/GPSDestLatitudeRef set no `PrintHex`, so neither the
    // OTHER branch nor the `Unknown (0x%x)` branch can fire here. `$val` is
    // interpolated verbatim: ExifTool 13.59 prints `Unknown ( )`, not
    // `Unknown ()`, for Samsung/SamsungSGH_G810.jpg's single-space value, so
    // wrap the caller's string rather than the trimmed one.
    format!("Unknown ({value})")
}

/// Formats a GPS longitude reference value to a human-readable direction.
///
/// Converts single-character longitude references or their ASCII byte values
/// to the corresponding cardinal direction name for ExifTool compatibility.
///
/// # Arguments
///
/// * `value` - The raw longitude reference value. Can be:
///   - A single character: "E" or "W"
///   - An ASCII byte value as a string: "69" (E) or "87" (W)
///
/// # Returns
///
/// - `"East"` for "E" or 0x45 (69)
/// - `"West"` for "W" or 0x57 (87)
/// - `"Unknown (<value>)"` for anything else, per `ExifTool.pm:3633`
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_lat_lon_ref::format_gps_lon_ref;
///
/// // Standard character inputs
/// assert_eq!(format_gps_lon_ref("E"), "East");
/// assert_eq!(format_gps_lon_ref("W"), "West");
///
/// // Handles whitespace trimming
/// assert_eq!(format_gps_lon_ref(" W "), "West");
///
/// // A miss takes ExifTool's `Unknown (...)` form, interpolating the raw
/// // value untouched (`ExifTool.pm:3633`, `$value = "Unknown ($val)"`).
/// assert_eq!(format_gps_lon_ref("N"), "Unknown (N)");
/// assert_eq!(format_gps_lon_ref("unknown"), "Unknown (unknown)");
/// ```
pub fn format_gps_lon_ref(value: &str) -> String {
    let trimmed = value.trim();

    // First, check for direct character match (most common case)
    match trimmed {
        "E" => return "East".to_string(),
        "W" => return "West".to_string(),
        _ => {}
    }

    // Check if the value is a single byte that matches our expected ASCII codes
    // This handles cases where raw bytes are passed as single-character strings
    if trimmed.len() == 1 {
        let byte = trimmed.as_bytes()[0];
        match byte {
            ASCII_E => return "East".to_string(),
            ASCII_W => return "West".to_string(),
            _ => {}
        }
    }

    // Check if the value is a numeric string representing an ASCII code
    // (e.g., "69" for 'E' or "87" for 'W')
    if let Ok(byte_val) = trimmed.parse::<u8>() {
        match byte_val {
            ASCII_E => return "East".to_string(),
            ASCII_W => return "West".to_string(),
            _ => {}
        }
    }

    // Idempotence guard, as on the latitude side.
    if is_already_converted(trimmed) {
        return trimmed.to_string();
    }

    // Same miss path as the latitude ref: `%printConvLonRef`'s `OTHER`
    // (`GPS.pm:42`) is write-only, so `ExifTool.pm:3633` supplies the value.
    format!("Unknown ({value})")
}

// -----------------------------------------------------------------------------
// Unit Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Latitude Reference Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_format_gps_lat_ref_north_character() {
        // Standard "N" character should convert to "North"
        assert_eq!(format_gps_lat_ref("N"), "North");
    }

    #[test]
    fn test_format_gps_lat_ref_south_character() {
        // Standard "S" character should convert to "South"
        assert_eq!(format_gps_lat_ref("S"), "South");
    }

    #[test]
    fn test_format_gps_lat_ref_north_ascii_byte() {
        // ASCII byte value 0x4E (78 decimal) should convert to "North"
        assert_eq!(format_gps_lat_ref("78"), "North");
    }

    #[test]
    fn test_format_gps_lat_ref_south_ascii_byte() {
        // ASCII byte value 0x53 (83 decimal) should convert to "South"
        assert_eq!(format_gps_lat_ref("83"), "South");
    }

    #[test]
    fn test_format_gps_lat_ref_with_whitespace() {
        // Should handle leading/trailing whitespace
        assert_eq!(format_gps_lat_ref(" N "), "North");
        assert_eq!(format_gps_lat_ref("\tS\n"), "South");
        assert_eq!(format_gps_lat_ref("  78  "), "North");
    }

    #[test]
    fn test_format_gps_lat_ref_unknown_value() {
        // `%printConvLatRef` (GPS.pm:23-35) names only N and S, and its OTHER
        // sub is write-only, so every other value lands on ExifTool.pm:3633's
        // `"Unknown ($val)"`.
        assert_eq!(format_gps_lat_ref("E"), "Unknown (E)");
        assert_eq!(format_gps_lat_ref("W"), "Unknown (W)");
        assert_eq!(format_gps_lat_ref("X"), "Unknown (X)");
        assert_eq!(format_gps_lat_ref("unknown"), "Unknown (unknown)");
    }

    /// The two shapes this actually takes on the corpus, both read back from
    /// `exiftool-pinned.sh -json -G1 -GPSLatitudeRef` at 13.59: 27 of the
    /// combined-samples files hold an empty value and print `Unknown ()`,
    /// and Samsung/SamsungSGH_G810.jpg holds a single space and prints
    /// `Unknown ( )` -- ExifTool does not trim before interpolating.
    #[test]
    fn test_format_gps_lat_ref_empty_and_space_are_not_trimmed() {
        assert_eq!(format_gps_lat_ref(""), "Unknown ()");
        assert_eq!(format_gps_lat_ref(" "), "Unknown ( )");
    }

    #[test]
    fn test_format_gps_lat_ref_invalid_numeric() {
        // Numbers outside the ASCII codes for N/S are misses like any other.
        assert_eq!(format_gps_lat_ref("0"), "Unknown (0)");
        assert_eq!(format_gps_lat_ref("255"), "Unknown (255)");
        assert_eq!(format_gps_lat_ref("999"), "Unknown (999)"); // Too large for u8
        assert_eq!(format_gps_lat_ref("-1"), "Unknown (-1)");
    }

    // -------------------------------------------------------------------------
    // Longitude Reference Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_format_gps_lon_ref_east_character() {
        // Standard "E" character should convert to "East"
        assert_eq!(format_gps_lon_ref("E"), "East");
    }

    #[test]
    fn test_format_gps_lon_ref_west_character() {
        // Standard "W" character should convert to "West"
        assert_eq!(format_gps_lon_ref("W"), "West");
    }

    #[test]
    fn test_format_gps_lon_ref_east_ascii_byte() {
        // ASCII byte value 0x45 (69 decimal) should convert to "East"
        assert_eq!(format_gps_lon_ref("69"), "East");
    }

    #[test]
    fn test_format_gps_lon_ref_west_ascii_byte() {
        // ASCII byte value 0x57 (87 decimal) should convert to "West"
        assert_eq!(format_gps_lon_ref("87"), "West");
    }

    #[test]
    fn test_format_gps_lon_ref_with_whitespace() {
        // Should handle leading/trailing whitespace
        assert_eq!(format_gps_lon_ref(" E "), "East");
        assert_eq!(format_gps_lon_ref("\tW\n"), "West");
        assert_eq!(format_gps_lon_ref("  69  "), "East");
    }

    #[test]
    fn test_format_gps_lon_ref_unknown_value() {
        // `%printConvLonRef` (GPS.pm:37-49) is the E/W twin of the latitude
        // hash, with the same write-only OTHER, so misses take the same form.
        assert_eq!(format_gps_lon_ref("N"), "Unknown (N)");
        assert_eq!(format_gps_lon_ref("S"), "Unknown (S)");
        assert_eq!(format_gps_lon_ref("X"), "Unknown (X)");
        assert_eq!(format_gps_lon_ref("unknown"), "Unknown (unknown)");
    }

    /// Longitude half of the corpus evidence: the same 27 files print
    /// `Unknown ()` for GPSLongitudeRef and SamsungSGH_G810.jpg prints
    /// `Unknown ( )`, under ExifTool 13.59.
    #[test]
    fn test_format_gps_lon_ref_empty_and_space_are_not_trimmed() {
        assert_eq!(format_gps_lon_ref(""), "Unknown ()");
        assert_eq!(format_gps_lon_ref(" "), "Unknown ( )");
    }

    #[test]
    fn test_format_gps_lon_ref_invalid_numeric() {
        // Numbers outside the ASCII codes for E/W are misses like any other.
        assert_eq!(format_gps_lon_ref("0"), "Unknown (0)");
        assert_eq!(format_gps_lon_ref("255"), "Unknown (255)");
        assert_eq!(format_gps_lon_ref("999"), "Unknown (999)"); // Too large for u8
        assert_eq!(format_gps_lon_ref("-1"), "Unknown (-1)");
    }

    // -------------------------------------------------------------------------
    // Edge Cases and Boundary Conditions
    // -------------------------------------------------------------------------

    /// Composites (`src/composite/mod.rs:447-454`) and the FLIR parser
    /// (`src/parsers/jpeg/flir_parser.rs:1294`) store the print form as the
    /// display value, so this function is handed its own output a second
    /// time. Wrapping that would turn two corpus files that match ExifTool
    /// today -- Samsung/SamsungGalaxyA55_5G.jpg's `Composite:GPSLatitudeRef`
    /// (`North`) and DJI/DJI_XT2.jpg's `FLIR:GPSLongitudeRef` (`West`) --
    /// into `Unknown (North)` / `Unknown (West)`.
    #[test]
    fn test_already_converted_values_are_idempotent() {
        assert_eq!(format_gps_lat_ref("North"), "North");
        assert_eq!(format_gps_lat_ref("South"), "South");
        assert_eq!(format_gps_lon_ref("East"), "East");
        assert_eq!(format_gps_lon_ref("West"), "West");
    }

    #[test]
    fn test_case_sensitivity() {
        // Perl hash keys are case-sensitive, so a lowercase letter is a miss.
        assert_eq!(format_gps_lat_ref("n"), "Unknown (n)");
        assert_eq!(format_gps_lat_ref("s"), "Unknown (s)");
        assert_eq!(format_gps_lon_ref("e"), "Unknown (e)");
        assert_eq!(format_gps_lon_ref("w"), "Unknown (w)");
    }

    #[test]
    fn test_preserves_original_whitespace_on_unknown() {
        // ExifTool.pm:3633 interpolates `$val` with no trim, so the wrapped
        // text is the caller's string verbatim -- whitespace included.
        assert_eq!(format_gps_lat_ref(" unknown "), "Unknown ( unknown )");
        assert_eq!(format_gps_lon_ref(" unknown "), "Unknown ( unknown )");
    }

    #[test]
    fn test_ascii_byte_values_are_correct() {
        // Verify our constants match actual ASCII values
        assert_eq!(ASCII_N, b'N');
        assert_eq!(ASCII_S, b'S');
        assert_eq!(ASCII_E, b'E');
        assert_eq!(ASCII_W, b'W');

        // Verify decimal string parsing works correctly
        assert_eq!(format!("{}", b'N'), "78");
        assert_eq!(format!("{}", b'S'), "83");
        assert_eq!(format!("{}", b'E'), "69");
        assert_eq!(format!("{}", b'W'), "87");
    }

    #[test]
    fn test_raw_byte_single_char_handling() {
        // Test that single raw bytes work correctly
        // This simulates cases where a byte is passed as a character
        let n_byte = String::from_utf8(vec![0x4E]).unwrap();
        let s_byte = String::from_utf8(vec![0x53]).unwrap();
        let e_byte = String::from_utf8(vec![0x45]).unwrap();
        let w_byte = String::from_utf8(vec![0x57]).unwrap();

        assert_eq!(format_gps_lat_ref(&n_byte), "North");
        assert_eq!(format_gps_lat_ref(&s_byte), "South");
        assert_eq!(format_gps_lon_ref(&e_byte), "East");
        assert_eq!(format_gps_lon_ref(&w_byte), "West");
    }
}
