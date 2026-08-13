//! Leica MakerNote Parser
//!
//! Parses Leica-specific EXIF MakerNote tags containing camera settings,
//! lens information, image quality parameters, and other proprietary metadata.
//!
//! Supports Leica digital cameras including:
//! - M-series digital rangefinders (M8, M9, M10, M11, M Monochrom)
//! - SL-series mirrorless (SL, SL2, SL2-S)
//! - Q-series fixed-lens compacts (Q, Q2, Q2 Monochrom)
//! - CL/TL mirrorless cameras
//!
//! Based on ExifTool's Leica.pm module.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use nom::{
    IResult,
    combinator::map,
    multi::count,
    number::complete::{be_u16, be_u32, le_u16, le_u32},
};
use std::collections::HashMap;

use super::lens_data::leica as leica_lenses;
use super::makernote_context::MakerNoteContext;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::{extract_i16_array, extract_u16_array, extract_u32_array};
use crate::const_decoder;

// ===== Leica MakerNote Tag IDs =====
//
// ExifTool keeps every Leica table in Panasonic.pm, not a Leica.pm -- and
// each `MakerNoteLeicaN` header picks a *different* table with its own,
// non-overlapping tag-id space (`%Panasonic::Leica2`, `::Leica3`, `::Leica4`,
// `::Leica5`, `::Leica6`, `::Leica9`, plus the `::Subdir`/`::Data1` pair
// Leica4 points its subdirectory entries at). A ` tag_id` is only meaningful
// paired with the table it was read against; the same id means different
// things in different tables (e.g. 0x304 is `WhiteBalance` in Leica2 but
// `FocusDistance` in Leica6/Leica9), so each layout below gets its own match
// arm instead of one shared switch.

/// `%Panasonic::Leica2` (Panasonic.pm:1603), used by the M8. Ids below are
/// exactly as declared there -- no offset, no renumbering.
mod leica2 {
    pub(super) const QUALITY: u16 = 0x0300;
    pub(super) const USER_PROFILE: u16 = 0x0302;
    pub(super) const SERIAL_NUMBER: u16 = 0x0303;
    pub(super) const WHITE_BALANCE: u16 = 0x0304;
    /// `PrintConv => \%leicaLensTypes` (Panasonic.pm:1648).
    pub(super) const LENS_TYPE: u16 = 0x0310;
    pub(super) const EXTERNAL_SENSOR_BRIGHTNESS_VALUE: u16 = 0x0311;
    pub(super) const MEASURED_LV: u16 = 0x0312;
    pub(super) const APPROXIMATE_F_NUMBER: u16 = 0x0313;
    pub(super) const CAMERA_TEMPERATURE: u16 = 0x0320;
    pub(super) const COLOR_TEMPERATURE: u16 = 0x0321;
    pub(super) const WB_RED_LEVEL: u16 = 0x0322;
    pub(super) const WB_GREEN_LEVEL: u16 = 0x0323;
    pub(super) const WB_BLUE_LEVEL: u16 = 0x0324;
    pub(super) const UV_IR_FILTER_CORRECTION: u16 = 0x0325;
    pub(super) const CCD_VERSION: u16 = 0x0330;
    pub(super) const CCD_BOARD_VERSION: u16 = 0x0331;
    pub(super) const CONTROLLER_BOARD_VERSION: u16 = 0x0332;
    pub(super) const M16C_VERSION: u16 = 0x0333;
    pub(super) const IMAGE_ID_NUMBER: u16 = 0x0340;
}

/// `%Panasonic::Leica3` (Panasonic.pm:1705), used by the R8/R9 digital
/// backs. Only two tags exist in this table.
mod leica3 {
    /// A `SubDirectory` pointer into `%Panasonic::SerialInfo`
    /// (Panasonic.pm:1723), whose own tag 4 is an 8-byte `SerialNumber`
    /// string at a fixed offset inside the pointed-to block.
    pub(super) const SERIAL_INFO: u16 = 0x000B;
    pub(super) const SERIAL_INFO_NUMBER_OFFSET: u32 = 4;
    pub(super) const SERIAL_INFO_NUMBER_LEN: usize = 8;
    pub(super) const WB_RGB_LEVELS: u16 = 0x000D;
}

/// `%Panasonic::Subdir` (Panasonic.pm:1772), pointed at by the M9/M
/// Monochrom's Leica4 subdirectory entries. Entries carry their full tag ID
/// here, not one relative to the subdirectory.
mod l4_subdir {
    pub(super) const CONTRAST: u16 = 0x300A;
    pub(super) const SHARPENING: u16 = 0x300B;
    pub(super) const SATURATION: u16 = 0x300D;
    pub(super) const WHITE_BALANCE: u16 = 0x3033;
    pub(super) const JPEG_QUALITY: u16 = 0x3034;
    pub(super) const WB_RGB_LEVELS: u16 = 0x3036;
    pub(super) const USER_PROFILE: u16 = 0x3038;
    pub(super) const SERIAL_NUMBER: u16 = 0x3103;
    pub(super) const FIRMWARE_VERSION: u16 = 0x3109;
    pub(super) const BASE_ISO: u16 = 0x312A;
    pub(super) const SENSOR_WIDTH: u16 = 0x312B;
    pub(super) const SENSOR_HEIGHT: u16 = 0x312C;
    pub(super) const SENSOR_BIT_DEPTH: u16 = 0x312D;
    pub(super) const CAMERA_TEMPERATURE: u16 = 0x3402;
    /// Same `int32u` and same `%leicaLensTypes` PrintConv as Leica2 0x0310.
    pub(super) const LENS_TYPE: u16 = 0x3405;
    pub(super) const APPROXIMATE_F_NUMBER: u16 = 0x3406;
    /// `int32s` with `ValueConv => '$val / 1e5'` (Panasonic.pm:1908), unlike
    /// Leica2/Leica9's rational64s of the same name.
    pub(super) const MEASURED_LV: u16 = 0x3407;
    pub(super) const EXTERNAL_SENSOR_BRIGHTNESS: u16 = 0x3408;
}

// Leica 4 (M9) Subdirectory pointer tags, in the top-level Leica4 IFD.
const L4_SUBDIR_3000: u16 = 0x3000;
const L4_SUBDIR_3100: u16 = 0x3100;
const L4_SUBDIR_3400: u16 = 0x3400;
const L4_SUBDIR_3900: u16 = 0x3900;

/// `%Panasonic::Leica5` (Panasonic.pm:1996), used by the X1/X2/X VARIO/T/TL
/// (via `MakerNoteLeica5`) and the Q/SL/CL (via `MakerNoteLeica8`, same
/// table). Only the fields this parser resolves are listed.
mod leica5 {
    /// `Condition => '$format eq "string"'` (Panasonic.pm:2003) -- some
    /// other Leica5 body may store 0x0303 in a different format this table
    /// does not name; skip rather than guess when the field type isn't
    /// ASCII.
    pub(super) const LENS_TYPE: u16 = 0x0303;
    pub(super) const SERIAL_NUMBER: u16 = 0x0305;
    /// `SubDirectory => { TagTable => PanasonicRaw::CameraIFD, Base =>
    /// '$start', ProcessProc => ProcessTIFF }` (Panasonic.pm:2052-2059).
    pub(super) const CAMERA_IFD: u16 = 0x05FF;
}

/// `%Panasonic::Leica6` (Panasonic.pm:2111), used by the S2 and M (Typ 240)
/// via `MakerNoteLeica6`, and the M Monochrom (Typ 246) via `MakerNoteLeica7`
/// (same table, different `Base`). `LensType` (0x303) and `PreviewImage`
/// (0x300) are written as a JPEG trailer on these bodies, so their
/// out-of-line bytes sit outside this parser's payload/TIFF window; only
/// `FocusDistance`, whose 4-byte value fits inline in the entry itself, is
/// resolved here.
mod leica6 {
    pub(super) const FOCUS_DISTANCE: u16 = 0x0304;
}

/// `%Panasonic::Leica9` (Panasonic.pm:2192), used by the M10/M11 and the S
/// (Typ 007).
mod leica9 {
    pub(super) const FOCUS_DISTANCE: u16 = 0x0304;
    pub(super) const EXTERNAL_SENSOR_BRIGHTNESS_VALUE: u16 = 0x0311;
    pub(super) const MEASURED_LV: u16 = 0x0312;
    pub(super) const USER_PROFILE: u16 = 0x034C;
    pub(super) const ISO_SELECTED: u16 = 0x0359;
    /// `ValueConv => '$val / 1000'` (Panasonic.pm:2228).
    pub(super) const F_NUMBER: u16 = 0x035A;
    pub(super) const CORRELATED_COLOR_TEMP: u16 = 0x035B;
    pub(super) const COLOR_TINT: u16 = 0x035C;
    pub(super) const WHITE_POINT: u16 = 0x035D;
    pub(super) const LENS_PROFILE_NAME: u16 = 0x0370;
}

