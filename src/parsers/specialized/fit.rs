//! Garmin FIT activity-file reader.
//!
//! One generic executor for `Image::ExifTool::Garmin::ProcessFIT` (ExifTool
//! 13.59; Garmin.pm 6293-6592, reviewed in
//! `docs/reference/garmin-fit-source-review.md`) over the generated specs in
//! [`crate::exiftool_tables::fit_tables`]. Message names, groups, field rows,
//! base types and conversions all come from that generated data; nothing in
//! this file names a FIT message, field or tag.
//!
//! OxiDex exposes neither ExifTool's `Unknown` nor its `ExtractEmbedded`
//! option, so this reproduces the default mode only: messages flagged
//! `Unknown` get no field list, only the first record of each message number
//! is read, and developer fields are skipped by size. (ProcessFIT decodes a
//! developer field from values it captures out of `DeveloperDataID` (207)
//! and `FieldDescription` (206) records; both edges carry `Unknown => 1`,
//! Garmin.pm 3299-3308, so without `-u` nothing is captured.)
//!
//! Where the reviewed control flow reaches a state it does not model (a
//! compressed header that would autovivify a missing definition, a timestamp
//! that is not an integer representable in `i64`, a message edge the
//! generator withheld), the walk stops and keeps what it already extracted:
//! an omission, never a guess.

use std::collections::HashSet;

use crate::core::{
    FileFormat, FileReader, FormatParser, Instance, MetadataMap, SHIM_DEFAULT_PRIORITY, TagValue,
};
use crate::error::{ExifToolError, Result};
use crate::exiftool_tables::exprs::perl_num;
use crate::exiftool_tables::fit_schema::{
    FitBaseType, FitField, FitFormat, FitPrintConv, FitProtocol, FitTable,
};
use crate::exiftool_tables::fit_tables::FIT_PROTOCOL;
use crate::exiftool_tables::runtime::{self, DecodedValue, Typed};

/// Garmin.pm 6313: issued at level 3 unless ExtractEmbedded is set; `Warn`
/// prefixes level-3 text with `[minor] ` (ExifTool.pm `Warn`).
const EXTRACT_EMBEDDED_WARNING: &str =
    "[minor] Use ExtractEmbedded option to extract all timed metadata";

/// One entry of a definition's field list (`$$theMsg{FieldInfo}`).
#[derive(Clone, Copy)]
enum FieldInfo {
    Standard {
        num: u8,
        size: usize,
        base: &'static FitBaseType,
    },
    /// Developer field: sized and skipped (default mode has no developer
    /// descriptions to decode it with).
    Developer { size: usize },
}

/// `$$theMsg{TS}`: where the message's timestamp comes from.
#[derive(Clone, Copy)]
enum TimeStampSlot {
    /// Field 253 of the record, at `offset` (ProcessFIT's running `$totSize`,
    /// which counts every declared standard field).
    Field {
        offset: usize,
        size: usize,
        base: &'static FitBaseType,
    },
    /// The value a compressed header computed (`[253, $ts]`).
    Compressed(i64),
}

/// The message a definition names, resolved against the generated map.
struct Message {
    name: String,
    group1: String,
    table: Option<&'static FitTable>,
}

struct Definition {
    message_num: u16,
    big_endian: bool,
    size: usize,
    message: Message,
    field_info: Option<Vec<FieldInfo>>,
    ts: Option<TimeStampSlot>,
}

/// The message a global number names, and whether it is flagged `Unknown`.
/// `None` for an edge the generator withheld.
fn resolve_message(num: u16) -> Option<(Message, bool)> {
    Some(match FIT_PROTOCOL.message(num) {
        Some(message) if message.withheld.is_some() => return None,
        Some(message) => (
            Message {
                name: message.name.to_string(),
                // An edge with no table gets `GROUPS { 1 => $msgName }`.
                group1: message
                    .table
                    .map_or(message.name, |table| table.group1)
                    .to_string(),
                table: message.table,
            },
            message.unknown,
        ),
        // Garmin.pm 6361-6363: an unlisted number becomes `Unknown<num>`.
        None => {
            let name = format!("Unknown{num}");
            (
                Message {
                    group1: name.clone(),
                    name,
                    table: None,
                },
                true,
            )
        }
    })
}

