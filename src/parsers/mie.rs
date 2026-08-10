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

/// Element budget for [`subfile_identifier`], well past any real MIE file's
/// top-level tag count. Bounds a malformed file rather than modelling a real
/// limit ExifTool enforces.
const MAX_TOP_LEVEL_ELEMENTS: usize = 10_000;

/// `%mimeType{MIE}`, `application/x-mie` -- the base type [`combine_mime`]
/// starts from. Not read from the generated table because this file has no
/// `crate::filetype::identify` call in its path to ask.
const MIE_BASE_MIME: &str = "application/x-mie";

/// ExifTool's `MIMEType` for a `.mie` file: `application/x-mie`, sharpened to
/// name the subfile it encapsulates.
///
/// A MIE file's own MIME type is generic; a real answer names the payload
/// too, e.g. `image/x-mie-jpeg` for a wrapped JPEG. `ProcessMIE` builds that
/// from the top-level `0Type` or `2MIME` tag -- whichever was read last
/// (MIE.pm:1609, 1681):
///
/// ```text
///     $mime = $val if $tag eq '0Type' or $tag eq '2MIME';
///     ...
///     $mime and not $$dirInfo{Parent} and $et->ModifyMimeType($mime);
/// ```
///
/// `not $$dirInfo{Parent}` is why only the file's own top-level group is
/// searched: a subfile nested inside a MIE group (an embedded thumbnail, say)
/// carries its own `0Type`/`2MIME` too, and those must not leak into the
/// outer file's MIME type.
///
/// `None` when the file has no `0Type`/`2MIME` at its top level, or the walk
/// cannot make sense of the structure -- either way, the caller's own
/// `application/x-mie` default from `%mimeType` stands.
pub(crate) fn document_mime_type(file: &[u8]) -> Option<String> {
    let mime = subfile_identifier(file)?;
    combine_mime(MIE_BASE_MIME, &mime)
}