// `%Panasonic::Subdir` decoders (Panasonic.pm:1772-1937), the M9/M
// Monochrom's Leica4 subdirectory table. The previous value tables here did
// not match Subdir's real `PrintConv` hashes at all (wrong order, wrong raw
// values) -- verified against `LeicaM9.jpg`'s ground truth.
const_decoder!(
    L4_DECODE_CONTRAST,
    i32,
    [
        (0, "Low"),
        (1, "Medium Low"),
        (2, "Normal"),
        (3, "Medium High"),
        (4, "High"),
    ]
);

const_decoder!(
    L4_DECODE_SHARPENING,
    i32,
    [
        (0, "Off"),
        (1, "Low"),
        (2, "Normal"),
        (3, "Medium High"),
        (4, "High"),
    ]
);

const_decoder!(
    L4_DECODE_SATURATION,
    i32,
    [
        (0, "Low"),
        (1, "Medium Low"),
        (2, "Normal"),
        (3, "Medium High"),
        (4, "High"),
        (5, "Black & White"),
        (6, "Vintage B&W"),
    ]
);

const_decoder!(
    L4_DECODE_WHITE_BALANCE,
    i32,
    [
        (0, "Auto"),
        (1, "Tungsten"),
        (2, "Fluorescent"),
        (3, "Daylight Fluorescent"),
        (4, "Daylight"),
        (5, "Flash"),
        (6, "Cloudy"),
        (7, "Shade"),
        (8, "Manual"),
        (9, "Kelvin"),
    ]
);

// `PrintConv => { 94 => 'Basic', 97 => 'Fine' }` (Panasonic.pm:1829-1835) --
// raw JPEG quality-factor values, not the small ordinal codes the old table
// used.
const_decoder!(L4_DECODE_JPEG_QUALITY, i32, [(94, "Basic"), (97, "Fine"),]);

// Leica MakerNote header signature
// Leica typically uses "LEICA\0\0\0" headers.
//
// The other long-form signature a Leica body writes, "LEICA CAMERA AG\0", is
// deliberately absent: ExifTool's `MakerNoteLeica10` routes it to
// `Panasonic::Main`, not to any Leica table, so it is recognised by
// `makernotes::panasonic::is_leica10_makernote` and decoded by the Panasonic
// parser. Claiming it here decoded nothing and shadowed the parser that can.
const LEICA_HEADER_SHORT: &[u8] = b"LEICA\0\0\0";
/// `MakerNoteLeica4`'s signature: the M9 and M Monochrom write "LEICA0\x03\0"
/// (ExifTool MakerNotes.pm:639-648, which matches on `^LEICA0`).
const LEICA_HEADER_LEICA4: &[u8] = b"LEICA0";
/// `MakerNoteLeica9`'s signature, written by the M10, M11 and S bodies
/// (ExifTool MakerNotes.pm:714-722).
const LEICA_HEADER_LEICA9: &[u8] = b"LEICA\0\x02\0";

/// `MakerNoteLeica6`/`MakerNoteLeica7`'s shared signature (MakerNotes.pm:666,
/// 690): the S2, M (Typ 240) and M Monochrom (Typ 246). Both route to
/// `%Panasonic::Leica6`; they differ only in `Base`, which does not affect
/// the inline-only fields this parser reads from that table.
const LEICA_HEADER_LEICA6: &[u8] = b"LEICA\0\x02\xff";

/// `MakerNoteLeica5`/`MakerNoteLeica8`'s shared second byte values
/// (MakerNotes.pm:650, 703): X1/X2/X VARIO/T/TL/X-U (`\x01,\x04,\x05,\x06,
/// \x07,\x10,\x1a`) and Q/SL/CL (`\x08,\x09,\x0a`). Both route to
/// `%Panasonic::Leica5`.
const LEICA5_SECOND_BYTES: &[u8] = b"\x01\x04\x05\x06\x07\x08\x09\x0a\x10\x1a";

/// Which of ExifTool's Leica MakerNote layouts a payload is in.
///
/// `MakerNotes.pm` selects these by header signature and gives each its own IFD
/// start and value-offset base, so the layout has to be settled before any
/// entry can be resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LeicaLayout {
    /// `MakerNoteLeica2`, written by the M8 (MakerNotes.pm:611-624).
    Leica2,
    /// `MakerNoteLeica3`, a bare IFD written by the R8/R9 digital backs
    /// (MakerNotes.pm:625-637).
    Leica3,
    /// `MakerNoteLeica4`, written by the M9 and M Monochrom
    /// (MakerNotes.pm:638-648).
    Leica4,
    /// `MakerNoteLeica5`/`MakerNoteLeica8`, sharing `%Panasonic::Leica5`:
    /// the X1/X2/X VARIO/T/TL/X-U and the Q/SL/CL (MakerNotes.pm:650-712).
    Leica5,
    /// `MakerNoteLeica6`/`MakerNoteLeica7`, sharing `%Panasonic::Leica6`:
    /// the S2, M (Typ 240) and M Monochrom (Typ 246) (MakerNotes.pm:666-701).
    Leica6,
    /// `MakerNoteLeica9`, written by the M10, M11 and S bodies
    /// (MakerNotes.pm:713-722).
    Leica9,
}

impl LeicaLayout {
    /// Distance from the payload's start to the first IFD byte.
    ///
    /// Every LEICA-signed layout puts its IFD at `$valuePtr + 8`; the bare
    /// Leica3 IFD starts at the payload itself.
    fn ifd_offset(self) -> usize {
        match self {
            LeicaLayout::Leica2
            | LeicaLayout::Leica4
            | LeicaLayout::Leica5
            | LeicaLayout::Leica6
            | LeicaLayout::Leica9 => 8,
            LeicaLayout::Leica3 => 0,
        }
    }
}

/// Identifies a payload's layout from its header, or `None` if no Leica layout
/// this parser handles claims it.
fn leica_layout(data: &[u8]) -> Option<LeicaLayout> {
    if data.len() < 8 {
        return None;
    }
    if data.starts_with(LEICA_HEADER_LEICA9) {
        return Some(LeicaLayout::Leica9);
    }
    if data.starts_with(LEICA_HEADER_LEICA6) {
        return Some(LeicaLayout::Leica6);
    }
    if &data[0..8] == LEICA_HEADER_SHORT {
        return Some(LeicaLayout::Leica2);
    }
    if data.starts_with(LEICA_HEADER_LEICA4) {
        return Some(LeicaLayout::Leica4);
    }
    if data.len() >= 8
        && data[0..5] == *b"LEICA"
        && data[5] == 0
        && LEICA5_SECOND_BYTES.contains(&data[6])
        && data[7] == 0
    {
        return Some(LeicaLayout::Leica5);
    }
    if data.starts_with(b"LEICA") {
        // Some other LEICA-signed layout (Leica10's variants). Reading its
        // entries with these tag names would be a guess.
        return None;
    }
    // No signature: a bare IFD, if the entry count is plausible.
    let entry_count = EndianReader::little_endian(data).u16_at(0).unwrap_or(0);
    (entry_count > 0 && entry_count < 150).then_some(LeicaLayout::Leica3)
}

/// Where a Leica MakerNote entry's out-of-line value lives.
///
/// A value longer than four bytes sits elsewhere and the entry holds an offset
/// to it -- but the base that offset counts from is per-layout, and ExifTool
/// spells each one out. `MakerNoteLeica2` declares `Base => '$start'`, the IFD
/// at `$valuePtr + 8` (MakerNotes.pm:621); `MakerNoteLeica4` declares
/// `Base => '$start - 8'`, the payload itself (MakerNotes.pm:645); and
/// `MakerNoteLeica9` declares no `Base` at all, so it inherits the enclosing
/// TIFF header (MakerNotes.pm:717-721).
///
/// The distinction is not cosmetic: resolving `LeicaM10-R.jpg`'s `MeasuredLV`
/// against the payload reads 899785574/2936739356 = 0.31 where ExifTool reads
/// -1993/100 = -19.93.
#[derive(Clone, Copy)]
struct LeicaValues<'a> {
    /// The block an entry's value offset indexes into.
    block: &'a [u8],
    /// What that offset is measured from, as an index into `block`.
    base: usize,
}

impl<'a> LeicaValues<'a> {
    /// Reads `len` bytes of an entry's out-of-line value, or `None` when the
    /// offset does not resolve inside the block.
    fn read(&self, value_offset: u32, len: usize) -> Option<&'a [u8]> {
        let start = self.base.checked_add(usize::try_from(value_offset).ok()?)?;
        self.block.get(start..start.checked_add(len)?)
    }
}

/// Reads a `rational64s` and renders it as ExifTool's `sprintf("%.2f", $val)`.
///
/// Returns `None` for a zero denominator, which ExifTool prints as `inf`
/// rather than a number, and for a slice too short to hold the pair.
fn leica_rational64s_2dp(bytes: &[u8], byte_order: ByteOrder) -> Option<String> {
    let reader = EndianReader::new(bytes, byte_order.to_io_byte_order());
    let numerator = reader.i32_at(0)?;
    let denominator = reader.i32_at(4)?;
    (denominator != 0).then(|| format!("{:.2}", f64::from(numerator) / f64::from(denominator)))
}

