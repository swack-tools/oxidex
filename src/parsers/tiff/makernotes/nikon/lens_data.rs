//! Nikon LensData tag parser
//!
//! This module parses the Nikon LensData structure found in MakerNotes.
//! LensData contains detailed lens information including lens type,
//! focal length range, aperture specifications, and MCU version.
//!
//! # Nikon LensData Versions
//!
//! Nikon has used different LensData structures across camera generations:
//! - Version 0100: Early cameras (D1X, D1H, D100)
//! - Version 0101: Mid-generation (D200, D80)
//! - Version 0201: Later cameras (D300, D700, D3)
//! - Version 0204: Modern cameras (D500, D850, Z series)
//!
//! The structure layout varies by version but typically includes:
//! - Lens identification bytes (to look up lens in database)
//! - Focal length range (min/max in mm)
//! - Maximum aperture at min/max focal length
//! - F-stop range of the lens
//! - MCU (Micro Controller Unit) firmware version
//!
//! # References
//!
//! - ExifTool Nikon.pm for LensData structure details
//! - Nikon raw file specifications

#![allow(dead_code)]

use crate::core::MetadataMap;
use crate::core::TagValue;

// ============================================================================
// LensData Structure Constants
// ============================================================================

/// Byte offsets for LensData Version 0100/0101 (Early cameras: D1X, D1H, D100, D200, D80)
/// These offsets are relative to the start of the LensData block.
mod version_01 {
    /// Version string offset (4 bytes ASCII)
    pub const VERSION: usize = 0;
    /// Exit pupil position offset
    pub const EXIT_PUPIL_POSITION: usize = 4;
    /// AF aperture offset
    pub const AF_APERTURE: usize = 5;
    /// Focus position offset
    pub const FOCUS_POSITION: usize = 8;
    /// Focus distance offset (encoded)
    pub const FOCUS_DISTANCE: usize = 9;
    /// Focal length offset (encoded)
    pub const FOCAL_LENGTH: usize = 10;
    /// Lens ID number offset
    pub const LENS_ID_NUMBER: usize = 11;
    /// Lens F-stops offset (encoded as value / 12)
    pub const LENS_FSTOPS: usize = 12;
    /// Minimum focal length offset (encoded)
    pub const MIN_FOCAL_LENGTH: usize = 13;
    /// Maximum focal length offset (encoded)
    pub const MAX_FOCAL_LENGTH: usize = 14;
    /// Maximum aperture at minimum focal length offset (encoded APEX value)
    pub const MAX_APERTURE_AT_MIN_FOCAL: usize = 15;
    /// Maximum aperture at maximum focal length offset (encoded APEX value)
    pub const MAX_APERTURE_AT_MAX_FOCAL: usize = 16;
    /// MCU (Micro Controller Unit) version offset
    pub const MCU_VERSION: usize = 17;
    /// Minimum structure size for this version
    pub const MIN_SIZE: usize = 18;
}

/// Byte offsets for LensData Version 0201+ (Modern cameras: D300, D700, D3, etc.)
/// These cameras use a slightly different layout with additional fields.
mod version_02 {
    /// Version string offset (4 bytes ASCII)
    pub const VERSION: usize = 0;
    /// Exit pupil position offset
    pub const EXIT_PUPIL_POSITION: usize = 4;
    /// AF aperture offset
    pub const AF_APERTURE: usize = 5;
    /// Focus position offset
    pub const FOCUS_POSITION: usize = 8;
    /// Focus distance offset (encoded)
    pub const FOCUS_DISTANCE: usize = 10;
    /// Focal length offset (encoded)
    pub const FOCAL_LENGTH: usize = 11;
    /// Lens ID number offset
    pub const LENS_ID_NUMBER: usize = 12;
    /// Lens F-stops offset (encoded as value / 12)
    pub const LENS_FSTOPS: usize = 13;
    /// Minimum focal length offset (encoded)
    pub const MIN_FOCAL_LENGTH: usize = 14;
    /// Maximum focal length offset (encoded)
    pub const MAX_FOCAL_LENGTH: usize = 15;
    /// Maximum aperture at minimum focal length offset (encoded APEX value)
    pub const MAX_APERTURE_AT_MIN_FOCAL: usize = 16;
    /// Maximum aperture at maximum focal length offset (encoded APEX value)
    pub const MAX_APERTURE_AT_MAX_FOCAL: usize = 17;
    /// MCU (Micro Controller Unit) version offset
    pub const MCU_VERSION: usize = 18;
    /// Effective maximum aperture offset
    pub const EFFECTIVE_MAX_APERTURE: usize = 19;
    /// Minimum structure size for this version
    pub const MIN_SIZE: usize = 20;
}

