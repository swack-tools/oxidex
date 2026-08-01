//! FlashPix property sets stored in OLE compound documents (MS-OLEPS).
//!
//! Microsoft Office documents (`.doc`, `.xls`, `.ppt`, `.msg`, ...) are OLE
//! compound files whose document metadata lives in two well-known streams:
//!
//! * `\x05SummaryInformation` -- Title, Author, CreateDate, ...
//! * `\x05DocumentSummaryInformation` -- Company, Slides, AppVersion, ... plus a
//!   second, *user-defined* property section carrying a name dictionary.
//!
//! ExifTool reports all of these under the `FlashPix` group (they use the same
//! property-set encoding as the FlashPix image format), so this module emits
//! `FlashPix:*` keys to match.
//!
//! Reference: `Image::ExifTool::FlashPix` (`ProcessProperties`, `ReadFPXValue`)
//! and `Image::ExifTool::Microsoft::%codePage`.
//!
//! Tag IDs here are taken from `FlashPix.pm` directly and are deliberately
//! written at their full 32-bit width: `LocaleIndicator` is `0x80000000`, which
//! must never be confused with `Dictionary` (`0x00000000`). See
//! `locale_indicator_id_is_not_truncated` in the tests below.

use std::collections::HashMap;

use crate::core::{MetadataMap, TagValue};
use crate::io::{ByteOrder, EndianReader};

/// Which property table a stream's IDs should be resolved against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PropertySet {
    /// `\x05SummaryInformation`
    SummaryInfo,
    /// `\x05DocumentSummaryInformation` (two sections: standard + user-defined)
    DocumentInfo,
    /// `\x05Extension List` (`%FlashPix::Extensions`). Its IDs carry an
    /// extension number in the high bits, so they are matched under a mask.
    Extensions,
    /// `\x05Audio Info` (`%FlashPix::AudioInfo`). The table declares no IDs of
    /// its own; only the universal `CodePage` / `LocaleIndicator` apply.
    AudioInfo,
}

/// Byte-order marks that may open a property set (MS-OLEPS 2.20).
const BOM_LITTLE_ENDIAN: [u8; 2] = [0xFE, 0xFF];
const BOM_BIG_ENDIAN: [u8; 2] = [0xFF, 0xFE];

/// Smallest possible property-set header (BOM..first section offset).
const MIN_PROPERTY_SET_LEN: usize = 48;

/// `VT_VECTOR` flag in the high nibble of a property type.
const VT_VECTOR: u32 = 0x1000;

/// Guards against a hostile `VT_VARIANT` nesting chain.
const MAX_VARIANT_DEPTH: usize = 8;

/// Seconds between the FILETIME epoch (1601-01-01) and the Unix epoch.
const FILETIME_UNIX_EPOCH_SECS: f64 = 134_774.0 * 86_400.0;

/// ExifTool's sanity window for a converted FILETIME: 100 years past 1970.
const HUNDRED_YEARS_SECS: f64 = 100.0 * 365.0 * 86_400.0;

/// Days between the OLE `VT_DATE` epoch (1899-12-30) and the Unix epoch.
const VT_DATE_EPOCH_DAYS: f64 = 25_569.0;

/// A value read out of a property set, before tag-specific conversion.
#[derive(Debug, Clone, PartialEq)]
enum FpxValue {
    Int(i64),
    Real(f64),
    Text(String),
    /// Raw bytes for `VT_BLOB` / `VT_CF`, kept so tags such as `Hyperlinks`
    /// can re-parse them.
    Bytes(Vec<u8>),
}

impl FpxValue {
    fn into_tag_value(self) -> TagValue {
        match self {
            FpxValue::Int(v) => TagValue::Integer(v),
            FpxValue::Real(v) => TagValue::Float(v),
            FpxValue::Text(v) => TagValue::String(v),
            FpxValue::Bytes(v) => TagValue::String(binary_placeholder(v.len())),
        }
    }

    fn as_int(&self) -> Option<i64> {
        match self {
            FpxValue::Int(v) => Some(*v),
            FpxValue::Real(v) => Some(*v as i64),
            _ => None,
        }
    }
}

/// ExifTool's stand-in for binary values that `-b` was not requested for.
fn binary_placeholder(len: usize) -> String {
    format!("(Binary data {} bytes, use -b option to extract)", len)
}

/// Supported OLE property formats (`%oleFormat` in FlashPix.pm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fmt {
    Int16s,
    Int32s,
    Float,
    Double,
    Int8s,
    Int8u,
    Int16u,
    Int32u,
    Int64s,
    Int64u,
    Date,
    Bstr,
    Variant,
    Lpstr,
    Lpwstr,
    Filetime,
    Blob,
    Cf,
    Clsid,
}

impl Fmt {
    fn from_code(code: u32) -> Option<Fmt> {
        Some(match code {
            2 => Fmt::Int16s,
            3 | 10 => Fmt::Int32s, // 10 = VT_ERROR
            4 => Fmt::Float,
            5 => Fmt::Double,
            7 => Fmt::Date,
            8 => Fmt::Bstr,
            11 => Fmt::Int16s, // VT_BOOL
            12 => Fmt::Variant,
            16 => Fmt::Int8s,
            17 => Fmt::Int8u,
            18 => Fmt::Int16u,
            19 => Fmt::Int32u,
            20 => Fmt::Int64s,
            21 => Fmt::Int64u,
            30 => Fmt::Lpstr,
            31 => Fmt::Lpwstr,
            64 => Fmt::Filetime,
            65 => Fmt::Blob,
            71 => Fmt::Cf,
            72 => Fmt::Clsid,
            _ => return None,
        })
    }

    /// Fixed-width formats read straight out of the buffer; `None` for the
    /// `VT_*` formats that carry their own length or need conversion.
    fn scalar_size(self) -> Option<usize> {
        Some(match self {
            Fmt::Int8s | Fmt::Int8u => 1,
            Fmt::Int16s | Fmt::Int16u => 2,
            Fmt::Int32s | Fmt::Int32u | Fmt::Float => 4,
            Fmt::Double | Fmt::Int64s | Fmt::Int64u => 8,
            _ => return None,
        })
    }

    /// Bytes consumed by the fixed part of a `VT_*` value (`%oleFormatSize`).
    fn header_size(self) -> usize {
        match self {
            Fmt::Date | Fmt::Filetime => 8,
            Fmt::Clsid => 16,
            _ => 4,
        }
    }
}