/// Reads a `rational64u` and renders it with `decimals` places, as ExifTool's
/// `sprintf("%.Nf", $val)`. `None` for a zero denominator or a too-short slice.
fn leica_rational_2dp(bytes: &[u8], byte_order: ByteOrder, decimals: usize) -> Option<String> {
    let reader = EndianReader::new(bytes, byte_order.to_io_byte_order());
    let numerator = reader.u32_at(0)?;
    let denominator = reader.u32_at(4)?;
    (denominator != 0).then(|| {
        format!(
            "{:.decimals$}",
            f64::from(numerator) / f64::from(denominator)
        )
    })
}

/// Reads a `rational64u` with no `PrintConv` and renders it the way ExifTool's
/// default number stringification does: round to 10 decimal places, then trim
/// trailing zeros (and a bare trailing point), so `1/1` prints as `1` and
/// `883/1858`-style ratios print with as few digits as the rounding allows --
/// verified against `LeicaM8.jpg`/`LeicaM8.2.jpg`'s `WBRedLevel` et al., where
/// Rust's shortest-round-trip `f64::to_string()` prints 6+ digits more than
/// ExifTool does for the same ratio.
fn leica_rational_unsigned(bytes: &[u8], byte_order: ByteOrder) -> Option<String> {
    let reader = EndianReader::new(bytes, byte_order.to_io_byte_order());
    let numerator = reader.u32_at(0)?;
    let denominator = reader.u32_at(4)?;
    if denominator == 0 {
        return None;
    }
    let value = f64::from(numerator) / f64::from(denominator);
    let formatted = format!("{value:.10}");
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    Some(if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    })
}

/// Reads an out-of-line ASCII string value, trimmed of its trailing NUL.
///
/// `count` is the TIFF component count (byte length for an ASCII field); a
/// count of 4 or fewer would normally live inline in the entry rather than
/// out-of-line, but every caller here is a Leica table entry ExifTool
/// declares `Writable => 'string'` with no such special case, so this always
/// resolves through `values`.
fn read_leica_string(values: LeicaValues<'_>, value_offset: u32, count: u32) -> Option<String> {
    let len = usize::try_from(count).ok()?;
    let bytes = values.read(value_offset, len)?;
    // These are fixed-width buffers padded past a NUL terminator with
    // trailing spaces, not more NULs (`LeicaT.jpg`'s LensType is 38
    // meaningful bytes inside a 60-byte field); a C-string read -- stop at
    // the first NUL -- is what ExifTool's own text extraction does here,
    // where `trim_end_matches('\0')` alone would leave the space padding in.
    let text = bytes.split(|&b| b == 0).next().unwrap_or(bytes);
    Some(String::from_utf8_lossy(text).trim_end().to_string())
}

/// Named entries in `%PanasonicRaw::CameraIFD` (PanasonicRaw.pm:500-668),
/// embedded by Leica5's 0x05ff `CameraIFD`.
///
/// Leica5 explicitly declares this as a TIFF subdirectory with its own byte
/// order and IFD offset.  Decode only fields whose type and conversion are
/// fully specified by that table; unknown camera fields stay absent rather
/// than being reinterpreted as generic TIFF tags.
fn parse_leica5_camera_ifd(data: &[u8], tags: &mut HashMap<String, String>) {
    let byte_order = match data.get(..4) {
        Some(b"II*\0") => ByteOrder::LittleEndian,
        Some(b"MM\0*") => ByteOrder::BigEndian,
        _ => return,
    };
    let reader = EndianReader::new(data, byte_order.to_io_byte_order());
    let Some(ifd_offset) = reader
        .u32_at(4)
        .and_then(|offset| usize::try_from(offset).ok())
    else {
        return;
    };
    let Some(entry_count) = reader.u16_at(ifd_offset).map(usize::from) else {
        return;
    };
    if entry_count > 200 {
        return;
    }
    let Some(entries_end) = ifd_offset.checked_add(2).and_then(|start| {
        entry_count
            .checked_mul(12)
            .and_then(|len| start.checked_add(len))
    }) else {
        return;
    };
    if entries_end > data.len() {
        return;
    }

    for index in 0..entry_count {
        let entry_offset = ifd_offset + 2 + index * 12;
        let entry_reader = EndianReader::new(
            &data[entry_offset..entry_offset + 12],
            byte_order.to_io_byte_order(),
        );
        let (Some(tag_id), Some(field_type), Some(count)) = (
            entry_reader.u16_at(0),
            entry_reader.u16_at(2),
            entry_reader.u32_at(4),
        ) else {
            continue;
        };
        if count != 1 {
            continue;
        }

        let u8_value = || entry_reader.u8_at(8);
        let u16_value = || entry_reader.u16_at(8);
        let i16_value = || entry_reader.i16_at(8);
        let u32_value = || entry_reader.u32_at(8);

        match (tag_id, field_type) {
            // PanasonicRaw declares these as int32u. Leica Q3 stores them
            // with its private int32u TIFF type (0x0101), while other Leica5
            // variants use ordinary LONG or BYTE forms.
            (0x1001, 1 | 4 | 0x0101) => match if field_type == 1 {
                u8_value().map(u32::from)
            } else {
                u32_value()
            } {
                Some(0) => {
                    tags.insert("PanasonicRaw:MultishotOn".to_string(), "No".to_string());
                }
                Some(1) => {
                    tags.insert("PanasonicRaw:MultishotOn".to_string(), "Yes".to_string());
                }
                _ => {}
            },
            // LeicaQ3_43.jpg stores 0x1100/0x1101 as TIFF SHORT. The
            // PanasonicRaw table has no PrintConv for either value.
            (0x1100, 3) => {
                if let Some(value) = entry_reader.u16_at(8) {
                    tags.insert("PanasonicRaw:FocusStepNear".to_string(), value.to_string());
                }
            }
            (0x1101, 3) => {
                if let Some(value) = entry_reader.u16_at(8) {
                    tags.insert("PanasonicRaw:FocusStepCount".to_string(), value.to_string());
                }
            }
            // 0x1102 has the same boolean PrintConv as 0x1001.
            (0x1102, 1 | 4 | 0x0101) => match if field_type == 1 {
                u8_value().map(u32::from)
            } else {
                u32_value()
            } {
                Some(0) => {
                    tags.insert("PanasonicRaw:FlashFired".to_string(), "No".to_string());
                }
                Some(1) => {
                    tags.insert("PanasonicRaw:FlashFired".to_string(), "Yes".to_string());
                }
                _ => {}
            },
            (0x1105, 4) => {
                if let Some(value) = u32_value() {
                    tags.insert("PanasonicRaw:ZoomPosition".to_string(), value.to_string());
                }
            }
            // 0x1200 has the same boolean PrintConv as 0x1001.
            (0x1200, 1 | 4 | 0x0101) => match if field_type == 1 {
                u8_value().map(u32::from)
            } else {
                u32_value()
            } {
                Some(0) => {
                    tags.insert("PanasonicRaw:LensAttached".to_string(), "No".to_string());
                }
                Some(1) => {
                    tags.insert("PanasonicRaw:LensAttached".to_string(), "Yes".to_string());
                }
                _ => {}
            },
            // 0x1202 uses `sprintf("%.4x", $val)` then swaps the two byte
            // pairs, so the LE value 0xf002 is rendered as `02 f0`.
            (0x1202, 3) => {
                if let Some(value) = u16_value() {
                    let bytes = value.to_be_bytes();
                    tags.insert(
                        "PanasonicRaw:LensTypeModel".to_string(),
                        format!("{:02x} {:02x}", bytes[1], bytes[0]),
                    );
                }
            }
            (0x1203, 3) => {
                if let Some(value) = u16_value() {
                    tags.insert(
                        "PanasonicRaw:FocalLengthIn35mmFormat".to_string(),
                        format!("{value} mm"),
                    );
                }
            }
            // ValueConv => `2 ** ($val / 512)`, PrintConv => `%.1f`.
            (0x1301, 8) => {
                if let Some(value) = i16_value() {
                    tags.insert(
                        "PanasonicRaw:ApertureValue".to_string(),
                        format!("{:.1}", 2_f64.powf(f64::from(value) / 512.0)),
                    );
                }
            }
            // ValueConv => `2 ** (-$val / 256)`, PrintConv =>
            // `Image::ExifTool::Exif::PrintExposureTime($val)`.
            (0x1302, 8) => {
                if let Some(value) = i16_value() {
                    let seconds = 2_f64.powf(-f64::from(value) / 256.0);
                    tags.insert(
                        "PanasonicRaw:ShutterSpeedValue".to_string(),
                        crate::core::formatters::exif_print_conv::print_exposure_time(seconds),
                    );
                }
            }
            // ValueConv => `$val / 256` with no PrintConv.
            (0x1303, 8) => {
                if let Some(value) = i16_value() {
                    tags.insert(
                        "PanasonicRaw:SensitivityValue".to_string(),
                        format_number(f64::from(value) / 256.0),
                    );
                }
            }
            (0x1412, 1) => match u8_value() {
                Some(0) => {
                    tags.insert("PanasonicRaw:FacesDetected".to_string(), "No".to_string());
                }
                Some(1) => {
                    tags.insert("PanasonicRaw:FacesDetected".to_string(), "Yes".to_string());
                }
                _ => {}
            },
            (0x3300, 1) => {
                if let Some(value) = u8_value().and_then(panasonic_white_balance) {
                    tags.insert(
                        "PanasonicRaw:WhiteBalanceSet".to_string(),
                        value.to_string(),
                    );
                }
            }
            (0x3420, 3) => {
                if let Some(value) = u16_value() {
                    tags.insert(
                        "PanasonicRaw:WB_RedLevelAuto".to_string(),
                        value.to_string(),
                    );
                }
            }
            (0x3421, 3) => {
                if let Some(value) = u16_value() {
                    tags.insert(
                        "PanasonicRaw:WB_BlueLevelAuto".to_string(),
                        value.to_string(),
                    );
                }
            }
            (0x3501, 1) => {
                if let Some(value) = u8_value().and_then(tiff_orientation) {
                    tags.insert("PanasonicRaw:Orientation".to_string(), value.to_string());
                }
            }
            (0x3600, 1) => {
                if let Some(value) = u8_value().and_then(panasonic_white_balance) {
                    tags.insert(
                        "PanasonicRaw:WhiteBalanceDetected".to_string(),
                        value.to_string(),
                    );
                }
            }
            _ => {}
        }
    }
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn panasonic_white_balance(value: u8) -> Option<&'static str> {
    Some(match value {
        0 => "Auto",
        1 => "Daylight",
        2 => "Cloudy",
        3 => "Tungsten",
        4 | 6 | 7 => "n/a",
        5 => "Flash",
        8 => "Custom#1",
        9 => "Custom#2",
        10 => "Custom#3",
        11 => "Custom#4",
        12 => "Shade",
        13 => "Kelvin",
        16 => "AWBc",
        _ => return None,
    })
}

