//! `Sony::Main` scalars main_table.rs does not implement -- generated, do not
//! hand-edit.
//!
//! Read out of ExifTool's own `%Image::ExifTool::Sony::Main` hash in-process
//! (13.59) rather than retyped, and interpreted by [`super::main_extra`]. A tag
//! whose Condition or conversion is not one of the forms that module implements
//! is omitted entirely rather than emitted with a guessed value.

use super::binary_data::{Dm, Fmt, NumCmp, Other, Pc, Raw, Vc};
use super::main_extra::{MCond, MainExtraTag};

#[rustfmt::skip]
static M0: &[(&str, &str)] = &[("0", "Off"), ("1", "On")];
#[rustfmt::skip]
static M1: &[(&str, &str)] = &[("0", "(none)")];
#[rustfmt::skip]
static M2: &[(&str, &str)] = &[("0 0", "n/a"), ("1 0", "Off"), ("1 1", "Standard"), ("1 2", "High"), ("65535 65535", "n/a")];
#[rustfmt::skip]
static M3: &[(&str, &str)] = &[("0", "Compressed RAW"), ("1", "Uncompressed RAW"), ("2", "Lossless Compressed RAW"), ("3", "Compressed RAW 2"), ("65535", "n/a")];
#[rustfmt::skip]
static M4: &[(&str, &str)] = &[("0", "Standard"), ("1", "Ambience"), ("2", "White")];
#[rustfmt::skip]
static M5: &[(&str, &str)] = &[("1024", "Average"), ("1280", "Highlight"), ("256", "Multi-segment"), ("512", "Center-weighted average"), ("769", "Spot (Standard)"), ("770", "Spot (Large)")];
#[rustfmt::skip]
static M6: &[(&str, &str)] = &[("0", "JPEG"), ("1", "HEIF"), ("65535", "n/a")];
#[rustfmt::skip]
static M7: &[(&str, &str)] = &[("0", "35mm (Off)"), ("1", "50mm"), ("2", "70mm")];
#[rustfmt::skip]
static M8: &[(&str, &str)] = &[("0", "Program AE"), ("1", "Portrait"), ("10", "Hi-Speed Shutter"), ("11", "Twilight Portrait"), ("12", "Soft Snap/Portrait"), ("13", "Fireworks"), ("14", "Smile Shutter"), ("15", "Manual"), ("18", "High Sensitivity"), ("19", "Macro"), ("2", "Beach"), ("20", "Advanced Sports Shooting"), ("29", "Underwater"), ("3", "Sports"), ("33", "Food"), ("34", "Sweep Panorama"), ("35", "Handheld Night Shot"), ("36", "Anti Motion Blur"), ("37", "Pet"), ("38", "Backlight Correction HDR"), ("39", "Superior Auto"), ("4", "Snow"), ("40", "Background Defocus"), ("41", "Soft Skin"), ("42", "3D Image"), ("5", "Landscape"), ("6", "Auto"), ("65535", "n/a"), ("7", "Aperture-priority AE"), ("8", "Shutter speed priority AE"), ("9", "Night Scene / Twilight")];
#[rustfmt::skip]
static M9: &[(&str, &str)] = &[("1", "AF-S"), ("2", "AF-C"), ("4", "Permanent-AF"), ("65535", "n/a")];
#[rustfmt::skip]
static M10: &[(&str, &str)] = &[("0", "Default"), ("1", "Multi"), ("14", "Tracking"), ("15", "Face Tracking"), ("2", "Center"), ("3", "Spot"), ("4", "Flexible Spot"), ("6", "Touch"), ("65535", "n/a")];
#[rustfmt::skip]
static M11: &[(&str, &str)] = &[("0", "Multi"), ("1", "Center"), ("10", "Selective (for Miniature effect)"), ("14", "Tracking"), ("15", "Face Tracking"), ("2", "Spot"), ("255", "Manual"), ("3", "Flexible Spot")];
#[rustfmt::skip]
static M12: &[(&str, &str)] = &[("0", "Manual"), ("2", "AF-S"), ("3", "AF-C"), ("5", "Semi-manual"), ("6", "DMF")];
#[rustfmt::skip]
static M13: &[(&str, &str)] = &[("0", "Normal"), ("1", "High"), ("2", "Low"), ("3", "Off"), ("65535", "n/a")];
#[rustfmt::skip]
static B0: &[(u32, &str)] = &[(0u32, "Center"), (1u32, "Top"), (2u32, "Upper-right"), (3u32, "Right"), (4u32, "Lower-right"), (5u32, "Bottom"), (6u32, "Lower-left"), (7u32, "Left"), (8u32, "Upper-left"), (9u32, "Far Right"), (10u32, "Far Left"), (11u32, "Upper-middle"), (12u32, "Near Right"), (13u32, "Lower-middle"), (14u32, "Near Left"), (15u32, "Upper Far Right"), (16u32, "Lower Far Right"), (17u32, "Lower Far Left"), (18u32, "Upper Far Left")];
#[rustfmt::skip]
static B1: &[(u32, &str)] = &[(0u32, "A5"), (1u32, "A6"), (2u32, "A7"), (3u32, "B2"), (4u32, "B3"), (5u32, "B4"), (6u32, "B5"), (7u32, "B6"), (8u32, "B7"), (9u32, "B8"), (10u32, "B9"), (11u32, "B10"), (12u32, "C1"), (13u32, "C2"), (14u32, "C3"), (15u32, "C4"), (16u32, "C5"), (17u32, "C6"), (18u32, "C7"), (19u32, "C8"), (20u32, "C9"), (21u32, "C10"), (22u32, "C11"), (23u32, "D1"), (24u32, "D2"), (25u32, "D3"), (26u32, "D4"), (27u32, "D5"), (28u32, "D6"), (29u32, "D7"), (30u32, "D8"), (31u32, "D9"), (32u32, "D10"), (33u32, "D11"), (34u32, "E1"), (35u32, "E2"), (36u32, "E3"), (37u32, "E4"), (38u32, "E5"), (39u32, "E6"), (40u32, "E7"), (41u32, "E8"), (42u32, "E9"), (43u32, "E10"), (44u32, "E11"), (45u32, "F1"), (46u32, "F2"), (47u32, "F3"), (48u32, "F4"), (49u32, "F5"), (50u32, "F6"), (51u32, "F7"), (52u32, "F8"), (53u32, "F9"), (54u32, "F10"), (55u32, "F11"), (56u32, "G1"), (57u32, "G2"), (58u32, "G3"), (59u32, "G4"), (60u32, "G5"), (61u32, "G6"), (62u32, "G7"), (63u32, "G8"), (64u32, "G9"), (65u32, "G10"), (66u32, "G11"), (67u32, "H2"), (68u32, "H3"), (69u32, "H4"), (70u32, "H5"), (71u32, "H6"), (72u32, "H7"), (73u32, "H8"), (74u32, "H9"), (75u32, "H10"), (76u32, "I5"), (77u32, "I6"), (78u32, "I7")];
#[rustfmt::skip]
static B2: &[(u32, &str)] = &[];

