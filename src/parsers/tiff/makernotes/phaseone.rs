//! Phase One MakerNote Parser
//!
//! Parses Phase One's proprietary MakerNote directory structure, found in
//! `.IIQ` raw files and in EXIF-JPEG previews written by Phase One/Leaf
//! digital backs. This is a rewrite driven by an audit of the tag table and
//! reachability against real ExifTool (`PhaseOne.pm`, `ProcessPhaseOne`) --
//! see the module docs on `registries::phaseone` for the tag-table defects
//! this replaced.
//!
//! ## Wire format (`ProcessPhaseOne`, PhaseOne.pm:596)
//!
//! This is *not* a standard TIFF IFD, so it cannot be read with the shared
//! `parse_ifd_entries` 12-byte-entry walker (that assumption made the old
//! parser's field offsets subtly wrong even where its tag IDs were right).
//! Layout, relative to `dir_start` (0 for the top-level `Main` table):
//!
//! ```text
//! dir_start +  0..4   4-byte repeated signature: "IIII" or "MMMM"
//!           +  4      1 version byte (Main table only; wildcard)
//!           +  5..8   "waR" (II) or "Raw" (MM) (Main); or, for the nested
//!                      SensorCalibration table (12-byte entries), the whole
//!                      8-byte header is instead the literal
//!                      "IIII\x01\0\0\0" / "MMMM\0\0\0\x01"
//!           +  8..12  u32 ifd_start (offset from dir_start to entry count)
//! dir_start + ifd_start        u32 num_entries (2..=300)
//! dir_start + ifd_start + 8    first entry
//! ```
//!
//! Each entry is `entry_size` bytes (16 in `Main`, 12 in
//! `SensorCalibration`, which has no per-entry format field):
//!
//! ```text
//! entry +  0..4                     u32 tag_id
//! entry +  4..8                     u32 format code (Main only; ExifTool
//!                                    overrides this with the tag's declared
//!                                    Format whenever the tag is known, so
//!                                    oxidex -- which only ever emits known
//!                                    tags -- never needs to decode it)
//! entry + entry_size-8..entry_size-4   u32 size (bytes)
//! entry + entry_size-4..entry_size     u32 value, or (if size > 4) a u32
//!                                       offset from dir_start to the value
//! ```
//!
//! The byte order for a directory is read fresh from its own 2-byte
//! signature prefix rather than trusted from the caller, exactly as
//! `ProcessPhaseOne` calls `SetByteOrder` per-directory.
//!
//! ## Dispatch
//!
//! ExifTool's `MakerNotePhaseOne` entry in `MakerNotes.pm` matches on this
//! signature alone (`/^(IIII.waR|MMMMRaw.)/`), independent of the `Make`
//! tag -- Phase One backs are OEMed and rebranded (this format also
//! appears under `Make: Leaf`, since Leaf was acquired by Phase One and
//! kept writing the same directory shape). `makernote_dispatcher.rs`
//! checks for this signature before falling through to the `Make`-keyed
//! table, so a `Leaf`-branded file carrying this signature reaches this
//! parser regardless of brand. (Previously it reached nothing at all --
//! dispatch matched only `"phase one"`/`"phase one a/s"`, so `Make=="Leaf"`
//! silently produced zero `PhaseOne:` tags for real Leaf/Phase One `.IIQ`
//! files; a since-deleted fabricated Leaf parser briefly misrouted the same
//! files and failed with `Invalid entry count: 18761`, reading Phase One's
//! bespoke header as a standard TIFF IFD entry count.)

use crate::core::formatters::numeric_precision::{perl_g, perl_number};
use crate::io::{ByteOrder as EndianOrder, EndianReader};
use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

use super::registries::phaseone::{phaseone_tag_name, sensor_calibration_tag_name};
use super::shared::MakerNoteParser;
use crate::core::formatters::exif_print_conv::print_exposure_time;

