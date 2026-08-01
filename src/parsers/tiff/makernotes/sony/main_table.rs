//! Sony MakerNote `Main` table: the tags stored directly as IFD entries.
//!
//! Transcribed from `Image::ExifTool::Sony::Main`. Every `PrintConv` hash here
//! was generated from that table rather than typed by hand, so an entry either
//! matches ExifTool exactly or is absent - a Sony `PrintConv` is a dense space
//! of near-identical strings ("On (Continuous)" vs "On (Shooting)") where a
//! plausible-looking guess is indistinguishable from a correct value in the
//! output but wrong in the file.
//!
//! Tags ExifTool defines but suppresses from its own default output are noted
//! where they are deliberately left out.

use super::binary::{lookup, print_float, signed_adjustment, unknown, unknown_hex};
use super::lens_spec::print_lens_spec;
use super::value::SonyValue;
use crate::parsers::tiff::makernotes::lens_data::sony as sony_lenses;

// ============================================================================
// PrintConv tables (generated from Image::ExifTool::Sony::Main)
// ============================================================================

static QUALITY: &[(i64, &str)] = &[
    (0, "RAW"),
    (1, "Super Fine"),
    (2, "Fine"),
    (3, "Standard"),
    (4, "Economy"),
    (5, "Extra Fine"),
    (6, "RAW + JPEG/HEIF"),
    (7, "Compressed RAW"),
    (8, "Compressed RAW + JPEG"),
    (9, "Light"),
    (4294967295, "n/a"),
];
static TELECONVERTER: &[(i64, &str)] = &[
    (0, "None"),
    (4, "Minolta/Sony AF 1.4x APO (D) (0x04)"),
    (5, "Minolta/Sony AF 2x APO (D) (0x05)"),
    (72, "Minolta/Sony AF 2x APO (D)"),
    (80, "Minolta AF 2x APO II"),
    (96, "Minolta AF 2x APO"),
    (136, "Minolta/Sony AF 1.4x APO (D)"),
    (144, "Minolta AF 1.4x APO II"),
    (160, "Minolta AF 1.4x APO"),
];
static WHITE_BALANCE_0115: &[(i64, &str)] = &[
    (0, "Auto"),
    (1, "Color Temperature/Color Filter"),
    (16, "Daylight"),
    (32, "Cloudy"),
    (48, "Shade"),
    (64, "Tungsten"),
    (80, "Flash"),
    (96, "Fluorescent"),
    (112, "Custom"),
    (128, "Underwater"),
];
static LONG_EXPOSURE_NR: &[(i64, &str)] = &[
    (0, "Off"),
    (1, "On (unused)"),
    (65537, "On (dark subtracted)"),
    (4294901760, "Off (65535)"),
    (4294901761, "On (65535)"),
    (4294967295, "n/a"),
];
static HIGH_ISO_NR: &[(i64, &str)] = &[
    (0, "Off"),
    (1, "Low"),
    (2, "Normal"),
    (3, "High"),
    (256, "Auto"),
    (65535, "n/a"),
];
static HDR_A: &[(i64, &str)] = &[
    (0, "Off"),
    (1, "Auto"),
    (16, "1.0 EV"),
    (17, "1.5 EV"),
    (18, "2.0 EV"),
    (19, "2.5 EV"),
    (20, "3.0 EV"),
    (21, "3.5 EV"),
    (22, "4.0 EV"),
    (23, "4.5 EV"),
    (24, "5.0 EV"),
    (25, "5.5 EV"),
    (26, "6.0 EV"),
];
static MULTI_FRAME_NR: &[(i64, &str)] = &[(0, "Off"), (1, "On"), (255, "n/a")];
static PICTURE_EFFECT: &[(i64, &str)] = &[
    (0, "Off"),
    (1, "Toy Camera"),
    (2, "Pop Color"),
    (3, "Posterization"),
    (4, "Posterization B/W"),
    (5, "Retro Photo"),
    (6, "Soft High Key"),
    (7, "Partial Color (red)"),
    (8, "Partial Color (green)"),
    (9, "Partial Color (blue)"),
    (10, "Partial Color (yellow)"),
    (13, "High Contrast Monochrome"),
    (16, "Toy Camera (normal)"),
    (17, "Toy Camera (cool)"),
    (18, "Toy Camera (warm)"),
    (19, "Toy Camera (green)"),
    (20, "Toy Camera (magenta)"),
    (32, "Soft Focus (low)"),
    (33, "Soft Focus"),
    (34, "Soft Focus (high)"),
    (48, "Miniature (auto)"),
    (49, "Miniature (top)"),
    (50, "Miniature (middle horizontal)"),
    (51, "Miniature (bottom)"),
    (52, "Miniature (left)"),
    (53, "Miniature (middle vertical)"),
    (54, "Miniature (right)"),
    (64, "HDR Painting (low)"),
    (65, "HDR Painting"),
    (66, "HDR Painting (high)"),
    (80, "Rich-tone Monochrome"),
    (97, "Water Color"),
    (98, "Water Color 2"),
    (112, "Illustration (low)"),
    (113, "Illustration"),
    (114, "Illustration (high)"),
];
static SOFT_SKIN_EFFECT: &[(i64, &str)] = &[
    (0, "Off"),
    (1, "Low"),
    (2, "Mid"),
    (3, "High"),
    (4294967295, "n/a"),
];
static CORRECTION_SETTING: &[(i64, &str)] = &[(0, "Off"), (2, "Auto"), (4294967295, "n/a")];
static AUTO_PORTRAIT_FRAMED: &[(i64, &str)] = &[(0, "No"), (1, "Yes")];
static FLASH_ACTION_MAIN: &[(i64, &str)] = &[
    (0, "Did not fire"),
    (1, "Flash Fired"),
    (2, "External Flash Fired"),
    (3, "Wireless Controlled Flash Fired"),
];
static EFCS: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
static MULTI_FRAME_NR_EFFECT: &[(i64, &str)] = &[(0, "Normal"), (1, "High")];
static SCENE_MODE: &[(i64, &str)] = &[
    (0, "Standard"),
    (1, "Portrait"),
    (2, "Text"),
    (3, "Night Scene"),
    (4, "Sunset"),
    (5, "Sports"),
    (6, "Landscape"),
    (7, "Night Portrait"),
    (8, "Macro"),
    (9, "Super Macro"),
    (16, "Auto"),
    (17, "Night View/Portrait"),
    (18, "Sweep Panorama"),
    (19, "Handheld Night Shot"),
    (20, "Anti Motion Blur"),
    (21, "Cont. Priority AE"),
    (22, "Auto+"),
    (23, "3D Sweep Panorama"),
    (24, "Superior Auto"),
    (25, "High Sensitivity"),
    (26, "Fireworks"),
    (27, "Food"),
    (28, "Pet"),
    (33, "HDR"),
    (65535, "n/a"),
];
static ZONE_MATCHING: &[(i64, &str)] = &[(0, "ISO Setting Used"), (1, "High Key"), (2, "Low Key")];
static DYNAMIC_RANGE_OPTIMIZER: &[(i64, &str)] = &[
    (0, "Off"),
    (1, "Standard"),
    (2, "Advanced Auto"),
    (3, "Auto"),
    (8, "Advanced Lv1"),
    (9, "Advanced Lv2"),
    (10, "Advanced Lv3"),
    (11, "Advanced Lv4"),
    (12, "Advanced Lv5"),
    (16, "Lv1"),
    (17, "Lv2"),
    (18, "Lv3"),
    (19, "Lv4"),
    (20, "Lv5"),
];
static IMAGE_STABILIZATION: &[(i64, &str)] = &[(0, "Off"), (1, "On"), (4294967295, "n/a")];
static COLOR_MODE: &[(i64, &str)] = &[
    (0, "Standard"),
    (1, "Vivid"),
    (2, "Portrait"),
    (3, "Landscape"),
    (4, "Sunset"),
    (5, "Night View/Portrait"),
    (6, "B&W"),
    (7, "Adobe RGB"),
    (12, "Neutral"),
    (13, "Clear"),
    (14, "Deep"),
    (15, "Light"),
    (16, "Autumn Leaves"),
    (17, "Sepia"),
    (18, "FL"),
    (19, "Vivid 2"),
    (20, "IN"),
    (21, "SH"),
    (22, "FL2"),
    (23, "FL3"),
    (100, "Neutral"),
    (101, "Clear"),
    (102, "Deep"),
    (103, "Light"),
    (104, "Night View"),
    (105, "Autumn Leaves"),
    (255, "Off"),
    (4294967295, "n/a"),
];
static MACRO: &[(i64, &str)] = &[(0, "Off"), (1, "On"), (2, "Close Focus"), (65535, "n/a")];
static EXPOSURE_MODE: &[(i64, &str)] = &[
    (0, "Program AE"),
    (1, "Portrait"),
    (2, "Beach"),
    (3, "Sports"),
    (4, "Snow"),
    (5, "Landscape"),
    (6, "Auto"),
    (7, "Aperture-priority AE"),
    (8, "Shutter speed priority AE"),
    (9, "Night Scene / Twilight"),
    (10, "Hi-Speed Shutter"),
    (11, "Twilight Portrait"),
    (12, "Soft Snap/Portrait"),
    (13, "Fireworks"),
    (14, "Smile Shutter"),
    (15, "Manual"),
    (18, "High Sensitivity"),
    (19, "Macro"),
    (20, "Advanced Sports Shooting"),
    (29, "Underwater"),
    (33, "Food"),
    (34, "Sweep Panorama"),
    (35, "Handheld Night Shot"),
    (36, "Anti Motion Blur"),
    (37, "Pet"),
    (38, "Backlight Correction HDR"),
    (39, "Superior Auto"),
    (40, "Background Defocus"),
    (41, "Soft Skin"),
    (42, "3D Image"),
    (65535, "n/a"),
];
static AF_ILLUMINATOR: &[(i64, &str)] = &[(0, "Off"), (1, "Auto"), (65535, "n/a")];
static JPEG_QUALITY: &[(i64, &str)] = &[
    (0, "Standard"),
    (1, "Fine"),
    (2, "Extra Fine"),
    (65535, "n/a"),
];
static FLASH_LEVEL: &[(i64, &str)] = &[
    (-32768, "Low"),
    (-9, "-9/3"),
    (-8, "-8/3"),
    (-7, "-7/3"),
    (-6, "-6/3"),
    (-5, "-5/3"),
    (-4, "-4/3"),
    (-3, "-3/3"),
    (-2, "-2/3"),
    (-1, "-1/3"),
    (0, "Normal"),
    (1, "+1/3"),
    (2, "+2/3"),
    (3, "+3/3"),
    (4, "+4/3"),
    (5, "+5/3"),
    (6, "+6/3"),
    (9, "+9/3"),
    (128, "n/a"),
    (32767, "High"),
];
static RELEASE_MODE: &[(i64, &str)] = &[
    (0, "Normal"),
    (2, "Continuous"),
    (5, "Exposure Bracketing"),
    (6, "White Balance Bracketing"),
    (8, "DRO Bracketing"),
    (65535, "n/a"),
];
static SEQUENCE_NUMBER: &[(i64, &str)] = &[(0, "Single"), (65535, "n/a")];
static ANTI_BLUR: &[(i64, &str)] = &[
    (0, "Off"),
    (1, "On (Continuous)"),
    (2, "On (Shooting)"),
    (65535, "n/a"),
];
static DRO_B04F: &[(i64, &str)] = &[(0, "Off"), (1, "Standard"), (2, "Plus")];
static INTELLIGENT_AUTO: &[(i64, &str)] = &[(0, "Off"), (1, "On"), (2, "Advanced")];
static WHITE_BALANCE_B054: &[(i64, &str)] = &[
    (0, "Auto"),
    (4, "Custom"),
    (5, "Daylight"),
    (6, "Cloudy"),
    (7, "Cool White Fluorescent"),
    (8, "Day White Fluorescent"),
    (9, "Daylight Fluorescent"),
    (10, "Incandescent2"),
    (11, "Warm White Fluorescent"),
    (14, "Incandescent"),
    (15, "Flash"),
    (17, "Underwater 1 (Blue Water)"),
    (18, "Underwater 2 (Green Water)"),
    (19, "Underwater Auto"),
];
static SONY_MODEL_ID: &[(i64, &str)] = &[
    (2, "DSC-R1"),
    (256, "DSLR-A100"),
    (257, "DSLR-A900"),
    (258, "DSLR-A700"),
    (259, "DSLR-A200"),
    (260, "DSLR-A350"),
    (261, "DSLR-A300"),
    (262, "DSLR-A900 (APS-C mode)"),
    (263, "DSLR-A380/A390"),
    (264, "DSLR-A330"),
    (265, "DSLR-A230"),
    (266, "DSLR-A290"),
    (269, "DSLR-A850"),
    (270, "DSLR-A850 (APS-C mode)"),
    (273, "DSLR-A550"),
    (274, "DSLR-A500"),
    (275, "DSLR-A450"),
    (278, "NEX-5"),
    (279, "NEX-3"),
    (280, "SLT-A33"),
    (281, "SLT-A55 / SLT-A55V"),
    (282, "DSLR-A560"),
    (283, "DSLR-A580"),
    (284, "NEX-C3"),
    (285, "SLT-A35"),
    (286, "SLT-A65 / SLT-A65V"),
    (287, "SLT-A77 / SLT-A77V"),
    (288, "NEX-5N"),
    (289, "NEX-7"),
    (290, "NEX-VG20E"),
    (291, "SLT-A37"),
    (292, "SLT-A57"),
    (293, "NEX-F3"),
    (294, "SLT-A99 / SLT-A99V"),
    (295, "NEX-6"),
    (296, "NEX-5R"),
    (297, "DSC-RX100"),
    (298, "DSC-RX1"),
    (299, "NEX-VG900"),
    (300, "NEX-VG30E"),
    (302, "ILCE-3000 / ILCE-3500"),
    (303, "SLT-A58"),
    (305, "NEX-3N"),
    (306, "ILCE-7"),
    (307, "NEX-5T"),
    (308, "DSC-RX100M2"),
    (309, "DSC-RX10"),
    (310, "DSC-RX1R"),
    (311, "ILCE-7R"),
    (312, "ILCE-6000"),
    (313, "ILCE-5000"),
    (317, "DSC-RX100M3"),
    (318, "ILCE-7S"),
    (319, "ILCA-77M2"),
    (339, "ILCE-5100"),
    (340, "ILCE-7M2"),
    (341, "DSC-RX100M4"),
    (342, "DSC-RX10M2"),
    (344, "DSC-RX1RM2"),
    (346, "ILCE-QX1"),
    (347, "ILCE-7RM2"),
    (350, "ILCE-7SM2"),
    (353, "ILCA-68"),
    (354, "ILCA-99M2"),
    (355, "DSC-RX10M3"),
    (356, "DSC-RX100M5"),
    (357, "ILCE-6300"),
    (358, "ILCE-9"),
    (360, "ILCE-6500"),
    (362, "ILCE-7RM3"),
    (363, "ILCE-7M3"),
    (364, "DSC-RX0"),
    (365, "DSC-RX10M4"),
    (366, "DSC-RX100M6"),
    (367, "DSC-HX99"),
    (369, "DSC-RX100M5A"),
    (371, "ILCE-6400"),
    (372, "DSC-RX0M2"),
    (373, "DSC-HX95"),
    (374, "DSC-RX100M7"),
    (375, "ILCE-7RM4"),
    (376, "ILCE-9M2"),
    (378, "ILCE-6600"),
    (379, "ILCE-6100"),
    (380, "ZV-1"),
    (381, "ILCE-7C"),
    (382, "ZV-E10"),
    (383, "ILCE-7SM3"),
    (384, "ILCE-1"),
    (385, "ILME-FX3"),
    (386, "ILCE-7RM3A"),
    (387, "ILCE-7RM4A"),
    (388, "ILCE-7M4"),
    (389, "ZV-1F"),
    (390, "ILCE-7RM5"),
    (391, "ILME-FX30"),
    (392, "ILCE-9M3"),
    (393, "ZV-E1"),
    (394, "ILCE-6700"),
    (395, "ZV-1M2"),
    (396, "ILCE-7CR"),
    (397, "ILCE-7CM2"),
    (398, "ILX-LR1"),
    (399, "ZV-E10M2"),
    (400, "ILCE-1M2"),
    (401, "DSC-RX1RM3"),
    (402, "ILCE-6400A"),
    (403, "ILCE-6100A"),
    (404, "DSC-RX100M7A"),
    (406, "ILME-FX2"),
    (407, "ILCE-7M5"),
    (408, "ZV-1A"),
];

