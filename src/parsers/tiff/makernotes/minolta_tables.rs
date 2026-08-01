//! Minolta MakerNote tables.
//!
//! Transcribed from `Image::ExifTool::Minolta`. Sony inherited this MakerNote
//! wholesale: the DSLR-A100 writes a complete Minolta MakerNote as a
//! sub-directory of its Sony one, and `LensType` is the same table on both
//! sides. Every `PrintConv` here was generated from ExifTool's tables rather
//! than typed out.

use super::sony::binary::{
    BinTable, BinTag, Fmt, lookup, print_exposure_time, print_float, unknown,
};

// ============================================================================
// Main table PrintConv hashes (Image::ExifTool::Minolta::Main)
// ============================================================================

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
static COLOR_MODE: &[(i64, &str)] = &[
    (0, "Natural color"),
    (1, "Black & White"),
    (2, "Vivid color"),
    (3, "Solarization"),
    (4, "Adobe RGB"),
    (5, "Sepia"),
    (9, "Natural"),
    (12, "Portrait"),
    (13, "Natural sRGB"),
    (14, "Natural+ sRGB"),
    (15, "Landscape"),
    (16, "Evening"),
    (17, "Night Scene"),
    (18, "Night Portrait"),
    (132, "Embed Adobe RGB"),
];
static MINOLTA_QUALITY: &[(i64, &str)] = &[
    (0, "Raw"),
    (1, "Super Fine"),
    (2, "Fine"),
    (3, "Standard"),
    (4, "Economy"),
    (5, "Extra fine"),
];
static MINOLTA_IMAGE_SIZE_0103: &[(i64, &str)] = &[
    (1, "1600x1200"),
    (2, "1280x960"),
    (3, "640x480"),
    (5, "2560x1920"),
    (6, "2272x1704"),
    (7, "2048x1536"),
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
static IMAGE_STABILIZATION_0107: &[(i64, &str)] = &[(1, "Off"), (5, "On")];
static RAW_AND_JPG_RECORDING: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
static ZONE_MATCHING: &[(i64, &str)] = &[(0, "ISO Setting Used"), (1, "High Key"), (2, "Low Key")];
static IMAGE_STABILIZATION_A100: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
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
];

// ============================================================================
// CameraSettings PrintConv hashes (Image::ExifTool::Minolta::CameraSettings)
// ============================================================================

