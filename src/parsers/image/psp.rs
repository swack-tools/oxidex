//! Paint Shop Pro (PSP) metadata parser.
//!
//! ExifTool routes `.psp`/`.pspimage`/`.pspframe`/`.pspshape`/`.psptube`/
//! `.tub` files through `Image::ExifTool::PSP::ProcessPSP` (PSP.pm:225-265),
//! which validates a fixed 32-byte signature, reads a two-`int16u` file
//! version, then walks a chain of `~BK\0` blocks. Three block IDs carry
//! metadata: 0 (`PSP::Image`), 1 (`PSP::Creator`) and 10 (`PSP::Ext`).
//!
//! # What comes from the transcription and what does not
//!
//! `PSP::Image` (PSP.pm:71-99) is a real `ProcessBinaryData` layout, so it is
//! read from the generated table -- including both its `PrintConv` enums --
//! rather than restated here.
//!
//! `PSP::Creator` (PSP.pm:101-147) is *not* a binary layout despite being
//! transcribed: its `PROCESS_PROC` is `ProcessExtData` (PSP.pm:163-219) and
//! its hash keys are sub-block **tag IDs**, not byte offsets. Decoding it
//! through the generated table would read tag ID 7 as "byte offset 7", so
//! this parser walks the `~FL\0` sub-blocks itself and applies each field's
//! declared format and conversion against the cited Perl.
//!
//! # What is deliberately absent
//!
//! - **`PSP::Ext` tag 3 (`EXIFInfo`, PSP.pm:154-157).** The embedded EXIF
//!   block does not use ordinary TIFF offsets. PSP.pm:199-202 spells out why:
//!   "They use a standard TIFF offset to point to the first IFD, but after
//!   that the offsets are relative to the start of the IFD instead of the
//!   TIFF base, which means that I must handle it as a special case." That
//!   non-standard base is not something this parser can hand the shared EXIF
//!   reader without rewriting its offset arithmetic, so the block is skipped
//!   and its tags (`IFD0:Copyright`, `XResolution`, `YResolution`,
//!   `ResolutionUnit` on the pinned fixture) are simply absent. Emitting them
//!   from a standard-offset read would put wrong values under real tag names.
//! - **Block 18 (`PreviewImage`).** ExifTool itself leaves this commented out
//!   (PSP.pm:65-69, "this is inside the composite image bank block (16),
//!   which I don't want to parse").
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/PSP.pm`

use crate::core::formatters::numeric_precision::perl_number;
use crate::core::{FileReader, MetadataMap, TagValue};
use crate::exiftool_tables::{decode_binary_table, find_table};
use crate::io::ByteOrder;
use crate::io::timestamp::unix_to_local_exif_datetime;

/// PSP.pm:232-233, the exact 32-byte file signature.
const PSP_SIGNATURE: &[u8] = b"Paint Shop Pro Image File\x0a\x1a\0\0\0\0\0";

/// PSP.pm:232-234: the 32-byte signature is followed by four version bytes,
/// so the first block header starts at 36 (PSP.pm:245).
const FIRST_BLOCK_OFFSET: u64 = 36;

/// PSP.pm:248, `$buff =~ /^~BK\0/`.
const BLOCK_MAGIC: &[u8] = b"~BK\0";

/// PSP.pm:169, `substr($$dataPt, $pos, 4) eq "~FL\0"`.
const SUB_BLOCK_MAGIC: &[u8] = b"~FL\0";

/// PSP.pm:174, each `~FL\0` sub-block header is 10 bytes: magic (4),
/// `int16u` tag (2), `int32u` length (4).
const SUB_BLOCK_HEADER_LEN: usize = 10;

/// Extract Paint Shop Pro metadata (`Image::ExifTool::PSP::ProcessPSP`).
pub fn parse_psp_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let file_size = reader.size();
    let header_len = PSP_SIGNATURE.len() + 4;
    if file_size < header_len as u64 {
        return Err("PSP file is too short for its 36-byte header".to_string());
    }
    let header = reader.read(0, header_len).map_err(|e| e.to_string())?;
    if &header[..PSP_SIGNATURE.len()] != PSP_SIGNATURE {
        return Err("invalid PSP signature".to_string());
    }

    // PSP.pm:237-241: `unpack('v*', $buff)` over the four version bytes --
    // PSP is little-endian throughout (`SetByteOrder('II')`, PSP.pm:236).
    let major = u16::from_le_bytes([header[32], header[33]]);
    let minor = u16::from_le_bytes([header[34], header[35]]);

    let mut metadata = MetadataMap::new();
    // PSP.pm:242: `HandleTag($tagTablePtr, FileVersion => "@a")` with
    // PSP.pm:39's `PrintConv => '$val=~tr/ /./; $val'` -- i.e. the two
    // version numbers joined by a dot.
    metadata.insert(
        "PSP:FileVersion".to_string(),
        TagValue::new_string(format!("{major}.{minor}")),
    );

    // PSP.pm:240: block headers are 10 bytes for file version > 3, else 14.
    let header_size: u64 = if major > 3 { 10 } else { 14 };

    let mut pos = FIRST_BLOCK_OFFSET;
    loop {
        // PSP.pm:247: the loop simply stops when a full block header cannot
        // be read -- a short tail is normal end-of-file, not an error.
        if pos + header_size > file_size {
            break;
        }
        let block = reader
            .read(pos, header_size as usize)
            .map_err(|e| e.to_string())?;
        if &block[..4] != BLOCK_MAGIC {
            // PSP.pm:249: "Lost synchronization while reading main PSP
            // blocks" -- ExifTool warns and stops.
            break;
        }
        let tag = u16::from_le_bytes([block[4], block[5]]);
        // PSP.pm:253, `Get32u(\$buff, $hlen - 4)`.
        let len_off = (header_size - 4) as usize;
        let len = u32::from_le_bytes([
            block[len_off],
            block[len_off + 1],
            block[len_off + 2],
            block[len_off + 3],
        ]) as u64;

        let body_start = pos + header_size;
        // PSP.pm:254: `$pos += $hlen + $len`.
        let next = body_start + len;

        // PSP.pm:255-258: an unrecognised block ID is skipped wholesale.
        if matches!(tag, 0 | 1 | 10) {
            if body_start + len > file_size {
                // PSP.pm:262, "Truncated main block".
                break;
            }
            let body = reader
                .read(body_start, len as usize)
                .map_err(|e| e.to_string())?;
            match tag {
                0 => read_image_block(&body, major, &mut metadata),
                1 => read_creator_block(&body, &mut metadata),
                // PSP.pm:154-157: the only tag in `PSP::Ext` is `EXIFInfo`,
                // which this parser deliberately does not read (see the
                // module docs). Walking the sub-blocks would find nothing
                // else to emit, so the block is skipped entirely.
                _ => {}
            }
        }

        if next <= pos {
            // A zero-length block with a zero-length header would spin
            // forever; ExifTool's RAF cannot rewind, so neither do we.
            break;
        }
        pos = next;
    }

    Ok(metadata)
}

/// PSP.pm:40-55, block 0 (`ImageInfo`): a `PSP::Image` subdirectory whose
/// `Start` is 4 when `$$self{PSPFileVersion} > 3` and 0 otherwise.
fn read_image_block(body: &[u8], major: u16, metadata: &mut MetadataMap) {
    let start = if major > 3 { 4usize } else { 0 };
    if body.len() <= start {
        return;
    }
    let Some(table) = find_table("PSP", "Image") else {
        return;
    };
    let decode = decode_binary_table(table, &body[start..], ByteOrder::Little);
    for decoded in decode.fields() {
        let Some(value) = decoded.emit() else {
            continue;
        };
        // PSP.pm:74's `ImageResolution` is a `double`. ExifTool stringifies
        // it with Perl's default `%.15g`, so 200.0 prints as `200` -- not
        // `200.00`, which is what falls out of a plain float rendering.
        let value = match value {
            TagValue::Float(f) => TagValue::new_string(perl_number(f)),
            other => other,
        };
        metadata.insert(format!("PSP:{}", decoded.field.name), value);
    }
}

/// PSP.pm:163-219, `ProcessExtData` over `PSP::Creator` (PSP.pm:101-147).
fn read_creator_block(body: &[u8], metadata: &mut MetadataMap) {
    let mut pos = 0usize;
    // PSP.pm:171, `while ($pos + 10 < $dirLen)`.
    while pos + SUB_BLOCK_HEADER_LEN < body.len() {
        if &body[pos..pos + 4] != SUB_BLOCK_MAGIC {
            // PSP.pm:172-173, "Lost synchronization while reading sub blocks".
            break;
        }
        let tag = u16::from_le_bytes([body[pos + 4], body[pos + 5]]);
        let len = u32::from_le_bytes([body[pos + 6], body[pos + 7], body[pos + 8], body[pos + 9]])
            as usize;
        // PSP.pm:176-180.
        let Some(end) = pos
            .checked_add(SUB_BLOCK_HEADER_LEN)
            .and_then(|p| p.checked_add(len))
        else {
            break;
        };
        if end > body.len() {
            // PSP.pm:179, "Truncated sub block".
            break;
        }
        let field = &body[end - len..end];
        if let Some((name, value)) = decode_creator_field(tag, field) {
            metadata.insert(format!("PSP:{name}"), value);
        }
        pos = end;
    }
}

/// One `PSP::Creator` sub-block (PSP.pm:101-147). Tags the table does not
/// declare are skipped, matching PSP.pm:181's `next unless $$tagTablePtr{$tag}`.
fn decode_creator_field(tag: u16, field: &[u8]) -> Option<(&'static str, TagValue)> {
    match tag {
        // PSP.pm:103, plain string.
        0 => Some(("Title", string_value(field)?)),
        // PSP.pm:104-110 / :111-117: `int32u` through
        // `ConvertUnixTime($val,1)` then `ConvertDateTime`.
        1 | 2 => {
            let seconds = i64::from(read_u32(field)?);
            let name = if tag == 1 { "CreateDate" } else { "ModifyDate" };
            Some((
                name,
                TagValue::new_string(unix_to_local_exif_datetime(seconds)?),
            ))
        }
        // PSP.pm:118-121 / :122-125 / :126, plain strings.
        3 => Some(("Artist", string_value(field)?)),
        4 => Some(("Copyright", string_value(field)?)),
        5 => Some(("Description", string_value(field)?)),
        // PSP.pm:127-134: `int32u` with a two-entry PrintConv. A raw value
        // outside the declared pair has no ExifTool rendering to copy, so
        // it is emitted unconverted exactly as ExifTool's hash-PrintConv
        // fallback does -- `Unknown (N)`.
        6 => {
            let raw = read_u32(field)?;
            let rendered = match raw {
                0 => "Unknown".to_string(),
                1 => "Paint Shop Pro".to_string(),
                other => format!("Unknown ({other})"),
            };
            Some(("CreatorAppID", TagValue::new_string(rendered)))
        }
        // PSP.pm:135-140: `int8u` Count 4, `ValueConv => 'join(" ",reverse
        // split " ", $val)'` (the bytes are stored low-order first) then
        // `PrintConv => '$val=~tr/ /./; $val'`.
        7 => {
            if field.len() < 4 {
                return None;
            }
            let rendered = field[..4]
                .iter()
                .rev()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(".");
            Some(("CreatorAppVersion", TagValue::new_string(rendered)))
        }
        _ => None,
    }
}

