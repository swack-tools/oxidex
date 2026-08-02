//! Table-driven walker for plain TIFF-style MakerNote IFDs.
//!
//! Several manufacturers store their MakerNote as one or more ordinary TIFF
//! IFDs whose tag IDs are namespaced *per directory*, so each directory needs
//! its own table. This module walks such a directory, decodes each entry by
//! its TIFF field type, and renders it through the table's ExifTool
//! `PrintConv` equivalent.
//!
//! Olympus stores most of its metadata in nested IFDs (Equipment 0x2010,
//! CameraSettings 0x2020, RawDevelopment 0x2030, RawDev2 0x2031,
//! ImageProcessing 0x2040, FocusInfo 0x2050, RawInfo 0x3000). Each of those
//! is a plain TIFF IFD whose tag IDs are namespaced *per directory* -- tag
//! 0x0100 means `CameraType2` inside Equipment but `PreviewImageValid` inside
//! CameraSettings -- so every directory needs its own table.
//!
//! ExifTool anchors: `Image::ExifTool::Olympus::{Main,Equipment,CameraSettings,
//! RawDevelopment,RawDevelopment2,ImageProcessing,FocusInfo,RawInfo}` and the
//! `MakerNoteOlympus`/`MakerNoteOlympus2` entries in MakerNotes.pm.

use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

/// TIFF field types we decode. Anything else is ignored.
///
/// The `TIFF_` prefix is load-bearing: cbindgen copies every `pub const` in
/// the crate into `api/oxidex.h`, where bare `BYTE`/`SHORT`/`LONG`/`FLOAT`/
/// `DOUBLE` would collide with common C macros.
pub(crate) mod ftype {
    pub const TIFF_BYTE: u16 = 1;
    pub const TIFF_ASCII: u16 = 2;
    pub const TIFF_SHORT: u16 = 3;
    pub const TIFF_LONG: u16 = 4;
    pub const TIFF_RATIONAL: u16 = 5;
    pub const TIFF_SBYTE: u16 = 6;
    pub const TIFF_UNDEF: u16 = 7;
    pub const TIFF_SSHORT: u16 = 8;
    pub const TIFF_SLONG: u16 = 9;
    pub const TIFF_SRATIONAL: u16 = 10;
    pub const TIFF_FLOAT: u16 = 11;
    pub const TIFF_DOUBLE: u16 = 12;
    pub const TIFF_IFD: u16 = 13;
    /// `int64u`, format code 16 in ExifTool's `@formatName` (Exif.pm). Apple
    /// stores `0x0017 LivePhotoVideoIndex` in it on newer iPhones.
    pub const TIFF_LONG8: u16 = 16;
    /// `int64s`, format code 17.
    pub const TIFF_SLONG8: u16 = 17;
}

/// Size in bytes of one element of each TIFF field type.
pub fn type_size(t: u16) -> usize {
    match t {
        ftype::TIFF_BYTE | ftype::TIFF_ASCII | ftype::TIFF_SBYTE | ftype::TIFF_UNDEF => 1,
        ftype::TIFF_SHORT | ftype::TIFF_SSHORT => 2,
        ftype::TIFF_LONG | ftype::TIFF_SLONG | ftype::TIFF_FLOAT | ftype::TIFF_IFD => 4,
        ftype::TIFF_RATIONAL
        | ftype::TIFF_SRATIONAL
        | ftype::TIFF_DOUBLE
        | ftype::TIFF_LONG8
        | ftype::TIFF_SLONG8 => 8,
        _ => 0,
    }
}

/// A decoded IFD entry value.
#[derive(Clone, Debug, PartialEq)]
pub enum OlyVal {
    /// Integer elements (BYTE/SHORT/LONG and their signed variants).
    Int(Vec<i64>),
    /// Rational elements as (numerator, denominator) pairs.
    Rat(Vec<(i64, i64)>),
    /// Floating point elements (FLOAT/DOUBLE).
    Float(Vec<f64>),
    /// Raw bytes (ASCII / UNDEF), before any string trimming.
    Bytes(Vec<u8>),
}

