//! Panasonic tag registry
//!
//! This module provides TagRegistry definitions for Panasonic MakerNotes.
//! Panasonic uses a straightforward tag structure with mostly simple value types
//! (strings, integers, and enumerated values) and no complex array-based tags.
//!
//! ## Supported Tags
//! This registry covers the majority of Panasonic MakerNote tags including:
//! - Basic camera settings (Quality, WhiteBalance, FocusMode)
//! - Image processing (Contrast, Saturation, Sharpness, NoiseReduction)
//! - Special modes (BurstMode, HDR, IntelligentExposure, PhotoStyle)
//! - Lens and sensor data (LensType, ImageStabilization, AFAreaMode)
//! - Supplementary information (Audio, TextStamp, Location, BabyAge)

use super::super::shared::{generic_decoders::*, tag_registry::TagRegistry};

// Re-export decoders from panasonic.rs
// These decoders are defined using const_decoder! macros in the main parser
use super::super::panasonic::{
    ADVANCED_SCENE_TYPE, AF_AREA_MODE, AF_ASSIST_LAMP, AUDIO, BRACKET_SETTINGS, BURST_MODE,
    BURST_SPEED, CLEAR_RETOUCH, COLOR_EFFECT, CONTRAST_MODE, CONVERSION_LENS, FACE_DETECTION,
    FILM_MODE, FLASH_CURTAIN, FLASH_WARNING, FOCUS_MODE, HDR, IMAGE_STABILIZATION,
    INTELLIGENT_D_RANGE, INTELLIGENT_EXPOSURE, INTELLIGENT_RESOLUTION, INTERNAL_ND_FILTER,
    LONG_EXPOSURE_NR, MACRO_MODE, NOISE_REDUCTION, OPTICAL_ZOOM_MODE, PHOTO_STYLE, ROTATION,
    SELF_TIMER_MODE, SHADING_COMPENSATION, SHOOTING_MODE, SHUTTER_TYPE, SWEEP_PANORAMA_DIRECTION,
    TEXT_STAMP, TIMER_RECORDING, TOUCH_AE, WHITE_BALANCE, WORLD_TIME_LOCATION,
};

// ============================================================================
// TAG REGISTRY
// ============================================================================