/// PSP.pm's string fields carry no declared `Format`, so ExifTool takes the
/// sub-block body verbatim. Trailing NUL padding is dropped, matching
/// ExifTool's `string` handling.
///
/// # Why a non-UTF-8 body yields no tag
///
/// PSP declares no character set, so ExifTool passes these bytes through
/// untouched: the pinned `t/images/PSP.psp` stores `Copyright` as a raw
/// `0xA9` (Latin-1 "(c)") and `exiftool -a -G1 -s` prints that byte
/// verbatim. A Rust `String` cannot hold it, and both available
/// substitutions are wrong in a way nothing downstream could detect --
/// `from_utf8_lossy` yields U+FFFD, and decoding as Latin-1 yields UTF-8
/// `0xC2 0xA9`, neither of which is the byte ExifTool wrote. (ExifTool's own
/// `-j` writer prints `?` here, so even the two oracle modes disagree.)
/// Per AGENTS.md an absent tag is the correct output, so a field that is not
/// valid UTF-8 is omitted rather than approximated.
fn string_value(field: &[u8]) -> Option<TagValue> {
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    std::str::from_utf8(&field[..end])
        .ok()
        .map(TagValue::new_string)
}

fn read_u32(field: &[u8]) -> Option<u32> {
    if field.len() < 4 {
        return None;
    }
    Some(u32::from_le_bytes([field[0], field[1], field[2], field[3]]))
}