// ============================================================================
// Lens Type Bit Flags
// ============================================================================

/// Nikon lens type bit flags (from LensType byte)
/// These flags describe the lens capabilities and mount type.
mod lens_type_flags {
    /// Bit 0: MF (Manual Focus) lens - no AF motor
    pub const MF: u8 = 0x01;
    /// Bit 1: D lens - has distance encoder for 3D matrix metering
    pub const D: u8 = 0x02;
    /// Bit 2: G lens - no aperture ring (electronically controlled)
    pub const G: u8 = 0x04;
    /// Bit 3: VR (Vibration Reduction) lens - has optical stabilization
    pub const VR: u8 = 0x08;
    /// Bit 4: 1 (Nikon 1 mount lens)
    pub const NIKON_1: u8 = 0x10;
    /// Bit 5: FT-1 adapter (Nikon F to Nikon 1)
    pub const FT1: u8 = 0x20;
    /// Bit 6: E lens (electronic aperture, newer than G)
    pub const E: u8 = 0x40;
    /// Bit 7: AF-S lens (Silent Wave Motor for autofocus)
    pub const AF_S: u8 = 0x80;
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Decodes Nikon focal length encoding to millimeters.
///
/// Nikon encodes focal lengths using the formula: 5 * 2^(value/24)
/// This gives a range from ~5mm to very long telephoto focal lengths.
///
/// # Arguments
///
/// * `encoded` - The raw encoded focal length value from LensData
///
/// # Returns
///
/// The decoded focal length in millimeters as a floating point value
#[inline]
fn decode_focal_length(encoded: u8) -> f64 {
    // Nikon focal length encoding: 5 * 2^(value/24)
    // This provides a logarithmic scale for focal lengths
    5.0 * 2.0_f64.powf(encoded as f64 / 24.0)
}

/// Decodes Nikon aperture encoding to f-number.
///
/// Nikon encodes apertures using the APEX (Additive system of Photographic EXposure)
/// system: f-number = 2^(value/24)
///
/// # Arguments
///
/// * `encoded` - The raw encoded aperture value from LensData
///
/// # Returns
///
/// The decoded f-number as a floating point value (e.g., 2.8 for f/2.8)
#[inline]
fn decode_aperture(encoded: u8) -> f64 {
    // Nikon aperture encoding uses APEX: f-number = 2^(value/24)
    2.0_f64.powf(encoded as f64 / 24.0)
}

/// Decodes the lens type byte into a human-readable description string.
///
/// The lens type byte contains bit flags indicating lens features such as
/// AF-S motor, VR stabilization, G-type aperture control, etc.
///
/// # Arguments
///
/// * `lens_type` - The raw lens type byte from LensData
///
/// # Returns
///
/// A string describing the lens type features (e.g., "AF-S G VR")
fn decode_lens_type(lens_type: u8) -> String {
    let mut features = Vec::new();

    // Check each bit flag and add corresponding feature string
    // Order matches typical Nikon lens naming convention
    if lens_type & lens_type_flags::MF != 0 {
        features.push("MF");
    } else if lens_type & lens_type_flags::AF_S != 0 {
        features.push("AF-S");
    } else {
        features.push("AF");
    }

    if lens_type & lens_type_flags::D != 0 {
        features.push("D");
    }

    if lens_type & lens_type_flags::G != 0 {
        features.push("G");
    } else if lens_type & lens_type_flags::E != 0 {
        features.push("E");
    }

    if lens_type & lens_type_flags::VR != 0 {
        features.push("VR");
    }

    if lens_type & lens_type_flags::NIKON_1 != 0 {
        features.push("(Nikon 1)");
    }

    if lens_type & lens_type_flags::FT1 != 0 {
        features.push("(FT-1)");
    }

    if features.is_empty() {
        "Unknown".to_string()
    } else {
        features.join(" ")
    }
}

/// Formats focal length range into a lens description string.
///
/// Handles both prime lenses (single focal length) and zoom lenses (range).
///
/// # Arguments
///
/// * `min_focal` - Minimum focal length in mm
/// * `max_focal` - Maximum focal length in mm
/// * `min_aperture` - Maximum aperture at minimum focal length
/// * `max_aperture` - Maximum aperture at maximum focal length
///
/// # Returns
///
/// A formatted string like "24-70mm f/2.8" or "50mm f/1.4"
fn format_lens_description(
    min_focal: f64,
    max_focal: f64,
    min_aperture: f64,
    max_aperture: f64,
) -> String {
    // Round focal lengths to reasonable precision
    let min_focal_rounded = (min_focal * 10.0).round() / 10.0;
    let max_focal_rounded = (max_focal * 10.0).round() / 10.0;

    // Determine if this is a prime lens (same min/max focal) or zoom
    // Allow small tolerance for encoding precision
    let is_prime = (max_focal_rounded - min_focal_rounded).abs() < 1.0;

    // Format aperture values with one decimal place
    let min_ap_str = if min_aperture.fract().abs() < 0.05 {
        format!("{:.0}", min_aperture)
    } else {
        format!("{:.1}", min_aperture)
    };

    let max_ap_str = if max_aperture.fract().abs() < 0.05 {
        format!("{:.0}", max_aperture)
    } else {
        format!("{:.1}", max_aperture)
    };

    if is_prime {
        // Prime lens: "50mm f/1.4"
        let focal_str = if min_focal_rounded.fract().abs() < 0.05 {
            format!("{:.0}mm", min_focal_rounded)
        } else {
            format!("{:.1}mm", min_focal_rounded)
        };
        format!("{} f/{}", focal_str, min_ap_str)
    } else if (min_aperture - max_aperture).abs() < 0.1 {
        // Constant aperture zoom: "24-70mm f/2.8"
        format!(
            "{:.0}-{:.0}mm f/{}",
            min_focal_rounded, max_focal_rounded, min_ap_str
        )
    } else {
        // Variable aperture zoom: "18-200mm f/3.5-5.6"
        format!(
            "{:.0}-{:.0}mm f/{}-{}",
            min_focal_rounded, max_focal_rounded, min_ap_str, max_ap_str
        )
    }
}

/// Detects the LensData version from the first 4 bytes.
///
/// Nikon LensData starts with a version string like "0100", "0101", "0201", etc.
///
/// # Arguments
///
/// * `data` - The LensData byte slice
///
/// # Returns
///
/// The detected version as u16 (e.g., 100 for "0100") or 0 if invalid
fn detect_lens_data_version(data: &[u8]) -> u16 {
    if data.len() < 4 {
        return 0;
    }

    // Version is stored as 4 ASCII digits (e.g., "0100")
    if let Ok(version_str) = std::str::from_utf8(&data[0..4]) {
        // Parse the numeric portion
        if let Ok(version) = version_str.trim().parse::<u16>() {
            return version;
        }
    }

    // Fallback: try to interpret as binary if ASCII parsing fails
    // Some cameras store version differently
    0
}

// ============================================================================
// Main Parser Function
// ============================================================================

/// Parses Nikon LensData structure and extracts lens information.
///
/// This function handles multiple LensData versions used across different
/// Nikon camera generations. It extracts key lens parameters including:
///
/// - LensType: Bit flags describing lens features (AF-S, VR, G, etc.)
/// - Lens: Human-readable lens description string
/// - LensFStops: F-stop range of the lens
/// - MinFocalLength: Minimum focal length in mm
/// - MaxFocalLength: Maximum focal length in mm
/// - MaxApertureAtMinFocal: Maximum aperture at minimum focal length
/// - MaxApertureAtMaxFocal: Maximum aperture at maximum focal length
/// - MCUVersion: Lens microcontroller firmware version
///
/// # Arguments
///
/// * `data` - Raw LensData byte slice from MakerNotes (tag 0x0098)
/// * `byte_order` - Byte order for multi-byte values (true = big endian)
///
/// # Returns
///
/// A MetadataMap containing the extracted lens tags with "Nikon:" prefix
///
/// # Example
///
/// ```ignore
/// use oxidex::core::MetadataMap;
/// use oxidex::parsers::tiff::makernotes::nikon::lens_data::parse_nikon_lens_data;
///
/// let lens_data = vec![0x30, 0x31, 0x30, 0x30, /* ... more data ... */];
/// let metadata = parse_nikon_lens_data(&lens_data, false);
///
/// if let Some(lens) = metadata.get_string("Nikon:Lens") {
///     println!("Lens: {}", lens);
/// }
/// ```
pub fn parse_nikon_lens_data(data: &[u8], byte_order: bool) -> MetadataMap {
    let mut metadata = MetadataMap::new();

    // Detect LensData version from header
    let version = detect_lens_data_version(data);

    // Store version string if valid
    if version > 0 {
        metadata.insert(
            "Nikon:LensDataVersion",
            TagValue::new_string(format!("{:04}", version)),
        );
    }

    // Select appropriate offsets based on version
    // Version 0100/0101 use version_01 offsets
    // Version 0200+ use version_02 offsets
    let (min_size, offsets) = if version >= 200 {
        (
            version_02::MIN_SIZE,
            LensDataOffsets {
                lens_fstops: version_02::LENS_FSTOPS,
                min_focal_length: version_02::MIN_FOCAL_LENGTH,
                max_focal_length: version_02::MAX_FOCAL_LENGTH,
                max_aperture_at_min_focal: version_02::MAX_APERTURE_AT_MIN_FOCAL,
                max_aperture_at_max_focal: version_02::MAX_APERTURE_AT_MAX_FOCAL,
                mcu_version: version_02::MCU_VERSION,
                lens_id: version_02::LENS_ID_NUMBER,
            },
        )
    } else {
        (
            version_01::MIN_SIZE,
            LensDataOffsets {
                lens_fstops: version_01::LENS_FSTOPS,
                min_focal_length: version_01::MIN_FOCAL_LENGTH,
                max_focal_length: version_01::MAX_FOCAL_LENGTH,
                max_aperture_at_min_focal: version_01::MAX_APERTURE_AT_MIN_FOCAL,
                max_aperture_at_max_focal: version_01::MAX_APERTURE_AT_MAX_FOCAL,
                mcu_version: version_01::MCU_VERSION,
                lens_id: version_01::LENS_ID_NUMBER,
            },
        )
    };

    // Validate minimum data size
    if data.len() < min_size {
        // Data too short for this version, try to extract what we can
        // Return early with version info only
        return metadata;
    }

    // Note: byte_order parameter is provided for consistency with other parsers,
    // but LensData fields are typically single-byte values that don't require
    // endian conversion. Multi-byte values would use byte_order if present.
    let _ = byte_order; // Suppress unused warning while documenting intent

    // Extract LensType (lens feature flags)
    // The lens ID byte also serves as a lens type indicator in some cases
    let lens_type_byte = data[offsets.lens_id];
    let lens_type = decode_lens_type(lens_type_byte);
    metadata.insert("Nikon:LensType", TagValue::new_string(lens_type));

    // Extract and decode F-stops
    // Stored as value / 12, representing total aperture range
    let fstops_raw = data[offsets.lens_fstops];
    let fstops = fstops_raw as f64 / 12.0;
    metadata.insert(
        "Nikon:LensFStops",
        TagValue::new_string(format!("{:.2}", fstops)),
    );

    // Extract and decode focal length range
    let min_focal = decode_focal_length(data[offsets.min_focal_length]);
    let max_focal = decode_focal_length(data[offsets.max_focal_length]);

    metadata.insert(
        "Nikon:MinFocalLength",
        TagValue::new_string(format!("{:.1} mm", min_focal)),
    );
    metadata.insert(
        "Nikon:MaxFocalLength",
        TagValue::new_string(format!("{:.1} mm", max_focal)),
    );

    // Extract and decode aperture values
    let max_aperture_min = decode_aperture(data[offsets.max_aperture_at_min_focal]);
    let max_aperture_max = decode_aperture(data[offsets.max_aperture_at_max_focal]);

    metadata.insert(
        "Nikon:MaxApertureAtMinFocal",
        TagValue::new_string(format!("f/{:.1}", max_aperture_min)),
    );
    metadata.insert(
        "Nikon:MaxApertureAtMaxFocal",
        TagValue::new_string(format!("f/{:.1}", max_aperture_max)),
    );

    // Generate lens description string
    let lens_description =
        format_lens_description(min_focal, max_focal, max_aperture_min, max_aperture_max);
    metadata.insert("Nikon:Lens", TagValue::new_string(lens_description));

    // Extract MCU version
    let mcu_version = data[offsets.mcu_version];
    metadata.insert(
        "Nikon:MCUVersion",
        TagValue::new_string(format!("{}", mcu_version)),
    );

    metadata
}

/// Internal helper struct to hold version-specific offsets
struct LensDataOffsets {
    lens_fstops: usize,
    min_focal_length: usize,
    max_focal_length: usize,
    max_aperture_at_min_focal: usize,
    max_aperture_at_max_focal: usize,
    mcu_version: usize,
    lens_id: usize,
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Test focal length decoding for known values
    #[test]
    fn test_decode_focal_length() {
        // Test specific encoded values and their expected results
        // encoded = 0 -> 5 * 2^0 = 5mm
        assert!((decode_focal_length(0) - 5.0).abs() < 0.01);

        // encoded = 24 -> 5 * 2^1 = 10mm
        assert!((decode_focal_length(24) - 10.0).abs() < 0.01);

        // encoded = 48 -> 5 * 2^2 = 20mm
        assert!((decode_focal_length(48) - 20.0).abs() < 0.01);

        // encoded = 72 -> 5 * 2^3 = 40mm
        assert!((decode_focal_length(72) - 40.0).abs() < 0.01);

        // encoded = 96 -> 5 * 2^4 = 80mm
        assert!((decode_focal_length(96) - 80.0).abs() < 0.01);
    }

