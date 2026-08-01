//! Samsung Type2 MakerNote table, ported from `Image::ExifTool::Samsung::Type2`.
//!
//! Type2 is a bare TIFF IFD with no signature -- ExifTool identifies it by the
//! Make plus the `\0\x01\0\x07\0{3}\x04 0100` shape of the first entry -- so it
//! walks with the shared table-IFD engine.

use super::super::shared::table_ifd::{OlyVal, TagDef};
use super::lookups::{DEVICE_TYPE, SAMSUNG_MODEL_ID};

static OFF_ON: &[(i64, &str)] = &[(0, "Off"), (1, "On")];

/// `PictureWizard` (0x0021) is a five-element `int16u` block, not an IFD, and
/// ExifTool's ValueConv subtracts 4 from the last three. Emitting them as one
/// tag would be wrong, so the caller expands them; this holds the names and
/// the mode lookup.
pub static PICTURE_WIZARD_MODE: &[(i64, &str)] = &[
    (0, "Standard"),
    (1, "Vivid"),
    (2, "Portrait"),
    (3, "Landscape"),
    (4, "Forest"),
    (5, "Retro"),
    (6, "Cool"),
    (7, "Calm"),
    (8, "Classic"),
    (9, "Custom1"),
    (10, "Custom2"),
    (11, "Custom3"),
    (255, "n/a"),
];

/// `SmartAlbumColor` (0x0020): ExifTool splits this across two conditional tag
/// definitions -- an all-zero value is `n/a`, anything else names the first of
/// the two `int16u` values as a colour and leaves the second raw.
fn print_smart_album_color(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.len() < 2 {
        return None;
    }
    if v[0] == 0 && v[1] == 0 {
        return Some("n/a".to_string());
    }
    let color = match SMART_ALBUM_COLOR.iter().find(|(k, _)| *k == v[0]) {
        Some((_, s)) => (*s).to_string(),
        None => format!("Unknown ({})", v[0]),
    };
    Some(format!("{}; {}", color, v[1]))
}

static SMART_ALBUM_COLOR: &[(i64, &str)] = &[
    (0, "Red"),
    (1, "Yellow"),
    (2, "Green"),
    (3, "Blue"),
    (4, "Magenta"),
    (5, "Black"),
    (6, "White"),
    (7, "Various"),
];

/// `CameraTemperature` (0x0043): ExifTool appends ` C` only when the value has
/// a digit, so a zero-denominator rational stays the bare word `undef`.
fn print_camera_temperature(val: &OlyVal) -> Option<String> {
    let raw = val.print_raw();
    if raw.chars().any(|c| c.is_ascii_digit()) {
        Some(format!("{} C", raw))
    } else {
        Some(raw)
    }
}

/// `LocalLocationName`/`LocationName` (0x0030/0x0031) hold two place names
/// separated by NUL+space and terminated by a double NUL.
fn print_location_name(val: &OlyVal) -> Option<String> {
    let OlyVal::Bytes(b) = val else { return None };
    let end = b.windows(2).position(|w| w == [0, 0]).unwrap_or(b.len());
    let text = super::super::shared::table_ifd::decode_text(&b[..end]);
    Some(
        text.split('\0')
            .map(|p| p.trim_start_matches(' '))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

pub static TYPE2: &[TagDef] = &[
    TagDef::text(0x0001, "MakerNoteVersion"),
    TagDef::lookup_hex(0x0002, "DeviceType", DEVICE_TYPE),
    TagDef::lookup_hex(0x0003, "SamsungModelID", SAMSUNG_MODEL_ID),
    TagDef::func(0x0020, "SmartAlbumColor", print_smart_album_color),
    TagDef::func(0x0030, "LocalLocationName", print_location_name),
    TagDef::func(0x0031, "LocationName", print_location_name),
    TagDef::lookup(
        0x0040,
        "RawDataByteOrder",
        &[
            (0, "Little-endian (Intel, II)"),
            (1, "Big-endian (Motorola, MM)"),
        ],
    ),
    TagDef::lookup(0x0041, "WhiteBalanceSetup", &[(0, "Auto"), (1, "Manual")]),
    TagDef::func(0x0043, "CameraTemperature", print_camera_temperature),
    TagDef::lookup(
        0x0050,
        "RawDataCFAPattern",
        &[(0, "Unchanged"), (1, "Swap"), (65535, "Roll")],
    ),
    TagDef::lookup(0x0100, "FaceDetect", OFF_ON),
    TagDef::lookup(0x0120, "FaceRecognition", OFF_ON),
    TagDef::text(0x0123, "FaceName"),
    TagDef::text(0xA001, "FirmwareName"),
    TagDef::text(0xA003, "LensType"),
    TagDef::text(0xA004, "LensFirmware"),
    TagDef::text(0xA005, "InternalLensSerialNumber"),
    TagDef::raw(0xA010, "SensorAreas"),
    TagDef::lookup(0xA011, "ColorSpace", &[(0, "sRGB"), (1, "Adobe RGB")]),
    TagDef::lookup(0xA012, "SmartRange", OFF_ON),
    TagDef::raw(0xA013, "ExposureCompensation"),
    TagDef::raw(0xA014, "ISO"),
    TagDef::raw(0xA020, "EncryptionKey"),
    TagDef::raw(0xA021, "WB_RGGBLevelsUncorrected"),
    TagDef::raw(0xA022, "WB_RGGBLevelsAuto"),
    TagDef::raw(0xA023, "WB_RGGBLevelsIlluminator1"),
    TagDef::raw(0xA024, "WB_RGGBLevelsIlluminator2"),
    TagDef::raw(0xA025, "HighlightLinearityLimit"),
    TagDef::raw(0xA028, "WB_RGGBLevelsBlack"),
    TagDef::raw(0xA030, "ColorMatrix"),
    TagDef::raw(0xA031, "ColorMatrixSRGB"),
    TagDef::raw(0xA032, "ColorMatrixAdobeRGB"),
    TagDef::raw(0xA033, "CbCrMatrixDefault"),
    TagDef::raw(0xA034, "CbCrMatrix"),
    TagDef::raw(0xA035, "CbCrGainDefault"),
    TagDef::raw(0xA036, "CbCrGain"),
    TagDef::raw(0xA040, "ToneCurveSRGBDefault"),
    TagDef::raw(0xA041, "ToneCurveAdobeRGBDefault"),
    TagDef::raw(0xA042, "ToneCurveSRGB"),
    TagDef::raw(0xA043, "ToneCurveAdobeRGB"),
    TagDef::raw(0xA048, "RawData"),
    TagDef::raw(0xA050, "Distortion"),
    TagDef::raw(0xA051, "ChromaticAberration"),
    TagDef::raw(0xA052, "Vignetting"),
    TagDef::raw(0xA053, "VignettingCorrection"),
    TagDef::raw(0xA054, "VignettingSetting"),
];
