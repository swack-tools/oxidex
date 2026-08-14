//! PC Paintbrush (PCX) image metadata parser.
//!
//! ExifTool 13.59's `PCX::ProcessPCX` (PCX.pm:80-92) reads a fixed 0x50-byte
//! header, validates it against `^\x0a[\0-\x05]\x01[\x01\x02\x04\x08].{64}[\0-\x02]`,
//! sets little-endian byte order and runs `ProcessBinaryData` over
//! `PCX::Main` (PCX.pm:20-77). This parser does the same and reads the
//! layout from the generated table rather than restating offsets here.
//!
//! # Why the table is not enough on its own
//!
//! `LeftMargin`/`TopMargin` (PCX.pm:44,48) each carry a `RawConv` that is a
//! pure `DataMember` side effect (`$$self{LeftMargin} = $val`, returning the
//! value unchanged) and `ImageWidth`/`ImageHeight` (PCX.pm:52,58) each carry
//! a `ValueConv` that subtracts the matching margin (`$val - $$self{LeftMargin}
//! + 1`). `ScreenWidth`/`ScreenHeight` (PCX.pm:75-76) carry a `RawConv` of
//! `'$val or undef'`, suppressing the tag when the raw value is zero. All
//! three shapes are hand-implemented below against the cited Perl, each
//! behind a [`RawAccess`] citation.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/PCX.pm`

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::exiftool_tables::{
    Acknowledged, DecodedValue, PerlCitation, RawAccess, decode_binary_table, find_table,
};
use crate::io::ByteOrder;

/// PCX.pm:85, `$raf->Read($buff, 0x50) == 0x50`.
const HEADER_LEN: usize = 0x50;

const fn citation(tag: &'static str, lines: &'static str) -> PerlCitation {
    PerlCitation {
        module: "PCX",
        table: "Main",
        tag,
        lines,
    }
}

const LEFT_MARGIN: PerlCitation = citation("LeftMargin", "PCX.pm:44-47");
const TOP_MARGIN: PerlCitation = citation("TopMargin", "PCX.pm:48-51");
const IMAGE_WIDTH: PerlCitation = citation("ImageWidth", "PCX.pm:52-57");
const IMAGE_HEIGHT: PerlCitation = citation("ImageHeight", "PCX.pm:58-63");
const SCREEN_WIDTH: PerlCitation = citation("ScreenWidth", "PCX.pm:75");
const SCREEN_HEIGHT: PerlCitation = citation("ScreenHeight", "PCX.pm:76");

/// PCX.pm:85-86's inline validation regex,
/// `^\x0a[\0-\x05]\x01[\x01\x02\x04\x08].{64}[\0-\x02]`.
fn header_matches_signature(header: &[u8]) -> bool {
    header.len() >= HEADER_LEN
        && header[0] == 0x0a
        && header[1] <= 0x05
        && header[2] == 0x01
        && matches!(header[3], 0x01 | 0x02 | 0x04 | 0x08)
        && matches!(header[HEADER_LEN - 1], 0x00..=0x02)
}

/// Extract PCX metadata using ExifTool's declared `PCX::Main` binary layout.
pub fn parse_pcx_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    if reader.size() < HEADER_LEN as u64 {
        return Err("PCX file is too short for the 0x50-byte header".to_string());
    }
    let header = reader
        .read(0, HEADER_LEN)
        .map_err(|error| error.to_string())?;
    if !header_matches_signature(header) {
        return Err("invalid PCX header".to_string());
    }

    let table = find_table("PCX", "Main").ok_or("missing PCX::Main table")?;
    let decode = decode_binary_table(table, header, ByteOrder::Little);

    let mut left_margin = 0_i64;
    let mut top_margin = 0_i64;
    for decoded in decode.fields() {
        match decoded.field.name {
            "LeftMargin" => {
                if let Some(access) = RawAccess::new(decoded, Acknowledged::RAW_CONV, &LEFT_MARGIN)
                    && let Some(raw) = access.raw().as_integer()
                {
                    left_margin = raw;
                }
            }
            "TopMargin" => {
                if let Some(access) = RawAccess::new(decoded, Acknowledged::RAW_CONV, &TOP_MARGIN)
                    && let Some(raw) = access.raw().as_integer()
                {
                    top_margin = raw;
                }
            }
            _ => {}
        }
    }

    let mut metadata = MetadataMap::new();
    for decoded in decode.fields() {
        let name = decoded.field.name;
        let key = format!("File:{name}");
        match name {
            "LeftMargin" => {
                if let Some(access) = RawAccess::new(decoded, Acknowledged::RAW_CONV, &LEFT_MARGIN)
                {
                    metadata.insert(key, access.emit_raw());
                }
            }
            "TopMargin" => {
                if let Some(access) = RawAccess::new(decoded, Acknowledged::RAW_CONV, &TOP_MARGIN) {
                    metadata.insert(key, access.emit_raw());
                }
            }
            "ImageWidth" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::VALUE_CONV, &IMAGE_WIDTH)
                    && let Some(raw) = access.raw().as_integer()
                {
                    metadata.insert(key, TagValue::new_integer(raw - left_margin + 1));
                }
            }
            "ImageHeight" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::VALUE_CONV, &IMAGE_HEIGHT)
                    && let Some(raw) = access.raw().as_integer()
                {
                    metadata.insert(key, TagValue::new_integer(raw - top_margin + 1));
                }
            }
            "ScreenWidth" => {
                if let Some(access) = RawAccess::new(decoded, Acknowledged::RAW_CONV, &SCREEN_WIDTH)
                    && let DecodedValue::Integer(raw) = access.raw()
                    && *raw != 0
                {
                    metadata.insert(key, TagValue::new_integer(*raw));
                }
            }
            "ScreenHeight" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::RAW_CONV, &SCREEN_HEIGHT)
                    && let DecodedValue::Integer(raw) = access.raw()
                    && *raw != 0
                {
                    metadata.insert(key, TagValue::new_integer(*raw));
                }
            }
            _ => {
                if let Some(value) = decoded.emit() {
                    metadata.insert(key, value);
                }
            }
        }
    }
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_accepts_pinned_fixture_header() {
        let mut header = vec![0_u8; HEADER_LEN];
        header[0] = 0x0a;
        header[1] = 0x05;
        header[2] = 0x01;
        header[3] = 0x08;
        header[HEADER_LEN - 1] = 0x00;
        assert!(header_matches_signature(&header));
    }

    #[test]
    fn signature_rejects_bad_manufacturer_byte() {
        let mut header = vec![0_u8; HEADER_LEN];
        header[0] = 0x0b;
        assert!(!header_matches_signature(&header));
    }
}
