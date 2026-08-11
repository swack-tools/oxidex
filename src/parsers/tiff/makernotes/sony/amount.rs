//! Binary-data sub-directories written by the A-mount DSLR bodies.
//!
//! `CameraInfo2` (tag 0x0010), `FocusInfo` (0x0020) and `CameraSettings`
//! (0x0114), transcribed from `Image::ExifTool::Sony`. Which table a body
//! selects is decided by the sub-directory's byte count, not by the model, so
//! the count checks below are the same ones ExifTool's `Condition` uses.

use super::binary::{
    BinTable, BinTag, Fmt, print_exposure_time, print_f_number, read_scalar, signed_adjustment,
};
use super::binary_data::model_matches;
use super::lens_spec::print_lens_spec;
use crate::exiftool_tables::{self, DecodedValue};
use crate::io::{ByteOrder as IoByteOrder, EndianReader};
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
// CameraInfo (tag 0x0010, count 368 or 5478)
// ============================================================================
//
// Distinct from `CameraInfo2` below: ExifTool's Condition picks between the
// two sub-directories purely by byte count (Sony.pm:716-747), and this table
// is always big-endian regardless of the file's own byte order, while
// `CameraInfo2` is always little-endian.

/// Byte counts ExifTool uses to recognise a `CameraInfo` block: A700 writes
/// 368, A850/A900 write 5478.
const CAMERA_INFO_COUNTS: [usize; 2] = [368, 5478];

/// Bodies whose `CameraInfo` block carries the AF micro-adjust group at byte
/// offsets 304-305 (0x130/0x131). ExifTool gates all three tags on
/// `$$self{Model} =~ /^DSLR-A(850|900)\b/` (Sony.pm:2874-2894); the A700
/// writes a shorter (368-byte) block that does not reach these offsets.
const AF_MICRO_ADJ_MODELS: [&str; 2] = ["DSLR-A850", "DSLR-A900"];

/// `Sony::CameraInfo`'s `LensSpec` (0x00) stores its two 16-bit BCD fields
/// byte-swapped relative to `CameraInfo2`: ExifTool's ValueConv is
/// `pack('v*', unpack('n*', $val))`, i.e. each big-endian 16-bit word read
/// back out little-endian, which swaps the bytes within each of the four
/// pairs before the shared `ConvLensSpec`/`PrintLensSpec` logic runs
/// (Sony.pm:2750-2764).
fn camera_info_lens_spec_bytes(bytes: &[u8]) -> Option<[u8; 8]> {
    let b: &[u8; 8] = bytes.try_into().ok()?;
    Some([b[1], b[0], b[3], b[2], b[5], b[4], b[7], b[6]])
}

