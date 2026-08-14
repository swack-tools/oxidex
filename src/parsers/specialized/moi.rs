//! MOI (MOD/TOD camcorder information file) metadata parser.
//!
//! MOI files store information about an associated MOD or TOD video, written
//! by some JVC, Canon and Panasonic camcorders. ExifTool routes them through
//! `Image::ExifTool::MOI::ProcessMOI` (MOI.pm:110-127), which validates a
//! 256-byte `V6`-prefixed header (optionally checking its embedded file-size
//! field against the real one) and then runs `ProcessBinaryData` over
//! `MOI::Main` (MOI.pm:20-91) with big-endian byte order. This parser does
//! the same and reads the layout from the generated table rather than
//! restating offsets here.
//!
//! # Why the table is not enough on its own
//!
//! Three fields carry a `ValueConv`/hash `ValueConv` the transcription
//! declines to reproduce: `DateTimeOriginal` (MOI.pm:29-38, an 8-byte packed
//! date/time), `Duration` (MOI.pm:39-43, milliseconds through
//! `ConvertDuration`) and `VideoBitrate` (MOI.pm:82-89, a 2-key raw-to-bps
//! hash through `ConvertBitrate`). All three are hand-implemented below
//! against the cited Perl, each behind a [`RawAccess`] citation.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/MOI.pm`

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::exiftool_tables::{
    Acknowledged, DecodedValue, PerlCitation, RawAccess, decode_binary_table, find_table,
};
use crate::io::ByteOrder;

/// MOI.pm:110, `$raf->Read($buff, 256) == 256 and $buff =~ /^V6/`.
const HEADER_LEN: usize = 256;
const MOI_SIGNATURE: &[u8] = b"V6";

/// Offset of the embedded big-endian file-size field MOI.pm:113 checks
/// against the real file size (`unpack('x2N', $buff)`, i.e. a `u32` at
/// byte offset 2).
const EMBEDDED_SIZE_OFFSET: usize = 2;

const fn citation(tag: &'static str, lines: &'static str) -> PerlCitation {
    PerlCitation {
        module: "MOI",
        table: "Main",
        tag,
        lines,
    }
}

const DATE_TIME_ORIGINAL: PerlCitation = citation("DateTimeOriginal", "MOI.pm:29-38");
const DURATION: PerlCitation = citation("Duration", "MOI.pm:39-43");
const AUDIO_BITRATE: PerlCitation = citation("AudioBitrate", "MOI.pm:78-81");
const VIDEO_BITRATE: PerlCitation = citation("VideoBitrate", "MOI.pm:82-89");

