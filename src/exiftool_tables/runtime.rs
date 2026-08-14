//! Runtime reader for generated ExifTool `ProcessBinaryData` tables.
//!
//! The generated registry describes byte layout and the conversions that the
//! transcription pipeline can reproduce exactly. This module is deliberately
//! smaller than ExifTool's full `ProcessBinaryData`: conversions the generator
//! refused to approximate are left for format-specific code instead of being
//! guessed. A fractional index with no `Mask` is not in that category --
//! ExifTool.pm:9957's `$entry = int($index) * $increment + $varSize` reads the
//! whole word at the integer part regardless of `Mask`, so a maskless
//! fractional field decodes the same way (Step 11, `OVERHAUL_OXIDEX_PLAN.md`
//! D4): the fractional suffix is retained on [`Field::sub`] as logical
//! identity/order metadata, not as a bit slice.

use crate::core::TagValue;
use crate::io::ByteOrder;

use super::cond;
use super::{ALL_BINARY_TABLES, BinaryTable, Field, Fmt, Omitted, PrintConv};

/// A value read directly from a generated binary-table field.
#[derive(Clone, Debug, PartialEq)]
pub enum DecodedValue {
    Integer(i64),
    Float(f64),
    UnsignedRational(u32, u32),
    SignedRational(i32, i32),
    String(String),
    Undefined(Vec<u8>),
    /// A repeated scalar field (`format[N]` in ExifTool's table schema).
    ///
    /// Scalar fields deliberately retain their existing variants so callers
    /// do not need to unwrap a one-element array.
    Array(Vec<DecodedValue>),
}

impl DecodedValue {
    fn integer(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }

    fn number(&self) -> Option<f64> {
        match self {
            Self::Integer(value) => Some(*value as f64),
            Self::Float(value) => Some(*value),
            Self::UnsignedRational(numerator, denominator) if *denominator != 0 => {
                Some(f64::from(*numerator) / f64::from(*denominator))
            }
            Self::SignedRational(numerator, denominator) if *denominator != 0 => {
                Some(f64::from(*numerator) / f64::from(*denominator))
            }
            _ => None,
        }
    }

    fn enum_key(&self) -> Option<String> {
        match self {
            Self::Integer(value) => Some(value.to_string()),
            Self::String(value) => Some(value.clone()),
            _ => None,
        }
    }
}

/// One successfully decoded generated field.
///
/// `raw` is private (Step 10, `OVERHAUL_OXIDEX_PLAN.md`'s bypass-proof API):
/// the only way to a value is [`DecodedField::emit`], which consults every
/// [`Omitted`] flag, or [`RawAccess`], which requires acknowledging each flag
/// the field actually sets plus a citation into the Perl being reproduced by
/// hand. There is no third way to reach `raw`.
#[derive(Clone, Debug)]
pub struct DecodedField {
    pub field: &'static Field,
    raw: DecodedValue,
}

impl DecodedField {
    /// The value ExifTool would report for this field, or `None` when any of
    /// `Field::omitted`'s five flags is set.
    ///
    /// A clean field (no flag set) renders its generated `PrintConv`; when
    /// that yields nothing -- `PrintConv::None`, or a hash/expression that
    /// does not cover this value -- the raw decoded value stands in, which is
    /// the same "no conversion is honest, a guess is not" rule
    /// [`PrintConv`]'s own doc comment states. A flagged field always
    /// refuses: ExifTool ran a `ValueConv`/`RawConv`/`Condition`/`Hook`/
    /// `SubDirectory` this schema does not reproduce, and reporting the raw
    /// bytes under the real tag name would be a confident wrong value
    /// (AGENTS.md, "never approximate a conversion").
    #[must_use]
    pub fn emit(&self) -> Option<TagValue> {
        if self.field.omitted.any() {
            return None;
        }
        Some(match apply_print_conv(self.field.print_conv, &self.raw) {
            Some(rendered) => TagValue::String(rendered),
            None => decoded_value_to_tag_value(&self.raw),
        })
    }
}

/// Render a [`DecodedValue`] as a [`TagValue`] with no conversion applied.
///
/// The fallback [`DecodedField::emit`] and [`RawAccess::emit_raw`] both use
/// when a `PrintConv` does not apply (absent, or a hash/expression that does
/// not cover this value) -- the raw value stands in, honestly, rather than a
/// guessed string.
fn decoded_value_to_tag_value(value: &DecodedValue) -> TagValue {
    match value {
        DecodedValue::Integer(v) => TagValue::Integer(*v),
        DecodedValue::Float(v) => TagValue::Float(*v),
        DecodedValue::UnsignedRational(n, d) => TagValue::Rational {
            numerator: *n as i32,
            denominator: *d as i32,
        },
        DecodedValue::SignedRational(n, d) => TagValue::Rational {
            numerator: *n,
            denominator: *d,
        },
        DecodedValue::String(s) => TagValue::String(s.clone()),
        DecodedValue::Undefined(bytes) => TagValue::Binary(bytes.clone()),
        DecodedValue::Array(values) => {
            TagValue::Array(values.iter().map(decoded_value_to_tag_value).collect())
        }
    }
}

/// A caller's acknowledgment of which of a field's [`Omitted`] semantics it
/// has independently supplied, one bit per flag.
///
/// Bitflag-style and deliberately not a `bool`: acknowledging `condition` on
/// a field that also has `value_conv` set must not unlock it, so a single
/// "yes, I checked" flag would be the wrong shape for this. Combine flags
/// with `|`, e.g. `Acknowledged::CONDITION | Acknowledged::VALUE_CONV`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Acknowledged(u8);

impl Acknowledged {
    pub const NONE: Self = Self(0);
    pub const VALUE_CONV: Self = Self(1 << 0);
    pub const RAW_CONV: Self = Self(1 << 1);
    pub const CONDITION: Self = Self(1 << 2);
    pub const HOOK: Self = Self(1 << 3);
    pub const SUBDIRECTORY: Self = Self(1 << 4);

    #[must_use]
    const fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 == flag.0
    }

    /// True when every flag `omitted` sets is also set here. An omitted flag
    /// that is `false` imposes no requirement, so a field with nothing set at
    /// all is trivially covered by [`Acknowledged::NONE`].
    #[must_use]
    const fn covers(self, omitted: Omitted) -> bool {
        (!omitted.value_conv || self.contains(Self::VALUE_CONV))
            && (!omitted.raw_conv || self.contains(Self::RAW_CONV))
            && (!omitted.condition || self.contains(Self::CONDITION))
            && (!omitted.hook || self.contains(Self::HOOK))
            && (!omitted.subdirectory || self.contains(Self::SUBDIRECTORY))
    }
}

