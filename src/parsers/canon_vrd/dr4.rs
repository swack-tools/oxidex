//! Canon DPP version 4 "DR4" recipe records (`%Image::ExifTool::CanonVRD::DR4`).
//!
//! DR4 is the successor to the fixed `VRD1` record decoded in the parent
//! module. Where VRD1 is a flat 0x272-byte struct, DR4 is a directory: a
//! 32-byte header followed by an entry count and fixed-size entries, each
//! naming a tag id, a storage format, three flag words, and an offset/length
//! pair pointing at the value. Canon writes it either as the `Edit4Data`
//! block (0xffff00f7) of a CanonVRD trailer or as a standalone `.DR4` file.
//!
//! Layout (`ProcessDR4`, CanonVRD.pm:1739-1916):
//!
//! ```text
//!   +0x00  int32u[8] header    -- 'IIII', 0x00040004, 6, DR4CameraModel,
//!                                 3, 4, 5, entry count
//!   +0x24  entry[count]        -- 28 bytes each:
//!            +0x00 int32u tag
//!            +0x04 int32u format code (%vrdFormat)
//!            +0x08 int32u flag 0   ("on/off flag")
//!            +0x0c int32u flag 1   ("is default" flag?)
//!            +0x10 int32u flag 2
//!            +0x14 int32u value offset, relative to the directory
//!            +0x18 int32u value length
//! ```
//!
//! Byte order comes from the first two bytes (`SetByteOrder`,
//! CanonVRD.pm:1801) -- always `II` in practice, since the magic number is
//! `IIII`. Five tags carry `SubDirectory`s of their own, each a plain
//! `ProcessBinaryData` table read at a fixed element stride.

use crate::core::formatters::numeric_precision::{perl_g, perl_number};
use crate::core::{MetadataMap, TagValue};
use crate::parsers::tiff::makernotes::canon::decode_canon_model_id;

mod tables;

use tables::{DR4_MAIN, DR4Entry, Sub};

/// `int32u[8]`, and `$dirLen < 32` is a hard error (CanonVRD.pm:1799).
const HEADER_LEN: usize = 32;

/// `Get32u($dataPt, $pos + 28)`.
const NUM_ENTRIES_OFFSET: usize = 28;

/// `my $entry = $pos + 36 + 28 * $index`.
const FIRST_ENTRY: usize = 36;

/// Bytes per directory entry.
const ENTRY_LEN: usize = 28;

/// `3 => { Name => 'DR4CameraModel', ... }` of `%CanonVRD::DR4Header`, an
/// `int32u` table so index 3 is byte 12.
const CAMERA_MODEL_OFFSET: usize = 12;

/// The group ExifTool files these under: `GROUPS => { 1 => 'CanonDR4' }`.
const GROUP: &str = "CanonDR4";

/// `%vrdFormat` (CanonVRD.pm:48-58) -- the storage format of a value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Format {
    Int32u,
    Int32s,
    Double,
    Str,
    /// Raw bytes, printed as hex. Also the fallback for an unlisted code.
    Undef,
}

impl Format {
    /// `%vrdFormat`; `undef` for a code the hash does not list, which
    /// CanonVRD.pm:1841-1844 renders as `unpack 'H*'`.
    fn from_code(code: u32) -> Option<Self> {
        Some(match code {
            1 | 8 | 33 => Format::Int32u,
            2 => Format::Str,
            9 | 24 => Format::Int32s,
            13 | 38 => Format::Double,
            255 => Format::Undef,
            _ => return None,
        })
    }

    /// `$formatSize{$format}`.
    fn width(self) -> usize {
        match self {
            Format::Int32u | Format::Int32s => 4,
            Format::Double => 8,
            // Strings and undef are read whole, not element by element.
            Format::Str | Format::Undef => 1,
        }
    }
}

/// A value as `ReadValue` assembles it: elements in order, joined with spaces
/// when printed.
#[derive(Clone, Debug)]
enum Value {
    Ints(Vec<i64>),
    Doubles(Vec<f64>),
    Text(String),
}

impl Value {
    /// The first element as an integer, when there is one.
    fn first_int(&self) -> Option<i64> {
        match self {
            Value::Ints(v) => v.first().copied(),
            Value::Doubles(v) => v.first().map(|f| *f as i64),
            Value::Text(_) => None,
        }
    }

