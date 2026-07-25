//! Low-level byte order writing utilities
//!
//! This module provides reusable helper functions for writing primitive types
//! in either little-endian or big-endian byte order. These utilities are used
//! throughout the TIFF writing process to ensure correct byte ordering.

use crate::parsers::tiff::ifd_parser::ByteOrder;

/// Writes a u16 value to the output buffer in the specified byte order.
///
/// # Parameters
///
/// - `output`: Mutable reference to the output buffer
/// - `value`: The u16 value to write
/// - `byte_order`: Endianness for serialization (LittleEndian or BigEndian)
///
/// # Example
///
/// ```ignore
/// use oxidex::writers::tiff_writer::tiff::byte_writer::write_u16;
/// use oxidex::parsers::tiff::ifd_parser::ByteOrder;
///
/// let mut buffer = Vec::new();
/// write_u16(&mut buffer, 0x1234, ByteOrder::LittleEndian);
/// assert_eq!(buffer, vec![0x34, 0x12]);
/// ```ignore
pub fn write_u16(output: &mut Vec<u8>, value: u16, byte_order: ByteOrder) {
    let bytes = match byte_order {
        ByteOrder::LittleEndian => value.to_le_bytes(),
        ByteOrder::BigEndian => value.to_be_bytes(),
    };
    output.extend_from_slice(&bytes);
}

/// Writes a u32 value to the output buffer in the specified byte order.
///
/// # Parameters
///
/// - `output`: Mutable reference to the output buffer
/// - `value`: The u32 value to write
/// - `byte_order`: Endianness for serialization (LittleEndian or BigEndian)
///
/// # Example
///
/// ```ignore
/// use oxidex::writers::tiff_writer::tiff::byte_writer::write_u32;
/// use oxidex::parsers::tiff::ifd_parser::ByteOrder;
///
/// let mut buffer = Vec::new();
/// write_u32(&mut buffer, 0x12345678, ByteOrder::LittleEndian);
/// assert_eq!(buffer, vec![0x78, 0x56, 0x34, 0x12]);
/// ```ignore
pub fn write_u32(output: &mut Vec<u8>, value: u32, byte_order: ByteOrder) {
    let bytes = match byte_order {
        ByteOrder::LittleEndian => value.to_le_bytes(),
        ByteOrder::BigEndian => value.to_be_bytes(),
    };
    output.extend_from_slice(&bytes);
}

/// Writes a TIFF header (8 bytes) to the output buffer.
///
/// The header structure consists of:
/// - Bytes 0-1: Byte order marker (0x4949 for LE, 0x4D4D for BE)
/// - Bytes 2-3: Magic number 42
/// - Bytes 4-7: Offset to first IFD (always 8 in our implementation)
///
/// # Parameters
///
/// - `output`: Mutable reference to the output buffer
/// - `byte_order`: Endianness for the TIFF file
///
/// # Example
///
/// ```ignore
/// use oxidex::writers::tiff_writer::tiff::byte_writer::write_tiff_header;
/// use oxidex::parsers::tiff::ifd_parser::ByteOrder;
///
/// let mut buffer = Vec::new();
/// write_tiff_header(&mut buffer, ByteOrder::LittleEndian);
/// assert_eq!(buffer.len(), 8);
/// assert_eq!(&buffer[0..2], b"II"); // Little-endian marker
/// ```ignore
pub fn write_tiff_header(output: &mut Vec<u8>, byte_order: ByteOrder) {
    match byte_order {
        ByteOrder::LittleEndian => {
            // "II" - Intel byte order (little-endian)
            output.extend_from_slice(&[0x49, 0x49]);
            // Magic number 42 (little-endian)
            output.extend_from_slice(&[0x2A, 0x00]);
            // First IFD offset: 8 (little-endian)
            output.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]);
        }
        ByteOrder::BigEndian => {
            // "MM" - Motorola byte order (big-endian)
            output.extend_from_slice(&[0x4D, 0x4D]);
            // Magic number 42 (big-endian)
            output.extend_from_slice(&[0x00, 0x2A]);
            // First IFD offset: 8 (big-endian)
            output.extend_from_slice(&[0x00, 0x00, 0x00, 0x08]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_u16_little_endian() {
        let mut buffer = Vec::new();
        write_u16(&mut buffer, 0x1234, ByteOrder::LittleEndian);
        assert_eq!(buffer, vec![0x34, 0x12]);
    }

    #[test]
    fn test_write_u16_big_endian() {
        let mut buffer = Vec::new();
        write_u16(&mut buffer, 0x1234, ByteOrder::BigEndian);
        assert_eq!(buffer, vec![0x12, 0x34]);
    }

    #[test]
    fn test_write_u32_little_endian() {
        let mut buffer = Vec::new();
        write_u32(&mut buffer, 0x12345678, ByteOrder::LittleEndian);
        assert_eq!(buffer, vec![0x78, 0x56, 0x34, 0x12]);
    }

    #[test]
    fn test_write_u32_big_endian() {
        let mut buffer = Vec::new();
        write_u32(&mut buffer, 0x12345678, ByteOrder::BigEndian);
        assert_eq!(buffer, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn test_write_tiff_header_little_endian() {
        let mut buffer = Vec::new();
        write_tiff_header(&mut buffer, ByteOrder::LittleEndian);

        assert_eq!(buffer.len(), 8);
        assert_eq!(&buffer[0..2], b"II"); // Little-endian marker
        assert_eq!(buffer[2], 0x2A); // Magic number
        assert_eq!(buffer[3], 0x00);
        assert_eq!(&buffer[4..8], &[0x08, 0x00, 0x00, 0x00]); // IFD offset
    }

    #[test]
    fn test_write_tiff_header_big_endian() {
        let mut buffer = Vec::new();
        write_tiff_header(&mut buffer, ByteOrder::BigEndian);

        assert_eq!(buffer.len(), 8);
        assert_eq!(&buffer[0..2], b"MM"); // Big-endian marker
        assert_eq!(buffer[2], 0x00); // Magic number
        assert_eq!(buffer[3], 0x2A);
        assert_eq!(&buffer[4..8], &[0x00, 0x00, 0x00, 0x08]); // IFD offset
    }
}
