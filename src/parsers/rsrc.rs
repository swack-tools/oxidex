//! Mac OS resource fork / DFONT parser.
//!
//! ExifTool reads `.dfont` and `.rsrc` files through `RSRC::ProcessRSRC`
//! (RSRC.pm:66-207): validate the resource-fork header, walk the resource
//! map's type list, and hand each resource it knows to the matching decoder.
//! A `Font.dfont` is exactly this -- a data-fork resource file whose `sfnt`
//! resource wraps an ordinary OTF/TTF block, which RSRC.pm:38-41 routes into
//! `Font::Name` (the same name-table walk TTF files get), and RSRC.pm:152-162
//! then overrides the file type to DFONT.
//!
//! # Implemented resource types
//!
//! * `sfnt` -- the embedded font block, at the resource's data offset + 4
//!   (RSRC.pm:152-162, `$$dirInfo{Base} = $resOff + 4` then
//!   `Font::ProcessOTF`). Only its `name` table is read, matching
//!   `%processTag` (Font.pm:30, `name => 1`; the `C2PA` entry is a gap this
//!   parser leaves open).
//! * `vers` id 1 -- `ApplicationVersion` (RSRC.pm:49, 142-151): the long
//!   version string that follows the short one, decoded as MacRoman.
//! * `POST` id 0x1f5 -- only the DFONT file-type override (RSRC.pm:196-198;
//!   other POST ids never reach it, `next unless $tagInfo` at RSRC.pm:141).
//!   The PostScript sub-document a `POST` id 0x1f5 resource carries
//!   (RSRC.pm:44-47) is NOT parsed: no corpus sample exercises it, and a
//!   guessed PostScript walk would risk plausible-but-wrong values. Its tags
//!   stay missing and counted.
//!
//! # Omitted resource types, deliberately
//!
//! Each of these is decodable in principle but has no corpus sample, so it
//! is left as a counted gap rather than shipped untested:
//!
//! * `8BIM` -- Photoshop image resources (RSRC.pm:34-37, 163-172).
//! * `usro` id 0 -- `OpenWithApplication` (RSRC.pm:48, 178-181).
//! * `STR ` ids 0xbff3/0xbff4 -- `ApplicationMissingMsg` /
//!   `CreatorApplication` (RSRC.pm:50-51, 173-177).
//! * `STR#` id 0x80 -- `Keywords` string list (RSRC.pm:54, 182-195).
//! * `TEXT` id 0x80 -- `Description` (RSRC.pm:55, 200-201).
//!
//! # File type
//!
//! `ProcessRSRC` sets `RSRC` up front (RSRC.pm:95) and overrides to `DFONT`
//! when a `sfnt` or `POST` resource shows the file is a data-fork font
//! (RSRC.pm:161, 198). This parser reports the same under the bare
//! `FileType` key; the `File:` group value stays owned by the identity
//! tables (`.dfont` resolves to DFONT there already).

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::io::EndianReader;
use crate::parsers::font::ttf::TTFParser;

/// RSRC.pm:126's cap on a single resource's data: "arbitrary size limit
/// (100MB)".
const MAX_RESOURCE_LEN: u32 = 100_000_000;

/// The validated resource-fork geometry, straight from RSRC.pm:76-93.
pub(crate) struct RsrcLayout {
    dat_off: u32,
    map_off: u32,
    map_len: u32,
    type_off: u16,
    num_types: u16,
}

