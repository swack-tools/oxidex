//! Canon `ColorData` parser (MakerNote tag 0x4001).
//!
//! Canon writes white-balance, colour-temperature and sensor-level data as one large
//! `int16` array whose layout changes with the camera generation. ExifTool models this as
//! twelve separate tables, `%Canon::ColorData1` through `%Canon::ColorData12`, selected by
//! the array's element count (Canon.pm:1972), with individual fields further gated on the
//! `ColorDataVersion` stored at index 0.
//!
//! # What this replaces
//!
//! The previous version of this module was a hand-written guess: a dozen fixed byte
//! offsets described as "common offsets found in many EOS cameras", validated by a
//! 1000-15000 K range check. Those offsets correspond to no ExifTool `ColorData` version
//! -- ExifTool's keys are `int16` *word* indices into the record, not byte offsets, and
//! they move between versions. Nothing in the crate ever called it: `parse_canon_color_data`
//! was reachable only from its own unit tests, which asserted the invented layout back to
//! itself.
//!
//! The tables below are transcribed from `Canon.pm` by script rather than by hand.
//! Fields ExifTool flags `Unknown` are omitted, since ExifTool itself hides them without
//! `-U`.

use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

/// How a `ColorData` field's value is laid out in the record's `int16` array.
#[derive(Clone, Copy)]
enum ColorFieldFormat {
    /// ExifTool `int16s` -- the table default.
    Int16s,
    /// ExifTool `int16u`.
    Int16u,
    /// ExifTool `int32u`, stored as two consecutive words.
    Int32u,
    /// ExifTool `int32s`, stored as two consecutive words.
    Int32s,
}

/// One clause of an ExifTool `Condition` on `$$self{ColorDataVersion}`.
#[derive(Clone, Copy)]
enum ColorVersionTest {
    Eq(i32),
    Lt(i32),
    Gt(i32),
    Le(i32),
    Ge(i32),
}

impl ColorVersionTest {
    fn matches(&self, version: i32) -> bool {
        match *self {
            ColorVersionTest::Eq(v) => version == v,
            ColorVersionTest::Lt(v) => version < v,
            ColorVersionTest::Gt(v) => version > v,
            ColorVersionTest::Le(v) => version <= v,
            ColorVersionTest::Ge(v) => version >= v,
        }
    }
}

/// The conversion ExifTool applies to a `ColorData` field before printing it.
#[derive(Clone, Copy)]
enum ColorFieldConv {
    /// Printed as the raw number(s).
    None,
    /// ExifTool `RawConv => '$val || undef'` -- a zero drops the tag.
    DropZero,
    /// `ColorDataVersion`'s own lookup, e.g. `7 (500D/550D/7D/1DmkIV)`.
    VersionMap(&'static [(i32, &'static str)]),
    /// ExifTool `\&SwapWords`: the 32-bit values are stored with big-endian word order,
    /// opposite to their byte order, so the two halves swap after a normal read.
    SwapWords,
    /// `ValueConv => '$val >= 255 ? 255 : exp(($val-200)/16*log(2))'` then
    /// `PrintConv => '$val == 255 ? "Strobe or Misfire" : sprintf("%.0f%%", $val * 100)'`.
    FlashOutput,
    /// `PrintConv => '$val ? sprintf("%.2fV", $val * 5 / 186) : "n/a"'`.
    FlashBatteryLevel,
}

/// One field of a `%Canon::ColorData*` table, transcribed from ExifTool.
///
/// `offset` is an index into the record's `int16` array, matching ExifTool's own
/// keys (the tables declare `FORMAT => 'int16s'`, so a key is a word index, not a
/// byte offset). `versions` restricts a field to the `ColorDataVersion` values whose
/// ExifTool `Condition` tests, OR-ed together; an empty slice means unconditional.
struct ColorDataField {
    offset: usize,
    name: &'static str,
    format: ColorFieldFormat,
    count: usize,
    conv: ColorFieldConv,
    versions: &'static [ColorVersionTest],
}

const COLOR_DATA_VERSION_10: &[(i32, &str)] = &[(32, "32 (1DXmkIII)"), (33, "33 (R5/R6)")];
const COLOR_DATA_VERSION_11: &[(i32, &str)] = &[(34, "34 (R3)"), (48, "48 (R7/R10/R50/R6mkII)")];
const COLOR_DATA_VERSION_12: &[(i32, &str)] = &[(64, "64 (R1/R5mkII)"), (65, "65 (R50V)")];
const COLOR_DATA_VERSION_3: &[(i32, &str)] = &[(1, "1 (1DmkIIN/5D/30D/400D)")];
const COLOR_DATA_VERSION_4: &[(i32, &str)] = &[
    (2, "2 (1DmkIII)"),
    (3, "3 (40D)"),
    (4, "4 (1DSmkIII)"),
    (5, "5 (450D/1000D)"),
    (6, "6 (50D/5DmkII)"),
    (7, "7 (500D/550D/7D/1DmkIV)"),
    (9, "9 (60D/1100D)"),
];
const COLOR_DATA_VERSION_5: &[(i32, &str)] = &[(-4, "-4 (M100/M5/M6)"), (-3, "-3 (M10/M3)")];
const COLOR_DATA_VERSION_6: &[(i32, &str)] = &[(10, "10 (600D/1200D)")];
const COLOR_DATA_VERSION_7: &[(i32, &str)] = &[
    (10, "10 (1DX/5DmkIII/6D/70D/100D/650D/700D/M/M2)"),
    (11, "11 (7DmkII/750D/760D/8000D)"),
];
const COLOR_DATA_VERSION_8: &[(i32, &str)] = &[
    (12, "12 (1DXmkII/5DS/5DSR)"),
    (13, "13 (80D/5DmkIV)"),
    (14, "14 (1300D/2000D/4000D)"),
    (15, "15 (6DmkII/77D/200D/800D,9000D)"),
];
const COLOR_DATA_VERSION_9: &[(i32, &str)] = &[
    (16, "16 (M50)"),
    (17, "17 (R)"),
    (18, "18 (RP/250D)"),
    (19, "19 (90D/850D/M6mkII/M200)"),
];

