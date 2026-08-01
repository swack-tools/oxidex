//! Where a MakerNote sits, and how far a decoder may read from it.
//!
//! # The defect this exists to fix
//!
//! A MakerNote is one EXIF entry (0x927C) with a declared byte count, but the
//! offsets *inside* it are frequently measured from the enclosing TIFF header
//! rather than from the payload, and they routinely address bytes past the
//! declared count. Handing a decoder only the declared block therefore makes
//! those values structurally unreachable, however correct the decoder is:
//!
//! * `Nikon/NikonCoolpixS8200.jpg` declares 2219 MakerNote bytes at TIFF
//!   offset 0x0384, so the block ends at 0x0C2F. `NEFBitDepth` (0x0E22,
//!   `int16u[4]`) stores its value at 0x0C2B and needs eight bytes -- the last
//!   four sit outside the block. 43 of the corpus's Nikon JPEGs are in this
//!   position.
//! * `NikonCOOLSCAN_VED.jpg`'s NikonScan IFD sits past the end of an 88-byte
//!   MakerNote value.
//! * A Sigma entry's value offset addresses the enclosing TIFF header outright,
//!   so nothing longer than four bytes -- which on a real file is every Sigma
//!   value, all of them strings -- can be read from the payload alone.
//!
//! ExifTool has no such limit: `Exif.pm`'s `ProcessExif` resolves a value
//! against the whole loaded data block and falls back to seeking in the file
//! when `$valuePtr + $size > $dataLen` (Exif.pm:6551). The block is what a
//! MakerNote's offsets are measured against, not the entry's own extent.
//!
//! # What this carries
//!
//! [`MakerNoteContext`] pairs the bytes with the base they are measured from,
//! because more bytes on their own do not make an offset meaningful:
//!
//! * [`MakerNoteContext::tiff`] -- the enclosing TIFF block, index 0 being the
//!   TIFF header, which is the base a TIFF-relative value offset counts from.
//! * [`MakerNoteContext::payload`] -- the declared MakerNote block, byte for
//!   byte what the `MakerNoteParser` trait has always received.
//! * [`MakerNoteContext::window`] -- the payload's start extended to the end of
//!   the enclosing block. Same index 0 as `payload`, so a decoder's existing
//!   offset arithmetic is unchanged; only its reach grows.
//! * [`MakerNoteContext::tiff_base`] -- the absolute file offset of `tiff[0]`,
//!   which `IsOffset` tags (`PreviewImageStart`, `ThumbnailOffset`) are
//!   absolutised with.
//!
//! # Bounds
//!
//! Widening a window is how a decoder starts reading unrelated file content and
//! emitting confident nonsense, so:
//!
//! * every accessor is bounds-checked and returns a slice that cannot leave the
//!   enclosing block;
//! * the enclosing block is the caller's TIFF/EXIF block, *not* the file. That
//!   is `$dataLen` in ExifTool's terms -- strictly narrower than the RAF
//!   fallback ExifTool itself allows, so this can never read bytes ExifTool
//!   would not have;
//! * [`value_overlaps_directory`] ports ExifTool's "Suspicious MakerNotes
//!   offset" test (Exif.pm:6549, `$valuePtr < $dirEnd and $valuePtr+$size >
//!   $dirStart`) so an offset that lands back in the IFD's own entry list is
//!   refused rather than followed; and
//! * a context built by [`MakerNoteContext::detached`] -- for a caller that
//!   holds only the MakerNote bytes -- reports `window() == payload()`, so
//!   nothing widens where there is no verified enclosing block to widen into.

/// The bytes a MakerNote decoder may read, and the base its offsets count from.
///
/// See the module documentation for why both halves are needed.
#[derive(Clone, Copy, Debug)]
pub struct MakerNoteContext<'a> {
    /// The enclosing TIFF block. Index 0 is the TIFF header ("II"/"MM"), the
    /// base MakerNote value offsets are measured from.
    tiff: &'a [u8],
    /// Offset of the MakerNote payload within `tiff`.
    value_offset: usize,
    /// The MakerNote entry's declared byte count, clamped to `tiff`.
    value_len: usize,
    /// Absolute file offset of `tiff[0]`.
    tiff_base: u64,
    /// Whether `tiff` is a real enclosing block or just the payload again.
    located: bool,
}