impl OlyVal {
    /// First integer element, if this is an integer value.
    pub fn first_int(&self) -> Option<i64> {
        match self {
            OlyVal::Int(v) => v.first().copied(),
            _ => None,
        }
    }

    /// Integer elements, if this is an integer value.
    pub fn ints(&self) -> Option<&[i64]> {
        match self {
            OlyVal::Int(v) => Some(v),
            _ => None,
        }
    }

    /// Number of elements.
    pub fn len(&self) -> usize {
        match self {
            OlyVal::Int(v) => v.len(),
            OlyVal::Rat(v) => v.len(),
            OlyVal::Float(v) => v.len(),
            OlyVal::Bytes(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes decoded as a NUL-terminated string, the way ExifTool's `string`
    /// conversion does: cut at the first NUL and keep everything before it.
    ///
    /// Bytes that are not valid UTF-8 become `?`, which is what ExifTool's
    /// JSON writer emits when the payload will not transcode (the all-`0xFF`
    /// `CameraID` on OlympusD450Z.jpg prints as 32 question marks).
    pub fn as_string(&self) -> Option<String> {
        match self {
            OlyVal::Bytes(b) => {
                let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
                Some(decode_text(&b[..end]))
            }
            _ => None,
        }
    }

    /// ExifTool's default rendering: elements joined by a single space.
    pub fn print_raw(&self) -> String {
        match self {
            OlyVal::Int(v) => v
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(" "),
            OlyVal::Rat(v) => v
                .iter()
                .map(|&(n, d)| print_rational(n, d))
                .collect::<Vec<_>>()
                .join(" "),
            OlyVal::Float(v) => v.iter().map(|&f| fmt_g15(f)).collect::<Vec<_>>().join(" "),
            OlyVal::Bytes(_) => self.as_string().unwrap_or_default(),
        }
    }
}

/// Decode bytes as text, substituting `?` for anything that is not valid
/// UTF-8 (ExifTool's behaviour when a MakerNote string will not transcode).
pub fn decode_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            let mut out = String::with_capacity(bytes.len());
            let mut rest = bytes;
            loop {
                match std::str::from_utf8(rest) {
                    Ok(s) => {
                        out.push_str(s);
                        break;
                    }
                    Err(e) => {
                        let valid = e.valid_up_to();
                        out.push_str(std::str::from_utf8(&rest[..valid]).unwrap_or(""));
                        let skip = e.error_len().unwrap_or(rest.len() - valid);
                        for _ in 0..skip {
                            out.push('?');
                        }
                        rest = &rest[valid + skip..];
                    }
                }
            }
            out
        }
    }
}

/// ExifTool renders a rational as its quotient (`inf` / `undef` when the
/// denominator is zero), rounded to ten significant digits.
///
/// `GetRational64s`/`GetRational64u` (ExifTool.pm:6107-6120) both end in
/// `RoundFloat($ratNumer / $ratDenom, 10)`, and `RoundFloat` (ExifTool.pm:5960)
/// is `sprintf("%.${sig}g", $val)`. Printing the full `%.15g` expansion instead
/// is a visible mismatch on any quotient that does not terminate: ExifTool
/// prints `AccelerationVector` as `-0.9245480894`, not `-0.924548089390588`.
pub fn print_rational(num: i64, den: i64) -> String {
    if den == 0 {
        return if num == 0 {
            "undef".into()
        } else {
            "inf".into()
        };
    }
    if num % den == 0 {
        return (num / den).to_string();
    }
    crate::core::formatters::numeric_precision::exiftool_rational_number(num as f64 / den as f64)
}

/// Format a float the way Perl stringifies one: `%.15g`, trailing zeros gone.
pub fn fmt_g15(x: f64) -> String {
    crate::core::formatters::numeric_precision::perl_number(x)
}