/// `%Canon::ColorData1` -- ExifTool selects it when tag 0x4001 has 582 elements.
const COLOR_DATA_1: &[ColorDataField] = &[
    ColorDataField {
        offset: 0x19,
        name: "WB_RGGBLevelsAsShot",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x1d,
        name: "ColorTempAsShot",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x1e,
        name: "WB_RGGBLevelsAuto",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x22,
        name: "ColorTempAuto",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x23,
        name: "WB_RGGBLevelsDaylight",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x27,
        name: "ColorTempDaylight",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x28,
        name: "WB_RGGBLevelsShade",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x2c,
        name: "ColorTempShade",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x2d,
        name: "WB_RGGBLevelsCloudy",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x31,
        name: "ColorTempCloudy",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x32,
        name: "WB_RGGBLevelsTungsten",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x36,
        name: "ColorTempTungsten",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x37,
        name: "WB_RGGBLevelsFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x3b,
        name: "ColorTempFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x3c,
        name: "WB_RGGBLevelsFlash",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x40,
        name: "ColorTempFlash",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x41,
        name: "WB_RGGBLevelsCustom1",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x45,
        name: "ColorTempCustom1",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x46,
        name: "WB_RGGBLevelsCustom2",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x4a,
        name: "ColorTempCustom2",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
];

/// `%Canon::ColorData2` -- ExifTool selects it when tag 0x4001 has 653 elements.
const COLOR_DATA_2: &[ColorDataField] = &[
    ColorDataField {
        offset: 0x18,
        name: "WB_RGGBLevelsAuto",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x1c,
        name: "ColorTempAuto",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x22,
        name: "WB_RGGBLevelsAsShot",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x26,
        name: "ColorTempAsShot",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x27,
        name: "WB_RGGBLevelsDaylight",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x2b,
        name: "ColorTempDaylight",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x2c,
        name: "WB_RGGBLevelsShade",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x30,
        name: "ColorTempShade",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x31,
        name: "WB_RGGBLevelsCloudy",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x35,
        name: "ColorTempCloudy",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x36,
        name: "WB_RGGBLevelsTungsten",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x3a,
        name: "ColorTempTungsten",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x3b,
        name: "WB_RGGBLevelsFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x3f,
        name: "ColorTempFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x40,
        name: "WB_RGGBLevelsKelvin",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x44,
        name: "ColorTempKelvin",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x45,
        name: "WB_RGGBLevelsFlash",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x49,
        name: "ColorTempFlash",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x90,
        name: "WB_RGGBLevelsPC1",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x94,
        name: "ColorTempPC1",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x95,
        name: "WB_RGGBLevelsPC2",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x99,
        name: "ColorTempPC2",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x9a,
        name: "WB_RGGBLevelsPC3",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x9e,
        name: "ColorTempPC3",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x26a,
        name: "RawMeasuredRGGB",
        format: ColorFieldFormat::Int32u,
        count: 4,
        conv: ColorFieldConv::SwapWords,
        versions: &[],
    },
];

/// `%Canon::ColorData3` -- ExifTool selects it when tag 0x4001 has 796 elements.
const COLOR_DATA_3: &[ColorDataField] = &[
    ColorDataField {
        offset: 0x0,
        name: "ColorDataVersion",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::VersionMap(COLOR_DATA_VERSION_3),
        versions: &[],
    },
    ColorDataField {
        offset: 0x3f,
        name: "WB_RGGBLevelsAsShot",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x43,
        name: "ColorTempAsShot",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x44,
        name: "WB_RGGBLevelsAuto",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x48,
        name: "ColorTempAuto",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x49,
        name: "WB_RGGBLevelsMeasured",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x4d,
        name: "ColorTempMeasured",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x4e,
        name: "WB_RGGBLevelsDaylight",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x52,
        name: "ColorTempDaylight",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x53,
        name: "WB_RGGBLevelsShade",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x57,
        name: "ColorTempShade",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x58,
        name: "WB_RGGBLevelsCloudy",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x5c,
        name: "ColorTempCloudy",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x5d,
        name: "WB_RGGBLevelsTungsten",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x61,
        name: "ColorTempTungsten",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x62,
        name: "WB_RGGBLevelsFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x66,
        name: "ColorTempFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x67,
        name: "WB_RGGBLevelsKelvin",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x6b,
        name: "ColorTempKelvin",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x6c,
        name: "WB_RGGBLevelsFlash",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x70,
        name: "ColorTempFlash",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x71,
        name: "WB_RGGBLevelsPC1",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x75,
        name: "ColorTempPC1",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x76,
        name: "WB_RGGBLevelsPC2",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x7a,
        name: "ColorTempPC2",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x7b,
        name: "WB_RGGBLevelsPC3",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x7f,
        name: "ColorTempPC3",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x80,
        name: "WB_RGGBLevelsCustom",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x84,
        name: "ColorTempCustom",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xc4,
        name: "PerChannelBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x248,
        name: "FlashOutput",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::FlashOutput,
        versions: &[],
    },
    ColorDataField {
        offset: 0x249,
        name: "FlashBatteryLevel",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::FlashBatteryLevel,
        versions: &[],
    },
    ColorDataField {
        offset: 0x24a,
        name: "ColorTempFlashData",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x287,
        name: "MeasuredRGGBData",
        format: ColorFieldFormat::Int32u,
        count: 4,
        conv: ColorFieldConv::SwapWords,
        versions: &[],
    },
];

/// `%Canon::ColorData4` -- ExifTool selects it when tag 0x4001 has 692, 674, 702, 1227, 1250, 1251, 1337, 1338, 1346 elements.
const COLOR_DATA_4: &[ColorDataField] = &[
    ColorDataField {
        offset: 0x0,
        name: "ColorDataVersion",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::VersionMap(COLOR_DATA_VERSION_4),
        versions: &[],
    },
    ColorDataField {
        offset: 0x3f,
        name: "WB_RGGBLevelsAsShot",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x43,
        name: "ColorTempAsShot",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x44,
        name: "WB_RGGBLevelsAuto",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x48,
        name: "ColorTempAuto",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x49,
        name: "WB_RGGBLevelsMeasured",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x4d,
        name: "ColorTempMeasured",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x53,
        name: "WB_RGGBLevelsDaylight",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x57,
        name: "ColorTempDaylight",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x58,
        name: "WB_RGGBLevelsShade",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x5c,
        name: "ColorTempShade",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x5d,
        name: "WB_RGGBLevelsCloudy",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x61,
        name: "ColorTempCloudy",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x62,
        name: "WB_RGGBLevelsTungsten",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x66,
        name: "ColorTempTungsten",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x67,
        name: "WB_RGGBLevelsFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x6b,
        name: "ColorTempFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x6c,
        name: "WB_RGGBLevelsKelvin",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x70,
        name: "ColorTempKelvin",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x71,
        name: "WB_RGGBLevelsFlash",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x75,
        name: "ColorTempFlash",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xe7,
        name: "AverageBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x26b,
        name: "FlashOutput",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::FlashOutput,
        versions: &[],
    },
    ColorDataField {
        offset: 0x26c,
        name: "FlashBatteryLevel",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::FlashBatteryLevel,
        versions: &[],
    },
    ColorDataField {
        offset: 0x280,
        name: "RawMeasuredRGGB",
        format: ColorFieldFormat::Int32u,
        count: 4,
        conv: ColorFieldConv::SwapWords,
        versions: &[],
    },
    ColorDataField {
        offset: 0x2b4,
        name: "PerChannelBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(4), ColorVersionTest::Eq(5)],
    },
    ColorDataField {
        offset: 0x2b8,
        name: "NormalWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::DropZero,
        versions: &[ColorVersionTest::Eq(4), ColorVersionTest::Eq(5)],
    },
    ColorDataField {
        offset: 0x2b9,
        name: "SpecularWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(4), ColorVersionTest::Eq(5)],
    },
    ColorDataField {
        offset: 0x2ba,
        name: "LinearityUpperMargin",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(4), ColorVersionTest::Eq(5)],
    },
    ColorDataField {
        offset: 0x2cb,
        name: "PerChannelBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(6), ColorVersionTest::Eq(7)],
    },
    ColorDataField {
        offset: 0x2cf,
        name: "NormalWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::DropZero,
        versions: &[ColorVersionTest::Eq(6), ColorVersionTest::Eq(7)],
    },
    ColorDataField {
        offset: 0x2cf,
        name: "PerChannelBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(9)],
    },
    ColorDataField {
        offset: 0x2d0,
        name: "SpecularWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(6), ColorVersionTest::Eq(7)],
    },
    ColorDataField {
        offset: 0x2d1,
        name: "LinearityUpperMargin",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(6), ColorVersionTest::Eq(7)],
    },
    ColorDataField {
        offset: 0x2d3,
        name: "NormalWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::DropZero,
        versions: &[ColorVersionTest::Eq(9)],
    },
    ColorDataField {
        offset: 0x2d4,
        name: "SpecularWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(9)],
    },
    ColorDataField {
        offset: 0x2d5,
        name: "LinearityUpperMargin",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(9)],
    },
];

