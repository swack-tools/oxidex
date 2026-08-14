//! Sony DSC-F1 PMP metadata parser.
//!
//! ExifTool routes `.pmp` files through `Image::ExifTool::Sony::ProcessPMP`
//! (Sony.pm:11366-11388), which validates a 128-byte prefix, stamps a fixed
//! `Make`/`Model`, reads a 124-byte proprietary header through
//! `Sony::PMP` (Sony.pm:10630-10724), and then hands the JPEG that starts at
//! offset 124 to the ordinary JPEG reader.
//!
//! # What comes from the transcription and what does not
//!
//! `Sony::PMP` is a real `ProcessBinaryData` layout, so every offset, format
//! and `PrintConv` enum is read from the generated table. Six of its fields
//! carry a `RawConv`/`ValueConv` the transcription declines to reproduce and
//! are therefore flagged `omitted`; those are hand-implemented below against
//! the cited Perl:
//!
//! - `DateTimeOriginal` and `ModifyDate` (Sony.pm:10668-10692), an
//!   `int8u[6]` with a two-digit year the ValueConv pivots at 70.
//! - `ExposureTime` (Sony.pm:10693-10699), `2 ** (-$val / 100)` through
//!   `PrintExposureTime`.
//! - `FNumber` (Sony.pm:10700-10705), `ExposureCompensation`
//!   (Sony.pm:10706-10711) and `FocalLength` (Sony.pm:10712-10719), each
//!   `$val / 100` behind its own `RawConv` guard.
//!
//! Each of those `RawConv` guards is reproduced exactly, because it is what
//! decides whether the tag exists at all: `$val <= 0 ? undef : $val` for
//! ExposureTime/FNumber/FocalLength, and
//! `($val == -1 or $val == -32768) ? undef : $val` for
//! ExposureCompensation. ExifTool marks the last three "(NC -- not written by
//! DSC-F1)" and comments that their scaling is "likely wrong"/"probably wrong
//! too"; that uncertainty is ExifTool's own, and this parser reproduces what
//! the Perl declares rather than substituting a different guess.
//!
//! # What is deliberately absent
//!
//! **The embedded JPEG's own tags.** Sony.pm:11385-11387 seeks to 124, sets
//! `Base => 124` and calls `ProcessJPEG`, which yields `File:ImageWidth`,
//! `ImageHeight`, `EncodingProcess`, `BitsPerSample`, `ColorComponents` and
//! `YCbCrSubSampling` describing the *embedded* image. Re-entering this
//! crate's JPEG reader at a non-zero base is not something the current
//! `FileReader` port expresses, and synthesising those six tags from a
//! partial hand-read of the SOF marker would put values under real
//! `File:`-group names with nothing downstream able to tell if the offset
//! arithmetic were wrong. They are left absent.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/Sony.pm`

use crate::core::formatters::print_exposure_time;
use crate::core::{FileReader, MetadataMap, TagValue};
use crate::exiftool_tables::{
    Acknowledged, DecodedValue, PerlCitation, RawAccess, decode_binary_table, find_table,
};
use crate::io::ByteOrder;

/// Sony.pm:11372, `$raf->Read($buff, 128) == 128`.
const HEADER_LEN: usize = 128;

/// Sony.pm:11385, the embedded JPEG begins here -- which is also the declared
/// length of the proprietary header (`\x7c` at offset 11 is 124).
const JPEG_OFFSET: u32 = 124;

const fn citation(tag: &'static str, lines: &'static str) -> PerlCitation {
    PerlCitation {
        module: "Sony",
        table: "PMP",
        tag,
        lines,
    }
}

const DATE_TIME_ORIGINAL: PerlCitation = citation("DateTimeOriginal", "Sony.pm:10668-10680");
const MODIFY_DATE: PerlCitation = citation("ModifyDate", "Sony.pm:10681-10692");
const EXPOSURE_TIME: PerlCitation = citation("ExposureTime", "Sony.pm:10693-10699");
const F_NUMBER: PerlCitation = citation("FNumber", "Sony.pm:10700-10705");
const EXPOSURE_COMPENSATION: PerlCitation = citation("ExposureCompensation", "Sony.pm:10706-10711");
const FOCAL_LENGTH: PerlCitation = citation("FocalLength", "Sony.pm:10712-10719");

