//! Canon MakerNote binary sub-tables.
//!
//! Most `%Canon` sub-tables are plain BinaryData: a fixed array of integers whose element
//! index is the ExifTool key. This module holds the ones that hang off a MakerNote tag
//! with no ExifTool `Condition` on the tag itself, so a single table always applies and no
//! per-body guess is involved.
//!
//! The tables are transcribed from `Canon.pm` by script, not by hand. A field is emitted
//! only when it can be reproduced exactly: an integer format with an optional array count,
//! and a `PrintConv` that is a literal hash, one of ExifTool's shared hashes, a `BITMASK`,
//! or absent. Fields with a `ValueConv`, a per-model `Condition`, a nested `SubDirectory`
//! or a non-integer format are left out rather than approximated, and fields ExifTool
//! flags `Unknown` are omitted because ExifTool itself hides them without `-U`.

use crate::core::formatters::exiftool_rational_number;
use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

/// How a field's value is laid out in the record's `int16` array.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CanonBinaryFormat {
    /// A `Format => 'int8u'` override inside a table whose own `FORMAT` is 16-bit: the
    /// key still steps two bytes, but only the first of them is the value.
    Int8u,
    Int16s,
    Int16u,
    Int32u,
    Int32s,
}

impl CanonBinaryFormat {
    /// Number of `i16` words occupied by one value in the table's declared format.
    const fn words(self) -> usize {
        match self {
            Self::Int8u | Self::Int16s | Self::Int16u => 1,
            Self::Int32u | Self::Int32s => 2,
        }
    }
}

/// The `PrintConv` ExifTool applies to a field.
#[derive(Clone, Copy)]
enum CanonBinaryConv {
    /// No `PrintConv`: the raw number(s).
    Raw,
    /// A lookup hash. An unlisted value prints as ExifTool's `Unknown (n)`.
    Map(&'static [(i64, &'static str)]),
    /// A `BITMASK` hash: set bit names (or `[n]` for unknown bits) joined with ", ".
    Bitmask(&'static [(i64, &'static str)]),
    /// ExifTool's shared `%printParameter`: 0 prints "Normal", positives carry a "+".
    PrintParameter,
    /// `%Canon::ContrastInfo` key 4 (Canon.pm:6612). A three-entry hash plus an `OTHER`
    /// sub that keeps the raw value visible:
    /// `return sprintf("On (0x%.2x)",$val) if $val & 0x08; return sprintf("Off (0x%.2x)",$val);`
    IntelligentContrast,
}

/// One field of a `%Canon` binary sub-table.
///
/// `index` is ExifTool's own key, i.e. an index into the record's element array in the
/// table's declared `FORMAT` units.
struct CanonBinaryField {
    index: usize,
    name: &'static str,
    format: CanonBinaryFormat,
    count: usize,
    conv: CanonBinaryConv,
}

const TABLE_MY_COLORS_CONV1: &[(i64, &str)] = &[
    (0, "Off"),
    (1, "Positive Film"),
    (2, "Light Skin Tone"),
    (3, "Dark Skin Tone"),
    (4, "Vivid Blue"),
    (5, "Vivid Green"),
    (6, "Vivid Red"),
    (7, "Color Accent"),
    (8, "Color Swap"),
    (9, "Custom"),
    (12, "Vivid"),
    (13, "Neutral"),
    (14, "Sepia"),
    (15, "B&W"),
];
const TABLE_TIME_INFO_CONV2: &[(i64, &str)] = &[
    (0, "n/a"),
    (1, "Chatham Islands"),
    (2, "Wellington"),
    (3, "Solomon Islands"),
    (4, "Sydney"),
    (5, "Adelaide"),
    (6, "Tokyo"),
    (7, "Hong Kong"),
    (8, "Bangkok"),
    (9, "Yangon"),
    (10, "Dhaka"),
    (11, "Kathmandu"),
    (12, "Delhi"),
    (13, "Karachi"),
    (14, "Kabul"),
    (15, "Dubai"),
    (16, "Tehran"),
    (17, "Moscow"),
    (18, "Cairo"),
    (19, "Paris"),
    (20, "London"),
    (21, "Azores"),
    (22, "Fernando de Noronha"),
    (23, "Sao Paulo"),
    (24, "Newfoundland"),
    (25, "Santiago"),
    (26, "Caracas"),
    (27, "New York"),
    (28, "Chicago"),
    (29, "Denver"),
    (30, "Los Angeles"),
    (31, "Anchorage"),
    (32, "Honolulu"),
    (33, "Samoa"),
    (32766, "(not set)"),
];
const TABLE_TIME_INFO_CONV3: &[(i64, &str)] = &[(0, "Off"), (60, "On")];
const TABLE_ASPECT_INFO_CONV4: &[(i64, &str)] = &[
    (0, "3:2"),
    (1, "1:1"),
    (2, "4:3"),
    (7, "16:9"),
    (8, "4:5"),
    (12, "3:2 (APS-H crop)"),
    (13, "3:2 (APS-C crop)"),
    (258, "4:3 crop"),
];
const TABLE_MODIFIED_INFO_CONV5: &[(i64, &str)] = &[(0, "Standard"), (1, "Manual"), (2, "Custom")];
const TABLE_MODIFIED_INFO_CONV6: &[(i64, &str)] = &[
    (0, "n/a"),
    (1, "Lowest"),
    (2, "Low"),
    (3, "Standard"),
    (4, "High"),
    (5, "Highest"),
];
const TABLE_MODIFIED_INFO_CONV7: &[(i64, &str)] = &[
    (0, "Auto"),
    (1, "Daylight"),
    (2, "Cloudy"),
    (3, "Tungsten"),
    (4, "Fluorescent"),
    (5, "Flash"),
    (6, "Custom"),
    (7, "Black & White"),
    (8, "Shade"),
    (9, "Manual Temperature (Kelvin)"),
    (10, "PC Set1"),
    (11, "PC Set2"),
    (12, "PC Set3"),
    (14, "Daylight Fluorescent"),
    (15, "Custom 1"),
    (16, "Custom 2"),
    (17, "Underwater"),
    (18, "Custom 3"),
    (19, "Custom 4"),
    (20, "PC Set4"),
    (21, "PC Set5"),
    (23, "Auto (ambience priority)"),
];
const TABLE_MODIFIED_INFO_CONV8: &[(i64, &str)] = &[
    (0, "None"),
    (1, "Standard"),
    (2, "Portrait"),
    (3, "High Saturation"),
    (4, "Adobe RGB"),
    (5, "Low Saturation"),
    (6, "CM Set 1"),
    (7, "CM Set 2"),
    (33, "User Def. 1"),
    (34, "User Def. 2"),
    (35, "User Def. 3"),
    (65, "PC 1"),
    (66, "PC 2"),
    (67, "PC 3"),
    (129, "Standard"),
    (130, "Portrait"),
    (131, "Landscape"),
    (132, "Neutral"),
    (133, "Faithful"),
    (134, "Monochrome"),
    (135, "Auto"),
    (136, "Fine Detail"),
    (255, "n/a"),
    (65535, "n/a"),
];
const TABLE_PREVIEW_IMAGE_INFO_CONV9: &[(i64, &str)] = &[
    (-1, "n/a"),
    (1, "Economy"),
    (2, "Normal"),
    (3, "Fine"),
    (4, "RAW"),
    (5, "Superfine"),
    (7, "CRAW"),
    (130, "Light (RAW)"),
    (131, "Standard (RAW)"),
];
const TABLE_AF_MICRO_ADJ_CONV10: &[(i64, &str)] = &[
    (0, "Disable"),
    (1, "Adjust all by the same amount"),
    (2, "Adjust by lens"),
];
const TABLE_VIGNETTING_CORR2_CONV11: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
const TABLE_VIGNETTING_CORR2_CONV12: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
const TABLE_VIGNETTING_CORR2_CONV13: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
const TABLE_VIGNETTING_CORR2_CONV14: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
const TABLE_LIGHTING_OPT_CONV15: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
const TABLE_LIGHTING_OPT_CONV16: &[(i64, &str)] =
    &[(0, "Standard"), (1, "Low"), (2, "Strong"), (3, "Off")];