impl std::ops::BitOr for Acknowledged {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// A citation into the pinned ExifTool Perl source (`.exiftool-version`) that
/// a [`RawAccess`] construction reproduces by hand.
///
/// Required on every call (D2 of Step 10's design: no exceptions) so a
/// reviewer -- or a future regeneration that moves the cited lines -- has
/// something concrete to check the hand-written conversion against, instead
/// of trusting a bare `raw` read on faith.
#[derive(Clone, Copy, Debug)]
pub struct PerlCitation {
    pub module: &'static str,
    pub table: &'static str,
    pub tag: &'static str,
    /// Human-readable line reference, e.g. `"5951-5967"`.
    pub lines: &'static str,
}

/// The sole escape hatch past [`DecodedField::emit`]'s refusal.
///
/// Constructing one requires covering *every* flag the field's
/// [`Field::omitted`] sets (acknowledging `condition` alone does not unlock a
/// field that also has `value_conv` set) and citing the Perl this call site
/// reproduces by hand. There is no other way to reach a flagged field's raw
/// value: `DecodedField::raw` is private, and [`RawAccess::new`] is the only
/// function in this crate that reads it from outside this module.
pub struct RawAccess<'a> {
    field: &'a DecodedField,
    /// Carried for provenance -- a call site exists because this was set,
    /// even though nothing here reads it back at runtime.
    #[allow(dead_code)]
    justification: &'static PerlCitation,
}

impl<'a> RawAccess<'a> {
    /// `None` when `acknowledged` does not cover every flag `field.field.omitted`
    /// sets -- acknowledging a subset is the same as acknowledging none.
    #[must_use]
    pub fn new(
        field: &'a DecodedField,
        acknowledged: Acknowledged,
        justification: &'static PerlCitation,
    ) -> Option<Self> {
        if acknowledged.covers(field.field.omitted) {
            Some(Self {
                field,
                justification,
            })
        } else {
            None
        }
    }

    /// The field this access covers.
    #[must_use]
    pub fn field(&self) -> &'static Field {
        self.field.field
    }

    /// The decoded value before any conversion -- what the caller
    /// acknowledged reading past `Field::omitted` to reach.
    #[must_use]
    pub fn raw(&self) -> &DecodedValue {
        &self.field.raw
    }

    /// Render the field's generated `PrintConv` against the raw value, or
    /// fall back to the raw value itself when the conversion does not apply.
    ///
    /// This is [`DecodedField::emit`]'s rendering step made available to a
    /// caller who has already supplied the semantics `emit` itself refuses to
    /// assume -- e.g. a `Condition`-gated field the call site has verified the
    /// gate for, whose `PrintConv` is otherwise exactly what the schema
    /// records.
    #[must_use]
    pub fn emit_raw(&self) -> TagValue {
        match apply_print_conv(self.field.field.print_conv, self.raw()) {
            Some(rendered) => TagValue::String(rendered),
            None => decoded_value_to_tag_value(self.raw()),
        }
    }
}

/// How many of a table's fractional (bit-field) entries [`decode_binary_table`]
/// can read, and how many it still refuses.
///
/// Before Step 11, a maskless fractional entry was refused outright: which
/// bits it named was unrecorded, and reporting the whole word under its name
/// would have been a confident wrong value. ExifTool.pm:9957 settled that --
/// `$entry = int($index) * $increment + $varSize` computes the same byte
/// offset regardless of `Mask`, and `Mask`'s absence just means ExifTool never
/// reduces the word (`ExifTool.pm`'s `$val = ($val & $mask) >> $BitShift if
/// $mask`), so the *whole word* is the value, not an unknown slice of one. So
/// `decoded` now counts every fractional entry -- masked ones as their bit
/// slice, maskless ones as the floor-indexed whole word -- and `refused` is
/// the count of fractional entries `decode_binary_table` still cannot place
/// (currently none: the one historical reason was this refusal). The split is
/// still pinned by a test, both because it is cheap insurance against a
/// regeneration silently reintroducing a refusal, and because a census nobody
/// measures is indistinguishable from a complete one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FractionalCensus {
    /// Fractional entries [`decode_binary_table`] produces a value for: a
    /// [`Mask`] makes it a bit slice, no `Mask` makes it the whole word read
    /// at `floor(index)` (Step 11).
    pub decoded: usize,
    /// Fractional entries `decode_binary_table` still cannot place. Always 0
    /// today -- kept so a future refusal reason has somewhere to land without
    /// changing this type's shape.
    pub refused: usize,
}

impl FractionalCensus {
    /// Fold another table's census into this one.
    #[must_use]
    pub const fn merge(self, other: Self) -> Self {
        Self {
            decoded: self.decoded + other.decoded,
            refused: self.refused + other.refused,
        }
    }
}

/// Split `table`'s fractional entries by whether [`decode_binary_table`] can
/// place them. Step 11 lifted the maskless refusal, so every fractional entry
/// counts as decoded; `refused` stays for shape (see [`FractionalCensus`]).
#[must_use]
pub fn fractional_census(table: &BinaryTable) -> FractionalCensus {
    let mut census = FractionalCensus::default();
    for field in table.fields {
        if field.sub.is_none() {
            continue;
        }
        census.decoded += 1;
    }
    census
}

/// The same split across every generated table.
#[must_use]
pub fn all_fractional_census() -> FractionalCensus {
    ALL_BINARY_TABLES
        .iter()
        .fold(FractionalCensus::default(), |total, table| {
            total.merge(fractional_census(table))
        })
}

/// How many fields [`decode_binary_table`] withheld from
/// [`DecodedField::emit`], broken out by reason.
///
/// Step 10 (`OVERHAUL_OXIDEX_PLAN.md`, D1): a field is withheld when any of
/// `Field::omitted`'s five flags is set, or when its offset falls past the
/// table's [`BinaryTable::offsets_sound_until`] boundary. A field can trip
/// more than one flag at once (e.g. both `condition` and `value_conv`), and
/// each is counted independently -- this is a census of *reasons*, not of
/// distinct fields, so the total can exceed the number of fields a table
/// actually withholds. The point of counting at all is that a withheld field
/// is not a silently dropped one: [`TableDecode::refusals`] is the seam Step
/// 13's diagnostic sink reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RefusalCounts {
    pub value_conv: usize,
    pub raw_conv: usize,
    pub condition: usize,
    pub hook: usize,
    pub subdirectory: usize,
    /// Fields whose offset sits past `offsets_sound_until` -- unlike the five
    /// flags above, there is no [`RawAccess`] escape for this one: the static
    /// `index * increment` formula is not just unmodeled here, it is not
    /// reliably the field's real byte offset at all, so there is no `raw` to
    /// acknowledge reading.
    pub offset_unsound: usize,
}

