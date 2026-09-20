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
        // ProcessBinaryData assigns `$count = $more` for a bare per-field
        // `Format => 'string'` before ReadValue. This is not `Fmt::Str(0)`,
        // whose zero has IFD entry-length semantics.
        Fmt::RemainderString => (1usize, more, true),
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
            Fmt::Str(_) | Fmt::RemainderString => {
                let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
                // RawConv receives this byte scalar before FoundTag's output
                // path runs FixUTF8 (ExifTool.pm:9484-9505). Keep it byte
                // exact so a later member regex observes native state.
                DecodedValue::StringBytes(bytes[..end].to_vec())
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
    /// The family-1 group ExifTool's `GetGroup` (ExifTool.pm:3810-3860)
    /// would report: for a binary table the field's own `Groups{1}` else the
    /// table's effective group 1 ([`BinaryTable::effective_groups`]); for an
    /// IFD table see [`super::ifd_engine`]'s precedence (a `SET_GROUP1` table
    /// reports its directory name, Exif.pm:7183).
    pub group1: &'static str,
    pub group2: &'static str,
    pub name: &'static str,
    pub value: TagValue,
    /// The value ExifTool's `-n` reports -- the `ValueConv` result before
    /// `PrintConv` (ExifTool.pm:3477: `GetValue` stops at `ValueConv` when
    /// the `PrintConv` option is off) -- when a `PrintConv` rendered `value`;
    /// `None` when `value` already is that unconverted form. A caller that
    /// records rows with `insert_occurrence_with_raw` hands this to
    /// `--no-print-conv`, which otherwise has only the printed string.
    pub value_conv: Option<TagValue>,
    /// ExifTool's `PRIORITY => 0` (ExifTool.pm:9471, `$priority = $$tbl{PRIORITY}`):
    /// this value must not displace one already reported under the same name.
    pub low_priority: bool,
    /// ExifTool's per-tag `Avoid => 1` (ExifTool.pm:9472: a tag with no
    /// priority of its own and no table `PRIORITY` defaults to priority 0
    /// when it carries `Avoid`). Binary-table fields never carry it; IFD tags
    /// do ([`super::ifd_schema::IfdFlags::avoid`]).
    pub avoid: bool,
    /// The fraction a single rational entry was read as, before
    /// `RoundFloat` -- ExifTool's `TAG_EXTRA{Rational}` (ExifTool.pm:6312-6320,
    /// Exif.pm:7185), which `Canon::CalcSensorDiag` reads for its sensor
    /// size (Canon.pm:10145-10175) -- set only when `value` IS that number
    /// unconverted (no `ValueConv`, no `PrintConv`), so a caller may keep
    /// the fraction as the row's `-n` form without changing what it prints.
    /// IFD tables only; `None` for every binary-table field.
    pub rational: Option<(i64, i64)>,
}

/// The `%dirInfo` a `ProcessBinaryData` call receives (ExifTool.pm:9880-9888).
#[derive(Clone, Copy, Debug)]
pub struct Dir<'a> {
    /// `$$dirInfo{DataPt}` -- the whole buffer, not just this directory.
    /// `SubDirectory` `Start` expressions are absolute offsets into *this*,
    /// so a caller that passes only the sub-slice cannot walk an edge.
    pub data: &'a [u8],
    /// Stable identity of the enclosing file/data domain.
    /// This is independent of `base`/`data_pos`, which correct stored offsets.
    pub data_domain: u64,
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
            data_domain: 0,
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
///
/// Shared with [`super::ifd_engine`]: `ProcessDirectory` keeps ONE
/// `$$self{PROCESSED}` per file whichever `PROCESS_PROC` a directory uses, so
/// an IFD walk that descends into a `ProcessBinaryData` table hands its
/// guard down rather than starting a fresh one -- otherwise the depth cap
/// would restart at every engine boundary.
#[derive(Clone, Debug)]
pub(super) struct Guard {
    processed: Vec<(usize, u64, i64)>,
    pub(super) depth: u32,
}

/// ExifTool has no fixed limit; this bounds the `NotDup` branch, where
/// `$$self{PROCESSED}` deliberately does not. Every edge in the pinned tree
/// nests at most 3 deep from a live root, so this is slack, not a policy.
pub(super) const MAX_SUBDIR_DEPTH: u32 = 8;

impl Guard {
    pub(super) fn new() -> Self {
        Self {
            processed: Vec::new(),
            depth: 0,
        }
    }

