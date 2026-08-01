//! GE (General Imaging) tag registry
//!
//! Mirrors `%Image::ExifTool::GE::Main` from ExifTool 13.55
//! (`Image/ExifTool/GE.pm` lines 22-52). That table defines exactly three
//! named tags; every other ID GE writes is left deliberately unnamed by
//! ExifTool and appears there only as a comment.

use super::super::shared::tag_registry::TagRegistry;

// Re-export decoder from ge.rs
use super::super::ge::DECODE_MACRO;

// Wrapper function to convert SimpleValueDecoder to a function pointer
fn decode_macro(value: u16) -> String {
    DECODE_MACRO.decode(value)
}

/// Create and return the GE tag registry
///
/// Every entry below is traceable to a `Name =>` line in ExifTool's `GE.pm`.
/// ExifTool explicitly leaves 0x0104, 0x0200, 0x0203-0x0206, 0x0500 and
/// 0x0600 unnamed (they appear only as comments in `GE::Main`), so this
/// registry must not name them either.
pub fn ge_registry() -> TagRegistry {
    TagRegistry::new()
        // GE.pm:33  0x0202 => { Name => 'Macro', PrintConv => { 0 => 'Off', 1 => 'On' } }
        .register_u16(0x0202, "Macro", decode_macro)
        // GE.pm:42  0x0207 => { Name => 'GEModel', Format => 'string' }
        .register_string_tag(0x0207, "GEModel")
        // GE.pm:46  0x0300 => { Name => 'GEMake', Format => 'string' }
        .register_string_tag(0x0300, "GEMake")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every ID here is quoted from ExifTool's `GE::Main`; a bare
    /// `get_tag_name(x) == Some(y)` round-trip would only mirror this file
    /// back at itself, so the ExifTool citation is the real assertion.
    #[test]
    fn test_registry_matches_exiftool_ge_main() {
        let registry = ge_registry();

        // GE.pm:33-41
        assert_eq!(registry.get_tag_name(0x0202), Some("Macro"));
        // GE.pm:42-45
        assert_eq!(registry.get_tag_name(0x0207), Some("GEModel"));
        // GE.pm:46-49
        assert_eq!(registry.get_tag_name(0x0300), Some("GEMake"));

        // ExifTool names exactly three GE tags - nothing else may be invented.
        assert_eq!(registry.len(), 3);
    }

    /// Guards the fabricated table this registry used to contain: tag IDs
    /// 0x0001-0x0005 named Quality/FocusMode/FlashMode/SceneMode/WhiteBalance.
    /// `exiftool -v3` on the corpus file GE.jpg shows the real GE maker note
    /// uses 0x0104/0x0200/0x0202-0x0207/0x0300/0x0500/0x0600 and never touches
    /// 0x0001-0x0005.
    #[test]
    fn test_no_fabricated_low_ids() {
        let registry = ge_registry();
        for id in 0x0001..=0x0005u16 {
            assert!(
                !registry.has_tag(id),
                "tag 0x{id:04x} is not in ExifTool's GE::Main"
            );
        }
    }

    /// ExifTool's Macro PrintConv is `{ 0 => 'Off', 1 => 'On' }` (GE.pm:37).
    /// The corpus file GE.jpg carries Macro=0 and `exiftool -a -G1 -s` prints
    /// `[GE] Macro : Off`.
    #[test]
    fn test_macro_printconv_matches_exiftool() {
        let registry = ge_registry();
        assert_eq!(registry.decode_u16(0x0202, 0), "Off");
        assert_eq!(registry.decode_u16(0x0202, 1), "On");
    }

    #[test]
    fn test_unknown_tag() {
        let registry = ge_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
