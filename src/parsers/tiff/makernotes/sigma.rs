//! Sigma/Foveon MakerNote decoder: the one Sigma tag table.
//!
//! `Image::ExifTool::Sigma::Main` is reached from `MakerNotes.pm`'s
//! `MakerNoteSigma` entry whenever a JPEG- or TIFF-structured file carries a
//! MakerNote (0x927C) whose payload begins `SIGMA` or `FOVEON`. Two callers
//! reach this module for exactly that reason:
//!
//! * a Sigma JPEG, via `core::tiff_helpers::parse_exif_subifd`; and
//! * an X3F, whose `[Sigma]` tags all come from the ExifIFD of the JpgFromRaw
//!   preview -- `SigmaRaw.pm`'s Main NOTES say ExifTool runs its ordinary
//!   JPEG reader over that image. 39 tags on SigmaDP2.x3f.
//!
//! # Why the callers pass a TIFF block rather than the MakerNote bytes
//!
//! A Sigma MakerNote entry stores its value offset relative to the enclosing
//! TIFF header, not to the MakerNote payload, so a decoder handed only the
//! payload cannot resolve any value longer than four bytes -- which is every
//! value on a real file, all of them strings. That is why this takes `tiff`
//! plus the payload's position inside it, and why it is not an implementor of
//! the `MakerNoteParser` trait (whose `parse` receives the payload alone).
//!
//! A registry-driven `SigmaMakerNoteParser` used to sit behind that trait. It
//! disagreed with `Sigma.pm` on 19 ids, started the IFD 8 bytes into the
//! payload rather than 10 (so it read the version word as the entry count),
//! and printed each entry's value *offset* in place of its value. On
//! `t/images/Sigma.jpg` it emitted one tag, `Sigma:Firmware = 59244544`, where
//! ExifTool reports `Firmware = 2.0.4.1642 Release`. It was deleted rather
//! than repaired: two Sigma tables is how the ids drifted apart in the first
//! place.
//!
//! # Version conditioning
//!
//! Most `Sigma.pm` entries are conditional on `$$self{MakerNoteSigmaVer}`,
//! which `MakerNotes.pm:1021` reads out of the MakerNote header:
//! `$$valPt =~ /^SIGMA\0\0\0.(.)/s ? ord($1) : -1`, i.e. the tenth byte. The
//! IFD itself starts at `$valuePtr + 10`. Ids 0x001c-0x0030 mean different
//! things either side of version 3 (the SD1 and the Merrill/Quattro bodies),
//! and this module honours those conditions rather than guessing.

use crate::core::{MetadataMap, TagValue};
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntries, parse_ifd};

/// Header ExifTool matches to recognise a Sigma MakerNote
/// (`MakerNotes.pm` `MakerNoteSigma`, `Validate => '$val =~ /^(SIGMA|FOVEON)/'`).
const SIGMA_HEADER: &[u8] = b"SIGMA\0\0\0";
const FOVEON_HEADER: &[u8] = b"FOVEON\0\0";

/// `Start => '$valuePtr + 10'` - the IFD begins after the 8-byte signature and
/// the two-byte version field.
const IFD_START: usize = 10;

/// TIFF field types used below.
const TYPE_STRING: u16 = 2;
const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
const TYPE_RATIONAL_U: u16 = 5;
const TYPE_UNDEF: u16 = 7;
const TYPE_RATIONAL_S: u16 = 10;

