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

use super::super::shared::tag_registry::TagRegistry;

// Re-export decoders from panasonic.rs
// These decoders are defined using const_decoder! macros in the main parser
use super::super::panasonic::{
    AF_ASSIST_LAMP, AUDIO, BRACKET_SETTINGS, BURST_MODE, CAMERA_ORIENTATION, CLEAR_RETOUCH,
    COLOR_EFFECT, COLOR_MODE, CONTRAST_MODE, CONVERSION_LENS, FILM_MODE, FLASH_CURTAIN,
    FLASH_WARNING, FOCUS_MODE, HDR, IMAGE_QUALITY, IMAGE_STABILIZATION, INTELLIGENT_D_RANGE,
    INTELLIGENT_EXPOSURE, INTELLIGENT_RESOLUTION, LONG_EXPOSURE_NR, MACRO_MODE, NOISE_REDUCTION,
    OPTICAL_ZOOM_MODE, PHOTO_STYLE, ROTATION, SELF_TIMER, SHADING_COMPENSATION, SHOOTING_MODE,
    SHUTTER_TYPE, SWEEP_PANORAMA_DIRECTION, TEXT_STAMP, TIMER_RECORDING, TOUCH_AE, WHITE_BALANCE,
    WORLD_TIME_LOCATION,
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
        .register_enum_tag_required(0x0001, "ImageQuality", &IMAGE_QUALITY)
        .register_enum_tag_required(0x0003, "WhiteBalance", &WHITE_BALANCE)
        .register_enum_tag_required(0x0007, "FocusMode", &FOCUS_MODE)
        // 0x000F AFAreaMode is an int8u pair decoded in parse_entry
        .register_raw(0x000F, "AFAreaMode")
        .register_enum_tag_required(0x001A, "ImageStabilization", &IMAGE_STABILIZATION)
        .register_enum_tag_required(0x001C, "MacroMode", &MACRO_MODE)
        .register_enum_tag_required(0x001F, "ShootingMode", &SHOOTING_MODE)
        .register_enum_tag_required(0x0020, "Audio", &AUDIO)
        .register_raw(0x0021, "DataDump")
        .register_i32(0x0027, "VideoFrameRate", decode_video_frame_rate)
        .register_enum_tag_required(0x0028, "ColorEffect", &COLOR_EFFECT)
        .register_enum_tag_required(0x002A, "BurstMode", &BURST_MODE)
        .register_enum_tag_required(0x002C, "ContrastMode", &CONTRAST_MODE)
        .register_enum_tag_required(0x002D, "NoiseReduction", &NOISE_REDUCTION)
        .register_enum_tag_required(0x002E, "SelfTimer", &SELF_TIMER)
        .register_enum_tag_required(0x0030, "Rotation", &ROTATION)
        .register_enum_tag_required(0x0031, "AFAssistLamp", &AF_ASSIST_LAMP)
        .register_enum_tag_required(0x0032, "ColorMode", &COLOR_MODE)
        .register_enum_tag_required(0x0034, "OpticalZoomMode", &OPTICAL_ZOOM_MODE)
        .register_enum_tag_required(0x0035, "ConversionLens", &CONVERSION_LENS)
        .register_integer_tag(0x0036, "TravelDay", None)
        .register_i32(0x0038, "BatteryLevel", decode_battery_level)
        .register_raw(0x0040, "Saturation")
        .register_raw(0x0041, "Sharpness")
        .register_enum_tag_required(0x0042, "FilmMode", &FILM_MODE)
        .register_i32(0x0043, "JPEGQuality", decode_jpeg_quality)
        .register_enum_tag_required(0x003A, "WorldTimeLocation", &WORLD_TIME_LOCATION)
        .register_enum_tag_required(0x003B, "TextStamp", &TEXT_STAMP)
        .register_integer_tag(0x003C, "ProgramISO", None)
        // AdvancedSceneType has no PrintConv in ExifTool; it stays numeric
        .register_integer_tag(0x003D, "AdvancedSceneType", None)
        .register_enum_tag_required(0x003E, "TextStamp", &TEXT_STAMP)
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
        // 0x004E and 0x0061 are `SubDirectory` tags in `%Panasonic::Main`
        // (Panasonic.pm:935 FaceDetInfo, :1007 FaceRecInfo) -- ExifTool descends
        // into the record and reports its fields, and reports no value for the
        // pointer itself. They are handled by `panasonic_binary_subdir` and must
        // not be registered as scalars here.
        .register_raw(0x0051, "LensType")
        .register_raw(0x0053, "AccessoryType")
        .register_raw(0x0059, "Transform")
        .register_enum_tag_required(0x005D, "IntelligentExposure", &INTELLIGENT_EXPOSURE)
        .register_integer_tag(0x0060, "LensFirmwareVersion", None)
        .register_enum_tag_required(0x0062, "FlashWarning", &FLASH_WARNING)
        .register_enum_tag_required(0x0070, "IntelligentResolution", &INTELLIGENT_RESOLUTION)
        // BurstSpeed is a plain count, not an enum: Panasonic.pm:1094 declares
        // `Writable => 'int16u', Notes => 'images per second'` and no PrintConv.
        // A Low/Mid/High decoder printed "Low" where ExifTool prints "0".
        .register_integer_tag(0x0077, "BurstSpeed", None)
        .register_enum_tag_required(0x0079, "IntelligentD-Range", &INTELLIGENT_D_RANGE)
        .register_enum_tag_required(0x007C, "ClearRetouch", &CLEAR_RETOUCH)
        .register_integer_tag(0x0086, "ManometerPressure", None)
        .register_enum_tag_required(0x0089, "PhotoStyle", &PHOTO_STYLE)
        .register_enum_tag_required(0x008A, "ShadingCompensation", &SHADING_COMPENSATION)
        // 0x008C-0x008E are int16u on the wire but `Format => 'int16s'`
        // overrides the read (Panasonic.pm:1170-1187): a plain unsigned
        // decode turned a negative reading like -3 into 65533. Signed
        // reinterpretation happens in parse_entry; no decoder needed here.
        .register_raw(0x008C, "AccelerometerZ")
        .register_raw(0x008D, "AccelerometerX")
        .register_raw(0x008E, "AccelerometerY")
        .register_enum_tag_required(0x008F, "CameraOrientation", &CAMERA_ORIENTATION)
        // 0x0090/0x0091 are also int16u-wire/int16s-Format, plus ValueConv
        // '$val/10' and '-$val/10' (Panasonic.pm:1200-1215). Handled in
        // parse_entry alongside the accelerometer axes.
        .register_raw(0x0090, "RollAngle")
        .register_raw(0x0091, "PitchAngle")
        .register_enum_tag_required(0x0093, "SweepPanoramaDirection", &SWEEP_PANORAMA_DIRECTION)
        .register_integer_tag(0x0094, "SweepPanoramaFieldOfView", None)
        .register_enum_tag_required(0x0096, "TimerRecording", &TIMER_RECORDING)
        .register_raw(0x009D, "InternalNDFilter")
        .register_enum_tag_required(0x009E, "HDR", &HDR)
        .register_enum_tag_required(0x009F, "ShutterType", &SHUTTER_TYPE)
        // rational64u with no ValueConv/PrintConv (Panasonic.pm:1305-1309);
        // handled in parse_entry so 0/0 reads "undef" per GetRational64u
        // instead of the wrong integer decode of the raw numerator bytes.
        .register_raw(0x00A3, "ClearRetouchValue")
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
        .register_i32(0x8001, "SceneMode", decode_scene_mode)
        .register_i32(0x8002, "HighlightWarning", decode_highlight_warning)
        .register_i32(
            0x8003,
            "DarkFocusEnvironment",
            decode_dark_focus_environment,
        )
        .register_integer_tag(0x8004, "WBRedLevel", None)
        .register_integer_tag(0x8005, "WBGreenLevel", None)
        .register_integer_tag(0x8006, "WBBlueLevel", None)
        // 0x8007 is not a tag. Panasonic.pm:1563-1567 carries it commented out:
        // `#0x8007 => { #PH - questionable [disabled because it conflicts with
        // EXIF in too many samples]`. ExifTool reports no `FlashFired` for any
        // Panasonic file, so neither may we.
        //
        // 0x8008 and 0x8009 are both plain `TextStamp` in ExifTool
        // (Panasonic.pm:1568, :1574), same PrintConv as 0x3b and 0x3e.
        .register_enum_tag_required(0x8008, "TextStamp", &TEXT_STAMP)
        .register_enum_tag_required(0x8009, "TextStamp", &TEXT_STAMP)
    // 0x8010 is `BabyAge` (Panasonic.pm:1580), not `BabyAge2`: a `string`
    // with `PrintConv => '$val eq "9999:99:99 00:00:00" ? "(not set)" : $val'`.
    // It was registered as a bare integer, which cannot produce that value, and
    // 0x0033 already delivers a matching `Panasonic:BabyAge`. Omitted rather
    // than renamed, so no wrong value ships under the real name.
    //
    // 0x8012 is `Transform` (Panasonic.pm:1587), not `Transform2`: `int16s`
    // `Count => 2` whose PrintConv keys are integer *pairs*
    // ('0 0' => 'Off', '-3 2' => 'Slim High', ...). A one-value On/Off decoder
    // cannot express that, so it is omitted too.
}

