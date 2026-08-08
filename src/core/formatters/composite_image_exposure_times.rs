//! `CompositeImageExposureTimes` decoder for EXIF tag 0xA462.
//!
//! Unlike every other binary EXIF tag this crate decodes, ExifTool's own
//! conversion for this one is not a fixed byte layout: it is a Perl closure
//! (`Exif.pm` 13.59, lines 3079-3095) that walks the buffer with a byte
//! offset and switches field type mid-stream. There is nothing in
//! `src/exiftool_tables` to transcribe -- `find_table` only carries static
//! `ProcessBinaryData` layouts, and this RawConv is not one.
//!
//! # RawConv (Exif.pm:3079-3095) -- the byte layout
//!
//! ```text
//! RawConv => sub {
//!     my $val = shift;
//!     my @v;
//!     my $i = 0;
//!     for (;;) {
//!         if ($i == 56 or $i == 58) {
//!             last if $i + 2 > length $val;
//!             push @v, Get16u(\$val, $i);
//!             $i += 2;
//!         } else {
//!             last if $i + 8 > length $val;
//!             push @v, Image::ExifTool::GetRational64u(\$val, $i);
//!             $i += 8;
//!         }
//!     }
//!     return join ' ', @v;
//! },
//! ```
//!
//! Every field is an unsigned rational64 (4-byte numerator, 4-byte
//! denominator, [`GetRational64u`]) *except* the two `int16u` counts at byte
//! offsets 56 and 58, which is why the loop special-cases exactly those two
//! offsets. Per the tag's `Notes` (Exif.pm:3070-3077) the fields are, in
//! order: total exposure period, total/used exposure of all source images,
//! max/min exposure of source and used images (6 rationals), then the two
//! `int16u` counts (number of sequences, number of source images in the
//! sequence), then one rational per source image's own exposure time.
//!
//! [`GetRational64u`] (`ExifTool.pm:6114-6120`) returns the *string* `'inf'`
//! when the denominator is zero and the numerator is not, and `'undef'` when
//! both are zero -- not a number -- so this decoder emits those same two
//! literal tokens for that case rather than a computed float.
//!
//! # PrintConv (Exif.pm:3106-3116) -- the display step
//!
//! ```text
//! PrintConv => sub {
//!     my $val = shift;
//!     my @v = split ' ', $val;
//!     my $i;
//!     for ($i=0; ; ++$i) {
//!         last unless defined $v[$i];
//!         $v[$i] = PrintExposureTime($v[$i]) unless $i == 7 or $i == 8;
//!     }
//!     return join ' ', @v;
//! },
//! ```
//!
//! `$i` here is the position in the *value array*, not a byte offset -- but
//! because the RawConv above always reads exactly 7 rationals (56 bytes)
//! before it can ever reach the `int16u` branch, array position 7 is reached
//! if and only if the first `int16u` was just read, and position 8 if and
//! only if the second was. So "is this element a count" is fully determined
//! by which branch of the RawConv loop produced it, and this module fuses
//! both steps into the single loop below instead of round-tripping through
//! an intermediate space-separated string: whatever came from the `int16u`
//! branch is left as a bare integer, and everything else is passed through
//! [`print_exposure_time`] (this crate's port of `PrintExposureTime`,
//! Exif.pm:5701).
//!
//! # Byte order
//!
//! Both `Get16u` and `GetRational64u` read using the enclosing TIFF header's
//! recorded byte order, which this decoder takes as an explicit parameter --
//! Apple writes this tag big-endian, but nothing in the tag's own bytes
//! marks that, so the caller must supply it from the IFD it was read from.