/// `%Canon::ColorData5` -- ExifTool selects it when tag 0x4001 has 5120 elements.
const COLOR_DATA_5: &[ColorDataField] = &[
    ColorDataField {
        offset: 0x0,
        name: "ColorDataVersion",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::VersionMap(COLOR_DATA_VERSION_5),
        versions: &[],
    },
    ColorDataField {
        offset: 0x47,
        name: "WB_RGGBLevelsAsShot",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x47,
        name: "WB_RGGBLevelsAsShot",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x4b,
        name: "ColorTempAsShot",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x4c,
        name: "WB_RGGBLevelsAuto",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x4e,
        name: "ColorTempAsShot",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x4f,
        name: "WB_RGGBLevelsAuto",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x50,
        name: "ColorTempAuto",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x51,
        name: "WB_RGGBLevelsMeasured",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x55,
        name: "ColorTempMeasured",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x56,
        name: "ColorTempAuto",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x57,
        name: "WB_RGGBLevelsMeasured",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x5b,
        name: "WB_RGGBLevelsDaylight",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x5e,
        name: "ColorTempMeasured",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x5f,
        name: "ColorTempDaylight",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x60,
        name: "WB_RGGBLevelsShade",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x64,
        name: "ColorTempShade",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x65,
        name: "WB_RGGBLevelsCloudy",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x67,
        name: "WB_RGGBLevelsDaylight",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x69,
        name: "ColorTempCloudy",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x6a,
        name: "WB_RGGBLevelsTungsten",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x6e,
        name: "ColorTempDaylight",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x6e,
        name: "ColorTempTungsten",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x6f,
        name: "WB_RGGBLevelsFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x6f,
        name: "WB_RGGBLevelsShade",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x73,
        name: "ColorTempFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x74,
        name: "WB_RGGBLevelsKelvin",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x76,
        name: "ColorTempShade",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x77,
        name: "WB_RGGBLevelsCloudy",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x78,
        name: "ColorTempKelvin",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x79,
        name: "WB_RGGBLevelsFlash",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x7d,
        name: "ColorTempFlash",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x7e,
        name: "ColorTempCloudy",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x7f,
        name: "WB_RGGBLevelsTungsten",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x86,
        name: "ColorTempTungsten",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x87,
        name: "WB_RGGBLevelsFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x8e,
        name: "ColorTempFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x8f,
        name: "WB_RGGBLevelsKelvin",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x96,
        name: "ColorTempKelvin",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x97,
        name: "WB_RGGBLevelsFlash",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x9e,
        name: "ColorTempFlash",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x108,
        name: "PerChannelBlackLevel",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x14d,
        name: "PerChannelBlackLevel",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x296,
        name: "SpecularWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-3)],
    },
    ColorDataField {
        offset: 0x569,
        name: "NormalWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
    ColorDataField {
        offset: 0x56a,
        name: "SpecularWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(-4)],
    },
];

/// `%Canon::ColorData6` -- ExifTool selects it when tag 0x4001 has 1273, 1275 elements.
const COLOR_DATA_6: &[ColorDataField] = &[
    ColorDataField {
        offset: 0x0,
        name: "ColorDataVersion",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::VersionMap(COLOR_DATA_VERSION_6),
        versions: &[],
    },
    ColorDataField {
        offset: 0x3f,
        name: "WB_RGGBLevelsAsShot",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x43,
        name: "ColorTempAsShot",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x44,
        name: "WB_RGGBLevelsAuto",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x48,
        name: "ColorTempAuto",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x49,
        name: "WB_RGGBLevelsMeasured",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x4d,
        name: "ColorTempMeasured",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x67,
        name: "WB_RGGBLevelsDaylight",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x6b,
        name: "ColorTempDaylight",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x6c,
        name: "WB_RGGBLevelsShade",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x70,
        name: "ColorTempShade",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x71,
        name: "WB_RGGBLevelsCloudy",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x75,
        name: "ColorTempCloudy",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x76,
        name: "WB_RGGBLevelsTungsten",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x7a,
        name: "ColorTempTungsten",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x7b,
        name: "WB_RGGBLevelsFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x7f,
        name: "ColorTempFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x80,
        name: "WB_RGGBLevelsKelvin",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x84,
        name: "ColorTempKelvin",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x85,
        name: "WB_RGGBLevelsFlash",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x89,
        name: "ColorTempFlash",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xfb,
        name: "AverageBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x194,
        name: "RawMeasuredRGGB",
        format: ColorFieldFormat::Int32u,
        count: 4,
        conv: ColorFieldConv::SwapWords,
        versions: &[],
    },
    ColorDataField {
        offset: 0x1df,
        name: "PerChannelBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x1e3,
        name: "NormalWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::DropZero,
        versions: &[],
    },
    ColorDataField {
        offset: 0x1e4,
        name: "SpecularWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x1e5,
        name: "LinearityUpperMargin",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
];

/// `%Canon::ColorData7` -- ExifTool selects it when tag 0x4001 has 1312, 1313, 1316, 1506 elements.
const COLOR_DATA_7: &[ColorDataField] = &[
    ColorDataField {
        offset: 0x0,
        name: "ColorDataVersion",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::VersionMap(COLOR_DATA_VERSION_7),
        versions: &[],
    },
    ColorDataField {
        offset: 0x3f,
        name: "WB_RGGBLevelsAsShot",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x43,
        name: "ColorTempAsShot",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x44,
        name: "WB_RGGBLevelsAuto",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x48,
        name: "ColorTempAuto",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x49,
        name: "WB_RGGBLevelsMeasured",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x4d,
        name: "ColorTempMeasured",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x80,
        name: "WB_RGGBLevelsDaylight",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x84,
        name: "ColorTempDaylight",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x85,
        name: "WB_RGGBLevelsShade",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x89,
        name: "ColorTempShade",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x8a,
        name: "WB_RGGBLevelsCloudy",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x8e,
        name: "ColorTempCloudy",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x8f,
        name: "WB_RGGBLevelsTungsten",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x93,
        name: "ColorTempTungsten",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x94,
        name: "WB_RGGBLevelsFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x98,
        name: "ColorTempFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x99,
        name: "WB_RGGBLevelsKelvin",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x9d,
        name: "ColorTempKelvin",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x9e,
        name: "WB_RGGBLevelsFlash",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xa2,
        name: "ColorTempFlash",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x114,
        name: "AverageBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x198,
        name: "FlashOutput",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::FlashOutput,
        versions: &[],
    },
    ColorDataField {
        offset: 0x199,
        name: "FlashBatteryLevel",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::FlashBatteryLevel,
        versions: &[],
    },
    ColorDataField {
        offset: 0x1ad,
        name: "RawMeasuredRGGB",
        format: ColorFieldFormat::Int32u,
        count: 4,
        conv: ColorFieldConv::SwapWords,
        versions: &[ColorVersionTest::Eq(10)],
    },
    ColorDataField {
        offset: 0x1f8,
        name: "PerChannelBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(10)],
    },
    ColorDataField {
        offset: 0x1fc,
        name: "NormalWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::DropZero,
        versions: &[ColorVersionTest::Eq(10)],
    },
    ColorDataField {
        offset: 0x1fd,
        name: "SpecularWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(10)],
    },
    ColorDataField {
        offset: 0x1fe,
        name: "LinearityUpperMargin",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(10)],
    },
    ColorDataField {
        offset: 0x26b,
        name: "RawMeasuredRGGB",
        format: ColorFieldFormat::Int32u,
        count: 4,
        conv: ColorFieldConv::SwapWords,
        versions: &[ColorVersionTest::Eq(11)],
    },
    ColorDataField {
        offset: 0x2d8,
        name: "PerChannelBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(11)],
    },
    ColorDataField {
        offset: 0x2dc,
        name: "NormalWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::DropZero,
        versions: &[ColorVersionTest::Eq(11)],
    },
    ColorDataField {
        offset: 0x2dd,
        name: "SpecularWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(11)],
    },
    ColorDataField {
        offset: 0x2de,
        name: "LinearityUpperMargin",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(11)],
    },
];