/// `ProcessRSRC`'s up-front validation (RSRC.pm:76-93), returning the
/// resource map geometry when every check passes.
///
/// This is also the detection gate: the `%magicNumber` pattern for RSRC --
/// `(....)?\0\0\x01\0` (ExifTool.pm:1030) -- is four bytes that every ICO
/// file also starts with, so the byte-level magic alone cannot route a file
/// here. ExifTool resolves that by trying modules until one accepts, and
/// `ProcessRSRC` accepts only after these structural checks; running the
/// same checks at detection time reproduces that arbitration.
pub(crate) fn validate_rsrc(reader: &dyn FileReader) -> Option<RsrcLayout> {
    // RSRC.pm:76: `return 0 unless $raf->Read($hdr, 30) == 30;`
    if reader.size() < 30 {
        return None;
    }
    let hdr = reader.read(0, 16).ok()?;
    let r = EndianReader::big_endian(hdr);
    let dat_off = r.u32_at(0)?;
    let map_off = r.u32_at(4)?;
    let dat_len = r.u32_at(8)?;
    let map_len = r.u32_at(12)?;
    let f_len = reader.size();

    // RSRC.pm:80-83, in the same order.
    if dat_off < 0x10 || u64::from(dat_off) + u64::from(dat_len) > f_len {
        return None;
    }
    if map_off < 0x10 || u64::from(map_off) + u64::from(map_len) > f_len || map_len < 30 {
        return None;
    }
    if dat_off < map_off && u64::from(dat_off) + u64::from(dat_len) > u64::from(map_off) {
        return None;
    }
    if map_off < dat_off && u64::from(map_off) + u64::from(map_len) > u64::from(dat_off) {
        return None;
    }

    // RSRC.pm:86-93: read the map, pull the three header fields, validate.
    let map_head = reader.read(u64::from(map_off), 30).ok()?;
    let mr = EndianReader::big_endian(map_head);
    let type_off = mr.u16_at(24)?;
    // The resource name list offset is validated (RSRC.pm:93) but not kept:
    // names are only read on ExifTool's verbose path (RSRC.pm:133-140).
    let name_off = mr.u16_at(26)?;
    let num_types = (mr.u16_at(28)?).wrapping_add(1);
    if type_off < 28 || name_off < 30 {
        return None;
    }

    Some(RsrcLayout {
        dat_off,
        map_off,
        map_len,
        type_off,
        num_types,
    })
}

/// An owned in-memory [`FileReader`] over one resource's data, so the sfnt
/// block inside a DFONT can reuse the TTF name-table machinery unchanged:
/// `ProcessOTF` is called with `Base => $resOff + 4` (RSRC.pm:154-155),
/// i.e. every table offset is relative to the start of the resource data,
/// which is offset 0 of this reader.
struct MemReader {
    data: Vec<u8>,
}

impl FileReader for MemReader {
    fn read(&self, offset: u64, length: usize) -> std::io::Result<&[u8]> {
        let start = usize::try_from(offset).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "offset out of range")
        })?;
        if start > self.data.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "offset beyond end of resource",
            ));
        }
        let end = start.saturating_add(length).min(self.data.len());
        Ok(&self.data[start..end])
    }

    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

