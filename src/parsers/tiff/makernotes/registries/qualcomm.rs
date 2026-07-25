//! Qualcomm MakerNote tag registry
//!
//! This module provides a centralized tag registry for Qualcomm Snapdragon MakerNotes,
//! eliminating code duplication by consolidating all tag definitions and decoders
//! into a single static registry.
//!
//! ## Tag Categories
//! - Clear Sight dual-camera fusion
//! - Chroma Flash multi-frame blending
//! - OptiZoom digital zoom enhancement
//! - HDR mode settings
//! - Multi-frame noise reduction
//! - AI scene detection
//! - Bokeh depth processing
//! - Low-light enhancement

use super::super::shared::generic_decoders::{ON_OFF, SimpleValueDecoder};
use super::super::shared::tag_registry::TagRegistry;
use std::sync::LazyLock;

// ============================================================================
// Tag ID Constants
// ============================================================================

/// ClearSight dual camera mode enabled
pub const QUALCOMM_CLEAR_SIGHT: u16 = 0x0001;
/// ClearSight mode type
pub const QUALCOMM_CLEAR_SIGHT_MODE: u16 = 0x0002;
/// ChromaFlash mode enabled
pub const QUALCOMM_CHROMA_FLASH: u16 = 0x0004;
/// Number of frames used for ChromaFlash
pub const QUALCOMM_CHROMA_FLASH_FRAMES: u16 = 0x0005;
/// OptiZoom hybrid zoom enabled
pub const QUALCOMM_OPTIZOOM: u16 = 0x0007;
/// Digital zoom level
pub const QUALCOMM_ZOOM_LEVEL: u16 = 0x0008;
/// HDR processing mode
pub const QUALCOMM_HDR_MODE: u16 = 0x000A;
/// Multi-frame noise reduction enabled
pub const QUALCOMM_MULTI_FRAME_NR: u16 = 0x000C;
/// Scene detection mode
pub const QUALCOMM_SCENE_DETECTION: u16 = 0x000E;
/// Bokeh depth effect mode
pub const QUALCOMM_BOKEH_MODE: u16 = 0x0010;
/// Bokeh blur intensity level
pub const QUALCOMM_BOKEH_LEVEL: u16 = 0x0011;
/// Low light enhancement mode
pub const QUALCOMM_LOW_LIGHT_MODE: u16 = 0x0013;
/// Night mode processing enabled
pub const QUALCOMM_NIGHT_MODE: u16 = 0x0015;
/// Phase detection autofocus enabled
pub const QUALCOMM_PHASE_DETECT_AF: u16 = 0x0017;
/// Image signal processor version
pub const QUALCOMM_ISP_VERSION: u16 = 0x0019;
/// Number of frames merged for final image
pub const QUALCOMM_FRAME_MERGE_COUNT: u16 = 0x001B;

// ============================================================================
// Decoders
// ============================================================================

/// Decoder for Clear Sight fusion status
pub const CLEAR_SIGHT: SimpleValueDecoder<i16> =
    SimpleValueDecoder::new(&[(0, "Off"), (1, "On"), (2, "Auto")]);

/// Decoder for Clear Sight fusion mode variant
pub const CLEAR_SIGHT_MODE: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
    (0, "None"),
    (1, "Monochrome + RGB Fusion"),
    (2, "Wide + Telephoto Fusion"),
    (3, "Multi-Camera Fusion"),
]);

/// Decoder for Chroma Flash multi-frame blending status
pub const CHROMA_FLASH: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
    (0, "Off"),
    (1, "Flash + No Flash Blend"),
    (2, "Multi-Flash Blend"),
]);

/// Decoder for HDR processing mode
pub const HDR_MODE: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
    (0, "Off"),
    (1, "HDR"),
    (2, "Auto HDR"),
    (3, "HDR+"),
    (4, "Staggered HDR"),
]);

/// Decoder for AI scene detection results
pub const SCENE_TYPE: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
    (0, "None"),
    (1, "Portrait"),
    (2, "Landscape"),
    (3, "Food"),
    (4, "Night"),
    (5, "Sunset"),
    (6, "Beach"),
    (7, "Snow"),
    (8, "Flower"),
    (9, "Pet"),
    (10, "Document"),
]);