/// Per-element conversion inside a `PrintConv => [ ... ]` list.
///
/// ExifTool applies element *i* of the list to element *i* of the value and
/// joins the results with `"; "`; elements past the end of the list print raw.
pub enum ElemConv {
    /// `undef` in ExifTool's list -- print the number unchanged.
    Raw,
    /// A PrintConv hash. Misses print `Unknown (N)`.
    Map(&'static [(i64, &'static str)]),
    /// A PrintConv hash with a `BITMASK`. An exact hash hit wins; otherwise
    /// the set bits are named and joined with `", "` (`ExifTool::DecodeBits`),
    /// with unnamed bits rendered as `[n]` and no bits at all as `(none)`.
    Bits {
        map: &'static [(i64, &'static str)],
        bits: &'static [(u32, &'static str)],
    },
    /// A `'"Label $val"'` template: the label, a space, then the number.
    Prefix(&'static str),
    /// A PrintConv hash keyed by a multi-value string (`Relist` groups).
    StrMap(&'static [(&'static str, &'static str)]),
}

/// How a tag's decoded value becomes the string ExifTool prints.
pub enum Conv {
    /// ExifTool's default: elements joined by a space.
    Raw,
    /// ASCII/undef payload printed as text (NUL-terminated, trailing
    /// whitespace kept -- use [`Conv::StrTrim`] when ExifTool strips it).
    Str,
    /// Like [`Conv::Str`] but with trailing whitespace removed.
    StrTrim,
    /// `(Binary data N bytes, use -b option to extract)`.
    Binary,
    /// PrintConv hash on the first integer; misses print `Unknown (N)`.
    Lookup(&'static [(i64, &'static str)]),
    /// Like [`Conv::Lookup`] for a tag ExifTool marks `PrintHex => 1`: a miss
    /// prints `Unknown (0xN)` rather than `Unknown (N)`.
    LookupHex(&'static [(i64, &'static str)]),
    /// PrintConv hash keyed on the space-joined integer list.
    ListLookup(&'static [(&'static str, &'static str)]),
    /// PrintConv hash on the NUL-terminated string payload.
    StrLookup(&'static [(&'static str, &'static str)]),
    /// PrintConv hash with a BITMASK on the first integer.
    Bitmask {
        map: &'static [(i64, &'static str)],
        bits: &'static [(u32, &'static str)],
    },
    /// ExifTool's list form: one conversion per value element, joined by `"; "`.
    List(&'static [ElemConv]),
    /// ExifTool's `Relist` form: the first `group` elements are joined by a
    /// space and fed to `convs[0]`, then each remaining element gets the next
    /// conversion. Used by `Gradation` (`Relist => [ [0..2], 3 ]`).
    Relist {
        group: usize,
        convs: &'static [ElemConv],
    },
    /// Anything else. Returning `None` drops the tag rather than guessing.
    Func(fn(&OlyVal) -> Option<String>),
}

/// One row of an Olympus directory table.
pub struct TagDef {
    pub id: u16,
    pub name: &'static str,
    /// Overrides the on-disk field type when ExifTool's table does
    /// (`Format => 'int16s'` on a tag stored as int16u, for instance).
    pub force_type: Option<u16>,
    pub conv: Conv,
}