/// Extract metadata from a Mac OS resource file (RSRC or DFONT).
pub fn parse_rsrc_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let layout = validate_rsrc(reader).ok_or("not a valid Mac OS resource file")?;

    let mut metadata = MetadataMap::new();
    // RSRC.pm:95, `$et->SetFileType('RSRC')`; overridden below on sfnt/POST.
    let mut file_type = "RSRC";

    let map = reader
        .read(u64::from(layout.map_off), layout.map_len as usize)
        .map_err(|e| e.to_string())?;
    let mr = EndianReader::big_endian(map);

    // RSRC.pm:101-204: the type-list / reference-list walk.
    for i in 0..u32::from(layout.num_types) {
        // `my $off = $typeOff + 2 + 8 * $i` (RSRC.pm:102).
        let off = u32::from(layout.type_off) + 2 + 8 * i;
        // `last if $off + 8 > $mapLen` (RSRC.pm:103).
        if off + 8 > layout.map_len {
            break;
        }
        let off = off as usize;
        let res_type: [u8; 4] = match map.get(off..off + 4) {
            Some(b) => [b[0], b[1], b[2], b[3]],
            None => break,
        };
        let res_num = mr.u16_at(off + 4).unwrap_or(0);
        let ref_off = u32::from(mr.u16_at(off + 6).unwrap_or(0)) + u32::from(layout.type_off);

        for j in 0..=u32::from(res_num) {
            // `my $roff = $refOff + 12 * $j` (RSRC.pm:109).
            let roff = ref_off + 12 * j;
            if roff + 12 > layout.map_len {
                break;
            }
            let roff = roff as usize;
            let id = mr.u16_at(roff).unwrap_or(0);
            // 24-bit resource data offset (RSRC.pm:113).
            let res_off = u64::from(mr.u32_at(roff + 4).unwrap_or(0) & 0x00ff_ffff)
                + u64::from(layout.dat_off);

            match &res_type {
                b"sfnt" => {
                    // The resource data must be readable at all: a failed
                    // read (or one past the 100MB cap) warns and hits `next`
                    // (RSRC.pm:124-131) before the sfnt branch, so neither
                    // tags nor the file-type override happen. Once read,
                    // `OverrideFileType('DFONT')` fires even when
                    // `ProcessOTF` rejects the block -- ExifTool only warns
                    // "Unrecognized sfnt resource format" (RSRC.pm:152-161).
                    if let Some(data) = read_resource(reader, res_off) {
                        if let Some(tags) = extract_sfnt_name_tags(data) {
                            for (key, value) in tags {
                                metadata.insert(key, value);
                            }
                        }
                        file_type = "DFONT";
                    }
                }
                b"vers" if id == 1 => {
                    // RSRC.pm:49 names it; RSRC.pm:142-151 decodes it.
                    if let Some(version) = read_resource(reader, res_off)
                        .and_then(|data| decode_vers_long_string(&data))
                    {
                        metadata.insert("RSRC:ApplicationVersion", TagValue::String(version));
                    }
                }
                b"POST" if id == 0x01f5 => {
                    // The Main table keys only `POST_0x01f5` (RSRC.pm:44-47),
                    // and `next unless $tagInfo` (RSRC.pm:141) skips every
                    // other POST id before the override at RSRC.pm:196-198
                    // can run; the data read must succeed too
                    // (RSRC.pm:124-131). The PostScript sub-document itself
                    // is deliberately not parsed -- see the module doc.
                    if read_resource(reader, res_off).is_some() {
                        file_type = "DFONT";
                    }
                }
                _ => {
                    // 8BIM / usro / STR / STR# / TEXT: counted gaps, see the
                    // module doc for the RSRC.pm citations.
                }
            }
        }
    }

    metadata.insert("FileType", TagValue::new_string(file_type));
    Ok(metadata)
}

/// Read one resource's data: a big-endian u32 length at `res_off`, then that
/// many bytes (RSRC.pm:125-131), capped at [`MAX_RESOURCE_LEN`].
fn read_resource(reader: &dyn FileReader, res_off: u64) -> Option<Vec<u8>> {
    let len_bytes = reader.read(res_off, 4).ok()?;
    let len = EndianReader::big_endian(len_bytes).u32_at(0)?;
    if len >= MAX_RESOURCE_LEN {
        return None;
    }
    let data = reader.read(res_off + 4, len as usize).ok()?;
    if data.len() != len as usize {
        return None;
    }
    Some(data.to_vec())
}

/// The `vers` resource's long version string (RSRC.pm:142-151):
/// skip the 6-byte numeric header, the Pascal short-version string at
/// offset 6, then read the Pascal long-version string and decode MacRoman.
fn decode_vers_long_string(val: &[u8]) -> Option<String> {
    // `next unless $valLen > 8` (RSRC.pm:144).
    if val.len() <= 8 {
        return None;
    }
    // `my $p = 7 + Get8u(\$val, 6)` (RSRC.pm:146).
    let mut p = 7 + usize::from(val[6]);
    // `next if $p >= $valLen` (RSRC.pm:147).
    if p >= val.len() {
        return None;
    }
    let vlen = usize::from(val[p]);
    p += 1;
    // `next if $p + $vlen > $valLen` (RSRC.pm:149).
    if p + vlen > val.len() {
        return None;
    }
    Some(TTFParser::decode_mac_roman(&val[p..p + vlen]))
}

