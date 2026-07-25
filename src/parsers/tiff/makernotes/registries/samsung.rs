//! Samsung MakerNote tag registry
//!
//! This module provides a centralized tag registry for Samsung MakerNotes,
//! supporting both traditional Samsung cameras (Type1) and newer Galaxy smartphones (Type2).
//!
//! ## Tag Categories - Type1 (Traditional Samsung MakerNotes)
//! - MakerNote version and device information
//! - Camera settings (white balance, color space, exposure)
//! - Face detection and recognition
//! - Lens information
//! - RAW processing data
//! - White balance levels and color matrices
//! - Tone curves
//!
//! ## Tag Categories - Type2 (Galaxy Smartphones)
//! - Scene Optimizer AI detection
//! - Single Take mode information
//! - Expert RAW processing data
//! - Multi-Frame Processing details
//! - Director's View settings
//! - Pro mode parameters
//! - Depth map information
//!
//! ## Architecture
//! Samsung cameras use standard TIFF IFD structure for MakerNotes.
//! Type1 tags are found in traditional Samsung cameras (NX series).
//! Type2 tags are found in newer Samsung Galaxy smartphones.
//! Some tag IDs overlap between Type1 and Type2 with different meanings.

use super::super::shared::generic_decoders::{ON_OFF, SimpleValueDecoder};
use super::super::shared::tag_registry::TagRegistry;
use once_cell::sync::Lazy;

// ============================================================================
// Type1 Tag ID Constants (Traditional Samsung Cameras)
// ============================================================================

/// MakerNote version string
pub const SAMSUNG_MAKERNOTE_VERSION: u16 = 0x0001;
/// Device type identifier
pub const SAMSUNG_DEVICE_TYPE: u16 = 0x0002;
/// Samsung model ID
pub const SAMSUNG_MODEL_ID: u16 = 0x0003;
/// Picture Wizard settings
pub const SAMSUNG_PICTURE_WIZARD: u16 = 0x0021;
/// Local location name
pub const SAMSUNG_LOCAL_LOCATION_NAME: u16 = 0x0030;
/// Location name
pub const SAMSUNG_LOCATION_NAME: u16 = 0x0031;
/// Preview image data
pub const SAMSUNG_PREVIEW: u16 = 0x0035;
/// RAW data byte order
pub const SAMSUNG_RAW_DATA_BYTE_ORDER: u16 = 0x0040;
/// White balance setup
pub const SAMSUNG_WHITE_BALANCE_SETUP: u16 = 0x0041;
/// Camera temperature
pub const SAMSUNG_CAMERA_TEMPERATURE: u16 = 0x0043;
/// RAW data CFA pattern
pub const SAMSUNG_RAW_DATA_CFA_PATTERN: u16 = 0x0050;
/// Face detect enabled
pub const SAMSUNG_FACE_DETECT: u16 = 0x0100;
/// Face recognition data
pub const SAMSUNG_FACE_RECOGNITION: u16 = 0x0120;
/// Face name data
pub const SAMSUNG_FACE_NAME: u16 = 0x0123;
/// Firmware name string
pub const SAMSUNG_FIRMWARE_NAME: u16 = 0x0201;
/// Sensor areas information
pub const SAMSUNG_SENSOR_AREAS: u16 = 0x0210;
/// Color space identifier
pub const SAMSUNG_COLOR_SPACE: u16 = 0x0221;
/// Smart Range setting
pub const SAMSUNG_SMART_RANGE: u16 = 0x0222;
/// Exposure compensation value
pub const SAMSUNG_EXPOSURE_COMPENSATION: u16 = 0x0223;
/// ISO speed value
pub const SAMSUNG_ISO: u16 = 0x0224;
/// Exposure time
pub const SAMSUNG_EXPOSURE_TIME: u16 = 0x0225;
/// F-Number (aperture)
pub const SAMSUNG_FNUMBER: u16 = 0x0226;
/// Focal length in 35mm format
pub const SAMSUNG_FOCAL_LENGTH_35MM: u16 = 0x0227;
/// Encryption key for encrypted data
pub const SAMSUNG_ENCRYPTION_KEY: u16 = 0x0230;
/// WB RGGB levels (uncorrected)
pub const SAMSUNG_WB_RGGB_LEVELS_UNCORRECTED: u16 = 0x0232;
/// WB RGGB levels (auto)
pub const SAMSUNG_WB_RGGB_LEVELS_AUTO: u16 = 0x0233;
/// WB RGGB levels (illuminator 1)
pub const SAMSUNG_WB_RGGB_LEVELS_ILLUMINATOR1: u16 = 0x0234;
/// WB RGGB levels (illuminator 2)
pub const SAMSUNG_WB_RGGB_LEVELS_ILLUMINATOR2: u16 = 0x0235;
/// WB RGGB levels (black)
pub const SAMSUNG_WB_RGGB_LEVELS_BLACK: u16 = 0x0236;
/// Color matrix data
pub const SAMSUNG_COLOR_MATRIX: u16 = 0x0240;
/// Color matrix for sRGB
pub const SAMSUNG_COLOR_MATRIX_SRGB: u16 = 0x0241;
/// Color matrix for Adobe RGB
pub const SAMSUNG_COLOR_MATRIX_ADOBERGB: u16 = 0x0242;
/// Tone curve 1 data
pub const SAMSUNG_TONE_CURVE_1: u16 = 0x0243;
/// Tone curve 2 data
pub const SAMSUNG_TONE_CURVE_2: u16 = 0x0244;
/// Tone curve 3 data
pub const SAMSUNG_TONE_CURVE_3: u16 = 0x0245;
/// Tone curve 4 data
pub const SAMSUNG_TONE_CURVE_4: u16 = 0x0246;
/// Lens type identifier (Type1)
pub const SAMSUNG_LENS_TYPE_T1: u16 = 0x0a01;
/// Lens firmware version
pub const SAMSUNG_LENS_FIRMWARE: u16 = 0x0a02;
/// Internal lens serial number
pub const SAMSUNG_INTERNAL_LENS_SERIAL_NUMBER: u16 = 0x0a03;

