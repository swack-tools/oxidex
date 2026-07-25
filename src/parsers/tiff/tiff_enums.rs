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
            34719 => {
                Some("Microsoft Document Imaging (MDI) Progressive Transform Codec".to_string())
            }
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

        // NewSubfileType (tag 0x00FE) - the standard SubfileType tag
        // Note: OldSubfileType is 0x00FF (deprecated, uses different bitmask values)
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

        // Predictor (tag 0x013D)
        0x013D => match value {
            1 => Some("None".to_string()),
            2 => Some("Horizontal differencing".to_string()),
            3 => Some("Floating point predictor".to_string()),
            _ => None,
        },

        // ColorSpace (EXIF tag 0xA001)
        0xA001 => match value {
            1 => Some("sRGB".to_string()),
            2 => Some("Adobe RGB".to_string()),
            65535 => Some("Uncalibrated".to_string()),
            _ => None,
        },

        // MeteringMode (EXIF tag 0x9207)
        // Defines the metering mode used to determine exposure
        0x9207 => match value {
            0 => Some("Unknown".to_string()),
            1 => Some("Average".to_string()),
            2 => Some("Center-weighted average".to_string()),
            3 => Some("Spot".to_string()),
            4 => Some("Multi-spot".to_string()),
            5 => Some("Multi-segment".to_string()),
            6 => Some("Partial".to_string()),
            255 => Some("Other".to_string()),
            _ => None,
        },

        // SensingMethod (EXIF tag 0xA217)
        // Indicates the image sensor type on the camera
        0xA217 => match value {
            1 => Some("Not defined".to_string()),
            2 => Some("One-chip color area".to_string()),
            3 => Some("Two-chip color area".to_string()),
            4 => Some("Three-chip color area".to_string()),
            5 => Some("Color sequential area".to_string()),
            7 => Some("Trilinear".to_string()),
            8 => Some("Color sequential linear".to_string()),
            _ => None,
        },

        // CustomRendered (EXIF tag 0xA401)
        // Indicates if special processing was applied to the image
        // Extended values (2+) are from Apple's HDR/Portrait processing
        0xA401 => match value {
            0 => Some("Normal".to_string()),
            1 => Some("Custom".to_string()),
            2 => Some("HDR (no original saved)".to_string()),
            3 => Some("HDR (original saved)".to_string()),
            4 => Some("Original (for HDR)".to_string()),
            6 => Some("Panorama".to_string()),
            7 => Some("Portrait HDR".to_string()),
            8 => Some("Portrait".to_string()),
            _ => None,
        },

        // ExposureMode (EXIF tag 0xA402)
        // Indicates the exposure mode set when the image was shot
        0xA402 => match value {
            0 => Some("Auto".to_string()),
            1 => Some("Manual".to_string()),
            2 => Some("Auto bracket".to_string()),
            _ => None,
        },

        // WhiteBalance (EXIF tag 0xA403)
        // Indicates the white balance mode set when the image was shot
        0xA403 => match value {
            0 => Some("Auto".to_string()),
            1 => Some("Manual".to_string()),
            _ => None,
        },

        // SceneCaptureType (EXIF tag 0xA406)
        // Indicates the type of scene that was shot
        0xA406 => match value {
            0 => Some("Standard".to_string()),
            1 => Some("Landscape".to_string()),
            2 => Some("Portrait".to_string()),
            3 => Some("Night".to_string()),
            4 => Some("Other".to_string()),
            _ => None,
        },

        // ExposureProgram (EXIF tag 0x8822)
        // The class of program used by the camera to set exposure
        0x8822 => match value {
            0 => Some("Not Defined".to_string()),
            1 => Some("Manual".to_string()),
            2 => Some("Program AE".to_string()),
            3 => Some("Aperture-priority AE".to_string()),
            4 => Some("Shutter speed priority AE".to_string()),
            5 => Some("Creative (Slow speed)".to_string()),
            6 => Some("Action (High speed)".to_string()),
            7 => Some("Portrait".to_string()),
            8 => Some("Landscape".to_string()),
            9 => Some("Bulb".to_string()),
            _ => None,
        },

        // LightSource (EXIF tag 0x9208)
        // The kind of light source
        0x9208 => match value {
            0 => Some("Unknown".to_string()),
            1 => Some("Daylight".to_string()),
            2 => Some("Fluorescent".to_string()),
            3 => Some("Tungsten (Incandescent)".to_string()),
            4 => Some("Flash".to_string()),
            9 => Some("Fine Weather".to_string()),
            10 => Some("Cloudy".to_string()),
            11 => Some("Shade".to_string()),
            12 => Some("Daylight Fluorescent".to_string()),
            13 => Some("Day White Fluorescent".to_string()),
            14 => Some("Cool White Fluorescent".to_string()),
            15 => Some("White Fluorescent".to_string()),
            16 => Some("Warm White Fluorescent".to_string()),
            17 => Some("Standard Light A".to_string()),
            18 => Some("Standard Light B".to_string()),
            19 => Some("Standard Light C".to_string()),
            20 => Some("D55".to_string()),
            21 => Some("D65".to_string()),
            22 => Some("D75".to_string()),
            23 => Some("D50".to_string()),
            24 => Some("ISO Studio Tungsten".to_string()),
            255 => Some("Other".to_string()),
            _ => None,
        },

        // GainControl (EXIF tag 0xA407)
        // The degree of overall image gain adjustment
        0xA407 => match value {
            0 => Some("None".to_string()),
            1 => Some("Low gain up".to_string()),
            2 => Some("High gain up".to_string()),
            3 => Some("Low gain down".to_string()),
            4 => Some("High gain down".to_string()),
            _ => None,
        },

        // Contrast (EXIF tag 0xA408)
        // The direction of contrast processing applied by the camera
        0xA408 => match value {
            0 => Some("Normal".to_string()),
            1 => Some("Low".to_string()),
            2 => Some("High".to_string()),
            _ => None,
        },

        // Saturation (EXIF tag 0xA409)
        // The direction of saturation processing applied by the camera
        0xA409 => match value {
            0 => Some("Normal".to_string()),
            1 => Some("Low".to_string()),
            2 => Some("High".to_string()),
            _ => None,
        },

        // Sharpness (EXIF tag 0xA40A)
        // The direction of sharpness processing applied by the camera
        0xA40A => match value {
            0 => Some("Normal".to_string()),
            1 => Some("Soft".to_string()),
            2 => Some("Hard".to_string()),
            _ => None,
        },

        // SubjectDistanceRange (EXIF tag 0xA40C)
        // The distance to the subject
        0xA40C => match value {
            0 => Some("Unknown".to_string()),
            1 => Some("Macro".to_string()),
            2 => Some("Close".to_string()),
            3 => Some("Distant".to_string()),
            _ => None,
        },

        // SceneType (EXIF tag 0xA301)
        // Indicates the type of scene. Value 1 is the only defined value.
        // Note: This tag is often stored as binary data and decoded by binary_decoders,
        // but can also appear as an integer value in some files.
        0xA301 => match value {
            1 => Some("Directly photographed".to_string()),
            _ => None,
        },

        // SensitivityType (EXIF tag 0x8830)
        // Indicates which sensitivity parameters are used for ISO speed
        0x8830 => match value {
            0 => Some("Unknown".to_string()),
            1 => Some("Standard Output Sensitivity".to_string()),
            2 => Some("Recommended Exposure Index".to_string()),
            3 => Some("ISO Speed".to_string()),
            4 => Some("Standard Output Sensitivity and Recommended Exposure Index".to_string()),
            5 => Some("Standard Output Sensitivity and ISO Speed".to_string()),
            6 => Some("Recommended Exposure Index and ISO Speed".to_string()),
            7 => Some(
                "Standard Output Sensitivity, Recommended Exposure Index and ISO Speed".to_string(),
            ),
            _ => None,
        },

        // CompositeImage (EXIF tag 0xA460)
        // Indicates if the image is a composite image
        0xA460 => match value {
            0 => Some("Unknown".to_string()),
            1 => Some("Not a Composite Image".to_string()),
            2 => Some("General Composite Image".to_string()),
            3 => Some("Composite Image Captured While Shooting".to_string()),
            _ => None,
        },

        // MakerNoteSafety (DNG tag 0xC635)
        // Indicates whether it is safe to preserve MakerNote data
        0xC635 => match value {
            0 => Some("Unsafe".to_string()),
            1 => Some("Safe".to_string()),
            _ => None,
        },

        _ => None,
    }
}
