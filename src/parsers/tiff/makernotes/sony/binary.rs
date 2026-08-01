//! Binary-data sub-directory machinery shared by the Sony MakerNote tables.
//!
//! ExifTool stores most of Sony's interesting metadata in `ProcessBinaryData`
//! tables: a blob whose tags are addressed by *index*, where the index is
//! multiplied by the size of the table's default `FORMAT` to reach a byte
//! offset. A tag may override the format for its own read without changing how
//! its offset is computed. This module reproduces that addressing exactly so a
//! table transcribed from `Sony.pm` keeps its original keys and needs no
//! hand-computed byte offsets to drift out of sync.

pub use crate::core::formatters::exif_print_conv::print_exposure_time;
use crate::io::EndianReader;
use std::collections::HashMap;

/// Scalar layouts a binary-data tag can be read as.
///
/// Names mirror ExifTool's `Format` strings so a transcribed table reads the
/// same as its `Sony.pm` original.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fmt {
    /// `int8u`
    U8,
    /// `int8s`
    I8,
    /// `int16u`
    U16,
    /// `int16s`
    I16,
    /// `int32u`
    U32,
    /// `int32s`
    I32,
}

impl Fmt {
    /// Bytes one value of this format occupies.
    pub const fn size(self) -> usize {
        match self {
            Fmt::U8 | Fmt::I8 => 1,
            Fmt::U16 | Fmt::I16 => 2,
            Fmt::U32 | Fmt::I32 => 4,
        }
    }
}

/// How a decoded integer becomes the string ExifTool prints.
pub enum Conv {
    /// Print the integer itself.
    Int,
    /// `PrintConv` hash. Unmatched values print `Unknown (N)`, matching
    /// ExifTool's fallback for a hash with no `OTHER`.
    Lookup(&'static [(i64, &'static str)]),
    /// `PrintConv` hash written with `PrintHex`, so unmatched values print
    /// `Unknown (0xN)`.
    LookupHex(&'static [(i64, &'static str)]),
    /// Anything else: a `ValueConv`/`PrintConv` pair expressed as Rust.
    /// Returning `None` suppresses the tag.
    Fn(fn(i64) -> Option<String>),
}

impl Conv {
    /// Applies the conversion, yielding the string ExifTool would print.
    pub fn apply(&self, value: i64) -> Option<String> {
        match self {
            Conv::Int => Some(value.to_string()),
            Conv::Lookup(map) => Some(lookup(map, value).unwrap_or_else(|| unknown(value))),
            Conv::LookupHex(map) => Some(lookup(map, value).unwrap_or_else(|| unknown_hex(value))),
            Conv::Fn(f) => f(value),
        }
    }
}

/// One entry of a binary-data table.
pub struct BinTag {
    /// ExifTool's tag key: an index in units of the table's default format.
    pub index: usize,
    /// Tag name, without group prefix.
    pub name: &'static str,
    /// Per-tag `Format` override, or `None` to use the table default.
    pub fmt: Option<Fmt>,
    /// `Mask` applied to the raw value before conversion, if any.
    pub mask: Option<i64>,
    /// Value/print conversion.
    pub conv: Conv,
}

impl BinTag {
    /// A tag read in the table's default format and printed as an integer.
    pub const fn int(index: usize, name: &'static str) -> Self {
        BinTag {
            index,
            name,
            fmt: None,
            mask: None,
            conv: Conv::Int,
        }
    }