    /// The first element as a float, when there is one.
    fn first_f64(&self) -> Option<f64> {
        match self {
            Value::Ints(v) => v.first().map(|i| *i as f64),
            Value::Doubles(v) => v.first().copied(),
            Value::Text(_) => None,
        }
    }

    /// `$count < 1 and return undef` -- a run of zero elements is no value.
    fn is_empty(&self) -> bool {
        match self {
            Value::Ints(v) => v.is_empty(),
            Value::Doubles(v) => v.is_empty(),
            Value::Text(_) => false,
        }
    }

    /// How ExifTool stringifies the value with no PrintConv in play.
    fn render(&self) -> String {
        match self {
            Value::Ints(v) => v.iter().map(i64::to_string).collect::<Vec<_>>().join(" "),
            Value::Doubles(v) => v
                .iter()
                .map(|f| perl_number(*f))
                .collect::<Vec<_>>()
                .join(" "),
            Value::Text(s) => s.clone(),
        }
    }
}

/// Reads a whole DR4 record -- the `Edit4Data` block, or a `.DR4` file.
///
/// `data` is the directory itself, starting at the `IIII` magic number.
#[must_use]
pub fn parse_dr4(data: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    // `$dirLen < 32` and `SetByteOrder` both fail out (CanonVRD.pm:1798-1801).
    if data.len() < HEADER_LEN {
        return metadata;
    }
    let big_endian = match &data[..2] {
        b"MM" => true,
        b"II" => false,
        _ => return metadata,
    };
    let r = Reader { data, big_endian };

    // `%CanonVRD::DR4Header` index 3, the only named header field.
    if let Some(model) = r.u32(CAMERA_MODEL_OFFSET) {
        insert(
            &mut metadata,
            "DR4CameraModel",
            TagValue::new_string(model_name(model)),
        );
    }

    let Some(num_entries) = r.u32(NUM_ENTRIES_OFFSET) else {
        return metadata;
    };
    // `$err = 1 if $dirLen < 36 + 28 * $numEntries` -- an over-long count is
    // "Invalid DR4 directory" and yields nothing at all.
    let Some(needed) = (num_entries as usize)
        .checked_mul(ENTRY_LEN)
        .and_then(|n| n.checked_add(FIRST_ENTRY))
    else {
        return MetadataMap::new();
    };
    if data.len() < needed {
        return MetadataMap::new();
    }

    for index in 0..num_entries as usize {
        let entry = FIRST_ENTRY + ENTRY_LEN * index;
        // `last if $entry + 28 > $dirEnd`
        if entry + ENTRY_LEN > data.len() {
            break;
        }
        let (Some(tag), Some(fmt), Some(flag0), Some(flag1), Some(flag2), Some(off), Some(len)) = (
            r.u32(entry),
            r.u32(entry + 4),
            r.u32(entry + 8),
            r.u32(entry + 12),
            r.u32(entry + 16),
            r.u32(entry + 20),
            r.u32(entry + 24),
        ) else {
            break;
        };
        let (off, len) = (off as usize, len as usize);
        // `next if $off + $len >= $dirEnd` -- note `>=`, so a value that ends
        // exactly at the end of the directory is skipped, not read.
        if off.checked_add(len).is_none_or(|end| end >= data.len()) {
            continue;
        }

        let flags = [flag0, flag1, flag2];
        match DR4_MAIN.iter().find(|e| e.tag == tag) {
            Some(def) => {
                match def.conv {
                    Conv::SubDir(sub) => parse_subdir(&r, sub, off, len, &mut metadata),
                    _ => {
                        if let Some(value) = r.value(fmt, off, len)
                            && let Some(printed) = convert(def.conv, &value)
                        {
                            insert(&mut metadata, def.name, printed);
                        }
                    }
                }
                emit_flags(def, &flags, &mut metadata);
            }
            None => continue,
        }
    }
    metadata
}

/// `foreach $i (0..2) { ... HandleTag($tagTablePtr, $flagID, $flg[$i]) }`
/// (CanonVRD.pm:1907-1910) -- the flag words are tags in their own right, but
/// only where the table declares `0x<tag>.<i>`.
fn emit_flags(def: &DR4Entry, flags: &[u32; 3], metadata: &mut MetadataMap) {
    for (i, flag) in flags.iter().enumerate() {
        let Some((name, conv)) = def.flags[i] else {
            continue;
        };
        let value = Value::Ints(vec![i64::from(*flag)]);
        if let Some(printed) = convert(conv, &value) {
            insert(metadata, name, printed);
        }
    }
}