/// A decoded field value and the Perl text ProcessFIT compares against the
/// base type's invalid value. `value` is `None` when the executor cannot
/// carry the number exactly (an unsigned value above `i64::MAX`, a non-finite
/// float): such a field reports its Perl text only when no conversion applies.
struct Decoded {
    value: Option<DecodedValue>,
    text: String,
}

fn read_unsigned(bytes: &[u8], big_endian: bool) -> u64 {
    let fold = |acc: u64, byte: &u8| acc << 8 | u64::from(*byte);
    if big_endian {
        bytes.iter().fold(0, fold)
    } else {
        bytes.iter().rev().fold(0, fold)
    }
}

/// Perl's text for a number read by `GetFloat`/`GetDouble`.
fn perl_float_text(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 { "Inf" } else { "-Inf" }.to_string()
    } else {
        perl_num(value)
    }
}

/// One numeric element as `ReadValue` returns it: its Perl text, and the
/// value when it can be carried exactly. `Get64u` keeps an unsigned 64-bit
/// value exact on a 64-bit Perl, so a value above `i64::MAX` is text only;
/// so is a non-finite float (Perl prints `Inf`/`-Inf`/`NaN`).
fn read_element(
    format: FitFormat,
    bytes: &[u8],
    big_endian: bool,
) -> (Option<DecodedValue>, String) {
    let raw = read_unsigned(bytes, big_endian);
    let width = bytes.len() * 8;
    let signed = |raw: u64| ((raw << (64 - width)) as i64) >> (64 - width);
    let float = |value: f64| {
        (
            value.is_finite().then_some(DecodedValue::Float(value)),
            perl_float_text(value),
        )
    };
    match format {
        FitFormat::Int8u | FitFormat::Int16u | FitFormat::Int32u | FitFormat::Int64u => (
            i64::try_from(raw).ok().map(DecodedValue::Integer),
            raw.to_string(),
        ),
        FitFormat::Int8s | FitFormat::Int16s | FitFormat::Int32s | FitFormat::Int64s => {
            let value = signed(raw);
            (Some(DecodedValue::Integer(value)), value.to_string())
        }
        FitFormat::Float => float(f64::from(f32::from_bits(raw as u32))),
        FitFormat::Double => float(f64::from_bits(raw)),
        FitFormat::String | FitFormat::Undef => {
            unreachable!("byte formats are not numeric elements")
        }
    }
}

/// `ReadValue(\$buff, $pos, $fmt, $count, $size)` for one field.
fn decode(format: FitFormat, bytes: &[u8], big_endian: bool) -> Decoded {
    match format {
        FitFormat::String => {
            // ExifTool.pm ReadValue: `s/\0.*//s` for `string`.
            let end = bytes
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(bytes.len());
            let text = String::from_utf8_lossy(&bytes[..end]).into_owned();
            Decoded {
                value: Some(DecodedValue::StringBytes(bytes[..end].to_vec())),
                text,
            }
        }
        FitFormat::Undef => Decoded {
            value: Some(DecodedValue::Undefined(bytes.to_vec())),
            text: String::from_utf8_lossy(bytes).into_owned(),
        },
        _ => {
            let size = format.size();
            if bytes.len() == size {
                let (value, text) = read_element(format, bytes, big_endian);
                return Decoded { value, text };
            }
            let (values, texts): (Vec<_>, Vec<_>) = bytes
                .chunks_exact(size)
                .map(|chunk| read_element(format, chunk, big_endian))
                .unzip();
            Decoded {
                value: values
                    .into_iter()
                    .collect::<Option<Vec<_>>>()
                    .map(DecodedValue::Array),
                text: texts.join(" "),
            }
        }
    }
}

/// The value as ExifTool stores it: Perl's own text for a floating-point
/// number (`%.15g`) and for an integer of 16 or more digits, which the JSON
/// writer then types exactly as `EscapeJSON` does; other values unchanged.
fn perl_value(value: &DecodedValue) -> TagValue {
    match value {
        DecodedValue::Float(number) => TagValue::String(perl_num(*number)),
        DecodedValue::Integer(integer) if integer.unsigned_abs() >= 1_000_000_000_000_000 => {
            TagValue::String(integer.to_string())
        }
        other => runtime::to_exiftool_value(other),
    }
}

