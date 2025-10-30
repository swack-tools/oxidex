//! TIFF enumeration value mappings
//!
//! This module provides mappings from numeric TIFF tag values to their
//! human-readable string representations, matching Perl ExifTool output.

/// Maps TIFF tag enum values to their string representations.
///
/// Returns the human-readable string for the given tag ID and value,
/// or None if the tag/value combination doesn't have a known mapping.
pub fn tiff_enum_to_string(tag_id: u16, value: i64) -> Option<String> {
    match tag_id {
        // Orientation (tag 0x0112)
        0x0112 => match value {
            1 => Some("Horizontal (normal)".to_string()),
            2 => Some("Mirror horizontal".to_string()),
            3 => Some("Rotate 180".to_string()),
            4 => Some("Mirror vertical".to_string()),
            5 => Some("Mirror horizontal and rotate 270 CW".to_string()),
            6 => Some("Rotate 90 CW".to_string()),
            7 => Some("Mirror horizontal and rotate 90 CW".to_string()),
            8 => Some("Rotate 270 CW".to_string()),
            _ => None,
        },

        // Compression (tag 0x0103)
        0x0103 => match value {
            1 => Some("Uncompressed".to_string()),
            2 => Some("CCITT 1D".to_string()),
            3 => Some("T4/Group 3 Fax".to_string()),
            4 => Some("T6/Group 4 Fax".to_string()),
            5 => Some("LZW".to_string()),
            6 => Some("JPEG (old-style)".to_string()),
            7 => Some("JPEG".to_string()),
            8 => Some("Adobe Deflate".to_string()),
            9 => Some("JBIG B&W".to_string()),
            10 => Some("JBIG Color".to_string()),
            99 => Some("JPEG".to_string()),
            262 => Some("Kodak 262".to_string()),
            32766 => Some("Next".to_string()),
            32767 => Some("Sony ARW Compressed".to_string()),
            32769 => Some("Packed RAW".to_string()),
            32770 => Some("Samsung SRW Compressed".to_string()),
            32771 => Some("CCIRLEW".to_string()),
            32773 => Some("PackBits".to_string()),
            32809 => Some("Thunderscan".to_string()),
            32867 => Some("Kodak KDC Compressed".to_string()),
            32895 => Some("IT8CTPAD".to_string()),
            32896 => Some("IT8LW".to_string()),
            32897 => Some("IT8MP".to_string()),
            32898 => Some("IT8BL".to_string()),
            32908 => Some("PixarFilm".to_string()),
            32909 => Some("PixarLog".to_string()),
            32946 => Some("Deflate".to_string()),
            32947 => Some("DCS".to_string()),
            34661 => Some("JBIG".to_string()),
            34676 => Some("SGILog".to_string()),
            34677 => Some("SGILog24".to_string()),
            34712 => Some("JPEG 2000".to_string()),
            34713 => Some("Nikon NEF Compressed".to_string()),
            34715 => Some("JBIG2 TIFF FX".to_string()),
            34718 => Some("Microsoft Document Imaging (MDI) Binary Level Codec".to_string()),
            34719 => Some("Microsoft Document Imaging (MDI) Progressive Transform Codec".to_string()),
            34720 => Some("Microsoft Document Imaging (MDI) Vector".to_string()),
            34892 => Some("Lossy JPEG".to_string()),
            65000 => Some("Kodak DCR Compressed".to_string()),
            65535 => Some("Pentax PEF Compressed".to_string()),
            _ => None,
        },

        // PhotometricInterpretation (tag 0x0106)
        0x0106 => match value {
            0 => Some("WhiteIsZero".to_string()),
            1 => Some("BlackIsZero".to_string()),
            2 => Some("RGB".to_string()),
            3 => Some("RGB Palette".to_string()),
            4 => Some("Transparency Mask".to_string()),
            5 => Some("CMYK".to_string()),
            6 => Some("YCbCr".to_string()),
            8 => Some("CIELab".to_string()),
            9 => Some("ICCLab".to_string()),
            10 => Some("ITULab".to_string()),
            32803 => Some("Color Filter Array".to_string()),
            32844 => Some("Pixar LogL".to_string()),
            32845 => Some("Pixar LogLuv".to_string()),
            34892 => Some("Linear Raw".to_string()),
            _ => None,
        },

        // PlanarConfiguration (tag 0x011C)
        0x011C => match value {
            1 => Some("Chunky".to_string()),
            2 => Some("Planar".to_string()),
            _ => None,
        },

        // ResolutionUnit (tag 0x0128)
        0x0128 => match value {
            1 => Some("None".to_string()),
            2 => Some("inches".to_string()),
            3 => Some("cm".to_string()),
            _ => None,
        },

        // FillOrder (tag 0x010A)
        0x010A => match value {
            1 => Some("Normal".to_string()),
            2 => Some("Reversed".to_string()),
            _ => None,
        },

        // SampleFormat (tag 0x0153)
        0x0153 => match value {
            1 => Some("Unsigned".to_string()),
            2 => Some("Signed".to_string()),
            3 => Some("Float".to_string()),
            4 => Some("Undefined".to_string()),
            5 => Some("Complex int".to_string()),
            6 => Some("Complex float".to_string()),
            _ => None,
        },

        // YCbCrPositioning (tag 0x0213)
        0x0213 => match value {
            1 => Some("Centered".to_string()),
            2 => Some("Co-sited".to_string()),
            _ => None,
        },

        // ExtraSamples (tag 0x0152)
        0x0152 => match value {
            0 => Some("Unspecified".to_string()),
            1 => Some("Associated Alpha".to_string()),
            2 => Some("Unassociated Alpha".to_string()),
            _ => None,
        },

        // SubfileType (tag 0x00FE)
        0x00FE => match value {
            0 => Some("Full-resolution image".to_string()),
            1 => Some("Reduced-resolution image".to_string()),
            2 => Some("Single page of multi-page image".to_string()),
            3 => Some("Single page of multi-page reduced-resolution image".to_string()),
            4 => Some("Transparency mask".to_string()),
            5 => Some("Transparency mask of reduced-resolution image".to_string()),
            6 => Some("Transparency mask of multi-page image".to_string()),
            7 => Some("Transparency mask of reduced-resolution multi-page image".to_string()),
            _ => None,
        },

        // ColorSpace (EXIF tag 0xA001)
        0xA001 => match value {
            1 => Some("sRGB".to_string()),
            2 => Some("Adobe RGB".to_string()),
            65535 => Some("Uncalibrated".to_string()),
            _ => None,
        },

        _ => None,
    }
}
