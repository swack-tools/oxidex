//! Vivo trailer reader (`Trailer.pm::ProcessVivo`).

use crate::core::{MetadataMap, TagValue};
use crate::parsers::trailer;

const FOOTER: &[u8] = b"\xff\xff\xff\xff\x1b*9HWfu\x84\x93\xa2\xb1";
const JSON_MARKER: &[u8] = b"vivo{\"";

/// Locate the start of the final Vivo trailer using the same structural
/// boundaries ProcessVivo uses: a recognized payload and its fixed EOF footer.
/// This is shared with Samsung because ProcessTrailers can expose Samsung at
/// the positive offset immediately before this bounded suffix.
pub(crate) fn validated_vivo_suffix_start(file: &[u8]) -> Option<usize> {
    let footer_start = file.len().checked_sub(FOOTER.len())?;
    if !file.ends_with(FOOTER) {
        return None;
    }
    let prefix = file.get(..footer_start)?;
    if let Some(marker) = memchr::memmem::rfind(prefix, JSON_MARKER) {
        let json_start = marker.checked_add(4)?;
        memchr::memmem::find(&prefix[json_start..], b"}\0")?;
        return Some(marker);
    }
    let marker = memchr::memmem::rfind(prefix, b"streamdata")?;
    let stream = prefix.get(marker..)?;
    (stream.starts_with(b"streamdata\xff\xd8\xff")
        && (memchr::memmem::find(stream, b"\xff\xd9streaminfo").is_some()
            || memchr::memmem::find(stream, b"\xff\xd9streamcoun").is_some()))
    .then_some(marker)
}

/// Retains JSON only when ExifTool's fixed footer and `}\0` boundary validate it.
pub fn parse_vivo_trailer(file: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let Some(json) = trailer::find_last(file, FOOTER.len(), FOOTER, FOOTER.len(), |file, end| {
        let footer_start = end.checked_sub(FOOTER.len())?;
        let prefix = file.get(..footer_start)?;
        let marker = memchr::memmem::rfind(prefix, JSON_MARKER)?;
        let json_start = marker.checked_add(4)?;
        let json_end =
            memchr::memmem::find(&prefix[json_start..], b"}\0")?.checked_add(json_start + 1)?;
        std::str::from_utf8(prefix.get(json_start..json_end)?).ok()
    }) else {
        return metadata;
    };
    metadata.insert_with_group1("Vivo:JSONInfo", TagValue::new_string(json), "Vivo");
    metadata
}