// ============================================================================
// Type2 Tag ID Constants (Galaxy Smartphones - some IDs may overlap with Type1)
// ============================================================================

/// Favorite color setting (Type2)
pub const SAMSUNG_FAVORITE_COLOR: u16 = 0x0004;
/// World time location (Type2)
pub const SAMSUNG_WORLD_TIME_LOCATION: u16 = 0x0005;
/// High dynamic range mode (Type2)
pub const SAMSUNG_HDR: u16 = 0x000a;
/// Mobile Country Code (Type2)
pub const SAMSUNG_MCC: u16 = 0x000c;
/// Mobile Network Code (Type2)
pub const SAMSUNG_MNC: u16 = 0x000d;
/// Leica Camera ID (Type2 - for Leica-Samsung partnership)
pub const SAMSUNG_LEICA_CAMERA_ID: u16 = 0x0011;
/// Leica Lens ID (Type2 - for Leica-Samsung partnership)
pub const SAMSUNG_LEICA_LENS_ID: u16 = 0x0012;
/// Contrast level (Type2)
pub const SAMSUNG_CONTRAST_LEVEL: u16 = 0x0040;
/// Sharpness level (Type2)
pub const SAMSUNG_SHARPNESS_LEVEL: u16 = 0x0041;
/// Saturation level (Type2)
pub const SAMSUNG_SATURATION_LEVEL: u16 = 0x0050;
/// Smart Album Color (Type2)
pub const SAMSUNG_SMART_ALBUM_COLOR: u16 = 0x0060;
/// Depth map width (Type2)
pub const SAMSUNG_DEPTH_MAP_WIDTH: u16 = 0x00a0;
/// Depth map height (Type2)
pub const SAMSUNG_DEPTH_MAP_HEIGHT: u16 = 0x00a1;
/// Depth map data (Type2)
pub const SAMSUNG_DEPTH_MAP: u16 = 0x00a2;

// ============================================================================
// Galaxy Smartphone Feature Tag ID Constants (non-overlapping tag IDs)
// These are used for AI and computational photography features
// ============================================================================

