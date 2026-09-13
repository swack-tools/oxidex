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
use super::runtime;
use super::{Fmt, KeyedDirectoryTable, KeyedEdge, KeyedLayout, KeyedTag, find_table};

/// Output family selected by the carrier, rather than a keyed source row.
#[derive(Clone, Copy, Debug)]
pub struct KeyedScope {
    pub group0: &'static str,
    pub group1: &'static str,
    pub group2: &'static str,
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

    fn keyed_enabled(&self, _table: &'static KeyedDirectoryTable) -> bool {
        false
    }
}

/// Counts withheld keyed-reader work. None of these conditions produces output.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyedWalkResult {
    pub entries_seen: usize,
    pub emitted: usize,
    pub gate_a_blocked: usize,
    pub gate_b_blocked: usize,
    pub malformed_directory: usize,
    pub high_bit_error: usize,
    pub duplicate_directory: usize,
    pub depth_limit: usize,
    pub initial_context_refusal: usize,
    pub omitted: usize,
    pub bad_value: usize,
    pub large_scalar: usize,
    pub unwalked_edge: usize,
    pub unavailable_target: usize,
}

/// Read one keyed directory and same-table CIFF subdirectories.
///
/// A named zero-start value block is dispatched only when it names an existing
/// binary table that independently passes Gate A and Gate B. All other target
/// shapes remain explicit refusals.
#[must_use]
pub fn process_keyed_directory(
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
    walk(table, block, ctx, sink, &mut visited, 0, &mut result);
    result
}

const MAX_KEYED_DEPTH: u32 = 8;