/// Decodes a Sigma MakerNote: a Sigma JPEG's, or that of the preview JPEG
/// inside an X3F.
///
/// * `tiff` - the enclosing TIFF block (offset 0 == TIFF header), which is
///   what MakerNote value offsets are relative to. Verified on SigmaDP2.x3f:
///   entry 0x0002 stores 0x0558 and the string "1008382" sits exactly
///   0x0558 bytes past the TIFF header.
/// * `value_offset` - where the MakerNote payload starts inside `tiff`.
/// * `tiff_base` - absolute file offset of the TIFF header, added to the
///   `IsOffset` tag `PreviewImageStart` the way ExifTool absolutises it.
///
/// Emits nothing when the payload carries no Sigma signature.
pub fn parse_sigma_makernote(
    tiff: &[u8],
    value_offset: usize,
    tiff_base: u64,
    metadata: &mut MetadataMap,
) {
    let Some(payload) = tiff.get(value_offset..) else {
        return;
    };
    if !(payload.starts_with(SIGMA_HEADER) || payload.starts_with(FOVEON_HEADER)) {
        return;
    }
    // `ord($1)` of /^SIGMA\0\0\0.(.)/ - the byte after the signature's ninth.
    let version = payload.get(9).copied().unwrap_or(0);

    let ifd_offset = value_offset + IFD_START;
    let order = entries_order(tiff, ifd_offset);
    let Some(entries) = read_ifd(tiff, ifd_offset, order) else {
        return;
    };

    let mut preview_start: Option<u32> = None;
    let mut preview_length: Option<u32> = None;

    for (tag_id, field_type, count, bytes) in &entries {
        let bytes = bytes.as_ref();
        if let Some((name, value)) = decode_tag(*tag_id, *field_type, *count, bytes, order, version)
        {
            // 0x003b Firmware and 0x003c WhiteBalance are `Priority => 0`
            // duplicates of 0x0017 and 0x0007: ExifTool keeps the first
            // extraction, so an existing value is never overwritten.
            if metadata.get(&format!("MakerNotes:{name}")).is_none() {
                metadata.insert(format!("MakerNotes:{name}"), value);
            }
        }
        match (*tag_id, *field_type) {
            (0x001A, TYPE_LONG) => preview_start = read_u32(bytes, order),
            (0x001B, TYPE_LONG) => preview_length = read_u32(bytes, order),
            _ => {}
        }
    }

    // PreviewImageStart is `IsOffset => 1`: ExifTool reports it relative to
    // the enclosing file, so the TIFF header's own file position is added.
    // Measured on SigmaDP2.x3f: stored 2388, ExifTool prints 2692 (= 2388 +
    // 292 for the JpgFromRaw payload + 12 for the TIFF header inside it).
    if let Some(start) = preview_start {
        metadata.insert(
            "MakerNotes:PreviewImageStart".to_string(),
            TagValue::new_integer(i64::from(start) + tiff_base as i64),
        );
    }

    // PreviewImage is the DataTag the offset pair points at. The offset is
    // TIFF-relative before absolutisation, which is how it addresses `tiff`.
    if let (Some(start), Some(length)) = (preview_start, preview_length)
        && length > 0
        && let Some(image) =
            tiff.get(start as usize..(start as usize).saturating_add(length as usize))
    {
        metadata.insert(
            "MakerNotes:PreviewImage".to_string(),
            TagValue::new_binary(image.to_vec()),
        );
    }
}

/// Reads the MakerNote's IFD. Entry value offsets are relative to the
/// enclosing TIFF header, so the reader addresses the whole TIFF block.
fn read_ifd(tiff: &[u8], ifd_offset: usize, order: ByteOrder) -> Option<IfdEntries> {
    let reader = crate::io::buffered_reader::BufferedReader::from_bytes(tiff);
    parse_ifd(&reader, ifd_offset as u64, order).ok()
}

/// Byte order of the MakerNote IFD at `ifd_offset`.
///
/// A Sigma MakerNote does not restate a TIFF header, so the order is inferred
/// from the entry count: a directory with more than 256 entries is the count
/// read the wrong way round.
fn entries_order(tiff: &[u8], ifd_offset: usize) -> ByteOrder {
    let Some(raw) = tiff.get(ifd_offset..ifd_offset + 2) else {
        return ByteOrder::BigEndian;
    };
    let big = u16::from_be_bytes([raw[0], raw[1]]);
    let little = u16::from_le_bytes([raw[0], raw[1]]);
    if big > 0 && big <= 256 {
        ByteOrder::BigEndian
    } else if little > 0 && little <= 256 {
        ByteOrder::LittleEndian
    } else {
        ByteOrder::BigEndian
    }
}