/// The second `PrintConv` of `HDR` (0x200a), applied to the second int16u.
/// ExifTool's array-form `PrintConv` runs one hash per component and joins the
/// results with "; ".
static HDR_B: &[(i64, &str)] = &[
    (0, "Uncorrected image"),
    (1, "HDR image (good)"),
    (2, "HDR image (fail 1)"),
    (3, "HDR image (fail 2)"),
];

/// `FileFormat` (0xb000) keys on all four int8u components at once.
static FILE_FORMAT: &[(&str, &str)] = &[
    ("0 0 0 2", "JPEG"),
    ("1 0 0 0", "SR2"),
    ("2 0 0 0", "ARW 1.0"),
    ("3 0 0 0", "ARW 2.0"),
    ("3 1 0 0", "ARW 2.1"),
    ("3 2 0 0", "ARW 2.2"),
    ("3 3 0 0", "ARW 2.3"),
    ("3 3 1 0", "ARW 2.3.1"),
    ("3 3 2 0", "ARW 2.3.2"),
    ("3 3 3 0", "ARW 2.3.3"),
    ("3 3 5 0", "ARW 2.3.5"),
    ("4 0 0 0", "ARW 4.0"),
    ("4 0 1 0", "ARW 4.0.1"),
    ("5 0 0 0", "ARW 5.0"),
    ("5 0 1 0", "ARW 5.0.1"),
    ("6 0 0 0", "ARW 6.0"),
];