const TABLE_LIGHTING_OPT_CONV17: &[(i64, &str)] = &[(0, "Off"), (1, "Auto"), (2, "On")];
const TABLE_LIGHTING_OPT_CONV18: &[(i64, &str)] =
    &[(0, "Standard"), (1, "Low"), (2, "Strong"), (3, "Off")];
const TABLE_LIGHTING_OPT_CONV19: &[(i64, &str)] = &[(0, "Off"), (1, "Standard"), (2, "High")];
const TABLE_LIGHTING_OPT_CONV20: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
const TABLE_MULTI_EXP_CONV21: &[(i64, &str)] = &[(0, "Off"), (1, "On"), (2, "On (RAW)")];
const TABLE_MULTI_EXP_CONV22: &[(i64, &str)] = &[
    (0, "Additive"),
    (1, "Average"),
    (2, "Bright (comparative)"),
    (3, "Dark (comparative)"),
];
const TABLE_HDR_INFO_CONV23: &[(i64, &str)] = &[(0, "Off"), (1, "Auto"), (2, "On")];
const TABLE_HDR_INFO_CONV24: &[(i64, &str)] = &[
    (0, "Natural"),
    (1, "Art (standard)"),
    (2, "Art (vivid)"),
    (3, "Art (bold)"),
    (4, "Art (embossed)"),
];
const TABLE_AF_CONFIG_CONV25: &[(i64, &str)] = &[
    (0, "Equal Priority"),
    (1, "Release Priority"),
    (2, "Focus Priority"),
];
const TABLE_AF_CONFIG_CONV26: &[(i64, &str)] = &[
    (0, "Equal Priority"),
    (1, "Release Priority"),
    (2, "Focus Priority"),
    (3, "Release High Priority"),
    (4, "Focus High Priority"),
];
const TABLE_AF_CONFIG_CONV27: &[(i64, &str)] = &[
    (0, "Enable"),
    (1, "Disable"),
    (2, "IR AF Assist Beam Only"),
    (3, "LED AF Assist Beam Only"),
];
const TABLE_AF_CONFIG_CONV28: &[(i64, &str)] = &[(0, "Focus Priority"), (1, "Release Priority")];
const TABLE_AF_CONFIG_CONV29: &[(i64, &str)] =
    &[(0, "Continue Focus Search"), (1, "Stop Focus Search")];
const TABLE_AF_CONFIG_CONV30: &[(i64, &str)] = &[
    (0, "Single-point AF"),
    (1, "Auto"),
    (2, "Zone AF"),
    (3, "AF Point Expansion (4 point)"),
    (4, "Spot AF"),
    (5, "AF Point Expansion (8 point)"),
];
const TABLE_AF_CONFIG_CONV31: &[(i64, &str)] = &[(0, "M-Fn Button"), (1, "Main Dial")];
const TABLE_AF_CONFIG_CONV32: &[(i64, &str)] = &[
    (0, "Same for Vert/Horiz Points"),
    (1, "Separate Vert/Horiz Points"),
    (2, "Separate Area+Points"),
];
const TABLE_AF_CONFIG_CONV33: &[(i64, &str)] = &[(0, "Stops at AF Area Edges"), (1, "Continuous")];
const TABLE_AF_CONFIG_CONV34: &[(i64, &str)] = &[
    (0, "Selected (constant)"),
    (1, "All (constant)"),
    (2, "Selected (pre-AF, focused)"),
    (3, "Selected (focused)"),
    (4, "Disabled"),
];
const TABLE_AF_CONFIG_CONV35: &[(i64, &str)] = &[(0, "Auto"), (1, "Enable"), (2, "Disable")];
const TABLE_AF_CONFIG_CONV36: &[(i64, &str)] = &[
    (0, "None"),
    (1, "People"),
    (2, "Animals"),
    (3, "Vehicles"),
    (4, "Auto"),
];
const TABLE_AF_CONFIG_CONV37: &[(i64, &str)] = &[
    (0, "Initial Priority"),
    (1, "On Subject"),
    (2, "Switch Subject"),
    (2147483647, "n/a"),
];
const TABLE_AF_CONFIG_CONV38: &[(i64, &str)] =
    &[(0, "Off"), (1, "Auto"), (2, "Left Eye"), (3, "Right Eye")];
const TABLE_AF_CONFIG_CONV39: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
const TABLE_AF_CONFIG_CONV40: &[(i64, &str)] = &[(0, "Case Auto"), (1, "Case Manual")];
const TABLE_AF_CONFIG_CONV41: &[(i64, &str)] = &[
    (-1, "Locked On"),
    (0, "Standard"),
    (1, "Responsive"),
    (2147483647, "n/a"),
];
const TABLE_AF_CONFIG_CONV42: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
const TABLE_AF_CONFIG_CONV43: &[(i64, &str)] =
    &[(0, "Soccer"), (1, "Basketball"), (2, "Volleyball")];
const TABLE_FOCUS_BRACKETING_INFO_CONV44: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
const TABLE_FOCUS_BRACKETING_INFO_CONV45: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
const TABLE_FOCUS_BRACKETING_INFO_CONV46: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
const TABLE_FOCUS_BRACKETING_INFO_CONV47: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
// Canon.pm:9096 -- `PrintConv => { %offOn, 2 => 'Enhanced' }` (github339).
const TABLE_LIGHTING_OPT_CONV48: &[(i64, &str)] = &[(0, "Off"), (1, "On"), (2, "Enhanced")];

/// `%Canon::MyColors` (MakerNote tag 0x001d), transcribed from ExifTool.
const TABLE_MY_COLORS: &[CanonBinaryField] = &[CanonBinaryField {
    index: 2,
    name: "MyColorMode",
    format: CanonBinaryFormat::Int16u,
    count: 1,
    conv: CanonBinaryConv::Map(TABLE_MY_COLORS_CONV1),
}];

/// `%Canon::WBInfo` (MakerNote tag 0x0029), transcribed from ExifTool.
const TABLE_WB_INFO: &[CanonBinaryField] = &[
    CanonBinaryField {
        index: 2,
        name: "WB_GRBGLevelsAuto",
        format: CanonBinaryFormat::Int32s,
        count: 4,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 10,
        name: "WB_GRBGLevelsDaylight",
        format: CanonBinaryFormat::Int32s,
        count: 4,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 18,
        name: "WB_GRBGLevelsCloudy",
        format: CanonBinaryFormat::Int32s,
        count: 4,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 26,
        name: "WB_GRBGLevelsTungsten",
        format: CanonBinaryFormat::Int32s,
        count: 4,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 34,
        name: "WB_GRBGLevelsFluorescent",
        format: CanonBinaryFormat::Int32s,
        count: 4,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 42,
        name: "WB_GRBGLevelsFluorHigh",
        format: CanonBinaryFormat::Int32s,
        count: 4,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 50,
        name: "WB_GRBGLevelsFlash",
        format: CanonBinaryFormat::Int32s,
        count: 4,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 58,
        name: "WB_GRBGLevelsUnderwater",
        format: CanonBinaryFormat::Int32s,
        count: 4,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 66,
        name: "WB_GRBGLevelsCustom1",
        format: CanonBinaryFormat::Int32s,
        count: 4,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 74,
        name: "WB_GRBGLevelsCustom2",
        format: CanonBinaryFormat::Int32s,
        count: 4,
        conv: CanonBinaryConv::Raw,
    },
];

/// `%Canon::TimeInfo` (MakerNote tag 0x0035), transcribed from ExifTool.
const TABLE_TIME_INFO: &[CanonBinaryField] = &[
    CanonBinaryField {
        index: 2,
        name: "TimeZoneCity",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_TIME_INFO_CONV2),
    },
    CanonBinaryField {
        index: 3,
        name: "DaylightSavings",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_TIME_INFO_CONV3),
    },
];