/// AI Scene Optimizer enabled (Galaxy)
pub const SAMSUNG_SCENE_OPTIMIZER: u16 = 0x1001;
/// Detected scene type (Galaxy)
pub const SAMSUNG_SCENE_TYPE: u16 = 0x1002;
/// Single Take mode enabled (Galaxy)
pub const SAMSUNG_SINGLE_TAKE: u16 = 0x1005;
/// Single Take frame number (Galaxy)
pub const SAMSUNG_SINGLE_TAKE_FRAME: u16 = 0x1006;
/// Expert RAW mode enabled (Galaxy)
pub const SAMSUNG_EXPERT_RAW: u16 = 0x1008;
/// Multi-frame noise reduction enabled (Galaxy)
pub const SAMSUNG_MULTI_FRAME_NR: u16 = 0x100A;
/// Director's View multi-camera mode (Galaxy)
pub const SAMSUNG_DIRECTORS_VIEW: u16 = 0x100C;
/// Pro mode manual controls enabled (Galaxy)
pub const SAMSUNG_PRO_MODE: u16 = 0x100E;
/// Object tracking autofocus enabled (Galaxy)
pub const SAMSUNG_OBJECT_TRACKING: u16 = 0x1010;
/// Night mode processing enabled (Galaxy)
pub const SAMSUNG_NIGHT_MODE: u16 = 0x1012;
/// Night Hyperlapse mode enabled (Galaxy)
pub const SAMSUNG_NIGHT_HYPERLAPSE: u16 = 0x1014;
/// Super Steady video stabilization (Galaxy)
pub const SAMSUNG_SUPER_STEADY: u16 = 0x1016;
/// Food photography mode enabled (Galaxy)
pub const SAMSUNG_FOOD_MODE: u16 = 0x1018;
/// Portrait Live Focus effect (Galaxy)
pub const SAMSUNG_PORTRAIT_EFFECT: u16 = 0x101A;
/// Active camera lens identifier (Galaxy)
pub const SAMSUNG_LENS_TYPE: u16 = 0x101C;
/// Digital zoom magnification level (Galaxy)
pub const SAMSUNG_ZOOM_LEVEL: u16 = 0x101E;

// ============================================================================
// Type1 Decoders (Traditional Samsung Cameras)
// ============================================================================

/// Decoder for device type
pub const DEVICE_TYPE: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0x1000, "Compact Digital Camera"),
    (0x2000, "High-end NX Camera"),
    (0x3000, "HXM Video Camera"),
    (0x12000, "Cell Phone"),
    (0x300000, "SMX Video Camera"),
]);

/// Decoder for Samsung Model ID
pub const MODEL_ID_DECODER: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0x100101c, "NX10"),
    (0x1001226, "NX100"),
    (0x1001230, "NX5"),
    (0x1001231, "NX11"),
    (0x1001232, "NX200"),
    (0x1001233, "NX210"),
    (0x1001234, "NX1000"),
    (0x1001235, "NX300"),
    (0x1001236, "NX2000"),
    (0x1001237, "NX300M"),
    (0x1001238, "NX30"),
    (0x100123a, "NX1"),
    (0x100123b, "NX3000"),
    (0x100123c, "NX mini"),
    (0x100123d, "NX500"),
]);

/// Decoder for RAW data byte order
pub const RAW_DATA_BYTE_ORDER: SimpleValueDecoder<i32> =
    SimpleValueDecoder::new(&[(0, "Little-endian (Intel)"), (1, "Big-endian (Motorola)")]);

/// Decoder for color space
pub const COLOR_SPACE_DECODER: SimpleValueDecoder<i32> =
    SimpleValueDecoder::new(&[(0, "sRGB"), (1, "Adobe RGB")]);

/// Decoder for Smart Range
pub const SMART_RANGE: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[(0, "Off"), (1, "On")]);

/// Decoder for face detect
pub const FACE_DETECT: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[(0, "Off"), (1, "On")]);

/// Decoder for CFA pattern
pub const CFA_PATTERN: SimpleValueDecoder<i32> =
    SimpleValueDecoder::new(&[(0, "RGGB"), (1, "GRBG"), (2, "GBRG"), (3, "BGGR")]);

/// Decoder for HDR mode (Type2)
pub const HDR_DECODER: SimpleValueDecoder<i32> =
    SimpleValueDecoder::new(&[(0, "Off"), (1, "On"), (2, "Auto")]);

// ============================================================================
// Galaxy Feature Decoders
// ============================================================================