fn walk(
    table: &'static KeyedDirectoryTable,
    block: KeyedBlock<'_>,
    ctx: &mut Ctx,
    sink: &mut dyn KeyedEmissionSink,
    visited: &mut Vec<(usize, usize)>,
    depth: u32,
    result: &mut KeyedWalkResult,
) {
    if depth >= MAX_KEYED_DEPTH {
        result.depth_limit += 1;
        return;
    }
    if !matches!(table.layout, KeyedLayout::Ciff10) {
        result.malformed_directory += 1;
        return;
    }
    let Some(directory_offset) = u32_at(
        block.data,
        block.data.len().saturating_sub(4),
        block.byte_order,
    )
    .and_then(|offset| usize::try_from(offset).ok()) else {
        result.malformed_directory += 1;
        return;
    };
    let Some(address) = (block.data.as_ptr() as usize).checked_add(directory_offset) else {
        result.malformed_directory += 1;
        return;
    };
    let table_id = table as *const KeyedDirectoryTable as usize;
    if visited.contains(&(table_id, address)) {
        result.duplicate_directory += 1;
        return;
    }
    visited.push((table_id, address));

    let Some(count) = u16_at(block.data, directory_offset, block.byte_order).map(usize::from)
    else {
        result.malformed_directory += 1;
        return;
    };
    let Some(entries_end) = directory_offset.checked_add(2).and_then(|start| {
        count
            .checked_mul(10)
            .and_then(|bytes| start.checked_add(bytes))
    }) else {
        result.malformed_directory += 1;
        return;
    };
    if entries_end > block.data.len() {
        result.malformed_directory += 1;
        return;
    }

    for index in 0..count {
        let entry = directory_offset + 2 + index * 10;
        let raw_tag = u16_at(block.data, entry, block.byte_order).expect("checked entry bounds");
        if raw_tag & 0x8000 != 0 {
            result.high_bit_error += 1;
            return;
        }
        result.entries_seen += 1;
        let raw_id = raw_tag & 0x3fff;
        let entry_type = (raw_tag >> 8) & 0x38;
        let inline = raw_tag & 0x4000 != 0;
        let payload = &block.data[entry + 2..entry + 10];
        let (value_offset, value_size) = if inline {
            (entry + 2, 8usize)
        } else {
            let Some(size) =
                u32_at(payload, 0, block.byte_order).and_then(|value| usize::try_from(value).ok())
            else {
                result.bad_value += 1;
                continue;
            };
            let Some(offset) =
                u32_at(payload, 4, block.byte_order).and_then(|value| usize::try_from(value).ok())
            else {
                result.bad_value += 1;
                continue;
            };
            (offset, size)
        };

        let resolved = resolve_tag(table, raw_id, ctx, result);
        // CIFF type controls same-parent recursion, including unknown keys.
        if !inline && matches!(entry_type, 0x28 | 0x30) {
            let Some(child) = bounded_block(block, value_offset, value_size) else {
                result.bad_value += 1;
                continue;
            };
            let name = resolved.map_or_else(
                || format!("CanonRaw_0x{raw_id:04x}"),
                |selected| selected.tag.name.to_owned(),
            );
            with_dir_name(ctx, name, |ctx| {
                walk(table, child, ctx, sink, visited, depth + 1, result);
            });
            continue;
        }

        let Some(resolved) = resolved else {
            continue;
        };
        let tag = resolved.tag;
        // A refused Condition or Hook can change whether this row exists or
        // how its bytes are located.  Do not decode it.  The other omitted
        // flags are handled after RawConv: FoundTag stores modeled member
        // state before deciding whether a value is reportable.
        if (tag.omitted.condition && !resolved.condition_resolved) || tag.omitted.hook {
            result.omitted += 1;
            continue;
        }
        if let Some(edge) = tag.edge {
            match edge {
                KeyedEdge::SameTableDirectory => {
                    result.unwalked_edge += 1;
                }
                KeyedEdge::BoundedValue {
                    module,
                    table: target,
                    start,
                    unwalked,
                } => {
                    if !matches!(start, super::KeyedStart::Zero) || !unwalked.is_empty() {
                        result.unwalked_edge += 1;
                        continue;
                    }
                    let Some(child) = bounded_block(block, value_offset, value_size) else {
                        result.bad_value += 1;
                        continue;
                    };
                    let Some(target_table) = find_table(module, target) else {
                        result.unavailable_target += 1;
                        continue;
                    };
                    if !super::is_enabled(target_table) {
                        result.gate_b_blocked += 1;
                        continue;
                    }
                    let mut rows = Vec::new();
                    with_dir_name(ctx, tag.name.to_owned(), |ctx| {
                        engine::process_binary_data(
                            target_table,
                            engine::Dir::whole(child.data, child.byte_order),
                            ctx,
                            &mut rows,
                        );
                    });
                    for row in rows {
                        sink.emit(re_scope(row, child.scope));
                        result.emitted += 1;
                    }
                }
            }
            continue;
        }

        if value_size > block.max_scalar_bytes {
            result.large_scalar += 1;
            continue;
        }
        let Some(value_end) = value_offset.checked_add(value_size) else {
            result.bad_value += 1;
            continue;
        };
        let Some(value) = block.data.get(value_offset..value_end) else {
            result.bad_value += 1;
            continue;
        };
        let format = tag.format.unwrap_or_else(|| default_format(entry_type));
        let count = native_count(tag, format, value_size, inline);
        let Some(raw) = engine::read_value(
            value,
            0,
            format,
            count,
            i64::try_from(value_size).unwrap_or(i64::MAX),
            block.byte_order,
        ) else {
            result.bad_value += 1;
            continue;
        };
        let mut omitted = tag.omitted;
        if resolved.condition_resolved {
            omitted.condition = false;
        }
        match tag.raw_conv {
            Some(super::RawConvEffect::SetMember { member }) => {
                let Some(member_value) = engine::member_value(&raw) else {
                    result.omitted += 1;
                    // An unrepresentable state value could change every
                    // following source Condition.  Match the binary engine's
                    // fail-closed tail behavior rather than leave stale state.
                    return;
                };
                ctx.members.insert(member, member_value);
                omitted.raw_conv = false;
            }
            Some(super::RawConvEffect::ValueLocal) | None => {}
        }
        if omitted.raw_conv && tag.raw_conv.is_none() {
            result.omitted += 1;
            // The source RawConv is unknown and may mutate shared state.
            return;
        }
        if omitted.any() {
            result.omitted += 1;
            continue;
        }
        let Some(converted) = runtime::apply_value_conv(tag.value_conv, &raw) else {
            result.omitted += 1;
            continue;
        };
        let (value, value_conv) = match runtime::render(tag.print_conv, &converted) {
            Some(rendered) => (
                TagValue::String(rendered),
                Some(runtime::to_exiftool_value(&converted)),
            ),
            None => (runtime::to_exiftool_value(&converted), None),
        };
        sink.emit(Emitted {
            module: table.module,
            table: table.table,
            group0: tag.groups.g0.unwrap_or(block.scope.group0),
            group1: tag.groups.g1.unwrap_or(block.scope.group1),
            group2: tag.groups.g2.unwrap_or(block.scope.group2),
            name: tag.name,
            value,
            value_conv,
            low_priority: false,
            avoid: false,
        });
        result.emitted += 1;
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
) -> Option<ResolvedTag> {
    if let Some(tag) = table.tags.iter().find(|tag| tag.raw_id == raw_id) {
        if let Some(condition) = tag.condition {
            if condition.needs_value_context() {
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
    if group
        .alternatives
        .iter()
        .any(|(condition, _)| condition.needs_value_context())
    {
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

fn with_dir_name(ctx: &mut Ctx, value: String, f: impl FnOnce(&mut Ctx)) {
    let old = ctx.members.insert("DIR_NAME", MemberValue::Str(value));
    f(ctx);
    if let Some(old) = old {
        ctx.members.insert("DIR_NAME", old);
    } else {
        ctx.members.remove("DIR_NAME");
    }
}

fn re_scope(row: Emitted, scope: KeyedScope) -> Emitted {
    Emitted {
        group0: scope.group0,
        group1: scope.group1,
        group2: scope.group2,
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

    use super::*;
    use crate::exiftool_tables::{
        Cond, GateA, KeyedNativeFacts, KeyedStart, KeyedVariantGroup, Omitted, PrintConv,
        RawConvEffect, TagGroups,
    };

    const FACTS: KeyedNativeFacts = KeyedNativeFacts {
        format: None,
        count: None,
        condition: None,
        groups: TagGroups::NONE,
        subdir: None,
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
        enabled: bool,
    }

    impl KeyedEmissionSink for Sink {
        fn emit(&mut self, row: Emitted) {
            self.rows.push(row);
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
            group0: "Carrier",
            group1: "Carrier",
            group2: "Other",
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
            assert!(sink.rows.iter().all(|row| row.group0 == "Carrier"));
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