/// Maps one MakerNote entry onto the ExifTool tag name and printed value.
///
/// Returns `None` for every id `Sigma.pm` leaves unnamed or marks
/// `Unknown => 1`; ExifTool reports none of those without `-u`.
fn decode_tag(
    tag_id: u16,
    field_type: u16,
    count: u32,
    bytes: &[u8],
    order: ByteOrder,
    version: u8,
) -> Option<(&'static str, TagValue)> {
    // `MakerNoteSigmaVer < 3` splits the SD1/Merrill/Quattro bodies from the
    // earlier ones for ids 0x001c-0x0039.
    let legacy = version < 3;

    let value = match tag_id {
        0x0002 => ("SerialNumber", string_value(bytes)),
        0x0003 => ("DriveMode", string_value(bytes)),
        0x0004 => ("ResolutionMode", string_value(bytes)),
        0x0005 => ("AFMode", string_value(bytes)),
        0x0006 => ("FocusSetting", string_value(bytes)),
        0x0007 => ("WhiteBalance", string_value(bytes)),
        // PrintConv A/M/P/S.
        0x0008 => (
            "ExposureMode",
            TagValue::new_string(match decode_string(bytes).as_str() {
                "A" => "Aperture-priority AE".to_string(),
                "M" => "Manual".to_string(),
                "P" => "Program AE".to_string(),
                "S" => "Shutter speed priority AE".to_string(),
                other => other.to_string(),
            }),
        ),
        // PrintConv A/C/8.
        0x0009 => (
            "MeteringMode",
            TagValue::new_string(match decode_string(bytes).as_str() {
                "A" => "Average".to_string(),
                "C" => "Center-weighted average".to_string(),
                "8" => "Multi-segment".to_string(),
                other => other.to_string(),
            }),
        ),
        0x000A => ("LensFocalRange", string_value(bytes)),
        0x000B => ("ColorSpace", string_value(bytes)),
        // 0x000c is ExposureCompensation only in its string spelling; the
        // rational spelling is `ExposureAdjust`, `Unknown => 1`.
        0x000C if field_type == TYPE_STRING => (
            "ExposureCompensation",
            TagValue::new_string(strip_prefix(&decode_string(bytes), "Expo:")),
        ),
        // Each of these has a "Xxxx: <n>" string spelling and a rational64s
        // spelling; both print the bare number.
        0x000D => (
            "Contrast",
            prefixed_number(bytes, field_type, count, order, "Cont:"),
        ),
        0x000E => (
            "Shadow",
            prefixed_number(bytes, field_type, count, order, "Shad:"),
        ),
        0x000F => (
            "Highlight",
            prefixed_number(bytes, field_type, count, order, "High:"),
        ),
        0x0010 => (
            "Saturation",
            prefixed_number(bytes, field_type, count, order, "Satu:"),
        ),
        0x0011 => (
            "Sharpness",
            prefixed_number(bytes, field_type, count, order, "Shar:"),
        ),
        0x0012 => (
            "X3FillLight",
            prefixed_number(bytes, field_type, count, order, "Fill:"),
        ),
        0x0014 => (
            "ColorAdjustment",
            prefixed_number(bytes, field_type, count, order, "CC:"),
        ),
        0x0015 => ("AdjustmentMode", string_value(bytes)),
        0x0016 => (
            "Quality",
            TagValue::new_string(strip_prefix(&decode_string(bytes), "Qual:")),
        ),
        0x0017 => ("Firmware", string_value(bytes)),
        0x0018 => ("Software", string_value(bytes)),
        // PrintConv '$val =~ s/(\d)of(\d)/$1 of $2/'.
        0x0019 => (
            "AutoBracket",
            TagValue::new_string(space_out_of(&decode_string(bytes))),
        ),
        // Handled by the caller (IsOffset absolutisation).
        0x001A if field_type == TYPE_LONG => return None,
        0x001A if field_type == TYPE_STRING => (
            "ChrominanceNoiseReduction",
            TagValue::new_string(strip_prefix(&decode_string(bytes), "Chro:")),
        ),
        0x001B if field_type == TYPE_LONG => (
            "PreviewImageLength",
            TagValue::new_integer(i64::from(read_u32(bytes, order)?)),
        ),
        0x001B if field_type == TYPE_STRING => (
            "LuminanceNoiseReduction",
            TagValue::new_string(strip_prefix(&decode_string(bytes), "Luma:")),
        ),
        // PrintConv '$val =~ tr/ /x/' over an int16u pair.
        0x001C if legacy => (
            "PreviewImageSize",
            TagValue::new_string(size_pair(bytes, count, order)?),
        ),
        0x001D if legacy => ("MakerNoteVersion", string_value(bytes)),
        0x001E if !legacy => (
            "PreviewImageSize",
            TagValue::new_string(size_pair(bytes, count, order)?),
        ),
        0x001F if legacy => ("AFPoint", string_value(bytes)),
        0x001F => ("MakerNoteVersion", string_value(bytes)),
        0x0022 if legacy => ("FileFormat", string_value(bytes)),
        0x0024 if legacy => ("Calibration", string_value(bytes)),
        0x0026 if !legacy => ("FileFormat", string_value(bytes)),
        0x002C if field_type == TYPE_LONG => (
            "ColorMode",
            TagValue::new_string(color_mode(read_u32(bytes, order)?)),
        ),
        0x0030 if legacy => ("LensApertureRange", string_value(bytes)),
        0x0030 => ("Calibration", string_value(bytes)),
        // PrintConv sprintf("%.1f").
        0x0031 if legacy => {
            let value = rational(bytes, field_type, order)?;
            ("FNumber", TagValue::new_string(format!("{value:.1}")))
        }
        // PrintConv PrintExposureTime.
        0x0032 if legacy => {
            let value = rational(bytes, field_type, order)?;
            (
                "ExposureTime",
                TagValue::new_string(print_exposure_time(value)),
            )
        }
        // ValueConv '$val * 1e-6' over the string spelling, then
        // PrintExposureTime.
        0x0033 if legacy => {
            let micros: f64 = decode_string(bytes).trim().parse().ok()?;
            (
                "ExposureTime2",
                TagValue::new_string(print_exposure_time(micros * 1e-6)),
            )
        }
        0x0034 if legacy => (
            "BurstShot",
            TagValue::new_integer(i64::from(read_u32(bytes, order)?)),
        ),
        // PrintConv '$val and $val =~ s/^(\d)/\+$1/' - Perl's `and` makes 0
        // fall through unsigned.
        0x0035 if legacy => (
            "ExposureCompensation",
            TagValue::new_string(signed_number(rational(bytes, field_type, order)?)),
        ),
        // PrintConv 'IsInt($val) ? "$val C" : $val'.
        0x0039 if legacy => {
            let text = decode_string(bytes);
            let printed = if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
                format!("{text} C")
            } else {
                text
            };
            ("SensorTemperature", TagValue::new_string(printed))
        }
        0x003A if legacy => (
            "FlashExposureComp",
            TagValue::new_string(format_number(rational(bytes, field_type, order)?)),
        ),
        0x003B if legacy => ("Firmware", string_value(bytes)),
        0x003C if legacy => ("WhiteBalance", string_value(bytes)),
        0x003D => ("PictureMode", string_value(bytes)),
        _ => return None,
    };

    Some(value)
}

