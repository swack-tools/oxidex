//! Runtime reader for generated ExifTool `ProcessBinaryData` tables.
//!
//! The generated registry describes byte layout and the conversions that the
//! transcription pipeline can reproduce exactly. This module is deliberately
//! smaller than ExifTool's full `ProcessBinaryData`: bit fields and unsupported
//! conversions are left for format-specific code instead of being guessed.

use crate::core::TagValue;
use crate::io::ByteOrder;

use super::{BinaryTable, Field, Fmt, PrintConv};

/// A value read directly from a generated binary-table field.
#[derive(Clone, Debug, PartialEq)]
pub enum DecodedValue {
    Integer(i64),
    Float(f64),
    UnsignedRational(u32, u32),
    SignedRational(i32, i32),
    String(String),
    Undefined(Vec<u8>),
}

impl DecodedValue {
    /// Convert a raw generated value to OxiDex's common value type without
    /// applying a display conversion.
    ///
    /// Unsigned rationals whose components do not fit `TagValue::Rational`'s
    /// signed 32-bit representation return `None` rather than silently
    /// truncating. Callers that need those values can retain `DecodedValue`
    /// and choose a format-specific representation.
    #[must_use]
    pub fn to_tag_value(&self) -> Option<TagValue> {
        Some(match self {
            Self::Integer(value) => TagValue::Integer(*value),
            Self::Float(value) => TagValue::Float(*value),
            Self::UnsignedRational(numerator, denominator) => TagValue::Rational {
                numerator: i32::try_from(*numerator).ok()?,
                denominator: i32::try_from(*denominator).ok()?,
            },
            Self::SignedRational(numerator, denominator) => TagValue::Rational {
                numerator: *numerator,
                denominator: *denominator,
            },
            Self::String(value) => TagValue::String(value.clone()),
            Self::Undefined(value) => TagValue::Binary(value.clone()),
        })
    }

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
    /// Convert this field's raw value to the shared OxiDex value type.
    #[must_use]
    pub fn to_tag_value(&self) -> Option<TagValue> {
        self.raw.to_tag_value()
    }

    /// Apply this field's generated `PrintConv` directly to its raw value.
    ///
    /// This is deliberately explicit. The generated schema does not yet say
    /// whether an unsupported `ValueConv` was omitted, so the decoder cannot
    /// soundly compose `PrintConv` with raw decoding on a caller's behalf.
    /// Call this only after verifying that the field has no intervening
    /// `ValueConv`, or after applying that conversion separately.
    #[must_use]
    pub fn apply_print_conv_to_raw(&self) -> Option<String> {
        apply_print_conv(self.field.print_conv, &self.raw)
    }
}

/// Decode the fields whose layouts are completely described by `table`.
///
/// Out-of-range fields and fractional bit-field indices are refused. The
/// latter need ExifTool's bit-mask semantics, which the generated schema does
/// not yet carry. This function performs raw decoding only. Callers must opt
/// into `PrintConv` with [`DecodedField::apply_print_conv_to_raw`] after
/// checking whether an intervening `ValueConv` is required.
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
            // ExifTool's fractional indices describe bit fields. Their masks
            // are not present in the generated runtime schema yet.
            if field.sub.is_some() {
                return None;
            }
            let offset = usize::try_from(table.byte_offset(field)).ok()?;
            let format = table.field_format(field);
            let width = usize::try_from(format.size()).ok()?;
            let bytes = data.get(offset..offset.checked_add(width)?)?;
            let raw = decode_value(bytes, format, byte_order)?;
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
    use crate::exiftool_tables::{ExprId, find_table};

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
        assert_eq!(
            get("ISO").and_then(DecodedField::to_tag_value),
            Some(TagValue::Integer(200))
        );
        assert_eq!(
            get("FNumber").and_then(DecodedField::to_tag_value),
            Some(TagValue::Rational {
                numerator: 28,
                denominator: 10,
            })
        );
    }

    #[test]
    fn reversed_endian_and_unsupported_bit_fields_are_explicit() {
        static FIELDS: &[Field] = &[
            Field {
                index: 0,
                sub: None,
                name: "Reversed",
                format: Some(Fmt::Int16uRev),
                print_conv: PrintConv::None,
            },
            Field {
                index: 2,
                sub: Some(1),
                name: "BitField",
                format: None,
                print_conv: PrintConv::Expr(ExprId::Sprintf0fValB74070),
            },
        ];
        static TABLE: BinaryTable = BinaryTable {
            module: "Test",
            table: "Endian",
            group0: "",
            group2: "",
            first_entry: 0,
            default_format: Fmt::Int8u,
            fields: FIELDS,
        };

        let fields = decode_binary_table(&TABLE, &[0x12, 0x34, 0xff], ByteOrder::Big);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].raw, DecodedValue::Integer(0x3412));
    }
}
