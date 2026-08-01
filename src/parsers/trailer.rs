//! Locating trailers -- blocks appended *after* the end of an image.
//!
//! ExifTool walks trailers as a chain. `IdentifyTrailer` (ExifTool.pm:7511)
//! matches a signature in the 64 bytes ending at `filesize - offset`, the
//! matching `Process<Name>` proc returns that trailer's `DataPos`/`DirLen`,
//! and `ProcessTrailers` (ExifTool.pm:7019) then adds `DirLen` to `offset` and
//! identifies the next one further in. Peeling the trailers off one at a time
//! is what lets ExifTool find a trailer that is not last in the file.
//!
//! oxidex cannot walk that chain, because reaching an inner trailer requires
//! being able to *size* every trailer that follows it, and oxidex implements
//! only three of the eighteen types `IdentifyTrailer` knows. In
//! `combined-samples/ExifTool.jpg` the AFCP trailer is the innermost of eight
//! -- two FotoStation records, CanonVRD, PhotoMechanic, MIE, Samsung and Vivo
//! all sit between it and the end of the file -- so a chain walk here would
//! stop at the first unimplemented type and find nothing.
//!
//! What oxidex does instead is scan backwards for each trailer's own end
//! marker, which [`find_last`] implements. That works without knowing anything
//! about neighbouring trailers, but it gives up ExifTool's structural
//! guarantee that a candidate really is a trailer boundary, so **every caller
//! must validate a candidate at a second, independent point** before accepting
//! it: take the length the marker's record declares and require a matching
//! structure at the other end of the trailer. A marker match alone is not
//! enough -- trailer markers are a handful of bytes, and the bytes being
//! scanned are other trailers' compressed payloads.

/// Scans backwards from the end of `file` for the outermost trailer whose end
/// marker `locate` then accepts.
///
/// Every trailer format read here ends with a fixed-layout record, so a
/// candidate is described by the `marker` bytes that record opens with and
/// `marker_back`, the distance from the trailer's end back to that marker.
/// Only ends whose marker really is present are offered to `locate`, which
/// lets this skip most of a large file with one SIMD search instead of testing
/// every byte offset in turn.
///
/// `locate` is called with the whole file and a candidate *end* offset -- one
/// past the trailer's last byte -- outermost first, and returns `None` for a
/// candidate it rejects. Ends below `min_len` are never offered, since no
/// trailer can end before its own minimum length has been read.
///
/// Scanning from the end mirrors ExifTool, which always works inwards from the
/// end of the file, so the trailer found first is the last one written.
///
/// See the module docs: `locate` owns the second point of the check.
pub fn find_last<'a, T>(
    file: &'a [u8],
    min_len: usize,
    marker: &[u8],
    marker_back: usize,
    locate: impl Fn(&'a [u8], usize) -> Option<T>,
) -> Option<T> {
    debug_assert!(
        marker_back >= marker.len(),
        "the marker must lie inside the trailer it ends"
    );
    memchr::memmem::rfind_iter(file, marker).find_map(|at| {
        let end = at.checked_add(marker_back)?;
        if end > file.len() || end < min_len {
            return None;
        }
        locate(file, end)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accept_any(_: &[u8], end: usize) -> Option<usize> {
        Some(end)
    }

    #[test]
    fn test_offers_only_ends_whose_marker_is_present() {
        let seen = std::cell::RefCell::new(Vec::new());
        let found = find_last(b"..MARK....MARK", 4, b"MARK", 4, |_, end| {
            seen.borrow_mut().push(end);
            None::<()>
        });
        assert!(found.is_none());
        // Outermost first, and nothing offered for the bytes in between.
        assert_eq!(*seen.borrow(), vec![14, 6]);
    }

    #[test]
    fn test_stops_at_the_first_accepted_candidate() {
        assert_eq!(
            find_last(b"..MARK....MARK", 4, b"MARK", 4, accept_any),
            Some(14)
        );

        // A `locate` that rejects the outer one falls through to the inner.
        let end = find_last(b"..MARK....MARK", 4, b"MARK", 4, |_, end| {
            (end != 14).then_some(end)
        });
        assert_eq!(end, Some(6));
    }

    #[test]
    fn test_marker_back_positions_the_end_after_the_marker() {
        // A trailer whose marker starts 10 bytes before its end.
        assert_eq!(
            find_last(b"...MARKxxyyzz", 4, b"MARK", 10, accept_any),
            Some(13)
        );
    }

    #[test]
    fn test_candidate_running_past_the_end_of_the_file_is_dropped() {
        // The marker is there but the trailer it claims does not fit.
        assert!(find_last(b"...MARKxx", 4, b"MARK", 10, accept_any).is_none());
    }

    #[test]
    fn test_candidate_shorter_than_the_minimum_is_dropped() {
        assert!(find_last(b"MARK", 8, b"MARK", 4, accept_any).is_none());
    }

    #[test]
    fn test_file_without_the_marker_offers_nothing() {
        let called = std::cell::Cell::new(false);
        assert!(
            find_last(b"nothing to see here", 4, b"MARK", 4, |_, _| {
                called.set(true);
                None::<()>
            })
            .is_none()
        );
        assert!(!called.get());
    }
}
