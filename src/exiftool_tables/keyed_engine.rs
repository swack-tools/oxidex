//! Reader for source-described keyed directories.
//!
//! CanonRaw ProcessCanonRaw reads CIFF10 entries, not a fixed-offset
//! ProcessBinaryData record. This module owns only CIFF10 layout, then hands
//! raw values to the shared decoder, conversion and rendering path. It has no
//! parser caller or enabled production table.

use crate::core::TagValue;
use crate::io::ByteOrder;

use super::cond::{Ctx, MemberValue};
use super::engine::{self, Emitted};
use super::pipeline::{
    self, Conversions, Groups, OnUnmodeledRawConv, OnUnrepresentableMember, Outcome, PipelineInput,
    Policy, PrintStage, Provenance, Reporting, StableFieldIdentity,
};
use super::runtime;
use super::{
    Cond, Fmt, KeyedDirectoryTable, KeyedEdge, KeyedLayout, KeyedTag, WordDirectory,
    find_keyed_table, find_table,
};

/// Carrier projection applied after generated tag/table group resolution.
///
/// CIFF APP0 sets ExifTool's `SET_GROUP1`; it does not replace the source
/// table's family-0 or family-2 groups. CRW has no carrier override.
#[derive(Clone, Copy, Debug)]
pub struct KeyedScope {
    pub group1_override: Option<&'static str>,
}

/// A caller-validated CIFF value block.
///
/// The caller establishes II/MM. The scalar limit makes requested/big-value
/// behavior explicit: without request policy, large scalars are withheld.
#[derive(Clone, Copy, Debug)]
pub struct KeyedBlock<'a> {
    pub data: &'a [u8],
    pub byte_order: ByteOrder,
    pub scope: KeyedScope,
    pub max_scalar_bytes: usize,
}

impl<'a> KeyedBlock<'a> {
    #[must_use]
    pub const fn new(data: &'a [u8], byte_order: ByteOrder, scope: KeyedScope) -> Self {
        Self {
            data,
            byte_order,
            scope,
            max_scalar_bytes: 512,
        }
    }
}

/// A container projection target for keyed output.
///
/// Gate B is supplied by the future caller. The default is disabled, so adding
/// this reader cannot activate a keyed table or named child.
pub trait KeyedEmissionSink {
    fn emit(&mut self, row: Emitted);

    /// Surface a source-declared reader warning. The default keeps existing
    /// inactive callers unchanged until a carrier chooses where warnings go.
    fn warn(&mut self, _warning: &'static str) {}

    /// Whether this caller requested native-style directory diagnostics.
    ///
    /// The default is deliberately quiet: a generated keyed layout cannot
    /// enable verbose output until a carrier explicitly asks for it.
    fn verbose_enabled(&self) -> bool {
        false
    }

    /// Mirror ExifTool's `VerboseDir` callback for an invoked word directory.
    /// `entry_count` is the source expression `size / 2 - 1`, so an odd-size
    /// directory retains its native fractional diagnostic value.
    fn verbose_directory(&mut self, _directory: &'static str, _entry_count: f64) {}

    fn keyed_enabled(&self, _table: &'static KeyedDirectoryTable) -> bool {
        false
    }
}

/// Counts withheld keyed-reader work. None of these conditions produces output.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyedWalkResult {
    pub entries_seen: usize,
    pub emitted: usize,
    pub gate_a_blocked: usize,
    pub gate_b_blocked: usize,
    pub malformed_directory: usize,
    pub high_bit_error: usize,
    pub duplicate_directory: usize,
    pub initial_context_refusal: usize,
    pub omitted: usize,
    pub bad_value: usize,
    pub large_scalar: usize,
    pub unwalked_edge: usize,
    pub unavailable_target: usize,
    /// A source-authenticated child Validate returned false. Native skips only
    /// that child and continues the enclosing keyed directory.
    pub validation_rejected: usize,
    /// Each native `HandleTag` invocation made by the length-prefixed u16
    /// layout, before unknown/omitted reporting policy may suppress output.
    /// `Index` is not a metadata value, but preserving it here proves the
    /// source handler contract without adding a public output field.
    pub word_entries: Vec<WordDirectoryEntry>,
    /// One record for each source-described word processor actually invoked.
    /// Gate A/Gate B skips produce no trace, which distinguishes them from a
    /// native processor that returned false after warning.
    pub word_traces: Vec<WordDirectoryTrace>,
}

/// The parameter record supplied to native `HandleTag` for a source-described
/// u16 word pair. This is a reader trace, not emitted metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WordDirectoryEntry {
    pub raw_id: u16,
    pub value: i64,
    pub index: usize,
    pub format: Fmt,
    pub count: usize,
    pub size: usize,
}

/// One source-described word processor invocation, before metadata rows are
/// selected, converted, or suppressed. This mirrors the independently
/// captured native callback boundary: return status, warnings, and raw
/// `HandleTag` operands are observable separately from final output.
#[derive(Clone, Debug, PartialEq)]
pub struct WordDirectoryTrace {
    pub module: &'static str,
    pub table: &'static str,
    pub returned: bool,
    pub warnings: Vec<&'static str>,
    /// Native `VerboseDir` calls made for this processor invocation.
    pub verbose_directories: Vec<WordDirectoryVerbose>,
    pub entries: Vec<WordDirectoryEntry>,
}

/// One native-style `VerboseDir` callback from a word-directory processor.
///
/// `entry_count` keeps the exact source arithmetic (`size / 2 - 1`) rather
/// than rounding an odd-sized source buffer to a number of decoded words.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WordDirectoryVerbose {
    pub directory: &'static str,
    pub entry_count: f64,
}

impl KeyedWalkResult {
    fn merge(&mut self, child: Self) {
        self.entries_seen += child.entries_seen;
        self.emitted += child.emitted;
        self.gate_a_blocked += child.gate_a_blocked;
        self.gate_b_blocked += child.gate_b_blocked;
        self.malformed_directory += child.malformed_directory;
        self.high_bit_error += child.high_bit_error;
        self.duplicate_directory += child.duplicate_directory;
        self.initial_context_refusal += child.initial_context_refusal;
        self.omitted += child.omitted;
        self.bad_value += child.bad_value;
        self.large_scalar += child.large_scalar;
        self.unwalked_edge += child.unwalked_edge;
        self.unavailable_target += child.unavailable_target;
        self.validation_rejected += child.validation_rejected;
        self.word_entries.extend(child.word_entries);
        self.word_traces.extend(child.word_traces);
    }
}

/// Read one source-described keyed directory.
///
/// The caller supplies the bounded carrier block and Gate B through `sink`.
/// This dispatches only a generated layout; it never identifies a Canon table
/// by tag ID or enables a production parser route.
#[must_use]
pub fn process_keyed_directory(
    table: &'static KeyedDirectoryTable,
    block: KeyedBlock<'_>,
    ctx: &mut Ctx,
    sink: &mut dyn KeyedEmissionSink,
) -> KeyedWalkResult {
    match table.layout {
        KeyedLayout::Ciff10 => process_ciff10_directory(table, block, ctx, sink),
        KeyedLayout::LengthPrefixedU16Pairs(word) => {
            process_word_directory(table, word, block, ctx, sink)
        }
    }
}

/// Read one CIFF10 directory and same-table CIFF subdirectories.
///
/// A named zero-start value block is dispatched only when it names a generated
/// binary or keyed table that independently passes Gate A and the caller's
/// Gate B policy. All other target shapes remain explicit refusals.
fn process_ciff10_directory(
    table: &'static KeyedDirectoryTable,
    block: KeyedBlock<'_>,
    ctx: &mut Ctx,
    sink: &mut dyn KeyedEmissionSink,
) -> KeyedWalkResult {
    let mut result = KeyedWalkResult::default();
    if !table.gate_a.passes() {
        result.gate_a_blocked = 1;
        return result;
    }
    if !sink.keyed_enabled(table) {
        result.gate_b_blocked = 1;
        return result;
    }
    let mut visited = Vec::new();
    let mut work = vec![KeyedWork::Enter(block)];
    while let Some(item) = work.pop() {
        match item {
            KeyedWork::RestoreDir(old) => {
                if let Some(old) = old {
                    ctx.members.insert("DIR_NAME", old);
                } else {
                    ctx.members.remove("DIR_NAME");
                }
            }
            KeyedWork::Enter(block) => {
                let Some(entries) = enter_directory(table, block, &mut visited, &mut result) else {
                    continue;
                };
                work.push(entries);
            }
            KeyedWork::Entries {
                block,
                directory_offset,
                count,
                index,
            } => {
                if index == count {
                    continue;
                }
                let entry = directory_offset + 2 + index * 10;
                let action = process_entry(table, block, entry, ctx, sink, &mut result);
                match action {
                    KeyedEntryAction::Tainted => {
                        // A child may have reached an unmodeled state-changing
                        // operation. Its members survive ProcessDirectory, so
                        // no pending parent frame may evaluate another
                        // condition against fabricated state.
                        // Unwind every pending directory scope before
                        // returning. Retaining only RestoreDir frames keeps
                        // ProcessDirectory's DIR_NAME save/restore contract
                        // while preventing any pending sibling evaluation.
                        work.retain(|pending| matches!(pending, KeyedWork::RestoreDir(_)));
                    }
                    KeyedEntryAction::StopDirectory => {}
                    KeyedEntryAction::Continue => work.push(KeyedWork::Entries {
                        block,
                        directory_offset,
                        count,
                        index: index + 1,
                    }),
                    KeyedEntryAction::Descend { block: child, name } => {
                        // Preserve native depth-first order and scope the
                        // child DIR_NAME without consuming the call stack.
                        work.push(KeyedWork::Entries {
                            block,
                            directory_offset,
                            count,
                            index: index + 1,
                        });
                        let old = ctx.members.insert("DIR_NAME", MemberValue::Str(name));
                        work.push(KeyedWork::RestoreDir(old));
                        work.push(KeyedWork::Enter(child));
                    }
                }
            }
        }
    }
    result
}