/// The value of the last `0Type` or `2MIME` tag directly inside the file's
/// top-level `0MIE` group.
///
/// Mirrors `ProcessMIE`'s file-level header handling followed by
/// `ProcessMIEGroup`'s element loop, restricted to depth 1: a nested group
/// element is skipped over rather than entered when its own data length says
/// where it ends (MIE.pm:1699-1712 for the header, 1483-1580 for the element
/// loop -- the tag-value capture is 1609, quoted on [`document_mime_type`]).
///
/// A group element can also declare a length of zero, meaning "read my
/// contents from right here in the stream" rather than "skip N bytes" --
/// `MIE.mie`'s `Meta` and `Camera` groups are both this shape. Those are
/// still skipped correctly: entering them just means the *next* elements read
/// are that group's own tags, one level deeper, until its terminator (a
/// zero-`TagLength` element) is reached and depth returns to where it was.
fn subfile_identifier(file: &[u8]) -> Option<String> {
    // File-level header: Sync + FormatCode + TagLength(=4) + DataLength +
    // "0MIE" (ProcessMIE's own regex, MIE.pm:1701:
    // `/^~(\x10|\x18)\x04(.)0MIE/s`). TagLength is asserted rather than read,
    // matching the literal `\x04` in that pattern.
    let header = file.get(0..8)?;
    if header[0] != b'~' || header[2] != 4 || &header[4..8] != b"0MIE" {
        return None;
    }
    // 0x10 = MM (big-endian), 0x18 = II (little-endian) -- the same code as
    // any other MIE group, just spelled out here because there is no
    // enclosing group to have set it already.
    let doc_little_endian = match header[1] {
        0x10 => false,
        0x18 => true,
        _ => return None,
    };

    // ProcessMIE never decodes this length -- it only needs to know how many
    // extension bytes to step over, which is `1 << (256 - code)` for any code
    // above 252 (MIE.pm:1706-1707). The actual byte count skipped for 253/
    // 254/255 is 8/4/2, i.e. `mie_data_length`'s own extension widths; reusing
    // it here keeps the two derivations from silently drifting apart.
    let mut pos = 8usize;
    if header[3] > 252 {
        let (_, extension_width) = mie_data_length(file, pos, header[3], doc_little_endian)?;
        pos += extension_width;
    }

    // One entry per group currently open below the top level, holding the
    // byte order in effect for elements at that depth. Empty means "still
    // directly inside the top-level 0MIE group", which is the only depth
    // `not $$dirInfo{Parent}` allows a capture at.
    let mut open_groups: Vec<bool> = Vec::new();
    let mut mime = None;

    for _ in 0..MAX_TOP_LEVEL_ELEMENTS {
        let header = file.get(pos..pos + 4)?;
        if header[0] != b'~' {
            return mime;
        }
        let format = header[1];
        let tag_len = usize::from(header[2]);
        let length_code = header[3];
        let little_endian = open_groups.last().copied().unwrap_or(doc_little_endian);

        let tag_start = pos + 4;
        let tag = file.get(tag_start..tag_start + tag_len)?;
        let data_pos = tag_start + tag_len;
        let (data_len, extension_width) =
            mie_data_length(file, data_pos, length_code, little_endian)?;
        let data_start = data_pos + extension_width;
        let data = file.get(data_start..data_start + data_len)?;
        pos = data_start + data_len;

        if tag_len == 0 {
            // Group terminator. Closes the innermost open group, or -- with
            // none open -- ends the top-level group itself, which is as far
            // as this function ever needs to look.
            if open_groups.pop().is_none() {
                return mime;
            }
            continue;
        }

        if format & 0xf0 == 0x10 {
            // A group. `format & 0x08` is its own byte order, which governs
            // its contents but not this element's own header -- exactly the
            // asymmetry `little_endian` above already captures for every
            // element, group or not.
            if data_len == 0 {
                // Streamed inline: its elements are read next, one level
                // deeper, ending at its own terminator.
                open_groups.push(format & 0x08 != 0);
            }
            // A self-contained group (data_len > 0) needs nothing further:
            // `pos` already skipped past its entire encoded form above.
            continue;
        }

        if open_groups.is_empty() && (tag == b"0Type" || tag == b"2MIME") {
            mime = std::str::from_utf8(data).ok().map(str::to_string);
        }
    }
    mime
}

/// `ModifyMimeType`'s `a/b + c/d => c/b-d` (ExifTool.pm:9748-9761).
///
/// `new_type` is either a literal MIME type (contains `/`) or a bare file
/// type name to resolve through `%mimeType` first -- `0Type` gives "JPEG",
/// not a MIME type, while `2MIME` (when present) already is one.
fn combine_mime(old: &str, new_type: &str) -> Option<String> {
    let resolved;
    let new_type = if new_type.contains('/') {
        new_type
    } else {
        resolved = crate::filetype::mime_for_type(new_type)?;
        resolved
    };
    let (_, old_subtype) = old.split_once('/')?;
    let (new_type_part, new_subtype) = new_type.split_once('/')?;
    let new_subtype = new_subtype.strip_prefix("x-").unwrap_or(new_subtype);
    Some(format!("{new_type_part}/{old_subtype}-{new_subtype}"))
}

#[cfg(test)]
mod document_mime_tests {
    use super::*;

    /// One MIE element: `~` + FormatCode + TagLength + DataLength + tag + data.
    /// Short form only -- every fixture here fits under 253 bytes of data.
    fn element(format: u8, tag: &[u8], data: &[u8]) -> Vec<u8> {
        let mut out = vec![b'~', format, tag.len() as u8, data.len() as u8];
        out.extend_from_slice(tag);
        out.extend_from_slice(data);
        out
    }

    /// The group terminator: a zero-`TagLength` element with no data.
    fn terminator() -> Vec<u8> {
        vec![b'~', 0, 0, 0]
    }