/// `CreativeStyle` (0xb020) is a string tag whose `PrintConv` prettifies the
/// camera's own spelling.
static CREATIVE_STYLE: &[(&str, &str)] = &[
    ("AdobeRGB", "Adobe RGB"),
    ("Autumnleaves", "Autumn Leaves"),
    ("BW", "B&W"),
    ("Clear", "Clear"),
    ("Deep", "Deep"),
    ("FL", "FL"),
    ("IN", "IN"),
    ("Landscape", "Landscape"),
    ("Light", "Light"),
    ("Neutral", "Neutral"),
    ("Nightview", "Night View/Portrait"),
    ("None", "None"),
    ("Portrait", "Portrait"),
    ("Real", "Real"),
    ("SH", "SH"),
    ("Sepia", "Sepia"),
    ("Standard", "Standard"),
    ("Sunset", "Sunset"),
    ("VV2", "Vivid 2"),
    ("Vivid", "Vivid"),
];

/// `Quality` (0x202e), written by the ILCE-7M3/7RM3 and newer. Two int16u
/// components keyed together, and a second tag of the same name as 0x0102 -
/// which it overrides, being listed later at equal priority.
static QUALITY2: &[(&str, &str)] = &[
    ("0 0", "n/a"),
    ("0 1", "Standard"),
    ("0 2", "Fine"),
    ("0 3", "Extra Fine"),
    ("0 4", "Light"),
    ("1 0", "RAW"),
    ("1 1", "RAW + Standard"),
    ("1 2", "RAW + Fine"),
    ("1 3", "RAW + Extra Fine"),
    ("1 4", "RAW + Light"),
    ("2 0", "S-size RAW"),
    ("2 1", "S-size RAW + Standard"),
    ("2 2", "S-size RAW + Fine"),
    ("2 3", "S-size RAW + Extra Fine"),
    ("2 4", "S-size RAW + Light"),
    ("3 0", "M-size RAW"),
    ("3 1", "M-size RAW + Standard"),
    ("3 2", "M-size RAW + Fine"),
    ("3 3", "M-size RAW + Extra Fine"),
    ("3 4", "M-size RAW + Light"),
    ("4 0", "Compressed RAW"),
    ("4 1", "Compressed RAW + Standard"),
    ("4 2", "Compressed RAW + Fine"),
    ("4 3", "Compressed RAW + Extra Fine"),
    ("4 4", "Compressed RAW + Light"),
    ("5 0", "Compressed (HQ) RAW"),
    ("5 1", "Compressed (HQ) RAW + Standard"),
    ("5 2", "Compressed (HQ) RAW + Fine"),
    ("5 3", "Compressed (HQ) RAW + Extra Fine"),
    ("5 4", "Compressed (HQ) RAW + Light"),
];

