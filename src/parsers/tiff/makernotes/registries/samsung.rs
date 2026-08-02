//! Samsung MakerNote tag registry
//!
//! This module provides a centralized tag registry for Samsung MakerNotes,
//! covering the tags found in `Image::ExifTool::Samsung::Type2` (the table
//! ExifTool uses for both traditional Samsung/NX cameras and Galaxy
//! smartphones/SRW images).
//!
//! ## Tag Categories
//! - MakerNote version and device information
//! - Camera settings (white balance, color space, exposure)
//! - Face detection and recognition
//! - Lens information
//! - RAW processing data
//! - White balance levels and color matrices
//! - Tone curves
//!
//! ## Architecture
//! Samsung cameras use a standard TIFF IFD structure for MakerNotes. All tag
//! IDs and names below are transcribed directly from ExifTool's
//! `Image::ExifTool::Samsung::Type2` table.
//!
//! A number of tag IDs/names that previously appeared in this registry
//! (Galaxy AI-feature tags such as `SceneOptimizer`, `SingleTake`,
//! `ExpertRAW`, `DirectorsView`, `ProMode`, etc., plus a `FavoriteColor` /
//! `WorldTimeLocation` / `Mcc` / `Mnc` / `LeicaCameraID` / `LeicaLensID` /
//! `ContrastLevel` / `SharpnessLevel` / `SaturationLevel` / `DepthMap*`
//! group) do not exist anywhere in ExifTool's source and have been removed
//! as fabricated. See the removal commit for the full list and verification
//! method.

use super::super::shared::generic_decoders::SimpleValueDecoder;
use super::super::shared::tag_registry::TagRegistry;
use once_cell::sync::Lazy;

// ============================================================================
// Tag ID Constants
//
// IDs are transcribed from Image::ExifTool::Samsung::Type2 (ExifTool 13.59).
// ============================================================================

/// MakerNote version string
pub const SAMSUNG_MAKERNOTE_VERSION: u16 = 0x0001;
/// Device type identifier
pub const SAMSUNG_DEVICE_TYPE: u16 = 0x0002;
/// Samsung model ID
pub const SAMSUNG_MODEL_ID: u16 = 0x0003;
/// Smart Album color tag
pub const SAMSUNG_SMART_ALBUM_COLOR: u16 = 0x0020;
/// Picture Wizard settings
pub const SAMSUNG_PICTURE_WIZARD: u16 = 0x0021;
/// Local location name
pub const SAMSUNG_LOCAL_LOCATION_NAME: u16 = 0x0030;
/// Location name
pub const SAMSUNG_LOCATION_NAME: u16 = 0x0031;
/// Preview IFD (SubDirectory in ExifTool; exposed here as a raw blob)
pub const SAMSUNG_PREVIEW_IFD: u16 = 0x0035;
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
pub const SAMSUNG_FIRMWARE_NAME: u16 = 0xa001;
/// Sensor areas information
pub const SAMSUNG_SENSOR_AREAS: u16 = 0xa010;
/// Color space identifier
pub const SAMSUNG_COLOR_SPACE: u16 = 0xa011;
/// Smart Range setting
pub const SAMSUNG_SMART_RANGE: u16 = 0xa012;
/// Exposure compensation value
pub const SAMSUNG_EXPOSURE_COMPENSATION: u16 = 0xa013;
/// ISO speed value
pub const SAMSUNG_ISO: u16 = 0xa014;
/// Exposure time
pub const SAMSUNG_EXPOSURE_TIME: u16 = 0xa018;
/// F-Number (aperture)
pub const SAMSUNG_FNUMBER: u16 = 0xa019;
/// Focal length in 35mm format
pub const SAMSUNG_FOCAL_LENGTH_35MM: u16 = 0xa01a;
/// Encryption key for encrypted data
pub const SAMSUNG_ENCRYPTION_KEY: u16 = 0xa020;
/// WB RGGB levels (uncorrected)
pub const SAMSUNG_WB_RGGB_LEVELS_UNCORRECTED: u16 = 0xa021;
/// WB RGGB levels (auto)
pub const SAMSUNG_WB_RGGB_LEVELS_AUTO: u16 = 0xa022;
/// WB RGGB levels (illuminator 1)
pub const SAMSUNG_WB_RGGB_LEVELS_ILLUMINATOR1: u16 = 0xa023;
/// WB RGGB levels (illuminator 2)
pub const SAMSUNG_WB_RGGB_LEVELS_ILLUMINATOR2: u16 = 0xa024;
/// WB RGGB levels (black)
pub const SAMSUNG_WB_RGGB_LEVELS_BLACK: u16 = 0xa028;
/// Color matrix data
pub const SAMSUNG_COLOR_MATRIX: u16 = 0xa030;
/// Color matrix for sRGB
pub const SAMSUNG_COLOR_MATRIX_SRGB: u16 = 0xa031;
/// Color matrix for Adobe RGB
pub const SAMSUNG_COLOR_MATRIX_ADOBERGB: u16 = 0xa032;
/// Default sRGB tone curve
pub const SAMSUNG_TONE_CURVE_SRGB_DEFAULT: u16 = 0xa040;
/// Default Adobe RGB tone curve
pub const SAMSUNG_TONE_CURVE_ADOBERGB_DEFAULT: u16 = 0xa041;
/// sRGB tone curve
pub const SAMSUNG_TONE_CURVE_SRGB: u16 = 0xa042;
/// Adobe RGB tone curve
pub const SAMSUNG_TONE_CURVE_ADOBERGB: u16 = 0xa043;
/// Lens type identifier
pub const SAMSUNG_LENS_TYPE: u16 = 0xa003;
/// Lens firmware version
pub const SAMSUNG_LENS_FIRMWARE: u16 = 0xa004;
/// Internal lens serial number
pub const SAMSUNG_INTERNAL_LENS_SERIAL_NUMBER: u16 = 0xa005;

