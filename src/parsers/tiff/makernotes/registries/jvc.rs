//! JVC tag registry
//!
//! Mirrors `%Image::ExifTool::JVC::Main` from ExifTool 13.55
//! (`Image/ExifTool/JVC.pm` lines 20-41). ExifTool names exactly two tags in
//! the EXIF-format JVC maker note; 0x0001 is deliberately left unnamed
//! ("almost always '2', but '3' for GR-DV700 samples", JVC.pm:26).

use super::super::shared::tag_registry::TagRegistry;

// Re-export decoder from jvc.rs
use super::super::jvc::DECODE_QUALITY;

// Wrapper function to convert SimpleValueDecoder to a function pointer
fn decode_quality(value: u16) -> String {
    DECODE_QUALITY.decode(value)
}

/// Create and return the JVC tag registry
///
/// Both entries are traceable to a `Name =>` line in ExifTool's `JVC.pm`.
pub fn jvc_registry() -> TagRegistry {
    TagRegistry::new()
        // JVC.pm:27  0x0002 => { Name => 'CPUVersions', ValueConv => ... }
        .register_string_tag(0x0002, "CPUVersions")
        // JVC.pm:32  0x0003 => { Name => 'Quality',
        //            PrintConv => { 0 => 'Low', 1 => 'Normal', 2 => 'Fine' } }
        .register_u16(0x0003, "Quality", decode_quality)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both IDs are quoted from ExifTool's `JVC::Main`, and both are confirmed
    /// against the corpus file JVC.jpg (JVC GR-DV500), whose maker note
    /// `exiftool -v3` renders as:
    ///
    /// ```text
    ///   | | | 0)  JVC_0x0001 = 2
    ///   | | | 1)  CPUVersions = CPU1 2.000CPU2 04960
    ///   | | | 2)  Quality = 1
    /// ```
    #[test]
    fn test_registry_matches_exiftool_jvc_main() {
        let registry = jvc_registry();

        // JVC.pm:27-31
        assert_eq!(registry.get_tag_name(0x0002), Some("CPUVersions"));
        // JVC.pm:32-40
        assert_eq!(registry.get_tag_name(0x0003), Some("Quality"));

        assert_eq!(registry.len(), 2);
    }

    /// This registry previously named 0x0001 "Quality", 0x0002 "FocusMode" and
    /// 0x0003 "FlashMode" - each one shifted off ExifTool's table, so JVC.jpg
    /// (which carries all three IDs) produced three wrong tags. ExifTool leaves
    /// 0x0001 unnamed and 0x0004-0x0006 do not exist in `JVC::Main` at all.
    #[test]
    fn test_shifted_and_invented_ids_are_absent() {
        let registry = jvc_registry();
        assert!(!registry.has_tag(0x0001), "JVC.pm:26 leaves 0x0001 unnamed");
        for id in 0x0004..=0x0006u16 {
            assert!(
                !registry.has_tag(id),
                "tag 0x{id:04x} is not in ExifTool's JVC::Main"
            );
        }
        // The old table put Quality on 0x0001; ExifTool puts it on 0x0003.
        assert_eq!(registry.get_tag_name(0x0003), Some("Quality"));
    }

    /// ExifTool JVC.pm:34-39 - `PrintConv => { 0 => 'Low', 1 => 'Normal',
    /// 2 => 'Fine' }`. JVC.jpg carries Quality=1 and `exiftool -a -G1 -s`
    /// prints `[JVC] Quality : Normal`.
    #[test]
    fn test_quality_printconv_matches_exiftool() {
        let registry = jvc_registry();
        assert_eq!(registry.decode_u16(0x0003, 0), "Low");
        assert_eq!(registry.decode_u16(0x0003, 1), "Normal");
        assert_eq!(registry.decode_u16(0x0003, 2), "Fine");
    }

    #[test]
    fn test_unknown_tag() {
        let registry = jvc_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
