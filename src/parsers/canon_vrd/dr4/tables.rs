//! `%Image::ExifTool::CanonVRD::DR4` and its five binary sub-tables,
//! transcribed from ExifTool 13.30 `lib/Image/ExifTool/CanonVRD.pm`.
//!
//! Every entry here is a line of that file: the main table is
//! CanonVRD.pm:1001-1295, `DR4Header` 1296-1319, `ToneCurve` 1321-1370,
//! `GammaInfo` 1372-1427, `CropInfo` 1429-1455, `StampInfo` 1457-1464 and
//! `DustInfo` 1466-1473.
//!
//! Tags ExifTool marks `Binary => 1` (`CustomPictureStyleData`, and the
//! `DLOData` of the version-1 record) are deliberately absent: without `-b`
//! ExifTool prints a placeholder rather than a value, and inventing one here
//! would be worse than omitting the tag.

use super::{Conv, Format};

/// One entry of `%CanonVRD::DR4`.
pub(super) struct DR4Entry {
    /// The int32u tag id stored in the directory entry.
    pub tag: u32,
    pub name: &'static str,
    pub conv: Conv,
    /// `0x<tag>.<i>` entries, which name the three flag words the directory
    /// carries alongside the value. Almost every tag names at most one.
    pub flags: [Option<(&'static str, Conv)>; 3],
}

/// One entry of a `ProcessBinaryData` sub-table.
pub(super) struct SubEntry {
    /// The table's key: an index in units of the table's default format size.
    pub index: usize,
    pub name: &'static str,
    /// `Format => ...` where the entry overrides the table default.
    pub format: Option<Format>,
    /// The `[n]` of a Perl format like `int32u[21]`.
    pub count: usize,
    pub conv: Conv,
}

/// A `ProcessBinaryData` sub-table: its default `FORMAT` and its entries.
#[derive(Clone, Copy)]
pub(super) struct Sub {
    pub format: Format,
    pub entries: &'static [SubEntry],
}

/// Shorthand for the common case: table default format, one element, no conv.
const fn plain(index: usize, name: &'static str) -> SubEntry {
    SubEntry {
        index,
        name,
        format: None,
        count: 1,
        conv: Conv::None,
    }
}

const fn main_tag(tag: u32, name: &'static str, conv: Conv) -> DR4Entry {
    DR4Entry {
        tag,
        name,
        conv,
        flags: [None, None, None],
    }
}

/// `%CanonVRD::ToneCurve` (CanonVRD.pm:1321). `FORMAT => 'int32u'`.
static TONE_CURVE: Sub = Sub {
    format: Format::Int32u,
    entries: &[
        SubEntry {
            index: 0x00,
            name: "ToneCurveColorSpace",
            format: None,
            count: 1,
            conv: Conv::Map(&[(0, "RGB"), (1, "Luminance")]),
        },
        SubEntry {
            index: 0x01,
            name: "ToneCurveShape",
            format: None,
            count: 1,
            conv: Conv::Map(&[(0, "Curve"), (1, "Straight")]),
        },
        SubEntry {
            index: 0x03,
            name: "ToneCurveInputRange",
            format: None,
            count: 2,
            conv: Conv::None,
        },
        SubEntry {
            index: 0x05,
            name: "ToneCurveOutputRange",
            format: None,
            count: 2,
            conv: Conv::None,
        },
        SubEntry {
            index: 0x07,
            name: "RGBCurvePoints",
            format: None,
            count: 21,
            conv: Conv::ToneCurve,
        },
        plain(0x0a, "ToneCurveX"),
        plain(0x0b, "ToneCurveY"),
        SubEntry {
            index: 0x2d,
            name: "RedCurvePoints",
            format: None,
            count: 21,
            conv: Conv::ToneCurve,
        },
        SubEntry {
            index: 0x53,
            name: "GreenCurvePoints",
            format: None,
            count: 21,
            conv: Conv::ToneCurve,
        },
        SubEntry {
            index: 0x79,
            name: "BlueCurvePoints",
            format: None,
            count: 21,
            conv: Conv::ToneCurve,
        },
    ],
};

/// `%CanonVRD::GammaInfo` (CanonVRD.pm:1372). `FORMAT => 'double'`.
static GAMMA_INFO: Sub = Sub {
    format: Format::Double,
    entries: &[
        plain(0x02, "GammaContrast"),
        plain(0x03, "GammaColorTone"),
        plain(0x04, "GammaSaturation"),
        plain(0x05, "GammaUnsharpMaskStrength"),
        plain(0x06, "GammaUnsharpMaskFineness"),
        plain(0x07, "GammaUnsharpMaskThreshold"),
        plain(0x08, "GammaSharpnessStrength"),
        plain(0x09, "GammaShadow"),
        plain(0x0a, "GammaHighlight"),
        SubEntry {
            index: 0x0c,
            name: "GammaBlackPoint",
            format: None,
            count: 1,
            conv: Conv::GammaBlackPoint,
        },
        SubEntry {
            index: 0x0d,
            name: "GammaWhitePoint",
            format: None,
            count: 1,
            conv: Conv::GammaWhitePoint,
        },
        SubEntry {
            index: 0x0e,
            name: "GammaMidPoint",
            format: None,
            count: 1,
            conv: Conv::GammaMidPoint,
        },
        SubEntry {
            index: 0x0f,
            name: "GammaCurveOutputRange",
            format: None,
            count: 2,
            conv: Conv::None,
        },
    ],
};

/// `%CanonVRD::CropInfo` (CanonVRD.pm:1429). `FORMAT => 'int32s'`, so the
/// double at index 8 still sits at byte 8*4.
static CROP_INFO: Sub = Sub {
    format: Format::Int32s,
    entries: &[
        SubEntry {
            index: 0,
            name: "CropActive",
            format: None,
            count: 1,
            conv: Conv::NoYes,
        },
        plain(1, "CropRotatedOriginalWidth"),
        plain(2, "CropRotatedOriginalHeight"),
        plain(3, "CropX"),
        plain(4, "CropY"),
        plain(5, "CropWidth"),
        plain(6, "CropHeight"),
        plain(7, "CropRotation"),
        SubEntry {
            index: 8,
            name: "CropAngle",
            format: Some(Format::Double),
            count: 1,
            conv: Conv::Sprintf7g,
        },
        plain(10, "CropOriginalWidth"),
        plain(11, "CropOriginalHeight"),
    ],
};

/// `%CanonVRD::StampInfo` (CanonVRD.pm:1457). `FORMAT => 'int32u'`.
static STAMP_INFO: Sub = Sub {
    format: Format::Int32u,
    entries: &[plain(0x02, "StampToolCount")],
};

/// `%CanonVRD::DustInfo` (CanonVRD.pm:1466). `FORMAT => 'int32u'`.
static DUST_INFO: Sub = Sub {
    format: Format::Int32u,
    entries: &[SubEntry {
        index: 0x02,
        name: "DustDeleteApplied",
        format: None,
        count: 1,
        conv: Conv::NoYes,
    }],
};

/// `%Image::ExifTool::CanonVRD::DR4` (CanonVRD.pm:1001).
pub(super) static DR4_MAIN: &[DR4Entry] = &[
    main_tag(0x10002, "Rotation", Conv::None),
    main_tag(0x10003, "AngleAdj", Conv::None),
    main_tag(0x10021, "CustomPictureStyle", Conv::None),
    main_tag(
        0x10100,
        "Rating",
        Conv::Map(&[
            (0, "Unrated"),
            (1, "1"),
            (2, "2"),
            (3, "3"),
            (4, "4"),
            (5, "5"),
            (4_294_967_295, "Rejected"),
        ]),
    ),
    main_tag(
        0x10101,
        "CheckMark",
        Conv::Map(&[
            (0, "Clear"),
            (1, "1"),
            (2, "2"),
            (3, "3"),
            (4, "4"),
            (5, "5"),
        ]),
    ),
    main_tag(
        0x10200,
        "WorkColorSpace",
        Conv::Map(&[
            (1, "sRGB"),
            (2, "Adobe RGB"),
            (3, "Wide Gamut RGB"),
            (4, "Apple RGB"),
            (5, "ColorMatch RGB"),
        ]),
    ),
    main_tag(0x20001, "RawBrightnessAdj", Conv::None),
    main_tag(
        0x20101,
        "WhiteBalanceAdj",
        Conv::Map(&[
            (-1, "Manual (Click)"),
            (0, "Auto"),
            (1, "Daylight"),
            (2, "Cloudy"),
            (3, "Tungsten"),
            (4, "Fluorescent"),
            (5, "Flash"),
            (8, "Shade"),
            (9, "Kelvin"),
            (255, "Shot Settings"),
        ]),
    ),
    main_tag(0x20102, "WBAdjColorTemp", Conv::None),
    main_tag(0x20105, "WBAdjMagentaGreen", Conv::None),
    main_tag(0x20106, "WBAdjBlueAmber", Conv::None),
    main_tag(0x20125, "WBAdjRGGBLevels", Conv::StripFirstInt),
    main_tag(0x20200, "GammaLinear", Conv::NoYes),
    main_tag(
        0x20301,
        "PictureStyle",
        Conv::MapHex(&[
            (0x81, "Standard"),
            (0x82, "Portrait"),
            (0x83, "Landscape"),
            (0x84, "Neutral"),
            (0x85, "Faithful"),
            (0x86, "Monochrome"),
            (0x87, "Auto"),
            (0x88, "Fine Detail"),
            (0xf0, "Shot Settings"),
            (0xff, "Custom"),
        ]),
    ),
    main_tag(0x20303, "ContrastAdj", Conv::None),
    main_tag(0x20304, "ColorToneAdj", Conv::None),
    main_tag(0x20305, "ColorSaturationAdj", Conv::None),
    main_tag(
        0x20306,
        "MonochromeToningEffect",
        Conv::Map(&[
            (0, "None"),
            (1, "Sepia"),
            (2, "Blue"),
            (3, "Purple"),
            (4, "Green"),
        ]),
    ),
    main_tag(
        0x20307,
        "MonochromeFilterEffect",
        Conv::Map(&[
            (0, "None"),
            (1, "Yellow"),
            (2, "Orange"),
            (3, "Red"),
            (4, "Green"),
        ]),
    ),
    main_tag(0x20308, "UnsharpMaskStrength", Conv::None),
    main_tag(0x20309, "UnsharpMaskFineness", Conv::None),
    main_tag(0x2030a, "UnsharpMaskThreshold", Conv::None),
    main_tag(0x2030b, "ShadowAdj", Conv::None),
    main_tag(0x2030c, "HighlightAdj", Conv::None),
    DR4Entry {
        tag: 0x20310,
        name: "SharpnessAdj",
        conv: Conv::Map(&[(0, "Sharpness"), (1, "Unsharp Mask")]),
        flags: [Some(("SharpnessAdjOn", Conv::NoYes)), None, None],
    },
    main_tag(0x20311, "SharpnessStrength", Conv::None),
    DR4Entry {
        tag: 0x20400,
        name: "ToneCurve",
        conv: Conv::SubDir(TONE_CURVE),
        flags: [None, Some(("ToneCurveOriginal", Conv::NoYes)), None],
    },
    main_tag(0x20410, "ToneCurveBrightness", Conv::None),
    main_tag(0x20411, "ToneCurveContrast", Conv::None),
    DR4Entry {
        tag: 0x20500,
        name: "AutoLightingOptimizer",
        conv: Conv::Map(&[(0, "Low"), (1, "Standard"), (2, "Strong")]),
        flags: [Some(("AutoLightingOptimizerOn", Conv::NoYes)), None, None],
    },
    main_tag(0x20600, "LuminanceNoiseReduction", Conv::None),
    main_tag(0x20601, "ChrominanceNoiseReduction", Conv::None),
    DR4Entry {
        tag: 0x20670,
        name: "ColorMoireReduction",
        conv: Conv::None,
        flags: [Some(("ColorMoireReductionOn", Conv::NoYes)), None, None],
    },
    main_tag(0x20701, "ShootingDistance", Conv::ShootingDistance),
    DR4Entry {
        tag: 0x20702,
        name: "PeripheralIllumination",
        conv: Conv::PercentG,
        flags: [Some(("PeripheralIlluminationOn", Conv::NoYes)), None, None],
    },
    DR4Entry {
        tag: 0x20703,
        name: "ChromaticAberration",
        conv: Conv::PercentG,
        flags: [Some(("ChromaticAberrationOn", Conv::NoYes)), None, None],
    },
    main_tag(0x20704, "ColorBlurOn", Conv::NoYes),
    DR4Entry {
        tag: 0x20705,
        name: "DistortionCorrection",
        conv: Conv::PercentG,
        flags: [Some(("DistortionCorrectionOn", Conv::NoYes)), None, None],
    },
    DR4Entry {
        tag: 0x20706,
        name: "DLOSetting",
        conv: Conv::None,
        flags: [Some(("DLOOn", Conv::NoYes)), None, None],
    },
    main_tag(0x20707, "ChromaticAberrationRed", Conv::PercentG),
    main_tag(0x20708, "ChromaticAberrationBlue", Conv::PercentG),
    main_tag(
        0x20709,
        "DistortionEffect",
        Conv::Map(&[
            (0, "Shot Settings"),
            (1, "Emphasize Linearity"),
            (2, "Emphasize Distance"),
            (3, "Emphasize Periphery"),
            (4, "Emphasize Center"),
        ]),
    ),
    main_tag(0x2070b, "DiffractionCorrectionOn", Conv::NoYes),
    main_tag(0x20900, "ColorHue", Conv::None),
    main_tag(0x20901, "SaturationAdj", Conv::None),
    main_tag(0x20910, "RedHSL", Conv::None),
    main_tag(0x20911, "OrangeHSL", Conv::None),
    main_tag(0x20912, "YellowHSL", Conv::None),
    main_tag(0x20913, "GreenHSL", Conv::None),
    main_tag(0x20914, "AquaHSL", Conv::None),
    main_tag(0x20915, "BlueHSL", Conv::None),
    main_tag(0x20916, "PurpleHSL", Conv::None),
    main_tag(0x20917, "MagentaHSL", Conv::None),
    main_tag(0x20a00, "GammaInfo", Conv::SubDir(GAMMA_INFO)),
    main_tag(0x20b10, "DPRAWMicroadjustBackFront", Conv::None),
    main_tag(0x20b12, "DPRAWMicroadjustStrength", Conv::None),
    main_tag(0x20b20, "DPRAWBokehShift", Conv::None),
    main_tag(0x20b21, "DPRAWBokehShiftArea", Conv::None),
    main_tag(0x20b30, "DPRAWGhostingReductionArea", Conv::None),
    main_tag(
        0x30101,
        "CropAspectRatio",
        Conv::Map(&[
            (0, "Free"),
            (1, "Custom"),
            (2, "1:1"),
            (3, "3:2"),
            (4, "2:3"),
            (5, "4:3"),
            (6, "3:4"),
            (7, "5:4"),
            (8, "4:5"),
            (9, "16:9"),
            (10, "9:16"),
        ]),
    ),
    main_tag(0x30102, "CropAspectRatioCustom", Conv::None),
    main_tag(0xf0100, "CropInfo", Conv::SubDir(CROP_INFO)),
    main_tag(0xf0510, "StampInfo", Conv::SubDir(STAMP_INFO)),
    main_tag(0xf0511, "DustInfo", Conv::SubDir(DUST_INFO)),
    main_tag(0xf0512, "LensFocalLength", Conv::None),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_ids_are_unique() {
        let mut ids: Vec<u32> = DR4_MAIN.iter().map(|e| e.tag).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate DR4 tag id");
    }

    #[test]
    fn subdirectory_entries_are_in_index_order() {
        // ProcessBinaryData walks a table in key order; keeping the tables in
        // that order is what makes a missing or transposed index visible.
        for sub in [
            &TONE_CURVE,
            &GAMMA_INFO,
            &CROP_INFO,
            &STAMP_INFO,
            &DUST_INFO,
        ] {
            assert!(
                sub.entries.windows(2).all(|w| w[0].index < w[1].index),
                "sub-table indices out of order"
            );
        }
    }
}