impl<'a> MakerNoteContext<'a> {
    /// Builds a context for a MakerNote at `value_offset` inside `tiff`.
    ///
    /// `value_len` is the EXIF entry's declared byte count and `tiff_base` the
    /// absolute file offset of the TIFF header. Both `value_offset` and
    /// `value_len` are clamped to `tiff`, so a corrupt entry cannot produce a
    /// context that reads outside the block.
    pub fn in_tiff(tiff: &'a [u8], value_offset: usize, value_len: usize, tiff_base: u64) -> Self {
        let value_offset = value_offset.min(tiff.len());
        let value_len = value_len.min(tiff.len() - value_offset);
        Self {
            tiff,
            value_offset,
            value_len,
            tiff_base,
            located: true,
        }
    }

    /// Builds a context for a caller that holds only the MakerNote bytes.
    ///
    /// There is no verified enclosing block, so `window()` equals `payload()`
    /// and nothing widens: this is exactly the reach every decoder had before
    /// contexts existed. Used by the AVI and RAW entry points, which are handed
    /// a detached copy of the value.
    pub fn detached(payload: &'a [u8]) -> Self {
        Self {
            tiff: payload,
            value_offset: 0,
            value_len: payload.len(),
            tiff_base: 0,
            located: false,
        }
    }

    /// The enclosing TIFF block, index 0 being the TIFF header.
    ///
    /// This is what a value offset measured from the TIFF header (Sigma's, for
    /// one) addresses directly.
    pub fn tiff(&self) -> &'a [u8] {
        self.tiff
    }

    /// The declared MakerNote block: `tiff[value_offset .. +value_len]`.
    ///
    /// Byte for byte what `MakerNoteParser::parse` has always received.
    pub fn payload(&self) -> &'a [u8] {
        &self.tiff[self.value_offset..self.value_offset + self.value_len]
    }

    /// The payload's start, extended to the end of the enclosing TIFF block.
    ///
    /// Index 0 is the same byte as `payload()`'s, so a decoder's offset
    /// arithmetic needs no adjustment -- only its reach changes. Equal to
    /// `payload()` for a [`detached`](Self::detached) context.
    pub fn window(&self) -> &'a [u8] {
        &self.tiff[self.value_offset..]
    }

    /// Where the payload starts inside [`tiff`](Self::tiff).
    ///
    /// Always a valid index into `tiff()`; for a detached context that is 0,
    /// because the payload is all there is. Use
    /// [`payload_tiff_offset`](Self::payload_tiff_offset) when the number is
    /// being subtracted from a TIFF-relative offset rather than used as an
    /// index.
    pub fn payload_offset(&self) -> usize {
        self.value_offset
    }

    /// The payload's distance from the TIFF header, when that is actually
    /// known.
    ///
    /// A decoder that resolves an entry as `payload[value_offset - base]`
    /// needs the real base or nothing: a detached context returning 0 would
    /// claim the payload begins at the TIFF header, and every such subtraction
    /// would land somewhere plausible and wrong.
    pub fn payload_tiff_offset(&self) -> Option<u32> {
        self.located
            .then(|| u32::try_from(self.value_offset).ok())
            .flatten()
    }

    /// Absolute file offset of `tiff[0]`, for absolutising `IsOffset` tags.
    pub fn tiff_base(&self) -> u64 {
        self.tiff_base
    }

    /// Whether the enclosing TIFF block is known, rather than the payload
    /// standing alone.
    pub fn is_located(&self) -> bool {
        self.located
    }

    /// Whether [`window`](Self::window) actually reaches past the declared
    /// block. False for a detached context, and for a MakerNote that already
    /// ends at the end of its enclosing block.
    pub fn is_widened(&self) -> bool {
        self.window().len() > self.value_len
    }
}