/// Parse one property-set stream, inserting `FlashPix:*` tags into `metadata`.
///
/// `DocumentSummaryInformation` carries a second section holding user-defined
/// properties, which ExifTool flags with `Multi => 1`; that is handled here by
/// walking up to two sections for [`PropertySet::DocumentInfo`].
pub(crate) fn parse_property_stream(
    data: &[u8],
    set: PropertySet,
    metadata: &mut MetadataMap,
) -> bool {
    if data.len() < MIN_PROPERTY_SET_LEN {
        return false;
    }

    let order = match [data[0], data[1]] {
        BOM_LITTLE_ENDIAN => ByteOrder::Little,
        BOM_BIG_ENDIAN => ByteOrder::Big,
        _ => return false,
    };
    let reader = EndianReader::new(data, order);
    let dir_end = data.len();

    let Some(first_section) = reader.u32_at(44) else {
        return false;
    };
    let mut pos = first_section as usize;
    if pos < MIN_PROPERTY_SET_LEN {
        return false;
    }

    let sections = if set == PropertySet::DocumentInfo {
        2
    } else {
        1
    };
    let mut found_any = false;

    for _ in 0..sections {
        if pos + 8 > dir_end {
            break;
        }
        let Some(section_size) = reader.u32_at(pos) else {
            break;
        };
        if section_size == 0 {
            break;
        }
        let Some(num_entries) = reader.u32_at(pos + 4) else {
            break;
        };
        let num_entries = num_entries as usize;
        // Bounds the entry table before any of it is read (ExifTool's
        // "Truncated property list" check).
        match num_entries
            .checked_mul(8)
            .and_then(|n| n.checked_add(pos + 8))
        {
            Some(end) if end <= dir_end => {}
            _ => break,
        }

        found_any |= parse_section(&reader, data, pos, num_entries, dir_end, set, metadata);

        pos = match pos.checked_add(section_size as usize) {
            Some(next) => next,
            None => break,
        };
    }

    found_any
}

/// Walk the entry table of a single property section.
fn parse_section(
    reader: &EndianReader<'_>,
    data: &[u8],
    section: usize,
    num_entries: usize,
    dir_end: usize,
    set: PropertySet,
    metadata: &mut MetadataMap,
) -> bool {
    // Maps a numeric property ID to the name declared by this section's
    // dictionary (property 0). Only user-defined sections carry one.
    let mut dictionary: HashMap<u32, String> = HashMap::new();
    let mut code_page: Option<i64> = None;
    let mut found_any = false;

    for index in 0..num_entries {
        let entry = section + 8 + index * 8;
        let (Some(tag), Some(offset)) = (reader.u32_at(entry), reader.u32_at(entry + 4)) else {
            break;
        };
        let offset = offset as usize;
        let Some(val_start) = section.checked_add(4).and_then(|p| p.checked_add(offset)) else {
            break;
        };
        if val_start >= dir_end {
            break;
        }
        let Some(vtype) = section.checked_add(offset).and_then(|p| reader.u32_at(p)) else {
            break;
        };

        let mut val_pos = val_start;

        if tag == 0 {
            // Property 0 is the dictionary: `vtype` doubles as its entry count.
            read_dictionary(reader, data, &mut val_pos, vtype, dir_end, &mut dictionary);
            continue;
        }

        let dict_name = dictionary.get(&tag).cloned();
        let custom = dict_name.is_some();

        let values = read_fpx_value(
            reader,
            data,
            &mut val_pos,
            vtype,
            dir_end,
            false,
            code_page,
            0,
        );

        // ExifTool resolves Dictionary/CodePage/LocaleIndicator against the
        // SummaryInfo table for *every* property set, before any table-specific
        // lookup, because masked IDs in other tables can overlap them.
        let name: Option<String> = if !custom && (tag == 1 || tag == 0x8000_0000) {
            summary_info_name(tag).map(str::to_string)
        } else if let Some(raw) = dict_name.as_deref() {
            Some(
                table_name_for_string(set, raw)
                    .map(str::to_string)
                    .unwrap_or_else(|| dictionary_display_name(raw)),
            )
        } else {
            table_name_for_id(set, tag).map(str::to_string)
        };

        if !custom && tag == 1 {
            if let Some(mut cp) = values.first().and_then(FpxValue::as_int) {
                // Some writers store the code page as int16s; ExifTool folds
                // the negative range back into 16 bits.
                if cp < 0 {
                    cp += 0x10000;
                }
                code_page = Some(cp);
            }
        }

        let Some(name) = name else {
            continue;
        };
        if let Some(value) = convert_value(&name, values) {
            metadata.insert(&format!("FlashPix:{}", name), value);
            found_any = true;
        }
    }

    found_any
}

/// Read the name dictionary that user-defined property sections start with.
fn read_dictionary(
    reader: &EndianReader<'_>,
    data: &[u8],
    val_pos: &mut usize,
    count: u32,
    dir_end: usize,
    dictionary: &mut HashMap<u32, String>,
) {
    for _ in 0..count {
        if *val_pos + 8 > dir_end {
            break;
        }
        let (Some(tag), Some(len)) = (reader.u32_at(*val_pos), reader.u32_at(*val_pos + 4)) else {
            break;
        };
        let Some(next) = val_pos
            .checked_add(8)
            .and_then(|p| p.checked_add(len as usize))
        else {
            break;
        };
        if next > dir_end {
            break;
        }
        let start = next - len as usize;
        *val_pos = next;

        let name = decode_bytes(&data[start..next], None);
        if name.is_empty() {
            continue;
        }
        dictionary.insert(tag, name);
    }
}