impl TagDef {
    pub const fn raw(id: u16, name: &'static str) -> Self {
        TagDef {
            id,
            name,
            force_type: None,
            conv: Conv::Raw,
        }
    }
    pub const fn text(id: u16, name: &'static str) -> Self {
        TagDef {
            id,
            name,
            force_type: None,
            conv: Conv::Str,
        }
    }
    pub const fn text_trim(id: u16, name: &'static str) -> Self {
        TagDef {
            id,
            name,
            force_type: None,
            conv: Conv::StrTrim,
        }
    }
    pub const fn binary(id: u16, name: &'static str) -> Self {
        TagDef {
            id,
            name,
            force_type: None,
            conv: Conv::Binary,
        }
    }
    pub const fn lookup(id: u16, name: &'static str, map: &'static [(i64, &'static str)]) -> Self {
        TagDef {
            id,
            name,
            force_type: None,
            conv: Conv::Lookup(map),
        }
    }
    pub const fn lookup_hex(
        id: u16,
        name: &'static str,
        map: &'static [(i64, &'static str)],
    ) -> Self {
        TagDef {
            id,
            name,
            force_type: None,
            conv: Conv::LookupHex(map),
        }
    }
    pub const fn list_lookup(
        id: u16,
        name: &'static str,
        map: &'static [(&'static str, &'static str)],
    ) -> Self {
        TagDef {
            id,
            name,
            force_type: None,
            conv: Conv::ListLookup(map),
        }
    }
    pub const fn str_lookup(
        id: u16,
        name: &'static str,
        map: &'static [(&'static str, &'static str)],
    ) -> Self {
        TagDef {
            id,
            name,
            force_type: None,
            conv: Conv::StrLookup(map),
        }
    }
    pub const fn func(id: u16, name: &'static str, f: fn(&OlyVal) -> Option<String>) -> Self {
        TagDef {
            id,
            name,
            force_type: None,
            conv: Conv::Func(f),
        }
    }
    pub const fn typed(id: u16, name: &'static str, t: u16) -> Self {
        TagDef {
            id,
            name,
            force_type: Some(t),
            conv: Conv::Raw,
        }
    }
    pub const fn typed_func(
        id: u16,
        name: &'static str,
        t: u16,
        f: fn(&OlyVal) -> Option<String>,
    ) -> Self {
        TagDef {
            id,
            name,
            force_type: Some(t),
            conv: Conv::Func(f),
        }
    }
}

/// Look up a PrintConv value, falling back to ExifTool's `Unknown (N)`.
pub fn lookup_or_unknown(map: &[(i64, &str)], v: i64) -> String {
    match map.iter().find(|(k, _)| *k == v) {
        Some((_, s)) => (*s).to_string(),
        None => format!("Unknown ({})", v),
    }
}

/// Same as [`lookup_or_unknown`] for the space-joined list form.
pub fn list_lookup_or_unknown(map: &[(&str, &str)], key: &str) -> String {
    match map.iter().find(|(k, _)| *k == key) {
        Some((_, s)) => (*s).to_string(),
        None => format!("Unknown ({})", key),
    }
}

/// Port of `Image::ExifTool::DecodeBits` for a single 32-bit value.
pub fn decode_bits(v: i64, bits: &[(u32, &str)]) -> String {
    let mut out: Vec<String> = Vec::new();
    for i in 0..32u32 {
        if v & (1i64 << i) == 0 {
            continue;
        }
        match bits.iter().find(|(b, _)| *b == i) {
            Some((_, name)) => out.push((*name).to_string()),
            None => out.push(format!("[{}]", i)),
        }
    }
    if out.is_empty() {
        "(none)".to_string()
    } else {
        out.join(", ")
    }
}

/// Render one element of a list PrintConv.
fn apply_elem(conv: &ElemConv, raw: &str) -> String {
    match conv {
        ElemConv::Raw => raw.to_string(),
        ElemConv::Map(map) => match raw.parse::<i64>() {
            Ok(n) => lookup_or_unknown(map, n),
            Err(_) => format!("Unknown ({})", raw),
        },
        ElemConv::Bits { map, bits } => match raw.parse::<i64>() {
            Ok(n) => match map.iter().find(|(k, _)| *k == n) {
                Some((_, s)) => (*s).to_string(),
                None => decode_bits(n, bits),
            },
            Err(_) => format!("Unknown ({})", raw),
        },
        ElemConv::Prefix(label) => format!("{} {}", label, raw),
        ElemConv::StrMap(map) => list_lookup_or_unknown(map, raw),
    }
}