/// Walk an embedded sfnt block's table directory and return its `name`
/// table's tags, using the same ExifTool-faithful walk TTF files get.
///
/// The block must open with one of `ProcessOTF`'s accepted signatures
/// (Font.pm:551-552): `\0\x01\0\0`, `OTTO`, `true`, `typ1`, `\xa5kbd` or
/// `\xa5lst`, followed by `\0` or `\x01`; and declare between 1 and 0x1ff
/// tables (Font.pm:557).
fn extract_sfnt_name_tags(data: Vec<u8>) -> Option<MetadataMap> {
    if data.len() < 12 {
        return None;
    }
    let sig_ok = matches!(
        &data[0..4],
        b"\x00\x01\x00\x00" | b"OTTO" | b"true" | b"typ1" | b"\xa5kbd" | b"\xa5lst"
    ) && matches!(data[4], 0x00 | 0x01);
    if !sig_ok {
        return None;
    }
    let num_tables = u16::from_be_bytes([data[4], data[5]]);
    if num_tables == 0 || num_tables >= 0x200 {
        return None;
    }

    let mem = MemReader { data };
    let tables = TTFParser::parse_table_directory(&mem, num_tables).ok()?;
    let name_table = TTFParser::find_table(&tables, b"name")?;
    TTFParser::extract_exiftool_name_tags(&mem, name_table).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// Build a minimal single-resource resource file. The geometry follows
    /// RSRC.pm's reader: data at `datOff`, map at `mapOff` with the type
    /// list at `typeOff` and one reference per resource.
    fn build_rsrc(res_type: &[u8; 4], id: u16, payload: &[u8]) -> Vec<u8> {
        build_rsrc_multi(&[(*res_type, id, payload.to_vec())])
    }

    fn build_rsrc_multi(resources: &[([u8; 4], u16, Vec<u8>)]) -> Vec<u8> {
        // Data area: each resource is a u32 length + payload.
        let mut data_area = Vec::new();
        let mut data_offsets = Vec::new();
        for (_, _, payload) in resources {
            data_offsets.push(data_area.len() as u32);
            data_area.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            data_area.extend_from_slice(payload);
        }

        let dat_off: u32 = 0x100;
        let dat_len = data_area.len() as u32;
        let map_off = dat_off + dat_len;

        // Map: 28-byte header (we only fill offsets 24..30), then the type
        // list, then the reference lists.
        // typeOff = 28 (map-relative), so the type list begins right after
        // the map header. numTypes-1 is stored at map[28] -- which is byte 0
        // of the type list area at typeOff==28, exactly as in real files.
        let num_types = resources.len() as u16;
        let type_off: u16 = 28;
        // Type list: 2 bytes count, then 8 bytes per type.
        // Reference lists follow, 12 bytes per resource.
        let ref_list_base = 2 + 8 * (num_types as usize);
        let mut type_list = Vec::new();
        type_list.extend_from_slice(&(num_types.wrapping_sub(1)).to_be_bytes());
        for (i, (res_type, _, _)) in resources.iter().enumerate() {
            type_list.extend_from_slice(res_type);
            type_list.extend_from_slice(&0u16.to_be_bytes()); // resNum = count-1 = 0
            let ref_off = (ref_list_base + 12 * i) as u16;
            type_list.extend_from_slice(&ref_off.to_be_bytes());
        }
        for (i, (_, id, _)) in resources.iter().enumerate() {
            type_list.extend_from_slice(&id.to_be_bytes());
            type_list.extend_from_slice(&0xffffu16.to_be_bytes()); // no name
            type_list.extend_from_slice(&data_offsets[i].to_be_bytes()); // attrs byte + 24-bit offset
            type_list.extend_from_slice(&0u32.to_be_bytes()); // handle placeholder
        }

        let name_off = (28 + type_list.len()) as u16; // empty name list
        let map_len = (28 + type_list.len() + 2) as u32;

        let mut file = Vec::new();
        file.extend_from_slice(&dat_off.to_be_bytes());
        file.extend_from_slice(&map_off.to_be_bytes());
        file.extend_from_slice(&dat_len.to_be_bytes());
        file.extend_from_slice(&map_len.to_be_bytes());
        file.resize(dat_off as usize, 0);
        file.extend_from_slice(&data_area);
        assert_eq!(file.len() as u32, map_off);
        // Map header.
        let mut map = vec![0u8; 24];
        map.extend_from_slice(&type_off.to_be_bytes());
        map.extend_from_slice(&name_off.to_be_bytes());
        map.extend_from_slice(&type_list);
        map.extend_from_slice(&[0, 0]); // empty name list
        assert_eq!(map.len() as u32, map_len);
        file.extend_from_slice(&map);
        file
    }

    /// A minimal sfnt block whose name table has one Macintosh/English
    /// FontFamily record.
    fn minimal_sfnt(family: &[u8]) -> Vec<u8> {
        let mut sfnt = vec![
            0x00, 0x01, 0x00, 0x00, // sfnt version
            0x00, 0x01, // numTables = 1
            0x00, 0x00, // searchRange
            0x00, 0x00, // entrySelector
            0x00, 0x00, // rangeShift
            b'n', b'a', b'm', b'e', // tag
            0x00, 0x00, 0x00, 0x00, // checksum
            0x00, 0x00, 0x00, 0x1c, // offset = 28
            0x00, 0x00, 0x00, 0x00, // length (unused)
            0x00, 0x00, // format
            0x00, 0x01, // count = 1
            0x00, 0x12, // stringOffset = 18
            0x00, 0x01, // platformID = Macintosh
            0x00, 0x00, // encodingID = Roman
            0x00, 0x00, // languageID = 0 (en)
            0x00, 0x01, // nameID = 1 (FontFamily)
            0x00, 0x00, // length (patched below)
            0x00, 0x00, // offset = 0
        ];
        let len_pos = sfnt.len() - 4;
        sfnt[len_pos..len_pos + 2].copy_from_slice(&(family.len() as u16).to_be_bytes());
        sfnt.extend_from_slice(family);
        sfnt
    }

    /// Pins the `vers` decode against RSRC.pm:142-151 using the layout the
    /// corpus `Font.dfont` carries: `exiftool -G1 -s Font.dfont` (13.59)
    /// prints `[RSRC] ApplicationVersion : ExifTool 8.0.7 DFONT Test`.
    /// The resource is a 'vers' with a short version string followed by the
    /// long one; only the long one is reported.
    #[test]
    fn vers_resource_long_string_is_application_version() {
        // 6 numeric bytes, short version "8.0.7" (Pascal), long version.
        let long = b"ExifTool 8.0.7 DFONT Test";
        let mut vers = vec![8, 0, 0x80, 0, 0, 0];
        vers.push(5);
        vers.extend_from_slice(b"8.0.7");
        vers.push(long.len() as u8);
        vers.extend_from_slice(long);

        let file = build_rsrc(b"vers", 1, &vers);
        let metadata = parse_rsrc_metadata(&TestReader::new(file)).unwrap();
        assert_eq!(
            metadata.get("RSRC:ApplicationVersion"),
            Some(&TagValue::String("ExifTool 8.0.7 DFONT Test".to_string()))
        );
        // No font resource: the file type stays RSRC (RSRC.pm:95).
        assert_eq!(
            metadata.get("FileType"),
            Some(&TagValue::String("RSRC".to_string()))
        );
    }

    /// MacRoman applies to the long version string (RSRC.pm:151,
    /// `$et->Decode(..., 'MacRoman')`): 0xa9 is the copyright sign.
    #[test]
    fn vers_long_string_is_mac_roman_decoded() {
        let mut vers = vec![1, 0, 0, 0, 0, 0];
        vers.push(0); // empty short version
        vers.extend_from_slice(&[4, 0xa9, b'X', b'Y', b'Z']);
        let file = build_rsrc(b"vers", 1, &vers);
        let metadata = parse_rsrc_metadata(&TestReader::new(file)).unwrap();
        assert_eq!(
            metadata.get("RSRC:ApplicationVersion"),
            Some(&TagValue::String("©XYZ".to_string()))
        );
    }

    /// A `vers` resource with a different ID is not ApplicationVersion:
    /// RSRC.pm keys it as `vers_0x0001` only (RSRC.pm:49).
    #[test]
    fn vers_resource_with_other_id_is_ignored() {
        let mut vers = vec![1, 0, 0, 0, 0, 0, 0];
        vers.extend_from_slice(&[3, b'a', b'b', b'c']);
        let file = build_rsrc(b"vers", 2, &vers);
        let metadata = parse_rsrc_metadata(&TestReader::new(file)).unwrap();
        assert!(metadata.get("RSRC:ApplicationVersion").is_none());
    }

    /// An sfnt resource routes through the shared name-table walk and
    /// overrides the file type to DFONT (RSRC.pm:152-162).
    #[test]
    fn sfnt_resource_yields_font_tags_and_dfont_type() {
        let file = build_rsrc(b"sfnt", 0x80, &minimal_sfnt(b"Stencil Std"));
        let metadata = parse_rsrc_metadata(&TestReader::new(file)).unwrap();
        assert_eq!(
            metadata.get("Font:FontFamily"),
            Some(&TagValue::String("Stencil Std".to_string()))
        );
        assert_eq!(
            metadata.get("FileType"),
            Some(&TagValue::String("DFONT".to_string()))
        );
    }

    /// A POST resource marks the file as a data-fork font even though its
    /// PostScript payload is not parsed (RSRC.pm:196-199).
    #[test]
    fn post_resource_overrides_file_type_only() {
        let file = build_rsrc(b"POST", 0x1f5, b"\x00\x00%!PS-AdobeFont-1.0");
        let metadata = parse_rsrc_metadata(&TestReader::new(file)).unwrap();
        assert_eq!(
            metadata.get("FileType"),
            Some(&TagValue::String("DFONT".to_string()))
        );
        assert_eq!(metadata.len(), 1, "POST payload must not be guessed at");
    }

    /// A POST resource with an id other than 0x1f5 never reaches the DFONT
    /// override: the Main table keys only `POST_0x01f5`, and
    /// `next unless $tagInfo` (RSRC.pm:141) skips it first.
    #[test]
    fn post_resource_with_other_id_keeps_rsrc_type() {
        let file = build_rsrc(b"POST", 0x80, b"\x00\x00%!PS-AdobeFont-1.0");
        let metadata = parse_rsrc_metadata(&TestReader::new(file)).unwrap();
        assert_eq!(
            metadata.get("FileType"),
            Some(&TagValue::String("RSRC".to_string()))
        );
    }

    /// An sfnt resource whose data cannot be read hits `next` before the
    /// sfnt branch (RSRC.pm:124-131), so the file type is NOT overridden.
    #[test]
    fn unreadable_sfnt_resource_does_not_override_type() {
        let mut file = build_rsrc(b"sfnt", 0x80, &minimal_sfnt(b"Stencil Std"));
        // Corrupt the resource's u32 length (at datOff) to reach far past
        // EOF: read_resource must fail, exactly as ExifTool's
        // `$raf->Read($val, $valLen) == $valLen` does.
        file[0x100..0x104].copy_from_slice(&0x00ff_0000u32.to_be_bytes());
        let metadata = parse_rsrc_metadata(&TestReader::new(file)).unwrap();
        assert_eq!(
            metadata.get("FileType"),
            Some(&TagValue::String("RSRC".to_string()))
        );
        assert!(metadata.get("Font:FontFamily").is_none());
    }

    /// ICO files share the first four bytes (`\0\0\x01\0`) with the RSRC
    /// magic; the structural validation is what tells them apart. A real
    /// ICO header (reserved=0, type=1, count=1, one directory entry) must
    /// not validate.
    #[test]
    fn ico_header_does_not_validate_as_rsrc() {
        let mut ico = vec![
            0x00, 0x00, 0x01, 0x00, 0x01, 0x00, // ICONDIR
            0x10, 0x10, 0x00, 0x00, 0x01, 0x00, 0x20, 0x00, // entry
            0x68, 0x04, 0x00, 0x00, 0x16, 0x00, 0x00, 0x00, // size+offset
        ];
        ico.resize(64, 0);
        assert!(validate_rsrc(&TestReader::new(ico)).is_none());
    }

    /// The corpus DFONT itself: geometry from `Font.dfont`'s real header
    /// (datOff 0x100, mapOff 0x68a, datLen 0x58a, mapLen 0x46) validates.
    #[test]
    fn real_dfont_geometry_validates() {
        let mut file = vec![0u8; 0x6d0];
        file[0..4].copy_from_slice(&0x100u32.to_be_bytes());
        file[4..8].copy_from_slice(&0x68au32.to_be_bytes());
        file[8..12].copy_from_slice(&0x58au32.to_be_bytes());
        file[12..16].copy_from_slice(&0x46u32.to_be_bytes());
        // map header at 0x68a: typeOff=28, nameOff=70 at offsets 24/26.
        file[0x68a + 24..0x68a + 26].copy_from_slice(&28u16.to_be_bytes());
        file[0x68a + 26..0x68a + 28].copy_from_slice(&70u16.to_be_bytes());
        assert!(validate_rsrc(&TestReader::new(file)).is_some());
    }
}