static CS_EXPOSURE_MODE: &[(i64, &str)] = &[
    (0, "Program"),
    (1, "Aperture Priority"),
    (2, "Shutter Priority"),
    (3, "Manual"),
];
static CS_FLASH_MODE: &[(i64, &str)] = &[
    (0, "Fill flash"),
    (1, "Red-eye reduction"),
    (2, "Rear flash sync"),
    (3, "Wireless"),
    (4, "Off?"),
];
static CS_IMAGE_SIZE: &[(i64, &str)] = &[
    (0, "Full"),
    (1, "1600x1200"),
    (2, "1280x960"),
    (3, "640x480"),
    (6, "2080x1560"),
    (7, "2560x1920"),
    (8, "3264x2176"),
];
static CS_QUALITY: &[(i64, &str)] = &[
    (0, "Raw"),
    (1, "Super Fine"),
    (2, "Fine"),
    (3, "Standard"),
    (4, "Economy"),
    (5, "Extra Fine"),
];
static CS_DRIVE_MODE: &[(i64, &str)] = &[
    (0, "Single"),
    (1, "Continuous"),
    (2, "Self-timer"),
    (4, "Bracketing"),
    (5, "Interval"),
    (6, "UHS continuous"),
    (7, "HS continuous"),
];
static CS_METERING_MODE: &[(i64, &str)] = &[
    (0, "Multi-segment"),
    (1, "Center-weighted average"),
    (2, "Spot"),
];
static CS_MACRO_MODE: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
static CS_DIGITAL_ZOOM: &[(i64, &str)] = &[(0, "Off"), (1, "Electronic magnification"), (2, "2x")];
static CS_BRACKET_STEP: &[(i64, &str)] = &[(0, "1/3 EV"), (1, "2/3 EV"), (2, "1 EV")];
static CS_FLASH_FIRED: &[(i64, &str)] = &[(0, "No"), (1, "Yes")];
static CS_FILE_NUMBER_MEMORY: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
static CS_SHARPNESS: &[(i64, &str)] = &[(0, "Hard"), (1, "Normal"), (2, "Soft")];
static CS_SUBJECT_PROGRAM: &[(i64, &str)] = &[
    (0, "None"),
    (1, "Portrait"),
    (2, "Text"),
    (3, "Night portrait"),
    (4, "Sunset"),
    (5, "Sports action"),
];
static CS_ISO_SETTING: &[(i64, &str)] = &[
    (0, "100"),
    (1, "200"),
    (2, "400"),
    (3, "800"),
    (4, "Auto"),
    (5, "64"),
];
static CS_MODEL_ID: &[(i64, &str)] = &[
    (0, "DiMAGE 7, X1, X21 or X31"),
    (1, "DiMAGE 5"),
    (2, "DiMAGE S304"),
    (3, "DiMAGE S404"),
    (4, "DiMAGE 7i"),
    (5, "DiMAGE 7Hi"),
    (6, "DiMAGE A1"),
    (7, "DiMAGE A2 or S414"),
];
static CS_INTERVAL_MODE: &[(i64, &str)] = &[(0, "Still Image"), (1, "Time-lapse Movie")];
static CS_FOLDER_NAME: &[(i64, &str)] = &[(0, "Standard Form"), (1, "Data Form")];
static CS_COLOR_MODE: &[(i64, &str)] = &[
    (0, "Natural color"),
    (1, "Black & White"),
    (2, "Vivid color"),
    (3, "Solarization"),
    (4, "Adobe RGB"),
];
static CS_INTERNAL_FLASH: &[(i64, &str)] = &[(0, "No"), (1, "Fired")];
static CS_WIDE_FOCUS_ZONE: &[(i64, &str)] = &[
    (0, "No zone"),
    (1, "Center zone (horizontal orientation)"),
    (2, "Center zone (vertical orientation)"),
    (3, "Left zone"),
    (4, "Right zone"),
];
static CS_FOCUS_MODE: &[(i64, &str)] = &[(0, "AF"), (1, "MF")];
static CS_FOCUS_AREA: &[(i64, &str)] = &[(0, "Wide Focus (normal)"), (1, "Spot Focus")];
static CS_DEC_POSITION: &[(i64, &str)] = &[
    (0, "Exposure"),
    (1, "Contrast"),
    (2, "Saturation"),
    (3, "Filter"),
];
static CS_FLASH_METERING: &[(i64, &str)] = &[
    (0, "ADI (Advanced Distance Integration)"),
    (1, "Pre-flash TTL"),
    (2, "Manual flash control"),
];

/// `%minoltaWhiteBalance`, used by `ConvertWhiteBalance`.
static MINOLTA_WHITE_BALANCE: &[(i64, &str)] = &[
    (0, "Auto"),
    (1, "Daylight"),
    (2, "Cloudy"),
    (3, "Tungsten"),
    (5, "Custom"),
    (7, "Fluorescent"),
    (8, "Fluorescent 2"),
    (11, "Custom 2"),
    (12, "Custom 3"),
    (0x0800000, "Auto"),
    (0x1800000, "Daylight"),
    (0x2800000, "Cloudy"),
    (0x3800000, "Tungsten"),
    (0x4800000, "Flash"),
    (0x5800000, "Fluorescent"),
    (0x6800000, "Shade"),
    (0x7800000, "Custom1"),
    (0x8800000, "Custom2"),
    (0x9800000, "Custom3"),
];

// ============================================================================
// CameraSettings conversions
// ============================================================================

/// `Image::ExifTool::Minolta::ConvertWhiteBalance`.
///
/// The DiMAGE A2 shifts a preset by up to three steps of 0x10000, which the
/// suffix records; anything else with high bits set is genuinely unknown.
fn convert_white_balance(value: i64) -> Option<String> {
    if let Some(name) = lookup(MINOLTA_WHITE_BALANCE, value) {
        return Some(name);
    }
    if value & 0xffff_0000 != 0 {
        let base = (value & 0xff00_0000) + 0x0080_0000;
        return Some(match lookup(MINOLTA_WHITE_BALANCE, base) {
            Some(name) => format!("{}{:+}", name, (value - base) / 0x10000),
            None => format!("Unknown (0x{:x})", value),
        });
    }
    Some(unknown(value))
}

/// `2 ** (($val-48)/8) * 100` printed as `int($val + 0.5)`.
fn iso(value: i64) -> Option<String> {
    let iso = 2f64.powf((value as f64 - 48.0) / 8.0) * 100.0;
    Some(format!("{}", (iso + 0.5) as i64))
}

