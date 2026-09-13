//! Reader for source-described `ProcessSerialData` layouts.
//!
//! This is intentionally a caller-less shared primitive.  The caller supplies
//! a bounded directory and must opt in through [`SerialEmissionSink`].  The
//! generated schema supplies all source-specific slot, format, count,
//! condition, conversion and group data.

use std::collections::HashMap;

use crate::core::TagValue;
use crate::io::ByteOrder;

use super::IfdFlags;
use super::cond::Ctx;
use super::engine::{self, Emitted};
use super::runtime::{self, DecodedValue};
use super::serial_schema::{
    SerialCount, SerialEntry, SerialPrintConv, SerialTable, SerialTag,
};

/// The bounded `dirInfo` values ProcessSerialData receives from its carrier.
#[derive(Clone, Copy, Debug)]
pub struct SerialDir<'a> {
    pub data: &'a [u8],
    pub dir_start: usize,
    pub dir_len: usize,
    pub base: i64,
    pub data_pos: i64,
    pub byte_order: ByteOrder,
}

/// A caller's projection target for a serial directory.
///
/// Gate B defaults to false.  Adding generated serial tables therefore cannot
/// activate a production parser without a carrier explicitly opting in.
pub trait SerialEmissionSink {
    fn emit(&mut self, row: Emitted);

    fn serial_enabled(&self, _table: &'static SerialTable) -> bool {
        false
    }

    /// ProcessSerialData temporarily enables Unknown for `GetTagInfo` to
    /// retain cursor alignment, but it calls FoundTag for an Unknown row only
    /// if the caller originally requested Unknown output.
    fn unknown_enabled(&self) -> bool {
        false
    }
}

/// Counted outcomes of one serial walk.  Every non-output path is explicit so
/// a future carrier cannot mistake a withheld row for native success.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SerialWalkResult {
    pub entries_seen: usize,
    pub emitted: usize,
    pub gate_a_blocked: usize,
    pub gate_b_blocked: usize,
    pub no_matching_alternative: usize,
    pub malformed_layout: usize,
    pub unreadable_value: usize,
    pub unavailable_prior_raw: usize,
    pub unavailable_condition_context: usize,
    pub omitted: usize,
    pub tainted: bool,
}

enum Selection {
    Selected {
        tag: &'static SerialTag,
        condition_resolved: bool,
    },
    NoMatch,
    UnavailableContext,
    RefusedCondition,
}

