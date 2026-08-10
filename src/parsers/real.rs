//! RealMedia (`.rm`/`.rv`/`.rmvb`) MIME identity.
//!
//! RM, RV and RMVB share one top-level chunk format and one Perl function
//! (`ProcessReal`, Real.pm:516-664) that reads it; `%Image::ExifTool::
//! Real::Media`'s own note says so: "used in RealMedia and RealVideo (RM, RV
//! and RMVB) files." `%mimeType` gives each a generic default --
//! `application/vnd.rn-realmedia` for RM -- but a real file wraps one or more
//! streams, and when there is exactly one, ExifTool reports *that stream's*
//! MIME type instead (Real.pm:653-657):
//!
//! ```text
//!     # override MIMEType with stream MIME type if we only have one stream
//!     if (@mimeTypes == 1 and length $mimeTypes[0]) {
//!         $$et{VALUE}{MIMEType} = $mimeTypes[0];
//! ```
//!
//! `@mimeTypes` is filled while walking the top-level chunks, from each
//! `MDPR` (stream properties) chunk's own embedded MIME string -- skipping
//! any that starts with `logical-`, a pseudo-type RealMedia uses for its own
//! bookkeeping streams rather than real media (Real.pm:644).
//!
//! Only that override is modelled here. RM/RV/RMVB have no parser in OxiDex
//! at all -- `File:FileType` and `File:FileTypeExtension` already come from
//! `crate::filetype`'s generated tables, which get those two right -- so this
//! reads just enough of the chunk structure to answer one question: what
//! MIME type, if any, should replace the generic default.

/// `RMF`, `PROP`, `MDPR`, `CONT`, `DATA`, `INDX`... every top-level chunk
/// shares this 10-byte header: FourCC, then a big-endian `u32` size that
/// includes the header itself, then a `u16` version (Real.pm's own `unpack('a4Nn', ...)`).
const CHUNK_HEADER_LEN: usize = 10;

/// `$max = 10`'s counterpart for the top level: a generous bound on how many
/// chunks to walk before giving up, well past what a real header carries.
const MAX_CHUNKS: usize = 64;

/// The MIME type ExifTool reports for a RealMedia file with exactly one
/// stream, or `None` when the generic default from `%mimeType` should stand
/// -- no streams, more than one, or a structure this does not recognise.
///
/// `file` is the whole file (or at least its header and stream properties;
/// RealMedia's payload comes after them). Reading past `DATA` is unnecessary
/// -- ExifTool's own non-verbose path stops there too (Real.pm:604: `last if
/// $tag eq 'DATA'`) -- and this never gets that far, since every fixture's
/// `MDPR` chunks precede it.
#[must_use]
pub fn single_stream_mime_type(file: &[u8]) -> Option<String> {
    // The 8 bytes ProcessReal reads to identify the file, then classify it:
    // `.RMF` selects the RealMedia path this function models.
    let header = file.get(0..8)?;
    if &header[0..4] != b".RMF" {
        return None;
    }
    // `unpack('x4N', $buff)` -- skip the 4-byte magic, read the big-endian
    // size of the RMF chunk itself, which is how far past the 8 already read
    // the rest of its header runs.
    let rmf_size = u32::from_be_bytes(header[4..8].try_into().ok()?) as usize;
    let mut pos = rmf_size;

    let mut mime_types = Vec::new();
    for _ in 0..MAX_CHUNKS {
        // Running out of chunk header to read is not a malformed file --
        // ExifTool's own test fixture ends exactly at the last chunk's final
        // byte, with no trailing terminator. Whatever was already collected
        // still stands; only a genuinely unreadable *body* below aborts the
        // scan, since that means a size field lied about the file's length.
        let Some(chunk_header) = file.get(pos..pos + CHUNK_HEADER_LEN) else {
            break;
        };
        let tag = &chunk_header[0..4];
        let size = u32::from_be_bytes(chunk_header[4..8].try_into().ok()?) as usize;

        if tag == b"\0\0\0\0" || tag == b"DATA" {
            break;
        }
        // `$size & 0x80000000 or $size < 10` -- the high bit can't be set on
        // an in-range `u32::from_be_bytes` read into `usize` here, so only
        // the minimum needs checking.
        if size < CHUNK_HEADER_LEN {
            break;
        }

        let body = file.get(pos + CHUNK_HEADER_LEN..pos + size)?;
        if tag == b"MDPR"
            && let Some(mime) = mdpr_stream_mime_type(body)
            && !mime.starts_with("logical-")
        {
            mime_types.push(mime);
        }
        pos += size;
    }

    match mime_types.as_slice() {
        [only] if !only.is_empty() => Some(only.clone()),
        _ => None,
    }
}