/// `%Canon::CropInfo` (MakerNote tag 0x0098), transcribed from ExifTool.
const TABLE_CROP_INFO: &[CanonBinaryField] = &[
    CanonBinaryField {
        index: 0,
        name: "CropLeftMargin",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 1,
        name: "CropRightMargin",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 2,
        name: "CropTopMargin",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 3,
        name: "CropBottomMargin",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
];

/// `%Canon::AspectInfo` (MakerNote tag 0x009a), transcribed from ExifTool.
const TABLE_ASPECT_INFO: &[CanonBinaryField] = &[
    CanonBinaryField {
        index: 0,
        name: "AspectRatio",
        format: CanonBinaryFormat::Int32u,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_ASPECT_INFO_CONV4),
    },
    CanonBinaryField {
        index: 1,
        name: "CroppedImageWidth",
        format: CanonBinaryFormat::Int32u,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 2,
        name: "CroppedImageHeight",
        format: CanonBinaryFormat::Int32u,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 3,
        name: "CroppedImageLeft",
        format: CanonBinaryFormat::Int32u,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 4,
        name: "CroppedImageTop",
        format: CanonBinaryFormat::Int32u,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
];

/// `%Canon::Flags` (MakerNote tag 0x00b0), transcribed from ExifTool.
const TABLE_FLAGS: &[CanonBinaryField] = &[CanonBinaryField {
    index: 1,
    name: "ModifiedParamFlag",
    format: CanonBinaryFormat::Int16s,
    count: 1,
    conv: CanonBinaryConv::Raw,
}];

/// `%Canon::ModifiedInfo` (MakerNote tag 0x00b1), transcribed from ExifTool.
const TABLE_MODIFIED_INFO: &[CanonBinaryField] = &[
    CanonBinaryField {
        index: 1,
        name: "ModifiedToneCurve",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_MODIFIED_INFO_CONV5),
    },
    CanonBinaryField {
        index: 3,
        name: "ModifiedSharpnessFreq",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_MODIFIED_INFO_CONV6),
    },
    CanonBinaryField {
        index: 4,
        name: "ModifiedSensorRedLevel",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 5,
        name: "ModifiedSensorBlueLevel",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 6,
        name: "ModifiedWhiteBalanceRed",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 7,
        name: "ModifiedWhiteBalanceBlue",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 8,
        name: "ModifiedWhiteBalance",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_MODIFIED_INFO_CONV7),
    },
    CanonBinaryField {
        index: 9,
        name: "ModifiedColorTemp",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 10,
        name: "ModifiedPictureStyle",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_MODIFIED_INFO_CONV8),
    },
];

/// `%Canon::PreviewImageInfo` (MakerNote tag 0x00b6), transcribed from ExifTool.
const TABLE_PREVIEW_IMAGE_INFO: &[CanonBinaryField] = &[
    CanonBinaryField {
        index: 1,
        name: "PreviewQuality",
        format: CanonBinaryFormat::Int32u,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_PREVIEW_IMAGE_INFO_CONV9),
    },
    CanonBinaryField {
        index: 2,
        name: "PreviewImageLength",
        format: CanonBinaryFormat::Int32u,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 3,
        name: "PreviewImageWidth",
        format: CanonBinaryFormat::Int32u,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 4,
        name: "PreviewImageHeight",
        format: CanonBinaryFormat::Int32u,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 5,
        name: "PreviewImageStart",
        format: CanonBinaryFormat::Int32u,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
];

/// `%Canon::ColorInfo` (MakerNote tag 0x4003), transcribed from ExifTool.
const TABLE_COLOR_INFO: &[CanonBinaryField] = &[CanonBinaryField {
    index: 2,
    name: "ColorTone",
    format: CanonBinaryFormat::Int16s,
    count: 1,
    conv: CanonBinaryConv::PrintParameter,
}];

/// `%Canon::AFMicroAdj` (MakerNote tag 0x4013), transcribed from ExifTool.
const TABLE_AF_MICRO_ADJ: &[CanonBinaryField] = &[CanonBinaryField {
    index: 1,
    name: "AFMicroAdjMode",
    format: CanonBinaryFormat::Int32s,
    count: 1,
    conv: CanonBinaryConv::Map(TABLE_AF_MICRO_ADJ_CONV10),
}];

/// `%Canon::VignettingCorr2` (MakerNote tag 0x4016), transcribed from ExifTool.
const TABLE_VIGNETTING_CORR2: &[CanonBinaryField] = &[
    CanonBinaryField {
        index: 5,
        name: "PeripheralLightingSetting",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_VIGNETTING_CORR2_CONV11),
    },
    CanonBinaryField {
        index: 6,
        name: "ChromaticAberrationSetting",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_VIGNETTING_CORR2_CONV12),
    },
    CanonBinaryField {
        index: 7,
        name: "DistortionCorrectionSetting",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_VIGNETTING_CORR2_CONV13),
    },
    CanonBinaryField {
        index: 9,
        name: "DigitalLensOptimizerSetting",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_VIGNETTING_CORR2_CONV14),
    },
];

/// `%Canon::LightingOpt` (MakerNote tag 0x4018), transcribed from ExifTool.
const TABLE_LIGHTING_OPT: &[CanonBinaryField] = &[
    CanonBinaryField {
        index: 1,
        name: "PeripheralIlluminationCorr",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_LIGHTING_OPT_CONV15),
    },
    CanonBinaryField {
        index: 2,
        name: "AutoLightingOptimizer",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_LIGHTING_OPT_CONV16),
    },
    CanonBinaryField {
        index: 3,
        name: "HighlightTonePriority",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_LIGHTING_OPT_CONV48),
    },
    CanonBinaryField {
        index: 4,
        name: "LongExposureNoiseReduction",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_LIGHTING_OPT_CONV17),
    },
    CanonBinaryField {
        index: 5,
        name: "HighISONoiseReduction",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_LIGHTING_OPT_CONV18),
    },
    CanonBinaryField {
        index: 10,
        name: "DigitalLensOptimizer",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_LIGHTING_OPT_CONV19),
    },
    CanonBinaryField {
        index: 11,
        name: "DualPixelRaw",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_LIGHTING_OPT_CONV20),
    },
];

/// `%Canon::MultiExp` (MakerNote tag 0x4021), transcribed from ExifTool.
const TABLE_MULTI_EXP: &[CanonBinaryField] = &[
    CanonBinaryField {
        index: 1,
        name: "MultiExposure",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_MULTI_EXP_CONV21),
    },
    CanonBinaryField {
        index: 2,
        name: "MultiExposureControl",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_MULTI_EXP_CONV22),
    },
    CanonBinaryField {
        index: 3,
        name: "MultiExposureShots",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
];

/// `%Canon::HDRInfo` (MakerNote tag 0x4025), transcribed from ExifTool.
const TABLE_HDR_INFO: &[CanonBinaryField] = &[
    CanonBinaryField {
        index: 1,
        name: "HDR",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_HDR_INFO_CONV23),
    },
    CanonBinaryField {
        index: 2,
        name: "HDREffect",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_HDR_INFO_CONV24),
    },
];

/// `%Canon::AFConfig` (MakerNote tag 0x4028), transcribed from ExifTool.
const TABLE_AF_CONFIG: &[CanonBinaryField] = &[
    CanonBinaryField {
        index: 5,
        name: "AIServoFirstImage",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV25),
    },
    CanonBinaryField {
        index: 6,
        name: "AIServoSecondImage",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV26),
    },
    CanonBinaryField {
        index: 8,
        name: "AFAssistBeam",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV27),
    },
    CanonBinaryField {
        index: 9,
        name: "OneShotAFRelease",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV28),
    },
    CanonBinaryField {
        index: 11,
        name: "LensDriveWhenAFImpossible",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV29),
    },
    CanonBinaryField {
        index: 12,
        name: "SelectAFAreaSelectionMode",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Bitmask(TABLE_AF_CONFIG_CONV30),
    },
    CanonBinaryField {
        index: 13,
        name: "AFAreaSelectionMethod",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV31),
    },
    CanonBinaryField {
        index: 14,
        name: "OrientationLinkedAF",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV32),
    },
    CanonBinaryField {
        index: 15,
        name: "ManualAFPointSelPattern",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV33),
    },
    CanonBinaryField {
        index: 16,
        name: "AFPointDisplayDuringFocus",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV34),
    },
    CanonBinaryField {
        index: 17,
        name: "VFDisplayIllumination",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV35),
    },
    CanonBinaryField {
        index: 20,
        name: "SubjectToDetect",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV36),
    },
    CanonBinaryField {
        index: 21,
        name: "SubjectSwitching",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV37),
    },
    CanonBinaryField {
        index: 24,
        name: "EyeDetection",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV38),
    },
    CanonBinaryField {
        index: 26,
        name: "WholeAreaTracking",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV39),
    },
    CanonBinaryField {
        index: 27,
        name: "ServoAFCharacteristics",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV40),
    },
    CanonBinaryField {
        index: 28,
        name: "CaseAutoSetting",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV41),
    },
    CanonBinaryField {
        index: 29,
        name: "ActionPriority",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV42),
    },
    CanonBinaryField {
        index: 30,
        name: "SportEvents",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_AF_CONFIG_CONV43),
    },
];

