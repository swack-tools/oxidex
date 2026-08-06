//! MIE trailer marker parser.
//!
//! ExifTool 13.59 (`ExifTool.pm:7007-7010`, `MIE.pm:1693-1730`) recognizes a
//! MIE trailer from the final empty `zmie` element in a main `0MIE` group. The
//! final element is followed by the group terminator, whose length reaches
//! back exactly to the `0MIE` header. The trailer may sit inside later
//! trailers, so candidates are scanned from the end and validated at both
//! ends rather than read only at EOF.

use crate::core::{MetadataMap, TagValue};
use crate::parsers::trailer;

const GROUP_HEADER: &[u8; 4] = b"0MIE";
const SHORT_TRAILER_MARKER: &[u8] = b"~\0\x04\0zmie~\0\0\x06";
const LONG_TRAILER_MARKER: &[u8] = b"~\0\x04\0zmie~\0\0\x0a";

/// Extracts the `MIE-Main:TrailerSignature` tag from a valid MIE trailer.
///
/// `TrailerSignature` is a marker with an empty value, so ExifTool 13.59
/// renders it as an empty string. No other MIE elements are decoded here.
pub fn parse_mie_trailer(file: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    if find_trailer(file).is_some() {
        metadata.insert("MIE-Main:TrailerSignature", TagValue::String(String::new()));
    }
    metadata
}

/// Finds the last valid MIE trailer footer, including ExifTool's two supported
/// data-length encodings: four bytes (`0x06`) and eight bytes (`0x0a`).
fn find_trailer(file: &[u8]) -> Option<usize> {
    let short = trailer::find_last(
        file,
        SHORT_TRAILER_MARKER.len(),
        SHORT_TRAILER_MARKER,
        SHORT_TRAILER_MARKER.len() + 6,
        |file, end| valid_trailer_end(file, end, 4).then_some(end),
    );
    let long = trailer::find_last(
        file,
        LONG_TRAILER_MARKER.len(),
        LONG_TRAILER_MARKER,
        LONG_TRAILER_MARKER.len() + 10,
        |file, end| valid_trailer_end(file, end, 8).then_some(end),
    );
    short.into_iter().chain(long).max()
}

/// Validates ExifTool's second trailer boundary: the footer's byte-order-aware
/// length must point to a main MIE group header.
fn valid_trailer_end(file: &[u8], end: usize, length_width: usize) -> bool {
    let footer_len = 4 + length_width + 2;
    let Some(footer) = end.checked_sub(footer_len).and_then(|at| file.get(at..end)) else {
        return false;
    };
    if footer[..4] != [b'~', 0, 0, if length_width == 4 { 6 } else { 10 }]
        || footer[footer_len - 1] != length_width as u8
    {
        return false;
    }
    let length_bytes = &footer[4..4 + length_width];
    let length = match (footer[footer_len - 2], length_width) {
        (0x10, 4) => u32::from_be_bytes(length_bytes.try_into().expect("four bytes")) as u64,
        (0x18, 4) => u32::from_le_bytes(length_bytes.try_into().expect("four bytes")) as u64,
        (0x10, 8) => u64::from_be_bytes(length_bytes.try_into().expect("eight bytes")),
        (0x18, 8) => u64::from_le_bytes(length_bytes.try_into().expect("eight bytes")),
        _ => return false,
    };
    let Ok(length) = usize::try_from(length) else {
        return false;
    };
    let Some(group) = end.checked_sub(length).and_then(|at| file.get(at..)) else {
        return false;
    };
    length >= 12
        && group.len() >= 8
        && group[0] == b'~'
        && matches!(group[1], 0x10 | 0x18)
        && group[2] == 4
        && group[4..8] == *GROUP_HEADER
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trailer(length_width: usize, byte_order: u8) -> Vec<u8> {
        let mut file = b"image data".to_vec();
        let group_start = file.len();
        file.extend_from_slice(&[b'~', byte_order, 4, 12]);
        file.extend_from_slice(GROUP_HEADER);
        file.extend_from_slice(b"body");
        file.extend_from_slice(b"~\0\x04\0zmie");
        file.extend_from_slice(&[b'~', 0, 0, if length_width == 4 { 6 } else { 10 }]);
        let length = (file.len() + length_width + 2 - group_start) as u64;
        match (length_width, byte_order) {
            (4, 0x10) => file.extend_from_slice(&(length as u32).to_be_bytes()),
            (4, 0x18) => file.extend_from_slice(&(length as u32).to_le_bytes()),
            (8, 0x10) => file.extend_from_slice(&length.to_be_bytes()),
            (8, 0x18) => file.extend_from_slice(&length.to_le_bytes()),
            _ => unreachable!(),
        }
        file.extend_from_slice(&[byte_order, length_width as u8]);
        file
    }

    #[test]
    fn accepts_both_mie_trailer_length_encodings() {
        for (width, order) in [(4, 0x10), (4, 0x18), (8, 0x10), (8, 0x18)] {
            let metadata = parse_mie_trailer(&trailer(width, order));
            assert_eq!(metadata.get_string("MIE-Main:TrailerSignature"), Some(""));
        }
    }

    #[test]
    fn rejects_a_zmie_marker_without_a_main_group_at_its_declared_start() {
        let mut file = trailer(4, 0x10);
        let group_start = file
            .windows(GROUP_HEADER.len())
            .position(|w| w == GROUP_HEADER)
            .unwrap();
        file[group_start] = b'X';
        assert!(parse_mie_trailer(&file).is_empty());
    }
}
