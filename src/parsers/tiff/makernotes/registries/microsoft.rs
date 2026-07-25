//! Microsoft (Lumia) MakerNote tag registry
//!
//! This module provides a centralized tag registry for Microsoft Lumia MakerNotes,
//! eliminating code duplication by consolidating all tag definitions and decoders
//! into a single static registry.
//!
//! ## Tag Categories
//! - Rich Capture mode (HDR + flash variants)
//! - Living Images (video + still)
//! - Dynamic Flash blending
//! - Refocus depth data
//! - PureView oversampling info
//! - Lumia Creative Studio effects
//! - 4K video recording data
//! - Audio recording settings

use super::super::shared::generic_decoders::{ON_OFF, SimpleValueDecoder};
use super::super::shared::tag_registry::TagRegistry;
use once_cell::sync::Lazy;

// ============================================================================
// Tag ID Constants
// ============================================================================

/// Rich Capture mode tag (Off/On/Auto)
pub const MICROSOFT_RICH_CAPTURE: u16 = 0x0001;
/// Rich Capture variant type (HDR, Flash, etc.)
pub const MICROSOFT_RICH_CAPTURE_MODE: u16 = 0x0002;
/// Living Image availability
pub const MICROSOFT_LIVING_IMAGE: u16 = 0x0004;
/// Dynamic Flash blending mode
pub const MICROSOFT_DYNAMIC_FLASH: u16 = 0x0006;
/// Refocus availability status
pub const MICROSOFT_REFOCUS: u16 = 0x0008;
/// Refocus depth map resolution
pub const MICROSOFT_REFOCUS_DEPTH: u16 = 0x0009;
/// PureView oversampling mode
pub const MICROSOFT_PUREVIEW_MODE: u16 = 0x000B;
/// PureView full resolution
pub const MICROSOFT_PUREVIEW_RESOLUTION: u16 = 0x000C;
/// Creative Studio effect type
pub const MICROSOFT_CREATIVE_EFFECT: u16 = 0x000E;
/// 4K video recording enabled
pub const MICROSOFT_VIDEO_4K: u16 = 0x0010;
/// Rich audio recording mode
pub const MICROSOFT_AUDIO_RICHRECORD: u16 = 0x0012;
/// Optical image stabilization status
pub const MICROSOFT_STABILIZATION: u16 = 0x0014;
/// Auto HDR mode enabled
pub const MICROSOFT_AUTO_HDR: u16 = 0x0016;
/// Panorama mode enabled
pub const MICROSOFT_PANORAMA_MODE: u16 = 0x0018;
/// Lens attachment type
pub const MICROSOFT_LENS_TYPE: u16 = 0x001A;

// ============================================================================
// Decoders
// ============================================================================

/// Decoder for Rich Capture mode (Off/On/Auto)
pub const RICH_CAPTURE: SimpleValueDecoder<i16> =
    SimpleValueDecoder::new(&[(0, "Off"), (1, "On"), (2, "Auto")]);

/// Decoder for Rich Capture variant type
pub const RICH_CAPTURE_MODE: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
    (0, "None"),
    (1, "HDR"),
    (2, "HDR + Flash"),
    (3, "Flash Variants"),
    (4, "Motion Blur Removal"),
]);

/// Decoder for Dynamic Flash status
pub const DYNAMIC_FLASH: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
    (0, "Off"),
    (1, "Flash + No Flash Blend"),
    (2, "Multi-Flash Blend"),
]);

/// Decoder for PureView oversampling mode
pub const PUREVIEW_MODE: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
    (0, "Off"),
    (1, "5MP Oversampled"),
    (2, "8MP Oversampled"),
    (3, "Full Resolution"),
    (4, "Lossless Zoom"),
]);

/// Decoder for Creative Studio effect type
pub const CREATIVE_EFFECT: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
    (0, "None"),
    (1, "Black & White"),
    (2, "Sepia"),
    (3, "Vintage"),
    (4, "Vivid"),
    (5, "Warm"),
    (6, "Cool"),
    (7, "Stamp"),
    (8, "Posterize"),
]);

