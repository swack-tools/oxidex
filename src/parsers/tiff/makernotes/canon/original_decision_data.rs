//! `Composite:OriginalDecisionData` -- `Image::ExifTool::Canon::ReadODD`
//! (Canon.pm:10368-10460), forward-ported from main bb326810 (#718).
//!
//! The Composite (Canon.pm:10082-10087) requires
//! `OriginalDecisionDataOffset` (Main 0x0083, a residual hand arm) and its
//! `RawConv` reads the block at that FILE offset. The block lives outside the
//! EXIF payload, so it is read from the file once the maker note has been
//! parsed. main ported version 3 only, with the EXIF byte order; this is the
//! whole sub: versions 1-3, the `$version > 20` byte-order toggle, and the
//! version-3 third length that counts its own word only when it is `>= 4`.

use crate::core::{FileReader, MetadataMap, TagValue};

/// Reads `[offset, offset + len)` of the file, or `None` past its end --
/// `$raf->Read($buff, $len) == $len`.
fn read_exact(reader: &dyn FileReader, offset: u64, len: usize) -> Option<Vec<u8>> {
    let end = offset.checked_add(u64::try_from(len).ok()?)?;
    if end > reader.size() {
        return None;
    }
    reader.read(offset, len).ok().map(<[u8]>::to_vec)
}

fn get32(bytes: &[u8], at: usize, little_endian: bool) -> Option<u32> {
    let word: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(if little_endian {
        u32::from_le_bytes(word)
    } else {
        u32::from_be_bytes(word)
    })
}

/// `ReadODD($et, $offset)`: the ODD block, or `None` where the sub returns
/// `undef` (no offset, bad header, unsupported version, short read).
/// `little_endian` is the byte order current when ExifTool evaluates the
/// Composite -- the EXIF block's.
fn read_odd(reader: &dyn FileReader, offset: u64, little_endian: bool) -> Option<Vec<u8>> {
    if offset == 0 {
        return None;
    }
    // `$buff=~/^\xff{4}.\0\0/s`.
    let mut buff = read_exact(reader, offset, 8)?;
    if buff[..4] != [0xff; 4] || buff[5] != 0 || buff[6] != 0 {
        return None;
    }
    let mut little_endian = little_endian;
    let mut version = get32(&buff, 4, little_endian)?;
    if version > 20 {
        little_endian = !little_endian;
        version = version.swap_bytes();
    }
    let mut pos = offset + 8;
    let mut read = |buff: &mut Vec<u8>, len: usize| -> Option<Vec<u8>> {
        let chunk = read_exact(reader, pos, len)?;
        pos += len as u64;
        buff.extend_from_slice(&chunk);
        Some(chunk)
    };
    match version {
        // 20-byte sha1 + record count, then `count` 32-byte records.
        1 | 2 => {
            let head = read(&mut buff, 24)?;
            let count = get32(&head, 20, little_endian)?;
            if count == 0 || count >= 20 {
                return None;
            }
            read(&mut buff, count as usize * 32)?;
        }
        // Three length-prefixed records; the third length includes its own
        // word ("doh!") when it is at least 4.
        3 => {
            for i in 0..3 {
                let word = read(&mut buff, 4)?;
                let mut len = get32(&word, 0, little_endian)?;
                if i == 2 && len >= 4 {
                    len -= 4;
                }
                if len > 0x10000 {
                    return None;
                }
                read(&mut buff, len as usize)?;
            }
        }
        _ => return None,
    }
    Some(buff)
}

/// `ReadODD` over bytes held in memory: the OriginalDecisionData block at
/// `offset` of `file`, or `None` as [`read_odd`] returns it. For the EXIF
/// writers' check that a rewrite left the block where the maker note's
/// `OriginalDecisionDataOffset` locates it (`writers::makernote_guard`).
pub(crate) fn odd_block(file: &[u8], offset: u64, little_endian: bool) -> Option<Vec<u8>> {
    struct Bytes<'a>(&'a [u8]);
    impl crate::core::FileReader for Bytes<'_> {
        fn read(&self, offset: u64, length: usize) -> std::io::Result<&[u8]> {
            let start = usize::try_from(offset).map_err(|_| std::io::ErrorKind::InvalidInput)?;
            start
                .checked_add(length)
                .and_then(|end| self.0.get(start..end))
                .ok_or_else(|| std::io::ErrorKind::UnexpectedEof.into())
        }
        fn size(&self) -> u64 {
            self.0.len() as u64
        }
    }
    read_odd(&Bytes(file), offset, little_endian)
}

