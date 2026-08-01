//! Binary-data sub-directories written by the A-mount DSLR bodies.
//!
//! `CameraInfo2` (tag 0x0010), `FocusInfo` (0x0020) and `CameraSettings`
//! (0x0114), transcribed from `Image::ExifTool::Sony`. Which table a body
//! selects is decided by the sub-directory's byte count, not by the model, so
//! the count checks below are the same ones ExifTool's `Condition` uses.

use super::binary::{
    BinTable, BinTag, Fmt, print_exposure_time, print_f_number, read_scalar, signed_adjustment,
};
use super::lens_spec::print_lens_spec;
use crate::io::EndianReader;
use std::collections::HashMap;

// ============================================================================
// Shared conversions
// ============================================================================

/// `Image::ExifTool::Minolta::afStatusInfo`: 0 is in focus, -32768 is no
/// reading at all, and anything else is a signed defocus amount.
fn af_status(value: i64) -> Option<String> {
    Some(match value {
        0 => "In Focus".to_string(),
        -32768 => "Out of Focus".to_string(),
        v if v < 0 => format!("Front Focus ({})", v),
        v => format!("Back Focus (+{})", v),
    })
}

/// `$val ? exp(($val/8-6)*log(2))*100 : $val` then `sprintf("%.0f")`, or
/// "Auto" at zero.
fn iso_setting(value: i64) -> Option<String> {
    if value == 0 {
        return Some("Auto".to_string());
    }
    let iso = ((value as f64 / 8.0 - 6.0) * std::f64::consts::LN_2).exp() * 100.0;
    Some(format!("{:.0}", iso))
}

/// `$val ? 2 ** (6 - $val/8) : 0`, printed as an exposure time; zero is Bulb.
fn shutter_speed(value: i64) -> Option<String> {
    if value == 0 {
        return Some("Bulb".to_string());
    }
    Some(print_exposure_time(2f64.powf(6.0 - value as f64 / 8.0)))
}

/// `2 ** (($val/8 - 1) / 2)`, printed as an f-number.
fn aperture(value: i64) -> Option<String> {
    Some(print_f_number(2f64.powf((value as f64 / 8.0 - 1.0) / 2.0)))
}

/// `($val - 128) / 24` then `$val ? sprintf("%+.1f",$val) : 0`.
fn exposure_compensation(value: i64) -> Option<String> {
    let ev = (value as f64 - 128.0) / 24.0;
    Some(if ev == 0.0 {
        "0".to_string()
    } else {
        format!("{:+.1}", ev)
    })
}

/// `$val * 100` printed as "N K".
fn color_temperature(value: i64) -> Option<String> {
    Some(format!("{} K", value * 100))
}

/// `$val > 128 ? $val - 256 : $val`, the byte-sized signed encoding Sony uses
/// for colour-compensation filters and white-balance fine tuning.
fn wrapped_signed(value: i64) -> i64 {
    if value > 128 { value - 256 } else { value }
}

/// The same, printed with ExifTool's `+N` slider convention.
fn color_compensation(value: i64) -> Option<String> {
    Some(signed_adjustment(wrapped_signed(value)))
}

/// `$val - 10` printed with the `+N` slider convention.
fn offset_by_ten(value: i64) -> Option<String> {
    Some(signed_adjustment(value - 10))
}

/// `"$val%"`.
fn percent(value: i64) -> Option<String> {
    Some(format!("{}%", value))
}

/// `FocusStatus`: two named values plus a BITMASK for everything else.
fn focus_status(value: i64) -> Option<String> {
    Some(match value {
        0 => "Not confirmed".to_string(),
        4 => "Not confirmed, Tracking".to_string(),
        bits => {
            let names = [(0, "Confirmed"), (1, "Failed"), (2, "Tracking")]
                .iter()
                .filter(|(bit, _)| bits & (1 << bit) != 0)
                .map(|(_, name)| *name)
                .collect::<Vec<_>>();
            if names.is_empty() {
                format!("Unknown ({})", bits)
            } else {
                names.join(", ")
            }
        }
    })
}

// ============================================================================
// CameraInfo2 (tag 0x0010, count 5506 or 6118)
// ============================================================================

static AF_POINT_SELECTED: &[(i64, &str)] = &[
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
];

static FOCUS_MODE_SETTING: &[(i64, &str)] = &[
    (0, "Manual"),
    (1, "AF-S"),
    (2, "AF-C"),
    (3, "AF-A"),
    (4, "DMF"),
];

