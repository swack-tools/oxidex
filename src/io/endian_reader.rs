//! Generic binary reading with configurable byte order.
//!
//! Provides [`EndianReader`] for random-access reading of binary data with
//! either big-endian or little-endian byte order.

/// Byte order for binary reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    /// Big-endian (most significant byte first)
    Big,
    /// Little-endian (least significant byte first)
    Little,
}

/// Random-access binary reader with configurable byte order.
///
/// All read methods return `Option<T>` to safely handle out-of-bounds reads.
///
/// # Example
///
/// ```
/// use oxidex::io::{EndianReader, ByteOrder};
///
/// let data = [0x12, 0x34, 0x56, 0x78];
/// let be = EndianReader::big_endian(&data);
/// let le = EndianReader::little_endian(&data);
///
/// assert_eq!(be.u32_at(0), Some(0x12345678));
/// assert_eq!(le.u32_at(0), Some(0x78563412));
/// ```
#[derive(Debug, Clone, Copy)]
pub struct EndianReader<'a> {
    data: &'a [u8],
    order: ByteOrder,
}

impl<'a> EndianReader<'a> {
    /// Creates a new reader with the specified byte order.
    #[inline]
    pub fn new(data: &'a [u8], order: ByteOrder) -> Self {
        Self { data, order }
    }

    /// Creates a big-endian reader.
    #[inline]
    pub fn big_endian(data: &'a [u8]) -> Self {
        Self::new(data, ByteOrder::Big)
    }

    /// Creates a little-endian reader.
    #[inline]
    pub fn little_endian(data: &'a [u8]) -> Self {
        Self::new(data, ByteOrder::Little)
    }

    /// Returns the byte order of this reader.
    #[inline]
    pub fn byte_order(&self) -> ByteOrder {
        self.order
    }