/// `Sigma.pm` 0x002c ColorMode PrintConv.
fn color_mode(raw: u32) -> String {
    match raw {
        0 => "n/a".to_string(),
        1 => "Sepia".to_string(),
        2 => "B&W".to_string(),
        3 => "Standard".to_string(),
        4 => "Vivid".to_string(),
        5 => "Neutral".to_string(),
        6 => "Portrait".to_string(),
        7 => "Landscape".to_string(),
        8 => "FOV Classic Blue".to_string(),
        other => format!("Unknown ({other})"),
    }
}

/// Renders the two int16u of a PreviewImageSize as ExifTool's "WxH".
fn size_pair(bytes: &[u8], count: u32, order: ByteOrder) -> Option<String> {
    if count < 2 || bytes.len() < 4 {
        return None;
    }
    let read = |off: usize| match order {
        ByteOrder::BigEndian => u16::from_be_bytes([bytes[off], bytes[off + 1]]),
        ByteOrder::LittleEndian => u16::from_le_bytes([bytes[off], bytes[off + 1]]),
    };
    Some(format!("{}x{}", read(0), read(2)))
}

/// One of the settings that has both a `"Xxxx: <n>"` string spelling (SIGMA
/// PhotoPro) and a rational64s spelling (the cameras), printed identically.
fn prefixed_number(
    bytes: &[u8],
    field_type: u16,
    count: u32,
    order: ByteOrder,
    prefix: &str,
) -> TagValue {
    if field_type == TYPE_STRING {
        return TagValue::new_string(strip_prefix(&decode_string(bytes), prefix));
    }
    let parts: Vec<String> = (0..count.max(1) as usize)
        .filter_map(|i| {
            let start = i * 8;
            rational(bytes.get(start..start + 8)?, field_type, order).map(format_number)
        })
        .collect();
    if parts.is_empty() {
        TagValue::new_string(String::new())
    } else {
        TagValue::new_string(parts.join(" "))
    }
}