static AF_POINT: &[(i64, &str)] = &[
    (0, "Top-right"),
    (1, "Bottom-right"),
    (2, "Bottom"),
    (3, "Middle Horizontal"),
    (4, "Center Vertical"),
    (5, "Top"),
    (6, "Top-left"),
    (7, "Bottom-left"),
];

/// An `AFStatus*` sensor reading: `int16s` at a byte offset, `afStatusInfo`.
const fn af(index: usize, name: &'static str) -> BinTag {
    BinTag::conv(index, name, af_status).with_fmt(Fmt::I16)
}

static CAMERA_INFO2_TAGS: &[BinTag] = &[
    BinTag::map(0x14, "AFPointSelected", AF_POINT_SELECTED),
    BinTag::map(0x15, "FocusModeSetting", FOCUS_MODE_SETTING),
    BinTag::map(0x18, "AFPoint", AF_POINT),
    af(0x1b, "AFStatusActiveSensor"),
    af(0x1d, "AFStatusTop-right"),
    af(0x1f, "AFStatusBottom-right"),
    af(0x21, "AFStatusBottom"),
    af(0x23, "AFStatusMiddleHorizontal"),
    af(0x25, "AFStatusCenterVertical"),
    af(0x27, "AFStatusTop"),
    af(0x29, "AFStatusTop-left"),
    af(0x2b, "AFStatusBottom-left"),
    af(0x2d, "AFStatusLeft"),
    af(0x2f, "AFStatusCenterHorizontal"),
    af(0x31, "AFStatusRight"),
];

static CAMERA_INFO2: BinTable = BinTable {
    format: Fmt::U8,
    big_endian: false,
    tags: CAMERA_INFO2_TAGS,
};

/// Byte counts ExifTool uses to recognise a `CameraInfo2` block:
/// A200/A300/A350 write 5506, A230/A290/A330/A380/A390 write 6118.
const CAMERA_INFO2_COUNTS: [usize; 2] = [5506, 6118];

// ============================================================================
// FocusInfo (tag 0x0020, count 19154 or 19148)
// ============================================================================

static ROTATION: &[(i64, &str)] = &[
    (0, "Horizontal (normal)"),
    (1, "Rotate 270 CW"),
    (2, "Rotate 90 CW"),
];

static ON_OFF: &[(i64, &str)] = &[(0, "Off"), (1, "On")];

static DRO_MODE: &[(i64, &str)] = &[
    (0, "Off"),
    (1, "Standard"),
    (2, "Advanced Auto"),
    (3, "Advanced Level"),
];

static BRACKETING_LEVEL: &[(i64, &str)] = &[(0, "Off"), (1, "Low"), (2, "High")];

static CREATIVE_STYLE_FOCUSINFO: &[(i64, &str)] = &[
    (1, "Standard"),
    (2, "Vivid"),
    (3, "Portrait"),
    (4, "Landscape"),
    (5, "Sunset"),
    (6, "Night View/Portrait"),
    (8, "B&W"),
    (9, "Adobe RGB"),
    (11, "Neutral"),
    (12, "Clear"),
    (13, "Deep"),
    (14, "Light"),
    (15, "Autumn Leaves"),
    (16, "Sepia"),
];

/// `DriveMode2` for the A230/A290/A330/A380/A390.
static DRIVE_MODE2_LATE: &[(i64, &str)] = &[
    (0x01, "Single Frame"),
    (0x02, "Continuous High"),
    (0x04, "Self-timer 10 sec"),
    (0x05, "Self-timer 2 sec, Mirror Lock-up"),
    (0x07, "Continuous Bracketing"),
    (0x0a, "Remote Commander"),
    (0x0b, "Continuous Self-timer"),
];

/// `DriveMode2` for the A200/A300/A350/A700/A850/A900.
static DRIVE_MODE2_EARLY: &[(i64, &str)] = &[
    (0x01, "Single Frame"),
    (0x02, "Continuous High"),
    (0x04, "Self-timer 10 sec"),
    (0x05, "Self-timer 2 sec, Mirror Lock-up"),
    (0x06, "Single-frame Bracketing"),
    (0x07, "Continuous Bracketing"),
    (0x0a, "Remote Commander"),
    (0x0b, "Mirror Lock-up"),
    (0x12, "Continuous Low"),
    (0x18, "White Balance Bracketing Low"),
    (0x19, "D-Range Optimizer Bracketing Low"),
    (0x28, "White Balance Bracketing High"),
    (0x29, "D-Range Optimizer Bracketing High"),
];