/// Perl `lc` on a byte string: ASCII letters only.
fn perl_lc(text: &str) -> String {
    text.to_ascii_lowercase()
}

struct Walk<'a> {
    metadata: &'a mut MetadataMap,
    warned: HashSet<String>,
    /// ProcessFIT's running `$timestamp` (a Perl integer; starts at 0).
    timestamp: i64,
    doc_count: u32,
    doc_num: Option<u32>,
}

impl Walk<'_> {
    fn instance(&self) -> Instance {
        Instance(self.doc_num.unwrap_or(0))
    }

    /// ExifTool.pm `Warn`: each distinct text is reported once.
    fn warn(&mut self, text: String) {
        if self.warned.insert(text.clone()) {
            let instance = self.instance();
            self.metadata.insert_occurrence(
                "ExifTool:Warning",
                TagValue::String(text),
                SHIM_DEFAULT_PRIORITY,
                "ExifTool",
                instance,
            );
        }
    }

    /// `HandleTag` for one generated field row: RawConv, ValueConv, PrintConv
    /// through the run-time domain rule. A withheld row, or a conversion whose
    /// domain the value is not in, reports nothing.
    fn emit(
        &mut self,
        group0: &str,
        group1: &str,
        field: &FitField,
        decoded: Decoded,
        binary: bool,
    ) {
        if field.withheld.is_some() {
            return;
        }
        let key = format!("{group0}:{}", field.name);
        let instance = self.instance();
        if binary {
            // Garmin.pm 6560 hands `\$val`; GetValue runs no conversion on a
            // scalar reference (ExifTool.pm `last if ref $value eq 'SCALAR'`).
            // A RawConv would see the reference itself: not modeled.
            if field.raw_conv.is_some() {
                return;
            }
            let Some(DecodedValue::Undefined(bytes)) = &decoded.value else {
                return;
            };
            let placeholder = TagValue::String(format!(
                "(Binary data {} bytes, use -b option to extract)",
                bytes.len()
            ));
            self.metadata.insert_occurrence_with_forms(
                key,
                placeholder.clone(),
                placeholder,
                None,
                SHIM_DEFAULT_PRIORITY,
                group1,
                instance,
            );
            return;
        }
        let Some(mut value) = decoded.value else {
            // Exact only as Perl text (an unsigned value above i64::MAX, a
            // non-finite float): reported as that text when no conversion
            // would read it, withheld otherwise.
            if field.raw_conv.is_some()
                || field.value_conv.is_some()
                || !matches!(field.print_conv, FitPrintConv::None)
            {
                return;
            }
            let text = TagValue::String(decoded.text);
            self.metadata.insert_occurrence_with_forms(
                key,
                text.clone(),
                text,
                None,
                SHIM_DEFAULT_PRIORITY,
                group1,
                instance,
            );
            return;
        };
        if let Some(conv) = field.raw_conv {
            match runtime::apply_typed(conv, &value) {
                Typed::Value(converted) => value = converted,
                // FoundTag: a RawConv returning undef drops the tag.
                Typed::Undef | Typed::DomainMismatch => return,
            }
        }
        if let Some(conv) = field.value_conv {
            match runtime::apply_typed(conv, &value) {
                Typed::Value(converted) => value = converted,
                // GetValue: `return () unless defined $value`.
                Typed::Undef | Typed::DomainMismatch => return,
            }
        }
        let unconverted = perl_value(&value);
        let display = match field.print_conv {
            FitPrintConv::None => unconverted.clone(),
            FitPrintConv::Table(conv) => {
                // Perl looks a hash up by the value's text, so the string
                // "12" finds integer key 12; the shared renderer keys strings
                // only against string maps. Numbers and lists render exactly.
                if !matches!(
                    value,
                    DecodedValue::Integer(_) | DecodedValue::Float(_) | DecodedValue::Array(_)
                ) {
                    return;
                }
                match runtime::render(conv, &value) {
                    Some(rendered) => TagValue::String(rendered),
                    None => return,
                }
            }
            FitPrintConv::Typed(conv) => match runtime::render_typed(conv, &value) {
                Typed::Value(rendered) => TagValue::String(rendered),
                // An undef PrintConv hides the tag only in print mode; one
                // stored form cannot express that split, so withhold it.
                Typed::Undef | Typed::DomainMismatch => return,
            },
        };
        self.metadata.insert_occurrence_with_forms(
            key,
            display,
            unconverted,
            None,
            SHIM_DEFAULT_PRIORITY,
            group1,
            instance,
        );
    }
}

