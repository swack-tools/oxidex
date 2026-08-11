//! Runtime reader for generated ExifTool `ProcessBinaryData` tables.
//!
//! The generated registry describes byte layout and the conversions that the
//! transcription pipeline can reproduce exactly. This module is deliberately
//! smaller than ExifTool's full `ProcessBinaryData`: a bit field whose slice
//! the schema does not record, and conversions the generator refused to
//! approximate, are left for format-specific code instead of being guessed.

use crate::io::ByteOrder;

use super::{ALL_BINARY_TABLES, BinaryTable, Field, Fmt, PrintConv};

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
#[derive(Clone, Debug)]
pub struct DecodedField {
    pub field: &'static Field,
    pub raw: DecodedValue,
}

impl DecodedField {
    /// Apply this field's generated `PrintConv` directly to its raw value.
    ///
    /// Refuses when the transcription recorded an omitted `ValueConv`. ExifTool
    /// runs `ValueConv` before `PrintConv`, so on such a field the generated
    /// enum is keyed on values the raw bytes never produce: `Minolta`'s
    /// Sharpness/Contrast/Saturation (`$val - 10`) would be looked up ten steps
    /// off, and `Nikon`'s `ISOAutoShutterTime` (`$val / 8`) eight times high.
    /// That yields either a wrong string or a silent fall-through to a wrong
    /// raw number, under a real ExifTool tag name.
    ///
    /// This precondition was documented before, as something the caller had to
    /// check -- but the schema carried no way to check it, so no caller could.
    /// Now that `Field::omitted` records it, the refusal happens here.
    #[must_use]
    pub fn apply_print_conv_to_raw(&self) -> Option<String> {
        if self.field.omitted.value_conv {
            return None;
        }
        apply_print_conv(self.field.print_conv, &self.raw)
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

/// Decode the fields whose layouts are completely described by `table`.
///
/// Out-of-range fields are refused, and so are fractional bit-field indices
/// that declare no [`Mask`] -- see [`fractional_census`] for that split.
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
/// [`DecodedValue::Array`]. This function performs raw decoding only; opt into
/// `PrintConv` with [`DecodedField::apply_print_conv_to_raw`].
#[must_use]
pub fn decode_binary_table(
    table: &'static BinaryTable,
    data: &[u8],
    byte_order: ByteOrder,
) -> Vec<DecodedField> {
    table
        .fields
        .iter()
        .filter_map(|field| {
            if field.sub.is_some() && field.mask.is_none() {
                return None;
            }
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
        })
        .collect()
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
        let fields = decode_binary_table(table, &data, ByteOrder::Little);
        let get = |name: &str| fields.iter().find(|decoded| decoded.field.name == name);

        assert_eq!(
            get("Make").map(|decoded| &decoded.raw),
            Some(&DecodedValue::String("PENTAX DIGITAL CAMERA".into()))
        );
        assert_eq!(
            get("FNumber").map(|decoded| &decoded.raw),
            Some(&DecodedValue::UnsignedRational(28, 10))
        );
        assert_eq!(
            get("FNumber").and_then(DecodedField::apply_print_conv_to_raw),
            Some("2.8".to_string())
        );
        assert_eq!(
            get("WhiteBalance").and_then(DecodedField::apply_print_conv_to_raw),
            Some("Shade".to_string())
        );
        assert_eq!(
            get("FocalLength").and_then(DecodedField::apply_print_conv_to_raw),
            Some("7.1 mm".to_string())
        );
        assert_eq!(
            get("ISO").map(|decoded| &decoded.raw),
            Some(&DecodedValue::Integer(200))
        );
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

        let fields = decode_binary_table(
            &TABLE,
            &[0x12, 0x34, 0xff, 0, 0, 1, 0, 2, 0, 3],
            ByteOrder::Big,
        );
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].raw, DecodedValue::Integer(0x3412));
        assert_eq!(
            fields[1].raw,
            DecodedValue::Array(vec![
                DecodedValue::Integer(1),
                DecodedValue::Integer(2),
                DecodedValue::Integer(3),
            ]),
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
        let fields = decode_binary_table(&TABLE, &data, ByteOrder::Big);
        let get = |name: &str| fields.iter().find(|decoded| decoded.field.name == name);

        assert_eq!(
            get("Low").map(|d| &d.raw),
            Some(&DecodedValue::Integer(0xD))
        );
        assert_eq!(
            get("Middle").map(|d| &d.raw),
            Some(&DecodedValue::Integer(0xB))
        );
        assert_eq!(
            get("Middle").and_then(DecodedField::apply_print_conv_to_raw),
            Some("Eleven".to_string()),
            "the enum is keyed on the slice, not on the 0xABCD word"
        );
        assert_eq!(
            get("Top").map(|d| &d.raw),
            Some(&DecodedValue::Integer(0xA))
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
        let fields = decode_binary_table(&TABLE, &[0xFD, 0xA3, 0x00], ByteOrder::Big);
        assert_eq!(fields[0].raw, DecodedValue::Integer(5));
        assert_eq!(
            fields[0].apply_print_conv_to_raw().as_deref(),
            Some("Rotated")
        );
        assert_eq!(fields[1].raw, DecodedValue::Integer(0xA));
        assert_eq!(fields[2].raw, DecodedValue::Integer(0));
        assert_eq!(
            fields[2].apply_print_conv_to_raw(),
            None,
            "a PrintConv behind an omitted ValueConv must not be applied"
        );
    }
}
