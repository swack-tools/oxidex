//! Sequential binary reading with position tracking.
//!
//! Provides [`Cursor`] for sequential reading of binary data with
//! configurable byte order. Useful for parsing variable-length formats.

use super::ByteOrder;

/// Sequential binary reader with position tracking.
///
/// Unlike [`EndianReader`](super::EndianReader) which uses absolute offsets,
/// `Cursor` maintains a position and advances it with each read.
///
/// # Example
///
/// ```
/// use oxidex::io::{Cursor, ByteOrder};
///
/// let data = [0x12, 0x34, 0x56, 0x78];
/// let mut cursor = Cursor::new(&data, ByteOrder::Big);
///
/// assert_eq!(cursor.read_u16(), Some(0x1234));
/// assert_eq!(cursor.position(), 2);
/// assert_eq!(cursor.read_u16(), Some(0x5678));
/// assert_eq!(cursor.position(), 4);
/// ```
#[derive(Debug)]
pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
    order: ByteOrder,
}

impl<'a> Cursor<'a> {
    /// Creates a new cursor with the specified byte order.
    #[inline]
    pub fn new(data: &'a [u8], order: ByteOrder) -> Self {
        Self {
            data,
            pos: 0,
            order,
        }
    }

    /// Creates a big-endian cursor.
    #[inline]
    pub fn big_endian(data: &'a [u8]) -> Self {
        Self::new(data, ByteOrder::Big)
    }

    /// Creates a little-endian cursor.
    #[inline]
    pub fn little_endian(data: &'a [u8]) -> Self {
        Self::new(data, ByteOrder::Little)
    }

