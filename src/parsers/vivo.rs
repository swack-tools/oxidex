//! Vivo trailer reader (`Trailer.pm::ProcessVivo`).

use crate::core::{MetadataMap, TagValue};
use crate::parsers::trailer;

const FOOTER: &[u8] = b"\xff\xff\xff\xff\x1b*9HWfu\x84\x93\xa2\xb1";
const JSON_MARKER: &[u8] = b"vivo{\"";
const STREAM_MARKER: &[u8] = b"streamdata";

/// Trailer.pm starts at the first `streamdata|vivo{\"` match, not the last
/// occurrence before the footer.  That position is also the positive-offset
/// boundary ProcessTrailers uses when it proceeds inward to Samsung.
fn first_vivo_marker(prefix: &[u8]) -> Option<usize> {
    match (
        memchr::memmem::find(prefix, STREAM_MARKER),
        memchr::memmem::find(prefix, JSON_MARKER),
    ) {
        (Some(stream), Some(json)) => Some(stream.min(json)),
        (Some(stream), None) => Some(stream),
        (None, Some(json)) => Some(json),
        (None, None) => None,
    }
}

/// Locate the start of the final Vivo trailer using Trailer.pm's first marker
/// plus its fixed EOF footer.
/// This is shared with Samsung because ProcessTrailers can expose Samsung at
/// the positive offset immediately before this bounded suffix.
pub(crate) fn validated_vivo_suffix_start(file: &[u8]) -> Option<usize> {
    let footer_start = file.len().checked_sub(FOOTER.len())?;
    if !file.ends_with(FOOTER) {
        return None;
    }
    let prefix = file.get(..footer_start)?;
    first_vivo_marker(prefix)
}

/// Retains JSON only when ExifTool's fixed footer and `}\0` boundary validate it.
pub fn parse_vivo_trailer(file: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let Some(json) = trailer::find_last(file, FOOTER.len(), FOOTER, FOOTER.len(), |file, end| {
        let footer_start = end.checked_sub(FOOTER.len())?;
        let prefix = file.get(..footer_start)?;
        let marker = first_vivo_marker(prefix)?;
        let suffix = prefix.get(marker..)?;
        let json_at = memchr::memmem::find(suffix, JSON_MARKER)?;
        let json_start = marker.checked_add(json_at)?.checked_add(4)?;
        let json_end =
            memchr::memmem::find(&prefix[json_start..], b"}\0")?.checked_add(json_start + 1)?;
        std::str::from_utf8(prefix.get(json_start..json_end)?).ok()
    }) else {
        return metadata;
    };
    metadata.insert_with_group1("Vivo:JSONInfo", TagValue::new_string(json), "Vivo");
    metadata
}