/// `PrintConv` for `CameraOrientation` (0x0100, PhaseOne.pm:36).
/// Raw value is masked with `0x03` before lookup.
pub fn decode_camera_orientation(val: i32) -> &'static str {
    match val & 0x03 {
        0 => "Horizontal (normal)",
        1 => "Rotate 90 CW",
        2 => "Rotate 270 CW",
        3 => "Rotate 180",
        _ => unreachable!("val & 0x03 is always 0..=3"),
    }
}

/// `PrintConv` for `RawFormat` (0x010e, PhaseOne.pm:68). Unmapped values
/// print as the bare number, matching ExifTool's default hash `PrintConv`
/// fallback.
fn decode_raw_format(val: i32) -> String {
    match val {
        0 => "Uncompressed".to_string(),
        1 => "RAW 1".to_string(),
        2 => "RAW 2".to_string(),
        3 => "IIQ L".to_string(),
        5 => "IIQ S".to_string(),
        6 => "IIQ Sv2".to_string(),
        8 => "IIQ L16".to_string(),
        other => other.to_string(),
    }
}

/// `PrintConv` for `SequenceKind` (0x0263, PhaseOne.pm:197).
fn decode_sequence_kind(val: i32) -> String {
    match val {
        0 => "Bracketing: Shutter Speed".to_string(),
        1 => "Bracketing: Aperture".to_string(),
        2 => "Bracketing: ISO".to_string(),
        3 => "Hyperfocal".to_string(),
        4 => "Time Lapse".to_string(),
        5 => "HDR".to_string(),
        6 => "Focus Stacking".to_string(),
        other => other.to_string(),
    }
}

/// Shared `ValueConv`+`PrintConv` for `ApertureValue` (0x0401),
/// `MaxApertureValue` (0x0414) and `MinApertureValue` (0x0415)
/// (PhaseOne.pm:223,251,259): `2 ** (val / 2)`, printed `"%.1f"`.
fn print_apex_aperture(raw: f64) -> String {
    format!("{:.1}", 2f64.powf(raw / 2.0))
}