/// Decoder for Scene Optimizer mode (Off/On/Auto)
pub const SCENE_OPTIMIZER: SimpleValueDecoder<i16> =
    SimpleValueDecoder::new(&[(0, "Off"), (1, "On"), (2, "Auto")]);

/// Decoder for AI scene detection result
pub const SCENE_TYPE: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
    (0, "None"),
    (1, "Food"),
    (2, "Sunset"),
    (3, "Blue Sky"),
    (4, "Snow"),
    (5, "Greenery"),
    (6, "Beach"),
    (7, "Night"),
    (8, "Flower"),
    (9, "Indoor"),
    (10, "Pet"),
    (11, "Text"),
    (12, "Backlit"),
]);

/// Decoder for Single Take mode status
pub const SINGLE_TAKE: SimpleValueDecoder<i16> =
    SimpleValueDecoder::new(&[(0, "Off"), (1, "Recording"), (2, "Processing")]);

/// Decoder for Portrait mode effect type
pub const PORTRAIT_EFFECT: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
    (0, "None"),
    (1, "Blur"),
    (2, "Spin"),
    (3, "Zoom"),
    (4, "Color Point"),
    (5, "Glitch"),
]);

/// Decoder for multi-camera lens type
pub const LENS_TYPE: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
    (0, "Wide (Main)"),
    (1, "Ultra Wide"),
    (2, "Telephoto"),
    (3, "Front Camera"),
    (4, "Telephoto 3x"),
    (5, "Telephoto 10x"),
]);

/// Decodes digital zoom level (10 = 1.0x, 100 = 10.0x)
pub fn decode_zoom_level(value: i16) -> String {
    if value <= 0 {
        return "1.0x".to_string();
    }
    let zoom = value as f32 / 10.0;
    format!("{:.1}x", zoom)
}

/// Decodes binary on/off values (value > 0 = On)
pub fn decode_binary_onoff(value: i16) -> String {
    ON_OFF.decode(if value > 0 { 1 } else { 0 })
}

/// Decodes camera temperature in Celsius
pub fn decode_camera_temperature(value: i32) -> String {
    format!("{} C", value)
}

/// Decodes exposure compensation (value in 1/100 EV units)
pub fn decode_exposure_compensation(value: i32) -> String {
    let ev = value as f64 / 100.0;
    if ev >= 0.0 {
        format!("+{:.2} EV", ev)
    } else {
        format!("{:.2} EV", ev)
    }
}

/// Decodes focal length in 35mm format (value in 1/10 mm units)
pub fn decode_focal_length_35mm(value: i32) -> String {
    let mm = value as f64 / 10.0;
    format!("{:.1} mm", mm)
}

// ============================================================================
// Tag Registry
// ============================================================================