impl RefusalCounts {
    /// Sum across all six reasons.
    #[must_use]
    pub const fn total(self) -> usize {
        self.value_conv
            + self.raw_conv
            + self.condition
            + self.hook
            + self.subdirectory
            + self.offset_unsound
    }

    /// Fold another table's counts into this one.
    #[must_use]
    pub const fn merge(self, other: Self) -> Self {
        Self {
            value_conv: self.value_conv + other.value_conv,
            raw_conv: self.raw_conv + other.raw_conv,
            condition: self.condition + other.condition,
            hook: self.hook + other.hook,
            subdirectory: self.subdirectory + other.subdirectory,
            offset_unsound: self.offset_unsound + other.offset_unsound,
        }
    }
}

/// The result of [`decode_binary_table`]: every field it could place bytes
/// for, plus a census of why any of them were withheld from
/// [`DecodedField::emit`].
///
/// `fields` deliberately still carries a withheld field's [`DecodedField`] --
/// its bytes decoded fine, only its *meaning* is unresolved -- so
/// [`RawAccess`] has something to acknowledge its way into. Only
/// `offset_unsound` fields are absent from `fields` entirely, because there is
/// no [`RawAccess`] path for those (see [`RefusalCounts::offset_unsound`]).
#[derive(Clone, Debug, Default)]
pub struct TableDecode {
    fields: Vec<DecodedField>,
    refusals: RefusalCounts,
}

impl TableDecode {
    /// Every field this table's bytes were long enough to decode, withheld or
    /// not. Call [`DecodedField::emit`] on each, or construct a [`RawAccess`]
    /// for one whose flags a caller can justify.
    #[must_use]
    pub fn fields(&self) -> &[DecodedField] {
        &self.fields
    }

    /// Why fields in this decode were withheld from `emit`, broken out by
    /// reason. See [`RefusalCounts`].
    #[must_use]
    pub fn refusals(&self) -> RefusalCounts {
        self.refusals
    }

    /// Consume this decode into its parts, for a caller that wants to fold
    /// `refusals` into a diagnostic sink alongside iterating `fields`.
    #[must_use]
    pub fn into_parts(self) -> (Vec<DecodedField>, RefusalCounts) {
        (self.fields, self.refusals)
    }
}

/// Decode the fields whose layouts are completely described by `table`.
///
/// Out-of-range fields are silently absent (never enough bytes to decode at
/// all); that is not part of [`RefusalCounts`], which is specifically about
/// resolved-but-withheld fields (D1).
///
/// A fractional key is ExifTool's `12.1` notation: several tags share the word
/// at index 12, each naming a slice of it. `Mask` is what says *which* slice,
/// so a fractional field that declares one is reduced to that slice; one that
/// does not still decodes -- Step 11 (`OVERHAUL_OXIDEX_PLAN.md` D4):
/// ExifTool.pm:9957's `$entry = int($index) * $increment + $varSize` computes
/// the byte offset the same way whether or not the tag has a `Mask`, and
/// further down `$val = ($val & $mask) >> $$tagInfo{BitShift} if $mask` only
/// reduces the word when `Mask` is set -- with no `Mask`, ExifTool reports the
/// whole word at `floor(index)` under the fractional tag's name. So a
/// maskless fractional field decodes here too, as the unreduced word at
/// `field.index` (the generator already stores only the integer part of a
/// `12.1`-style key there, with the fraction in [`Field::sub`]); `sub` is
/// retained on the decoded field only as the logical identity/order that
/// distinguishes it from sibling entries sharing the same word, never as a
/// bit slice.
///
/// A field carrying a [`Mask`] is reduced to `(val & bits) >> shift` here,
/// which is what ExifTool does before any conversion runs
/// (`$val = ($val & $mask) >> $$tagInfo{BitShift} if $mask`, ExifTool.pm's
/// `ProcessBinaryData`). Repeated scalar fields decode as
/// [`DecodedValue::Array`]. This function performs raw decoding only; get a
/// value out with [`DecodedField::emit`] or [`RawAccess`].
#[must_use]
pub fn decode_binary_table(
    table: &'static BinaryTable,
    data: &[u8],
    byte_order: ByteOrder,
) -> TableDecode {
    let mut fields = Vec::new();
    let mut refusals = RefusalCounts::default();

    for field in table.fields {
        // D1: past this bound, `index * increment` is a nominal offset, not a
        // trustworthy one -- there is no `raw` here to hand a caller at all.
        if let Some(bound) = table.offsets_sound_until
            && field.index > bound
        {
            refusals.offset_unsound += 1;
            continue;
        }

        let Some(decoded) = decode_field(table, field, data, byte_order) else {
            continue;
        };

        let omitted = field.omitted;
        if omitted.value_conv {
            refusals.value_conv += 1;
        }
        if omitted.raw_conv {
            refusals.raw_conv += 1;
        }
        if omitted.condition {
            refusals.condition += 1;
        }
        if omitted.hook {
            refusals.hook += 1;
        }
        if omitted.subdirectory {
            refusals.subdirectory += 1;
        }
        fields.push(decoded);
    }

    TableDecode { fields, refusals }
}