/// Create Panasonic tag registry with all tag definitions
///
/// This registry provides a centralized, declarative definition of all Panasonic
/// MakerNote tags including:
/// - Simple string tags (version, model, firmware, serial numbers, lens names)
/// - Simple integer tags (contrast, saturation, sharpness, RGB levels, angles)
/// - Enumerated tags with decoders (quality, white balance, focus mode, etc.)
/// - Special feature tags (BabyAge, Location, TextStamp, etc.)
///
/// ## Tag Coverage
/// This registry covers approximately 90+ Panasonic MakerNote tags, significantly
/// improving compatibility with ExifTool's Panasonic.pm module.
///
/// # Returns
/// A fully configured TagRegistry ready for Panasonic MakerNote parsing
pub fn panasonic_registry() -> TagRegistry {
    TagRegistry::new()
        // ====================================================================
        // String tags - text-based metadata fields
        // ====================================================================
        .register_string_tag(0x0001, "ImageQuality")
        .register_string_tag(0x0002, "FirmwareVersion")
        .register_string_tag(0x0025, "InternalSerialNumber")
        .register_string_tag(0x0026, "PanasonicExifVersion")
        .register_string_tag(0x0033, "BabyAge")
        .register_string_tag(0x0052, "LensSerialNumber")
        .register_string_tag(0x0054, "AccessorySerialNumber")
        .register_string_tag(0x0065, "Title")
        .register_string_tag(0x0066, "BabyName")
        .register_string_tag(0x0067, "Location")
        .register_string_tag(0x0069, "Country")
        .register_string_tag(0x006B, "State")
        .register_string_tag(0x006D, "City")
        .register_string_tag(0x006F, "Landmark")
        .register_string_tag(0x0080, "City2")
        // ====================================================================
        // Enumerated tags with decoders - values mapped to human-readable strings
        // ====================================================================
        // Basic camera settings
        .register_enum_tag_required(0x0003, "WhiteBalance", &WHITE_BALANCE)
        .register_enum_tag_required(0x0007, "FocusMode", &FOCUS_MODE)
        .register_enum_tag_required(0x000F, "AFAreaMode", &AF_AREA_MODE)
        .register_enum_tag_required(0x001A, "ImageStabilization", &IMAGE_STABILIZATION)
        .register_enum_tag_required(0x001C, "MacroMode", &MACRO_MODE)
        .register_enum_tag_required(0x001F, "ShootingMode", &SHOOTING_MODE)
        .register_enum_tag_required(0x0020, "Audio", &AUDIO)
        .register_enum_tag_required(0x0028, "ColorEffect", &COLOR_EFFECT)
        .register_enum_tag_required(0x002A, "BurstMode", &BURST_MODE)
        .register_enum_tag_required(0x002C, "ContrastMode", &CONTRAST_MODE)
        .register_enum_tag_required(0x002D, "NoiseReduction", &NOISE_REDUCTION)
        .register_enum_tag_required(0x002E, "SelfTimer", &SELF_TIMER_MODE)
        .register_enum_tag_required(0x0030, "Rotation", &ROTATION)
        .register_enum_tag_required(0x0031, "AFAssistLamp", &AF_ASSIST_LAMP)
        .register_raw(0x0032, "ColorMode")
        .register_enum_tag_required(0x0034, "OpticalZoomMode", &OPTICAL_ZOOM_MODE)
        .register_enum_tag_required(0x0035, "ConversionLens", &CONVERSION_LENS)
        .register_integer_tag(0x0036, "TravelDay", None)
        .register_raw(0x0040, "Saturation")
        .register_raw(0x0041, "Sharpness")
        .register_enum_tag_required(0x0042, "FilmMode", &FILM_MODE)
        .register_enum_tag_required(0x003A, "WorldTimeLocation", &WORLD_TIME_LOCATION)
        .register_enum_tag_required(0x003B, "TextStamp", &TEXT_STAMP)
        .register_integer_tag(0x003C, "ProgramISO", None)
        .register_enum_tag_required(0x003D, "AdvancedSceneType", &ADVANCED_SCENE_TYPE)
        .register_enum_tag_required(0x003E, "TextStamp2", &TEXT_STAMP)
        .register_integer_tag(0x003F, "FacesDetected", None)
        .register_integer_tag(0x0044, "ColorTempKelvin", None)
        .register_enum_tag_required(0x0045, "BracketSettings", &BRACKET_SETTINGS)
        .register_integer_tag(0x0046, "WBShiftAB", None)
        .register_integer_tag(0x0047, "WBShiftGM", None)
        .register_enum_tag_required(0x0048, "FlashCurtain", &FLASH_CURTAIN)
        .register_enum_tag_required(0x0049, "LongExposureNoiseReduction", &LONG_EXPOSURE_NR)
        .register_integer_tag(0x004B, "PanasonicImageWidth", None)
        .register_integer_tag(0x004C, "PanasonicImageHeight", None)
        .register_raw(0x004D, "AFPointPosition")
        .register_enum_tag_required(0x004E, "FaceDetection", &FACE_DETECTION)
        .register_raw(0x0051, "LensType")
        .register_raw(0x0053, "AccessoryType")
        .register_raw(0x0059, "Transform")
        .register_enum_tag_required(0x005D, "IntelligentExposure", &INTELLIGENT_EXPOSURE)
        .register_integer_tag(0x0060, "LensFirmwareVersion", None)
        .register_raw(0x0061, "FaceRecInfo")
        .register_enum_tag_required(0x0062, "FlashWarning", &FLASH_WARNING)
        .register_enum_tag_required(0x0070, "IntelligentResolution", &INTELLIGENT_RESOLUTION)
        .register_enum_tag_required(0x0077, "BurstSpeed", &BURST_SPEED)
        .register_enum_tag_required(0x0079, "IntelligentD-Range", &INTELLIGENT_D_RANGE)
        .register_enum_tag_required(0x007C, "ClearRetouch", &CLEAR_RETOUCH)
        .register_integer_tag(0x0086, "ManometerPressure", None)
        .register_enum_tag_required(0x0089, "PhotoStyle", &PHOTO_STYLE)
        .register_enum_tag_required(0x008A, "ShadingCompensation", &SHADING_COMPENSATION)
        .register_integer_tag(0x008C, "AccelerometerZ", None)
        .register_integer_tag(0x008D, "AccelerometerX", None)
        .register_integer_tag(0x008E, "AccelerometerY", None)
        .register_integer_tag(0x008F, "CameraOrientation", None)
        .register_integer_tag(0x0090, "RollAngle", None)
        .register_integer_tag(0x0091, "PitchAngle", None)
        .register_enum_tag_required(0x0093, "SweepPanoramaDirection", &SWEEP_PANORAMA_DIRECTION)
        .register_integer_tag(0x0094, "SweepPanoramaFieldOfView", None)
        .register_enum_tag_required(0x0096, "TimerRecording", &TIMER_RECORDING)
        .register_enum_tag_required(0x009D, "InternalNDFilter", &INTERNAL_ND_FILTER)
        .register_enum_tag_required(0x009E, "HDR", &HDR)
        .register_enum_tag_required(0x009F, "ShutterType", &SHUTTER_TYPE)
        .register_integer_tag(0x00A3, "ClearRetouchValue", None)
        .register_enum_tag_required(0x00AB, "TouchAE", &TOUCH_AE)
        // ====================================================================
        // Additional integer/numeric tags
        // ====================================================================
        .register_integer_tag(0x0023, "WhiteBalanceBias", None)
        .register_integer_tag(0x0024, "FlashBias", None)
        .register_integer_tag(0x0029, "TimeSincePowerOn", None)
        .register_integer_tag(0x002B, "SequenceNumber", None)
        .register_integer_tag(0x0039, "Contrast", None)
        // ====================================================================
        // Internal/diagnostic tags (0x8xxx range)
        // ====================================================================
        .register_integer_tag(0x8000, "MakerNoteVersion", None)
        .register_integer_tag(0x8001, "SceneMode", None)
        .register_integer_tag(0x8004, "WBRedLevel", None)
        .register_integer_tag(0x8005, "WBGreenLevel", None)
        .register_integer_tag(0x8006, "WBBlueLevel", None)
        .register_enum_tag_required(0x8007, "FlashFired", &ON_OFF_I32)
        .register_enum_tag_required(0x8008, "TextStamp3", &TEXT_STAMP)
        .register_enum_tag_required(0x8009, "TextStamp4", &TEXT_STAMP)
        .register_integer_tag(0x8010, "BabyAge2", None)
        .register_enum_tag_required(0x8012, "Transform2", &ON_OFF_I32)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let _registry = panasonic_registry();
        // Verify registry can be created successfully
        assert!(true, "Registry created successfully");
    }

    #[test]
    fn test_registry_has_tags() {
        let registry = panasonic_registry();
        // Verify registry contains some expected tags
        assert!(!registry.is_empty(), "Registry should have tags");
    }

    #[test]
    fn test_registry_has_extended_tags() {
        let registry = panasonic_registry();
        // Verify the new extended tags are registered
        assert!(
            registry.get_tag_name(0x0020).is_some(),
            "Audio tag should be registered"
        );
        assert!(
            registry.get_tag_name(0x003B).is_some(),
            "TextStamp tag should be registered"
        );
        assert!(
            registry.get_tag_name(0x0048).is_some(),
            "FlashCurtain tag should be registered"
        );
        assert!(
            registry.get_tag_name(0x009F).is_some(),
            "ShutterType tag should be registered"
        );
    }
}
