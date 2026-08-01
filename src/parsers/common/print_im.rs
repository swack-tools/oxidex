//! Epson Print Image Matching (`PrintIM`) directory decoding.
//!
//! This is a direct translation of ExifTool's
//! `Image::ExifTool::PrintIM::ProcessPrintIM` (`PrintIM.pm` 1.07).  PrintIM
//! blocks are reached through several declared subdirectory edges (standard
//! EXIF, camera MakerNotes, and Toshiba's APP6 EPPIM wrapper), but the block
//! validation is identical at every edge and belongs in one place.

use crate::parsers::tiff::ifd_parser::ByteOrder;

/// ExifTool's family-0/family-1 name for the only known default-visible tag.
pub const PRINT_IM_VERSION_TAG: &str = "PrintIM:PrintIMVersion";

/// Decode the default-visible version from a PrintIM directory.
///
/// ExifTool requires a block larger than 15 bytes, checks the seven-byte
/// `PrintIM` signature, then validates the entry count at offset 14.  Some
/// writers store that count in the opposite order to the containing IFD, so
/// ExifTool retries with toggled byte order only when the first count would
/// overrun the block.  Unknown six-byte entries remain hidden by default;
/// therefore the four version bytes at offset 8 are the only returned value.
#[must_use]
pub fn decode_print_im_version(data: &[u8], byte_order: ByteOrder) -> Option<String> {
    if data.len() <= 15 || data.get(..7)? != b"PrintIM" {
        return None;
    }

    let count_bytes: [u8; 2] = data.get(14..16)?.try_into().ok()?;
    let count = read_count(count_bytes, byte_order);
    if !count_fits(data.len(), count) {
        let toggled = read_count(count_bytes, opposite(byte_order));
        if !count_fits(data.len(), toggled) {
            return None;
        }
    }

    Some(String::from_utf8_lossy(data.get(8..12)?).into_owned())
}

fn read_count(bytes: [u8; 2], byte_order: ByteOrder) -> u16 {
    match byte_order {
        ByteOrder::LittleEndian => u16::from_le_bytes(bytes),
        ByteOrder::BigEndian => u16::from_be_bytes(bytes),
    }
}

fn opposite(byte_order: ByteOrder) -> ByteOrder {
    match byte_order {
        ByteOrder::LittleEndian => ByteOrder::BigEndian,
        ByteOrder::BigEndian => ByteOrder::LittleEndian,
    }
}

fn count_fits(size: usize, count: u16) -> bool {
    16usize
        .checked_add(usize::from(count).saturating_mul(6))
        .is_some_and(|needed| size >= needed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(version: &[u8; 4], count: u16, order: ByteOrder) -> Vec<u8> {
        let mut data = b"PrintIM\0".to_vec();
        data.extend_from_slice(version);
        data.extend_from_slice(&[0, 0]);
        let count_bytes = match order {
            ByteOrder::LittleEndian => count.to_le_bytes(),
            ByteOrder::BigEndian => count.to_be_bytes(),
        };
        data.extend_from_slice(&count_bytes);
        data.resize(16 + usize::from(count) * 6, 0);
        data
    }

    #[test]
    fn decodes_known_versions_in_either_byte_order() {
        for (version, order) in [
            (b"0100", ByteOrder::LittleEndian),
            (b"0250", ByteOrder::BigEndian),
            (b"0300", ByteOrder::LittleEndian),
        ] {
            let data = block(version, 2, order);
            assert_eq!(
                decode_print_im_version(&data, order).as_deref(),
                Some(std::str::from_utf8(version).unwrap())
            );
        }
    }

    #[test]
    fn retries_the_count_with_opposite_byte_order() {
        // Count 1 is encoded big-endian but the containing IFD says little.
        // Interpreting it as 256 overruns, so ProcessPrintIM toggles and retries.
        let data = block(b"0250", 1, ByteOrder::BigEndian);
        assert_eq!(
            decode_print_im_version(&data, ByteOrder::LittleEndian).as_deref(),
            Some("0250")
        );
    }

    #[test]
    fn rejects_every_process_print_im_validation_failure() {
        assert!(decode_print_im_version(&[], ByteOrder::LittleEndian).is_none());
        assert!(decode_print_im_version(b"PrintIM\0", ByteOrder::LittleEndian).is_none());

        let mut bad_magic = block(b"0250", 0, ByteOrder::LittleEndian);
        bad_magic[0] = b'X';
        assert!(decode_print_im_version(&bad_magic, ByteOrder::LittleEndian).is_none());

        // 0x0101 overruns in both orders (257 entries need 1558 bytes).
        let mut bad_count = block(b"0250", 0, ByteOrder::LittleEndian);
        bad_count[14..16].copy_from_slice(&[1, 1]);
        assert!(decode_print_im_version(&bad_count, ByteOrder::LittleEndian).is_none());
    }
}