/// Reads a single rational, signed or unsigned per the field type.
fn rational(bytes: &[u8], field_type: u16, order: ByteOrder) -> Option<f64> {
    if bytes.len() < 8 {
        return None;
    }
    let (num_raw, den_raw) = match order {
        ByteOrder::BigEndian => (
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        ),
        ByteOrder::LittleEndian => (
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        ),
    };
    let den = if field_type == TYPE_RATIONAL_S {
        f64::from(den_raw as i32)
    } else {
        f64::from(den_raw)
    };
    if den == 0.0 {
        return None;
    }
    let num = if field_type == TYPE_RATIONAL_S {
        f64::from(num_raw as i32)
    } else {
        f64::from(num_raw)
    };
    Some(num / den)
}

/// Perl's default number stringification for the values these tags carry.
fn format_number(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        let text = format!("{value}");
        text
    }
}

/// `'$val and $val =~ s/^(\d)/\+$1/'` - a leading "+" on positive values only,
/// and never on zero (Perl treats 0 as false).
fn signed_number(value: f64) -> String {
    let text = format_number(value);
    if value > 0.0 {
        format!("+{text}")
    } else {
        text
    }
}

/// `Image::ExifTool::Exif::PrintExposureTime`.
fn print_exposure_time(seconds: f64) -> String {
    if seconds > 0.0 && seconds < 0.25001 {
        return format!("1/{}", (0.5 + 1.0 / seconds) as i64);
    }
    if seconds == seconds.trunc() {
        return format!("{}", seconds as i64);
    }
    format!("{seconds:.1}")
}