/// Read one property value, advancing `pos` past it.
///
/// Mirrors ExifTool's `ReadFPXValue`, including the quirk that `VT_VECTOR`
/// elements are sometimes, but not always, padded to a 4-byte boundary.
#[allow(clippy::too_many_arguments)]
fn read_fpx_value(
    reader: &EndianReader<'_>,
    data: &[u8],
    pos: &mut usize,
    vtype: u32,
    dir_end: usize,
    no_pad_in: bool,
    code_page: Option<i64>,
    depth: usize,
) -> Vec<FpxValue> {
    let mut vals = Vec::new();
    if depth > MAX_VARIANT_DEPTH {
        return vals;
    }
    let Some(fmt) = Fmt::from_code(vtype & 0x0fff) else {
        return vals;
    };

    let mut count: usize = 1;
    let mut no_pad = no_pad_in;
    let flags = vtype & 0xf000;
    if flags != 0 {
        if flags != VT_VECTOR {
            // VT_ARRAY / VT_BYREF are not representable here, and guessing
            // would fabricate values.
            return vals;
        }
        no_pad = true;
        let Some(n) = reader.u32_at(*pos) else {
            return vals;
        };
        if *pos + 4 > dir_end {
            return vals;
        }
        *pos += 4;
        if n == 0 {
            vals.push(FpxValue::Text(String::new()));
            return vals;
        }
        // Every element consumes at least 4 bytes, so a count larger than the
        // remaining buffer can only come from a corrupt or hostile file.
        count = (n as usize).min(dir_end.saturating_sub(*pos) / 4 + 1);
    }

    // Fixed-width numeric formats are read as a block.
    if let Some(elem_size) = fmt.scalar_size() {
        let Some(total) = elem_size.checked_mul(count) else {
            return vals;
        };
        if *pos + total > dir_end {
            return vals;
        }
        for i in 0..count {
            let at = *pos + i * elem_size;
            let v = match fmt {
                Fmt::Int8s => reader.i8_at(at).map(|v| FpxValue::Int(v as i64)),
                Fmt::Int8u => reader.u8_at(at).map(|v| FpxValue::Int(v as i64)),
                Fmt::Int16s => reader.i16_at(at).map(|v| FpxValue::Int(v as i64)),
                Fmt::Int16u => reader.u16_at(at).map(|v| FpxValue::Int(v as i64)),
                Fmt::Int32s => reader.i32_at(at).map(|v| FpxValue::Int(v as i64)),
                Fmt::Int32u => reader.u32_at(at).map(|v| FpxValue::Int(v as i64)),
                Fmt::Int64s => reader.i64_at(at).map(FpxValue::Int),
                Fmt::Int64u => reader.u64_at(at).map(|v| FpxValue::Int(v as i64)),
                Fmt::Float => reader
                    .u32_at(at)
                    .map(|v| FpxValue::Real(f32::from_bits(v) as f64)),
                Fmt::Double => reader.u64_at(at).map(|v| FpxValue::Real(f64::from_bits(v))),
                _ => None,
            };
            match v {
                Some(v) => vals.push(v),
                None => break,
            }
        }
        // Values are padded out to a 4-byte boundary.
        *pos += (total + 3) & !3;
        return vals;
    }

    let header = fmt.header_size();
    // Length of the previous element, used only for the vector padding quirk.
    let mut prev_len: Option<usize> = None;

    for _ in 0..count {
        if *pos + header > dir_end {
            break;
        }
        if no_pad
            && let Some(len) = prev_len
            && len & 3 != 0
        {
            let pad = 4 - (len & 3);
            if *pos + pad + header <= dir_end && data[*pos..*pos + pad].iter().all(|&b| b == 0) {
                *pos += pad;
            }
        }
        prev_len = None;

        match fmt {
            Fmt::Variant => {
                let Some(sub_type) = reader.u32_at(*pos) else {
                    break;
                };
                *pos += header;
                let sub = read_fpx_value(
                    reader,
                    data,
                    pos,
                    sub_type,
                    dir_end,
                    no_pad,
                    code_page,
                    depth + 1,
                );
                if sub.is_empty() {
                    break;
                }
                // ExifTool reads nested variants in scalar context.
                if sub.len() == 1 {
                    vals.push(sub.into_iter().next().expect("len checked"));
                } else {
                    let joined = sub
                        .into_iter()
                        .map(|v| value_to_display(&v))
                        .collect::<Vec<_>>()
                        .join(" ");
                    vals.push(FpxValue::Text(joined));
                }
                continue; // `pos` was already advanced by the nested read
            }
            Fmt::Filetime => {
                let Some(raw) = reader.u64_at(*pos) else {
                    break;
                };
                vals.push(convert_filetime(raw, data, *pos));
            }
            Fmt::Date => {
                let Some(bits) = reader.u64_at(*pos) else {
                    break;
                };
                let days = f64::from_bits(bits);
                let secs = if days != 0.0 {
                    (days - VT_DATE_EPOCH_DAYS) * 86_400.0
                } else {
                    days
                };
                vals.push(FpxValue::Text(format_unix_time(secs)));
            }
            Fmt::Bstr | Fmt::Lpstr | Fmt::Lpwstr => {
                let Some(raw_len) = reader.u32_at(*pos) else {
                    break;
                };
                let len = if fmt == Fmt::Lpwstr {
                    (raw_len as usize).saturating_mul(2)
                } else {
                    raw_len as usize
                };
                let Some(end) = pos.checked_add(4).and_then(|p| p.checked_add(len)) else {
                    break;
                };
                if end > dir_end {
                    break;
                }
                let bytes = &data[*pos + 4..end];
                let text = if fmt == Fmt::Lpwstr {
                    decode_utf16(bytes, reader.byte_order())
                } else {
                    decode_bytes(bytes, code_page)
                };
                vals.push(FpxValue::Text(text));
                prev_len = Some(len);
                *pos += if no_pad { len } else { (len + 3) & !3 };
            }
            Fmt::Blob | Fmt::Cf => {
                let Some(len) = reader.u32_at(*pos) else {
                    break;
                };
                let len = len as usize;
                let Some(end) = pos.checked_add(4).and_then(|p| p.checked_add(len)) else {
                    break;
                };
                if end > dir_end {
                    break;
                }
                vals.push(FpxValue::Bytes(data[*pos + 4..end].to_vec()));
                // Blobs are always padded (ExifTool keeps `prev_len` unset here
                // on purpose, so the vector quirk above does not fire).
                *pos += (len + 3) & !3;
            }
            Fmt::Clsid => {
                let Some(bytes) = data.get(*pos..*pos + 16) else {
                    break;
                };
                vals.push(FpxValue::Text(format_guid(bytes)));
            }
            _ => break,
        }

        *pos += header;
    }

    vals
}

/// `VT_FILETIME` -> either a date string or an elapsed-seconds count.
///
/// ExifTool only treats the value as an absolute date when it exceeds a year,
/// which is what lets `TotalEditTime` stay a duration.
fn convert_filetime(raw: u64, data: &[u8], pos: usize) -> FpxValue {
    let mut val = raw as f64 * 1e-7;
    if val <= 365.0 * 86_400.0 {
        return FpxValue::Real(val);
    }
    val -= FILETIME_UNIX_EPOCH_SECS;
    if !(0.0..=HUNDRED_YEARS_SECS).contains(&val) {
        // Some writers emit the two 32-bit words in the right order but with
        // the wrong byte order inside each word.
        if let Some(bytes) = data.get(pos..pos + 8) {
            let hi = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64;
            let lo = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as f64;
            let swapped = (hi + lo * 4_294_967_296.0) * 1e-7 - FILETIME_UNIX_EPOCH_SECS;
            if swapped > 0.0 && swapped < HUNDRED_YEARS_SECS {
                return FpxValue::Text(format_unix_time(swapped));
            }
        }
        // Or the wrong time base entirely.
        if val < 0.0 && val + FILETIME_UNIX_EPOCH_SECS > 0.0 {
            val += FILETIME_UNIX_EPOCH_SECS;
        }
    }
    FpxValue::Text(format_unix_time(val))
}

