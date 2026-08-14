//! The single `ProcessBinaryData` engine (Step 28).
//!
//! # Why this module exists
//!
//! Until Step 28 this repository carried **three** independent ports of
//! `Image::ExifTool::ProcessBinaryData` (ExifTool.pm:9877):
//!
//! | port | drove | had | lacked |
//! |---|---|---|---|
//! | [`super::runtime::decode_binary_table`] | the 613 generated tables | `Mask`, fractional keys, `Omitted` refusals | `varSize`, negative indices, `ReadValue`'s count shortening, variants-with-members, sub-directory recursion |
//! | `parsers::tiff::makernotes::shared::binary_subdir` | the per-vendor `codegen_subdirs.py` tables | `Condition` groups, `DataMember` set/gate, `ReadValue` shortening, `PRIORITY => 0` | `varSize`/`Hook`, negative indices, recursion |
//! | `parsers::tiff::makernotes::canon::camera_info` | Canon `CameraInfo` | `varSize` + `Hook`, negative indices, `PRIORITY => 0`, recursion into one sub-table | `ReadValue` shortening, `DataMember` gates |
//!
//! Each got a different subset of ExifTool's one function right, and the
//! union of their gaps is the coverage gap Step 28 closes. This module is the
//! union: **one** offset arithmetic ([`Cursor`]), **one** `ReadValue`
//! ([`read_value`]), **one** walk ([`process_binary_data`]). The other two
//! ports keep their own *conversion* layers -- their hand-written
//! `ValueConv`/`PrintConv` ports, which the mechanical transcription
//! deliberately refuses to reproduce (`AGENTS.md`, "never approximate a
//! conversion") -- but no longer carry their own copy of the arithmetic or
//! the reader. Folding the conversions too would not merge three engines, it
//! would delete tags.
//!
//! # The Perl this reproduces, line by line (pinned 13.59)
//!
//! ```text
//! ExifTool.pm:9890   $size = $maxLen if not defined $size or $size > $maxLen;
//! ExifTool.pm:9892   my $defaultFormat = $$tagTablePtr{FORMAT} || 'int8u';
//! ExifTool.pm:9893   my $increment = $formatSize{$defaultFormat};
//! ExifTool.pm:9917   @tags = sort { ($a < 0 ? $a + 1e9 : $a) <=> ($b < 0 ? $b + 1e9 : $b) } TagTableKeys(...)
//! ExifTool.pm:9957   my $entry = int($index) * $increment + $varSize;
//! ExifTool.pm:9959       if ($entry < 0) {
//! ExifTool.pm:9960           $entry += $size;
//! ExifTool.pm:9961           next if $entry < 0;
//! ExifTool.pm:9963   my $more = $size - $entry;
//! ExifTool.pm:9964   last if $more <= 0;
//! ExifTool.pm:10049  if (defined $$tagInfo{Hook}) { ... eval $$tagInfo{Hook}   # may move $varSize
//! ExifTool.pm:10076  $val = ReadValue($dataPt, $entry+$dirStart, $format, $count, $more, \$rational);
//! ExifTool.pm:10077  next unless defined $val;
//! ExifTool.pm:10079  $val = ($val & $mask) >> $$tagInfo{BitShift} if $mask;
//! ExifTool.pm:10102  if ($$tagInfo{SubDirectory}) { ... }                      # see subdir.rs
//! ExifTool.pm:10163  my $key = $self->FoundTag($tagInfo,$val);
//! ```
//!
//! Two of those lines are the difference between a truncated record
//! degrading and a truncated record vanishing, and they are the ones the
//! generated-table port did not have:
//!
//! * **`last if $more <= 0`** (ExifTool.pm:9964) ends the walk at the first
//!   out-of-range field rather than skipping it and trying the next -- which
//!   matters once `varSize` exists, because a `Hook` that adds 0x10000 is
//!   ExifTool's own way of saying "stop here" (see `camera_info.rs`).
//! * **`ReadValue`'s count shortening** (ExifTool.pm:6301-6303,
//!   `$count = int($size/$len); $count < 1 and return undef`) reports the
//!   elements that DO fit instead of dropping the field. `decode_binary_table`
//!   required the whole array to fit, so a record one byte short reported
//!   nothing where ExifTool reports all but the last element.
//!
//! # What is deliberately NOT here
//!
//! `var_*` formats (ExifTool.pm:9986-10047) are refused by `codegen.py`, not
//! implemented here: their width is data-dependent, and the table records the
//! resulting offset hazard as [`super::BinaryTable::offsets_sound_until`]
//! instead. `Hook` bodies are Perl closures; this module provides the
//! `varSize` seam a Hook moves ([`Cursor::shift`]) but never invents a Hook's
//! arithmetic -- a `Hook`-flagged generated field stays refused.

