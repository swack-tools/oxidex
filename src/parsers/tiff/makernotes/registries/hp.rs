//! HP tag registry
//!
//! Mirrors `%Image::ExifTool::HP::Main` from ExifTool 13.55
//! (`Image/ExifTool/HP.pm` lines 21-38). That EXIF-format table (PhotoSmart
//! 720, and the Vivitar ViviCam 3705/3705B/3715 that reuse it) contains
//! exactly one tag: 0x0e00 PrintIM.
//!
//! HP's other maker-note flavours - `HP::Type2`, `HP::Type4`, `HP::Type6` and
//! `HP::TDHD` - are **not** IFDs. They are `ProcessBinaryData` tables keyed by
//! byte offset (e.g. Type4 0x0c MaxAperture, 0x10 ExposureTime, 0x34 ISO), so
//! their offsets must never be registered here as IFD tag IDs.

use super::super::shared::tag_registry::TagRegistry;

/// Create and return the HP tag registry
///
/// PrintIM is a SubDirectory in ExifTool and is handled by the shared PrintIM
/// path rather than by this registry, so the registry is intentionally empty.
/// It must stay empty until a real `HP.pm` table is implemented: naming IDs
/// that ExifTool does not name produces confidently wrong output.
pub fn hp_registry() -> TagRegistry {
    TagRegistry::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This registry previously named six tags - 0x0001 Model, 0x0003 Quality,
    /// 0x0005 ColorMode, 0x0007 FlashMode, 0x0009 WhiteBalance and
    /// 0x000B Sharpness. None of those IDs or names appears anywhere in
    /// ExifTool's `HP.pm`; the only ID in `HP::Main` is 0x0e00 (PrintIM).
    #[test]
    fn test_registry_has_no_fabricated_tags() {
        let registry = hp_registry();
        for (id, name) in [
            (0x0001u16, "Model"),
            (0x0003, "Quality"),
            (0x0005, "ColorMode"),
            (0x0007, "FlashMode"),
            (0x0009, "WhiteBalance"),
            (0x000B, "Sharpness"),
        ] {
            assert!(
                !registry.has_tag(id),
                "0x{id:04x} ({name}) is not in ExifTool's HP::Main"
            );
        }
        assert!(registry.is_empty());
    }

    #[test]
    fn test_unknown_tag() {
        let registry = hp_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