/// ExifTool's `ConvertUnixTime`: UTC, `YYYY:MM:DD HH:MM:SS`.
fn format_unix_time(secs: f64) -> String {
    let whole = secs.trunc();
    if !whole.is_finite() || whole < i64::MIN as f64 || whole > i64::MAX as f64 {
        return String::new();
    }
    match chrono::DateTime::from_timestamp(whole as i64, 0) {
        Some(dt) => dt.format("%Y:%m:%d %H:%M:%S").to_string(),
        None => String::new(),
    }
}

/// ExifTool's `ASF::GetGUID` layout.
fn format_guid(bytes: &[u8]) -> String {
    format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        bytes[3],
        bytes[2],
        bytes[1],
        bytes[0],
        bytes[5],
        bytes[4],
        bytes[7],
        bytes[6],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

/// Decode a byte string, honouring the section's code page where we can.
///
/// ExifTool truncates every decoded string at its first NUL; unsupported code
/// pages fall through as raw bytes rather than being guessed at.
fn decode_bytes(bytes: &[u8], code_page: Option<i64>) -> String {
    let text = match code_page {
        Some(1200) => decode_utf16(bytes, ByteOrder::Little),
        Some(1201) => decode_utf16(bytes, ByteOrder::Big),
        Some(65001) => String::from_utf8_lossy(bytes).into_owned(),
        _ => match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            // Not UTF-8: map each byte to the code point of the same value so
            // nothing is silently replaced with U+FFFD.
            Err(_) => bytes.iter().map(|&b| b as char).collect(),
        },
    };
    truncate_at_nul(text)
}

fn decode_utf16(bytes: &[u8], order: ByteOrder) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| match order {
            ByteOrder::Little => u16::from_le_bytes([c[0], c[1]]),
            ByteOrder::Big => u16::from_be_bytes([c[0], c[1]]),
        })
        .collect();
    truncate_at_nul(String::from_utf16_lossy(&units))
}

