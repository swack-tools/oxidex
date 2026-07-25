//! Google tag registry with tag definitions
//!
//! This module contains TagRegistry definitions for Google Pixel MakerNotes,
//! providing declarative tag definitions for computational photography metadata.

use super::super::shared::tag_registry::TagRegistry;

// Re-export existing decoders from google.rs
use super::super::google::{
    DECODE_HDR_PLUS_MODE, DECODE_NIGHT_SIGHT, DECODE_SCENE_TYPE
};

/// Create Google tag registry with all tag definitions
///
/// Google Pixel MakerNotes contain computational photography settings.
/// Unlike traditional cameras, Google doesn't use array-based tags extensively,
/// instead using individual tags for each feature (HDR+, Night Sight, etc.).
///
/// # Returns
/// A TagRegistry configured for Google Pixel MakerNote parsing
pub fn google_registry() -> TagRegistry {
    TagRegistry::new()
        // Computational photography mode tags with decoders
        .register_simple_i16(0x0001, "HDRPlusMode", &DECODE_HDR_PLUS_MODE)
        .register_simple_i16(0x0003, "NightSight", &DECODE_NIGHT_SIGHT)
        .register_simple_i16(0x000B, "SceneDetection", &DECODE_SCENE_TYPE)

        // Raw numeric tags (no decoders needed)
        .register_raw(0x0004, "NightSightExposureTime")  // Exposure time in ms
        .register_raw(0x0005, "SuperResZoom")            // Zoom level (needs custom decoder)
        .register_raw(0x0009, "FaceRetouching")          // 0-100 value
        .register_raw(0x000D, "PortraitBlur")            // Blur amount
        .register_raw(0x0019, "MergedFrameCount")        // Number of frames merged
        .register_raw(0x001B, "ExposureStack")           // Multi-exposure stack info

        // String tags (handled separately in parser)
        .register_raw(0x0007, "MotionPhotoID")           // Motion Photo video identifier

        // Boolean-style tags (0=Off, >0=On, handled in parser)
        .register_raw(0x000F, "ColorPop")                // Color Pop effect
        .register_raw(0x0011, "Astrophotography")        // Astrophotography mode
        .register_raw(0x0013, "CinematicMode")           // Cinematic blur mode
        .register_raw(0x0015, "MagicEraser")             // Magic Eraser applied
        .register_raw(0x0017, "FaceUnblur")              // Face Unblur applied
}