/// `FocusMode` (0x201b).
static FOCUS_MODE_201B: &[(i64, &str)] = &[
    (0, "Manual"),
    (2, "AF-S"),
    (3, "AF-C"),
    (4, "AF-A"),
    (6, "DMF"),
    (7, "AF-D"),
];

/// `AFAreaModeSetting` (0x201c) for `SLT-`/`HV` bodies.
static AF_AREA_MODE_SETTING_SLT: &[(i64, &str)] =
    &[(0, "Wide"), (4, "Local"), (8, "Zone"), (9, "Spot")];

/// `AFAreaModeSetting` (0x201c) for `ILCA-` bodies (Sony.pm:1292-1304).
static AF_AREA_MODE_SETTING_ILCA: &[(i64, &str)] = &[
    (0, "Wide"),
    (4, "Flexible Spot"),
    (8, "Zone"),
    (9, "Center"),
    (12, "Expanded Flexible Spot"),
];

/// `AFTracking` (0x2021).
static AF_TRACKING: &[(i64, &str)] = &[(0, "Off"), (1, "Face tracking"), (2, "Lock On AF")];

/// `AFPointSelected` (0x201e) for `SLT-`/`HV` bodies, and for an ILCE/ILME
/// whose `AFAreaModeSetting` is 4 -- an A-mount lens on an LA-EA2/EA4 adapter
/// (Sony.pm:1326-1354).
static AF_POINT_SELECTED_SLT: &[(i64, &str)] = &[
    (0, "Auto"),
    (1, "Center"),
    (2, "Top"),
    (3, "Upper-right"),
    (4, "Right"),
    (5, "Lower-right"),
    (6, "Bottom"),
    (7, "Lower-left"),
    (8, "Left"),
    (9, "Upper-left"),
    (10, "Far Right"),
    (11, "Far Left"),
    (12, "Upper-middle"),
    (13, "Near Right"),
    (14, "Lower-middle"),
    (15, "Near Left"),
    (16, "Upper Far Right"),
    (17, "Lower Far Right"),
    (18, "Lower Far Left"),
    (19, "Upper Far Left"),
];

/// `AFAreaModeSetting` (0x201c) for NEX, ILCE, ILME, ZV and the eight DSC
/// bodies ExifTool names (Sony.pm:1275-1289). `0` prints as `Wide` even on the
/// NEX and ILCE-3000/3500, where Sony's own menu calls it `Multi`.
static AF_AREA_MODE_SETTING_ILCE: &[(i64, &str)] = &[
    (0, "Wide"),
    (1, "Center"),
    (3, "Flexible Spot"),
    (4, "Flexible Spot (LA-EA4)"),
    (9, "Center (LA-EA4)"),
    (11, "Zone"),
    (12, "Expanded Flexible Spot"),
    (13, "Custom AF Area"),
];

/// `AFPointSelected` (0x201e) for the ILCA-68 and ILCA-77M2 (Sony.pm:1357-1368),
/// `%afPoints79` plus the `-1 => 'Auto'` and `39 => 'E6 (Center)'` the arm adds.
/// Keys are post-`ValueConv`, which is `$val - 1`.
static AF_POINT_SELECTED_ILCA_79: &[(i64, &str)] = &[
    (-1, "Auto"),
    (0, "A5"),
    (1, "A6"),
    (2, "A7"),
    (3, "B2"),
    (4, "B3"),
    (5, "B4"),
    (6, "B5"),
    (7, "B6"),
    (8, "B7"),
    (9, "B8"),
    (10, "B9"),
    (11, "B10"),
    (12, "C1"),
    (13, "C2"),
    (14, "C3"),
    (15, "C4"),
    (16, "C5"),
    (17, "C6"),
    (18, "C7"),
    (19, "C8"),
    (20, "C9"),
    (21, "C10"),
    (22, "C11"),
    (23, "D1"),
    (24, "D2"),
    (25, "D3"),
    (26, "D4"),
    (27, "D5"),
    (28, "D6"),
    (29, "D7"),
    (30, "D8"),
    (31, "D9"),
    (32, "D10"),
    (33, "D11"),
    (34, "E1"),
    (35, "E2"),
    (36, "E3"),
    (37, "E4"),
    (38, "E5"),
    (39, "E6 (Center)"),
    (40, "E7"),
    (41, "E8"),
    (42, "E9"),
    (43, "E10"),
    (44, "E11"),
    (45, "F1"),
    (46, "F2"),
    (47, "F3"),
    (48, "F4"),
    (49, "F5"),
    (50, "F6"),
    (51, "F7"),
    (52, "F8"),
    (53, "F9"),
    (54, "F10"),
    (55, "F11"),
    (56, "G1"),
    (57, "G2"),
    (58, "G3"),
    (59, "G4"),
    (60, "G5"),
    (61, "G6"),
    (62, "G7"),
    (63, "G8"),
    (64, "G9"),
    (65, "G10"),
    (66, "G11"),
    (67, "H2"),
    (68, "H3"),
    (69, "H4"),
    (70, "H5"),
    (71, "H6"),
    (72, "H7"),
    (73, "H8"),
    (74, "H9"),
    (75, "H10"),
    (76, "I5"),
    (77, "I6"),
    (78, "I7"),
];