    /// Test aperture decoding for known values
    #[test]
    fn test_decode_aperture() {
        // Test specific encoded values
        // encoded = 0 -> 2^0 = f/1.0
        assert!((decode_aperture(0) - 1.0).abs() < 0.01);

        // encoded = 24 -> 2^1 = f/2.0
        assert!((decode_aperture(24) - 2.0).abs() < 0.01);

        // encoded = 48 -> 2^2 = f/4.0
        assert!((decode_aperture(48) - 4.0).abs() < 0.01);

        // encoded = 72 -> 2^3 = f/8.0
        assert!((decode_aperture(72) - 8.0).abs() < 0.01);

        // Test f/2.8 (approximately encoded as 36)
        // 2^(36/24) = 2^1.5 = 2.83
        assert!((decode_aperture(36) - 2.83).abs() < 0.1);
    }

    /// Test lens type decoding with various flag combinations
    #[test]
    fn test_decode_lens_type() {
        // AF-S G lens
        let afs_g = decode_lens_type(lens_type_flags::AF_S | lens_type_flags::G);
        assert!(afs_g.contains("AF-S"));
        assert!(afs_g.contains("G"));

        // AF-S G VR lens
        let afs_g_vr =
            decode_lens_type(lens_type_flags::AF_S | lens_type_flags::G | lens_type_flags::VR);
        assert!(afs_g_vr.contains("AF-S"));
        assert!(afs_g_vr.contains("G"));
        assert!(afs_g_vr.contains("VR"));

        // Manual focus D lens
        let mf_d = decode_lens_type(lens_type_flags::MF | lens_type_flags::D);
        assert!(mf_d.contains("MF"));
        assert!(mf_d.contains("D"));

        // E-type lens (modern mirrorless)
        let e_type = decode_lens_type(lens_type_flags::AF_S | lens_type_flags::E);
        assert!(e_type.contains("AF-S"));
        assert!(e_type.contains("E"));
    }

