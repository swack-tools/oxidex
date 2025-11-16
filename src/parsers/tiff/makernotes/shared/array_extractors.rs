use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};

/// Extract i16 array from IFD entry
///
/// Used by: Canon CameraSettings, Nikon ShotInfo, Sony CameraSettings
pub fn extract_i16_array(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
) -> Option<Vec<i16>> {
    if entry.value_count == 0 {
        return None;
    }

    let offset = entry.value_offset as usize;
    if offset + (entry.value_count as usize * 2) > data.len() {
        return None;
    }

    let mut array = Vec::with_capacity(entry.value_count as usize);
    for i in 0..entry.value_count {
        let pos = offset + (i as usize * 2);
        let value = match byte_order {
            ByteOrder::LittleEndian => {
                i16::from_le_bytes([data[pos], data[pos + 1]])
            }
            ByteOrder::BigEndian => {
                i16::from_be_bytes([data[pos], data[pos + 1]])
            }
        };
        array.push(value);
    }

    Some(array)
}

/// Extract u16 array from IFD entry
///
/// Used by: Nikon LensData, Sony AFInfo, Fuji FaceDetection
pub fn extract_u16_array(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
) -> Option<Vec<u16>> {
    if entry.value_count == 0 {
        return None;
    }

    let offset = entry.value_offset as usize;
    if offset + (entry.value_count as usize * 2) > data.len() {
        return None;
    }

    let mut array = Vec::with_capacity(entry.value_count as usize);
    for i in 0..entry.value_count {
        let pos = offset + (i as usize * 2);
        let value = match byte_order {
            ByteOrder::LittleEndian => {
                u16::from_le_bytes([data[pos], data[pos + 1]])
            }
            ByteOrder::BigEndian => {
                u16::from_be_bytes([data[pos], data[pos + 1]])
            }
        };
        array.push(value);
    }

    Some(array)
}

/// Extract u32 array from IFD entry
///
/// Used by: Canon FileInfo, Nikon ShutterData, Pentax CameraInfo
pub fn extract_u32_array(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
) -> Option<Vec<u32>> {
    if entry.value_count == 0 {
        return None;
    }

    let offset = entry.value_offset as usize;
    if offset + (entry.value_count as usize * 4) > data.len() {
        return None;
    }

    let mut array = Vec::with_capacity(entry.value_count as usize);
    for i in 0..entry.value_count {
        let pos = offset + (i as usize * 4);
        let value = match byte_order {
            ByteOrder::LittleEndian => {
                u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
            }
            ByteOrder::BigEndian => {
                u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
            }
        };
        array.push(value);
    }

    Some(array)
}

/// Extract i32 array from IFD entry
///
/// Used by: Olympus CameraSettings, Panasonic WBInfo
pub fn extract_i32_array(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
) -> Option<Vec<i32>> {
    if entry.value_count == 0 {
        return None;
    }

    let offset = entry.value_offset as usize;
    if offset + (entry.value_count as usize * 4) > data.len() {
        return None;
    }

    let mut array = Vec::with_capacity(entry.value_count as usize);
    for i in 0..entry.value_count {
        let pos = offset + (i as usize * 4);
        let value = match byte_order {
            ByteOrder::LittleEndian => {
                i32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
            }
            ByteOrder::BigEndian => {
                i32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
            }
        };
        array.push(value);
    }

    Some(array)
}

/// Extract rational (u32/u32) array from IFD entry
///
/// Used by: GPS coordinates, exposure times, focal lengths
pub fn extract_rational_array(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
) -> Option<Vec<(u32, u32)>> {
    if entry.value_count == 0 {
        return None;
    }

    let offset = entry.value_offset as usize;
    if offset + (entry.value_count as usize * 8) > data.len() {
        return None;
    }

    let mut array = Vec::with_capacity(entry.value_count as usize);
    for i in 0..entry.value_count {
        let pos = offset + (i as usize * 8);
        let (numerator, denominator) = match byte_order {
            ByteOrder::LittleEndian => {
                let num = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
                let den = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
                (num, den)
            }
            ByteOrder::BigEndian => {
                let num = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
                let den = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
                (num, den)
            }
        };
        array.push((numerator, denominator));
    }

    Some(array)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_i16_array_big_endian() {
        let data = vec![0x00, 0x0A, 0x00, 0x14, 0xFF, 0xF6]; // [10, 20, -10]
        let entry = IfdEntry {
            tag_id: 0x0001,
            field_type: 3,
            value_count: 3,
            value_offset: 0,
        };

        let result = extract_i16_array(&entry, &data, ByteOrder::BigEndian);
        assert_eq!(result, Some(vec![10, 20, -10]));
    }

    #[test]
    fn test_extract_u32_array_little_endian() {
        let data = vec![0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00]; // [1, 2]
        let entry = IfdEntry {
            tag_id: 0x0001,
            field_type: 4,
            value_count: 2,
            value_offset: 0,
        };

        let result = extract_u32_array(&entry, &data, ByteOrder::LittleEndian);
        assert_eq!(result, Some(vec![1, 2]));
    }
}