/// Read the complete, source-authenticated `ProcessCanonCustom` shape.
///
/// The header exception is deliberately after the exact equality fast path:
/// native code does not inspect `Model` when the ordinary length matches.
/// `Get16u`'s undef numeric coercion is represented by `u16_or_zero`, so an
/// odd final pair remains a native zero-valued `HandleTag` attempt.
fn process_word_directory(
    table: &'static KeyedDirectoryTable,
    word: WordDirectory,
    block: KeyedBlock<'_>,
    ctx: &mut Ctx,
    sink: &mut dyn KeyedEmissionSink,
) -> KeyedWalkResult {
    let mut result = KeyedWalkResult::default();
    if !table.gate_a.passes() {
        result.gate_a_blocked = 1;
        return result;
    }
    if !sink.keyed_enabled(table) {
        result.gate_b_blocked = 1;
        return result;
    }
    if !word.exact_length_first || !word.missing_model_as_empty || !word.short_u16_as_zero {
        result.unwalked_edge = 1;
        return result;
    }

    let mut trace = WordDirectoryTrace {
        module: table.module,
        table: table.table,
        returned: false,
        warnings: Vec::new(),
        verbose_directories: Vec::new(),
        entries: Vec::new(),
    };

    let header = u16_or_zero(block.data, 0, block.byte_order);
    let directory_size = block.data.len();
    if usize::from(header) != directory_size {
        let model_matches = eval_missing_model_as_empty(word.model_condition, ctx);
        let adjusted = usize::from(header).checked_add(word.header_adjustment);
        if !model_matches || adjusted != Some(directory_size) {
            result.bad_value += 1;
            sink.warn(word.invalid_warning);
            trace.warnings.push(word.invalid_warning);
            result.word_traces.push(trace);
            return result;
        }
    }

    if word.pair_stride == 0 || word.index_divisor == 0 || word.key_shift >= u16::BITS as usize {
        result.bad_value += 1;
        result.word_traces.push(trace);
        return result;
    }
    if sink.verbose_enabled() {
        let entry_count = directory_size as f64 / 2.0 - 1.0;
        sink.verbose_directory(word.verbose_directory, entry_count);
        trace.verbose_directories.push(WordDirectoryVerbose {
            directory: word.verbose_directory,
            entry_count,
        });
    }
    let mut position = word.pair_start;
    while position < directory_size {
        let packed = u16_or_zero(block.data, position, block.byte_order);
        let raw_id = packed >> word.key_shift;
        let Some(mask) = u16::try_from(word.value_mask).ok() else {
            result.bad_value += 1;
            result.word_traces.push(trace);
            return result;
        };
        let value = packed & mask;
        let Some(index) = position
            .checked_div(word.index_divisor)
            .and_then(|value| value.checked_sub(word.index_bias))
        else {
            result.bad_value += 1;
            result.word_traces.push(trace);
            return result;
        };
        let entry = WordDirectoryEntry {
            raw_id,
            value: i64::from(value),
            index,
            format: word.value_format,
            count: word.value_count,
            size: word.value_size,
        };
        result.word_entries.push(entry);
        trace.entries.push(entry);
        result.entries_seen += 1;

        let resolved = with_word_selection_context(word, ctx, |ctx| {
            resolve_tag(table, raw_id, ctx, &mut result, true)
        });
        if let Some(resolved) = resolved {
            if resolved.tag.edge.is_some() {
                // The authenticated population has no word-table child
                // subdirectories. Do not infer an edge from a tag name.
                result.unwalked_edge += 1;
                result.word_traces.push(trace);
                return result;
            }
            // HandleTag receives the processor's numeric `$val` directly.
            // Format/Count/Size guide tag selection but do not re-read or
            // truncate this masked u16 value.
            let raw = runtime::DecodedValue::Integer(i64::from(value));
            let stored = runtime::to_stored_tag_value(&raw, block.byte_order);
            if matches!(
                emit_resolved_scalar(
                    table,
                    block.scope,
                    resolved,
                    raw,
                    stored,
                    ctx,
                    sink,
                    &mut result,
                ),
                ScalarAction::Tainted
            ) {
                result.word_traces.push(trace);
                return result;
            }
        }

        let Some(next) = position.checked_add(word.pair_stride) else {
            result.bad_value += 1;
            result.word_traces.push(trace);
            return result;
        };
        position = next;
    }
    trace.returned = true;
    result.word_traces.push(trace);
    result
}

/// `Cond::MemberRegex` normally treats an absent member as a failed match.
/// This native processor instead applies Perl's regex operator to undef, whose
/// string subject is empty. Keep the temporary binding local to the header
/// exception and restore the caller state before processing any pair.
fn eval_missing_model_as_empty(condition: Cond, ctx: &mut Ctx) -> bool {
    if ctx.members.contains_key("Model") {
        return condition.eval(ctx);
    }
    ctx.members.insert("Model", MemberValue::Str(String::new()));
    let matched = condition.eval(ctx);
    ctx.members.remove("Model");
    matched
}

/// `HandleTag` supplies Format and Count while choosing its tag information.
/// The word compiler currently accepts only `int8u`/1/1, but the reader uses
/// the descriptor rather than hard-coding that shape so a reviewed expansion
/// cannot silently lose the selection inputs.
fn with_word_selection_context<T>(
    word: WordDirectory,
    ctx: &mut Ctx,
    f: impl FnOnce(&mut Ctx) -> T,
) -> T {
    let prior_format = ctx.format;
    let prior_count = ctx.count;
    ctx.format = Some(word_format_name(word.value_format));
    ctx.count = i64::try_from(word.value_count).ok();
    let value = f(ctx);
    ctx.format = prior_format;
    ctx.count = prior_count;
    value
}

fn word_format_name(format: Fmt) -> &'static str {
    match format {
        Fmt::Int8u => "int8u",
        Fmt::Int8s => "int8s",
        Fmt::Int16u => "int16u",
        Fmt::Int16s => "int16s",
        Fmt::Int16uRev => "int16uRev",
        Fmt::Int32u => "int32u",
        Fmt::Int32s => "int32s",
        Fmt::Int32uRev => "int32uRev",
        Fmt::Int64u => "int64u",
        Fmt::Int64s => "int64s",
        Fmt::Float => "float",
        Fmt::Double => "double",
        Fmt::Rational32u => "rational32u",
        Fmt::Rational32s => "rational32s",
        Fmt::Rational64u => "rational64u",
        Fmt::Rational64s => "rational64s",
        Fmt::Fixed16s => "fixed16s",
        Fmt::Fixed16u => "fixed16u",
        Fmt::Fixed32s => "fixed32s",
        Fmt::Fixed32u => "fixed32u",
        Fmt::Extended => "extended",
        Fmt::PString => "pstring",
        Fmt::Str(_) | Fmt::RemainderString => "string",
        Fmt::Undef(_) => "undef",
        Fmt::Var(_) => "var",
    }
}

enum KeyedWork<'a> {
    Enter(KeyedBlock<'a>),
    Entries {
        block: KeyedBlock<'a>,
        directory_offset: usize,
        count: usize,
        index: usize,
    },
    RestoreDir(Option<MemberValue>),
}

enum KeyedEntryAction<'a> {
    Continue,
    StopDirectory,
    Tainted,
    Descend { block: KeyedBlock<'a>, name: String },
}

fn enter_directory<'a>(
    table: &'static KeyedDirectoryTable,
    block: KeyedBlock<'a>,
    visited: &mut Vec<(usize, usize)>,
    result: &mut KeyedWalkResult,
) -> Option<KeyedWork<'a>> {
    if !matches!(table.layout, KeyedLayout::Ciff10) {
        result.malformed_directory += 1;
        return None;
    }
    let Some(directory_offset) = u32_at(
        block.data,
        block.data.len().saturating_sub(4),
        block.byte_order,
    )
    .and_then(|offset| usize::try_from(offset).ok()) else {
        result.malformed_directory += 1;
        return None;
    };
    let Some(address) = (block.data.as_ptr() as usize).checked_add(directory_offset) else {
        result.malformed_directory += 1;
        return None;
    };
    let table_id = table as *const KeyedDirectoryTable as usize;
    if visited.contains(&(table_id, address)) {
        result.duplicate_directory += 1;
        return None;
    }
    let Some(count) = u16_at(block.data, directory_offset, block.byte_order).map(usize::from)
    else {
        result.malformed_directory += 1;
        return None;
    };
    let Some(entries_end) = directory_offset.checked_add(2).and_then(|start| {
        count
            .checked_mul(10)
            .and_then(|bytes| start.checked_add(bytes))
    }) else {
        result.malformed_directory += 1;
        return None;
    };
    if entries_end > block.data.len() {
        result.malformed_directory += 1;
        return None;
    }
    visited.push((table_id, address));
    Some(KeyedWork::Entries {
        block,
        directory_offset,
        count,
        index: 0,
    })
}

fn process_entry<'a>(
    table: &'static KeyedDirectoryTable,
    block: KeyedBlock<'a>,
    entry: usize,
    ctx: &mut Ctx,
    sink: &mut dyn KeyedEmissionSink,
    result: &mut KeyedWalkResult,
) -> KeyedEntryAction<'a> {
    let raw_tag = u16_at(block.data, entry, block.byte_order).expect("checked entry bounds");
    if raw_tag & 0x8000 != 0 {
        result.high_bit_error += 1;
        return KeyedEntryAction::StopDirectory;
    }
    result.entries_seen += 1;
    let raw_id = raw_tag & 0x3fff;
    let entry_type = (raw_tag >> 8) & 0x38;
    let inline = raw_tag & 0x4000 != 0;
    let payload = &block.data[entry + 2..entry + 10];
    let (value_offset, value_size, declared_size) = if inline {
        // ProcessCanonRaw overwrites `$size` with the eight inline bytes.
        (entry + 2, 8usize, 8u32)
    } else {
        let Some(declared_size) = u32_at(payload, 0, block.byte_order) else {
            result.bad_value += 1;
            return KeyedEntryAction::Continue;
        };
        let Some(size) = usize::try_from(declared_size).ok() else {
            result.bad_value += 1;
            return KeyedEntryAction::Continue;
        };
        let Some(offset) =
            u32_at(payload, 4, block.byte_order).and_then(|value| usize::try_from(value).ok())
        else {
            result.bad_value += 1;
            return KeyedEntryAction::Continue;
        };
        (offset, size, declared_size)
    };

    let resolved = resolve_tag(table, raw_id, ctx, result, false);
    // CIFF type controls same-parent recursion, including unknown keys.
    if !inline && matches!(entry_type, 0x28 | 0x30) {
        let Some(child) = bounded_block(block, value_offset, value_size) else {
            result.bad_value += 1;
            return KeyedEntryAction::Continue;
        };
        // `GetTagInfo` suppresses an Unknown row before ProcessCanonRaw
        // chooses a directory name. It still walks the structural child, but
        // under the native fallback name rather than the suppressed row's
        // Name.
        let name = resolved
            .filter(|selected| !selected.tag.flags.unknown)
            .map_or_else(
                || format!("CanonRaw_0x{raw_id:04x}"),
                |selected| selected.tag.name.to_owned(),
            );
        return KeyedEntryAction::Descend { block: child, name };
    }

    let Some(resolved) = resolved else {
        return KeyedEntryAction::Continue;
    };
    let tag = resolved.tag;
    // ExifTool.pm:9180-9186: selecting an Unknown alternative suppresses it
    // unless `-u` is requested. Selection is terminal: the next alternative
    // must not be tried after this return.
    if tag.flags.unknown {
        return KeyedEntryAction::Continue;
    }
    // A refused Condition or Hook can change whether this row exists or how
    // its bytes are located. The other omitted flags are handled after
    // RawConv: FoundTag stores modeled member state before reportability.
    if (tag.omitted.condition && !resolved.condition_resolved) || tag.omitted.hook {
        result.omitted += 1;
        return KeyedEntryAction::Continue;
    }
    if let Some(edge) = tag.edge {
        match edge {
            KeyedEdge::SameTableDirectory => result.unwalked_edge += 1,
            KeyedEdge::BoundedValue {
                module,
                table: target,
                start,
                validation,
                unwalked,
            } => {
                if !matches!(start, super::KeyedStart::Zero) || !unwalked.is_empty() {
                    result.unwalked_edge += 1;
                    return KeyedEntryAction::Tainted;
                }
                let Some(child) = bounded_block(block, value_offset, value_size) else {
                    result.bad_value += 1;
                    return KeyedEntryAction::Tainted;
                };
                // CanonRaw evaluates Start first, then calls Validate against
                // the complete `$value` buffer and the declared CIFF size.
                // Do not slice at Start before this check: Get16u receives
                // `$dirData,$subdirStart+offset`, not a post-Start view.
                // A false Validate warns and skips only this child; it never
                // opens ProcessDirectory or changes parent state.
                if validation.is_some_and(|check| {
                    !check.matches(child.data, 0, declared_size, child.byte_order)
                }) {
                    result.validation_rejected += 1;
                    return KeyedEntryAction::Continue;
                }
                if let Some(target_table) = find_table(module, target) {
                    if !target_table.gate_a.passes() {
                        result.gate_a_blocked += 1;
                        return KeyedEntryAction::Tainted;
                    }
                    if !super::is_enabled(target_table) {
                        result.gate_b_blocked += 1;
                        return KeyedEntryAction::Tainted;
                    }
                    let mut rows = Vec::new();
                    let outcome = with_dir_name(ctx, tag.name.to_owned(), |ctx| {
                        engine::process_binary_data_checked(
                            target_table,
                            engine::Dir::whole(child.data, child.byte_order),
                            ctx,
                            &mut rows,
                        )
                    });
                    if outcome == engine::BinaryWalkOutcome::Tainted {
                        return KeyedEntryAction::Tainted;
                    }
                    for row in rows {
                        sink.emit(re_scope(row, child.scope));
                        result.emitted += 1;
                    }
                } else if let Some(target_table) = find_keyed_table(module, target) {
                    // A named keyed child remains just as opt-in as its parent:
                    // generated presence never substitutes for carrier evidence.
                    if !target_table.gate_a.passes() {
                        result.gate_a_blocked += 1;
                        return KeyedEntryAction::Tainted;
                    }
                    if !sink.keyed_enabled(target_table) {
                        result.gate_b_blocked += 1;
                        return KeyedEntryAction::Tainted;
                    }
                    let child_result = with_dir_name(ctx, tag.name.to_owned(), |ctx| {
                        process_keyed_directory(target_table, child, ctx, sink)
                    });
                    result.merge(child_result);
                } else {
                    result.unavailable_target += 1;
                    return KeyedEntryAction::Tainted;
                }
            }
        }
        return KeyedEntryAction::Continue;
    }

    if value_size > block.max_scalar_bytes {
        result.large_scalar += 1;
        return KeyedEntryAction::Continue;
    }
    let Some(value_end) = value_offset.checked_add(value_size) else {
        result.bad_value += 1;
        return KeyedEntryAction::Continue;
    };
    let Some(value) = block.data.get(value_offset..value_end) else {
        result.bad_value += 1;
        return KeyedEntryAction::Continue;
    };
    let format = tag.format.unwrap_or_else(|| default_format(entry_type));
    let count = native_count(tag, format, value_size, inline);
    let Some(read) = engine::read_value_with_stored(
        value,
        0,
        format,
        count,
        i64::try_from(value_size).unwrap_or(i64::MAX),
        block.byte_order,
    ) else {
        result.bad_value += 1;
        return KeyedEntryAction::Continue;
    };
    match emit_resolved_scalar(
        table,
        block.scope,
        resolved,
        read.decoded,
        read.stored,
        ctx,
        sink,
        result,
    ) {
        ScalarAction::Continue => KeyedEntryAction::Continue,
        ScalarAction::Tainted => KeyedEntryAction::Tainted,
    }
}