/// `AFPointSelected` (0x201e) for the ILCA-99M2 (Sony.pm:1370-1381), `%afPoints99M2`
/// plus `0 => 'Auto'` and `162 => 'E6 (162, Center)'`. Its `OTHER` sub is
/// `sub { shift }`, so an unmatched value passes through unchanged.
static AF_POINT_SELECTED_ILCA_99M2: &[(i64, &str)] = &[
    (0, "Auto"),
    (93, "A5 (93)"),
    (94, "A6 (94)"),
    (95, "A7 (95)"),
    (106, "B2 (106)"),
    (107, "B3 (107)"),
    (108, "B4 (108)"),
    (110, "B5 (110)"),
    (111, "B6 (111)"),
    (112, "B7 (112)"),
    (114, "B8 (114)"),
    (115, "B9 (115)"),
    (116, "B10 (116)"),
    (122, "C1 (122)"),
    (123, "C2 (123)"),
    (124, "C3 (124)"),
    (125, "C4 (125)"),
    (127, "C5 (127)"),
    (128, "C6 (128)"),
    (129, "C7 (129)"),
    (131, "C8 (131)"),
    (132, "C9 (132)"),
    (133, "C10 (133)"),
    (134, "C11 (134)"),
    (139, "D1 (139)"),
    (140, "D2 (140)"),
    (141, "D3 (141)"),
    (142, "D4 (142)"),
    (144, "D5 (144)"),
    (145, "D6 (145)"),
    (146, "D7 (146)"),
    (148, "D8 (148)"),
    (149, "D9 (149)"),
    (150, "D10 (150)"),
    (151, "D11 (151)"),
    (156, "E1 (156)"),
    (157, "E2 (157)"),
    (158, "E3 (158)"),
    (159, "E4 (159)"),
    (161, "E5 (161)"),
    (162, "E6 (162, Center)"),
    (163, "E7 (163)"),
    (165, "E8 (165)"),
    (166, "E9 (166)"),
    (167, "E10 (167)"),
    (168, "E11 (168)"),
    (173, "F1 (173)"),
    (174, "F2 (174)"),
    (175, "F3 (175)"),
    (176, "F4 (176)"),
    (178, "F5 (178)"),
    (179, "F6 (179)"),
    (180, "F7 (180)"),
    (182, "F8 (182)"),
    (183, "F9 (183)"),
    (184, "F10 (184)"),
    (185, "F11 (185)"),
    (190, "G1 (190)"),
    (191, "G2 (191)"),
    (192, "G3 (192)"),
    (193, "G4 (193)"),
    (195, "G5 (195)"),
    (196, "G6 (196)"),
    (197, "G7 (197)"),
    (199, "G8 (199)"),
    (200, "G9 (200)"),
    (201, "G10 (201)"),
    (202, "G11 (202)"),
    (208, "H2 (208)"),
    (209, "H3 (209)"),
    (210, "H4 (210)"),
    (212, "H5 (212)"),
    (213, "H6 (213)"),
    (214, "H7 (214)"),
    (216, "H8 (216)"),
    (217, "H9 (217)"),
    (218, "H10 (218)"),
    (229, "I5 (229)"),
    (230, "I6 (230)"),
    (231, "I7 (231)"),
];

/// `AFPointSelected` (0x201e) for any ILCA with `AFAreaModeSetting` set to Zone
/// (Sony.pm:1383-1399).
static AF_POINT_SELECTED_ILCA_ZONE: &[(i64, &str)] = &[
    (0, "n/a"),
    (1, "Top Left Zone"),
    (2, "Top Zone"),
    (3, "Top Right Zone"),
    (4, "Left Zone"),
    (5, "Center Zone"),
    (6, "Right Zone"),
    (7, "Bottom Left Zone"),
    (8, "Bottom Zone"),
    (9, "Bottom Right Zone"),
];

/// `AFPointSelected` (0x201e) for NEX, ILCE, ILME, ZV and DSC-RX bodies
/// (Sony.pm:1403-1419); non-zero only when the AF area is a Zone.
static AF_POINT_SELECTED_ILCE_ZONE: &[(i64, &str)] = &[
    (0, "n/a"),
    (1, "Center Zone"),
    (2, "Top Zone"),
    (3, "Right Zone"),
    (4, "Left Zone"),
    (5, "Bottom Zone"),
    (6, "Bottom Right Zone"),
    (7, "Bottom Left Zone"),
    (8, "Top Left Zone"),
    (9, "Top Right Zone"),
];

// ============================================================================
// Table definition
// ============================================================================

/// The state a `Main` entry's `Condition` can test.
///
/// ExifTool threads these as `$$self{...}` data members filled while it walks
/// the IFD, so a tag sees only what entries *before* it wrote. The Sony walk in
/// `sony.rs` is in file order for exactly that reason, and this carries the
/// same discipline into the table: a member is `None` until the entry that
/// defines it has been read.
pub struct MainCtx<'a> {
    /// The EXIF `Model`, which most Sony `Condition`s key on.
    pub model: Option<&'a str>,
    /// The raw value of `AFAreaModeSetting` (0x201c). ExifTool stores it as
    /// `$$self{AFAreaILCE}` on the NEX/ILCE arm (Sony.pm:1279) and as
    /// `$$self{AFAreaILCA}` on the ILCA arm (Sony.pm:1297); which name it lands
    /// under is decided by the same `Model` test the readers apply, so one
    /// field serves both.
    pub af_area_mode_setting: Option<i64>,
}

/// How a `Main`-table entry turns its raw IFD value into ExifTool's string.
pub enum Print {
    /// Print the first component as an integer.
    Int,
    /// `PrintConv` hash on the first component.
    Map(&'static [(i64, &'static str)]),
    /// The same, declared `PrintHex`, so misses print in hexadecimal.
    MapHex(&'static [(i64, &'static str)]),
    /// ExifTool's `$val > 0 ? "+$val" : $val` slider rendering.
    Adjust,
    /// Anything with real logic. `ctx` carries the EXIF `Model`, which many
    /// Sony tags condition on, and the `$$self{...}` data members ExifTool
    /// fills as it walks the IFD. Returning `None` drops the tag.
    Fn(fn(&SonyValue<'_>, &MainCtx<'_>) -> Option<String>),
}

/// One `Main`-table entry.
pub struct MainTag {
    /// IFD tag id.
    pub id: u16,
    /// Tag name, without group prefix.
    pub name: &'static str,
    /// How to print it.
    pub print: Print,
    /// A raw value ExifTool's `RawConv` maps to `undef`, dropping the tag
    /// before any `PrintConv` runs. Sony writes 65535 in a whole block of
    /// 0xb0xx tags to mean "this body does not report this", and the
    /// difference between dropping it and printing "n/a" is visible in the
    /// output.
    pub drop_raw: Option<i64>,
    /// ExifTool's `Priority`, which decides which copy of a duplicated tag
    /// name survives. Defaults to 1; Sony overrides it on a couple of tags it
    /// considers less reliable than a same-named sibling.
    pub priority: u8,
}

const fn tag(id: u16, name: &'static str, print: Print) -> MainTag {
    MainTag {
        id,
        name,
        print,
        drop_raw: None,
        priority: 1,
    }
}

impl MainTag {
    /// Marks a raw value as ExifTool's `RawConv => undef` case.
    const fn drop_when(mut self, raw: i64) -> Self {
        self.drop_raw = Some(raw);
        self
    }

    /// Overrides ExifTool's default `Priority`.
    const fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }
}