/// `%Canon::FocusBracketingInfo` (MakerNote tag 0x4053), transcribed from ExifTool.
const TABLE_FOCUS_BRACKETING_INFO: &[CanonBinaryField] = &[
    CanonBinaryField {
        index: 1,
        name: "FocusBracketing",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_FOCUS_BRACKETING_INFO_CONV44),
    },
    CanonBinaryField {
        index: 2,
        name: "FocusBracketingImageCount",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 3,
        name: "FocusBracketingFocusIncrement",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 4,
        name: "FocusBracketingExposureSmoothing",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_FOCUS_BRACKETING_INFO_CONV45),
    },
    CanonBinaryField {
        index: 5,
        name: "FocusBracketingDepthComposite",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_FOCUS_BRACKETING_INFO_CONV46),
    },
    CanonBinaryField {
        index: 6,
        name: "FocusBracketingCropDepthComposite",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_FOCUS_BRACKETING_INFO_CONV47),
    },
    CanonBinaryField {
        index: 7,
        name: "FocusBracketingFlashInterval",
        format: CanonBinaryFormat::Int32s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
];

/// `%Canon::ContrastInfo` (MakerNote tag 0x0027, Canon.pm:6607). `FORMAT => 'int16u'`
/// with no `FIRST_ENTRY`, so key 4 is byte 8.
const TABLE_CONTRAST_INFO: &[CanonBinaryField] = &[CanonBinaryField {
    index: 4,
    name: "IntelligentContrast",
    format: CanonBinaryFormat::Int16u,
    count: 1,
    conv: CanonBinaryConv::IntelligentContrast,
}];

/// `%Canon::FaceDetect1` (MakerNote tag 0x0024, Canon.pm:6733). `FORMAT => 'int16u'`,
/// `FIRST_ENTRY => 0`.
///
/// Keys 0x08..0x18 (`Face1Position`..`Face9Position`) carry
/// `RawConv => '$$self{FacesDetected} < n ? undef : $val'`, so they are gated by
/// [`FACE_DETECT1_GATE`] rather than emitted unconditionally.
const TABLE_FACE_DETECT1: &[CanonBinaryField] = &[
    CanonBinaryField {
        index: 0x02,
        name: "FacesDetected",
        format: CanonBinaryFormat::Int16u,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 0x03,
        name: "FaceDetectFrameSize",
        format: CanonBinaryFormat::Int16u,
        count: 2,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 0x08,
        name: "Face1Position",
        format: CanonBinaryFormat::Int16s,
        count: 2,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 0x0a,
        name: "Face2Position",
        format: CanonBinaryFormat::Int16s,
        count: 2,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 0x0c,
        name: "Face3Position",
        format: CanonBinaryFormat::Int16s,
        count: 2,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 0x0e,
        name: "Face4Position",
        format: CanonBinaryFormat::Int16s,
        count: 2,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 0x10,
        name: "Face5Position",
        format: CanonBinaryFormat::Int16s,
        count: 2,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 0x12,
        name: "Face6Position",
        format: CanonBinaryFormat::Int16s,
        count: 2,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 0x14,
        name: "Face7Position",
        format: CanonBinaryFormat::Int16s,
        count: 2,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 0x16,
        name: "Face8Position",
        format: CanonBinaryFormat::Int16s,
        count: 2,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 0x18,
        name: "Face9Position",
        format: CanonBinaryFormat::Int16s,
        count: 2,
        conv: CanonBinaryConv::Raw,
    },
];

/// `%Canon::FaceDetect3` (MakerNote tag 0x002f, Canon.pm:6830). `FORMAT => 'int16u'`,
/// `FIRST_ENTRY => 1` -- key 0 is the record's own byte count.
const TABLE_FACE_DETECT3: &[CanonBinaryField] = &[CanonBinaryField {
    index: 3,
    name: "FacesDetected",
    format: CanonBinaryFormat::Int16u,
    count: 1,
    conv: CanonBinaryConv::Raw,
}];

/// `%Canon::Ambience` (MakerNote tag 0x4020, Canon.pm:9151). `FORMAT => 'int32s'`, so
/// key 1 is byte 4.
const TABLE_AMBIENCE: &[CanonBinaryField] = &[CanonBinaryField {
    index: 1,
    name: "AmbienceSelection",
    format: CanonBinaryFormat::Int32s,
    count: 1,
    conv: CanonBinaryConv::Map(TABLE_AMBIENCE_CONV_SELECTION),
}];

const TABLE_AMBIENCE_CONV_SELECTION: &[(i64, &str)] = &[
    (0, "Standard"),
    (1, "Vivid"),
    (2, "Warm"),
    (3, "Soft"),
    (4, "Cool"),
    (5, "Intense"),
    (6, "Brighter"),
    (7, "Darker"),
    (8, "Monochrome"),
];

/// `%Canon::VignettingCorr` (MakerNote tag 0x4015, Canon.pm:8999). `FORMAT => 'int16s'`,
/// and `FIRST_ENTRY => 1` here does *not* mean a leading byte count: key 0 is
/// `VignettingCorrVersion` and the size word sits at byte 2 (Canon.pm:2103 validates
/// `$subdirStart+2`). The record therefore must not be realigned.
///
/// Keys 4 and 5 are both named `ChromaticAberrationCorr` in ExifTool; the later key wins
/// in a name-keyed map, which matches what `exiftool -G1 -s` prints for the pair.
const TABLE_VIGNETTING_CORR: &[CanonBinaryField] = &[
    CanonBinaryField {
        index: 0,
        name: "VignettingCorrVersion",
        format: CanonBinaryFormat::Int8u,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 2,
        name: "PeripheralLighting",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_VIGNETTING_CORR_CONV_OFF_ON),
    },
    CanonBinaryField {
        index: 3,
        name: "DistortionCorrection",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_VIGNETTING_CORR_CONV_OFF_ON),
    },
    CanonBinaryField {
        index: 4,
        name: "ChromaticAberrationCorr",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_VIGNETTING_CORR_CONV_OFF_ON),
    },
    CanonBinaryField {
        index: 5,
        name: "ChromaticAberrationCorr",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Map(TABLE_VIGNETTING_CORR_CONV_OFF_ON),
    },
    CanonBinaryField {
        index: 6,
        name: "PeripheralLightingValue",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 9,
        name: "DistortionCorrectionValue",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 11,
        name: "OriginalImageWidth",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
    CanonBinaryField {
        index: 12,
        name: "OriginalImageHeight",
        format: CanonBinaryFormat::Int16s,
        count: 1,
        conv: CanonBinaryConv::Raw,
    },
];

/// `%offOn` (Canon.pm:1218).
const TABLE_VIGNETTING_CORR_CONV_OFF_ON: &[(i64, &str)] = &[(0, "Off"), (1, "On")];

/// `%Canon::VignettingCorrUnknown` (Canon.pm:9036), the table behind tag 0x4015's second
/// and third alternatives. Its only named field is the version byte; ExifTool marks the
/// rest of the record `Unknown` and hides it without `-U`.
const TABLE_VIGNETTING_CORR_UNKNOWN: &[CanonBinaryField] = &[CanonBinaryField {
    index: 0,
    name: "VignettingCorrVersion",
    format: CanonBinaryFormat::Int8u,
    count: 1,
    conv: CanonBinaryConv::Raw,
}];

/// An ExifTool `RawConv => '$$self{<member>} < n ? undef : $val'` chain: the named fields
/// are emitted only when the record's count field holds at least `n`.
struct CountGate {
    /// The `DataMember` key, in the same units as [`CanonBinaryField::index`].
    index: usize,
    /// `(field name, minimum count)`, in table order.
    minimums: &'static [(&'static str, i64)],
}

