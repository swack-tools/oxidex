//! Motorola tag registry
//!
//! Mirrors `%Image::ExifTool::Motorola::Main` from ExifTool 13.55
//! (`Image/ExifTool/Motorola.pm` lines 20-127). Motorola writes well over a
//! hundred maker-note IDs, but ExifTool names only six of them; the rest are
//! listed there as comments and deliberately left unnamed.

use super::super::shared::tag_registry::TagRegistry;

/// Create and return the Motorola tag registry
///
/// Every entry is traceable to a `Name =>` line in ExifTool's `Motorola.pm`.
/// All six are `Writable => 'string'`.
pub fn motorola_registry() -> TagRegistry {
    TagRegistry::new()
        // Motorola.pm:27  0x5500 => { Name => 'BuildNumber', Writable => 'string' }
        .register_string_tag(0x5500, "BuildNumber")
        // Motorola.pm:28  0x5501 => { Name => 'SerialNumber', Writable => 'string' }
        .register_string_tag(0x5501, "SerialNumber")
        // Motorola.pm:58  0x6420 => { Name => 'CustomRendered', Writable => 'string' }
        .register_string_tag(0x6420, "CustomRendered")
        // Motorola.pm:97  0x64d0 => { Name => 'DriveMode', Writable => 'string' }
        .register_string_tag(0x64d0, "DriveMode")
        // Motorola.pm:120 0x665e => { Name => 'Sensor', Writable => 'string' }
        .register_string_tag(0x665e, "Sensor")
        // Motorola.pm:126 0x6705 => { Name => 'ManufactureDate', Writable => 'string' }
        .register_string_tag(0x6705, "ManufactureDate")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every ID is quoted from ExifTool's `Motorola::Main`. Four of the six are
    /// confirmed present on the corpus file Motorola.jpg (Moto XT1575), which
    /// `exiftool -a -G1 -s` renders as:
    ///
    /// ```text
    /// [Motorola]      BuildNumber                     : LPH23.116-18
    /// [Motorola]      SerialNumber                    : NX0A3S0075
    /// [Motorola]      Sensor                          : BACK,IMX230
    /// [Motorola]      ManufactureDate                 : 03Jun2015
    /// ```
    #[test]
    fn test_registry_matches_exiftool_motorola_main() {
        let registry = motorola_registry();

        assert_eq!(registry.get_tag_name(0x5500), Some("BuildNumber"));
        assert_eq!(registry.get_tag_name(0x5501), Some("SerialNumber"));
        assert_eq!(registry.get_tag_name(0x6420), Some("CustomRendered"));
        assert_eq!(registry.get_tag_name(0x64d0), Some("DriveMode"));
        assert_eq!(registry.get_tag_name(0x665e), Some("Sensor"));
        assert_eq!(registry.get_tag_name(0x6705), Some("ManufactureDate"));

        assert_eq!(registry.len(), 6);
    }

    /// This registry previously held eight tags on IDs 0x0001-0x0008
    /// (CameraMode/HDRMode/NightMode/BurstMode/SceneMode/FlashMode/FocusMode/
    /// PortraitMode). None of those IDs or names occur in Motorola.pm, and
    /// `exiftool -v3 Motorola.jpg` shows the real maker note starts at 0x54e0 -
    /// so the old table matched nothing and emitted nothing.
    #[test]
    fn test_no_fabricated_low_ids() {
        let registry = motorola_registry();
        for id in 0x0001..=0x0008u16 {
            assert!(
                !registry.has_tag(id),
                "tag 0x{id:04x} is not in ExifTool's Motorola::Main"
            );
        }
    }

    /// IDs Motorola.pm lists only as comments must stay unnamed, or oxidex
    /// would invent names for the ~100 unknown values on every Moto photo.
    #[test]
    fn test_commented_out_ids_stay_unnamed() {
        let registry = motorola_registry();
        // Motorola.pm:25-26, :29-30, :55, :81 - all comment-only.
        for id in [
            0x54e0u16, 0x54f0, 0x5502, 0x5503, 0x5510, 0x5580, 0x6400, 0x6410,
        ] {
            assert!(
                !registry.has_tag(id),
                "tag 0x{id:04x} is a comment in Motorola.pm, not a named tag"
            );
        }
    }

    #[test]
    fn test_unknown_tag() {
        let registry = motorola_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
