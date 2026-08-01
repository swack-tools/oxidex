//! Canon `CameraInfo` (MakerNote tag 0x000D) and `FilterInfo` (0x4024) tables.
//!
//! ExifTool models tag 0x0D as a list of 33 alternatives keyed on the camera
//! model, then on the record's declared format and element count -- see the
//! `0xd => [...]` list in `Canon.pm`.  Each alternative names its own
//! `%Canon::CameraInfo*` binary table, and every table puts its fields at
//! different byte offsets.  There is no length prefix and no version word to
//! vote on: the record is only interpretable once the body is known, which is
//! why this is a model dispatch and not a heuristic.
//!
//! This file is TRANSCRIBED BY SCRIPT from ExifTool's own in-memory Perl
//! hashes, not typed out by hand.  The transcriber refuses to emit anything it
//! cannot reproduce exactly -- a Format, RawConv, ValueConv, PrintConv,
//! Condition or Hook expression it has not seen is a hard error rather than a
//! silent approximation, because a plausible-looking wrong number under a real
//! ExifTool tag name is worse than no tag at all.
//!
//! Two mechanisms in these tables are worth naming:
//!
//! * **Firmware look-ahead.** Nine tables open with a hidden
//!   `FirmwareVersionLookAhead` field whose `RawConv` probes a handful of byte
//!   offsets for a `N.N.N` version string and records which one matched as
//!   `CanonFirm`. Later fields carry a `Hook` that shifts every subsequent
//!   offset by a firmware-dependent amount.  When no probe matches, ExifTool
//!   adds 0x10000 to the running offset, which pushes the next field past the
//!   end of the record and stops the walk -- reproduced here exactly.
//!
//! * **`PRIORITY => 0`.** Every CameraInfo table is priority zero, so a tag
//!   that any other Canon table also produces keeps that other value.  These
//!   fields therefore only fill gaps; see `merge_priority0`.

use std::collections::HashMap;

use crate::parsers::tiff::ifd_parser::ByteOrder;

/// A binary-table field format. `Int16uRev` is ExifTool's "reversed" 16-bit
/// integer: read with the opposite endianness to the rest of the record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Fmt {
    Int8u,
    Int8s,
    Int16u,
    Int16s,
    Int16uRev,
    Int32u,
    Int32s,
    /// `string[N]` - N bytes, truncated at the first NUL.
    Str(u32),
    /// `undef[N]` - N raw bytes.
    Undef(u32),
}

impl Fmt {
    pub(crate) const fn size(self) -> u32 {
        match self {
            Fmt::Int8u | Fmt::Int8s => 1,
            Fmt::Int16u | Fmt::Int16s | Fmt::Int16uRev => 2,
            Fmt::Int32u | Fmt::Int32s => 4,
            Fmt::Str(n) | Fmt::Undef(n) => n,
        }
    }
}

/// `RawConv` - runs on the raw value and can veto the field entirely.
#[derive(Clone, Copy)]
pub(crate) enum Rc {
    None,
    /// `$val ? $val : undef`
    SkipZero,
    /// `$val =~ /^\d+\.\d+\.\d+\s*$/ ? $val : undef`
    RequireVersionString,
    /// The `FirmwareVersionLookAhead` probe list: `(byte offset, CanonFirm)`.
    FirmwareProbe(&'static [(u32, u8)]),
}

/// `ValueConv`.
#[derive(Clone, Copy)]
pub(crate) enum Vc {
    None,
    Div100,
    Plus1,
    Minus1,
    Minus128,
    Ev8Minus6,
    CanonExposureTime,
    CanonFNumber,
    CanonIso,
    MacroMagnification,
    UnixTime,
    HexBytes,
    PowerShotIso,
    PowerShotFNumber,
    PowerShotExposureTime,
}

/// `PrintConv`.
#[derive(Clone, Copy)]
pub(crate) enum Pc {
    None,
    /// Hash lookup. The flag is ExifTool's `PrintHex`, which only changes how
    /// an unmatched value is rendered: `Unknown (0x1f)` rather than `Unknown (31)`.
    Map(&'static [(i64, &'static str)], bool),
    /// Hash lookup with `%psConv`'s `OTHER => sub { shift }` - an unmatched
    /// value is returned unchanged rather than wrapped in `Unknown (...)`.
    MapOrRaw(&'static [(i64, &'static str)]),
    /// Hash lookup with `%filterConv`'s `OTHER => sub { "On ($val)" }`.
    MapOrOn(&'static [(i64, &'static str)]),
    /// Hash lookup with `%printParameter`'s OTHER, `Exif::PrintParameter`,
    /// which prints a positive adjustment with its sign: 7 renders as "+7".
    MapOrSigned(&'static [(i64, &'static str)]),
    /// Hash lookup with a `BITMASK` fallback: `(exact matches, bit names)`.
    BitMask(
        &'static [(i64, &'static str)],
        &'static [(i64, &'static str)],
    ),
    Mm,
    FocusDistance,
    Celsius,
    ExposureTime,
    Sprintf2G,
    Sprintf0F,
    Sprintf1Fx,
    DateTime,
}

/// A field `Condition`.
#[derive(Clone, Copy)]
pub(crate) enum Cond {
    Always,
    /// `$$self{Model} =~ /\bLIT$/`
    ModelEndsWord(&'static str),
    /// `$$self{Model} =~ /(A|B|C)\b/`
    ModelHasWord(&'static [&'static str]),
    /// `$$self{Model} =~ /^LIT/`
    ModelStartsWith(&'static str),
    /// `$$self{LensType} and $$self{LensType} == N`
    LensTypeIs(i64),
    /// `$$self{CameraInfoCount} == N`
    CountEq(u32),
    CountEither(u32, u32),
    CountGreater(u32),
    /// `$$valPt =~ /^\d\.\d\.\d\0/`
    ValueLooksLikeVersion,
    /// `$$self{FileType} eq ...` - the MakerNote parser is handed the model but
    /// not the container file type, so these fields are never emitted. See the
    /// module note on `CameraInfoG5XII`.
    FileTypeUnavailable,
}

#[derive(Clone, Copy)]
pub(crate) enum Cmp {
    Lt,
    Gt,
    Eq,
    Ge,
    Le,
}

/// One `$varSize <op> N if $$self{CanonFirm} <cmp> M` statement from a `Hook`.
/// `zero_delta` is the arm ExifTool takes when `CanonFirm` is 0 (no firmware
/// string matched), which is written `($$self{CanonFirm} ? -4 : 0x10000)`.
#[derive(Clone, Copy)]
pub(crate) struct HookRule {
    pub cmp: Cmp,
    pub firm: u8,
    pub delta: i64,
    pub zero_delta: i64,
}

/// A nested binary table reachable from a CameraInfo field.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubTable {
    PSInfo,
    PSInfo2,
}