/// Adds `Composite:OriginalDecisionData` when the Canon maker note reported
/// an `OriginalDecisionDataOffset` and the block there reads as ExifTool's
/// `ReadODD` requires.
pub fn process_original_decision_data(reader: &dyn FileReader, metadata: &mut MetadataMap) {
    let offset = metadata
        .get_integer("Canon:OriginalDecisionDataOffset")
        .and_then(|value| u64::try_from(value).ok())
        .or_else(|| {
            metadata
                .get_string("Canon:OriginalDecisionDataOffset")
                .and_then(|value| value.parse().ok())
        });
    let Some(offset) = offset else {
        return;
    };
    let little_endian = metadata
        .get_string("File:ExifByteOrder")
        .is_some_and(|order| order.starts_with("Little-endian"));
    if let Some(block) = read_odd(reader, offset, little_endian) {
        metadata.insert("Composite:OriginalDecisionData", TagValue::Binary(block));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    fn metadata_for(offset: usize) -> MetadataMap {
        let mut metadata = MetadataMap::new();
        metadata.insert(
            "Canon:OriginalDecisionDataOffset",
            TagValue::Integer(offset as i64),
        );
        metadata.insert(
            "File:ExifByteOrder",
            TagValue::String("Little-endian (Intel, II)".to_string()),
        );
        metadata
    }

    fn odd(file: Vec<u8>, offset: usize) -> Option<TagValue> {
        let reader = TestReader::new(file);
        let mut metadata = metadata_for(offset);
        process_original_decision_data(&reader, &mut metadata);
        metadata.get("Composite:OriginalDecisionData").cloned()
    }

    #[test]
    fn version_three_reads_three_length_prefixed_records() {
        let offset = 16;
        let mut file = vec![0_u8; offset];
        file.extend_from_slice(&[0xff; 4]);
        file.extend_from_slice(&3_u32.to_le_bytes());
        file.extend_from_slice(&20_u32.to_le_bytes());
        file.extend_from_slice(&[1_u8; 20]);
        file.extend_from_slice(&20_u32.to_le_bytes());
        file.extend_from_slice(&[2_u8; 20]);
        file.extend_from_slice(&12_u32.to_le_bytes());
        file.extend_from_slice(&[3_u8; 8]);
        let expected = file[offset..].to_vec();
        file.extend_from_slice(&[9_u8; 16]); // trailing data is not part of it
        assert_eq!(odd(file, offset), Some(TagValue::Binary(expected)));
    }

    /// A third length below 4 is not reduced (`$len -= 4 if ... $len >= 4`).
    #[test]
    fn version_three_short_third_length_is_read_as_is() {
        let mut file = vec![0xff; 4];
        file.extend_from_slice(&3_u32.to_le_bytes());
        file.extend_from_slice(&0_u32.to_le_bytes());
        file.extend_from_slice(&0_u32.to_le_bytes());
        file.extend_from_slice(&3_u32.to_le_bytes());
        file.extend_from_slice(&[7_u8; 3]);
        let expected = file.clone();
        // Offset 0 is `return undef unless $offset`.
        assert_eq!(odd(file.clone(), 0), None);
        let mut shifted = vec![0_u8; 4];
        shifted.extend_from_slice(&file);
        assert_eq!(odd(shifted, 4), Some(TagValue::Binary(expected)));
    }

    /// A big-endian version 1 block under a little-endian EXIF order: the
    /// version reads > 20, so the order toggles.
    #[test]
    fn version_one_toggles_the_byte_order() {
        let offset = 4;
        let mut file = vec![0_u8; offset];
        file.extend_from_slice(&[0xff; 4]);
        file.extend_from_slice(&1_u32.to_be_bytes());
        file.extend_from_slice(&[0xaa; 20]);
        file.extend_from_slice(&1_u32.to_be_bytes());
        file.extend_from_slice(&[0xbb; 32]);
        let expected = file[offset..].to_vec();
        assert_eq!(odd(file, offset), Some(TagValue::Binary(expected)));
    }

    #[test]
    fn rejects_bad_header_version_and_oversized_records() {
        let mut bad = vec![0xff, 0xff, 0xff, 0xfe];
        bad.extend_from_slice(&3_u32.to_le_bytes());
        let mut shifted = vec![0_u8; 4];
        shifted.extend_from_slice(&bad);
        assert_eq!(odd(shifted, 4), None);

        let mut v4 = vec![0_u8; 4];
        v4.extend_from_slice(&[0xff; 4]);
        v4.extend_from_slice(&4_u32.to_le_bytes());
        assert_eq!(odd(v4, 4), None);

        let mut big = vec![0_u8; 4];
        big.extend_from_slice(&[0xff; 4]);
        big.extend_from_slice(&3_u32.to_le_bytes());
        big.extend_from_slice(&0x10001_u32.to_le_bytes());
        assert_eq!(odd(big, 4), None);
    }
}