/// Static registry containing all Samsung MakerNote tag definitions
///
/// This registry includes tags from both Type1 (traditional cameras) and
/// Type2 (Galaxy smartphones) formats, as well as Galaxy-specific AI features.
/// When tag IDs overlap between Type1 and Type2, Type1 definitions are used.
pub static SAMSUNG_TAGS: Lazy<TagRegistry> = Lazy::new(|| {
    TagRegistry::with_capacity(80)
        // === Type1 tags (Traditional cameras - NX series) ===
        // Version and device info
        .register_string_tag(SAMSUNG_MAKERNOTE_VERSION, "MakerNoteVersion")
        .register_enum_tag_required(SAMSUNG_DEVICE_TYPE, "DeviceType", &DEVICE_TYPE)
        .register_enum_tag_required(SAMSUNG_MODEL_ID, "SamsungModelID", &MODEL_ID_DECODER)
        // Location and preview
        .register_string_tag(SAMSUNG_LOCAL_LOCATION_NAME, "LocalLocationName")
        .register_string_tag(SAMSUNG_LOCATION_NAME, "LocationName")
        .register_raw(SAMSUNG_PREVIEW, "PreviewImage")
        // RAW and processing settings
        .register_enum_tag_required(
            SAMSUNG_RAW_DATA_BYTE_ORDER,
            "RawDataByteOrder",
            &RAW_DATA_BYTE_ORDER,
        )
        .register_raw(SAMSUNG_WHITE_BALANCE_SETUP, "WhiteBalanceSetup")
        .register_i32(
            SAMSUNG_CAMERA_TEMPERATURE,
            "CameraTemperature",
            decode_camera_temperature,
        )
        .register_enum_tag_required(
            SAMSUNG_RAW_DATA_CFA_PATTERN,
            "RawDataCFAPattern",
            &CFA_PATTERN,
        )
        // Face detection
        .register_enum_tag_required(SAMSUNG_FACE_DETECT, "FaceDetect", &FACE_DETECT)
        .register_raw(SAMSUNG_FACE_RECOGNITION, "FaceRecognition")
        .register_string_tag(SAMSUNG_FACE_NAME, "FaceName")
        // Firmware and sensor
        .register_string_tag(SAMSUNG_FIRMWARE_NAME, "FirmwareName")
        .register_raw(SAMSUNG_SENSOR_AREAS, "SensorAreas")
        // Color and exposure
        .register_enum_tag_required(SAMSUNG_COLOR_SPACE, "ColorSpace", &COLOR_SPACE_DECODER)
        .register_enum_tag_required(SAMSUNG_SMART_RANGE, "SmartRange", &SMART_RANGE)
        .register_i32(
            SAMSUNG_EXPOSURE_COMPENSATION,
            "ExposureCompensation",
            decode_exposure_compensation,
        )
        .register_raw(SAMSUNG_ISO, "ISO")
        .register_raw(SAMSUNG_EXPOSURE_TIME, "ExposureTime")
        .register_raw(SAMSUNG_FNUMBER, "FNumber")
        .register_i32(
            SAMSUNG_FOCAL_LENGTH_35MM,
            "FocalLengthIn35mmFormat",
            decode_focal_length_35mm,
        )
        // Encryption and white balance
        .register_raw(SAMSUNG_ENCRYPTION_KEY, "EncryptionKey")
        .register_raw(
            SAMSUNG_WB_RGGB_LEVELS_UNCORRECTED,
            "WBRGGBLevelsUncorrected",
        )
        .register_raw(SAMSUNG_WB_RGGB_LEVELS_AUTO, "WBRGGBLevelsAuto")
        .register_raw(
            SAMSUNG_WB_RGGB_LEVELS_ILLUMINATOR1,
            "WBRGGBLevelsIlluminator1",
        )
        .register_raw(
            SAMSUNG_WB_RGGB_LEVELS_ILLUMINATOR2,
            "WBRGGBLevelsIlluminator2",
        )
        .register_raw(SAMSUNG_WB_RGGB_LEVELS_BLACK, "WBRGGBLevelsBlack")
        // Color matrices
        .register_raw(SAMSUNG_COLOR_MATRIX, "ColorMatrix")
        .register_raw(SAMSUNG_COLOR_MATRIX_SRGB, "ColorMatrixSRGB")
        .register_raw(SAMSUNG_COLOR_MATRIX_ADOBERGB, "ColorMatrixAdobeRGB")
        // Tone curves
        .register_raw(SAMSUNG_TONE_CURVE_1, "ToneCurve1")
        .register_raw(SAMSUNG_TONE_CURVE_2, "ToneCurve2")
        .register_raw(SAMSUNG_TONE_CURVE_3, "ToneCurve3")
        .register_raw(SAMSUNG_TONE_CURVE_4, "ToneCurve4")
        // Lens information
        .register_string_tag(SAMSUNG_LENS_TYPE_T1, "LensType")
        .register_string_tag(SAMSUNG_LENS_FIRMWARE, "LensFirmware")
        .register_string_tag(
            SAMSUNG_INTERNAL_LENS_SERIAL_NUMBER,
            "InternalLensSerialNumber",
        )
        // Picture Wizard
        .register_raw(SAMSUNG_PICTURE_WIZARD, "PictureWizard")
        // === Type2 tags (Galaxy smartphones - non-overlapping) ===
        .register_raw(SAMSUNG_FAVORITE_COLOR, "FavoriteColor")
        .register_raw(SAMSUNG_WORLD_TIME_LOCATION, "WorldTimeLocation")
        .register_enum_tag_required(SAMSUNG_HDR, "HighDynamicRange", &HDR_DECODER)
        .register_raw(SAMSUNG_MCC, "Mcc")
        .register_raw(SAMSUNG_MNC, "Mnc")
        .register_raw(SAMSUNG_LEICA_CAMERA_ID, "LeicaCameraID")
        .register_raw(SAMSUNG_LEICA_LENS_ID, "LeicaLensID")
        .register_raw(SAMSUNG_SMART_ALBUM_COLOR, "SmartAlbumColor")
        .register_raw(SAMSUNG_DEPTH_MAP_WIDTH, "DepthMapWidth")
        .register_raw(SAMSUNG_DEPTH_MAP_HEIGHT, "DepthMapHeight")
        .register_raw(SAMSUNG_DEPTH_MAP, "DepthMap")
        // === Galaxy feature tags (AI and computational photography) ===
        .register_simple_i16(SAMSUNG_SCENE_OPTIMIZER, "SceneOptimizer", &SCENE_OPTIMIZER)
        .register_simple_i16(SAMSUNG_SCENE_TYPE, "SceneType", &SCENE_TYPE)
        .register_simple_i16(SAMSUNG_SINGLE_TAKE, "SingleTake", &SINGLE_TAKE)
        .register_simple_i16(SAMSUNG_PORTRAIT_EFFECT, "PortraitEffect", &PORTRAIT_EFFECT)
        .register_simple_i16(SAMSUNG_LENS_TYPE, "GalaxyLensType", &LENS_TYPE)
        .register_i16(SAMSUNG_ZOOM_LEVEL, "ZoomLevel", decode_zoom_level)
        .register_raw(SAMSUNG_SINGLE_TAKE_FRAME, "SingleTakeFrame")
        .register_i16(SAMSUNG_EXPERT_RAW, "ExpertRAW", decode_binary_onoff)
        .register_i16(
            SAMSUNG_MULTI_FRAME_NR,
            "MultiFrameNoiseReduction",
            decode_binary_onoff,
        )
        .register_i16(SAMSUNG_DIRECTORS_VIEW, "DirectorsView", decode_binary_onoff)
        .register_i16(SAMSUNG_PRO_MODE, "ProMode", decode_binary_onoff)
        .register_i16(
            SAMSUNG_OBJECT_TRACKING,
            "ObjectTracking",
            decode_binary_onoff,
        )
        .register_i16(SAMSUNG_NIGHT_MODE, "NightMode", decode_binary_onoff)
        .register_i16(
            SAMSUNG_NIGHT_HYPERLAPSE,
            "NightHyperlapse",
            decode_binary_onoff,
        )
        .register_i16(SAMSUNG_SUPER_STEADY, "SuperSteady", decode_binary_onoff)
        .register_i16(SAMSUNG_FOOD_MODE, "FoodMode", decode_binary_onoff)
});