/// Extract Sony PMP metadata (`Image::ExifTool::Sony::ProcessPMP`).
pub fn parse_pmp_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    if reader.size() < HEADER_LEN as u64 {
        return Err("PMP file is too short for the 128-byte header".to_string());
    }
    let header = reader.read(0, HEADER_LEN).map_err(|e| e.to_string())?;

    // Sony.pm:11374, `$buff =~ /^.{8}\0{3}\x7c.{112}\xff\xd8\xff\xdb$/s` --
    // three NULs and a 0x7c header length at offset 8, and the JPEG SOI plus
    // a DQT marker exactly at offset 124.
    if header[8..11] != [0, 0, 0] || header[11] != 0x7c {
        return Err("invalid PMP header length field".to_string());
    }
    if header[124..128] != [0xff, 0xd8, 0xff, 0xdb] {
        return Err("PMP header is not followed by a JPEG SOI/DQT".to_string());
    }

    let mut metadata = MetadataMap::new();
    // Sony.pm:11377-11378: both are stamped unconditionally, not read.
    metadata.insert("ExifTool:Make".to_string(), TagValue::new_string("Sony"));
    metadata.insert("ExifTool:Model".to_string(), TagValue::new_string("DSC-F1"));

    let table = find_table("Sony", "PMP").ok_or("missing Sony::PMP table")?;
    // Sony.pm:11376, `SetByteOrder('MM')`.
    let decode = decode_binary_table(table, &header, ByteOrder::Big);

    for decoded in decode.fields() {
        let name = decoded.field.name;
        let key = format!("Sony:{name}");
        let value = match name {
            // Sony.pm:10668-10692: `int8u[6]`, year pivoted at 70.
            "DateTimeOriginal" | "ModifyDate" => {
                let cite = if name == "DateTimeOriginal" {
                    &DATE_TIME_ORIGINAL
                } else {
                    &MODIFY_DATE
                };
                RawAccess::new(decoded, Acknowledged::VALUE_CONV, cite)
                    .and_then(|access| format_pmp_date(access.raw()))
                    .map(TagValue::new_string)
            }
            // Sony.pm:10693-10699.
            "ExposureTime" => {
                RawAccess::new(
                    decoded,
                    Acknowledged::VALUE_CONV | Acknowledged::RAW_CONV,
                    &EXPOSURE_TIME,
                )
                .and_then(|access| access.raw().as_integer())
                // `RawConv => '$val <= 0 ? undef : $val'`.
                .filter(|raw| *raw > 0)
                // `ValueConv => '2 ** (-$val / 100)'`, then
                // `PrintConv => 'Image::ExifTool::Exif::PrintExposureTime($val)'`.
                .map(|raw| {
                    let seconds = 2f64.powf(-(raw as f64) / 100.0);
                    TagValue::new_string(print_exposure_time(seconds))
                })
            }
            // Sony.pm:10700-10705. ExifTool's own comment calls the scaling
            // "(likely wrong)"; it is reproduced as declared, not replaced.
            "FNumber" => RawAccess::new(
                decoded,
                Acknowledged::VALUE_CONV | Acknowledged::RAW_CONV,
                &F_NUMBER,
            )
            .and_then(|access| access.raw().as_integer())
            .filter(|raw| *raw > 0)
            .map(|raw| TagValue::new_float(raw as f64 / 100.0)),
            // Sony.pm:10706-10711.
            "ExposureCompensation" => RawAccess::new(
                decoded,
                Acknowledged::VALUE_CONV | Acknowledged::RAW_CONV,
                &EXPOSURE_COMPENSATION,
            )
            .and_then(|access| access.raw().as_integer())
            .filter(|raw| *raw != -1 && *raw != -32768)
            .map(|raw| TagValue::new_float(raw as f64 / 100.0)),
            // Sony.pm:10712-10719, `PrintConv => 'sprintf("%.1f mm",$val)'`.
            "FocalLength" => RawAccess::new(
                decoded,
                Acknowledged::VALUE_CONV | Acknowledged::RAW_CONV,
                &FOCAL_LENGTH,
            )
            .and_then(|access| access.raw().as_integer())
            .filter(|raw| *raw > 0)
            .map(|raw| TagValue::new_string(format!("{:.1} mm", raw as f64 / 100.0))),
            _ => decoded.emit(),
        };
        if let Some(value) = value {
            metadata.insert(key, value);
        }
    }

    // Sony.pm:10639-10645: `JpgFromRawStart`/`JpgFromRawLength` describe an
    // extractable JPEG block, which ExifTool reports as a binary-data
    // placeholder under `-a -G1 -s`.
    if let Some(TagValue::Integer(length)) = metadata.get("Sony:JpgFromRawLength").cloned() {
        let start = metadata
            .get("Sony:JpgFromRawStart")
            .and_then(|v| v.as_integer());
        if start == Some(i64::from(JPEG_OFFSET))
            && length > 0
            && (JPEG_OFFSET as u64 + length as u64) <= reader.size()
        {
            metadata.insert(
                "Sony:JpgFromRaw".to_string(),
                TagValue::new_string(format!(
                    "(Binary data {length} bytes, use -b option to extract)"
                )),
            );
        }
    }

    Ok(metadata)
}

/// Sony.pm:10672-10678 (and the identical block at :10685-10691):
///
/// ```text
/// my @a = split ' ', $val;
/// $a[0] += $a[0] < 70 ? 2000 : 1900;
/// sprintf('%.4d:%.2d:%.2d %.2d:%.2d:%.2d', @a);
/// ```
///
/// The `PrintConv` is `$self->ConvertDateTime($val)`, which with no
/// `-dateFormat` option returns its input unchanged, so this is the whole
/// chain.
fn format_pmp_date(raw: &DecodedValue) -> Option<String> {
    let DecodedValue::Array(items) = raw else {
        return None;
    };
    let parts: Vec<i64> = items.iter().filter_map(DecodedValue::as_integer).collect();
    if parts.len() < 6 {
        return None;
    }
    let year = parts[0] + if parts[0] < 70 { 2000 } else { 1900 };
    Some(format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        year, parts[1], parts[2], parts[3], parts[4], parts[5]
    ))
}