/// Walk a source-described serial table in native key and cursor order.
///
/// `GetTagInfo` selection happens before a selected row's count and bytes are
/// computed.  Once ReadValue succeeds its raw value is retained under this
/// index before reporting, exactly so a later count expression observes raw
/// native data rather than rendered output.
#[must_use]
pub fn process_serial_directory(
    table: &'static SerialTable,
    dir: SerialDir<'_>,
    ctx: &mut Ctx,
    sink: &mut dyn SerialEmissionSink,
) -> SerialWalkResult {
    let mut result = SerialWalkResult::default();
    if !table.gate_a.passes() {
        result.gate_a_blocked = 1;
        return result;
    }
    if !sink.serial_enabled(table) {
        result.gate_b_blocked = 1;
        return result;
    }

    let mut prior_raw = HashMap::<usize, DecodedValue>::new();
    let mut pos = 0usize;
    for (expected_index, entry) in table.entries.iter().enumerate() {
        if entry.serial_index != expected_index {
            result.malformed_layout += 1;
            result.tainted = true;
            break;
        }
        if pos > dir.dir_len {
            break;
        }
        result.entries_seen += 1;
        let selected = select(entry, ctx);
        let (tag, condition_resolved) = match selected {
            Selection::Selected {
                tag,
                condition_resolved,
            } => (tag, condition_resolved),
            // Native `GetTagInfo(...) or last`: no matching alternative ends
            // this serial directory rather than falling through to its next
            // numeric key.
            Selection::NoMatch => {
                result.no_matching_alternative += 1;
                break;
            }
            Selection::UnavailableContext => {
                result.unavailable_condition_context += 1;
                result.tainted = true;
                break;
            }
            Selection::RefusedCondition => {
                result.omitted += 1;
                result.tainted = true;
                break;
            }
        };

        // A native Hook executes before ProcessSerialData computes this
        // field's length and calls ReadValue.  Reading with the generated
        // operands first would manufacture both this value and every later
        // cursor position, so a malformed artifact cannot defer this refusal
        // until report time.
        if tag.omitted.hook {
            result.omitted += 1;
            result.tainted = true;
            break;
        }

        let more = match dir.dir_len.checked_sub(pos) {
            Some(more) => more,
            None => break,
        };
        // The staged reader models only its source-proven scalar and string
        // operand family. An artifact must not promote another common `Fmt`
        // merely because the shared low-level decoder happens to know how to
        // read it.
        if !serial_format_supported(tag.format.format) {
            result.malformed_layout += 1;
            result.tainted = true;
            break;
        }
        let count = match count_for(tag, &prior_raw, more) {
            Some(count) => count,
            None => {
                result.unavailable_prior_raw += 1;
                result.tainted = true;
                break;
            }
        };
        let Some(len) = usize::try_from(tag.format.format.size())
            .ok()
            .and_then(|size| size.checked_mul(count))
        else {
            result.malformed_layout += 1;
            result.tainted = true;
            break;
        };
        // ProcessSerialData checks its requested span before ReadValue, so a
        // short final scalar stops the directory instead of becoming a
        // shortened value (the general read port still owns its own string
        // shortening rules for a valid request).
        if match pos.checked_add(len) {
            Some(end) => end > dir.dir_len,
            None => true,
        } {
            break;
        }
        let Some(offset) = dir.dir_start.checked_add(pos) else {
            result.unreadable_value += 1;
            break;
        };
        let Some(more_i64) = i64::try_from(more).ok() else {
            result.malformed_layout += 1;
            result.tainted = true;
            break;
        };
        let Some(raw) = engine::read_value(
            dir.data,
            offset,
            tag.format.format,
            count,
            more_i64,
            dir.byte_order,
        ) else {
            result.unreadable_value += 1;
            break;
        };
        prior_raw.insert(entry.serial_index, raw.clone());

        // No serial SubDirectory is currently represented. It would run
        // after raw storage and before FoundTag, so an omitted edge taints the
        // remaining directory rather than allowing later state to look safe.
        if tag.omitted.subdirectory {
            result.omitted += 1;
            result.tainted = true;
            break;
        }
        // ProcessSerialData still remembers a zero-length ReadValue result,
        // but its `FoundTag` call is guarded by `if $count`; RawConv and
        // reporting therefore do not run for this slot.
        if count == 0 {
            pos += len;
            continue;
        }
        // The processor retains Unknown rows for index synchronization, but
        // never invokes FoundTag (and therefore never RawConv) unless Unknown
        // output was requested by its caller.
        if tag.flags.unknown && !sink.unknown_enabled() {
            pos += len;
            continue;
        }
        if emit_selected(table, tag, condition_resolved, raw, ctx, sink, &mut result) {
            break;
        }
        pos += len;
    }
    result
}

fn select(entry: &SerialEntry, ctx: &mut Ctx) -> Selection {
    for tag in entry.alternatives {
        if tag.condition.is_none() && tag.omitted.condition {
            return Selection::RefusedCondition;
        }
        let Some(condition) = tag.condition else {
            return Selection::Selected {
                tag,
                condition_resolved: false,
            };
        };
        // ProcessSerialData calls GetTagInfo before it has `$format`, `$count`
        // or a value pointer. This staged reader has no faithful retry path;
        // its generator must gate such rows, and this catches a bad artifact.
        if condition.needs_value_context() {
            return Selection::UnavailableContext;
        }
        if condition.eval(ctx) {
            return Selection::Selected {
                tag,
                condition_resolved: true,
            };
        }
    }
    Selection::NoMatch
}

const fn serial_format_supported(format: super::Fmt) -> bool {
    matches!(
        format,
        super::Fmt::Int8u
            | super::Fmt::Int16u
            | super::Fmt::Int16s
            | super::Fmt::Int32u
            | super::Fmt::Str(_)
            | super::Fmt::Undef(_)
            | super::Fmt::RemainderString
    )
}

fn count_for(
    tag: &SerialTag,
    prior_raw: &HashMap<usize, DecodedValue>,
    more: usize,
) -> Option<usize> {
    match tag.format.count {
        SerialCount::Fixed { value } => Some(value),
        SerialCount::RemainingBytes => Some(more),
        SerialCount::PriorRaw { serial_index } => prior_raw
            .get(&serial_index)?
            .as_integer()
            .and_then(|value| usize::try_from(value).ok()),
        SerialCount::FloorDivPriorRaw {
            serial_index,
            add,
            divisor,
            trailing_add,
        } => prior_raw
            .get(&serial_index)?
            .as_integer()
            .and_then(|value| usize::try_from(value).ok())?
            .checked_add(add)
            .and_then(|value| value.checked_div(divisor))?
            .checked_add(trailing_add),
    }
}