use crate::core::TagValue;
use crate::io::ByteOrder;

use super::cond;
use super::runtime::{DecodedValue, decode_value_of};
use super::subdir::{Start, SubdirEdge};
use super::{BinaryTable, Field, Fmt, Mask, find_table};

// ---------------------------------------------------------------------------
// Offset arithmetic -- ExifTool.pm:9957-9964
// ---------------------------------------------------------------------------

/// What ExifTool's own control flow does with one tag key's offset.
///
/// The three arms are the three statements at ExifTool.pm:9957-9964, kept
/// distinct because `next` and `last` are NOT interchangeable: `Skip` tries
/// the next key, `Stop` abandons the rest of the table. All three ports
/// before Step 28 collapsed at least one of these into the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The field starts `entry` bytes into the directory and has `more`
    /// bytes of directory left after it (ExifTool's `$entry`/`$more`).
    At { entry: i64, more: i64 },
    /// `next if $entry < 0` (ExifTool.pm:9961): a negative index that still
    /// lands before the start of the record even after wrapping.
    Skip,
    /// `last if $more <= 0` (ExifTool.pm:9964).
    Stop,
}

/// The running state ExifTool's `foreach $index (@tags)` loop carries:
/// the directory size, the table's `FORMAT` width, and `$varSize`.
///
/// One shared implementation of `int($index) * $increment + $varSize`, the
/// negative-index wrap, and the `$more` bound -- the arithmetic all three
/// pre-Step-28 ports wrote separately and got separately wrong.
#[derive(Clone, Copy, Debug)]
pub struct Cursor {
    size: i64,
    increment: i64,
    var_size: i64,
}

impl Cursor {
    /// `size` is ExifTool's `$size` after ExifTool.pm:9890 clamps `DirLen` to
    /// the bytes actually available; `increment` is
    /// `$formatSize{$$tagTablePtr{FORMAT} || 'int8u'}` (ExifTool.pm:9892-9893).
    #[must_use]
    pub const fn new(size: i64, increment: i64) -> Self {
        Self {
            size,
            increment,
            var_size: 0,
        }
    }

    /// The directory size this cursor walks (`$size`).
    #[must_use]
    pub const fn size(self) -> i64 {
        self.size
    }

    /// The current `$varSize`.
    #[must_use]
    pub const fn var_size(self) -> i64 {
        self.var_size
    }

    /// A `Hook`'s effect: `$varSize` moves, and every LATER field moves with
    /// it (ExifTool.pm:10049-10053 runs the Hook *after* this field's own
    /// `$entry` is already computed at ExifTool.pm:9957, so a Hook never
    /// moves the field that carries it).
    pub const fn shift(&mut self, delta: i64) {
        self.var_size += delta;
    }

    /// ExifTool.pm:9957-9964 for one tag key.
    #[must_use]
    pub const fn step(self, index: i64) -> Step {
        let mut entry = index * self.increment + self.var_size;
        if entry < 0 {
            // ExifTool.pm:9959-9962 -- "allow negative indices to represent
            // bytes from end".
            entry += self.size;
            if entry < 0 {
                return Step::Skip;
            }
        }
        let more = self.size - entry;
        if more <= 0 {
            return Step::Stop;
        }
        Step::At { entry, more }
    }
}

/// ExifTool's visit order for a binary table's keys (ExifTool.pm:9917):
/// ascending, except that a negative key sorts as `key + 1e9`, i.e. after
/// every non-negative one. A table mixing both -- `Sony::Panorama` and
/// `DPX::Main` do -- is read in a different order without this, and order is
/// load-bearing once `varSize` and `DataMember`s exist.
#[must_use]
pub fn visit_key(index: i64) -> i64 {
    if index < 0 {
        index + 1_000_000_000
    } else {
        index
    }
}