/// Returns a reference to the Samsung tag registry
///
/// This function provides access to the centralized tag registry,
/// allowing the parser to look up tag names and decoders efficiently.
pub fn samsung_registry() -> &'static TagRegistry {
    &SAMSUNG_TAGS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scene_optimizer_decoder() {
        assert_eq!(SCENE_OPTIMIZER.decode(0), "Off");
        assert_eq!(SCENE_OPTIMIZER.decode(1), "On");
        assert_eq!(SCENE_OPTIMIZER.decode(2), "Auto");
    }

    #[test]
    fn test_scene_type_decoder() {
        assert_eq!(SCENE_TYPE.decode(0), "None");
        assert_eq!(SCENE_TYPE.decode(1), "Food");
        assert_eq!(SCENE_TYPE.decode(7), "Night");
    }

    #[test]
    fn test_single_take_decoder() {
        assert_eq!(SINGLE_TAKE.decode(0), "Off");
        assert_eq!(SINGLE_TAKE.decode(1), "Recording");
    }

    #[test]
    fn test_portrait_effect_decoder() {
        assert_eq!(PORTRAIT_EFFECT.decode(0), "None");
        assert_eq!(PORTRAIT_EFFECT.decode(1), "Blur");
        assert_eq!(PORTRAIT_EFFECT.decode(4), "Color Point");
    }

    #[test]
    fn test_lens_type_decoder() {
        assert_eq!(LENS_TYPE.decode(0), "Wide (Main)");
        assert_eq!(LENS_TYPE.decode(1), "Ultra Wide");
        assert_eq!(LENS_TYPE.decode(5), "Telephoto 10x");
    }

    #[test]
    fn test_decode_zoom_level() {
        assert_eq!(decode_zoom_level(10), "1.0x");
        assert_eq!(decode_zoom_level(100), "10.0x");
        assert_eq!(decode_zoom_level(35), "3.5x");
    }

    #[test]
    fn test_decode_binary_onoff() {
        assert_eq!(decode_binary_onoff(0), "Off");
        assert_eq!(decode_binary_onoff(1), "On");
        assert_eq!(decode_binary_onoff(5), "On");
    }

    #[test]
    fn test_type1_decoders() {
        assert_eq!(DEVICE_TYPE.decode(0x2000), "High-end NX Camera");
        assert_eq!(MODEL_ID_DECODER.decode(0x100123a), "NX1");
        assert_eq!(COLOR_SPACE_DECODER.decode(0), "sRGB");
        assert_eq!(COLOR_SPACE_DECODER.decode(1), "Adobe RGB");
        assert_eq!(CFA_PATTERN.decode(0), "RGGB");
    }

    #[test]
    fn test_decode_camera_temperature() {
        assert_eq!(decode_camera_temperature(25), "25 C");
        assert_eq!(decode_camera_temperature(-5), "-5 C");
    }

    #[test]
    fn test_decode_exposure_compensation() {
        assert_eq!(decode_exposure_compensation(100), "+1.00 EV");
        assert_eq!(decode_exposure_compensation(-150), "-1.50 EV");
    }

    #[test]
    fn test_decode_focal_length_35mm() {
        assert_eq!(decode_focal_length_35mm(500), "50.0 mm");
        assert_eq!(decode_focal_length_35mm(1000), "100.0 mm");
    }

    #[test]
    fn test_registry_has_type1_tags() {
        let registry = samsung_registry();
        assert!(registry.has_tag(SAMSUNG_MAKERNOTE_VERSION));
        assert!(registry.has_tag(SAMSUNG_DEVICE_TYPE));
        assert!(registry.has_tag(SAMSUNG_MODEL_ID));
        assert!(registry.has_tag(SAMSUNG_COLOR_SPACE));
        assert!(registry.has_tag(SAMSUNG_LENS_TYPE_T1));
        assert!(registry.has_tag(SAMSUNG_FIRMWARE_NAME));
    }

    #[test]
    fn test_registry_has_type2_tags() {
        let registry = samsung_registry();
        assert!(registry.has_tag(SAMSUNG_HDR));
        assert!(registry.has_tag(SAMSUNG_DEPTH_MAP_WIDTH));
        assert!(registry.has_tag(SAMSUNG_DEPTH_MAP_HEIGHT));
    }

    #[test]
    fn test_registry_has_galaxy_feature_tags() {
        let registry = samsung_registry();
        assert!(registry.has_tag(SAMSUNG_SCENE_OPTIMIZER));
        assert!(registry.has_tag(SAMSUNG_SCENE_TYPE));
        assert!(registry.has_tag(SAMSUNG_SINGLE_TAKE));
        assert!(registry.has_tag(SAMSUNG_SINGLE_TAKE_FRAME));
        assert!(registry.has_tag(SAMSUNG_EXPERT_RAW));
        assert!(registry.has_tag(SAMSUNG_MULTI_FRAME_NR));
        assert!(registry.has_tag(SAMSUNG_DIRECTORS_VIEW));
        assert!(registry.has_tag(SAMSUNG_PRO_MODE));
        assert!(registry.has_tag(SAMSUNG_OBJECT_TRACKING));
        assert!(registry.has_tag(SAMSUNG_NIGHT_MODE));
        assert!(registry.has_tag(SAMSUNG_NIGHT_HYPERLAPSE));
        assert!(registry.has_tag(SAMSUNG_SUPER_STEADY));
        assert!(registry.has_tag(SAMSUNG_FOOD_MODE));
        assert!(registry.has_tag(SAMSUNG_PORTRAIT_EFFECT));
        assert!(registry.has_tag(SAMSUNG_LENS_TYPE));
        assert!(registry.has_tag(SAMSUNG_ZOOM_LEVEL));
    }

    #[test]
    fn test_registry_count() {
        let registry = samsung_registry();
        // Combined registry should have 50+ tags (Type1 + Type2 + Galaxy features)
        assert!(registry.len() >= 50);
    }
}