enum ScalarAction {
    Continue,
    Tainted,
}

/// The keyed readers' explicit conversion policy for the shared stage.
/// Unrepresentable state and an unmodeled `RawConv` both taint: pending
/// parent frames may evaluate Conditions against the shared member state.
const KEYED_POLICY: Policy = Policy {
    member_value: engine::member_value,
    on_unrepresentable_member: OnUnrepresentableMember::Taint,
    set_member_clears_omission: true,
    on_unmodeled_raw_conv: OnUnmodeledRawConv::Taint,
    perl_length: pipeline::scalar_perl_length,
    scalar_form: runtime::to_exiftool_value,
};

/// Apply the shared keyed tag-reporting semantics after a layout has supplied
/// one logical scalar. CIFF and source-authenticated word directories differ
/// only in obtaining that scalar; selection stays here and state, conversion
/// and the emitted shape are the shared `FoundTag` stage.
#[allow(clippy::too_many_arguments)]
fn emit_resolved_scalar(
    table: &'static KeyedDirectoryTable,
    scope: KeyedScope,
    resolved: ResolvedTag,
    raw: runtime::DecodedValue,
    stored: TagValue,
    ctx: &mut Ctx,
    sink: &mut dyn KeyedEmissionSink,
    result: &mut KeyedWalkResult,
) -> ScalarAction {
    let tag = resolved.tag;
    if tag.flags.unknown {
        return ScalarAction::Continue;
    }
    let input = PipelineInput {
        identity: StableFieldIdentity::KeyedRawId(tag.raw_id),
        provenance: Provenance {
            module: table.module,
            table: table.table,
        },
        groups: Groups {
            g0: tag.groups.g0.unwrap_or(table.group0),
            g1: Some(
                scope
                    .group1_override
                    .unwrap_or(tag.groups.g1.unwrap_or(table.group1)),
            ),
            g2: tag.groups.g2.unwrap_or(table.group2),
        },
        reporting: Reporting {
            name: tag.name,
            // The keyed compiler has already folded table PRIORITY and AVOID
            // into the tag flags. Only ExifTool's final Avoid default remains.
            low_priority: pipeline::effective_priority(tag.flags.priority, None, tag.flags.avoid)
                == Some(0),
            avoid: tag.flags.avoid,
            is_list: tag.flags.list,
        },
        conversions: Conversions {
            omitted: tag.omitted,
            condition_resolved: resolved.condition_resolved,
            raw_conv: tag.raw_conv,
            value_conv: tag.value_conv,
            print_conv: PrintStage::Shared(tag.print_conv),
            binary: tag.flags.binary,
        },
        raw,
        stored,
        // `Emitted::rational` is for IFD tables only; a keyed directory never
        // keeps the raw fraction.
        rational: None,
    };
    match pipeline::execute(input, &KEYED_POLICY, ctx.members, None) {
        Outcome::Report(row) => {
            if !super::attribution::silenced(super::attribution::Token::Keyed) {
                sink.emit(row);
            }
            result.emitted += 1;
            ScalarAction::Continue
        }
        Outcome::Omitted | Outcome::Declined => {
            result.omitted += 1;
            ScalarAction::Continue
        }
        // An unrepresentable state value, or an unmodeled RawConv, could
        // change every following source Condition. Stop this directory
        // rather than leave stale state for its later siblings.
        Outcome::Tainted => {
            result.omitted += 1;
            ScalarAction::Tainted
        }
    }
}

#[derive(Clone, Copy)]
struct ResolvedTag {
    tag: &'static KeyedTag,
    condition_resolved: bool,
}

fn resolve_tag(
    table: &'static KeyedDirectoryTable,
    raw_id: u16,
    ctx: &mut Ctx,
    result: &mut KeyedWalkResult,
    allow_format_count_context: bool,
) -> Option<ResolvedTag> {
    if let Some(tag) = table.tags.iter().find(|tag| tag.raw_id == raw_id) {
        if let Some(condition) = tag.condition {
            if condition_needs_unavailable_context(condition, allow_format_count_context) {
                result.initial_context_refusal += 1;
                return None;
            }
            if !condition.eval(ctx) {
                return None;
            }
        }
        return Some(ResolvedTag {
            tag,
            condition_resolved: tag.condition.is_some(),
        });
    }
    let group = table.variants.iter().find(|group| group.raw_id == raw_id)?;
    if group.alternatives.iter().any(|(condition, _)| {
        condition_needs_unavailable_context(*condition, allow_format_count_context)
    }) {
        result.initial_context_refusal += 1;
        return None;
    }
    for (condition, tag) in group.alternatives {
        // The alternatives themselves ARE GetTagInfo's Condition array.
        // Re-evaluating the copied row condition would duplicate assignment
        // tails and diverge from the native first-match visit.
        if condition.eval(ctx) {
            return Some(ResolvedTag {
                tag,
                condition_resolved: true,
            });
        }
    }
    None
}

/// A CIFF lookup has no `$format`, `$count`, or `$$valPt` retry context. A
/// source-authenticated word directory supplies the first two through
/// `HandleTag`, but it still has no byte-slice value context. Keep that
/// distinction explicit so an accepted future `ValPt` condition cannot be
/// evaluated against invented data.
fn condition_needs_unavailable_context(condition: Cond, allow_format_count_context: bool) -> bool {
    match condition {
        Cond::ValPtRegex { .. } => true,
        Cond::FormatEq { .. } | Cond::FormatRegex { .. } | Cond::CountCmp { .. } => {
            !allow_format_count_context
        }
        Cond::And(left, right) | Cond::Or(left, right) => {
            condition_needs_unavailable_context(*left, allow_format_count_context)
                || condition_needs_unavailable_context(*right, allow_format_count_context)
        }
        Cond::SetMember { source, then, .. } => {
            (matches!(source, super::EffectSource::Count) && !allow_format_count_context)
                || then.is_some_and(|next| {
                    condition_needs_unavailable_context(*next, allow_format_count_context)
                })
        }
        _ => false,
    }
}

fn native_count(tag: &KeyedTag, format: Fmt, value_size: usize, inline: bool) -> usize {
    let mut count = tag.count;
    if inline && count.is_none() && !matches!(format, Fmt::Str(_) | Fmt::RemainderString) {
        count = Some(1);
    }
    match count {
        Some(count) if count != 0 => count,
        _ => {
            let width = usize::try_from(format.size()).unwrap_or(0);
            if width == 0 { 0 } else { value_size / width }
        }
    }
}

fn default_format(entry_type: u16) -> Fmt {
    match entry_type {
        0x00 => Fmt::Int8u,
        // CanonRaw's default `string` remains count-aware.  RemainderString
        // is only an explicit bare Format override in ProcessBinaryData.
        0x08 => Fmt::Str(1),
        0x10 => Fmt::Int16u,
        0x18 => Fmt::Int32u,
        _ => Fmt::Undef(0),
    }
}