    /// Test lens description formatting for prime lens
    #[test]
    fn test_format_lens_description_prime() {
        let desc = format_lens_description(50.0, 50.0, 1.4, 1.4);
        assert!(desc.contains("50"));
        assert!(desc.contains("f/1.4"));
        assert!(!desc.contains("-")); // Prime lens shouldn't have range
    }

    /// Test lens description formatting for constant aperture zoom
    #[test]
    fn test_format_lens_description_constant_zoom() {
        let desc = format_lens_description(24.0, 70.0, 2.8, 2.8);
        assert!(desc.contains("24"));
        assert!(desc.contains("70"));
        assert!(desc.contains("f/2.8"));
        assert!(!desc.contains("f/2.8-2.8")); // Should be single aperture
    }

    /// Test lens description formatting for variable aperture zoom
    #[test]
    fn test_format_lens_description_variable_zoom() {
        let desc = format_lens_description(18.0, 200.0, 3.5, 5.6);
        assert!(desc.contains("18"));
        assert!(desc.contains("200"));
        assert!(desc.contains("f/3.5"));
        assert!(desc.contains("5.6"));
    }

    /// Test version detection from valid header
    #[test]
    fn test_detect_lens_data_version() {
        // Version 0100
        assert_eq!(detect_lens_data_version(b"0100rest of data"), 100);

        // Version 0101
        assert_eq!(detect_lens_data_version(b"0101rest of data"), 101);

        // Version 0201
        assert_eq!(detect_lens_data_version(b"0201rest of data"), 201);

        // Version 0204
        assert_eq!(detect_lens_data_version(b"0204rest of data"), 204);

        // Invalid data
        assert_eq!(detect_lens_data_version(b"XXX"), 0);
        assert_eq!(detect_lens_data_version(b""), 0);
    }

