//! iTunes Cover Flow (ITC) metadata parser.
//!
//! ExifTool 13.59's `ITC::ProcessITC` (ITC.pm:83-166) walks a flat sequence
//! of `(u32 size, 4-byte tag)` blocks. The first block must be `itch`
//! (header, ITC.pm:112-124, run through `ITC::Header`); any `item` block
//! (ITC.pm:125-160) carries a variable-length, mostly-unknown prefix
//! ExifTool itself calls "just a guess about how to parse this
//! variable-length part" (ITC.pm:140), followed by a fixed item-info record
//! (`ITC::Item`) once a `0xb4`-byte-minimum block is found ending in the
//! literal `data` marker at relative offset `0xb0`, and finally the raw
//! embedded image bytes. This parser reproduces that walk exactly, including
//! its validation gates, and reads the two binary-table layouts from the
//! generated tables rather than restating offsets here.
//!
//! # Why the tables are not enough on their own
//!
//! `ITC::Item`'s `LibraryID`/`TrackID` (ITC.pm:53-60) carry a `ValueConv` of
//! `'uc unpack "H*", $val'` (uppercase hex), and `ImageType` (ITC.pm:66-69)
//! carries a `ValueConv` hash comparing the raw 4 bytes against two exact
//! byte-string keys. All three are hand-implemented below against the cited
//! Perl, each behind a [`RawAccess`] citation.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/ITC.pm`

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::exiftool_tables::{
    Acknowledged, DecodedValue, PerlCitation, RawAccess, decode_binary_table, find_table,
};
use crate::io::ByteOrder;

/// ITC.pm:117, `last unless $size >= 0x1c and $size < 0x10000` (the `itch`
/// block's own size gate).
const ITCH_MIN_SIZE: u32 = 0x1c;
const ITCH_MAX_SIZE: u32 = 0x10000;
/// ITC.pm:100, `last unless $size >= 8 and $size < 0x80000000` (every block
/// after the first).
const BLOCK_MAX_SIZE: u32 = 0x8000_0000;
/// ITC.pm:132-133: the item header length field must fall in `[0xd0, size]`.
const ITEM_HEADER_MIN_LEN: u32 = 0xd0;
/// ITC.pm:147: the item-info record must be at least this long, with the
/// literal `data` marker at relative offset `0xb0`.
const ITEM_INFO_MIN_LEN: usize = 0xb4;
const ITEM_INFO_MARKER_OFFSET: usize = 0xb0;

const fn item_citation(tag: &'static str, lines: &'static str) -> PerlCitation {
    PerlCitation {
        module: "ITC",
        table: "Item",
        tag,
        lines,
    }
}

const LIBRARY_ID: PerlCitation = item_citation("LibraryID", "ITC.pm:53-56");
const TRACK_ID: PerlCitation = item_citation("TrackID", "ITC.pm:57-60");
const IMAGE_TYPE: PerlCitation = item_citation("ImageType", "ITC.pm:66-69");
/// `DataType` (ITC.pm:37-40) and `DataLocation` (ITC.pm:61-65) both carry a
/// hash `PrintConv` keyed on the raw `undef[4]` bytes. The generated engine's
/// enum matching (`runtime::DecodedValue::enum_key`) only compares `Integer`
/// and `String` raw values against a `PrintConv::StrEnum` table, so an
/// `undef`-formatted field's `StrEnum` never matches and `.emit()` falls back
/// to the raw bytes -- not a wrong value (the raw fallback is the engine's
/// own documented "no conversion is honest" behavior), but not what ExifTool
/// reports either. Neither field is `omitted` by the generator (the
/// `StrEnum` itself transcribed correctly), so [`RawAccess::new`] needs no
/// acknowledgment beyond citing the Perl reproduced by hand here.
const DATA_TYPE: PerlCitation = PerlCitation {
    module: "ITC",
    table: "Header",
    tag: "DataType",
    lines: "ITC.pm:37-40",
};
const DATA_LOCATION: PerlCitation = item_citation("DataLocation", "ITC.pm:61-65");