/// A `ProcessBinaryData` subdirectory, read at the table's own element stride.
fn parse_subdir(r: &Reader, sub: Sub, off: usize, len: usize, metadata: &mut MetadataMap) {
    let end = off + len;
    for entry in sub.entries {
        let format = entry.format.unwrap_or(sub.format);
        let start = off + entry.index * sub.format.width();
        let want = format.width() * entry.count;
        if start + want > end {
            continue;
        }
        let Some(value) = r.read(format, start, want) else {
            continue;
        };
        if let Some(printed) = convert(entry.conv, &value) {
            insert(metadata, entry.name, printed);
        }
    }
}

fn insert(metadata: &mut MetadataMap, name: &str, value: TagValue) {
    metadata.insert(format!("{GROUP}:{name}"), value);
}

/// `PrintHex => 1` with `%canonModelID` as the PrintConv.
fn model_name(id: u32) -> String {
    let name = decode_canon_model_id(id);
    // `decode_canon_model_id` reports unknown ids in decimal; ExifTool's
    // PrintHex prints them in hex.
    if name.starts_with("Unknown (") {
        format!("Unknown (0x{id:x})")
    } else {
        name
    }
}

/// Applies one table entry's ValueConv and PrintConv, as ExifTool prints them.
fn convert(conv: Conv, value: &Value) -> Option<TagValue> {
    let printed = match conv {
        Conv::None => {
            // A lone plain integer stays typed; anything else is a string.
            return Some(match value {
                Value::Ints(v) if v.len() == 1 => TagValue::Integer(v[0]),
                other => TagValue::String(other.render()),
            });
        }
        Conv::Map(table) => print_conv(table, value.first_int()?),
        Conv::MapHex(table) => {
            let v = value.first_int()?;
            table.iter().find(|(key, _)| *key == v).map_or_else(
                || format!("Unknown (0x{v:x})"),
                |(_, label)| (*label).to_string(),
            )
        }
        Conv::NoYes => print_conv(&[(0, "No"), (1, "Yes")], value.first_int()?),
        // `ValueConv => '$val / 10'`, `PrintConv => 'sprintf("%.0f%%", $val * 100)'`
        Conv::ShootingDistance => format!("{:.0}%", value.first_f64()? / 10.0 * 100.0),
        // `PrintConv => 'sprintf "%g", $val'`
        Conv::PercentG => perl_g(value.first_f64()?, 6),
        // `PrintConv => 'sprintf("%.7g",$val)'`
        Conv::Sprintf7g => perl_g(value.first_f64()?, 7),
        // `PrintConv => '$val =~ s/^\d+ //; $val'`
        Conv::StripFirstInt => {
            let raw = value.render();
            raw.split_once(' ')
                .filter(|(head, _)| !head.is_empty() && head.bytes().all(|b| b.is_ascii_digit()))
                .map_or(raw.clone(), |(_, rest)| rest.to_string())
        }
        Conv::ToneCurve => tone_curve_print(value),
        // The three gamma points share `sprintf("%+.3f")` over a log ValueConv
        // that differs only in its offset (CanonVRD.pm:1399-1425).
        Conv::GammaBlackPoint => format!("{:+.3}", gamma_point(value.first_f64()?, 1.0, true)),
        Conv::GammaWhitePoint => {
            format!(
                "{:+.3}",
                gamma_point(value.first_f64()?, -11.771_093_251_699_54, false)
            )
        }
        Conv::GammaMidPoint => format!("{:+.3}", gamma_point(value.first_f64()?, -8.0, false)),
        Conv::SubDir(_) => return None,
    };
    Some(TagValue::String(printed))
}

/// A PrintConv hash lookup, with ExifTool's fallback for an unlisted key.
fn print_conv(table: &[(i64, &str)], v: i64) -> String {
    table.iter().find(|(key, _)| *key == v).map_or_else(
        || format!("Unknown ({v})"),
        |(_, label)| (*label).to_string(),
    )
}

