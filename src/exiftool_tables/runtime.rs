//! Runtime reader for generated ExifTool `ProcessBinaryData` tables.
//!
//! The generated registry describes byte layout and the conversions that the
//! transcription pipeline can reproduce exactly. This module is deliberately
//! smaller than ExifTool's full `ProcessBinaryData`: a bit field whose slice
//! the schema does not record, and conversions the generator refused to
//! approximate, are left for format-specific code instead of being guessed.

use crate::core::TagValue;
use crate::io::ByteOrder;

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
/// The refusal is an under-claim, not a bug, and the point of counting it is
/// that an under-claim nobody measures is indistinguishable from a complete
/// one. Regenerating the tables against a later ExifTool can move either
/// number; a test pins both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FractionalCensus {
    /// Fractional entries declaring a [`Mask`], so their bit slice is known.
    pub decoded: usize,
    /// Fractional entries with no `Mask`: which bits they name is unrecorded.
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

/// Split `table`'s fractional entries by whether their bit semantics are known.
#[must_use]
pub fn fractional_census(table: &BinaryTable) -> FractionalCensus {
    let mut census = FractionalCensus::default();
    for field in table.fields {
        if field.sub.is_none() {
            continue;
        }
        if field.mask.is_some() {
            census.decoded += 1;
        } else {
            census.refused += 1;
        }
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
/// Out-of-range fields are silently absent (never enough bytes to decode
/// at all), and so are fractional bit-field indices that declare no
/// [`Mask`] -- see [`fractional_census`] for that split; neither is part of
/// [`RefusalCounts`], which is specifically about resolved-but-withheld
/// fields (D1).
///
/// A fractional key is ExifTool's `12.1` notation: several tags share the word
/// at index 12, each naming a slice of it. `Mask` is what says *which* slice,
/// so a fractional field that declares one is fully described and decodes
/// here; one that does not leaves its bit semantics undetermined, and
/// reporting the whole word under its name would be a confident wrong value
/// rather than a missing tag.
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
        if field.sub.is_some() && field.mask.is_none() {
            continue;
        }
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

    /// A fractional key without a `Mask` says a tag is a slice of the word
    /// without saying which bits, so it stays refused. `BitField` below is
    /// that case: the whole word 0xFF under an ExifTool tag name would be a
    /// wrong value, and a wrong value is worse than an absent one.
    #[test]
    fn reversed_endian_and_maskless_bit_fields_are_explicit() {
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
        };

        let decode = decode_binary_table(
            &TABLE,
            &[0x12, 0x34, 0xff, 0, 0, 1, 0, 2, 0, 3],
            ByteOrder::Big,
        );
        let fields = decode.fields();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].emit(), Some(TagValue::Integer(0x3412)));
        assert_eq!(
            fields[1].emit(),
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
    /// three of these read the same `int16u` and differ only in their mask.
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
            },
            // Same word, no mask: which bits it names is unrecorded.
            Field {
                index: 4,
                sub: Some(4),
                name: "Undetermined",
                format: Some(Fmt::Int16u),
                count: 1,
                mask: None,
                omitted: Omitted::NONE,
                print_conv: PrintConv::None,
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
        assert!(
            get("Undetermined").is_none(),
            "a fractional entry with no Mask has no defined slice to report"
        );
    }

    /// The generated tables are overwhelmingly the decodable case, which is
    /// why lifting the refusal was worth measuring. Pinning both halves makes
    /// a regeneration that changes the balance visible instead of silent.
    #[test]
    fn generated_fractional_entries_are_almost_all_masked() {
        let census = all_fractional_census();
        assert_eq!(
            census,
            FractionalCensus {
                decoded: 993,
                refused: 11,
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
        };

        let decode = decode_binary_table(&TABLE, &[0, 0, 0, 0, 0, 9], ByteOrder::Big);
        assert_eq!(decode.fields().len(), 1, "only the sound field decodes");
        assert_eq!(decode.fields()[0].field.name, "BeforeBound");
        assert_eq!(decode.refusals().offset_unsound, 1);
    }
}