/// `%Canon::ColorData8` -- ExifTool selects it when tag 0x4001 has 1560, 1592, 1353, 1602 elements.
const COLOR_DATA_8: &[ColorDataField] = &[
    ColorDataField {
        offset: 0x0,
        name: "ColorDataVersion",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::VersionMap(COLOR_DATA_VERSION_8),
        versions: &[],
    },
    ColorDataField {
        offset: 0x3f,
        name: "WB_RGGBLevelsAsShot",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x43,
        name: "ColorTempAsShot",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x44,
        name: "WB_RGGBLevelsAuto",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x48,
        name: "ColorTempAuto",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x49,
        name: "WB_RGGBLevelsMeasured",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x4d,
        name: "ColorTempMeasured",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x85,
        name: "WB_RGGBLevelsDaylight",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x89,
        name: "ColorTempDaylight",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x8a,
        name: "WB_RGGBLevelsShade",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x8e,
        name: "ColorTempShade",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x8f,
        name: "WB_RGGBLevelsCloudy",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x93,
        name: "ColorTempCloudy",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x94,
        name: "WB_RGGBLevelsTungsten",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x98,
        name: "ColorTempTungsten",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x99,
        name: "WB_RGGBLevelsFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x9d,
        name: "ColorTempFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x9e,
        name: "WB_RGGBLevelsKelvin",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xa2,
        name: "ColorTempKelvin",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xa3,
        name: "WB_RGGBLevelsFlash",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xa7,
        name: "ColorTempFlash",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x146,
        name: "AverageBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x22c,
        name: "PerChannelBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(14)],
    },
    ColorDataField {
        offset: 0x230,
        name: "NormalWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::DropZero,
        versions: &[ColorVersionTest::Eq(14)],
    },
    ColorDataField {
        offset: 0x231,
        name: "SpecularWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(14)],
    },
    ColorDataField {
        offset: 0x232,
        name: "LinearityUpperMargin",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Eq(14)],
    },
    ColorDataField {
        offset: 0x30a,
        name: "PerChannelBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Lt(14), ColorVersionTest::Eq(15)],
    },
    ColorDataField {
        offset: 0x30e,
        name: "NormalWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::DropZero,
        versions: &[ColorVersionTest::Lt(14), ColorVersionTest::Eq(15)],
    },
    ColorDataField {
        offset: 0x30f,
        name: "SpecularWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Lt(14), ColorVersionTest::Eq(15)],
    },
    ColorDataField {
        offset: 0x310,
        name: "LinearityUpperMargin",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[ColorVersionTest::Lt(14), ColorVersionTest::Eq(15)],
    },
];