fn tiff_orientation(value: u8) -> Option<&'static str> {
    Some(match value {
        1 => "Horizontal (normal)",
        2 => "Mirror horizontal",
        3 => "Rotate 180",
        4 => "Mirror vertical",
        5 => "Mirror horizontal and rotate 270 CW",
        6 => "Rotate 90 CW",
        7 => "Mirror horizontal and rotate 90 CW",
        8 => "Rotate 270 CW",
        _ => return None,
    })
}

/// Reads an ASCII entry of the `%Panasonic::Leica4` sub-directories.
///
/// The value field of a TIFF entry holds the bytes themselves whenever they fit
/// in four (Exif.pm:6502 sets `$valuePtr = $entry + 8` and only follows the
/// pointer `if ($size > 4)`). Treating the field as an offset regardless read
/// the wrong bytes for every short string: on `LeicaM_Monochrom.jpg`,
/// `SerialNumber` is `string[2]` = `35 00` stored inline, and following `0x35`
/// as an offset produced no tag at all where ExifTool prints `5`; `UserProfile`
/// is `string[1]` = `00`, an empty string ExifTool prints as `""`.
fn l4_string(
    format: u16,
    count: u32,
    value_offset: u32,
    full_data: &[u8],
    byte_order: ByteOrder,
) -> Option<String> {
    if format != 2 || count == 0 {
        return None;
    }
    let len = count as usize;
    let owned;
    let bytes: &[u8] = if len <= 4 {
        owned = match byte_order {
            ByteOrder::LittleEndian => value_offset.to_le_bytes(),
            ByteOrder::BigEndian => value_offset.to_be_bytes(),
        };
        &owned[..len]
    } else {
        let start = value_offset as usize;
        full_data.get(start..start.checked_add(len)?)?
    };
    Some(
        String::from_utf8_lossy(bytes)
            .trim_end_matches('\0')
            .to_string(),
    )
}

/// Byte length of one component of a TIFF field type, or `0` for a type this
/// parser never reads (the entry is skipped either way).
fn leica_type_size(field_type: u16) -> usize {
    match field_type {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => 0,
    }
}

/// Reproduces ExifTool's `FixLeicaBase` (MakerNotes.pm:1669-1691): the M8
/// writes `MakerNoteLeica2` with the IFD's value offsets counting from the IFD
/// itself in most files, but from 8 bytes earlier -- the payload's own start
/// -- in others (`LeicaM8.2.jpg` among them). ExifTool detects which by
/// comparing the lowest out-of-line value offset against where the entry list
/// ends: a gap bigger than 8 bytes means the offsets were written relative to
/// the payload start, not the IFD.
///
/// Returns the shift to apply to the IFD-relative base: `0` normally, `-8`
/// when the fixup applies.
fn leica2_base_shift(ifd_data: &[u8], entry_count: u16, byte_order: ByteOrder) -> i64 {
    let reader = EndianReader::new(ifd_data, byte_order.to_io_byte_order());
    let min_value_ptr = (0..entry_count)
        .filter_map(|i| {
            let entry_offset = 2 + usize::from(i) * 12;
            let field_type = reader.u16_at(entry_offset + 2)?;
            let count = reader.u32_at(entry_offset + 4)?;
            let size = usize::try_from(count)
                .ok()?
                .checked_mul(leica_type_size(field_type))?;
            // A value of 4 bytes or fewer lives in the entry itself, not
            // out-of-line, so it says nothing about the base.
            (size > 4).then(|| reader.u32_at(entry_offset + 8))?
        })
        .min();
    match min_value_ptr {
        Some(min_value_ptr) if i64::from(min_value_ptr) - (i64::from(entry_count) * 12 + 4) > 8 => {
            -8
        }
        _ => 0,
    }
}

/// Checks if the provided data has a valid Leica MakerNote header
///
/// # Arguments
/// * `data` - Raw MakerNote data to validate
///
/// # Returns
/// * `true` if data contains a valid Leica header
/// * `false` otherwise
pub fn is_leica_makernote(data: &[u8]) -> bool {
    leica_layout(data).is_some()
}

// ============================================================================
// DECODERS - Leica Value Decoders
// ============================================================================
// Following the shared decoder pattern from fujifilm.rs, canon.rs, and sony.rs
// Each decoder is a constant that implements the Decode trait

// Decodes `%Panasonic::Leica2` 0x0300 `Quality` (Panasonic.pm:1608-1614) --
// only two values are defined; anything else prints as the raw number.
const_decoder!(pub
    LEICA2_DECODE_QUALITY, i32, [
        (1, "Fine"),
        (2, "Basic"),
    ]
);

// Decodes `%Panasonic::Leica2` 0x0302 `UserProfile` (Panasonic.pm:1616-1624).
const_decoder!(pub
    LEICA2_DECODE_USER_PROFILE, i32, [
        (1, "User Profile 1"),
        (2, "User Profile 2"),
        (3, "User Profile 3"),
        (4, "User Profile 0 (Dynamic)"),
    ]
);

/// Decodes `%Panasonic::Leica2` 0x0304 `WhiteBalance` (Panasonic.pm:1626-1638).
/// Values above `0x8000` are a Kelvin temperature (`WhiteBalanceConv`,
/// Panasonic.pm:2731-2739): `(val - 0x8000) . ' Kelvin'`.
fn leica2_decode_white_balance(value: i32) -> String {
    match value {
        0 => "Auto or Manual".to_string(),
        1 => "Daylight".to_string(),
        2 => "Fluorescent".to_string(),
        3 => "Tungsten".to_string(),
        4 => "Flash".to_string(),
        10 => "Cloudy".to_string(),
        11 => "Shade".to_string(),
        v if v > 0x8000 => format!("{} Kelvin", v - 0x8000),
        v => format!("Unknown ({v})"),
    }
}

/// Decodes `%Panasonic::Leica9` 0x0359 `ISOSelected` (Panasonic.pm:2219-2226):
/// `0` prints as `Auto`, anything else prints as the raw value.
fn leica9_decode_iso_selected(value: i32) -> String {
    if value == 0 {
        "Auto".to_string()
    } else {
        value.to_string()
    }
}

/// Leica MakerNote Parser
///
/// Implements the MakerNoteParser trait for Leica cameras.
pub struct LeicaMakerNoteParser;