/// Apply an ExifTool list PrintConv: element *i* of `convs` to element *i* of
/// the value, joined by `"; "`. Elements past the end of `convs` print raw.
fn apply_list(convs: &[ElemConv], val: &OlyVal) -> String {
    let elems: Vec<String> = match val {
        OlyVal::Int(v) => v.iter().map(|n| n.to_string()).collect(),
        OlyVal::Rat(v) => v.iter().map(|&(n, d)| print_rational(n, d)).collect(),
        OlyVal::Float(v) => v.iter().map(|&f| fmt_g15(f)).collect(),
        OlyVal::Bytes(_) => return val.print_raw(),
    };
    elems
        .iter()
        .enumerate()
        .map(|(i, raw)| match convs.get(i) {
            Some(c) => apply_elem(c, raw),
            None => raw.clone(),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Render one decoded value through its table conversion.
pub fn apply_conv(def: &TagDef, val: &OlyVal) -> Option<String> {
    match &def.conv {
        Conv::Raw => Some(val.print_raw()),
        Conv::Str => val.as_string(),
        Conv::StrTrim => val.as_string().map(|s| s.trim_end().to_string()),
        // ExifTool's `Binary => 1` reports the length of the *value string*,
        // not the element count: FaceDetectArea on OlympusE-M1.jpg is 192
        // int16s values, whose space-joined rendering is 383 characters.
        Conv::Binary => {
            let len = match val {
                OlyVal::Bytes(b) => b.len(),
                other => other.print_raw().len(),
            };
            Some(format!(
                "(Binary data {} bytes, use -b option to extract)",
                len
            ))
        }
        // ExifTool hands the PrintConv hash the *whole* value string, so a
        // multi-element value misses every scalar key and prints e.g.
        // "Unknown (0 0)" (Olympus_u20D.jpg FlashDevice) rather than
        // "Unknown (0)".
        Conv::Lookup(map) => {
            let raw = val.print_raw();
            Some(match raw.parse::<i64>() {
                Ok(n) => lookup_or_unknown(map, n),
                Err(_) => format!("Unknown ({})", raw),
            })
        }
        Conv::LookupHex(map) => {
            let raw = val.print_raw();
            Some(match raw.parse::<i64>() {
                Ok(n) => match map.iter().find(|(k, _)| *k == n) {
                    Some((_, s)) => (*s).to_string(),
                    None => format!("Unknown (0x{:x})", n),
                },
                Err(_) => format!("Unknown ({})", raw),
            })
        }
        Conv::ListLookup(map) => match val {
            OlyVal::Int(_) => Some(list_lookup_or_unknown(map, &val.print_raw())),
            _ => None,
        },
        Conv::StrLookup(map) => val.as_string().map(|s| {
            let s = s.trim_end();
            match map.iter().find(|(k, _)| *k == s) {
                Some((_, v)) => (*v).to_string(),
                None => format!("Unknown ({})", s),
            }
        }),
        Conv::Bitmask { map, bits } => {
            let raw = val.print_raw();
            Some(match raw.parse::<i64>() {
                Ok(n) => match map.iter().find(|(k, _)| *k == n) {
                    Some((_, s)) => (*s).to_string(),
                    None => decode_bits(n, bits),
                },
                Err(_) => format!("Unknown ({})", raw),
            })
        }
        Conv::List(convs) => Some(apply_list(convs, val)),
        Conv::Relist { group, convs } => {
            let ints = val.ints()?;
            if ints.len() < *group {
                return None;
            }
            let head = ints[..*group]
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            let mut parts = vec![match convs.first() {
                Some(c) => apply_elem(c, &head),
                None => head,
            }];
            for (i, n) in ints[*group..].iter().enumerate() {
                let raw = n.to_string();
                parts.push(match convs.get(i + 1) {
                    Some(c) => apply_elem(c, &raw),
                    None => raw,
                });
            }
            Some(parts.join("; "))
        }
        Conv::Func(f) => f(val),
    }
}

/// A single raw IFD entry.
#[derive(Clone, Copy, Debug)]
pub struct RawEntry {
    pub tag_id: u16,
    pub field_type: u16,
    pub count: u32,
    pub value_offset: u32,
    /// Byte offset of the entry's 4-byte value field inside `data`.
    pub value_field_pos: usize,
}

/// Parse an IFD header at `ifd_start` and return its entries.
///
/// Returns `None` when the directory does not look like an IFD (implausible
/// entry count or a header that runs off the end of the buffer), which is how
/// we avoid emitting tags decoded from garbage.
pub fn read_ifd(data: &[u8], ifd_start: usize, order: ByteOrder) -> Option<Vec<RawEntry>> {
    if ifd_start + 2 > data.len() {
        return None;
    }
    let reader = EndianReader::new(data, order.to_io_byte_order());
    let count = reader.u16_at(ifd_start)? as usize;
    if count == 0 || count > 512 {
        return None;
    }
    if ifd_start + 2 + count * 12 > data.len() {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let off = ifd_start + 2 + i * 12;
        out.push(RawEntry {
            tag_id: reader.u16_at(off)?,
            field_type: reader.u16_at(off + 2)?,
            count: reader.u32_at(off + 4)?,
            value_offset: reader.u32_at(off + 8)?,
            value_field_pos: off + 8,
        });
    }
    Some(out)
}

/// Decode an entry's payload.
///
/// `base` is added to every stored value offset to land inside `data`. It is
/// signed because the older `OLYMP\0` MakerNotes store TIFF-header-relative
/// offsets while we only hold the MakerNote slice, so the correction is
/// negative (see `fix_base` in the parser). Values of four bytes or fewer
/// live in the entry itself and ignore `base` entirely.
pub fn decode_entry(
    data: &[u8],
    entry: &RawEntry,
    base: Option<i64>,
    order: ByteOrder,
    force_type: Option<u16>,
) -> Option<OlyVal> {
    decode_entry_with_floor(data, entry, base, order, force_type, 0)
}

/// [`decode_entry`] with a lower bound on where an out-of-line value may
/// start.
///
/// The older `OLYMP\0` MakerNotes store offsets measured from the TIFF header
/// while we hold only the MakerNote slice, so a stored offset can land
/// *inside* our buffer by coincidence and decode unrelated bytes. Every real
/// value block sits after its own directory, so refusing anything that starts
/// before the end of the IFD turns those coincidences back into dropped tags.
pub fn decode_entry_with_floor(
    data: &[u8],
    entry: &RawEntry,
    base: Option<i64>,
    order: ByteOrder,
    force_type: Option<u16>,
    floor: usize,
) -> Option<OlyVal> {
    let stored_type = entry.field_type;
    let ft = force_type.unwrap_or(stored_type);
    let elem = type_size(ft);
    if elem == 0 {
        return None;
    }
    // A forced format can reinterpret the same bytes with a different element
    // size (ExifTool's `Format => 'int16s'` over an int8u array, say), so the
    // byte length always comes from the *stored* type.
    let stored_elem = type_size(stored_type).max(1);
    let byte_len = (entry.count as usize).checked_mul(stored_elem)?;
    // A zero count is legal and ExifTool prints it as an empty value
    // (RawDev2's 0x0108 on OlympusE-M1.jpg).
    if byte_len > data.len() {
        return None;
    }
    let bytes: &[u8] = if byte_len <= 4 {
        &data[entry.value_field_pos..entry.value_field_pos + byte_len]
    } else {
        // `None` means the caller could not establish where stored offsets
        // point, so out-of-line values stay unread rather than being decoded
        // from whatever the raw offset lands on.
        let start = i64::from(entry.value_offset).checked_add(base?)?;
        if start < 0 {
            return None;
        }
        let start = start as usize;
        if start < floor {
            return None;
        }
        let end = start.checked_add(byte_len)?;
        if end > data.len() {
            return None;
        }
        &data[start..end]
    };
    decode_bytes(bytes, ft, order)
}

/// Decode a byte slice as `count = bytes.len() / type_size(ft)` elements.
pub fn decode_bytes(bytes: &[u8], ft: u16, order: ByteOrder) -> Option<OlyVal> {
    let elem = type_size(ft);
    if elem == 0 {
        return None;
    }
    if ft == ftype::TIFF_ASCII || ft == ftype::TIFF_UNDEF {
        return Some(OlyVal::Bytes(bytes.to_vec()));
    }
    let n = bytes.len() / elem;
    let le = matches!(order, ByteOrder::LittleEndian);
    let rd16 = |c: &[u8]| {
        if le {
            u16::from_le_bytes([c[0], c[1]])
        } else {
            u16::from_be_bytes([c[0], c[1]])
        }
    };
    let rd32 = |c: &[u8]| {
        if le {
            u32::from_le_bytes([c[0], c[1], c[2], c[3]])
        } else {
            u32::from_be_bytes([c[0], c[1], c[2], c[3]])
        }
    };
    match ft {
        ftype::TIFF_BYTE => Some(OlyVal::Int(bytes.iter().map(|&b| b as i64).collect())),
        ftype::TIFF_SBYTE => Some(OlyVal::Int(bytes.iter().map(|&b| b as i8 as i64).collect())),
        ftype::TIFF_SHORT => Some(OlyVal::Int(
            (0..n).map(|i| rd16(&bytes[i * 2..]) as i64).collect(),
        )),
        ftype::TIFF_SSHORT => Some(OlyVal::Int(
            (0..n)
                .map(|i| rd16(&bytes[i * 2..]) as i16 as i64)
                .collect(),
        )),
        ftype::TIFF_LONG | ftype::TIFF_IFD => Some(OlyVal::Int(
            (0..n).map(|i| rd32(&bytes[i * 4..]) as i64).collect(),
        )),
        ftype::TIFF_SLONG => Some(OlyVal::Int(
            (0..n)
                .map(|i| rd32(&bytes[i * 4..]) as i32 as i64)
                .collect(),
        )),
        ftype::TIFF_RATIONAL => Some(OlyVal::Rat(
            (0..n)
                .map(|i| {
                    (
                        rd32(&bytes[i * 8..]) as i64,
                        rd32(&bytes[i * 8 + 4..]) as i64,
                    )
                })
                .collect(),
        )),
        ftype::TIFF_SRATIONAL => Some(OlyVal::Rat(
            (0..n)
                .map(|i| {
                    (
                        rd32(&bytes[i * 8..]) as i32 as i64,
                        rd32(&bytes[i * 8 + 4..]) as i32 as i64,
                    )
                })
                .collect(),
        )),
        ftype::TIFF_FLOAT => Some(OlyVal::Float(
            (0..n)
                .map(|i| f32::from_bits(rd32(&bytes[i * 4..])) as f64)
                .collect(),
        )),
        ftype::TIFF_DOUBLE => Some(OlyVal::Float(
            (0..n)
                .map(|i| {
                    let hi = rd32(&bytes[i * 8..]) as u64;
                    let lo = rd32(&bytes[i * 8 + 4..]) as u64;
                    let bits = if le { (lo << 32) | hi } else { (hi << 32) | lo };
                    f64::from_bits(bits)
                })
                .collect(),
        )),
        ftype::TIFF_LONG8 | ftype::TIFF_SLONG8 => {
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let hi = rd32(&bytes[i * 8..]) as u64;
                let lo = rd32(&bytes[i * 8 + 4..]) as u64;
                let raw = if le { (lo << 32) | hi } else { (hi << 32) | lo };
                if ft == ftype::TIFF_SLONG8 {
                    out.push(raw as i64);
                } else {
                    // An int64u above i64::MAX has no representation here, and
                    // printing it as a negative would be worse than omitting
                    // it, so the whole value is dropped instead.
                    out.push(i64::try_from(raw).ok()?);
                }
            }
            Some(OlyVal::Int(out))
        }
        _ => None,
    }
}

/// Walk one directory, emitting `<prefix>:<TagName>` for every table hit.
///
/// Nested directories are flattened into the single manufacturer group,
/// exactly as ExifTool does: `exiftool -G1` prints Olympus Equipment's
/// `CameraType2` as `[Olympus] CameraType2`, not under a per-sub-IFD group.
pub fn walk_directory(
    data: &[u8],
    ifd_start: usize,
    base: Option<i64>,
    order: ByteOrder,
    prefix: &str,
    table: &[TagDef],
    tags: &mut HashMap<String, String>,
) {
    let Some(entries) = read_ifd(data, ifd_start, order) else {
        return;
    };
    let floor = ifd_start + 2 + entries.len() * 12 + 4;
    for entry in &entries {
        let Some(def) = table.iter().find(|d| d.id == entry.tag_id) else {
            continue;
        };
        let Some(val) = decode_entry_with_floor(data, entry, base, order, def.force_type, floor)
        else {
            continue;
        };
        if let Some(printed) = apply_conv(def, &val) {
            tags.insert(format!("{}:{}", prefix, def.name), printed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_prints_like_perl() {
        assert_eq!(print_rational(2160, 100), "21.6");
        assert_eq!(print_rational(203, 256), "0.79296875");
        assert_eq!(print_rational(0, 1), "0");
        // `RoundFloat($num/$den, 10)`, not the full `%.15g` expansion:
        // `perl -e 'printf "%.10g", 1/3'` prints 0.3333333333.
        assert_eq!(print_rational(1, 3), "0.3333333333");
        // Apple_iPhone13Pro.jpg's AccelerationVector[0], -48487/52444, which
        // `exiftool -a -G1 -s` prints as -0.9245480894.
        assert_eq!(print_rational(-48487, 52444), "-0.9245480894");
        assert_eq!(print_rational(1, 0), "inf");
        assert_eq!(print_rational(0, 0), "undef");
    }

    #[test]
    fn g15_matches_perl_default_stringification() {
        assert_eq!(fmt_g15(21.6), "21.6");
        assert_eq!(fmt_g15(0.0), "0");
        assert_eq!(fmt_g15(-0.5), "-0.5");
        assert_eq!(fmt_g15(1234567.0), "1234567");
    }

    #[test]
    fn ascii_value_cuts_at_first_nul() {
        let v = OlyVal::Bytes(b"BHP200308   \0junk".to_vec());
        assert_eq!(v.as_string().unwrap(), "BHP200308   ");
    }

    #[test]
    fn short_values_are_read_inline_from_the_entry() {
        // count*size == 2, so the value lives in the entry's own 4-byte field.
        let mut data = vec![0u8; 32];
        data[20] = 0x07;
        data[21] = 0x00;
        let entry = RawEntry {
            tag_id: 0x0207,
            field_type: ftype::TIFF_SHORT,
            count: 1,
            value_offset: 7,
            value_field_pos: 20,
        };
        let v = decode_entry(&data, &entry, Some(0), ByteOrder::LittleEndian, None).unwrap();
        assert_eq!(v, OlyVal::Int(vec![7]));
    }

    #[test]
    fn long_values_are_read_from_base_plus_offset() {
        let mut data = vec![0u8; 64];
        data[40..48].copy_from_slice(&[1, 0, 2, 0, 3, 0, 4, 0]);
        let entry = RawEntry {
            tag_id: 0x0100,
            field_type: ftype::TIFF_SHORT,
            count: 4,
            value_offset: 40,
            value_field_pos: 8,
        };
        let v = decode_entry(&data, &entry, Some(0), ByteOrder::LittleEndian, None).unwrap();
        assert_eq!(v.print_raw(), "1 2 3 4");
        assert!(decode_entry(&data, &entry, None, ByteOrder::LittleEndian, None).is_none());
    }

    #[test]
    fn implausible_entry_counts_are_rejected() {
        // 0x0700 == 1792 entries in a 16-byte buffer.
        let data = vec![0x00, 0x07, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(read_ifd(&data, 0, ByteOrder::LittleEndian).is_none());
    }
}