/// Extract ITC metadata by walking ExifTool's declared block structure and
/// the `ITC::Header`/`ITC::Item` binary layouts.
pub fn parse_itc_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let mut metadata = MetadataMap::new();
    let size = reader.size();
    let mut pos: u64 = 0;
    let mut seen_itch = false;

    loop {
        if pos + 8 > size {
            break;
        }
        let block_header = reader.read(pos, 8).map_err(|e| e.to_string())?;
        let block_size = u32::from_be_bytes([
            block_header[0],
            block_header[1],
            block_header[2],
            block_header[3],
        ]);
        let tag = &block_header[4..8];
        pos += 8;

        if !seen_itch {
            if tag != b"itch" || !(ITCH_MIN_SIZE..ITCH_MAX_SIZE).contains(&block_size) {
                return Err("not a valid ITC file (first block is not itch)".to_string());
            }
            seen_itch = true;
        } else if block_size < 8 || block_size >= BLOCK_MAX_SIZE {
            break;
        }

        match tag {
            b"itch" => {
                let data_len = (block_size - 8) as u64;
                if pos + data_len > size {
                    break;
                }
                let data = reader
                    .read(pos, data_len as usize)
                    .map_err(|e| e.to_string())?;
                parse_itc_header(data, &mut metadata);
                pos += data_len;
            }
            b"item" => {
                if block_size <= 12 {
                    break;
                }
                if pos + 4 > size {
                    break;
                }
                let len_bytes = reader.read(pos, 4).map_err(|e| e.to_string())?;
                let mut item_len =
                    u32::from_be_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]);
                pos += 4;
                if item_len < ITEM_HEADER_MIN_LEN || item_len > block_size {
                    break;
                }
                // ITC.pm:136-137: `$size -= $len; $len -= 12;` -- `$size` is
                // the trailing image-data length, `$len` the remaining item
                // header length after the 12 bytes already consumed (8-byte
                // block header + the 4-byte length field just read).
                let mut trailing_size = (block_size - item_len) as u64;
                item_len -= 12;

                // ITC.pm:139-143: consume 4-byte words until a run of four
                // NUL bytes, ExifTool's own "just a guess" heuristic.
                let mut found_terminator = false;
                while item_len >= 4 {
                    if pos + 4 > size {
                        return Ok(metadata);
                    }
                    let word = reader.read(pos, 4).map_err(|e| e.to_string())?;
                    pos += 4;
                    item_len -= 4;
                    if word == [0, 0, 0, 0] {
                        found_terminator = true;
                        break;
                    }
                }
                if !found_terminator || item_len < 4 {
                    break;
                }

                if pos + u64::from(item_len) > size {
                    break;
                }
                let item_info = reader
                    .read(pos, item_len as usize)
                    .map_err(|e| e.to_string())?;
                let item_info_pos = pos;

                if item_info.len() < ITEM_INFO_MIN_LEN
                    || &item_info[ITEM_INFO_MARKER_OFFSET..ITEM_INFO_MARKER_OFFSET + 4] != b"data"
                {
                    // ITC.pm:150-153: a parsing error here aborts the whole
                    // walk, not just this block.
                    break;
                }
                parse_itc_item(item_info, &mut metadata);

                // ITC.pm:161-167: the embedded image follows immediately.
                if trailing_size > 0 {
                    let image_pos = item_info_pos + u64::from(item_len);
                    trailing_size = trailing_size.min(size.saturating_sub(image_pos));
                    if trailing_size > 0
                        && let Ok(image) = reader.read(image_pos, trailing_size as usize)
                    {
                        metadata.insert("ITC:ImageData", TagValue::Binary(image.to_vec()));
                    }
                    pos = image_pos + trailing_size;
                } else {
                    pos = item_info_pos + u64::from(item_len);
                }
            }
            _ => {
                // ITC.pm:163-165: skip unknown blocks entirely.
                let remaining = (block_size as u64).saturating_sub(8);
                pos += remaining;
            }
        }
    }

    if !seen_itch {
        return Err("not a valid ITC file".to_string());
    }
    Ok(metadata)
}