/// `%Canon::FaceDetect1` (Canon.pm:6733) gates `FaceNPosition` on `FacesDetected`
/// (key 0x02, its sole `DATAMEMBER`).
const FACE_DETECT1_GATE: CountGate = CountGate {
    index: 0x02,
    minimums: &[
        ("Face1Position", 1),
        ("Face2Position", 2),
        ("Face3Position", 3),
        ("Face4Position", 4),
        ("Face5Position", 5),
        ("Face6Position", 6),
        ("Face7Position", 7),
        ("Face8Position", 8),
        ("Face9Position", 9),
    ],
};

/// One MakerNote tag's binary sub-table.
struct CanonBinaryTable {
    tag: u16,
    fields: &'static [CanonBinaryField],
    /// The table is `FIRST_ENTRY => 1` *and* its index 0 holds the record's own byte
    /// count, so a record stored one word out of step can be realigned.
    length_prefixed: bool,
    /// ExifTool's `Condition` on the MakerNote tag, evaluated against the raw record
    /// bytes exactly as `$$valPt` is. `None` when the tag names this table
    /// unconditionally.
    condition: Option<fn(&[u8]) -> bool>,
    /// An ExifTool `DATAMEMBER` count that suppresses later fields.
    gate: Option<&'static CountGate>,
}

/// `0x27 => { Condition => '$$valPt =~ /^\x0a\0/', ... }` (Canon.pm:1719).
fn contrast_info_applies(raw: &[u8]) -> bool {
    raw.starts_with(&[0x0a, 0x00])
}

/// `0x4015 => [{ Condition => '$$valPt =~ /^\0/ and $$valPt !~ /^(\0\0\0\0|\x00\x40\xdc\x05)/', ... }]`
/// (Canon.pm:2100), the first of that tag's three alternatives and the only one with a
/// fully named table.
fn vignetting_corr_applies(raw: &[u8]) -> bool {
    raw.first() == Some(&0)
        && !raw.starts_with(&[0x00, 0x00, 0x00, 0x00])
        && !raw.starts_with(&[0x00, 0x40, 0xdc, 0x05])
}

/// `Condition => '$$valPt =~ /^[\x01\x02\x10\x20]/ and $$valPt !~ /^(\0\0\0\0|\x02\x50\x7c\x04)/'`
/// (Canon.pm:2108), tag 0x4015's second alternative.
fn vignetting_corr_unknown1_applies(raw: &[u8]) -> bool {
    matches!(raw.first(), Some(0x01 | 0x02 | 0x10 | 0x20))
        && !raw.starts_with(&[0x02, 0x50, 0x7c, 0x04])
}

/// `Condition => '$$valPt !~ /^\0\0\0\0/'` (Canon.pm:2116), tag 0x4015's third and last
/// alternative. Reached only when neither of the two above matched.
fn vignetting_corr_unknown2_applies(raw: &[u8]) -> bool {
    !raw.starts_with(&[0x00, 0x00, 0x00, 0x00])
}

/// `0x4020 => { Condition => '$$valPt !~ /^\0\0\0\0/', ... }` (Canon.pm:2144).
fn ambience_applies(raw: &[u8]) -> bool {
    !raw.starts_with(&[0x00, 0x00, 0x00, 0x00])
}

/// MakerNote tag -> the `%Canon` binary table ExifTool parses it with.
///
/// A tag whose ExifTool entry is a list of alternatives appears once per alternative, in
/// ExifTool's own order; [`select_table`] takes the first whose `Condition` holds.
const CANON_BINARY_TABLES: &[CanonBinaryTable] = &[
    table(0x001d, TABLE_MY_COLORS, false), // %Canon::MyColors
    table(0x0024, TABLE_FACE_DETECT1, false).gated(&FACE_DETECT1_GATE), // %Canon::FaceDetect1
    table(0x0027, TABLE_CONTRAST_INFO, false).when(contrast_info_applies), // %Canon::ContrastInfo
    table(0x0029, TABLE_WB_INFO, true),    // %Canon::WBInfo
    table(0x002f, TABLE_FACE_DETECT3, true), // %Canon::FaceDetect3
    table(0x0035, TABLE_TIME_INFO, true),  // %Canon::TimeInfo
    table(0x0098, TABLE_CROP_INFO, false), // %Canon::CropInfo
    table(0x009a, TABLE_ASPECT_INFO, false), // %Canon::AspectInfo
    table(0x00b0, TABLE_FLAGS, true),      // %Canon::Flags
    table(0x00b1, TABLE_MODIFIED_INFO, true), // %Canon::ModifiedInfo
    table(0x00b6, TABLE_PREVIEW_IMAGE_INFO, true), // %Canon::PreviewImageInfo
    table(0x4003, TABLE_COLOR_INFO, true), // %Canon::ColorInfo
    table(0x4013, TABLE_AF_MICRO_ADJ, true), // %Canon::AFMicroAdj
    table(0x4015, TABLE_VIGNETTING_CORR, false).when(vignetting_corr_applies), // %Canon::VignettingCorr
    table(0x4015, TABLE_VIGNETTING_CORR_UNKNOWN, false).when(vignetting_corr_unknown1_applies),
    table(0x4015, TABLE_VIGNETTING_CORR_UNKNOWN, false).when(vignetting_corr_unknown2_applies),
    table(0x4016, TABLE_VIGNETTING_CORR2, true), // %Canon::VignettingCorr2
    table(0x4018, TABLE_LIGHTING_OPT, true),     // %Canon::LightingOpt
    table(0x4020, TABLE_AMBIENCE, true).when(ambience_applies), // %Canon::Ambience
    table(0x4021, TABLE_MULTI_EXP, true),        // %Canon::MultiExp
    table(0x4025, TABLE_HDR_INFO, true),         // %Canon::HDRInfo
    table(0x4028, TABLE_AF_CONFIG, true),        // %Canon::AFConfig
    table(0x4053, TABLE_FOCUS_BRACKETING_INFO, true), // %Canon::FocusBracketingInfo
];

const fn table(
    tag: u16,
    fields: &'static [CanonBinaryField],
    length_prefixed: bool,
) -> CanonBinaryTable {
    CanonBinaryTable {
        tag,
        fields,
        length_prefixed,
        condition: None,
        gate: None,
    }
}

impl CanonBinaryTable {
    const fn when(self, condition: fn(&[u8]) -> bool) -> Self {
        Self {
            condition: Some(condition),
            ..self
        }
    }

    const fn gated(self, gate: &'static CountGate) -> Self {
        Self {
            gate: Some(gate),
            ..self
        }
    }
}

/// Reads one field's values out of the record, or `None` if it runs past the end.
fn read_field(record: &[i16], field: &CanonBinaryField, byte_order: ByteOrder) -> Option<Vec<i64>> {
    let mut values = Vec::with_capacity(field.count);
    let words = field.format.words();
    for offset in 0..field.count {
        // ExifTool keys are element indexes in the table's declared FORMAT, while
        // `record` is always a 16-bit-word view of the MakerNote payload. An int32
        // key therefore starts at `index * 2`, not `index`; the latter silently read
        // the preceding field (or half of one) from every 32-bit Canon table.
        let word = field
            .index
            .checked_mul(words)?
            .checked_add(offset * words)?;
        match field.format {
            CanonBinaryFormat::Int8u => {
                // The key still steps one 16-bit word, but the value is only the first
                // stored byte of it -- the low half under little-endian, the high half
                // under big-endian.
                let raw = *record.get(word)? as u16;
                values.push(i64::from(match byte_order {
                    ByteOrder::LittleEndian => raw & 0x00ff,
                    ByteOrder::BigEndian => raw >> 8,
                }));
            }
            CanonBinaryFormat::Int16s => {
                values.push(i64::from(*record.get(word)?));
            }
            CanonBinaryFormat::Int16u => {
                values.push(i64::from(*record.get(word)? as u16));
            }
            CanonBinaryFormat::Int32u | CanonBinaryFormat::Int32s => {
                let low = u32::from(*record.get(word)? as u16);
                let high = u32::from(*record.get(word + 1)? as u16);
                // The words were decoded with the file's byte order, so recombining them
                // has to follow that same order to rebuild the 32-bit value.
                let combined = match byte_order {
                    ByteOrder::LittleEndian => low | (high << 16),
                    ByteOrder::BigEndian => (low << 16) | high,
                };
                values.push(match field.format {
                    CanonBinaryFormat::Int32s => i64::from(combined as i32),
                    _ => i64::from(combined),
                });
            }
        }
    }
    Some(values)
}