/// Everything in `FocusInfo` that is not gated on the camera model.
///
/// `ExposureProgram` (0x3f) and `ISO` (0x6f) are omitted: the table is
/// `PRIORITY => 0`, so ExifTool always prefers the identically-named standard
/// EXIF tags and never prints these. `DynamicRangeOptimizerMode` also appears
/// at 0x77; ExifTool keeps the first-extracted copy, which is 0x15.
static FOCUS_INFO_TAGS: &[BinTag] = &[
    BinTag::map(0x10, "Rotation", ROTATION),
    BinTag::map(0x14, "ImageStabilizationSetting", ON_OFF),
    BinTag::map(0x15, "DynamicRangeOptimizerMode", DRO_MODE),
    BinTag::int(0x2b, "BracketShotNumber"),
    BinTag::map(0x2c, "WhiteBalanceBracketing", BRACKETING_LEVEL),
    BinTag::int(0x2d, "BracketShotNumber2"),
    BinTag::map(0x2e, "DynamicRangeOptimizerBracket", BRACKETING_LEVEL),
    BinTag::int(0x2f, "ExposureBracketShotNumber"),
    BinTag::map(0x41, "CreativeStyle", CREATIVE_STYLE_FOCUSINFO),
    BinTag::conv(0x6d, "ISOSetting", iso_setting),
    BinTag::int(0x79, "DynamicRangeOptimizerLevel"),
];

static FOCUS_INFO: BinTable = BinTable {
    format: Fmt::U8,
    big_endian: false,
    tags: FOCUS_INFO_TAGS,
};

/// Byte counts ExifTool uses to recognise a `FocusInfo` block.
const FOCUS_INFO_COUNTS: [usize; 2] = [19154, 19148];

/// `FocusPosition` (0x09bb) is only decoded for these bodies.
const FOCUS_POSITION_MODELS: [&str; 11] = [
    "DSLR-A200",
    "DSLR-A230",
    "DSLR-A290",
    "DSLR-A300",
    "DSLR-A330",
    "DSLR-A350",
    "DSLR-A380",
    "DSLR-A390",
    "DSLR-A700",
    "DSLR-A850",
    "DSLR-A900",
];

/// `ShutterCount` (0x0846) is only valid for these bodies.
const SHUTTER_COUNT_MODELS: [&str; 7] = [
    "DSLR-A230",
    "DSLR-A290",
    "DSLR-A330",
    "DSLR-A380",
    "DSLR-A390",
    "DSLR-A850",
    "DSLR-A900",
];

/// Bodies whose `DriveMode2` uses the later of ExifTool's two lists.
const DRIVE_MODE2_LATE_MODELS: [&str; 5] = [
    "DSLR-A230",
    "DSLR-A290",
    "DSLR-A330",
    "DSLR-A380",
    "DSLR-A390",
];

/// `TiffMeteringImage` (0x1110) needs 9600 bytes; ExifTool then reports a
/// fixed 7404-byte TIFF rather than reading the samples.
const TIFF_METERING_IMAGE_OFFSET: usize = 0x1110;
const TIFF_METERING_IMAGE_LEN: usize = 9600;

// ============================================================================
// CameraSettings (tag 0x0114, count 280 or 364)
// ============================================================================

static HIGH_SPEED_SYNC: &[(i64, &str)] = &[(0, "Off"), (1, "On")];

static DRIVE_MODE: &[(i64, &str)] = &[
    (1, "Single Frame"),
    (2, "Continuous High"),
    (4, "Self-timer 10 sec"),
    (5, "Self-timer 2 sec, Mirror Lock-up"),
    (6, "Single-frame Bracketing"),
    (7, "Continuous Bracketing"),
    (10, "Remote Commander"),
    (11, "Mirror Lock-up"),
    (18, "Continuous Low"),
    (24, "White Balance Bracketing Low"),
    (25, "D-Range Optimizer Bracketing Low"),
    (40, "White Balance Bracketing High"),
    (41, "D-Range Optimizer Bracketing High"),
];

static WHITE_BALANCE_SETTING: &[(i64, &str)] = &[
    (2, "Auto"),
    (4, "Daylight"),
    (5, "Fluorescent"),
    (6, "Tungsten"),
    (7, "Flash"),
    (16, "Cloudy"),
    (17, "Shade"),
    (18, "Color Temperature/Color Filter"),
    (32, "Custom 1"),
    (33, "Custom 2"),
    (34, "Custom 3"),
];

