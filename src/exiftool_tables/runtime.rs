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
use super::{ALL_BINARY_TABLES, BinaryTable, ExprValue, Field, Fmt, Omitted, PrintConv};

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
    /// The integer this value is, if it is one.
    ///
    /// Public because [`super::engine`] needs it for two ExifTool rules that
    /// are stated in terms of the raw value: `$val = ($val & $mask) >>
    /// $$tagInfo{BitShift} if $mask` (ExifTool.pm:10079) and the
    /// `next unless $val` guard on a `$`-bearing `SubDirectory` `Start`
    /// (ExifTool.pm:10128).
    #[must_use]
    pub const fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }

    fn integer(&self) -> Option<i64> {
        self.as_integer().or_else(|| match self {
            // Perl hash PrintConv keys see an integral NV (for example
            // the result of `$val + 1`) as the same key as the matching
            // IV.  Preserve that post-ValueConv lookup behaviour instead
            // of making a converted `2.0` miss an enum keyed by `2`.
            // GetValue queues ValueConv before PrintConv and evaluates
            // it before reporting: Image/ExifTool.pm:3524-3525,
            // 3530-3664 (pinned 13.59).
            Self::Float(value)
                if value.is_finite()
                    && value.fract() == 0.0
                    && *value >= i64::MIN as f64
                    && *value <= i64::MAX as f64 =>
            {
                Some(*value as i64)
            }
            _ => None,
        })
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

    /// The string ExifTool would look a `PrintConv` hash up by.
    ///
    /// ExifTool's hash `PrintConv` is a plain `$$printConv{$val}`
    /// (ExifTool.pm:3711), and `$val` for an `undef[N]` field is the raw byte
    /// string `ReadValue` returned (ExifTool.pm:6308) -- Perl draws no
    /// distinction between that and a `string` field's value, so a
    /// `%palmTypes`-style hash keyed by 8-character type/creator pairs
    /// (Palm.pm:23-52, reached from Palm.pm:95-99's `Format => 'undef[8]'`)
    /// resolves normally. Without `Undefined` here, that field fell back to
    /// its raw bytes and printed `(Binary data 8 bytes, ...)` where ExifTool
    /// prints `Mobipocket`.
    ///
    /// Bytes that are not valid UTF-8 yield `None`, which is not a loss: a
    /// generated `StrEnum`'s keys are all `&str`, so no non-UTF-8 value could
    /// have matched one.
    fn enum_key(&self) -> Option<String> {
        match self {
            Self::Integer(value) => Some(value.to_string()),
            Self::String(value) => Some(value.clone()),
            Self::Undefined(bytes) => String::from_utf8(bytes.clone()).ok(),
            _ => None,
        }
    }

    /// The text Perl interpolates for `$val` in `"Unknown ($val)"`
    /// (ExifTool.pm:3630) when a hash `PrintConv` misses.
    ///
    /// Integers and strings are their own text; a float prints the way Perl
    /// stringifies an NV; a fixed-count field's `$val` is ExifTool's
    /// space-joined element string (ExifTool.pm:6312), so the elements are
    /// joined the same way. `None` for a rational: ExifTool's raw rational is
    /// `RoundFloat($num / $den, 10)` (ExifTool.pm `GetRational64u`), a
    /// rounding this schema does not reproduce, so the caller falls back to
    /// the raw value rather than print a digit string Perl would not.
    fn perl_string(&self) -> Option<String> {
        match self {
            Self::Integer(value) => Some(value.to_string()),
            Self::Float(value) => Some(super::exprs::perl_num(*value)),
            Self::String(value) => Some(value.clone()),
            Self::Undefined(bytes) => String::from_utf8(bytes.clone()).ok(),
            Self::UnsignedRational(..) | Self::SignedRational(..) => None,
            Self::Array(values) => values
                .iter()
                .map(DecodedValue::perl_string)
                .collect::<Option<Vec<_>>>()
                .map(|parts| parts.join(" ")),
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
    /// `Field::omitted`'s six flags is set.
    ///
    /// A clean field (no flag set) renders its generated `PrintConv`. A hash
    /// `PrintConv` always renders: a key it does not carry yields ExifTool's
    /// own `"Unknown ($val)"` (ExifTool.pm:3624-3631), which is the exact
    /// conversion, not a guess. Only when the conversion yields nothing --
    /// `PrintConv::None`, an expression returning Perl `undef`, or a value
    /// this schema cannot key (a rational under a hash) -- does the raw
    /// decoded value stand in, which is the same "no conversion is honest, a
    /// guess is not" rule [`PrintConv`]'s own doc comment states. A flagged field always
    /// refuses: ExifTool ran a `ValueConv`/`RawConv`/`Condition`/`Hook`/
    /// `SubDirectory` this schema does not reproduce, and reporting the raw
    /// bytes under the real tag name would be a confident wrong value
    /// (AGENTS.md, "never approximate a conversion").
    #[must_use]
    pub fn emit(&self) -> Option<TagValue> {
        if self.field.omitted.any() {
            return None;
        }
        let value = apply_value_conv(self.field.value_conv, &self.raw)?;
        Some(match render(self.field.print_conv, &value) {
            Some(rendered) => TagValue::String(rendered),
            None => to_tag_value(&value),
        })
    }
}

/// Apply the oracle-approved ValueConv carried by a generated field.
///
/// ExifTool reads/masks at `Image/ExifTool.pm:10076-10079` and passes raw
/// data to FoundTag at :10163. `GetValue` then queues ValueConv ahead of
/// PrintConv (:3524-3525) and evaluates it at :3530-3664. `None` means either
/// there was no ValueConv or the conversion returned Perl `undef`; the latter
/// suppresses the tag rather than falling back to its raw bytes.
#[must_use]
pub fn apply_value_conv(
    conversion: Option<super::ExprId>,
    value: &DecodedValue,
) -> Option<DecodedValue> {
    let Some(conversion) = conversion else {
        return Some(value.clone());
    };
    let output = match value {
        DecodedValue::Integer(_)
        | DecodedValue::Float(_)
        | DecodedValue::UnsignedRational(..)
        | DecodedValue::SignedRational(..) => conversion.value_num(value.number()?),
        DecodedValue::String(value) => conversion.value_str(value),
        DecodedValue::Undefined(value) => conversion.value_bytes(value),
        // A fixed-count field's ValueConv sees the space-joined list ReadValue
        // built (ExifTool.pm:6286 ff.) and re-splits it; the list-domain
        // compiler (`tools/exiftool-tables/exprs.py::_compile_list`, checked
        // by verify_exprs.py at every element count the tables carry) takes
        // the elements as numbers. R2 refused this branch until that
        // compiler existed; it still refuses an array whose elements are not
        // all numeric, and a scalar ExprId applied element-wise remains an
        // approximation this branch never makes -- `value_list` is None for
        // every ExprId that is not list-domain.
        DecodedValue::Array(values) => {
            let numbers: Option<Vec<f64>> = values.iter().map(DecodedValue::number).collect();
            conversion.value_list(&numbers?)
        }
    }?;
    Some(match output {
        ExprValue::Number(value)
            if value.is_finite()
                && value.fract() == 0.0
                && value >= i64::MIN as f64
                && value <= i64::MAX as f64 =>
        {
            DecodedValue::Integer(value as i64)
        }
        ExprValue::Number(value) => DecodedValue::Float(value),
        ExprValue::String(value) => DecodedValue::String(value),
    })
}

/// Render a [`DecodedValue`] as a [`TagValue`] with no conversion applied.
///
/// The fallback [`DecodedField::emit`] and [`RawAccess::emit_raw`] both use
/// when a `PrintConv` does not apply (absent, an expression that returned
/// `undef`, or a hash over a value it cannot key) -- the raw value stands in,
/// honestly, rather than a guessed string. A hash miss on a keyable value is
/// NOT this case: [`render`] gives it ExifTool's own `Unknown ($val)`.
#[must_use]
pub fn to_tag_value(value: &DecodedValue) -> TagValue {
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
        DecodedValue::Array(values) => TagValue::Array(values.iter().map(to_tag_value).collect()),
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
    /// The caller has supplied the `PrintConv` this schema refused -- i.e. it
    /// renders the field itself from [`RawAccess::raw`] rather than through
    /// [`RawAccess::emit_raw`], which would fall back to the raw value
    /// precisely because the generated `print_conv` is `PrintConv::None`
    /// here. Acknowledging this flag and then calling `emit_raw` reproduces
    /// exactly the wrong output the flag exists to prevent.
    pub const PRINT_CONV: Self = Self(1 << 5);

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
            && (!omitted.print_conv || self.contains(Self::PRINT_CONV))
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
        match render(self.field.field.print_conv, self.raw()) {
            Some(rendered) => TagValue::String(rendered),
            None => to_tag_value(self.raw()),
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
/// `Field::omitted`'s six flags is set, or when its offset falls past the
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
    /// Fields carrying a `PrintConv` the generator refused to reproduce
    /// (`Omitted::print_conv`). Counted apart from the four above because
    /// what it withholds is different in kind: those fields have a raw value
    /// that is not the reported value, whereas these have a raw value
    /// ExifTool would have rendered as a *string*. Before the flag existed
    /// they were not withheld at all -- they were emitted raw, silently.
    pub print_conv: usize,
    /// Fields whose offset sits past `offsets_sound_until` -- unlike the six
    /// flags above, there is no [`RawAccess`] escape for this one: the static
    /// `index * increment` formula is not just unmodeled here, it is not
    /// reliably the field's real byte offset at all, so there is no `raw` to
    /// acknowledge reading.
    pub offset_unsound: usize,
}

impl RefusalCounts {
    /// Sum across all seven reasons.
    #[must_use]
    pub const fn total(self) -> usize {
        self.value_conv
            + self.raw_conv
            + self.condition
            + self.hook
            + self.subdirectory
            + self.print_conv
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
            print_conv: self.print_conv + other.print_conv,
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
        if omitted.print_conv {
            refusals.print_conv += 1;
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
    // Step 26: a `var_*` field is modeled in the schema but never decoded.
    // Its width depends on the bytes, so reading it at the static offset
    // would report whatever happens to sit there under a real ExifTool tag
    // name. `Fmt::Var` carries which rule applies (`VarFmt::kind`) precisely
    // so a future walker can implement it; until one does, this refuses.
    // This sits ahead of the Step 28 enablement branch because it holds on
    // both paths: `Fmt::Var(_).size()` is 0, which the shared `read_value`
    // port refuses too, but refusing it here keeps the reason legible.
    if matches!(format, Fmt::Var(_)) {
        return None;
    }
    // pstring carries its own length in the byte at `offset`
    // (ExifTool.pm:9972-9975: `$count = Get8u($dataPt, ($entry++)+$dirStart)`).
    // The count byte is consumed here so `decode_value_of` sees only the
    // payload. ExifTool does not touch `$varSize` for a pstring, so no later
    // field's offset depends on the length we read -- a short or oversized
    // count can only lose this one field, never shift the rest of the table.
    // Also ahead of the enablement branch: `Fmt::PString.size()` is 1 (the
    // count byte), so handing it to the shared port would report that count
    // byte as the field's value rather than reading the payload it prefixes.
    if format == Fmt::PString {
        let length = usize::from(*data.get(offset)?);
        let bytes = data.get(offset + 1..offset.checked_add(1 + length)?)?;
        let raw = decode_value_of(bytes, format, byte_order)?;
        return Some(DecodedField { field, raw });
    }
    let raw = if table.enabled() {
        // Step 28: an ENABLED table reads through the one shared `ReadValue`
        // port (ExifTool.pm:6286), which is the whole point of the fold. The
        // observable difference from the legacy path below is ExifTool's
        // count shortening at ExifTool.pm:6301-6303 -- a field whose array
        // or string runs off the end of the record reports the elements that
        // fit, where the legacy path drops it entirely. `more` is the bytes
        // of record remaining at this field, ExifTool's own `$more`
        // (ExifTool.pm:9963).
        let more = i64::try_from(data.len().saturating_sub(offset)).unwrap_or(i64::MAX);
        super::engine::read_value(data, offset, format, field.count, more, byte_order)?
    } else {
        // Not enabled: unchanged strict read. A table that has not passed
        // both gates must behave EXACTLY as it did before Step 28, or the
        // per-table A/B that gate B is measured with has no control group
        // (design section 4: "an engine that enables 591 tables at once would
        // produce a delta nobody can attribute").
        let width = usize::try_from(format.size()).ok()?;
        let byte_count = width.checked_mul(field.count)?;
        let bytes = data.get(offset..offset.checked_add(byte_count)?)?;
        if field.count == 1 {
            decode_value_of(bytes, format, byte_order)?
        } else {
            let values = bytes
                .chunks_exact(width)
                .map(|chunk| decode_value_of(chunk, format, byte_order))
                .collect::<Option<Vec<_>>>()?;
            DecodedValue::Array(values)
        }
    };
    // ExifTool.pm:10079. A mask on a non-integer is a construct this schema
    // cannot express; refuse rather than report the unmasked value.
    let raw = super::engine::apply_mask(raw, field.mask)?;
    Some(DecodedField { field, raw })
}

/// Decode ONE element of `format` from `bytes`.
///
/// Public so [`super::engine`]'s [`super::engine::read_value`] -- the single
/// `ReadValue` port (ExifTool.pm:6286) all three folded engines now share --
/// can reuse the per-format decoding without a second copy of it. Callers
/// wanting ExifTool's count/shortening/string rules want that function, not
/// this one: this reads exactly `format.size()` bytes and applies none of
/// ExifTool.pm:6301-6311.
#[must_use]
pub fn decode_value_of(bytes: &[u8], format: Fmt, byte_order: ByteOrder) -> Option<DecodedValue> {
    // `int16uRev`/`int32uRev` are ExifTool's DoUnpackRev (ExifTool.pm:6087-6088):
    // the same unpack template read against the *opposite* byte order to the
    // rest of the record. Canon really does mix endianness inside one table.
    let order = if matches!(format, Fmt::Int16uRev | Fmt::Int32uRev) {
        opposite(byte_order)
    } else {
        byte_order
    };
    Some(match format {
        Fmt::Int8u => DecodedValue::Integer(i64::from(*bytes.first()?)),
        Fmt::Int8s => DecodedValue::Integer(i64::from(*bytes.first()? as i8)),
        Fmt::Int16u | Fmt::Int16uRev => DecodedValue::Integer(i64::from(read_u16(bytes, order)?)),
        Fmt::Int16s => DecodedValue::Integer(i64::from(read_i16(bytes, order)?)),
        Fmt::Int32u | Fmt::Int32uRev => DecodedValue::Integer(i64::from(read_u32(bytes, order)?)),
        Fmt::Int32s => DecodedValue::Integer(i64::from(read_i32(bytes, order)?)),
        // Get64u/Get64s (%readValueProc, ExifTool.pm:6252-6253). An int64u
        // above i64::MAX has no i64 representation; refuse rather than wrap it
        // into a negative number that would print as a real value.
        Fmt::Int64u => DecodedValue::Integer(i64::try_from(read_u64(bytes, order)?).ok()?),
        Fmt::Int64s => DecodedValue::Integer(read_u64(bytes, order)? as i64),
        Fmt::Float => DecodedValue::Float(f64::from(read_f32(bytes, order)?)),
        Fmt::Double => DecodedValue::Float(read_f64(bytes, order)?),
        // GetRational32u/s (ExifTool.pm:6100-6106): a 16/16 pair in four bytes.
        Fmt::Rational32u => DecodedValue::UnsignedRational(
            u32::from(read_u16(bytes.get(..2)?, order)?),
            u32::from(read_u16(bytes.get(2..4)?, order)?),
        ),
        Fmt::Rational32s => DecodedValue::SignedRational(
            i32::from(read_i16(bytes.get(..2)?, order)?),
            i32::from(read_i16(bytes.get(2..4)?, order)?),
        ),
        Fmt::Rational64u => DecodedValue::UnsignedRational(
            read_u32(bytes.get(..4)?, order)?,
            read_u32(bytes.get(4..8)?, order)?,
        ),
        Fmt::Rational64s => DecodedValue::SignedRational(
            read_i32(bytes.get(..4)?, order)?,
            read_i32(bytes.get(4..8)?, order)?,
        ),
        // The four GetFixed* subs (ExifTool.pm:6121-6144). Each divides by a
        // fixed denominator and then drops the insignificant digits by the
        // `int($val * 10^k + copysign(0.5))/10^k` idiom -- reproduced exactly,
        // including the sign of the rounding bias, which differs between the
        // signed and unsigned forms.
        Fmt::Fixed16s => {
            let value = f64::from(read_i16(bytes, order)?) / f64::from(0x100);
            DecodedValue::Float(round_fixed(value, 1e3, value < 0.0))
        }
        Fmt::Fixed16u => {
            let value = f64::from(read_u16(bytes, order)?) / f64::from(0x100);
            DecodedValue::Float(round_fixed(value, 1e3, false))
        }
        Fmt::Fixed32s => {
            let value = f64::from(read_i32(bytes, order)?) / f64::from(0x10000);
            // GetFixed32s biases by -0.5 unless $val > 0 -- note this is the
            // `>` test ExifTool writes, so exactly 0.0 takes the negative arm.
            DecodedValue::Float(round_fixed(value, 1e5, value <= 0.0))
        }
        Fmt::Fixed32u => {
            let value = f64::from(read_u32(bytes, order)?) / f64::from(0x10000);
            DecodedValue::Float(round_fixed(value, 1e5, false))
        }
        Fmt::Extended => DecodedValue::Float(read_extended(bytes, order)?),
        // pstring: leading int8u count, then that many bytes of string
        // (ExifTool.pm:9972-9975). `decode_field` has already sliced `bytes` to
        // exactly the payload, so the length byte is gone by the time we get
        // here -- this arm and Str share the truncate-at-NUL rule.
        Fmt::PString | Fmt::Str(_) => {
            let end = bytes
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(bytes.len());
            DecodedValue::String(std::str::from_utf8(&bytes[..end]).ok()?.to_string())
        }
        Fmt::Undef(_) => DecodedValue::Undefined(bytes.to_vec()),
        // Unreachable via `decode_field`, which refuses `Fmt::Var` before it
        // gets here. Spelled out rather than left to a catch-all so that
        // adding a genuinely decodable format cannot silently land in a
        // `_ => None` arm and be dropped without anyone noticing.
        Fmt::Var(_) => return None,
    })
}

/// ExifTool's `int($val * $scale + ±0.5) / $scale`, the idiom every `GetFixed*`
/// sub ends with (ExifTool.pm:6121-6144). `negative_bias` selects which sign
/// the 0.5 carries; the four subs do not agree on the test that picks it, so
/// the caller states it rather than re-deriving it here.
fn round_fixed(value: f64, scale: f64, negative_bias: bool) -> f64 {
    let bias = if negative_bias { -0.5 } else { 0.5 };
    // Perl's int() truncates toward zero, which is Rust's `f64::trunc`.
    (value * scale + bias).trunc() / scale
}

/// 80-bit IEEE extended precision, per `GetExtended` (Writer.pl:4498-4507).
///
/// ExifTool reads the 16-bit exponent and the 64-bit significand from opposite
/// ends of the ten bytes depending on byte order (`$pt = GetByteOrder() eq 'MM'
/// ? 0 : 2`), then returns `$sign * $sig * 2 ** $exp` with the exponent
/// biased by `-16383 - 63` -- the extra 63 fractionalizes the significand,
/// whose leading integer bit is explicit in this format (unlike IEEE double).
fn read_extended(bytes: &[u8], order: ByteOrder) -> Option<f64> {
    let exponent_at = match order {
        ByteOrder::Big => 0,
        ByteOrder::Little => 2,
    };
    let exponent = read_u16(bytes.get(exponent_at..exponent_at + 2)?, order)?;
    let significand_at = 2 - exponent_at;
    let significand = read_u64(bytes.get(significand_at..significand_at + 8)?, order)?;
    let sign = if exponent & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::from(exponent & 0x7fff) - 16383 - 63;
    Some(sign * significand as f64 * 2f64.powi(exponent))
}

fn read_u64(bytes: &[u8], order: ByteOrder) -> Option<u64> {
    let bytes: [u8; 8] = bytes.get(..8)?.try_into().ok()?;
    Some(match order {
        ByteOrder::Big => u64::from_be_bytes(bytes),
        ByteOrder::Little => u64::from_le_bytes(bytes),
    })
}

/// Render `value` through `conv`, or `None` when the conversion does not
/// apply (absent, an expression that returns `undef`, or a value the
/// conversion cannot key).
///
/// A hash `PrintConv` (`IntEnum`/`StrEnum`/`PartialEnumInt`/`Bitmask`) never
/// returns `None` for a value it can key: ExifTool's hash lookup has its own
/// miss rendering, `"Unknown ($val)"` (ExifTool.pm:3624-3631, ported as
/// [`unknown_fallback`]), and reproducing it IS the exact conversion. Until
/// 4b-i `IntEnum`/`StrEnum` stayed silent on a miss and the raw value stood
/// in, which is not what ExifTool prints: the pinned 13.59 oracle over the
/// corpus reports `ICC_Profile:PrimaryPlatform` as `Unknown ()` in 24 files
/// and `Unknown (SEC)` in 3 (blank / unlisted signature), where the raw
/// fallback would have printed `` and `SEC`.
///
/// Public for [`super::engine`], which reaches the same rendering decision at
/// the end of its own walk; a second copy would be a second place for the
/// "a guess is worse than the raw value" rule to drift.
#[must_use]
pub fn render(conv: PrintConv, value: &DecodedValue) -> Option<String> {
    match conv {
        PrintConv::None => None,
        PrintConv::IntEnum(map) => Some(match value.integer() {
            Some(key) => map
                .binary_search_by_key(&key, |(candidate, _)| *candidate)
                .ok()
                .map_or_else(
                    || unknown_fallback(key, false),
                    |index| map[index].1.to_string(),
                ),
            // Not an integer (a non-integral float, a string, a fixed-count
            // list): `$$conv{$val}` misses an integer-keyed hash and ExifTool
            // renders the same fallback around Perl's text for `$val`.
            None => unknown_text(&value.perl_string()?),
        }),
        PrintConv::StrEnum(map) => {
            let key = value.enum_key()?;
            Some(
                map.iter()
                    .find(|(candidate, _)| *candidate == key)
                    .map_or_else(
                        || unknown_text(&key),
                        |(_, rendered)| (*rendered).to_string(),
                    ),
            )
        }
        PrintConv::Expr(expression) => match value {
            DecodedValue::Integer(_)
            | DecodedValue::Float(_)
            | DecodedValue::UnsignedRational(..)
            | DecodedValue::SignedRational(..) => expression.apply(value.number()?),
            DecodedValue::String(value) => expression.apply_str(value),
            DecodedValue::Undefined(value) => expression.apply_bytes(value),
            // A fixed-count field's PrintConv on the elements as numbers --
            // see `apply_value_conv`'s Array arm for the list domain.
            DecodedValue::Array(values) => {
                let numbers: Option<Vec<f64>> = values.iter().map(DecodedValue::number).collect();
                expression.apply_list(&numbers?)
            }
        },
        PrintConv::Bitmask { exact, bits } => {
            let value = value.integer()?;
            Some(
                exact
                    .binary_search_by_key(&value, |(key, _)| *key)
                    .ok()
                    .map(|index| exact[index].1.to_string())
                    .unwrap_or_else(|| decode_bits(value, bits)),
            )
        }
        PrintConv::PartialEnumInt {
            exact,
            other,
            print_hex,
        } => {
            let value = value.integer()?;
            Some(
                exact
                    .binary_search_by_key(&value, |(key, _)| *key)
                    .ok()
                    .map(|index| exact[index].1.to_string())
                    .or_else(|| other.and_then(|id| id.apply(value)))
                    .unwrap_or_else(|| unknown_fallback(value, print_hex)),
            )
        }
    }
}

/// `Image::ExifTool::DecodeBits` (ExifTool.pm:6385-6407), restricted to the
/// single-word case: `$bits` (`BitsPerWord`) defaults to 32, and every
/// BITMASK-carrying `PrintConv` in the pinned 13.59 binary-table corpus
/// leaves it unset (`codegen.py`'s `bitmask_emitted` census: 61 fields, none
/// declaring `BitsPerWord`, none needing a second space-separated `$vals`
/// word). ExifTool's loop:
///
/// ```perl
/// foreach $val (split ' ', $vals) {
///     for ($i=0; $i<$bits; ++$i) {
///         next unless $val & (1 << $i);
///         my $n = $i + $num;
///         if (not $lookup) { push @bitList, $n; }
///         elsif ($$lookup{$n}) { push @bitList, $$lookup{$n}; }
///         else { push @bitList, "[$n]"; }
///     }
///     $num += $bits;
/// }
/// return '(none)' unless @bitList;
/// return join($lookup ? ', ' : ',', @bitList);
/// ```
///
/// This port always passes a `lookup` (the caller's static `BITMASK`
/// sub-hash -- possibly `&[]` when ExifTool's own sub-hash is empty, which
/// is still a truthy hashref in Perl, so every set bit there renders `[n]`
/// exactly as it would upstream), so the join separator is always `", "`;
/// the no-lookup `@bitList` (a bare bit-number list joined by a bare `,`)
/// branch is out of scope for this generated-schema caller. It is also out
/// of scope for the hand-written parsers this function's other callers
/// (`apple.rs`, `lnk.rs`, `table_ifd.rs`, `binary_subdir.rs`,
/// `camera_info.rs`'s `Pc::BitMask`) replaced: every one of them already
/// passed a lookup table too, just with its own (divergent, in
/// `camera_info.rs`'s case buggy -- see that file's `Pc::BitMask` doc
/// comment) copy of this same loop.
#[must_use]
pub fn decode_bits(val: i64, lookup: &[(u32, &str)]) -> String {
    let bits = val as u64;
    let mut parts = Vec::new();
    for i in 0..32u32 {
        if bits & (1u64 << i) == 0 {
            continue;
        }
        match lookup.iter().find(|(bit, _)| *bit == i) {
            Some((_, name)) => parts.push((*name).to_string()),
            None => parts.push(format!("[{i}]")),
        }
    }
    if parts.is_empty() {
        "(none)".to_string()
    } else {
        parts.join(", ")
    }
}

/// ExifTool's own fallback for an unmatched hash `PrintConv`
/// (ExifTool.pm:3624-3631):
///
/// ```perl
/// if ($$tagInfo{PrintHex} and defined $val and IsInt($val) and $convType eq 'PrintConv') {
///     $value = sprintf('Unknown (0x%x)', $val);
/// } else {
///     $value = "Unknown ($val)";
/// }
/// ```
///
/// `IsInt($val)` is trivially true here -- every caller passes an integer
/// key; [`unknown_text`] is the same form for a key that is not an integer,
/// where `PrintHex` cannot apply -- so the only live condition is `PrintHex`. Perl's `%x` on a
/// negative value formats its native-width two's-complement bit pattern;
/// `val as u64` reproduces that for a 64-bit build.
#[must_use]
pub fn unknown_fallback(val: i64, print_hex: bool) -> String {
    if print_hex {
        format!("Unknown (0x{:x})", val as u64)
    } else {
        format!("Unknown ({val})")
    }
}

/// [`unknown_fallback`] for a key that is not an integer: the plain
/// `"Unknown ($val)"` arm of ExifTool.pm:3630, with `$val` as Perl would
/// interpolate it -- a string key verbatim (an empty signature reads
/// `Unknown ()`, exactly what the pinned oracle prints for a blank
/// `PrimaryPlatform`), a float or a space-joined list through
/// `DecodedValue::perl_string`. `PrintHex` cannot apply: `IsInt($val)` is
/// false for every value that reaches here.
#[must_use]
pub fn unknown_text(text: &str) -> String {
    format!("Unknown ({text})")
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
    use crate::exiftool_tables::TagGroups;
    use crate::exiftool_tables::{ALL_BINARY_TABLES, ExprId, Mask, Omitted, OtherId, find_table};

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
        // Pentax.pm:1472-1478 runs `$val * 1e-5` before PrintExposureTime.
        // R2 carries that oracle-approved ExprId instead of withholding the
        // field under `omitted.value_conv`; this zero fixture therefore
        // reaches the same `0` display ExifTool's helper returns.
        assert!(get("ExposureTime").unwrap().field.value_conv.is_some());
        assert!(!get("ExposureTime").unwrap().field.omitted.value_conv);
        assert_eq!(
            get("ExposureTime").and_then(DecodedField::emit),
            Some(TagValue::String("0".to_string()))
        );
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
                value_conv: None,
                print_conv: PrintConv::None,
                subdir: None,
                hook: &[],
                groups: TagGroups::NONE,
            },
            Field {
                index: 2,
                sub: Some(1),
                name: "BitField",
                format: None,
                count: 1,
                mask: None,
                omitted: Omitted::NONE,
                value_conv: None,
                print_conv: PrintConv::Expr(ExprId::Sprintf0fValB74070),
                subdir: None,
                hook: &[],
                groups: TagGroups::NONE,
            },
            Field {
                index: 4,
                sub: None,
                name: "ThreeValues",
                format: Some(Fmt::Int16u),
                count: 3,
                mask: None,
                omitted: Omitted::NONE,
                value_conv: None,
                print_conv: PrintConv::None,
                subdir: None,
                hook: &[],
                groups: TagGroups::NONE,
            },
        ];
        static TABLE: BinaryTable = BinaryTable {
            module: "Test",
            table: "Endian",
            group0: "",
            group1: "",
            group2: "",
            first_entry: 0,
            default_format: Fmt::Int8u,
            offsets_sound_until: None,
            priority: None,
            gate_a: super::super::GateA { blocked_by: &[] },
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
                value_conv: None,
                print_conv: PrintConv::None,
                subdir: None,
                hook: &[],
                groups: TagGroups::NONE,
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
                value_conv: None,
                print_conv: PrintConv::IntEnum(&[(0xB, "Eleven")]),
                subdir: None,
                hook: &[],
                groups: TagGroups::NONE,
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
                value_conv: None,
                print_conv: PrintConv::None,
                subdir: None,
                hook: &[],
                groups: TagGroups::NONE,
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
                value_conv: None,
                print_conv: PrintConv::None,
                subdir: None,
                hook: &[],
                groups: TagGroups::NONE,
            },
        ];
        static TABLE: BinaryTable = BinaryTable {
            module: "Test",
            table: "Fractional",
            group0: "",
            group1: "",
            group2: "",
            first_entry: 0,
            default_format: Fmt::Int8u,
            offsets_sound_until: None,
            priority: None,
            gate_a: super::super::GateA { blocked_by: &[] },
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

    /// The behaviour change `Omitted::print_conv` exists to produce, shown
    /// on a real generated table rather than a fixture.
    ///
    /// `Nikon::PictureControl` PictureControlName is the pure case: ExifTool
    /// renders it through `Image::ExifTool::Nikon::FormatString`
    /// (`Nikon.pm:3504`, `:13526-13551`), and this schema reproduces no part
    /// of that -- `FormatString`'s first branch is a function of
    /// `$et->Options('LimitLongValues')` (`Nikon.pm:13530`), not of `$val`,
    /// which is outside the "pure function of the value" boundary
    /// `exprs.py`'s registries draw. The field carries no `ValueConv`,
    /// `RawConv`, `Condition`, `Hook` or `SubDirectory`, so before this flag
    /// existed NOTHING withheld it: `emit()` returned the raw 20-byte string
    /// `"AUTO                "` under ExifTool's own `PictureControlName`,
    /// where ExifTool prints `"Auto"`.
    ///
    /// Two assertions, and the second is the point: it is not enough that
    /// `emit` refuses, the refusal has to be COUNTED, or a caller reading
    /// `RefusalCounts` cannot tell a withheld field from one ExifTool has no
    /// tag for.
    #[test]
    fn a_refused_print_conv_is_withheld_and_counted() {
        let table = crate::exiftool_tables::find_table("Nikon", "PictureControl")
            .expect("Nikon::PictureControl is in the generated set");
        let field = table
            .fields
            .iter()
            .find(|f| f.name == "PictureControlName")
            .expect("PictureControlName is transcribed");
        assert!(
            field.omitted.print_conv,
            "the whole test rests on this field being the refused-PrintConv case",
        );

        // 4 bytes of PictureControlVersion, then the 20-byte name Nikon
        // actually writes (all caps, space padded), then the 20-byte
        // PictureControlBase at offset 24 -- the table's other refused
        // field, included so the count below is over both rather than over
        // a buffer that happens to stop before the second one.
        let mut data = b"0300".to_vec();
        data.extend_from_slice(b"AUTO                ");
        data.extend_from_slice(b"AUTO                ");
        let decode = decode_binary_table(table, &data, ByteOrder::Big);

        let decoded = decode
            .fields()
            .iter()
            .find(|d| d.field.name == "PictureControlName")
            .expect("the bytes are there, only the meaning is refused");
        assert_eq!(
            decoded.emit(),
            None,
            "a refused PrintConv must withhold the field, not report `AUTO` \
             where ExifTool prints `Auto`",
        );
        assert_eq!(
            decode.refusals().print_conv,
            2,
            "PictureControlName + PictureControlBase"
        );
    }

    /// Step 11 makes a maskless fractional field's bytes readable, but
    /// readable is not reportable: Step 10's `Omitted` flags apply
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
                print_conv: false,
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        }];
        static TABLE: BinaryTable = BinaryTable {
            module: "Test",
            table: "GatedFractional",
            group0: "",
            group1: "",
            group2: "",
            first_entry: 0,
            default_format: Fmt::Int8u,
            offsets_sound_until: None,
            priority: None,
            gate_a: super::super::GateA { blocked_by: &[] },
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
                value_conv: None,
                print_conv: PrintConv::IntEnum(&[(1, "Upright"), (5, "Rotated")]),
                subdir: None,
                hook: &[],
                groups: TagGroups::NONE,
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
                value_conv: None,
                print_conv: PrintConv::None,
                subdir: None,
                hook: &[],
                groups: TagGroups::NONE,
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
                    print_conv: false,
                },
                value_conv: None,
                print_conv: PrintConv::IntEnum(&[(0, "Wrong")]),
                subdir: None,
                hook: &[],
                groups: TagGroups::NONE,
            },
        ];
        static TABLE: BinaryTable = BinaryTable {
            module: "Test",
            table: "Masked",
            group0: "",
            group1: "",
            group2: "",
            first_entry: 0,
            default_format: Fmt::Int8u,
            offsets_sound_until: None,
            priority: None,
            gate_a: super::super::GateA { blocked_by: &[] },
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
                print_conv: false,
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
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

    /// Step 28's per-table gate, proved on real tables rather than argued.
    ///
    /// `ID3::v1` is on the measured allowlist, so it reads through the shared
    /// `ReadValue` (ExifTool.pm:6286) and shortens a run-off-the-end field to
    /// what fits (ExifTool.pm:6301-6303). `Pentax::AFInfoK3III` is not (gate
    /// A blocks it: `expr_unsupported=2, tag_fmt_unsupported=1`), so it keeps
    /// the pre-Step-28 all-or-nothing read exactly.
    ///
    /// That difference is the whole enablement mechanism. If a future change
    /// makes the two paths agree, either the gate stopped gating or the
    /// legacy path stopped being legacy -- both worth failing over, because
    /// gate B's A/B has no control group without it.
    #[test]
    fn only_an_enabled_table_gets_exiftools_count_shortening() {
        let id3 = find_table("ID3", "v1").expect("generated ID3::v1");
        assert!(id3.enabled(), "ID3::v1 is on the Step 28 allowlist");

        // ID3v1's Title is `string[30]` at index 3 of an int8u table, i.e.
        // bytes 3..33. Hand it a record that stops at byte 20: ExifTool
        // reports the 17 bytes that fit, and so must an enabled table.
        let mut short = vec![0u8; 20];
        short[3..8].copy_from_slice(b"HELLO");
        let decode = decode_binary_table(id3, &short, ByteOrder::Big);
        let title = decode
            .fields()
            .iter()
            .find(|f| f.field.name == "Title")
            .expect("an enabled table shortens the count instead of dropping the field");
        // ID3::v1's Title carries a ValueConv this schema does not reproduce,
        // so `emit` still refuses it -- Step 28 changed which BYTES are read,
        // not which semantics are honoured.
        assert!(title.field.omitted.value_conv);
        assert_eq!(title.emit(), None);

        // The same shape on a table gate A blocks: unchanged strict read.
        let afinfo = find_table("Pentax", "AFInfoK3III").expect("generated Pentax::AFInfoK3III");
        assert!(!afinfo.enabled(), "gate A blocks Pentax::AFInfoK3III");
        assert!(
            !afinfo.gate_a.passes(),
            "and it is blocked by gate A, not merely absent from the allowlist"
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
                    print_conv: false,
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
                    print_conv: false,
                },
                value_conv: None,
                print_conv: PrintConv::None,
                subdir: None,
                hook: &[],
                groups: TagGroups::NONE,
            },
            Field {
                index: 2,
                sub: None,
                name: "Clean",
                format: Some(Fmt::Int8u),
                count: 1,
                mask: None,
                omitted: Omitted::NONE,
                value_conv: None,
                print_conv: PrintConv::None,
                subdir: None,
                hook: &[],
                groups: TagGroups::NONE,
            },
        ];
        static TABLE: BinaryTable = BinaryTable {
            module: "Test",
            table: "Refusals",
            group0: "",
            group1: "",
            group2: "",
            first_entry: 0,
            default_format: Fmt::Int8u,
            offsets_sound_until: None,
            priority: None,
            gate_a: super::super::GateA { blocked_by: &[] },
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
                print_conv: 0,
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
                value_conv: None,
                print_conv: PrintConv::None,
                subdir: None,
                hook: &[],
                groups: TagGroups::NONE,
            },
            Field {
                index: 5,
                sub: None,
                name: "PastBound",
                format: Some(Fmt::Int8u),
                count: 1,
                mask: None,
                omitted: Omitted::NONE,
                value_conv: None,
                print_conv: PrintConv::None,
                subdir: None,
                hook: &[],
                groups: TagGroups::NONE,
            },
        ];
        static TABLE: BinaryTable = BinaryTable {
            module: "Test",
            table: "Unsound",
            group0: "",
            group1: "",
            group2: "",
            first_entry: 0,
            default_format: Fmt::Int8u,
            offsets_sound_until: Some(3),
            priority: None,
            gate_a: super::super::GateA { blocked_by: &[] },
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
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
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
            group1: "",
            group2: "",
            first_entry: 0,
            default_format: Fmt::Int8u,
            offsets_sound_until: None,
            priority: None,
            gate_a: super::super::GateA { blocked_by: &[] },
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

    // --- Step 25: DecodeBits (ExifTool.pm:6385-6407) -----------------------

    #[test]
    fn decode_bits_matches_exiftool_pm_6385() {
        // `return '(none)' unless @bitList;` (ExifTool.pm:6403).
        assert_eq!(decode_bits(0, &[(0, "A"), (1, "B")]), "(none)");
        // A named bit contributes its label.
        assert_eq!(decode_bits(0b1, &[(0, "A"), (1, "B")]), "A");
        // Multiple set bits join with `", "` (`join($lookup ? ', ' : ',', ...)`,
        // ExifTool.pm:6404, taken here since a lookup is always passed).
        assert_eq!(decode_bits(0b11, &[(0, "A"), (1, "B")]), "A, B");
        // A set bit with no entry in `lookup` renders `"[n]"` (`push
        // @bitList, "[$n]"`, ExifTool.pm:6401).
        assert_eq!(decode_bits(0b100, &[(0, "A"), (1, "B")]), "[2]");
        // Named and unnamed bits interleave in bit order.
        assert_eq!(decode_bits(0b101, &[(0, "A"), (1, "B")]), "A, [2]");
        // An empty lookup (`BITMASK => {}`) is still a truthy hashref in
        // Perl, so DecodeBits takes the `elsif ($$lookup{$n})`/`else` path,
        // never the `not $lookup` one: every set bit renders `[n]`.
        assert_eq!(decode_bits(0b11, &[]), "[0], [1]");
        // Bit 31 (top of the default 32-bit word) is in range.
        assert_eq!(decode_bits(1i64 << 31, &[(31, "Top")]), "Top");
        // A negative i64 exercises the two's-complement bit pattern via the
        // `val as u64` cast -- ExifTool's Perl `&` operates on the same
        // native-width bit pattern, so `-1` (all bits set) still finds bit 0
        // and bit 31.
        let all_set = decode_bits(-1, &[(0, "A"), (31, "Top")]);
        assert!(all_set.starts_with("A, "), "{all_set}");
        assert!(all_set.ends_with(", Top"), "{all_set}");
    }

    #[test]
    fn unknown_fallback_matches_exiftool_pm_3624() {
        // `$value = "Unknown ($val)";` (ExifTool.pm:3630) -- the plain form.
        assert_eq!(unknown_fallback(5, false), "Unknown (5)");
        assert_eq!(unknown_fallback(-1, false), "Unknown (-1)");
        // `$value = sprintf('Unknown (0x%x)',$val);` (ExifTool.pm:3627) --
        // taken when the tag declares `PrintHex`.
        assert_eq!(unknown_fallback(31, true), "Unknown (0x1f)");
        // Perl's `%x` on a negative value formats its native-width two's
        // complement bit pattern.
        assert_eq!(unknown_fallback(-1, true), "Unknown (0xffffffffffffffff)");
    }

    // --- 4b-i: IntEnum / StrEnum misses (ExifTool.pm:3624-3631) -----------

    /// A plain hash `PrintConv` renders ExifTool's `"Unknown ($val)"` for a
    /// key it does not carry, instead of falling back to the raw value. The
    /// expected strings are what `exiftool-pinned.sh -j` (13.59) prints for
    /// the same situations in the corpus: `ICC_Profile:PrimaryPlatform` is
    /// `Unknown ()` in 24 files and `Unknown (SEC)` in 3, `ProfileCMMType`
    /// is `Unknown (UCCM)` in 1 (scratchpad `icc-header-oracle.json`,
    /// 2026-09-06).
    #[test]
    fn int_enum_miss_renders_exiftool_unknown() {
        let conv = PrintConv::IntEnum(&[(0, "Perceptual"), (1, "Media-Relative Colorimetric")]);
        assert_eq!(
            render(conv, &DecodedValue::Integer(1)),
            Some("Media-Relative Colorimetric".to_string())
        );
        assert_eq!(
            render(conv, &DecodedValue::Integer(7)),
            Some("Unknown (7)".to_string())
        );
        // An integral float is the same key as its IV (post-ValueConv `2.0`).
        assert_eq!(
            render(conv, &DecodedValue::Float(1.0)),
            Some("Media-Relative Colorimetric".to_string())
        );
        // A non-integral float misses and interpolates as Perl prints an NV.
        assert_eq!(
            render(conv, &DecodedValue::Float(2.5)),
            Some("Unknown (2.5)".to_string())
        );
        // A fixed-count field's `$val` is the space-joined element list.
        assert_eq!(
            render(
                conv,
                &DecodedValue::Array(vec![DecodedValue::Integer(1), DecodedValue::Integer(2)])
            ),
            Some("Unknown (1 2)".to_string())
        );
        // A rational cannot be keyed the way Perl would print it: no
        // rendering, the raw value stands in (documented gap, not a guess).
        assert_eq!(render(conv, &DecodedValue::UnsignedRational(1, 3)), None);
    }

    #[test]
    fn str_enum_miss_renders_exiftool_unknown() {
        let conv = PrintConv::StrEnum(&[
            ("", ""),
            ("APPL", "Apple Computer Inc."),
            ("MSFT", "Microsoft Corporation"),
        ]);
        assert_eq!(
            render(conv, &DecodedValue::String("APPL".to_string())),
            Some("Apple Computer Inc.".to_string())
        );
        // manuSig-style `'' => ''`: a blank key that IS in the hash renders
        // the empty string, not Unknown.
        assert_eq!(
            render(conv, &DecodedValue::String(String::new())),
            Some(String::new())
        );
        assert_eq!(
            render(conv, &DecodedValue::String("SEC".to_string())),
            Some("Unknown (SEC)".to_string())
        );
        // The PrimaryPlatform hash has no blank key: blank misses to `Unknown ()`.
        let platform = PrintConv::StrEnum(&[("APPL", "Apple Computer Inc.")]);
        assert_eq!(
            render(platform, &DecodedValue::String(String::new())),
            Some("Unknown ()".to_string())
        );
        // An `undef[N]` field keys by its bytes when they are UTF-8 ...
        assert_eq!(
            render(platform, &DecodedValue::Undefined(b"UCCM".to_vec())),
            Some("Unknown (UCCM)".to_string())
        );
        // ... and cannot be keyed at all when they are not.
        assert_eq!(
            render(platform, &DecodedValue::Undefined(vec![0xff, 0xfe])),
            None
        );
    }

    // --- Step 25: PartialEnumInt (ExifTool.pm:3612-3631) --------------------

    #[test]
    fn partial_enum_int_checks_exact_match_before_other_before_unknown() {
        // Exact match wins even when `other` is registered -- ExifTool checks
        // the whole PrintConv hash (`$$conv{$val}`) before ever falling back
        // to BITMASK/OTHER (ExifTool.pm:3612).
        let conv = PrintConv::PartialEnumInt {
            exact: &[(1, "One")],
            other: Some(OtherId::Identity),
            print_hex: false,
        };
        assert_eq!(
            render(conv, &DecodedValue::Integer(1)),
            Some("One".to_string())
        );
        // Falls to the registered OTHER closure for anything `exact` misses.
        assert_eq!(
            render(conv, &DecodedValue::Integer(5)),
            Some("5".to_string())
        );
    }

    #[test]
    fn partial_enum_int_falls_back_to_unknown_when_other_is_absent_or_undefined() {
        // No OTHER registered at all: an unmapped value renders ExifTool's
        // own `"Unknown ($val)"` form (ExifTool.pm:3624-3631) rather than the
        // bare raw value -- this is Step 25's fix for the 362/16 "partial"
        // enums that used to drop the fallback entirely.
        let conv = PrintConv::PartialEnumInt {
            exact: &[(1, "One")],
            other: None,
            print_hex: false,
        };
        assert_eq!(
            render(conv, &DecodedValue::Integer(5)),
            Some("Unknown (5)".to_string())
        );
        // `PrintHex => 1`: `sprintf('Unknown (0x%x)', $val)` (ExifTool.pm:3627).
        let conv_hex = PrintConv::PartialEnumInt {
            exact: &[(1, "One")],
            other: None,
            print_hex: true,
        };
        assert_eq!(
            render(conv_hex, &DecodedValue::Integer(31)),
            Some("Unknown (0x1f)".to_string())
        );
        // An exact match still wins over the hex fallback.
        assert_eq!(
            render(conv_hex, &DecodedValue::Integer(1)),
            Some("One".to_string())
        );
    }

    // --- Step 25: Bitmask ----------------------------------------------------

    #[test]
    fn bitmask_checks_exact_match_before_decode_bits() {
        let conv = PrintConv::Bitmask {
            exact: &[(0, "(none)")],
            bits: &[
                (0, "Animation"),
                (1, "Limited Range"),
                (3, "Extension Present"),
            ],
        };
        // Exact match (BPG::Main-style: value 0 has its own entry alongside
        // BITMASK) wins over DecodeBits.
        assert_eq!(
            render(conv, &DecodedValue::Integer(0)),
            Some("(none)".to_string())
        );
        // No exact match: DecodeBits decodes the set bits.
        assert_eq!(
            render(conv, &DecodedValue::Integer(0b11)),
            Some("Animation, Limited Range".to_string())
        );
        // `render`'s `Bitmask` arm never returns `None` -- unlike `IntEnum`,
        // a value outside `exact` always gets a rendering (DecodeBits itself,
        // which returns `"(none)"`/`"[n]"` rather than failing).
        assert_eq!(
            render(conv, &DecodedValue::Integer(1 << 5)),
            Some("[5]".to_string())
        );
    }

    // --- Step 25: OtherId registry -------------------------------------------

    #[test]
    fn other_id_identity_passes_the_value_through() {
        assert_eq!(OtherId::Identity.apply(127), Some("127".to_string()));
        assert_eq!(OtherId::Identity.apply(-3), Some("-3".to_string()));
    }

    #[test]
    fn other_id_exif_print_parameter_matches_exif_pm_5628() {
        // Exif.pm:5628-5639's `PrintParameter`: `return $val if $inv` is the
        // PrintConvInv path, not reachable from here.
        // `$val <= 0` passes through unchanged.
        assert_eq!(OtherId::ExifPrintParameter.apply(0), Some("0".to_string()));
        assert_eq!(
            OtherId::ExifPrintParameter.apply(-4),
            Some("-4".to_string())
        );
        // `0 < $val <= 0xfff0` gets a leading `+`.
        assert_eq!(OtherId::ExifPrintParameter.apply(7), Some("+7".to_string()));
        assert_eq!(
            OtherId::ExifPrintParameter.apply(0xfff0),
            Some("+65520".to_string())
        );
        // `$val > 0xfff0` is really a negative value in disguise:
        // `$val - 0x10000`.
        assert_eq!(
            OtherId::ExifPrintParameter.apply(0xfff1),
            Some("-15".to_string())
        );
    }

    #[test]
    fn other_id_minolta_af_status_focus_matches_minolta_pm_648() {
        // Minolta.pm:648-658's `%afStatusInfo` OTHER sub, forward path only.
        assert_eq!(
            OtherId::MinoltaAfStatusFocus.apply(-5),
            Some("Front Focus (-5)".to_string())
        );
        assert_eq!(
            OtherId::MinoltaAfStatusFocus.apply(5),
            Some("Back Focus (+5)".to_string())
        );
        assert_eq!(
            OtherId::MinoltaAfStatusFocus.apply(0),
            Some("Back Focus (+0)".to_string())
        );
    }

    /// Step 26: effective groups -- `GetTagTable`'s table-level defaulting
    /// (ExifTool.pm:8980-8991) plus `AddTagToTable`'s per-tag override
    /// (ExifTool.pm:9236-9244).
    #[test]
    fn effective_groups_default_then_override() {
        // AIFF::Common declares `GROUPS => { 2 => 'Audio' }` and nothing else,
        // so families 0 and 1 default to the module name and family 2 keeps
        // the declared value. Confirmed against the pinned oracle, which
        // reports AIFF tags under group1 `AIFF`:
        //   $ exiftool-pinned.sh -G1 t/images/AIFF.aif
        //   [AIFF]  Num Channels : 1
        let aiff = find_table("AIFF", "Common").expect("AIFF::Common");
        assert_eq!(aiff.group0, "AIFF");
        assert_eq!(aiff.group1, "AIFF");
        assert_eq!(aiff.group2, "Audio");

        let channels = aiff
            .fields
            .iter()
            .find(|f| f.name == "NumChannels")
            .expect("NumChannels");
        // No per-tag override: every family comes from the table.
        assert_eq!(channels.groups, TagGroups::NONE);
        assert_eq!(aiff.effective_groups(channels), ("AIFF", "AIFF", "Audio"));

        // A per-tag family-2 override. QuickTime::MovieHeader is GROUPS
        // { 2 => 'Video' }, and CreateDate carries `Groups => { 2 => 'Time' }`
        // -- so family 2 is the tag's, families 0/1 still the table's.
        let movie = find_table("QuickTime", "MovieHeader").expect("QuickTime::MovieHeader");
        let create = movie
            .fields
            .iter()
            .find(|f| f.name == "CreateDate")
            .expect("CreateDate");
        assert_eq!(create.groups.g2, Some("Time"));
        assert_eq!(create.groups.g0, None, "family 0 is not overridden");
        let (g0, g1, g2) = movie.effective_groups(create);
        assert_eq!((g0, g1), (movie.group0, movie.group1));
        assert_eq!(g2, "Time");

        // The defaulting is not cosmetic: no emitted table may have an empty
        // group, which is what the raw GROUPS hash produced for 512 of them
        // before this rule was applied.
        for table in ALL_BINARY_TABLES {
            assert!(
                !table.group0.is_empty() && !table.group1.is_empty() && !table.group2.is_empty(),
                "{}::{} has an empty effective group",
                table.module,
                table.table
            );
        }
    }

    /// Step 26: the two `Hook` idioms the census contains, pinned as data.
    ///
    /// These are MODELED, not applied -- `Omitted::hook` is still set on both
    /// fields, so `emit()` still refuses them. The assertion is that the
    /// compiled rule matches the Perl, not that the walk uses it.
    #[test]
    fn hook_effects_compile_to_the_two_census_idioms() {
        use super::super::{CmpOp, HookCond, HookDelta, HookEffect};

        // Idiom 1, a format switch. QuickTime.pm's MovieHeader CreateDate:
        // `$$self{MovieHeaderVersion} and $format = "int64u", $varSize += 4`
        // -- Perl's comma is a low-precedence sequence inside the `and`, so
        // both halves are gated on the same DataMember.
        let movie = find_table("QuickTime", "MovieHeader").expect("QuickTime::MovieHeader");
        let create = movie
            .fields
            .iter()
            .find(|f| f.name == "CreateDate")
            .expect("CreateDate");
        assert_eq!(
            create.hook,
            &[HookEffect::SwitchFormat {
                when: HookCond::MemberTruthy("MovieHeaderVersion"),
                format: Fmt::Int64u,
                delta: 4,
            }]
        );
        // Modeled is not applied: `Omitted::hook` is still set, which is what
        // makes `DecodedField::emit` refuse the field.
        assert!(create.omitted.hook);

        // Idiom 2, a varSize shift chain. Canon.pm CameraInfo5DmkIII
        // FocusDistanceLower: `$varSize -= 4 if $$self{CanonFirm} < 3;
        // $varSize += 5 if $$self{CanonFirm} > 4;` -- two gated shifts, applied
        // in order, which must compile as a pair or not at all.
        let canon = find_table("Canon", "CameraInfo5DmkIII").expect("Canon::CameraInfo5DmkIII");
        let focus = canon
            .fields
            .iter()
            .find(|f| f.name == "FocusDistanceLower")
            .expect("FocusDistanceLower");
        assert_eq!(
            focus.hook,
            &[
                HookEffect::ShiftVarSize {
                    delta: HookDelta::Const(4),
                    negate: true,
                    when: Some(HookCond::MemberInt {
                        member: "CanonFirm",
                        op: CmpOp::Lt,
                        value: 3,
                    }),
                },
                HookEffect::ShiftVarSize {
                    delta: HookDelta::Const(5),
                    negate: false,
                    when: Some(HookCond::MemberInt {
                        member: "CanonFirm",
                        op: CmpOp::Gt,
                        value: 4,
                    }),
                },
            ]
        );

        // A Hook outside the closed grammar compiles to nothing rather than
        // to something approximate. Samsung::DualShotExtra's reads the data
        // block and runs a regex over it; `Omitted::hook` still marks it.
        let samsung = find_table("Samsung", "DualShotExtra").expect("Samsung::DualShotExtra");
        let dummy = samsung
            .fields
            .iter()
            .find(|f| f.name == "DualShotDummy")
            .expect("DualShotDummy");
        assert!(dummy.hook.is_empty(), "refused, not approximated");
        assert!(dummy.omitted.hook);
    }

    /// Step 26: `AIFF::Common` SampleRate, the 80-bit IEEE `extended` AGENTS.md
    /// names as the canonical "transcribed table is short because the
    /// generator will not guess" case.
    ///
    /// The bytes are the real COMM chunk body of the pinned ExifTool test
    /// sample `t/images/AIFF.aif` (byte-identical to the copy in the pinned
    /// corpus), read straight out of the file:
    ///
    /// ```text
    /// $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -G1 -a t/images/AIFF.aif
    /// [AIFF]  Sample Rate   : 22050
    /// ```
    ///
    /// 0x400d is the biased exponent (16397 - 16383 - 63 = -49) and
    /// 0xac44000000000000 the explicit-leading-bit significand (44100 << 48),
    /// so the value is 44100 * 2^48 * 2^-49 = 22050 exactly -- which is the
    /// whole reason AIFF stores a sample rate in this format.
    ///
    /// This drives the generated table, not `parsers::audio::aiff`'s own
    /// hand decode: the point is that `Fmt::Extended` now reads the same
    /// number the parser does, so the two paths cannot drift apart silently.
    #[test]
    fn aiff_common_sample_rate_decodes_extended() {
        let comm = [
            0x00, 0x01, // NumChannels    = 1
            0x00, 0x00, 0x2d, 0x22, // NumSampleFrames = 11554
            0x00, 0x08, // SampleSize     = 8
            0x40, 0x0d, 0xac, 0x44, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, // SampleRate (extended)
        ];

        let table = find_table("AIFF", "Common").expect("generated AIFF::Common table");
        let sample_rate = table
            .fields
            .iter()
            .find(|f| f.name == "SampleRate")
            .expect("SampleRate is transcribed now that `extended` is supported");
        assert_eq!(sample_rate.format, Some(Fmt::Extended));

        // AIFF is big-endian by definition (AIFF.pm sets 'MM').
        let decode = decode_binary_table(table, &comm, ByteOrder::Big);
        let get = |name: &str| {
            decode
                .fields()
                .iter()
                .find(|decoded| decoded.field.name == name)
                .and_then(DecodedField::emit)
        };

        assert_eq!(get("NumChannels"), Some(TagValue::Integer(1)));
        assert_eq!(get("NumSampleFrames"), Some(TagValue::Integer(11554)));
        assert_eq!(get("SampleSize"), Some(TagValue::Integer(8)));
        // ExifTool prints 22050, not 22050.0 -- Perl renders an integral
        // float without a decimal point.
        assert_eq!(
            get("SampleRate"),
            Some(TagValue::Float(22050.0)),
            "SampleRate must decode to exactly 22050, matching the pinned oracle"
        );
    }
}
