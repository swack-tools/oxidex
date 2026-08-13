//! Vivo trailer parser (`Trailer.pm:77-117`).
//!
//! Vivo appends metadata after the image data.  Its footer has no length, so
//! ExifTool validates the fixed 16-byte trailer terminator, then scans for the
//! `vivo{"` JSON marker and reads through the first `}\0` terminator.

use crate::core::{MetadataMap, TagValue};
use crate::parsers::trailer;

/// Trailer.pm's `ProcessVivo` footer validation literal.
const FOOTER: &[u8] = b"\xff\xff\xff\xff\x1b*9HWfu\x84\x93\xa2\xb1";
const JSON_MARKER: &[u8] = b"vivo{\"";

/// Extract Vivo's raw JSONInfo trailer tag when both ExifTool boundaries are
/// present.  The JSON is intentionally retained verbatim: Trailer.pm applies
/// no conversion to the byte slice it passes to `HandleTag`.
pub fn parse_vivo_trailer(file: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let Some(json) = trailer::find_last(file, FOOTER.len(), FOOTER, FOOTER.len(), |file, end| {
        let footer_start = end.checked_sub(FOOTER.len())?;
        let prefix = file.get(..footer_start)?;
        let marker = memchr::memmem::rfind(prefix, JSON_MARKER)?;
        let json_start = marker.checked_add(4)?; // `pos($buff) - 2`: the `{`
        let json_end =
            memchr::memmem::find(&prefix[json_start..], b"}\0")?.checked_add(json_start + 1)?; // exclude ExifTool's NUL terminator
        std::str::from_utf8(prefix.get(json_start..json_end)?).ok()
    }) else {
        return metadata;
    };
    metadata.insert("Vivo:JSONInfo", TagValue::String(json.to_string()));
    metadata
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_footer_validated_vivo_json() {
        let mut file = b"jpeg...vivo{\"hdr\":20737}\0ignored".to_vec();
        file.extend_from_slice(FOOTER);
        assert_eq!(
            parse_vivo_trailer(&file).get_string("Vivo:JSONInfo"),
            Some("{\"hdr\":20737}")
        );
        assert!(parse_vivo_trailer(b"vivo{\"hdr\":20737}\0").is_empty());
    }
}