    /// Returns the length of the underlying data.
    #[inline]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns true if the underlying data is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Returns a slice of the underlying data.
    #[inline]
    pub fn slice(&self, start: usize, end: usize) -> Option<&'a [u8]> {
        self.data.get(start..end)
    }

    /// Returns the underlying data.
    #[inline]
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Reads a u8 at the given offset.
    #[inline]
    pub fn u8_at(&self, offset: usize) -> Option<u8> {
        self.data.get(offset).copied()
    }

    /// Reads a u16 at the given offset.
    #[inline]
    pub fn u16_at(&self, offset: usize) -> Option<u16> {
        let bytes = self.data.get(offset..offset + 2)?;
        Some(match self.order {
            ByteOrder::Big => u16::from_be_bytes([bytes[0], bytes[1]]),
            ByteOrder::Little => u16::from_le_bytes([bytes[0], bytes[1]]),
        })
    }

    /// Reads a u32 at the given offset.
    #[inline]
    pub fn u32_at(&self, offset: usize) -> Option<u32> {
        let bytes = self.data.get(offset..offset + 4)?;
        Some(match self.order {
            ByteOrder::Big => u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            ByteOrder::Little => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        })
    }

    /// Reads a u64 at the given offset.
    #[inline]
    pub fn u64_at(&self, offset: usize) -> Option<u64> {
        let bytes = self.data.get(offset..offset + 8)?;
        Some(match self.order {
            ByteOrder::Big => u64::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            ByteOrder::Little => u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
        })
    }

    /// Reads an i8 at the given offset.
    #[inline]
    pub fn i8_at(&self, offset: usize) -> Option<i8> {
        self.data.get(offset).map(|&b| b as i8)
    }

    /// Reads an i16 at the given offset.
    #[inline]
    pub fn i16_at(&self, offset: usize) -> Option<i16> {
        let bytes = self.data.get(offset..offset + 2)?;
        Some(match self.order {
            ByteOrder::Big => i16::from_be_bytes([bytes[0], bytes[1]]),
            ByteOrder::Little => i16::from_le_bytes([bytes[0], bytes[1]]),
        })
    }

    /// Reads an i32 at the given offset.
    #[inline]
    pub fn i32_at(&self, offset: usize) -> Option<i32> {
        let bytes = self.data.get(offset..offset + 4)?;
        Some(match self.order {
            ByteOrder::Big => i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            ByteOrder::Little => i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        })
    }

    /// Reads an i64 at the given offset.
    #[inline]
    pub fn i64_at(&self, offset: usize) -> Option<i64> {
        let bytes = self.data.get(offset..offset + 8)?;
        Some(match self.order {
            ByteOrder::Big => i64::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            ByteOrder::Little => i64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
        })
    }

    /// Reads an f32 at the given offset.
    #[inline]
    pub fn f32_at(&self, offset: usize) -> Option<f32> {
        let bytes = self.data.get(offset..offset + 4)?;
        Some(match self.order {
            ByteOrder::Big => f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            ByteOrder::Little => f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        })
    }

    /// Reads an f64 at the given offset.
    #[inline]
    pub fn f64_at(&self, offset: usize) -> Option<f64> {
        let bytes = self.data.get(offset..offset + 8)?;
        Some(match self.order {
            ByteOrder::Big => f64::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            ByteOrder::Little => f64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
        })
    }

    /// Reads an unsigned rational (two u32 values) at the given offset.
    /// Returns (numerator, denominator).
    #[inline]
    pub fn rational_at(&self, offset: usize) -> Option<(u32, u32)> {
        let num = self.u32_at(offset)?;
        let denom = self.u32_at(offset + 4)?;
        Some((num, denom))
    }

    /// Reads a signed rational (two i32 values) at the given offset.
    /// Returns (numerator, denominator).
    #[inline]
    pub fn srational_at(&self, offset: usize) -> Option<(i32, i32)> {
        let num = self.i32_at(offset)?;
        let denom = self.i32_at(offset + 4)?;
        Some((num, denom))
    }

    /// Reads a UTF-8 string of the given length at the given offset.
    /// Returns None if the bytes are not valid UTF-8.
    #[inline]
    pub fn str_at(&self, offset: usize, len: usize) -> Option<&'a str> {
        let bytes = self.data.get(offset..offset + len)?;
        std::str::from_utf8(bytes).ok()
    }

    /// Reads a null-terminated string starting at the given offset.
    /// Reads up to max_len bytes.
    pub fn cstr_at(&self, offset: usize, max_len: usize) -> Option<&'a str> {
        let start = offset;
        let end = (offset + max_len).min(self.data.len());
        let bytes = self.data.get(start..end)?;

        let null_pos = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        std::str::from_utf8(&bytes[..null_pos]).ok()
    }

    /// Reads raw bytes at the given offset.
    #[inline]
    pub fn bytes_at(&self, offset: usize, len: usize) -> Option<&'a [u8]> {
        self.data.get(offset..offset + len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_big_endian_integers() {
        let data = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
        let reader = EndianReader::big_endian(&data);

        assert_eq!(reader.u8_at(0), Some(0x12));
        assert_eq!(reader.u16_at(0), Some(0x1234));
        assert_eq!(reader.u32_at(0), Some(0x12345678));
        assert_eq!(reader.u64_at(0), Some(0x123456789ABCDEF0));
    }

    #[test]
    fn test_little_endian_integers() {
        let data = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
        let reader = EndianReader::little_endian(&data);

        assert_eq!(reader.u8_at(0), Some(0x12));
        assert_eq!(reader.u16_at(0), Some(0x3412));
        assert_eq!(reader.u32_at(0), Some(0x78563412));
        assert_eq!(reader.u64_at(0), Some(0xF0DEBC9A78563412));
    }

    #[test]
    fn test_signed_integers() {
        // -1 in big-endian
        let data = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let reader = EndianReader::big_endian(&data);

        assert_eq!(reader.i8_at(0), Some(-1));
        assert_eq!(reader.i16_at(0), Some(-1));
        assert_eq!(reader.i32_at(0), Some(-1));
        assert_eq!(reader.i64_at(0), Some(-1));
    }

    #[test]
    fn test_floats() {
        // f32: 1.0 in big-endian = 0x3F800000
        let data = [
            0x3F, 0x80, 0x00, 0x00, 0x3F, 0xF0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let reader = EndianReader::big_endian(&data);

        assert_eq!(reader.f32_at(0), Some(1.0f32));
        assert_eq!(reader.f64_at(4), Some(1.0f64));
    }

    #[test]
    fn test_rationals() {
        // 3/4 in big-endian
        let data = [0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x04];
        let reader = EndianReader::big_endian(&data);

        assert_eq!(reader.rational_at(0), Some((3, 4)));

        // -3/4 in big-endian
        let data = [0xFF, 0xFF, 0xFF, 0xFD, 0x00, 0x00, 0x00, 0x04];
        let reader = EndianReader::big_endian(&data);

        assert_eq!(reader.srational_at(0), Some((-3, 4)));
    }

    #[test]
    fn test_strings() {
        let data = b"Hello\0World";
        let reader = EndianReader::big_endian(data);

        assert_eq!(reader.str_at(0, 5), Some("Hello"));
        assert_eq!(reader.cstr_at(0, 20), Some("Hello"));
        assert_eq!(reader.cstr_at(6, 20), Some("World"));
    }

    #[test]
    fn test_out_of_bounds() {
        let data = [0x12, 0x34];
        let reader = EndianReader::big_endian(&data);

        assert_eq!(reader.u8_at(0), Some(0x12));
        assert_eq!(reader.u8_at(1), Some(0x34));
        assert_eq!(reader.u8_at(2), None);
        assert_eq!(reader.u16_at(0), Some(0x1234));
        assert_eq!(reader.u16_at(1), None);
        assert_eq!(reader.u32_at(0), None);
    }

    #[test]
    fn test_slice_and_bytes() {
        let data = [0x01, 0x02, 0x03, 0x04, 0x05];
        let reader = EndianReader::big_endian(&data);

        assert_eq!(reader.slice(1, 4), Some(&[0x02, 0x03, 0x04][..]));
        assert_eq!(reader.bytes_at(2, 2), Some(&[0x03, 0x04][..]));
        assert_eq!(reader.slice(3, 10), None);
    }

    #[test]
    fn test_len_and_empty() {
        let data = [0x01, 0x02];
        let reader = EndianReader::big_endian(&data);
        assert_eq!(reader.len(), 2);
        assert!(!reader.is_empty());

        let empty: &[u8] = &[];
        let reader = EndianReader::big_endian(empty);
        assert_eq!(reader.len(), 0);
        assert!(reader.is_empty());
    }

    #[test]
    fn test_byte_order_accessor() {
        let data = [0x01, 0x02];
        let be = EndianReader::big_endian(&data);
        let le = EndianReader::little_endian(&data);

        assert_eq!(be.byte_order(), ByteOrder::Big);
        assert_eq!(le.byte_order(), ByteOrder::Little);
    }

    #[test]
    fn test_data_accessor() {
        let data = [0x01, 0x02, 0x03];
        let reader = EndianReader::big_endian(&data);

        assert_eq!(reader.data(), &data);
    }

    #[test]
    fn test_reads_at_various_offsets() {
        let data = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07];
        let reader = EndianReader::big_endian(&data);

        // Test reading at different offsets
        assert_eq!(reader.u16_at(2), Some(0x0203));
        assert_eq!(reader.u32_at(4), Some(0x04050607));
        assert_eq!(reader.u16_at(6), Some(0x0607));
    }

    #[test]
    fn test_little_endian_rationals() {
        // 3/4 in little-endian
        let data = [0x03, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00];
        let reader = EndianReader::little_endian(&data);

        assert_eq!(reader.rational_at(0), Some((3, 4)));
    }

    #[test]
    fn test_invalid_utf8_string() {
        let data = [0xFF, 0xFE, 0x00]; // Invalid UTF-8
        let reader = EndianReader::big_endian(&data);

        assert_eq!(reader.str_at(0, 2), None);
    }

    #[test]
    fn test_cstr_at_end_of_buffer() {
        let data = b"NoNull";
        let reader = EndianReader::big_endian(data);

        // Should return full string when no null found
        assert_eq!(reader.cstr_at(0, 10), Some("NoNull"));
    }
}
