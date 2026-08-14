//! JPEG 2000 codestream metadata.
//!
//! `Jpeg2000.pm:1538-1557` delegates a bare J2C codestream to ExifTool's
//! JPEG marker processor.  Its COM markers have the normal JPEG framing:
//! marker `0xff64`, a big-endian length including the two length bytes, then
//! the comment.  Retaining each marker is important -- `Jpeg2000.j2c` has
//! two distinct comments.

use crate::core::{FileReader, MetadataMap, TagValue};

pub fn parse_jpeg2000_metadata(
    reader: &dyn FileReader,
) -> std::result::Result<MetadataMap, String> {
    let data = reader
        .read(0, reader.size() as usize)
        .map_err(|err| err.to_string())?;
    let mut metadata = MetadataMap::new();

    // Only a raw codestream has JPEG markers at offset zero.  JP2 containers
    // use the box walker in Jpeg2000.pm and are deliberately left to the
    // identity layer until that whole box/subdirectory graph is ported.
    if !data.starts_with(b"\xff\x4f") {
        return Ok(metadata);
    }

    let mut pos = 2usize;
    while pos + 1 < data.len() {
        if data[pos] != 0xff {
            pos += 1;
            continue;
        }
        let marker = data[pos + 1];
        if marker == 0xd9 {
            break;
        }
        // SOC/EOC and the two byte packet delimiters do not carry a length.
        if matches!(marker, 0x4f | 0x90 | 0x91 | 0x92 | 0x93) {
            pos += 2;
            continue;
        }
        let Some(length_bytes) = data.get(pos + 2..pos + 4) else {
            break;
        };
        let length = u16::from_be_bytes([length_bytes[0], length_bytes[1]]) as usize;
        if length < 2 {
            break;
        }
        let Some(end) = pos.checked_add(2 + length) else {
            break;
        };
        let Some(payload) = data.get(pos + 4..end) else {
            break;
        };
        // COM starts with the two-byte registration value (`Rcom`); only
        // the following bytes are the ExifTool `File:Comment` string.  Both
        // real comments in Jpeg2000.j2c use Rcom=1 (ISO/IEC 15444-1 §A.9).
        if marker == 0x64
            && let Some(comment) = payload.get(2..)
            && let Ok(comment) = std::str::from_utf8(comment)
        {
            metadata.insert("File:Comment", TagValue::new_string(comment));
        }
        pos = end;
    }
    Ok(metadata)
}