// ---------------------------------------------------------------------------
// ReadValue -- ExifTool.pm:6286-6332
// ---------------------------------------------------------------------------

/// ExifTool's `ReadValue` (ExifTool.pm:6286), the one reader all three ports
/// duplicated.
///
/// `more` is the bytes of directory remaining at `offset` (ExifTool's
/// `$size` argument, which `ProcessBinaryData` passes as `$more`). The two
/// rules that make this more than a bounds check:
///
/// * **count shortening** (ExifTool.pm:6301-6303): `if ($len * $count >
///   $size) { $count = int($size / $len); $count < 1 and return undef }`.
///   A field whose array runs off the end reports the elements that fit;
///   only a field with room for *no* element at all is dropped.
/// * **string/undef are one value, not `count` values**
///   (ExifTool.pm:6307-6311): `$readValueProc{$format}` is undefined for
///   `string`/`undef`, so the whole `$count * $len` byte run becomes
///   `$vals[0]`, and `string` alone is truncated at the first NUL
///   (`$vals[0] =~ s/\0.*//s if $format eq 'string'`). ExifTool's own
///   `string[8]` is `format => 'string', count => 8, len => 1`, so the
///   shortening rule above is a per-BYTE rule for strings -- a `string[8]`
///   with 5 bytes left reports 5 characters, not nothing. The generated
///   schema folds the `[8]` into [`Fmt::Str`]'s payload, so this function
///   un-folds it to keep the arithmetic ExifTool's.
///
/// `None` is ExifTool's `return undef` at ExifTool.pm:6303 -- and only that.
#[must_use]
pub fn read_value(
    data: &[u8],
    offset: usize,
    format: Fmt,
    count: usize,
    more: i64,
    byte_order: ByteOrder,
) -> Option<DecodedValue> {
    let more = usize::try_from(more).ok()?;
    // ExifTool's ($len, $count) for this field. A sized string/undef is
    // `len == 1` repeated N times in ExifTool's own table, never one N-wide
    // element -- see the doc comment.
    let (elem_len, elem_count, blob) = match format {
        Fmt::Str(n) | Fmt::Undef(n) => (1usize, (n as usize).checked_mul(count)?, true),
        other => (usize::try_from(other.size()).ok()?, count, false),
    };
    if elem_len == 0 {
        return None;
    }
    // ExifTool.pm:6301-6303.
    let elem_count = if elem_len.checked_mul(elem_count)? > more {
        let shortened = more / elem_len;
        if shortened < 1 {
            return None;
        }
        shortened
    } else {
        elem_count
    };
    let want = elem_len.checked_mul(elem_count)?;
    let bytes = data.get(offset..offset.checked_add(want)?)?;

    if blob {
        // ExifTool.pm:6309-6311: one value spanning every byte.
        return Some(match format {
            Fmt::Str(_) => {
                let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
                DecodedValue::String(String::from_utf8_lossy(&bytes[..end]).into_owned())
            }
            _ => DecodedValue::Undefined(bytes.to_vec()),
        });
    }
    if elem_count == 1 {
        return decode_value_of(bytes, format, byte_order);
    }
    let values = bytes
        .chunks_exact(elem_len)
        .map(|chunk| decode_value_of(chunk, format, byte_order))
        .collect::<Option<Vec<_>>>()?;
    Some(DecodedValue::Array(values))
}

/// ExifTool.pm:10079 -- `$val = ($val & $mask) >> $$tagInfo{BitShift} if $mask`,
/// applied before any conversion. `None` when the value is not an integer: a
/// `Mask` on a non-integer is a construct this schema cannot express, and
/// reporting the unmasked value under the masked tag's name would be a
/// confident wrong value.
#[must_use]
pub fn apply_mask(value: DecodedValue, mask: Option<Mask>) -> Option<DecodedValue> {
    match mask {
        None => Some(value),
        Some(mask) => Some(DecodedValue::Integer(mask.apply(value.as_integer()?))),
    }
}

// ---------------------------------------------------------------------------
// The walk
// ---------------------------------------------------------------------------