impl MakerNoteParser for LeicaMakerNoteParser {
    fn manufacturer_name(&self) -> &'static str {
        "Leica"
    }

    fn tag_prefix(&self) -> &'static str {
        "Leica:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        is_leica_makernote(data)
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        // No enclosing block, so a Leica9 payload's TIFF-relative value offsets
        // stay unresolvable and the two tags that need them are skipped rather
        // than guessed. `parse_with_context` is the entry point that has them.
        self.parse_payload(data, byte_order, None, tags)
    }

    /// Leica9's value offsets are measured from the enclosing TIFF header
    /// rather than from the MakerNote, so `MeasuredLV` and
    /// `ExternalSensorBrightnessValue` on an M10/M11/S are reachable only from
    /// a located context.
    fn parse_with_context(
        &self,
        ctx: &MakerNoteContext<'_>,
        byte_order: ByteOrder,
        _model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        self.parse_payload(ctx.payload(), byte_order, Some(ctx), tags)
    }
}

impl LeicaMakerNoteParser {
    /// Routes a payload to the decoder for its layout.
    ///
    /// `ctx` is the enclosing TIFF block when the caller knows it; only Leica9
    /// needs it, and only for its two rational exposure measurements.
    fn parse_payload(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        ctx: Option<&MakerNoteContext<'_>>,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        // Validate minimum data length
        if data.len() < 8 {
            return Err("Leica MakerNote data too short".to_string());
        }

        let Some(layout) = leica_layout(data) else {
            return Err("Unrecognised Leica MakerNote layout".to_string());
        };

        // The M9's entries are subdirectory pointers, not tags.
        if layout == LeicaLayout::Leica4 {
            return self.parse_leica4(data, byte_order, tags);
        }

        let offset = layout.ifd_offset();

        // Ensure we have enough data after the header
        if offset >= data.len() {
            return Err("No data after Leica header".to_string());
        }

        let ifd_data = &data[offset..];

        // Parse IFD entry count
        if ifd_data.len() < 2 {
            return Err("Insufficient data for IFD entry count".to_string());
        }

        // Parse IFD entry count using EndianReader
        let ifd_reader = EndianReader::new(ifd_data, byte_order.to_io_byte_order());
        let entry_count = ifd_reader.u16_at(0).unwrap_or(0);

        // Validate entry count is reasonable
        if entry_count == 0 || entry_count > 200 {
            return Err(format!("Invalid Leica IFD entry count: {}", entry_count));
        }

        // Where this layout's out-of-line values are measured from.
        let values = match layout {
            // No `Base`, so the enclosing TIFF header. Without a located
            // context there is nothing to measure against.
            //
            // `MakerNoteLeica3` (R8/R9) declares no `Base` either
            // (MakerNotes.pm:625-637, only `Start => '$valuePtr'`), so its
            // out-of-line reads are the same TIFF-relative story: R9's
            // `SerialInfo` (0x0b) entry holds `value_offset=772`, which
            // resolves to `1000975` only when read from the enclosing TIFF
            // block, not the ~240-byte MakerNote payload alone.
            LeicaLayout::Leica9 | LeicaLayout::Leica3 => {
                ctx.filter(|ctx| ctx.is_located()).map(|ctx| LeicaValues {
                    block: ctx.tiff(),
                    base: 0,
                })
            }
            // `Base => '$start'`: the IFD -- except `FixLeicaBase` shifts it 8
            // bytes earlier, to the payload's own start, on the files whose
            // value offsets were written that way instead. Either base can
            // land a value outside the declared payload (`LeicaM8.2.jpg`'s
            // sit ~700 bytes past it), so this prefers the located context's
            // wider window and only falls back to the bare payload -- which
            // resolves an in-bounds value just as well -- when no context was
            // given.
            LeicaLayout::Leica2 => {
                let shift = leica2_base_shift(ifd_data, entry_count, byte_order);
                let located = ctx.filter(|ctx| ctx.is_located());
                let base =
                    i64::try_from(located.map_or(offset, |ctx| ctx.payload_offset() + offset))
                        .ok()
                        .and_then(|base| base.checked_add(shift))
                        .and_then(|base| usize::try_from(base).ok());
                base.map(|base| LeicaValues {
                    block: located.map_or(data, |ctx| ctx.tiff()),
                    base,
                })
            }
            // `MakerNoteLeica5` declares `Base => '$start - 8'`
            // (MakerNotes.pm:657) -- the payload's own start, same
            // convention as Leica4.
            LeicaLayout::Leica5 => Some(LeicaValues {
                block: data,
                base: 0,
            }),
            // Leica3, Leica6 (inline-only in this parser) and the
            // long-header layout have no out-of-line reads that need a base
            // other than the IFD itself.
            _ => Some(LeicaValues {
                block: ifd_data,
                base: 0,
            }),
        };

        // Each IFD entry is 12 bytes: 2 (tag) + 2 (type) + 4 (count) + 4 (value/offset)
        let required_size = 2 + (entry_count as usize * 12);
        if ifd_data.len() < required_size {
            return Err(format!(
                "Insufficient data for {} IFD entries (need {}, have {})",
                entry_count,
                required_size,
                ifd_data.len()
            ));
        }

        // Parse each IFD entry
        for i in 0..entry_count {
            let entry_offset = 2 + (i as usize * 12);
            let entry_data = &ifd_data[entry_offset..entry_offset + 12];
            let entry_reader = EndianReader::new(entry_data, byte_order.to_io_byte_order());

            // Parse IFD entry fields using EndianReader
            let tag_id = entry_reader.u16_at(0).unwrap_or(0);
            let format = entry_reader.u16_at(2).unwrap_or(0);
            let component_count = entry_reader.u32_at(4).unwrap_or(0);
            let value_offset = entry_reader.u32_at(8).unwrap_or(0);

            // Create IfdEntry for this tag
            let entry = IfdEntry {
                tag_id,
                field_type: format,
                value_count: component_count,
                value_offset,
            };

            // Each layout points at its own ExifTool table, and the same
            // numeric id means different things in different tables, so
            // dispatch on `layout` before matching on `tag_id`.
            match layout {
                LeicaLayout::Leica2 => self.decode_leica2_entry(&entry, values, byte_order, tags),
                LeicaLayout::Leica3 => self.decode_leica3_entry(&entry, values, byte_order, tags),
                LeicaLayout::Leica5 => {
                    // Leica5's ordinary values use the MakerNote-relative
                    // `Base => '$start - 8'`, but its 0x05ff CameraIFD is a
                    // nested TIFF directory.  The directory entry's offset
                    // is relative to the enclosing TIFF header, not to the
                    // Leica payload: Q3_43.jpg stores 0x095c here, which
                    // addresses the TIFF at 0x095c (the `II*` begins there),
                    // while indexing the payload at 0x095c lands elsewhere.
                    // Prefer the located TIFF context for that one entry;
                    // retain the payload path for detached callers/tests.
                    let camera_ifd = (entry.tag_id == leica5::CAMERA_IFD)
                        .then(|| {
                            ctx.filter(|ctx| ctx.is_located()).and_then(|ctx| {
                                usize::try_from(entry.value_offset).ok().and_then(|offset| {
                                    usize::try_from(entry.value_count).ok().and_then(|count| {
                                        ctx.tiff().get(offset..offset.checked_add(count)?)
                                    })
                                })
                            })
                        })
                        .flatten();
                    self.decode_leica5_entry(&entry, values, camera_ifd, tags);
                }
                LeicaLayout::Leica6 => self.decode_leica6_entry(&entry, tags),
                LeicaLayout::Leica9 => self.decode_leica9_entry(&entry, values, byte_order, tags),
                // No ExifTool table is known to correspond to this header;
                // decoding tag ids against it would be a guess.
                LeicaLayout::Leica4 => {}
            }
        }

        Ok(())
    }