const fn map(id: u16, name: &'static str, m: &'static [(i64, &'static str)]) -> MainTag {
    tag(id, name, Print::Map(m))
}

const fn map_hex(id: u16, name: &'static str, m: &'static [(i64, &'static str)]) -> MainTag {
    tag(id, name, Print::MapHex(m))
}

const fn int(id: u16, name: &'static str) -> MainTag {
    tag(id, name, Print::Int)
}

const fn func(
    id: u16,
    name: &'static str,
    f: fn(&SonyValue<'_>, &MainCtx<'_>) -> Option<String>,
) -> MainTag {
    tag(id, name, Print::Fn(f))
}

/// The Sony `Main` table, in ExifTool's tag-id order.
///
/// Deliberately absent, because ExifTool defines them but never prints them
/// from these files:
/// * 0x2004/0x2005/0x2006 `Contrast`/`Saturation`/`Sharpness` and 0xb041
///   `ExposureMode` - duplicates of the identically-named standard EXIF tags,
///   which outrank them in ExifTool's duplicate suppression.
/// * 0x2001 `PreviewImage` - the preview lives outside the MakerNote block, so
///   this parser cannot see its bytes. ExifTool's own behaviour splits on
///   whether the read succeeds: SonyDSLR-A350.jpg gets the "Binary data"
///   placeholder because the range runs past EOF and ExifTool never seeks,
///   while SonySLT-A77.jpg and SonyILCA-77M2.jpg get no tag at all
///   ("[minor] Error reading PreviewImage"). Emitting it unconditionally would
///   invent the tag for two of the three.
/// * 0x2000, 0x2003, 0x200c, 0x200d, 0x2015, 0x2018, 0x2019, 0x201d, 0x201f,
///   0x2022, 0x2025, 0x5001, 0x5002, 0xb045, 0xb046, 0xb04c, 0xb04d, 0xb04e,
///   0xb050, 0xb051, 0xb053 - named `Sony_0xNNNN` and flagged `Unknown`, so
///   they are hidden without `-u`.
pub static MAIN_TABLE: &[MainTag] = &[
    map(0x0102, "Quality", QUALITY),
    func(0x0104, "FlashExposureComp", |v, _cx| {
        v.rational(0).map(print_float)
    }),
    map_hex(0x0105, "Teleconverter", TELECONVERTER),
    int(0x0112, "WhiteBalanceFineTune"),
    // `Priority => 2`: ExifTool trusts this over 0xb054, which carries the
    // same name.
    map_hex(0x0115, "WhiteBalance", WHITE_BALANCE_0115).with_priority(2),
    int(0x2002, "Rating"),
    map_hex(0x2008, "LongExposureNoiseReduction", LONG_EXPOSURE_NR),
    map(0x2009, "HighISONoiseReduction", HIGH_ISO_NR),
    func(0x200a, "HDR", |v, _cx| {
        // Stored as one int32u but read as two int16u, each with its own
        // PrintConv; ExifTool joins the components with "; ".
        let words = v.as_u16_pair()?;
        Some(format!(
            "{}; {}",
            lookup(HDR_A, words.0).unwrap_or_else(|| unknown_hex(words.0)),
            lookup(HDR_B, words.1).unwrap_or_else(|| unknown_hex(words.1)),
        ))
    }),
    map(0x200b, "MultiFrameNoiseReduction", MULTI_FRAME_NR),
    map(0x200e, "PictureEffect", PICTURE_EFFECT),
    map(0x200f, "SoftSkinEffect", SOFT_SKIN_EFFECT),
    map(0x2011, "VignettingCorrection", CORRECTION_SETTING),
    map(0x2012, "LateralChromaticAberration", CORRECTION_SETTING),
    map(0x2013, "DistortionCorrectionSetting", CORRECTION_SETTING),
    func(0x2014, "WBShiftAB_GM", |v, _cx| Some(v.join_ints())),
    map(0x2016, "AutoPortraitFramed", AUTO_PORTRAIT_FRAMED),
    map(0x2017, "FlashAction", FLASH_ACTION_MAIN),
    map(0x201a, "ElectronicFrontCurtainShutter", EFCS),
    func(0x201b, "FocusMode", |v, cx| {
        // ExifTool restricts this to non-DSC bodies plus a named handful of
        // late DSC models; only the non-DSC half is reachable here.
        let model = cx.model?;
        (!model.starts_with("DSC-")).then(|| ())?;
        let raw = v.first_int()?;
        Some(lookup(FOCUS_MODE_201B, raw).unwrap_or_else(|| unknown(raw)))
    }),
    func(0x201c, "AFAreaModeSetting", |v, cx| {
        let raw = v.first_int()?;
        let table = af_area_mode_setting_table(cx.model?)?;
        Some(lookup(table, raw).unwrap_or_else(|| unknown(raw)))
    }),
    func(0x201e, "AFPointSelected", |v, cx| {
        // ExifTool's five arms, in its order (Sony.pm:1321-1421). The three
        // ILCA arms are gated on `$$self{AFAreaILCA}` and the ILCE half of the
        // first on `$$self{AFAreaILCE}`; both are the raw AFAreaModeSetting
        // this walk recorded when it passed 0x201c.
        let model = cx.model?;
        let raw = v.first_int()?;

        // Arm 1: SLT/HV outright, or an ILCE/ILME reporting AFAreaModeSetting 4
        // -- `Flexible Spot (LA-EA4)`, i.e. an A-mount lens on an adapter, which
        // is what puts the SLT's phase-detect point names back in play.
        if model.starts_with("SLT-")
            || model.starts_with("HV")
            || ((model.starts_with("ILCE-") || model.starts_with("ILME-"))
                && cx.af_area_mode_setting == Some(4))
        {
            return Some(lookup(AF_POINT_SELECTED_SLT, raw).unwrap_or_else(|| unknown(raw)));
        }

        if model.starts_with("ILCA-") {
            // Every ILCA arm requires the data member to be defined; when 0x201c
            // was absent no arm matches and ExifTool prints nothing.
            let af_area = cx.af_area_mode_setting?;
            // Arm 4: any ILCA whose AF area is Zone reads the zone table.
            if af_area == 8 {
                return Some(
                    lookup(AF_POINT_SELECTED_ILCA_ZONE, raw).unwrap_or_else(|| unknown(raw)),
                );
            }
            // Arm 2: `ValueConv => '$val - 1'` runs before the PrintConv, so the
            // lookup and any "Unknown (n)" both use the shifted number.
            if model.starts_with("ILCA-68") || model.starts_with("ILCA-77M2") {
                let shifted = raw - 1;
                return Some(
                    lookup(AF_POINT_SELECTED_ILCA_79, shifted).unwrap_or_else(|| unknown(shifted)),
                );
            }
            // Arm 3: `OTHER => sub { shift }` prints an unmatched value as the
            // bare number rather than wrapping it.
            if model.starts_with("ILCA-99M2") {
                return Some(
                    lookup(AF_POINT_SELECTED_ILCA_99M2, raw).unwrap_or_else(|| raw.to_string()),
                );
            }
            return None;
        }

        // Arm 5.
        if is_ilce_af_point_body(model) {
            return Some(lookup(AF_POINT_SELECTED_ILCE_ZONE, raw).unwrap_or_else(|| unknown(raw)));
        }
        None
    }),
    map(0x2021, "AFTracking", AF_TRACKING),
    map(0x2023, "MultiFrameNREffect", MULTI_FRAME_NR_EFFECT),
    func(0x202e, "Quality", |v, _cx| {
        let key = v.join_ints();
        QUALITY2
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, name)| (*name).to_string())
            .or_else(|| Some(format!("Unknown ({})", key)))
    }),
    func(0xb000, "FileFormat", |v, _cx| {
        let key = v.join_ints();
        FILE_FORMAT
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, name)| (*name).to_string())
            .or_else(|| Some(format!("Unknown ({})", key)))
    }),
    map(0xb001, "SonyModelID", SONY_MODEL_ID),
    func(0xb020, "CreativeStyle", |v, _cx| {
        let raw = v.string()?;
        Some(
            CREATIVE_STYLE
                .iter()
                .find(|(k, _)| *k == raw)
                .map(|(_, name)| (*name).to_string())
                .unwrap_or(raw),
        )
    }),
    func(0xb021, "ColorTemperature", |v, _cx| {
        let raw = v.first_int()?;
        Some(match raw {
            0 => "Auto".to_string(),
            0xffff_ffff => "n/a".to_string(),
            other => other.to_string(),
        })
    }),
    func(0xb022, "ColorCompensationFilter", |v, _cx| {
        v.first_int_as::<i32>().map(|v| v.to_string())
    }),
    map(0xb023, "SceneMode", SCENE_MODE),
    map(0xb024, "ZoneMatching", ZONE_MATCHING),
    map(0xb025, "DynamicRangeOptimizer", DYNAMIC_RANGE_OPTIMIZER),
    map(0xb026, "ImageStabilization", IMAGE_STABILIZATION),
    func(0xb027, "LensType", |v, _cx| {
        let raw = v.first_int()?;
        Some(
            sony_lenses::lookup(u16::try_from(raw).ok()?)
                .map(|s| s.to_string())
                .unwrap_or_else(|| unknown(raw)),
        )
    }),
    map(0xb029, "ColorMode", COLOR_MODE),
    func(0xb02a, "LensSpec", |v, _cx| print_lens_spec(v.bytes())),
    func(0xb02b, "FullImageSize", |v, _cx| v.reversed_size()),
    func(0xb02c, "PreviewImageSize", |v, _cx| v.reversed_size()),
    map(0xb040, "Macro", MACRO).drop_when(65535),
    map(0xb044, "AFIlluminator", AF_ILLUMINATOR).drop_when(65535),
    map(0xb047, "JPEGQuality", JPEG_QUALITY).drop_when(65535),
    func(0xb048, "FlashLevel", |v, cx| {
        let raw = v.first_int_as::<i16>()? as i64;
        // RawConv drops the A100's -1; every other body reports a real level.
        if raw == -1 && cx.model.is_some_and(|m| m.starts_with("DSLR-A100")) {
            return None;
        }
        Some(lookup(FLASH_LEVEL, raw).unwrap_or_else(|| unknown(raw)))
    }),
    map(0xb049, "ReleaseMode", RELEASE_MODE).drop_when(65535),
    // `OTHER => sub { shift }`: a burst position other than 0 prints as the
    // bare number, not as "Unknown (N)".
    func(0xb04a, "SequenceNumber", |v, _cx| {
        let raw = v.first_int()?;
        (raw != 65535).then(|| lookup(SEQUENCE_NUMBER, raw).unwrap_or_else(|| raw.to_string()))
    }),
    map(0xb04b, "Anti-Blur", ANTI_BLUR).drop_when(65535),
    // `Priority => 0`, noted "unreliable for the A77": 0xb025 wins even though
    // it is listed earlier.
    map(0xb04f, "DynamicRangeOptimizer", DRO_B04F).with_priority(0),
    map(0xb052, "IntelligentAuto", INTELLIGENT_AUTO),
    map(0xb054, "WhiteBalance", WHITE_BALANCE_B054),
];