/// ExifTool's `GetRational64s` conversion for `%Canon::AFMicroAdj` key 2.
///
/// `AFMicroAdj` declares `FORMAT => 'int32s'`, so its key 2 begins at word 4 in the
/// parser's 16-bit view. The field itself is a signed numerator/denominator pair, and
/// `GetRational64s` renders a non-zero denominator via `RoundFloat(..., 10)`.
fn read_af_micro_adj_value(record: &[i16], byte_order: ByteOrder) -> Option<String> {
    fn read_i32(record: &[i16], word: usize, byte_order: ByteOrder) -> Option<i32> {
        let first = u32::from(*record.get(word)? as u16);
        let second = u32::from(*record.get(word.checked_add(1)?)? as u16);
        let combined = match byte_order {
            ByteOrder::LittleEndian => first | (second << 16),
            ByteOrder::BigEndian => (first << 16) | second,
        };
        Some(combined as i32)
    }

    let numerator = read_i32(record, 4, byte_order)?;
    let denominator = read_i32(record, 6, byte_order)?;
    if denominator == 0 {
        return Some(if numerator == 0 { "undef" } else { "inf" }.to_string());
    }
    Some(exiftool_rational_number(
        f64::from(numerator) / f64::from(denominator),
    ))
}

/// Renders one value through the field's `PrintConv`.
fn render_value(conv: CanonBinaryConv, value: i64) -> String {
    match conv {
        CanonBinaryConv::Raw => value.to_string(),
        CanonBinaryConv::Map(table) => table
            .iter()
            .find(|(key, _)| *key == value)
            .map(|(_, label)| (*label).to_string())
            .unwrap_or_else(|| format!("Unknown ({})", value)),
        CanonBinaryConv::PrintParameter => {
            // `%Image::ExifTool::Exif::printParameter` (Exif.pm:317) plus
            // `Exif::PrintParameter` (Exif.pm:5533): zero prints "Normal", a positive
            // value carries an explicit "+", and a value above 0xfff0 is an int16
            // negative in disguise.
            if value == 0 {
                return "Normal".to_string();
            }
            if value > 0 {
                if value > 0xfff0 {
                    return (value - 0x10000).to_string();
                }
                return format!("+{}", value);
            }
            value.to_string()
        }
        CanonBinaryConv::IntelligentContrast => match value {
            0x00 => "Off".to_string(),
            0x08 => "On".to_string(),
            0xffff => "n/a".to_string(),
            other if other & 0x08 != 0 => format!("On (0x{other:02x})"),
            other => format!("Off (0x{other:02x})"),
        },
        CanonBinaryConv::Bitmask(table) => {
            // ExifTool's DecodeBits defaults to a 32-bit word and retains set bits
            // missing from the lookup as `[n]`. Dropping those bits made Canon's
            // SelectAFAreaSelectionMode disagree whenever a newer body populated
            // reserved bits beyond the six names known to Canon.pm.
            let bits = value as u32;
            let names: Vec<String> = (0..u32::BITS)
                .filter(|bit| bits & (1u32 << bit) != 0)
                .map(|bit| {
                    table
                        .iter()
                        .find(|(known_bit, _)| *known_bit == i64::from(bit))
                        .map_or_else(|| format!("[{bit}]"), |(_, label)| (*label).to_string())
                })
                .collect();
            if names.is_empty() {
                "(none)".to_string()
            } else {
                names.join(", ")
            }
        }
    }
}

fn lookup(tag_id: u16) -> Option<&'static CanonBinaryTable> {
    CANON_BINARY_TABLES.iter().find(|entry| entry.tag == tag_id)
}

/// The alternative ExifTool would pick for this record: the first whose `Condition`
/// holds, with an unconditional entry always holding.
///
/// A conditioned table needs the untyped bytes ExifTool's `$$valPt` sees; when the record
/// was short enough to live inline in the entry there are none to test, and no
/// alternative is chosen rather than guessing one.
fn select_table(tag_id: u16, raw: &[u8]) -> Option<&'static CanonBinaryTable> {
    CANON_BINARY_TABLES
        .iter()
        .filter(|entry| entry.tag == tag_id)
        .find(|entry| match entry.condition {
            None => true,
            Some(condition) => !raw.is_empty() && condition(raw),
        })
}

/// Decodes a `%Canon` binary sub-table into `Canon:`-prefixed tags.
///
/// `raw` is the record exactly as stored, for the ExifTool `Condition` that some tags put
/// on their sub-table; `record` is the same bytes as 16-bit words.
///
/// Returns `false` when `tag_id` has no table here, so the caller can fall through to its
/// own handling.
pub(crate) fn parse_binary_table(
    tag_id: u16,
    raw: &[u8],
    record: &[i16],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) -> bool {
    if lookup(tag_id).is_none() {
        return false;
    }
    let Some(entry) = select_table(tag_id, raw) else {
        // The record matches no alternative ExifTool declares for this tag, so emit
        // nothing rather than the wrong names.
        return true;
    };

    // A gated table's count field decides how many of the later fields exist at all.
    let count = entry.gate.and_then(|gate| {
        entry
            .fields
            .iter()
            .find(|field| field.index == gate.index)
            .and_then(|field| read_field(record, field, byte_order))
            .and_then(|values| values.first().copied())
    });

    for field in entry.fields {
        if let Some(gate) = entry.gate
            && let Some((_, minimum)) = gate.minimums.iter().find(|(name, _)| *name == field.name)
            && count.is_none_or(|detected| detected < *minimum)
        {
            continue;
        }
        let Some(values) = read_field(record, field, byte_order) else {
            continue;
        };
        let rendered = values
            .iter()
            .map(|&value| render_value(field.conv, value))
            .collect::<Vec<_>>()
            .join(" ");
        tags.insert(format!("Canon:{}", field.name), rendered);
    }
    if tag_id == 0x4013
        && let Some(value) = read_af_micro_adj_value(record, byte_order)
    {
        tags.insert("Canon:AFMicroAdjValue".to_string(), value);
    }
    true
}

/// Whether the table for `tag_id` opens with the record's own byte count, so a record
/// stored one word out of step can be realigned before its keys are applied.
pub(crate) fn table_is_length_prefixed(tag_id: u16) -> bool {
    lookup(tag_id).is_some_and(|entry| entry.length_prefixed)
}

