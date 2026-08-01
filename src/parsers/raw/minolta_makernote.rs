//! Minolta MakerNote decoding for the MRW (Minolta RAW) TTW block.
//!
//! An MRW file is a chain of tagged blocks; the `\0TTW` block holds a complete
//! TIFF/EXIF structure whose ExifIFD carries a Minolta MakerNote (signature
//! `MLT0`). ExifTool decodes that note with `Image::ExifTool::Minolta::Main`,
//! and the bulk of the tags come from the `MinoltaCameraSettings` subdirectory
//! it points at (`Image::ExifTool::Minolta::CameraSettings`).
//!
//! # Why this is not routed through the generic MakerNote dispatcher
//!
//! Minolta stores its MakerNote value offsets relative to the **TIFF base**
//! (the start of the TTW block), not relative to the start of the MakerNote
//! itself. The generic dispatcher receives only the MakerNote byte slice, so
//! every offset it resolves would be short by the note's own position and it
//! would decode unrelated bytes. This module therefore walks the TTW buffer
//! directly, which keeps the base correct by construction.
//!
//! Every table below is transcribed from ExifTool's `Minolta.pm`. Tags whose
//! decoding depends on data this module cannot confirm -- the Sony A100-only
//! subdirectories, and the large `minoltaLensTypes` / `minoltaTeleconverters`
//! tables -- are deliberately left undecoded rather than guessed at.

use crate::core::TagValue;
use crate::parsers::common::print_im::{PRINT_IM_VERSION_TAG, decode_print_im_version};
use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

/// A parsed value plus the tags it produced.
type Tags = HashMap<String, TagValue>;

// ============================================================================
// Byte access helpers
// ============================================================================

/// Bounds-checked reader over a TIFF buffer with a known byte order.
struct Tiff<'a> {
    data: &'a [u8],
    big_endian: bool,
}

impl<'a> Tiff<'a> {
    fn u16(&self, off: usize) -> Option<u16> {
        let b = self.data.get(off..off + 2)?;
        Some(if self.big_endian {
            u16::from_be_bytes([b[0], b[1]])
        } else {
            u16::from_le_bytes([b[0], b[1]])
        })
    }

    fn u32(&self, off: usize) -> Option<u32> {
        let b = self.data.get(off..off + 4)?;
        Some(if self.big_endian {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        })
    }

    fn i32(&self, off: usize) -> Option<i32> {
        self.u32(off).map(|v| v as i32)
    }
}

/// One IFD entry, with its payload already resolved to a slice of the TIFF
/// buffer (inline values are handled by pointing back at the entry itself).
struct Entry {
    tag: u16,
    format: u16,
    count: u32,
    /// Absolute offset of the value within the TIFF buffer.
    value_offset: usize,
    /// Total byte length of the value.
    value_len: usize,
}

/// Byte width of each TIFF field type (index = type code).
fn format_size(format: u16) -> usize {
    match format {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => 1,
    }
}

/// Read the entries of an IFD at `ifd_offset`, resolving each value's location.
fn read_ifd(t: &Tiff<'_>, ifd_offset: usize) -> Vec<Entry> {
    let mut out = Vec::new();
    let Some(count) = t.u16(ifd_offset) else {
        return out;
    };
    // A plausibility guard: a corrupt count would otherwise drive a long loop.
    if count as usize > 512 {
        return out;
    }
    for i in 0..count as usize {
        let e = ifd_offset + 2 + i * 12;
        let (Some(tag), Some(format), Some(n)) = (t.u16(e), t.u16(e + 2), t.u32(e + 4)) else {
            break;
        };
        let value_len = (n as usize).saturating_mul(format_size(format));
        // Values of four bytes or fewer are stored inline in the entry.
        let value_offset = if value_len <= 4 {
            e + 8
        } else {
            match t.u32(e + 8) {
                Some(o) => o as usize,
                None => break,
            }
        };
        if t.data.len() < value_offset.saturating_add(value_len) {
            continue; // value points outside the buffer -- skip this entry
        }
        out.push(Entry {
            tag,
            format,
            count: n,
            value_offset,
            value_len,
        });
    }
    out
}

impl Entry {
    /// Read the entry as a single unsigned 32-bit value (int32u / int16u).
    fn as_u32(&self, t: &Tiff<'_>) -> Option<u32> {
        match self.format {
            3 | 8 => t.u16(self.value_offset).map(u32::from),
            4 | 9 => t.u32(self.value_offset),
            _ => None,
        }
    }