    /// Returns the current position.
    #[inline]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Sets the position.
    #[inline]
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }

    /// Skips the given number of bytes.
    #[inline]
    pub fn skip(&mut self, count: usize) {
        self.pos = self.pos.saturating_add(count);
    }

    /// Returns the number of remaining bytes.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Returns the total length of the data.
    #[inline]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns true if there are no more bytes to read.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Returns the underlying data slice.
    #[inline]
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Reads a u8 and advances the position.
    #[inline]
    pub fn read_u8(&mut self) -> Option<u8> {
        let value = self.data.get(self.pos).copied()?;
        self.pos += 1;
        Some(value)
    }

    /// Reads a u16 and advances the position.
    #[inline]
    pub fn read_u16(&mut self) -> Option<u16> {
        let bytes = self.data.get(self.pos..self.pos + 2)?;
        let value = match self.order {
            ByteOrder::Big => u16::from_be_bytes([bytes[0], bytes[1]]),
            ByteOrder::Little => u16::from_le_bytes([bytes[0], bytes[1]]),
        };
        self.pos += 2;
        Some(value)
    }

    /// Reads a u32 and advances the position.
    #[inline]
    pub fn read_u32(&mut self) -> Option<u32> {
        let bytes = self.data.get(self.pos..self.pos + 4)?;
        let value = match self.order {
            ByteOrder::Big => u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            ByteOrder::Little => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        };
        self.pos += 4;
        Some(value)
    }

    /// Reads a u64 and advances the position.
    #[inline]
    pub fn read_u64(&mut self) -> Option<u64> {
        let bytes = self.data.get(self.pos..self.pos + 8)?;
        let value = match self.order {
            ByteOrder::Big => u64::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            ByteOrder::Little => u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
        };
        self.pos += 8;
        Some(value)
    }

    /// Reads an i8 and advances the position.
    #[inline]
    pub fn read_i8(&mut self) -> Option<i8> {
        self.read_u8().map(|v| v as i8)
    }

    /// Reads an i16 and advances the position.
    #[inline]
    pub fn read_i16(&mut self) -> Option<i16> {
        let bytes = self.data.get(self.pos..self.pos + 2)?;
        let value = match self.order {
            ByteOrder::Big => i16::from_be_bytes([bytes[0], bytes[1]]),
            ByteOrder::Little => i16::from_le_bytes([bytes[0], bytes[1]]),
        };
        self.pos += 2;
        Some(value)
    }

    /// Reads an i32 and advances the position.
    #[inline]
    pub fn read_i32(&mut self) -> Option<i32> {
        let bytes = self.data.get(self.pos..self.pos + 4)?;
        let value = match self.order {
            ByteOrder::Big => i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            ByteOrder::Little => i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        };
        self.pos += 4;
        Some(value)
    }

    /// Reads an i64 and advances the position.
    #[inline]
    pub fn read_i64(&mut self) -> Option<i64> {
        let bytes = self.data.get(self.pos..self.pos + 8)?;
        let value = match self.order {
            ByteOrder::Big => i64::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            ByteOrder::Little => i64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
        };
        self.pos += 8;
        Some(value)
    }

    /// Reads raw bytes and advances the position.
    #[inline]
    pub fn read_bytes(&mut self, len: usize) -> Option<&'a [u8]> {
        let bytes = self.data.get(self.pos..self.pos + len)?;
        self.pos += len;
        Some(bytes)
    }

    /// Reads a null-terminated string up to max_len bytes.
    pub fn read_cstring(&mut self, max_len: usize) -> Option<&'a str> {
        let end = (self.pos + max_len).min(self.data.len());
        let bytes = self.data.get(self.pos..end)?;

        let null_pos = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        let result = std::str::from_utf8(&bytes[..null_pos]).ok()?;
        self.pos += null_pos + 1; // Skip past null terminator
        Some(result)
    }

    /// Reads an EBML variable-length integer (used in MKV/WebM).
    /// Returns the value and the number of bytes consumed.
    pub fn read_vint(&mut self) -> Option<(u64, usize)> {
        let first = self.data.get(self.pos).copied()?;

        // Count leading zeros to determine length
        let len = if first & 0x80 != 0 {
            1
        } else if first & 0x40 != 0 {
            2
        } else if first & 0x20 != 0 {
            3
        } else if first & 0x10 != 0 {
            4
        } else if first & 0x08 != 0 {
            5
        } else if first & 0x04 != 0 {
            6
        } else if first & 0x02 != 0 {
            7
        } else if first & 0x01 != 0 {
            8
        } else {
            return None; // Invalid VINT
        };

        if self.pos + len > self.data.len() {
            return None;
        }

        // Read value with length marker stripped
        let mut value = (first & (0xFF >> len)) as u64;
        for i in 1..len {
            value = (value << 8) | self.data[self.pos + i] as u64;
        }

        self.pos += len;
        Some((value, len))
    }

    /// Reads an EBML element ID (preserves the length marker bits).
    pub fn read_vint_id(&mut self) -> Option<(u64, usize)> {
        let first = self.data.get(self.pos).copied()?;

        let len = if first & 0x80 != 0 {
            1
        } else if first & 0x40 != 0 {
            2
        } else if first & 0x20 != 0 {
            3
        } else if first & 0x10 != 0 {
            4
        } else {
            return None; // IDs are max 4 bytes
        };

        if self.pos + len > self.data.len() {
            return None;
        }

        // Read value preserving length marker
        let mut value = first as u64;
        for i in 1..len {
            value = (value << 8) | self.data[self.pos + i] as u64;
        }

        self.pos += len;
        Some((value, len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sequential_reads() {
        let data = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
        let mut cursor = Cursor::big_endian(&data);

        assert_eq!(cursor.read_u8(), Some(0x12));
        assert_eq!(cursor.position(), 1);

        assert_eq!(cursor.read_u16(), Some(0x3456));
        assert_eq!(cursor.position(), 3);

        cursor.seek(0);
        assert_eq!(cursor.read_u32(), Some(0x12345678));
        assert_eq!(cursor.read_u32(), Some(0x9ABCDEF0));
    }

    #[test]
    fn test_little_endian() {
        let data = [0x12, 0x34, 0x56, 0x78];
        let mut cursor = Cursor::little_endian(&data);

        assert_eq!(cursor.read_u16(), Some(0x3412));
        assert_eq!(cursor.read_u16(), Some(0x7856));
    }

    #[test]
    fn test_skip_and_remaining() {
        let data = [0x01, 0x02, 0x03, 0x04, 0x05];
        let mut cursor = Cursor::big_endian(&data);

        assert_eq!(cursor.remaining(), 5);
        cursor.skip(2);
        assert_eq!(cursor.remaining(), 3);
        assert_eq!(cursor.read_u8(), Some(0x03));
    }

    #[test]
    fn test_read_bytes() {
        let data = [0x01, 0x02, 0x03, 0x04, 0x05];
        let mut cursor = Cursor::big_endian(&data);

        assert_eq!(cursor.read_bytes(3), Some(&[0x01, 0x02, 0x03][..]));
        assert_eq!(cursor.position(), 3);
    }

    #[test]
    fn test_read_cstring() {
        let data = b"Hello\0World\0";
        let mut cursor = Cursor::big_endian(data);

        assert_eq!(cursor.read_cstring(20), Some("Hello"));
        assert_eq!(cursor.read_cstring(20), Some("World"));
    }

    #[test]
    fn test_vint_single_byte() {
        // 0x81 = 1000_0001 -> value = 1
        let data = [0x81];
        let mut cursor = Cursor::big_endian(&data);
        assert_eq!(cursor.read_vint(), Some((1, 1)));
    }

    #[test]
    fn test_vint_two_bytes() {
        // 0x40_01 = 0100_0000 0000_0001 -> value = 1
        let data = [0x40, 0x01];
        let mut cursor = Cursor::big_endian(&data);
        assert_eq!(cursor.read_vint(), Some((1, 2)));
    }

    #[test]
    fn test_vint_id() {
        // EBML ID 0x1A45DFA3 (EBML header)
        let data = [0x1A, 0x45, 0xDF, 0xA3];
        let mut cursor = Cursor::big_endian(&data);
        assert_eq!(cursor.read_vint_id(), Some((0x1A45DFA3, 4)));
    }

    #[test]
    fn test_out_of_bounds() {
        let data = [0x01, 0x02];
        let mut cursor = Cursor::big_endian(&data);

        assert_eq!(cursor.read_u16(), Some(0x0102));
        assert_eq!(cursor.read_u8(), None);
        assert_eq!(cursor.read_u16(), None);
    }

    #[test]
    fn test_signed_integers() {
        // -1 in big-endian
        let data = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let mut cursor = Cursor::big_endian(&data);

        assert_eq!(cursor.read_i8(), Some(-1));
        cursor.seek(0);
        assert_eq!(cursor.read_i16(), Some(-1));
        cursor.seek(0);
        assert_eq!(cursor.read_i32(), Some(-1));
        cursor.seek(0);
        assert_eq!(cursor.read_i64(), Some(-1));
    }

    #[test]
    fn test_u64_read() {
        let data = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let mut cursor = Cursor::big_endian(&data);

        assert_eq!(cursor.read_u64(), Some(0x0102030405060708));
        assert_eq!(cursor.position(), 8);
    }

    #[test]
    fn test_len_and_is_empty() {
        let data = [0x01, 0x02, 0x03];
        let cursor = Cursor::big_endian(&data);

        assert_eq!(cursor.len(), 3);
        assert!(!cursor.is_empty());

        let empty: &[u8] = &[];
        let cursor = Cursor::big_endian(empty);
        assert!(cursor.is_empty());
    }

    #[test]
    fn test_data_accessor() {
        let data = [0x01, 0x02, 0x03];
        let cursor = Cursor::big_endian(&data);

        assert_eq!(cursor.data(), &data);
    }

    #[test]
    fn test_vint_three_bytes() {
        // 0x20_00_01 = 0010_0000 0000_0000 0000_0001 -> value = 1
        let data = [0x20, 0x00, 0x01];
        let mut cursor = Cursor::big_endian(&data);
        assert_eq!(cursor.read_vint(), Some((1, 3)));
    }

    #[test]
    fn test_vint_four_bytes() {
        // 0x10_00_00_01 -> value = 1
        let data = [0x10, 0x00, 0x00, 0x01];
        let mut cursor = Cursor::big_endian(&data);
        assert_eq!(cursor.read_vint(), Some((1, 4)));
    }

    #[test]
    fn test_vint_invalid_zero() {
        // 0x00 is invalid VINT
        let data = [0x00];
        let mut cursor = Cursor::big_endian(&data);
        assert_eq!(cursor.read_vint(), None);
    }

    #[test]
    fn test_vint_truncated() {
        // Two-byte vint but only 1 byte available
        let data = [0x40]; // Needs 2 bytes
        let mut cursor = Cursor::big_endian(&data);
        assert_eq!(cursor.read_vint(), None);
    }

    #[test]
    fn test_skip_saturating() {
        let data = [0x01, 0x02];
        let mut cursor = Cursor::big_endian(&data);

        cursor.skip(100); // Skip way past end
        assert_eq!(cursor.remaining(), 0);
    }
}