// ============================================================================
// Decoders
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

/// Decodes camera temperature in Celsius
pub fn decode_camera_temperature(value: i32) -> String {
    format!("{} C", value)
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
/// All entries are transcribed from `Image::ExifTool::Samsung::Type2`.
pub static SAMSUNG_TAGS: Lazy<TagRegistry> = Lazy::new(|| {
    TagRegistry::with_capacity(40)
        // Version and device info
        .register_string_tag(SAMSUNG_MAKERNOTE_VERSION, "MakerNoteVersion")
        .register_enum_tag_required(SAMSUNG_DEVICE_TYPE, "DeviceType", &DEVICE_TYPE)
        .register_enum_tag_required(SAMSUNG_MODEL_ID, "SamsungModelID", &MODEL_ID_DECODER)
        // Album/location/preview
        .register_raw(SAMSUNG_SMART_ALBUM_COLOR, "SmartAlbumColor")
        .register_raw(SAMSUNG_PICTURE_WIZARD, "PictureWizard")
        .register_string_tag(SAMSUNG_LOCAL_LOCATION_NAME, "LocalLocationName")
        .register_string_tag(SAMSUNG_LOCATION_NAME, "LocationName")
        .register_raw(SAMSUNG_PREVIEW_IFD, "PreviewIFD")
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
        // ExposureCompensation is `rational64s` in ExifTool with no scaling
        // PrintConv; registered raw rather than guessing at a conversion.
        .register_raw(SAMSUNG_EXPOSURE_COMPENSATION, "ExposureCompensation")
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
        .register_raw(SAMSUNG_TONE_CURVE_SRGB_DEFAULT, "ToneCurveSRGBDefault")
        .register_raw(
            SAMSUNG_TONE_CURVE_ADOBERGB_DEFAULT,
            "ToneCurveAdobeRGBDefault",
        )
        .register_raw(SAMSUNG_TONE_CURVE_SRGB, "ToneCurveSRGB")
        .register_raw(SAMSUNG_TONE_CURVE_ADOBERGB, "ToneCurveAdobeRGB")
        // Lens information
        .register_string_tag(SAMSUNG_LENS_TYPE, "LensType")
        .register_string_tag(SAMSUNG_LENS_FIRMWARE, "LensFirmware")
        .register_string_tag(
            SAMSUNG_INTERNAL_LENS_SERIAL_NUMBER,
            "InternalLensSerialNumber",
        )
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
    fn test_decode_focal_length_35mm() {
        assert_eq!(decode_focal_length_35mm(500), "50.0 mm");
        assert_eq!(decode_focal_length_35mm(1000), "100.0 mm");
    }

    #[test]
    fn test_registry_has_core_tags() {
        let registry = samsung_registry();
        assert!(registry.has_tag(SAMSUNG_MAKERNOTE_VERSION));
        assert!(registry.has_tag(SAMSUNG_DEVICE_TYPE));
        assert!(registry.has_tag(SAMSUNG_MODEL_ID));
        assert!(registry.has_tag(SAMSUNG_COLOR_SPACE));
        assert!(registry.has_tag(SAMSUNG_LENS_TYPE));
        assert!(registry.has_tag(SAMSUNG_FIRMWARE_NAME));
        assert!(registry.has_tag(SAMSUNG_SMART_ALBUM_COLOR));
    }

    #[test]
    fn test_registry_count() {
        let registry = samsung_registry();
        // All entries are verified against Image::ExifTool::Samsung::Type2.
        assert!(registry.len() >= 30);
    }
}
