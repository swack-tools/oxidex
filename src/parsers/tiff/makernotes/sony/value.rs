//! Typed access to the raw bytes behind a Sony MakerNote IFD entry.

use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::borrow::Cow;

/// TIFF field types the Sony MakerNote actually uses.
const TYPE_BYTE: u16 = 1;
const TYPE_ASCII: u16 = 2;
const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
const TYPE_RATIONAL: u16 = 5;
const TYPE_SBYTE: u16 = 6;
const TYPE_UNDEFINED: u16 = 7;
const TYPE_SSHORT: u16 = 8;
const TYPE_SLONG: u16 = 9;
const TYPE_SRATIONAL: u16 = 10;

/// The bytes of one MakerNote IFD entry plus the type information needed to
/// read them.
///
/// The bytes are the entry's *resolved* value: either the four inline bytes or
/// the slice the entry's offset pointed at.
pub struct SonyValue<'a> {
    /// TIFF field type.
    pub field_type: u16,
    /// Number of components.
    pub count: u32,
    /// Borrowed when the value lived outside the entry, owned when it was the
    /// entry's own inline four bytes.
    bytes: Cow<'a, [u8]>,
    byte_order: ByteOrder,
}

impl<'a> SonyValue<'a> {
    /// Wraps an entry's resolved bytes.
    pub fn new(
        field_type: u16,
        count: u32,
        bytes: impl Into<Cow<'a, [u8]>>,
        byte_order: ByteOrder,
    ) -> Self {
        SonyValue {
            field_type,
            count,
            bytes: bytes.into(),
            byte_order,
        }
    }

    /// The raw bytes, exactly as stored.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Length of the raw bytes.
    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }

    fn reader(&self) -> EndianReader<'_> {
        EndianReader::new(&self.bytes, self.byte_order.to_io_byte_order())
    }

    /// Size in bytes of one component of this entry's type, or `None` for types
    /// that are not fixed-size integers.
    fn component_size(&self) -> Option<usize> {
        Some(match self.field_type {
            TYPE_BYTE | TYPE_SBYTE | TYPE_UNDEFINED | TYPE_ASCII => 1,
            TYPE_SHORT | TYPE_SSHORT => 2,
            TYPE_LONG | TYPE_SLONG => 4,
            _ => return None,
        })
    }

    /// Reads component `i` as a signed 64-bit integer, sign-extending signed
    /// TIFF types and zero-extending unsigned ones.
    pub fn int_at(&self, i: usize) -> Option<i64> {
        let size = self.component_size()?;
        let reader = self.reader();
        let offset = i.checked_mul(size)?;
        Some(match self.field_type {
            TYPE_BYTE | TYPE_UNDEFINED | TYPE_ASCII => reader.u8_at(offset)? as i64,
            TYPE_SBYTE => reader.u8_at(offset)? as i8 as i64,
            TYPE_SHORT => reader.u16_at(offset)? as i64,
            TYPE_SSHORT => reader.u16_at(offset)? as i16 as i64,
            TYPE_LONG => reader.u32_at(offset)? as i64,
            TYPE_SLONG => reader.u32_at(offset)? as i32 as i64,
            _ => return None,
        })
    }

    /// Reads an int16u straight out of the stored bytes, ignoring the entry's
    /// declared type -- ExifTool's `Get16u($valPt, n)`, which some `Main`
    /// Conditions use on a value whose type says otherwise.
    pub fn u16_at_raw(&self, offset: usize) -> Option<u16> {
        self.reader().u16_at(offset)
    }

    /// Reads the first component as an integer.
    pub fn first_int(&self) -> Option<i64> {
        self.int_at(0)
    }

    /// Reads the first component reinterpreted as `T`.
    ///
    /// Sony writes several tags as `int32u` while ExifTool declares a signed
    /// `Format`; this applies that reinterpretation without pretending the
    /// stored type was different.
    pub fn first_int_as<T: FromI64>(&self) -> Option<T> {
        self.first_int().map(T::from_i64)
    }

    /// Every component, as integers.
    pub fn ints(&self) -> Vec<i64> {
        (0..self.count as usize)
            .map_while(|i| self.int_at(i))
            .collect()
    }

    /// All components joined with spaces, ExifTool's rendering of a numeric
    /// array in its plain (non-JSON) output.
    pub fn join_ints(&self) -> String {
        self.ints()
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The two int16u halves of a 4-byte entry, in stored order.
    pub fn as_u16_pair(&self) -> Option<(i64, i64)> {
        let reader = self.reader();
        Some((reader.u16_at(0)? as i64, reader.u16_at(2)? as i64))
    }

    /// Reads component `i` of a RATIONAL/SRATIONAL entry.
    pub fn rational(&self, i: usize) -> Option<f64> {
        let reader = self.reader();
        let offset = i.checked_mul(8)?;
        match self.field_type {
            TYPE_RATIONAL => {
                let (num, den) = (reader.u32_at(offset)?, reader.u32_at(offset + 4)?);
                (den != 0).then(|| num as f64 / den as f64)
            }
            TYPE_SRATIONAL => {
                let (num, den) = (
                    reader.u32_at(offset)? as i32,
                    reader.u32_at(offset + 4)? as i32,
                );
                (den != 0).then(|| num as f64 / den as f64)
            }
            _ => None,
        }
    }

    /// An ASCII entry's text, with trailing NULs and whitespace removed.
    pub fn string(&self) -> Option<String> {
        let text = String::from_utf8_lossy(&self.bytes);
        let trimmed = text.trim_end_matches(['\0', ' ']).to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    }

    /// ExifTool's `FullImageSize`/`PreviewImageSize` rendering: the stored
    /// height/width pair reversed to width-first and joined with an "x".
    pub fn reversed_size(&self) -> Option<String> {
        let values = self.ints();
        (values.len() == 2).then(|| format!("{}x{}", values[1], values[0]))
    }
}

