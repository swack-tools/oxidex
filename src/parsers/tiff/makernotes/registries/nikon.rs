//! Nikon tag registry with array schemas
//!
//! Provides declarative tag and array schema definitions for Nikon MakerNotes.
//! Reduces repetitive array extraction code by centralizing tag definitions.

use super::super::shared::{
    array_schemas::*, generic_decoders::SimpleValueDecoder, tag_registry::TagRegistry,
};

// ============================================================================
// VALUE DECODERS
// ============================================================================

/// Decoder for Nikon quality settings
pub const QUALITY: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (1, "VGA Basic"),
    (2, "VGA Normal"),
    (3, "VGA Fine"),
    (4, "SXGA Basic"),
    (5, "SXGA Normal"),
    (6, "SXGA Fine"),
    (7, "XGA Basic"),
    (8, "XGA Normal"),
    (9, "XGA Fine"),
    (10, "UXGA Basic"),
    (11, "UXGA Normal"),
    (12, "UXGA Fine"),
]);

/// Decoder for Nikon white balance settings
pub const WHITE_BALANCE: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0, "Auto"),
    (1, "Daylight"),
    (2, "Incandescent"),
    (3, "Fluorescent"),
    (4, "Cloudy"),
    (5, "Speedlight"),
    (6, "Custom"),
    (7, "Shade"),
    (8, "Kelvin"),
]);

/// Decoder for Nikon focus mode settings
pub const FOCUS_MODE: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0, "AF-S"),
    (1, "AF-C"),
    (2, "AF-A"),
    (3, "MF (Manual)"),
    (4, "AF-S (Single)"),
    (5, "AF-C (Continuous)"),
]);

/// Decoder for Nikon flash settings
pub const FLASH_SETTING: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0, "Normal"),
    (1, "Red-eye Reduction"),
    (2, "Rear Curtain"),
    (3, "Slow Sync"),
    (4, "Red-eye + Slow"),
    (5, "Rear + Slow"),
    (6, "Off"),
]);

/// Decoder for Nikon flash mode settings
pub const FLASH_MODE: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0, "Did Not Fire"),
    (1, "Fired, Manual"),
    (3, "Not Ready"),
    (7, "Fired, External"),
    (8, "Fired, Commander Mode"),
    (9, "Fired, TTL Mode"),
]);

/// Decoder for Nikon shooting mode settings
pub const SHOOTING_MODE: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0, "Single Frame"),
    (1, "Continuous"),
    (2, "Self-timer"),
    (3, "Delayed Remote"),
    (4, "Quick-Response Remote"),
    (5, "Self-timer (Mirror Up)"),
    (6, "Interval Timer"),
]);

/// Decoder for Nikon color space settings
pub const COLOR_SPACE: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (1, "sRGB"),
    (2, "Adobe RGB"),
]);

/// Decoder for Nikon Active D-Lighting settings
pub const ACTIVE_D_LIGHTING: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0, "Off"),
    (1, "Low"),
    (3, "Normal"),
    (5, "High"),
    (7, "Extra High"),
    (8, "Extra High 1"),
    (9, "Extra High 2"),
    (0xFFFF, "Auto"),
]);

/// Decoder for Nikon vignette control settings
pub const VIGNETTE_CONTROL: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0, "Off"),
    (1, "Low"),
    (2, "Normal"),
    (3, "High"),
]);

// ============================================================================
// ARRAY SCHEMAS
// ============================================================================

/// ShotInfo array schema (Tag 0x0091)
/// Contains camera shooting settings and status information
/// Note: Indices may vary by camera model, these are common positions
static SHOT_INFO_SCHEMA: ArraySchema = ArraySchema {
    name: "ShotInfo",
    indices: &[
        ArrayIndexDef::raw(0, "Version"),
        ArrayIndexDef::raw(1, "ShutterCount"),
        ArrayIndexDef::raw(2, "AFPointUsed"),
        ArrayIndexDef::raw(4, "VibrationReduction"),
        ArrayIndexDef::raw(6, "AutoISO"),
        ArrayIndexDef::raw(10, "ColorMode"),
    ],
};

/// LensData array schema (Tag 0x0098)
/// Contains lens information including ID, focal length, aperture, and focus data
/// Type 1 format (D1X, D1H, D100)
static LENS_DATA_SCHEMA: ArraySchema = ArraySchema {
    name: "LensData",
    indices: &[
        ArrayIndexDef::raw(0, "Version"),
        ArrayIndexDef::raw(1, "ExitPupilPosition"),
        ArrayIndexDef::raw(2, "AFAperture"),
        ArrayIndexDef::raw(4, "FocusPosition"),
        ArrayIndexDef::raw(5, "FocusDistance"),
        ArrayIndexDef::raw(6, "FocalLength"),
        ArrayIndexDef::raw(7, "LensID"),
        ArrayIndexDef::raw(8, "LensFStops"),
        ArrayIndexDef::raw(9, "MinFocalLength"),
        ArrayIndexDef::raw(10, "MaxFocalLength"),
        ArrayIndexDef::raw(11, "MaxApertureAtMinFocal"),
        ArrayIndexDef::raw(12, "MaxApertureAtMaxFocal"),
    ],
};