/// Resolve and decode `table.variants` (Step 23's `_variants` schema) against
/// `ctx`, in addition to whatever [`decode_binary_table`] would decode from
/// `table.fields`.
///
/// For each [`cond::VariantGroup`], [`cond::first_match`] walks its
/// alternatives in ExifTool's `GetTagInfo` order and returns the first whose
/// [`cond::Cond`] evaluates true against `ctx` -- applying any
/// [`cond::Cond::SetMember`] side effects along the way, even from
/// alternatives that lose (see `src/exiftool_tables/cond.rs`'s module doc for
/// why that matters). A group with no matching alternative contributes
/// nothing: that is the correct behaviour, not a refusal -- real ExifTool
/// does not produce a tag for an index whose every `Condition` failed either.
///
/// The winning alternative's [`Field`] is otherwise decoded exactly like a
/// `table.fields` entry (same [`Mask`], same [`super::PrintConv`], same
/// `offsets_sound_until` soundness bound), because once a `Cond` picks it,
/// its own semantics are fully resolved -- it carries no unresolved
/// `Condition` for [`DecodedField::emit`] to refuse over (`conds.py`
/// compiled it; there is nothing left omitted unless the alternative
/// separately declares a `ValueConv`/`RawConv`/`Hook`/`SubDirectory`, which
/// still gates [`DecodedField::emit`] the ordinary way).
#[must_use]
pub fn decode_binary_table_variants(
    table: &'static BinaryTable,
    data: &[u8],
    byte_order: ByteOrder,
    ctx: &mut cond::Ctx,
) -> Vec<DecodedField> {
    let mut out = Vec::new();
    for group in table.variants {
        let Some(field) = cond::first_match(group.alternatives, ctx) else {
            continue;
        };
        if let Some(bound) = table.offsets_sound_until
            && field.index > bound
        {
            continue;
        }
        if let Some(decoded) = decode_field(table, field, data, byte_order) {
            out.push(decoded);
        }
    }
    out
}

/// Decode one field's bytes, applying its [`Mask`] if it has one. `None` means
/// the field's bytes did not fit in `data` -- a short read, not a refusal.
fn decode_field(
    table: &'static BinaryTable,
    field: &'static Field,
    data: &[u8],
    byte_order: ByteOrder,
) -> Option<DecodedField> {
    let offset = usize::try_from(table.byte_offset(field)).ok()?;
    let format = table.field_format(field);
    let width = usize::try_from(format.size()).ok()?;
    let byte_count = width.checked_mul(field.count)?;
    let bytes = data.get(offset..offset.checked_add(byte_count)?)?;
    let raw = if field.count == 1 {
        decode_value(bytes, format, byte_order)?
    } else {
        let values = bytes
            .chunks_exact(width)
            .map(|chunk| decode_value(chunk, format, byte_order))
            .collect::<Option<Vec<_>>>()?;
        DecodedValue::Array(values)
    };
    let raw = match field.mask {
        None => raw,
        // A mask on a non-integer is a construct this schema cannot
        // express; refuse rather than report the unmasked value.
        Some(mask) => DecodedValue::Integer(mask.apply(raw.integer()?)),
    };
    Some(DecodedField { field, raw })
}

fn decode_value(bytes: &[u8], format: Fmt, byte_order: ByteOrder) -> Option<DecodedValue> {
    let order = if format == Fmt::Int16uRev {
        opposite(byte_order)
    } else {
        byte_order
    };
    Some(match format {
        Fmt::Int8u => DecodedValue::Integer(i64::from(*bytes.first()?)),
        Fmt::Int8s => DecodedValue::Integer(i64::from(*bytes.first()? as i8)),
        Fmt::Int16u | Fmt::Int16uRev => DecodedValue::Integer(i64::from(read_u16(bytes, order)?)),
        Fmt::Int16s => DecodedValue::Integer(i64::from(read_i16(bytes, order)?)),
        Fmt::Int32u => DecodedValue::Integer(i64::from(read_u32(bytes, order)?)),
        Fmt::Int32s => DecodedValue::Integer(i64::from(read_i32(bytes, order)?)),
        Fmt::Float => DecodedValue::Float(f64::from(read_f32(bytes, order)?)),
        Fmt::Double => DecodedValue::Float(read_f64(bytes, order)?),
        Fmt::Rational64u => DecodedValue::UnsignedRational(
            read_u32(bytes.get(..4)?, order)?,
            read_u32(bytes.get(4..8)?, order)?,
        ),
        Fmt::Rational64s => DecodedValue::SignedRational(
            read_i32(bytes.get(..4)?, order)?,
            read_i32(bytes.get(4..8)?, order)?,
        ),
        Fmt::Str(_) => {
            let end = bytes
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(bytes.len());
            DecodedValue::String(std::str::from_utf8(&bytes[..end]).ok()?.to_string())
        }
        Fmt::Undef(_) => DecodedValue::Undefined(bytes.to_vec()),
    })
}

fn apply_print_conv(conv: PrintConv, value: &DecodedValue) -> Option<String> {
    match conv {
        PrintConv::None => None,
        PrintConv::IntEnum(map) => {
            let value = value.integer()?;
            map.binary_search_by_key(&value, |(key, _)| *key)
                .ok()
                .map(|index| map[index].1.to_string())
        }
        PrintConv::StrEnum(map) => {
            let key = value.enum_key()?;
            map.iter()
                .find(|(candidate, _)| *candidate == key)
                .map(|(_, rendered)| (*rendered).to_string())
        }
        PrintConv::Expr(expression) => expression.apply(value.number()?),
    }
}

const fn opposite(order: ByteOrder) -> ByteOrder {
    match order {
        ByteOrder::Big => ByteOrder::Little,
        ByteOrder::Little => ByteOrder::Big,
    }
}

fn read_u16(bytes: &[u8], order: ByteOrder) -> Option<u16> {
    let bytes: [u8; 2] = bytes.get(..2)?.try_into().ok()?;
    Some(match order {
        ByteOrder::Big => u16::from_be_bytes(bytes),
        ByteOrder::Little => u16::from_le_bytes(bytes),
    })
}

fn read_i16(bytes: &[u8], order: ByteOrder) -> Option<i16> {
    read_u16(bytes, order).map(|value| value as i16)
}

fn read_u32(bytes: &[u8], order: ByteOrder) -> Option<u32> {
    let bytes: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
    Some(match order {
        ByteOrder::Big => u32::from_be_bytes(bytes),
        ByteOrder::Little => u32::from_le_bytes(bytes),
    })
}

fn read_i32(bytes: &[u8], order: ByteOrder) -> Option<i32> {
    read_u32(bytes, order).map(|value| value as i32)
}

fn read_f32(bytes: &[u8], order: ByteOrder) -> Option<f32> {
    read_u32(bytes, order).map(f32::from_bits)
}