    /// ExifTool.pm:9067 -- `if ($$self{PROCESSED}{$addr} and not $$dirInfo{NotDup})`.
    pub(super) fn admit(
        &mut self,
        data_domain: u64,
        addr: i64,
        table: usize,
        not_dup: bool,
    ) -> bool {
        if self.depth >= MAX_SUBDIR_DEPTH {
            return false;
        }
        if !not_dup && self.processed.contains(&(table, data_domain, addr)) {
            return false;
        }
        // ExifTool.pm:9072 records the address either way.
        self.processed.push((table, data_domain, addr));
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
    // ProcessDirectory resumes its caller after a child ProcessBinaryData
    // table stops. Preserve that legacy public behavior for ICC, H264, and
    // every existing direct caller.
    let _ = walk_with_policy(table, dir, ctx, &mut guard, out, ChildTaintPolicy::Contain);
}

/// Process a binary table and expose whether an unmodeled stateful operation
/// made its remaining sibling work unsafe. The keyed reader deliberately
/// chooses propagation because its own pending frames may evaluate Conditions
/// against the child's shared member state.
pub(crate) fn process_binary_data_checked(
    table: &'static BinaryTable,
    dir: Dir<'_>,
    ctx: &mut cond::Ctx,
    out: &mut Vec<Emitted>,
) -> BinaryWalkOutcome {
    let mut guard = Guard::new();
    walk_with_policy(
        table,
        dir,
        ctx,
        &mut guard,
        out,
        ChildTaintPolicy::Propagate,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinaryWalkOutcome {
    Complete,
    Tainted,
}

/// Whether a child table's locally unsafe tail is also unsafe for its caller.
/// `ProcessDirectory` contains that stop for legacy direct callers; the keyed
/// reader opts in to propagation because it has deferred parent frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChildTaintPolicy {
    Contain,
    Propagate,
}

/// One raw table candidate, kept unresolved until the walk reaches its native
/// key. Resolving all variant groups before sorting would let a condition at a
/// later key observe state that ExifTool has not stored yet.
#[derive(Clone, Copy)]
enum Candidate {
    Field(&'static Field),
    Variants(&'static cond::VariantGroup),
}

/// `GetTagInfo`'s first lookup result before `ProcessBinaryData` knows the
/// final `entry`/`more` bounds. `NeedsValue` is its defined-false sentinel:
/// only a Condition mentioning `$valPt`, `$format`, or `$count` takes the
/// retry path with a <=128-byte value pointer (ExifTool.pm:9155-9188,
/// 9946-9964).
enum Lookup {
    Selected(Entry),
    NeedsValue,
    Skipped,
}

/// One tag key's worth of resolved table entry.
struct Entry {
    field: &'static Field,
    /// True when the field came from a `_variants` group, whose `Condition`
    /// [`cond::first_match`] has already resolved -- so `Omitted::condition`
    /// on the alternative is not an outstanding refusal (see
    /// [`super::runtime::decode_binary_table_variants`]).
    condition_resolved: bool,
}

/// [`process_binary_data`] with a caller-supplied [`Guard`] -- the entry
/// [`super::ifd_engine`] uses when a `ProcessExif` table's `SubDirectory`
/// points at a `ProcessBinaryData` table, so the cycle set and depth count
/// span both engines the way ExifTool's single `$$self{PROCESSED}` does.
pub(super) fn walk(
    table: &'static BinaryTable,
    dir: Dir<'_>,
    ctx: &mut cond::Ctx,
    guard: &mut Guard,
    out: &mut Vec<Emitted>,
) {
    let _ = walk_with_policy(table, dir, ctx, guard, out, ChildTaintPolicy::Contain);
}

fn walk_with_policy(
    table: &'static BinaryTable,
    dir: Dir<'_>,
    ctx: &mut cond::Ctx,
    guard: &mut Guard,
    out: &mut Vec<Emitted>,
    child_taint_policy: ChildTaintPolicy,
) -> BinaryWalkOutcome {
    let size = dir.size();
    let increment = i64::from(table.default_format.size());
    if increment <= 0 {
        return BinaryWalkOutcome::Complete;
    }
    let cursor = Cursor::new(size, increment);

    for (index, candidate) in visit_order(table) {
        // ProcessBinaryData calls GetTagInfo BEFORE it computes the final
        // entry/more bound. Member-only Conditions, including SetMember
        // conditions, therefore run even on a key that later ends the walk.
        // Conditions that ask for a value context return the native retry
        // sentinel instead and are not evaluated unless this key is in range.
        let initial = lookup(candidate, ctx, None);
        // A failed initial GetTagInfo is a native `next` before any offset
        // arithmetic. In particular, a high skipped key must not `last` the
        // loop and suppress a later negative key (which sorts at the end).
        if matches!(initial, Lookup::Skipped) {
            continue;
        }
        let (at, more, entry) = match initial {
            Lookup::Selected(entry) => match cursor.step(index) {
                Step::At { entry: at, more } => (at, more, entry),
                // ExifTool.pm:9961: `next` -- try the following key.
                Step::Skip => continue,
                // The final `$more <= 0` check is reached only after a tag
                // was selected, so only this arm is native `last`.
                Step::Stop => break,
            },
            Lookup::NeedsValue => {
                // The retry branch has its own pre-read `entry >= $size`
                // guard at ExifTool.pm:9923-9930. That is `next`, not the
                // later final `last`, because GetTagInfo has not selected a
                // tag yet.
                let (at, more) = match cursor.step(index) {
                    Step::At { entry: at, more } => (at, more),
                    Step::Skip | Step::Stop => continue,
                };
                let offset =
                    match usize::try_from(at + i64::try_from(dir.dir_start).unwrap_or(i64::MAX)) {
                        Ok(offset) => offset,
                        Err(_) => continue,
                    };
                let val_pt_len = usize::try_from(more).unwrap_or(0).min(128);
                let Some(val_pt) = dir.data.get(offset..offset.saturating_add(val_pt_len)) else {
                    continue;
                };
                match lookup(candidate, ctx, Some(val_pt)) {
                    Lookup::Selected(entry) => (at, more, entry),
                    Lookup::NeedsValue | Lookup::Skipped => continue,
                }
            }
            Lookup::Skipped => unreachable!("handled before cursor arithmetic"),
        };
        let field = entry.field;

        // An unmodeled Hook runs before ReadValue and may replace the format
        // or shift every later offset (ExifTool.pm:10044-10063). An
        // unmodeled RawConv runs through FoundTag before later keys, and an
        // unresolved SubDirectory may do the same in its child walk. Each is
        // an execution dependency, so the tail is not trustworthy until the
        // corresponding effect is modeled and applied.
        if field.omitted.hook
            || (field.omitted.raw_conv && field.raw_conv.is_none())
            || (field.omitted.subdirectory && field.subdir.is_none())
        {
            return BinaryWalkOutcome::Tainted;
        }

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
            if descend(
                table,
                field,
                edge,
                &raw,
                &dir,
                at,
                more,
                ctx,
                guard,
                out,
                child_taint_policy,
            ) == BinaryWalkOutcome::Tainted
                && child_taint_policy == ChildTaintPolicy::Propagate
            {
                return BinaryWalkOutcome::Tainted;
            }
            continue;
        }

        let mut omitted = field.omitted;
        if entry.condition_resolved {
            omitted.condition = false;
        }
        match field.raw_conv {
            Some(super::ifd_schema::RawConvEffect::SetMember { member }) => {
                let Some(value) = member_value(&raw) else {
                    // `$val` is shared state for later Conditions. If it has
                    // a domain this closed MemberValue model cannot preserve,
                    // the safe answer is to stop this table before later
                    // fields can observe a fabricated or absent value.
                    return BinaryWalkOutcome::Tainted;
                };
                // FoundTag runs RawConv before it considers whether to report
                // a tag. The assignment returns `$val`, so clearing this
                // local omission is valid only after the state change.
                ctx.members.insert(member, value);
                omitted.raw_conv = false;
            }
            // This conversion changes only FoundTag's local `$val`; the
            // value itself remains withheld until a renderer is modeled.
            Some(super::ifd_schema::RawConvEffect::ValueLocal) | None => {}
        }
        if omitted.any() {
            continue;
        }
        let Some(converted) = super::runtime::apply_value_conv(field.value_conv, &raw) else {
            // A verified ValueConv may faithfully return Perl undef.  That is
            // tag suppression, not permission to emit the raw value.
            continue;
        };
        // The unconverted value in the form ExifTool reports it: a
        // fixed-count field is ONE space-joined string (ExifTool.pm:6312
        // `join ' '`), not a list -- `exiftool -j` prints
        // `"ConnectionSpaceIlluminant": "0.9642 1 0.82491"`.
        let (value, value_conv) = match super::runtime::render(field.print_conv, &converted) {
            Some(rendered) => (
                TagValue::String(rendered),
                Some(super::runtime::to_exiftool_value(&converted)),
            ),
            None => (super::runtime::to_exiftool_value(&converted), None),
        };
        out.push(Emitted {
            module: table.module,
            table: table.table,
            group0: table.group0,
            // The field's own `Groups{1}` else the table's (ExifTool.pm:
            // 9236-9244 via `effective_groups`).
            group1: table.effective_groups(field).1,
            group2: table.group2,
            name: field.name,
            value,
            value_conv,
            low_priority: table.priority == Some(0),
            // `Avoid` is not part of the binary-table schema (`Field` has no
            // flags); no ProcessBinaryData field in the pinned tree declares
            // it.
            avoid: false,
            rational: None,
        });
    }
    BinaryWalkOutcome::Complete
}

/// One `GetTagInfo` lookup. `value_context` is absent for the native first
/// lookup and present only for ProcessBinaryData's retry; `$format`/`$count`
/// are deliberately always absent for this table kind.
fn lookup(candidate: Candidate, ctx: &mut cond::Ctx, value_context: Option<&[u8]>) -> Lookup {
    let mut condition_ctx = cond::Ctx {
        members: &mut *ctx.members,
        val_pt: value_context,
        format: None,
        count: None,
    };
    match candidate {
        Candidate::Field(field) => {
            // A standalone source Condition was present but outside the
            // closed grammar. GetTagInfo decides whether this field exists
            // before ProcessBinaryData reads it or follows SubDirectory
            // (ExifTool.pm:9162-9181, 10102), so an unconditional selection
            // would permit an unproved state write or child walk.
            if field.condition.is_none() && field.omitted.condition {
                return Lookup::Skipped;
            }
            match field.condition {
                None => Lookup::Selected(Entry {
                    field,
                    condition_resolved: false,
                }),
                Some(condition) if value_context.is_none() && condition.needs_value_context() => {
                    Lookup::NeedsValue
                }
                Some(condition) if condition.eval(&mut condition_ctx) => Lookup::Selected(Entry {
                    field,
                    condition_resolved: true,
                }),
                Some(_) => Lookup::Skipped,
            }
        }
        Candidate::Variants(group) => {
            for (condition, field) in group.alternatives {
                if value_context.is_none() && condition.needs_value_context() {
                    return Lookup::NeedsValue;
                }
                if condition.eval(&mut condition_ctx) {
                    return Lookup::Selected(Entry {
                        field,
                        condition_resolved: true,
                    });
                }
            }
            Lookup::Skipped
        }
    }
}

/// ExifTool's key order (ExifTool.pm:9917) over the union of `fields` and
/// `_variants` groups. The groups remain unresolved here: their Conditions
/// execute at this exact key during [`walk`].
///
/// The two live in separate arrays in the generated schema but are one key
/// space in ExifTool's table, and interleaving them correctly is what makes
/// `varSize` and `DataMember` ordering mean the same thing here as there.
fn visit_order(table: &'static BinaryTable) -> Vec<(i64, Candidate)> {
    let mut entries: Vec<(i64, u32, Candidate)> = Vec::with_capacity(table.fields.len());
    for field in table.fields {
        entries.push((field.index, field.sub.unwrap_or(0), Candidate::Field(field)));
    }
    for group in table.variants {
        entries.push((
            group.index,
            group.sub.unwrap_or(0),
            Candidate::Variants(group),
        ));
    }
    entries.sort_by_key(|(index, sub, _)| (visit_key(*index), *sub));
    entries
        .into_iter()
        .map(|(index, _, entry)| (index, entry))
        .collect()
}

/// The exact scalar text a `$$self{Member} = $val` RawConv stores after
/// ProcessBinaryData has read and masked the field. Numeric values stay
/// numeric for numeric Conditions; other values use the same Perl text the
/// legacy representation can prove. A rational with width-dependent text is
/// deliberately not invented.
/// The exact scalar state a modeled `RawConv => $$self{Member} = $val`
/// stores.  Keyed-directory readers use the same closed state domain; they
/// must not stringify a value that this binary reader would refuse.
pub(crate) fn member_value(raw: &DecodedValue) -> Option<cond::MemberValue> {
    match raw {
        DecodedValue::Integer(n) => Some(cond::MemberValue::Num(*n)),
        DecodedValue::StringBytes(bytes) => Some(cond::MemberValue::Bytes(bytes.clone())),
        DecodedValue::String(value) => Some(cond::MemberValue::Str(value.clone())),
        DecodedValue::Undefined(bytes) => String::from_utf8(bytes.clone())
            .ok()
            .map(cond::MemberValue::Str),
        // `MemberValue` only supports the scalar domains the condition
        // grammar can compare exactly. Do not stringify floats, rationals or
        // arrays: a later numeric Condition would otherwise treat an exact
        // Perl value as an unrelated string. Invalid raw bytes have no Rust
        // string representation, so they fail closed too.
        DecodedValue::Float(_)
        | DecodedValue::UnsignedRational(..)
        | DecodedValue::SignedRational(..)
        | DecodedValue::Array(_) => None,
    }
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
    child_taint_policy: ChildTaintPolicy,
) -> BinaryWalkOutcome {
    let Some(target) = binary_target(edge.module, edge.table) else {
        // Not a defect in the edge: many targets (`IPTC::Main`,
        // `LNK::LinkInfo`'s neighbours) are not ProcessBinaryData tables this
        // crate transcribed a layout for. See subdir.rs.
        return BinaryWalkOutcome::Complete;
    };
    if !binary_walkable(target) {
        // Opt-in (Step 28 D1): an edge never enables its target. Walking into
        // a table that has not passed both gates would enable it by the back
        // door, with no allowlist line to review or revert.
        return BinaryWalkOutcome::Complete;
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
                return BinaryWalkOutcome::Complete;
            }
            let start = expr.eval(val, dir_start);
            // ExifTool.pm:10131.
            if start < dir_start || start > data_len {
                return BinaryWalkOutcome::Complete;
            }
            // ExifTool.pm:10132-10133: DirLen is not modeled by this schema,
            // so the `unless` arm is the only one reachable.
            len = data_len - start;
            (start, false)
        }
    };
    let Ok(start) = usize::try_from(start) else {
        return BinaryWalkOutcome::Complete;
    };
    let Ok(len) = usize::try_from(len) else {
        return BinaryWalkOutcome::Complete;
    };

    // ExifTool.pm:9066 -- `$addr = DirStart + DataPos + Base`.
    let addr = i64::try_from(start).unwrap_or(i64::MAX) + dir.data_pos + subdir_base;
    if !guard.admit(
        dir.data_domain,
        addr,
        std::ptr::from_ref(target) as usize,
        not_dup,
    ) {
        return BinaryWalkOutcome::Complete;
    }
    guard.depth += 1;
    let outcome = walk_with_policy(
        target,
        Dir {
            data: dir.data,
            data_domain: dir.data_domain,
            dir_start: start,
            dir_len: Some(len),
            base: subdir_base,
            data_pos: dir.data_pos,
            byte_order: dir.byte_order,
        },
        ctx,
        guard,
        out,
        child_taint_policy,
    );
    guard.depth -= 1;
    outcome
}

/// Resolve an enabled binary target. Tests can temporarily register a
/// hand-built target, keeping the production registry and gates unchanged.
fn binary_target(module: &str, table: &str) -> Option<&'static BinaryTable> {
    #[cfg(test)]
    {
        tests::registered_table(module, table).or_else(|| find_table(module, table))
    }
    #[cfg(not(test))]
    {
        find_table(module, table)
    }
}