/// The shared shape of `GammaBlackPoint`, `GammaWhitePoint` and
/// `GammaMidPoint` (CanonVRD.pm:1399-1425):
///
/// ```text
///     return 0 if $val <= 0;                     # (black point)
///     return $val if $val <= 0;                  # (white and mid points)
///     $val = log($val / 4.6875) / log(2) + <offset>;
///     return abs($val) > 1e-10 ? $val : 0;
/// ```
fn gamma_point(val: f64, offset: f64, zero_below: bool) -> f64 {
    if val <= 0.0 {
        return if zero_below { 0.0 } else { val };
    }
    let out = (val / 4.6875).log2() + offset;
    if out.abs() > 1e-10 { out } else { 0.0 }
}

/// `CanonVRD::ToneCurvePrint` (CanonVRD.pm:1499) -- a point count followed by
/// up to 10 (x,y) pairs. A count outside 2..=10, or a value that is not 21
/// elements long, prints raw.
fn tone_curve_print(value: &Value) -> String {
    let Value::Ints(values) = value else {
        return value.render();
    };
    if values.len() != 21 {
        return value.render();
    }
    let count = values[0];
    if !(2..=10).contains(&count) {
        return value.render();
    }
    (0..count as usize)
        .map(|i| format!("({},{})", values[1 + i * 2], values[2 + i * 2]))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Reads scalars out of the directory in its declared byte order.
struct Reader<'a> {
    data: &'a [u8],
    big_endian: bool,
}

