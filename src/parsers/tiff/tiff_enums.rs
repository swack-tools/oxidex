//! TIFF enumeration value mappings
//!
//! This module provides mappings from numeric TIFF tag values to their
//! human-readable string representations, matching Perl ExifTool output.

use crate::core::formatters::exif_enums::compression_label;

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

        // Compression (tag 0x0103): `%Image::ExifTool::Exif::compression`, held
        // once in `core::formatters::exif_enums`. The 40-id excerpt this file
        // used to carry stopped at `34892 => 'Lossy JPEG'`, so a Samsung NX3000
        // (32772), an Aperio slide (33003/33005) or any LibTiff 4.7 codec
        // (50000/50001/50002, 52546) fell through to `None` and printed as a
        // bare number, and 32766 printed `Next` where ExifTool prints
        // `NeXt or Sony ARW Compressed 2`.
        0x0103 => compression_label(value).map(str::to_string),

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

        // Thresholding (tag 0x0107)
        0x0107 => match value {
            1 => Some("No dithering or halftoning".to_string()),
            2 => Some("Ordered dither or halftone".to_string()),
            3 => Some("Randomized dither".to_string()),
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
            3 => Some("Floating point".to_string()),
            34892 => Some("Horizontal difference X2".to_string()),
            34893 => Some("Horizontal difference X4".to_string()),
            34894 => Some("Floating point X2".to_string()),
            34895 => Some("Floating point X4".to_string()),
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

        // LightSource (0x9208) and the DNG calibration illuminants
        // (0xC65A/0xC65B) share one table in ExifTool: Exif.pm:3639 declares
        // `PrintConv => \%lightSource` for CalibrationIlluminant1, the same
        // hash 0x9208 uses. Only 0x9208 was routed here, so the DNG pair
        // reported raw `17` and `21` where ExifTool prints `Standard Light A`
        // and `D65`.
        0x9208 | 0xC65A | 0xC65B => match value {
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

#[cfg(test)]
mod tests {
    use super::tiff_enum_to_string;

    /// ExifTool 13.59 `Exif.pm` maps TIFF Thresholding (0x0107) through a
    /// three-value PrintConv table. A missing arm makes the CLI expose the
    /// numeric code instead of the declared TIFF meaning.
    #[test]
    fn thresholding_matches_exiftool_13_59() {
        for (code, label) in [
            (1i64, "No dithering or halftoning"),
            (2, "Ordered dither or halftone"),
            (3, "Randomized dither"),
        ] {
            assert_eq!(
                tiff_enum_to_string(0x0107, code).as_deref(),
                Some(label),
                "Thresholding {code}"
            );
        }
        assert_eq!(tiff_enum_to_string(0x0107, 0), None);
        assert_eq!(tiff_enum_to_string(0x0107, 4), None);
    }

    /// ExifTool 13.59 `Exif.pm` defines all seven Predictor PrintConv values,
    /// including the DNG 1.5 variants. Keep their spelling exact: code 3 is
    /// `Floating point`, not the plausible but incorrect `... predictor`.
    #[test]
    fn predictor_matches_exiftool_13_59() {
        for (code, label) in [
            (1i64, "None"),
            (2, "Horizontal differencing"),
            (3, "Floating point"),
            (34892, "Horizontal difference X2"),
            (34893, "Horizontal difference X4"),
            (34894, "Floating point X2"),
            (34895, "Floating point X4"),
        ] {
            assert_eq!(
                tiff_enum_to_string(0x013D, code).as_deref(),
                Some(label),
                "Predictor {code}"
            );
        }
        assert_eq!(tiff_enum_to_string(0x013D, 34896), None);
    }

    /// Compression (0x0103) now resolves through the one
    /// `%Image::ExifTool::Exif::compression` table.
    ///
    /// The 40-id excerpt this file used to carry returned `None` for every code
    /// below, so the tag printed as a bare number, and named 32766 `Next`.
    /// This file had no test module at all, so nothing caught it.
    #[test]
    fn compression_codes_the_old_excerpt_dropped() {
        for (code, label) in [
            (32772i64, "Samsung SRW Compressed 2"),
            (33003, "Aperio JPEG 2000 YCbCr"),
            (33005, "Aperio JPEG 2000 RGB"),
            (34887, "ESRI Lerc"),
            (34925, "LZMA2"),
            (34926, "Zstd (old)"),
            (34927, "WebP (old)"),
            (34933, "PNG"),
            (34934, "JPEG XR"),
            (50000, "Zstd"),
            (50001, "WebP"),
            (50002, "JPEG XL (old)"),
            (52546, "JPEG XL"),
        ] {
            assert_eq!(
                tiff_enum_to_string(0x0103, code).as_deref(),
                Some(label),
                "Compression {code}"
            );
        }
    }

    #[test]
    fn compression_codes_the_old_excerpt_misspelled() {
        assert_eq!(
            tiff_enum_to_string(0x0103, 32766).as_deref(),
            Some("NeXt or Sony ARW Compressed 2")
        );
        assert_eq!(
            tiff_enum_to_string(0x0103, 9).as_deref(),
            Some("JBIG B&W or VC-5")
        );
    }

    /// Unknown codes still yield `None` -- consolidation must not start
    /// inventing labels ExifTool never prints.
    #[test]
    fn compression_unknown_codes_still_yield_none() {
        assert_eq!(tiff_enum_to_string(0x0103, 33004), None);
        assert_eq!(tiff_enum_to_string(0x0103, 0), None);
        assert_eq!(tiff_enum_to_string(0x0103, 1536), None);
        assert_eq!(tiff_enum_to_string(0x0103, 34316), None);
    }

    /// Codes the excerpt already had keep their exact former spelling.
    #[test]
    fn compression_codes_the_old_excerpt_had_are_unchanged() {
        for (code, label) in [
            (1i64, "Uncompressed"),
            (6, "JPEG (old-style)"),
            (7, "JPEG"),
            (32767, "Sony ARW Compressed"),
            (34713, "Nikon NEF Compressed"),
            (65535, "Pentax PEF Compressed"),
        ] {
            assert_eq!(
                tiff_enum_to_string(0x0103, code).as_deref(),
                Some(label),
                "Compression {code}"
            );
        }
    }
}