/// ExifTool's "Suspicious MakerNotes offset" test.
///
/// `Exif.pm:6549` marks an entry suspect when its value overlaps the directory
/// that declared it -- `$valuePtr < $dirEnd and $valuePtr + $size > $dirStart`
/// -- and `Exif.pm:6675` then drops the tag rather than reporting a value read
/// out of the entry list (or out of the MakerNote's own TIFF header). All four
/// arguments are offsets in the same coordinate space, whichever one the caller
/// resolves its entries in.
///
/// Honouring such an offset yields a confident wrong number: NikonD3000's
/// `ExposureBracketValue` reads back as +162113114 from the MakerNote's own
/// "MM\0*" header when it is followed.
pub fn value_overlaps_directory(
    value_start: usize,
    value_size: usize,
    dir_start: usize,
    dir_end: usize,
) -> bool {
    value_start < dir_end && value_start.saturating_add(value_size) > dir_start
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in TIFF block: 0..8 header, 8..24 the "MakerNote", 24..40 the
    /// bytes past its declared end that ExifTool can still reach.
    fn block() -> Vec<u8> {
        (0u8..40).collect()
    }

    #[test]
    fn payload_is_exactly_the_declared_block() {
        let tiff = block();
        let ctx = MakerNoteContext::in_tiff(&tiff, 8, 16, 100);
        assert_eq!(ctx.payload(), &tiff[8..24]);
        assert_eq!(ctx.payload().len(), 16);
        assert_eq!(ctx.payload_offset(), 8);
        assert_eq!(ctx.tiff_base(), 100);
    }

    #[test]
    fn window_starts_where_payload_starts_and_reaches_the_block_end() {
        let tiff = block();
        let ctx = MakerNoteContext::in_tiff(&tiff, 8, 16, 0);
        assert_eq!(ctx.window()[0], ctx.payload()[0]);
        assert_eq!(ctx.window(), &tiff[8..]);
        assert!(ctx.is_widened());
    }

    #[test]
    fn window_never_leaves_the_enclosing_block() {
        let tiff = block();
        let ctx = MakerNoteContext::in_tiff(&tiff, 8, 16, 0);
        assert_eq!(ctx.window().len(), tiff.len() - 8);
    }

    #[test]
    fn a_declared_length_past_the_block_end_is_clamped() {
        let tiff = block();
        let ctx = MakerNoteContext::in_tiff(&tiff, 32, 4096, 0);
        assert_eq!(ctx.payload(), &tiff[32..]);
        assert!(!ctx.is_widened());
    }

    #[test]
    fn an_offset_past_the_block_end_yields_an_empty_context() {
        let tiff = block();
        let ctx = MakerNoteContext::in_tiff(&tiff, 4096, 16, 0);
        assert!(ctx.payload().is_empty());
        assert!(ctx.window().is_empty());
        assert!(!ctx.is_widened());
    }

    #[test]
    fn a_detached_context_does_not_widen() {
        let payload = vec![1u8, 2, 3, 4];
        let ctx = MakerNoteContext::detached(&payload);
        assert_eq!(ctx.payload(), &payload[..]);
        assert_eq!(ctx.window(), ctx.payload());
        assert!(!ctx.is_widened());
        assert_eq!(ctx.tiff_base(), 0);
    }

    #[test]
    fn suspicious_offsets_are_the_ones_that_overlap_the_entry_list() {
        // A directory of 2 entries at 10: 10 .. 10 + 2 + 24 + 4 = 40.
        let (dir_start, dir_end) = (10, 40);
        // A value in front of the entry list, reaching into it.
        assert!(value_overlaps_directory(8, 8, dir_start, dir_end));
        // A value wholly inside the entry list -- the NikonD3000 case.
        assert!(value_overlaps_directory(12, 4, dir_start, dir_end));
        // A value after the entry list: the normal case.
        assert!(!value_overlaps_directory(40, 8, dir_start, dir_end));
        // A value wholly in front of the directory, not touching it.
        assert!(!value_overlaps_directory(0, 8, dir_start, dir_end));
    }
}