impl Reader<'_> {
    fn u32(&self, offset: usize) -> Option<u32> {
        let b: [u8; 4] = self.data.get(offset..offset + 4)?.try_into().ok()?;
        Some(if self.big_endian {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    }

    /// The value of a directory entry, given its `%vrdFormat` code.
    fn value(&self, fmt: u32, off: usize, len: usize) -> Option<Value> {
        match Format::from_code(fmt) {
            // "if (not $format) { $val = unpack 'H*', ... }"
            None => Some(Value::Text(hex(self.data.get(off..off + len)?))),
            Some(format) => self.read(format, off, len),
        }
    }

    fn read(&self, format: Format, off: usize, len: usize) -> Option<Value> {
        let bytes = self.data.get(off..off + len)?;
        Some(match format {
            // `$vals[0] =~ s/\0.*//s` for 'string'.
            Format::Str => {
                let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
                Value::Text(String::from_utf8_lossy(&bytes[..end]).into_owned())
            }
            Format::Undef => Value::Text(String::from_utf8_lossy(bytes).into_owned()),
            Format::Int32u => Value::Ints(
                bytes
                    .chunks_exact(4)
                    .map(|c| i64::from(self.u32_bytes(c)))
                    .collect(),
            ),
            Format::Int32s => Value::Ints(
                bytes
                    .chunks_exact(4)
                    .map(|c| i64::from(self.u32_bytes(c) as i32))
                    .collect(),
            ),
            Format::Double => Value::Doubles(
                bytes
                    .chunks_exact(8)
                    .map(|c| {
                        let v = self.f64_bytes(c);
                        // "avoid teeny weeny values" (CanonVRD.pm:1846-1847)
                        if v.abs() < 1e-100 { 0.0 } else { v }
                    })
                    .collect(),
            ),
        })
        .filter(|v: &Value| !v.is_empty())
    }

    fn u32_bytes(&self, c: &[u8]) -> u32 {
        let b: [u8; 4] = c.try_into().unwrap_or([0; 4]);
        if self.big_endian {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        }
    }

    fn f64_bytes(&self, c: &[u8]) -> f64 {
        let b: [u8; 8] = c.try_into().unwrap_or([0; 8]);
        if self.big_endian {
            f64::from_be_bytes(b)
        } else {
            f64::from_le_bytes(b)
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// The PrintConv of a `%CanonVRD::DR4` entry.
#[derive(Clone, Copy)]
enum Conv {
    /// Value passes through as ExifTool's raw string.
    None,
    /// A PrintConv lookup table.
    Map(&'static [(i64, &'static str)]),
    /// A PrintConv lookup table under `PrintHex => 1`.
    MapHex(&'static [(i64, &'static str)]),
    /// `%noYes` (CanonVRD.pm:42): `{ 0 => 'No', 1 => 'Yes' }`.
    NoYes,
    /// `ValueConv => '$val / 10'`, `PrintConv => 'sprintf("%.0f%%", $val * 100)'`.
    ShootingDistance,
    /// `PrintConv => 'sprintf "%g", $val'`.
    PercentG,
    /// `PrintConv => 'sprintf("%.7g",$val)'`.
    Sprintf7g,
    /// `PrintConv => '$val =~ s/^\d+ //; $val'` -- `WBAdjRGGBLevels` drops the
    /// leading count.
    StripFirstInt,
    /// `CanonVRD::ToneCurvePrint($val)`.
    ToneCurve,
    /// The three gamma points, which share a shape but not an offset.
    GammaBlackPoint,
    GammaWhitePoint,
    GammaMidPoint,
    /// `SubDirectory => { TagTable => ... }` over a `ProcessBinaryData` table.
    SubDir(Sub),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal DR4 directory: the 32-byte header, an entry count, one
    /// entry per (tag, format, flags, value) tuple, and the values packed after
    /// the entry table.
    ///
    /// Laid out as `combined-samples/CanonVRD.dr4` is: little-endian, camera
    /// model at header word 3, entry count at word 7.
    fn dr4(model: u32, entries: &[(u32, u32, [u32; 3], Vec<u8>)]) -> Vec<u8> {
        let mut header = Vec::new();
        header.extend_from_slice(b"IIII");
        header.extend_from_slice(&0x0004_0004u32.to_le_bytes());
        header.extend_from_slice(&6u32.to_le_bytes());
        header.extend_from_slice(&model.to_le_bytes());
        for w in [3u32, 4, 5] {
            header.extend_from_slice(&w.to_le_bytes());
        }
        header.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        assert_eq!(header.len(), HEADER_LEN);
        // Word 8 sits between the header and the first entry and is not read.
        header.extend_from_slice(&0u32.to_le_bytes());

        let mut table = Vec::new();
        let mut values = Vec::new();
        let value_base = FIRST_ENTRY + ENTRY_LEN * entries.len();
        for (tag, fmt, flags, bytes) in entries {
            table.extend_from_slice(&tag.to_le_bytes());
            table.extend_from_slice(&fmt.to_le_bytes());
            for f in flags {
                table.extend_from_slice(&f.to_le_bytes());
            }
            table.extend_from_slice(&((value_base + values.len()) as u32).to_le_bytes());
            table.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            values.extend_from_slice(bytes);
        }
        let mut out = header;
        out.extend_from_slice(&table);
        out.extend_from_slice(&values);
        // `next if $off + $len >= $dirEnd` skips a value that ends exactly at
        // the directory end, so leave a byte of slack after the last one.
        out.push(0);
        out
    }

    fn le32(v: u32) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }

    fn f64le(v: f64) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }

    /// Every assertion is `exiftool -a -G1 -s combined-samples/CanonVRD.dr4`
    /// (ExifTool 13.55), byte for byte.
    #[test]
    fn reads_the_header_and_simple_values() {
        let m = parse_dr4(&dr4(
            0x8000_0289,
            &[
                // 0x10002 Rotation, fmt 8 (int32u)
                (0x10002, 8, [1, 1, 1], le32(0)),
                // 0x10003 AngleAdj, fmt 13 (double)
                (0x10003, 13, [1, 0, 1], f64le(13.08)),
                // 0x10100 Rating, PrintConv
                (0x10100, 8, [1, 1, 1], le32(4)),
                // 0x10200 WorkColorSpace
                (0x10200, 8, [1, 1, 1], le32(3)),
                // 0x20101 WhiteBalanceAdj
                (0x20101, 9, [1, 0, 1], le32(2)),
                // 0x20301 PictureStyle, PrintHex
                (0x20301, 8, [1, 0, 1], le32(0x84)),
            ],
        ));
        assert_eq!(
            m.get_string("CanonDR4:DR4CameraModel"),
            Some("EOS 7D Mark II")
        );
        assert_eq!(m.get_integer("CanonDR4:Rotation"), Some(0));
        assert_eq!(m.get_string("CanonDR4:AngleAdj"), Some("13.08"));
        assert_eq!(m.get_string("CanonDR4:Rating"), Some("4"));
        assert_eq!(
            m.get_string("CanonDR4:WorkColorSpace"),
            Some("Wide Gamut RGB")
        );
        assert_eq!(m.get_string("CanonDR4:WhiteBalanceAdj"), Some("Cloudy"));
        assert_eq!(m.get_string("CanonDR4:PictureStyle"), Some("Neutral"));
    }

    #[test]
    fn flag_words_become_tags_of_their_own() {
        // `0x20702.0` is PeripheralIlluminationOn, read from the first flag
        // word rather than from the value (CanonVRD.pm:1907-1910).
        let m = parse_dr4(&dr4(0, &[(0x20702, 13, [1, 0, 1], f64le(34.0))]));
        assert_eq!(m.get_string("CanonDR4:PeripheralIllumination"), Some("34"));
        assert_eq!(
            m.get_string("CanonDR4:PeripheralIlluminationOn"),
            Some("Yes")
        );
        // Flags the table does not name stay unreported.
        assert!(m.get("CanonDR4:PeripheralIllumination.1").is_none());
    }

    #[test]
    fn subdirectories_are_read_at_the_tables_own_stride() {
        // CropInfo is int32s with a double at index 8, i.e. byte 32.
        let mut crop = Vec::new();
        for v in [1i32, 6089, 4740, 2060, 1291, 2952, 1476, 0] {
            crop.extend_from_slice(&v.to_le_bytes());
        }
        crop.extend_from_slice(&13.08f64.to_le_bytes());
        for v in [5472i32, 3648] {
            crop.extend_from_slice(&v.to_le_bytes());
        }
        let m = parse_dr4(&dr4(0, &[(0xf0100, 255, [1, 0, 1], crop)]));
        assert_eq!(m.get_string("CanonDR4:CropActive"), Some("Yes"));
        assert_eq!(
            m.get_integer("CanonDR4:CropRotatedOriginalWidth"),
            Some(6089)
        );
        assert_eq!(m.get_integer("CanonDR4:CropX"), Some(2060));
        assert_eq!(m.get_string("CanonDR4:CropAngle"), Some("13.08"));
        assert_eq!(m.get_integer("CanonDR4:CropOriginalWidth"), Some(5472));
        assert_eq!(m.get_integer("CanonDR4:CropOriginalHeight"), Some(3648));
    }

    #[test]
    fn tone_curve_points_print_as_pairs() {
        // 3 points: (0,79) (117,172) (242,255), the curve in CanonVRD.dr4.
        let mut tc = vec![0u8; 0x9f * 4];
        let mut put = |index: usize, v: u32| {
            tc[index * 4..index * 4 + 4].copy_from_slice(&v.to_le_bytes());
        };
        put(0x00, 1); // ToneCurveColorSpace -> Luminance
        put(0x01, 1); // ToneCurveShape -> Straight
        put(0x03, 0);
        put(0x04, 242); // ToneCurveInputRange
        put(0x05, 79);
        put(0x06, 255); // ToneCurveOutputRange
        for (i, v) in [3u32, 0, 79, 117, 172, 242, 255].iter().enumerate() {
            put(0x07 + i, *v);
        }
        put(0x0a, 117);
        put(0x0b, 172);
        let m = parse_dr4(&dr4(0, &[(0x20400, 255, [1, 0, 1], tc)]));
        assert_eq!(
            m.get_string("CanonDR4:ToneCurveColorSpace"),
            Some("Luminance")
        );
        assert_eq!(m.get_string("CanonDR4:ToneCurveShape"), Some("Straight"));
        assert_eq!(m.get_string("CanonDR4:ToneCurveInputRange"), Some("0 242"));
        assert_eq!(
            m.get_string("CanonDR4:ToneCurveOutputRange"),
            Some("79 255")
        );
        assert_eq!(
            m.get_string("CanonDR4:RGBCurvePoints"),
            Some("(0,79) (117,172) (242,255)")
        );
        assert_eq!(m.get_integer("CanonDR4:ToneCurveX"), Some(117));
        // The 0x20400.1 flag is ToneCurveOriginal.
        assert_eq!(m.get_string("CanonDR4:ToneCurveOriginal"), Some("No"));
    }

    #[test]
    fn gamma_points_apply_the_log_value_conv() {
        let mut g = vec![0u8; 0x11 * 8];
        let mut put = |index: usize, v: f64| {
            g[index * 8..index * 8 + 8].copy_from_slice(&v.to_le_bytes());
        };
        put(0x05, 3.0); // GammaUnsharpMaskStrength
        put(0x0c, 0.0); // GammaBlackPoint -- `return 0 if $val <= 0`
        put(0x0f, 0.0);
        put(0x10, 16383.0); // GammaCurveOutputRange
        let m = parse_dr4(&dr4(0, &[(0x20a00, 255, [1, 0, 1], g)]));
        assert_eq!(m.get_string("CanonDR4:GammaUnsharpMaskStrength"), Some("3"));
        assert_eq!(m.get_string("CanonDR4:GammaBlackPoint"), Some("+0.000"));
        assert_eq!(
            m.get_string("CanonDR4:GammaCurveOutputRange"),
            Some("0 16383")
        );
        // 4.6875 is the ValueConv's own base, so log2(1) + 1 = 1.
        let mut g2 = vec![0u8; 0x11 * 8];
        g2[0x0c * 8..0x0c * 8 + 8].copy_from_slice(&4.6875f64.to_le_bytes());
        let m2 = parse_dr4(&dr4(0, &[(0x20a00, 255, [1, 0, 1], g2)]));
        assert_eq!(m2.get_string("CanonDR4:GammaBlackPoint"), Some("+1.000"));
    }

    #[test]
    fn shooting_distance_is_a_percentage_of_infinity() {
        // ValueConv $val/10 then sprintf("%.0f%%", $val*100): 10 -> "100%".
        let m = parse_dr4(&dr4(0, &[(0x20701, 13, [1, 0, 1], f64le(10.0))]));
        assert_eq!(m.get_string("CanonDR4:ShootingDistance"), Some("100%"));
    }

    #[test]
    fn hsl_triples_join_with_spaces() {
        let mut hsl = Vec::new();
        for v in [-1.0f64, -0.9, -0.8] {
            hsl.extend_from_slice(&v.to_le_bytes());
        }
        let m = parse_dr4(&dr4(0, &[(0x20910, 38, [1, 0, 1], hsl)]));
        assert_eq!(m.get_string("CanonDR4:RedHSL"), Some("-1 -0.9 -0.8"));
    }

    #[test]
    fn an_unlisted_print_conv_key_reports_unknown() {
        let m = parse_dr4(&dr4(0, &[(0x10200, 8, [1, 1, 1], le32(42))]));
        assert_eq!(
            m.get_string("CanonDR4:WorkColorSpace"),
            Some("Unknown (42)")
        );
        // PrintHex tables report the unknown key in hex.
        let m = parse_dr4(&dr4(0, &[(0x20301, 8, [1, 0, 1], le32(0x42))]));
        assert_eq!(
            m.get_string("CanonDR4:PictureStyle"),
            Some("Unknown (0x42)")
        );
    }

    #[test]
    fn wb_adj_rggb_levels_drops_the_leading_count() {
        // `PrintConv => '$val =~ s/^\d+ //; $val'` -- the 14 is dropped.
        let mut levels = Vec::new();
        for v in [14u32, 2000, 1024, 1024, 1500] {
            levels.extend_from_slice(&v.to_le_bytes());
        }
        let m = parse_dr4(&dr4(0, &[(0x20125, 33, [1, 0, 1], levels)]));
        assert_eq!(
            m.get_string("CanonDR4:WBAdjRGGBLevels"),
            Some("2000 1024 1024 1500")
        );
    }

    #[test]
    fn a_directory_shorter_than_its_entry_count_yields_nothing() {
        // `$err = 1 if $dirLen < 36 + 28 * $numEntries` -- "Invalid DR4
        // directory", and ExifTool returns before extracting anything, header
        // included.
        let mut data = dr4(0x8000_0289, &[(0x10002, 8, [1, 1, 1], le32(0))]);
        data[NUM_ENTRIES_OFFSET..NUM_ENTRIES_OFFSET + 4].copy_from_slice(&999u32.to_le_bytes());
        assert!(parse_dr4(&data).is_empty());
    }

    #[test]
    fn a_truncated_or_foreign_header_is_refused() {
        assert!(parse_dr4(b"IIII\x04\x00\x04\x00").is_empty());
        assert!(parse_dr4(&[0u8; 64]).is_empty());
        let mut wrong = dr4(0, &[(0x10002, 8, [1, 1, 1], le32(0))]);
        wrong[..2].copy_from_slice(b"XX");
        assert!(parse_dr4(&wrong).is_empty());
    }

    #[test]
    fn an_unrecognised_format_code_is_printed_as_hex() {
        // `if (not $format) { $val = unpack 'H*', ... }`. Tag 0x30102 has no
        // PrintConv, so the hex string is what gets reported.
        let m = parse_dr4(&dr4(0, &[(0x30102, 77, [1, 0, 1], vec![0xde, 0xad])]));
        assert_eq!(m.get_string("CanonDR4:CropAspectRatioCustom"), Some("dead"));
    }
}
