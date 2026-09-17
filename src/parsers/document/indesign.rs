//! Adobe InDesign document (.indd, .ind) metadata parser.
//!
//! ExifTool 13.59's `InDesign::ProcessIND` (InDesign.pm:33-247) validates two
//! 4096-byte master pages, picks whichever carries the higher sequence
//! number, reads the object-database offset from it, and then walks a chain
//! of contiguous objects looking for exactly one thing: an XMP stream. There
//! is no `InDesign::Main` tag table -- every tag ExifTool reports from an
//! InDesign file comes out of `XMP::Main` (InDesign.pm:174, 194).
//!
//! This parser reproduces that walk and hands the stream it finds to this
//! crate's XMP reader, the same way `djvu.rs` and `pdf/xmp_extractor.rs` do.
//!
//! # Deliberately absent
//!
//! Everything under `if ($outfile)` -- InDesign.pm:84-94, 175-192, 203-227,
//! 229-234 -- is the writer, and this is a reader. The `LargeFileSupport`
//! branch (InDesign.pm:76-83) and the >300 MiB XMP guard
//! (InDesign.pm:143-154) are option-gated warnings that change no tag value.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/InDesign.pm`

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::parsers::xmp::rdf_parser::{XmpValue, parse_xmp_typed};

/// InDesign.pm:25.
const MASTER_PAGE_GUID: &[u8; 16] =
    b"\x06\x06\xed\xf5\xd8\x1d\x46\xe5\xbd\x31\xef\xe7\xfe\x74\xb7\x1d";
/// InDesign.pm:26.
const OBJECT_HEADER_GUID: &[u8; 16] =
    b"\xde\x39\x39\x79\x51\x88\x4b\x6c\x8E\x63\xee\xf8\xae\xe0\xdd\x38";
/// InDesign.pm:27.
const OBJECT_TRAILER_GUID: &[u8; 16] =
    b"\xfd\xce\xdb\x70\xf7\x86\x4b\x4f\xa4\xd3\xc7\x28\xb3\x41\x71\x06";

const MASTER_PAGE_LEN: usize = 4096;
/// InDesign.pm:102, `$raf->Read($hdr, 32)`.
const OBJECT_HEADER_LEN: usize = 32;
/// InDesign.pm:134-135: the smallest stream that could hold an XMP header.
const MIN_XMP_STREAM: u32 = 56;