/// Decodes tag 0x0010 when it holds a `CameraInfo` block (A700, A850, A900).
///
/// Returns `false` when the byte count belongs to `CameraInfo2` or
/// `CameraInfo3`, so the caller can try those in turn.
pub fn extract_camera_info(
    data: &[u8],
    model: Option<&str>,
    tags: &mut HashMap<String, String>,
) -> bool {
    if !CAMERA_INFO_COUNTS.contains(&data.len()) {
        return false;
    }
    let Some(table) = exiftool_tables::find_table("Sony", "CameraInfo") else {
        return false;
    };

    for decoded in exiftool_tables::decode_binary_table(table, data, IoByteOrder::Big) {
        let name = decoded.field.name;
        match name {
            "LensSpec" => {
                if let Some(printed) = data
                    .get(..8)
                    .and_then(camera_info_lens_spec_bytes)
                    .and_then(|swapped| print_lens_spec(&swapped))
                {
                    tags.insert("Sony:LensSpec".to_string(), printed);
                }
            }
            // All three are `Condition => '$$self{Model} =~ /^DSLR-A(850|900)\b/'`
            // (Sony.pm:2874-2894), which this table's schema records but cannot
            // evaluate, so the generic path would report them on an A700 too.
            // The block below applies the model gate and the `$val - 20`
            // ValueConv the schema also declines to run.
            //
            // `AFMicroAdjRegisteredLenses` is 305.1, a bit-field sharing byte
            // 0x131 with `AFMicroAdjMode`; it reached this match only once
            // masked fractional entries began decoding. Until then it was
            // filtered out upstream, and it survived the generic arm merely
            // because its `PrintConv` is empty -- name it rather than rest on
            // that.
            "AFMicroAdjValue" | "AFMicroAdjMode" | "AFMicroAdjRegisteredLenses" => {}
            // `afStatusInfo`'s `OTHER` sub formats every value the two literal
            // enum entries do not cover ("Front Focus (N)" / "Back Focus
            // (+N)"); the generated IntEnum can only carry the two literals,
            // so apply the hand-verified `af_status` helper instead of the
            // generic PrintConv, which would silently omit every non-zero
            // reading.
            _ if name.starts_with("AFStatus") => {
                if let DecodedValue::Integer(v) = decoded.raw
                    && let Some(printed) = af_status(v)
                {
                    tags.insert(format!("Sony:{name}"), printed);
                }
            }
            _ => {
                if let Some(printed) = decoded.apply_print_conv_to_raw() {
                    tags.insert(format!("Sony:{name}"), printed);
                }
            }
        }
    }

    // `AFMicroAdjValue` (0x130, ValueConv `$val - 20`) and the byte 0x131
    // that packs `AFMicroAdjMode` (Mask 0x80) with `AFMicroAdjRegisteredLenses`
    // (Mask 0x7f), all A850/A900-only (Sony.pm:2874-2894).
    if AF_MICRO_ADJ_MODELS.contains(&model.unwrap_or(""))
        && let (Some(&value_byte), Some(&mode_byte)) = (data.get(304), data.get(305))
    {
        tags.insert(
            "Sony:AFMicroAdjValue".to_string(),
            (i64::from(value_byte) - 20).to_string(),
        );
        tags.insert(
            "Sony:AFMicroAdjMode".to_string(),
            (if mode_byte & 0x80 != 0 { "On" } else { "Off" }).to_string(),
        );
        tags.insert(
            "Sony:AFMicroAdjRegisteredLenses".to_string(),
            (mode_byte & 0x7f).to_string(),
        );
    }

    true
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

// ============================================================================
// Panorama (tag 0x1003)
// ============================================================================

/// Decodes tag 0x1003's `Panorama` sub-directory (Sony.pm's `Panorama` table,
/// 11 `int32u` scalars starting at the sub-directory's first byte).
///
/// The caller only reaches this once it has confirmed the panorama flag bytes
/// matched -- Sony.pm's own `Condition` on 0x1003 is what gates whether this
/// sub-directory is processed at all (non-panorama images write all zeros
/// here and ExifTool skips it).
pub fn extract_panorama(data: &[u8], byte_order: IoByteOrder, tags: &mut HashMap<String, String>) {
    let Some(table) = exiftool_tables::find_table("Sony", "Panorama") else {
        return;
    };
    for decoded in exiftool_tables::decode_binary_table(table, data, byte_order) {
        let name = decoded.field.name;
        if let Some(printed) = decoded.apply_print_conv_to_raw() {
            tags.insert(format!("Sony:{name}"), printed);
        } else if let DecodedValue::Integer(v) = decoded.raw {
            tags.insert(format!("Sony:{name}"), v.to_string());
        }
    }
}

// ============================================================================
// ExtraInfo / ExtraInfo2 / ExtraInfo3 (tag 0x0116)
// ============================================================================

/// `$$self{Model} =~ /^DSLR-A(850|900)\b/` (Sony.pm:855-859): selects the
/// `ExtraInfo` layout, always decoded big-endian regardless of the file's
/// normal byte order.
const EXTRA_INFO_MODELS: [&str; 2] = ["DSLR-A850", "DSLR-A900"];

/// `$$self{Model} =~ /^DSLR-A(230|290|330|380|390)\b/` (Sony.pm:865-867):
/// selects the `ExtraInfo2` layout.
const EXTRA_INFO2_MODELS: [&str; 5] = [
    "DSLR-A230",
    "DSLR-A290",
    "DSLR-A330",
    "DSLR-A380",
    "DSLR-A390",
];

/// `ExtraInfo3`'s NEX model gate, shared verbatim by `BatteryVoltage1`
/// (0x0006), `BatteryVoltage2` (0x0008), `ImageStabilization` (0x0011,
/// Sony.pm:5951-5988) and the non-NEX `CameraOrientation` (0x0018, mask
/// 0x30, Sony.pm:6070-6079): `Condition => '$$self{Model} !~
/// /^(NEX-(3|5|5C|C3|VG10|VG10E))\b/'`.
const EXTRA_INFO3_NEX_MODEL_RE: &str = r"^(NEX-(3|5|5C|C3|VG10|VG10E))\b";

/// Sony.pm:6058-6067 and 6070-6079: `ExtraInfo3`'s two `CameraOrientation`
/// variants (NEX 0x0016/mask 0xc0, non-NEX 0x0018/mask 0x30) share this enum.
fn camera_orientation(value: i64) -> Option<String> {
    Some(
        match value {
            0 => "Horizontal (normal)",
            1 => "Rotate 90 CW",
            2 => "Rotate 270 CW",
            3 => "Rotate 180",
            _ => return None,
        }
        .to_string(),
    )
}

/// Decodes tag 0x0116, whose layout Sony.pm picks by model: `ExtraInfo`
/// (A850/A900, forced big-endian), `ExtraInfo2` (A230/A290/A330/A380/A390),
/// or `ExtraInfo3` (every other body that writes this tag, Sony.pm:868-871).
pub fn extract_extra_info(
    data: &[u8],
    model: Option<&str>,
    byte_order: IoByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let model = model.unwrap_or("");
    let (table_name, order) = if EXTRA_INFO_MODELS.contains(&model) {
        ("ExtraInfo", IoByteOrder::Big)
    } else if EXTRA_INFO2_MODELS.contains(&model) {
        ("ExtraInfo2", byte_order)
    } else {
        ("ExtraInfo3", byte_order)
    };
    let Some(table) = exiftool_tables::find_table("Sony", table_name) else {
        return;
    };

    // `ExtraInfo3` fields Sony.pm gates on `$$self{Model}`
    // (Sony.pm:5951-5988, 6070-6079). `Field::omitted.condition` records that
    // a `Condition` exists but not what it tests, so `apply_print_conv_to_raw`
    // cannot evaluate it -- the generic `_` arm below must not run for these,
    // or a NEX body gets DSLR/SLT-only fields it never carries. That includes
    // `CameraOrientation`: its masked byte at 0x18 happened to read 0 on the
    // one NEX-VG10E sample this was checked against, coincidentally matching
    // the *real* NEX-only reading at a different offset (0x16) -- a wrong
    // value under the right name that a spot check cannot tell from a right
    // one. Values are captured here and only emitted after the loop, once the
    // model predicate has actually been evaluated.
    let mut battery_voltage1: Option<i64> = None;
    let mut battery_voltage2: Option<i64> = None;
    let mut image_stabilization_gated: Option<String> = None;
    let mut camera_orientation_non_nex: Option<String> = None;

    for decoded in exiftool_tables::decode_binary_table(table, data, order) {
        let name = decoded.field.name;
        match name {
            // Sony.pm: `PrintConv => '$val=~tr/ /./; $val'` -- a raw string
            // substitution on the default space-joined `int8u[4]` rendering,
            // which the generated schema has no PrintConv form for. Joining
            // the decoded array with '.' directly reproduces it exactly.
            "ExtraInfoVersion" => {
                if let DecodedValue::Array(values) = &decoded.raw {
                    let mut parts = Vec::with_capacity(values.len());
                    for v in values {
                        let DecodedValue::Integer(n) = v else {
                            parts.clear();
                            break;
                        };
                        parts.push(n.to_string());
                    }
                    if !parts.is_empty() {
                        tags.insert("Sony:ExtraInfoVersion".to_string(), parts.join("."));
                    }
                }
            }
            // Sony.pm:5951-5967: `ValueConv => '$val / 128'`, `PrintConv =>
            // 'sprintf("%.2f V",$val)'`. `Field::omitted.value_conv` is set
            // (the schema has no PrintConv form for a value the ValueConv
            // already transformed), so `apply_print_conv_to_raw` always
            // refuses this field; the ValueConv+PrintConv pair is applied by
            // hand below, but ONLY once the model condition captured here has
            // been checked -- never unconditionally.
            "BatteryVoltage1" => {
                if let DecodedValue::Integer(v) = decoded.raw {
                    battery_voltage1 = Some(v);
                }
            }
            "BatteryVoltage2" => {
                if let DecodedValue::Integer(v) = decoded.raw {
                    battery_voltage2 = Some(v);
                }
            }
            // `ExtraInfo2`'s `ImageStabilization` (A230/290/330/380/390) has
            // no `Condition` at all -- only `ExtraInfo3`'s carries the NEX
            // gate -- so this guard on `omitted.condition` leaves the
            // `ExtraInfo2` field on the unconditional `_` path below and
            // diverts only the gated one.
            "ImageStabilization" if decoded.field.omitted.condition => {
                image_stabilization_gated = decoded.apply_print_conv_to_raw();
            }
            "CameraOrientation" => {
                camera_orientation_non_nex = decoded.apply_print_conv_to_raw();
            }
            _ => {
                if let Some(printed) = decoded.apply_print_conv_to_raw() {
                    tags.insert(format!("Sony:{name}"), printed);
                }
            }
        }
    }

    if table_name != "ExtraInfo3" {
        return;
    }
    if !model_matches(EXTRA_INFO3_NEX_MODEL_RE, model) {
        // Sony.pm:5951-5988: the negated condition holds, so ExifTool reports
        // these for every non-NEX body that writes `ExtraInfo3`. Confirmed
        // absent instead for a NEX body by the pinned oracle (ExifTool
        // 13.59, `-a -G1 -s`, SonyNEX-VG10E.jpg): no BatteryVoltage1/
        // BatteryVoltage2 tag at all.
        if let Some(v) = battery_voltage1 {
            tags.insert(
                "Sony:BatteryVoltage1".to_string(),
                format!("{:.2} V", v as f64 / 128.0),
            );
        }
        if let Some(v) = battery_voltage2 {
            tags.insert(
                "Sony:BatteryVoltage2".to_string(),
                format!("{:.2} V", v as f64 / 128.0),
            );
        }
        if let Some(printed) = image_stabilization_gated {
            tags.insert("Sony:ImageStabilization".to_string(), printed);
        }
        if let Some(printed) = camera_orientation_non_nex {
            tags.insert("Sony:CameraOrientation".to_string(), printed);
        }
    } else if let Some(&raw) = data.get(0x16) {
        // Sony.pm:6058-6067 -- the NEX-only `CameraOrientation` variant
        // (mask 0xc0). `0x0016` is a Perl array of two model-conditioned
        // alternatives at the same offset (`MemoryCardConfiguration` for
        // DSLR bodies, this one for NEX); the generator does not transcribe
        // that shape, so `SONY_EXTRAINFO3` (`binary_tables.rs`) has no field
        // at this offset at all -- there is no `decoded.raw` to read it from.
        // Reading the byte directly here is not an approximation: it is the
        // exact byte/mask Sony.pm names, gated on the exact model predicate
        // above. Verified against the pinned oracle's `-v3` trace on
        // SonyNEX-VG10E.jpg: byte 0x16 = 0x3e, `(0x3e & 0xc0) >> 6` = 0 ->
        // "Horizontal (normal)", matching `-a -G1 -s`'s `CameraOrientation`.
        if let Some(printed) = camera_orientation((i64::from(raw) & 0xc0) >> 6) {
            tags.insert("Sony:CameraOrientation".to_string(), printed);
        }
    }
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
        assert!(!extract_camera_info(&[0u8; 100], None, &mut tags));
        assert!(!extract_camera_info2(&[0u8; 100], &mut tags));
        assert!(!extract_focus_info(&[0u8; 100], None, &mut tags));
        assert!(tags.is_empty());
    }

    #[test]
    fn camera_info_lens_spec_undoes_the_16_bit_word_swap() {
        // CameraInfo2's ValueConv passes LensSpec bytes straight through, but
        // CameraInfo's does `pack('v*', unpack('n*', $val))` first -- a
        // pairwise byte swap. Feeding the ILCA-77M2 kit lens's CameraInfo2
        // bytes through the swap and back should reproduce the original.
        let unswapped = [0x01, 0x00, 0x16, 0x00, 0x50, 0x28, 0x00, 0x01];
        let camera_info_bytes = [0x00, 0x01, 0x00, 0x16, 0x28, 0x50, 0x01, 0x00];
        assert_eq!(
            camera_info_lens_spec_bytes(&camera_info_bytes),
            Some(unswapped)
        );
        assert_eq!(
            print_lens_spec(&camera_info_lens_spec_bytes(&camera_info_bytes).unwrap()),
            Some("DT 16-50mm F2.8 SSM".to_string())
        );
    }

    #[test]
    fn camera_info_decodes_af_status_and_gates_micro_adjust_on_model() {
        // A 5478-byte block shaped like the DSLR-A900 sample: AF-S,
        // AFPointSelected/AFPoint both Lower-right, AFStatusFarRight showing
        // -65 (a front-focus reading outside the two literal enum values),
        // AFMicroAdjValue/Mode all zero.
        let mut data = vec![0u8; 5478];
        data[20] = 1; // FocusModeSetting: AF-S
        data[21] = 5; // AFPointSelected: Lower-right
        data[25] = 21; // AFPoint: Lower-right
        data[68..70].copy_from_slice(&(-65i16).to_be_bytes()); // AFStatusFarRight

        let mut tags = HashMap::new();
        assert!(extract_camera_info(&data, Some("DSLR-A900"), &mut tags));
        assert_eq!(tags.get("Sony:FocusModeSetting"), Some(&"AF-S".to_string()));
        assert_eq!(
            tags.get("Sony:AFPointSelected"),
            Some(&"Lower-right".to_string())
        );
        assert_eq!(tags.get("Sony:AFPoint"), Some(&"Lower-right".to_string()));
        assert_eq!(
            tags.get("Sony:AFStatusFarRight"),
            Some(&"Front Focus (-65)".to_string())
        );
        // Zero-filled by default: af_status(0) is "In Focus".
        assert_eq!(
            tags.get("Sony:AFStatusActiveSensor"),
            Some(&"In Focus".to_string())
        );
        assert_eq!(tags.get("Sony:AFMicroAdjValue"), Some(&"-20".to_string()));
        assert_eq!(tags.get("Sony:AFMicroAdjMode"), Some(&"Off".to_string()));
        assert_eq!(
            tags.get("Sony:AFMicroAdjRegisteredLenses"),
            Some(&"0".to_string())
        );

        // The A700 writes the shorter 368-byte block and never reaches the
        // AF micro-adjust group, whether or not the model gate would allow it.
        let short = vec![0u8; 368];
        let mut short_tags = HashMap::new();
        assert!(extract_camera_info(
            &short,
            Some("DSLR-A700"),
            &mut short_tags
        ));
        assert!(!short_tags.contains_key("Sony:AFMicroAdjValue"));

        // A body outside the A850/A900 gate never gets the micro-adjust group
        // even at the 5478-byte length (Sony.pm's Condition is model-gated,
        // not count-gated, for these three tags).
        let mut ungated_tags = HashMap::new();
        assert!(extract_camera_info(
            &data,
            Some("DSLR-A700"),
            &mut ungated_tags
        ));
        assert!(!ungated_tags.contains_key("Sony:AFMicroAdjValue"));
    }

    /// The 30-byte `ExtraInfo3` payload from the SonyNEX-VG10E.jpg sample's
    /// tag 0x0116 (`oxidex -v3` / the pinned oracle's `-v3` trace agree on the
    /// bytes: `ff 03 48 00 2a ff 02 c0 c4 cc ce cf d1 ae ae 00 cc 00 ff ff ff
    /// 3a 3e ff cc cc e4 02 f7 02`). Byte 0x16 = 0x3e is the one this test
    /// pins: `(0x3e & 0xc0) >> 6 = 0`, ExifTool's NEX-only `CameraOrientation`
    /// reading of "Horizontal (normal)" (Sony.pm:6058-6067).
    const EXTRA_INFO3_NEX_VG10E_BYTES: [u8; 30] = [
        0xff, 0x03, 0x48, 0x00, 0x2a, 0xff, 0x02, 0xc0, 0xc4, 0xcc, 0xce, 0xcf, 0xd1, 0xae, 0xae,
        0x00, 0xcc, 0x00, 0xff, 0xff, 0xff, 0x3a, 0x3e, 0xff, 0xcc, 0xcc, 0xe4, 0x02, 0xf7, 0x02,
    ];

    #[test]
    fn extra_info3_omits_battery_voltage_on_nex_bodies() {
        // Sony.pm:5951-5967 -- `Condition => '$$self{Model} !~
        // /^(NEX-(3|5|5C|C3|VG10|VG10E))\b/'`. NEX-VG10E matches the excluded
        // set, so ExifTool reports neither BatteryVoltage1 nor
        // BatteryVoltage2 at all. Confirmed against the pinned oracle
        // (ExifTool 13.59, `-a -G1 -s`, SonyNEX-VG10E.jpg): no
        // BatteryVoltage1/BatteryVoltage2 tag in the output. Before this fix,
        // oxidex reported "384.02 V" / "409.53 V" here -- the correct
        // ValueConv arithmetic run without the model gate that should have
        // suppressed it.
        let mut tags = HashMap::new();
        extract_extra_info(
            &EXTRA_INFO3_NEX_VG10E_BYTES,
            Some("NEX-VG10E"),
            IoByteOrder::Little,
            &mut tags,
        );
        assert!(!tags.contains_key("Sony:BatteryVoltage1"));
        assert!(!tags.contains_key("Sony:BatteryVoltage2"));
        // Sony.pm:5980-5988 carries the same gate; also absent.
        assert!(!tags.contains_key("Sony:ImageStabilization"));
    }

    #[test]
    fn extra_info3_camera_orientation_reads_the_nex_only_byte_and_mask() {
        // Sony.pm:6058-6067 -- the NEX variant lives at 0x0016/mask 0xc0, not
        // the DSLR/SLT one this table transcribes at 0x0018/mask 0x30 (which
        // Sony.pm gates OUT for NEX bodies). Byte 0x16 = 0x3e decodes to 0 ->
        // "Horizontal (normal)", matching the pinned oracle's `-a -G1 -s`
        // `Sony:CameraOrientation` on this exact sample.
        let mut tags = HashMap::new();
        extract_extra_info(
            &EXTRA_INFO3_NEX_VG10E_BYTES,
            Some("NEX-VG10E"),
            IoByteOrder::Little,
            &mut tags,
        );
        assert_eq!(
            tags.get("Sony:CameraOrientation"),
            Some(&"Horizontal (normal)".to_string())
        );
    }

    #[test]
    fn extra_info3_emits_battery_voltage_and_the_non_nex_orientation_on_dslr_bodies() {
        // The same bytes under a body Sony.pm's negated condition does NOT
        // exclude: BatteryVoltage1/2 (0x0006/0x0008, ValueConv `$val / 128`),
        // ImageStabilization (0x0011) and the 0x0018/mask 0x30
        // `CameraOrientation` variant should all be reported.
        let mut tags = HashMap::new();
        extract_extra_info(
            &EXTRA_INFO3_NEX_VG10E_BYTES,
            Some("DSLR-A580"),
            IoByteOrder::Little,
            &mut tags,
        );
        // bytes 6-7 = 0x02 0xc0 LE = 0xc002 = 49154; 49154 / 128 = 384.015625
        assert_eq!(
            tags.get("Sony:BatteryVoltage1"),
            Some(&"384.02 V".to_string())
        );
        // bytes 8-9 = 0xc4 0xcc LE = 0xccc4 = 52420; 52420 / 128 = 409.53125
        assert_eq!(
            tags.get("Sony:BatteryVoltage2"),
            Some(&"409.53 V".to_string())
        );
        // byte 0x11 = 0x00 -> Off.
        assert_eq!(
            tags.get("Sony:ImageStabilization"),
            Some(&"Off".to_string())
        );
        // byte 0x18 = 0xcc, mask 0x30 -> 0 -> "Horizontal (normal)" (the
        // non-NEX variant this table transcribes; distinct from the NEX-only
        // 0x16/0xc0 byte, which is not read for a non-NEX model).
        assert_eq!(
            tags.get("Sony:CameraOrientation"),
            Some(&"Horizontal (normal)".to_string())
        );
    }
}