    /// Read the entry as a signed 32-bit value.
    fn as_i32(&self, t: &Tiff<'_>) -> Option<i32> {
        match self.format {
            4 | 9 => t.i32(self.value_offset),
            _ => None,
        }
    }

    /// Read the entry as a rational (num/den).
    fn as_rational(&self, t: &Tiff<'_>) -> Option<f64> {
        if !matches!(self.format, 5 | 10) {
            return None;
        }
        let num = t.i32(self.value_offset)? as f64;
        let den = t.i32(self.value_offset + 4)? as f64;
        if den == 0.0 { None } else { Some(num / den) }
    }

    fn bytes<'b>(&self, t: &Tiff<'b>) -> &'b [u8] {
        &t.data[self.value_offset..self.value_offset + self.value_len]
    }

    fn ascii(&self, t: &Tiff<'_>) -> String {
        String::from_utf8_lossy(self.bytes(t))
            .trim_end_matches('\0')
            .trim()
            .to_string()
    }
}

// ============================================================================
// ExifTool-compatible number and PrintConv formatting
// ============================================================================

/// Render a float the way Perl stringifies one: integral values lose the
/// fractional part, everything else uses the shortest exact representation.
fn num(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// Perl's `sprintf("%.*g")`, optionally with a forced sign (`%+.*g`).
fn fmt_g(v: f64, sig: usize, force_sign: bool) -> String {
    let sign = if force_sign && v >= 0.0 { "+" } else { "" };
    if v == 0.0 {
        return format!("{sign}0");
    }
    let exp = v.abs().log10().floor() as i32;
    let body = if exp < -4 || exp >= sig as i32 {
        let s = format!("{:.*e}", sig.saturating_sub(1), v);
        // Rust renders exponents as `e5`; Perl uses `e+05`.
        match s.split_once('e') {
            Some((mantissa, e)) => {
                let mantissa = trim_zeros(mantissa);
                let (esign, digits) = match e.strip_prefix('-') {
                    Some(d) => ("-", d),
                    None => ("+", e),
                };
                format!("{mantissa}e{esign}{digits:0>2}")
            }
            None => s,
        }
    } else {
        let decimals = (sig as i32 - 1 - exp).max(0) as usize;
        trim_zeros(&format!("{:.*}", decimals, v))
    };
    format!("{sign}{body}")
}

/// Drop trailing zeros (and a dangling decimal point) from a decimal string.
fn trim_zeros(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

/// ExifTool `Image::ExifTool::Exif::PrintExposureTime`.
fn print_exposure_time(secs: f64) -> String {
    if secs < 0.25001 && secs > 0.0 {
        return format!("1/{}", (0.5 + 1.0 / secs).trunc() as i64);
    }
    trim_zeros(&format!("{secs:.1}"))
}

/// ExifTool `Image::ExifTool::Exif::PrintFraction`.
fn print_fraction(val: f64) -> String {
    let v = val * 1.00001; // ExifTool's own round-off guard
    if v == 0.0 {
        return "0".to_string();
    }
    for (mult, suffix) in [(1.0, ""), (2.0, "/2"), (3.0, "/3")] {
        let scaled = v * mult;
        let whole = scaled.trunc();
        if whole / scaled > 0.999 {
            return format!("{:+}{}", whole as i64, suffix);
        }
    }
    fmt_g(v, 3, true)
}

/// ExifTool `Image::ExifTool::Exif::PrintParameter`, wrapped by the
/// `printParameter` PrintConv which maps 0 to "Normal".
fn print_parameter(val: i64) -> String {
    if val == 0 {
        return "Normal".to_string();
    }
    if val > 0 {
        if val > 0xfff0 {
            // A negative value in disguise.
            return format!("{}", val - 0x10000);
        }
        return format!("+{val}");
    }
    format!("{val}")
}

/// Look up `val` in a PrintConv table, falling back to ExifTool's
/// `Unknown (n)` rendering.
fn print_conv(val: u32, table: &[(u32, &str)]) -> String {
    match table.iter().find(|(k, _)| *k == val) {
        Some((_, name)) => (*name).to_string(),
        None => format!("Unknown ({val})"),
    }
}

// ============================================================================
// Minolta.pm lookup tables
// ============================================================================

/// `%minoltaWhiteBalance` from Minolta.pm.
const WHITE_BALANCE: &[(u32, &str)] = &[
    (0, "Auto"),
    (1, "Daylight"),
    (2, "Cloudy"),
    (3, "Tungsten"),
    (5, "Custom"),
    (7, "Fluorescent"),
    (8, "Fluorescent 2"),
    (11, "Custom 2"),
    (12, "Custom 3"),
    // The following come from tests with the A2 (ExifTool ref 2).
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

/// `%minoltaColorMode` from Minolta.pm.
const COLOR_MODE: &[(u32, &str)] = &[
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
    (0x84, "Embed Adobe RGB"),
];

/// `%minoltaSceneMode` from Minolta.pm.
const SCENE_MODE: &[(u32, &str)] = &[
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
];

/// Quality PrintConv shared by Main 0x0102/0x0103 and CameraSettings index 5.
const QUALITY: &[(u32, &str)] = &[
    (0, "Raw"),
    (1, "Super Fine"),
    (2, "Fine"),
    (3, "Standard"),
    (4, "Economy"),
    (5, "Extra fine"),
];

/// Look up `%minoltaColorMode`, the colour mode table ExifTool shares between
/// the Minolta MakerNote and the MRW `RIF` block.
pub fn minolta_color_mode(val: u32) -> String {
    print_conv(val, COLOR_MODE)
}

/// ExifTool `Image::ExifTool::Minolta::ConvertWhiteBalance`.
fn convert_white_balance(val: u32) -> String {
    if let Some((_, name)) = WHITE_BALANCE.iter().find(|(k, _)| *k == val) {
        return (*name).to_string();
    }
    if val & 0xffff0000 != 0 {
        // A2 values can be shifted by +/- 3 settings, each step adding or
        // subtracting 0x10000 (ExifTool ref 2).
        let base = (val & 0xff000000).wrapping_add(0x800000);
        if let Some((_, name)) = WHITE_BALANCE.iter().find(|(k, _)| *k == base) {
            let offset = (val as f64 - base as f64) / 65536.0;
            return format!("{}{}", name, fmt_g(offset, 8, true));
        }
        return format!("Unknown (0x{val:x})");
    }
    format!("Unknown ({val})")
}

// ============================================================================
// CameraSettings subdirectory
// ============================================================================

/// Decode `Image::ExifTool::Minolta::CameraSettings`.
///
/// The table's FORMAT is `int32u` and it is always stored big-endian (the
/// SubDirectory pins `ByteOrder => 'BigEndian'`), so table keys are indices
/// into an array of 32-bit words rather than byte offsets.
///
/// `model` selects between the two documented offsets for the parameter tags:
/// ExifTool subtracts 5 for the DiMAGE A2 and 3 for every other body.
fn parse_camera_settings(data: &[u8], model: &str) -> Tags {
    let mut tags = Tags::new();
    let word = |i: usize| -> Option<u32> {
        let b = data.get(i * 4..i * 4 + 4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let mut put = |name: &str, value: String| {
        tags.insert(format!("MakerNotes:{name}"), TagValue::new_string(value));
    };

    // ValueConv offset for Saturation/Contrast/ColorFilter.
    let param_offset: i64 = if model.contains("DiMAGE A2") { 5 } else { 3 };

    if let Some(v) = word(1) {
        put(
            "ExposureMode",
            print_conv(
                v,
                &[
                    (0, "Program"),
                    (1, "Aperture Priority"),
                    (2, "Shutter Priority"),
                    (3, "Manual"),
                ],
            ),
        );
    }
    if let Some(v) = word(2) {
        put(
            "FlashMode",
            print_conv(
                v,
                &[
                    (0, "Fill flash"),
                    (1, "Red-eye reduction"),
                    (2, "Rear flash sync"),
                    (3, "Wireless"),
                    (4, "Off?"),
                ],
            ),
        );
    }
    if let Some(v) = word(3) {
        put("WhiteBalance", convert_white_balance(v));
    }
    if let Some(v) = word(4) {
        put(
            "MinoltaImageSize",
            print_conv(
                v,
                &[
                    (0, "Full"),
                    (1, "1600x1200"),
                    (2, "1280x960"),
                    (3, "640x480"),
                    (6, "2080x1560"),
                    (7, "2560x1920"),
                    (8, "3264x2176"),
                ],
            ),
        );
    }
    if let Some(v) = word(5) {
        // CameraSettings spells the last entry "Extra Fine"; Main 0x0102 uses
        // "Extra fine". Both are transcribed as written in Minolta.pm.
        let quality = match v {
            5 => "Extra Fine".to_string(),
            other => print_conv(other, QUALITY),
        };
        put("MinoltaQuality", quality);
    }
    if let Some(v) = word(6) {
        put(
            "DriveMode",
            print_conv(
                v,
                &[
                    (0, "Single"),
                    (1, "Continuous"),
                    (2, "Self-timer"),
                    (4, "Bracketing"),
                    (5, "Interval"),
                    (6, "UHS continuous"),
                    (7, "HS continuous"),
                ],
            ),
        );
    }
    if let Some(v) = word(7) {
        put(
            "MeteringMode",
            print_conv(
                v,
                &[
                    (0, "Multi-segment"),
                    (1, "Center-weighted average"),
                    (2, "Spot"),
                ],
            ),
        );
    }
    if let Some(v) = word(8) {
        // ValueConv 2 ** (($val-48)/8) * 100, PrintConv int($val + 0.5)
        let iso = 2f64.powf((f64::from(v) - 48.0) / 8.0) * 100.0;
        put("ISO", format!("{}", (iso + 0.5).trunc() as i64));
    }
    if let Some(v) = word(9) {
        // ValueConv 2 ** ((48-$val)/8)
        let secs = 2f64.powf((48.0 - f64::from(v)) / 8.0);
        put("ExposureTime", print_exposure_time(secs));
    }
    if let Some(v) = word(10) {
        // ValueConv 2 ** (($val-8)/16)
        let f = 2f64.powf((f64::from(v) - 8.0) / 16.0);
        put("FNumber", format!("{f:.1}"));
    }
    if let Some(v) = word(11) {
        put("MacroMode", print_conv(v, &[(0, "Off"), (1, "On")]));
    }
    if let Some(v) = word(12) {
        put(
            "DigitalZoom",
            print_conv(v, &[(0, "Off"), (1, "Electronic magnification"), (2, "2x")]),
        );
    }
    if let Some(v) = word(13) {
        // ValueConv $val/3 - 2
        put(
            "ExposureCompensation",
            print_fraction(f64::from(v) / 3.0 - 2.0),
        );
    }
    if let Some(v) = word(14) {
        put(
            "BracketStep",
            print_conv(v, &[(0, "1/3 EV"), (1, "2/3 EV"), (2, "1 EV")]),
        );
    }
    if let Some(v) = word(16) {
        put("IntervalLength", format!("{v}"));
    }
    if let Some(v) = word(17) {
        put("IntervalNumber", format!("{v}"));
    }
    if let Some(v) = word(18) {
        put("FocalLength", format!("{:.1} mm", f64::from(v) / 256.0));
    }
    if let Some(v) = word(19) {
        // ValueConv $val / 1000, PrintConv '$val ? "$val m" : "inf"'
        let d = f64::from(v) / 1000.0;
        put(
            "FocusDistance",
            if d == 0.0 {
                "inf".to_string()
            } else {
                format!("{} m", num(d))
            },
        );
    }
    if let Some(v) = word(20) {
        put("FlashFired", print_conv(v, &[(0, "No"), (1, "Yes")]));
    }
    if let Some(v) = word(21) {
        put(
            "MinoltaDate",
            format!("{:4}:{:02}:{:02}", v >> 16, (v & 0xff00) >> 8, v & 0xff),
        );
    }
    if let Some(v) = word(22) {
        put(
            "MinoltaTime",
            format!("{:02}:{:02}:{:02}", v >> 16, (v & 0xff00) >> 8, v & 0xff),
        );
    }
    if let Some(v) = word(23) {
        let f = 2f64.powf((f64::from(v) - 8.0) / 16.0);
        put("MaxAperture", format!("{f:.1}"));
    }
    if let Some(v) = word(26) {
        put("FileNumberMemory", print_conv(v, &[(0, "Off"), (1, "On")]));
    }
    if let Some(v) = word(27) {
        put("LastFileNumber", format!("{v}"));
    }
    for (idx, name) in [
        (28usize, "ColorBalanceRed"),
        (29, "ColorBalanceGreen"),
        (30, "ColorBalanceBlue"),
    ] {
        if let Some(v) = word(idx) {
            put(name, num(f64::from(v) / 256.0));
        }
    }
    for (idx, name) in [(31usize, "Saturation"), (32, "Contrast")] {
        if let Some(v) = word(idx) {
            put(name, print_parameter(i64::from(v) - param_offset));
        }
    }
    if let Some(v) = word(33) {
        put(
            "Sharpness",
            print_conv(v, &[(0, "Hard"), (1, "Normal"), (2, "Soft")]),
        );
    }
    if let Some(v) = word(34) {
        put(
            "SubjectProgram",
            print_conv(
                v,
                &[
                    (0, "None"),
                    (1, "Portrait"),
                    (2, "Text"),
                    (3, "Night portrait"),
                    (4, "Sunset"),
                    (5, "Sports action"),
                ],
            ),
        );
    }
    if let Some(v) = word(35) {
        // ValueConv ($val - 6) / 3
        put(
            "FlashExposureComp",
            print_fraction((f64::from(v) - 6.0) / 3.0),
        );
    }
    if let Some(v) = word(36) {
        put(
            "ISOSetting",
            print_conv(
                v,
                &[
                    (0, "100"),
                    (1, "200"),
                    (2, "400"),
                    (3, "800"),
                    (4, "Auto"),
                    (5, "64"),
                ],
            ),
        );
    }
    if let Some(v) = word(37) {
        put(
            "MinoltaModelID",
            print_conv(
                v,
                &[
                    (0, "DiMAGE 7, X1, X21 or X31"),
                    (1, "DiMAGE 5"),
                    (2, "DiMAGE S304"),
                    (3, "DiMAGE S404"),
                    (4, "DiMAGE 7i"),
                    (5, "DiMAGE 7Hi"),
                    (6, "DiMAGE A1"),
                    (7, "DiMAGE A2 or S414"),
                ],
            ),
        );
    }
    if let Some(v) = word(38) {
        put(
            "IntervalMode",
            print_conv(v, &[(0, "Still Image"), (1, "Time-lapse Movie")]),
        );
    }
    if let Some(v) = word(39) {
        put(
            "FolderName",
            print_conv(v, &[(0, "Standard Form"), (1, "Data Form")]),
        );
    }
    if let Some(v) = word(40) {
        put(
            "ColorMode",
            print_conv(
                v,
                &[
                    (0, "Natural color"),
                    (1, "Black & White"),
                    (2, "Vivid color"),
                    (3, "Solarization"),
                    (4, "Adobe RGB"),
                ],
            ),
        );
    }
    if let Some(v) = word(41) {
        put("ColorFilter", format!("{}", i64::from(v) - param_offset));
    }
    if let Some(v) = word(42) {
        put("BWFilter", format!("{v}"));
    }
    if let Some(v) = word(43) {
        put("InternalFlash", print_conv(v, &[(0, "No"), (1, "Fired")]));
    }
    if let Some(v) = word(44) {
        // ValueConv $val/8 - 6
        put("Brightness", num(f64::from(v) / 8.0 - 6.0));
    }
    if let Some(v) = word(45) {
        put("SpotFocusPointX", format!("{v}"));
    }
    if let Some(v) = word(46) {
        put("SpotFocusPointY", format!("{v}"));
    }
    if let Some(v) = word(47) {
        put(
            "WideFocusZone",
            print_conv(
                v,
                &[
                    (0, "No zone"),
                    (1, "Center zone (horizontal orientation)"),
                    (2, "Center zone (vertical orientation)"),
                    (3, "Left zone"),
                    (4, "Right zone"),
                ],
            ),
        );
    }
    if let Some(v) = word(48) {
        put("FocusMode", print_conv(v, &[(0, "AF"), (1, "MF")]));
    }
    if let Some(v) = word(49) {
        put(
            "FocusArea",
            print_conv(v, &[(0, "Wide Focus (normal)"), (1, "Spot Focus")]),
        );
    }
    if let Some(v) = word(50) {
        put(
            "DECPosition",
            print_conv(
                v,
                &[
                    (0, "Exposure"),
                    (1, "Contrast"),
                    (2, "Saturation"),
                    (3, "Filter"),
                ],
            ),
        );
    }
    // Indices 51 (ColorProfile) and 52 (DataImprint) are DiMAGE 7Hi only.
    if model == "DiMAGE 7Hi" {
        if let Some(v) = word(51) {
            put(
                "ColorProfile",
                print_conv(v, &[(0, "Not Embedded"), (1, "Embedded")]),
            );
        }
        if let Some(v) = word(52) {
            put(
                "DataImprint",
                print_conv(
                    v,
                    &[
                        (0, "None"),
                        (1, "YYYY/MM/DD"),
                        (2, "MM/DD/HH:MM"),
                        (3, "Text"),
                        (4, "Text + ID#"),
                    ],
                ),
            );
        }
    }
    if let Some(v) = word(63) {
        put(
            "FlashMetering",
            print_conv(
                v,
                &[
                    (0, "ADI (Advanced Distance Integration)"),
                    (1, "Pre-flash TTL"),
                    (2, "Manual flash control"),
                ],
            ),
        );
    }

    tags
}

// ============================================================================
// Minolta::Main
// ============================================================================

/// Decode the Minolta MakerNote IFD at `mn_offset` within the TTW buffer.
fn parse_main(t: &Tiff<'_>, mn_offset: usize, make: &str, model: &str) -> Tags {
    let mut tags = Tags::new();
    let mut preview: (Option<usize>, Option<usize>) = (None, None);

    for e in read_ifd(t, mn_offset) {
        match e.tag {
            // 0x0000 MakerNoteVersion -- four undefined bytes, e.g. "MLT0".
            0x0000 => {
                let v = e.ascii(t);
                if !v.is_empty() {
                    tags.insert(
                        "MakerNotes:MakerNoteVersion".to_string(),
                        TagValue::new_string(v),
                    );
                }
            }
            // 0x0001 MinoltaCameraSettingsOld / 0x0003 MinoltaCameraSettings.
            // 0x0003 does not apply to the DiMAGE X31.
            0x0001 | 0x0003 => {
                if e.tag == 0x0003 && model == "DiMAGE X31" {
                    continue;
                }
                for (k, v) in parse_camera_settings(e.bytes(t), model) {
                    tags.insert(k, v);
                }
            }
            // 0x0018 is an 8 kB block whose mere presence means stabilisation
            // was enabled, but only on the bodies ExifTool lists. On the A100
            // the same tag is ISInfoA100, which is not decoded here.
            0x0018 => {
                if matches!(model, "DiMAGE A1" | "DiMAGE A2" | "DiMAGE X1") {
                    tags.insert(
                        "MakerNotes:ImageStabilization".to_string(),
                        TagValue::new_string("On"),
                    );
                }
            }
            0x0040 => {
                if let Some(v) = e.as_u32(t) {
                    tags.insert(
                        "MakerNotes:CompressedImageSize".to_string(),
                        TagValue::Integer(i64::from(v)),
                    );
                }
            }
            // 0x0081 is an inline JPEG preview (DiMAGE 7).
            0x0081 => {
                tags.insert(
                    "MakerNotes:PreviewImage".to_string(),
                    TagValue::Binary(e.bytes(t).to_vec()),
                );
            }
            0x0088 => {
                if let Some(v) = e.as_u32(t) {
                    preview.0 = Some(v as usize);
                    tags.insert(
                        "MakerNotes:PreviewImageStart".to_string(),
                        TagValue::Integer(i64::from(v)),
                    );
                }
            }
            0x0089 => {
                if let Some(v) = e.as_u32(t) {
                    preview.1 = Some(v as usize);
                    tags.insert(
                        "MakerNotes:PreviewImageLength".to_string(),
                        TagValue::Integer(i64::from(v)),
                    );
                }
            }
            0x0100 => {
                if let Some(v) = e.as_u32(t) {
                    tags.insert(
                        "MakerNotes:SceneMode".to_string(),
                        TagValue::new_string(print_conv(v, SCENE_MODE)),
                    );
                }
            }
            // 0x0101 ColorMode -- the Sony variant uses a different table, so
            // only the Minolta reading is emitted here.
            0x0101 => {
                if !make.starts_with("SONY")
                    && let Some(v) = e.as_u32(t)
                {
                    tags.insert(
                        "MakerNotes:ColorMode".to_string(),
                        TagValue::new_string(print_conv(v, COLOR_MODE)),
                    );
                }
            }
            0x0102 => {
                if let Some(v) = e.as_u32(t) {
                    tags.insert(
                        "MakerNotes:MinoltaQuality".to_string(),
                        TagValue::new_string(print_conv(v, QUALITY)),
                    );
                }
            }
            // 0x0103 is quality on the A2/7Hi and image size elsewhere
            // (except the A200, where ExifTool decodes neither).
            0x0103 => {
                if let Some(v) = e.as_u32(t) {
                    if matches!(model, "DiMAGE A2" | "DiMAGE 7Hi") {
                        tags.insert(
                            "MakerNotes:MinoltaQuality".to_string(),
                            TagValue::new_string(print_conv(v, QUALITY)),
                        );
                    } else if model != "DiMAGE A200" {
                        tags.insert(
                            "MakerNotes:MinoltaImageSize".to_string(),
                            TagValue::new_string(print_conv(
                                v,
                                &[
                                    (1, "1600x1200"),
                                    (2, "1280x960"),
                                    (3, "640x480"),
                                    (5, "2560x1920"),
                                    (6, "2272x1704"),
                                    (7, "2048x1536"),
                                ],
                            )),
                        );
                    }
                }
            }
            0x0104 => {
                if let Some(v) = e.as_rational(t) {
                    tags.insert(
                        "MakerNotes:FlashExposureComp".to_string(),
                        TagValue::new_string(num(v)),
                    );
                }
            }
            0x0107 => {
                if let Some(v) = e.as_u32(t) {
                    tags.insert(
                        "MakerNotes:ImageStabilization".to_string(),
                        TagValue::new_string(print_conv(v, &[(1, "Off"), (5, "On")])),
                    );
                }
            }
            0x0109 => {
                if let Some(v) = e.as_u32(t) {
                    tags.insert(
                        "MakerNotes:RawAndJpgRecording".to_string(),
                        TagValue::new_string(print_conv(v, &[(0, "Off"), (1, "On")])),
                    );
                }
            }
            0x010a => {
                if let Some(v) = e.as_u32(t) {
                    tags.insert(
                        "MakerNotes:ZoneMatching".to_string(),
                        TagValue::new_string(print_conv(
                            v,
                            &[(0, "ISO Setting Used"), (1, "High Key"), (2, "Low Key")],
                        )),
                    );
                }
            }
            0x010b => {
                if let Some(v) = e.as_u32(t) {
                    tags.insert(
                        "MakerNotes:ColorTemperature".to_string(),
                        TagValue::Integer(i64::from(v)),
                    );
                }
            }
            0x0111 => {
                if let Some(v) = e.as_i32(t) {
                    tags.insert(
                        "MakerNotes:ColorCompensationFilter".to_string(),
                        TagValue::Integer(i64::from(v)),
                    );
                }
            }
            0x0112 => {
                if let Some(v) = e.as_i32(t) {
                    tags.insert(
                        "MakerNotes:WhiteBalanceFineTune".to_string(),
                        TagValue::Integer(i64::from(v)),
                    );
                }
            }
            0x0115 => {
                if let Some(v) = e.as_u32(t) {
                    tags.insert(
                        "MakerNotes:WhiteBalance".to_string(),
                        TagValue::new_string(print_conv(
                            v,
                            &[
                                (0x00, "Auto"),
                                (0x01, "Color Temperature/Color Filter"),
                                (0x10, "Daylight"),
                                (0x20, "Cloudy"),
                                (0x30, "Shade"),
                                (0x40, "Tungsten"),
                                (0x50, "Flash"),
                                (0x60, "Fluorescent"),
                                (0x70, "Custom"),
                            ],
                        )),
                    );
                }
            }
            // 0x0e00 PrintIM.
            0x0e00 => {
                let order = if t.big_endian {
                    ByteOrder::BigEndian
                } else {
                    ByteOrder::LittleEndian
                };
                if let Some(version) = decode_print_im_version(e.bytes(t), order) {
                    tags.insert(
                        PRINT_IM_VERSION_TAG.to_string(),
                        TagValue::new_string(version),
                    );
                }
            }
            _ => {}
        }
    }

    // ExifTool synthesises PreviewImage from the offset/length pair. Both are
    // relative to the TIFF base, which is this buffer.
    if !tags.contains_key("MakerNotes:PreviewImage")
        && let (Some(start), Some(len)) = preview
        && len > 0
        && let Some(bytes) = t.data.get(start..start.saturating_add(len))
    {
        tags.insert(
            "MakerNotes:PreviewImage".to_string(),
            TagValue::Binary(bytes.to_vec()),
        );
    }

    tags
}

// ============================================================================
// Entry point
// ============================================================================

/// Decode the Minolta MakerNote and PrintIM directory carried by an MRW TTW
/// block.
///
/// `ttw` must be the raw TTW block payload, which is a complete little- or
/// big-endian TIFF structure. Returns the tags keyed by family, ready to be
/// merged into the file's metadata map.
pub fn parse_ttw_makernotes(ttw: &[u8]) -> Tags {
    let mut tags = Tags::new();

    let big_endian = match ttw.get(0..2) {
        Some(b"MM") => true,
        Some(b"II") => false,
        _ => return tags,
    };
    let t = Tiff {
        data: ttw,
        big_endian,
    };
    if t.u16(2) != Some(42) {
        return tags;
    }
    let Some(ifd0) = t.u32(4) else {
        return tags;
    };

    // Walk IFD0 for the camera identity, the PrintIM block, and the ExifIFD
    // pointer. Make and Model gate several Condition-guarded MakerNote tags.
    let mut make = String::new();
    let mut model = String::new();
    let mut exif_ifd = None;
    for e in read_ifd(&t, ifd0 as usize) {
        match e.tag {
            0x010f => make = e.ascii(&t),
            0x0110 => model = e.ascii(&t),
            0x8769 => exif_ifd = e.as_u32(&t),
            0xc4a5 => {
                let order = if t.big_endian {
                    ByteOrder::BigEndian
                } else {
                    ByteOrder::LittleEndian
                };
                if let Some(version) = decode_print_im_version(e.bytes(&t), order) {
                    tags.insert(
                        PRINT_IM_VERSION_TAG.to_string(),
                        TagValue::new_string(version),
                    );
                }
            }
            _ => {}
        }
    }

    let Some(exif_ifd) = exif_ifd else {
        return tags;
    };

    // The MakerNote lives in the ExifIFD as tag 0x927C.
    for e in read_ifd(&t, exif_ifd as usize) {
        if e.tag == 0x927c && e.count > 2 {
            for (k, v) in parse_main(&t, e.value_offset, &make, &model) {
                tags.insert(k, v);
            }
            break;
        }
    }

    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_exposure_time_matches_exiftool() {
        // 2 ** ((48-67)/8) = 0.19275 -> "1/5"
        assert_eq!(print_exposure_time(2f64.powf((48.0 - 67.0) / 8.0)), "1/5");
        assert_eq!(print_exposure_time(1.0), "1");
        assert_eq!(print_exposure_time(2.5), "2.5");
    }

    #[test]
    fn print_fraction_matches_exiftool() {
        assert_eq!(print_fraction(0.0), "0");
        assert_eq!(print_fraction(1.0), "+1");
        assert_eq!(print_fraction(-1.0), "-1");
        assert_eq!(print_fraction(0.5), "+1/2");
        assert_eq!(print_fraction(1.0 / 3.0), "+1/3");
    }

    #[test]
    fn print_parameter_maps_zero_to_normal() {
        assert_eq!(print_parameter(0), "Normal");
        assert_eq!(print_parameter(1), "+1");
        assert_eq!(print_parameter(-2), "-2");
        // A negative value in disguise.
        assert_eq!(print_parameter(0xfff1), "-15");
    }

    #[test]
    fn white_balance_handles_a2_shifted_values() {
        assert_eq!(convert_white_balance(0x800000), "Auto");
        assert_eq!(convert_white_balance(1), "Daylight");
        assert_eq!(convert_white_balance(4), "Unknown (4)");
        // 0x1810000 is Daylight shifted by one step.
        assert_eq!(convert_white_balance(0x1810000), "Daylight+1");
    }

    #[test]
    fn camera_settings_decodes_reference_words() {
        // Reference words taken from the DiMAGE A2 sample: index 8 (ISO 44),
        // index 18 (FocalLength 1889) and index 21 (MinoltaDate).
        let mut data = vec![0u8; 4 * 64];
        let set = |d: &mut Vec<u8>, i: usize, v: u32| {
            d[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
        };
        set(&mut data, 8, 44);
        set(&mut data, 18, 1889);
        set(&mut data, 21, 131_336_978);
        set(&mut data, 22, 1_510_970);
        set(&mut data, 31, 5); // Saturation, A2 offset of 5 -> 0 -> "Normal"

        let tags = parse_camera_settings(&data, "DiMAGE A2");
        let get = |k: &str| tags.get(k).and_then(|v| v.as_string()).unwrap_or_default();
        assert_eq!(get("MakerNotes:ISO"), "71");
        assert_eq!(get("MakerNotes:FocalLength"), "7.4 mm");
        assert_eq!(get("MakerNotes:MinoltaDate"), "2004:11:18");
        assert_eq!(get("MakerNotes:MinoltaTime"), "23:14:58");
        assert_eq!(get("MakerNotes:Saturation"), "Normal");
    }

    #[test]
    fn non_tiff_input_is_rejected() {
        assert!(parse_ttw_makernotes(b"not a tiff").is_empty());
        assert!(parse_ttw_makernotes(&[]).is_empty());
    }
}