/// `ConvertUnixTime` + `ConvertDateTime`: seconds since the epoch, UTC, as
/// `YYYY:MM:DD HH:MM:SS`. Used for `DateTimeOriginal` (0x0112).
fn format_unix_time(epoch_seconds: i64) -> String {
    let days = epoch_seconds.div_euclid(86_400);
    let secs = epoch_seconds.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant's algorithm), shifted to a March-based year.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        y,
        m,
        d,
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// Reads a NUL-trimmed ASCII/UTF-8 string from a value's raw bytes.
fn read_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

/// Reads a single `int32s`-ish scalar (PhaseOne's default `Format`) from a
/// value's raw bytes, using whatever byte order the entry's directory has.
fn read_i32(bytes: &[u8], order: EndianOrder) -> Option<i32> {
    let reader = EndianReader::new(bytes, order);
    reader.i32_at(0)
}

/// Reads a single `float` scalar from a value's raw bytes.
fn read_f32(bytes: &[u8], order: EndianOrder) -> Option<f32> {
    let reader = EndianReader::new(bytes, order);
    reader.f32_at(0)
}

/// Reads every `float` element packed into a value's raw bytes.
fn read_f32_array(bytes: &[u8], order: EndianOrder) -> Vec<f32> {
    let reader = EndianReader::new(bytes, order);
    (0..bytes.len() / 4)
        .filter_map(|i| reader.f32_at(i * 4))
        .collect()
}

/// Formats a "(Binary data N bytes, use -b option to extract)" placeholder,
/// matching ExifTool's default handling of `Binary => 1` tags.
fn binary_placeholder(size: usize) -> String {
    format!("(Binary data {} bytes, use -b option to extract)", size)
}

/// Decodes a `Binary => 1` tag whose declared `Format` is numeric (not
/// `undef`) into the space-joined number string ExifTool's `ReadValue`
/// would produce, so its length can stand in for the "N bytes" a
/// `binary_placeholder` reports for such tags -- see the 0x021c/0x0223
/// call sites for why this string, not the source byte count, is correct.
fn binary_int_array_string(bytes: &[u8], order: EndianOrder, unsigned_16: bool) -> String {
    let reader = EndianReader::new(bytes, order);
    if unsigned_16 {
        (0..bytes.len() / 2)
            .filter_map(|i| reader.u16_at(i * 2))
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        (0..bytes.len() / 4)
            .filter_map(|i| reader.i32_at(i * 4))
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Formats one `PhaseOne::Main` tag's value from its raw bytes.
///
/// Returns `None` for `SensorCalibration` (0x0110), which is a
/// `SubDirectory` handled by recursion in [`parse_phaseone_dir`] rather
/// than emitted as a scalar tag.
fn format_main_value(tag_id: u32, bytes: &[u8], order: EndianOrder) -> Option<String> {
    match tag_id {
        0x0100 => read_i32(bytes, order).map(|v| decode_camera_orientation(v).to_string()),
        0x0102 => Some(read_string(bytes)),
        0x0105 | 0x0108 | 0x0109 | 0x010a | 0x010b | 0x010c | 0x010d | 0x0113 | 0x021d | 0x0222
        | 0x0264 | 0x0265 => read_i32(bytes, order).map(|v| v.to_string()),
        0x0106 | 0x0226 => {
            let vals = read_f32_array(bytes, order);
            if vals.is_empty() {
                None
            } else {
                Some(
                    vals.iter()
                        .map(|v| format!("{:.3}", v))
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            }
        }
        0x0107 => {
            let vals = read_f32_array(bytes, order);
            if vals.is_empty() {
                None
            } else {
                Some(
                    vals.iter()
                        .map(|v| perl_number(*v as f64))
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            }
        }
        0x010e => read_i32(bytes, order).map(|v| decode_raw_format(v)),
        // RawData has an explicit `Format => 'undef'` (PhaseOne.pm:86), so
        // ExifTool's Binary placeholder reports the true byte count.
        0x010f => Some(binary_placeholder(bytes.len())),
        0x0110 => None, // SubDirectory, recursed into separately
        0x0112 => read_i32(bytes, order).map(|v| format_unix_time(v as i64)),
        0x0203 | 0x0204 | 0x0262 | 0x0301 | 0x0410 | 0x0412 | 0x0455 => Some(read_string(bytes)),
        0x0210 | 0x0211 => read_f32(bytes, order).map(|v| format!("{:.2} C", v)),
        // StripOffsets has NO explicit Format (PhaseOne.pm:144), so it
        // defaults to the table's `int32s`; ExifTool decodes it to a
        // space-joined number string *before* checking `Binary`, so the
        // placeholder's byte count is the printed string's length, not the
        // source byte count -- see the 0x0223 comment below for the same
        // quirk, verified byte-for-byte against `exiftool -v3`.
        0x021c => Some(binary_placeholder(
            binary_int_array_string(bytes, order, false).len(),
        )),
        // BlackLevelData has `Format => 'int16u'` (PhaseOne.pm:150): ExifTool
        // still runs it through `ReadValue` (producing "25660 28021 ...")
        // before the generic Binary-tag display collapses that already-
        // decoded string to "(Binary data N bytes...)" -- N is
        // `length($value)` of the decoded text, not the 22 raw source
        // bytes. Confirmed against `exiftool -v3`: the fixture's 11
        // int16u values print as a 65-character string.
        0x0223 => Some(binary_placeholder(
            binary_int_array_string(bytes, order, true).len(),
        )),
        0x0263 => read_i32(bytes, order).map(|v| decode_sequence_kind(v)),
        0x0267 => read_f32(bytes, order).map(|v| perl_number(v as f64)),
        0x0400 => read_f32(bytes, order).map(|v| {
            let raw = v as f64;
            let secs = if raw.abs() < 100.0 {
                2f64.powf(-raw)
            } else {
                0.0
            };
            print_exposure_time(secs)
        }),
        0x0401 | 0x0414 | 0x0415 => read_f32(bytes, order).map(|v| print_apex_aperture(v as f64)),
        0x0402 => read_f32(bytes, order).map(|v| format!("{:.3}", v)),
        0x0403 => read_f32(bytes, order).map(|v| format!("{:.1} mm", v)),
        _ => None,
    }
}

/// Formats one `PhaseOne::SensorCalibration` tag's value from its raw bytes.
fn format_sensor_calibration_value(
    tag_id: u32,
    bytes: &[u8],
    order: EndianOrder,
) -> Option<String> {
    match tag_id {
        0x0400 => Some(binary_placeholder(bytes.len())),
        0x0407 => Some(read_string(bytes)),
        0x0419 | 0x041a => {
            let vals = read_f32_array(bytes, order);
            if vals.is_empty() {
                None
            } else {
                Some(
                    vals.iter()
                        .map(|v| perl_g(*v as f64, 5))
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            }
        }
        _ => None,
    }
}

/// Phase One's directory-version signature bytes, checked at `dir_start`.
///
/// Mirrors `ProcessPhaseOne`'s two-branch validation (PhaseOne.pm:617-622):
/// entries with a per-entry format field (`entry_size == 16`, the `Main`
/// table) use the text signature `IIII.waR` / `MMMMRaw.`; entries without
/// one (`entry_size == 12`, `SensorCalibration`) use a fixed 8-byte marker.
fn header_signature_ok(hdr: &[u8; 8], entry_size: usize) -> bool {
    if entry_size == 16 {
        (hdr[0..4] == *b"IIII" && hdr[5..8] == *b"waR")
            || (hdr[0..4] == *b"MMMM" && hdr[4..7] == *b"Raw")
    } else {
        hdr == b"IIII\x01\0\0\0" || hdr == b"MMMM\0\0\0\x01"
    }
}

/// Phase One MakerNote header/directory signatures this parser recognises.
///
/// `ExifTool`'s `MakerNotePhaseOne` `Condition` (MakerNotes.pm:844) is
/// `/^(IIII.waR|MMMMRaw.)/`, checked against the MakerNote value's own
/// first bytes -- independent of `Make`.
pub fn is_phaseone_makernote(data: &[u8]) -> bool {
    data.len() >= 8 && header_signature_ok(data[0..8].try_into().unwrap(), 16)
}

/// Recursively walks one Phase One directory (`Main` or
/// `SensorCalibration`), inserting `"{prefix}:{Name}"` for every tag whose
/// value this parser knows how to format.
///
/// `dir_start` is relative to `data` (0 for the top-level call; the
/// resolved value offset of a `SubDirectory` tag for a recursive call --
/// `SensorCalibration` is addressed by offsets into the *same* buffer as
/// `Main`, exactly as `ProcessPhaseOne` recurses with the same `$dataPt`
/// and a new `$dirStart`).
#[allow(clippy::too_many_arguments)]
fn parse_phaseone_dir(
    data: &[u8],
    dir_start: usize,
    entry_size: usize,
    prefix: &str,
    tag_name: fn(u32) -> Option<&'static str>,
    format_value: fn(u32, &[u8], EndianOrder) -> Option<String>,
    tags: &mut HashMap<String, String>,
    depth: u32,
) -> Result<(), String> {
    // ProcessPhaseOne doesn't recurse arbitrarily deep -- Main only ever
    // points at SensorCalibration -- but guard against a corrupt/adversarial
    // 0x0110 offset pointing back into itself.
    if depth > 4 {
        return Err("Phase One directory nesting too deep".to_string());
    }

    let dir_len = data.len().saturating_sub(dir_start);
    if dir_len < 12 {
        return Err("Phase One directory too short".to_string());
    }

    let hdr: [u8; 8] = data[dir_start..dir_start + 8].try_into().unwrap();
    if !header_signature_ok(&hdr, entry_size) {
        return Err(format!(
            "Unrecognized PhaseOne directory version at {dir_start:#x}"
        ));
    }
    let order = if hdr[0] == b'M' {
        EndianOrder::Big
    } else {
        EndianOrder::Little
    };
    let reader = EndianReader::new(data, order);

    let ifd_start = reader
        .u32_at(dir_start + 8)
        .ok_or_else(|| "Phase One directory header truncated".to_string())?
        as usize;
    if ifd_start + 8 > dir_len {
        return Err("Phase One IFD offset out of range".to_string());
    }
    let num_entries = reader
        .u32_at(dir_start + ifd_start)
        .ok_or_else(|| "Failed to read Phase One entry count".to_string())?
        as usize;
    if !(2..=300).contains(&num_entries) {
        return Err(format!(
            "Invalid Phase One entry count: {num_entries} (expected 2-300)"
        ));
    }
    let ifd_end = ifd_start + 8 + entry_size * num_entries;
    if ifd_end > dir_len {
        return Err("Phase One directory entries run past the end of the buffer".to_string());
    }

    let entries_start = dir_start + ifd_start + 8;
    for i in 0..num_entries {
        let entry = entries_start + i * entry_size;
        let Some(tag_id) = reader.u32_at(entry) else {
            break;
        };
        let Some(size) = reader.u32_at(entry + entry_size - 8) else {
            break;
        };
        let value_field = entry + entry_size - 4;
        let size = size as usize;

        let value_bytes: &[u8] = if size > 4 {
            let Some(value_offset) = reader.u32_at(value_field) else {
                continue;
            };
            let absolute = dir_start + value_offset as usize;
            match data.get(absolute..absolute.saturating_add(size)) {
                Some(bytes) => bytes,
                None => continue, // offset out of range; skip this entry like ExifTool's Warn-and-return
            }
        } else {
            match data.get(value_field..value_field + size) {
                Some(bytes) => bytes,
                None => continue,
            }
        };

        // SensorCalibration (0x0110) is a SubDirectory: recurse into it
        // instead of emitting a scalar tag. Only meaningful when the value
        // was offset-addressed (size > 4); a same-buffer directory can't
        // fit in 4 inline bytes.
        if prefix == "PhaseOne" && tag_id == 0x0110 && size > 4 {
            if let Some(value_offset) = reader.u32_at(value_field) {
                let sub_dir_start = dir_start + value_offset as usize;
                // A malformed/adversarial offset shouldn't abort the whole
                // directory -- best-effort recurse, ignore failures.
                let _ = parse_phaseone_dir(
                    data,
                    sub_dir_start,
                    12,
                    "PhaseOne", // SensorCalibration has no GROUPS override; still family-1 PhaseOne
                    sensor_calibration_tag_name,
                    format_sensor_calibration_value,
                    tags,
                    depth + 1,
                );
            }
            continue;
        }

        let Some(name) = tag_name(tag_id) else {
            continue; // Unknown to this table, or an Unknown/Hidden-flagged tag we don't surface
        };
        if let Some(formatted) = format_value(tag_id, value_bytes, order) {
            tags.insert(format!("{prefix}:{name}"), formatted);
        }
    }

    Ok(())
}

/// Phase One MakerNote Parser
pub struct PhaseOneMakerNoteParser;

impl MakerNoteParser for PhaseOneMakerNoteParser {
    fn manufacturer_name(&self) -> &'static str {
        "PhaseOne"
    }

    fn tag_prefix(&self) -> &'static str {
        "PhaseOne:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        is_phaseone_makernote(data)
    }

    fn parse(
        &self,
        data: &[u8],
        _byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        // Byte order is re-derived per-directory from the signature itself
        // (ProcessPhaseOne calls SetByteOrder from the header), so the
        // caller's byte_order is unused -- kept for MakerNoteParser trait
        // compatibility.
        parse_phaseone_dir(
            data,
            0,
            16,
            "PhaseOne",
            phaseone_tag_name,
            format_main_value,
            tags,
            0,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::corpus_fixture;

    /// Builds a minimal synthetic Phase One `Main` directory: header +
    /// entry table, entries appended in declaration order. Each entry is
    /// `(tag_id, format_code, inline_or_offset_value)`; values that need
    /// out-of-line bytes are appended after the entry table and referenced
    /// by offset from `dir_start` (0 here).
    fn build_dir(entries: &[(u32, u32, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"IIIICwaR"); // signature, byte 4 ('C') is a wildcard version char
        let ifd_start: u32 = 12; // header is exactly 12 bytes
        out.extend_from_slice(&ifd_start.to_le_bytes());
        assert_eq!(out.len(), 12);

        out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        out.extend_from_slice(&[0u8; 4]); // unused padding word

        let entries_start = out.len();
        let mut tail = Vec::new();
        let table_end_offset = entries_start + entries.len() * 16;

        for (tag_id, format_code, value) in entries {
            out.extend_from_slice(&tag_id.to_le_bytes());
            out.extend_from_slice(&format_code.to_le_bytes());
            out.extend_from_slice(&(value.len() as u32).to_le_bytes());
            if value.len() <= 4 {
                let mut inline = [0u8; 4];
                inline[..value.len()].copy_from_slice(value);
                out.extend_from_slice(&inline);
            } else {
                let value_offset = (table_end_offset + tail.len()) as u32;
                out.extend_from_slice(&value_offset.to_le_bytes());
                tail.extend_from_slice(value);
            }
        }
        out.extend_from_slice(&tail);
        out
    }

    #[test]
    fn test_is_phaseone_makernote_valid_signature() {
        let data = build_dir(&[(0x0105, 4, 100i32.to_le_bytes().to_vec())]);
        assert!(is_phaseone_makernote(&data));
    }

    #[test]
    fn test_is_phaseone_makernote_rejects_generic_data() {
        // The old heuristic accepted any plausible-looking entry count;
        // real files never start with the literal "Phase One" text this
        // parser used to require either. Neither should validate now.
        assert!(!is_phaseone_makernote(b"Phase One\x00\x01\x02"));
        assert!(!is_phaseone_makernote(&[0x05, 0x00]));
        assert!(!is_phaseone_makernote(&[]));
    }

    #[test]
    fn test_parses_iso_and_camera_orientation() {
        let data = build_dir(&[
            (0x0100, 4, 0i32.to_le_bytes().to_vec()), // CameraOrientation
            (0x0105, 4, 100i32.to_le_bytes().to_vec()), // ISO
        ]);
        let mut tags = HashMap::new();
        PhaseOneMakerNoteParser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .unwrap();
        assert_eq!(
            tags.get("PhaseOne:CameraOrientation").map(String::as_str),
            Some("Horizontal (normal)")
        );
        assert_eq!(tags.get("PhaseOne:ISO").map(String::as_str), Some("100"));
    }

    #[test]
    fn test_0x0211_is_sensor_temperature2_not_lens_id() {
        // Regression: the old registry mapped 0x0211 to a fabricated
        // "LensID" tag. It's SensorTemperature2 (float, PhaseOne.pm:123).
        // ProcessPhaseOne requires >= 2 entries; pad with an unknown tag.
        let data = build_dir(&[
            (0x0211, 4, 37.0f32.to_le_bytes().to_vec()),
            (0x9999, 4, 0i32.to_le_bytes().to_vec()),
        ]);
        let mut tags = HashMap::new();
        PhaseOneMakerNoteParser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .unwrap();
        assert!(!tags.contains_key("PhaseOne:LensID"));
        assert_eq!(
            tags.get("PhaseOne:SensorTemperature2").map(String::as_str),
            Some("37.00 C")
        );
    }

    #[test]
    fn test_invented_lens_tags_never_appear() {
        // 0x0213/0x0214/0x0215 aren't PhaseOne::Main tags at all.
        let data = build_dir(&[
            (0x0213, 4, 96i32.to_le_bytes().to_vec()),
            (0x0214, 4, 800i32.to_le_bytes().to_vec()),
            (0x0215, 4, 500i32.to_le_bytes().to_vec()),
        ]);
        let mut tags = HashMap::new();
        PhaseOneMakerNoteParser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .unwrap();
        assert!(tags.is_empty());
    }

    #[test]
    fn test_0x0412_is_lens_model_string_not_white_balance() {
        let mut name = b"Mamiya LS 80mm f/2.8 D".to_vec();
        name.push(0);
        let data = build_dir(&[(0x0412, 1, name), (0x9999, 4, 0i32.to_le_bytes().to_vec())]);
        let mut tags = HashMap::new();
        PhaseOneMakerNoteParser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .unwrap();
        assert!(!tags.contains_key("PhaseOne:WhiteBalance"));
        assert_eq!(
            tags.get("PhaseOne:LensModel").map(String::as_str),
            Some("Mamiya LS 80mm f/2.8 D")
        );
    }

    #[test]
    fn test_focal_length_and_exposure_compensation() {
        // 0x0403 used to be registered as "Aperture"; it's FocalLength.
        // 0x0402 used to be "ShutterSpeed"; it's ExposureCompensation.
        let data = build_dir(&[
            (0x0403, 4, 80.0f32.to_le_bytes().to_vec()),
            (0x0402, 4, (-0.333f32).to_le_bytes().to_vec()),
        ]);
        let mut tags = HashMap::new();
        PhaseOneMakerNoteParser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .unwrap();
        assert_eq!(
            tags.get("PhaseOne:FocalLength").map(String::as_str),
            Some("80.0 mm")
        );
        assert_eq!(
            tags.get("PhaseOne:ExposureCompensation")
                .map(String::as_str),
            Some("-0.333")
        );
        assert!(!tags.contains_key("PhaseOne:Aperture"));
        assert!(!tags.contains_key("PhaseOne:ShutterSpeed"));
    }

    #[test]
    fn test_color_matrix_and_wb_rgb_levels_arrays() {
        let mut color_matrix = Vec::new();
        for v in [1.280f32, -0.280, 0.0] {
            color_matrix.extend_from_slice(&v.to_le_bytes());
        }
        let data = build_dir(&[
            (0x0106, 4, color_matrix),
            (0x9999, 4, 0i32.to_le_bytes().to_vec()),
        ]);
        let mut tags = HashMap::new();
        PhaseOneMakerNoteParser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .unwrap();
        assert_eq!(
            tags.get("PhaseOne:ColorMatrix1").map(String::as_str),
            Some("1.280 -0.280 0.000")
        );
    }

    #[test]
    fn test_binary_tag_placeholder() {
        let data = build_dir(&[
            (0x010f, 5, vec![0u8; 15]), // RawData, Format 'undef'
            (0x9999, 4, 0i32.to_le_bytes().to_vec()),
        ]);
        let mut tags = HashMap::new();
        PhaseOneMakerNoteParser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .unwrap();
        assert_eq!(
            tags.get("PhaseOne:RawData").map(String::as_str),
            Some("(Binary data 15 bytes, use -b option to extract)")
        );
    }

    #[test]
    fn test_shutter_speed_and_aperture_apex_conversions() {
        // ShutterSpeedValue APEX raw log2 for 1/1250s: -log2(1/1250) ~= 10.288.
        let raw_tv = -((1.0f64 / 1250.0).log2()) as f32;
        // ApertureValue APEX raw for f/5.6: 2*log2(5.6) ~= 4.807.
        let raw_av = (2.0 * 5.6f64.log2()) as f32;
        let data = build_dir(&[
            (0x0400, 4, raw_tv.to_le_bytes().to_vec()),
            (0x0401, 4, raw_av.to_le_bytes().to_vec()),
        ]);
        let mut tags = HashMap::new();
        PhaseOneMakerNoteParser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .unwrap();
        assert_eq!(
            tags.get("PhaseOne:ShutterSpeedValue").map(String::as_str),
            Some("1/1250")
        );
        assert_eq!(
            tags.get("PhaseOne:ApertureValue").map(String::as_str),
            Some("5.6")
        );
    }

    #[test]
    fn test_real_iiq_fixture_matches_exiftool() {
        // Ground truth: `exiftool -G1 -s
        // /tmp/oxidex-exiftool-cache/combined-samples/PhaseOne.iiq`, ExifTool
        // 13.55 (byte-identical PhaseOne.pm to the 13.59 corpus checkout).
        let Some(path) = corpus_fixture("PhaseOne.iiq") else {
            return;
        };
        let file = std::fs::read(&path).expect("read pinned PhaseOne.iiq fixture");
        // MakerNote value starts right after the 8-byte TIFF header
        // (PutFirst => 1 places it there); this dummy fixture's directory
        // runs to the end of the small file.
        let mn = &file[8..];
        assert!(is_phaseone_makernote(mn));
        let mut tags = HashMap::new();
        PhaseOneMakerNoteParser
            .parse(mn, ByteOrder::LittleEndian, &mut tags)
            .unwrap();

        assert_eq!(
            tags.get("PhaseOne:LensModel").map(String::as_str),
            Some("Mamiya LS 80mm f/2.8 D")
        );
        assert_eq!(
            tags.get("PhaseOne:SensorWidth").map(String::as_str),
            Some("7372")
        );
        assert_eq!(
            tags.get("PhaseOne:SensorHeight").map(String::as_str),
            Some("5536")
        );
        assert_eq!(
            tags.get("PhaseOne:ImageWidth").map(String::as_str),
            Some("7320")
        );
        assert_eq!(
            tags.get("PhaseOne:ImageHeight").map(String::as_str),
            Some("5484")
        );
        assert_eq!(
            tags.get("PhaseOne:RawFormat").map(String::as_str),
            Some("IIQ L")
        );
        assert_eq!(
            tags.get("PhaseOne:ImageNumber").map(String::as_str),
            Some("1288")
        );
        assert_eq!(tags.get("PhaseOne:ISO").map(String::as_str), Some("100"));
        assert_eq!(
            tags.get("PhaseOne:SensorTemperature").map(String::as_str),
            Some("37.00 C")
        );
        assert_eq!(
            tags.get("PhaseOne:SensorTemperature2").map(String::as_str),
            Some("37.00 C")
        );
        assert_eq!(
            tags.get("PhaseOne:BlackLevel").map(String::as_str),
            Some("1024")
        );
        assert_eq!(
            tags.get("PhaseOne:SplitColumn").map(String::as_str),
            Some("3696")
        );
        assert_eq!(
            tags.get("PhaseOne:ShutterSpeedValue").map(String::as_str),
            Some("1/1250")
        );
        assert_eq!(
            tags.get("PhaseOne:ApertureValue").map(String::as_str),
            Some("5.6")
        );
        assert_eq!(
            tags.get("PhaseOne:FocalLength").map(String::as_str),
            Some("80.0 mm")
        );
        assert_eq!(
            tags.get("PhaseOne:SerialNumber").map(String::as_str),
            Some("LD001055")
        );
        assert_eq!(
            tags.get("PhaseOne:SensorDefects").map(String::as_str),
            Some("(Binary data 12 bytes, use -b option to extract)")
        );
        // ExifTool decodes BlackLevelData's int16u[11] before collapsing it
        // to a Binary placeholder, so "N bytes" is the 65-character decoded
        // string's length, not the tag's 22 raw source bytes.
        assert_eq!(
            tags.get("PhaseOne:BlackLevelData").map(String::as_str),
            Some("(Binary data 65 bytes, use -b option to extract)")
        );
        // Never fabricated: nothing under the old wrong names survives.
        assert!(!tags.contains_key("PhaseOne:LensID"));
        assert!(!tags.contains_key("PhaseOne:LensSerialNumber"));
        assert!(!tags.contains_key("PhaseOne:FocusDistance"));
        assert!(!tags.contains_key("PhaseOne:WhiteBalance"));
        assert!(!tags.contains_key("PhaseOne:Aperture"));
        assert!(!tags.contains_key("PhaseOne:ShutterSpeed"));
    }
}
