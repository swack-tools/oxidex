//! The `OTHER` fallbacks of `%FujiFilm`'s settings tables.
//!
//! ExifTool writes a `PrintConv` as a hash plus, when the hash cannot cover the
//! range, an `OTHER => sub {...}` run for anything unlisted. Those subs are
//! anonymous, so nothing names them and nothing quotes them -- which is why
//! `dump_tables.pl` deparses them and `codegen_subdirs.py` binds a translation
//! to the exact deparsed body. This module holds the translations; each carries
//! the Perl it was written against, and if that Perl changes upstream the
//! generator stops instead of leaving one of these behind a real tag name.
//!
//! Only the read direction. Every one of these subs also has an `$inv` branch,
//! which is ExifTool's writer inverting the string back to a number.

/// `OTHER => sub { return $_[0] }` (FujiFilm.pm:1089, `AFAreaPointSize`).
///
/// Zero means `n/a` and the hash says so; any other size prints as itself.
pub(super) fn identity(value: i64) -> String {
    value.to_string()
}

/// `AFAreaZoneSize`, FujiFilm.pm:1097-1108:
///
/// ```text
/// ($w, $h) = ($val & 0x0f, $val >> 5);
/// return "$w x $h";
/// ```
///
/// `$val` here is already masked and shifted (`Mask => 0xff0000`), so the two
/// nibbles are the zone's width and height. The shifts overlap deliberately --
/// `>> 5`, not `>> 4` -- which is why this cannot be written as a lookup.
pub(super) fn zone_size(value: i64) -> String {
    let w = value & 0x0f;
    let h = value >> 5;
    format!("{w} x {h}")
}

/// `AF-CSetting`, FujiFilm.pm:1131-1135:
/// `return sprintf 'Set 6 (custom 0x%.3x)', $val;`
///
/// The five in-camera presets are in the hash; anything else is a user-defined
/// combination and ExifTool prints its raw code.
pub(super) fn custom_afc_set(value: i64) -> String {
    format!("Set 6 (custom 0x{value:03x})")
}

/// `DriveSpeed`, FujiFilm.pm:1180-1186: `return "$val fps" unless $inv;`
pub(super) fn fps(value: i64) -> String {
    format!("{value} fps")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_size_splits_on_five_bits_not_four() {
        // The one corpus body that reports a zone (X-H2S) stores 0x63: the low
        // nibble is the width and `>> 5` is the height, so both are 3. A `>> 4`
        // would print "3 x 6".
        assert_eq!(zone_size(0x63), "3 x 3");
    }

    #[test]
    fn custom_afc_set_pads_to_three_hex_digits() {
        assert_eq!(custom_afc_set(0x12), "Set 6 (custom 0x012)");
        assert_eq!(custom_afc_set(0x123), "Set 6 (custom 0x123)");
    }

    #[test]
    fn identity_and_fps_render_the_number() {
        assert_eq!(identity(4), "4");
        assert_eq!(fps(20), "20 fps");
    }
}
