//! Nintendo tag registry
//!
//! Mirrors `%Image::ExifTool::Nintendo::Main` from ExifTool 13.55
//! (`Image/ExifTool/Nintendo.pm` lines 19-34). That table holds exactly one
//! entry:
//!
//! ```text
//! 0x1101 => {
//!     Name => 'CameraInfo',
//!     SubDirectory => {
//!         TagTable => 'Image::ExifTool::Nintendo::CameraInfo',
//!         ByteOrder => 'Little-endian',
//!     },
//! },
//! ```
//!
//! Everything a 3DS actually reports - ModelID, TimeStamp,
//! InternalSerialNumber, Parallax and Category - lives inside that
//! `ProcessBinaryData` subdirectory at byte offsets 0x00/0x08/0x18/0x28/0x30
//! (`Nintendo::CameraInfo`, Nintendo.pm:37-89), not as IFD tag IDs.

use super::super::shared::tag_registry::TagRegistry;

/// Create and return the Nintendo tag registry
///
/// Intentionally empty: `Nintendo::Main`'s only tag is a SubDirectory, and the
/// binary `CameraInfo` block it points at is not implemented yet. Emitting
/// nothing is correct; emitting invented tag names is not.
pub fn nintendo_registry() -> TagRegistry {
    TagRegistry::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This registry previously declared ten tags on IDs 0x0001, 0x0002 and
    /// 0x0100-0x0107, named Model, SystemVersion, CameraMode, CameraSelection,
    /// Parallax, 3DEffect, FaceDetection, MiiDetected, Filter and GameTitle.
    ///
    /// None of those IDs or names occurs anywhere in ExifTool's `Nintendo.pm`.
    /// The only ID in `Nintendo::Main` is 0x1101, so on a real 3DS MPO the old
    /// table matched nothing - its own mirror tests were the only thing it ever
    /// satisfied.
    #[test]
    fn test_registry_has_no_fabricated_tags() {
        let registry = nintendo_registry();
        for (id, name) in [
            (0x0001u16, "Model"),
            (0x0002, "SystemVersion"),
            (0x0100, "CameraMode"),
            (0x0101, "CameraSelection"),
            (0x0102, "Parallax"),
            (0x0103, "3DEffect"),
            (0x0104, "FaceDetection"),
            (0x0105, "MiiDetected"),
            (0x0106, "Filter"),
            (0x0107, "GameTitle"),
        ] {
            assert!(
                !registry.has_tag(id),
                "0x{id:04x} ({name}) is not in ExifTool's Nintendo::Main"
            );
        }
        assert!(registry.is_empty());
    }

    #[test]
    fn test_unknown_tag() {
        let registry = nintendo_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