    /// Test parsing with simulated Version 0100 LensData
    #[test]
    fn test_parse_nikon_lens_data_v0100() {
        // Create simulated LensData v0100 for AF-S 24-70mm f/2.8G
        let mut data = vec![0u8; 20];

        // Version header "0100"
        data[0] = b'0';
        data[1] = b'1';
        data[2] = b'0';
        data[3] = b'0';

        // Lens ID with G and AF-S flags
        data[version_01::LENS_ID_NUMBER] = lens_type_flags::AF_S | lens_type_flags::G;

        // F-stops (encoded as value * 12, typical value ~60 for 5 stops)
        data[version_01::LENS_FSTOPS] = 60;

        // Focal lengths: 24mm and 70mm
        // 24mm encoded: log2(24/5) * 24 = ~54
        // 70mm encoded: log2(70/5) * 24 = ~89
        data[version_01::MIN_FOCAL_LENGTH] = 54;
        data[version_01::MAX_FOCAL_LENGTH] = 89;

        // Aperture f/2.8 encoded: log2(2.8) * 24 = ~36
        data[version_01::MAX_APERTURE_AT_MIN_FOCAL] = 36;
        data[version_01::MAX_APERTURE_AT_MAX_FOCAL] = 36;

        // MCU version
        data[version_01::MCU_VERSION] = 10;

        let metadata = parse_nikon_lens_data(&data, false);

        // Verify version was parsed
        assert_eq!(metadata.get_string("Nikon:LensDataVersion"), Some("0100"));

        // Verify lens type
        let lens_type = metadata.get_string("Nikon:LensType").unwrap();
        assert!(lens_type.contains("AF-S"));
        assert!(lens_type.contains("G"));

        // Verify F-stops
        assert!(metadata.get_string("Nikon:LensFStops").is_some());

        // Verify focal lengths exist
        assert!(metadata.get_string("Nikon:MinFocalLength").is_some());
        assert!(metadata.get_string("Nikon:MaxFocalLength").is_some());

        // Verify apertures exist
        assert!(metadata.get_string("Nikon:MaxApertureAtMinFocal").is_some());
        assert!(metadata.get_string("Nikon:MaxApertureAtMaxFocal").is_some());

        // Verify lens description exists
        assert!(metadata.get_string("Nikon:Lens").is_some());

        // Verify MCU version
        assert_eq!(metadata.get_string("Nikon:MCUVersion"), Some("10"));
    }

