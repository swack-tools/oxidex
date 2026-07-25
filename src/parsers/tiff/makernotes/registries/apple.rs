//! Apple (iPhone/iPad) tag registry
//!
//! This module provides a declarative registry for Apple MakerNote tags,
//! consolidating tag definitions and decoders for cleaner parser implementation.
//!
//! ## Tag Categories
//! - Core identification (MakerNote version, runtime)
//! - Computational Photography (HDR, Portrait Mode, Night Mode)
//! - Multi-camera metadata (lens identification, camera type)
//! - Scene detection and semantic styles
//! - Live Photo and burst mode metadata
//! - AF performance and depth measurement (LiDAR)
//! - Signal-to-noise ratio and noise analysis
//! - Color temperature and correction
//! - Image processing flags

use super::super::shared::tag_registry::TagRegistry;

// Re-export existing decoders from apple.rs to avoid duplication
// These decoders are already defined using const_decoder! macro
use super::super::apple::{
    DECODE_CAMERA_TYPE, DECODE_GREEN_GHOST_MITIGATION, DECODE_HDR_TYPE, DECODE_IMAGE_CAPTURE_TYPE,
    DECODE_OIS_MODE, DECODE_SEMANTIC_STYLE, DECODE_SNR_TYPE,
};

// ============================================================================
// APPLE MAKERNOTE TAG IDS
// ============================================================================
// Comprehensive list of Apple MakerNote tags

// Core identification tags
const APPLE_MAKERNOTE_VERSION: u16 = 0x0001;
const APPLE_AE_MATRIX: u16 = 0x0002;
const APPLE_RUN_TIME: u16 = 0x0003;
const APPLE_AE_STABLE: u16 = 0x0004;
const APPLE_AE_TARGET: u16 = 0x0005;
const APPLE_AE_AVERAGE: u16 = 0x0006;
const APPLE_AF_STABLE: u16 = 0x0007;
const APPLE_ACCELERATION_VECTOR: u16 = 0x0008;

// HDR and image processing tags
const APPLE_HDR_IMAGE_TYPE: u16 = 0x000A;
const APPLE_BURST_UUID: u16 = 0x000B;
const APPLE_FOCUS_DISTANCE_RANGE: u16 = 0x000C;
const APPLE_OIS_MODE: u16 = 0x000F;

// Content and image identification
const APPLE_CONTENT_IDENTIFIER: u16 = 0x0011;
const APPLE_IMAGE_CAPTURE_TYPE: u16 = 0x0014;
const APPLE_IMAGE_UNIQUE_ID: u16 = 0x0015;
const APPLE_LIVE_PHOTO_VIDEO_INDEX: u16 = 0x0017;
const APPLE_IMAGE_PROCESSING_FLAGS: u16 = 0x0019;
const APPLE_QUALITY_HINT: u16 = 0x001A;

// Noise and signal analysis
const APPLE_LUMINANCE_NOISE_AMPLITUDE: u16 = 0x001D;
const APPLE_PHOTOS_APP_FEATURE_FLAGS: u16 = 0x001F;

// HDR headroom and capture request
const APPLE_IMAGE_CAPTURE_REQUEST_ID: u16 = 0x0020;
const APPLE_HDR_HEADROOM: u16 = 0x0021;
const APPLE_AF_PERFORMANCE: u16 = 0x0023;

// Scene analysis
const APPLE_SCENE_FLAGS: u16 = 0x0025;
const APPLE_SIGNAL_TO_NOISE_RATIO_TYPE: u16 = 0x0026;
const APPLE_SIGNAL_TO_NOISE_RATIO: u16 = 0x0027;

// Photo identifiers and camera info
const APPLE_PHOTO_IDENTIFIER: u16 = 0x002B;
const APPLE_COLOR_TEMPERATURE: u16 = 0x002D;
const APPLE_CAMERA_TYPE: u16 = 0x002E;
const APPLE_FOCUS_POSITION: u16 = 0x002F;
const APPLE_HDR_GAIN: u16 = 0x0030;