fn read_f64(bytes: &[u8], order: ByteOrder) -> Option<f64> {
    let bytes: [u8; 8] = bytes.get(..8)?.try_into().ok()?;
    Some(match order {
        ByteOrder::Big => f64::from_be_bytes(bytes),
        ByteOrder::Little => f64::from_le_bytes(bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exiftool_tables::{ALL_BINARY_TABLES, ExprId, Mask, Omitted, find_table};

    #[test]
    fn generated_pentax_layout_decodes_offsets_types_and_conversions() {
        let mut data = vec![0; 177];
        data[..22].copy_from_slice(b"PENTAX DIGITAL CAMERA\0");
        data[42..46].copy_from_slice(&28_u32.to_le_bytes());
        data[46..50].copy_from_slice(&10_u32.to_le_bytes());
        data[68..70].copy_from_slice(&2_u16.to_le_bytes());
        data[72..76].copy_from_slice(&71_u32.to_le_bytes());
        data[76..80].copy_from_slice(&10_u32.to_le_bytes());
        data[175..177].copy_from_slice(&200_u16.to_le_bytes());

        let table = find_table("Pentax", "MOV").expect("generated Pentax::MOV table");
        let decode = decode_binary_table(table, &data, ByteOrder::Little);
        let get = |name: &str| {
            decode
                .fields()
                .iter()
                .find(|decoded| decoded.field.name == name)
        };

        assert_eq!(
            get("Make").and_then(DecodedField::emit),
            Some(TagValue::String("PENTAX DIGITAL CAMERA".into()))
        );
        assert_eq!(
            get("FNumber").and_then(DecodedField::emit),
            Some(TagValue::String("2.8".to_string()))
        );
        assert_eq!(
            get("WhiteBalance").and_then(DecodedField::emit),
            Some(TagValue::String("Shade".to_string()))
        );
        assert_eq!(
            get("FocalLength").and_then(DecodedField::emit),
            Some(TagValue::String("7.1 mm".to_string()))
        );
        assert_eq!(
            get("ISO").and_then(DecodedField::emit),
            Some(TagValue::Integer(200))
        );
        // `ExposureTime` (Pentax.pm's `%MOV` key 38) carries a `ValueConv` this
        // schema does not reproduce, so `emit` refuses it -- the field is not
        // simply absent; it decoded fine and is withheld.
        assert!(get("ExposureTime").unwrap().field.omitted.value_conv);
        assert_eq!(get("ExposureTime").and_then(DecodedField::emit), None);
    }

    /// A fractional key without a `Mask` no longer stays refused (Step 11):
    /// ExifTool.pm:9957 reads the whole word at `floor(index)` regardless of
    /// `Mask`, so `BitField` below decodes the whole byte at index 2 -- 0xFF,
    /// i.e. 255 -- and its `PrintConv` renders that word, not a bit slice of
    /// it.
    #[test]
    fn reversed_endian_and_maskless_fractional_fields_decode() {
        static FIELDS: &[Field] = &[
            Field {
                index: 0,
                sub: None,
                name: "Reversed",
                format: Some(Fmt::Int16uRev),
                count: 1,
                mask: None,
                omitted: Omitted::NONE,
                print_conv: PrintConv::None,
                subdir: None,
            },
            Field {
                index: 2,
                sub: Some(1),
                name: "BitField",
                format: None,
                count: 1,
                mask: None,
                omitted: Omitted::NONE,
                print_conv: PrintConv::Expr(ExprId::Sprintf0fValB74070),
                subdir: None,
            },
            Field {
                index: 4,
                sub: None,
                name: "ThreeValues",
                format: Some(Fmt::Int16u),
                count: 3,
                mask: None,
                omitted: Omitted::NONE,
                print_conv: PrintConv::None,
                subdir: None,
            },
        ];
        static TABLE: BinaryTable = BinaryTable {
            module: "Test",
            table: "Endian",
            group0: "",
            group2: "",
            first_entry: 0,
            default_format: Fmt::Int8u,
            offsets_sound_until: None,
            fields: FIELDS,
            variants: &[],
        };

        let decode = decode_binary_table(
            &TABLE,
            &[0x12, 0x34, 0xff, 0, 0, 1, 0, 2, 0, 3],
            ByteOrder::Big,
        );
        let fields = decode.fields();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].emit(), Some(TagValue::Integer(0x3412)));
        assert_eq!(
            fields[1].emit(),
            Some(TagValue::String("255".to_string())),
            "BitField has no Mask, so it reports the whole byte at index 2 \
             (0xFF), rendered through its PrintConv"
        );
        assert_eq!(
            fields[2].emit(),
            Some(TagValue::Array(vec![
                TagValue::Integer(1),
                TagValue::Integer(2),
                TagValue::Integer(3),
            ])),
        );
    }

    /// The whole point of a fractional key: several tags share one word, and
    /// each `Mask` picks a different slice of it. `byte_offset` uses only the
    /// integer part, which is ExifTool's `int($index) * $increment`, so all
    /// four of these read the same `int16u` and differ only in their mask --
    /// or, for `WholeWord`, in having none at all (Step 11: the maskless
    /// sibling reports the entire word, not a slice of it).
    #[test]
    fn fractional_entries_sharing_a_word_decode_to_their_own_slices() {
        static FIELDS: &[Field] = &[
            Field {
                index: 4,
                sub: Some(1),
                name: "Low",
                format: Some(Fmt::Int16u),
                count: 1,
                mask: Some(Mask {
                    bits: 0xf,
                    shift: 0,
                }),
                omitted: Omitted::NONE,
                print_conv: PrintConv::None,
                subdir: None,
            },
            Field {
                index: 4,
                sub: Some(2),
                name: "Middle",
                format: Some(Fmt::Int16u),
                count: 1,
                mask: Some(Mask {
                    bits: 0xf00,
                    shift: 8,
                }),
                omitted: Omitted::NONE,
                print_conv: PrintConv::IntEnum(&[(0xB, "Eleven")]),
                subdir: None,
            },
            Field {
                index: 4,
                sub: Some(3),
                name: "Top",
                format: Some(Fmt::Int16u),
                count: 1,
                mask: Some(Mask {
                    bits: 0xf000,
                    shift: 12,
                }),
                omitted: Omitted::NONE,
                print_conv: PrintConv::None,
                subdir: None,
            },
            // Same word, no mask: Step 11 reports the whole word, not a slice.
            Field {
                index: 4,
                sub: Some(4),
                name: "WholeWord",
                format: Some(Fmt::Int16u),
                count: 1,
                mask: None,
                omitted: Omitted::NONE,
                print_conv: PrintConv::None,
                subdir: None,
            },
        ];
        static TABLE: BinaryTable = BinaryTable {
            module: "Test",
            table: "Fractional",
            group0: "",
            group2: "",
            first_entry: 0,
            default_format: Fmt::Int8u,
            offsets_sound_until: None,
            fields: FIELDS,
            variants: &[],
        };

        // int8u FORMAT, so index 4 is byte 4: the big-endian word 0xABCD.
        let data = [0, 0, 0, 0, 0xAB, 0xCD];
        let decode = decode_binary_table(&TABLE, &data, ByteOrder::Big);
        let get = |name: &str| {
            decode
                .fields()
                .iter()
                .find(|decoded| decoded.field.name == name)
        };

        assert_eq!(
            get("Low").and_then(DecodedField::emit),
            Some(TagValue::Integer(0xD))
        );
        assert_eq!(
            get("Middle").and_then(DecodedField::emit),
            Some(TagValue::String("Eleven".to_string())),
            "the enum is keyed on the slice, not on the 0xABCD word"
        );
        assert_eq!(
            get("Top").and_then(DecodedField::emit),
            Some(TagValue::Integer(0xA))
        );
        assert_eq!(
            get("WholeWord").and_then(DecodedField::emit),
            Some(TagValue::Integer(0xABCD)),
            "a fractional entry with no Mask reports the whole word at \
             floor(index), per ExifTool.pm:9957 (Step 11)"
        );
    }

    /// Before Step 11 this was 993 decoded / 11 refused: 11 fractional
    /// entries declared no `Mask`, and the runtime withheld them entirely.
    /// ExifTool.pm:9957 says otherwise -- it reads the whole word at
    /// `floor(index)` whether or not `Mask` is set -- so all 1004 fractional
    /// entries across the generated tables are decodable now, and none are
    /// refused. Pinning both halves makes a regeneration that changes the
    /// balance visible instead of silent.
    #[test]
    fn generated_fractional_entries_are_all_decodable() {
        let census = all_fractional_census();
        assert_eq!(
            census,
            FractionalCensus {
                decoded: 1004,
                refused: 0,
            },
            "fractional-entry split moved; if `just regen-tables` caused this, \
             re-measure coverage before updating the numbers"
        );

        // Every counted entry is reachable through the same predicate the
        // decoder uses, so the census cannot drift from the refusal it counts.
        let per_table: usize = ALL_BINARY_TABLES
            .iter()
            .map(|table| fractional_census(table).decoded)
            .sum();
        assert_eq!(per_table, census.decoded);
    }

    /// Named example 1 of 2 (Step 11, `OVERHAUL_OXIDEX_PLAN.md` D4):
    /// `Image::ExifTool::Pentax::AFInfoK3III`'s `0.1 => { Name => 'AFMode', ...
    /// }` (Pentax.pm) declares no `Mask`. `FORMAT => 'int16u'` at the table
    /// level and no per-field `Format` override means AFMode reads as the
    /// default `int16u`; `index: 0` is `floor(0.1)`, so ExifTool.pm:9957's
    /// `$entry = int($index) * $increment` places it at byte offset 0 -- the
    /// same word AFInfoK3III's own whole-array entry (key `0`) covers, read
    /// here as a lone `int16u` instead of an array. `PrintConv => { 0 =>
    /// 'Phase Detect', 2 => 'Contrast Detect', 255 => 'Manual Focus' }` keys
    /// on that whole word.
    #[test]
    fn pentax_afinfok3iii_afmode_decodes_the_maskless_fractional_word() {
        let table = find_table("Pentax", "AFInfoK3III").expect("generated Pentax::AFInfoK3III");
        let field = table
            .fields
            .iter()
            .find(|f| f.name == "AFMode")
            .expect("AFInfoK3III declares AFMode");
        assert_eq!(field.index, 0, "0.1's integer part is 0");
        assert_eq!(field.sub, Some(1), "0.1's fractional part is 1");
        assert_eq!(field.mask, None, "AFMode declares no Mask in Pentax.pm");

        // int16u big-endian 0x0002 at offset 0 -- ExifTool's "Contrast Detect".
        let data = [0x00, 0x02];
        let decode = decode_binary_table(table, &data, ByteOrder::Big);
        let afmode = decode
            .fields()
            .iter()
            .find(|decoded| decoded.field.name == "AFMode")
            .expect("AFMode decodes at floor(0.1) == byte offset 0");
        assert_eq!(
            afmode.emit(),
            Some(TagValue::String("Contrast Detect".to_string()))
        );
    }

    /// Named example 2 of 2 (Step 11, `OVERHAUL_OXIDEX_PLAN.md` D4):
    /// `Image::ExifTool::NikonCustom::SettingsD5`'s `12.1 => { # CSd2 Name =>
    /// 'MaxContinuousRelease' }` (NikonCustom.pm) declares no `Mask` and no
    /// `PrintConv` -- ExifTool comments it `# values: 1-100`, i.e. the raw
    /// byte is the value. The table's `FORMAT` is `int8u` and MaxContinuousRelease
    /// has no per-field `Format`, so `index: 12` (`floor(12.1)`) places it at
    /// byte offset 12 via ExifTool.pm:9957's `int($index) * $increment` (here
    /// `increment` is the int8u width, 1).
    #[test]
    fn nikoncustom_settingsd5_maxcontinuousrelease_decodes_the_maskless_fractional_word() {
        let table =
            find_table("NikonCustom", "SettingsD5").expect("generated NikonCustom::SettingsD5");
        let field = table
            .fields
            .iter()
            .find(|f| f.name == "MaxContinuousRelease")
            .expect("SettingsD5 declares MaxContinuousRelease");
        assert_eq!(field.index, 12, "12.1's integer part is 12");
        assert_eq!(field.sub, Some(1), "12.1's fractional part is 1");
        assert_eq!(
            field.mask, None,
            "MaxContinuousRelease declares no Mask in NikonCustom.pm"
        );

        let mut data = vec![0u8; 13];
        data[12] = 42; // within ExifTool's documented "values: 1-100" range
        let decode = decode_binary_table(table, &data, ByteOrder::Big);
        let max_release = decode
            .fields()
            .iter()
            .find(|decoded| decoded.field.name == "MaxContinuousRelease")
            .expect("MaxContinuousRelease decodes at floor(12.1) == byte offset 12");
        assert_eq!(max_release.emit(), Some(TagValue::Integer(42)));
    }

    /// Step 11 makes a maskless fractional field's bytes readable, but
    /// readable is not reportable: Step 10's five `Omitted` flags apply
    /// independently of *why* a field was previously withheld. A fractional
    /// field gated on a `Condition` (or any other flag) must stay refused by
    /// `emit`, exactly as a masked field with the same flag would, and must
    /// still be counted in `RefusalCounts` -- lifting one refusal reason must
    /// not silently lift the others.
    #[test]
    fn maskless_fractional_field_with_another_flag_still_refuses() {
        static FIELDS: &[Field] = &[Field {
            index: 0,
            sub: Some(1),
            name: "GatedWholeWord",
            format: None,
            count: 1,
            mask: None,
            omitted: Omitted {
                value_conv: false,
                raw_conv: false,
                condition: true,
                hook: false,
                subdirectory: false,
            },
            print_conv: PrintConv::None,
            subdir: None,
        }];
        static TABLE: BinaryTable = BinaryTable {
            module: "Test",
            table: "GatedFractional",
            group0: "",
            group2: "",
            first_entry: 0,
            default_format: Fmt::Int8u,
            offsets_sound_until: None,
            fields: FIELDS,
            variants: &[],
        };

        let decode = decode_binary_table(&TABLE, &[0x42], ByteOrder::Big);
        let fields = decode.fields();
        assert_eq!(
            fields.len(),
            1,
            "the bytes decode fine -- Step 11 makes a maskless fractional \
             field's offset and format decodable regardless of Omitted flags"
        );
        assert_eq!(
            fields[0].emit(),
            None,
            "but Condition is still set, so emit still refuses it, the same \
             as it would for a masked field carrying the same flag"
        );
        assert_eq!(
            decode.refusals().condition,
            1,
            "the refusal is still counted in RefusalCounts"
        );

        // The Step 10 escape hatch still works, unchanged by Step 11.
        const CITATION: PerlCitation = PerlCitation {
            module: "Test",
            table: "GatedFractional",
            tag: "GatedWholeWord",
            lines: "n/a (unit test fixture)",
        };
        assert!(RawAccess::new(&fields[0], Acknowledged::NONE, &CITATION).is_none());
        let access = RawAccess::new(&fields[0], Acknowledged::CONDITION, &CITATION)
            .expect("condition acknowledged");
        assert_eq!(access.raw(), &DecodedValue::Integer(0x42));
    }

    /// ExifTool reduces a masked field to `(val & Mask) >> BitShift` before any
    /// conversion, so both the value and the enum lookup depend on it. Before
    /// the schema carried `Mask`, these fields reported the whole word and then
    /// looked the enum up with it, which usually matched nothing.
    #[test]
    fn masked_fields_are_reduced_before_print_conv() {
        static FIELDS: &[Field] = &[
            Field {
                index: 0,
                sub: None,
                name: "Orientation",
                format: Some(Fmt::Int8u),
                count: 1,
                mask: Some(Mask {
                    bits: 0x7,
                    shift: 0,
                }),
                omitted: Omitted::NONE,
                print_conv: PrintConv::IntEnum(&[(1, "Upright"), (5, "Rotated")]),
                subdir: None,
            },
            // A high slice: shift is what makes the enum keys mean anything.
            Field {
                index: 1,
                sub: None,
                name: "Slice",
                format: Some(Fmt::Int8u),
                count: 1,
                mask: Some(Mask {
                    bits: 0xf0,
                    shift: 4,
                }),
                omitted: Omitted::NONE,
                print_conv: PrintConv::None,
                subdir: None,
            },
            // A PrintConv keyed on post-ValueConv values must not be applied
            // to the raw one.
            Field {
                index: 2,
                sub: None,
                name: "Converted",
                format: Some(Fmt::Int8u),
                count: 1,
                mask: None,
                omitted: Omitted {
                    value_conv: true,
                    raw_conv: false,
                    condition: false,
                    hook: false,
                    subdirectory: false,
                },
                print_conv: PrintConv::IntEnum(&[(0, "Wrong")]),
                subdir: None,
            },
        ];
        static TABLE: BinaryTable = BinaryTable {
            module: "Test",
            table: "Masked",
            group0: "",
            group2: "",
            first_entry: 0,
            default_format: Fmt::Int8u,
            offsets_sound_until: None,
            fields: FIELDS,
            variants: &[],
        };

        // 0xFD & 0x7 == 5; the unmasked 0xFD matches no enum key at all.
        let decode = decode_binary_table(&TABLE, &[0xFD, 0xA3, 0x00], ByteOrder::Big);
        let fields = decode.fields();
        assert_eq!(
            fields[0].emit(),
            Some(TagValue::String("Rotated".to_string()))
        );
        assert_eq!(fields[1].emit(), Some(TagValue::Integer(0xA)));
        assert_eq!(
            fields[2].emit(),
            None,
            "a PrintConv behind an omitted ValueConv must not be applied"
        );

        // The escape hatch: acknowledging `value_conv` reaches the raw value
        // RawAccess::new refuses without it.
        const CITATION: PerlCitation = PerlCitation {
            module: "Test",
            table: "Masked",
            tag: "Converted",
            lines: "n/a (unit test fixture)",
        };
        assert!(RawAccess::new(&fields[2], Acknowledged::NONE, &CITATION).is_none());
        let access = RawAccess::new(&fields[2], Acknowledged::VALUE_CONV, &CITATION)
            .expect("value_conv acknowledged");
        assert_eq!(access.raw(), &DecodedValue::Integer(0));
    }

    #[test]
    fn raw_access_requires_covering_every_set_flag_at_once() {
        static FIELD: Field = Field {
            index: 0,
            sub: None,
            name: "DoubleFlagged",
            format: Some(Fmt::Int8u),
            count: 1,
            mask: None,
            omitted: Omitted {
                value_conv: true,
                raw_conv: false,
                condition: true,
                hook: false,
                subdirectory: false,
            },
            print_conv: PrintConv::None,
            subdir: None,
        };
        let decoded = DecodedField {
            field: &FIELD,
            raw: DecodedValue::Integer(1),
        };
        const CITATION: PerlCitation = PerlCitation {
            module: "Test",
            table: "Masked",
            tag: "DoubleFlagged",
            lines: "n/a (unit test fixture)",
        };

        // Acknowledging one of the two set flags is not the same as
        // acknowledging both (D1: "all six at once").
        assert!(RawAccess::new(&decoded, Acknowledged::CONDITION, &CITATION).is_none());
        assert!(RawAccess::new(&decoded, Acknowledged::VALUE_CONV, &CITATION).is_none());
        assert!(
            RawAccess::new(
                &decoded,
                Acknowledged::CONDITION | Acknowledged::VALUE_CONV,
                &CITATION
            )
            .is_some()
        );
    }

    #[test]
    fn refusal_counts_tally_every_set_flag_independently() {
        static FIELDS: &[Field] = &[
            Field {
                index: 0,
                sub: None,
                name: "OnlyCondition",
                format: Some(Fmt::Int8u),
                count: 1,
                mask: None,
                omitted: Omitted {
                    value_conv: false,
                    raw_conv: false,
                    condition: true,
                    hook: false,
                    subdirectory: false,
                },
                print_conv: PrintConv::None,
                subdir: None,
            },
            Field {
                index: 1,
                sub: None,
                name: "BothConditionAndValueConv",
                format: Some(Fmt::Int8u),
                count: 1,
                mask: None,
                omitted: Omitted {
                    value_conv: true,
                    raw_conv: false,
                    condition: true,
                    hook: false,
                    subdirectory: false,
                },
                print_conv: PrintConv::None,
                subdir: None,
            },
            Field {
                index: 2,
                sub: None,
                name: "Clean",
                format: Some(Fmt::Int8u),
                count: 1,
                mask: None,
                omitted: Omitted::NONE,
                print_conv: PrintConv::None,
                subdir: None,
            },
        ];
        static TABLE: BinaryTable = BinaryTable {
            module: "Test",
            table: "Refusals",
            group0: "",
            group2: "",
            first_entry: 0,
            default_format: Fmt::Int8u,
            offsets_sound_until: None,
            fields: FIELDS,
            variants: &[],
        };

        let decode = decode_binary_table(&TABLE, &[1, 2, 3], ByteOrder::Big);
        // Both flagged fields still decode into `fields()` -- withheld from
        // `emit`, but present for `RawAccess` and counted, never dropped.
        assert_eq!(decode.fields().len(), 3);
        assert_eq!(
            decode.refusals(),
            RefusalCounts {
                value_conv: 1,
                raw_conv: 0,
                condition: 2,
                hook: 0,
                subdirectory: 0,
                offset_unsound: 0,
            }
        );
        assert_eq!(decode.refusals().total(), 3);
    }

    #[test]
    fn offset_unsound_fields_are_withheld_with_no_raw_access_path() {
        static FIELDS: &[Field] = &[
            Field {
                index: 0,
                sub: None,
                name: "BeforeBound",
                format: Some(Fmt::Int8u),
                count: 1,
                mask: None,
                omitted: Omitted::NONE,
                print_conv: PrintConv::None,
                subdir: None,
            },
            Field {
                index: 5,
                sub: None,
                name: "PastBound",
                format: Some(Fmt::Int8u),
                count: 1,
                mask: None,
                omitted: Omitted::NONE,
                print_conv: PrintConv::None,
                subdir: None,
            },
        ];
        static TABLE: BinaryTable = BinaryTable {
            module: "Test",
            table: "Unsound",
            group0: "",
            group2: "",
            first_entry: 0,
            default_format: Fmt::Int8u,
            offsets_sound_until: Some(3),
            fields: FIELDS,
            variants: &[],
        };

        let decode = decode_binary_table(&TABLE, &[0, 0, 0, 0, 0, 9], ByteOrder::Big);
        assert_eq!(decode.fields().len(), 1, "only the sound field decodes");
        assert_eq!(decode.fields()[0].field.name, "BeforeBound");
        assert_eq!(decode.refusals().offset_unsound, 1);
    }

    /// `decode_binary_table_variants` end to end: first-match-wins over a
    /// `VariantGroup`'s alternatives, and no field at all when nothing
    /// matches -- modelled on Sony ExtraInfo3's 0x0016 split (DSLR ->
    /// `MemoryCardConfiguration`, NEX -> `CameraOrientation`, everything
    /// else -> nothing; see `src/parsers/tiff/makernotes/sony/amount.rs`).
    #[test]
    fn decode_binary_table_variants_first_match_wins_or_nothing() {
        use super::cond::{Cond, Ctx, MemberValue};
        use std::collections::HashMap;

        static DSLR_FIELD: Field = Field {
            index: 0,
            sub: None,
            name: "MemoryCardConfiguration",
            format: Some(Fmt::Int8u),
            count: 1,
            mask: None,
            omitted: Omitted::NONE,
            print_conv: PrintConv::None,
            subdir: None,
        };
        static NEX_FIELD: Field = Field {
            name: "CameraOrientation",
            ..DSLR_FIELD
        };
        static ALTERNATIVES: &[(Cond, Field)] = &[
            (
                Cond::MemberRegex {
                    member: "Model",
                    pattern: "^DSLR-",
                    ignore_case: false,
                    negate: false,
                },
                DSLR_FIELD,
            ),
            (
                Cond::MemberRegex {
                    member: "Model",
                    pattern: "^NEX-",
                    ignore_case: false,
                    negate: false,
                },
                NEX_FIELD,
            ),
        ];
        static VARIANTS: &[super::cond::VariantGroup] = &[super::cond::VariantGroup {
            index: 0,
            sub: None,
            alternatives: ALTERNATIVES,
        }];
        static TABLE: BinaryTable = BinaryTable {
            module: "Test",
            table: "Variants",
            group0: "",
            group2: "",
            first_entry: 0,
            default_format: Fmt::Int8u,
            offsets_sound_until: None,
            fields: &[],
            variants: VARIANTS,
        };

        let mut dslr_members = HashMap::new();
        dslr_members.insert("Model", MemberValue::Str("DSLR-A580".to_string()));
        let decoded = decode_binary_table_variants(
            &TABLE,
            &[7],
            ByteOrder::Big,
            &mut Ctx::new(&mut dslr_members),
        );
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].field.name, "MemoryCardConfiguration");
        assert_eq!(decoded[0].emit(), Some(TagValue::Integer(7)));

        let mut nex_members = HashMap::new();
        nex_members.insert("Model", MemberValue::Str("NEX-5".to_string()));
        let decoded = decode_binary_table_variants(
            &TABLE,
            &[7],
            ByteOrder::Big,
            &mut Ctx::new(&mut nex_members),
        );
        assert_eq!(decoded[0].field.name, "CameraOrientation");

        // An SLT body matches neither alternative -- real ExifTool produces
        // no tag at all for this offset under that model, and so must this.
        let mut slt_members = HashMap::new();
        slt_members.insert("Model", MemberValue::Str("SLT-A77".to_string()));
        let decoded = decode_binary_table_variants(
            &TABLE,
            &[7],
            ByteOrder::Big,
            &mut Ctx::new(&mut slt_members),
        );
        assert!(decoded.is_empty());
    }
}