    /// `%Panasonic::Leica2` (Panasonic.pm:1603), the M8.
    fn decode_leica2_entry(
        &self,
        entry: &IfdEntry,
        values: Option<LeicaValues<'_>>,
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        match entry.tag_id {
            leica2::QUALITY => {
                let value = entry.value_offset as i32;
                tags.insert(
                    "Leica:Quality".to_string(),
                    LEICA2_DECODE_QUALITY.decode(value).to_string(),
                );
            }
            leica2::USER_PROFILE => {
                let value = entry.value_offset as i32;
                tags.insert(
                    "Leica:UserProfile".to_string(),
                    LEICA2_DECODE_USER_PROFILE.decode(value).to_string(),
                );
            }
            // `PrintConv => sprintf("%.7d", $val)` (Panasonic.pm:1631).
            leica2::SERIAL_NUMBER => {
                tags.insert(
                    "Leica:SerialNumber".to_string(),
                    format!("{:07}", entry.value_offset),
                );
            }
            leica2::WHITE_BALANCE => {
                let value = entry.value_offset as i32;
                tags.insert(
                    "Leica:WhiteBalance".to_string(),
                    leica2_decode_white_balance(value),
                );
            }
            leica2::LENS_TYPE => {
                let raw = entry.value_offset;
                let printed = leica_lenses::lookup(raw)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("Unknown ({})", leica_lenses::value_conv(raw)));
                tags.insert("Leica:LensType".to_string(), printed);
            }
            leica2::MEASURED_LV => {
                if let Some(lv) = values
                    .and_then(|values| values.read(entry.value_offset, 8))
                    .and_then(|bytes| leica_rational64s_2dp(bytes, byte_order))
                {
                    tags.insert("Leica:MeasuredLV".to_string(), lv);
                }
            }
            leica2::EXTERNAL_SENSOR_BRIGHTNESS_VALUE => {
                if let Some(ev) = values
                    .and_then(|values| values.read(entry.value_offset, 8))
                    .and_then(|bytes| leica_rational64s_2dp(bytes, byte_order))
                {
                    tags.insert("Leica:ExternalSensorBrightnessValue".to_string(), ev);
                }
            }
            // `Writable => 'rational64u'`, `PrintConv => sprintf("%.1f", $val)`.
            leica2::APPROXIMATE_F_NUMBER => {
                if let Some(f) = values
                    .and_then(|values| values.read(entry.value_offset, 8))
                    .and_then(|bytes| leica_rational_2dp(bytes, byte_order, 1))
                {
                    tags.insert("Leica:ApproximateFNumber".to_string(), f);
                }
            }
            // `PrintConv => '"$val C"'` (Panasonic.pm:1673).
            leica2::CAMERA_TEMPERATURE => {
                let value = entry.value_offset as i32;
                tags.insert("Leica:CameraTemperature".to_string(), format!("{value} C"));
            }
            // Plain `int32u`/`rational64u`, no PrintConv (Panasonic.pm:1678-1682).
            leica2::COLOR_TEMPERATURE => {
                tags.insert(
                    "Leica:ColorTemperature".to_string(),
                    entry.value_offset.to_string(),
                );
            }
            leica2::WB_RED_LEVEL => {
                if let Some(v) = values
                    .and_then(|values| values.read(entry.value_offset, 8))
                    .and_then(|bytes| leica_rational_unsigned(bytes, byte_order))
                {
                    tags.insert("Leica:WBRedLevel".to_string(), v);
                }
            }
            leica2::WB_GREEN_LEVEL => {
                if let Some(v) = values
                    .and_then(|values| values.read(entry.value_offset, 8))
                    .and_then(|bytes| leica_rational_unsigned(bytes, byte_order))
                {
                    tags.insert("Leica:WBGreenLevel".to_string(), v);
                }
            }
            leica2::WB_BLUE_LEVEL => {
                if let Some(v) = values
                    .and_then(|values| values.read(entry.value_offset, 8))
                    .and_then(|bytes| leica_rational_unsigned(bytes, byte_order))
                {
                    tags.insert("Leica:WBBlueLevel".to_string(), v);
                }
            }
            leica2::UV_IR_FILTER_CORRECTION => {
                let name = match entry.value_offset {
                    0 => "Not Active",
                    1 => "Active",
                    _ => return,
                };
                tags.insert("Leica:UV-IRFilterCorrection".to_string(), name.to_string());
            }
            leica2::CCD_VERSION => {
                tags.insert(
                    "Leica:CCDVersion".to_string(),
                    entry.value_offset.to_string(),
                );
            }
            leica2::CCD_BOARD_VERSION => {
                tags.insert(
                    "Leica:CCDBoardVersion".to_string(),
                    entry.value_offset.to_string(),
                );
            }
            leica2::CONTROLLER_BOARD_VERSION => {
                tags.insert(
                    "Leica:ControllerBoardVersion".to_string(),
                    entry.value_offset.to_string(),
                );
            }
            leica2::M16C_VERSION => {
                tags.insert(
                    "Leica:M16CVersion".to_string(),
                    entry.value_offset.to_string(),
                );
            }
            leica2::IMAGE_ID_NUMBER => {
                tags.insert(
                    "Leica:ImageIDNumber".to_string(),
                    entry.value_offset.to_string(),
                );
            }
            _ => {}
        }
    }

    /// `%Panasonic::Leica3` (Panasonic.pm:1705), the R8/R9 digital backs.
    fn decode_leica3_entry(
        &self,
        entry: &IfdEntry,
        values: Option<LeicaValues<'_>>,
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        match entry.tag_id {
            // `SubDirectory => Panasonic::SerialInfo`, whose tag 4 is an
            // 8-byte `SerialNumber` string (Panasonic.pm:1723-1734).
            leica3::SERIAL_INFO => {
                if let Some(bytes) = values.and_then(|values| {
                    values.read(
                        entry.value_offset + leica3::SERIAL_INFO_NUMBER_OFFSET,
                        leica3::SERIAL_INFO_NUMBER_LEN,
                    )
                }) {
                    let serial = String::from_utf8_lossy(bytes)
                        .trim_end_matches('\0')
                        .to_string();
                    tags.insert("Leica:SerialNumber".to_string(), serial);
                }
            }
            // Three `int16u` components (6 bytes) do not fit in the entry's
            // own four, so they sit out-of-line at `value_offset`.
            leica3::WB_RGB_LEVELS => {
                if entry.value_count == 3 {
                    if let Some(bytes) =
                        values.and_then(|values| values.read(entry.value_offset, 6))
                    {
                        let reader = EndianReader::new(bytes, byte_order.to_io_byte_order());
                        if let (Some(r), Some(g), Some(b)) =
                            (reader.u16_at(0), reader.u16_at(2), reader.u16_at(4))
                        {
                            tags.insert("Leica:WB_RGBLevels".to_string(), format!("{r} {g} {b}"));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// `%Panasonic::Leica5` (Panasonic.pm:1996), the X1/X2/X VARIO/T/TL
    /// (`MakerNoteLeica5`) and Q/SL/CL (`MakerNoteLeica8`, same table).
    fn decode_leica5_entry(
        &self,
        entry: &IfdEntry,
        values: Option<LeicaValues<'_>>,
        camera_ifd: Option<&[u8]>,
        tags: &mut HashMap<String, String>,
    ) {
        match entry.tag_id {
            // `Condition => '$format eq "string"'` (Panasonic.pm:2003) --
            // field type 2 is TIFF ASCII.
            leica5::LENS_TYPE if entry.field_type == 2 => {
                if let Some(s) = values.and_then(|values| {
                    read_leica_string(values, entry.value_offset, entry.value_count)
                }) {
                    tags.insert("Leica:LensType".to_string(), s);
                }
            }
            leica5::SERIAL_NUMBER => {
                tags.insert(
                    "Leica:SerialNumber".to_string(),
                    entry.value_offset.to_string(),
                );
            }
            leica5::CAMERA_IFD => {
                if let Some(camera_ifd) = camera_ifd.or_else(|| {
                    values.and_then(|values| {
                        usize::try_from(entry.value_count)
                            .ok()
                            .and_then(|count| values.read(entry.value_offset, count))
                    })
                }) {
                    parse_leica5_camera_ifd(camera_ifd, tags);
                }
            }
            _ => {}
        }
    }

    /// `%Panasonic::Leica6` (Panasonic.pm:2111), the S2/M (Typ 240) via
    /// `MakerNoteLeica6` and the M Monochrom (Typ 246) via `MakerNoteLeica7`.
    /// `LensType` (0x303) and `PreviewImage` (0x300) are written to a JPEG
    /// trailer on these bodies and are not reachable from the MakerNote
    /// payload/TIFF window this parser is handed; only `FocusDistance`,
    /// whose value fits inline in the entry, is decoded.
    fn decode_leica6_entry(&self, entry: &IfdEntry, tags: &mut HashMap<String, String>) {
        if entry.tag_id == leica6::FOCUS_DISTANCE {
            tags.insert(
                "Leica:FocusDistance".to_string(),
                entry.value_offset.to_string(),
            );
        }
    }

    /// `%Panasonic::Leica9` (Panasonic.pm:2192), the M10/M11 and the S (Typ 007).
    fn decode_leica9_entry(
        &self,
        entry: &IfdEntry,
        values: Option<LeicaValues<'_>>,
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        match entry.tag_id {
            leica9::FOCUS_DISTANCE => {
                tags.insert(
                    "Leica:FocusDistance".to_string(),
                    entry.value_offset.to_string(),
                );
            }
            leica9::MEASURED_LV => {
                if let Some(lv) = values
                    .and_then(|values| values.read(entry.value_offset, 8))
                    .and_then(|bytes| leica_rational64s_2dp(bytes, byte_order))
                {
                    tags.insert("Leica:MeasuredLV".to_string(), lv);
                }
            }
            leica9::EXTERNAL_SENSOR_BRIGHTNESS_VALUE => {
                if let Some(ev) = values
                    .and_then(|values| values.read(entry.value_offset, 8))
                    .and_then(|bytes| leica_rational64s_2dp(bytes, byte_order))
                {
                    tags.insert("Leica:ExternalSensorBrightnessValue".to_string(), ev);
                }
            }
            leica9::USER_PROFILE => {
                if let Some(s) = values.and_then(|values| {
                    read_leica_string(values, entry.value_offset, entry.value_count)
                }) {
                    tags.insert("Leica:UserProfile".to_string(), s);
                }
            }
            leica9::ISO_SELECTED => {
                let value = entry.value_offset as i32;
                tags.insert(
                    "Leica:ISOSelected".to_string(),
                    leica9_decode_iso_selected(value),
                );
            }
            // `ValueConv => '$val / 1000'`, `PrintConv => sprintf("%.1f", $val)`.
            leica9::F_NUMBER => {
                let value = entry.value_offset as i32 as f64 / 1000.0;
                tags.insert("Leica:FNumber".to_string(), format!("{value:.1}"));
            }
            leica9::CORRELATED_COLOR_TEMP => {
                tags.insert(
                    "Leica:CorrelatedColorTemp".to_string(),
                    (entry.value_offset as u16).to_string(),
                );
            }
            leica9::COLOR_TINT => {
                tags.insert(
                    "Leica:ColorTint".to_string(),
                    (entry.value_offset as i16).to_string(),
                );
            }
            // Two `rational64u` components (x/y); both sit out-of-line.
            leica9::WHITE_POINT => {
                if let Some(bytes) = values.and_then(|values| values.read(entry.value_offset, 16)) {
                    if let (Some(x), Some(y)) = (
                        leica_rational_unsigned(&bytes[0..8], byte_order),
                        leica_rational_unsigned(&bytes[8..16], byte_order),
                    ) {
                        tags.insert("Leica:WhitePoint".to_string(), format!("{x} {y}"));
                    }
                }
            }
            leica9::LENS_PROFILE_NAME => {
                if let Some(s) = values.and_then(|values| {
                    read_leica_string(values, entry.value_offset, entry.value_count)
                }) {
                    tags.insert("Leica:LensProfileName".to_string(), s);
                }
            }
            _ => {}
        }
    }
}

impl LeicaMakerNoteParser {
    /// Parse Leica4 format (M9/M Monochrom)
    ///
    /// This format uses a "LEICA0\x03\0" header followed by an IFD with
    /// subdirectory tags at 0x3000, 0x3100, 0x3400, 0x3900.
    fn parse_leica4(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        // Skip 8-byte header: "LEICA0\x03\0"
        let header_size = 8;
        if data.len() <= header_size {
            return Err("No data after Leica4 header".to_string());
        }

        let ifd_data = &data[header_size..];
        if ifd_data.len() < 2 {
            return Err("Insufficient data for IFD entry count".to_string());
        }

        let reader = EndianReader::new(ifd_data, byte_order.to_io_byte_order());
        let entry_count = reader.u16_at(0).unwrap_or(0);

        if entry_count == 0 || entry_count > 50 {
            return Err(format!("Invalid Leica4 IFD entry count: {}", entry_count));
        }

        let required_size = 2 + (entry_count as usize * 12);
        if ifd_data.len() < required_size {
            return Err("Insufficient data for IFD entries".to_string());
        }

        // Parse each IFD entry - these are subdirectory pointers
        for i in 0..entry_count {
            let entry_offset = 2 + (i as usize * 12);
            let entry_data = &ifd_data[entry_offset..entry_offset + 12];
            let entry_reader = EndianReader::new(entry_data, byte_order.to_io_byte_order());

            let tag_id = entry_reader.u16_at(0).unwrap_or(0);
            let count = entry_reader.u32_at(4).unwrap_or(0);
            let value_offset = entry_reader.u32_at(8).unwrap_or(0);

            // Each main tag points to a subdirectory
            match tag_id {
                L4_SUBDIR_3000 | L4_SUBDIR_3100 | L4_SUBDIR_3400 | L4_SUBDIR_3900 => {
                    let subdir_offset = value_offset as usize;
                    let subdir_size = count as usize;
                    if subdir_offset + subdir_size <= data.len() {
                        let subdir_data = &data[subdir_offset..subdir_offset + subdir_size];
                        self.parse_leica4_subdirectory(subdir_data, data, byte_order, tags);
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Parse a Leica4 subdirectory (0x3000, 0x3100, 0x3400, or 0x3900)
    fn parse_leica4_subdirectory(
        &self,
        subdir_data: &[u8],
        full_data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        if subdir_data.len() < 2 {
            return;
        }

        let reader = EndianReader::new(subdir_data, byte_order.to_io_byte_order());
        let entry_count = reader.u16_at(0).unwrap_or(0);

        if entry_count == 0 || entry_count > 100 {
            return;
        }

        let required_size = 2 + (entry_count as usize * 12);
        if subdir_data.len() < required_size {
            return;
        }

        for i in 0..entry_count {
            let entry_offset = 2 + (i as usize * 12);
            if entry_offset + 12 > subdir_data.len() {
                break;
            }

            let entry_data = &subdir_data[entry_offset..entry_offset + 12];
            let entry_reader = EndianReader::new(entry_data, byte_order.to_io_byte_order());

            let tag_id = entry_reader.u16_at(0).unwrap_or(0);
            let format = entry_reader.u16_at(2).unwrap_or(0);
            let count = entry_reader.u32_at(4).unwrap_or(0);
            let value_offset = entry_reader.u32_at(8).unwrap_or(0);

            match tag_id {
                l4_subdir::CONTRAST => {
                    let value = value_offset as i32;
                    tags.insert(
                        "Leica:Contrast".to_string(),
                        L4_DECODE_CONTRAST.decode(value),
                    );
                }
                l4_subdir::SHARPENING => {
                    let value = value_offset as i32;
                    tags.insert(
                        "Leica:Sharpening".to_string(),
                        L4_DECODE_SHARPENING.decode(value),
                    );
                }
                l4_subdir::SATURATION => {
                    let value = value_offset as i32;
                    tags.insert(
                        "Leica:Saturation".to_string(),
                        L4_DECODE_SATURATION.decode(value),
                    );
                }
                l4_subdir::WHITE_BALANCE => {
                    let value = value_offset as i32;
                    tags.insert(
                        "Leica:WhiteBalance".to_string(),
                        L4_DECODE_WHITE_BALANCE.decode(value),
                    );
                }
                l4_subdir::JPEG_QUALITY => {
                    let value = value_offset as i32;
                    tags.insert(
                        "Leica:JPEGQuality".to_string(),
                        L4_DECODE_JPEG_QUALITY.decode(value),
                    );
                }
                l4_subdir::WB_RGB_LEVELS => {
                    // WB RGB Levels are stored as 3 rational values
                    if format == 5 && count == 3 {
                        // Read rational values from offset
                        let offset = value_offset as usize;
                        if offset + 24 <= full_data.len() {
                            let wb_reader = EndianReader::new(
                                &full_data[offset..],
                                byte_order.to_io_byte_order(),
                            );
                            let r_num = wb_reader.u32_at(0).unwrap_or(0);
                            let r_den = wb_reader.u32_at(4).unwrap_or(1);
                            let g_num = wb_reader.u32_at(8).unwrap_or(0);
                            let g_den = wb_reader.u32_at(12).unwrap_or(1);
                            let b_num = wb_reader.u32_at(16).unwrap_or(0);
                            let b_den = wb_reader.u32_at(20).unwrap_or(1);
                            let round10 = |num: u32, den: u32| -> Option<String> {
                                (den > 0).then(|| {
                                    let value = f64::from(num) / f64::from(den);
                                    let formatted = format!("{value:.10}");
                                    let trimmed =
                                        formatted.trim_end_matches('0').trim_end_matches('.');
                                    if trimmed.is_empty() {
                                        "0".to_string()
                                    } else {
                                        trimmed.to_string()
                                    }
                                })
                            };
                            if let (Some(r), Some(g), Some(b)) = (
                                round10(r_num, r_den),
                                round10(g_num, g_den),
                                round10(b_num, b_den),
                            ) {
                                // `Name => 'WB_RGBLevels'` (Panasonic.pm:1839),
                                // not the underscore-less "WBRGBLevels" this
                                // used to print.
                                tags.insert(
                                    "Leica:WB_RGBLevels".to_string(),
                                    format!("{r} {g} {b}"),
                                );
                            }
                        }
                    }
                }
                l4_subdir::USER_PROFILE => {
                    if let Some(s) = l4_string(format, count, value_offset, full_data, byte_order) {
                        tags.insert("Leica:UserProfile".to_string(), s);
                    }
                }
                l4_subdir::SERIAL_NUMBER => {
                    // `%Panasonic::Leica4` 0x3103 (Panasonic.pm:1862) is
                    // `Name => 'SerialNumber', Writable => 'string'` with no PrintConv,
                    // so ExifTool prints the stored string verbatim. This used to
                    // overwrite it with `"*".repeat(len.min(7))` under the comment
                    // "ExifTool masks serial numbers with asterisks", which ExifTool
                    // does not do anywhere. The claim looked true only because the one
                    // M9 sample on hand has a serial that was redacted to "*******" in
                    // the file itself; on LeicaM_Monochrom.jpg ExifTool prints `5`.
                    if let Some(s) = l4_string(format, count, value_offset, full_data, byte_order) {
                        tags.insert("Leica:SerialNumber".to_string(), s);
                    }
                }
                l4_subdir::FIRMWARE_VERSION => {
                    if let Some(s) = l4_string(format, count, value_offset, full_data, byte_order) {
                        tags.insert("Leica:FirmwareVersion".to_string(), s);
                    }
                }
                l4_subdir::BASE_ISO => {
                    tags.insert("Leica:BaseISO".to_string(), format!("{}", value_offset));
                }
                l4_subdir::SENSOR_WIDTH => {
                    tags.insert("Leica:SensorWidth".to_string(), format!("{}", value_offset));
                }
                l4_subdir::SENSOR_HEIGHT => {
                    tags.insert(
                        "Leica:SensorHeight".to_string(),
                        format!("{}", value_offset),
                    );
                }
                l4_subdir::SENSOR_BIT_DEPTH => {
                    tags.insert(
                        "Leica:SensorBitDepth".to_string(),
                        format!("{}", value_offset),
                    );
                }
                l4_subdir::CAMERA_TEMPERATURE => {
                    let value = value_offset as i32;
                    tags.insert(
                        "Leica:CameraTemperature".to_string(),
                        format!("{} C", value),
                    );
                }
                l4_subdir::LENS_TYPE => {
                    let printed = leica_lenses::lookup(value_offset)
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            format!("Unknown ({})", leica_lenses::value_conv(value_offset))
                        });
                    tags.insert("Leica:LensType".to_string(), printed);
                }
                l4_subdir::APPROXIMATE_F_NUMBER => {
                    // Stored as rational64u
                    if format == 5 {
                        let offset = value_offset as usize;
                        if offset + 8 <= full_data.len() {
                            let f_reader = EndianReader::new(
                                &full_data[offset..],
                                byte_order.to_io_byte_order(),
                            );
                            let num = f_reader.u32_at(0).unwrap_or(0);
                            let den = f_reader.u32_at(4).unwrap_or(1);
                            if den > 0 {
                                let f_value = num as f64 / den as f64;
                                tags.insert(
                                    "Leica:ApproximateFNumber".to_string(),
                                    format!("{:.1}", f_value),
                                );
                            }
                        }
                    }
                }
                // int32s fitting in the entry's own four bytes, so no offset is
                // involved. ValueConv `$val / 1e5`, PrintConv `sprintf("%.2f")`:
                // LeicaM9.jpg stores 691968 and ExifTool prints 6.92.
                l4_subdir::MEASURED_LV => {
                    let lv = f64::from(value_offset as i32) / 1e5;
                    tags.insert("Leica:MeasuredLV".to_string(), format!("{:.2}", lv));
                }
                l4_subdir::EXTERNAL_SENSOR_BRIGHTNESS => {
                    let ev = f64::from(value_offset as i32) / 1e5;
                    tags.insert(
                        "Leica:ExternalSensorBrightnessValue".to_string(),
                        format!("{:.2}", ev),
                    );
                }
                _ => {
                    // Unknown tags are silently skipped
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leica_header_validation() {
        // Valid short LEICA header
        let valid_short = b"LEICA\0\0\0extra data";
        assert!(is_leica_makernote(valid_short));

        // "LEICA CAMERA AG" is NOT this parser's to claim. ExifTool's
        // `MakerNoteLeica10` matches `^LEICA CAMERA AG\0` and routes it to
        // `Panasonic::Main` (MakerNotes.pm:724-731), so the Panasonic parser
        // decodes it and this one must decline -- it has no table for those
        // tag ids and previously returned true only to emit nothing.
        let leica10 = b"LEICA CAMERA AG\0\0\0\x9d\0\x01\0\x03\0";
        assert!(!is_leica_makernote(leica10));
        assert!(crate::parsers::tiff::makernotes::panasonic::is_leica10_makernote(leica10));

        // The bare prefix without the terminating NUL is not a Leica10 header
        // either, and is still not a Leica layout.
        let not_leica10 = b"LEICA CAMERA AG extra data";
        assert!(!is_leica_makernote(not_leica10));

        // Invalid header
        let invalid = b"CANON\0\x00\x00\x00\x00\x00\x00";
        assert!(!is_leica_makernote(invalid));

        // Too short
        let too_short = b"LEICA\0";
        assert!(!is_leica_makernote(too_short));

        // Valid IFD entry count (must be at least 8 bytes for minimal validation)
        let valid_ifd = b"\x0A\x00\x00\x00\x00\x00\x00\x00"; // 10 entries + padding
        assert!(is_leica_makernote(valid_ifd));

        // Invalid IFD entry count (too many entries)
        let invalid_ifd = b"\xFF\x00\x00\x00\x00\x00\x00\x00"; // 255 entries - too many
        assert!(!is_leica_makernote(invalid_ifd));
    }

    // `%Panasonic::Leica2` 0x0300 `Quality` (Panasonic.pm:1608-1614): only
    // Fine/Basic are defined; ExifTool would print an undefined value as the
    // raw number, which `Decode::Unknown` on a two-entry table also does.
    #[test]
    fn test_leica2_decode_quality() {
        assert_eq!(LEICA2_DECODE_QUALITY.decode(1), "Fine");
        assert_eq!(LEICA2_DECODE_QUALITY.decode(2), "Basic");
        assert_eq!(LEICA2_DECODE_QUALITY.decode(99), "Unknown (99)");
    }

    // `%Panasonic::Leica2` 0x0302 `UserProfile` (Panasonic.pm:1616-1624).
    #[test]
    fn test_leica2_decode_user_profile() {
        assert_eq!(LEICA2_DECODE_USER_PROFILE.decode(1), "User Profile 1");
        assert_eq!(
            LEICA2_DECODE_USER_PROFILE.decode(4),
            "User Profile 0 (Dynamic)"
        );
        assert_eq!(LEICA2_DECODE_USER_PROFILE.decode(99), "Unknown (99)");
    }

    // `%Panasonic::Leica2` 0x0304 `WhiteBalance` (Panasonic.pm:1626-1638) and
    // its `WhiteBalanceConv` Kelvin fallback (Panasonic.pm:2731-2739): the
    // M8's ground-truth `Auto or Manual` value is 0, verified against
    // `LeicaM8.jpg`.
    #[test]
    fn test_leica2_decode_white_balance() {
        assert_eq!(leica2_decode_white_balance(0), "Auto or Manual");
        assert_eq!(leica2_decode_white_balance(10), "Cloudy");
        assert_eq!(leica2_decode_white_balance(11), "Shade");
        assert_eq!(leica2_decode_white_balance(0x8000 + 5500), "5500 Kelvin");
        assert_eq!(leica2_decode_white_balance(7), "Unknown (7)");
    }

    // `%Panasonic::Leica9` 0x0359 `ISOSelected` (Panasonic.pm:2219-2226):
    // ground-truth `Auto` on `LeicaM10-R.jpg`, a raw ISO on `LeicaM11.jpg`.
    #[test]
    fn test_leica9_decode_iso_selected() {
        assert_eq!(leica9_decode_iso_selected(0), "Auto");
        assert_eq!(leica9_decode_iso_selected(100), "100");
    }

    #[test]
    fn test_parser_trait_implementation() {
        let parser = LeicaMakerNoteParser;
        assert_eq!(parser.manufacturer_name(), "Leica");
        assert_eq!(parser.tag_prefix(), "Leica:");
    }

    // The M-Monochrom (Typ 246)/M (Typ 262) header ExifTool's
    // `MakerNoteLeica7` matches (MakerNotes.pm:690-701), routing to
    // `%Panasonic::Leica6` -- previously unrecognised by this parser.
    #[test]
    fn test_leica6_header_recognised() {
        let data = b"LEICA\0\x02\xff\0\0\0\0";
        assert!(is_leica_makernote(data));
        assert_eq!(leica_layout(data), Some(LeicaLayout::Leica6));
    }

    // The T/TL/TL2/X-series/Q-series/SL-series header ExifTool's
    // `MakerNoteLeica5`/`MakerNoteLeica8` match (MakerNotes.pm:650-712),
    // routing to `%Panasonic::Leica5` -- previously unrecognised by this
    // parser (`LeicaT.jpg`, `LeicaTL.jpg`, `LeicaTL2.jpg` all use \x06).
    #[test]
    fn test_leica5_header_recognised() {
        for second_byte in [0x01u8, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x10, 0x1a] {
            let data = [b'L', b'E', b'I', b'C', b'A', 0, second_byte, 0];
            assert_eq!(
                leica_layout(&data),
                Some(LeicaLayout::Leica5),
                "second byte {second_byte:#04x} should resolve to Leica5"
            );
        }
    }

    // `LeicaM10-R.jpg`'s M10 header must keep resolving to Leica9 and not be
    // shadowed by the new Leica5/Leica6 detection.
    #[test]
    fn test_leica9_header_still_recognised() {
        let data = b"LEICA\0\x02\0\0\0\0\0";
        assert_eq!(leica_layout(data), Some(LeicaLayout::Leica9));
    }
}