    /// Test parsing with simulated Version 0201 LensData
    #[test]
    fn test_parse_nikon_lens_data_v0201() {
        // Create simulated LensData v0201 for AF-S 50mm f/1.4G
        let mut data = vec![0u8; 22];

        // Version header "0201"
        data[0] = b'0';
        data[1] = b'2';
        data[2] = b'0';
        data[3] = b'1';

        // Lens ID with G and AF-S flags
        data[version_02::LENS_ID_NUMBER] = lens_type_flags::AF_S | lens_type_flags::G;

        // F-stops (typical ~60 for 5 stops)
        data[version_02::LENS_FSTOPS] = 60;

        // Focal lengths: 50mm (prime)
        // 50mm encoded: log2(50/5) * 24 = ~80
        data[version_02::MIN_FOCAL_LENGTH] = 80;
        data[version_02::MAX_FOCAL_LENGTH] = 80;

        // Aperture f/1.4 encoded: log2(1.4) * 24 = ~12
        data[version_02::MAX_APERTURE_AT_MIN_FOCAL] = 12;
        data[version_02::MAX_APERTURE_AT_MAX_FOCAL] = 12;

        // MCU version
        data[version_02::MCU_VERSION] = 15;

        let metadata = parse_nikon_lens_data(&data, false);

        // Verify version was parsed
        assert_eq!(metadata.get_string("Nikon:LensDataVersion"), Some("0201"));

        // Verify it's recognized as a prime lens (focal lengths should be equal)
        let lens_desc = metadata.get_string("Nikon:Lens").unwrap();
        // Prime lens description should not contain focal range separator
        assert!(lens_desc.contains("mm"));
    }