/// Panasonic SceneMode (0x8001).
///
/// Panasonic.pm:1531-1539 declares it as
/// `PrintConv => { 0 => 'Off', %shootingMode }` -- the same table
/// `ShootingMode` (0x001F) uses, with a zero entry prepended. The tag was
/// registered with no decoder at all, so `Panasonic.rw2` reported the raw `0`
/// where `exiftool -G1 -s` prints `Off`.
fn decode_scene_mode(value: i32) -> String {
    if value == 0 {
        return "Off".to_string();
    }
    SHOOTING_MODE.decode(value)
}

fn decode_video_frame_rate(value: i32) -> String {
    match value {
        0 => "n/a".to_string(),
        _ => value.to_string(),
    }
}

fn decode_battery_level(value: i32) -> String {
    match value {
        1 => "Full".to_string(),
        2 => "Medium".to_string(),
        3 => "Low".to_string(),
        4 => "Near Empty".to_string(),
        7 => "Near Full".to_string(),
        8 => "Medium Low".to_string(),
        256 => "n/a".to_string(),
        _ => value.to_string(),
    }
}

fn decode_jpeg_quality(value: i32) -> String {
    match value {
        0 => "n/a (Movie)".to_string(),
        2 => "High".to_string(),
        3 => "Standard".to_string(),
        6 => "Very High".to_string(),
        255 => "n/a (RAW only)".to_string(),
        _ => value.to_string(),
    }
}

fn decode_highlight_warning(value: i32) -> String {
    match value {
        0 => "Disabled".to_string(),
        1 => "No".to_string(),
        2 => "Yes".to_string(),
        _ => value.to_string(),
    }
}

fn decode_dark_focus_environment(value: i32) -> String {
    match value {
        1 => "No".to_string(),
        2 => "Yes".to_string(),
        _ => value.to_string(),
    }
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
