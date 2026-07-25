//! Motorola tag registry
//!
//! Registry of all Motorola MakerNote tags with their metadata and decoders.
//! Supports Motorola smartphones including RAZR and DROID series.

use super::super::shared::tag_registry::TagRegistry;

// Re-export decoders from motorola.rs
use super::super::motorola::{CAMERA_MODE, SCENE_MODE};

// Wrapper functions to convert SimpleValueDecoder to function pointers
fn decode_camera_mode(value: u16) -> String {
    CAMERA_MODE.decode(value)
}
fn decode_scene_mode(value: u16) -> String {
    SCENE_MODE.decode(value)
}

/// Create and return the Motorola tag registry
///
/// This registry contains all known Motorola MakerNote tags including:
/// - Camera modes (auto, photo, video, portrait, night, pro)
/// - Scene detection modes
/// - HDR and night mode settings
/// - Burst shot and computational photography features
/// - Flash and focus modes
pub fn motorola_registry() -> TagRegistry {
    TagRegistry::new()
        // Camera Modes
        .register_u16(0x0001, "CameraMode", decode_camera_mode)
        // Computational Photography
        .register_raw(0x0002, "HDRMode")
        .register_raw(0x0003, "NightMode")
        // Burst and Continuous Shooting
        .register_raw(0x0004, "BurstMode")
        // Scene Detection
        .register_u16(0x0005, "SceneMode", decode_scene_mode)
        // Flash and Focus
        .register_raw(0x0006, "FlashMode")
        .register_raw(0x0007, "FocusMode")
        // Portrait Mode
        .register_raw(0x0008, "PortraitMode")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = motorola_registry();

        // Verify key tags are registered
        assert!(registry.has_tag(0x0001)); // CameraMode
        assert!(registry.has_tag(0x0002)); // HDRMode
        assert!(registry.has_tag(0x0005)); // SceneMode
        assert!(registry.has_tag(0x0008)); // PortraitMode
    }

    #[test]
    fn test_registry_tag_names() {
        let registry = motorola_registry();

        assert_eq!(registry.get_tag_name(0x0001), Some("CameraMode"));
        assert_eq!(registry.get_tag_name(0x0004), Some("BurstMode"));
        assert_eq!(registry.get_tag_name(0x0005), Some("SceneMode"));
        assert_eq!(registry.get_tag_name(0x0007), Some("FocusMode"));
    }

    #[test]
    fn test_unknown_tag() {
        let registry = motorola_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