/// Everything ProcessFIT extracts from the record stream (default options).
fn parse_records(file: &[u8], metadata: &mut MetadataMap) {
    let Some(protocol_version) = file.get(1).copied() else {
        return;
    };
    let mut walk = Walk {
        metadata,
        warned: HashSet::new(),
        timestamp: 0,
        doc_count: 0,
        doc_num: None,
    };
    // Garmin.pm 6311: `HandleTag($tagTbl, vers => Get8u(\$buff, 1))`.
    if let Some(name) = FIT_PROTOCOL.header_name {
        walk.metadata.insert_occurrence_with_forms(
            format!("{}:{name}", FIT_PROTOCOL.header_group0),
            TagValue::Integer(i64::from(protocol_version)),
            TagValue::Integer(i64::from(protocol_version)),
            None,
            SHIM_DEFAULT_PRIORITY,
            FIT_PROTOCOL.header_group1,
            Instance::default(),
        );
    }
    walk.warn(EXTRACT_EMBEDDED_WARNING.to_string());

    let header_len = usize::from(file[0]);
    let data_len = u32::from_le_bytes([file[4], file[5], file[6], file[7]]) as usize;
    let end = header_len.saturating_add(data_len);
    // Garmin.pm 6317: the rest of a longer header is read (and skipped).
    let mut pos = header_len.max(12).min(file.len());
    let mut definitions: [Option<Definition>; 16] = std::array::from_fn(|_| None);
    let mut done: HashSet<u16> = HashSet::new();
    let mut error: Option<String> = None;

    loop {
        if pos >= end {
            break;
        }
        let Some(&flags) = file.get(pos) else {
            break;
        };
        pos += 1;
        let local;
        if flags & 0x80 != 0 {
            // Garmin.pm 6323-6336: compressed timestamp header.
            local = usize::from((flags >> 5) & 0x03);
            let offset = i64::from(flags & 0x1f);
            if offset != 0 {
                // Perl's `&` converts a negative IV to its two's-complement UV,
                // so the masked value always fits: `0xffffffe0 + 0x3f` at most.
                let running = walk.timestamp as u64;
                let low = (running & 0x1f) as i64;
                let mut ts = (running & 0xffff_ffe0) as i64 + offset;
                if offset < low {
                    ts += 0x20;
                }
                // `$msg{$localNum}{TS} = [253, $ts]` autovivifies a missing
                // definition, a state ProcessFIT then reads without a size or
                // message number. Not modeled: stop.
                let Some(definition) = definitions[local].as_mut() else {
                    break;
                };
                definition.ts = Some(TimeStampSlot::Compressed(ts));
            }
        } else {
            local = usize::from(flags & 0x0f);
            if flags & 0x40 != 0 {
                // Definition message (Garmin.pm 6341-6431).
                let Some(fixed) = file.get(pos..pos + 5) else {
                    pos = file.len();
                    if pos != end + 2 {
                        error = Some("Unexpected end of file".to_string());
                    }
                    break;
                };
                pos += 5;
                let big_endian = fixed[1] != 0;
                let message_num = if big_endian {
                    u16::from_be_bytes([fixed[2], fixed[3]])
                } else {
                    u16::from_le_bytes([fixed[2], fixed[3]])
                };
                let field_count = usize::from(fixed[4]);
                let Some((message, unknown)) = resolve_message(message_num) else {
                    break; // an edge the generator withheld: its semantics are unreviewed
                };
                let Some(fields) = file.get(pos..pos + field_count * 3) else {
                    error = Some("Truncated definition message".to_string());
                    break;
                };
                pos += field_count * 3;
                // Garmin.pm 6381: no field list for an Unknown message.
                let mut field_info = (!unknown).then(Vec::new);
                let mut ts = None;
                let mut total = 0usize;
                for field in fields.chunks_exact(3) {
                    let (num, size, type_id) = (field[0], usize::from(field[1]), field[2]);
                    if let Some(base) = FIT_PROTOCOL.base_type(type_id) {
                        if num == 253 {
                            ts = Some(TimeStampSlot::Field {
                                offset: total,
                                size,
                                base,
                            });
                        }
                        if let Some(list) = field_info.as_mut() {
                            list.push(FieldInfo::Standard { num, size, base });
                        }
                    } else {
                        walk.warn(format!("Unknown field type {type_id}"));
                    }
                    total += size;
                }
                if flags & 0x20 != 0 {
                    let Some(&dev_count) = file.get(pos) else {
                        error = Some("Missing developer definition".to_string());
                        break;
                    };
                    pos += 1;
                    let length = usize::from(dev_count) * 3;
                    let Some(dev_fields) = file.get(pos..pos + length) else {
                        error = Some("Truncated developer definition".to_string());
                        break;
                    };
                    pos += length;
                    for field in dev_fields.chunks_exact(3) {
                        let size = usize::from(field[1]);
                        if let Some(list) = field_info.as_mut() {
                            list.push(FieldInfo::Developer { size });
                        }
                        total += size;
                    }
                }
                definitions[local] = Some(Definition {
                    message_num,
                    big_endian,
                    size: total,
                    message,
                    field_info,
                    ts,
                });
                continue;
            }
        }

        // Data message (Garmin.pm 6435-6592).
        let Some(definition) = definitions[local].as_ref() else {
            error = Some(format!("Missing definition for local message {local}"));
            break;
        };
        let size = definition.size;
        // Garmin.pm 6438-6445: without ExtractEmbedded only the first record
        // of each message number is processed.
        if !done.insert(definition.message_num) {
            pos = pos.saturating_add(size);
            continue;
        }
        let Some(record) = file.get(pos..pos + size) else {
            error = Some("Truncated data message".to_string());
            break;
        };
        pos += size;

        // Garmin.pm 6459-6482: the running timestamp and document number.
        if let Some(slot) = definition.ts {
            let value = match slot {
                TimeStampSlot::Compressed(value) => Some(value),
                TimeStampSlot::Field { offset, size, base } => {
                    let width = base.format.size();
                    let integer = !matches!(
                        base.format,
                        FitFormat::Float | FitFormat::Double | FitFormat::String | FitFormat::Undef
                    );
                    match record.get(offset..offset + width) {
                        Some(bytes) if integer && size >= width => {
                            match read_element(base.format, bytes, definition.big_endian) {
                                (Some(DecodedValue::Integer(value)), _) => Some(value),
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                }
            };
            let Some(value) = value else {
                break; // not an i64 integer (a float, a short read, >= 2^63): not modeled
            };
            if walk.timestamp != value {
                walk.timestamp = value;
                walk.doc_count += 1;
                walk.doc_num = Some(walk.doc_count);
                let from_field = matches!(slot, TimeStampSlot::Field { .. });
                if !(definition.field_info.is_some() && from_field) {
                    if let Some(field) = FIT_PROTOCOL.common.field(253) {
                        let decoded = Decoded {
                            value: Some(DecodedValue::Integer(value)),
                            text: value.to_string(),
                        };
                        let group0 = FIT_PROTOCOL.common.group0;
                        let group1 = definition.message.group1.clone();
                        walk.emit(group0, &group1, field, decoded, false);
                    }
                }
            }
        }

        let Some(field_info) = definition.field_info.as_ref() else {
            continue;
        };
        let mut offset = 0usize;
        for info in field_info {
            let (num, size, base) = match *info {
                FieldInfo::Standard { num, size, base } => (num, size, base),
                FieldInfo::Developer { size } => {
                    offset += size;
                    continue;
                }
            };
            // Garmin.pm 6492-6502: message table, else Common under the
            // message's group 1, else an Unknown tag (default mode: silent).
            let table_field = definition
                .message
                .table
                .and_then(|table| table.field(num).map(|field| (table, field)));
            let target = table_field.or_else(|| {
                FIT_PROTOCOL
                    .common
                    .field(num)
                    .map(|field| (FIT_PROTOCOL.common, field))
            });
            let width = base.format.size();
            if size % width != 0 {
                walk.warn(format!(
                    "Bad count for {} {} field {num}",
                    format_name(base.format),
                    definition.message.name
                ));
                offset += size;
                continue;
            }
            if let Some((table, field)) = target {
                let decoded = record
                    .get(offset..offset + size)
                    .map(|bytes| decode(base.format, bytes, definition.big_endian));
                // Garmin.pm 6557: drop the value whose text is the invalid value.
                if let Some(decoded) =
                    decoded.filter(|decoded| perl_lc(&decoded.text) != base.invalid)
                {
                    if base.admitted {
                        let group1 = if std::ptr::eq(table, FIT_PROTOCOL.common) {
                            definition.message.group1.clone()
                        } else {
                            table.group1.to_string()
                        };
                        walk.emit(
                            table.group0,
                            &group1,
                            field,
                            decoded,
                            base.format == FitFormat::Undef,
                        );
                    }
                }
            }
            offset += size;
        }
    }
    if let Some(error) = error {
        walk.warn(error);
    }
}

/// ExifTool's format name, as ProcessFIT interpolates it into warnings.
const fn format_name(format: FitFormat) -> &'static str {
    match format {
        FitFormat::Int8u => "int8u",
        FitFormat::Int8s => "int8s",
        FitFormat::Int16u => "int16u",
        FitFormat::Int16s => "int16s",
        FitFormat::Int32u => "int32u",
        FitFormat::Int32s => "int32s",
        FitFormat::Int64u => "int64u",
        FitFormat::Int64s => "int64s",
        FitFormat::Float => "float",
        FitFormat::Double => "double",
        FitFormat::String => "string",
        FitFormat::Undef => "undef",
    }
}

/// A refused protocol, or a release without the Garmin module, extracts
/// nothing (not even the header row or the ExtractEmbedded warning).
fn extract(protocol: &FitProtocol, file: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    if protocol.active() {
        parse_records(file, &mut metadata);
    }
    metadata
}

pub struct FITParser;

impl FITParser {
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 12 {
            return Ok(false);
        }
        Ok(reader.read(8, 4)? == b".FIT")
    }
}

impl FormatParser for FITParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("invalid FIT signature"));
        }
        let file = reader.read(0, reader.size() as usize)?;
        Ok(extract(&FIT_PROTOCOL, file))
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        format == FileFormat::FIT
    }
}