/// Every row, in ExifTool's own order so a Condition list resolves the same way.
#[rustfmt::skip]
pub static TAGS: &[MainExtraTag] = &[
    MainExtraTag { id: 0x1000, name: "MultiBurstMode", cond: MCond::EntryFormat("undef"), fmt: Some(Fmt::U8), raw: Raw::None, vc: Vc::None, pc: Pc::Map(M0, Other::None), print_hex: false, low_priority: false },
    MainExtraTag { id: 0x1001, name: "MultiBurstImageWidth", cond: MCond::EntryFormat("int16u"), fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::None, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x1002, name: "MultiBurstImageHeight", cond: MCond::EntryFormat("int16u"), fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::None, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2004, name: "Contrast", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::PlusOrVal, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2005, name: "Saturation", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::PlusOrVal, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2006, name: "Sharpness", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::PlusOrVal, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2007, name: "Brightness", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::PlusOrVal, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x201d, name: "FlexibleSpotPosition", cond: MCond::ModelRe(false, r"^(NEX-|ILCE-|ILME-|ZV-|DSC-(RX10M4|RX100M6|RX100M7|RX100M5A|HX95|HX99|RX0M2|RX1RM3))"), fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::None, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2020, name: "AFPointsUsed", cond: MCond::ModelRe(true, r"^(ILCA-|DSC-|ZV-)"), fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::Bitmask(M1, B0, 8, Other::None), print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2020, name: "AFPointsUsed", cond: MCond::ModelRe(false, r"^ILCA-(68|77M2)"), fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::Bitmask(M1, B1, 8, Other::None), print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2022, name: "FocalPlaneAFPointsUsed", cond: MCond::ModelRe(false, r"^(ILCE-(5100|6000|7M2))"), fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::Bitmask(M1, B2, 8, Other::None), print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2022, name: "FocalPlaneAFPointsUsed", cond: MCond::ModelRe(false, r"^ILCE-7RM2"), fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::Bitmask(M1, B2, 8, Other::None), print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2026, name: "WBShiftAB_GM_Precise", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::WbShiftPrecise, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2027, name: "FocusLocation", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::None, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2028, name: "VariableLowPassFilter", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M2, Other::None), print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2029, name: "RAWFileType", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M3, Other::None), print_hex: false, low_priority: false },
    MainExtraTag { id: 0x202b, name: "PrioritySetInAWB", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M4, Other::None), print_hex: false, low_priority: false },
    MainExtraTag { id: 0x202c, name: "MeteringMode2", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M5, Other::None), print_hex: true, low_priority: false },
    MainExtraTag { id: 0x202d, name: "ExposureStandardAdjustment", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::Signed1OrZero, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2031, name: "SerialNumber", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::SerialNumberSwap, pc: Pc::ZeroPad(8), print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2032, name: "Shadows", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::PlusOrVal, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2033, name: "Highlights", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::PlusOrVal, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2034, name: "Fade", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::PlusOrVal, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2035, name: "SharpnessRange", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::PlusOrVal, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2036, name: "Clarity", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::PlusOrVal, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2037, name: "FocusFrameSize", cond: MCond::Always, fmt: Some(Fmt::U16), raw: Raw::None, vc: Vc::None, pc: Pc::FocusFrameSize, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x2039, name: "JPEG-HEIFSwitch", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M6, Other::None), print_hex: false, low_priority: false },
    MainExtraTag { id: 0x204a, name: "FocusLocation2", cond: MCond::Always, fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::None, print_hex: false, low_priority: false },
    MainExtraTag { id: 0x205c, name: "StepCropShooting", cond: MCond::ModelRe(false, r"^(DSC-RX1RM3)\b"), fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M7, Other::None), print_hex: false, low_priority: false },
    MainExtraTag { id: 0xb041, name: "ExposureMode", cond: MCond::Always, fmt: None, raw: Raw::DropIfEq(65535.0_f64), vc: Vc::None, pc: Pc::Map(M8, Other::None), print_hex: false, low_priority: false },
    MainExtraTag { id: 0xb042, name: "FocusMode", cond: MCond::All(&[MCond::StoreU16(Dm::TagB042), MCond::Any(&[MCond::DmFalsy(Dm::MetaVersion), MCond::DmStrCmp(Dm::MetaVersion, false, "DC7303320222000")])]), fmt: None, raw: Raw::DropIfEq(65535.0_f64), vc: Vc::None, pc: Pc::Map(M9, Other::None), print_hex: false, low_priority: false },
    MainExtraTag { id: 0xb043, name: "AFAreaMode", cond: MCond::Any(&[MCond::DmFalsy(Dm::MetaVersion), MCond::DmStrCmp(Dm::MetaVersion, false, "DC7303320222000")]), fmt: None, raw: Raw::DropIfEq(65535.0_f64), vc: Vc::None, pc: Pc::Map(M10, Other::None), print_hex: false, low_priority: false },
    MainExtraTag { id: 0xb043, name: "AFAreaMode", cond: MCond::All(&[MCond::DmTruthy(Dm::TagB042), MCond::DmCmp(Dm::TagB042, NumCmp::Ne, 0.0_f64)]), fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M11, Other::None), print_hex: false, low_priority: false },
    MainExtraTag { id: 0xb04e, name: "FocusMode", cond: MCond::All(&[MCond::DmTruthy(Dm::MetaVersion), MCond::DmStrCmp(Dm::MetaVersion, true, "DC7303320222000")]), fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M12, Other::None), print_hex: false, low_priority: false },
    MainExtraTag { id: 0xb050, name: "HighISONoiseReduction2", cond: MCond::ModelRe(false, r"^(DSC-|Stellar)"), fmt: None, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M13, Other::None), print_hex: false, low_priority: false },
];