fn truncate_at_nul(s: String) -> String {
    match s.find('\0') {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

fn value_to_display(v: &FpxValue) -> String {
    match v {
        FpxValue::Int(i) => i.to_string(),
        FpxValue::Real(f) => format_real(*f),
        FpxValue::Text(s) => s.clone(),
        FpxValue::Bytes(b) => binary_placeholder(b.len()),
    }
}

/// Render a float the way Perl stringifies one: no trailing `.0`.
fn format_real(f: f64) -> String {
    if f == f.trunc() && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        format!("{}", f)
    }
}

/// Turn a dictionary key into ExifTool's derived tag name.
///
/// `s/(^| )([a-z])/\U$2/g` followed by `tr/-_a-zA-Z0-9//dc`: capitalise the
/// first letter of every word, then drop everything that is not alphanumeric,
/// `-` or `_`. "Custom Text" becomes "CustomText".
fn dictionary_display_name(raw: &str) -> String {
    let mut capitalised = String::with_capacity(raw.len());
    let mut at_word_start = true;
    for ch in raw.chars() {
        if at_word_start && ch.is_ascii_lowercase() {
            capitalised.push(ch.to_ascii_uppercase());
        } else {
            capitalised.push(ch);
        }
        at_word_start = ch == ' ';
    }
    capitalised
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

/// Apply the tag's ValueConv/PrintConv and collapse the value list.
fn convert_value(name: &str, values: Vec<FpxValue>) -> Option<TagValue> {
    if values.is_empty() {
        return None;
    }

    match name {
        "CodePage" => {
            let cp = values.first()?.as_int()?;
            Some(TagValue::String(code_page_name(cp)))
        }
        "TotalEditTime" => match values.first()? {
            FpxValue::Real(secs) => Some(TagValue::String(convert_time_span(*secs))),
            FpxValue::Int(secs) => Some(TagValue::String(convert_time_span(*secs as f64))),
            other => Some(other.clone().into_tag_value()),
        },
        "Security" => {
            let v = values.first()?.as_int()?;
            Some(TagValue::String(security_print_conv(v)))
        }
        "ExtensionPersistence" => {
            let v = values.first()?.as_int()?;
            Some(TagValue::String(match v {
                0 => "Always Valid".to_string(),
                1 => "Invalidated By Modification".to_string(),
                2 => "Potentially Invalidated By Modification".to_string(),
                other => format!("Unknown ({})", other),
            }))
        }
        "ScaleCrop" | "LinksUpToDate" | "SharedDoc" | "HyperlinksChanged" => {
            let v = values.first()?.as_int()?;
            Some(TagValue::String(yes_no(v)))
        }
        "AppVersion" => {
            let v = values.first()?.as_int()? as u32;
            Some(TagValue::String(format!("{}.{:04}", v >> 16, v & 0xffff)))
        }
        "HyperlinkBase" => match values.first()? {
            // Stored as a UTF-16LE blob rather than a VT_LPWSTR.
            FpxValue::Bytes(b) => Some(TagValue::String(decode_utf16(b, ByteOrder::Little))),
            other => Some(other.clone().into_tag_value()),
        },
        "Hyperlinks" => match values.first()? {
            FpxValue::Bytes(b) => {
                let links = process_hyperlinks(b);
                if links.is_empty() {
                    None
                } else {
                    Some(TagValue::Array(
                        links.into_iter().map(TagValue::String).collect(),
                    ))
                }
            }
            other => Some(other.clone().into_tag_value()),
        },
        _ => {
            if values.len() == 1 {
                Some(values.into_iter().next()?.into_tag_value())
            } else {
                Some(TagValue::Array(
                    values.into_iter().map(FpxValue::into_tag_value).collect(),
                ))
            }
        }
    }
}

/// ExifTool's `ConvertTimeSpan` (no multiplier).
fn convert_time_span(secs: f64) -> String {
    if secs == 0.0 || !secs.is_finite() {
        return format_real(secs);
    }
    if secs < 60.0 {
        format!("{} seconds", format_real(secs))
    } else if secs < 3600.0 {
        format!("{:.1} minutes", secs / 60.0)
    } else if secs < 24.0 * 3600.0 {
        format!("{:.1} hours", secs / 3600.0)
    } else {
        format!("{:.1} days", secs / (24.0 * 3600.0))
    }
}

fn yes_no(v: i64) -> String {
    match v {
        0 => "No".to_string(),
        1 => "Yes".to_string(),
        // An unrecognised code reports itself rather than being rounded to a
        // neighbouring label.
        other => format!("Unknown ({})", other),
    }
}

/// `Security` PrintConv: 0 is "None", otherwise a bitmask.
fn security_print_conv(v: i64) -> String {
    if v == 0 {
        return "None".to_string();
    }
    const BITS: [&str; 4] = [
        "Password protected",
        "Read-only recommended",
        "Read-only enforced",
        "Locked for annotations",
    ];
    let mut set = Vec::new();
    let mut leftover = v;
    for (bit, label) in BITS.iter().enumerate() {
        if v & (1 << bit) != 0 {
            set.push((*label).to_string());
            leftover &= !(1 << bit);
        }
    }
    if leftover != 0 {
        set.push(format!("Unknown ({})", leftover));
    }
    set.join(", ")
}

/// Extract the address (and optional sub-address) of each `_PID_HLINKS` entry.
///
/// The blob is a count followed by 6 `VT_VARIANT`s per link; entries 4 and 5 of
/// each group hold the address and sub-address.
fn process_hyperlinks(blob: &[u8]) -> Vec<String> {
    if blob.len() < 4 {
        return Vec::new();
    }
    let reader = EndianReader::little_endian(blob);
    let Some(num) = reader.u32_at(0) else {
        return Vec::new();
    };
    let dir_end = blob.len();
    let mut pos = 4usize;
    let mut vals: Vec<String> = Vec::new();
    // Each variant consumes at least 4 bytes.
    let num = (num as usize).min(dir_end / 4 + 1);
    for _ in 0..num {
        let read = read_fpx_value(&reader, blob, &mut pos, 12, dir_end, false, None, 0);
        if read.is_empty() {
            break;
        }
        vals.push(
            read.iter()
                .map(value_to_display)
                .collect::<Vec<_>>()
                .join(" "),
        );
    }

    let mut links = Vec::new();
    let mut i = 0;
    while i + 5 < vals.len() {
        let mut link = vals[i + 4].clone();
        if !vals[i + 5].is_empty() {
            link.push('#');
            link.push_str(&vals[i + 5]);
        }
        links.push(link);
        i += 6;
    }
    links
}

/// `Image::ExifTool::FlashPix::SummaryInfo` tag IDs.
///
/// `Dictionary` (0x00) is consumed as the name dictionary and never emitted.
fn summary_info_name(tag: u32) -> Option<&'static str> {
    Some(match tag {
        0x01 => "CodePage",
        0x02 => "Title",
        0x03 => "Subject",
        0x04 => "Author",
        0x05 => "Keywords",
        0x06 => "Comments",
        0x07 => "Template",
        0x08 => "LastModifiedBy",
        0x09 => "RevisionNumber",
        0x0a => "TotalEditTime",
        0x0b => "LastPrinted",
        0x0c => "CreateDate",
        0x0d => "ModifyDate",
        0x0e => "Pages",
        0x0f => "Words",
        0x10 => "Characters",
        0x11 => "ThumbnailClip",
        0x12 => "Software",
        0x13 => "Security",
        0x22 => "CreatedBy",
        0x23 => "DocumentID",
        // Full 32-bit ID; NOT 0x0000.
        0x8000_0000 => "LocaleIndicator",
        _ => return None,
    })
}

/// `Image::ExifTool::FlashPix::DocumentInfo` tag IDs.
fn document_info_name(tag: u32) -> Option<&'static str> {
    Some(match tag {
        0x02 => "Category",
        0x03 => "PresentationTarget",
        0x04 => "Bytes",
        0x05 => "Lines",
        0x06 => "Paragraphs",
        0x07 => "Slides",
        0x08 => "Notes",
        0x09 => "HiddenSlides",
        0x0a => "MMClips",
        0x0b => "ScaleCrop",
        0x0c => "HeadingPairs",
        0x0d => "TitleOfParts",
        0x0e => "Manager",
        0x0f => "Company",
        0x10 => "LinksUpToDate",
        0x11 => "CharCountWithSpaces",
        0x13 => "SharedDoc",
        0x16 => "HyperlinksChanged",
        0x17 => "AppVersion",
        0x1a => "ContentType",
        0x1b => "ContentStatus",
        0x1c => "Language",
        0x1d => "DocVersion",
        _ => return None,
    })
}

/// `Image::ExifTool::FlashPix::Extensions` tag IDs.
///
/// Only the low bits identify the property; the rest is the 1-based extension
/// number, so `\x05Extension List` stores `ExtensionName` for extension 2 as
/// `0x00020001`. ExifTool masks with `0x0000ffff` for most IDs and
/// `0x0000f00f` for the two that would otherwise collide.
fn extensions_name(tag: u32) -> Option<&'static str> {
    // Not an extension property: one list-wide value.
    if tag == 0x1000_0000 {
        return Some("UsedExtensionNumbers");
    }
    // An unmasked hit wins, exactly as it does in ExifTool.
    if let Some(name) = extensions_id_name(tag) {
        return Some(name);
    }
    let wide = tag & 0x0000_ffff;
    if !is_narrow_masked_id(wide)
        && let Some(name) = extensions_id_name(wide)
    {
        return Some(name);
    }
    let narrow = tag & 0x0000_f00f;
    if is_narrow_masked_id(narrow) {
        return extensions_id_name(narrow);
    }
    None
}

/// The two IDs ExifTool masks with `0x0000f00f` rather than `0x0000ffff`.
fn is_narrow_masked_id(id: u32) -> bool {
    matches!(id, 0x3001 | 0x3002)
}

fn extensions_id_name(id: u32) -> Option<&'static str> {
    Some(match id {
        0x0001 => "ExtensionName",
        0x0002 => "ExtensionClassID",
        0x0003 => "ExtensionPersistence",
        0x0004 => "ExtensionCreateDate",
        0x0005 => "ExtensionModifyDate",
        0x0006 => "CreatingApplication",
        0x0007 => "ExtensionDescription",
        0x1000 => "Storage-StreamPathname",
        0x2000 => "FlashPixStreamPathname",
        0x2001 => "FlashPixStreamFieldOffset",
        0x3000 => "PropertySetPathname",
        0x3001 => "PropertySetIDCodes",
        0x3002 => "PropertyVectorElements",
        0x4000 => "SubimageResolutions",
        _ => return None,
    })
}