/// Returns true when a state-affecting refusal makes the remaining directory
/// unsafe to interpret.
fn emit_selected(
    table: &'static SerialTable,
    tag: &'static SerialTag,
    condition_resolved: bool,
    raw: DecodedValue,
    ctx: &mut Ctx,
    sink: &mut dyn SerialEmissionSink,
    result: &mut SerialWalkResult,
) -> bool {
    let mut omitted = tag.omitted;
    if condition_resolved {
        omitted.condition = false;
    }
    match tag.raw_conv {
        Some(super::RawConvEffect::SetMember { member }) => {
            let Some(value) = engine::member_value(&raw) else {
                result.omitted += 1;
                result.tainted = true;
                return true;
            };
            ctx.members.insert(member, value);
            omitted.raw_conv = false;
        }
        Some(super::RawConvEffect::ValueLocal) | None => {}
    }
    if omitted.raw_conv && tag.raw_conv.is_none() {
        result.omitted += 1;
        result.tainted = true;
        return true;
    }
    if omitted.any() {
        result.omitted += 1;
        return false;
    }
    let Some(converted) = runtime::apply_value_conv(tag.value_conv, &raw) else {
        result.omitted += 1;
        return false;
    };
    let (value, value_conv) = if tag.flags.binary && tag.value_conv.is_none() {
        let Some(length) = perl_length(&raw) else {
            result.omitted += 1;
            return false;
        };
        (
            TagValue::String(format!(
                "(Binary data {length} bytes, use -b option to extract)"
            )),
            None,
        )
    } else {
        let unconverted = || {
            if tag.flags.list {
                runtime::to_tag_value(&converted)
            } else {
                runtime::to_exiftool_value(&converted)
            }
        };
        match render_serial(tag.print_conv, &converted) {
            Some(rendered) => (TagValue::String(rendered), Some(unconverted())),
            None => (unconverted(), None),
        }
    };
    sink.emit(Emitted {
        module: table.module,
        table: table.table,
        group0: tag.groups.g0.unwrap_or(table.group0),
        group1: tag.groups.g1.unwrap_or(table.group1),
        group2: tag.groups.g2.unwrap_or(table.group2),
        name: tag.name,
        value,
        value_conv,
        low_priority: low_priority(tag.flags),
        avoid: tag.flags.avoid,
        rational: None,
    });
    result.emitted += 1;
    false
}

/// Render the serial-only conversion arm after ValueConv.  This is kept at
/// the ProcessSerialData boundary because the native input is its scalar or
/// space-joined list of words, not a general binary-table BITMASK.
fn render_serial(conv: SerialPrintConv, value: &DecodedValue) -> Option<String> {
    match conv {
        SerialPrintConv::None => None,
        SerialPrintConv::Shared(conv) => runtime::render(conv, value),
        SerialPrintConv::DecodeBitsWords { bits_per_word } => {
            decode_bits_words(value, bits_per_word)
        }
    }
}

/// `Image::ExifTool::DecodeBits($val, undef, $bits)` for the closed serial
/// no-lookup shape. Signed words are masked to their width before each bit is
/// inspected: native `-32768` with 16 bits yields `15`, never sign-extended
/// positions above 15. Native joins this no-lookup branch with `","`.
fn decode_bits_words(value: &DecodedValue, bits_per_word: u8) -> Option<String> {
    let bits = u32::from(bits_per_word);
    if bits == 0 || bits > 63 {
        return None;
    }
    let values: Vec<i64> = match value {
        DecodedValue::Integer(value) => vec![*value],
        DecodedValue::Array(values) => values
            .iter()
            .map(DecodedValue::as_integer)
            .collect::<Option<Vec<_>>>()?,
        _ => return None,
    };
    let mask = (1_u64.checked_shl(bits)?).checked_sub(1)?;
    let mut labels = Vec::new();
    for (word_index, word) in values.into_iter().enumerate() {
        let word = (word as u64) & mask;
        let base = u32::try_from(word_index).ok()?.checked_mul(bits)?;
        for bit in 0..bits {
            if word & (1_u64 << bit) != 0 {
                labels.push(base.checked_add(bit)?.to_string());
            }
        }
    }
    Some(if labels.is_empty() {
        "(none)".to_string()
    } else {
        labels.join(",")
    })
}

fn low_priority(flags: IfdFlags) -> bool {
    flags.priority.or(if flags.avoid { Some(0) } else { None }) == Some(0)
}