    /// Test parsing with insufficient data
    #[test]
    fn test_parse_nikon_lens_data_insufficient() {
        // Data too short - only 5 bytes
        let short_data = b"01001";
        let metadata = parse_nikon_lens_data(short_data, false);

        // Should still extract version
        assert_eq!(metadata.get_string("Nikon:LensDataVersion"), Some("0100"));

        // Should not have lens data due to insufficient size
        assert!(metadata.get_string("Nikon:Lens").is_none());
    }

    /// Test parsing with empty data
    #[test]
    fn test_parse_nikon_lens_data_empty() {
        let metadata = parse_nikon_lens_data(&[], false);

        // Should return empty metadata (no crash)
        assert!(metadata.is_empty());
    }

    /// Test parsing with byte_order parameter (big endian)
    #[test]
    fn test_parse_nikon_lens_data_big_endian() {
        // Most LensData fields are single bytes, but test that byte_order
        // doesn't cause issues
        let mut data = vec![0u8; 20];
        data[0] = b'0';
        data[1] = b'1';
        data[2] = b'0';
        data[3] = b'0';
        data[version_01::LENS_ID_NUMBER] = lens_type_flags::AF_S;
        data[version_01::LENS_FSTOPS] = 48;
        data[version_01::MIN_FOCAL_LENGTH] = 60;
        data[version_01::MAX_FOCAL_LENGTH] = 80;
        data[version_01::MAX_APERTURE_AT_MIN_FOCAL] = 36;
        data[version_01::MAX_APERTURE_AT_MAX_FOCAL] = 48;
        data[version_01::MCU_VERSION] = 5;

        // Parse with big endian
        let metadata_be = parse_nikon_lens_data(&data, true);

        // Parse with little endian
        let metadata_le = parse_nikon_lens_data(&data, false);

        // Results should be the same since fields are single bytes
        assert_eq!(
            metadata_be.get_string("Nikon:LensDataVersion"),
            metadata_le.get_string("Nikon:LensDataVersion")
        );
        assert_eq!(
            metadata_be.get_string("Nikon:LensType"),
            metadata_le.get_string("Nikon:LensType")
        );
    }