/// `%Canon::ColorData9` -- ExifTool selects it when tag 0x4001 has 1816, 1820, 1824 elements.
const COLOR_DATA_9: &[ColorDataField] = &[
    ColorDataField {
        offset: 0x0,
        name: "ColorDataVersion",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::VersionMap(COLOR_DATA_VERSION_9),
        versions: &[],
    },
    ColorDataField {
        offset: 0x47,
        name: "WB_RGGBLevelsAsShot",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x4b,
        name: "ColorTempAsShot",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x4c,
        name: "WB_RGGBLevelsAuto",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x50,
        name: "ColorTempAuto",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x51,
        name: "WB_RGGBLevelsMeasured",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x55,
        name: "ColorTempMeasured",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x88,
        name: "WB_RGGBLevelsDaylight",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x8c,
        name: "ColorTempDaylight",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x8d,
        name: "WB_RGGBLevelsShade",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x91,
        name: "ColorTempShade",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x92,
        name: "WB_RGGBLevelsCloudy",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x96,
        name: "ColorTempCloudy",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x97,
        name: "WB_RGGBLevelsTungsten",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x9b,
        name: "ColorTempTungsten",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x9c,
        name: "WB_RGGBLevelsFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xa0,
        name: "ColorTempFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xa1,
        name: "WB_RGGBLevelsKelvin",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xa5,
        name: "ColorTempKelvin",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xa6,
        name: "WB_RGGBLevelsFlash",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xaa,
        name: "ColorTempFlash",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x149,
        name: "PerChannelBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x31c,
        name: "NormalWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::DropZero,
        versions: &[],
    },
    ColorDataField {
        offset: 0x31d,
        name: "SpecularWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x31e,
        name: "LinearityUpperMargin",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
];

/// `%Canon::ColorData10` -- ExifTool selects it when tag 0x4001 has 2024, 3656 elements.
const COLOR_DATA_10: &[ColorDataField] = &[
    ColorDataField {
        offset: 0x0,
        name: "ColorDataVersion",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::VersionMap(COLOR_DATA_VERSION_10),
        versions: &[],
    },
    ColorDataField {
        offset: 0x55,
        name: "WB_RGGBLevelsAsShot",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x59,
        name: "ColorTempAsShot",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x5a,
        name: "WB_RGGBLevelsAuto",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x5e,
        name: "ColorTempAuto",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x5f,
        name: "WB_RGGBLevelsMeasured",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x63,
        name: "ColorTempMeasured",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x96,
        name: "WB_RGGBLevelsDaylight",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x9a,
        name: "ColorTempDaylight",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x9b,
        name: "WB_RGGBLevelsShade",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x9f,
        name: "ColorTempShade",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xa0,
        name: "WB_RGGBLevelsCloudy",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xa4,
        name: "ColorTempCloudy",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xa5,
        name: "WB_RGGBLevelsTungsten",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xa9,
        name: "ColorTempTungsten",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xaa,
        name: "WB_RGGBLevelsFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xae,
        name: "ColorTempFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xaf,
        name: "WB_RGGBLevelsKelvin",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xb3,
        name: "ColorTempKelvin",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xb4,
        name: "WB_RGGBLevelsFlash",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xb8,
        name: "ColorTempFlash",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x157,
        name: "PerChannelBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x299,
        name: "FlashOutput",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::FlashOutput,
        versions: &[],
    },
    ColorDataField {
        offset: 0x29a,
        name: "FlashBatteryLevel",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::FlashBatteryLevel,
        versions: &[],
    },
    ColorDataField {
        offset: 0x32a,
        name: "NormalWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::DropZero,
        versions: &[],
    },
    ColorDataField {
        offset: 0x32b,
        name: "SpecularWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x32c,
        name: "LinearityUpperMargin",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
];

/// `%Canon::ColorData11` -- ExifTool selects it when tag 0x4001 has 3973 elements.
const COLOR_DATA_11: &[ColorDataField] = &[
    ColorDataField {
        offset: 0x0,
        name: "ColorDataVersion",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::VersionMap(COLOR_DATA_VERSION_11),
        versions: &[],
    },
    ColorDataField {
        offset: 0x69,
        name: "WB_RGGBLevelsAsShot",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x6d,
        name: "ColorTempAsShot",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x6e,
        name: "WB_RGGBLevelsAuto",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x72,
        name: "ColorTempAuto",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x73,
        name: "WB_RGGBLevelsMeasured",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x77,
        name: "ColorTempMeasured",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xcd,
        name: "WB_RGGBLevelsDaylight",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xd1,
        name: "ColorTempDaylight",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xd2,
        name: "WB_RGGBLevelsShade",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xd6,
        name: "ColorTempShade",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xd7,
        name: "WB_RGGBLevelsCloudy",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xdb,
        name: "ColorTempCloudy",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xdc,
        name: "WB_RGGBLevelsTungsten",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xe0,
        name: "ColorTempTungsten",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xe1,
        name: "WB_RGGBLevelsFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xe5,
        name: "ColorTempFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xe6,
        name: "WB_RGGBLevelsKelvin",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xea,
        name: "ColorTempKelvin",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xeb,
        name: "WB_RGGBLevelsFlash",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0xef,
        name: "ColorTempFlash",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x16b,
        name: "PerChannelBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x280,
        name: "NormalWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::DropZero,
        versions: &[],
    },
    ColorDataField {
        offset: 0x281,
        name: "SpecularWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x282,
        name: "LinearityUpperMargin",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
];

/// `%Canon::ColorData12` -- ExifTool selects it when tag 0x4001 has 4528, 3778 elements.
const COLOR_DATA_12: &[ColorDataField] = &[
    ColorDataField {
        offset: 0x0,
        name: "ColorDataVersion",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::VersionMap(COLOR_DATA_VERSION_12),
        versions: &[],
    },
    ColorDataField {
        offset: 0x69,
        name: "WB_RGGBLevelsAsShot",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x6d,
        name: "ColorTempAsShot",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x6e,
        name: "WB_RGGBLevelsDaylight",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x72,
        name: "ColorTempDaylight",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x73,
        name: "WB_RGGBLevelsShade",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x77,
        name: "ColorTempShade",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x78,
        name: "WB_RGGBLevelsCloudy",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x7c,
        name: "ColorTempCloudy",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x7d,
        name: "WB_RGGBLevelsTungsten",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x81,
        name: "ColorTempTungsten",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x82,
        name: "WB_RGGBLevelsFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x86,
        name: "ColorTempFluorescent",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x87,
        name: "WB_RGGBLevelsFlash",
        format: ColorFieldFormat::Int16s,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x8b,
        name: "ColorTempFlash",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x17f,
        name: "PerChannelBlackLevel",
        format: ColorFieldFormat::Int16u,
        count: 4,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x203,
        name: "FlashOutput",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::FlashOutput,
        versions: &[],
    },
    ColorDataField {
        offset: 0x204,
        name: "FlashBatteryLevel",
        format: ColorFieldFormat::Int16s,
        count: 1,
        conv: ColorFieldConv::FlashBatteryLevel,
        versions: &[],
    },
    ColorDataField {
        offset: 0x294,
        name: "NormalWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::DropZero,
        versions: &[],
    },
    ColorDataField {
        offset: 0x295,
        name: "SpecularWhiteLevel",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
    ColorDataField {
        offset: 0x296,
        name: "LinearityUpperMargin",
        format: ColorFieldFormat::Int16u,
        count: 1,
        conv: ColorFieldConv::None,
        versions: &[],
    },
];

/// Chooses the `%Canon::ColorData*` table for a record, following Canon.pm:1972.
///
/// Selection is by element count, except that count 3778 is shared: ExifTool's
/// `ColorData11` arm carries the extra guard `$$valPt =~ /^[\0-\x40]/`, a regex on the
/// raw byte string testing whether its first byte -- the version word's low byte -- falls
/// in 0..=0x40 (0..=64). `ColorData12` has no such guard, so it is the fallthrough for
/// every other 3778-element record. This is a `<=`, not a `!=`: a 3778-element record with
/// version 48 (R7/R10/R50/R6mkII) stays on `ColorData11`, while version 65 (R50V) *or any
/// higher, undocumented version* (e.g. 66, the R6mkIII) falls through to `ColorData12`.
fn select_color_data_table(
    element_count: usize,
    version: i32,
) -> Option<&'static [ColorDataField]> {
    let table = match element_count {
        582 => COLOR_DATA_1,
        653 => COLOR_DATA_2,
        796 => COLOR_DATA_3,
        692 | 674 | 702 | 1227 | 1250 | 1251 | 1337 | 1338 | 1346 => COLOR_DATA_4,
        5120 => COLOR_DATA_5,
        1273 | 1275 => COLOR_DATA_6,
        1312 | 1313 | 1316 | 1506 => COLOR_DATA_7,
        1560 | 1592 | 1353 | 1602 => COLOR_DATA_8,
        1816 | 1820 | 1824 => COLOR_DATA_9,
        2024 | 3656 => COLOR_DATA_10,
        3973 => COLOR_DATA_11,
        3778 if version <= 0x40 => COLOR_DATA_11,
        3778 | 4528 => COLOR_DATA_12,
        // ExifTool's `ColorDataUnknown` arm: the record is real but its layout is not
        // documented, and it defines no extractable tags.
        _ => return None,
    };
    Some(table)
}

/// Reads one field's values out of the record, or `None` if it runs past the end.
fn read_field(record: &[i16], field: &ColorDataField, byte_order: ByteOrder) -> Option<Vec<i64>> {
    let mut values = Vec::with_capacity(field.count);
    for index in 0..field.count {
        match field.format {
            ColorFieldFormat::Int16s => {
                values.push(i64::from(*record.get(field.offset + index)?));
            }
            ColorFieldFormat::Int16u => {
                values.push(i64::from(*record.get(field.offset + index)? as u16));
            }
            ColorFieldFormat::Int32u | ColorFieldFormat::Int32s => {
                let word_index = field.offset + index * 2;
                let low = u32::from(*record.get(word_index)? as u16);
                let high = u32::from(*record.get(word_index + 1)? as u16);
                // The words were decoded with the file's byte order, so recombining them
                // has to follow that same order to rebuild the 32-bit value.
                let combined = match byte_order {
                    ByteOrder::LittleEndian => low | (high << 16),
                    ByteOrder::BigEndian => (low << 16) | high,
                };
                values.push(match field.format {
                    ColorFieldFormat::Int32s => i64::from(combined as i32),
                    _ => i64::from(combined),
                });
            }
        }
    }
    Some(values)
}

/// Renders one field's values the way ExifTool's ValueConv/PrintConv pair does.
fn render_field(field: &ColorDataField, values: &[i64]) -> Option<String> {
    match field.conv {
        ColorFieldConv::None => Some(join_values(values)),
        ColorFieldConv::DropZero => {
            if values.iter().all(|&value| value == 0) {
                None
            } else {
                Some(join_values(values))
            }
        }
        ColorFieldConv::VersionMap(table) => {
            let value = i32::try_from(*values.first()?).ok()?;
            Some(
                table
                    .iter()
                    .find(|(key, _)| *key == value)
                    .map(|(_, label)| (*label).to_string())
                    .unwrap_or_else(|| format!("Unknown ({})", value)),
            )
        }
        ColorFieldConv::SwapWords => Some(join_values(
            &values
                .iter()
                .map(|&value| {
                    let raw = value as u32;
                    i64::from((raw >> 16) | (raw << 16))
                })
                .collect::<Vec<_>>(),
        )),
        ColorFieldConv::FlashOutput => {
            let raw = *values.first()?;
            if raw >= 255 {
                return Some("Strobe or Misfire".to_string());
            }
            let output = 2.0_f64.powf((raw as f64 - 200.0) / 16.0);
            Some(format!("{:.0}%", output * 100.0))
        }
        ColorFieldConv::FlashBatteryLevel => {
            let raw = *values.first()?;
            if raw == 0 {
                return Some("n/a".to_string());
            }
            Some(format!("{:.2}V", raw as f64 * 5.0 / 186.0))
        }
    }
}

fn join_values(values: &[i64]) -> String {
    values
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Decodes a Canon `ColorData` record (MakerNote tag 0x4001) into `Canon:`-prefixed tags.
///
/// `record` is the raw `int16` array exactly as stored -- ExifTool indexes it from 0 with
/// no leading size word, so it must NOT be run through the length-prefixed realignment the
/// `FIRST_ENTRY => 1` records need.
pub(crate) fn parse_color_data(
    record: &[i16],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let Some(&version_word) = record.first() else {
        return;
    };
    let version = i32::from(version_word);
    let Some(table) = select_color_data_table(record.len(), version) else {
        return;
    };

    for field in table {
        if !field.versions.is_empty() && !field.versions.iter().any(|test| test.matches(version)) {
            continue;
        }
        let Some(values) = read_field(record, field, byte_order) else {
            continue;
        };
        if let Some(rendered) = render_field(field, &values) {
            tags.insert(format!("Canon:{}", field.name), rendered);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a `ColorData` record of `elements` words with `version` at index 0.
    fn record(elements: usize, version: i16) -> Vec<i16> {
        let mut record = vec![0i16; elements];
        record[0] = version;
        record
    }

    /// ExifTool selects the table by element count (Canon.pm:1972). A count it does not
    /// list is `ColorDataUnknown`, which defines no extractable tags.
    #[test]
    fn test_table_selection_by_element_count() {
        assert!(select_color_data_table(582, 0).is_some()); // ColorData1
        assert!(select_color_data_table(796, 1).is_some()); // ColorData3
        assert!(select_color_data_table(1251, 7).is_some()); // ColorData4
        assert!(select_color_data_table(3656, 33).is_some()); // ColorData10
        assert!(select_color_data_table(999, 0).is_none());
    }

    /// Count 3778 is shared. ExifTool's ColorData11 arm carries the extra guard
    /// `$$valPt =~ /^[\0-\x40]/`, a byte-range test (0..=64), not an inequality against
    /// 0x41 alone: version 48 (R7/R10/R50/R6mkII) stays on ColorData11, while version
    /// 0x41 (65, R50V) falls through to ColorData12.
    #[test]
    fn test_count_3778_splits_on_version_word() {
        let eleven = select_color_data_table(3778, 48).expect("table for version 48");
        let twelve = select_color_data_table(3778, 0x41).expect("table for version 65");
        assert!(!std::ptr::eq(eleven, twelve));
        assert!(std::ptr::eq(
            twelve,
            select_color_data_table(4528, 64).expect("4528 is ColorData12")
        ));
        assert!(std::ptr::eq(
            eleven,
            select_color_data_table(3973, 34).expect("3973 is ColorData11")
        ));
    }

    /// Regression for the R6mkIII (oxidex/goofy-hopper-712519): its ColorDataVersion is
    /// 66, an undocumented value ExifTool's own PrintConv doesn't name either -- but
    /// ExifTool's guard is `version <= 0x40`, not `version != 0x41`, so any *unseen*
    /// version above 65 must still fall through to ColorData12, not stay on ColorData11.
    #[test]
    fn test_count_3778_undocumented_version_above_65_selects_color_data_12() {
        let twelve = select_color_data_table(3778, 66).expect("table for version 66");
        let expected = select_color_data_table(3778, 0x41).expect("table for version 65");
        assert!(std::ptr::eq(twelve, expected));
    }

    #[test]
    fn test_color_data_version_is_named() {
        let mut tags = HashMap::new();
        parse_color_data(&record(1251, 7), ByteOrder::LittleEndian, &mut tags);
        assert_eq!(
            tags.get("Canon:ColorDataVersion"),
            Some(&"7 (500D/550D/7D/1DmkIV)".to_string())
        );
    }

    /// ColorData4 stores `SpecularWhiteLevel` at three different offsets depending on
    /// the `ColorDataVersion`: 0x2b9 for versions 4-5, 0x2d0 for 6-7, 0x2d4 for 9. Each
    /// is gated by its own ExifTool `Condition`, so exactly one applies to a given body
    /// and reading the wrong one would report an unrelated word as a white level.
    #[test]
    fn test_version_gates_pick_one_offset_per_body() {
        for (version, offset) in [(4usize, 0x2b9usize), (7, 0x2d0), (9, 0x2d4)] {
            let mut raw = record(1251, version as i16);
            // Fill all three candidate slots with distinguishable values.
            raw[0x2b9] = 4004;
            raw[0x2d0] = 7007;
            raw[0x2d4] = 9009;
            let expected = raw[offset].to_string();
            let mut tags = HashMap::new();
            parse_color_data(&raw, ByteOrder::LittleEndian, &mut tags);
            assert_eq!(
                tags.get("Canon:SpecularWhiteLevel"),
                Some(&expected),
                "ColorDataVersion {version} should read offset {offset:#x}"
            );
        }
    }

    /// `RawMeasuredRGGB` is `int32u[4]` written with big-endian word order opposite to
    /// its byte order, so ExifTool swaps the halves (`\&SwapWords`). Words taken from
    /// CanonEOS_REBEL_T1i.jpg, where ExifTool reports `78407 116200 113862 57296`.
    #[test]
    fn test_swap_words_matches_exiftool() {
        let mut raw = record(1251, 7);
        for (slot, word) in [1i16, 12871, 1, -14872, 1, -17210, 0, -8240]
            .into_iter()
            .enumerate()
        {
            raw[0x280 + slot] = word;
        }
        let mut tags = HashMap::new();
        parse_color_data(&raw, ByteOrder::LittleEndian, &mut tags);
        assert_eq!(
            tags.get("Canon:RawMeasuredRGGB"),
            Some(&"78407 116200 113862 57296".to_string())
        );
    }

    /// `$val >= 255 ? 255 : exp(($val-200)/16*log(2))` then
    /// `$val == 255 ? "Strobe or Misfire" : sprintf("%.0f%%", $val * 100)`.
    #[test]
    fn test_flash_output_and_battery_level() {
        let mut raw = record(1251, 7);
        raw[0x26b] = 0;
        raw[0x26c] = 0;
        let mut tags = HashMap::new();
        parse_color_data(&raw, ByteOrder::LittleEndian, &mut tags);
        assert_eq!(tags.get("Canon:FlashOutput"), Some(&"0%".to_string()));
        assert_eq!(
            tags.get("Canon:FlashBatteryLevel"),
            Some(&"n/a".to_string())
        );

        raw[0x26b] = 255;
        raw[0x26c] = 186;
        let mut tags = HashMap::new();
        parse_color_data(&raw, ByteOrder::LittleEndian, &mut tags);
        assert_eq!(
            tags.get("Canon:FlashOutput"),
            Some(&"Strobe or Misfire".to_string())
        );
        assert_eq!(
            tags.get("Canon:FlashBatteryLevel"),
            Some(&"5.00V".to_string())
        );
    }

    /// A record shorter than a field's offset must drop that field, not panic.
    #[test]
    fn test_short_record_is_safe() {
        let mut tags = HashMap::new();
        parse_color_data(&[7i16, 0, 0], ByteOrder::LittleEndian, &mut tags);
        assert!(tags.is_empty());
    }
}
