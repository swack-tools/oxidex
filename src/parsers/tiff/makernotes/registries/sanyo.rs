//! Sanyo tag registry
//!
//! Registry of all Sanyo MakerNote tags with their metadata and decoders.
//! Supports Sanyo Xacti dual camera/camcorder and VPC series digital cameras.

use super::super::shared::generic_decoders::SimpleValueDecoder;
use super::super::shared::tag_registry::TagRegistry;

// ============================================================================
// Sanyo Decoders
// ============================================================================
// Inline decoders to avoid cross-module import issues

/// Decoder for Sanyo quality settings
const QUALITY: SimpleValueDecoder<u16> =
    SimpleValueDecoder::new(&[(0, "Normal"), (1, "Fine"), (2, "Super Fine")]);

/// Decoder for Sanyo focus modes
const FOCUS_MODE: SimpleValueDecoder<u16> = SimpleValueDecoder::new(&[(0, "Normal"), (1, "Macro")]);

/// Decoder for Sanyo sequential modes
const SEQUENTIAL_MODE: SimpleValueDecoder<u16> = SimpleValueDecoder::new(&[
    (0, "None"),
    (1, "Standard"),
    (2, "Best"),
    (3, "Adjust Exposure"),
]);

/// Decoder for Sanyo scene modes
const SCENE_MODE: SimpleValueDecoder<u16> = SimpleValueDecoder::new(&[
    (0, "Normal"),
    (1, "Portrait"),
    (2, "Scenery"),
    (3, "Sports"),
    (4, "Night"),
    (5, "Beach"),
    (6, "Snow"),
]);

/// Decoder for Sanyo record modes
const RECORD_MODE: SimpleValueDecoder<u16> =
    SimpleValueDecoder::new(&[(0, "Still Image"), (1, "Video")]);

// Wrapper functions to convert SimpleValueDecoder to function pointers
fn decode_quality(value: u16) -> String {
    QUALITY.decode(value)
}
fn decode_focus_mode(value: u16) -> String {
    FOCUS_MODE.decode(value)
}
fn decode_sequential_mode(value: u16) -> String {
    SEQUENTIAL_MODE.decode(value)
}
fn decode_scene_mode(value: u16) -> String {
    SCENE_MODE.decode(value)
}
fn decode_record_mode(value: u16) -> String {
    RECORD_MODE.decode(value)
}

/// Create and return the Sanyo tag registry
///
/// This registry contains all known Sanyo MakerNote tags including:
/// - Image quality and record mode settings
/// - Focus and flash modes
/// - Scene and sequential shooting modes
/// - White balance and color settings
/// - Sharpness adjustments
pub fn sanyo_registry() -> TagRegistry {
    TagRegistry::new()
        // Image Quality and Mode
        .register_u16(0x0100, "Quality", decode_quality)
        .register_u16(0x010B, "RecordMode", decode_record_mode)
        // Focus and Flash
        .register_u16(0x0102, "FocusMode", decode_focus_mode)
        .register_raw(0x0103, "FlashMode")
        // Shooting Modes
        .register_u16(0x0104, "SequentialMode", decode_sequential_mode)
        .register_u16(0x010A, "SceneMode", decode_scene_mode)
        // Color and White Balance
        .register_raw(0x0105, "WhiteBalance")
        .register_raw(0x0108, "ColorMode")
        // Image Enhancement
        .register_raw(0x0107, "Sharpness")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = sanyo_registry();

        // Verify key tags are registered
        assert!(registry.has_tag(0x0100)); // Quality
        assert!(registry.has_tag(0x0102)); // FocusMode
        assert!(registry.has_tag(0x0104)); // SequentialMode
        assert!(registry.has_tag(0x010A)); // SceneMode
        assert!(registry.has_tag(0x010B)); // RecordMode
    }

    #[test]
    fn test_registry_tag_names() {
        let registry = sanyo_registry();

        assert_eq!(registry.get_tag_name(0x0100), Some("Quality"));
        assert_eq!(registry.get_tag_name(0x0102), Some("FocusMode"));
        assert_eq!(registry.get_tag_name(0x0105), Some("WhiteBalance"));
        assert_eq!(registry.get_tag_name(0x010B), Some("RecordMode"));
    }

    #[test]
    fn test_unknown_tag() {
        let registry = sanyo_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