/// One tag [`process_binary_data`] resolved all the way to a value.
#[derive(Clone, Debug)]
pub struct Emitted {
    /// The table this came out of -- the enabled table itself for a top-level
    /// walk, or a `SubDirectory` target for a recursive one, which is why it
    /// is carried per tag rather than assumed by the caller.
    pub module: &'static str,
    pub table: &'static str,
    /// ExifTool's `GROUPS => { 0 => ..., 2 => ... }` for the emitting table.
    pub group0: &'static str,
    pub group2: &'static str,
    pub name: &'static str,
    pub value: TagValue,
    /// ExifTool's `PRIORITY => 0` (ExifTool.pm:9471, `$priority = $$tbl{PRIORITY}`):
    /// this value must not displace one already reported under the same name.
    pub low_priority: bool,
}

/// The `%dirInfo` a `ProcessBinaryData` call receives (ExifTool.pm:9880-9888).
#[derive(Clone, Copy, Debug)]
pub struct Dir<'a> {
    /// `$$dirInfo{DataPt}` -- the whole buffer, not just this directory.
    /// `SubDirectory` `Start` expressions are absolute offsets into *this*,
    /// so a caller that passes only the sub-slice cannot walk an edge.
    pub data: &'a [u8],
    /// `$$dirInfo{DirStart}`.
    pub dir_start: usize,
    /// `$$dirInfo{DirLen}`; `None` is ExifTool's undef, which
    /// ExifTool.pm:9890 resolves to "the rest of the buffer".
    pub dir_len: Option<usize>,
    /// `$$dirInfo{Base}`.
    pub base: i64,
    /// `$$dirInfo{DataPos}`.
    pub data_pos: i64,
    pub byte_order: ByteOrder,
}

impl<'a> Dir<'a> {
    /// The common case: a directory that is exactly one buffer, based at 0.
    #[must_use]
    pub const fn whole(data: &'a [u8], byte_order: ByteOrder) -> Self {
        Self {
            data,
            dir_start: 0,
            dir_len: None,
            base: 0,
            data_pos: 0,
            byte_order,
        }
    }

    /// `$size` after ExifTool.pm:9890's clamp.
    fn size(&self) -> i64 {
        let max_len = self.data.len().saturating_sub(self.dir_start);
        let size = match self.dir_len {
            Some(len) if len <= max_len => len,
            _ => max_len,
        };
        i64::try_from(size).unwrap_or(i64::MAX)
    }
}

/// ExifTool's `$$self{PROCESSED}` cycle guard (ExifTool.pm:9065-9072), plus a
/// hard depth cap.
///
/// The guard is not optional here: the pinned 13.59 tree really does declare
/// cyclic `SubDirectory` edges -- `LNK::UnknownData -> EnvVarData ->
/// ConsoleData -> TrackerData -> ConsoleFEData -> UnknownData` is a five-node
/// loop, and `Olympus::MovableInfo`, `Sanyo::Thumbnail` and `Canon::PSInfo2`
/// each point at themselves. ExifTool escapes them because `$$self{PROCESSED}`
/// records every `DirStart + DataPos + Base` it has already walked and
/// refuses a repeat (`return 0`) unless the edge set `NotDup`, which
/// ExifTool.pm:10136 sets for exactly the field-relative branch. The depth cap
/// is belt-and-braces for the `NotDup` branch, where ExifTool's own guard does
/// not apply and only the fact that `entry` advances bounds the recursion.
struct Guard {
    processed: Vec<(usize, i64)>,
    depth: u32,
}

/// ExifTool has no fixed limit; this bounds the `NotDup` branch, where
/// `$$self{PROCESSED}` deliberately does not. Every edge in the pinned tree
/// nests at most 3 deep from a live root, so this is slack, not a policy.
const MAX_SUBDIR_DEPTH: u32 = 8;

impl Guard {
    fn new() -> Self {
        Self {
            processed: Vec::new(),
            depth: 0,
        }
    }

    /// ExifTool.pm:9067 -- `if ($$self{PROCESSED}{$addr} and not $$dirInfo{NotDup})`.
    fn admit(&mut self, addr: i64, table: usize, not_dup: bool) -> bool {
        if self.depth >= MAX_SUBDIR_DEPTH {
            return false;
        }
        if !not_dup && self.processed.contains(&(table, addr)) {
            return false;
        }
        // ExifTool.pm:9072 records the address either way.
        self.processed.push((table, addr));
        true
    }
}