/// `2 ** ((48-$val)/8)` printed as an exposure time.
fn exposure_time(value: i64) -> Option<String> {
    Some(print_exposure_time(2f64.powf((48.0 - value as f64) / 8.0)))
}

/// `2 ** (($val-8)/16)` printed with one decimal place.
fn f_number(value: i64) -> Option<String> {
    Some(format!("{:.1}", 2f64.powf((value as f64 - 8.0) / 16.0)))
}

/// `$val / 256` printed as "N.N mm".
fn focal_length(value: i64) -> Option<String> {
    Some(format!("{:.1} mm", value as f64 / 256.0))
}

/// `$val / 1000` printed as "N m", or "inf" at zero.
fn focus_distance(value: i64) -> Option<String> {
    if value == 0 {
        return Some("inf".to_string());
    }
    Some(format!("{} m", print_float(value as f64 / 1000.0)))
}

/// `$val / 256`, printed as a bare number.
fn divide_by_256(value: i64) -> Option<String> {
    Some(print_float(value as f64 / 256.0))
}

/// `$val/8 - 6`.
fn brightness(value: i64) -> Option<String> {
    Some(print_float(value as f64 / 8.0 - 6.0))
}

/// `sprintf("%4d:%.2d:%.2d", $val>>16, ($val&0xff00)>>8, $val&0xff)`.
fn minolta_date(value: i64) -> Option<String> {
    Some(format!(
        "{:4}:{:02}:{:02}",
        value >> 16,
        (value & 0xff00) >> 8,
        value & 0xff
    ))
}

/// `sprintf("%.2d:%.2d:%.2d", $val>>16, ($val&0xff00)>>8, $val&0xff)`.
fn minolta_time(value: i64) -> Option<String> {
    Some(format!(
        "{:02}:{:02}:{:02}",
        value >> 16,
        (value & 0xff00) >> 8,
        value & 0xff
    ))
}

/// `Image::ExifTool::Exif::PrintFraction`, used for the EV compensations.
fn print_fraction(value: f64) -> String {
    crate::core::formatters::exif_print_conv::print_fraction(value)
}

/// `($val - 6) / 3` printed as a fraction.
fn flash_exposure_comp(value: i64) -> Option<String> {
    Some(print_fraction((value as f64 - 6.0) / 3.0))
}

/// `PrintConv => { 0 => 'Normal' }` over a `$val - 3` slider: anything but the
/// centre position prints as the bare number.
fn centred_slider(value: i64) -> Option<String> {
    let shifted = value - 3;
    Some(if shifted == 0 {
        "Normal".to_string()
    } else {
        shifted.to_string()
    })
}

/// `$val - 3`, printed bare (ColorFilter has no PrintConv).
fn shifted_by_three(value: i64) -> Option<String> {
    Some((value - 3).to_string())
}

// ============================================================================
// CameraSettings table (tags 0x0001 and 0x0003)
// ============================================================================