/// `AFAreaModeSetting` uses a different `PrintConv` per body family; the
/// families ExifTool distinguishes that can reach this code are the A-mount
/// SLT/HV bodies and the ILCA bodies.
fn af_area_mode_setting_table(model: &str) -> Option<&'static [(i64, &'static str)]> {
    if model.starts_with("SLT-") || model.starts_with("HV") {
        Some(AF_AREA_MODE_SETTING_SLT)
    } else if is_ilce_af_area_body(model) {
        Some(AF_AREA_MODE_SETTING_ILCE)
    } else if model.starts_with("ILCA-") {
        Some(AF_AREA_MODE_SETTING_ILCA)
    } else {
        None
    }
}

/// Sony.pm:1276 --
/// `/^(NEX-|ILCE-|ILME-|ZV-|DSC-(RX10M4|RX100M6|RX100M7|RX100M5A|HX95|HX99|RX0M2|RX1RM3))/`
///
/// The DSC alternatives are unanchored at their right-hand end, so `DSC-RX100M7A`
/// matches through `RX100M7`, exactly as the Perl does.
fn is_ilce_af_area_body(model: &str) -> bool {
    const DSC: &[&str] = &[
        "RX10M4", "RX100M6", "RX100M7", "RX100M5A", "HX95", "HX99", "RX0M2", "RX1RM3",
    ];
    model.starts_with("NEX-")
        || model.starts_with("ILCE-")
        || model.starts_with("ILME-")
        || model.starts_with("ZV-")
        || model
            .strip_prefix("DSC-")
            .is_some_and(|rest| DSC.iter().any(|d| rest.starts_with(d)))
}

/// Sony.pm:1406 -- `/^(NEX-|ILCE-|ILME-|ZV-|DSC-RX)/`, the wider body list the
/// last `AFPointSelected` arm uses. Every `DSC-RX` qualifies here, not just the
/// eight `AFAreaModeSetting` names them.
fn is_ilce_af_point_body(model: &str) -> bool {
    model.starts_with("NEX-")
        || model.starts_with("ILCE-")
        || model.starts_with("ILME-")
        || model.starts_with("ZV-")
        || model.starts_with("DSC-RX")
}

impl Print {
    /// Renders `value` the way ExifTool would, or `None` to drop the tag.
    pub fn apply(&self, value: &SonyValue<'_>, ctx: &MainCtx<'_>) -> Option<String> {
        match self {
            Print::Int => value.first_int().map(|v| v.to_string()),
            Print::Map(m) => {
                let raw = value.first_int()?;
                Some(lookup(m, raw).unwrap_or_else(|| unknown(raw)))
            }
            Print::MapHex(m) => {
                let raw = value.first_int()?;
                Some(lookup(m, raw).unwrap_or_else(|| unknown_hex(raw)))
            }
            Print::Adjust => value.first_int().map(signed_adjustment),
            Print::Fn(f) => f(value, ctx),
        }
    }
}