    /// A `.mie` file: the file-level `0MIE` header (big-endian, short-form
    /// length since real files are far under 253 bytes in these tests), then
    /// `elements`, then the top-level terminator.
    fn mie_file(elements: &[Vec<u8>]) -> Vec<u8> {
        let body_len: usize = elements.iter().map(Vec::len).sum::<usize>() + terminator().len();
        let mut out = vec![b'~', 0x10, 4, body_len as u8];
        out.extend_from_slice(b"0MIE");
        for e in elements {
            out.extend_from_slice(e);
        }
        out.extend_from_slice(&terminator());
        out
    }

    #[test]
    fn a_2mime_tag_names_the_subfile_directly() {
        // MIE.mie's own shape: 0Type "JPEG" followed by 2MIME "image/jpeg" --
        // the later tag wins per ExifTool's last-one-set rule.
        let file = mie_file(&[
            element(0x20, b"0Type", b"JPEG"),
            element(0x20, b"2MIME", b"image/jpeg"),
        ]);
        assert_eq!(
            document_mime_type(&file).as_deref(),
            Some("image/x-mie-jpeg")
        );
    }

    #[test]
    fn a_bare_0type_resolves_through_the_mime_table() {
        let file = mie_file(&[element(0x20, b"0Type", b"JPEG")]);
        assert_eq!(
            document_mime_type(&file).as_deref(),
            Some("image/x-mie-jpeg")
        );
    }

    #[test]
    fn an_x_subtype_is_not_doubled() {
        // "image/x-mie-x-raw" would be wrong; ExifTool strips the leading
        // "x-" from the subfile's own subtype before splicing it in.
        let file = mie_file(&[element(0x20, b"2MIME", b"image/x-raw")]);
        assert_eq!(
            document_mime_type(&file).as_deref(),
            Some("image/x-mie-raw")
        );
    }

    #[test]
    fn a_self_contained_nested_group_is_skipped_by_its_own_length() {
        // A group element with data_len > 0 is a complete blob; a 0Type
        // inside it must not surface as the outer file's answer.
        let inner = element(0x20, b"0Type", b"PNG");
        let file = mie_file(&[
            element(0x10, b"Thumbnail", &inner),
            element(0x20, b"0Type", b"JPEG"),
        ]);
        assert_eq!(
            document_mime_type(&file).as_deref(),
            Some("image/x-mie-jpeg")
        );
    }

    #[test]
    fn a_streamed_nested_group_is_walked_past_not_into() {
        // MIE.mie's actual shape: `Meta` and `Camera` both declare data_len 0
        // ("read my contents from right here"), nested two deep, each ending
        // at its own terminator. A tag inside either must not be captured,
        // and the walk must still reach the real answer afterward.
        let file = mie_file(&[
            element(0x20, b"2MIME", b"image/jpeg"),
            element(0x10, b"Meta", &[]),
            element(0x10, b"Camera", &[]),
            element(0x20, b"0Type", b"NotTheAnswer"),
            terminator(), // closes Camera
            terminator(), // closes Meta
        ]);
        assert_eq!(
            document_mime_type(&file).as_deref(),
            Some("image/x-mie-jpeg")
        );
    }

    #[test]
    fn no_top_level_tag_leaves_the_generic_mime_type_alone() {
        let file = mie_file(&[element(0x20, b"0Vers", b"1.1")]);
        assert_eq!(document_mime_type(&file), None);
    }

    #[test]
    fn little_endian_document_header_is_accepted() {
        let mut file = mie_file(&[element(0x20, b"0Type", b"JPEG")]);
        file[1] = 0x18; // II
        assert_eq!(
            document_mime_type(&file).as_deref(),
            Some("image/x-mie-jpeg")
        );
    }

    #[test]
    fn a_non_mie_file_is_declined() {
        assert_eq!(document_mime_type(b"not a mie file at all"), None);
    }

    #[test]
    fn combine_mime_matches_modifymimetype() {
        assert_eq!(
            combine_mime("application/x-mie", "image/jpeg").as_deref(),
            Some("image/x-mie-jpeg")
        );
        assert_eq!(
            combine_mime("application/x-mie", "image/x-raw").as_deref(),
            Some("image/x-mie-raw")
        );
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