/// Decoder for lens attachment type
pub const LENS_TYPE: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
    (0, "Built-in"),
    (1, "Wide Angle Attachment"),
    (2, "Telephoto Attachment"),
    (3, "Macro Attachment"),
]);

/// Decodes Refocus availability status (value > 0 = Available)
pub fn decode_refocus(value: i16) -> String {
    if value > 0 {
        "Available".to_string()
    } else {
        "Not Available".to_string()
    }
}

/// Decodes resolution from packed u32 (upper 16 bits = width, lower 16 bits = height)
pub fn decode_resolution(value: u32) -> String {
    format!("{}x{}", value >> 16, value & 0xFFFF)
}

/// Decodes binary on/off values (value > 0 = On)
pub fn decode_binary_onoff(value: i16) -> String {
    ON_OFF.decode(if value > 0 { 1 } else { 0 })
}

/// Decodes OIS stabilization status (value > 0 = On (OIS))
pub fn decode_stabilization(value: i16) -> String {
    if value > 0 {
        "On (OIS)".to_string()
    } else {
        "Off".to_string()
    }
}

// ============================================================================
// Tag Registry
// ============================================================================

/// Static registry containing all Microsoft MakerNote tag definitions
///
/// This registry eliminates code duplication by centralizing all tag definitions,
/// reducing the need for large match statements in the parser implementation.
pub static MICROSOFT_TAGS: Lazy<TagRegistry> = Lazy::new(|| {
    TagRegistry::with_capacity(15)
        // Rich Capture tags
        .register_simple_i16(MICROSOFT_RICH_CAPTURE, "RichCapture", &RICH_CAPTURE)
        .register_simple_i16(
            MICROSOFT_RICH_CAPTURE_MODE,
            "RichCaptureMode",
            &RICH_CAPTURE_MODE,
        )
        // Living Image (handled separately - string value)
        // Dynamic Flash
        .register_simple_i16(MICROSOFT_DYNAMIC_FLASH, "DynamicFlash", &DYNAMIC_FLASH)
        // Refocus tags
        .register_i16(MICROSOFT_REFOCUS, "Refocus", decode_refocus)
        .register_u32(
            MICROSOFT_REFOCUS_DEPTH,
            "RefocusDepthResolution",
            decode_resolution,
        )
        // PureView tags
        .register_simple_i16(MICROSOFT_PUREVIEW_MODE, "PureViewMode", &PUREVIEW_MODE)
        .register_u32(
            MICROSOFT_PUREVIEW_RESOLUTION,
            "PureViewFullResolution",
            decode_resolution,
        )
        // Creative effect
        .register_simple_i16(
            MICROSOFT_CREATIVE_EFFECT,
            "CreativeEffect",
            &CREATIVE_EFFECT,
        )
        // Binary on/off tags
        .register_i16(MICROSOFT_VIDEO_4K, "Video4K", decode_binary_onoff)
        .register_i16(
            MICROSOFT_AUDIO_RICHRECORD,
            "RichRecordingAudio",
            decode_binary_onoff,
        )
        .register_i16(
            MICROSOFT_STABILIZATION,
            "OpticalStabilization",
            decode_stabilization,
        )
        .register_i16(MICROSOFT_AUTO_HDR, "AutoHDR", decode_binary_onoff)
        .register_i16(MICROSOFT_PANORAMA_MODE, "PanoramaMode", decode_binary_onoff)
        // Lens type
        .register_simple_i16(MICROSOFT_LENS_TYPE, "LensType", &LENS_TYPE)
});