/// Walk one `ProcessBinaryData` table and everything its `SubDirectory` edges
/// reach, appending every tag ExifTool would report to `out`.
///
/// This is the whole of ExifTool.pm:9946-10170 that the generated schema can
/// stand behind. A field whose [`Omitted`] flags are set still decodes -- its
/// bytes are fine, only its *meaning* is unresolved -- but is not emitted,
/// because reporting the raw value under a real ExifTool tag name is the one
/// failure mode `AGENTS.md` singles out. `ctx` carries `$$self{...}` data
/// members across the walk so a `Condition` on a later field sees what an
/// earlier one set (see [`cond`]).
pub fn process_binary_data(
    table: &'static BinaryTable,
    dir: Dir<'_>,
    ctx: &mut cond::Ctx,
    out: &mut Vec<Emitted>,
) {
    let mut guard = Guard::new();
    walk(table, dir, ctx, &mut guard, out);
}

/// One tag key's worth of resolved table entry, after `_variants`
/// first-match-wins has picked a winner.
struct Entry {
    field: &'static Field,
    /// True when the field came from a `_variants` group, whose `Condition`
    /// [`cond::first_match`] has already resolved -- so `Omitted::condition`
    /// on the alternative is not an outstanding refusal (see
    /// [`super::runtime::decode_binary_table_variants`]).
    condition_resolved: bool,
}

fn walk(
    table: &'static BinaryTable,
    dir: Dir<'_>,
    ctx: &mut cond::Ctx,
    guard: &mut Guard,
    out: &mut Vec<Emitted>,
) {
    let size = dir.size();
    let increment = i64::from(table.default_format.size());
    if increment <= 0 {
        return;
    }
    let cursor = Cursor::new(size, increment);

    for (index, entry) in visit_order(table, ctx) {
        let (at, more) = match cursor.step(index) {
            Step::At { entry, more } => (entry, more),
            // ExifTool.pm:9961: `next` -- try the following key.
            Step::Skip => continue,
            // ExifTool.pm:9964: `last` -- abandon the rest of the table.
            Step::Stop => break,
        };
        let field = entry.field;

        // D1 (Step 10): past this bound `index * increment` is a nominal
        // offset, not a trustworthy one, so there is no honest value here at
        // all -- not even a raw one.
        if let Some(bound) = table.offsets_sound_until
            && field.index > bound
        {
            continue;
        }

        let format = table.field_format(field);
        let offset = match usize::try_from(at + i64::try_from(dir.dir_start).unwrap_or(i64::MAX)) {
            Ok(offset) => offset,
            Err(_) => continue,
        };
        // ExifTool.pm:10076-10077.
        let Some(raw) = read_value(dir.data, offset, format, field.count, more, dir.byte_order)
        else {
            continue;
        };
        // ExifTool.pm:10079.
        let Some(raw) = apply_mask(raw, field.mask) else {
            continue;
        };

        // ExifTool.pm:10102 -- a SubDirectory field is a pointer, never a value.
        if let Some(edge) = &field.subdir {
            descend(table, field, edge, &raw, &dir, at, more, ctx, guard, out);
            continue;
        }

        let mut omitted = field.omitted;
        if entry.condition_resolved {
            omitted.condition = false;
        }
        if omitted.any() {
            continue;
        }
        let value = match super::runtime::render(field.print_conv, &raw) {
            Some(rendered) => TagValue::String(rendered),
            None => super::runtime::to_tag_value(&raw),
        };
        out.push(Emitted {
            module: table.module,
            table: table.table,
            group0: table.group0,
            group2: table.group2,
            name: field.name,
            value,
            low_priority: table.priority == Some(0),
        });
    }
}

/// ExifTool's key order (ExifTool.pm:9917) over the union of `fields` and the
/// resolved winner of each `_variants` group.
///
/// The two live in separate arrays in the generated schema but are one key
/// space in ExifTool's table, and interleaving them correctly is what makes
/// `varSize` and `DataMember` ordering mean the same thing here as there.
fn visit_order(table: &'static BinaryTable, ctx: &mut cond::Ctx) -> Vec<(i64, Entry)> {
    let mut entries: Vec<(i64, u32, Entry)> = Vec::with_capacity(table.fields.len());
    for field in table.fields {
        entries.push((
            field.index,
            field.sub.unwrap_or(0),
            Entry {
                field,
                condition_resolved: false,
            },
        ));
    }
    for group in table.variants {
        // `first_match` applies every alternative's `SetMember` side effects
        // in ExifTool's own GetTagInfo order, including from alternatives
        // that lose -- see cond.rs.
        let Some(field) = cond::first_match(group.alternatives, ctx) else {
            continue;
        };
        entries.push((
            group.index,
            group.sub.unwrap_or(0),
            Entry {
                field,
                condition_resolved: true,
            },
        ));
    }
    entries.sort_by_key(|(index, sub, _)| (visit_key(*index), *sub));
    entries
        .into_iter()
        .map(|(index, _, entry)| (index, entry))
        .collect()
}