/// The `StreamMimeType` field of one `MDPR` chunk's body.
///
/// `%Image::ExifTool::Real::MediaProps` (Real.pm:115-136) is `FORMAT =>
/// 'int32u'` with one override: field 0 (`StreamNumber`) is `int16u`. The six
/// `int32u` fields after it are all fixed-width and never read here, so this
/// only has to walk past their byte count to reach `StreamNameLen`,
/// `StreamName`, `StreamMimeLen` and `StreamMimeType` in sequence.
fn mdpr_stream_mime_type(body: &[u8]) -> Option<String> {
    // StreamNumber (2) + 7 int32u fields (28) = 30 bytes before the
    // length-prefixed strings begin.
    let mut pos = 2 + 7 * 4;
    let name_len = usize::from(*body.get(pos)?);
    pos += 1 + name_len;
    let mime_len = usize::from(*body.get(pos)?);
    pos += 1;
    let mime = body.get(pos..pos + mime_len)?;
    std::str::from_utf8(mime).ok().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One top-level chunk: FourCC + big-endian size (header included) +
    /// version, then `body`.
    fn chunk(tag: &[u8; 4], version: u16, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(tag);
        out.extend_from_slice(&((CHUNK_HEADER_LEN + body.len()) as u32).to_be_bytes());
        out.extend_from_slice(&version.to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    /// An `MDPR` body naming one stream: 30 bytes of fixed fields (zeroed,
    /// since none of them are read), then the length-prefixed name and MIME.
    fn mdpr_body(name: &str, mime: &str) -> Vec<u8> {
        let mut out = vec![0u8; 2 + 7 * 4];
        out.push(name.len() as u8);
        out.extend_from_slice(name.as_bytes());
        out.push(mime.len() as u8);
        out.extend_from_slice(mime.as_bytes());
        out
    }

    fn rm_file(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b".RMF");
        out.extend_from_slice(&18u32.to_be_bytes()); // ExifTool's own RMF chunk size
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // padding to 18 bytes total
        for c in chunks {
            out.extend_from_slice(c);
        }
        out
    }

    #[test]
    fn a_single_stream_overrides_the_generic_mime_type() {
        // Real.rm's own shape: one Audio Stream MDPR, then trailing chunks
        // that must not confuse the walk.
        let file = rm_file(&[
            chunk(b"PROP", 0, &[0u8; 40]),
            chunk(
                b"MDPR",
                0,
                &mdpr_body("Audio Stream", "audio/x-pn-realaudio"),
            ),
            chunk(b"CONT", 0, &[0u8; 20]),
        ]);
        assert_eq!(
            single_stream_mime_type(&file).as_deref(),
            Some("audio/x-pn-realaudio")
        );
    }

    #[test]
    fn a_logical_stream_is_not_counted() {
        // Real.pm:644 excludes `logical-*` pseudo-types from @mimeTypes
        // before the single-stream check runs, so a file with only one of
        // these has zero *real* streams, not one.
        let file = rm_file(&[chunk(b"MDPR", 0, &mdpr_body("", "logical-fileinfo"))]);
        assert_eq!(single_stream_mime_type(&file), None);
    }

    #[test]
    fn multiple_streams_leave_the_generic_type_alone() {
        let file = rm_file(&[
            chunk(
                b"MDPR",
                0,
                &mdpr_body("Video Stream", "video/x-pn-realvideo"),
            ),
            chunk(
                b"MDPR",
                0,
                &mdpr_body("Audio Stream", "audio/x-pn-realaudio"),
            ),
        ]);
        assert_eq!(single_stream_mime_type(&file), None);
    }

    #[test]
    fn a_data_chunk_ends_the_walk() {
        // A stream declared only after DATA does not exist as far as
        // ExifTool's non-verbose path is concerned.
        let file = rm_file(&[
            chunk(b"DATA", 0, &[0u8; 4]),
            chunk(
                b"MDPR",
                0,
                &mdpr_body("Audio Stream", "audio/x-pn-realaudio"),
            ),
        ]);
        assert_eq!(single_stream_mime_type(&file), None);
    }

    #[test]
    fn no_streams_leaves_the_generic_type_alone() {
        let file = rm_file(&[chunk(b"PROP", 0, &[0u8; 40])]);
        assert_eq!(single_stream_mime_type(&file), None);
    }

    #[test]
    fn a_non_rm_file_is_declined() {
        assert_eq!(single_stream_mime_type(b".ra\xfd0000"), None);
        assert_eq!(single_stream_mime_type(b"not a real file"), None);
    }
}