fn bounded_block(parent: KeyedBlock<'_>, offset: usize, size: usize) -> Option<KeyedBlock<'_>> {
    let data = parent.data.get(offset..offset.checked_add(size)?)?;
    Some(KeyedBlock { data, ..parent })
}

fn with_dir_name<T>(ctx: &mut Ctx, value: String, f: impl FnOnce(&mut Ctx) -> T) -> T {
    let old = ctx.members.insert("DIR_NAME", MemberValue::Str(value));
    let result = f(ctx);
    if let Some(old) = old {
        ctx.members.insert("DIR_NAME", old);
    } else {
        ctx.members.remove("DIR_NAME");
    }
    result
}

fn re_scope(row: Emitted, scope: KeyedScope) -> Emitted {
    Emitted {
        group1: scope.group1_override.unwrap_or(row.group1),
        ..row
    }
}

fn u16_at(data: &[u8], offset: usize, order: ByteOrder) -> Option<u16> {
    let bytes: [u8; 2] = data.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(match order {
        ByteOrder::Big => u16::from_be_bytes(bytes),
        ByteOrder::Little => u16::from_le_bytes(bytes),
    })
}

/// `Get16u` returns undef for an incomplete word; the authenticated native
/// processor immediately uses that result in numeric operations, where Perl
/// coerces it to zero. This helper is intentionally layout-local.
fn u16_or_zero(data: &[u8], offset: usize, order: ByteOrder) -> u16 {
    u16_at(data, offset, order).unwrap_or(0)
}

fn u32_at(data: &[u8], offset: usize, order: ByteOrder) -> Option<u32> {
    let bytes: [u8; 4] = data.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(match order {
        ByteOrder::Big => u32::from_be_bytes(bytes),
        ByteOrder::Little => u32::from_le_bytes(bytes),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::Write;
    use std::process::{Command, Stdio};

    use super::*;
    use crate::exiftool_tables::{
        ALL_KEYED_TABLES, Cond, GateA, IfdFlags, KeyedNativeFacts, KeyedStart, KeyedVariantGroup,
        Omitted, PrintConv, RawConvEffect, SizeExpectation, TagGroups, U16SizeCheck,
    };

    const FACTS: KeyedNativeFacts = KeyedNativeFacts {
        format: None,
        count: None,
        condition: None,
        groups: TagGroups::NONE,
        subdir: None,
        flags: IfdFlags::NONE,
    };
    const EMPTY_TAGS: &[KeyedTag] = &[];
    const EMPTY_VARIANTS: &[super::super::KeyedVariantGroup] = &[];
    const EMPTY_TABLE: KeyedDirectoryTable = KeyedDirectoryTable {
        module: "Test",
        table: "Main",
        group0: "Test",
        group1: "Test",
        group2: "Other",
        layout: KeyedLayout::Ciff10,
        gate_a: GateA { blocked_by: &[] },
        tags: EMPTY_TAGS,
        variants: EMPTY_VARIANTS,
    };

    #[derive(Default)]
    struct Sink {
        rows: Vec<Emitted>,
        warnings: Vec<&'static str>,
        verbose: bool,
        verbose_directories: Vec<WordDirectoryVerbose>,
        enabled: bool,
    }

    #[derive(Default)]
    struct ParentOnlySink {
        rows: Vec<Emitted>,
        warnings: Vec<&'static str>,
    }

    impl KeyedEmissionSink for ParentOnlySink {
        fn emit(&mut self, row: Emitted) {
            self.rows.push(row);
        }

        fn warn(&mut self, warning: &'static str) {
            self.warnings.push(warning);
        }

        fn keyed_enabled(&self, table: &'static KeyedDirectoryTable) -> bool {
            table.module == "CanonRaw" && table.table == "Main"
        }
    }

    impl KeyedEmissionSink for Sink {
        fn emit(&mut self, row: Emitted) {
            self.rows.push(row);
        }

        fn warn(&mut self, warning: &'static str) {
            self.warnings.push(warning);
        }

        fn verbose_enabled(&self) -> bool {
            self.verbose
        }

        fn verbose_directory(&mut self, directory: &'static str, entry_count: f64) {
            self.verbose_directories.push(WordDirectoryVerbose {
                directory,
                entry_count,
            });
        }

        fn keyed_enabled(&self, _table: &'static KeyedDirectoryTable) -> bool {
            self.enabled
        }
    }

    const fn tag(
        raw_id: u16,
        name: &'static str,
        format: Option<Fmt>,
        count: Option<usize>,
    ) -> KeyedTag {
        KeyedTag {
            raw_id,
            name,
            format,
            count,
            flags: IfdFlags::NONE,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::None,
            groups: TagGroups::NONE,
            edge: None,
            native: FACTS,
        }
    }

    const fn table(tags: &'static [KeyedTag]) -> KeyedDirectoryTable {
        KeyedDirectoryTable {
            tags,
            ..EMPTY_TABLE
        }
    }

    fn scope() -> KeyedScope {
        KeyedScope {
            group1_override: Some("Carrier"),
        }
    }

    fn native_scope() -> KeyedScope {
        KeyedScope {
            group1_override: None,
        }
    }

    fn ciff(order: ByteOrder, entries: &[(u16, Vec<u8>)]) -> Vec<u8> {
        let mut values = Vec::new();
        let mut placed = Vec::new();
        for (raw, value) in entries {
            if raw & 0x4000 != 0 {
                placed.push((*raw, 0usize, 0usize, value.clone()));
            } else {
                let offset = values.len();
                values.extend_from_slice(value);
                placed.push((*raw, offset, value.len(), Vec::new()));
            }
        }
        let directory = values.len();
        let mut out = values;
        push_u16(&mut out, entries.len() as u16, order);
        for (raw, offset, size, inline) in placed {
            push_u16(&mut out, raw, order);
            if raw & 0x4000 != 0 {
                let mut inline = inline;
                inline.resize(8, 0);
                out.extend_from_slice(&inline);
            } else {
                push_u32(&mut out, size as u32, order);
                push_u32(&mut out, offset as u32, order);
            }
        }
        push_u32(&mut out, 0, order);
        push_u32(&mut out, directory as u32, order);
        out
    }

    fn push_u16(out: &mut Vec<u8>, value: u16, order: ByteOrder) {
        let bytes = match order {
            ByteOrder::Big => value.to_be_bytes(),
            ByteOrder::Little => value.to_le_bytes(),
        };
        out.extend_from_slice(&bytes);
    }

    fn push_u32(out: &mut Vec<u8>, value: u32, order: ByteOrder) {
        let bytes = match order {
            ByteOrder::Big => value.to_be_bytes(),
            ByteOrder::Little => value.to_le_bytes(),
        };
        out.extend_from_slice(&bytes);
    }

    fn walk_test(
        table: &'static KeyedDirectoryTable,
        data: &[u8],
        order: ByteOrder,
    ) -> (Sink, KeyedWalkResult) {
        let mut members = HashMap::new();
        let mut ctx = Ctx::new(&mut members);
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let result = process_keyed_directory(
            table,
            KeyedBlock::new(data, order, scope()),
            &mut ctx,
            &mut sink,
        );
        (sink, result)
    }

    const WORD_LAYOUT: WordDirectory = WordDirectory {
        pair_start: 2,
        pair_stride: 2,
        key_shift: 8,
        // This deliberately exceeds `int8u`: HandleTag is given the masked
        // processor value, not a byte decoded from the carrier.
        value_mask: 0x01ff,
        header_adjustment: 2,
        model_condition: Cond::MemberRegex {
            member: "Model",
            pattern: "^$",
            ignore_case: false,
            negate: false,
        },
        exact_length_first: true,
        missing_model_as_empty: true,
        short_u16_as_zero: true,
        index_divisor: 2,
        index_bias: 1,
        value_format: Fmt::Int8u,
        value_count: 1,
        value_size: 1,
        invalid_warning: "Invalid CanonCustom data",
        verbose_directory: "CanonCustom",
        source_file: "CanonCustom.pm",
        source_sha256: "fixture-source",
        source_body_sha256: "fixture-body",
        reader_contract_sha256: "fixture-contract",
    };

    const fn word_table(
        tags: &'static [KeyedTag],
        variants: &'static [KeyedVariantGroup],
    ) -> KeyedDirectoryTable {
        KeyedDirectoryTable {
            layout: KeyedLayout::LengthPrefixedU16Pairs(WORD_LAYOUT),
            tags,
            variants,
            ..EMPTY_TABLE
        }
    }

    fn words(order: ByteOrder, header: u16, pairs: &[u16], trailing: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        push_u16(&mut out, header, order);
        for pair in pairs {
            push_u16(&mut out, *pair, order);
        }
        out.extend_from_slice(trailing);
        out
    }

    fn hex_bytes(hex: &str) -> Vec<u8> {
        assert_eq!(hex.len() % 2, 0, "probe fixture hex must be byte-aligned");
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                std::str::from_utf8(pair)
                    .ok()
                    .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                    .expect("probe fixture hex must be valid")
            })
            .collect()
    }

    /// Invoke the independently maintained native v1 word-processor probe.
    /// This test is deliberately ignored by default because it needs the
    /// pinned ExifTool tree and a Perl whose module set can load it; the
    /// focused native-validation command supplies both variables explicitly.
    fn probe_word_processor(requests: &[serde_json::Value]) -> Vec<serde_json::Value> {
        let root = std::env::var("OXIDEX_PINNED_EXIFTOOL")
            .expect("ignored native probe needs OXIDEX_PINNED_EXIFTOOL");
        let perl =
            std::env::var("EXIFTOOL_PERL").expect("ignored native probe needs EXIFTOOL_PERL");
        let probe = format!(
            "{}/tools/exiftool-tables/probe_word_processor.pl",
            env!("CARGO_MANIFEST_DIR")
        );
        let mut child = Command::new(perl)
            .arg(probe)
            .arg("--lib")
            .arg("lib")
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start native word-processor probe");
        {
            let stdin = child.stdin.as_mut().expect("native probe stdin");
            for request in requests {
                writeln!(
                    stdin,
                    "{}",
                    serde_json::to_string(request).expect("serialize probe request")
                )
                .expect("write native probe request");
            }
        }
        let output = child
            .wait_with_output()
            .expect("wait for native word-processor probe");
        assert!(
            output.status.success(),
            "native probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "native probe wrote stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
            .stdout
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).expect("native probe JSONL response"))
            .collect()
    }

    fn native_number(value: &serde_json::Value) -> i64 {
        value["numeric"]
            .as_i64()
            .expect("native probe scalar numeric value")
    }

    #[test]
    fn word_directory_preserves_numeric_mask_and_handle_tag_selection_in_both_orders() {
        static FORMAT_IS_INT8U: Cond = Cond::FormatEq {
            value: "int8u",
            negate: false,
        };
        static COUNT_IS_ONE: Cond = Cond::CountCmp {
            op: crate::exiftool_tables::CmpOp::Eq,
            value: 1,
        };
        static FORMAT_AND_COUNT: Cond = Cond::And(&FORMAT_IS_INT8U, &COUNT_IS_ONE);
        static ROW: KeyedTag = tag(1, "CustomFunction", Some(Fmt::Int8u), Some(1));
        static ALTERNATIVES: [(Cond, KeyedTag); 1] = [(FORMAT_AND_COUNT, ROW)];
        static VARIANTS: [KeyedVariantGroup; 1] = [KeyedVariantGroup {
            raw_id: 1,
            alternatives: &ALTERNATIVES,
        }];
        static TABLE: KeyedDirectoryTable = word_table(&[], &VARIANTS);

        for order in [ByteOrder::Big, ByteOrder::Little] {
            // `0x01ff >> 8` selects key 1, and a source-authorized `0x1ff`
            // mask preserves numeric 511 despite the HandleTag Format being
            // `int8u` selection metadata.
            let data = words(order, 4, &[0x01ff], &[]);
            let (sink, result) = walk_test(&TABLE, &data, order);
            assert_eq!(result.entries_seen, 1);
            assert_eq!(
                result.word_entries,
                vec![WordDirectoryEntry {
                    raw_id: 1,
                    value: 511,
                    index: 0,
                    format: Fmt::Int8u,
                    count: 1,
                    size: 1,
                }]
            );
            assert_eq!(sink.rows.len(), 1);
            assert_eq!(sink.rows[0].name, "CustomFunction");
            assert_eq!(sink.rows[0].value, TagValue::Integer(511));
            assert_eq!(result.word_traces.len(), 1);
            assert!(result.word_traces[0].returned);
            assert!(result.word_traces[0].warnings.is_empty());
            assert!(result.word_traces[0].verbose_directories.is_empty());
            assert!(sink.verbose_directories.is_empty());
            assert_eq!(result.word_traces[0].entries, result.word_entries);
        }
    }

    #[test]
    fn word_directory_verbose_projection_is_opt_in_and_keeps_native_size_expression() {
        static TAGS: [KeyedTag; 1] = [tag(1, "CustomFunction", Some(Fmt::Int8u), Some(1))];
        static TABLE: KeyedDirectoryTable = word_table(&TAGS, &[]);
        // Native calls `VerboseDir('CanonCustom', $size / 2 - 1)`. An odd
        // byte count must stay 1.5 rather than being rounded to decoded pairs.
        let data = words(ByteOrder::Little, 3, &[0x0102], &[0x34]);
        let mut members = HashMap::new();
        let mut ctx = Ctx::new(&mut members);
        let mut verbose = Sink {
            enabled: true,
            verbose: true,
            ..Sink::default()
        };
        let result = process_keyed_directory(
            &TABLE,
            KeyedBlock::new(&data, ByteOrder::Little, scope()),
            &mut ctx,
            &mut verbose,
        );
        assert_eq!(
            result.word_traces[0].verbose_directories,
            vec![WordDirectoryVerbose {
                directory: "CanonCustom",
                entry_count: 1.5,
            }]
        );
        assert_eq!(
            verbose.verbose_directories,
            result.word_traces[0].verbose_directories
        );

        let mut quiet_members = HashMap::new();
        let mut quiet_ctx = Ctx::new(&mut quiet_members);
        let mut quiet = Sink {
            enabled: true,
            ..Sink::default()
        };
        let quiet_result = process_keyed_directory(
            &TABLE,
            KeyedBlock::new(&data, ByteOrder::Little, scope()),
            &mut quiet_ctx,
            &mut quiet,
        );
        assert!(quiet_result.word_traces[0].verbose_directories.is_empty());
        assert!(quiet.verbose_directories.is_empty());
    }

    #[test]
    fn word_trace_distinguishes_native_rejection_from_gated_or_empty_processing() {
        static TAGS: [KeyedTag; 1] = [tag(1, "CustomFunction", Some(Fmt::Int8u), Some(1))];
        static TABLE: KeyedDirectoryTable = word_table(&TAGS, &[]);

        // A source processor was invoked and rejected the header. Its warning
        // and false return are visible separately from generic counters.
        let rejected = words(ByteOrder::Big, 4, &[0x0102, 0x0304], &[]);
        let mut rejected_members = HashMap::from([(
            "Model",
            MemberValue::Str("does-not-match-empty-fixture".into()),
        )]);
        let mut rejected_ctx = Ctx::new(&mut rejected_members);
        let mut rejected_sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let rejected_result = process_keyed_directory(
            &TABLE,
            KeyedBlock::new(&rejected, ByteOrder::Big, scope()),
            &mut rejected_ctx,
            &mut rejected_sink,
        );
        assert_eq!(rejected_result.bad_value, 1);
        assert_eq!(rejected_sink.warnings, vec!["Invalid CanonCustom data"]);
        assert_eq!(
            rejected_result.word_traces,
            vec![WordDirectoryTrace {
                module: "Test",
                table: "Main",
                returned: false,
                warnings: vec!["Invalid CanonCustom data"],
                verbose_directories: vec![],
                entries: vec![],
            }]
        );

        // A zero-byte directory is a successful native invocation with no
        // HandleTag calls; it is not the same result as rejection.
        let mut empty_members = HashMap::new();
        let mut empty_ctx = Ctx::new(&mut empty_members);
        let mut empty_sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let empty_result = process_keyed_directory(
            &TABLE,
            KeyedBlock::new(&[], ByteOrder::Big, scope()),
            &mut empty_ctx,
            &mut empty_sink,
        );
        assert_eq!(empty_result.word_traces.len(), 1);
        assert!(empty_result.word_traces[0].returned);
        assert!(empty_result.word_traces[0].warnings.is_empty());
        assert!(empty_result.word_traces[0].entries.is_empty());
        assert!(empty_sink.warnings.is_empty());

        // Gate B never invokes the source processor, so it deliberately has
        // no native-style return or warning trace.
        let mut gated_members = HashMap::new();
        let mut gated_ctx = Ctx::new(&mut gated_members);
        let mut gated_sink = Sink::default();
        let gated_result = process_keyed_directory(
            &TABLE,
            KeyedBlock::new(&[], ByteOrder::Big, scope()),
            &mut gated_ctx,
            &mut gated_sink,
        );
        assert_eq!(gated_result.gate_b_blocked, 1);
        assert!(gated_result.word_traces.is_empty());
        assert!(gated_sink.warnings.is_empty());
    }

    #[test]
    #[ignore = "requires the pinned ExifTool tree and EXIFTOOL_PERL; run in the native validation wave"]
    fn generated_word_directory_matches_native_probe_v1() {
        let table = find_keyed_table("CanonCustom", "FunctionsD30")
            .expect("canonical generated FunctionsD30 table");
        let KeyedLayout::LengthPrefixedU16Pairs(word) = table.layout else {
            panic!("FunctionsD30 must retain its generated word layout");
        };
        assert!(table.gate_a.passes());
        assert_eq!(word.source_file, "Image/ExifTool/CanonCustom.pm");

        let cases = vec![
            serde_json::json!({
                "protocol": "oxidex.word_processor.v1",
                "module": "CanonCustom",
                "table": "FunctionsD30",
                "case": {
                    "name": "ii-normal", "byte_order": "II", "data_hex": "060002010403",
                    "dir_start": 0, "dir_len": 6, "members": {},
                },
            }),
            serde_json::json!({
                "protocol": "oxidex.word_processor.v1",
                "module": "Image::ExifTool::CanonCustom",
                "table": "FunctionsD30",
                "case": {
                    "name": "mm-normal", "byte_order": "MM", "data_hex": "000601020304",
                    "dir_start": 0, "dir_len": 6, "members": {},
                },
            }),
            serde_json::json!({
                "protocol": "oxidex.word_processor.v1",
                "module": "CanonCustom",
                "table": "FunctionsD30",
                "case": {
                    "name": "verbose-normal", "byte_order": "II", "data_hex": "060002010403",
                    "dir_start": 0, "dir_len": 6, "members": {}, "verbose": true,
                },
            }),
            serde_json::json!({
                "protocol": "oxidex.word_processor.v1",
                "module": "CanonCustom",
                "table": "FunctionsD30",
                "case": {
                    "name": "missing-model-rejects-exception", "byte_order": "II", "data_hex": "0300020134",
                    "dir_start": 0, "dir_len": 5, "members": {}, "verbose": true,
                },
            }),
            serde_json::json!({
                "protocol": "oxidex.word_processor.v1",
                "module": "CanonCustom",
                "table": "FunctionsD30",
                "case": {
                    "name": "d60-length-exception", "byte_order": "II", "data_hex": "0300020134",
                    "dir_start": 0, "dir_len": 5, "members": {"Model": "EOS D60"}, "verbose": true,
                },
            }),
            serde_json::json!({
                "protocol": "oxidex.word_processor.v1",
                "module": "CanonCustom",
                "table": "FunctionsD30",
                "case": {
                    "name": "mm-odd-short", "byte_order": "MM", "data_hex": "00050102ff",
                    "dir_start": 0, "dir_len": 5, "members": {},
                },
            }),
            serde_json::json!({
                "protocol": "oxidex.word_processor.v1",
                "module": "CanonCustom",
                "table": "FunctionsD30",
                "case": {
                    "name": "empty", "byte_order": "II", "data_hex": "",
                    "dir_start": 0, "dir_len": 0, "members": {}, "verbose": true,
                },
            }),
        ];
        let replies = probe_word_processor(&cases);
        assert_eq!(replies.len(), cases.len());

        for (request, reply) in cases.iter().zip(replies) {
            assert_eq!(reply["protocol"], "oxidex.word_processor.v1");
            assert_eq!(reply["ok"], true, "native probe response: {reply}");
            assert_eq!(
                reply["unexpected_side_effects"],
                serde_json::json!([]),
                "native probe response: {reply}"
            );
            assert_eq!(
                reply["selection"]["process"]["source_file"], word.source_file,
                "native source identity changed"
            );
            assert_eq!(
                reply["selection"]["process"]["source_sha256"], word.source_sha256,
                "native source digest changed"
            );
            assert_eq!(
                reply["selection"]["process"]["source_body_sha256"], word.source_body_sha256,
                "native processor body digest changed"
            );
            assert_eq!(
                reply["selection"]["reader_binding"]["requested"],
                "Image::ExifTool::CanonCustom::Get16u"
            );
            assert_eq!(
                reply["selection"]["reader_binding"]["name"],
                "Image::ExifTool::Get16u"
            );

            let order = match request["case"]["byte_order"]
                .as_str()
                .expect("probe byte order")
            {
                "II" => ByteOrder::Little,
                "MM" => ByteOrder::Big,
                other => panic!("unsupported probe order {other}"),
            };
            let data = hex_bytes(
                request["case"]["data_hex"]
                    .as_str()
                    .expect("probe data hex"),
            );
            let mut members = HashMap::new();
            if let Some(model) = request["case"]["members"]["Model"].as_str() {
                members.insert("Model", MemberValue::Str(model.to_owned()));
            }
            let mut ctx = Ctx::new(&mut members);
            let mut sink = Sink {
                enabled: true,
                verbose: request["case"]["verbose"].as_bool().unwrap_or(false),
                ..Sink::default()
            };
            let result = process_keyed_directory(
                table,
                KeyedBlock::new(&data, order, native_scope()),
                &mut ctx,
                &mut sink,
            );
            assert_eq!(result.word_traces.len(), 1);
            let trace = &result.word_traces[0];
            assert_eq!(trace.module, "CanonCustom");
            assert_eq!(trace.table, "FunctionsD30");
            assert_eq!(trace.returned, native_number(&reply["returned"]) == 1);
            assert_eq!(trace.entries, result.word_entries);

            let native_warnings = reply["warnings"]
                .as_array()
                .expect("native warnings array")
                .iter()
                .filter_map(|arguments| arguments[0]["string"].as_str())
                .map(str::trim_end)
                .collect::<Vec<_>>();
            assert_eq!(trace.warnings, native_warnings);
            assert_eq!(sink.warnings, trace.warnings);

            let native_verbose = reply["verbose_dirs"]
                .as_array()
                .expect("native VerboseDir array");
            assert_eq!(trace.verbose_directories.len(), native_verbose.len());
            assert_eq!(sink.verbose_directories, trace.verbose_directories);
            for (rust, native) in trace.verbose_directories.iter().zip(native_verbose) {
                assert_eq!(
                    rust.directory,
                    native[0]["string"]
                        .as_str()
                        .expect("native verbose directory name")
                );
                let native_count = native[1]["numeric"]
                    .as_f64()
                    .expect("native verbose directory count");
                assert_eq!(rust.entry_count, native_count);
            }

            let native_tags = reply["handle_tags"]
                .as_array()
                .expect("native HandleTag array");
            assert_eq!(trace.entries.len(), native_tags.len());
            for (entry, native) in trace.entries.iter().zip(native_tags) {
                assert_eq!(i64::from(entry.raw_id), native_number(&native["raw_id"]));
                assert_eq!(entry.value, native_number(&native["value"]));
                assert_eq!(entry.index as i64, native_number(&native["index"]));
                assert_eq!(word_format_name(entry.format), native["format"]["string"]);
                assert_eq!(entry.count as i64, native_number(&native["count"]));
                assert_eq!(entry.size as i64, native_number(&native["size"]));
            }
        }
    }

    #[test]
    fn generated_canonraw_child_dispatches_to_word_table_without_unblocking_main() {
        let main = find_keyed_table("CanonRaw", "Main").expect("canonical generated CanonRaw Main");
        let target = find_keyed_table("CanonCustom", "FunctionsD30")
            .expect("canonical generated FunctionsD30");
        assert!(!main.gate_a.passes(), "full CanonRaw Main remains blocked");
        assert!(target.gate_a.passes());

        for order in [ByteOrder::Little, ByteOrder::Big] {
            let child = words(order, 4, &[0x0101], &[]);
            let carrier = ciff(order, &[(0x1033, child)]);
            let mut members = HashMap::from([("Model", MemberValue::Str("EOS D30".into()))]);
            let mut ctx = Ctx::new(&mut members);
            let mut blocked_sink = Sink {
                enabled: true,
                ..Sink::default()
            };
            let blocked = process_keyed_directory(
                main,
                KeyedBlock::new(&carrier, order, native_scope()),
                &mut ctx,
                &mut blocked_sink,
            );
            assert_eq!(blocked.gate_a_blocked, 1);
            assert!(blocked.word_traces.is_empty());
            assert!(blocked_sink.rows.is_empty());

            // Test only the generated 0x1033 edge: every source tag,
            // variant, validation operand, and target identity remains from
            // the immutable canonical parent. The local Gate A projection
            // does not claim the full parent is ready.
            let narrowed: &'static KeyedDirectoryTable = Box::leak(Box::new(KeyedDirectoryTable {
                gate_a: GateA { blocked_by: &[] },
                ..*main
            }));
            let mut routed_members = HashMap::from([("Model", MemberValue::Str("EOS D30".into()))]);
            let mut routed_ctx = Ctx::new(&mut routed_members);
            let mut routed_sink = Sink {
                enabled: true,
                ..Sink::default()
            };
            let routed = process_keyed_directory(
                narrowed,
                KeyedBlock::new(&carrier, order, native_scope()),
                &mut routed_ctx,
                &mut routed_sink,
            );
            assert_eq!(routed.validation_rejected, 0);
            assert_eq!(routed.gate_a_blocked, 0);
            assert_eq!(routed.gate_b_blocked, 0);
            assert_eq!(routed.word_traces.len(), 1);
            assert!(routed.word_traces[0].returned);
            assert_eq!(routed.word_traces[0].module, "CanonCustom");
            assert_eq!(routed.word_traces[0].table, "FunctionsD30");
            assert_eq!(
                routed.word_traces[0].entries,
                vec![WordDirectoryEntry {
                    raw_id: 1,
                    value: 1,
                    index: 0,
                    format: Fmt::Int8u,
                    count: 1,
                    size: 1,
                }]
            );
            assert_eq!(routed_sink.rows.len(), 1);
            assert_eq!(routed_sink.rows[0].name, "LongExposureNoiseReduction");
            assert_eq!(routed_sink.rows[0].value, TagValue::String("On".into()));

            let mut gate_b_members = HashMap::from([("Model", MemberValue::Str("EOS D30".into()))]);
            let mut gate_b_ctx = Ctx::new(&mut gate_b_members);
            let mut parent_only_sink = ParentOnlySink::default();
            let gate_b = process_keyed_directory(
                narrowed,
                KeyedBlock::new(&carrier, order, native_scope()),
                &mut gate_b_ctx,
                &mut parent_only_sink,
            );
            assert_eq!(gate_b.gate_b_blocked, 1);
            assert!(gate_b.word_traces.is_empty());
            assert!(parent_only_sink.rows.is_empty());
            assert!(parent_only_sink.warnings.is_empty());
        }
    }

    #[test]
    fn canonical_word_registry_is_complete_but_remains_unrouted() {
        let word_tables = ALL_KEYED_TABLES
            .iter()
            .copied()
            .filter(|table| matches!(table.layout, KeyedLayout::LengthPrefixedU16Pairs(_)))
            .collect::<Vec<_>>();
        assert_eq!(word_tables.len(), 9);
        assert_eq!(
            word_tables
                .iter()
                .map(|table| table.tags.len())
                .sum::<usize>(),
            132
        );
        assert!(word_tables.iter().all(|table| {
            table.module == "CanonCustom"
                && table.gate_a.passes()
                && matches!(table.layout, KeyedLayout::LengthPrefixedU16Pairs(_))
        }));
        // Generated presence is only schema/reader data: the concrete caller
        // policy remains disabled unless a future carrier opts in.
        let funcs_d30 = find_keyed_table("CanonCustom", "FunctionsD30").unwrap();
        let mut members = HashMap::new();
        let mut ctx = Ctx::new(&mut members);
        let mut disabled = Sink::default();
        let result = process_keyed_directory(
            funcs_d30,
            KeyedBlock::new(&[], ByteOrder::Little, native_scope()),
            &mut ctx,
            &mut disabled,
        );
        assert_eq!(result.gate_b_blocked, 1);
        assert!(result.word_traces.is_empty());
    }

    #[test]
    fn real_keyed_word_table_retains_source_coordinate_storage_and_list_fact() {
        let table = find_keyed_table("CanonCustom", "FunctionsD30")
            .expect("generated CanonCustom::FunctionsD30 table");
        let data = words(ByteOrder::Big, 4, &[0x0101], &[]);
        let mut members = HashMap::new();
        let mut ctx = Ctx::new(&mut members);
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let result = process_keyed_directory(
            table,
            KeyedBlock::new(
                &data,
                ByteOrder::Big,
                KeyedScope {
                    group1_override: Some("Canon"),
                },
            ),
            &mut ctx,
            &mut sink,
        );
        assert_eq!(result.emitted, 1);
        assert_eq!(sink.rows.len(), 1);
        let row = &sink.rows[0];
        assert_eq!(row.name, "LongExposureNoiseReduction");
        assert_eq!(row.source_id, oxidex_tags::TagId::Numeric(1));
        assert_eq!(row.stored, TagValue::Integer(1));
        assert_eq!(row.value, TagValue::String("On".to_owned()));
        assert_eq!(row.group0, "MakerNotes");
        assert_eq!(row.group1, "Canon");
        assert_eq!(row.group2, "Camera");
        assert!(!row.is_list, "source flags do not declare List");
    }

    #[test]
    fn word_directory_uses_empty_missing_model_only_for_length_exception() {
        static TAGS: [KeyedTag; 1] = [tag(1, "CustomFunction", Some(Fmt::Int8u), Some(1))];
        static TABLE: KeyedDirectoryTable = word_table(&TAGS, &[]);
        // Header says four bytes while the bounded directory has six. The
        // missing `Model` is the native empty regex subject, so `+ 2` admits
        // this carrier without permanently inventing a model member.
        let data = words(ByteOrder::Big, 4, &[0x01ff, 0x0201], &[]);
        let mut members = HashMap::new();
        let mut ctx = Ctx::new(&mut members);
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let result = process_keyed_directory(
            &TABLE,
            KeyedBlock::new(&data, ByteOrder::Big, scope()),
            &mut ctx,
            &mut sink,
        );
        assert_eq!(result.bad_value, 0);
        assert_eq!(result.entries_seen, 2);
        assert!(!members.contains_key("Model"));

        static FAST_LAYOUT: WordDirectory = WordDirectory {
            model_condition: Cond::SetMember {
                member: "HeaderMustNotReadModel",
                source: crate::exiftool_tables::EffectSource::Const(1),
                then: None,
            },
            ..WORD_LAYOUT
        };
        static FAST_TABLE: KeyedDirectoryTable = KeyedDirectoryTable {
            layout: KeyedLayout::LengthPrefixedU16Pairs(FAST_LAYOUT),
            tags: &TAGS,
            ..EMPTY_TABLE
        };
        let exact_data = words(ByteOrder::Big, 4, &[0x01ff], &[]);
        let mut fast_members = HashMap::new();
        let mut fast_ctx = Ctx::new(&mut fast_members);
        let mut fast_sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let fast_result = process_keyed_directory(
            &FAST_TABLE,
            KeyedBlock::new(&exact_data, ByteOrder::Big, scope()),
            &mut fast_ctx,
            &mut fast_sink,
        );
        assert_eq!(fast_result.bad_value, 0);
        assert!(!fast_members.contains_key("HeaderMustNotReadModel"));
    }

    #[test]
    fn word_directory_coerces_odd_final_u16_to_zero_in_physical_order() {
        static TAGS: [KeyedTag; 2] = [
            tag(1, "First", Some(Fmt::Int8u), Some(1)),
            tag(0, "ShortWord", Some(Fmt::Int8u), Some(1)),
        ];
        static TABLE: KeyedDirectoryTable = word_table(&TAGS, &[]);
        // The final byte cannot form a u16. Native `Get16u` returns undef,
        // then its numeric use yields zero and still invokes HandleTag.
        for order in [ByteOrder::Big, ByteOrder::Little] {
            let data = words(order, 5, &[0x01ff], &[0xff]);
            let (sink, result) = walk_test(&TABLE, &data, order);
            assert_eq!(
                result.word_entries,
                vec![
                    WordDirectoryEntry {
                        raw_id: 1,
                        value: 511,
                        index: 0,
                        format: Fmt::Int8u,
                        count: 1,
                        size: 1,
                    },
                    WordDirectoryEntry {
                        raw_id: 0,
                        value: 0,
                        index: 1,
                        format: Fmt::Int8u,
                        count: 1,
                        size: 1,
                    },
                ]
            );
            assert_eq!(
                sink.rows.iter().map(|row| row.name).collect::<Vec<_>>(),
                vec!["First", "ShortWord"]
            );
        }
    }

    #[test]
    fn native_u16_directory_count_has_no_reader_entry_cap() {
        // ProcessCanonRaw accepts the full u16 entry count and relies on
        // directory-byte bounds. A well-formed 4,097-entry CIFF directory
        // must not acquire an arbitrary reader refusal.
        static TAGS: [KeyedTag; 1] = [tag(1, "Value", Some(Fmt::Int8u), None)];
        static TABLE: KeyedDirectoryTable = table(&TAGS);
        let entries = vec![(0x4001, vec![7]); 4097];
        let data = ciff(ByteOrder::Little, &entries);
        let (sink, result) = walk_test(&TABLE, &data, ByteOrder::Little);
        assert_eq!(result.entries_seen, 4097);
        assert_eq!(result.emitted, 4097);
        assert_eq!(result.malformed_directory, 0);
        assert_eq!(sink.rows.len(), 4097);
    }

    #[test]
    fn ciff10_carriers_honor_byte_order_inline_and_zero_count() {
        static TAGS: [KeyedTag; 2] = [
            tag(0x1001, "Inline", Some(Fmt::Int16u), None),
            tag(0x1002, "ZeroCount", Some(Fmt::Int16u), Some(0)),
        ];
        static TABLE: KeyedDirectoryTable = table(&TAGS);
        for order in [ByteOrder::Little, ByteOrder::Big] {
            let one = match order {
                ByteOrder::Little => vec![0x34, 0x12],
                ByteOrder::Big => vec![0x12, 0x34],
            };
            let two = match order {
                ByteOrder::Little => vec![1, 0, 2, 0],
                ByteOrder::Big => vec![0, 1, 0, 2],
            };
            let data = ciff(order, &[(0x5001, one), (0x1002, two)]);
            let (sink, result) = walk_test(&TABLE, &data, order);
            assert_eq!(result.emitted, 2);
            assert_eq!(sink.rows[0].value, TagValue::Integer(0x1234));
            assert_eq!(sink.rows[1].value, TagValue::String("1 2".into()));
            assert!(sink.rows.iter().all(|row| row.group0 == "Test"));
            assert!(sink.rows.iter().all(|row| row.group1 == "Carrier"));
            assert!(sink.rows.iter().all(|row| row.group2 == "Other"));
        }
    }

    #[test]
    fn carrier_overrides_only_group1_after_source_group_resolution() {
        static TAG: KeyedTag = KeyedTag {
            groups: TagGroups {
                g0: Some("Source0"),
                g1: Some("Source1"),
                g2: Some("Source2"),
            },
            ..tag(1, "Value", Some(Fmt::Int8u), None)
        };
        static TAGS: [KeyedTag; 1] = [TAG];
        static TABLE: KeyedDirectoryTable = KeyedDirectoryTable {
            group0: "Table0",
            group1: "Table1",
            group2: "Table2",
            tags: &TAGS,
            ..EMPTY_TABLE
        };
        let data = ciff(ByteOrder::Little, &[(0x4001, vec![7])]);
        for (scope, expected_group1) in [(native_scope(), "Source1"), (scope(), "Carrier")] {
            let mut members = HashMap::new();
            let mut ctx = Ctx::new(&mut members);
            let mut sink = Sink {
                enabled: true,
                ..Sink::default()
            };
            let result = process_keyed_directory(
                &TABLE,
                KeyedBlock::new(&data, ByteOrder::Little, scope),
                &mut ctx,
                &mut sink,
            );
            assert_eq!(result.emitted, 1);
            assert_eq!(sink.rows[0].group0, "Source0");
            assert_eq!(sink.rows[0].group1, expected_group1);
            assert_eq!(sink.rows[0].group2, "Source2");
        }
    }

    #[test]
    fn default_string_honors_a_declared_count() {
        // A native default type 0x08 is `string`, not a bare ProcessBinaryData
        // RemainderString. Count 3 must not consume the sixth source byte.
        static TAGS: [KeyedTag; 1] = [tag(0x0803, "Counted", None, Some(3))];
        static TABLE: KeyedDirectoryTable = table(&TAGS);
        let data = ciff(ByteOrder::Little, &[(0x0803, b"abcdef".to_vec())]);
        let (sink, result) = walk_test(&TABLE, &data, ByteOrder::Little);
        assert_eq!(result.emitted, 1);
        assert_eq!(sink.rows[0].value, TagValue::String("abc".into()));
    }

    #[test]
    fn dir_name_is_scoped_and_unknown_same_table_keys_recurse() {
        static CHILD: KeyedTag = KeyedTag {
            condition: Some(Cond::MemberStrEq {
                member: "DIR_NAME",
                value: "Description",
                negate: false,
            }),
            ..tag(0x0805, "Child", Some(Fmt::RemainderString), None)
        };
        static TAGS: [KeyedTag; 2] = [
            KeyedTag {
                edge: Some(KeyedEdge::SameTableDirectory),
                ..tag(0x2804, "Description", None, None)
            },
            CHILD,
        ];
        static TABLE: KeyedDirectoryTable = table(&TAGS);
        for order in [ByteOrder::Little, ByteOrder::Big] {
            let child = ciff(order, &[(0x0805, b"nested\0".to_vec())]);
            let unknown = ciff(order, &[(0x0805, b"unknown\0".to_vec())]);
            let data = ciff(order, &[(0x2804, child), (0x2809, unknown)]);
            let (sink, result) = walk_test(&TABLE, &data, order);
            assert_eq!(result.emitted, 1);
            assert_eq!(sink.rows[0].name, "Child");
            assert_eq!(sink.rows[0].value, TagValue::String("nested".into()));
        }
    }

    #[test]
    fn recorded_native_ciff_carriers_replay_the_dir_name_case_in_both_orders() {
        // Copied from native-keyed-fixtures after pinned 13.59 oracle capture:
        // dirname-{ii,mm}.crw each reports CanonFileDescription child-description.
        static CHILD: KeyedTag = KeyedTag {
            condition: Some(Cond::MemberStrEq {
                member: "DIR_NAME",
                value: "Description",
                negate: false,
            }),
            ..tag(
                0x0805,
                "CanonFileDescription",
                Some(Fmt::RemainderString),
                None,
            )
        };
        static TAGS: [KeyedTag; 2] = [
            KeyedTag {
                edge: Some(KeyedEdge::SameTableDirectory),
                ..tag(0x2804, "Description", None, None)
            },
            CHILD,
        ];
        static TABLE: KeyedDirectoryTable = table(&TAGS);
        for (order, file) in [
            (
                ByteOrder::Little,
                include_bytes!("../../tests/fixtures/keyed_reader/dirname-ii.crw").as_slice(),
            ),
            (
                ByteOrder::Big,
                include_bytes!("../../tests/fixtures/keyed_reader/dirname-mm.crw").as_slice(),
            ),
        ] {
            // ProcessCRW gives ProcessCanonRaw the heap beginning after the
            // 14-byte II/MM, heap-offset and HEAPCCDR carrier header.
            let (sink, result) = walk_test(&TABLE, &file[14..], order);
            assert_eq!(result.emitted, 1);
            assert_eq!(sink.rows[0].name, "CanonFileDescription");
            assert_eq!(
                sink.rows[0].value,
                TagValue::String("child-description".into())
            );
        }
    }

    #[test]
    fn native_nine_directory_fixture_reaches_the_leaf_without_a_depth_cap() {
        // Pinned ExifTool 13.59 reports CanonFileDescription "nine-deep" from
        // this copied CIFF fixture. The nine directory offsets are distinct.
        static CHILD: KeyedTag = KeyedTag {
            condition: Some(Cond::MemberStrEq {
                member: "DIR_NAME",
                value: "ImageDescription",
                negate: false,
            }),
            ..tag(
                0x0805,
                "CanonFileDescription",
                Some(Fmt::RemainderString),
                None,
            )
        };
        static TAGS: [KeyedTag; 2] = [
            KeyedTag {
                edge: Some(KeyedEdge::SameTableDirectory),
                ..tag(0x2804, "ImageDescription", None, None)
            },
            CHILD,
        ];
        static TABLE: KeyedDirectoryTable = table(&TAGS);
        let carrier =
            include_bytes!("../../tests/fixtures/keyed_reader/nine-distinct-directories.crw");
        let (sink, result) = walk_test(&TABLE, &carrier[14..], ByteOrder::Little);
        assert_eq!(result.duplicate_directory, 0);
        assert_eq!(result.emitted, 1);
        assert_eq!(sink.rows[0].name, "CanonFileDescription");
        assert_eq!(sink.rows[0].value, TagValue::String("nine-deep".into()));
    }

    #[test]
    fn iterative_walk_handles_a_deep_acyclic_directory_chain() {
        // This is deliberately much deeper than the native nine-directory
        // control. The explicit work stack must not become a Rust call stack.
        static TAGS: [KeyedTag; 1] = [tag(1, "Leaf", Some(Fmt::Int8u), None)];
        static TABLE: KeyedDirectoryTable = table(&TAGS);
        let mut data = ciff(ByteOrder::Little, &[(0x4001, vec![7])]);
        for _ in 0..256 {
            data = ciff(ByteOrder::Little, &[(0x2804, data)]);
        }
        let (sink, result) = walk_test(&TABLE, &data, ByteOrder::Little);
        assert_eq!(result.duplicate_directory, 0);
        assert_eq!(result.emitted, 1);
        assert_eq!(sink.rows[0].name, "Leaf");
        assert_eq!(sink.rows[0].value, TagValue::Integer(7));
    }

    #[test]
    fn raw_state_follows_file_order_without_prescan() {
        static MODEL: KeyedTag = KeyedTag {
            raw_conv: Some(RawConvEffect::SetMember { member: "Model" }),
            omitted: Omitted {
                raw_conv: true,
                ..Omitted::NONE
            },
            ..tag(0x0801, "Model", Some(Fmt::RemainderString), None)
        };
        static SERIAL: KeyedTag = KeyedTag {
            condition: Some(Cond::MemberStrEq {
                member: "Model",
                value: "D30",
                negate: false,
            }),
            ..tag(0x1802, "Serial", Some(Fmt::Int32u), None)
        };
        static TAGS: [KeyedTag; 2] = [MODEL, SERIAL];
        static TABLE: KeyedDirectoryTable = table(&TAGS);
        let model_first = ciff(
            ByteOrder::Little,
            &[
                (0x0801, b"D30\0".to_vec()),
                (0x1802, 0x1234_0042u32.to_le_bytes().to_vec()),
            ],
        );
        let serial_first = ciff(
            ByteOrder::Little,
            &[
                (0x1802, 0x1234_0042u32.to_le_bytes().to_vec()),
                (0x0801, b"D30\0".to_vec()),
            ],
        );
        let (model_first_sink, _) = walk_test(&TABLE, &model_first, ByteOrder::Little);
        let (serial_first_sink, _) = walk_test(&TABLE, &serial_first, ByteOrder::Little);
        assert_eq!(model_first_sink.rows.len(), 2);
        assert_eq!(serial_first_sink.rows.len(), 1);
        assert_eq!(model_first_sink.rows[1].name, "Serial");
    }

    #[test]
    fn high_bit_gate_and_unwalked_edges_refuse_without_output() {
        static SCALAR: KeyedTag = tag(1, "Value", Some(Fmt::Int8u), None);
        static SCALAR_TAGS: [KeyedTag; 1] = [SCALAR];
        static BLOCKED: KeyedDirectoryTable = KeyedDirectoryTable {
            gate_a: GateA {
                blocked_by: &[("keyed_edge_unwalked", 1)],
            },
            tags: &SCALAR_TAGS,
            ..EMPTY_TABLE
        };
        static UNWALKED: KeyedTag = KeyedTag {
            edge: Some(KeyedEdge::BoundedValue {
                module: "CanonRaw",
                table: "MakeModel",
                start: KeyedStart::Zero,
                validation: None,
                unwalked: &["Validate"],
            }),
            ..tag(0x1001, "Edge", None, None)
        };
        static UNWALKED_TAGS: [KeyedTag; 1] = [UNWALKED];
        static UNWALKED_TABLE: KeyedDirectoryTable = table(&UNWALKED_TAGS);
        let data = ciff(ByteOrder::Little, &[(0x4001, vec![7]), (0x8001, vec![9])]);
        let (sink, result) = walk_test(&EMPTY_TABLE, &data, ByteOrder::Little);
        assert!(sink.rows.is_empty());
        assert_eq!(result.high_bit_error, 1);
        let (_, blocked) = walk_test(&BLOCKED, &data, ByteOrder::Little);
        assert_eq!(blocked.gate_a_blocked, 1);
        let edge_data = ciff(ByteOrder::Little, &[(0x1001, vec![1, 0, 0, 0])]);
        let (edge_sink, edge_result) = walk_test(&UNWALKED_TABLE, &edge_data, ByteOrder::Little);
        assert!(edge_sink.rows.is_empty());
        assert_eq!(edge_result.unwalked_edge, 1);
    }

    #[test]
    fn false_bounded_value_validation_skips_only_the_child_before_target_lookup() {
        static VALIDATION: U16SizeCheck = U16SizeCheck {
            offset: 0,
            expected: &[SizeExpectation::Relative(0)],
            expression: "Test::Validate($dirData,$subdirStart,$size)",
            callee: "Test::Validate",
            source_file: "test.pm",
            source_sha256: "test",
            // Supplied by the pending source-reader contract schema. A test
            // fixture has no external reader provenance.
            reader_contract_sha256: None,
        };
        static CHILD: KeyedTag = KeyedTag {
            edge: Some(KeyedEdge::BoundedValue {
                module: "Unavailable",
                table: "MustNotBeLookedUp",
                start: KeyedStart::Zero,
                validation: Some(VALIDATION),
                unwalked: &[],
            }),
            ..tag(0x1001, "Child", None, None)
        };
        static LATER: KeyedTag = tag(2, "Later", Some(Fmt::Int8u), None);
        static TAGS: [KeyedTag; 2] = [CHILD, LATER];
        static TABLE: KeyedDirectoryTable = table(&TAGS);

        // The child declares six bytes, but its first u16 is five: native
        // Validate is false. The missing target is deliberately irrelevant;
        // the later parent row proves false validation continued normally.
        let data = ciff(
            ByteOrder::Big,
            &[(0x1001, vec![0, 5, 0, 0, 0, 0]), (0x4002, vec![7])],
        );
        let mut members = HashMap::new();
        members.insert("DIR_NAME", MemberValue::Str("Parent".into()));
        let mut ctx = Ctx::new(&mut members);
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let result = process_keyed_directory(
            &TABLE,
            KeyedBlock::new(&data, ByteOrder::Big, scope()),
            &mut ctx,
            &mut sink,
        );
        assert_eq!(result.validation_rejected, 1);
        assert_eq!(result.unavailable_target, 0);
        assert_eq!(result.gate_b_blocked, 0);
        assert_eq!(
            sink.rows.iter().map(|row| row.name).collect::<Vec<_>>(),
            vec!["Later"]
        );
        assert_eq!(
            members.get("DIR_NAME"),
            Some(&MemberValue::Str("Parent".into()))
        );
    }

    #[test]
    fn inline_bounded_value_validation_uses_all_eight_ciff_bytes_in_both_orders() {
        // ProcessCanonRaw gives an inline CIFF value a declared `$size` of
        // eight. Native Validate sees the complete eight-byte `$value`; the
        // terminal u16 below is deliberately at bytes 6..8, not in a sliced
        // prefix. A nonzero accepted word also detects swapped byte order.
        static VALIDATION: U16SizeCheck = U16SizeCheck {
            offset: 6,
            expected: &[SizeExpectation::Relative(0)],
            expression: "Test::Validate($dirData,$subdirStart + 6,$size)",
            callee: "Test::Validate",
            source_file: "test.pm",
            source_sha256: "test",
            reader_contract_sha256: None,
        };
        static CHILD: KeyedTag = KeyedTag {
            edge: Some(KeyedEdge::BoundedValue {
                module: "Unavailable",
                table: "MustNotBeLookedUp",
                start: KeyedStart::Zero,
                validation: Some(VALIDATION),
                unwalked: &[],
            }),
            // Raw IDs retain the type bits. The on-disk inline entry below is
            // 0x5001: inline bit 0x4000 plus this source key 0x1001.
            ..tag(0x1001, "InlineChild", None, None)
        };
        static LATER: KeyedTag = tag(2, "Later", Some(Fmt::Int8u), None);
        static TAGS: [KeyedTag; 2] = [CHILD, LATER];
        static TABLE: KeyedDirectoryTable = table(&TAGS);

        for order in [ByteOrder::Little, ByteOrder::Big] {
            let mut accepted = vec![0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6];
            accepted.extend_from_slice(&match order {
                ByteOrder::Little => 8u16.to_le_bytes(),
                ByteOrder::Big => 8u16.to_be_bytes(),
            });
            assert_eq!(accepted.len(), 8);
            let accepted_data = ciff(order, &[(0x5001, accepted)]);
            let (_, accepted_result) = walk_test(&TABLE, &accepted_data, order);
            // Reaching the target proves the full inline tail and declared
            // size are both eight, decoded in the inherited byte order.
            assert_eq!(accepted_result.validation_rejected, 0);
            assert_eq!(accepted_result.unavailable_target, 1);

            let mut rejected = vec![0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6];
            rejected.extend_from_slice(&match order {
                ByteOrder::Little => 9u16.to_le_bytes(),
                ByteOrder::Big => 9u16.to_be_bytes(),
            });
            assert_eq!(rejected.len(), 8);
            let data = ciff(order, &[(0x5001, rejected), (0x4002, vec![7])]);
            let mut members = HashMap::new();
            members.insert("DIR_NAME", MemberValue::Str("Parent".into()));
            let mut ctx = Ctx::new(&mut members);
            let mut sink = Sink {
                enabled: true,
                ..Sink::default()
            };
            let result = process_keyed_directory(
                &TABLE,
                KeyedBlock::new(&data, order, scope()),
                &mut ctx,
                &mut sink,
            );
            // Nine differs from the declared eight bytes, so validation must
            // reject without reaching the missing target. The later row and
            // parent name prove that rejection resumes the parent unchanged.
            assert_eq!(result.validation_rejected, 1);
            assert_eq!(result.unavailable_target, 0);
            assert_eq!(
                sink.rows.iter().map(|row| row.name).collect::<Vec<_>>(),
                vec!["Later"]
            );
            assert_eq!(
                members.get("DIR_NAME"),
                Some(&MemberValue::Str("Parent".into()))
            );
        }
    }

    #[test]
    fn value_context_conditions_and_gate_b_are_never_retried_or_defaulted() {
        static TAG: KeyedTag = KeyedTag {
            condition: Some(Cond::ValPtRegex {
                pattern: "^x",
                negate: false,
            }),
            ..tag(1, "NeedsValue", Some(Fmt::Int8u), None)
        };
        static TAGS: [KeyedTag; 1] = [TAG];
        static TABLE: KeyedDirectoryTable = table(&TAGS);
        let data = ciff(ByteOrder::Little, &[(0x4001, vec![b'x'])]);
        let (sink, result) = walk_test(&TABLE, &data, ByteOrder::Little);
        assert!(sink.rows.is_empty());
        assert_eq!(result.initial_context_refusal, 1);

        let mut members = HashMap::new();
        let mut ctx = Ctx::new(&mut members);
        let mut disabled = Sink::default();
        let result = process_keyed_directory(
            &TABLE,
            KeyedBlock::new(&data, ByteOrder::Little, scope()),
            &mut ctx,
            &mut disabled,
        );
        assert_eq!(result.gate_b_blocked, 1);
        assert!(disabled.rows.is_empty());
    }

    #[test]
    fn compiled_condition_clears_only_its_own_omission_once() {
        // Codegen retains omitted.condition for audit even when its closed
        // Cond was emitted. A resolved normal row remains reportable.
        static TAG: KeyedTag = KeyedTag {
            condition: Some(Cond::Always),
            omitted: Omitted {
                condition: true,
                ..Omitted::NONE
            },
            ..tag(1, "Normal", Some(Fmt::Int8u), None)
        };
        static TAGS: [KeyedTag; 1] = [TAG];
        static TABLE: KeyedDirectoryTable = table(&TAGS);
        let data = ciff(ByteOrder::Little, &[(0x4001, vec![7])]);
        let (sink, result) = walk_test(&TABLE, &data, ByteOrder::Little);
        assert_eq!(result.emitted, 1);
        assert_eq!(sink.rows[0].value, TagValue::Integer(7));
    }

    #[test]
    fn variant_condition_is_evaluated_once_even_when_row_carries_a_copy() {
        static ROW: KeyedTag = KeyedTag {
            // The row copy must not receive a second value-context attempt.
            condition: Some(Cond::ValPtRegex {
                pattern: "^x",
                negate: false,
            }),
            omitted: Omitted {
                condition: true,
                ..Omitted::NONE
            },
            ..tag(0x1001, "Variant", Some(Fmt::Int8u), None)
        };
        static ALTERNATIVES: [(Cond, KeyedTag); 1] = [(Cond::Always, ROW)];
        static VARIANTS: [KeyedVariantGroup; 1] = [KeyedVariantGroup {
            raw_id: 0x1001,
            alternatives: &ALTERNATIVES,
        }];
        static TABLE: KeyedDirectoryTable = KeyedDirectoryTable {
            variants: &VARIANTS,
            ..EMPTY_TABLE
        };
        let data = ciff(ByteOrder::Little, &[(0x5001, vec![9])]);
        let (sink, result) = walk_test(&TABLE, &data, ByteOrder::Little);
        assert_eq!(result.emitted, 1);
        assert_eq!(sink.rows[0].value, TagValue::Integer(9));
    }

    #[test]
    fn reporting_flags_keep_unknown_selection_terminal_and_preserve_output_policy() {
        // `GetTagInfo` selects the first matching Unknown alternative, then
        // returns undef unless -u is requested. The following ordinary row
        // must not be tried as a fallback.
        static UNKNOWN: KeyedTag = KeyedTag {
            flags: IfdFlags {
                unknown: true,
                ..IfdFlags::NONE
            },
            ..tag(1, "Unknown", Some(Fmt::Int8u), None)
        };
        static FALLBACK: KeyedTag = tag(1, "Fallback", Some(Fmt::Int8u), None);
        static UNKNOWN_ALTERNATIVES: [(Cond, KeyedTag); 2] =
            [(Cond::Always, UNKNOWN), (Cond::Always, FALLBACK)];
        static UNKNOWN_VARIANTS: [KeyedVariantGroup; 1] = [KeyedVariantGroup {
            raw_id: 1,
            alternatives: &UNKNOWN_ALTERNATIVES,
        }];
        static UNKNOWN_TABLE: KeyedDirectoryTable = KeyedDirectoryTable {
            variants: &UNKNOWN_VARIANTS,
            ..EMPTY_TABLE
        };
        let unknown_data = ciff(ByteOrder::Little, &[(0x4001, vec![7])]);
        let (unknown_sink, unknown_result) =
            walk_test(&UNKNOWN_TABLE, &unknown_data, ByteOrder::Little);
        assert!(unknown_sink.rows.is_empty());
        assert_eq!(unknown_result.emitted, 0);

        static LIST: KeyedTag = KeyedTag {
            flags: IfdFlags {
                list: true,
                ..IfdFlags::NONE
            },
            ..tag(2, "List", Some(Fmt::Int8u), Some(2))
        };
        static BINARY: KeyedTag = KeyedTag {
            flags: IfdFlags {
                binary: true,
                ..IfdFlags::NONE
            },
            ..tag(0x0803, "Binary", Some(Fmt::Str(1)), Some(3))
        };
        static AVOID: KeyedTag = KeyedTag {
            flags: IfdFlags {
                avoid: true,
                ..IfdFlags::NONE
            },
            ..tag(4, "Avoid", Some(Fmt::Int8u), None)
        };
        static EXPLICIT_PRIORITY: KeyedTag = KeyedTag {
            flags: IfdFlags {
                avoid: true,
                priority: Some(1),
                ..IfdFlags::NONE
            },
            ..tag(5, "Priority", Some(Fmt::Int8u), None)
        };
        static TAGS: [KeyedTag; 4] = [LIST, BINARY, AVOID, EXPLICIT_PRIORITY];
        static TABLE: KeyedDirectoryTable = table(&TAGS);
        let data = ciff(
            ByteOrder::Little,
            &[
                (0x4002, vec![1, 2]),
                (0x4803, b"hi\0".to_vec()),
                (0x4004, vec![9]),
                (0x4005, vec![10]),
            ],
        );
        let (sink, result) = walk_test(&TABLE, &data, ByteOrder::Little);
        assert_eq!(result.emitted, 4);
        assert_eq!(
            sink.rows[0].value,
            TagValue::Array(vec![TagValue::Integer(1), TagValue::Integer(2)])
        );
        assert_eq!(
            sink.rows[0].stored,
            TagValue::Array(vec![TagValue::Integer(1), TagValue::Integer(2)])
        );
        assert_eq!(sink.rows[0].source_id, oxidex_tags::TagId::Numeric(2));
        assert!(sink.rows[0].is_list);
        assert_eq!(
            sink.rows[1].value,
            TagValue::String("(Binary data 2 bytes, use -b option to extract)".into())
        );
        let scoped = re_scope(
            sink.rows[0].clone(),
            KeyedScope {
                group1_override: Some("Nested"),
                ..scope()
            },
        );
        assert_eq!(scoped.group1, "Nested");
        assert_eq!(scoped.stored, sink.rows[0].stored);
        assert_eq!(scoped.source_id, sink.rows[0].source_id);
        assert!(scoped.is_list);
        assert!(sink.rows[2].low_priority);
        assert!(sink.rows[2].avoid);
        assert!(!sink.rows[3].low_priority);
        assert!(sink.rows[3].avoid);
    }

    #[test]
    fn tainted_child_stops_pending_parent_entries() {
        // An unsupported stateful child cannot be contained by restoring
        // DIR_NAME: ProcessDirectory preserves arbitrary member state. The
        // parent Serial-like row must not run after a tainted child.
        static STATE: KeyedTag = KeyedTag {
            raw_conv: Some(RawConvEffect::SetMember { member: "Model" }),
            omitted: Omitted {
                raw_conv: true,
                ..Omitted::NONE
            },
            ..tag(0x1801, "State", Some(Fmt::Float), None)
        };
        static LATER: KeyedTag = tag(2, "Later", Some(Fmt::Int8u), None);
        static TAGS: [KeyedTag; 2] = [STATE, LATER];
        static TABLE: KeyedDirectoryTable = table(&TAGS);
        let child = ciff(
            ByteOrder::Little,
            &[(0x1801, 1.5f32.to_le_bytes().to_vec())],
        );
        let parent = ciff(ByteOrder::Little, &[(0x2804, child), (0x4002, vec![7])]);
        let mut members = HashMap::new();
        members.insert("DIR_NAME", MemberValue::Str("ParentDirectory".into()));
        let mut ctx = Ctx::new(&mut members);
        let mut sink = Sink {
            enabled: true,
            ..Sink::default()
        };
        let result = process_keyed_directory(
            &TABLE,
            KeyedBlock::new(&parent, ByteOrder::Little, scope()),
            &mut ctx,
            &mut sink,
        );
        assert!(sink.rows.is_empty());
        assert_eq!(result.emitted, 0);
        assert_eq!(result.omitted, 1);
        assert_eq!(
            members.get("DIR_NAME"),
            Some(&MemberValue::Str("ParentDirectory".into())),
            "taint must discard pending sibling frames but still unwind child DIR_NAME"
        );
    }

    #[test]
    fn unrepresentable_member_state_stops_the_remaining_directory() {
        static STATE: KeyedTag = KeyedTag {
            raw_conv: Some(RawConvEffect::SetMember { member: "Model" }),
            omitted: Omitted {
                raw_conv: true,
                ..Omitted::NONE
            },
            ..tag(0x1801, "State", Some(Fmt::Float), None)
        };
        static LATER: KeyedTag = tag(2, "Later", Some(Fmt::Int8u), None);
        static TAGS: [KeyedTag; 2] = [STATE, LATER];
        static TABLE: KeyedDirectoryTable = table(&TAGS);
        let data = ciff(
            ByteOrder::Little,
            &[(0x1801, 1.5f32.to_le_bytes().to_vec()), (0x4002, vec![7])],
        );
        let (sink, result) = walk_test(&TABLE, &data, ByteOrder::Little);
        assert!(sink.rows.is_empty());
        assert_eq!(result.omitted, 1);
    }
}