use crate::core::formatters::exif_print_conv::print_exposure_time;
use crate::core::operations_helpers::{read_u16, read_u32};
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// Decodes and formats `CompositeImageExposureTimes` (EXIF 0xA462) exactly as
/// ExifTool 13.59 prints it.
///
/// `data` is the tag's raw `undef`-format bytes, exactly as stored in the
/// file (the 58-byte payload `-b` would extract). `byte_order` is the byte
/// order of the enclosing TIFF/EXIF header.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::composite_image_exposure_times::format_composite_image_exposure_times;
/// use oxidex::parsers::tiff::ifd_parser::ByteOrder;
///
/// // Apple_iPhone15Plus.jpg's CompositeImageExposureTimes, big-endian, 58 bytes.
/// // `exiftool -G -s -a` (pinned 13.59) prints exactly this string.
/// let data = [
///     0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0xf6, 0x7c, 0x00, 0x00, 0x48, 0x7f,
///     0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02, 0x24, 0x5c, 0x00, 0x05, 0x0a, 0x45,
///     0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02, 0x24, 0x5c, 0x00, 0x05, 0x0a, 0x45,
///     0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
/// ];
/// assert_eq!(
///     format_composite_image_exposure_times(&data, ByteOrder::BigEndian),
///     "0 3.4 0 0.4 0 0.4 0 0"
/// );
/// ```
pub fn format_composite_image_exposure_times(data: &[u8], byte_order: ByteOrder) -> String {
    let mut tokens: Vec<String> = Vec::new();
    let mut i = 0usize;

    loop {
        if i == 56 || i == 58 {
            // Exif.pm:3086 -- `push @v, Get16u(\$val, $i);` -- one of the two
            // "count" fields. Exif.pm:3112 excludes exactly these two array
            // positions from PrintExposureTime, so this branch never applies it.
            if i + 2 > data.len() {
                break;
            }
            let value = read_u16(&data[i..], byte_order);
            tokens.push(value.to_string());
            i += 2;
        } else {
            // Exif.pm:3090 -- `push @v, Image::ExifTool::GetRational64u(\$val, $i);`
            if i + 8 > data.len() {
                break;
            }
            let numerator = read_u32(&data[i..], byte_order);
            let denominator = read_u32(&data[i + 4..], byte_order);
            let token = if denominator == 0 {
                // ExifTool.pm:6118 -- `or return $ratNumer ? 'inf' : 'undef';`
                // Neither is a number, so PrintExposureTime's `IsFloat` guard
                // (Exif.pm:5704) passes them through unchanged.
                if numerator != 0 {
                    "inf".to_string()
                } else {
                    "undef".to_string()
                }
            } else {
                print_exposure_time(f64::from(numerator) / f64::from(denominator))
            };
            tokens.push(token);
            i += 8;
        }
    }

    tokens.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact 58 bytes `Apple_iPhone15Plus.jpg` stores for this tag
    /// (`exiftool -v3` hex dump, offsets 0a1e-0a57), and the exact string
    /// `exiftool -G -s -a` (pinned 13.59) prints for it:
    /// `0 3.4 0 0.4 0 0.4 0 0`.
    const IPHONE_15_PLUS_BYTES: [u8; 58] = [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0xf6, 0x7c, 0x00, 0x00, 0x48,
        0x7f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02, 0x24, 0x5c, 0x00, 0x05,
        0x0a, 0x45, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02, 0x24, 0x5c, 0x00,
        0x05, 0x0a, 0x45, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
    ];

    #[test]
    fn apple_iphone_15_plus_matches_pinned_exiftool() {
        assert_eq!(
            format_composite_image_exposure_times(&IPHONE_15_PLUS_BYTES, ByteOrder::BigEndian),
            "0 3.4 0 0.4 0 0.4 0 0"
        );
    }

    /// Feeding big-endian bytes through the little-endian reader must not
    /// coincidentally reproduce the same string -- if it did, this decoder
    /// would not actually be honouring `byte_order`.
    #[test]
    fn byte_order_is_not_ignored() {
        let wrong_order =
            format_composite_image_exposure_times(&IPHONE_15_PLUS_BYTES, ByteOrder::LittleEndian);
        assert_ne!(wrong_order, "0 3.4 0 0.4 0 0.4 0 0");
    }

    /// The same logical values as the iPhone sample, but re-encoded
    /// little-endian, must decode to the identical display string when read
    /// with `ByteOrder::LittleEndian` -- confirming the decoder truly follows
    /// the byte order parameter rather than assuming big-endian.
    #[test]
    fn little_endian_byte_order_is_honoured() {
        let mut le_bytes = Vec::with_capacity(58);
        // Same 7 rationals as the sample, each numerator/denominator re-spelled LE.
        let rationals: [(u32, u32); 7] = [
            (0, 1),
            (0x0000f67c, 0x0000487f),
            (0, 1),
            (0x0002245c, 0x00050a45),
            (0, 1),
            (0x0002245c, 0x00050a45),
            (0, 1),
        ];
        for (num, den) in rationals {
            le_bytes.extend_from_slice(&num.to_le_bytes());
            le_bytes.extend_from_slice(&den.to_le_bytes());
        }
        le_bytes.extend_from_slice(&0u16.to_le_bytes());
        assert_eq!(le_bytes.len(), 58);

        assert_eq!(
            format_composite_image_exposure_times(&le_bytes, ByteOrder::LittleEndian),
            "0 3.4 0 0.4 0 0.4 0 0"
        );
    }

    /// ExifTool.pm:6118: a zero denominator with a nonzero numerator reads as
    /// the literal string `'inf'`, which `PrintExposureTime`'s `IsFloat`
    /// guard (Exif.pm:5704) leaves untouched -- not a computed number.
    #[test]
    fn zero_denominator_nonzero_numerator_is_the_literal_inf() {
        let mut data = Vec::new();
        data.extend_from_slice(&42u32.to_be_bytes()); // numerator
        data.extend_from_slice(&0u32.to_be_bytes()); // denominator = 0
        assert_eq!(
            format_composite_image_exposure_times(&data, ByteOrder::BigEndian),
            "inf"
        );
    }

    /// ExifTool.pm:6118: a zero numerator over a zero denominator reads as
    /// the literal string `'undef'`.
    #[test]
    fn zero_over_zero_is_the_literal_undef() {
        let data = [0u8; 8]; // 0/0
        assert_eq!(
            format_composite_image_exposure_times(&data, ByteOrder::BigEndian),
            "undef"
        );
    }

    /// Sub-0.25s rationals take PrintExposureTime's reciprocal branch
    /// (Exif.pm:5705-5707), same as ExposureTime itself: `1/250`, not `0.004`.
    #[test]
    fn fast_exposure_uses_the_reciprocal_form() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&250u32.to_be_bytes());
        assert_eq!(
            format_composite_image_exposure_times(&data, ByteOrder::BigEndian),
            "1/250"
        );
    }

    /// Fewer than 8 bytes: the RawConv loop's very first bounds check fails
    /// (`last if $i + 8 > length $val` at $i=0), so `@v` is empty and the
    /// joined result is the empty string -- not a panic, not a fabricated
    /// element.
    #[test]
    fn truncated_value_shorter_than_one_field_is_empty() {
        assert_eq!(
            format_composite_image_exposure_times(&[0, 0, 0], ByteOrder::BigEndian),
            ""
        );
        assert_eq!(
            format_composite_image_exposure_times(&[], ByteOrder::BigEndian),
            ""
        );
    }

    /// A value with data past both `int16u` counts (i.e. more than 60 bytes)
    /// resumes reading rationals at $i=60 -- Exif.pm's "10-N. Exposure times
    /// of each source image" -- and those trailing elements go back through
    /// PrintExposureTime like the first seven, since array position 9 is
    /// neither 7 nor 8.
    #[test]
    fn data_past_the_two_counts_resumes_as_exposure_times() {
        let mut data = Vec::new();
        for _ in 0..7 {
            data.extend_from_slice(&0u32.to_be_bytes());
            data.extend_from_slice(&1u32.to_be_bytes());
        }
        data.extend_from_slice(&2u16.to_be_bytes()); // count field 1 (array idx 7)
        data.extend_from_slice(&3u16.to_be_bytes()); // count field 2 (array idx 8)
        // One more source-image exposure time: 1/2 s.
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&2u32.to_be_bytes());
        assert_eq!(data.len(), 68);

        assert_eq!(
            format_composite_image_exposure_times(&data, ByteOrder::BigEndian),
            "0 0 0 0 0 0 0 2 3 0.5"
        );
    }

    /// A value that ends exactly at $i=56 (56 bytes: 7 full rationals, no
    /// room for even the first `int16u`) never enters the special-case
    /// branch at all -- both counts are simply absent, matching the
    /// "11 or more values" note describing a variable-length tail.
    #[test]
    fn exactly_seven_rationals_and_nothing_else() {
        let mut data = Vec::new();
        // 3/10 = 0.3, comfortably above the 0.25001 reciprocal-branch cutoff
        // (Exif.pm:5705), so this exercises the plain "%.1f" branch instead.
        for _ in 0..7 {
            data.extend_from_slice(&3u32.to_be_bytes());
            data.extend_from_slice(&10u32.to_be_bytes());
        }
        assert_eq!(data.len(), 56);
        assert_eq!(
            format_composite_image_exposure_times(&data, ByteOrder::BigEndian),
            "0.3 0.3 0.3 0.3 0.3 0.3 0.3"
        );
    }
}