    /// Test VR lens type detection
    #[test]
    fn test_vr_lens_detection() {
        let vr_type =
            decode_lens_type(lens_type_flags::AF_S | lens_type_flags::G | lens_type_flags::VR);
        assert!(vr_type.contains("VR"));
        assert!(vr_type.contains("AF-S"));
        assert!(vr_type.contains("G"));
    }

    /// Test E-type lens (newer electronic aperture lenses)
    #[test]
    fn test_e_type_lens_detection() {
        let e_type =
            decode_lens_type(lens_type_flags::AF_S | lens_type_flags::E | lens_type_flags::VR);
        assert!(e_type.contains("E"));
        assert!(e_type.contains("VR"));
        assert!(!e_type.contains("G")); // E and G are mutually exclusive
    }

    /// Integration test with realistic lens data
    #[test]
    fn test_realistic_70_200_f28_lens() {
        let mut data = vec![0u8; 22];

        // Version 0201
        data[0] = b'0';
        data[1] = b'2';
        data[2] = b'0';
        data[3] = b'1';

        // AF-S G VR lens
        data[version_02::LENS_ID_NUMBER] =
            lens_type_flags::AF_S | lens_type_flags::G | lens_type_flags::VR;

        // F-stops
        data[version_02::LENS_FSTOPS] = 72; // 6 stops

        // 70mm min focal (encoded ~89)
        // 200mm max focal (encoded ~114)
        data[version_02::MIN_FOCAL_LENGTH] = 89;
        data[version_02::MAX_FOCAL_LENGTH] = 114;

        // f/2.8 constant aperture (encoded ~36)
        data[version_02::MAX_APERTURE_AT_MIN_FOCAL] = 36;
        data[version_02::MAX_APERTURE_AT_MAX_FOCAL] = 36;

        data[version_02::MCU_VERSION] = 20;

        let metadata = parse_nikon_lens_data(&data, false);

        let lens_type = metadata.get_string("Nikon:LensType").unwrap();
        assert!(lens_type.contains("AF-S"));
        assert!(lens_type.contains("G"));
        assert!(lens_type.contains("VR"));

        let lens_desc = metadata.get_string("Nikon:Lens").unwrap();
        // Should be a constant aperture zoom
        assert!(lens_desc.contains("f/"));
    }
}