fn binary_walkable(table: &'static BinaryTable) -> bool {
    table.enabled() || {
        #[cfg(test)]
        {
            tests::registered_enabled(table)
        }
        #[cfg(not(test))]
        {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;
    use crate::exiftool_tables::{Omitted, PrintConv, RawConvEffect, TagGroups};

    // -- Test-only binary target registry ------------------------------------
    //
    // The generated allowlist intentionally leaves arbitrary fixture tables
    // off. Registering a pointer makes it findable and walkable only in this
    // module's tests, so nested-child control flow can be pinned without
    // changing production gates or generated data.
    thread_local! {
        static REGISTERED: RefCell<Vec<&'static BinaryTable>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn registered_enabled(table: &'static BinaryTable) -> bool {
        REGISTERED.with(|registered| {
            registered
                .borrow()
                .iter()
                .any(|candidate| std::ptr::eq(*candidate, table))
        })
    }

    pub(super) fn registered_table(module: &str, table: &str) -> Option<&'static BinaryTable> {
        REGISTERED.with(|registered| {
            registered
                .borrow()
                .iter()
                .copied()
                .find(|candidate| candidate.module == module && candidate.table == table)
        })
    }

    struct Registered(usize);

    impl Registered {
        fn new(tables: &[&'static BinaryTable]) -> Self {
            REGISTERED.with(|registered| registered.borrow_mut().extend_from_slice(tables));
            Self(tables.len())
        }
    }

    impl Drop for Registered {
        fn drop(&mut self) {
            REGISTERED.with(|registered| {
                let mut registered = registered.borrow_mut();
                let keep = registered.len() - self.0;
                registered.truncate(keep);
            });
        }
    }

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
            Some(DecodedValue::StringBytes(b"ABCDE".to_vec()))
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
            Some(DecodedValue::StringBytes(b"AB ".to_vec()))
        );
    }

    #[test]
    fn a_bare_string_uses_the_remainder_but_repairs_only_when_reported() {
        // ProcessBinaryData changes only an explicit field `Format =>
        // 'string'` to `$count = $more` (ExifTool.pm:9962-9975). The raw
        // member value stops at NUL and retains the invalid byte; FixUTF8 is
        // deferred until the tag becomes public output.
        let data = *b"EOS\xe9\0trailing";
        let raw = read_value(
            &data,
            0,
            Fmt::RemainderString,
            1,
            data.len() as i64,
            ByteOrder::Big,
        );
        assert_eq!(raw, Some(DecodedValue::StringBytes(b"EOS\xe9".to_vec())));
        assert_eq!(
            raw.as_ref().map(super::super::runtime::to_tag_value),
            Some(TagValue::String("EOS?".to_string()))
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
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 1,
            sub: None,
            name: "Gated",
            format: Some(Fmt::Int16u),
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted {
                value_conv: true,
                ..Omitted::NONE
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ];

    static PLAIN: BinaryTable = BinaryTable {
        module: "Test",
        table: "Plain",
        group0: "MakerNotes",
        // Step 26: `group1` is the table's EFFECTIVE family-1 group. This
        // fixture declares no GROUPS{1}, so GetTagTable's defaulting
        // (ExifTool.pm:8980-8991) falls back to the module name.
        group1: "Test",
        group2: "Camera",
        first_entry: 0,
        default_format: Fmt::Int16u,
        offsets_sound_until: None,
        priority: Some(0),
        gate_a: super::super::GateA { blocked_by: &[] },
        fields: PLAIN_FIELDS,
        variants: &[],
    };

    // Native execution order test: key 0's SetMember must happen before the
    // condition at key 1 and before the variant at key 2 is selected. The
    // old `visit_order(table, ctx)` selected all variants while Locations was
    // still absent, silently choosing the fallback.
    const LOCATIONS_AT_LEAST_TWO: cond::Cond = cond::Cond::MemberCmp {
        member: "Locations",
        op: cond::CmpOp::Ge,
        value: 2,
    };
    static STATE_FIELDS: &[Field] = &[
        Field {
            index: 0,
            sub: None,
            name: "Locations",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: Some(RawConvEffect::SetMember {
                member: "Locations",
            }),
            // The schema remains safe for `decode_binary_table`, which has
            // no member context. `walk` clears this after applying it.
            omitted: Omitted {
                raw_conv: true,
                ..Omitted::NONE
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 1,
            sub: None,
            name: "LocationOne",
            format: None,
            count: 1,
            mask: None,
            condition: Some(LOCATIONS_AT_LEAST_TWO),
            raw_conv: None,
            // Same conservative legacy contract; `walk` clears this only
            // after the Condition returned true at key 1.
            omitted: Omitted {
                condition: true,
                ..Omitted::NONE
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ];
    static STATE_VARIANTS: &[cond::VariantGroup] = &[cond::VariantGroup {
        index: 2,
        sub: None,
        alternatives: &[
            (
                LOCATIONS_AT_LEAST_TWO,
                Field {
                    index: 2,
                    sub: None,
                    name: "TwoOrMore",
                    format: None,
                    count: 1,
                    mask: None,
                    condition: None,
                    raw_conv: None,
                    omitted: Omitted {
                        condition: true,
                        ..Omitted::NONE
                    },
                    value_conv: None,
                    print_conv: PrintConv::None,
                    subdir: None,
                    hook: &[],
                    groups: TagGroups::NONE,
                },
            ),
            (
                cond::Cond::Always,
                Field {
                    index: 2,
                    sub: None,
                    name: "Fallback",
                    format: None,
                    count: 1,
                    mask: None,
                    condition: None,
                    raw_conv: None,
                    omitted: Omitted {
                        condition: true,
                        ..Omitted::NONE
                    },
                    value_conv: None,
                    print_conv: PrintConv::None,
                    subdir: None,
                    hook: &[],
                    groups: TagGroups::NONE,
                },
            ),
        ],
    }];
    static STATE: BinaryTable = BinaryTable {
        module: "Test",
        table: "State",
        group0: "MakerNotes",
        group1: "Test",
        group2: "Camera",
        first_entry: 0,
        default_format: Fmt::Int8u,
        offsets_sound_until: None,
        priority: None,
        gate_a: super::super::GateA { blocked_by: &[] },
        fields: STATE_FIELDS,
        variants: STATE_VARIANTS,
    };
    static REFUSED_CONDITION_EDGE: SubdirEdge = SubdirEdge {
        module: "Test",
        table: "Child",
        start: Start::FieldRelative(0),
        base: None,
        byte_order: None,
        validate: false,
    };
    static UNRESOLVED_CONDITION_FIELDS: &[Field] = &[
        Field {
            index: 0,
            sub: None,
            name: "UnprovedParent",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: Some(RawConvEffect::SetMember { member: "Unsafe" }),
            // A source Condition exists, but this fixture deliberately has
            // no compiled Cond. The edge and SetMember are both unusable.
            omitted: Omitted {
                condition: true,
                raw_conv: true,
                subdirectory: true,
                ..Omitted::NONE
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: Some(REFUSED_CONDITION_EDGE),
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 1,
            sub: None,
            name: "AfterUnprovedParent",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ];
    static UNRESOLVED_CONDITION: BinaryTable = BinaryTable {
        table: "UnresolvedCondition",
        fields: UNRESOLVED_CONDITION_FIELDS,
        ..STATE
    };
    static HOOK_BARRIER_FIELDS: &[Field] = &[
        Field {
            index: 0,
            sub: None,
            name: "HookedState",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: Some(RawConvEffect::SetMember { member: "Hooked" }),
            // The empty data slice marks a Hook the closed grammar refused.
            // It still executes before this field is read in native Perl.
            omitted: Omitted {
                raw_conv: true,
                hook: true,
                ..Omitted::NONE
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 1,
            sub: None,
            name: "AfterHook",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ];
    static HOOK_BARRIER: BinaryTable = BinaryTable {
        table: "HookBarrier",
        fields: HOOK_BARRIER_FIELDS,
        ..STATE
    };
    static UNMODELED_RAW_CONV_FIELDS: &[Field] = &[
        Field {
            index: 0,
            sub: None,
            name: "UnmodeledRawConv",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted {
                raw_conv: true,
                ..Omitted::NONE
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 1,
            sub: None,
            name: "AfterRawConv",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ];
    static UNMODELED_RAW_CONV: BinaryTable = BinaryTable {
        table: "UnmodeledRawConv",
        fields: UNMODELED_RAW_CONV_FIELDS,
        ..STATE
    };
    static VALUE_LOCAL_RAW_CONV_FIELDS: &[Field] = &[
        Field {
            index: 0,
            sub: None,
            name: "Track",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: Some(RawConvEffect::ValueLocal),
            // The value-local expression is not yet rendered, so Track is
            // still withheld. It proves only that later fields are safe.
            omitted: Omitted {
                raw_conv: true,
                ..Omitted::NONE
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 1,
            sub: None,
            name: "Genre",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ];
    static VALUE_LOCAL_RAW_CONV: BinaryTable = BinaryTable {
        table: "ValueLocalRawConv",
        fields: VALUE_LOCAL_RAW_CONV_FIELDS,
        ..STATE
    };
    static UNRESOLVED_SUBDIR_FIELDS: &[Field] = &[
        Field {
            index: 0,
            sub: None,
            name: "UnresolvedSubdir",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted {
                subdirectory: true,
                ..Omitted::NONE
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 1,
            sub: None,
            name: "AfterSubdir",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ];
    static UNRESOLVED_SUBDIR: BinaryTable = BinaryTable {
        table: "UnresolvedSubdir",
        fields: UNRESOLVED_SUBDIR_FIELDS,
        ..STATE
    };
    // A real ProcessDirectory call resumes this parent after the child has
    // stopped at its unmodeled RawConv. The synthetic registered child lets
    // this exact nested control flow be tested without enabling a table.
    static NESTED_TAINT_EDGE: SubdirEdge = SubdirEdge {
        module: "Test",
        table: "NestedTaintedChild",
        start: Start::FieldRelative(1),
        base: None,
        byte_order: None,
        validate: false,
    };
    static NESTED_TAINT_CHILD_FIELDS: &[Field] = &[Field {
        index: 0,
        sub: None,
        name: "ChildUnmodeledRawConv",
        format: None,
        count: 1,
        mask: None,
        condition: None,
        raw_conv: None,
        omitted: Omitted {
            raw_conv: true,
            ..Omitted::NONE
        },
        value_conv: None,
        print_conv: PrintConv::None,
        subdir: None,
        hook: &[],
        groups: TagGroups::NONE,
    }];
    static NESTED_TAINT_CHILD: BinaryTable = BinaryTable {
        table: "NestedTaintedChild",
        fields: NESTED_TAINT_CHILD_FIELDS,
        ..STATE
    };
    static NESTED_TAINT_PARENT_FIELDS: &[Field] = &[
        Field {
            index: 0,
            sub: None,
            name: "ChildDirectory",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted {
                subdirectory: true,
                ..Omitted::NONE
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: Some(NESTED_TAINT_EDGE),
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 1,
            sub: None,
            name: "AfterChild",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ];
    static NESTED_TAINT_PARENT: BinaryTable = BinaryTable {
        table: "NestedTaintedParent",
        fields: NESTED_TAINT_PARENT_FIELDS,
        ..STATE
    };
    const MODEL_IS_EOS: cond::Cond = cond::Cond::MemberRegex {
        member: "Model",
        pattern: "EOS",
        ignore_case: false,
        negate: false,
    };
    static BYTE_MEMBER_FIELDS: &[Field] = &[
        Field {
            index: 0,
            sub: None,
            name: "Model",
            format: Some(Fmt::Str(4)),
            count: 1,
            mask: None,
            condition: None,
            raw_conv: Some(RawConvEffect::SetMember { member: "Model" }),
            omitted: Omitted {
                raw_conv: true,
                ..Omitted::NONE
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 1,
            sub: None,
            name: "SerialNumber",
            format: None,
            count: 1,
            mask: None,
            condition: Some(MODEL_IS_EOS),
            raw_conv: None,
            omitted: Omitted {
                condition: true,
                ..Omitted::NONE
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ];
    static BYTE_MEMBER: BinaryTable = BinaryTable {
        table: "ByteMember",
        fields: BYTE_MEMBER_FIELDS,
        ..STATE
    };
    static UNREPRESENTABLE_MEMBER_FIELDS: &[Field] = &[
        Field {
            index: 0,
            sub: None,
            name: "ByteMember",
            format: Some(Fmt::Undef(1)),
            count: 1,
            mask: None,
            condition: None,
            raw_conv: Some(RawConvEffect::SetMember { member: "Bytes" }),
            omitted: Omitted {
                raw_conv: true,
                ..Omitted::NONE
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 1,
            sub: None,
            name: "AfterBytes",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ];
    static UNREPRESENTABLE_MEMBER: BinaryTable = BinaryTable {
        table: "UnrepresentableMember",
        fields: UNREPRESENTABLE_MEMBER_FIELDS,
        ..STATE
    };
    const SET_BEFORE_BOUND: cond::Cond = cond::Cond::SetMember {
        member: "SawPastEnd",
        source: cond::EffectSource::Const(1),
        then: None,
    };
    static PREBOUND_FIELDS: &[Field] = &[Field {
        index: 1,
        sub: None,
        name: "PastEnd",
        format: None,
        count: 1,
        mask: None,
        condition: Some(SET_BEFORE_BOUND),
        raw_conv: None,
        omitted: Omitted {
            condition: true,
            ..Omitted::NONE
        },
        value_conv: None,
        print_conv: PrintConv::None,
        subdir: None,
        hook: &[],
        groups: TagGroups::NONE,
    }];
    static PREBOUND: BinaryTable = BinaryTable {
        module: "Test",
        table: "Prebound",
        group0: "MakerNotes",
        group1: "Test",
        group2: "Camera",
        first_entry: 0,
        default_format: Fmt::Int8u,
        offsets_sound_until: None,
        priority: None,
        gate_a: super::super::GateA { blocked_by: &[] },
        fields: PREBOUND_FIELDS,
        variants: &[],
    };
    const VALUE_CONTEXT_GUARD: cond::Cond = cond::Cond::And(
        &SET_BEFORE_BOUND,
        &cond::Cond::ValPtRegex {
            pattern: r"^\x01",
            negate: false,
        },
    );
    static VALUE_CONTEXT_PREBOUND_FIELDS: &[Field] = &[Field {
        index: 1,
        sub: None,
        name: "NeedsValue",
        format: None,
        count: 1,
        mask: None,
        condition: Some(VALUE_CONTEXT_GUARD),
        raw_conv: None,
        omitted: Omitted {
            condition: true,
            ..Omitted::NONE
        },
        value_conv: None,
        print_conv: PrintConv::None,
        subdir: None,
        hook: &[],
        groups: TagGroups::NONE,
    }];
    static VALUE_CONTEXT_PREBOUND: BinaryTable = BinaryTable {
        fields: VALUE_CONTEXT_PREBOUND_FIELDS,
        ..PREBOUND
    };
    const NEVER: cond::Cond = cond::Cond::MemberCmp {
        member: "Missing",
        op: cond::CmpOp::Eq,
        value: 1,
    };
    const FIRST_BYTE_IS_ONE: cond::Cond = cond::Cond::ValPtRegex {
        pattern: r"^\x01",
        negate: false,
    };
    static NEGATIVE_AFTER_SKIPPED_FIELDS: &[Field] = &[
        Field {
            index: 99,
            sub: None,
            name: "HighSkipped",
            format: None,
            count: 1,
            mask: None,
            condition: Some(NEVER),
            raw_conv: None,
            omitted: Omitted {
                condition: true,
                ..Omitted::NONE
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: -1,
            sub: None,
            name: "FromEnd",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ];
    static NEGATIVE_AFTER_VALUE_RETRY_FIELDS: &[Field] = &[
        Field {
            index: 99,
            sub: None,
            name: "HighNeedsValue",
            format: None,
            count: 1,
            mask: None,
            condition: Some(FIRST_BYTE_IS_ONE),
            raw_conv: None,
            omitted: Omitted {
                condition: true,
                ..Omitted::NONE
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: -1,
            sub: None,
            name: "FromEnd",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ];
    static NEGATIVE_AFTER_SKIPPED: BinaryTable = BinaryTable {
        table: "NegativeAfterSkipped",
        fields: NEGATIVE_AFTER_SKIPPED_FIELDS,
        ..PREBOUND
    };
    static NEGATIVE_AFTER_VALUE_RETRY: BinaryTable = BinaryTable {
        table: "NegativeAfterValueRetry",
        fields: NEGATIVE_AFTER_VALUE_RETRY_FIELDS,
        ..PREBOUND
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
    fn raw_conv_state_and_conditions_execute_at_their_native_keys() {
        let got = run(&STATE, &[2, 44, 55]);
        assert_eq!(
            got.iter().map(|tag| tag.name).collect::<Vec<_>>(),
            vec!["Locations", "LocationOne", "TwoOrMore"],
            "key 0 stores Locations before the direct and variant Conditions run"
        );
        assert_eq!(got[0].value, TagValue::Integer(2));
        assert_eq!(got[1].value, TagValue::Integer(44));
        assert_eq!(got[2].value, TagValue::Integer(55));

        let got = run(&STATE, &[1, 44, 55]);
        assert_eq!(
            got.iter().map(|tag| tag.name).collect::<Vec<_>>(),
            vec!["Locations", "Fallback"],
            "false direct Condition suppresses its tag and the variant falls back"
        );
    }

    #[test]
    fn unresolved_direct_condition_cannot_trigger_state_or_a_subdirectory() {
        use std::collections::HashMap;

        // GetTagInfo evaluates a standalone Condition before
        // ProcessBinaryData reads the field or takes its SubDirectory branch
        // (ExifTool.pm:9162-9181, 10102). No compiled Cond is therefore a
        // rejected lookup, rather than permission to use this edge or write
        // the RawConv data member.
        let mut members = HashMap::new();
        let mut ctx = cond::Ctx::new(&mut members);
        assert!(matches!(
            lookup(
                Candidate::Field(&UNRESOLVED_CONDITION_FIELDS[0]),
                &mut ctx,
                None
            ),
            Lookup::Skipped
        ));
        let mut out = Vec::new();
        process_binary_data(
            &UNRESOLVED_CONDITION,
            Dir::whole(&[1, 7], ByteOrder::Big),
            &mut ctx,
            &mut out,
        );
        assert_eq!(
            out.iter().map(|tag| tag.name).collect::<Vec<_>>(),
            vec!["AfterUnprovedParent"]
        );
        assert!(!members.contains_key("Unsafe"));
    }

    #[test]
    fn unexecuted_hook_is_a_barrier_before_state_and_later_offsets() {
        use std::collections::HashMap;

        // A Hook changes the current format and/or varSize before ReadValue
        // (ExifTool.pm:10044-10063). The common engine has not executed it,
        // so neither the member assignment nor the tail can be trusted.
        let mut members = HashMap::new();
        let mut ctx = cond::Ctx::new(&mut members);
        let mut out = Vec::new();
        process_binary_data(
            &HOOK_BARRIER,
            Dir::whole(&[1, 7], ByteOrder::Big),
            &mut ctx,
            &mut out,
        );
        assert!(out.is_empty());
        assert!(!members.contains_key("Hooked"));
    }

    #[test]
    fn unmodeled_raw_conv_and_subdirectory_are_tail_barriers() {
        use std::collections::HashMap;

        // An unmodeled RawConv can write `$$self` through FoundTag
        // (ExifTool.pm:10159-10169). An unmodeled SubDirectory may write the
        // same shared state while it processes the child (10102-10151).
        // Neither permits a later parent field to be decoded as if nothing
        // happened.
        for table in [&UNMODELED_RAW_CONV, &UNRESOLVED_SUBDIR] {
            assert!(
                run(table, &[1, 7]).is_empty(),
                "{} must not reach its second field",
                table.table
            );
            let mut members = HashMap::new();
            let mut ctx = cond::Ctx::new(&mut members);
            let mut out = Vec::new();
            assert_eq!(
                process_binary_data_checked(
                    table,
                    Dir::whole(&[1, 7], ByteOrder::Big),
                    &mut ctx,
                    &mut out,
                ),
                BinaryWalkOutcome::Tainted,
                "{} must expose its unsafe tail to a keyed parent",
                table.table
            );
        }
    }

    #[test]
    fn nested_child_taint_is_contained_for_legacy_and_propagates_for_keyed() {
        let _registered = Registered::new(&[&NESTED_TAINT_CHILD]);
        // The old `descend()` had no return value, so ProcessBinaryData
        // resumed the parent and emitted this later sibling. That remains the
        // public wrapper's contract for ICC/H264 callers. Checked keyed
        // descent instead exposes the child state hazard to its own frames.
        let data = [0, 7];
        let mut legacy_members = HashMap::new();
        let mut legacy_ctx = cond::Ctx::new(&mut legacy_members);
        let mut legacy_rows = Vec::new();
        process_binary_data(
            &NESTED_TAINT_PARENT,
            Dir::whole(&data, ByteOrder::Big),
            &mut legacy_ctx,
            &mut legacy_rows,
        );
        assert_eq!(
            legacy_rows.iter().map(|row| row.name).collect::<Vec<_>>(),
            vec!["AfterChild"],
            "legacy ProcessDirectory resumes the parent after the child stops"
        );

        let mut checked_members = HashMap::new();
        let mut checked_ctx = cond::Ctx::new(&mut checked_members);
        let mut checked_rows = Vec::new();
        assert_eq!(
            process_binary_data_checked(
                &NESTED_TAINT_PARENT,
                Dir::whole(&data, ByteOrder::Big),
                &mut checked_ctx,
                &mut checked_rows,
            ),
            BinaryWalkOutcome::Tainted
        );
        assert!(
            checked_rows.is_empty(),
            "keyed callers must stop their pending parent work"
        );
    }

    #[test]
    fn proven_value_local_raw_conv_withholds_itself_but_reaches_later_fields() {
        // Native ID3::v1 Track uses the exact local-only form. It may turn a
        // leading-zero track into undef, but it cannot alter Genre or table
        // state; `/usr/bin/perl` against the pinned table reports
        // Track=missing, Genre=Rock for these bytes.
        let got = run(&VALUE_LOCAL_RAW_CONV, &[0, 17]);
        assert_eq!(
            got.iter()
                .map(|tag| (tag.name, &tag.value))
                .collect::<Vec<_>>(),
            vec![("Genre", &TagValue::Integer(17))]
        );
    }

    #[test]
    fn set_member_preserves_raw_string_bytes_for_later_conditions() {
        use std::collections::HashMap;

        // CanonRaw::MakeModel's Model RawConv writes its byte string before
        // CanonRaw's later `/EOS/` branch runs. The byte after EOS is
        // deliberately invalid UTF-8: native Perl preserves it in `$$self`
        // and still takes the branch, while output repair happens later.
        let mut members = HashMap::new();
        let mut ctx = cond::Ctx::new(&mut members);
        let mut out = Vec::new();
        process_binary_data(
            &BYTE_MEMBER,
            Dir::whole(b"EOS\xe9\0\0", ByteOrder::Big),
            &mut ctx,
            &mut out,
        );
        assert_eq!(
            members.get("Model"),
            Some(&cond::MemberValue::Bytes(b"EOS\xe9".to_vec()))
        );
        assert_eq!(
            out.iter()
                .find(|tag| tag.name == "Model")
                .map(|tag| &tag.value),
            Some(&TagValue::String("EOS?".to_string())),
            "FixUTF8 belongs to the reported TagValue, after RawConv state"
        );
        assert!(
            out.iter().any(|tag| tag.name == "SerialNumber"),
            "the later byte-regex condition must see raw pre-FixUTF8 state"
        );

        assert_eq!(
            member_value(&DecodedValue::Undefined(b"NIKN".to_vec())),
            Some(cond::MemberValue::Str("NIKN".to_string()))
        );
        assert_eq!(member_value(&DecodedValue::Float(1.5)), None);
        assert_eq!(member_value(&DecodedValue::UnsignedRational(1, 2)), None);
        assert_eq!(member_value(&DecodedValue::SignedRational(1, 2)), None);
        assert_eq!(
            member_value(&DecodedValue::Array(vec![DecodedValue::Integer(1)])),
            None
        );
    }

    #[test]
    fn set_member_stops_when_raw_value_has_no_exact_member_domain() {
        use std::collections::HashMap;

        // Non-UTF-8 `undef` is legal Perl byte data but is not yet a modeled
        // `MemberValue`. Continuing would let a later Condition see missing
        // state, so the table stops at this RawConv.
        let mut members = HashMap::new();
        let mut ctx = cond::Ctx::new(&mut members);
        let mut out = Vec::new();
        process_binary_data(
            &UNREPRESENTABLE_MEMBER,
            Dir::whole(&[0xff, 7], ByteOrder::Big),
            &mut ctx,
            &mut out,
        );
        assert!(out.is_empty());
        assert!(!members.contains_key("Bytes"));
    }

    #[test]
    fn member_only_conditions_run_before_the_final_bounds_check() {
        use std::collections::HashMap;

        // ExifTool.pm:9946 calls GetTagInfo before :9957-9964 computes
        // `$entry`/`$more`. The key is out of range and emits nothing, but
        // its assignment-as-condition still updates `$$self` first.
        let mut members = HashMap::new();
        let mut ctx = cond::Ctx::new(&mut members);
        let mut out = Vec::new();
        process_binary_data(
            &PREBOUND,
            Dir::whole(&[0], ByteOrder::Big),
            &mut ctx,
            &mut out,
        );
        assert!(out.is_empty());
        assert_eq!(members.get("SawPastEnd"), Some(&cond::MemberValue::Num(1)));
    }

    #[test]
    fn value_context_conditions_do_not_run_when_the_retry_is_out_of_range() {
        use std::collections::HashMap;

        // GetTagInfo returns its defined-false retry sentinel before it has
        // evaluated any part of a Condition mentioning `$valPt`. The later
        // entry bound prevents that retry, so even the left SetMember does
        // not run (ExifTool.pm:9168, 9946-9964).
        let mut members = HashMap::new();
        let mut ctx = cond::Ctx::new(&mut members);
        let mut out = Vec::new();
        process_binary_data(
            &VALUE_CONTEXT_PREBOUND,
            Dir::whole(&[0], ByteOrder::Big),
            &mut ctx,
            &mut out,
        );
        assert!(out.is_empty());
        assert!(!members.contains_key("SawPastEnd"));
    }

    #[test]
    fn skipped_and_value_retry_keys_do_not_stop_a_later_negative_key() {
        // Pinned ExifTool.pm:9917 sorts -1 after 99; :9919's initial
        // GetTagInfo failure and :9923's value-context retry bound both use
        // `next`, while only the selected tag's final :9964 check uses
        // `last`. The one-byte native probe emits FromEnd=7 in both cases.
        for table in [&NEGATIVE_AFTER_SKIPPED, &NEGATIVE_AFTER_VALUE_RETRY] {
            let got = run(table, &[7]);
            assert_eq!(
                got.iter()
                    .map(|tag| (tag.name, &tag.value))
                    .collect::<Vec<_>>(),
                vec![("FromEnd", &TagValue::Integer(7))]
            );
        }
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
