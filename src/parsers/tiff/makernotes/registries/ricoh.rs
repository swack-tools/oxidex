//! Ricoh tag registry
//!
//! Registry of all Ricoh MakerNote tags with their metadata and decoders.
//! Supports Ricoh digital cameras including GR series and Caplio models.

use super::super::shared::tag_registry::TagRegistry;
use crate::const_decoder;

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================

const_decoder!(
    RICOH_SHOOTING_MODE,
    u16,
    [
        (0, "Auto"),
        (1, "Program"),
        (2, "Aperture Priority"),
        (3, "Manual"),
    ]
);

const_decoder!(RICOH_FLASH_MODE, u16, [(0, "Auto"), (1, "On"), (2, "Off"),]);

const_decoder!(
    RICOH_WHITE_BALANCE,
    u16,
    [
        (0, "Auto"),
        (1, "Daylight"),
        (2, "Shade"),
        (3, "Fluorescent"),
        (4, "Tungsten"),
    ]
);

/// Create and return the Ricoh tag registry
///
/// This registry contains all known Ricoh MakerNote tags including:
/// - Camera model and firmware information
/// - Shooting mode and scene settings
/// - Flash and focus modes
/// - White balance and color mode
/// - ISO settings and image parameters
pub fn ricoh_registry() -> TagRegistry {
    TagRegistry::new()
        // Camera Information
        .register_raw(0x0001, "Model")
        .register_raw(0x0002, "Firmware")
        // Shooting Settings
        .register_simple_u16(0x0005, "ShootingMode", &RICOH_SHOOTING_MODE)
        .register_simple_u16(0x000C, "FlashMode", &RICOH_FLASH_MODE)
        .register_raw(0x001D, "FocusMode")
        .register_simple_u16(0x001E, "WhiteBalance", &RICOH_WHITE_BALANCE)
        .register_raw(0x0022, "ISOSetting")
        // Image Parameters
        .register_raw(0x0034, "ColorMode")
        .register_raw(0x0035, "Sharpness")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = ricoh_registry();

        // Verify key tags are registered
        assert!(registry.has_tag(0x0001)); // Model
        assert!(registry.has_tag(0x000C)); // FlashMode
        assert!(registry.has_tag(0x001E)); // WhiteBalance
    }

    #[test]
    fn test_registry_tag_names() {
        let registry = ricoh_registry();

        assert_eq!(registry.get_tag_name(0x0001), Some("Model"));
        assert_eq!(registry.get_tag_name(0x0005), Some("ShootingMode"));
        assert_eq!(registry.get_tag_name(0x0035), Some("Sharpness"));
    }

    #[test]
    fn test_unknown_tag() {
        let registry = ricoh_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
