//! Canon tag registry with array schemas

use super::super::shared::{array_schemas::*, tag_registry::TagRegistry};

// Re-export existing decoders from canon.rs
// These decoders convert numeric values to human-readable strings
use super::super::canon::{
    CONTRAST, DIGITAL_ZOOM, DRIVE_MODE, EASY_MODE, EXPOSURE_MODE, FLASH_MODE, FOCUS_MODE,
    FOCUS_RANGE, MACRO_MODE, METERING_MODE, QUALITY, SATURATION, SHARPNESS,
};

// ============================================================================
// ARRAY SCHEMAS
// ============================================================================

/// CameraSettings array schema (Tag 0x0001)
/// Contains 18+ camera configuration settings
/// Reference: ExifTool Canon.pm CameraSettings table
static CAMERA_SETTINGS_SCHEMA: ArraySchema = ArraySchema {
    name: "CameraSettings",
    indices: &[
        ArrayIndexDef::with_i16_decoder(1, "MacroMode", &MACRO_MODE),
        ArrayIndexDef::raw(2, "SelfTimer"),
        ArrayIndexDef::with_i16_decoder(3, "Quality", &QUALITY),
        ArrayIndexDef::with_i16_decoder(4, "FlashMode", &FLASH_MODE),
        ArrayIndexDef::with_i16_decoder(5, "DriveMode", &DRIVE_MODE),
        ArrayIndexDef::with_i16_decoder(7, "FocusMode", &FOCUS_MODE),
        ArrayIndexDef::raw(10, "ImageSize"),
        ArrayIndexDef::with_i16_decoder(11, "EasyMode", &EASY_MODE),
        ArrayIndexDef::with_i16_decoder(12, "DigitalZoom", &DIGITAL_ZOOM),
        ArrayIndexDef::with_i16_decoder(13, "Contrast", &CONTRAST),
        ArrayIndexDef::with_i16_decoder(14, "Saturation", &SATURATION),
        ArrayIndexDef::with_i16_decoder(15, "Sharpness", &SHARPNESS),
        ArrayIndexDef::raw(16, "ISO"),
        ArrayIndexDef::with_i16_decoder(17, "MeteringMode", &METERING_MODE),
        ArrayIndexDef::with_i16_decoder(18, "FocusRange", &FOCUS_RANGE),
        ArrayIndexDef::raw(19, "AFPoint"),
        ArrayIndexDef::with_i16_decoder(20, "ExposureMode", &EXPOSURE_MODE),
        ArrayIndexDef::raw(28, "FlashActivity"),
        ArrayIndexDef::raw(32, "FocusContinuous"),
    ],
};

/// ShotInfo array schema (Tag 0x0004)
/// Contains exposure and shooting information
static SHOT_INFO_SCHEMA: ArraySchema = ArraySchema {
    name: "ShotInfo",
    indices: &[
        ArrayIndexDef::raw(1, "AutoISO"),
        ArrayIndexDef::raw(2, "BaseISO"),
        ArrayIndexDef::raw(3, "MeasuredEV"),
        ArrayIndexDef::raw(4, "TargetAperture"),
        ArrayIndexDef::raw(5, "TargetShutterSpeed"),
        ArrayIndexDef::raw(19, "SubjectDistance"),
    ],
};

/// FileInfo array schema (Tag 0x0093)
/// Contains file and shutter count information
static FILE_INFO_SCHEMA: ArraySchema = ArraySchema {
    name: "FileInfo",
    indices: &[
        ArrayIndexDef::raw(1, "FileNumber"),
        ArrayIndexDef::raw(2, "ShutterCountLow"),
        ArrayIndexDef::raw(3, "ShutterCountHigh"),
        // Note: LensID at index 6 needs special handling for lens lookup
    ],
};

/// AFInfo array schema (Tag 0x0012, 0x0026)
/// Contains autofocus information
static AF_INFO_SCHEMA: ArraySchema = ArraySchema {
    name: "AFInfo",
    indices: &[
        ArrayIndexDef::raw(1, "NumAFPoints"),
        ArrayIndexDef::raw(2, "AFImageWidth"),
        ArrayIndexDef::raw(3, "AFImageHeight"),
        ArrayIndexDef::raw(8, "AFPointsInFocus"),
        ArrayIndexDef::raw(9, "AFPointsSelected"),
    ],
};

// ============================================================================
// TAG REGISTRY
// ============================================================================

/// Create Canon tag registry with all tag definitions and array schemas
pub fn canon_registry() -> TagRegistry {
    TagRegistry::new()
        // Simple string tags
        .register_raw(0x0006, "ImageType")
        .register_raw(0x0007, "FirmwareVersion")
        .register_raw(0x0009, "OwnerName")
        .register_raw(0x0095, "LensModel")
        // Simple integer tags
        .register_raw(0x0008, "FileNumber")
        .register_raw(0x000C, "SerialNumber")
        .register_raw(0x0010, "ModelID")
        // Array-based tags
        .register_array_schema(0x0001, &CAMERA_SETTINGS_SCHEMA)
        .register_array_schema(0x0004, &SHOT_INFO_SCHEMA)
        .register_array_schema(0x0093, &FILE_INFO_SCHEMA)
        .register_array_schema(0x0012, &AF_INFO_SCHEMA)
        .register_array_schema(0x0026, &AF_INFO_SCHEMA) // AFInfo2 uses same schema
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Process FileInfo array with special lens lookup handling
pub fn process_file_info_with_lens(
    array: &[i16],
    prefix: &str,
    lens_db: &impl super::super::shared::LensDatabase,
    tags: &mut std::collections::HashMap<String, String>,
) {
    // Process standard fields via schema
    FILE_INFO_SCHEMA.process_i16_array(array, prefix, tags);

    // Special handling for lens ID (index 6)
    if let Some(&lens_id) = array.get(6) {
        if let Some(lens_name) = lens_db.lookup(lens_id as u16) {
            tags.insert(format!("{}:LensType", prefix), lens_name.to_string());
        } else {
            tags.insert(format!("{}:LensID", prefix), lens_id.to_string());
        }
    }
}