// Front-facing camera flag
const APPLE_FRONT_FACING_CAMERA: u16 = 0x0032;

// Advanced AF and processing tags
const APPLE_AF_MEASURED_DEPTH: u16 = 0x0038;
const APPLE_AF_CONFIDENCE: u16 = 0x003D;
const APPLE_COLOR_CORRECTION_MATRIX: u16 = 0x003E;
const APPLE_GREEN_GHOST_MITIGATION_STATUS: u16 = 0x003F;

// Semantic Style tags (Photographic Styles - iOS 15+)
const APPLE_SEMANTIC_STYLE: u16 = 0x0040;
const APPLE_SEMANTIC_STYLE_RENDERING_VER: u16 = 0x0041;
const APPLE_SEMANTIC_STYLE_PRESET: u16 = 0x0042;

// ============================================================================
// TAG REGISTRY
// ============================================================================

/// Creates the Apple tag registry with all tag definitions and decoders
///
/// This registry provides a centralized, declarative definition of all Apple
/// MakerNote tags, replacing scattered match statements with a clean lookup table.
///
/// # Returns
/// A TagRegistry instance with all Apple tags registered
///
/// # Example
/// ```ignore
/// let registry = apple_registry();
/// let hdr_type = registry.decode_i16(0x000A, 4); // "Smart HDR"
/// ```
pub fn apple_registry() -> TagRegistry {
    TagRegistry::new()
        // ================================================================
        // Core identification tags
        // ================================================================
        .register_raw(APPLE_MAKERNOTE_VERSION, "MakerNoteVersion")
        .register_raw(APPLE_AE_MATRIX, "AEMatrix")
        .register_raw(APPLE_RUN_TIME, "RunTime")
        .register_raw(APPLE_AE_STABLE, "AEStable")
        .register_raw(APPLE_AE_TARGET, "AETarget")
        .register_raw(APPLE_AE_AVERAGE, "AEAverage")
        .register_raw(APPLE_AF_STABLE, "AFStable")
        .register_raw(APPLE_ACCELERATION_VECTOR, "AccelerationVector")
        // ================================================================
        // HDR and image processing tags
        // ================================================================
        .register_simple_i16(APPLE_HDR_IMAGE_TYPE, "HDRImageType", &DECODE_HDR_TYPE)
        .register_raw(APPLE_BURST_UUID, "BurstUUID")
        .register_raw(APPLE_FOCUS_DISTANCE_RANGE, "FocusDistanceRange")
        .register_simple_i16(APPLE_OIS_MODE, "OISMode", &DECODE_OIS_MODE)
        // ================================================================
        // Content and image identification
        // ================================================================
        .register_raw(APPLE_CONTENT_IDENTIFIER, "ContentIdentifier")
        .register_simple_i16(
            APPLE_IMAGE_CAPTURE_TYPE,
            "ImageCaptureType",
            &DECODE_IMAGE_CAPTURE_TYPE,
        )
        .register_raw(APPLE_IMAGE_UNIQUE_ID, "ImageUniqueID")
        .register_raw(APPLE_LIVE_PHOTO_VIDEO_INDEX, "LivePhotoVideoIndex")
        .register_raw(APPLE_IMAGE_PROCESSING_FLAGS, "ImageProcessingFlags")
        .register_raw(APPLE_QUALITY_HINT, "QualityHint")
        // ================================================================
        // Noise and signal analysis
        // ================================================================
        .register_raw(APPLE_LUMINANCE_NOISE_AMPLITUDE, "LuminanceNoiseAmplitude")
        .register_raw(APPLE_PHOTOS_APP_FEATURE_FLAGS, "PhotosAppFeatureFlags")
        // ================================================================
        // HDR headroom and capture request
        // ================================================================
        .register_raw(
            APPLE_IMAGE_CAPTURE_REQUEST_ID,
            "ImageCaptureRequestIdentifier",
        )
        .register_raw(APPLE_HDR_HEADROOM, "HDRHeadroom")
        .register_raw(APPLE_AF_PERFORMANCE, "AFPerformance")
        // ================================================================
        // Scene analysis
        // ================================================================
        .register_raw(APPLE_SCENE_FLAGS, "SceneFlags")
        .register_simple_i16(
            APPLE_SIGNAL_TO_NOISE_RATIO_TYPE,
            "SignalToNoiseRatioType",
            &DECODE_SNR_TYPE,
        )
        .register_raw(APPLE_SIGNAL_TO_NOISE_RATIO, "SignalToNoiseRatio")
        // ================================================================
        // Photo identifiers and camera info
        // ================================================================
        .register_raw(APPLE_PHOTO_IDENTIFIER, "PhotoIdentifier")
        .register_raw(APPLE_COLOR_TEMPERATURE, "ColorTemperature")
        .register_simple_i16(APPLE_CAMERA_TYPE, "CameraType", &DECODE_CAMERA_TYPE)
        .register_raw(APPLE_FOCUS_POSITION, "FocusPosition")
        .register_raw(APPLE_HDR_GAIN, "HDRGain")
        // ================================================================
        // Front-facing camera
        // ================================================================
        .register_raw(APPLE_FRONT_FACING_CAMERA, "FrontFacingCamera")
        // ================================================================
        // Advanced AF and processing tags
        // ================================================================
        .register_raw(APPLE_AF_MEASURED_DEPTH, "AFMeasuredDepth")
        .register_raw(APPLE_AF_CONFIDENCE, "AFConfidence")
        .register_raw(APPLE_COLOR_CORRECTION_MATRIX, "ColorCorrectionMatrix")
        .register_simple_i16(
            APPLE_GREEN_GHOST_MITIGATION_STATUS,
            "GreenGhostMitigationStatus",
            &DECODE_GREEN_GHOST_MITIGATION,
        )
        // ================================================================
        // Semantic Style tags (Photographic Styles)
        // ================================================================
        .register_simple_i16(
            APPLE_SEMANTIC_STYLE,
            "SemanticStyle",
            &DECODE_SEMANTIC_STYLE,
        )
        .register_raw(
            APPLE_SEMANTIC_STYLE_RENDERING_VER,
            "SemanticStyleRenderingVer",
        )
        .register_raw(APPLE_SEMANTIC_STYLE_PRESET, "SemanticStylePreset")
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Decode front-facing camera flag to human-readable string
///
/// # Arguments
/// * `value` - Camera flag (1 = Front, 0 = Back)
///
/// # Returns
/// "Front" or "Back" based on flag value
#[inline]
pub fn decode_facing_camera(value: i16) -> String {
    if value == 1 {
        "Front".to_string()
    } else {
        "Back".to_string()
    }
}

/// Decode night mode flag to human-readable string
///
/// # Arguments
/// * `value` - Night mode flag (>0 = On, 0 = Off)
///
/// # Returns
/// "On" or "Off" based on flag value
#[inline]
pub fn decode_night_mode(value: i16) -> String {
    if value > 0 {
        "On".to_string()
    } else {
        "Off".to_string()
    }
}

/// Format runtime flags as hexadecimal string
///
/// # Arguments
/// * `value` - 32-bit runtime flags
///
/// # Returns
/// Hexadecimal representation (e.g., "0x00001234")
#[inline]
pub fn format_runtime_flags(value: u32) -> String {
    format!("0x{:08X}", value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = apple_registry();
        assert!(!registry.is_empty());
        assert!(registry.len() >= 35); // At least 35 tags registered now
    }

    #[test]
    fn test_hdr_type_decoding() {
        let registry = apple_registry();
        assert_eq!(registry.decode_i16(APPLE_HDR_IMAGE_TYPE, 0), "Off");
        assert_eq!(registry.decode_i16(APPLE_HDR_IMAGE_TYPE, 4), "Smart HDR");
        assert_eq!(registry.decode_i16(APPLE_HDR_IMAGE_TYPE, 8), "Smart HDR 5");
    }

    #[test]
    fn test_camera_type_decoding() {
        let registry = apple_registry();
        assert_eq!(registry.decode_i16(APPLE_CAMERA_TYPE, 1), "Back Normal");
        assert_eq!(registry.decode_i16(APPLE_CAMERA_TYPE, 6), "Front");
    }

    #[test]
    fn test_ois_mode_decoding() {
        let registry = apple_registry();
        assert_eq!(registry.decode_i16(APPLE_OIS_MODE, 0), "Off");
        assert_eq!(registry.decode_i16(APPLE_OIS_MODE, 1), "On");
        assert_eq!(registry.decode_i16(APPLE_OIS_MODE, 3), "Action Mode");
    }

    #[test]
    fn test_image_capture_type_decoding() {
        let registry = apple_registry();
        assert_eq!(registry.decode_i16(APPLE_IMAGE_CAPTURE_TYPE, 0), "Photo");
        assert_eq!(registry.decode_i16(APPLE_IMAGE_CAPTURE_TYPE, 1), "Portrait");
        assert_eq!(
            registry.decode_i16(APPLE_IMAGE_CAPTURE_TYPE, 4),
            "Night Mode"
        );
    }

    #[test]
    fn test_semantic_style_decoding() {
        let registry = apple_registry();
        assert_eq!(registry.decode_i16(APPLE_SEMANTIC_STYLE, 0), "Standard");
        assert_eq!(registry.decode_i16(APPLE_SEMANTIC_STYLE, 2), "Vibrant");
    }

    #[test]
    fn test_snr_type_decoding() {
        let registry = apple_registry();
        assert_eq!(
            registry.decode_i16(APPLE_SIGNAL_TO_NOISE_RATIO_TYPE, 0),
            "None"
        );
        assert_eq!(
            registry.decode_i16(APPLE_SIGNAL_TO_NOISE_RATIO_TYPE, 1),
            "Luminance"
        );
    }

    #[test]
    fn test_green_ghost_mitigation_decoding() {
        let registry = apple_registry();
        assert_eq!(
            registry.decode_i16(APPLE_GREEN_GHOST_MITIGATION_STATUS, 0),
            "Off"
        );
        assert_eq!(
            registry.decode_i16(APPLE_GREEN_GHOST_MITIGATION_STATUS, 1),
            "Applied"
        );
    }

    #[test]
    fn test_tag_names() {
        let registry = apple_registry();
        assert_eq!(
            registry.get_tag_name(APPLE_HDR_IMAGE_TYPE),
            Some("HDRImageType")
        );
        assert_eq!(registry.get_tag_name(APPLE_CAMERA_TYPE), Some("CameraType"));
        assert_eq!(
            registry.get_tag_name(APPLE_LIVE_PHOTO_VIDEO_INDEX),
            Some("LivePhotoVideoIndex")
        );
        assert_eq!(
            registry.get_tag_name(APPLE_SEMANTIC_STYLE),
            Some("SemanticStyle")
        );
        assert_eq!(
            registry.get_tag_name(APPLE_AF_MEASURED_DEPTH),
            Some("AFMeasuredDepth")
        );
    }

    #[test]
    fn test_raw_tags() {
        let registry = apple_registry();
        // Raw tags should return value as-is
        assert_eq!(registry.decode_i16(APPLE_FOCUS_POSITION, 150), "150");
        assert_eq!(registry.decode_i16(APPLE_HDR_HEADROOM, 2500), "2500");
    }

    #[test]
    fn test_helper_facing_camera() {
        assert_eq!(decode_facing_camera(1), "Front");
        assert_eq!(decode_facing_camera(0), "Back");
    }

    #[test]
    fn test_helper_night_mode() {
        assert_eq!(decode_night_mode(0), "Off");
        assert_eq!(decode_night_mode(1), "On");
        assert_eq!(decode_night_mode(5), "On");
    }

    #[test]
    fn test_helper_runtime_flags() {
        assert_eq!(format_runtime_flags(0x12345678), "0x12345678");
        assert_eq!(format_runtime_flags(0), "0x00000000");
    }
}