/// Extract MOI metadata using ExifTool's declared `MOI::Main` binary layout.
pub fn parse_moi_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    if reader.size() < HEADER_LEN as u64 {
        return Err("MOI file is too short for the 256-byte header".to_string());
    }
    let header = reader
        .read(0, HEADER_LEN)
        .map_err(|error| error.to_string())?;
    if !header.starts_with(MOI_SIGNATURE) {
        return Err("invalid MOI signature".to_string());
    }
    // MOI.pm:113-115: when the caller already knows the real file size,
    // require the header's own record of it to match before accepting the
    // file. `unpack('x2N', $buff)` reads a big-endian u32 at offset 2.
    if header.len() >= EMBEDDED_SIZE_OFFSET + 4 {
        let embedded_size = u32::from_be_bytes([
            header[EMBEDDED_SIZE_OFFSET],
            header[EMBEDDED_SIZE_OFFSET + 1],
            header[EMBEDDED_SIZE_OFFSET + 2],
            header[EMBEDDED_SIZE_OFFSET + 3],
        ]);
        if u64::from(embedded_size) != reader.size() {
            return Err("MOI header file-size field does not match the real file size".to_string());
        }
    }

    let table = find_table("MOI", "Main").ok_or("missing MOI::Main table")?;
    let decode = decode_binary_table(table, &header, ByteOrder::Big);

    let mut metadata = MetadataMap::new();
    for decoded in decode.fields() {
        let name = decoded.field.name;
        let key = format!("MOI:{name}");
        match name {
            "DateTimeOriginal" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::VALUE_CONV, &DATE_TIME_ORIGINAL)
                    && let DecodedValue::Undefined(bytes) = access.raw()
                    && let Some(rendered) = format_date_time_original(bytes)
                {
                    metadata.insert(key, TagValue::new_string(rendered));
                }
            }
            "Duration" => {
                if let Some(access) = RawAccess::new(decoded, Acknowledged::VALUE_CONV, &DURATION)
                    && let Some(ms) = access.raw().as_integer()
                {
                    metadata.insert(
                        key,
                        TagValue::new_string(convert_duration(ms as f64 / 1000.0)),
                    );
                }
            }
            "AudioBitrate" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::VALUE_CONV, &AUDIO_BITRATE)
                    && let Some(raw) = access.raw().as_integer()
                {
                    // MOI.pm:79: `ValueConv => '$val * 16000 + 48000'`.
                    let bps = (raw as f64) * 16000.0 + 48000.0;
                    metadata.insert(key, TagValue::new_string(convert_bitrate(bps)));
                }
            }
            "VideoBitrate" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::VALUE_CONV, &VIDEO_BITRATE)
                    && let Some(raw) = access.raw().as_integer()
                    && let Some(bps) = video_bitrate_value(raw)
                {
                    metadata.insert(key, TagValue::new_string(convert_bitrate(bps)));
                }
            }
            "AspectRatio" => {
                // MOI.pm:44-63: not flagged `omitted` by the generator (the
                // block is a `PrintConv => q{...}` Perl sub, not a
                // `ValueConv`/`RawConv`/`Condition`/`Hook`/`SubDirectory`),
                // so the raw byte reaches here through the ordinary `.emit()`
                // path with `PrintConv::None` -- hand-implemented against the
                // cited Perl rather than left as an unconverted integer.
                if let Some(TagValue::Integer(raw)) = decoded.emit() {
                    metadata.insert(key, TagValue::new_string(format_aspect_ratio(raw as u8)));
                }
            }
            _ => {
                if let Some(value) = decoded.emit() {
                    metadata.insert(key, value);
                }
            }
        }
    }
    Ok(metadata)
}

/// MOI.pm:31-37: `unpack('nCCCCn', $val)` (year u16, month/day/hour/min u8,
/// millisecond u16 BE), then `$v[5] /= 1000` and
/// `sprintf('%.4d:%.2d:%.2d %.2d:%.2d:%06.3f', @v)`. The `PrintConv` is
/// `$self->ConvertDateTime($val)`, which with no `-dateFormat`/timezone
/// option in play returns the `ValueConv` string unchanged, so this
/// reproduces the whole chain.
fn format_date_time_original(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 8 {
        return None;
    }
    let year = u16::from_be_bytes([bytes[0], bytes[1]]);
    let month = bytes[2];
    let day = bytes[3];
    let hour = bytes[4];
    let minute = bytes[5];
    let millisecond = u16::from_be_bytes([bytes[6], bytes[7]]);
    let seconds = f64::from(millisecond) / 1000.0;
    Some(format!(
        "{year:04}:{month:02}:{day:02} {hour:02}:{minute:02}:{seconds:06.3}"
    ))
}

/// MOI.pm:44-63's `PrintConv` block: low nibble picks the base ratio, high
/// nibble appends a TV-system suffix.
fn format_aspect_ratio(raw: u8) -> String {
    let lo = raw & 0x0f;
    let hi = raw >> 4;
    let mut aspect = if lo < 2 {
        "4:3".to_string()
    } else if lo == 4 || lo == 5 {
        "16:9".to_string()
    } else {
        "Unknown".to_string()
    };
    if hi == 4 {
        aspect.push_str(" NTSC");
    } else if hi == 5 {
        aspect.push_str(" PAL");
    }
    aspect
}

/// MOI.pm:82-89's `ValueConv` hash: only these two raw values are declared,
/// anything else is unconverted (ExifTool's hash-`ValueConv` fallback is the
/// raw value itself, but `ConvertBitrate` on an unrecognized raw code would
/// print a number that is not actually a bitrate -- so this only covers the
/// two keys the Perl declares and omits everything else, per AGENTS.md
/// "never approximate a conversion").
fn video_bitrate_value(raw: i64) -> Option<f64> {
    match raw {
        0x5896 => Some(8_500_000.0),
        0x813d => Some(5_500_000.0),
        _ => None,
    }
}

