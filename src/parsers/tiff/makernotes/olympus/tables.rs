//! Olympus MakerNote directory tables, ported from ExifTool's `Olympus.pm`.
//!
//! One table per directory, because Olympus reuses tag IDs across directories
//! (0x0100 is `CameraType2` in Equipment, `PreviewImageValid` in
//! CameraSettings, `WB_RBLevels` in ImageProcessing). Tags whose ExifTool
//! conversion we cannot reproduce exactly are simply left out -- a dropped tag
//! is better than a wrong one.

use super::ifd::{Conv, ElemConv, OlyVal, TagDef, ftype};
use super::lookups::{CAMERA_TYPE2, EQUIPMENT_EXTENDER, EQUIPMENT_LENS_TYPE};

// ===========================================================================
// Shared PrintConv hashes
// ===========================================================================

static OFF_ON: &[(i64, &str)] = &[(0, "Off"), (1, "On")];
static NO_YES: &[(i64, &str)] = &[(0, "No"), (1, "Yes")];
static COLOR_SPACE: &[(i64, &str)] = &[(0, "sRGB"), (1, "Adobe RGB"), (2, "Pro Photo RGB")];

/// `Olympus.pm` `%filters` -- shared by ArtFilter, MagicFilter and
/// RawDevArtFilter.
static FILTERS: &[(i64, &str)] = &[
    (0, "Off"),
    (1, "Soft Focus"),
    (2, "Pop Art"),
    (3, "Pale & Light Color"),
    (4, "Light Tone"),
    (5, "Pin Hole"),
    (6, "Grainy Film"),
    (8, "Underwater"),
    (9, "Diorama"),
    (10, "Cross Process"),
    (12, "Fish Eye"),
    (13, "Drawing"),
    (14, "Gentle Sepia"),
    (15, "Pale & Light Color II"),
    (16, "Pop Art II"),
    (17, "Pin Hole II"),
    (18, "Pin Hole III"),
    (19, "Grainy Film II"),
    (20, "Dramatic Tone"),
    (21, "Punk"),
    (22, "Soft Focus 2"),
    (23, "Sparkle"),
    (24, "Watercolor"),
    (25, "Key Line"),
    (26, "Key Line II"),
    (27, "Miniature"),
    (28, "Reflection"),
    (29, "Fragmented"),
    (31, "Cross Process II"),
    (32, "Dramatic Tone II"),
    (33, "Watercolor I"),
    (34, "Watercolor II"),
    (35, "Diorama II"),
    (36, "Vintage"),
    (37, "Vintage II"),
    (38, "Vintage III"),
    (39, "Partial Color"),
    (40, "Partial Color II"),
    (41, "Partial Color III"),
];

/// `Olympus.pm` `%toneLevelType`.
static TONE_LEVEL_TYPE: &[(i64, &str)] = &[
    (0, "0"),
    (-31999, "Highlights"),
    (-31998, "Shadows"),
    (-31997, "Midtones"),
];

/// PrintConv list shared by `ContrastSetting`, `SharpnessSetting`,
/// `CustomSaturation`, `PictureMode{Saturation,Contrast,Sharpness}`:
/// ExifTool renders these as `"$v[0] (min $v[1], max $v[2])"`.
fn print_min_max(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.len() < 3 {
        return None;
    }
    Some(format!("{} (min {}, max {})", v[0], v[1], v[2]))
}

/// `Olympus.pm` `PrintAFAreas`: each non-zero 32-bit word is four bytes of
/// big-endian corner coordinates, with three well-known words named.
fn print_af_areas(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    let mut parts: Vec<String> = Vec::new();
    for &pt in v {
        if pt == 0 {
            continue;
        }
        let w = pt as u32;
        let name = match w {
            0x3679_4285 => "Left ",
            0x7979_8585 => "Center ",
            0xBD79_C985 => "Right ",
            _ => "",
        };
        let b = w.to_be_bytes();
        parts.push(format!("{}({},{})-({},{})", name, b[0], b[1], b[2], b[3]));
    }
    if parts.is_empty() {
        return Some("none".to_string());
    }
    Some(parts.join(", "))
}

/// `Olympus.pm` Equipment 0x0104 / 0x0204 / 0x0304 firmware version:
/// `sprintf("%x")` then a decimal point three digits from the right.
fn print_firmware(val: &OlyVal) -> Option<String> {
    let v = val.first_int()?;
    let mut s = format!("{:x}", v);
    if s.len() >= 3 {
        let at = s.len() - 3;
        s.insert(at, '.');
    }
    Some(s)
}

/// Equipment 0x0201 LensType: ExifTool keys the hash on
/// `sprintf("%x %.2x %.2x", @bytes[0,2,3])`.
fn print_lens_type(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.len() < 4 {
        return None;
    }
    let key = format!("{:x} {:02x} {:02x}", v[0], v[2], v[3]);
    Some(match EQUIPMENT_LENS_TYPE.iter().find(|(k, _)| *k == key) {
        Some((_, s)) => (*s).to_string(),
        None => format!("Unknown ({})", key),
    })
}

/// Equipment 0x0301 Extender: `sprintf("%x %.2x", @bytes[0,2])`.
fn print_extender(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.len() < 3 {
        return None;
    }
    let key = format!("{:x} {:02x}", v[0], v[2]);
    Some(match EQUIPMENT_EXTENDER.iter().find(|(k, _)| *k == key) {
        Some((_, s)) => (*s).to_string(),
        None => format!("Unknown ({})", key),
    })
}

/// Equipment 0x0205/0x0206/0x020A apertures: ExifTool's
/// `ValueConv => '$val ? sqrt(2)**($val/256) : 0'` then `sprintf("%.1f")`.
fn print_aperture(val: &OlyVal) -> Option<String> {
    let v = val.first_int()?;
    let f = if v == 0 {
        0.0
    } else {
        2f64.sqrt().powf(v as f64 / 256.0)
    };
    Some(format!("{:.1}", f))
}

/// Equipment 0x1001 FlashModel PrintConv.
static EQUIPMENT_FLASH_MODEL: &[(i64, &str)] = &[
    (0, "None"),
    (1, "FL-20"),
    (2, "FL-50"),
    (3, "RF-11"),
    (4, "TF-22"),
    (5, "FL-36"),
    (6, "FL-50R"),
    (7, "FL-36R"),
    (9, "FL-14"),
    (11, "FL-600R"),
    (13, "FL-LM3"),
    (15, "FL-900R"),
];

/// Equipment 0x020B LensProperties: ExifTool prints it as `0x%x`.
fn print_hex(val: &OlyVal) -> Option<String> {
    let v = val.first_int()?;
    Some(format!("0x{:x}", v))
}

/// `"$val mm"`.
fn print_mm(val: &OlyVal) -> Option<String> {
    Some(format!("{} mm", val.print_raw()))
}

/// CameraSettings 0x0405/0x0406: ExifTool prints all-`undef` rationals as
/// `n/a` (or `n/a (x4)` for four of them) and leaves anything else raw.
fn print_flash_strength(val: &OlyVal) -> Option<String> {
    let raw = val.print_raw();
    let n = val.len();
    if raw.split(' ').all(|p| p == "undef") {
        return Some(if n == 4 {
            "n/a (x4)".to_string()
        } else {
            "n/a".to_string()
        });
    }
    Some(raw)
}

/// CameraSettings 0x0901 ManometerReading: both values are tenths, printed as
/// `"<m> m, <ft> ft"`.
fn print_manometer_reading(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.len() < 2 {
        return None;
    }
    Some(format!(
        "{} m, {} ft",
        super::ifd::fmt_g15(v[0] as f64 / 10.0),
        super::ifd::fmt_g15(v[1] as f64 / 10.0)
    ))
}