/// ColorBalanceA array schema (Tag 0x0097)
/// Contains white balance RGB coefficients
static COLOR_BALANCE_A_SCHEMA: ArraySchema = ArraySchema {
    name: "WB_RBLevels",
    indices: &[
        ArrayIndexDef::raw(0, "Red"),
        ArrayIndexDef::raw(1, "Blue"),
    ],
};

// ============================================================================
// TAG REGISTRY
// ============================================================================

/// Create Nikon tag registry with all tag definitions and array schemas
///
/// Returns a TagRegistry configured for Nikon MakerNote parsing with:
/// - Array schemas for ShotInfo, LensData, and ColorBalanceA
/// - Simple tag registrations for version, serial number, counts, etc.
pub fn nikon_registry() -> TagRegistry {
    TagRegistry::new()
        // Array-based tags with schemas
        .register_array_schema(0x0091, &SHOT_INFO_SCHEMA) // ShotInfo
        .register_array_schema(0x0098, &LENS_DATA_SCHEMA) // LensData
        .register_array_schema(0x0097, &COLOR_BALANCE_A_SCHEMA) // ColorBalanceA
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Process LensData array with special lens lookup handling
///
/// This function processes the LensData array using the schema, then performs
/// special handling for the lens ID field (index 7) to look up the lens name
/// from the lens database.
///
/// # Arguments
/// * `array` - The u16 array containing lens data
/// * `prefix` - Tag prefix (typically "Nikon")
/// * `lens_db` - Lens database implementing the LensDatabase trait
/// * `tags` - HashMap to populate with extracted tags
pub fn process_lens_data_with_lookup(
    array: &[u16],
    prefix: &str,
    lens_db: &impl super::super::shared::LensDatabase,
    tags: &mut std::collections::HashMap<String, String>,
) {
    // Process standard fields via schema
    LENS_DATA_SCHEMA.process_u16_array(array, prefix, tags);

    // Special handling for lens ID lookup (index 7)
    if let Some(&lens_id) = array.get(7) {
        if let Some(lens_name) = lens_db.lookup(lens_id) {
            tags.insert(
                format!("{}:LensData:LensName", prefix),
                lens_name.to_string(),
            );
        }
    }

    // Special handling for focal length formatting (index 6)
    if let Some(&focal_length) = array.get(6) {
        tags.insert(
            format!("{}:LensData:FocalLengthFormatted", prefix),
            format!("{} mm", focal_length),
        );
    }

    // Special handling for focus distance formatting (index 5)
    if let Some(&focus_distance) = array.get(5) {
        tags.insert(
            format!("{}:LensData:FocusDistanceFormatted", prefix),
            format!("{} mm", focus_distance),
        );
    }

    // Special handling for aperture range formatting
    if let Some(&max_aperture_min) = array.get(11) {
        tags.insert(
            format!("{}:LensData:MaxApertureAtMinFocalFormatted", prefix),
            format!("f/{:.1}", max_aperture_min as f32 / 10.0),
        );
    }

    if let Some(&max_aperture_max) = array.get(12) {
        tags.insert(
            format!("{}:LensData:MaxApertureAtMaxFocalFormatted", prefix),
            format!("f/{:.1}", max_aperture_max as f32 / 10.0),
        );
    }
}

/// Process ShotInfo array with special VR and AutoISO formatting
///
/// This function processes the ShotInfo array using the schema, then performs
/// special formatting for vibration reduction and Auto ISO fields.
///
/// # Arguments
/// * `array` - The u16 array containing shot information
/// * `prefix` - Tag prefix (typically "Nikon")
/// * `tags` - HashMap to populate with extracted tags
pub fn process_shot_info(
    array: &[u16],
    prefix: &str,
    tags: &mut std::collections::HashMap<String, String>,
) {
    // Process standard fields via schema
    SHOT_INFO_SCHEMA.process_u16_array(array, prefix, tags);

    // Special handling for vibration reduction (index 4)
    if let Some(&vr) = array.get(4) {
        let vr_status = if vr == 0 { "Off" } else { "On" };
        tags.insert(
            format!("{}:ShotInfo:VibrationReductionFormatted", prefix),
            vr_status.to_string(),
        );
    }

    // Special handling for Auto ISO formatting (index 6)
    if let Some(&auto_iso) = array.get(6) {
        if auto_iso > 0 {
            tags.insert(
                format!("{}:ShotInfo:AutoISOFormatted", prefix),
                format!("ISO {}", auto_iso),
            );
        }
    }
}

// Decoders are already public via const declarations above and used by nikon.rs via import