fn table_name_for_id(set: PropertySet, tag: u32) -> Option<&'static str> {
    match set {
        PropertySet::SummaryInfo => summary_info_name(tag),
        PropertySet::DocumentInfo => document_info_name(tag),
        PropertySet::Extensions => extensions_name(tag),
        PropertySet::AudioInfo => None,
    }
}

/// The two dictionary keys `DocumentInfo` predefines.
fn table_name_for_string(set: PropertySet, key: &str) -> Option<&'static str> {
    if set != PropertySet::DocumentInfo {
        return None;
    }
    match key {
        "_PID_LINKBASE" => Some("HyperlinkBase"),
        "_PID_HLINKS" => Some("Hyperlinks"),
        _ => None,
    }
}

/// Extract the user name from a Word/PowerPoint `Current User` stream.
///
/// Layout per ExifTool's `Current User` ValueConv: a 4-byte header, then the
/// record size and the offset of the ANSI name within the record.
pub(crate) fn parse_current_user(data: &[u8]) -> Option<String> {
    if data.len() < 12 {
        return None;
    }
    let reader = EndianReader::little_endian(data);
    let size = reader.u32_at(4)? as usize;
    let offset = reader.u32_at(8)? as usize;
    let len = size.checked_sub(offset)?.checked_sub(4)?;
    if data.len() < size + 8 {
        return None;
    }
    let start = offset.checked_add(8)?;
    let end = start.checked_add(len)?;
    let bytes = data.get(start..end)?;
    let name = decode_bytes(bytes, None);
    if name.is_empty() { None } else { Some(name) }
}