pub fn parse_fit_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    FITParser.parse(reader).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exiftool_tables::fit_schema::FitUnavailable;

    const TS: u32 = 1_100_000_000;

    /// A FIT stream: 12-byte header, records, two (unchecked) CRC bytes.
    fn fit(records: &[u8]) -> Vec<u8> {
        let mut out = vec![12, 0x10, 0x54, 0x08];
        out.extend_from_slice(&(records.len() as u32).to_le_bytes());
        out.extend_from_slice(b".FIT");
        out.extend_from_slice(records);
        out.extend_from_slice(&[0, 0]);
        out
    }

    fn definition(local: u8, message: u16, big: bool, fields: &[(u8, u8, u8)]) -> Vec<u8> {
        let mut out = vec![0x40 | local, 0, u8::from(big)];
        out.extend_from_slice(&if big {
            message.to_be_bytes()
        } else {
            message.to_le_bytes()
        });
        out.push(fields.len() as u8);
        for &(num, size, base) in fields {
            out.extend_from_slice(&[num, size, base]);
        }
        out
    }

    fn read(bytes: &[u8]) -> MetadataMap {
        let mut metadata = MetadataMap::new();
        parse_records(bytes, &mut metadata);
        metadata
    }

    fn text(value: &TagValue) -> String {
        match value {
            TagValue::String(text) => text.clone(),
            TagValue::Integer(integer) => integer.to_string(),
            other => format!("{other:?}"),
        }
    }

    fn value(metadata: &MetadataMap, key: &str) -> Option<String> {
        metadata.get(key).map(text)
    }

    fn group1(metadata: &MetadataMap, key: &str) -> Vec<String> {
        metadata
            .occurrences_for(key)
            .iter()
            .map(|occurrence| occurrence.group1.as_ref().to_string())
            .collect()
    }

    /// The Session fields the replaced handwritten parser reported, with the
    /// values it reported for them (which matched pinned ExifTool).
    fn session_record(big: bool) -> Vec<u8> {
        let fields = [
            (16, 1, 0x02),
            (18, 1, 0x02),
            (92, 1, 0x02),
            (116, 4, 0x02),
            (117, 4, 0x02),
            (118, 4, 0x02),
            (119, 4, 0x02),
            (122, 2, 0x02),
        ];
        let mut records = definition(0, 18, big, &fields);
        records.push(0);
        records.extend_from_slice(&[87, 13, 68]);
        records.extend_from_slice(&[255; 16]);
        records.extend_from_slice(&[255, 255]);
        records
    }

    #[test]
    fn replaced_session_values_are_preserved_under_their_message_group() {
        for big in [false, true] {
            let metadata = read(&fit(&session_record(big)));
            let expected = [
                ("Garmin:AvgHeartRate", "87 bpm"),
                ("Garmin:AvgCadence", "13 rpm"),
                ("Garmin:AvgFractionalCadence", "0.53125 rpm"),
                ("Garmin:AvgLeftPowerPhase", "255 255 255 255"),
                ("Garmin:AvgLeftPowerPhasePeak", "255 255 255 255"),
                ("Garmin:AvgRightPowerPhase", "255 255 255 255"),
                ("Garmin:AvgRightPowerPhasePeak", "255 255 255 255"),
                ("Garmin:AvgCadencePosition", "255 255"),
            ];
            for (key, text) in expected {
                assert_eq!(
                    value(&metadata, key).as_deref(),
                    Some(text),
                    "{key} big={big}"
                );
                assert_eq!(group1(&metadata, key), ["Session"], "{key}");
            }
            assert_eq!(
                value(&metadata, "Garmin:ProtocolVersion").as_deref(),
                Some("16")
            );
            assert_eq!(group1(&metadata, "Garmin:ProtocolVersion"), ["File"]);
        }
    }

    #[test]
    fn unknown_base_type_is_sized_but_shifts_later_reads() {
        // Garmin.pm 6386-6401: the unknown-type field counts toward the record
        // size but not the field list, so field 16 reads its byte.
        let mut records = definition(
            0,
            18,
            false,
            &[(200, 1, 0x55), (16, 1, 0x02), (18, 1, 0x02)],
        );
        records.extend_from_slice(&[0, 50, 60, 70]);
        let metadata = read(&fit(&records));
        assert_eq!(
            value(&metadata, "Garmin:AvgHeartRate").as_deref(),
            Some("50 bpm")
        );
        assert_eq!(
            value(&metadata, "Garmin:AvgCadence").as_deref(),
            Some("60 rpm")
        );
        let warnings: Vec<_> = metadata
            .occurrences_for("ExifTool:Warning")
            .iter()
            .map(|occurrence| text(&occurrence.raw))
            .collect();
        assert!(
            warnings
                .iter()
                .any(|warning| warning == "Unknown field type 85"),
            "{warnings:?}"
        );
    }

    #[test]
    fn only_the_first_record_of_a_message_is_read() {
        let mut records = definition(0, 18, false, &[(16, 1, 0x02)]);
        records.extend_from_slice(&[0, 87, 0, 99]);
        let metadata = read(&fit(&records));
        assert_eq!(
            value(&metadata, "Garmin:AvgHeartRate").as_deref(),
            Some("87 bpm")
        );
    }

    #[test]
    fn timestamps_report_through_common_under_the_message_group() {
        // An Unknown-flagged message (Activity, 34) contributes only its
        // TimeStamp; a compressed header rolls it forward for Record (20).
        let mut records = definition(0, 34, false, &[(253, 4, 0x86), (1, 2, 0x84)]);
        records.push(0);
        records.extend_from_slice(&TS.to_le_bytes());
        records.extend_from_slice(&[7, 0]);
        records.extend(definition(1, 20, false, &[(3, 1, 0x02)]));
        records.push(0x80 | (1 << 5) | ((TS + 3) & 0x1f) as u8);
        records.push(70);
        let metadata = read(&fit(&records));
        assert_eq!(
            group1(&metadata, "Garmin:TimeStamp"),
            ["Activity", "Record"]
        );
        assert_eq!(
            value(&metadata, "Garmin:HeartRate").as_deref(),
            Some("70 bpm")
        );
        assert!(metadata.get("Garmin:Timer").is_none());
    }

    #[test]
    fn stream_errors_end_the_walk_with_the_native_warning() {
        let mut records = session_record(false);
        records.extend_from_slice(&[3, 0]);
        let metadata = read(&fit(&records));
        assert_eq!(
            value(&metadata, "Garmin:AvgHeartRate").as_deref(),
            Some("87 bpm")
        );
        assert!(
            metadata
                .occurrences_for("ExifTool:Warning")
                .iter()
                .any(|occurrence| text(&occurrence.raw)
                    == "Missing definition for local message 3")
        );
    }

    #[test]
    fn generated_protocol_is_admitted() {
        // A regeneration that refuses the protocol (a changed ProcessFIT or
        // value reader) must fail loudly here, not ship a silent FIT reader.
        // A release proven to ship no Garmin module is the one other
        // accepted state, and it must define nothing at all.
        match FIT_PROTOCOL.refusal {
            None => assert!(
                FIT_PROTOCOL
                    .messages
                    .iter()
                    .all(|message| message.withheld.is_none())
            ),
            Some(FitUnavailable::ModuleAbsent { .. }) => {
                assert!(FIT_PROTOCOL.messages.is_empty());
                assert!(FIT_PROTOCOL.base_types.is_empty());
                assert!(FIT_PROTOCOL.common.fields.is_empty());
                assert_eq!(FIT_PROTOCOL.header_name, None);
            }
            Some(FitUnavailable::Refused(reasons)) => panic!("FIT protocol refused: {reasons}"),
        }
    }

    /// A protocol like the one generated for a release without the module.
    fn module_absent_protocol() -> FitProtocol {
        static EMPTY: FitTable = FitTable {
            table: "",
            group0: "",
            group1: "",
            group2: "",
            fields: &[],
        };
        FitProtocol {
            refusal: Some(FitUnavailable::ModuleAbsent {
                exiftool_version: "11.78",
            }),
            base_types: &[],
            messages: &[],
            common: &EMPTY,
            header_name: None,
            header_group0: "",
            header_group1: "",
            header_group2: "",
        }
    }

    #[test]
    fn module_absent_and_refused_are_inactive_but_distinct() {
        let absent = module_absent_protocol();
        let refused = FitProtocol {
            refusal: Some(FitUnavailable::Refused(
                "missing_or_changed_reader_protocol:ProcessFIT",
            )),
            ..absent
        };
        assert!(!absent.active());
        assert!(!refused.active());
        assert_ne!(absent.refusal, refused.refusal);
        assert!(matches!(
            absent.refusal,
            Some(FitUnavailable::ModuleAbsent {
                exiftool_version: "11.78"
            })
        ));
        // The same stream the pinned protocol reads yields zero tags under
        // either inactive state (and under the generated protocol too, when
        // this build was generated for a release without the module).
        let stream = fit(&session_record(false));
        assert_eq!(
            extract(&FIT_PROTOCOL, &stream).is_empty(),
            !FIT_PROTOCOL.active()
        );
        assert!(extract(&absent, &stream).is_empty());
        assert!(extract(&refused, &stream).is_empty());
    }

    #[test]
    fn executor_names_no_generated_message_or_field() {
        // Names come only from the generated specs: no FIT message or field
        // name appears as a string literal in this executor.
        let source = include_str!("fit.rs");
        let executor = &source[..source.find("#[cfg(test)]").expect("test module")];
        let mut names: Vec<&str> = FIT_PROTOCOL
            .messages
            .iter()
            .map(|message| message.name)
            .collect();
        let tables = FIT_PROTOCOL
            .messages
            .iter()
            .filter_map(|message| message.table)
            .chain(std::iter::once(FIT_PROTOCOL.common));
        names.extend(tables.flat_map(|table| table.fields.iter().map(|field| field.name)));
        names.extend(FIT_PROTOCOL.header_name);
        for name in names {
            assert!(
                !executor.contains(&format!("\"{name}\"")),
                "hand-written name {name}"
            );
        }
    }
}