fn perl_length(value: &DecodedValue) -> Option<usize> {
    match value {
        DecodedValue::Undefined(bytes) | DecodedValue::StringBytes(bytes) => Some(bytes.len()),
        DecodedValue::String(value) => Some(value.len()),
        other => other.perl_string().map(|value| value.len()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::exiftool_tables::{
        Cond, EffectSource, GateA, IfdFlags, Omitted, PrintConv, RawConvEffect, SerialCondition, SerialCount,
        SerialEntry, SerialFormat, SerialMissingMember, SerialPrintConv, SerialProcessorFacts,
        SerialTable, SerialTag, TagGroups,
    };

    static PROCESSOR: SerialProcessorFacts = SerialProcessorFacts {
        name: "Image::ExifTool::Shared::ProcessSerialData",
        source_file: "Image/ExifTool/Shared.pm",
        source_sha256: "source",
        source_body_sha256: "body",
    };
    static EMPTY_GATE: GateA = GateA { blocked_by: &[] };
    static BLOCKED_GATE: GateA = GateA {
        blocked_by: &[("serial_row_shape", 1)],
    };
    static PLAIN: SerialTag = SerialTag {
        name: "Count",
        format: SerialFormat {
            format: super::super::Fmt::Int8u,
            count: SerialCount::Fixed { value: 1 },
        },
        condition: None,
        flags: IfdFlags::NONE,
        raw_conv: None,
        omitted: Omitted::NONE,
        value_conv: None,
        print_conv: SerialPrintConv::None,
        groups: TagGroups::NONE,
    };
    static PRIOR: SerialTag = SerialTag {
        name: "Values",
        format: SerialFormat {
            format: super::super::Fmt::Int16u,
            count: SerialCount::PriorRaw { serial_index: 0 },
        },
        condition: None,
        flags: IfdFlags::NONE,
        raw_conv: None,
        omitted: Omitted::NONE,
        value_conv: None,
        print_conv: SerialPrintConv::None,
        groups: TagGroups::NONE,
    };
    static UNKNOWN: SerialTag = SerialTag {
        name: "UnknownCount",
        format: SerialFormat {
            format: super::super::Fmt::Int8u,
            count: SerialCount::Fixed { value: 1 },
        },
        condition: None,
        flags: IfdFlags {
            unknown: true,
            ..IfdFlags::NONE
        },
        raw_conv: Some(RawConvEffect::SetMember {
            member: "UnknownSeen",
        }),
        omitted: Omitted {
            raw_conv: true,
            ..Omitted::NONE
        },
        value_conv: None,
        print_conv: SerialPrintConv::None,
        groups: TagGroups::NONE,
    };
    static MEMBER: SerialTag = SerialTag {
        name: "State",
        format: SerialFormat {
            format: super::super::Fmt::Int8u,
            count: SerialCount::Fixed { value: 1 },
        },
        condition: None,
        flags: IfdFlags::NONE,
        raw_conv: Some(RawConvEffect::SetMember { member: "Seen" }),
        omitted: Omitted {
            raw_conv: true,
            ..Omitted::NONE
        },
        value_conv: None,
        print_conv: SerialPrintConv::None,
        groups: TagGroups::NONE,
    };
    static AFTER_MEMBER: SerialTag = SerialTag {
        name: "AfterState",
        format: SerialFormat {
            format: super::super::Fmt::Int8u,
            count: SerialCount::Fixed { value: 1 },
        },
        condition: Some(SerialCondition {
            cond: Cond::MemberCmp {
                member: "Seen",
                op: super::super::CmpOp::Eq,
                value: 7,
            },
            missing_member: SerialMissingMember::SharedDefault,
        }),
        flags: IfdFlags::NONE,
        raw_conv: None,
        omitted: Omitted {
            condition: true,
            ..Omitted::NONE
        },
        value_conv: None,
        print_conv: SerialPrintConv::None,
        groups: TagGroups::NONE,
    };
    static NEVER: SerialTag = SerialTag {
        name: "Never",
        format: SerialFormat {
            format: super::super::Fmt::Int8u,
            count: SerialCount::Fixed { value: 1 },
        },
        condition: Some(SerialCondition {
            cond: Cond::MemberTruthy {
                member: "missing",
                negate: false,
            },
            missing_member: SerialMissingMember::SharedDefault,
        }),
        flags: IfdFlags::NONE,
        raw_conv: None,
        omitted: Omitted::NONE,
        value_conv: None,
        print_conv: SerialPrintConv::None,
        groups: TagGroups::NONE,
    };
    static NON_NUMERIC: SerialTag = SerialTag {
        name: "NonNumericCount",
        format: SerialFormat {
            format: super::super::Fmt::Str(1),
            count: SerialCount::Fixed { value: 1 },
        },
        condition: None,
        flags: IfdFlags::NONE,
        raw_conv: None,
        omitted: Omitted::NONE,
        value_conv: None,
        print_conv: SerialPrintConv::None,
        groups: TagGroups::NONE,
    };
    static NEEDS_CONTEXT: SerialTag = SerialTag {
        name: "NeedsContext",
        format: SerialFormat {
            format: super::super::Fmt::Int8u,
            count: SerialCount::Fixed { value: 1 },
        },
        condition: Some(SerialCondition {
            cond: Cond::CountCmp {
                op: super::super::CmpOp::Eq,
                value: 1,
            },
            missing_member: SerialMissingMember::SharedDefault,
        }),
        flags: IfdFlags::NONE,
        raw_conv: None,
        omitted: Omitted::NONE,
        value_conv: None,
        print_conv: SerialPrintConv::None,
        groups: TagGroups::NONE,
    };
    static COUNTED_STRING: SerialTag = SerialTag {
        name: "CountedString",
        format: SerialFormat {
            format: super::super::Fmt::Str(1),
            count: SerialCount::PriorRaw { serial_index: 0 },
        },
        condition: None,
        flags: IfdFlags::NONE,
        raw_conv: None,
        omitted: Omitted::NONE,
        value_conv: None,
        print_conv: SerialPrintConv::None,
        groups: TagGroups::NONE,
    };
    static HOOK: SerialTag = SerialTag {
        name: "Hooked",
        format: SerialFormat {
            format: super::super::Fmt::Int8u,
            count: SerialCount::Fixed { value: 1 },
        },
        condition: None,
        flags: IfdFlags::NONE,
        raw_conv: None,
        omitted: Omitted {
            hook: true,
            ..Omitted::NONE
        },
        value_conv: None,
        print_conv: SerialPrintConv::None,
        groups: TagGroups::NONE,
    };
    static UNSUPPORTED_FORMAT: SerialTag = SerialTag {
        name: "UnprovenFormat",
        format: SerialFormat {
            format: super::super::Fmt::Int8s,
            count: SerialCount::Fixed { value: 1 },
        },
        condition: None,
        flags: IfdFlags::NONE,
        raw_conv: None,
        omitted: Omitted::NONE,
        value_conv: None,
        print_conv: SerialPrintConv::None,
        groups: TagGroups::NONE,
    };
    static REMAINDER: SerialTag = SerialTag {
        name: "Remainder",
        format: SerialFormat {
            format: super::super::Fmt::RemainderString,
            count: SerialCount::RemainingBytes,
        },
        condition: None,
        flags: IfdFlags::NONE,
        raw_conv: None,
        omitted: Omitted::NONE,
        value_conv: None,
        print_conv: SerialPrintConv::None,
        groups: TagGroups::NONE,
    };
    static SIGNED_BITS: SerialTag = SerialTag {
        name: "SignedBits",
        format: SerialFormat {
            format: super::super::Fmt::Int16s,
            count: SerialCount::Fixed { value: 3 },
        },
        condition: None,
        flags: IfdFlags::NONE,
        raw_conv: None,
        omitted: Omitted::NONE,
        value_conv: None,
        print_conv: SerialPrintConv::DecodeBitsWords { bits_per_word: 16 },
        groups: TagGroups::NONE,
    };
    static ENTRY_COUNT: [SerialTag; 1] = [PLAIN];
    static ENTRY_VALUES: [SerialTag; 1] = [PRIOR];
    static ENTRY_UNKNOWN: [SerialTag; 1] = [UNKNOWN];
    static ENTRY_MEMBER: [SerialTag; 1] = [MEMBER];
    static ENTRY_AFTER_MEMBER: [SerialTag; 1] = [AFTER_MEMBER];
    static ENTRY_NEVER: [SerialTag; 1] = [NEVER];
    static ENTRY_NON_NUMERIC: [SerialTag; 1] = [NON_NUMERIC];
    static ENTRY_NEEDS_CONTEXT: [SerialTag; 1] = [NEEDS_CONTEXT];
    static ENTRY_COUNTED_STRING: [SerialTag; 1] = [COUNTED_STRING];
    static ENTRY_HOOK: [SerialTag; 1] = [HOOK];
    static ENTRY_UNSUPPORTED_FORMAT: [SerialTag; 1] = [UNSUPPORTED_FORMAT];
    static ENTRY_REMAINDER: [SerialTag; 1] = [REMAINDER];
    static ENTRY_SIGNED_BITS: [SerialTag; 1] = [SIGNED_BITS];
    static TWO_ENTRIES: [SerialEntry; 2] = [
        SerialEntry {
            serial_index: 0,
            alternatives: &ENTRY_COUNT,
        },
        SerialEntry {
            serial_index: 1,
            alternatives: &ENTRY_VALUES,
        },
    ];
    static UNKNOWN_ENTRIES: [SerialEntry; 2] = [
        SerialEntry {
            serial_index: 0,
            alternatives: &ENTRY_UNKNOWN,
        },
        SerialEntry {
            serial_index: 1,
            alternatives: &ENTRY_VALUES,
        },
    ];
    static STATE_ENTRIES: [SerialEntry; 2] = [
        SerialEntry {
            serial_index: 0,
            alternatives: &ENTRY_MEMBER,
        },
        SerialEntry {
            serial_index: 1,
            alternatives: &ENTRY_AFTER_MEMBER,
        },
    ];
    static STOP_ENTRIES: [SerialEntry; 2] = [
        SerialEntry {
            serial_index: 0,
            alternatives: &ENTRY_NEVER,
        },
        SerialEntry {
            serial_index: 1,
            alternatives: &ENTRY_COUNT,
        },
    ];
    static NON_NUMERIC_ENTRIES: [SerialEntry; 2] = [
        SerialEntry {
            serial_index: 0,
            alternatives: &ENTRY_NON_NUMERIC,
        },
        SerialEntry {
            serial_index: 1,
            alternatives: &ENTRY_VALUES,
        },
    ];
    static CONTEXT_ENTRIES: [SerialEntry; 1] = [SerialEntry {
        serial_index: 0,
        alternatives: &ENTRY_NEEDS_CONTEXT,
    }];
    static COUNTED_STRING_ENTRIES: [SerialEntry; 2] = [
        SerialEntry {
            serial_index: 0,
            alternatives: &ENTRY_COUNT,
        },
        SerialEntry {
            serial_index: 1,
            alternatives: &ENTRY_COUNTED_STRING,
        },
    ];
    static HOOK_ENTRIES: [SerialEntry; 1] = [SerialEntry {
        serial_index: 0,
        alternatives: &ENTRY_HOOK,
    }];
    static UNSUPPORTED_FORMAT_ENTRIES: [SerialEntry; 1] = [SerialEntry {
        serial_index: 0,
        alternatives: &ENTRY_UNSUPPORTED_FORMAT,
    }];
    static REMAINDER_ENTRIES: [SerialEntry; 1] = [SerialEntry {
        serial_index: 0,
        alternatives: &ENTRY_REMAINDER,
    }];
    static SIGNED_BITS_ENTRIES: [SerialEntry; 1] = [SerialEntry {
        serial_index: 0,
        alternatives: &ENTRY_SIGNED_BITS,
    }];
    static TABLE: SerialTable = SerialTable {
        module: "Shared",
        table: "Serial",
        group0: "MakerNotes",
        group1: "Shared",
        group2: "Camera",
        default_format: super::super::Fmt::Int8u,
        processor: PROCESSOR,
        gate_a: EMPTY_GATE,
        entries: &TWO_ENTRIES,
    };

    #[derive(Default)]
    struct Sink {
        rows: Vec<Emitted>,
        enabled: bool,
        unknown: bool,
    }
    impl SerialEmissionSink for Sink {
        fn emit(&mut self, row: Emitted) {
            self.rows.push(row);
        }
        fn serial_enabled(&self, _: &'static SerialTable) -> bool {
            self.enabled
        }
        fn unknown_enabled(&self) -> bool {
            self.unknown
        }
    }

    fn walk(
        table: &'static SerialTable,
        data: &[u8],
        sink: &mut Sink,
        members: &mut HashMap<&'static str, super::super::MemberValue>,
    ) -> SerialWalkResult {
        process_serial_directory(
            table,
            SerialDir {
                data,
                dir_start: 0,
                dir_len: data.len(),
                base: 0,
                data_pos: 0,
                byte_order: ByteOrder::Little,
            },
            &mut Ctx::new(members),
            sink,
        )
    }

    #[test]
    fn gate_b_blocks_before_reading_any_slot() {
        let mut sink = Sink::default();
        let mut members = HashMap::new();
        let result = walk(&TABLE, &[2, 1, 0, 2, 0], &mut sink, &mut members);
        assert_eq!(result.gate_b_blocked, 1);
        assert!(sink.rows.is_empty());
    }

    #[test]
    fn prior_raw_count_drives_following_read_before_rendering() {
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let mut members = HashMap::new();
        let result = walk(&TABLE, &[2, 1, 0, 2, 0], &mut sink, &mut members);
        assert_eq!(result.emitted, 2);
        assert_eq!(sink.rows[0].value, TagValue::Integer(2));
        assert_eq!(sink.rows[1].value, TagValue::String("1 2".to_owned()));
    }

    #[test]
    fn unknown_row_keeps_cursor_but_does_not_report_or_apply_foundtag_work() {
        let table = SerialTable {
            entries: &UNKNOWN_ENTRIES,
            ..TABLE
        };
        let table: &'static SerialTable = Box::leak(Box::new(table));
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let mut members = HashMap::new();
        let result = walk(table, &[2, 8, 0, 9, 0], &mut sink, &mut members);
        assert_eq!(result.emitted, 1);
        assert_eq!(sink.rows[0].name, "Values");
        assert_eq!(sink.rows[0].value, TagValue::String("8 9".to_owned()));
        assert!(!members.contains_key("UnknownSeen"));
    }

    #[test]
    fn raw_state_is_saved_before_later_condition_selects() {
        let table = SerialTable {
            entries: &STATE_ENTRIES,
            ..TABLE
        };
        let table: &'static SerialTable = Box::leak(Box::new(table));
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let mut members = HashMap::new();
        let result = walk(table, &[7, 3], &mut sink, &mut members);
        assert_eq!(result.emitted, 2);
        assert_eq!(sink.rows[1].name, "AfterState");
    }

    #[test]
    fn no_matching_alternative_stops_before_later_slots() {
        let table = SerialTable {
            entries: &STOP_ENTRIES,
            ..TABLE
        };
        let table: &'static SerialTable = Box::leak(Box::new(table));
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let mut members = HashMap::new();
        let result = walk(table, &[7, 3], &mut sink, &mut members);
        assert_eq!(result.no_matching_alternative, 1);
        assert!(sink.rows.is_empty());
    }

    #[test]
    fn gate_a_blocks_the_entire_directory() {
        let table = SerialTable {
            gate_a: BLOCKED_GATE,
            ..TABLE
        };
        let table: &'static SerialTable = Box::leak(Box::new(table));
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let mut members = HashMap::new();
        let result = walk(table, &[2, 1, 0, 2, 0], &mut sink, &mut members);
        assert_eq!(result.gate_a_blocked, 1);
        assert!(sink.rows.is_empty());
    }

    #[test]
    fn nonnumeric_prior_raw_count_is_refused_without_coercion() {
        let table = SerialTable {
            entries: &NON_NUMERIC_ENTRIES,
            ..TABLE
        };
        let table: &'static SerialTable = Box::leak(Box::new(table));
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let mut members = HashMap::new();
        let result = walk(table, b"x\0\0", &mut sink, &mut members);
        assert_eq!(result.unavailable_prior_raw, 1);
        assert!(result.tainted);
        assert_eq!(sink.rows.len(), 1);
        assert_eq!(sink.rows[0].name, "NonNumericCount");
    }

    #[test]
    fn unavailable_gettaginfo_context_taints_before_reading() {
        let table = SerialTable {
            entries: &CONTEXT_ENTRIES,
            ..TABLE
        };
        let table: &'static SerialTable = Box::leak(Box::new(table));
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let mut members = HashMap::new();
        let result = walk(table, &[1], &mut sink, &mut members);
        assert_eq!(result.unavailable_condition_context, 1);
        assert!(result.tainted);
        assert!(sink.rows.is_empty());
    }

    #[test]
    fn dynamic_string_count_uses_prior_raw_slot_and_truncates_at_nul() {
        let table = SerialTable {
            entries: &COUNTED_STRING_ENTRIES,
            ..TABLE
        };
        let table: &'static SerialTable = Box::leak(Box::new(table));
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let mut members = HashMap::new();
        let result = walk(table, b"\x04ok\0x", &mut sink, &mut members);
        assert_eq!(result.emitted, 2);
        assert_eq!(sink.rows[1].name, "CountedString");
        assert_eq!(sink.rows[1].value, TagValue::String("ok".to_owned()));
    }

    #[test]
    fn omitted_hook_taints_before_reading_or_storing_any_value() {
        let table = SerialTable {
            entries: &HOOK_ENTRIES,
            ..TABLE
        };
        let table: &'static SerialTable = Box::leak(Box::new(table));
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let mut members = HashMap::new();
        let result = walk(table, &[7], &mut sink, &mut members);
        assert_eq!(result.omitted, 1);
        assert!(result.tainted);
        assert!(sink.rows.is_empty());
        assert!(members.is_empty());
    }

    #[test]
    fn signed_format_is_refused_before_it_can_supply_a_dynamic_count() {
        let table = SerialTable {
            entries: &UNSUPPORTED_FORMAT_ENTRIES,
            ..TABLE
        };
        let table: &'static SerialTable = Box::leak(Box::new(table));
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let mut members = HashMap::new();
        let result = walk(table, &[0, 0, 0, 0], &mut sink, &mut members);
        assert_eq!(result.malformed_layout, 1);
        assert!(result.tainted);
        assert!(sink.rows.is_empty());
    }

    #[test]
    fn zero_length_bare_string_is_not_reported() {
        let table = SerialTable {
            entries: &REMAINDER_ENTRIES,
            ..TABLE
        };
        let table: &'static SerialTable = Box::leak(Box::new(table));
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let mut members = HashMap::new();
        let result = walk(table, b"", &mut sink, &mut members);
        assert_eq!(result.entries_seen, 1);
        assert_eq!(result.emitted, 0);
        assert!(sink.rows.is_empty());
    }

    #[test]
    fn bare_string_consumes_the_remaining_bytes_and_truncates_at_nul() {
        let table = SerialTable {
            entries: &REMAINDER_ENTRIES,
            ..TABLE
        };
        let table: &'static SerialTable = Box::leak(Box::new(table));
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let mut members = HashMap::new();
        let result = walk(table, b"raw\0ignored", &mut sink, &mut members);
        assert_eq!(result.emitted, 1);
        assert_eq!(sink.rows[0].value, TagValue::String("raw".to_owned()));
    }

    #[test]
    fn signed_multiword_decode_bits_masks_each_native_word() {
        let table = SerialTable {
            entries: &SIGNED_BITS_ENTRIES,
            ..TABLE
        };
        let table: &'static SerialTable = Box::leak(Box::new(table));
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let mut members = HashMap::new();
        // Little-endian 1, -32768, 3. Native DecodeBits sees a space-joined
        // signed scalar list, then masks every 16-bit word before indexing.
        let result = walk(table, &[1, 0, 0, 0x80, 3, 0], &mut sink, &mut members);
        assert_eq!(result.emitted, 1);
        assert_eq!(sink.rows[0].value, TagValue::String("0,31,32,33".to_owned()));
        assert_eq!(
            decode_bits_words(&DecodedValue::Integer(-1), 16),
            Some("0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15".to_owned())
        );
    }

    #[test]
    fn trailing_add_runs_after_closed_prior_raw_division() {
        let tag = SerialTag {
            format: SerialFormat {
                format: super::super::Fmt::Int16s,
                count: SerialCount::FloorDivPriorRaw {
                    serial_index: 0,
                    add: 15,
                    divisor: 16,
                    trailing_add: 1,
                },
            },
            ..SIGNED_BITS
        };
        let mut prior = HashMap::new();
        prior.insert(0, DecodedValue::Integer(17));
        assert_eq!(count_for(&tag, &prior, 0), Some(3));
        prior.insert(0, DecodedValue::Integer(-1));
        assert_eq!(count_for(&tag, &prior, 0), None);
    }

    #[test]
    fn serial_missing_member_policy_is_string_local_and_short_circuits() {
        static MISSING_TRUE: Cond = Cond::MemberTruthy {
            member: "AFInfoCount",
            negate: true,
        };
        static WOULD_MUTATE: Cond = Cond::SetMember {
            member: "Unexpected",
            source: EffectSource::Const(1),
            then: None,
        };
        let short_circuit = SerialCondition {
            cond: Cond::Or(&MISSING_TRUE, &WOULD_MUTATE),
            missing_member: SerialMissingMember::EmptyStringForStringOps,
        };
        let missing_regex = SerialCondition {
            cond: Cond::MemberRegex {
                member: "Model",
                pattern: "EOS",
                ignore_case: false,
                negate: true,
            },
            missing_member: SerialMissingMember::EmptyStringForStringOps,
        };
        let defined = SerialCondition {
            cond: Cond::MemberDefined {
                member: "Model",
                negate: false,
            },
            missing_member: SerialMissingMember::EmptyStringForStringOps,
        };
        let mut members = HashMap::new();
        let mut ctx = Ctx::new(&mut members);
        assert!(short_circuit.eval(&mut ctx));
        assert!(!ctx.members.contains_key("Unexpected"));
        assert!(missing_regex.eval(&mut ctx));
        assert!(!defined.eval(&mut ctx));
    }
}