/// CameraSettings 0x0900 ManometerPressure: tenths of a kPa.
fn print_manometer_pressure(val: &OlyVal) -> Option<String> {
    let v = val.first_int()?;
    Some(format!("{} kPa", super::ifd::fmt_g15(v as f64 / 10.0)))
}

/// CameraSettings 0x0908 DateTimeUTC: stored `YYYY:MM:DD HH:MM:SS`, printed
/// unchanged by `ConvertDateTime` in the default (no -d) configuration.
fn print_datetime(val: &OlyVal) -> Option<String> {
    let s = val.as_string()?;
    let s = s.trim_end_matches(|c: char| c == '\0' || c.is_whitespace());
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// Main 0x0200 SpecialMode: three values -- mode, sequence number, panorama
/// direction (`Olympus.pm` `PrintConv` sub).
fn print_special_mode(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.is_empty() {
        return None;
    }
    let modes = ["Normal", "Unknown", "Fast", "Panorama"];
    let dirs = [
        "(none)",
        "Left to Right",
        "Right to Left",
        "Bottom to Top",
        "Top to Bottom",
    ];
    let mode = match usize::try_from(v[0]).ok().and_then(|i| modes.get(i)) {
        Some(m) => (*m).to_string(),
        None => format!("Unknown ({})", v[0]),
    };
    let seq = v.get(1).copied().unwrap_or(0);
    let dir_raw = v.get(2).copied().unwrap_or(0);
    let dir = match usize::try_from(dir_raw).ok().and_then(|i| dirs.get(i)) {
        Some(d) => (*d).to_string(),
        None => format!("Unknown ({})", dir_raw),
    };
    Some(format!("{}, Sequence: {}, Panorama: {}", mode, seq, dir))
}

/// Main 0x0204 DigitalZoom: ExifTool appends `.0` when the value has no
/// decimal point.
fn print_digital_zoom(val: &OlyVal) -> Option<String> {
    let raw = val.print_raw();
    Some(if raw.contains('.') {
        raw
    } else {
        format!("{}.0", raw)
    })
}

/// Main 0x1001 ISOValue: `100 * 2 ** ($val - 5)` rounded to two decimals.
fn print_iso_value(val: &OlyVal) -> Option<String> {
    let v = match val {
        OlyVal::Rat(r) => {
            let (n, d) = *r.first()?;
            if d == 0 {
                return None;
            }
            n as f64 / d as f64
        }
        OlyVal::Int(i) => *i.first()? as f64,
        _ => return None,
    };
    let iso = 100.0 * 2f64.powf(v - 5.0);
    Some(super::ifd::fmt_g15((iso * 100.0 + 0.5).floor() / 100.0))
}

/// FocusInfo 0x1209 ManualFlash: `Off`, or `On (1/N strength)`.
fn print_manual_flash(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.is_empty() {
        return None;
    }
    if v[0] == 0 {
        return Some("Off".to_string());
    }
    let b = *v.get(1)?;
    Some(if b == 1 {
        "On (Full strength)".to_string()
    } else {
        format!("On (1/{} strength)", b)
    })
}

/// CameraSettings 0x0305 AFPointSelected: five rationals, the first ignored;
/// any `undef` (zero denominator) means `n/a`, otherwise two percentages.
fn print_af_point_selected(val: &OlyVal) -> Option<String> {
    let OlyVal::Rat(r) = val else { return None };
    if r.len() < 5 {
        return None;
    }
    let rest = &r[1..];
    if rest.iter().any(|&(_, d)| d == 0) {
        return Some("n/a".to_string());
    }
    let pct: Vec<i64> = rest
        .iter()
        .map(|&(n, d)| (n as f64 / d as f64 * 100.0) as i64)
        .collect();
    Some(format!(
        "({}%,{}%) ({}%,{}%)",
        pct[0], pct[1], pct[2], pct[3]
    ))
}

/// CameraSettings 0x0600 DriveMode, ported from `Olympus.pm`'s PrintConv.
fn print_drive_mode(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.is_empty() {
        return None;
    }
    let (a, b, c, e, f) = (
        v[0],
        v.get(1).copied().unwrap_or(0),
        v.get(2).copied(),
        v.get(4).copied(),
        v.get(5).copied().unwrap_or(0),
    );
    let shot = if b != 0 {
        format!(", Shot {}", b)
    } else {
        String::new()
    };
    let shutter = match e {
        None | Some(4) => String::new(),
        Some(0) => "; Mechanical shutter".to_string(),
        Some(2) => "; Anti-shock".to_string(),
        Some(other) => format!("; Unknown ({})", other),
    };
    let mode = if a == 5 && c.is_some() {
        let bits = super::ifd::decode_bits(
            c.unwrap_or(0),
            &[
                (0, "AE"),
                (1, "WB"),
                (2, "FL"),
                (3, "MF"),
                (4, "ISO"),
                (5, "AE Auto"),
                (6, "Focus"),
            ],
        );
        format!("{} Bracketing", bits.replace(", ", "+"))
    } else if f != 0 {
        match DRIVE_MODE_BYTE6.iter().find(|(k, _)| *k == f) {
            Some((_, s)) => (*s).to_string(),
            None => format!("Unknown ({})", f),
        }
    } else {
        match DRIVE_MODE_BASIC.iter().find(|(k, _)| *k == a) {
            Some((_, s)) => (*s).to_string(),
            None => format!("Unknown ({})", a),
        }
    };
    Some(format!("{}{}{}", mode, shot, shutter))
}

static DRIVE_MODE_BASIC: &[(i64, &str)] = &[
    (0, "Single Shot"),
    (1, "Continuous Shooting"),
    (2, "Exposure Bracketing"),
    (3, "White Balance Bracketing"),
    (4, "Exposure+WB Bracketing"),
];

/// `Olympus.pm` DriveMode byte 6 (E-M1 and later).
static DRIVE_MODE_BYTE6: &[(i64, &str)] = &[
    (0x01, "Single Shot"),
    (0x02, "Sequential L"),
    (0x03, "Sequential H"),
    (0x07, "Sequential"),
    (0x11, "Single Shot"),
    (0x12, "Sequential L"),
    (0x13, "Sequential H"),
    (0x14, "Self-Timer 12 sec"),
    (0x15, "Self-Timer 2 sec"),
    (0x16, "Custom Self-Timer"),
    (0x17, "Sequential"),
    (0x21, "Single Shot"),
    (0x22, "Sequential L"),
    (0x23, "Sequential H"),
    (0x24, "Self-Timer 2 sec"),
    (0x25, "Self-Timer 12 sec"),
    (0x26, "Custom Self-Timer"),
    (0x27, "Sequential"),
    (0x28, "Sequential SH1"),
    (0x29, "Sequential SH2"),
    (0x30, "HighRes Shot"),
    (0x41, "ProCap H"),
    (0x42, "ProCap L"),
    (0x43, "ProCap"),
    (0x48, "ProCap SH1"),
    (0x49, "ProCap SH2"),
];

/// CameraSettings 0x0601 PanoramaMode: `Off`, or direction plus shot number.
fn print_panorama_mode(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.is_empty() {
        return None;
    }
    if v[0] == 0 {
        return Some("Off".to_string());
    }
    let dir = match v[0] {
        1 => "Left to Right".to_string(),
        2 => "Right to Left".to_string(),
        3 => "Bottom to Top".to_string(),
        4 => "Top to Bottom".to_string(),
        n => format!("Unknown ({})", n),
    };
    Some(format!("{}, Shot {}", dir, v.get(1).copied().unwrap_or(0)))
}

/// FocusInfo 0x1500 SensorTemperature, multi-value form: ExifTool drops a
/// trailing `" 0 0"` and appends ` C`. The single-value form needs the camera
/// model to pick between two conversions, so it is left to the caller.
fn print_sensor_temperature_multi(val: &OlyVal) -> Option<String> {
    let raw = val.print_raw();
    let trimmed = raw.strip_suffix(" 0 0").unwrap_or(&raw);
    Some(format!("{} C", trimmed))
}

// ===========================================================================
// Olympus::Main
// ===========================================================================

pub static MAIN: &[TagDef] = &[
    TagDef::func(0x0200, "SpecialMode", print_special_mode),
    TagDef::lookup(
        0x0202,
        "Macro",
        &[(0, "Off"), (1, "On"), (2, "Super Macro")],
    ),
    TagDef::lookup(0x0203, "BWMode", &[(0, "Off"), (1, "On"), (6, "(none)")]),
    TagDef::func(0x0204, "DigitalZoom", print_digital_zoom),
    TagDef::func(0x0205, "FocalPlaneDiagonal", print_mm),
    TagDef::raw(0x0206, "LensDistortionParams"),
    TagDef::text(0x0209, "CameraID"),
    TagDef::raw(0x020B, "EpsonImageWidth"),
    TagDef::raw(0x020C, "EpsonImageHeight"),
    TagDef::text(0x020D, "EpsonSoftware"),
    TagDef::binary(0x0280, "PreviewImage"),
    TagDef::raw(0x0300, "PreCaptureFrames"),
    TagDef::raw(0x0301, "WhiteBoard"),
    TagDef::lookup(
        0x0302,
        "OneTouchWB",
        &[(0, "Off"), (1, "On"), (2, "On (Preset)")],
    ),
    TagDef::raw(0x0303, "WhiteBalanceBracket"),
    TagDef::raw(0x0304, "WhiteBalanceBias"),
    TagDef::binary(0x0F00, "DataDump"),
    TagDef::binary(0x0F01, "DataDump2"),
    TagDef::raw(0x0F04, "ZoomedPreviewStart"),
    TagDef::raw(0x0F05, "ZoomedPreviewLength"),
    TagDef::raw(0x0F06, "ZoomedPreviewSize"),
    TagDef::func(0x1001, "ISOValue", print_iso_value),
    TagDef::raw(0x1003, "BrightnessValue"),
    TagDef::lookup(0x1004, "FlashMode", &[(2, "On"), (3, "Off")]),
    TagDef::lookup(
        0x1005,
        "FlashDevice",
        &[
            (0, "None"),
            (1, "Internal"),
            (4, "External"),
            (5, "Internal + External"),
        ],
    ),
    TagDef::raw(0x1006, "ExposureCompensation"),
    TagDef::raw(0x1008, "LensTemperature"),
    TagDef::raw(0x1009, "LightCondition"),
    TagDef::lookup(0x100A, "FocusRange", &[(0, "Normal"), (1, "Macro")]),
    TagDef::lookup(0x100B, "FocusMode", &[(0, "Auto"), (1, "Manual")]),
    TagDef::func(0x100C, "ManualFocusDistance", print_mm),
    TagDef::raw(0x100D, "ZoomStepCount"),
    TagDef::raw(0x100E, "FocusStepCount"),
    TagDef::lookup(
        0x100F,
        "Sharpness",
        &[(0, "Normal"), (1, "Hard"), (2, "Soft")],
    ),
    TagDef::raw(0x1010, "FlashChargeLevel"),
    TagDef::typed(0x1011, "ColorMatrix", ftype::TIFF_SSHORT),
    TagDef::raw(0x1012, "BlackLevel"),
    TagDef::raw(0x1019, "ColorMatrixNumber"),
    TagDef::raw(0x1024, "InternalFlashTable"),
    TagDef::raw(0x1025, "ExternalFlashGValue"),
    TagDef::lookup(0x1026, "ExternalFlashBounce", NO_YES),
    TagDef::raw(0x1027, "ExternalFlashZoom"),
    TagDef::raw(0x1028, "ExternalFlashMode"),
    TagDef::lookup(
        0x1029,
        "Contrast",
        &[(0, "High"), (1, "Normal"), (2, "Low")],
    ),
    TagDef::raw(0x102A, "SharpnessFactor"),
    TagDef::raw(0x102B, "ColorControl"),
    TagDef::raw(0x102C, "ValidBits"),
    TagDef::raw(0x102D, "CoringFilter"),
    TagDef::raw(0x102E, "OlympusImageWidth"),
    TagDef::raw(0x102F, "OlympusImageHeight"),
    TagDef::raw(0x1030, "SceneDetect"),
    TagDef::raw(0x1034, "CompressionRatio"),
    TagDef::lookup(0x1035, "PreviewImageValid", NO_YES),
    TagDef::raw(0x1038, "AFResult"),
    TagDef::lookup(
        0x1039,
        "CCDScanMode",
        &[(0, "Interlaced"), (1, "Progressive")],
    ),
    TagDef::lookup(0x103A, "NoiseReduction", OFF_ON),
    TagDef::raw(0x103B, "FocusStepInfinity"),
    TagDef::raw(0x103C, "FocusStepNear"),
    TagDef::raw(0x103D, "LightValueCenter"),
    TagDef::raw(0x103E, "LightValuePeriphery"),
    TagDef::raw(0x103F, "FieldCount"),
];

// ===========================================================================
// Olympus::Equipment (0x2010)
// ===========================================================================

pub static EQUIPMENT: &[TagDef] = &[
    TagDef::text(0x0000, "EquipmentVersion"),
    TagDef::str_lookup(0x0100, "CameraType2", CAMERA_TYPE2),
    TagDef::text_trim(0x0101, "SerialNumber"),
    TagDef::text(0x0102, "InternalSerialNumber"),
    TagDef::func(0x0103, "FocalPlaneDiagonal", print_mm),
    TagDef::func(0x0104, "BodyFirmwareVersion", print_firmware),
    TagDef::func(0x0201, "LensType", print_lens_type),
    TagDef::text(0x0202, "LensSerialNumber"),
    TagDef::text(0x0203, "LensModel"),
    TagDef::func(0x0204, "LensFirmwareVersion", print_firmware),
    TagDef::func(0x0205, "MaxApertureAtMinFocal", print_aperture),
    TagDef::func(0x0206, "MaxApertureAtMaxFocal", print_aperture),
    TagDef::raw(0x0207, "MinFocalLength"),
    TagDef::raw(0x0208, "MaxFocalLength"),
    TagDef::func(0x020A, "MaxAperture", print_aperture),
    TagDef::func(0x020B, "LensProperties", print_hex),
    TagDef::func(0x0301, "Extender", print_extender),
    TagDef::text(0x0302, "ExtenderSerialNumber"),
    TagDef::text(0x0303, "ExtenderModel"),
    TagDef::func(0x0304, "ExtenderFirmwareVersion", print_firmware),
    TagDef::text(0x0403, "ConversionLens"),
    TagDef::lookup(
        0x1000,
        "FlashType",
        &[
            (0, "None"),
            (2, "Simple E-System"),
            (3, "E-System"),
            (4, "E-System (body powered)"),
        ],
    ),
    TagDef::lookup(0x1001, "FlashModel", EQUIPMENT_FLASH_MODEL),
    TagDef::func(0x1002, "FlashFirmwareVersion", print_firmware),
    TagDef::text(0x1003, "FlashSerialNumber"),
];

// ===========================================================================
// Olympus::CameraSettings (0x2020)
// ===========================================================================

static CS_METERING: &[(i64, &str)] = &[
    (2, "Center-weighted average"),
    (3, "Spot"),
    (5, "ESP"),
    (261, "Pattern+AF"),
    (515, "Spot+Highlight control"),
    (1027, "Spot+Shadow control"),
];

static CS_FOCUS_MODE_1: &[(i64, &str)] = &[
    (0, "Single AF"),
    (1, "Sequential shooting AF"),
    (2, "Continuous AF"),
    (3, "Multi AF"),
    (4, "Face Detect"),
    (10, "MF"),
];

static CS_FOCUS_MODE_BITS: &[(u32, &str)] = &[
    (0, "S-AF"),
    (2, "C-AF"),
    (4, "MF"),
    (5, "Face Detect"),
    (6, "Imager AF"),
    (7, "Live View Magnification Frame"),
    (8, "AF sensor"),
    (9, "Starry Sky AF"),
];

static CS_FLASH_MODE_BITS: &[(u32, &str)] = &[
    (0, "On"),
    (1, "Fill-in"),
    (2, "Red-eye"),
    (3, "Slow-sync"),
    (4, "Forced On"),
    (5, "2nd Curtain"),
];

static CS_NOISE_REDUCTION_BITS: &[(u32, &str)] = &[
    (0, "Noise Reduction"),
    (1, "Noise Filter"),
    (2, "Noise Filter (ISO Boost)"),
    (3, "Auto"),
];

static CS_WHITE_BALANCE2: &[(i64, &str)] = &[
    (0, "Auto"),
    (1, "Auto (Keep Warm Color Off)"),
    (16, "7500K (Fine Weather with Shade)"),
    (17, "6000K (Cloudy)"),
    (18, "5300K (Fine Weather)"),
    (20, "3000K (Tungsten light)"),
    (21, "3600K (Tungsten light-like)"),
    (22, "Auto Setup"),
    (23, "5500K (Flash)"),
    (33, "6600K (Daylight fluorescent)"),
    (34, "4500K (Neutral white fluorescent)"),
    (35, "4000K (Cool white fluorescent)"),
    (36, "White Fluorescent"),
    (48, "3600K (Tungsten light-like)"),
    (67, "Underwater"),
    (256, "One Touch WB 1"),
    (257, "One Touch WB 2"),
    (258, "One Touch WB 3"),
    (259, "One Touch WB 4"),
    (512, "Custom WB 1"),
    (513, "Custom WB 2"),
    (514, "Custom WB 3"),
    (515, "Custom WB 4"),
];

static CS_SCENE_MODE: &[(i64, &str)] = &[
    (0, "Standard"),
    (6, "Auto"),
    (7, "Sport"),
    (8, "Portrait"),
    (9, "Landscape+Portrait"),
    (10, "Landscape"),
    (11, "Night Scene"),
    (12, "Self Portrait"),
    (13, "Panorama"),
    (14, "2 in 1"),
    (15, "Movie"),
    (16, "Landscape+Portrait"),
    (17, "Night+Portrait"),
    (18, "Indoor"),
    (19, "Fireworks"),
    (20, "Sunset"),
    (21, "Beauty Skin"),
    (22, "Macro"),
    (23, "Super Macro"),
    (24, "Food"),
    (25, "Documents"),
    (26, "Museum"),
    (27, "Shoot & Select"),
    (28, "Beach & Snow"),
    (29, "Self Protrait+Timer"),
    (30, "Candle"),
    (31, "Available Light"),
    (32, "Behind Glass"),
    (33, "My Mode"),
    (34, "Pet"),
    (35, "Underwater Wide1"),
    (36, "Underwater Macro"),
    (37, "Shoot & Select1"),
    (38, "Shoot & Select2"),
    (39, "High Key"),
    (40, "Digital Image Stabilization"),
    (41, "Auction"),
    (42, "Beach"),
    (43, "Snow"),
    (44, "Underwater Wide2"),
    (45, "Low Key"),
    (46, "Children"),
    (47, "Vivid"),
    (48, "Nature Macro"),
    (49, "Underwater Snapshot"),
    (50, "Shooting Guide"),
    (54, "Face Portrait"),
    (57, "Bulb"),
    (59, "Smile Shot"),
    (60, "Quick Shutter"),
    (63, "Slow Shutter"),
    (64, "Bird Watching"),
    (65, "Multiple Exposure"),
    (66, "e-Portrait"),
    (67, "Soft Background Shot"),
    (142, "Hand-held Starlight"),
    (154, "HDR"),
    (197, "Panning"),
    (203, "Light Trails"),
    (204, "Backlight HDR"),
    (205, "Silent"),
    (206, "Multi Focus Shot"),
];

static CS_PICTURE_MODE: &[(i64, &str)] = &[
    (1, "Vivid"),
    (2, "Natural"),
    (3, "Muted"),
    (4, "Portrait"),
    (5, "i-Enhance"),
    (6, "e-Portrait"),
    (7, "Color Creator"),
    (8, "Underwater"),
    (9, "Color Profile 1"),
    (10, "Color Profile 2"),
    (11, "Color Profile 3"),
    (12, "Monochrome Profile 1"),
    (13, "Monochrome Profile 2"),
    (14, "Monochrome Profile 3"),
    (17, "Art Mode"),
    (18, "Monochrome Profile 4"),
    (256, "Monotone"),
    (512, "Sepia"),
];

static CS_BW_FILTER: &[(i64, &str)] = &[
    (0, "n/a"),
    (1, "Neutral"),
    (2, "Yellow"),
    (3, "Orange"),
    (4, "Red"),
    (5, "Green"),
];

static CS_PICTURE_TONE: &[(i64, &str)] = &[
    (0, "n/a"),
    (1, "Neutral"),
    (2, "Sepia"),
    (3, "Blue"),
    (4, "Purple"),
    (5, "Green"),
];

static CS_NOISE_FILTER: &[(&str, &str)] = &[
    ("-1 -2 1", "Low"),
    ("-2 -2 1", "Off"),
    ("0 -2 1", "Standard"),
    ("0 0 0", "n/a"),
    ("1 -2 1", "High"),
];

static CS_PICTURE_MODE_EFFECT: &[(&str, &str)] = &[
    ("-1 -1 1", "Low"),
    ("0 -1 1", "Standard"),
    ("0 0 0", "n/a"),
    ("1 -1 1", "High"),
];

static CS_GRADATION_HEAD: &[(&str, &str)] = &[
    ("0 0 0", "n/a"),
    ("-1 -1 1", "Low Key"),
    ("0 -1 1", "Normal"),
    ("1 -1 1", "High Key"),
];

static CS_ART_FILTER_LIST: &[ElemConv] = &[ElemConv::Map(FILTERS)];

static CS_ART_FILTER_EFFECT_LIST: &[ElemConv] = &[
    ElemConv::Map(FILTERS),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Prefix("Partial Color"),
    ElemConv::Map(&[
        (0x0000, "No Effect"),
        (0x8010, "Star Light"),
        (0x8020, "Pin Hole"),
        (0x8030, "Frame"),
        (0x8040, "Soft Focus"),
        (0x8050, "White Edge"),
        (0x8060, "B&W"),
        (0x8080, "Blur Top and Bottom"),
        (0x8081, "Blur Left and Right"),
    ]),
    ElemConv::Raw,
    ElemConv::Map(&[
        (0, "No Color Filter"),
        (1, "Yellow Color Filter"),
        (2, "Orange Color Filter"),
        (3, "Red Color Filter"),
        (4, "Green Color Filter"),
    ]),
];

static CS_TONE_LEVEL_LIST: &[ElemConv] = &[
    ElemConv::Map(TONE_LEVEL_TYPE),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Map(TONE_LEVEL_TYPE),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Map(TONE_LEVEL_TYPE),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Map(TONE_LEVEL_TYPE),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Map(TONE_LEVEL_TYPE),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Map(TONE_LEVEL_TYPE),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Map(TONE_LEVEL_TYPE),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Raw,
];

static CS_COLOR_CREATOR_LIST: &[ElemConv] = &[
    ElemConv::Prefix("Color"),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Prefix("Strength"),
    ElemConv::Raw,
    ElemConv::Raw,
];

static CS_MONO_PROFILE_LIST: &[ElemConv] = &[
    ElemConv::Map(&[
        (0, "No Filter"),
        (1, "Yellow Filter"),
        (2, "Orange Filter"),
        (3, "Red Filter"),
        (4, "Magenta Filter"),
        (5, "Blue Filter"),
        (6, "Cyan Filter"),
        (7, "Green Filter"),
        (8, "Yellow-green Filter"),
    ]),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Prefix("Strength"),
    ElemConv::Raw,
    ElemConv::Raw,
];

static CS_COLOR_PROFILE_LIST: &[ElemConv] = &[
    ElemConv::Prefix("Min"),
    ElemConv::Prefix("Max"),
    ElemConv::Prefix("Yellow"),
    ElemConv::Prefix("Orange"),
    ElemConv::Prefix("Orange-red"),
    ElemConv::Prefix("Red"),
    ElemConv::Prefix("Magenta"),
    ElemConv::Prefix("Violet"),
    ElemConv::Prefix("Blue"),
    ElemConv::Prefix("Blue-cyan"),
    ElemConv::Prefix("Cyan"),
    ElemConv::Prefix("Green-cyan"),
    ElemConv::Prefix("Green"),
    ElemConv::Prefix("Yellow-green"),
];

static CS_FOCUS_MODE_LIST: &[ElemConv] = &[
    ElemConv::Map(CS_FOCUS_MODE_1),
    ElemConv::Bits {
        map: &[(0, "(none)")],
        bits: CS_FOCUS_MODE_BITS,
    },
];

static CS_FOCUS_PROCESS_LIST: &[ElemConv] = &[ElemConv::Map(&[(0, "AF Not Used"), (1, "AF Used")])];

static CS_PICTURE_MODE_LIST: &[ElemConv] = &[ElemConv::Map(CS_PICTURE_MODE)];

static CS_GRADATION_CONVS: &[ElemConv] = &[
    ElemConv::StrMap(CS_GRADATION_HEAD),
    ElemConv::Map(&[(0, "User-Selected"), (1, "Auto-Override")]),
];

pub static CAMERA_SETTINGS: &[TagDef] = &[
    TagDef::text(0x0000, "CameraSettingsVersion"),
    TagDef::lookup(0x0100, "PreviewImageValid", NO_YES),
    TagDef::raw(0x0102, "PreviewImageLength"),
    TagDef::lookup(
        0x0200,
        "ExposureMode",
        &[
            (1, "Manual"),
            (2, "Program"),
            (3, "Aperture-priority AE"),
            (4, "Shutter speed priority AE"),
            (5, "Program-shift"),
        ],
    ),
    TagDef::lookup(0x0201, "AELock", OFF_ON),
    TagDef::lookup(0x0202, "MeteringMode", CS_METERING),
    TagDef::raw(0x0203, "ExposureShift"),
    TagDef::lookup(0x0204, "NDFilter", OFF_ON),
    TagDef::lookup(
        0x0300,
        "MacroMode",
        &[(0, "Off"), (1, "On"), (2, "Super Macro")],
    ),
    TagDef {
        id: 0x0301,
        name: "FocusMode",
        force_type: None,
        conv: Conv::List(CS_FOCUS_MODE_LIST),
    },
    TagDef {
        id: 0x0302,
        name: "FocusProcess",
        force_type: None,
        conv: Conv::List(CS_FOCUS_PROCESS_LIST),
    },
    TagDef::lookup(0x0303, "AFSearch", &[(0, "Not Ready"), (1, "Ready")]),
    TagDef::func(0x0304, "AFAreas", print_af_areas),
    TagDef::func(0x0305, "AFPointSelected", print_af_point_selected),
    TagDef::lookup(0x0306, "AFFineTune", OFF_ON),
    TagDef::raw(0x0307, "AFFineTuneAdj"),
    TagDef::raw(0x0308, "FocusBracketStepSize"),
    TagDef::lookup(
        0x0309,
        "AISubjectTrackingMode",
        &[
            (0, "Off"),
            (256, "Motorsports; Object Not Found"),
            (257, "Motorsports; Racing Car Found"),
            (258, "Motorsports; Car Found"),
            (259, "Motorsports; Motorcyle Found"),
            (512, "Airplanes; Object Not Found"),
            (513, "Airplanes; Passenger/Transport Plane Found"),
            (514, "Airplanes; Small Plane/Fighter Jet Found"),
            (515, "Airplanes; Helicopter Found"),
            (768, "Trains; Object Not Found"),
            (769, "Trains; Object Found"),
            (1024, "Birds; Object Not Found"),
            (1025, "Birds; Object Found"),
            (1280, "Dogs & Cats; Object Not Found"),
            (1281, "Dogs & Cats; Object Found"),
            (1536, "Human; Object Not Found"),
            (1537, "Human; Object Found"),
        ],
    ),
    TagDef {
        id: 0x0400,
        name: "FlashMode",
        force_type: None,
        conv: Conv::Bitmask {
            map: &[(0, "Off")],
            bits: CS_FLASH_MODE_BITS,
        },
    },
    TagDef::raw(0x0401, "FlashExposureComp"),
    TagDef::lookup(
        0x0403,
        "FlashRemoteControl",
        &[
            (0, "Off"),
            (1, "Channel 1, Low"),
            (2, "Channel 2, Low"),
            (3, "Channel 3, Low"),
            (4, "Channel 4, Low"),
            (9, "Channel 1, Mid"),
            (10, "Channel 2, Mid"),
            (11, "Channel 3, Mid"),
            (12, "Channel 4, Mid"),
            (17, "Channel 1, High"),
            (18, "Channel 2, High"),
            (19, "Channel 3, High"),
            (20, "Channel 4, High"),
        ],
    ),
    TagDef {
        id: 0x0404,
        name: "FlashControlMode",
        force_type: None,
        conv: Conv::List(&[ElemConv::Map(&[
            (0, "Off"),
            (1, "TTL"),
            (2, "Auto"),
            (3, "Manual"),
        ])]),
    },
    TagDef::func(0x0405, "FlashIntensity", print_flash_strength),
    TagDef::func(0x0406, "ManualFlashStrength", print_flash_strength),
    TagDef::lookup(0x0500, "WhiteBalance2", CS_WHITE_BALANCE2),
    TagDef::func(0x0501, "WhiteBalanceTemperature", |v| {
        let n = v.first_int()?;
        Some(if n == 0 {
            "Auto".to_string()
        } else {
            n.to_string()
        })
    }),
    TagDef::raw(0x0502, "WhiteBalanceBracket"),
    TagDef::func(0x0503, "CustomSaturation", print_min_max),
    TagDef::lookup(
        0x0504,
        "ModifiedSaturation",
        &[
            (0, "Off"),
            (1, "CM1 (Red Enhance)"),
            (2, "CM2 (Green Enhance)"),
            (3, "CM3 (Blue Enhance)"),
            (4, "CM4 (Skin Tones)"),
        ],
    ),
    TagDef::func(0x0505, "ContrastSetting", print_min_max),
    TagDef::func(0x0506, "SharpnessSetting", print_min_max),
    TagDef::lookup(0x0507, "ColorSpace", COLOR_SPACE),
    TagDef::lookup(0x0509, "SceneMode", CS_SCENE_MODE),
    TagDef {
        id: 0x050A,
        name: "NoiseReduction",
        force_type: None,
        conv: Conv::Bitmask {
            map: &[(0, "(none)")],
            bits: CS_NOISE_REDUCTION_BITS,
        },
    },
    TagDef::lookup(0x050B, "DistortionCorrection", OFF_ON),
    TagDef::lookup(0x050C, "ShadingCompensation", OFF_ON),
    TagDef::raw(0x050D, "CompressionFactor"),
    TagDef {
        id: 0x050F,
        name: "Gradation",
        force_type: None,
        conv: Conv::Relist {
            group: 3,
            convs: CS_GRADATION_CONVS,
        },
    },
    TagDef {
        id: 0x0520,
        name: "PictureMode",
        force_type: None,
        conv: Conv::List(CS_PICTURE_MODE_LIST),
    },
    TagDef::func(0x0521, "PictureModeSaturation", print_min_max),
    TagDef::func(0x0523, "PictureModeContrast", print_min_max),
    TagDef::func(0x0524, "PictureModeSharpness", print_min_max),
    TagDef::lookup(0x0525, "PictureModeBWFilter", CS_BW_FILTER),
    TagDef::lookup(0x0526, "PictureModeTone", CS_PICTURE_TONE),
    TagDef::list_lookup(0x0527, "NoiseFilter", CS_NOISE_FILTER),
    TagDef {
        id: 0x0529,
        name: "ArtFilter",
        force_type: None,
        conv: Conv::List(CS_ART_FILTER_LIST),
    },
    TagDef {
        id: 0x052C,
        name: "MagicFilter",
        force_type: None,
        conv: Conv::List(CS_ART_FILTER_LIST),
    },
    TagDef::list_lookup(0x052D, "PictureModeEffect", CS_PICTURE_MODE_EFFECT),
    TagDef {
        id: 0x052E,
        name: "ToneLevel",
        force_type: None,
        conv: Conv::List(CS_TONE_LEVEL_LIST),
    },
    TagDef {
        id: 0x052F,
        name: "ArtFilterEffect",
        force_type: None,
        conv: Conv::List(CS_ART_FILTER_EFFECT_LIST),
    },
    TagDef {
        id: 0x0532,
        name: "ColorCreatorEffect",
        force_type: None,
        conv: Conv::List(CS_COLOR_CREATOR_LIST),
    },
    TagDef {
        id: 0x0537,
        name: "MonochromeProfileSettings",
        force_type: None,
        conv: Conv::List(CS_MONO_PROFILE_LIST),
    },
    TagDef::lookup(
        0x0538,
        "FilmGrainEffect",
        &[(0, "Off"), (1, "Low"), (2, "Medium"), (3, "High")],
    ),
    TagDef {
        id: 0x0539,
        name: "ColorProfileSettings",
        force_type: None,
        conv: Conv::List(CS_COLOR_PROFILE_LIST),
    },
    TagDef::raw(0x053A, "MonochromeVignetting"),
    TagDef::lookup(
        0x053B,
        "MonochromeColor",
        &[
            (0, "(none)"),
            (1, "Normal"),
            (2, "Sepia"),
            (3, "Blue"),
            (4, "Purple"),
            (5, "Green"),
        ],
    ),
    TagDef::func(0x0600, "DriveMode", print_drive_mode),
    TagDef::func(0x0601, "PanoramaMode", print_panorama_mode),
    TagDef::lookup(
        0x0603,
        "ImageQuality2",
        &[(1, "SQ"), (2, "HQ"), (3, "SHQ"), (4, "RAW"), (5, "SQ (5)")],
    ),
    TagDef::lookup(
        0x0604,
        "ImageStabilization",
        &[
            (0, "Off"),
            (1, "On, S-IS1 (All Direction Shake IS)"),
            (2, "On, S-IS2 (Vertical Shake IS)"),
            (3, "On, S-IS3 (Horizontal Shake IS)"),
            (4, "On, S-IS Auto"),
        ],
    ),
    TagDef::raw(0x0821, "ISOAutoSettings"),
    TagDef::func(0x0900, "ManometerPressure", print_manometer_pressure),
    TagDef::func(0x0901, "ManometerReading", print_manometer_reading),
    TagDef::lookup(0x0902, "ExtendedWBDetect", OFF_ON),
    TagDef::func(0x0908, "DateTimeUTC", print_datetime),
];

// ===========================================================================
// Olympus::RawDevelopment (0x2030)
// ===========================================================================

static RD_NOISE_REDUCTION_BITS: &[(u32, &str)] = &[
    (0, "Noise Reduction"),
    (1, "Noise Filter"),
    (2, "Noise Filter (ISO Boost)"),
];

pub static RAW_DEVELOPMENT: &[TagDef] = &[
    TagDef::text(0x0000, "RawDevVersion"),
    TagDef::raw(0x0100, "RawDevExposureBiasValue"),
    TagDef::raw(0x0101, "RawDevWhiteBalanceValue"),
    TagDef::raw(0x0102, "RawDevWBFineAdjustment"),
    TagDef::raw(0x0103, "RawDevGrayPoint"),
    TagDef::raw(0x0104, "RawDevSaturationEmphasis"),
    TagDef::raw(0x0105, "RawDevMemoryColorEmphasis"),
    TagDef::raw(0x0106, "RawDevContrastValue"),
    TagDef::raw(0x0107, "RawDevSharpnessValue"),
    TagDef::lookup(0x0108, "RawDevColorSpace", COLOR_SPACE),
    TagDef::lookup(
        0x0109,
        "RawDevEngine",
        &[
            (0, "High Speed"),
            (1, "High Function"),
            (2, "Advanced High Speed"),
            (3, "Advanced High Function"),
        ],
    ),
    TagDef {
        id: 0x010A,
        name: "RawDevNoiseReduction",
        force_type: None,
        conv: Conv::Bitmask {
            map: &[(0, "(none)")],
            bits: RD_NOISE_REDUCTION_BITS,
        },
    },
    TagDef::lookup(
        0x010B,
        "RawDevEditStatus",
        &[
            (0, "Original"),
            (1, "Edited (Landscape)"),
            (6, "Edited (Portrait)"),
            (8, "Edited (Portrait)"),
        ],
    ),
    TagDef {
        id: 0x010C,
        name: "RawDevSettings",
        force_type: None,
        conv: Conv::Bitmask {
            map: &[(0, "(none)")],
            bits: &[
                (0, "WB Color Temp"),
                (1, "WB Gray Point"),
                (2, "Saturation"),
                (3, "Contrast"),
                (4, "Sharpness"),
                (5, "Color Space"),
                (6, "High Function"),
                (7, "Noise Reduction"),
            ],
        },
    },
];

// ===========================================================================
// Olympus::RawDevelopment2 (0x2031)
// ===========================================================================

pub static RAW_DEVELOPMENT2: &[TagDef] = &[
    TagDef::text(0x0000, "RawDevVersion"),
    TagDef::raw(0x0100, "RawDevExposureBiasValue"),
    TagDef::lookup(
        0x0101,
        "RawDevWhiteBalance",
        &[(1, "Color Temperature"), (2, "Gray Point")],
    ),
    TagDef::raw(0x0102, "RawDevWhiteBalanceValue"),
    TagDef::raw(0x0103, "RawDevWBFineAdjustment"),
    TagDef::raw(0x0104, "RawDevGrayPoint"),
    TagDef::raw(0x0105, "RawDevContrastValue"),
    TagDef::raw(0x0106, "RawDevSharpnessValue"),
    TagDef::raw(0x0107, "RawDevSaturationEmphasis"),
    TagDef::raw(0x0108, "RawDevMemoryColorEmphasis"),
    TagDef::lookup(0x0109, "RawDevColorSpace", COLOR_SPACE),
    TagDef {
        id: 0x010A,
        name: "RawDevNoiseReduction",
        force_type: None,
        conv: Conv::Bitmask {
            map: &[(0, "(none)")],
            bits: RD_NOISE_REDUCTION_BITS,
        },
    },
    TagDef::lookup(
        0x010B,
        "RawDevEngine",
        &[(0, "High Speed"), (1, "High Function")],
    ),
    TagDef::lookup(
        0x010C,
        "RawDevPictureMode",
        &[
            (1, "Vivid"),
            (2, "Natural"),
            (3, "Muted"),
            (256, "Monotone"),
            (512, "Sepia"),
        ],
    ),
    TagDef::raw(0x010D, "RawDevPMSaturation"),
    TagDef::raw(0x010E, "RawDevPMContrast"),
    TagDef::raw(0x010F, "RawDevPMSharpness"),
    TagDef::lookup(
        0x0110,
        "RawDevPM_BWFilter",
        &[
            (1, "Neutral"),
            (2, "Yellow"),
            (3, "Orange"),
            (4, "Red"),
            (5, "Green"),
        ],
    ),
    TagDef::lookup(
        0x0111,
        "RawDevPMPictureTone",
        &[
            (1, "Neutral"),
            (2, "Sepia"),
            (3, "Blue"),
            (4, "Purple"),
            (5, "Green"),
        ],
    ),
    TagDef::raw(0x0112, "RawDevGradation"),
    TagDef::raw(0x0113, "RawDevSaturation3"),
    TagDef::lookup(0x0119, "RawDevAutoGradation", OFF_ON),
    TagDef::raw(0x0120, "RawDevPMNoiseFilter"),
    TagDef {
        id: 0x0121,
        name: "RawDevArtFilter",
        force_type: None,
        conv: Conv::List(CS_ART_FILTER_LIST),
    },
];

// ===========================================================================
// Olympus::ImageProcessing (0x2040)
// ===========================================================================

pub static IMAGE_PROCESSING: &[TagDef] = &[
    TagDef::text(0x0000, "ImageProcessingVersion"),
    TagDef::raw(0x0100, "WB_RBLevels"),
    TagDef::raw(0x0102, "WB_RBLevels3000K"),
    TagDef::raw(0x0103, "WB_RBLevels3300K"),
    TagDef::raw(0x0104, "WB_RBLevels3600K"),
    TagDef::raw(0x0105, "WB_RBLevels3900K"),
    TagDef::raw(0x0106, "WB_RBLevels4000K"),
    TagDef::raw(0x0107, "WB_RBLevels4300K"),
    TagDef::raw(0x0108, "WB_RBLevels4500K"),
    TagDef::raw(0x0109, "WB_RBLevels4800K"),
    TagDef::raw(0x010A, "WB_RBLevels5300K"),
    TagDef::raw(0x010B, "WB_RBLevels6000K"),
    TagDef::raw(0x010C, "WB_RBLevels6600K"),
    TagDef::raw(0x010D, "WB_RBLevels7500K"),
    TagDef::raw(0x010E, "WB_RBLevelsCWB1"),
    TagDef::raw(0x010F, "WB_RBLevelsCWB2"),
    TagDef::raw(0x0110, "WB_RBLevelsCWB3"),
    TagDef::raw(0x0111, "WB_RBLevelsCWB4"),
    TagDef::raw(0x0113, "WB_GLevel3000K"),
    TagDef::raw(0x0114, "WB_GLevel3300K"),
    TagDef::raw(0x0115, "WB_GLevel3600K"),
    TagDef::raw(0x0116, "WB_GLevel3900K"),
    TagDef::raw(0x0117, "WB_GLevel4000K"),
    TagDef::raw(0x0118, "WB_GLevel4300K"),
    TagDef::raw(0x0119, "WB_GLevel4500K"),
    TagDef::raw(0x011A, "WB_GLevel4800K"),
    TagDef::raw(0x011B, "WB_GLevel5300K"),
    TagDef::raw(0x011C, "WB_GLevel6000K"),
    TagDef::raw(0x011D, "WB_GLevel6600K"),
    TagDef::raw(0x011E, "WB_GLevel7500K"),
    TagDef::raw(0x011F, "WB_GLevel"),
    TagDef::typed(0x0200, "ColorMatrix", ftype::TIFF_SSHORT),
    TagDef::raw(0x0300, "Enhancer"),
    TagDef::raw(0x0301, "EnhancerValues"),
    TagDef::raw(0x0310, "CoringFilter"),
    TagDef::raw(0x0311, "CoringValues"),
    TagDef::raw(0x0600, "BlackLevel2"),
    TagDef::raw(0x0610, "GainBase"),
    TagDef::raw(0x0611, "ValidBits"),
    TagDef::raw(0x0612, "CropLeft"),
    TagDef::raw(0x0613, "CropTop"),
    TagDef::raw(0x0614, "CropWidth"),
    TagDef::raw(0x0615, "CropHeight"),
    TagDef::raw(0x0805, "SensorCalibration"),
    TagDef {
        id: 0x1010,
        name: "NoiseReduction2",
        force_type: None,
        conv: Conv::Bitmask {
            map: &[(0, "(none)")],
            bits: RD_NOISE_REDUCTION_BITS,
        },
    },
    TagDef::lookup(0x1011, "DistortionCorrection2", OFF_ON),
    TagDef::lookup(0x1012, "ShadingCompensation2", OFF_ON),
    TagDef {
        id: 0x101C,
        name: "MultipleExposureMode",
        force_type: None,
        conv: Conv::List(&[ElemConv::Map(&[
            (0, "Off"),
            (1, "Live Composite"),
            (2, "On (2 frames)"),
            (3, "On (3 frames)"),
        ])]),
    },
    TagDef::list_lookup(
        0x1112,
        "AspectRatio",
        &[
            ("1 1", "4:3"),
            ("1 4", "1:1"),
            ("2 1", "3:2 (RAW)"),
            ("2 2", "3:2"),
            ("3 1", "16:9 (RAW)"),
            ("3 3", "16:9"),
            ("4 1", "1:1 (RAW)"),
            ("4 4", "6:6"),
            ("5 5", "5:4"),
            ("6 6", "7:6"),
            ("7 7", "6:5"),
            ("8 8", "7:5"),
            ("9 1", "3:4 (RAW)"),
            ("9 9", "3:4"),
        ],
    ),
    TagDef::raw(0x1113, "AspectFrame"),
    TagDef::raw(0x1200, "FacesDetected"),
    TagDef::binary(0x1201, "FaceDetectArea"),
    TagDef::raw(0x1202, "MaxFaces"),
    TagDef::raw(0x1203, "FaceDetectFrameSize"),
    TagDef::raw(0x1207, "FaceDetectFrameCrop"),
    TagDef::typed_func(0x1306, "CameraTemperature", ftype::TIFF_SSHORT, |v| {
        match v.first_int() {
            Some(0) | None => None,
            Some(n) => Some(n.to_string()),
        }
    }),
    TagDef::list_lookup(
        0x1900,
        "KeystoneCompensation",
        &[("0 0", "Off"), ("0 1", "On")],
    ),
    TagDef::lookup(
        0x1901,
        "KeystoneDirection",
        &[(0, "Vertical"), (1, "Horizontal")],
    ),
    TagDef::raw(0x1906, "KeystoneValue"),
    TagDef::lookup(
        0x2110,
        "GNDFilterType",
        &[(0, "High"), (1, "Medium"), (2, "Soft")],
    ),
];

// ===========================================================================
// Olympus::FocusInfo (0x2050)
// ===========================================================================

pub static FOCUS_INFO: &[TagDef] = &[
    TagDef::text(0x0000, "FocusInfoVersion"),
    TagDef::raw(0x0210, "SceneDetect"),
    TagDef::raw(0x0300, "ZoomStepCount"),
    TagDef::raw(0x0301, "FocusStepCount"),
    TagDef::raw(0x0303, "FocusStepInfinity"),
    TagDef::raw(0x0304, "FocusStepNear"),
    TagDef::list_lookup(0x1201, "ExternalFlash", &[("0 0", "Off"), ("1 0", "On")]),
    TagDef::lookup(
        0x1204,
        "ExternalFlashBounce",
        &[(0, "Bounce or Off"), (1, "Direct")],
    ),
    TagDef::raw(0x1205, "ExternalFlashZoom"),
    TagDef::list_lookup(
        0x1208,
        "InternalFlash",
        &[("0", "Off"), ("0 0", "Off"), ("1", "On"), ("1 0", "On")],
    ),
    TagDef::func(0x1209, "ManualFlash", print_manual_flash),
    TagDef::lookup(0x120A, "MacroLED", OFF_ON),
];

/// FocusInfo 0x1500 SensorTemperature has two ExifTool variants selected by
/// the model and value count; only the multi-value one is unambiguous from
/// the MakerNote alone.
pub static FOCUS_INFO_SENSOR_TEMP: TagDef =
    TagDef::func(0x1500, "SensorTemperature", print_sensor_temperature_multi);

// ===========================================================================
// Olympus::RawInfo (0x3000)
// ===========================================================================

pub static RAW_INFO: &[TagDef] = &[
    TagDef::text(0x0000, "RawInfoVersion"),
    TagDef::raw(0x0100, "WB_RBLevelsUsed"),
    TagDef::raw(0x0110, "WB_RBLevelsAuto"),
    TagDef::raw(0x0120, "WB_RBLevelsShade"),
    TagDef::raw(0x0121, "WB_RBLevelsCloudy"),
    TagDef::raw(0x0122, "WB_RBLevelsFineWeather"),
    TagDef::raw(0x0123, "WB_RBLevelsTungsten"),
    TagDef::raw(0x0124, "WB_RBLevelsEveningSunlight"),
    TagDef::raw(0x0130, "WB_RBLevelsDaylightFluor"),
    TagDef::raw(0x0131, "WB_RBLevelsNeutralWhiteFluor"),
    TagDef::raw(0x0132, "WB_RBLevelsCoolWhiteFluor"),
    TagDef::raw(0x0133, "WB_RBLevelsWhiteFluorescent"),
    TagDef::typed(0x0200, "ColorMatrix2", ftype::TIFF_SSHORT),
    TagDef::raw(0x0310, "CoringFilter"),
    TagDef::raw(0x0311, "CoringValues"),
    TagDef::raw(0x0600, "BlackLevel2"),
    TagDef::raw(0x0601, "YCbCrCoefficients"),
    TagDef::raw(0x0611, "ValidBits"),
    TagDef::raw(0x0612, "CropLeft"),
    TagDef::raw(0x0613, "CropTop"),
    TagDef::raw(0x0614, "CropWidth"),
    TagDef::raw(0x0615, "CropHeight"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::tiff::makernotes::olympus::ifd::apply_conv;

    fn conv_of(table: &'static [TagDef], id: u16) -> &'static TagDef {
        table.iter().find(|d| d.id == id).expect("tag in table")
    }

    #[test]
    fn focus_mode_uses_exiftool_list_printconv() {
        // E-M1: raw "0 65" -> "Single AF; S-AF, Imager AF"
        let def = conv_of(CAMERA_SETTINGS, 0x0301);
        let v = OlyVal::Int(vec![0, 65]);
        assert_eq!(apply_conv(def, &v).unwrap(), "Single AF; S-AF, Imager AF");
    }

    #[test]
    fn focus_process_prints_extra_elements_raw() {
        let def = conv_of(CAMERA_SETTINGS, 0x0302);
        let v = OlyVal::Int(vec![1, 64]);
        assert_eq!(apply_conv(def, &v).unwrap(), "AF Used; 64");
    }

    #[test]
    fn noise_reduction_bitmask_names_bit_three_auto() {
        let def = conv_of(CAMERA_SETTINGS, 0x050A);
        assert_eq!(apply_conv(def, &OlyVal::Int(vec![8])).unwrap(), "Auto");
        assert_eq!(apply_conv(def, &OlyVal::Int(vec![0])).unwrap(), "(none)");
    }

    #[test]
    fn flash_mode_zero_is_off_not_a_bitmask_miss() {
        let def = conv_of(CAMERA_SETTINGS, 0x0400);
        assert_eq!(apply_conv(def, &OlyVal::Int(vec![0])).unwrap(), "Off");
        assert_eq!(apply_conv(def, &OlyVal::Int(vec![2])).unwrap(), "Fill-in");
    }

    #[test]
    fn gradation_relists_the_first_three_values() {
        let def = conv_of(CAMERA_SETTINGS, 0x050F);
        let v = OlyVal::Int(vec![0, -1, 1, 0]);
        assert_eq!(apply_conv(def, &v).unwrap(), "Normal; User-Selected");
    }

    #[test]
    fn tone_level_names_every_fourth_element() {
        let def = conv_of(CAMERA_SETTINGS, 0x052E);
        let v = OlyVal::Int(vec![-31999, 0, -7, 7, -31998, 0, -7, 7, 0, 0, 0, 0]);
        assert_eq!(
            apply_conv(def, &v).unwrap(),
            "Highlights; 0; -7; 7; Shadows; 0; -7; 7; 0; 0; 0; 0"
        );
    }

    #[test]
    fn color_creator_effect_uses_label_templates() {
        let def = conv_of(CAMERA_SETTINGS, 0x0532);
        let v = OlyVal::Int(vec![0, 0, 29, 0, -4, 3]);
        assert_eq!(
            apply_conv(def, &v).unwrap(),
            "Color 0; 0; 29; Strength 0; -4; 3"
        );
    }

    #[test]
    fn camera_type2_maps_the_body_code() {
        let def = conv_of(EQUIPMENT, 0x0100);
        let v = OlyVal::Bytes(b"S0047\0".to_vec());
        assert_eq!(apply_conv(def, &v).unwrap(), "E-M1");
    }

    #[test]
    fn lens_type_keys_on_bytes_0_2_3() {
        let def = conv_of(EQUIPMENT, 0x0201);
        let v = OlyVal::Int(vec![0, 0, 0x15, 0, 0, 0]);
        assert_eq!(
            apply_conv(def, &v).unwrap(),
            "Olympus Zuiko Digital ED 7-14mm F4.0"
        );
    }

    #[test]
    fn body_firmware_version_is_hex_with_an_inserted_point() {
        let def = conv_of(EQUIPMENT, 0x0104);
        assert_eq!(
            apply_conv(def, &OlyVal::Int(vec![0x1005])).unwrap(),
            "1.005"
        );
    }

    #[test]
    fn af_areas_renders_corner_pairs() {
        let def = conv_of(CAMERA_SETTINGS, 0x0304);
        let v = OlyVal::Int(vec![0xC845_E56B, 0, 0]);
        assert_eq!(apply_conv(def, &v).unwrap(), "(200,69)-(229,107)");
        assert_eq!(apply_conv(def, &OlyVal::Int(vec![0, 0])).unwrap(), "none");
    }

    #[test]
    fn min_max_triples_match_exiftool_wording() {
        let def = conv_of(CAMERA_SETTINGS, 0x0505);
        let v = OlyVal::Int(vec![0, -5, 5]);
        assert_eq!(apply_conv(def, &v).unwrap(), "0 (min -5, max 5)");
    }
}
