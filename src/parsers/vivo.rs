//! Vivo trailer reader (`Trailer.pm::ProcessVivo`).

const FOOTER: &[u8] = b"\xff\xff\xff\xff\x1b*9HWfu\x84\x93\xa2\xb1";
const JSON_MARKER: &[u8] = b"vivo{\"";
const STREAM_MARKER: &[u8] = b"streamdata";
const HDR_IMAGE_START: &[u8] = b"streamdata\xff\xd8\xff";
const HDR_IMAGE_END: &[u8] = b"\xff\xd9stream";
/// Trailer.pm accepts a trailer only when `$len < 1e7`.
const MAX_TRAILER_LEN: usize = 10_000_000;

/// A Vivo trailer as ProcessVivo sizes it for ProcessTrailers.
pub(crate) struct VivoTrailer<'a> {
    /// `DataPos`: the first `streamdata|vivo{"` at or after TrailerStart.
    pub start: usize,
    /// The `}\0`-bounded JSON value, when present and valid UTF-8.
    pub json: Option<&'a str>,
}

/// The JPEG `TrailerStart`: the byte after the EOI that ProcessJPEG reaches by
/// continuing its marker walk from `scan_from` (just past the SOS marker's
/// fixed bytes) through the entropy-coded data (ExifTool.pm:7337-7400,7464-7468).
///
/// Like ExifTool it reads up to each `0xff`, skips `0xff` padding, treats the
/// `%markerLenBytes` markers as stand-alone, skips any other segment by its
/// length word, and gives up (no TrailerStart) at SOD, a bad length or EOF.
pub(crate) fn jpeg_trailer_start(file: &[u8], scan_from: usize) -> Option<usize> {
    let mut pos = scan_from;
    loop {
        let mut at = pos + memchr::memchr(0xff, file.get(pos..)?)? + 1;
        while *file.get(at)? == 0xff {
            at += 1;
        }
        let marker = file[at];
        at += 1;
        pos = match marker {
            0xd9 => return Some(at),
            0x93 => return None,
            0x00 | 0x01 | 0xd0..=0xd8 | 0xda | 0x30..=0x3f | 0x4f | 0x92 => at,
            0x74 | 0x75 | 0x77 => {
                let length = u32::from_be_bytes(file.get(at..at + 4)?.try_into().ok()?);
                let length = usize::try_from(length).ok().filter(|length| *length >= 4)?;
                let end = at.checked_add(length)?;
                (end <= file.len()).then_some(end)?
            }
            _ => {
                let length =
                    usize::from(u16::from_be_bytes(file.get(at..at + 2)?.try_into().ok()?));
                if length < 2 {
                    return None;
                }
                let end = at + length;
                (end <= file.len()).then_some(end)?
            }
        };
    }
}

/// Returns whether IdentifyTrailer would classify the trailer ending at `end`
/// as Vivo (its fixed footer ends there).
pub(crate) fn has_footer_at(file: &[u8], end: usize) -> bool {
    file.get(..end).is_some_and(|file| file.ends_with(FOOTER))
}

/// Trailer.pm starts at the first `streamdata|vivo{\"` match, not the last
/// occurrence before the footer.
fn first_vivo_marker(buffer: &[u8]) -> Option<usize> {
    match (
        memchr::memmem::find(buffer, STREAM_MARKER),
        memchr::memmem::find(buffer, JSON_MARKER),
    ) {
        (Some(stream), Some(json)) => Some(stream.min(json)),
        (Some(stream), None) => Some(stream),
        (None, Some(json)) => Some(json),
        (None, None) => None,
    }
}

/// ProcessVivo for the trailer ending at `end`, scanning from the JPEG's
/// `TrailerStart` (the byte after EOI). ExifTool has no TrailerStart when
/// reading other carriers, where ProcessVivo returns 0; callers express that
/// by not calling this. `None` is ProcessVivo's failure return, which also
/// ends ProcessTrailers' walk.
pub(crate) fn process_vivo(
    file: &[u8],
    end: usize,
    trailer_start: usize,
) -> Option<VivoTrailer<'_>> {
    let buffer = file.get(trailer_start..end)?;
    if buffer.is_empty() || buffer.len() >= MAX_TRAILER_LEN || !buffer.ends_with(FOOTER) {
        return None;
    }
    let marker = first_vivo_marker(buffer)?;
    let trailer = &buffer[marker..];
    // An HDRImage stream advances the JSON search past its `\xff\xd9stream`
    // (info|coun) terminator, exactly as the shared `pos($buff)` does.
    let search_from = if trailer.starts_with(HDR_IMAGE_START) {
        memchr::memmem::find_iter(trailer, HDR_IMAGE_END)
            .find(|at| {
                let tail = &trailer[at + HDR_IMAGE_END.len()..];
                tail.starts_with(b"info") || tail.starts_with(b"coun")
            })
            .map_or(0, |at| at + HDR_IMAGE_END.len() + 4)
    } else {
        0
    };
    let json = memchr::memmem::find(&trailer[search_from..], JSON_MARKER).and_then(|at| {
        let json_start = search_from + at + JSON_MARKER.len() - 2;
        let after_marker = search_from + at + JSON_MARKER.len();
        let close = memchr::memmem::find(&trailer[after_marker..], b"}\0")?;
        std::str::from_utf8(&trailer[json_start..=after_marker + close]).ok()
    });
    Some(VivoTrailer {
        start: trailer_start + marker,
        json,
    })
}