static AF_AREA_MODE: &[(i64, &str)] = &[(0, "Wide"), (1, "Local"), (2, "Spot")];

static AF_POINT_SETTING: &[(i64, &str)] = &[
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
];

static FLASH_MODE: &[(i64, &str)] = &[
    (0, "Autoflash"),
    (2, "Rear Sync"),
    (3, "Wireless"),
    (4, "Fill-flash"),
    (5, "Flash Off"),
    (6, "Slow Sync"),
];

static FLASH_CONTROL: &[(i64, &str)] = &[(0, "ADI"), (1, "Pre-flash TTL"), (2, "Manual")];

static PRIORITY_SETUP: &[(i64, &str)] = &[(0, "AF"), (1, "Release")];

static AF_ILLUMINATOR: &[(i64, &str)] = &[(0, "Auto"), (1, "Off")];

static AF_WITH_SHUTTER: &[(i64, &str)] = &[(0, "On"), (1, "Off")];

static HIGH_ISO_NR_CS: &[(i64, &str)] = &[(0, "Normal"), (1, "Low"), (2, "High"), (3, "Off")];

static IMAGE_STYLE: &[(i64, &str)] = &[
    (1, "Standard"),
    (2, "Vivid"),
    (3, "Portrait"),
    (4, "Landscape"),
    (5, "Sunset"),
    (7, "Night View/Portrait"),
    (8, "B&W"),
    (9, "Adobe RGB"),
    (11, "Neutral"),
    (129, "StyleBox1"),
    (130, "StyleBox2"),
    (131, "StyleBox3"),
    (132, "StyleBox4"),
    (133, "StyleBox5"),
    (134, "StyleBox6"),
];

static FOCUS_MODE_SWITCH: &[(i64, &str)] = &[(0, "AF"), (1, "Manual")];

static FLASH_ACTION: &[(i64, &str)] = &[
    (0, "Did not fire"),
    (1, "Fired"),
    (2, "External Flash, Did not fire"),
    (3, "External Flash, Fired"),
];

static ROTATION_CS: &[(i64, &str)] = &[
    (0, "Horizontal (normal)"),
    (1, "Rotate 90 CW"),
    (2, "Rotate 270 CW"),
];

static AE_LOCK: &[(i64, &str)] = &[(1, "Off"), (2, "On")];

static FLASH_ACTION2: &[(i64, &str)] = &[
    (1, "Fired, Autoflash"),
    (2, "Fired, Fill-flash"),
    (3, "Fired, Rear Sync"),
    (4, "Fired, Wireless"),
    (5, "Did not fire"),
    (6, "Fired, Slow Sync"),
    (17, "Fired, Autoflash, Red-eye reduction"),
    (18, "Fired, Fill-flash, Red-eye reduction"),
    (34, "Fired, Fill-flash, HSS"),
];

static BATTERY_STATE: &[(i64, &str)] = &[
    (2, "Empty"),
    (3, "Very Low"),
    (4, "Low"),
    (5, "Sufficient"),
    (6, "Full"),
];

static SONY_IMAGE_SIZE: &[(i64, &str)] = &[(1, "Large"), (2, "Medium"), (3, "Small")];

static ASPECT_RATIO: &[(i64, &str)] = &[(1, "3:2"), (2, "16:9")];

static EXPOSURE_LEVEL_INCREMENTS: &[(i64, &str)] = &[(33, "1/3 EV"), (50, "1/2 EV")];