impl MainTag {
    /// Renders the value, honouring the `RawConv` that drops it entirely.
    pub fn render(&self, value: &SonyValue<'_>, ctx: &MainCtx<'_>) -> Option<String> {
        if let Some(dropped) = self.drop_raw
            && value.first_int() == Some(dropped)
        {
            return None;
        }
        self.print.apply(value, ctx)
    }
}

/// Looks up a `Main`-table entry by tag id.
pub fn main_tag(id: u16) -> Option<&'static MainTag> {
    MAIN_TABLE.iter().find(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::tiff::ifd_parser::ByteOrder;

    #[test]
    fn quality_printconv_matches_exiftool() {
        assert_eq!(lookup(QUALITY, 2), Some("Fine".to_string()));
        assert_eq!(lookup(QUALITY, 5), Some("Extra Fine".to_string()));
        assert_eq!(lookup(QUALITY, 6), Some("RAW + JPEG/HEIF".to_string()));
    }

    #[test]
    fn white_balance_0115_is_keyed_in_hex_steps() {
        // 0x70 is the value DSLR-A350 writes; ExifTool prints "Custom".
        assert_eq!(lookup(WHITE_BALANCE_0115, 0x70), Some("Custom".to_string()));
        assert_eq!(lookup(WHITE_BALANCE_0115, 0), Some("Auto".to_string()));
    }

    #[test]
    fn every_tag_id_appears_once_except_the_two_exiftool_repeats() {
        // DynamicRangeOptimizer and WhiteBalance genuinely appear twice in
        // ExifTool's table under different ids; no id may repeat.
        let mut ids: Vec<u16> = MAIN_TABLE.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate tag id in MAIN_TABLE");
    }

    #[test]
    fn sony_model_id_covers_the_corpus_bodies() {
        assert_eq!(lookup(SONY_MODEL_ID, 256), Some("DSLR-A100".to_string()));
        assert_eq!(lookup(SONY_MODEL_ID, 260), Some("DSLR-A350".to_string()));
        assert_eq!(lookup(SONY_MODEL_ID, 319), Some("ILCA-77M2".to_string()));
    }

    /// One int8u component, which is how Sony writes both 0x201c and 0x201e.
    fn int8u(v: u8) -> SonyValue<'static> {
        SonyValue::new(1, 1, vec![v], ByteOrder::LittleEndian)
    }

    fn ctx(model: &str, af_area: Option<i64>) -> MainCtx<'_> {
        MainCtx {
            model: Some(model),
            af_area_mode_setting: af_area,
        }
    }

    fn render(id: u16, raw: u8, model: &str, af_area: Option<i64>) -> Option<String> {
        main_tag(id)
            .expect("tag in MAIN_TABLE")
            .render(&int8u(raw), &ctx(model, af_area))
    }

    /// `exiftool -G1 -s combined-samples/Sony/*.jpg`, byte for byte. Each raw
    /// value is the one that file stores, read with `exiftool -v2`.
    #[test]
    fn af_area_mode_setting_picks_exiftools_arm_per_body() {
        // SLT arm (Sony.pm:1264): SonySLT-A58.jpg stores 9 and prints "Spot",
        // which is "Center" on the ILCA arm -- the two tables disagree on the
        // same number, so picking the wrong arm is a wrong value, not a gap.
        assert_eq!(render(0x201c, 9, "SLT-A58", None).as_deref(), Some("Spot"));
        assert_eq!(
            render(0x201c, 9, "ILCA-68", None).as_deref(),
            Some("Center")
        );
        // NEX/ILCE arm (Sony.pm:1275), previously absent entirely.
        assert_eq!(
            render(0x201c, 0, "ILCE-6000", None).as_deref(),
            Some("Wide")
        );
        assert_eq!(
            render(0x201c, 1, "ILCE-7S", None).as_deref(),
            Some("Center")
        );
        assert_eq!(
            render(0x201c, 3, "ILCE-6600", None).as_deref(),
            Some("Flexible Spot")
        );
        assert_eq!(render(0x201c, 11, "NEX-5T", None).as_deref(), Some("Zone"));
        // The DSC alternatives are a fixed list; DSC-RX100M7A matches through
        // the RX100M7 alternative, and a DSC outside the list gets no arm.
        assert_eq!(
            render(0x201c, 0, "DSC-RX100M7A", None).as_deref(),
            Some("Wide")
        );
        assert_eq!(render(0x201c, 0, "DSC-W120", None), None);
    }

    #[test]
    fn af_point_selected_follows_exiftools_five_arms() {
        // Arm 1, SLT: SonySLT-A99.jpg stores 16.
        assert_eq!(
            render(0x201e, 16, "SLT-A99", Some(4)).as_deref(),
            Some("Upper Far Right")
        );
        // Arm 1's ILCE half needs AFAreaModeSetting == 4; without it the body
        // falls to arm 5, where the same raw value means something else.
        assert_eq!(
            render(0x201e, 4, "ILCE-7RM2", Some(4)).as_deref(),
            Some("Right")
        );
        assert_eq!(
            render(0x201e, 4, "ILCE-7RM2", Some(0)).as_deref(),
            Some("Left Zone")
        );
        // Arm 2: ValueConv is $val - 1, so the stored 0 is -1 -> "Auto".
        assert_eq!(
            render(0x201e, 0, "ILCA-77M2", Some(9)).as_deref(),
            Some("Auto")
        );
        assert_eq!(
            render(0x201e, 40, "ILCA-77M2", Some(9)).as_deref(),
            Some("E6 (Center)")
        );
        // Arm 3: no ValueConv, and OTHER passes an unmatched value through.
        assert_eq!(
            render(0x201e, 0, "ILCA-99M2", Some(0)).as_deref(),
            Some("Auto")
        );
        assert_eq!(
            render(0x201e, 162, "ILCA-99M2", Some(0)).as_deref(),
            Some("E6 (162, Center)")
        );
        assert_eq!(
            render(0x201e, 5, "ILCA-99M2", Some(0)).as_deref(),
            Some("5")
        );
        // Arm 4: any ILCA whose AF area is Zone reads the zone names instead.
        assert_eq!(
            render(0x201e, 5, "ILCA-77M2", Some(8)).as_deref(),
            Some("Center Zone")
        );
        // Arm 5.
        assert_eq!(
            render(0x201e, 0, "ILCE-6000", Some(0)).as_deref(),
            Some("n/a")
        );
        assert_eq!(render(0x201e, 0, "DSC-RX1", None).as_deref(), Some("n/a"));
        // No arm matches an ILCA with the data member undefined, and none
        // matches a DSC outside the DSC-RX family.
        assert_eq!(render(0x201e, 0, "ILCA-77M2", None), None);
        assert_eq!(render(0x201e, 0, "DSC-W120", None), None);
    }
}