/// One transcribed table field. `idx` is the ExifTool table key: a byte offset
/// for `FORMAT => int8u` tables, an element index otherwise, and negative when
/// ExifTool counts from the end of the record.
pub(crate) struct F {
    pub idx: i64,
    pub name: &'static str,
    pub fmt: Option<Fmt>,
    pub cond: Cond,
    pub rc: Rc,
    pub vc: Vc,
    pub pc: Pc,
    pub hook: &'static [HookRule],
    pub mask: Option<i64>,
    pub sub: Option<SubTable>,
    pub unknown: bool,
    pub hidden: bool,
}

pub(crate) struct Table {
    pub name: &'static str,
    pub default_fmt: Fmt,
    pub fields: &'static [F],
}

#[rustfmt::skip]
static PC_FLASHMODEL_0: &[(i64, &str)] = &[
    (0, "n/a"),
    (4, "Speedlite 540EZ"),
    (5, "Speedlite 380EX"),
    (6, "Speedlite 550EX"),
    (8, "Speedlite ST-E2"),
    (9, "Speedlite MR-14EX"),
    (12, "Speedlite 580EX"),
    (13, "Speedlite 430EX"),
    (17, "Speedlite 580EX II"),
    (18, "Speedlite 430EX II"),
    (22, "Speedlite 600EX-RT"),
    (23, "Speedlite 600EX II-RT"),
    (24, "Speedlite 90EX"),
    (25, "Speedlite 430EX III-RT"),
    (31, "Speedlite EL-1 ver2"),
    (33, "Speedlite EL-5"),
    (34, "Speedlite EL-10"),
];

#[rustfmt::skip]
static PC_FLASHMETERINGMODE_1: &[(i64, &str)] = &[
    (0, "E-TTL"),
    (3, "TTL"),
    (4, "External Auto"),
    (5, "External Manual"),
    (6, "Off"),
];

#[rustfmt::skip]
static PC_CAMERAORIENTATION_2: &[(i64, &str)] = &[
    (0, "Horizontal (normal)"),
    (1, "Rotate 90 CW"),
    (2, "Rotate 270 CW"),
];

