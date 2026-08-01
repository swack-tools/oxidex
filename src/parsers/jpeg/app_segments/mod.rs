//! APP segment parsers for JPEG files
//!
//! This module contains parsers for various APP segments beyond the standard
//! EXIF, XMP, and IPTC segments.

pub mod app0;
pub mod app10_hdr;
pub mod app11_jpeg_hdr;
pub mod app12_olympus;
pub mod app12_picture_info;
pub mod app14_adobe;
pub mod app6;
pub mod app8_isothermal;
pub mod cbor;
pub mod infiray;
pub mod infiray_tables;
pub mod jumbf;
pub mod meta_app3;
pub mod photoshop;

// Re-export main parsing functions
pub use app0::parse_app0;
pub use app6::{parse_app6, parse_app6_ijpeg};
pub use app8_isothermal::parse_infiray_isothermal;
pub use app10_hdr::parse_app10_hdr;
pub use app11_jpeg_hdr::parse_app11_jpeg_hdr;
pub use app12_olympus::parse_app12_olympus;
pub use app12_picture_info::parse_app12_picture_info;
pub use app14_adobe::parse_app14_adobe;
pub use jumbf::parse_jumbf;
pub use meta_app3::parse_meta_app3;
pub use photoshop::parse_photoshop_irb;

/// Renders a number the way Perl stringifies an NV (`%.15g`), which is what
/// every ExifTool value ultimately goes through.
///
/// Re-exported from the shared formatter so every APP-segment parser and the
/// TIFF MakerNote parsers stringify through one implementation. A 32-bit
/// float widened to a double keeps its binary error, so 1.1f32 prints as
/// "1.10000002384186" here exactly as it does under ExifTool -- rounding it
/// to "1.1" would be a value ExifTool never reports.
pub(crate) use crate::core::formatters::perl_number;

#[cfg(test)]
mod tests {
    use super::perl_number;

    #[test]
    fn test_perl_number_matches_perl_stringification() {
        // `perl -e 'print unpack("f>", pack("H*","3f8ccccd"))'` -> 1.10000002384186
        assert_eq!(
            perl_number(f32::from_bits(0x3f8c_cccd) as f64),
            "1.10000002384186"
        );
        assert_eq!(perl_number(0.0), "0");
        assert_eq!(perl_number(-0.0), "0");
        assert_eq!(perl_number(1.0), "1");
        assert_eq!(perl_number(2.0), "2");
        assert_eq!(perl_number(0.5), "0.5");
        assert_eq!(perl_number(72.0), "72");
        assert_eq!(perl_number(80.0), "80");
        assert_eq!(perl_number(-20.0), "-20");
        assert_eq!(perl_number(-2.25), "-2.25");
    }

    /// Outside `%g`'s fixed-notation window Perl switches to an exponent, and
    /// its exponent always carries a sign and at least two digits. Reference:
    /// `perl -e 'printf("%.15g", $x)'` for each value below.
    #[test]
    fn test_perl_number_uses_perl_exponent_form() {
        assert_eq!(perl_number(1e20), "1e+20");
        assert_eq!(perl_number(1e-5), "1e-05");
        assert_eq!(perl_number(1.5e-7), "1.5e-07");
        assert_eq!(perl_number(123456789012345678.0), "1.23456789012346e+17");
        // The last value still inside the fixed-notation window.
        assert_eq!(perl_number(0.0001), "0.0001");
    }
}
