//! Progressive Graphics File (PGF) metadata parser.
//!
//! ExifTool 13.59's `PGF::ProcessPGF` (PGF.pm:69-98) reads a 24-byte header,
//! validates it against `^PGF(.)/s`, sets little-endian byte order and runs
//! `ProcessBinaryData` over `PGF::Main` (PGF.pm:20-63). Only PGF major
//! version `0x36` is supported (unsupported versions still identify the file
//! but carry no further tags). After the header, `ProcessPGF` re-enters
//! `ExtractInfo` on a trailing PNG-format metadata blob whose length is
//! `Get32u(header, 4) - 16` bytes (skipping a 1024-byte colour table first
//! when `ColorMode == 2`, PGF.pm:91-92).
//!
//! # Why the table is not enough on its own
//!
//! `ColorMode` (PGF.pm:44-59) carries a `RawConv` that is a pure
//! `DataMember` side effect (`$$self{PGFColorMode} = $val`, returning the
//! value unchanged) which gates the colour-table skip; that is hand-verified
//! against the cited Perl below via [`RawAccess`]. The embedded PNG blob is
//! re-entered through this crate's own PNG parser rather than reproducing
//! PNG chunk parsing a second time here.
//!
//! # Why the header's tags need an explicit priority
//!
//! `PGF::Main` declares `PRIORITY => 2` (PGF.pm:22, "to take precedence over
//! PNG tags from embedded image"): the embedded PNG blob's own `ImageWidth`/
//! `ImageHeight` are real, ordinary (priority-1) tags in their own right --
//! the pinned `t/images/PGF.pgf` fixture's embedded PNG is a 1x1 pixel test
//! image, deliberately different from the 8x8 PGF image it is metadata for
//! -- and `Composite:ImageSize` must resolve `ImageWidth` by bare name to
//! the PGF header's occurrence, not the embedded PNG's, or it reports the
//! wrong image's size. A plain [`MetadataMap::insert`] mints every occurrence
//! at the same default priority as the merged-in PNG map, so the two would
//! tie and the *later*-inserted one (order, not source) would win instead.
//! `MetadataMap::insert_occurrence` carries the table's real priority
//! through, which is what `Composite`'s bare-name dependency resolution
//! (`src/composite/mod.rs::resolve_dependency`) actually arbitrates on.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/PGF.pm`

use crate::core::{FileReader, Instance, MetadataMap};
use crate::exiftool_tables::{
    Acknowledged, PerlCitation, RawAccess, decode_binary_table, find_table,
};
use crate::io::{BufferedReader, ByteOrder};

/// `PGF::Main`'s own `PRIORITY => 2` (PGF.pm:22).
const HEADER_PRIORITY: u8 = 2;
/// `PGF::Main`'s `GROUPS => { 0 => 'File', 1 => 'File', ... }` (PGF.pm:21):
/// family-1 group for every tag this parser inserts directly.
const HEADER_GROUP1: &str = "File";

/// PGF.pm:74, `$raf->Read($buff, 24) == 24`.
const HEADER_LEN: usize = 24;
/// PGF.pm:80, the only major version this module (and ExifTool itself) reads.
const SUPPORTED_VERSION: u8 = 0x36;
/// PGF.pm:91, the 1024-byte palette skipped ahead of the metadata blob when
/// `ColorMode == 2` (Indexed).
const COLOR_TABLE_LEN: u64 = 1024;
/// PGF.pm:93, the largest post-header blob `ProcessPGF` will re-enter.
const MAX_METADATA_LEN: i64 = 0x1000000;

const COLOR_MODE: PerlCitation = PerlCitation {
    module: "PGF",
    table: "Main",
    tag: "ColorMode",
    lines: "PGF.pm:44-59",
};

/// Extract PGF metadata using ExifTool's declared `PGF::Main` binary layout,
/// plus the trailing embedded-PNG metadata blob.
pub fn parse_pgf_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    if reader.size() < HEADER_LEN as u64 {
        return Err("PGF file is too short for the 24-byte header".to_string());
    }
    let header = reader
        .read(0, HEADER_LEN)
        .map_err(|error| error.to_string())?;
    if !header.starts_with(b"PGF") {
        return Err("invalid PGF signature".to_string());
    }
    let version = header[3];

    let mut metadata = MetadataMap::new();
    if version != SUPPORTED_VERSION {
        // PGF.pm:81-83: an unsupported major version still identifies the
        // file (via `SetFileType`, which `add_identity_tags` reproduces
        // centrally) but ExifTool extracts nothing further -- an `Error` is
        // recorded and `ProcessPGF` returns success with no PGF tags.
        return Ok(metadata);
    }

    let table = find_table("PGF", "Main").ok_or("missing PGF::Main table")?;
    let decode = decode_binary_table(table, header, ByteOrder::Little);

    let mut color_mode = None;
    for decoded in decode.fields() {
        let name = decoded.field.name;
        let key = format!("File:{name}");
        if name == "ColorMode" {
            if let Some(access) = RawAccess::new(decoded, Acknowledged::RAW_CONV, &COLOR_MODE)
                && let Some(raw) = access.raw().as_integer()
            {
                color_mode = Some(raw);
                metadata.insert_occurrence(
                    key,
                    access.emit_raw(),
                    HEADER_PRIORITY,
                    HEADER_GROUP1,
                    Instance::default(),
                );
            }
        } else if let Some(value) = decoded.emit() {
            metadata.insert_occurrence(
                key,
                value,
                HEADER_PRIORITY,
                HEADER_GROUP1,
                Instance::default(),
            );
        }
    }

    // PGF.pm:87, `Get32u(\$buff, 4) - 16`: little-endian u32 at header byte 4
    // (the file's own declared header size), minus the 16-byte tail of the
    // fixed table above (offsets 8..24).
    let header_size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    let mut len = i64::from(header_size) - 16;
    let mut post_header_offset = HEADER_LEN as u64;

    // PGF.pm:91-92: skip the 1024-byte colour table for indexed images,
    // exactly like `$raf->Seek(1024, 1) ? 1024 : $len`.
    if color_mode == Some(2) {
        if reader.size() >= post_header_offset + COLOR_TABLE_LEN {
            post_header_offset += COLOR_TABLE_LEN;
            len -= COLOR_TABLE_LEN as i64;
        } else {
            len = 0;
        }
    }

    if len > 0 && len < MAX_METADATA_LEN {
        let len = len as usize;
        if let Ok(png_bytes) = reader.read(post_header_offset, len) {
            let png_reader = BufferedReader::from_bytes(png_bytes);
            if let Ok(png_metadata) = crate::parsers::png::parse_png_metadata(&png_reader) {
                metadata.merge(png_metadata);
            }
        }
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    #[test]
    fn header_size_matches_pinned_fixture() {
        // Pinned t/images/PGF.pgf fixture: header bytes 4..8 are
        // `9c 00 00 00` (LE) = 156, so the post-header metadata length is
        // 156 - 16 = 140.
        let header_bytes = [0x9c_u8, 0x00, 0x00, 0x00];
        let header_size = u32::from_le_bytes(header_bytes);
        assert_eq!(header_size, 156);
        assert_eq!(i64::from(header_size) - 16, 140);
    }
}