#[rustfmt::skip]
static PC_WHITEBALANCE_3: &[(i64, &str)] = &[
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

/// ExifTool's `%canonLensTypes`, shared with `%Canon::CameraSettings` key 22
/// and transcribed once in `lens_data::canon`.  ExifTool's own tables hold a
/// reference to the single hash rather than a copy, and so does this.
static PC_LENSTYPE_4: &[(i64, &str)] = &super::super::lens_data::canon::CANON_LENS_TYPES;

#[rustfmt::skip]
static PC_SHARPNESSFREQUENCY_5: &[(i64, &str)] = &[
    (0, "n/a"),
    (1, "Lowest"),
    (2, "Low"),
    (3, "Standard"),
    (4, "High"),
    (5, "Highest"),
];

#[rustfmt::skip]
static PC_PICTURESTYLE_6: &[(i64, &str)] = &[
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

#[rustfmt::skip]
static PC_FOCALTYPE_7: &[(i64, &str)] = &[
    (0, "Fixed"),
    (2, "Zoom"),
];

#[rustfmt::skip]
static PC_CANONIMAGESIZE_8: &[(i64, &str)] = &[
    (-1, "n/a"),
    (0, "Large"),
    (1, "Medium"),
    (2, "Small"),
    (5, "Medium 1"),
    (6, "Medium 2"),
    (7, "Medium 3"),
    (8, "Postcard"),
    (9, "Widescreen"),
    (10, "Medium Widescreen"),
    (14, "Small 1"),
    (15, "Small 2"),
    (16, "Small 3"),
    (128, "640x480 Movie"),
    (129, "Medium Movie"),
    (130, "Small Movie"),
    (137, "1280x720 Movie"),
    (142, "1920x1080 Movie"),
    (143, "4096x2160 Movie"),
];

#[rustfmt::skip]
static PC_SATURATION_9: &[(i64, &str)] = &[
    (0, "Normal"),
];

#[rustfmt::skip]
static PC_HIGHLIGHTTONEPRIORITY_10: &[(i64, &str)] = &[
    (0, "Off"),
    (1, "On"),
];

#[rustfmt::skip]
static PC_HIGHISONOISEREDUCTION_11: &[(i64, &str)] = &[
    (0, "Standard"),
    (1, "Low"),
    (2, "Strong"),
    (3, "Off"),
];

#[rustfmt::skip]
static PC_AFPOINTSINFOCUS5D_12: &[(i64, &str)] = &[
    (0, "(none)"),
];

#[rustfmt::skip]
static PC_AFPOINTSINFOCUS5D_BITS_13: &[(i64, &str)] = &[
    (0, "Center"),
    (1, "Top"),
    (2, "Bottom"),
    (3, "Upper-left"),
    (4, "Upper-right"),
    (5, "Lower-left"),
    (6, "Lower-right"),
    (7, "Left"),
    (8, "Right"),
    (9, "AI Servo1"),
    (10, "AI Servo2"),
    (11, "AI Servo3"),
    (12, "AI Servo4"),
    (13, "AI Servo5"),
    (14, "AI Servo6"),
];

#[rustfmt::skip]
static PC_FILTEREFFECTMONOCHROME_14: &[(i64, &str)] = &[
    (-559038737, "n/a"),
    (0, "None"),
    (1, "Yellow"),
    (2, "Orange"),
    (3, "Red"),
    (4, "Green"),
];

#[rustfmt::skip]
static PC_TONINGEFFECTMONOCHROME_15: &[(i64, &str)] = &[
    (-559038737, "n/a"),
    (0, "None"),
    (1, "Sepia"),
    (2, "Blue"),
    (3, "Purple"),
    (4, "Green"),
];

#[rustfmt::skip]
static PC_USERDEF1PICTURESTYLE_16: &[(i64, &str)] = &[
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
];

#[rustfmt::skip]
static PC_CAMERAPICTURESTYLE_17: &[(i64, &str)] = &[
    (33, "User Defined 1"),
    (34, "User Defined 2"),
    (35, "User Defined 3"),
    (129, "Standard"),
    (130, "Portrait"),
    (131, "Landscape"),
    (132, "Neutral"),
    (133, "Faithful"),
    (134, "Monochrome"),
];

#[rustfmt::skip]
static PC_GRAINYBWFILTER_18: &[(i64, &str)] = &[
    (-1, "Off"),
];

#[rustfmt::skip]
static PC_MINIATUREFILTERORIENTATION_19: &[(i64, &str)] = &[
    (0, "Horizontal"),
    (1, "Vertical"),
];

#[rustfmt::skip]
static PC_CONTRASTSTANDARD_20: &[(i64, &str)] = &[
    (-559038737, "n/a"),
];

#[rustfmt::skip]
static TBL_CAMERAINFO1000D: Table = Table {
    name: "CameraInfo1000D",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 19, name: "FlashModel", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FLASHMODEL_0, false), hook: &[], mask: Some(127), sub: None, unknown: false, hidden: false },
        F { idx: 21, name: "FlashMeteringMode", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FLASHMETERINGMODE_1, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 24, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 27, name: "MacroMagnification", fmt: None, cond: Cond::LensTypeIs(124), rc: Rc::None, vc: Vc::MacroMagnification, pc: Pc::Sprintf1Fx, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 29, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 48, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 67, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 69, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 111, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 115, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 226, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 228, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 230, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 267, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 311, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 323, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 615, name: "PictureStyleInfo", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo), unknown: false, hidden: false },
        F { idx: 2359, name: "LensModel", fmt: Some(Fmt::Str(64)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO1D: Table = Table {
    name: "CameraInfo1D",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 10, name: "FocalLength", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 13, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 14, name: "MinFocalLength", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 16, name: "MaxFocalLength", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 65, name: "SharpnessFrequency", fmt: None, cond: Cond::ModelEndsWord("1D"), rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_SHARPNESSFREQUENCY_5, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 66, name: "Sharpness", fmt: Some(Fmt::Int8s), cond: Cond::ModelEndsWord("1D"), rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 68, name: "WhiteBalance", fmt: None, cond: Cond::ModelEndsWord("1D"), rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 71, name: "SharpnessFrequency", fmt: None, cond: Cond::ModelEndsWord("1DS"), rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_SHARPNESSFREQUENCY_5, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 72, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::ModelEndsWord("1D"), rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 72, name: "Sharpness", fmt: Some(Fmt::Int8s), cond: Cond::ModelEndsWord("1DS"), rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 74, name: "WhiteBalance", fmt: None, cond: Cond::ModelEndsWord("1DS"), rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 75, name: "PictureStyle", fmt: None, cond: Cond::ModelEndsWord("1D"), rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_PICTURESTYLE_6, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 78, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::ModelEndsWord("1DS"), rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 81, name: "PictureStyle", fmt: None, cond: Cond::ModelEndsWord("1DS"), rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_PICTURESTYLE_6, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO1DX: Table = Table {
    name: "CameraInfo1DX",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 0, name: "FirmwareVersionLookAhead", fmt: Some(Fmt::Undef(651)), cond: Cond::Always, rc: Rc::FirmwareProbe(&[(625, 1), (633, 2), (640, 3), (645, 4)]), vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: true },
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 27, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[HookRule { cmp: Cmp::Lt, firm: 3, delta: -3, zero_delta: -3 }], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 35, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 125, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 140, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 142, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[HookRule { cmp: Cmp::Lt, firm: 3, delta: -4, zero_delta: -4 }, HookRule { cmp: Cmp::Eq, firm: 4, delta: 5, zero_delta: 5 }], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 188, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 192, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 244, name: "PictureStyle", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_PICTURESTYLE_6, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 423, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 425, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 427, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[HookRule { cmp: Cmp::Lt, firm: 2, delta: -8, zero_delta: 65536 }], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 640, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 720, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 732, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 1012, name: "PictureStyleInfo", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo2), unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO1DMKII: Table = Table {
    name: "CameraInfo1DmkII",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 9, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 12, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 17, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 19, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 45, name: "FocalType", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FOCALTYPE_7, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 54, name: "WhiteBalance", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 55, name: "ColorTemperature", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 57, name: "CanonImageSize", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CANONIMAGESIZE_8, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 102, name: "JPEGQuality", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 108, name: "PictureStyle", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_PICTURESTYLE_6, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 110, name: "Saturation", fmt: Some(Fmt::Int8s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrSigned(PC_SATURATION_9), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 111, name: "ColorTone", fmt: Some(Fmt::Int8s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrSigned(PC_SATURATION_9), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 114, name: "Sharpness", fmt: Some(Fmt::Int8s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 115, name: "Contrast", fmt: Some(Fmt::Int8s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrSigned(PC_SATURATION_9), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 117, name: "ISO", fmt: Some(Fmt::Str(5)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO1DMKIII: Table = Table {
    name: "CameraInfo1DmkIII",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 24, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 27, name: "MacroMagnification", fmt: None, cond: Cond::LensTypeIs(124), rc: Rc::None, vc: Vc::MacroMagnification, pc: Pc::Sprintf1Fx, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 29, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 48, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 67, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 69, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 94, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 98, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 134, name: "PictureStyle", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_PICTURESTYLE_6, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 273, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 275, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 277, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 310, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 370, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 374, name: "ShutterCount", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 382, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 682, name: "PictureStyleInfo", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo), unknown: false, hidden: false },
        F { idx: 1114, name: "TimeStamp1", fmt: Some(Fmt::Int32u), cond: Cond::ModelEndsWord("1D Mark III"), rc: Rc::SkipZero, vc: Vc::UnixTime, pc: Pc::DateTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 1118, name: "TimeStamp", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::UnixTime, pc: Pc::DateTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO1DMKIIN: Table = Table {
    name: "CameraInfo1DmkIIN",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 9, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 12, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 17, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 19, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 54, name: "WhiteBalance", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 55, name: "ColorTemperature", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 115, name: "PictureStyle", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_PICTURESTYLE_6, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 116, name: "Sharpness", fmt: Some(Fmt::Int8s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 117, name: "Contrast", fmt: Some(Fmt::Int8s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrSigned(PC_SATURATION_9), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 118, name: "Saturation", fmt: Some(Fmt::Int8s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrSigned(PC_SATURATION_9), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 119, name: "ColorTone", fmt: Some(Fmt::Int8s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrSigned(PC_SATURATION_9), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 121, name: "ISO", fmt: Some(Fmt::Str(5)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO1DMKIV: Table = Table {
    name: "CameraInfo1DmkIV",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 0, name: "FirmwareVersionLookAhead", fmt: Some(Fmt::Undef(509)), cond: Cond::Always, rc: Rc::FirmwareProbe(&[(488, 1), (493, 2)]), vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: true },
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 7, name: "HighlightTonePriority", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_HIGHLIGHTTONEPRIORITY_10, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 8, name: "MeasuredEV2", fmt: None, cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::Ev8Minus6, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 9, name: "MeasuredEV3", fmt: None, cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::Ev8Minus6, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 21, name: "FlashMeteringMode", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FLASHMETERINGMODE_1, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 25, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 30, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 53, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 84, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 86, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[HookRule { cmp: Cmp::Lt, firm: 2, delta: -1, zero_delta: 65536 }], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 120, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 124, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 335, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 337, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 339, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[HookRule { cmp: Cmp::Lt, firm: 2, delta: -4, zero_delta: -4 }], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 493, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 556, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 568, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 872, name: "PictureStyleInfo", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo), unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO40D: Table = Table {
    name: "CameraInfo40D",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 21, name: "FlashMeteringMode", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FLASHMETERINGMODE_1, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 24, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 27, name: "MacroMagnification", fmt: None, cond: Cond::LensTypeIs(124), rc: Rc::None, vc: Vc::MacroMagnification, pc: Pc::Sprintf1Fx, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 29, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 48, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 67, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 69, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 111, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 115, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 214, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 216, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 218, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 255, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 307, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 319, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 603, name: "PictureStyleInfo", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo), unknown: false, hidden: false },
        F { idx: 2347, name: "LensModel", fmt: Some(Fmt::Str(64)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO450D: Table = Table {
    name: "CameraInfo450D",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 21, name: "FlashMeteringMode", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FLASHMETERINGMODE_1, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 24, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 27, name: "MacroMagnification", fmt: None, cond: Cond::LensTypeIs(124), rc: Rc::None, vc: Vc::MacroMagnification, pc: Pc::Sprintf1Fx, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 29, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 48, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 67, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 69, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 111, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 115, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 222, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 263, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 271, name: "OwnerName", fmt: Some(Fmt::Str(32)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 307, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 319, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 611, name: "PictureStyleInfo", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo), unknown: false, hidden: false },
        F { idx: 2355, name: "LensModel", fmt: Some(Fmt::Str(64)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO500D: Table = Table {
    name: "CameraInfo500D",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 7, name: "HighlightTonePriority", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_HIGHLIGHTTONEPRIORITY_10, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 21, name: "FlashMeteringMode", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FLASHMETERINGMODE_1, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 25, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 30, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 49, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 80, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 82, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 115, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 119, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 171, name: "PictureStyle", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_PICTURESTYLE_6, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 188, name: "HighISONoiseReduction", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_HIGHISONOISEREDUCTION_11, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 190, name: "AutoLightingOptimizer", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_HIGHISONOISEREDUCTION_11, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 246, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 248, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 250, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 400, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::RequireVersionString, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 467, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 479, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 779, name: "PictureStyleInfo", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo), unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO50D: Table = Table {
    name: "CameraInfo50D",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 0, name: "FirmwareVersionLookAhead", fmt: Some(Fmt::Undef(356)), cond: Cond::Always, rc: Rc::FirmwareProbe(&[(346, 1), (350, 2)]), vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: true },
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 7, name: "HighlightTonePriority", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_HIGHLIGHTTONEPRIORITY_10, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 21, name: "FlashMeteringMode", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FLASHMETERINGMODE_1, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 25, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 30, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 49, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 80, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 82, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 111, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 115, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 167, name: "PictureStyle", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_PICTURESTYLE_6, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 189, name: "HighISONoiseReduction", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_HIGHISONOISEREDUCTION_11, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 191, name: "AutoLightingOptimizer", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_HIGHISONOISEREDUCTION_11, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 234, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 236, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 238, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[HookRule { cmp: Cmp::Lt, firm: 2, delta: -4, zero_delta: 65536 }], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 350, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 411, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 423, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 727, name: "PictureStyleInfo", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo), unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO550D: Table = Table {
    name: "CameraInfo550D",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 7, name: "HighlightTonePriority", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_HIGHLIGHTTONEPRIORITY_10, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 21, name: "FlashMeteringMode", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FLASHMETERINGMODE_1, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 25, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 30, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 53, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 84, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 86, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 120, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 124, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 176, name: "PictureStyle", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_PICTURESTYLE_6, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 255, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 257, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 259, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 420, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::RequireVersionString, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 484, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 496, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 796, name: "PictureStyleInfo", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo), unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO5D: Table = Table {
    name: "CameraInfo5D",
    default_fmt: Fmt::Int8s,
    fields: &[
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 12, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 23, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 27, name: "MacroMagnification", fmt: None, cond: Cond::LensTypeIs(124), rc: Rc::None, vc: Vc::MacroMagnification, pc: Pc::Sprintf1Fx, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 39, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 40, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 56, name: "AFPointsInFocus5D", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::BitMask(PC_AFPOINTSINFOCUS5D_12, PC_AFPOINTSINFOCUS5D_BITS_13), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 84, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 88, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 108, name: "PictureStyle", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_PICTURESTYLE_6, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 147, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 149, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 151, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 164, name: "FirmwareRevision", fmt: Some(Fmt::Str(8)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 172, name: "ShortOwnerName", fmt: Some(Fmt::Str(16)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 204, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 208, name: "FileIndex", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 232, name: "ContrastStandard", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 233, name: "ContrastPortrait", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 234, name: "ContrastLandscape", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 235, name: "ContrastNeutral", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 236, name: "ContrastFaithful", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 237, name: "ContrastMonochrome", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 238, name: "ContrastUserDef1", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 239, name: "ContrastUserDef2", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 240, name: "ContrastUserDef3", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 241, name: "SharpnessStandard", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 242, name: "SharpnessPortrait", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 243, name: "SharpnessLandscape", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 244, name: "SharpnessNeutral", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 245, name: "SharpnessFaithful", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 246, name: "SharpnessMonochrome", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 247, name: "SharpnessUserDef1", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 248, name: "SharpnessUserDef2", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 249, name: "SharpnessUserDef3", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 250, name: "SaturationStandard", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 251, name: "SaturationPortrait", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 252, name: "SaturationLandscape", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 253, name: "SaturationNeutral", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 254, name: "SaturationFaithful", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 255, name: "FilterEffectMonochrome", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FILTEREFFECTMONOCHROME_14, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 256, name: "SaturationUserDef1", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 257, name: "SaturationUserDef2", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 258, name: "SaturationUserDef3", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 259, name: "ColorToneStandard", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 260, name: "ColorTonePortrait", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 261, name: "ColorToneLandscape", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 262, name: "ColorToneNeutral", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 263, name: "ColorToneFaithful", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 264, name: "ToningEffectMonochrome", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_TONINGEFFECTMONOCHROME_15, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 265, name: "ColorToneUserDef1", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 266, name: "ColorToneUserDef2", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 267, name: "ColorToneUserDef3", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 268, name: "UserDef1PictureStyle", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_USERDEF1PICTURESTYLE_16, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 270, name: "UserDef2PictureStyle", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_USERDEF1PICTURESTYLE_16, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 272, name: "UserDef3PictureStyle", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_USERDEF1PICTURESTYLE_16, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 284, name: "TimeStamp", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::UnixTime, pc: Pc::DateTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO5DMKII: Table = Table {
    name: "CameraInfo5DmkII",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 0, name: "FirmwareVersionLookAhead", fmt: Some(Fmt::Undef(388)), cond: Cond::Always, rc: Rc::FirmwareProbe(&[(346, 1), (382, 2)]), vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: true },
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 7, name: "HighlightTonePriority", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_HIGHLIGHTTONEPRIORITY_10, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 19, name: "FlashModel", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FLASHMODEL_0, false), hook: &[], mask: Some(127), sub: None, unknown: false, hidden: false },
        F { idx: 21, name: "FlashMeteringMode", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FLASHMETERINGMODE_1, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 25, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 27, name: "MacroMagnification", fmt: None, cond: Cond::LensTypeIs(124), rc: Rc::None, vc: Vc::MacroMagnification, pc: Pc::Sprintf1Fx, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 30, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 49, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 80, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 82, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 111, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 115, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 167, name: "PictureStyle", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_PICTURESTYLE_6, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 189, name: "HighISONoiseReduction", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_HIGHISONOISEREDUCTION_11, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 191, name: "AutoLightingOptimizer", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_HIGHISONOISEREDUCTION_11, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 230, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 232, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 234, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[HookRule { cmp: Cmp::Lt, firm: 2, delta: -36, zero_delta: 65536 }], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 382, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::RequireVersionString, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 398, name: "OwnerName", fmt: Some(Fmt::Str(32)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 443, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 455, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 759, name: "PictureStyleInfo", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo), unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO5DMKIII: Table = Table {
    name: "CameraInfo5DmkIII",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 0, name: "FirmwareVersionLookAhead", fmt: Some(Fmt::Undef(589)), cond: Cond::Always, rc: Rc::FirmwareProbe(&[(556, 1), (557, 2), (572, 3), (578, 4), (583, 5)]), vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: true },
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 27, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[HookRule { cmp: Cmp::Lt, firm: 3, delta: -1, zero_delta: 65536 }], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 35, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[HookRule { cmp: Cmp::Eq, firm: 1, delta: -3, zero_delta: -3 }, HookRule { cmp: Cmp::Eq, firm: 2, delta: -2, zero_delta: -2 }, HookRule { cmp: Cmp::Ge, firm: 4, delta: 6, zero_delta: 6 }], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 125, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 140, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 142, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[HookRule { cmp: Cmp::Lt, firm: 3, delta: -4, zero_delta: -4 }, HookRule { cmp: Cmp::Gt, firm: 4, delta: 5, zero_delta: 5 }], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 188, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 192, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 244, name: "PictureStyle", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_PICTURESTYLE_6, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 339, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 341, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 343, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[HookRule { cmp: Cmp::Lt, firm: 3, delta: -8, zero_delta: -8 }], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 356, name: "LensSerialNumber", fmt: Some(Fmt::Undef(5)), cond: Cond::Always, rc: Rc::None, vc: Vc::HexBytes, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 572, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 652, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 656, name: "FileIndex2", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 664, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 668, name: "DirectoryIndex2", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 944, name: "PictureStyleInfo", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo2), unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO600D: Table = Table {
    name: "CameraInfo600D",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 7, name: "HighlightTonePriority", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_HIGHLIGHTTONEPRIORITY_10, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 21, name: "FlashMeteringMode", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FLASHMETERINGMODE_1, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 25, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 30, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 56, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 87, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 89, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 123, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 127, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 179, name: "PictureStyle", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_PICTURESTYLE_6, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 234, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 236, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 238, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 411, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::RequireVersionString, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 475, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 487, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 763, name: "PictureStyleInfo", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo2), unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO60D: Table = Table {
    name: "CameraInfo60D",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 25, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 30, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 54, name: "CameraOrientation", fmt: None, cond: Cond::ModelEndsWord("EOS 60D"), rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 58, name: "CameraOrientation", fmt: None, cond: Cond::ModelHasWord(&["1200D", "REBEL T5", "Kiss X70"]), rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 85, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::ModelEndsWord("EOS 60D"), rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 87, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::ModelEndsWord("EOS 60D"), rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 125, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::ModelEndsWord("EOS 60D"), rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 232, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 234, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 236, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 409, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 473, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::ModelEndsWord("EOS 60D"), rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 485, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::ModelEndsWord("EOS 60D"), rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 761, name: "PictureStyleInfo", fmt: None, cond: Cond::ModelHasWord(&["1200D", "REBEL T5", "Kiss X70"]), rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo2), unknown: false, hidden: false },
        F { idx: 801, name: "PictureStyleInfo", fmt: None, cond: Cond::ModelEndsWord("EOS 60D"), rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo2), unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO650D: Table = Table {
    name: "CameraInfo650D",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 27, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 35, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 125, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 140, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 142, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 188, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 192, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 244, name: "PictureStyle", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_PICTURESTYLE_6, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 295, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 297, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 299, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 539, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::ModelHasWord(&["650D", "REBEL T4i", "Kiss X6i"]), rc: Rc::RequireVersionString, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 544, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::ModelHasWord(&["700D", "REBEL T5i", "Kiss X7i"]), rc: Rc::RequireVersionString, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 624, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::ModelHasWord(&["650D", "REBEL T4i", "Kiss X6i"]), rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 628, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::ModelHasWord(&["700D", "REBEL T5i", "Kiss X7i"]), rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 636, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::ModelHasWord(&["650D", "REBEL T4i", "Kiss X6i"]), rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 640, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::ModelHasWord(&["700D", "REBEL T5i", "Kiss X7i"]), rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 912, name: "PictureStyleInfo", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo2), unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO6D: Table = Table {
    name: "CameraInfo6D",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 27, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 35, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 131, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 146, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 148, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 194, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 198, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 250, name: "PictureStyle", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_PICTURESTYLE_6, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 353, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 355, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 357, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 598, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 682, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 694, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 966, name: "PictureStyleInfo", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo2), unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO70D: Table = Table {
    name: "CameraInfo70D",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 27, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 35, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 132, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 147, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 149, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 199, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 358, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 360, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 362, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 606, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 691, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 703, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 975, name: "PictureStyleInfo", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo2), unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO750D: Table = Table {
    name: "CameraInfo750D",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 27, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 35, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 150, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 165, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 167, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 305, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 309, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 361, name: "PictureStyle", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_PICTURESTYLE_6, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 388, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 390, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 392, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 1085, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::RequireVersionString, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 1097, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::RequireVersionString, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO7D: Table = Table {
    name: "CameraInfo7D",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 0, name: "FirmwareVersionLookAhead", fmt: Some(Fmt::Undef(434)), cond: Cond::Always, rc: Rc::FirmwareProbe(&[(424, 1), (428, 2)]), vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: true },
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 7, name: "HighlightTonePriority", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_HIGHLIGHTTONEPRIORITY_10, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 8, name: "MeasuredEV2", fmt: None, cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::Ev8Minus6, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 9, name: "MeasuredEV", fmt: None, cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::Ev8Minus6, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 21, name: "FlashMeteringMode", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FLASHMETERINGMODE_1, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 25, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 30, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[HookRule { cmp: Cmp::Lt, firm: 2, delta: -4, zero_delta: 65536 }], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 53, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 84, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 86, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 119, name: "WhiteBalance", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_WHITEBALANCE_3, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 123, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 175, name: "CameraPictureStyle", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAPICTURESTYLE_17, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 201, name: "HighISONoiseReduction", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_HIGHISONOISEREDUCTION_11, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 274, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 276, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 278, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 428, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::RequireVersionString, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 491, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 503, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 807, name: "PictureStyleInfo", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: Some(SubTable::PSInfo), unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFO80D: Table = Table {
    name: "CameraInfo80D",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 3, name: "FNumber", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "ExposureTime", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::CanonExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ISO", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::CanonIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 27, name: "CameraTemperature", fmt: Some(Fmt::Int8u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 35, name: "FocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::SkipZero, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 150, name: "CameraOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_CAMERAORIENTATION_2, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 165, name: "FocusDistanceUpper", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 167, name: "FocusDistanceLower", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::Div100, pc: Pc::FocusDistance, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 314, name: "ColorTemperature", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 393, name: "LensType", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_LENSTYPE_4, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 395, name: "MinFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 397, name: "MaxFocalLength", fmt: Some(Fmt::Int16uRev), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Mm, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 1114, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 1198, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 1210, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::Minus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFOG5XII: Table = Table {
    name: "CameraInfoG5XII",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 659, name: "ShutterCount", fmt: Some(Fmt::Int32u), cond: Cond::FileTypeUnavailable, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 2709, name: "ShutterCount", fmt: Some(Fmt::Int32u), cond: Cond::FileTypeUnavailable, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 2849, name: "DirectoryIndex", fmt: Some(Fmt::Int32u), cond: Cond::FileTypeUnavailable, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 2861, name: "FileIndex", fmt: Some(Fmt::Int32u), cond: Cond::FileTypeUnavailable, rc: Rc::None, vc: Vc::Plus1, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFOPOWERSHOT: Table = Table {
    name: "CameraInfoPowerShot",
    default_fmt: Fmt::Int32s,
    fields: &[
        F { idx: 0, name: "ISO", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::PowerShotIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 5, name: "FNumber", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::PowerShotFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "ExposureTime", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::PowerShotExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 23, name: "Rotation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 135, name: "CameraTemperature", fmt: None, cond: Cond::CountEq(138), rc: Rc::None, vc: Vc::None, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 145, name: "CameraTemperature", fmt: None, cond: Cond::CountEq(148), rc: Rc::None, vc: Vc::None, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFOPOWERSHOT2: Table = Table {
    name: "CameraInfoPowerShot2",
    default_fmt: Fmt::Int32s,
    fields: &[
        F { idx: 1, name: "ISO", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::PowerShotIso, pc: Pc::Sprintf0F, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 6, name: "FNumber", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::PowerShotFNumber, pc: Pc::Sprintf2G, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 7, name: "ExposureTime", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::PowerShotExposureTime, pc: Pc::ExposureTime, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 24, name: "Rotation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 153, name: "CameraTemperature", fmt: None, cond: Cond::CountEq(156), rc: Rc::None, vc: Vc::None, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 159, name: "CameraTemperature", fmt: None, cond: Cond::CountEq(162), rc: Rc::None, vc: Vc::None, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 164, name: "CameraTemperature", fmt: None, cond: Cond::CountEq(167), rc: Rc::None, vc: Vc::None, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 168, name: "CameraTemperature", fmt: None, cond: Cond::CountEq(171), rc: Rc::None, vc: Vc::None, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 261, name: "CameraTemperature", fmt: None, cond: Cond::CountEq(264), rc: Rc::None, vc: Vc::None, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFOR6: Table = Table {
    name: "CameraInfoR6",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 2522, name: "CameraTemperature", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::Minus128, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 2801, name: "ShutterCount", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFOR6M2: Table = Table {
    name: "CameraInfoR6m2",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 3369, name: "ShutterCount", fmt: Some(Fmt::Int32u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFOR6M3: Table = Table {
    name: "CameraInfoR6m3",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 2157, name: "ImageCount", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFOUNKNOWN: Table = Table {
    name: "CameraInfoUnknown",
    default_fmt: Fmt::Int8s,
    fields: &[
        F { idx: 363, name: "LensSerialNumber", fmt: Some(Fmt::Undef(5)), cond: Cond::ModelStartsWith("Canon EOS 5DS"), rc: Rc::None, vc: Vc::HexBytes, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 1473, name: "FirmwareVersion", fmt: Some(Fmt::Str(6)), cond: Cond::ValueLooksLikeVersion, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFOUNKNOWN16: Table = Table {
    name: "CameraInfoUnknown16",
    default_fmt: Fmt::Int16s,
    fields: &[
    ],
};

#[rustfmt::skip]
static TBL_CAMERAINFOUNKNOWN32: Table = Table {
    name: "CameraInfoUnknown32",
    default_fmt: Fmt::Int32s,
    fields: &[
        F { idx: 71, name: "CameraTemperature", fmt: None, cond: Cond::CountEq(72), rc: Rc::None, vc: Vc::None, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 83, name: "CameraTemperature", fmt: None, cond: Cond::CountEq(85), rc: Rc::None, vc: Vc::None, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 91, name: "CameraTemperature", fmt: None, cond: Cond::CountEither(93, 94), rc: Rc::None, vc: Vc::None, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 92, name: "CameraTemperature", fmt: None, cond: Cond::CountEq(96), rc: Rc::None, vc: Vc::None, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 100, name: "CameraTemperature", fmt: None, cond: Cond::CountEq(104), rc: Rc::None, vc: Vc::None, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: -3, name: "CameraTemperature", fmt: None, cond: Cond::CountGreater(400), rc: Rc::None, vc: Vc::None, pc: Pc::Celsius, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_FILTERINFO: Table = Table {
    name: "FilterInfo",
    default_fmt: Fmt::Int32s,
    fields: &[
        F { idx: 257, name: "GrainyBWFilter", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrOn(PC_GRAINYBWFILTER_18), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 513, name: "SoftFocusFilter", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrOn(PC_GRAINYBWFILTER_18), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 769, name: "ToyCameraFilter", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrOn(PC_GRAINYBWFILTER_18), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 1025, name: "MiniatureFilter", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrOn(PC_GRAINYBWFILTER_18), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 1026, name: "MiniatureFilterOrientation", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_MINIATUREFILTERORIENTATION_19, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 1027, name: "MiniatureFilterPosition", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 1028, name: "MiniatureFilterParameter", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::None, hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 1281, name: "FisheyeFilter", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrOn(PC_GRAINYBWFILTER_18), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 1537, name: "PaintingFilter", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrOn(PC_GRAINYBWFILTER_18), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 1793, name: "WatercolorFilter", fmt: None, cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrOn(PC_GRAINYBWFILTER_18), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_PSINFO: Table = Table {
    name: "PSInfo",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 0, name: "ContrastStandard", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "SharpnessStandard", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 8, name: "SaturationStandard", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 12, name: "ColorToneStandard", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 16, name: "FilterEffectStandard", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 20, name: "ToningEffectStandard", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 24, name: "ContrastPortrait", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 28, name: "SharpnessPortrait", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 32, name: "SaturationPortrait", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 36, name: "ColorTonePortrait", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 40, name: "FilterEffectPortrait", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 44, name: "ToningEffectPortrait", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 48, name: "ContrastLandscape", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 52, name: "SharpnessLandscape", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 56, name: "SaturationLandscape", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 60, name: "ColorToneLandscape", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 64, name: "FilterEffectLandscape", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 68, name: "ToningEffectLandscape", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 72, name: "ContrastNeutral", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 76, name: "SharpnessNeutral", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 80, name: "SaturationNeutral", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 84, name: "ColorToneNeutral", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 88, name: "FilterEffectNeutral", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 92, name: "ToningEffectNeutral", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 96, name: "ContrastFaithful", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 100, name: "SharpnessFaithful", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 104, name: "SaturationFaithful", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 108, name: "ColorToneFaithful", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 112, name: "FilterEffectFaithful", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 116, name: "ToningEffectFaithful", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 120, name: "ContrastMonochrome", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 124, name: "SharpnessMonochrome", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 128, name: "SaturationMonochrome", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 132, name: "ColorToneMonochrome", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 136, name: "FilterEffectMonochrome", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FILTEREFFECTMONOCHROME_14, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 140, name: "ToningEffectMonochrome", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_TONINGEFFECTMONOCHROME_15, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 144, name: "ContrastUserDef1", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 148, name: "SharpnessUserDef1", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 152, name: "SaturationUserDef1", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 156, name: "ColorToneUserDef1", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 160, name: "FilterEffectUserDef1", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FILTEREFFECTMONOCHROME_14, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 164, name: "ToningEffectUserDef1", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_TONINGEFFECTMONOCHROME_15, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 168, name: "ContrastUserDef2", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 172, name: "SharpnessUserDef2", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 176, name: "SaturationUserDef2", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 180, name: "ColorToneUserDef2", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 184, name: "FilterEffectUserDef2", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FILTEREFFECTMONOCHROME_14, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 188, name: "ToningEffectUserDef2", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_TONINGEFFECTMONOCHROME_15, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 192, name: "ContrastUserDef3", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 196, name: "SharpnessUserDef3", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 200, name: "SaturationUserDef3", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 204, name: "ColorToneUserDef3", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 208, name: "FilterEffectUserDef3", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FILTEREFFECTMONOCHROME_14, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 212, name: "ToningEffectUserDef3", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_TONINGEFFECTMONOCHROME_15, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 216, name: "UserDef1PictureStyle", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_USERDEF1PICTURESTYLE_16, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 218, name: "UserDef2PictureStyle", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_USERDEF1PICTURESTYLE_16, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 220, name: "UserDef3PictureStyle", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_USERDEF1PICTURESTYLE_16, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

#[rustfmt::skip]
static TBL_PSINFO2: Table = Table {
    name: "PSInfo2",
    default_fmt: Fmt::Int8u,
    fields: &[
        F { idx: 0, name: "ContrastStandard", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 4, name: "SharpnessStandard", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 8, name: "SaturationStandard", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 12, name: "ColorToneStandard", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 16, name: "FilterEffectStandard", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 20, name: "ToningEffectStandard", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 24, name: "ContrastPortrait", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 28, name: "SharpnessPortrait", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 32, name: "SaturationPortrait", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 36, name: "ColorTonePortrait", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 40, name: "FilterEffectPortrait", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 44, name: "ToningEffectPortrait", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 48, name: "ContrastLandscape", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 52, name: "SharpnessLandscape", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 56, name: "SaturationLandscape", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 60, name: "ColorToneLandscape", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 64, name: "FilterEffectLandscape", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 68, name: "ToningEffectLandscape", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 72, name: "ContrastNeutral", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 76, name: "SharpnessNeutral", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 80, name: "SaturationNeutral", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 84, name: "ColorToneNeutral", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 88, name: "FilterEffectNeutral", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 92, name: "ToningEffectNeutral", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 96, name: "ContrastFaithful", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 100, name: "SharpnessFaithful", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 104, name: "SaturationFaithful", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 108, name: "ColorToneFaithful", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 112, name: "FilterEffectFaithful", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 116, name: "ToningEffectFaithful", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 120, name: "ContrastMonochrome", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 124, name: "SharpnessMonochrome", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 128, name: "SaturationMonochrome", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 132, name: "ColorToneMonochrome", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: true, hidden: false },
        F { idx: 136, name: "FilterEffectMonochrome", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FILTEREFFECTMONOCHROME_14, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 140, name: "ToningEffectMonochrome", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_TONINGEFFECTMONOCHROME_15, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 144, name: "ContrastAuto", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 148, name: "SharpnessAuto", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 152, name: "SaturationAuto", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 156, name: "ColorToneAuto", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 160, name: "FilterEffectAuto", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FILTEREFFECTMONOCHROME_14, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 164, name: "ToningEffectAuto", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_TONINGEFFECTMONOCHROME_15, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 168, name: "ContrastUserDef1", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 172, name: "SharpnessUserDef1", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 176, name: "SaturationUserDef1", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 180, name: "ColorToneUserDef1", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 184, name: "FilterEffectUserDef1", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FILTEREFFECTMONOCHROME_14, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 188, name: "ToningEffectUserDef1", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_TONINGEFFECTMONOCHROME_15, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 192, name: "ContrastUserDef2", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 196, name: "SharpnessUserDef2", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 200, name: "SaturationUserDef2", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 204, name: "ColorToneUserDef2", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 208, name: "FilterEffectUserDef2", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FILTEREFFECTMONOCHROME_14, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 212, name: "ToningEffectUserDef2", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_TONINGEFFECTMONOCHROME_15, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 216, name: "ContrastUserDef3", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 220, name: "SharpnessUserDef3", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 224, name: "SaturationUserDef3", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 228, name: "ColorToneUserDef3", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::MapOrRaw(PC_CONTRASTSTANDARD_20), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 232, name: "FilterEffectUserDef3", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_FILTEREFFECTMONOCHROME_14, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 236, name: "ToningEffectUserDef3", fmt: Some(Fmt::Int32s), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_TONINGEFFECTMONOCHROME_15, true), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 240, name: "UserDef1PictureStyle", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_USERDEF1PICTURESTYLE_16, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 242, name: "UserDef2PictureStyle", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_USERDEF1PICTURESTYLE_16, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
        F { idx: 244, name: "UserDef3PictureStyle", fmt: Some(Fmt::Int16u), cond: Cond::Always, rc: Rc::None, vc: Vc::None, pc: Pc::Map(PC_USERDEF1PICTURESTYLE_16, false), hook: &[], mask: None, sub: None, unknown: false, hidden: false },
    ],
};

/// Every transcribed table, for the dispatcher and for the tests.
#[rustfmt::skip]
pub(crate) static ALL_TABLES: &[&Table] = &[
    &TBL_CAMERAINFO1000D,
    &TBL_CAMERAINFO1D,
    &TBL_CAMERAINFO1DX,
    &TBL_CAMERAINFO1DMKII,
    &TBL_CAMERAINFO1DMKIII,
    &TBL_CAMERAINFO1DMKIIN,
    &TBL_CAMERAINFO1DMKIV,
    &TBL_CAMERAINFO40D,
    &TBL_CAMERAINFO450D,
    &TBL_CAMERAINFO500D,
    &TBL_CAMERAINFO50D,
    &TBL_CAMERAINFO550D,
    &TBL_CAMERAINFO5D,
    &TBL_CAMERAINFO5DMKII,
    &TBL_CAMERAINFO5DMKIII,
    &TBL_CAMERAINFO600D,
    &TBL_CAMERAINFO60D,
    &TBL_CAMERAINFO650D,
    &TBL_CAMERAINFO6D,
    &TBL_CAMERAINFO70D,
    &TBL_CAMERAINFO750D,
    &TBL_CAMERAINFO7D,
    &TBL_CAMERAINFO80D,
    &TBL_CAMERAINFOG5XII,
    &TBL_CAMERAINFOPOWERSHOT,
    &TBL_CAMERAINFOPOWERSHOT2,
    &TBL_CAMERAINFOR6,
    &TBL_CAMERAINFOR6M2,
    &TBL_CAMERAINFOR6M3,
    &TBL_CAMERAINFOUNKNOWN,
    &TBL_CAMERAINFOUNKNOWN16,
    &TBL_CAMERAINFOUNKNOWN32,
    &TBL_FILTERINFO,
    &TBL_PSINFO,
    &TBL_PSINFO2,
];

pub(crate) fn sub_table(which: SubTable) -> &'static Table {
    match which {
        SubTable::PSInfo => &TBL_PSINFO,
        SubTable::PSInfo2 => &TBL_PSINFO2,
    }
}

/// The `0xd => [...]` alternative list, in ExifTool's order. `None` marks an
/// alternative whose table this transcription does not carry.
#[rustfmt::skip]
pub(crate) static DISPATCH: &[(&str, &Table)] = &[
    ("\\b1DS?$", &TBL_CAMERAINFO1D),
    ("\\b1Ds? Mark II$", &TBL_CAMERAINFO1DMKII),
    ("\\b1Ds? Mark II N$", &TBL_CAMERAINFO1DMKIIN),
    ("\\b1Ds? Mark III$", &TBL_CAMERAINFO1DMKIII),
    ("\\b1D Mark IV$", &TBL_CAMERAINFO1DMKIV),
    ("EOS-1D X$", &TBL_CAMERAINFO1DX),
    ("EOS 5D$", &TBL_CAMERAINFO5D),
    ("EOS 5D Mark II$", &TBL_CAMERAINFO5DMKII),
    ("EOS 5D Mark III$", &TBL_CAMERAINFO5DMKIII),
    ("EOS 6D$", &TBL_CAMERAINFO6D),
    ("EOS 7D$", &TBL_CAMERAINFO7D),
    ("EOS 40D$", &TBL_CAMERAINFO40D),
    ("EOS 50D$", &TBL_CAMERAINFO50D),
    ("EOS 60D$", &TBL_CAMERAINFO60D),
    ("EOS 70D$", &TBL_CAMERAINFO70D),
    ("EOS 80D$", &TBL_CAMERAINFO80D),
    ("\\b(450D|REBEL XSi|Kiss X2)\\b", &TBL_CAMERAINFO450D),
    ("\\b(500D|REBEL T1i|Kiss X3)\\b", &TBL_CAMERAINFO500D),
    ("\\b(550D|REBEL T2i|Kiss X4)\\b", &TBL_CAMERAINFO550D),
    ("\\b(600D|REBEL T3i|Kiss X5)\\b", &TBL_CAMERAINFO600D),
    ("\\b(650D|REBEL T4i|Kiss X6i)\\b", &TBL_CAMERAINFO650D),
    ("\\b(700D|REBEL T5i|Kiss X7i)\\b", &TBL_CAMERAINFO650D),
    ("\\b(750D|Rebel T6i|Kiss X8i)\\b", &TBL_CAMERAINFO750D),
    ("\\b(760D|Rebel T6s|8000D)\\b", &TBL_CAMERAINFO750D),
    ("\\b(1000D|REBEL XS|Kiss F)\\b", &TBL_CAMERAINFO1000D),
    ("\\b(1100D|REBEL T3|Kiss X50)\\b", &TBL_CAMERAINFO600D),
    ("\\b(1200D|REBEL T5|Kiss X70)\\b", &TBL_CAMERAINFO60D),
    ("\\bEOS R[56]$", &TBL_CAMERAINFOR6),
    ("\\bEOS (R6m2|R8|R50)$", &TBL_CAMERAINFOR6M2),
    ("\\bEOS R6 Mark III$", &TBL_CAMERAINFOR6M3),
    ("\\bG5 X Mark II$", &TBL_CAMERAINFOG5XII),
];

/// `%Canon::FilterInfo` (MakerNote tag 0x4024), walked by `super::filter_info`.
pub(crate) static TBL_FILTERINFO_REF: &Table = &TBL_FILTERINFO;
pub(crate) static TBL_POWERSHOT: &Table = &TBL_CAMERAINFOPOWERSHOT;
pub(crate) static TBL_POWERSHOT2: &Table = &TBL_CAMERAINFOPOWERSHOT2;
pub(crate) static TBL_UNKNOWN32: &Table = &TBL_CAMERAINFOUNKNOWN32;
pub(crate) static TBL_UNKNOWN16: &Table = &TBL_CAMERAINFOUNKNOWN16;
pub(crate) static TBL_UNKNOWN: &Table = &TBL_CAMERAINFOUNKNOWN;