/// `Image::ExifTool::Minolta::CameraSettings`, keyed in `int32u` units and read
/// big-endian.
///
/// Omitted because this `PRIORITY => 0` table loses ExifTool's duplicate
/// suppression to the identically-named standard EXIF tags: index 1
/// `ExposureMode`, 7 `MeteringMode`, 8 `ISO`, 9 `ExposureTime`, 10 `FNumber`,
/// 13 `ExposureCompensation`, 18 `FocalLength`, 31 `Saturation`, 32
/// `Contrast`, 33 `Sharpness`.
///
/// The `$self->{Model} =~ /DiMAGE A2/` variants of Saturation, Contrast and
/// ColorFilter shift by 5 rather than 3; only the general form is used here,
/// and Saturation/Contrast are dropped anyway.
static CAMERA_SETTINGS_TAGS: &[BinTag] = &[
    BinTag::map(2, "FlashMode", CS_FLASH_MODE),
    BinTag::conv(3, "WhiteBalance", convert_white_balance),
    BinTag::map(4, "MinoltaImageSize", CS_IMAGE_SIZE),
    BinTag::map(5, "MinoltaQuality", CS_QUALITY),
    BinTag::map(6, "DriveMode", CS_DRIVE_MODE),
    BinTag::map(11, "MacroMode", CS_MACRO_MODE),
    BinTag::map(12, "DigitalZoom", CS_DIGITAL_ZOOM),
    BinTag::map(14, "BracketStep", CS_BRACKET_STEP),
    BinTag::int(16, "IntervalLength"),
    BinTag::int(17, "IntervalNumber"),
    BinTag::conv(19, "FocusDistance", focus_distance),
    BinTag::map(20, "FlashFired", CS_FLASH_FIRED),
    BinTag::conv(21, "MinoltaDate", minolta_date),
    BinTag::conv(22, "MinoltaTime", minolta_time),
    BinTag::conv(23, "MaxAperture", f_number),
    BinTag::map(26, "FileNumberMemory", CS_FILE_NUMBER_MEMORY),
    BinTag::int(27, "LastFileNumber"),
    BinTag::conv(28, "ColorBalanceRed", divide_by_256),
    BinTag::conv(29, "ColorBalanceGreen", divide_by_256),
    BinTag::conv(30, "ColorBalanceBlue", divide_by_256),
    BinTag::map(34, "SubjectProgram", CS_SUBJECT_PROGRAM),
    BinTag::conv(35, "FlashExposureComp", flash_exposure_comp),
    BinTag::map(36, "ISOSetting", CS_ISO_SETTING),
    BinTag::map(37, "MinoltaModelID", CS_MODEL_ID),
    BinTag::map(38, "IntervalMode", CS_INTERVAL_MODE),
    BinTag::map(39, "FolderName", CS_FOLDER_NAME),
    BinTag::map(40, "ColorMode", CS_COLOR_MODE),
    BinTag::conv(41, "ColorFilter", shifted_by_three),
    BinTag::int(42, "BWFilter"),
    BinTag::map(43, "InternalFlash", CS_INTERNAL_FLASH),
    BinTag::conv(44, "Brightness", brightness),
    BinTag::int(45, "SpotFocusPointX"),
    BinTag::int(46, "SpotFocusPointY"),
    BinTag::map(47, "WideFocusZone", CS_WIDE_FOCUS_ZONE),
    BinTag::map(48, "FocusMode", CS_FOCUS_MODE),
    BinTag::map(49, "FocusArea", CS_FOCUS_AREA),
    BinTag::map(50, "DECPosition", CS_DEC_POSITION),
    BinTag::map(63, "FlashMetering", CS_FLASH_METERING),
];

/// The `CameraSettings` binary table.
pub static CAMERA_SETTINGS: BinTable = BinTable {
    format: Fmt::U32,
    big_endian: true,
    tags: CAMERA_SETTINGS_TAGS,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn white_balance_handles_the_a2_shifted_presets() {
        assert_eq!(convert_white_balance(0), Some("Auto".to_string()));
        assert_eq!(
            convert_white_balance(0x1800000),
            Some("Daylight".to_string())
        );
        assert_eq!(
            convert_white_balance(0x1810000),
            Some("Daylight+1".to_string())
        );
    }

    #[test]
    fn date_and_time_unpack_from_one_int32u() {
        // Minolta.jpg: 2002:06:01 and 12:37:27.
        assert_eq!(
            minolta_date((2002 << 16) | (6 << 8) | 1),
            Some("2002:06:01".to_string())
        );
        assert_eq!(
            minolta_time((12 << 16) | (37 << 8) | 27),
            Some("12:37:27".to_string())
        );
    }

    #[test]
    fn colour_balance_divides_by_256() {
        assert_eq!(divide_by_256(383), Some("1.49609375".to_string()));
        assert_eq!(divide_by_256(256), Some("1".to_string()));
        assert_eq!(divide_by_256(352), Some("1.375".to_string()));
    }

    #[test]
    fn focus_distance_reports_infinity_at_zero() {
        assert_eq!(focus_distance(0), Some("inf".to_string()));
        assert_eq!(focus_distance(2000), Some("2 m".to_string()));
    }

    #[test]
    fn print_fraction_matches_exiftool() {
        assert_eq!(print_fraction(0.0), "0");
        assert_eq!(print_fraction(1.0), "+1");
        assert_eq!(print_fraction(-1.0 / 3.0), "-1/3");
        assert_eq!(print_fraction(0.5), "+1/2");
    }

    #[test]
    fn camera_settings_indices_are_scaled_by_int32u() {
        // Index 20 (FlashFired) is byte offset 80.
        let mut data = vec![0u8; 84];
        data[83] = 1;
        let mut tags = HashMap::new();
        CAMERA_SETTINGS.extract(&data, "Minolta", &mut tags);
        assert_eq!(tags.get("Minolta:FlashFired"), Some(&"Yes".to_string()));
    }
}