    /// A tag read in the table's default format with a `PrintConv` hash.
    pub const fn map(index: usize, name: &'static str, m: &'static [(i64, &'static str)]) -> Self {
        BinTag {
            index,
            name,
            fmt: None,
            mask: None,
            conv: Conv::Lookup(m),
        }
    }

    /// A tag read in the table's default format with a Rust-expressed
    /// `ValueConv`/`PrintConv`.
    pub const fn conv(index: usize, name: &'static str, f: fn(i64) -> Option<String>) -> Self {
        BinTag {
            index,
            name,
            fmt: None,
            mask: None,
            conv: Conv::Fn(f),
        }
    }

    /// Overrides the read format without changing how the offset is computed,
    /// exactly as a per-tag `Format` does in ExifTool.
    pub const fn with_fmt(mut self, fmt: Fmt) -> Self {
        self.fmt = Some(fmt);
        self
    }

    /// Applies a `Mask` to the raw value before conversion.
    pub const fn with_mask(mut self, mask: i64) -> Self {
        self.mask = Some(mask);
        self
    }

    /// Marks the `PrintConv` hash as `PrintHex`, so unmatched values print in
    /// hexadecimal.
    pub const fn hex(mut self) -> Self {
        if let Conv::Lookup(m) = self.conv {
            self.conv = Conv::LookupHex(m);
        }
        self
    }
}

/// A `ProcessBinaryData` table.
pub struct BinTable {
    /// Table's default `FORMAT`; also the multiplier turning an index into a
    /// byte offset.
    pub format: Fmt,
    /// Byte order the sub-directory is read in. Sony sets this per table and it
    /// is frequently the opposite of the file's own TIFF byte order.
    pub big_endian: bool,
    /// The tags, in any order.
    pub tags: &'static [BinTag],
}

impl BinTable {
    /// Reads every tag whose bytes fall inside `data` and inserts the printed
    /// values into `tags` under `group:Name`.
    ///
    /// Entries that run past the end of the blob are skipped rather than
    /// clamped: ExifTool ignores out-of-range binary-data tags, and a clamped
    /// read would invent a value the file does not contain.
    pub fn extract(&self, data: &[u8], group: &str, tags: &mut HashMap<String, String>) {
        let reader = if self.big_endian {
            EndianReader::big_endian(data)
        } else {
            EndianReader::little_endian(data)
        };
        let increment = self.format.size();

        for tag in self.tags {
            let fmt = tag.fmt.unwrap_or(self.format);
            let offset = tag.index * increment;
            let Some(raw) = read_scalar(&reader, offset, fmt) else {
                continue;
            };
            let value = match tag.mask {
                Some(mask) => raw & mask,
                None => raw,
            };
            if let Some(printed) = tag.conv.apply(value) {
                tags.insert(format!("{}:{}", group, tag.name), printed);
            }
        }
    }
}

/// Reads one scalar of `fmt` at `offset`, or `None` when it does not fit.
pub fn read_scalar(reader: &EndianReader<'_>, offset: usize, fmt: Fmt) -> Option<i64> {
    Some(match fmt {
        Fmt::U8 => reader.u8_at(offset)? as i64,
        Fmt::I8 => reader.u8_at(offset)? as i8 as i64,
        Fmt::U16 => reader.u16_at(offset)? as i64,
        Fmt::I16 => reader.u16_at(offset)? as i16 as i64,
        Fmt::U32 => reader.u32_at(offset)? as i64,
        Fmt::I32 => reader.u32_at(offset)? as i32 as i64,
    })
}

/// Looks a value up in a `PrintConv` hash.
pub fn lookup(map: &'static [(i64, &'static str)], value: i64) -> Option<String> {
    map.iter()
        .find(|(k, _)| *k == value)
        .map(|(_, v)| (*v).to_string())
}

/// ExifTool's rendering of a value its `PrintConv` hash does not cover.
pub fn unknown(value: i64) -> String {
    format!("Unknown ({})", value)
}

/// The same, for a hash declared `PrintHex`.
pub fn unknown_hex(value: i64) -> String {
    format!("Unknown (0x{:x})", value)
}

/// ExifTool's `+N` / `N` rendering used by every Sony adjustment slider
/// (Contrast, Saturation, Sharpness, ...).
pub fn signed_adjustment(value: i64) -> String {
    if value > 0 {
        format!("+{}", value)
    } else {
        value.to_string()
    }
}

/// Prints a floating point value the way Perl's `"$val"` does: no trailing
/// zeros, no trailing decimal point.
pub fn print_float(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }
    let mut s = format!("{:.10}", value);
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

/// ExifTool's `Image::ExifTool::Exif::PrintFNumber`: one decimal place, or two
/// below f/1.0. The decimal is not trimmed, so f/8 prints as "8.0".
pub fn print_f_number(f: f64) -> String {
    if f <= 0.0 {
        return print_float(f);
    }
    if f < 1.0 {
        format!("{:.2}", f)
    } else {
        format!("{:.1}", f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static SAMPLE: &[BinTag] = &[
        BinTag::int(0, "First"),
        BinTag::map(2, "Second", &[(7, "Seven")]),
    ];

    static TABLE: BinTable = BinTable {
        format: Fmt::U16,
        big_endian: true,
        tags: SAMPLE,
    };

    #[test]
    fn index_is_scaled_by_the_table_format() {
        // index 2 in an int16u table is byte offset 4, not byte offset 2.
        let data = [0x00, 0x01, 0xff, 0xff, 0x00, 0x07];
        let mut tags = HashMap::new();
        TABLE.extract(&data, "Sony", &mut tags);
        assert_eq!(tags.get("Sony:First"), Some(&"1".to_string()));
        assert_eq!(tags.get("Sony:Second"), Some(&"Seven".to_string()));
    }

    #[test]
    fn out_of_range_tags_are_dropped_not_clamped() {
        let data = [0x00, 0x01];
        let mut tags = HashMap::new();
        TABLE.extract(&data, "Sony", &mut tags);
        assert_eq!(tags.get("Sony:First"), Some(&"1".to_string()));
        assert!(!tags.contains_key("Sony:Second"));
    }

    #[test]
    fn unmatched_printconv_values_report_unknown() {
        assert_eq!(unknown(5), "Unknown (5)");
        assert_eq!(unknown_hex(0x1f), "Unknown (0x1f)");
    }

    #[test]
    fn exposure_time_matches_exiftool_rendering() {
        assert_eq!(print_exposure_time(1.0 / 13.0), "1/13");
        assert_eq!(print_exposure_time(2.0), "2");
        assert_eq!(print_exposure_time(0.4), "0.4");
    }

    #[test]
    fn signed_adjustment_prefixes_positive_values() {
        assert_eq!(signed_adjustment(3), "+3");
        assert_eq!(signed_adjustment(0), "0");
        assert_eq!(signed_adjustment(-3), "-3");
    }
}
