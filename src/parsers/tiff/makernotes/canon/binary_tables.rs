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

use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

/// How a field's value is laid out in the record's `int16` array.
#[derive(Clone, Copy)]
enum CanonBinaryFormat {
    Int16s,
    Int16u,
    Int32u,
    Int32s,
}

impl CanonBinaryFormat {
    /// Number of `i16` words occupied by one value in the table's declared format.
    const fn words(self) -> usize {
        match self {
            Self::Int16s | Self::Int16u => 1,
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

/// MakerNote tag -> the `%Canon` binary table ExifTool parses it with, and whether
/// that table is `FIRST_ENTRY => 1` (index 0 holds the record's own byte count).
const CANON_BINARY_TABLES: &[(u16, &[CanonBinaryField], bool)] = &[
    (0x001d, TABLE_MY_COLORS, false),            // %Canon::MyColors
    (0x0029, TABLE_WB_INFO, true),               // %Canon::WBInfo
    (0x0035, TABLE_TIME_INFO, true),             // %Canon::TimeInfo
    (0x0098, TABLE_CROP_INFO, false),            // %Canon::CropInfo
    (0x009a, TABLE_ASPECT_INFO, false),          // %Canon::AspectInfo
    (0x00b0, TABLE_FLAGS, true),                 // %Canon::Flags
    (0x00b1, TABLE_MODIFIED_INFO, true),         // %Canon::ModifiedInfo
    (0x00b6, TABLE_PREVIEW_IMAGE_INFO, true),    // %Canon::PreviewImageInfo
    (0x4003, TABLE_COLOR_INFO, true),            // %Canon::ColorInfo
    (0x4013, TABLE_AF_MICRO_ADJ, true),          // %Canon::AFMicroAdj
    (0x4016, TABLE_VIGNETTING_CORR2, true),      // %Canon::VignettingCorr2
    (0x4018, TABLE_LIGHTING_OPT, true),          // %Canon::LightingOpt
    (0x4021, TABLE_MULTI_EXP, true),             // %Canon::MultiExp
    (0x4025, TABLE_HDR_INFO, true),              // %Canon::HDRInfo
    (0x4028, TABLE_AF_CONFIG, true),             // %Canon::AFConfig
    (0x4053, TABLE_FOCUS_BRACKETING_INFO, true), // %Canon::FocusBracketingInfo
];

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

/// Decodes a `%Canon` binary sub-table into `Canon:`-prefixed tags.
///
/// Returns `false` when `tag_id` has no table here, so the caller can fall through to its
/// own handling.
pub(crate) fn parse_binary_table(
    tag_id: u16,
    record: &[i16],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) -> bool {
    let Some(&(_, fields, _)) = CANON_BINARY_TABLES
        .iter()
        .find(|(tag, _, _)| *tag == tag_id)
    else {
        return false;
    };

    for field in fields {
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
    true
}

/// Whether the table for `tag_id` is `FIRST_ENTRY => 1`, i.e. its index 0 holds the
/// record's own byte count rather than a field.
pub(crate) fn table_is_length_prefixed(tag_id: u16) -> bool {
    CANON_BINARY_TABLES
        .iter()
        .find(|(tag, _, _)| *tag == tag_id)
        .is_some_and(|(_, _, length_prefixed)| *length_prefixed)
}

/// Whether any table here handles `tag_id`.
pub(crate) fn handles_tag(tag_id: u16) -> bool {
    CANON_BINARY_TABLES.iter().any(|(tag, _, _)| *tag == tag_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unhandled_tag_reports_false() {
        let mut tags = HashMap::new();
        assert!(!parse_binary_table(
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
        assert!(parse_binary_table(
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

    /// `%Canon::TimeInfo` declares `FORMAT => 'int32s'`, so ExifTool key 2 starts at
    /// i16 word 4 and key 3 at word 6. Reading keys as word indexes instead shifted every
    /// field two slots early and returned plausible-but-wrong neighboring values.
    #[test]
    fn test_int32_table_keys_are_scaled_to_i16_words() {
        let mut little_endian = vec![0i16; 8];
        little_endian[4] = 20; // key 2: London
        little_endian[6] = 60; // key 3: daylight savings on
        let mut tags = HashMap::new();
        assert!(parse_binary_table(
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
        assert!(parse_binary_table(
            0x0035,
            &big_endian,
            ByteOrder::BigEndian,
            &mut tags,
        ));
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
        assert!(parse_binary_table(
            0x009a,
            &record,
            ByteOrder::LittleEndian,
            &mut tags,
        ));
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
        assert!(parse_binary_table(
            0x0029,
            &record,
            ByteOrder::LittleEndian,
            &mut tags,
        ));
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
        assert!(parse_binary_table(
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
        assert!(parse_binary_table(
            0x4053,
            &focus,
            ByteOrder::LittleEndian,
            &mut tags,
        ));
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

    /// A record shorter than a field's index drops that field rather than panicking.
    #[test]
    fn test_short_record_is_safe() {
        let mut tags = HashMap::new();
        assert!(parse_binary_table(
            0x0098,
            &[11i16],
            ByteOrder::LittleEndian,
            &mut tags
        ));
        assert_eq!(tags.get("Canon:CropLeftMargin"), Some(&"11".to_string()));
        assert_eq!(tags.get("Canon:CropRightMargin"), None);
    }
}
