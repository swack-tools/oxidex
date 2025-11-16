use crate::parsers::tiff::ifd_parser::ByteOrder;

/// Read u16 from byte slice at offset
///
/// Returns None if offset is out of bounds
pub fn read_u16(data: &[u8], offset: usize, byte_order: ByteOrder) -> Option<u16> {
    if offset + 2 > data.len() {
        return None;
    }

    let value = match byte_order {
        ByteOrder::LittleEndian => u16::from_le_bytes([data[offset], data[offset + 1]]),
        ByteOrder::BigEndian => u16::from_be_bytes([data[offset], data[offset + 1]]),
    };

    Some(value)
}

/// Read i16 from byte slice at offset
pub fn read_i16(data: &[u8], offset: usize, byte_order: ByteOrder) -> Option<i16> {
    if offset + 2 > data.len() {
        return None;
    }

    let value = match byte_order {
        ByteOrder::LittleEndian => i16::from_le_bytes([data[offset], data[offset + 1]]),
        ByteOrder::BigEndian => i16::from_be_bytes([data[offset], data[offset + 1]]),
    };

    Some(value)
}

/// Read u32 from byte slice at offset
pub fn read_u32(data: &[u8], offset: usize, byte_order: ByteOrder) -> Option<u32> {
    if offset + 4 > data.len() {
        return None;
    }

    let value = match byte_order {
        ByteOrder::LittleEndian => u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]),
        ByteOrder::BigEndian => u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]),
    };

    Some(value)
}

/// Read ASCII string from byte slice
///
/// Reads up to `length` bytes or until null terminator
pub fn read_ascii_string(data: &[u8], offset: usize, length: usize) -> Option<String> {
    if offset + length > data.len() {
        return None;
    }

    let bytes = &data[offset..offset + length];
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(length);

    String::from_utf8(bytes[..end].to_vec()).ok()
}

/// Parse null-terminated string from beginning of slice
pub fn parse_null_terminated_string(data: &[u8]) -> String {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    String::from_utf8_lossy(&data[..end]).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_u16_big_endian() {
        let data = vec![0x01, 0x02, 0x03, 0x04];
        assert_eq!(read_u16(&data, 0, ByteOrder::BigEndian), Some(0x0102));
        assert_eq!(read_u16(&data, 2, ByteOrder::BigEndian), Some(0x0304));
        assert_eq!(read_u16(&data, 3, ByteOrder::BigEndian), None); // Out of bounds
    }

    #[test]
    fn test_read_ascii_string() {
        let data = b"Hello\0World";
        assert_eq!(read_ascii_string(data, 0, 11), Some("Hello".to_string()));
    }

    #[test]
    fn test_parse_null_terminated_string() {
        let data = b"Test\0Ignored";
        assert_eq!(parse_null_terminated_string(data), "Test");
    }
}