/// Decoder for OptiZoom enhancement level
pub const OPTIZOOM: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
    (0, "Off"),
    (1, "Low"),
    (2, "Medium"),
    (3, "High"),
    (4, "Maximum"),
]);

/// Decodes zoom level from encoded value (10 = 1.0x, 100 = 10.0x)
pub fn decode_zoom_level(value: i16) -> String {
    if value <= 0 {
        return "1.0x".to_string();
    }
    let zoom = value as f32 / 10.0;
    format!("{:.1}x", zoom)
}

/// Decodes Phase Detect AF status (value > 0 = Active)
pub fn decode_phase_detect_af(value: i16) -> String {
    if value > 0 {
        "Active".to_string()
    } else {
        "Inactive".to_string()
    }
}

/// Decodes ISP version from u32 (upper 16 bits = major, lower 16 bits = minor)
pub fn decode_isp_version(value: u32) -> String {
    format!("{}.{}", value >> 16, value & 0xFFFF)
}

/// Decodes binary on/off values (value > 0 = On)
pub fn decode_binary_onoff(value: i16) -> String {
    ON_OFF.decode(if value > 0 { 1 } else { 0 })
}

// ============================================================================
// Tag Registry
// ============================================================================

/// Static registry containing all Qualcomm MakerNote tag definitions
///
/// This registry eliminates code duplication by centralizing all tag definitions,
/// reducing the need for large match statements in the parser implementation.
pub static QUALCOMM_TAGS: LazyLock<TagRegistry> = LazyLock::new(|| {
    TagRegistry::with_capacity(15)
        // Clear Sight dual-camera fusion tags
        .register_simple_i16(QUALCOMM_CLEAR_SIGHT, "ClearSight", &CLEAR_SIGHT)
        .register_simple_i16(
            QUALCOMM_CLEAR_SIGHT_MODE,
            "ClearSightMode",
            &CLEAR_SIGHT_MODE,
        )
        // Chroma Flash multi-frame blending tags
        .register_simple_i16(QUALCOMM_CHROMA_FLASH, "ChromaFlash", &CHROMA_FLASH)
        .register_raw(QUALCOMM_CHROMA_FLASH_FRAMES, "ChromaFlashFrames")
        // OptiZoom digital enhancement tags
        .register_simple_i16(QUALCOMM_OPTIZOOM, "OptiZoom", &OPTIZOOM)
        .register_i16(QUALCOMM_ZOOM_LEVEL, "ZoomLevel", decode_zoom_level)
        // HDR processing tags
        .register_simple_i16(QUALCOMM_HDR_MODE, "HDRMode", &HDR_MODE)
        // Multi-frame noise reduction (binary On/Off)
        .register_i16(
            QUALCOMM_MULTI_FRAME_NR,
            "MultiFrameNoiseReduction",
            decode_binary_onoff,
        )
        // AI scene detection tag
        .register_simple_i16(QUALCOMM_SCENE_DETECTION, "SceneDetection", &SCENE_TYPE)
        // Bokeh depth processing tags
        .register_i16(QUALCOMM_BOKEH_MODE, "BokehMode", decode_binary_onoff)
        .register_raw(QUALCOMM_BOKEH_LEVEL, "BokehLevel")
        // Low-light enhancement (binary On/Off)
        .register_i16(QUALCOMM_LOW_LIGHT_MODE, "LowLightMode", decode_binary_onoff)
        // Night mode processing (binary On/Off)
        .register_i16(QUALCOMM_NIGHT_MODE, "NightMode", decode_binary_onoff)
        // Phase detection autofocus tag
        .register_i16(
            QUALCOMM_PHASE_DETECT_AF,
            "PhaseDetectAF",
            decode_phase_detect_af,
        )
        // ISP version tag (u32 decoder)
        .register_u32(QUALCOMM_ISP_VERSION, "ISPVersion", decode_isp_version)
        // Frame merge count (raw numeric value)
        .register_raw(QUALCOMM_FRAME_MERGE_COUNT, "FrameMergeCount")
});