/// `ITC::Header` (ITC.pm:33-40): a single `DataType` field at offset 0x10.
fn parse_itc_header(data: &[u8], metadata: &mut MetadataMap) {
    let Some(table) = find_table("ITC", "Header") else {
        return;
    };
    let decode = decode_binary_table(table, data, ByteOrder::Big);
    for decoded in decode.fields() {
        let key = format!("ITC:{}", decoded.field.name);
        if decoded.field.name == "DataType" {
            if let Some(access) = RawAccess::new(decoded, Acknowledged::NONE, &DATA_TYPE)
                && let DecodedValue::Undefined(bytes) = access.raw()
                && bytes == b"artw"
            {
                metadata.insert(key, TagValue::new_string("Artwork"));
            }
        } else if let Some(value) = decoded.emit() {
            metadata.insert(key, value);
        }
    }
}

/// `ITC::Item` (ITC.pm:43-76).
fn parse_itc_item(data: &[u8], metadata: &mut MetadataMap) {
    let Some(table) = find_table("ITC", "Item") else {
        return;
    };
    let decode = decode_binary_table(table, data, ByteOrder::Big);
    for decoded in decode.fields() {
        let name = decoded.field.name;
        let key = format!("ITC:{name}");
        match name {
            "LibraryID" => {
                if let Some(access) = RawAccess::new(decoded, Acknowledged::VALUE_CONV, &LIBRARY_ID)
                    && let DecodedValue::Undefined(bytes) = access.raw()
                {
                    metadata.insert(key, TagValue::new_string(hex_uppercase(bytes)));
                }
            }
            "TrackID" => {
                if let Some(access) = RawAccess::new(decoded, Acknowledged::VALUE_CONV, &TRACK_ID)
                    && let DecodedValue::Undefined(bytes) = access.raw()
                {
                    metadata.insert(key, TagValue::new_string(hex_uppercase(bytes)));
                }
            }
            "ImageType" => {
                if let Some(access) = RawAccess::new(decoded, Acknowledged::VALUE_CONV, &IMAGE_TYPE)
                    && let DecodedValue::Undefined(bytes) = access.raw()
                    && let Some(name) = image_type_name(bytes)
                {
                    metadata.insert(key, TagValue::new_string(name));
                }
            }
            "DataLocation" => {
                if let Some(access) = RawAccess::new(decoded, Acknowledged::NONE, &DATA_LOCATION)
                    && let DecodedValue::Undefined(bytes) = access.raw()
                    && let Some(name) = data_location_name(bytes)
                {
                    metadata.insert(key, TagValue::new_string(name));
                }
            }
            _ => {
                if let Some(value) = decoded.emit() {
                    metadata.insert(key, value);
                }
            }
        }
    }
}

/// ITC.pm:55/59: `uc unpack "H*", $val`.
fn hex_uppercase(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// ITC.pm:67-68: `{ 'PNGf' => 'PNG', "\0\0\0\x0d" => 'JPEG' }`.
fn image_type_name(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        b"PNGf" => Some("PNG"),
        b"\0\0\0\x0d" => Some("JPEG"),
        _ => None,
    }
}

/// ITC.pm:61-65: `{ down => 'Downloaded Separately', locl => 'Local Music File' }`.
fn data_location_name(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        b"down" => Some("Downloaded Separately"),
        b"locl" => Some("Local Music File"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_uppercase_matches_exiftool_sample() {
        let bytes = [0x91, 0x4A, 0x6D, 0xE0, 0x1A, 0x27, 0x96, 0x11];
        assert_eq!(hex_uppercase(&bytes), "914A6DE01A279611");
    }

    #[test]
    fn image_type_recognizes_png_and_jpeg() {
        assert_eq!(image_type_name(b"PNGf"), Some("PNG"));
        assert_eq!(image_type_name(b"\0\0\0\x0d"), Some("JPEG"));
        assert_eq!(image_type_name(b"xxxx"), None);
    }

    #[test]
    fn data_location_matches_exiftool_sample() {
        assert_eq!(data_location_name(b"locl"), Some("Local Music File"));
        assert_eq!(data_location_name(b"down"), Some("Downloaded Separately"));
        assert_eq!(data_location_name(b"xxxx"), None);
    }
}
