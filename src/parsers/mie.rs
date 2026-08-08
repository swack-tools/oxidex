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

/// Extracts the supported MIE trailer tags from a valid MIE trailer.
///
/// `TrailerSignature` is a marker with an empty value.  The trailer is still
/// a normal MIE hierarchy, so the one losslessly-described nested route seen
/// in the pinned JPEG fixture -- `MIE-Meta` / `MIE-Doc` / UTF-8 `Copyright` --
/// is decoded as well.
pub fn parse_mie_trailer(file: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    if let Some(trailer) = find_trailer(file) {
        // ExifTool's group-1 name is MIE-Main, but the comparison harness's
        // canonical family for MIE trailers is MIE.
        metadata.insert("MIE:TrailerSignature", TagValue::String(String::new()));
        extract_document_copyright(file, trailer, &mut metadata);
    }
    metadata
}

#[derive(Clone, Copy)]
struct MieTrailer {
    start: usize,
    end: usize,
}

/// Finds the last valid MIE trailer footer, including ExifTool's two supported
/// data-length encodings: four bytes (`0x06`) and eight bytes (`0x0a`).
fn find_trailer(file: &[u8]) -> Option<MieTrailer> {
    let short = trailer::find_last(
        file,
        SHORT_TRAILER_MARKER.len(),
        SHORT_TRAILER_MARKER,
        SHORT_TRAILER_MARKER.len() + 6,
        |file, end| trailer_start(file, end, 4).map(|start| MieTrailer { start, end }),
    );
    let long = trailer::find_last(
        file,
        LONG_TRAILER_MARKER.len(),
        LONG_TRAILER_MARKER,
        LONG_TRAILER_MARKER.len() + 10,
        |file, end| trailer_start(file, end, 8).map(|start| MieTrailer { start, end }),
    );
    short
        .into_iter()
        .chain(long)
        .max_by_key(|trailer| trailer.end)
}

/// Validates ExifTool's second trailer boundary: the footer's byte-order-aware
/// length must point to a main MIE group header.
fn trailer_start(file: &[u8], end: usize, length_width: usize) -> Option<usize> {
    let footer_len = 4 + length_width + 2;
    let Some(footer) = end.checked_sub(footer_len).and_then(|at| file.get(at..end)) else {
        return None;
    };
    if footer[..4] != [b'~', 0, 0, if length_width == 4 { 6 } else { 10 }]
        || footer[footer_len - 1] != length_width as u8
    {
        return None;
    }
    let length_bytes = &footer[4..4 + length_width];
    let length = match (footer[footer_len - 2], length_width) {
        (0x10, 4) => u32::from_be_bytes(length_bytes.try_into().expect("four bytes")) as u64,
        (0x18, 4) => u32::from_le_bytes(length_bytes.try_into().expect("four bytes")) as u64,
        (0x10, 8) => u64::from_be_bytes(length_bytes.try_into().expect("eight bytes")),
        (0x18, 8) => u64::from_le_bytes(length_bytes.try_into().expect("eight bytes")),
        _ => return None,
    };
    let Ok(length) = usize::try_from(length) else {
        return None;
    };
    let Some(group) = end.checked_sub(length).and_then(|at| file.get(at..)) else {
        return None;
    };
    (length >= 12
        && group.len() >= 8
        && group[0] == b'~'
        && matches!(group[1], 0x10 | 0x18)
        && group[2] == 4
        && group[4..8] == *GROUP_HEADER)
        .then_some(end - length)
}

/// Decode exactly the `0MIE` / `Meta` / `Document` / UTF-8 `Copyright` path
/// declared by MIE.pm.  Group elements have no inline data and are delimited
/// by the normal empty MIE terminator; other MIE tables and compression stay
/// deliberately out of scope.
fn extract_document_copyright(file: &[u8], trailer: MieTrailer, metadata: &mut MetadataMap) {
    let mut cursor = trailer.start;
    let mut groups = Vec::new();
    let mut little_endian = false;

    while cursor < trailer.end {
        let Some(header) = file.get(cursor..cursor + 4) else {
            return;
        };
        if header[0] != b'~' {
            return;
        }
        let format = header[1];
        let tag_len = usize::from(header[2]);
        let len_code = header[3];
        let tag_start = cursor + 4;
        let Some(tag_bytes) = file.get(tag_start..tag_start + tag_len) else {
            return;
        };
        let tag_end = tag_start + tag_len;
        let Some((data_len, length_width)) =
            mie_data_length(file, tag_end, len_code, little_endian)
        else {
            return;
        };
        let data_start = tag_end + length_width;
        let Some(data) = file.get(data_start..data_start + data_len) else {
            return;
        };
        cursor = data_start + data_len;

        // Empty format-0/tag-0/data-0 element terminates the current group.
        if format == 0 && tag_bytes.is_empty() && data.is_empty() {
            groups.pop();
            continue;
        }

        let Ok(tag) = std::str::from_utf8(tag_bytes) else {
            return;
        };
        if format & 0xf0 == 0x10 {
            // MIE.pm's Main and Meta tables define these as subdirectories.
            // The byte-order modifier belongs to the group it opens.
            little_endian = format & 0x08 != 0;
            groups.push(tag);
            continue;
        }

        if tag == "Copyright"
            && format == 0x28
            && groups.as_slice() == ["0MIE", "Meta", "Document"]
            && let Ok(value) = std::str::from_utf8(data)
        {
            metadata.insert("MIE:Copyright", TagValue::String(value.to_string()));
        }
    }
}

/// MIE 1.1's variable-width data-length field.  The byte order of extended
/// lengths is inherited from the containing MIE group.
fn mie_data_length(
    file: &[u8],
    at: usize,
    code: u8,
    little_endian: bool,
) -> Option<(usize, usize)> {
    match code {
        0..=252 => Some((usize::from(code), 0)),
        253 => {
            let bytes: [u8; 8] = file.get(at..at + 8)?.try_into().ok()?;
            usize::try_from(if little_endian {
                u64::from_le_bytes(bytes)
            } else {
                u64::from_be_bytes(bytes)
            })
            .ok()
            .map(|length| (length, 8))
        }
        254 => {
            let bytes: [u8; 4] = file.get(at..at + 4)?.try_into().ok()?;
            Some((
                if little_endian {
                    u32::from_le_bytes(bytes)
                } else {
                    u32::from_be_bytes(bytes)
                } as usize,
                4,
            ))
        }
        255 => {
            let bytes: [u8; 2] = file.get(at..at + 2)?.try_into().ok()?;
            Some((
                usize::from(if little_endian {
                    u16::from_le_bytes(bytes)
                } else {
                    u16::from_be_bytes(bytes)
                }),
                2,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_pinned_exiftool_jpeg_mie_document_trailer() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let file = std::fs::read("/tmp/oxidex-exiftool-cache/combined-samples/ExifTool.jpg")
            .expect("pinned ExifTool JPEG fixture should be available");
        let metadata = parse_mie_trailer(&file);

        assert_eq!(metadata.get_string("MIE:TrailerSignature"), Some(""));
        assert_eq!(
            metadata.get_string("MIE:Copyright"),
            Some("© 2006 Phil Harvey")
        );
    }

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
            assert_eq!(metadata.get_string("MIE:TrailerSignature"), Some(""));
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