/// Whether any table here handles `tag_id`.
pub(crate) fn handles_tag(tag_id: u16) -> bool {
    lookup(tag_id).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests describe a record as the decoded words a caller would pass in. Only the
    /// conditioned tags look at the untyped bytes, so re-encode them from those words.
    fn parse(
        tag: u16,
        record: &[i16],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> bool {
        let raw: Vec<u8> = record
            .iter()
            .flat_map(|word| match byte_order {
                ByteOrder::LittleEndian => (*word as u16).to_le_bytes(),
                ByteOrder::BigEndian => (*word as u16).to_be_bytes(),
            })
            .collect();
        parse_binary_table(tag, &raw, record, byte_order, tags)
    }

    #[test]
    fn test_unhandled_tag_reports_false() {
        let mut tags = HashMap::new();
        assert!(!parse(
            0x0001,
            &[0i16; 8],
            ByteOrder::LittleEndian,
            &mut tags
        ));
        assert!(tags.is_empty());
    }

    /// `%Canon::CropInfo` (tag 0x0098) is `FIRST_ENTRY => 0`, so its keys index the record
    /// straight from 0.
    #[test]
    fn test_crop_info_is_indexed_from_zero() {
        assert!(!table_is_length_prefixed(0x0098));
        let mut tags = HashMap::new();
        assert!(parse(
            0x0098,
            &[11i16, 22, 33, 44],
            ByteOrder::LittleEndian,
            &mut tags
        ));
        assert_eq!(tags.get("Canon:CropLeftMargin"), Some(&"11".to_string()));
        assert_eq!(tags.get("Canon:CropRightMargin"), Some(&"22".to_string()));
        assert_eq!(tags.get("Canon:CropTopMargin"), Some(&"33".to_string()));
        assert_eq!(tags.get("Canon:CropBottomMargin"), Some(&"44".to_string()));
    }

    /// `%Canon::AFConfig` (tag 0x4028) is `FIRST_ENTRY => 1`: index 0 is the record's own
    /// byte count, so key 5 is element 5.
    #[test]
    fn test_length_prefixed_table_keeps_exiftool_indices() {
        assert!(table_is_length_prefixed(0x4028));
    }

    /// The AFMicroAdj record from CanonRaw.cr3 has a signed rational at key 2.
    /// ExifTool 13.59 reads its bytes `00 00 00 00 0a 00 00 00` as `0/10` and
    /// renders `AFMicroAdjValue` as `0`.
    #[test]
    fn test_af_micro_adj_value_reads_signed_rational_from_canonraw_cr3() {
        let record = [
            0x2c, 0x00, 0x00, 0x00, // record byte count
            0x00, 0x00, 0x00, 0x00, // key 1: AFMicroAdjMode = Disable
            0x00, 0x00, 0x00, 0x00, // key 2 numerator
            0x0a, 0x00, 0x00, 0x00, // key 2 denominator
        ];

        assert_eq!(
            parse_bytes(0x4013, &record).get("Canon:AFMicroAdjValue"),
            Some(&"0".to_string())
        );
    }

    /// `%Canon::TimeInfo` declares `FORMAT => 'int32s'`, so ExifTool key 2 starts at
    /// i16 word 4 and key 3 at word 6. Reading keys as word indexes instead shifted every
    /// field two slots early and returned plausible-but-wrong neighboring values.
    #[test]
    fn test_int32_table_keys_are_scaled_to_i16_words() {
        let mut little_endian = vec![0i16; 8];
        little_endian[4] = 20; // key 2: London
        little_endian[6] = 60; // key 3: daylight savings on
        let mut tags = HashMap::new();
        assert!(parse(
            0x0035,
            &little_endian,
            ByteOrder::LittleEndian,
            &mut tags,
        ));
        assert_eq!(tags.get("Canon:TimeZoneCity"), Some(&"London".to_string()));
        assert_eq!(tags.get("Canon:DaylightSavings"), Some(&"On".to_string()));

        let mut big_endian = vec![0i16; 8];
        big_endian[5] = 20;
        big_endian[7] = 60;
        let mut tags = HashMap::new();
        assert!(parse(0x0035, &big_endian, ByteOrder::BigEndian, &mut tags,));
        assert_eq!(tags.get("Canon:TimeZoneCity"), Some(&"London".to_string()));
        assert_eq!(tags.get("Canon:DaylightSavings"), Some(&"On".to_string()));
    }

    /// Bare scalar entries inherit their table's `FORMAT`. AspectInfo is `int32u`, so
    /// width/height must retain both words instead of being truncated to signed int16.
    #[test]
    fn test_bare_int32_fields_inherit_full_table_width() {
        let mut record = vec![0i16; 10];
        // key 1 = 70_000 (0x0001_1170), key 2 = 80_000 (0x0001_3880)
        record[2] = 0x1170;
        record[3] = 0x0001;
        record[4] = 0x3880;
        record[5] = 0x0001;
        let mut tags = HashMap::new();
        assert!(parse(0x009a, &record, ByteOrder::LittleEndian, &mut tags,));
        assert_eq!(
            tags.get("Canon:CroppedImageWidth"),
            Some(&"70000".to_string())
        );
        assert_eq!(
            tags.get("Canon:CroppedImageHeight"),
            Some(&"80000".to_string())
        );
    }

    /// Array counts advance in declared-format units too: four int32 values consume
    /// eight i16 words starting at the scaled key offset.
    #[test]
    fn test_int32_array_count_advances_by_two_words() {
        let mut record = vec![0i16; 12];
        for (offset, value) in [10i16, 20, 30, 40].into_iter().enumerate() {
            record[4 + offset * 2] = value;
        }
        let mut tags = HashMap::new();
        assert!(parse(0x0029, &record, ByteOrder::LittleEndian, &mut tags,));
        assert_eq!(
            tags.get("Canon:WB_GRBGLevelsAuto"),
            Some(&"10 20 30 40".to_string())
        );
    }

    /// These fields splice ExifTool's shared `%offOn` hash. The original table
    /// transcription accidentally copied an unrelated color-space map, yielding
    /// values such as `People` and `sRGB` for boolean camera settings.
    #[test]
    fn test_shared_off_on_conversions_match_exiftool() {
        let mut vignetting = vec![0i16; 20];
        vignetting[10] = 1; // key 5
        let mut tags = HashMap::new();
        assert!(parse(
            0x4016,
            &vignetting,
            ByteOrder::LittleEndian,
            &mut tags,
        ));
        assert_eq!(
            tags.get("Canon:PeripheralLightingSetting"),
            Some(&"On".to_string())
        );

        let mut focus = vec![0i16; 16];
        focus[10] = 1; // key 5
        let mut tags = HashMap::new();
        assert!(parse(0x4053, &focus, ByteOrder::LittleEndian, &mut tags,));
        assert_eq!(tags.get("Canon:FocusBracketing"), Some(&"Off".to_string()));
        assert_eq!(
            tags.get("Canon:FocusBracketingDepthComposite"),
            Some(&"On".to_string())
        );
    }

    /// `%Canon::ColorInfo` key 2 `ColorTone` carries ExifTool's shared `%printParameter`
    /// as a hash splice rather than a literal `PrintConv`, so 0 prints "Normal" and a
    /// positive value carries a "+". Emitting it raw reported "0" for "Normal".
    #[test]
    fn test_print_parameter_conv() {
        assert_eq!(render_value(CanonBinaryConv::PrintParameter, 0), "Normal");
        assert_eq!(render_value(CanonBinaryConv::PrintParameter, 1), "+1");
        assert_eq!(render_value(CanonBinaryConv::PrintParameter, -2), "-2");
        assert_eq!(
            render_value(CanonBinaryConv::PrintParameter, 0xfff1),
            (0xfff1i64 - 0x10000).to_string()
        );
    }

    #[test]
    fn test_map_and_bitmask_rendering() {
        let map = CanonBinaryConv::Map(&[(0, "Off"), (1, "On")]);
        assert_eq!(render_value(map, 0), "Off");
        assert_eq!(render_value(map, 1), "On");
        assert_eq!(render_value(map, 9), "Unknown (9)");

        let bits = CanonBinaryConv::Bitmask(&[(0, "First"), (2, "Third")]);
        assert_eq!(render_value(bits, 0), "(none)");
        assert_eq!(render_value(bits, 0b101), "First, Third");
        assert_eq!(render_value(bits, 0b1101), "First, Third, [3]");
        assert_eq!(render_value(bits, i64::from(i32::MIN)), "[31]");
    }

    /// Decodes a little-endian byte record exactly as the MakerNote walker does, so a
    /// test can paste the bytes `exiftool -v3` printed for a real file.
    fn words(bytes: &[u8]) -> Vec<i16> {
        bytes
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect()
    }

    fn parse_bytes(tag: u16, bytes: &[u8]) -> HashMap<String, String> {
        let mut tags = HashMap::new();
        assert!(parse_binary_table(
            tag,
            bytes,
            &words(bytes),
            ByteOrder::LittleEndian,
            &mut tags,
        ));
        tags
    }

    /// Tag 0x0027 on CanonIXUS170.jpg. `exiftool -G1 -s` reports
    /// `[Canon] IntelligentContrast : Off (0x10)`; `exiftool -v3` shows key 4 read from
    /// byte 8 of the record. The `OTHER` sub keeps the raw value visible for anything
    /// outside the three-entry hash.
    #[test]
    fn test_contrast_info_matches_exiftool() {
        let record = [
            0x0a, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00, 0x00, 0x10, 0x00, 0x2a, 0x00, 0x01, 0x00,
        ];
        assert_eq!(
            parse_bytes(0x0027, &record).get("Canon:IntelligentContrast"),
            Some(&"Off (0x10)".to_string())
        );

        assert_eq!(render_value(CanonBinaryConv::IntelligentContrast, 0), "Off");
        assert_eq!(render_value(CanonBinaryConv::IntelligentContrast, 8), "On");
        assert_eq!(
            render_value(CanonBinaryConv::IntelligentContrast, 0xffff),
            "n/a"
        );
        assert_eq!(
            render_value(CanonBinaryConv::IntelligentContrast, 0x19),
            "On (0x19)"
        );
    }

    /// `0x27 => { Condition => '$$valPt =~ /^\x0a\0/' }` (Canon.pm:1719). Canon.pm's own
    /// comment records other, undocumented versions of this record under the same tag, so
    /// a record that fails the condition must produce nothing rather than the wrong name.
    #[test]
    fn test_contrast_info_condition_rejects_other_records() {
        let record = [0x01, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x08, 0x00];
        let mut tags = HashMap::new();
        assert!(parse_binary_table(
            0x0027,
            &record,
            &words(&record),
            ByteOrder::LittleEndian,
            &mut tags,
        ));
        assert!(tags.is_empty());
    }

    /// Tag 0x002f on CanonEOS_M10.jpg: `FIRST_ENTRY => 1`, key 0 is the 34-byte count and
    /// key 3 is `FacesDetected`. `exiftool -G1 -s` reports `FacesDetected : 65535`, so the
    /// field is int16u and must not come back as -1.
    #[test]
    fn test_face_detect3_reads_key_three_as_unsigned() {
        let record = [
            0x22, 0x00, 0x02, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        ];
        assert_eq!(
            parse_bytes(0x002f, &record).get("Canon:FacesDetected"),
            Some(&"65535".to_string())
        );
    }

    /// Tag 0x0024 on CanonDIGITAL_IXUS70.jpg. `exiftool -G1 -s` reports
    /// `FacesDetected : 0` and `FaceDetectFrameSize : 320 240` and nothing else: every
    /// `FaceNPosition` carries `RawConv => '$$self{FacesDetected} < n ? undef : $val'`.
    #[test]
    fn test_face_detect1_gates_positions_on_face_count() {
        let record = [
            0x9c, 0x00, 0x23, 0x00, 0x00, 0x00, 0x40, 0x01, 0xf0, 0x00, 0x01, 0x00, 0x01, 0x00,
            0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let tags = parse_bytes(0x0024, &record);
        assert_eq!(tags.get("Canon:FacesDetected"), Some(&"0".to_string()));
        assert_eq!(
            tags.get("Canon:FaceDetectFrameSize"),
            Some(&"320 240".to_string())
        );
        assert_eq!(tags.get("Canon:Face1Position"), None);

        // With one face detected, key 0x08 becomes real: it is the signed int16 pair at
        // bytes 16..20, and key 0x0a (bytes 20..24) stays suppressed.
        let mut one_face = record;
        one_face[4] = 0x01;
        one_face[16] = 0x0a;
        one_face[17] = 0x00;
        one_face[18] = 0xf6;
        one_face[19] = 0xff;
        let tags = parse_bytes(0x0024, &one_face);
        assert_eq!(tags.get("Canon:FacesDetected"), Some(&"1".to_string()));
        assert_eq!(tags.get("Canon:Face1Position"), Some(&"10 -10".to_string()));
        assert_eq!(tags.get("Canon:Face2Position"), None);
    }

    /// Tag 0x4015 on CanonEOS-1D_C.jpg. Its size word sits at byte 2, not byte 0, so the
    /// record must not be realigned; keys 11 and 12 then land on the 5184x3456 that
    /// `exiftool -G1 -s` reports for `OriginalImageWidth`/`OriginalImageHeight`.
    #[test]
    fn test_vignetting_corr_layout_matches_exiftool() {
        let record = [
            0x00, 0x21, 0xc8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x26, 0x1b, 0x40, 0x14, 0x80, 0x0d,
        ];
        assert!(!table_is_length_prefixed(0x4015));
        let tags = parse_bytes(0x4015, &record);
        assert_eq!(
            tags.get("Canon:VignettingCorrVersion"),
            Some(&"0".to_string())
        );
        assert_eq!(
            tags.get("Canon:PeripheralLighting"),
            Some(&"Off".to_string())
        );
        assert_eq!(
            tags.get("Canon:OriginalImageWidth"),
            Some(&"5184".to_string())
        );
        assert_eq!(
            tags.get("Canon:OriginalImageHeight"),
            Some(&"3456".to_string())
        );
    }

    /// `0x4015` is a three-alternative list. A record that fails the first condition falls
    /// through to `%Canon::VignettingCorrUnknown`, which names only the version byte -- it
    /// must never be read with `%Canon::VignettingCorr`'s field names.
    #[test]
    fn test_vignetting_corr_falls_through_to_unknown_variants() {
        // Second alternative: first byte in [\x01\x02\x10\x20].
        let record = [
            0x02u8, 0x50, 0x7c, 0x00, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
        ];
        let tags = parse_bytes(0x4015, &record);
        assert_eq!(
            tags.get("Canon:VignettingCorrVersion"),
            Some(&"2".to_string())
        );
        assert_eq!(tags.get("Canon:OriginalImageWidth"), None);

        // Third alternative: anything else that is not four leading zero bytes.
        let record = [
            0x03u8, 0x00, 0x7c, 0x00, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
        ];
        let tags = parse_bytes(0x4015, &record);
        assert_eq!(
            tags.get("Canon:VignettingCorrVersion"),
            Some(&"3".to_string())
        );
        assert_eq!(tags.get("Canon:PeripheralLighting"), None);

        // All three conditions exclude an all-zero record.
        let zeros = [0u8; 16];
        let mut tags = HashMap::new();
        assert!(parse_binary_table(
            0x4015,
            &zeros,
            &words(&zeros),
            ByteOrder::LittleEndian,
            &mut tags,
        ));
        assert!(tags.is_empty());
    }

    /// Tag 0x4020 on CanonEOS-1D_XMarkIII.jpg. `FORMAT => 'int32s'`, so key 1 is byte 4,
    /// and `exiftool -G1 -s` reports `AmbienceSelection : Standard`.
    #[test]
    fn test_ambience_reads_int32_key_one() {
        let record = [
            0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        assert_eq!(
            parse_bytes(0x4020, &record).get("Canon:AmbienceSelection"),
            Some(&"Standard".to_string())
        );

        // `Condition => '$$valPt !~ /^\0\0\0\0/'` (Canon.pm:2144): the 60D writes an
        // all-zero record that ExifTool declines to decode.
        let zeros = [0u8; 12];
        let mut tags = HashMap::new();
        assert!(parse_binary_table(
            0x4020,
            &zeros,
            &words(&zeros),
            ByteOrder::LittleEndian,
            &mut tags,
        ));
        assert!(tags.is_empty());
    }

    /// A `Format => 'int8u'` override inside a 16-bit table reads the first stored byte of
    /// the key's word under either byte order.
    #[test]
    fn test_int8u_field_reads_first_stored_byte() {
        let field = CanonBinaryField {
            index: 0,
            name: "VignettingCorrVersion",
            format: CanonBinaryFormat::Int8u,
            count: 1,
            conv: CanonBinaryConv::Raw,
        };
        // Bytes 03 21 on disk: little-endian decodes them to 0x2103, big-endian to 0x0321,
        // and either way the value at byte 0 is 3.
        assert_eq!(
            read_field(&[0x2103], &field, ByteOrder::LittleEndian),
            Some(vec![3])
        );
        assert_eq!(
            read_field(&[0x0321], &field, ByteOrder::BigEndian),
            Some(vec![3])
        );
    }

    /// A record shorter than a field's index drops that field rather than panicking.
    #[test]
    fn test_short_record_is_safe() {
        let mut tags = HashMap::new();
        assert!(parse(0x0098, &[11i16], ByteOrder::LittleEndian, &mut tags));
        assert_eq!(tags.get("Canon:CropLeftMargin"), Some(&"11".to_string()));
        assert_eq!(tags.get("Canon:CropRightMargin"), None);
    }
}
