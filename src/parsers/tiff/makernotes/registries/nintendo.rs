//! Nintendo tag registry
//!
//! Registry of all Nintendo 3DS MakerNote tags with their metadata and decoders.
//! Supports Nintendo 3DS and 2DS handheld gaming device cameras.

use super::super::shared::tag_registry::TagRegistry;

// Re-export decoders from nintendo.rs
use super::super::nintendo::{CAMERA_MODE, CAMERA_SELECTION, FILTER};

// Wrapper functions to convert SimpleValueDecoder to function pointers
fn decode_camera_mode(value: i16) -> String {
    CAMERA_MODE.decode(value)
}
fn decode_camera_selection(value: i16) -> String {
    CAMERA_SELECTION.decode(value)
}
fn decode_filter(value: i16) -> String {
    FILTER.decode(value)
}

/// Create and return the Nintendo tag registry
///
/// This registry contains all known Nintendo MakerNote tags including:
/// - Camera mode (2D/3D)
/// - Camera selection (inner/outer)
/// - Parallax and 3D effect settings
/// - Face and Mii detection
/// - Photo filters
/// - Game title metadata
pub fn nintendo_registry() -> TagRegistry {
    TagRegistry::new()
        // Device Information
        .register_raw(0x0001, "Model")
        .register_raw(0x0002, "SystemVersion")
        // Camera Modes
        .register_i16(0x0100, "CameraMode", decode_camera_mode)
        .register_i16(0x0101, "CameraSelection", decode_camera_selection)
        // Stereoscopic Settings
        .register_raw(0x0102, "Parallax")
        .register_raw(0x0103, "3DEffect")
        // Detection Features
        .register_raw(0x0104, "FaceDetection")
        .register_raw(0x0105, "MiiDetected")
        // Photo Effects
        .register_i16(0x0106, "Filter", decode_filter)
        // Game Integration
        .register_raw(0x0107, "GameTitle")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = nintendo_registry();

        // Verify key tags are registered
        assert!(registry.has_tag(0x0001)); // Model
        assert!(registry.has_tag(0x0100)); // CameraMode
        assert!(registry.has_tag(0x0101)); // CameraSelection
        assert!(registry.has_tag(0x0106)); // Filter
    }

    #[test]
    fn test_registry_tag_names() {
        let registry = nintendo_registry();

        assert_eq!(registry.get_tag_name(0x0001), Some("Model"));
        assert_eq!(registry.get_tag_name(0x0100), Some("CameraMode"));
        assert_eq!(registry.get_tag_name(0x0102), Some("Parallax"));
        assert_eq!(registry.get_tag_name(0x0107), Some("GameTitle"));
    }

    #[test]
    fn test_unknown_tag() {
        let registry = nintendo_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