/// Returns a reference to the Qualcomm tag registry
///
/// This function provides access to the centralized tag registry,
/// allowing the parser to look up tag names and decoders efficiently.
pub fn qualcomm_registry() -> &'static TagRegistry {
    &QUALCOMM_TAGS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clear_sight_decoder() {
        assert_eq!(CLEAR_SIGHT.decode(0), "Off");
        assert_eq!(CLEAR_SIGHT.decode(1), "On");
        assert_eq!(CLEAR_SIGHT.decode(2), "Auto");
    }

    #[test]
    fn test_clear_sight_mode_decoder() {
        assert_eq!(CLEAR_SIGHT_MODE.decode(0), "None");
        assert_eq!(CLEAR_SIGHT_MODE.decode(1), "Monochrome + RGB Fusion");
        assert_eq!(CLEAR_SIGHT_MODE.decode(3), "Multi-Camera Fusion");
    }

    #[test]
    fn test_chroma_flash_decoder() {
        assert_eq!(CHROMA_FLASH.decode(0), "Off");
        assert_eq!(CHROMA_FLASH.decode(1), "Flash + No Flash Blend");
    }

    #[test]
    fn test_hdr_mode_decoder() {
        assert_eq!(HDR_MODE.decode(0), "Off");
        assert_eq!(HDR_MODE.decode(1), "HDR");
        assert_eq!(HDR_MODE.decode(4), "Staggered HDR");
    }

    #[test]
    fn test_scene_type_decoder() {
        assert_eq!(SCENE_TYPE.decode(0), "None");
        assert_eq!(SCENE_TYPE.decode(1), "Portrait");
        assert_eq!(SCENE_TYPE.decode(10), "Document");
    }

    #[test]
    fn test_optizoom_decoder() {
        assert_eq!(OPTIZOOM.decode(0), "Off");
        assert_eq!(OPTIZOOM.decode(2), "Medium");
        assert_eq!(OPTIZOOM.decode(4), "Maximum");
    }

    #[test]
    fn test_decode_zoom_level() {
        assert_eq!(decode_zoom_level(10), "1.0x");
        assert_eq!(decode_zoom_level(50), "5.0x");
        assert_eq!(decode_zoom_level(100), "10.0x");
    }

    #[test]
    fn test_decode_phase_detect_af() {
        assert_eq!(decode_phase_detect_af(0), "Inactive");
        assert_eq!(decode_phase_detect_af(1), "Active");
    }

    #[test]
    fn test_decode_isp_version() {
        assert_eq!(decode_isp_version(0x00010002), "1.2");
        assert_eq!(decode_isp_version(0x00020003), "2.3");
    }

    #[test]
    fn test_decode_binary_onoff() {
        assert_eq!(decode_binary_onoff(0), "Off");
        assert_eq!(decode_binary_onoff(1), "On");
        assert_eq!(decode_binary_onoff(5), "On");
    }

    #[test]
    fn test_registry_has_all_tags() {
        let registry = qualcomm_registry();
        assert!(registry.has_tag(QUALCOMM_CLEAR_SIGHT));
        assert!(registry.has_tag(QUALCOMM_CLEAR_SIGHT_MODE));
        assert!(registry.has_tag(QUALCOMM_CHROMA_FLASH));
        assert!(registry.has_tag(QUALCOMM_CHROMA_FLASH_FRAMES));
        assert!(registry.has_tag(QUALCOMM_OPTIZOOM));
        assert!(registry.has_tag(QUALCOMM_ZOOM_LEVEL));
        assert!(registry.has_tag(QUALCOMM_HDR_MODE));
        assert!(registry.has_tag(QUALCOMM_MULTI_FRAME_NR));
        assert!(registry.has_tag(QUALCOMM_SCENE_DETECTION));
        assert!(registry.has_tag(QUALCOMM_BOKEH_MODE));
        assert!(registry.has_tag(QUALCOMM_BOKEH_LEVEL));
        assert!(registry.has_tag(QUALCOMM_LOW_LIGHT_MODE));
        assert!(registry.has_tag(QUALCOMM_NIGHT_MODE));
        assert!(registry.has_tag(QUALCOMM_PHASE_DETECT_AF));
        assert!(registry.has_tag(QUALCOMM_ISP_VERSION));
        assert!(registry.has_tag(QUALCOMM_FRAME_MERGE_COUNT));
    }

    #[test]
    fn test_registry_count() {
        let registry = qualcomm_registry();
        assert_eq!(registry.len(), 16);
    }
}