/// ExifTool.pm:6877-6894 `ConvertDuration`.
fn convert_duration(time: f64) -> String {
    if time == 0.0 {
        return "0 s".to_string();
    }
    let (sign, mut time) = if time > 0.0 { ("", time) } else { ("-", -time) };
    if time < 30.0 {
        return format!("{sign}{time:.2} s");
    }
    time += 0.5; // round off to the nearest second
    let mut h = (time / 3600.0) as i64;
    time -= (h as f64) * 3600.0;
    let m = (time / 60.0) as i64;
    time -= (m as f64) * 60.0;
    if h > 24 {
        let d = h / 24;
        h -= d * 24;
        return format!("{sign}{d} days {h}:{m:02}:{:02}", time as i64);
    }
    format!("{sign}{h}:{m:02}:{:02}", time as i64)
}

/// ExifTool.pm:6900-6912 `ConvertBitrate`.
fn convert_bitrate(mut bitrate: f64) -> String {
    let units = ["bps", "kbps", "Mbps", "Gbps"];
    let mut unit_index = 0;
    while bitrate >= 1000.0 && unit_index < units.len() - 1 {
        bitrate /= 1000.0;
        unit_index += 1;
    }
    let number = if bitrate < 100.0 {
        // Perl's "%.3g": three significant digits, trimmed of trailing zeros.
        format_significant(bitrate, 3)
    } else {
        format!("{:.0}", bitrate)
    };
    format!("{number} {}", units[unit_index])
}

/// Perl's `%.Ng` formatting: `N` significant digits, no trailing zeros or
/// decimal point, no exponent for the magnitudes this call site ever sees.
fn format_significant(value: f64, digits: i32) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let magnitude = value.abs().log10().floor() as i32;
    let decimals = (digits - 1 - magnitude).max(0);
    let formatted = format!("{value:.*}", decimals as usize);
    if formatted.contains('.') {
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        formatted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_time_original_matches_exiftool_sample() {
        // MOI.pm:31-38 on the pinned t/images/MOI.moi fixture's bytes at
        // offset 6: year=2011, month=5, day=15, hour=17, minute=58,
        // millisecond=48000 (48.000 s).
        let bytes = [0x07, 0xdb, 0x05, 0x0f, 0x11, 0x3a, 0xbb, 0x80];
        assert_eq!(
            format_date_time_original(&bytes).as_deref(),
            Some("2011:05:15 17:58:48.000")
        );
    }

    #[test]
    fn duration_matches_exiftool_sample() {
        // 8160 ms -> 8.16 s (MOI.pm:39-43, ConvertDuration for a value < 30).
        assert_eq!(convert_duration(8160.0 / 1000.0), "8.16 s");
    }

    #[test]
    fn video_bitrate_matches_exiftool_sample() {
        assert_eq!(
            convert_bitrate(video_bitrate_value(0x5896).unwrap()),
            "8.5 Mbps"
        );
        assert_eq!(
            convert_bitrate(video_bitrate_value(0x813d).unwrap()),
            "5.5 Mbps"
        );
        assert_eq!(video_bitrate_value(0x1234), None);
    }

    #[test]
    fn convert_duration_hours_format() {
        // ConvertDuration for a value >= 30 s formats h:mm:ss.
        assert_eq!(convert_duration(3725.0), "1:02:05");
    }

    #[test]
    fn aspect_ratio_matches_exiftool_sample() {
        // Pinned t/images/MOI.moi fixture: raw byte 0x51 -> "4:3 PAL".
        assert_eq!(format_aspect_ratio(0x51), "4:3 PAL");
    }

    #[test]
    fn audio_bitrate_matches_exiftool_sample() {
        // Pinned fixture's raw AudioBitrate byte is 11 -> 224000 bps -> "224 kbps".
        let bps = 11.0 * 16000.0 + 48000.0;
        assert_eq!(convert_bitrate(bps), "224 kbps");
    }
}