/// Returns a reference to the Microsoft tag registry
///
/// This function provides access to the centralized tag registry,
/// allowing the parser to look up tag names and decoders efficiently.
pub fn microsoft_registry() -> &'static TagRegistry {
    &MICROSOFT_TAGS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rich_capture_decoder() {
        assert_eq!(RICH_CAPTURE.decode(0), "Off");
        assert_eq!(RICH_CAPTURE.decode(1), "On");
        assert_eq!(RICH_CAPTURE.decode(2), "Auto");
    }

    #[test]
    fn test_rich_capture_mode_decoder() {
        assert_eq!(RICH_CAPTURE_MODE.decode(0), "None");
        assert_eq!(RICH_CAPTURE_MODE.decode(1), "HDR");
        assert_eq!(RICH_CAPTURE_MODE.decode(2), "HDR + Flash");
    }

    #[test]
    fn test_dynamic_flash_decoder() {
        assert_eq!(DYNAMIC_FLASH.decode(0), "Off");
        assert_eq!(DYNAMIC_FLASH.decode(1), "Flash + No Flash Blend");
    }

    #[test]
    fn test_pureview_mode_decoder() {
        assert_eq!(PUREVIEW_MODE.decode(0), "Off");
        assert_eq!(PUREVIEW_MODE.decode(1), "5MP Oversampled");
        assert_eq!(PUREVIEW_MODE.decode(4), "Lossless Zoom");
    }

    #[test]
    fn test_creative_effect_decoder() {
        assert_eq!(CREATIVE_EFFECT.decode(0), "None");
        assert_eq!(CREATIVE_EFFECT.decode(1), "Black & White");
        assert_eq!(CREATIVE_EFFECT.decode(4), "Vivid");
    }

    #[test]
    fn test_lens_type_decoder() {
        assert_eq!(LENS_TYPE.decode(0), "Built-in");
        assert_eq!(LENS_TYPE.decode(1), "Wide Angle Attachment");
    }

    #[test]
    fn test_decode_refocus() {
        assert_eq!(decode_refocus(0), "Not Available");
        assert_eq!(decode_refocus(1), "Available");
        assert_eq!(decode_refocus(5), "Available");
    }

    #[test]
    fn test_decode_resolution() {
        assert_eq!(decode_resolution(0x19200F00), "6432x3840");
        assert_eq!(decode_resolution(0x0F001920), "3840x6432");
    }

    #[test]
    fn test_decode_binary_onoff() {
        assert_eq!(decode_binary_onoff(0), "Off");
        assert_eq!(decode_binary_onoff(1), "On");
        assert_eq!(decode_binary_onoff(5), "On");
    }

    #[test]
    fn test_decode_stabilization() {
        assert_eq!(decode_stabilization(0), "Off");
        assert_eq!(decode_stabilization(1), "On (OIS)");
    }

    #[test]
    fn test_registry_has_all_tags() {
        let registry = microsoft_registry();
        assert!(registry.has_tag(MICROSOFT_RICH_CAPTURE));
        assert!(registry.has_tag(MICROSOFT_RICH_CAPTURE_MODE));
        assert!(registry.has_tag(MICROSOFT_DYNAMIC_FLASH));
        assert!(registry.has_tag(MICROSOFT_REFOCUS));
        assert!(registry.has_tag(MICROSOFT_REFOCUS_DEPTH));
        assert!(registry.has_tag(MICROSOFT_PUREVIEW_MODE));
        assert!(registry.has_tag(MICROSOFT_PUREVIEW_RESOLUTION));
        assert!(registry.has_tag(MICROSOFT_CREATIVE_EFFECT));
        assert!(registry.has_tag(MICROSOFT_VIDEO_4K));
        assert!(registry.has_tag(MICROSOFT_AUDIO_RICHRECORD));
        assert!(registry.has_tag(MICROSOFT_STABILIZATION));
        assert!(registry.has_tag(MICROSOFT_AUTO_HDR));
        assert!(registry.has_tag(MICROSOFT_PANORAMA_MODE));
        assert!(registry.has_tag(MICROSOFT_LENS_TYPE));
    }

    #[test]
    fn test_registry_count() {
        let registry = microsoft_registry();
        assert_eq!(registry.len(), 14);
    }
}