/// `'$val =~ s/(\d)of(\d)/$1 of $2/'` for AutoBracket.
fn space_out_of(text: &str) -> String {
    let bytes: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 2);
    let mut i = 0;
    while i < bytes.len() {
        if i + 3 < bytes.len()
            && bytes[i].is_ascii_digit()
            && bytes[i + 1] == 'o'
            && bytes[i + 2] == 'f'
            && bytes[i + 3].is_ascii_digit()
        {
            out.push(bytes[i]);
            out.push_str(" of ");
            out.push(bytes[i + 3]);
            i += 4;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// `'$val =~ s/Xxxx:\s*//'` - drops the label SIGMA PhotoPro writes in front
/// of the number, along with the white space that follows it.
fn strip_prefix(text: &str, prefix: &str) -> String {
    match text.split_once(prefix) {
        Some((head, tail)) if head.is_empty() => tail.trim_start().to_string(),
        _ => text.to_string(),
    }
}

fn string_value(bytes: &[u8]) -> TagValue {
    TagValue::new_string(decode_string(bytes))
}

/// A TIFF ASCII value: everything up to the first NUL. Interior spaces are
/// significant (`Software` is a single space on SigmaDP2.x3f, and ExifTool
/// prints it as one).
fn decode_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

fn read_u32(bytes: &[u8], order: ByteOrder) -> Option<u32> {
    let raw: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
    Some(match order {
        ByteOrder::BigEndian => u32::from_be_bytes(raw),
        ByteOrder::LittleEndian => u32::from_le_bytes(raw),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_photopro_labels() {
        assert_eq!(strip_prefix("Qual:BASIC", "Qual:"), "BASIC");
        assert_eq!(strip_prefix("Cont: +1.0", "Cont:"), "+1.0");
        // A value that does not carry the label is left alone.
        assert_eq!(strip_prefix("BASIC", "Qual:"), "BASIC");
    }

    #[test]
    fn auto_bracket_gains_spaces() {
        assert_eq!(space_out_of("1of3"), "1 of 3");
        assert_eq!(space_out_of("1 of 3"), "1 of 3");
        assert_eq!(space_out_of(" "), " ");
    }

    #[test]
    fn exposure_times_use_the_reciprocal_form() {
        assert_eq!(print_exposure_time(0.1), "1/10");
        assert_eq!(print_exposure_time(1.0), "1");
        assert_eq!(print_exposure_time(0.4), "0.4");
    }

    #[test]
    fn exposure_compensation_signs_positives_only() {
        assert_eq!(signed_number(0.0), "0");
        assert_eq!(signed_number(1.0), "+1");
        assert_eq!(signed_number(-0.7), "-0.7");
    }

    #[test]
    fn strings_stop_at_the_first_nul() {
        assert_eq!(decode_string(b"AF-S\0\0\0\0"), "AF-S");
        // A lone space is a value, not padding to be trimmed.
        assert_eq!(decode_string(b" \0\0\0"), " ");
        assert_eq!(decode_string(b"\0\0\0\0"), "");
        // The Calibration string carries an embedded newline.
        assert_eq!(decode_string(b"a\nb\0"), "a\nb");
    }

    #[test]
    fn preview_size_prints_as_wxh() {
        let bytes = [0x02u8, 0x80, 0x01, 0xE0];
        assert_eq!(
            size_pair(&bytes, 2, ByteOrder::BigEndian),
            Some("640x480".to_string())
        );
    }

    #[test]
    fn rationals_print_as_plain_numbers() {
        // 0/10 signed
        let zero = [0, 0, 0, 0, 0, 0, 0, 10];
        assert_eq!(
            rational(&zero, TYPE_RATIONAL_S, ByteOrder::BigEndian).map(format_number),
            Some("0".to_string())
        );
        // 28/10 unsigned
        let f_number = [0, 0, 0, 28, 0, 0, 0, 10];
        assert_eq!(
            rational(&f_number, TYPE_RATIONAL_U, ByteOrder::BigEndian),
            Some(2.8)
        );
    }

    #[test]
    fn color_adjustment_joins_its_three_rationals() {
        let mut bytes = Vec::new();
        for _ in 0..3 {
            bytes.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 10]);
        }
        assert_eq!(
            prefixed_number(&bytes, TYPE_RATIONAL_S, 3, ByteOrder::BigEndian, "CC:"),
            TagValue::new_string("0 0 0".to_string())
        );
    }

    #[test]
    fn version_three_moves_preview_size_and_file_format() {
        // Ver >= 3 reads PreviewImageSize from 0x001e, not 0x001c.
        let size = [0x02u8, 0x80, 0x01, 0xE0];
        assert!(decode_tag(0x001C, TYPE_SHORT, 2, &size, ByteOrder::BigEndian, 3).is_none());
        assert_eq!(
            decode_tag(0x001E, TYPE_SHORT, 2, &size, ByteOrder::BigEndian, 3).map(|(n, _)| n),
            Some("PreviewImageSize")
        );
        assert_eq!(
            decode_tag(0x001C, TYPE_SHORT, 2, &size, ByteOrder::BigEndian, 2).map(|(n, _)| n),
            Some("PreviewImageSize")
        );
    }

    #[test]
    fn unknown_ids_emit_nothing() {
        for id in [0x0020u16, 0x0021, 0x0023, 0x0025, 0x0028, 0x002D, 0x0036] {
            assert!(
                decode_tag(id, TYPE_STRING, 1, b"x\0", ByteOrder::BigEndian, 2).is_none(),
                "id {id:#06x} should not be reported"
            );
        }
    }

    #[test]
    fn undefined_field_types_still_decode_as_text() {
        assert_eq!(
            decode_tag(0x001D, TYPE_UNDEF, 4, b"0100", ByteOrder::BigEndian, 2)
                .map(|(n, v)| (n, v)),
            Some(("MakerNoteVersion", TagValue::new_string("0100".to_string())))
        );
    }

    /// Every id the deleted `registries::sigma` table named differently from
    /// `Sigma.pm`, pinned to the name and the `Sigma.pm` line it comes from.
    ///
    /// A wrong id->name mapping does not show up as a coverage gap: it emits a
    /// confident value under a real tag name, which no total can catch. This
    /// test is the thing that catches it.
    #[test]
    fn ids_the_old_registry_got_wrong_follow_sigma_pm() {
        // (id, MakerNoteSigmaVer, Sigma.pm name, Sigma.pm line, name the
        //  deleted registry used)
        let cases: &[(u16, u8, Option<&str>, u32, &str)] = &[
            (0x000A, 2, Some("LensFocalRange"), 298, "LensRange"),
            (0x0012, 2, Some("X3FillLight"), 381, "FillLight"),
            // 0x001a/0x001b are int32u offset pairs on a camera-written file
            // and are handled by the caller; in their SIGMA PhotoPro string
            // spelling they are the noise-reduction settings, never a lens.
            (
                0x001A,
                2,
                Some("ChrominanceNoiseReduction"),
                436,
                "LensType",
            ),
            (0x001B, 2, Some("LuminanceNoiseReduction"), 458, "LensID"),
            (0x001C, 2, Some("PreviewImageSize"), 467, "LensModel"),
            (
                0x001D,
                2,
                Some("MakerNoteVersion"),
                490,
                "CameraTemperature",
            ),
            (0x001E, 3, Some("PreviewImageSize"), 509, "ColorMode"),
            (0x001F, 2, Some("AFPoint"), 519, "PictureStyle"),
            (0x001F, 3, Some("MakerNoteVersion"), 527, "PictureStyle"),
            // Sigma.pm lines 531/532 are comments, not table entries: 0x0020
            // and 0x0021 are not tags at all.
            (0x0020, 2, None, 531, "X3FillLight"),
            (0x0021, 2, None, 532, "ColorHue"),
            (0x0022, 2, Some("FileFormat"), 534, "HueAdjustment"),
            (0x0030, 2, Some("LensApertureRange"), 618, "ShutterCount"),
            (0x0030, 3, Some("Calibration"), 626, "ShutterCount"),
            (0x0031, 2, Some("FNumber"), 630, "FlashMode"),
            (0x0032, 2, Some("ExposureTime"), 639, "FlashExposureComp"),
            (0x0033, 2, Some("ExposureTime2"), 648, "FlashMeteringMode"),
            // %Sigma::Main has 69 keys and none of these is among them.
            (0x0040, 2, None, 0, "FileFormat"),
            (0x0041, 2, None, 0, "Compression"),
            (0x0050, 2, None, 0, "Calibration"),
            (0x0051, 2, None, 0, "DustRemovalData"),
        ];

        // These ids do not all read the same shape of value: some are strings,
        // some a single rational, and the PreviewImageSize pair is two int16u.
        // Offer each shape and take the first that decodes, so a `None` means
        // "this id is not a tag" rather than "the probe was the wrong shape".
        let rational = [0u8, 0, 0, 28, 0, 0, 0, 10];
        let pair = [0x02u8, 0x80, 0x01, 0xE0];
        for &(id, version, expected, line, old_name) in cases {
            let shapes: [(u16, u32, &[u8]); 3] = [
                (TYPE_STRING, 1, b"8\0"),
                (TYPE_RATIONAL_U, 1, &rational),
                (TYPE_SHORT, 2, &pair),
            ];
            let got = shapes
                .into_iter()
                .find_map(|(field_type, count, bytes)| {
                    decode_tag(id, field_type, count, bytes, ByteOrder::BigEndian, version)
                })
                .map(|(name, _)| name);
            assert_eq!(
                got, expected,
                "id {id:#06x} (ver {version}): Sigma.pm line {line} says {expected:?}, \
                 the deleted registry said {old_name:?}"
            );
        }
    }
}