/// `CameraSettings`, keyed in `int16u` units and read big-endian.
///
/// Omitted because this `PRIORITY => 0` table loses ExifTool's duplicate
/// suppression to a higher-priority tag of the same name:
/// * 0x00 `ExposureTime`, 0x01 `FNumber`, 0x15 `MeteringMode`, 0x1b
///   `ColorSpace`, 0x1c `Sharpness`, 0x1d `Contrast`, 0x1e `Saturation`,
///   0x3c `ExposureProgram` - lose to the standard EXIF tags.
/// * 0x06 `WhiteBalanceFineTune`, 0x0f `WhiteBalance`, 0x56 `Quality` - lose to
///   the Sony `Main` table entries 0x0112, 0x0115 and 0x0102.
static CAMERA_SETTINGS_TAGS: &[BinTag] = &[
    BinTag::map(0x02, "HighSpeedSync", HIGH_SPEED_SYNC),
    BinTag::conv(0x03, "ExposureCompensationSet", exposure_compensation),
    BinTag::map(0x04, "DriveMode", DRIVE_MODE)
        .with_mask(0xff)
        .hex(),
    BinTag::map(0x05, "WhiteBalanceSetting", WHITE_BALANCE_SETTING),
    BinTag::conv(0x07, "ColorTemperatureSet", color_temperature),
    BinTag::conv(0x08, "ColorCompensationFilterSet", color_compensation),
    BinTag::conv(0x0c, "ColorTemperatureCustom", color_temperature),
    BinTag::conv(0x0d, "ColorCompensationFilterCustom", color_compensation),
    BinTag::map(0x10, "FocusModeSetting", FOCUS_MODE_SETTING),
    BinTag::map(0x11, "AFAreaMode", AF_AREA_MODE),
    BinTag::map(0x12, "AFPointSetting", AF_POINT_SETTING),
    BinTag::map(0x13, "FlashMode", FLASH_MODE),
    BinTag::conv(0x14, "FlashExposureCompSet", exposure_compensation),
    BinTag::conv(0x16, "ISOSetting", iso_setting),
    BinTag::map(0x18, "DynamicRangeOptimizerMode", DRO_MODE),
    BinTag::int(0x19, "DynamicRangeOptimizerLevel"),
    BinTag::map(0x1a, "CreativeStyle", CREATIVE_STYLE_FOCUSINFO),
    BinTag::conv(0x1f, "ZoneMatchingValue", offset_by_ten),
    BinTag::conv(0x22, "Brightness", offset_by_ten),
    BinTag::map(0x23, "FlashControl", FLASH_CONTROL),
    BinTag::map(0x28, "PrioritySetupShutterRelease", PRIORITY_SETUP),
    BinTag::map(0x29, "AFIlluminator", AF_ILLUMINATOR),
    BinTag::map(0x2a, "AFWithShutter", AF_WITH_SHUTTER),
    BinTag::map(0x2b, "LongExposureNoiseReduction", ON_OFF),
    BinTag::map(0x2c, "HighISONoiseReduction", HIGH_ISO_NR_CS),
    BinTag::map(0x2d, "ImageStyle", IMAGE_STYLE),
    BinTag::map(0x2e, "FocusModeSwitch", FOCUS_MODE_SWITCH),
    BinTag::conv(0x2f, "ShutterSpeedSetting", shutter_speed),
    BinTag::conv(0x30, "ApertureSetting", aperture),
    BinTag::map(0x3d, "ImageStabilizationSetting", ON_OFF),
    BinTag::map(0x3e, "FlashAction", FLASH_ACTION),
    BinTag::map(0x3f, "Rotation", ROTATION_CS),
    BinTag::map(0x40, "AELock", AE_LOCK),
    BinTag::map(0x4c, "FlashAction2", FLASH_ACTION2),
    BinTag::map(0x4d, "FocusMode", FOCUS_MODE_SETTING),
    BinTag::map(0x50, "BatteryState", BATTERY_STATE),
    BinTag::conv(0x51, "BatteryLevel", percent),
    BinTag::conv(0x53, "FocusStatus", focus_status),
    BinTag::map(0x54, "SonyImageSize", SONY_IMAGE_SIZE),
    BinTag::map(0x55, "AspectRatio", ASPECT_RATIO),
    BinTag::map(0x58, "ExposureLevelIncrements", EXPOSURE_LEVEL_INCREMENTS),
    BinTag::map(0x6a, "RedEyeReduction", ON_OFF),
];

static CAMERA_SETTINGS: BinTable = BinTable {
    format: Fmt::U16,
    big_endian: true,
    tags: CAMERA_SETTINGS_TAGS,
};

/// Byte counts ExifTool uses to recognise a `CameraSettings` block:
/// A200/A300/A350/A700 write 280, A850/A900 write 364.
const CAMERA_SETTINGS_COUNTS: [usize; 2] = [280, 364];

// ============================================================================
// Entry points
// ============================================================================

/// Decodes tag 0x0010 when it holds a `CameraInfo2` block.
///
/// Returns `false` when the byte count belongs to one of the sibling tables
/// (`CameraInfo`, `CameraInfo3`) this module does not implement, so the caller
/// leaves the tag alone rather than decoding it with the wrong layout.
pub fn extract_camera_info2(data: &[u8], tags: &mut HashMap<String, String>) -> bool {
    if !CAMERA_INFO2_COUNTS.contains(&data.len()) {
        return false;
    }
    CAMERA_INFO2.extract(data, "Sony", tags);
    // Index 0 is an 8-byte LensSpec rather than a scalar.
    if let Some(spec) = data.get(..8).and_then(print_lens_spec) {
        tags.insert("Sony:LensSpec".to_string(), spec);
    }
    true
}