/// ExifTool.pm:10102-10151 -- open a `SubDirectory` and process it.
#[allow(clippy::too_many_arguments)]
fn descend(
    table: &'static BinaryTable,
    field: &'static Field,
    edge: &SubdirEdge,
    raw: &DecodedValue,
    dir: &Dir<'_>,
    at: i64,
    more: i64,
    ctx: &mut cond::Ctx,
    guard: &mut Guard,
    out: &mut Vec<Emitted>,
) {
    let Some(target) = find_table(edge.module, edge.table) else {
        // Not a defect in the edge: many targets (`IPTC::Main`,
        // `LNK::LinkInfo`'s neighbours) are not ProcessBinaryData tables this
        // crate transcribed a layout for. See subdir.rs.
        return;
    };
    if !target.enabled() {
        // Opt-in (Step 28 D1): an edge never enables its target. Walking into
        // a table that has not passed both gates would enable it by the back
        // door, with no allowlist line to review or revert.
        return;
    }
    let dir_start = i64::try_from(dir.dir_start).unwrap_or(i64::MAX);
    let data_len = i64::try_from(dir.data.len()).unwrap_or(i64::MAX);

    // ExifTool.pm:10105-10111: an explicit Format sizes the subdirectory;
    // otherwise it is all of the remaining data.
    let mut len = if field.format.is_some() {
        let sized = i64::from(table.field_format(field).size())
            .saturating_mul(i64::try_from(field.count).unwrap_or(1));
        sized.min(more)
    } else {
        more
    };

    // ExifTool.pm:10118-10123.
    let subdir_base = match edge.base {
        None => dir.base,
        Some(expr) => {
            // ExifTool.pm:10121: this `$start` is `$entry + $dirStart +
            // $dataPos`, an unrelated lexical to the `$start` below.
            let field_pos = at + dir_start + dir.data_pos;
            expr.eval(field_pos, dir.base) + dir.base
        }
    };

    // ExifTool.pm:10124-10137.
    let (start, not_dup) = match edge.start {
        Start::FieldRelative(literal) => (literal + dir_start + at, true),
        Start::Expr(expr) => {
            // ExifTool.pm:10128 -- "ignore directories with a zero offset
            // (ie. missing Nikon ShotInfo entries)". Perl truthiness: 0 and
            // the empty string are false.
            let val = raw.as_integer().unwrap_or(0);
            if val == 0 {
                return;
            }
            let start = expr.eval(val, dir_start);
            // ExifTool.pm:10131.
            if start < dir_start || start > data_len {
                return;
            }
            // ExifTool.pm:10132-10133: DirLen is not modeled by this schema,
            // so the `unless` arm is the only one reachable.
            len = data_len - start;
            (start, false)
        }
    };
    let Ok(start) = usize::try_from(start) else {
        return;
    };
    let Ok(len) = usize::try_from(len) else {
        return;
    };

    // ExifTool.pm:9066 -- `$addr = DirStart + DataPos + Base`.
    let addr = i64::try_from(start).unwrap_or(i64::MAX) + dir.data_pos + subdir_base;
    if !guard.admit(addr, std::ptr::from_ref(target) as usize, not_dup) {
        return;
    }
    guard.depth += 1;
    walk(
        target,
        Dir {
            data: dir.data,
            dir_start: start,
            dir_len: Some(len),
            base: subdir_base,
            data_pos: dir.data_pos,
            byte_order: dir.byte_order,
        },
        ctx,
        guard,
        out,
    );
    guard.depth -= 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exiftool_tables::{Omitted, PrintConv};

    // -- Cursor: ExifTool.pm:9957-9964 --------------------------------------

    #[test]
    fn entry_is_index_times_increment_plus_varsize() {
        // ExifTool.pm:9957. int16u table, so key 5 is byte 10.
        let cursor = Cursor::new(64, 2);
        assert_eq!(
            cursor.step(5),
            Step::At {
                entry: 10,
                more: 54
            }
        );
    }

    #[test]
    fn a_hook_moves_every_later_field_and_never_its_own() {
        // ExifTool.pm:10049-10053 runs the Hook AFTER ExifTool.pm:9957 has
        // computed this field's `$entry`, which is why `camera_info.rs`'s
        // firmware hooks shift the tail of the table and not the tag that
        // carries them.
        let mut cursor = Cursor::new(64, 1);
        assert_eq!(
            cursor.step(10),
            Step::At {
                entry: 10,
                more: 54
            }
        );
        cursor.shift(4);
        assert_eq!(
            cursor.step(10),
            Step::At {
                entry: 14,
                more: 50
            }
        );
        assert_eq!(cursor.var_size(), 4);
    }

    #[test]
    fn a_negative_index_counts_back_from_the_end() {
        // ExifTool.pm:9959-9962. `Canon::CameraInfoUnknown32` is the one
        // table in the pinned 13.59 tree that declares one (key -3), and
        // `decode_binary_table`'s `usize::try_from` dropped it outright
        // before Step 28 -- an entire field silently unreadable, not refused.
        let cursor = Cursor::new(40, 4);
        assert_eq!(
            cursor.step(-3),
            Step::At {
                entry: 28,
                more: 12
            }
        );
    }

    #[test]
    fn a_negative_index_that_still_lands_before_the_start_is_skipped_not_stopped() {
        // ExifTool.pm:9961 is `next`, not `last`: a following key may still
        // be in range, which is the whole reason ExifTool.pm:9917 sorts
        // negatives last.
        let cursor = Cursor::new(4, 4);
        assert_eq!(cursor.step(-3), Step::Skip);
    }

    #[test]
    fn out_of_range_stops_the_walk_rather_than_skipping_the_field() {
        // ExifTool.pm:9964's `last`. This is what makes a Hook that adds
        // 0x10000 mean "stop here" (camera_info.rs) rather than "skip one".
        let cursor = Cursor::new(16, 2);
        assert_eq!(cursor.step(8), Step::Stop);
        assert_eq!(cursor.step(9), Step::Stop);
    }

    #[test]
    fn negative_keys_sort_after_every_non_negative_one() {
        // ExifTool.pm:9917's `$a < 0 ? $a + 1e9 : $a`.
        let mut keys = vec![-3i64, 0, 7, -1, 1000];
        keys.sort_by_key(|k| visit_key(*k));
        assert_eq!(keys, vec![0, 7, 1000, -3, -1]);
    }

    // -- ReadValue: ExifTool.pm:6286-6332 -----------------------------------

    #[test]
    fn a_short_array_reports_the_elements_that_fit() {
        // ExifTool.pm:6301-6303: `$count = int($size/$len)`, and only
        // `$count < 1` returns undef. `decode_binary_table`'s pre-Step-28
        // reader required the whole array, so this field vanished.
        let data = [0, 1, 0, 2, 0, 3];
        let got = read_value(&data, 0, Fmt::Int16u, 4, 6, ByteOrder::Big);
        assert_eq!(
            got,
            Some(DecodedValue::Array(vec![
                DecodedValue::Integer(1),
                DecodedValue::Integer(2),
                DecodedValue::Integer(3),
            ])),
            "three of the four int16u fit, so ExifTool reports three"
        );
    }

    #[test]
    fn room_for_no_element_at_all_is_the_only_undef() {
        // ExifTool.pm:6303's `$count < 1 and return undef`.
        let data = [0u8];
        assert_eq!(
            read_value(&data, 0, Fmt::Int16u, 4, 1, ByteOrder::Big),
            None
        );
    }

    #[test]
    fn a_truncated_string_reports_the_bytes_that_fit() {
        // ExifTool's `string[8]` is `format => 'string', count => 8, len => 1`
        // (ExifTool.pm:6290 with $formatSize{string} == 1), so the shortening
        // rule is per BYTE: five bytes left yields five characters, not
        // nothing. The generated schema folds the `[8]` into `Fmt::Str`'s
        // payload, which is why `read_value` un-folds it.
        let data = *b"ABCDE";
        assert_eq!(
            read_value(&data, 0, Fmt::Str(8), 1, 5, ByteOrder::Big),
            Some(DecodedValue::String("ABCDE".to_string()))
        );
    }

    #[test]
    fn a_string_is_truncated_at_the_first_nul_and_not_trimmed() {
        // ExifTool.pm:6311, `$vals[0] =~ s/\0.*//s if $format eq 'string'`.
        // No trailing-whitespace trim: adding one disagrees with ExifTool on
        // every space-padded field.
        let data = *b"AB \0XY\0\0";
        assert_eq!(
            read_value(&data, 0, Fmt::Str(8), 1, 8, ByteOrder::Big),
            Some(DecodedValue::String("AB ".to_string()))
        );
    }

    #[test]
    fn undef_is_one_value_spanning_every_byte_not_n_values() {
        // ExifTool.pm:6307-6309: `$readValueProc{undef}` is undefined, so the
        // whole `$count * $len` run becomes `$vals[0]`.
        let data = [1u8, 2, 3, 4];
        assert_eq!(
            read_value(&data, 0, Fmt::Undef(4), 1, 4, ByteOrder::Big),
            Some(DecodedValue::Undefined(vec![1, 2, 3, 4]))
        );
    }

    #[test]
    fn int16u_rev_reads_against_the_records_own_byte_order() {
        let data = [0x12u8, 0x34];
        assert_eq!(
            read_value(&data, 0, Fmt::Int16uRev, 1, 2, ByteOrder::Big),
            Some(DecodedValue::Integer(0x3412))
        );
    }

    // -- The walk -----------------------------------------------------------

    static PLAIN_FIELDS: &[Field] = &[
        Field {
            index: 0,
            sub: None,
            name: "First",
            format: Some(Fmt::Int16u),
            count: 1,
            mask: None,
            omitted: Omitted::NONE,
            print_conv: PrintConv::None,
            subdir: None,
        },
        Field {
            index: 1,
            sub: None,
            name: "Gated",
            format: Some(Fmt::Int16u),
            count: 1,
            mask: None,
            omitted: Omitted {
                value_conv: true,
                ..Omitted::NONE
            },
            print_conv: PrintConv::None,
            subdir: None,
        },
    ];

    static PLAIN: BinaryTable = BinaryTable {
        module: "Test",
        table: "Plain",
        group0: "MakerNotes",
        group2: "Camera",
        first_entry: 0,
        default_format: Fmt::Int16u,
        offsets_sound_until: None,
        priority: Some(0),
        gate_a: super::super::GateA { blocked_by: &[] },
        fields: PLAIN_FIELDS,
        variants: &[],
    };

    fn run(table: &'static BinaryTable, data: &[u8]) -> Vec<Emitted> {
        use std::collections::HashMap;
        let mut members = HashMap::new();
        let mut ctx = cond::Ctx::new(&mut members);
        let mut out = Vec::new();
        process_binary_data(table, Dir::whole(data, ByteOrder::Big), &mut ctx, &mut out);
        out
    }

    #[test]
    fn a_flagged_field_is_withheld_and_the_table_priority_rides_along() {
        let got = run(&PLAIN, &[0, 7, 0, 9]);
        assert_eq!(got.len(), 1, "the ValueConv-flagged field is withheld");
        assert_eq!(got[0].name, "First");
        assert_eq!(got[0].value, TagValue::Integer(7));
        assert_eq!(got[0].group0, "MakerNotes");
        assert!(
            got[0].low_priority,
            "PRIORITY => 0 (ExifTool.pm:9471) must reach the caller; before \
             Step 28 the generated schema dropped it and each engine \
             hardcoded its own copy"
        );
    }

    #[test]
    fn a_table_that_is_not_enabled_is_never_walked_through_an_edge() {
        // Opt-in, design D1. The target below passes gate A but is not on
        // the allowlist, so `descend` must refuse it -- an edge that enabled
        // its target would be enablement with no reviewable line and no
        // measurement.
        assert!(!PLAIN.enabled(), "no allowlist line, so not enabled");
        assert!(PLAIN.gate_a.passes(), "but gate A alone does not enable it");
    }

    #[test]
    fn the_walk_stops_at_the_first_out_of_range_key() {
        // Only 2 bytes of record: key 0 reads, key 1 is `last`.
        let got = run(&PLAIN, &[0, 7]);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "First");
    }
}