/// Reinterprets a raw integer as a narrower signed type.
pub trait FromI64 {
    /// Performs the reinterpretation.
    fn from_i64(value: i64) -> Self;
}

impl FromI64 for i16 {
    fn from_i64(value: i64) -> Self {
        value as u16 as i16
    }
}

impl FromI64 for i32 {
    fn from_i64(value: i64) -> Self {
        value as u32 as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_entries_declared_signed_are_reinterpreted_not_truncated() {
        // 0xb022 ColorCompensationFilter is stored int32u, read int32s.
        let bytes = [0xfd, 0xff, 0xff, 0xff];
        let value = SonyValue::new(TYPE_LONG, 1, &bytes, ByteOrder::LittleEndian);
        assert_eq!(value.first_int(), Some(4_294_967_293));
        assert_eq!(value.first_int_as::<i32>(), Some(-3));
    }

    #[test]
    fn reversed_size_puts_width_first() {
        let bytes = [0xa0, 0x0f, 0x00, 0x00, 0x70, 0x17, 0x00, 0x00];
        let value = SonyValue::new(TYPE_LONG, 2, &bytes, ByteOrder::LittleEndian);
        assert_eq!(value.reversed_size(), Some("6000x4000".to_string()));
    }

    #[test]
    fn rationals_with_zero_denominator_are_dropped() {
        let bytes = [0x00; 8];
        let value = SonyValue::new(TYPE_RATIONAL, 1, &bytes, ByteOrder::LittleEndian);
        assert_eq!(value.rational(0), None);
    }

    #[test]
    fn ascii_entries_lose_their_padding() {
        let mut bytes = *b"Standard\0\0\0\0\0\0\0\0";
        bytes[8] = 0;
        let value = SonyValue::new(TYPE_ASCII, 16, &bytes, ByteOrder::LittleEndian);
        assert_eq!(value.string(), Some("Standard".to_string()));
    }
}