fn u32_le(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn u64_le(data: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        data.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

/// Extract InDesign metadata, which is to say: find the XMP stream.
pub fn parse_indesign_metadata(
    reader: &dyn FileReader,
) -> std::result::Result<MetadataMap, String> {
    // InDesign.pm:41-42.
    let head = reader.read(0, 16).map_err(|error| error.to_string())?;
    if head != &MASTER_PAGE_GUID[..] {
        return Err("not an InDesign master page".to_string());
    }

    // InDesign.pm:48-58: two full master pages, the second also GUID-tagged.
    let first = reader
        .read(0, MASTER_PAGE_LEN)
        .map_err(|error| error.to_string())?;
    let second = reader
        .read(MASTER_PAGE_LEN as u64, MASTER_PAGE_LEN)
        .map_err(|error| error.to_string())?;
    if first.len() < MASTER_PAGE_LEN || second.len() < MASTER_PAGE_LEN {
        return Err("unexpected end of file in InDesign master pages".to_string());
    }
    if !second.starts_with(&MASTER_PAGE_GUID[..]) {
        return Err("second InDesign master page is invalid".to_string());
    }

    // InDesign.pm:54, `SetByteOrder('II')` -- the *headers* are always
    // little-endian; only the stream length word at InDesign.pm:64-72 varies.
    let sequence_first = u64_le(&first, 264).ok_or("short InDesign master page")?;
    let sequence_second = u64_le(&second, 264).ok_or("short InDesign master page")?;
    // InDesign.pm:62, `$seq2 > $seq1 ? \$buf2 : \$buff`.
    let current = if sequence_second > sequence_first {
        &second
    } else {
        &first
    };

    // InDesign.pm:64-72.
    let stream_big_endian = match current[24] {
        1 => false,
        2 => true,
        _ => return Err("invalid InDesign stream byte order".to_string()),
    };

    // InDesign.pm:73-75.
    let pages = u32_le(current, 280).ok_or("short InDesign master page")?;
    if pages < 2 {
        return Err("invalid InDesign page count".to_string());
    }
    let mut pos = u64::from(pages) * MASTER_PAGE_LEN as u64;

    let mut metadata = MetadataMap::new();
    // InDesign.pm:101-228, the contiguous-object walk.
    loop {
        let Ok(header) = reader.read(pos, OBJECT_HEADER_LEN) else {
            break;
        };
        // InDesign.pm:103-105: anything that is not an object header ends the
        // walk; all-null is ordinary padding, anything else is a warning
        // ExifTool issues without changing a tag.
        if header.len() != OBJECT_HEADER_LEN || !header.starts_with(&OBJECT_HEADER_GUID[..]) {
            break;
        }
        pos += OBJECT_HEADER_LEN as u64;

        let mut len = u32_le(&header, 24).ok_or("short InDesign object header")?;

        // InDesign.pm:134-199.
        if len > MIN_XMP_STREAM {
            let Ok(peek) = reader.read(pos, MIN_XMP_STREAM as usize) else {
                break;
            };
            if peek.len() != MIN_XMP_STREAM as usize {
                break;
            }
            if let Some(declared) = xmp_stream_length(&peek, stream_big_endian) {
                // InDesign.pm:138: the four-byte length word is not part of
                // the XMP.
                let xmp_len = len - 4;
                // InDesign.pm:156, `$raf->Seek(-52, 1)` -- back up over the
                // 52 bytes of XMP already peeked at, leaving the length word
                // consumed.
                let Ok(xmp) = reader.read(pos + 4, xmp_len as usize) else {
                    break;
                };
                if xmp.len() != xmp_len as usize {
                    break;
                }
                // InDesign.pm:166-173: a declared length shorter than the
                // stream truncates the XMP; a longer one is a read error and
                // ExifTool parses nothing.
                let usable = match declared.cmp(&xmp_len) {
                    std::cmp::Ordering::Less => &xmp[..declared as usize],
                    std::cmp::Ordering::Equal => &xmp[..],
                    std::cmp::Ordering::Greater => break,
                };
                insert_xmp(usable, &mut metadata);
                // InDesign.pm:196, `$len = 0` -- the whole stream is consumed.
                pos += u64::from(len);
                len = 0;
            } else {
                // InDesign.pm:198, `$len -= 56`.
                pos += u64::from(MIN_XMP_STREAM);
                len -= MIN_XMP_STREAM;
            }
        }
        // InDesign.pm:212-215, skip whatever is left of the stream.
        pos += u64::from(len);

        // InDesign.pm:216-220: every object ends with a trailer, and a
        // missing one ends the walk.
        let Ok(trailer) = reader.read(pos, OBJECT_HEADER_LEN) else {
            break;
        };
        if trailer.len() != OBJECT_HEADER_LEN || !trailer.starts_with(&OBJECT_TRAILER_GUID[..]) {
            break;
        }
        pos += OBJECT_HEADER_LEN as u64;
    }

    Ok(metadata)
}

/// InDesign.pm:136's XMP test:
///
/// ```text
/// $buff =~ /^(....)<\?xpacket begin=(['"])\xef\xbb\xbf\2 id=(['"])W5M0MpCehiHzreSzNTczkc9d\3/s
/// ```
///
/// Returns the leading length word decoded with the stream byte order
/// (InDesign.pm:166, `unpack($streamInt32u, $lenWord)`), or `None` when the
/// stream is not XMP.
fn xmp_stream_length(peek: &[u8], big_endian: bool) -> Option<u32> {
    let body = peek.get(4..)?;
    let rest = body.strip_prefix(b"<?xpacket begin=".as_slice())?;
    let quote = *rest.first()?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let rest = rest.get(1..)?.strip_prefix(b"\xef\xbb\xbf".as_slice())?;
    if *rest.first()? != quote {
        return None;
    }
    let rest = rest.get(1..)?.strip_prefix(b" id=".as_slice())?;
    let id_quote = *rest.first()?;
    if id_quote != b'"' && id_quote != b'\'' {
        return None;
    }
    let rest = rest
        .get(1..)?
        .strip_prefix(b"W5M0MpCehiHzreSzNTczkc9d".as_slice())?;
    if *rest.first()? != id_quote {
        return None;
    }
    let word: [u8; 4] = peek.get(..4)?.try_into().ok()?;
    Some(if big_endian {
        u32::from_be_bytes(word)
    } else {
        u32::from_le_bytes(word)
    })
}

/// InDesign.pm:174,194: hand the stream to `XMP::Main`.
fn insert_xmp(xmp: &[u8], metadata: &mut MetadataMap) {
    let Ok(tags) = parse_xmp_typed(xmp) else {
        return;
    };
    for (name, value) in tags {
        let value = match value {
            XmpValue::Scalar(value) => TagValue::new_string(value),
            XmpValue::List(values) => {
                TagValue::Array(values.into_iter().map(TagValue::new_string).collect())
            }
        };
        metadata.insert(name, value);
    }
}
