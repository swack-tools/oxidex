//! Portable FloatMap (PFM) image format parser
//!
//! ExifTool source: `Image::ExifTool::Other::PFM` / `ProcessPFM2` in
//! `lib/Image/ExifTool/Other.pm` (see
//! <http://www.pauldebevec.com/Research/HDR/PFM/> for the file spec).
//!
//! A PFM file starts with a small ASCII header:
//! ```text
//! P[Ff]\n<width> <height>\n<scale>\n
//! ```
//! followed by raw big/little-endian IEEE-754 float pixel data. ExifTool only
//! extracts four tags from the header: `ColorSpace`, `ImageWidth`,
//! `ImageHeight` and `ByteOrder` (derived from the sign of `<scale>`).

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// Matches ExifTool's magic regex: `^P[Ff]\x0a\d+ \d+\x0a[-+0-9.]+\x0a`
pub fn looks_like_pfm(bytes: &[u8]) -> bool {
    parse_header(bytes).is_some()
}

/// Parsed PFM header fields: (color_space, width, height, scale)
struct PfmHeader {
    color_space: &'static str,
    width: u32,
    height: u32,
    scale: f64,
}

/// Parses the ASCII PFM header from the start of `bytes`, mirroring
/// ExifTool's magic regex exactly (anchored at offset 0, requires all three
/// newline-terminated fields to be present).
fn parse_header(bytes: &[u8]) -> Option<PfmHeader> {
    // "P" + [Ff]
    if bytes.len() < 3 || bytes[0] != b'P' {
        return None;
    }
    let color_space = match bytes[1] {
        b'F' => "PF",
        b'f' => "Pf",
        _ => return None,
    };
    if bytes[2] != b'\n' {
        return None;
    }
    let rest = &bytes[3..];

    // "<width> <height>\n"
    let nl1 = rest.iter().position(|&b| b == b'\n')?;
    let dims_str = std::str::from_utf8(&rest[..nl1]).ok()?;
    let mut parts = dims_str.split(' ');
    let width_str = parts.next()?;
    let height_str = parts.next()?;
    if parts.next().is_some() || width_str.is_empty() || height_str.is_empty() {
        return None;
    }
    if !width_str.bytes().all(|b| b.is_ascii_digit())
        || !height_str.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let width: u32 = width_str.parse().ok()?;
    let height: u32 = height_str.parse().ok()?;

    // "<scale>\n" -- ExifTool's char class is [-+0-9.]+
    let after_dims = &rest[nl1 + 1..];
    let nl2 = after_dims.iter().position(|&b| b == b'\n')?;
    let scale_str = std::str::from_utf8(&after_dims[..nl2]).ok()?;
    if scale_str.is_empty()
        || !scale_str
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'-' || b == b'+' || b == b'.')
    {
        return None;
    }
    let scale: f64 = scale_str.parse().ok()?;

    Some(PfmHeader {
        color_space,
        width,
        height,
        scale,
    })
}

/// Parser for Portable FloatMap (PFM) images.
pub struct PFMParser;

impl PFMParser {
    /// Verifies the PFM file signature/header against ExifTool's magic regex.
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        let probe_len = reader.size().min(256) as usize;
        if probe_len == 0 {
            return Ok(false);
        }
        let probe = reader.read(0, probe_len)?;
        Ok(looks_like_pfm(probe))
    }
}