/// `Image::ExifTool::Microsoft::%codePage`.
fn code_page_name(cp: i64) -> String {
    let name = match cp {
        37 => "IBM EBCDIC US-Canada",
        437 => "DOS United States",
        500 => "IBM EBCDIC International",
        708 => "Arabic (ASMO 708)",
        709 => "Arabic (ASMO-449+, BCON V4)",
        710 => "Arabic - Transparent Arabic",
        720 => "DOS Arabic (Transparent ASMO)",
        737 => "DOS Greek (formerly 437G)",
        775 => "DOS Baltic",
        850 => "DOS Latin 1 (Western European)",
        852 => "DOS Latin 2 (Central European)",
        855 => "DOS Cyrillic (primarily Russian)",
        857 => "DOS Turkish",
        858 => "DOS Multilingual Latin 1 with Euro",
        860 => "DOS Portuguese",
        861 => "DOS Icelandic",
        862 => "DOS Hebrew",
        863 => "DOS French Canadian",
        864 => "DOS Arabic",
        865 => "DOS Nordic",
        866 => "DOS Russian (Cyrillic)",
        869 => "DOS Modern Greek",
        870 => "IBM EBCDIC Multilingual/ROECE (Latin 2)",
        874 => "Windows Thai (same as 28605, ISO 8859-15)",
        875 => "IBM EBCDIC Greek Modern",
        932 => "Windows Japanese (Shift-JIS)",
        936 => "Windows Simplified Chinese (PRC, Singapore)",
        949 => "Windows Korean (Unified Hangul Code)",
        950 => "Windows Traditional Chinese (Taiwan)",
        1026 => "IBM EBCDIC Turkish (Latin 5)",
        1047 => "IBM EBCDIC Latin 1/Open System",
        1140 => "IBM EBCDIC US-Canada with Euro",
        1141 => "IBM EBCDIC Germany with Euro",
        1142 => "IBM EBCDIC Denmark-Norway with Euro",
        1143 => "IBM EBCDIC Finland-Sweden with Euro",
        1144 => "IBM EBCDIC Italy with Euro",
        1145 => "IBM EBCDIC Latin America-Spain with Euro",
        1146 => "IBM EBCDIC United Kingdom with Euro",
        1147 => "IBM EBCDIC France with Euro",
        1148 => "IBM EBCDIC International with Euro",
        1149 => "IBM EBCDIC Icelandic with Euro",
        1200 => "Unicode UTF-16, little endian",
        1201 => "Unicode UTF-16, big endian",
        1250 => "Windows Latin 2 (Central European)",
        1251 => "Windows Cyrillic",
        1252 => "Windows Latin 1 (Western European)",
        1253 => "Windows Greek",
        1254 => "Windows Turkish",
        1255 => "Windows Hebrew",
        1256 => "Windows Arabic",
        1257 => "Windows Baltic",
        1258 => "Windows Vietnamese",
        1361 => "Korean (Johab)",
        10000 => "Mac Roman (Western European)",
        10001 => "Mac Japanese",
        10002 => "Mac Traditional Chinese",
        10003 => "Mac Korean",
        10004 => "Mac Arabic",
        10005 => "Mac Hebrew",
        10006 => "Mac Greek",
        10007 => "Mac Cyrillic",
        10008 => "Mac Simplified Chinese",
        10010 => "Mac Romanian",
        10017 => "Mac Ukrainian",
        10021 => "Mac Thai",
        10029 => "Mac Latin 2 (Central European)",
        10079 => "Mac Icelandic",
        10081 => "Mac Turkish",
        10082 => "Mac Croatian",
        12000 => "Unicode UTF-32, little endian",
        12001 => "Unicode UTF-32, big endian",
        20000 => "CNS Taiwan",
        20001 => "TCA Taiwan",
        20002 => "Eten Taiwan",
        20003 => "IBM5550 Taiwan",
        20004 => "TeleText Taiwan",
        20005 => "Wang Taiwan",
        20105 => "IA5 (IRV International Alphabet No. 5, 7-bit)",
        20106 => "IA5 German (7-bit)",
        20107 => "IA5 Swedish (7-bit)",
        20108 => "IA5 Norwegian (7-bit)",
        20127 => "US-ASCII (7-bit)",
        20261 => "T.61",
        20269 => "ISO 6937 Non-Spacing Accent",
        20273 => "IBM EBCDIC Germany",
        20277 => "IBM EBCDIC Denmark-Norway",
        20278 => "IBM EBCDIC Finland-Sweden",
        20280 => "IBM EBCDIC Italy",
        20284 => "IBM EBCDIC Latin America-Spain",
        20285 => "IBM EBCDIC United Kingdom",
        20290 => "IBM EBCDIC Japanese Katakana Extended",
        20297 => "IBM EBCDIC France",
        20420 => "IBM EBCDIC Arabic",
        20423 => "IBM EBCDIC Greek",
        20424 => "IBM EBCDIC Hebrew",
        20833 => "IBM EBCDIC Korean Extended",
        20838 => "IBM EBCDIC Thai",
        20866 => "Russian/Cyrillic (KOI8-R)",
        20871 => "IBM EBCDIC Icelandic",
        20880 => "IBM EBCDIC Cyrillic Russian",
        20905 => "IBM EBCDIC Turkish",
        20924 => "IBM EBCDIC Latin 1/Open System with Euro",
        20932 => "Japanese (JIS 0208-1990 and 0121-1990)",
        20936 => "Simplified Chinese (GB2312)",
        20949 => "Korean Wansung",
        21025 => "IBM EBCDIC Cyrillic Serbian-Bulgarian",
        21027 => "Extended Alpha Lowercase (deprecated)",
        21866 => "Ukrainian/Cyrillic (KOI8-U)",
        28591 => "ISO 8859-1 Latin 1 (Western European)",
        28592 => "ISO 8859-2 (Central European)",
        28593 => "ISO 8859-3 Latin 3",
        28594 => "ISO 8859-4 Baltic",
        28595 => "ISO 8859-5 Cyrillic",
        28596 => "ISO 8859-6 Arabic",
        28597 => "ISO 8859-7 Greek",
        28598 => "ISO 8859-8 Hebrew (Visual)",
        28599 => "ISO 8859-9 Turkish",
        28603 => "ISO 8859-13 Estonian",
        28605 => "ISO 8859-15 Latin 9",
        29001 => "Europa 3",
        38598 => "ISO 8859-8 Hebrew (Logical)",
        50220 => "ISO 2022 Japanese with no halfwidth Katakana (JIS)",
        50221 => "ISO 2022 Japanese with halfwidth Katakana (JIS-Allow 1 byte Kana)",
        50222 => "ISO 2022 Japanese JIS X 0201-1989 (JIS-Allow 1 byte Kana - SO/SI)",
        50225 => "ISO 2022 Korean",
        50227 => "ISO 2022 Simplified Chinese",
        50229 => "ISO 2022 Traditional Chinese",
        50930 => "EBCDIC Japanese (Katakana) Extended",
        50931 => "EBCDIC US-Canada and Japanese",
        50933 => "EBCDIC Korean Extended and Korean",
        50935 => "EBCDIC Simplified Chinese Extended and Simplified Chinese",
        50936 => "EBCDIC Simplified Chinese",
        50937 => "EBCDIC US-Canada and Traditional Chinese",
        50939 => "EBCDIC Japanese (Latin) Extended and Japanese",
        51932 => "EUC Japanese",
        51936 => "EUC Simplified Chinese",
        51949 => "EUC Korean",
        51950 => "EUC Traditional Chinese",
        52936 => "HZ-GB2312 Simplified Chinese",
        54936 => "Windows XP and later: GB18030 Simplified Chinese (4 byte)",
        57002 => "ISCII Devanagari",
        57003 => "ISCII Bengali",
        57004 => "ISCII Tamil",
        57005 => "ISCII Telugu",
        57006 => "ISCII Assamese",
        57007 => "ISCII Oriya",
        57008 => "ISCII Kannada",
        57009 => "ISCII Malayalam",
        57010 => "ISCII Gujarati",
        57011 => "ISCII Punjabi",
        65000 => "Unicode (UTF-7)",
        65001 => "Unicode (UTF-8)",
        _ => return format!("Unknown ({})", cp),
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The generated tag YAML truncates IDs to 16 bits. `LocaleIndicator` is
    /// 0x80000000, whose low 16 bits are 0x0000 -- the `Dictionary` ID. If a
    /// future regeneration reintroduces that truncation this fails loudly
    /// instead of silently resolving the wrong tag.
    #[test]
    fn locale_indicator_id_is_not_truncated() {
        assert_eq!(summary_info_name(0x8000_0000), Some("LocaleIndicator"));
        // The low 16 bits alone must NOT resolve to LocaleIndicator.
        assert_eq!(summary_info_name(0x0000), None);
        // ...and must not collide with a real 16-bit tag either.
        assert_ne!(summary_info_name(0x0000), summary_info_name(0x8000_0000));
    }

    /// `\x05Extension List` repeats the same property IDs once per extension,
    /// with the 1-based extension number in the high bits.
    #[test]
    fn extension_list_ids_resolve_under_their_mask() {
        // Extension 1 and extension 2 name the same property.
        assert_eq!(extensions_name(0x0001_0001), Some("ExtensionName"));
        assert_eq!(extensions_name(0x0002_0001), Some("ExtensionName"));
        assert_eq!(extensions_name(0x0001_1000), Some("Storage-StreamPathname"));
        assert_eq!(extensions_name(0x0002_1000), Some("Storage-StreamPathname"));
        // Unnumbered IDs still resolve.
        assert_eq!(extensions_name(0x0003), Some("ExtensionPersistence"));
        // The list-wide ID is not an extension property and must not be masked
        // down to 0x0000 (which names nothing) or to a numbered property.
        assert_eq!(extensions_name(0x1000_0000), Some("UsedExtensionNumbers"));
        // These two use the narrower 0x0000f00f mask, so the bits between are
        // part of the extension number rather than the property ID.
        assert_eq!(extensions_name(0x0001_3001), Some("PropertySetIDCodes"));
        assert_eq!(extensions_name(0x0012_3002), Some("PropertyVectorElements"));
        // ...and must not be reachable through the wide mask.
        assert_eq!(extensions_name(0x0001_3003), None);
        // An ID belonging to no property stays unnamed rather than guessing.
        assert_eq!(extensions_name(0x0001_0009), None);
    }

    #[test]
    fn audio_info_declares_no_ids_of_its_own() {
        // ExifTool's %FlashPix::AudioInfo is empty; only the universal
        // CodePage/LocaleIndicator (handled before any table lookup) appear.
        assert_eq!(table_name_for_id(PropertySet::AudioInfo, 0x0002), None);
        assert_eq!(table_name_for_id(PropertySet::AudioInfo, 0x1000_0000), None);
    }

    #[test]
    fn document_info_ids_match_flashpix_pm() {
        assert_eq!(document_info_name(0x02), Some("Category"));
        assert_eq!(document_info_name(0x17), Some("AppVersion"));
        assert_eq!(document_info_name(0x1d), Some("DocVersion"));
        // 0x12, 0x14, 0x15 are undocumented in FlashPix.pm and must stay unnamed.
        assert_eq!(document_info_name(0x12), None);
        assert_eq!(document_info_name(0x14), None);
    }

    #[test]
    fn app_version_splits_into_major_and_four_digit_minor() {
        // 657778 == 0x000A0972 -> 10.2418 (the FlashPix.ppt sample)
        let v = convert_value("AppVersion", vec![FpxValue::Int(657778)]).unwrap();
        assert_eq!(v, TagValue::String("10.2418".to_string()));
    }

    #[test]
    fn total_edit_time_uses_convert_time_span() {
        let v = convert_value("TotalEditTime", vec![FpxValue::Real(266.0)]).unwrap();
        assert_eq!(v, TagValue::String("4.4 minutes".to_string()));
        assert_eq!(convert_time_span(30.0), "30 seconds");
        assert_eq!(convert_time_span(7200.0), "2.0 hours");
        assert_eq!(convert_time_span(172_800.0), "2.0 days");
    }

    #[test]
    fn code_page_10000_is_mac_roman() {
        assert_eq!(code_page_name(10000), "Mac Roman (Western European)");
        assert_eq!(code_page_name(1252), "Windows Latin 1 (Western European)");
        // The table must cover the whole of Microsoft.pm's %codePage, not just
        // the low range: UTF-8 sits at the very top and is the code page most
        // likely to turn up in a modern file.
        assert_eq!(code_page_name(65001), "Unicode (UTF-8)");
        assert_eq!(code_page_name(28605), "ISO 8859-15 Latin 9");
        assert_eq!(code_page_name(57011), "ISCII Punjabi");
        // An unknown code page reports itself rather than a nearby label.
        assert_eq!(code_page_name(999_999), "Unknown (999999)");
    }

    #[test]
    fn boolean_print_conv_reports_unknown_codes() {
        assert_eq!(yes_no(0), "No");
        assert_eq!(yes_no(1), "Yes");
        assert_eq!(yes_no(7), "Unknown (7)");
    }

    #[test]
    fn security_bitmask() {
        assert_eq!(security_print_conv(0), "None");
        assert_eq!(security_print_conv(1), "Password protected");
        assert_eq!(
            security_print_conv(3),
            "Password protected, Read-only recommended"
        );
    }

    #[test]
    fn dictionary_names_lose_spaces_and_capitalise_words() {
        assert_eq!(dictionary_display_name("Custom Text"), "CustomText");
        assert_eq!(dictionary_display_name("custom text"), "CustomText");
        assert_eq!(dictionary_display_name("_PID_LINKBASE"), "_PID_LINKBASE");
        assert_eq!(dictionary_display_name("a/b c"), "AbC");
    }

    #[test]
    fn filetime_above_one_year_becomes_a_utc_date() {
        // 0x01C74C669E538F80 -- CreateDate of the FlashPix.ppt sample.
        let raw: u64 = 128_155_118_030_000_000;
        let bytes = raw.to_le_bytes();
        assert_eq!(
            convert_filetime(raw, &bytes, 0),
            FpxValue::Text("2007:02:09 16:23:23".to_string())
        );
    }

    #[test]
    fn filetime_below_one_year_stays_a_duration() {
        let raw: u64 = 2_660_000_000; // 266 seconds
        let bytes = raw.to_le_bytes();
        assert_eq!(convert_filetime(raw, &bytes, 0), FpxValue::Real(266.0));
    }

    #[test]
    fn strings_truncate_at_the_first_nul() {
        assert_eq!(decode_bytes(b"Times\0\x13\0", None), "Times");
        let utf16: Vec<u8> = "ab\0".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(decode_utf16(&utf16, ByteOrder::Little), "ab");
    }

    #[test]
    fn rejects_a_stream_without_a_byte_order_mark() {
        let mut metadata = MetadataMap::new();
        let data = vec![0u8; 64];
        assert!(!parse_property_stream(
            &data,
            PropertySet::SummaryInfo,
            &mut metadata
        ));
        assert_eq!(metadata.len(), 0);
    }

    #[test]
    fn rejects_a_truncated_stream() {
        let mut metadata = MetadataMap::new();
        let data = vec![0xFE, 0xFF, 0x00, 0x00];
        assert!(!parse_property_stream(
            &data,
            PropertySet::SummaryInfo,
            &mut metadata
        ));
    }

    /// Builds a minimal little-endian SummaryInformation stream with a single
    /// VT_LPSTR Title, and checks the whole walk end to end.
    #[test]
    fn parses_a_minimal_summary_information_stream() {
        let mut data = vec![0u8; 48];
        data[0..2].copy_from_slice(&BOM_LITTLE_ENDIAN);
        data[44..48].copy_from_slice(&48u32.to_le_bytes()); // section offset

        let mut section = Vec::new();
        section.extend_from_slice(&0u32.to_le_bytes()); // size (patched below)
        section.extend_from_slice(&1u32.to_le_bytes()); // 1 property
        section.extend_from_slice(&2u32.to_le_bytes()); // tag 0x02 = Title
        // Offset (within the section) of the property's 4-byte TYPE field; the
        // value itself starts at section + 4 + offset.
        section.extend_from_slice(&16u32.to_le_bytes());
        section.extend_from_slice(&30u32.to_le_bytes()); // VT_LPSTR
        section.extend_from_slice(&6u32.to_le_bytes()); // length
        section.extend_from_slice(b"title\0");
        let size = section.len() as u32;
        section[0..4].copy_from_slice(&size.to_le_bytes());
        data.extend_from_slice(&section);

        let mut metadata = MetadataMap::new();
        assert!(parse_property_stream(
            &data,
            PropertySet::SummaryInfo,
            &mut metadata
        ));
        assert_eq!(metadata.get_string("FlashPix:Title"), Some("title"));
    }

    #[test]
    fn current_user_stream_yields_the_user_name() {
        let mut data = vec![0u8; 41];
        data[4..8].copy_from_slice(&33u32.to_le_bytes()); // size
        data[8..12].copy_from_slice(&20u32.to_le_bytes()); // offset
        data[28..37].copy_from_slice(b"user name");
        assert_eq!(parse_current_user(&data), Some("user name".to_string()));
    }

    #[test]
    fn current_user_rejects_inconsistent_lengths() {
        let mut data = vec![0u8; 41];
        data[4..8].copy_from_slice(&10u32.to_le_bytes()); // size < offset + 4
        data[8..12].copy_from_slice(&20u32.to_le_bytes());
        assert_eq!(parse_current_user(&data), None);
    }
}