/// Decodes tag 0x0020 when it holds a `FocusInfo` block.
pub fn extract_focus_info(
    data: &[u8],
    model: Option<&str>,
    tags: &mut HashMap<String, String>,
) -> bool {
    if !FOCUS_INFO_COUNTS.contains(&data.len()) {
        return false;
    }
    FOCUS_INFO.extract(data, "Sony", tags);

    let reader = EndianReader::little_endian(data);
    let model = model.unwrap_or("");

    // DriveMode2 uses one of two PrintConv lists depending on the body.
    if let Some(raw) = read_scalar(&reader, 0x0e, Fmt::U8) {
        let list = if DRIVE_MODE2_LATE_MODELS.contains(&model) {
            DRIVE_MODE2_LATE
        } else {
            DRIVE_MODE2_EARLY
        };
        let printed =
            super::binary::lookup(list, raw).unwrap_or_else(|| super::binary::unknown_hex(raw));
        tags.insert("Sony:DriveMode2".to_string(), printed);
    }

    if SHUTTER_COUNT_MODELS.contains(&model)
        && let Some(raw) = read_scalar(&reader, 0x0846, Fmt::U32)
    {
        tags.insert(
            "Sony:ShutterCount".to_string(),
            (raw & 0x00ff_ffff).to_string(),
        );
    }

    if FOCUS_POSITION_MODELS.contains(&model)
        && let Some(raw) = read_scalar(&reader, 0x09bb, Fmt::U8)
    {
        tags.insert("Sony:FocusPosition".to_string(), raw.to_string());
    }

    if data.len() >= TIFF_METERING_IMAGE_OFFSET + TIFF_METERING_IMAGE_LEN {
        tags.insert(
            "Sony:TiffMeteringImage".to_string(),
            "(Binary data 7404 bytes, use -b option to extract)".to_string(),
        );
    }
    true
}

/// Decodes tag 0x0114 when it holds a `CameraSettings` block.
pub fn extract_camera_settings(data: &[u8], tags: &mut HashMap<String, String>) -> bool {
    if !CAMERA_SETTINGS_COUNTS.contains(&data.len()) {
        return false;
    }
    CAMERA_SETTINGS.extract(data, "Sony", tags);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn af_status_renders_signed_defocus() {
        assert_eq!(af_status(33), Some("Back Focus (+33)".to_string()));
        assert_eq!(af_status(-10), Some("Front Focus (-10)".to_string()));
        assert_eq!(af_status(0), Some("In Focus".to_string()));
        assert_eq!(af_status(-32768), Some("Out of Focus".to_string()));
    }

    #[test]
    fn iso_setting_inverts_sonys_log_encoding() {
        // DSLR-A350 stores 48 and exiftool reports ISO 100.
        assert_eq!(iso_setting(48), Some("100".to_string()));
        assert_eq!(iso_setting(0), Some("Auto".to_string()));
    }

    #[test]
    fn aperture_and_shutter_match_exiftool_rendering() {
        // DSLR-A350: ApertureSetting 8.0, ShutterSpeedSetting 1/12.
        assert_eq!(aperture(56), Some("8.0".to_string()));
        assert_eq!(shutter_speed(77), Some("1/12".to_string()));
        assert_eq!(shutter_speed(0), Some("Bulb".to_string()));
    }

    #[test]
    fn colour_compensation_uses_byte_sized_two_s_complement() {
        assert_eq!(wrapped_signed(253), -3);
        assert_eq!(wrapped_signed(3), 3);
        assert_eq!(color_compensation(253), Some("-3".to_string()));
    }

    #[test]
    fn focus_status_bitmask() {
        assert_eq!(focus_status(1), Some("Confirmed".to_string()));
        assert_eq!(focus_status(0), Some("Not confirmed".to_string()));
        assert_eq!(focus_status(4), Some("Not confirmed, Tracking".to_string()));
    }

    #[test]
    fn wrong_sized_blocks_are_refused_rather_than_misread() {
        let mut tags = HashMap::new();
        assert!(!extract_camera_info2(&[0u8; 100], &mut tags));
        assert!(!extract_camera_settings(&[0u8; 100], &mut tags));
        assert!(!extract_focus_info(&[0u8; 100], None, &mut tags));
        assert!(tags.is_empty());
    }
}