impl FormatParser for PFMParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let probe_len = reader.size().min(256) as usize;
        let probe = reader.read(0, probe_len)?;
        let header =
            parse_header(probe).ok_or_else(|| ExifToolError::parse_error("Invalid PFM header"))?;

        let mut metadata = MetadataMap::new();
        // A name for `normalize_identity_tags` to fall back on, nothing more:
        // it is dropped once `File:FileType` carries a real value, which for
        // `.pfm` it always does.
        //
        // The MIME type used to be hardcoded beside it, because ExifTool
        // hardcodes it -- `%mimeType` has no PFM entry, since the extension is
        // shared with Windows Printer Font Metrics, which takes the Font
        // module's `application/x-font-type1`, so `ProcessPFM2` supplies the
        // image MIME type itself (Other.pm:44):
        //
        // ```text
        //     $et->SetFileType('PFM', 'image/x-pfm');
        // ```
        //
        // `crate::filetype::refine` already draws exactly that distinction for
        // the `File:`-grouped tags, keyed on the magic number rather than the
        // extension, and `normalize_identity_tags` never promotes a parser's
        // MIMEType. Writing it here only produced a second, ungrouped copy of an
        // answer the tables had already given correctly.
        metadata.insert("FileType".to_string(), TagValue::String("PFM".to_string()));
        metadata.insert(
            "ColorSpace".to_string(),
            TagValue::String(
                match header.color_space {
                    "PF" => "RGB",
                    "Pf" => "Monochrome",
                    other => other,
                }
                .to_string(),
            ),
        );
        metadata.insert(
            "ImageWidth".to_string(),
            TagValue::Integer(header.width as i64),
        );
        metadata.insert(
            "ImageHeight".to_string(),
            TagValue::Integer(header.height as i64),
        );
        metadata.insert(
            "ByteOrder".to_string(),
            TagValue::String(
                if header.scale > 0.0 {
                    "Big-endian"
                } else {
                    "Little-endian"
                }
                .to_string(),
            ),
        );

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::PFM)
    }
}

/// Parses metadata from a Portable FloatMap (PFM) file.
///
/// This is a convenience wrapper around [`PFMParser`] that provides a
/// functional API matching the other format parsers.
pub fn parse_pfm_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = PFMParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    fn make_reader(header: &str, extra_bytes: usize) -> TestReader {
        let mut data = header.as_bytes().to_vec();
        data.extend(std::iter::repeat_n(0u8, extra_bytes));
        TestReader::new(data)
    }

    #[test]
    fn parses_color_pfm_big_endian() {
        let reader = make_reader("PF\n4 2\n1.0\n", 4 * 2 * 3 * 4);
        let meta = parse_pfm_metadata(&reader).expect("parse should succeed");
        assert_eq!(
            meta.get("ColorSpace"),
            Some(&TagValue::String("RGB".to_string()))
        );
        assert_eq!(meta.get("ImageWidth"), Some(&TagValue::Integer(4)));
        assert_eq!(meta.get("ImageHeight"), Some(&TagValue::Integer(2)));
        assert_eq!(
            meta.get("ByteOrder"),
            Some(&TagValue::String("Big-endian".to_string()))
        );
    }

    #[test]
    fn parses_monochrome_pfm_little_endian() {
        let reader = make_reader("Pf\n8 3\n-1.0\n", 8 * 3 * 4);
        let meta = parse_pfm_metadata(&reader).expect("parse should succeed");
        assert_eq!(
            meta.get("ColorSpace"),
            Some(&TagValue::String("Monochrome".to_string()))
        );
        assert_eq!(meta.get("ImageWidth"), Some(&TagValue::Integer(8)));
        assert_eq!(meta.get("ImageHeight"), Some(&TagValue::Integer(3)));
        assert_eq!(
            meta.get("ByteOrder"),
            Some(&TagValue::String("Little-endian".to_string()))
        );
    }

    #[test]
    fn reports_the_image_file_type_not_the_font_one() {
        // The `.pfm` extension is shared with Windows Printer Font Metrics,
        // which ExifTool reports as the same FileType `PFM` but with MIMEType
        // `application/x-font-type1`.
        let reader = make_reader("PF\n4 2\n1.0\n", 4 * 2 * 3 * 4);
        let meta = parse_pfm_metadata(&reader).expect("parse should succeed");
        assert_eq!(
            meta.get("FileType"),
            Some(&TagValue::String("PFM".to_string()))
        );
        // The MIME half of that split belongs to `filetype::refine`, and is
        // pinned for both the FloatMap and the Printer Font Metrics case by
        // `filetype::tests::pfm_is_two_formats_told_apart_by_the_header`. This
        // parser must not answer it: `normalize_identity_tags` never promotes a
        // parser's MIMEType, so a value written here could only be a duplicate.
        assert!(meta.get("MIMEType").is_none());
    }

    #[test]
    fn rejects_non_pfm_header() {
        let reader = make_reader("P6\n4 2\n255\n", 10);
        assert!(!PFMParser::verify_signature(&reader).unwrap());
    }
}
