//! JPEG-HDR APP11 segment parser
//!
//! JPEG-HDR is a backward-compatible extension to JPEG for storing High Dynamic
//! Range images: a tone-mapped base image in standard JPEG form, plus a ratio
//! image and its reconstruction parameters carried in an APP11 segment.
//!
//! # Segment structure
//!
//! The APP11 payload is **ASCII text**, not a packed binary record. ExifTool
//! dispatches on a seven-byte identifier and then scans the text for
//! `key=value` pairs:
//!
//! ```text
//! HDR_RI ver=11
//!  ln0=0.122262, ln1=2.634655, s2n=2.269635e+03 alp=1.000000 bet=1.000000 cor=0
//! ~\0<ratio image bytes>
//! ```
//!
//! ExifTool, JPEG.pm:
//!
//! ```text
//!     APP11 => [{
//!         Name => 'JPEG-HDR',
//!         Condition => '$$valPt =~ /^HDR_RI /',
//!         SubDirectory => { TagTable => 'Image::ExifTool::JPEG::HDR' },
//!       }, {
//! ```
//!
//! ```text
//! sub ProcessJPEG_HDR($$$)
//! {
//!     my ($et, $dirInfo, $tagTablePtr) = @_;
//!     my $dataPt = $$dirInfo{DataPt};
//!     $$dataPt =~ /~\0/g or $et->Warn('Unrecognized JPEG-HDR format'), return 0;
//!     my $pos = pos $$dataPt;
//!     my $meta = substr($$dataPt, 7, $pos-9);
//!     $et->VerboseDir('APP11 JPEG-HDR', undef, length $$dataPt);
//!     while ($meta =~ /(\w+)=([^,\s]*)/g) {
//!         my ($tag, $val) = ($1, $2);
//!         AddTagToTable($tagTablePtr, $tag) unless $$tagTablePtr{$tag};
//!         $et->HandleTag($tagTablePtr, $tag, $val);
//!     }
//!     $et->HandleTag($tagTablePtr, 'RatioImage', substr($$dataPt, $pos));
//!     return 1;
//! }
//! ```
//!
//! # Values are verbatim
//!
//! ```text
//! %Image::ExifTool::JPEG::HDR = (
//!     GROUPS => { 0 => 'APP11', 1 => 'JPEG-HDR', 2 => 'Image' },
//!     PROCESS_PROC => \&ProcessJPEG_HDR,
//!     TAG_PREFIX => '', # (no prefix for unknown tags)
//!     NOTES => 'Information extracted from APP11 of a JPEG-HDR image.',
//!     ver => 'JPEG-HDRVersion',
//!     # (need names for the next 3 tags)
//!     ln0 => { Description => 'Ln0' },
//!     ln1 => { Description => 'Ln1' },
//!     s2n => { Description => 'S2n' },
//!     alp => { Name => 'Alpha' }, # (Alpha/Beta are saturation parameters)
//!     bet => { Name => 'Beta' },
//!     cor => { Name => 'CorrectionMethod' },
//!     RatioImage => {
//!         Groups => { 2 => 'Preview' },
//!         Notes => 'the embedded JPEG-compressed ratio image',
//!         Binary => 1,
//!     },
//! );
//! ```
//!
//! Not one entry carries a `ValueConv` or a `PrintConv`, so every value is
//! reported exactly as it was written in the segment. `s2n=2.269635e+03` is
//! reported as `2.269635e+03`, not as `2269.635`, and `alp=1.000000` keeps all
//! six decimals. Storing these as numbers and re-rendering them is lossy, so
//! this parser keeps the source text.
//!
//! # ExifTool Compatibility
//!
//! Tags are emitted with the `APP11` family prefix (ExifTool's family-0 group
//! for this table): `APP11:Alpha`, `APP11:JPEG-HDRVersion`, and so on.
//!
//! # References
//!
//! - Ward, G. & Simmons, M. (2004). "JPEG-HDR: A Backwards-Compatible, High
//!   Dynamic Range Extension to JPEG"

use crate::core::{MetadataMap, TagValue};
use crate::error::Result;

/// APP11 JPEG-HDR identifier, including the trailing space.
///
/// ExifTool's dispatch condition is `$$valPt =~ /^HDR_RI /`, and
/// `ProcessJPEG_HDR` then starts its scan at `substr($$dataPt, 7, ...)` — i.e.
/// just past these seven bytes.
const HDR_RI_IDENTIFIER: &[u8] = b"HDR_RI ";

/// Terminator that separates the ASCII parameter block from the ratio image.
///
/// `ProcessJPEG_HDR` locates it with `$$dataPt =~ /~\0/g` and bails out with
/// "Unrecognized JPEG-HDR format" when it is absent.
const HDR_META_TERMINATOR: &[u8] = b"~\0";

/// Correction method identifiers used in JPEG-HDR tone mapping
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionMethod {
    /// No correction applied
    None,
    /// Multiplicative correction
    Multiplicative,
    /// Additive correction
    Additive,
    /// Logarithmic correction (most common for HDR)
    Logarithmic,
    /// Gamma correction
    Gamma,
    /// Unknown or proprietary method
    Unknown(u8),
}

impl CorrectionMethod {
    /// Converts a byte value to a CorrectionMethod enum variant
    fn from_byte(value: u8) -> Self {
        match value {
            0 => CorrectionMethod::None,
            1 => CorrectionMethod::Multiplicative,
            2 => CorrectionMethod::Additive,
            3 => CorrectionMethod::Logarithmic,
            4 => CorrectionMethod::Gamma,
            other => CorrectionMethod::Unknown(other),
        }
    }

    /// Returns a human-readable description of the correction method
    fn description(&self) -> String {
        match self {
            CorrectionMethod::None => "None".to_string(),
            CorrectionMethod::Multiplicative => "Multiplicative".to_string(),
            CorrectionMethod::Additive => "Additive".to_string(),
            CorrectionMethod::Logarithmic => "Logarithmic".to_string(),
            CorrectionMethod::Gamma => "Gamma".to_string(),
            CorrectionMethod::Unknown(v) => format!("Unknown ({})", v),
        }
    }
}

/// Parsed JPEG-HDR parameters
///
/// This structure holds all the HDR-related parameters extracted from an APP11
/// segment, decoded from the textual values for callers that want numbers.
#[derive(Debug, Clone, Default)]
pub struct JpegHdrParameters {
    /// JPEG-HDR format version (major.minor)
    pub version: Option<(u8, u8)>,
    /// Alpha saturation parameter
    pub alpha: Option<f32>,
    /// Beta saturation parameter
    pub beta: Option<f32>,
    /// Method used for HDR correction/reconstruction
    pub correction_method: Option<CorrectionMethod>,
    /// Lower luminance bound (Ln0) in log space
    pub ln0: Option<f32>,
    /// Upper luminance bound (Ln1) in log space
    pub ln1: Option<f32>,
    /// Signal-to-noise ratio estimate
    pub s2n: Option<f32>,
    /// Size of ratio image data in bytes (if present)
    pub ratio_image_size: Option<usize>,
    /// Indicates if this segment contains ratio image data
    pub has_ratio_image: bool,
}

/// Maps a JPEG-HDR text key to the tag name ExifTool reports.
///
/// The table names `ver`, `ln0`, `ln1`, `s2n`, `alp`, `bet` and `cor`
/// explicitly; `TAG_PREFIX => ''` means any other key is added on the fly, and
/// `AddTagToTable` runs it through `MakeTagName`:
///
/// ```text
/// sub MakeTagName($)
/// {
///     my $name = shift;
///     $name =~ tr/-_a-zA-Z0-9//dc;    # remove illegal characters
///     $name = ucfirst $name;          # capitalize first letter
///     # must at least 2 characters long and not start with - or 0-9-
///     $name = "Tag$name" if length($name) < 2 or $name =~ /^[-0-9]/;
///     return $name;
/// }
/// ```
fn hdr_tag_name(key: &str) -> String {
    match key {
        "ver" => "JPEG-HDRVersion".to_string(),
        "ln0" => "Ln0".to_string(),
        "ln1" => "Ln1".to_string(),
        "s2n" => "S2n".to_string(),
        "alp" => "Alpha".to_string(),
        "bet" => "Beta".to_string(),
        "cor" => "CorrectionMethod".to_string(),
        other => {
            let cleaned: String = other
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            let mut chars = cleaned.chars();
            let name = match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            };
            let needs_prefix = name.chars().count() < 2
                || name.starts_with(|c: char| c == '-' || c.is_ascii_digit());
            if needs_prefix {
                format!("Tag{name}")
            } else {
                name
            }
        }
    }
}

/// Scans the parameter text for `key=value` pairs.
///
/// Mirrors `while ($meta =~ /(\w+)=([^,\s]*)/g)`: a key is a run of
/// `[A-Za-z0-9_]`, and the value runs to the next comma or whitespace (and may
/// be empty).
fn scan_hdr_pairs(meta: &str) -> Vec<(&str, &str)> {
    let bytes = meta.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut pairs = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if !is_word(bytes[i]) {
            i += 1;
            continue;
        }
        let key_start = i;
        while i < bytes.len() && is_word(bytes[i]) {
            i += 1;
        }
        // A word run only yields a tag when it is immediately followed by '='.
        // Perl's engine would then retry from one past the run's start, but a
        // `\w+` that is not followed by '=' can never produce a match starting
        // inside itself either, so advancing past it is equivalent.
        if i >= bytes.len() || bytes[i] != b'=' {
            continue;
        }
        let key_end = i;
        i += 1; // skip '='
        let val_start = i;
        while i < bytes.len() && bytes[i] != b',' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        pairs.push((&meta[key_start..key_end], &meta[val_start..i]));
    }

    pairs
}

/// Parses a JPEG-HDR APP11 segment and returns extracted metadata.
///
/// # Arguments
///
/// * `data` - Raw APP11 segment data (excluding the APP11 marker and length bytes)
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully parsed metadata with `APP11:` tags
/// * `Err` - If the segment is not a JPEG-HDR segment, or has no `~\0` marker
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::jpeg::app_segments::app11_jpeg_hdr::parse_app11_jpeg_hdr;
///
/// let metadata = parse_app11_jpeg_hdr(segment_data)?;
/// if let Some(version) = metadata.get_string("APP11:JPEG-HDRVersion") {
///     println!("JPEG-HDR Version: {}", version);
/// }
/// ```
pub fn parse_app11_jpeg_hdr(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // `Condition => '$$valPt =~ /^HDR_RI /'`
    if data.len() < HDR_RI_IDENTIFIER.len() || &data[..HDR_RI_IDENTIFIER.len()] != HDR_RI_IDENTIFIER
    {
        return Err(crate::error::ExifToolError::parse_error(
            "Not a JPEG-HDR segment (expected \"HDR_RI \" identifier)",
        ));
    }

    // `$$dataPt =~ /~\0/g or $et->Warn('Unrecognized JPEG-HDR format'), return 0;`
    let Some(term) = data
        .windows(HDR_META_TERMINATOR.len())
        .position(|w| w == HDR_META_TERMINATOR)
    else {
        return Err(crate::error::ExifToolError::parse_error(
            "Unrecognized JPEG-HDR format",
        ));
    };

    // `my $meta = substr($$dataPt, 7, $pos-9);` — from just past the identifier
    // up to, but not including, the `~`.
    if term > HDR_RI_IDENTIFIER.len() {
        let meta = String::from_utf8_lossy(&data[HDR_RI_IDENTIFIER.len()..term]);
        for (key, value) in scan_hdr_pairs(&meta) {
            metadata.insert(
                format!("APP11:{}", hdr_tag_name(key)),
                TagValue::String(value.to_string()),
            );
        }
    }

    // `$et->HandleTag($tagTablePtr, 'RatioImage', substr($$dataPt, $pos));`
    // `RatioImage` is `Binary => 1`, so without -b ExifTool prints a placeholder.
    let ratio_image_size = data.len() - (term + HDR_META_TERMINATOR.len());
    metadata.insert(
        "APP11:RatioImage".to_string(),
        TagValue::String(format!(
            "(Binary data {} bytes, use -b option to extract)",
            ratio_image_size
        )),
    );

    Ok(metadata)
}

/// Extracts JPEG-HDR parameters into a structured, numeric form.
///
/// The metadata map holds ExifTool's verbatim text; this decodes it for callers
/// that need numbers rather than the reported strings.
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::jpeg::app_segments::app11_jpeg_hdr::extract_hdr_parameters;
///
/// let params = extract_hdr_parameters(segment_data)?;
/// if let Some(version) = params.version {
///     println!("JPEG-HDR version: {}", version.0);
/// }
/// ```
pub fn extract_hdr_parameters(data: &[u8]) -> Result<JpegHdrParameters> {
    let metadata = parse_app11_jpeg_hdr(data)?;
    let mut params = JpegHdrParameters::default();

    let text = |key: &str| metadata.get_string(key);
    let number = |key: &str| text(key).and_then(|v| v.trim().parse::<f32>().ok());

    if let Some(version) = number("APP11:JPEG-HDRVersion") {
        params.version = Some((version as u8, 0));
    }

    params.alpha = number("APP11:Alpha");
    params.beta = number("APP11:Beta");
    params.ln0 = number("APP11:Ln0");
    params.ln1 = number("APP11:Ln1");
    params.s2n = number("APP11:S2n");

    if let Some(correction) = number("APP11:CorrectionMethod") {
        params.correction_method = Some(CorrectionMethod::from_byte(correction as u8));
    }

    if let Some(ratio_str) = text("APP11:RatioImage") {
        // Parse size from "(Binary data N bytes, use -b option to extract)"
        if let Some(size_start) = ratio_str.find("Binary data ") {
            let size_part = &ratio_str[size_start + 12..];
            if let Some(size_end) = size_part.find(' ')
                && let Ok(size) = size_part[..size_end].parse::<usize>()
            {
                params.ratio_image_size = Some(size);
                params.has_ratio_image = size > 0;
            }
        }
    }

    Ok(params)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The APP11 payload of ExifTool's own `t/images/ExifTool.jpg`, byte for
    /// byte. `exiftool -G1 -s` reports, from this segment:
    ///
    /// ```text
    /// [JPEG-HDR]      JPEG-HDRVersion                 : 11
    /// [JPEG-HDR]      Ln0                             : 0.122262
    /// [JPEG-HDR]      Ln1                             : 2.634655
    /// [JPEG-HDR]      S2n                             : 2.269635e+03
    /// [JPEG-HDR]      Alpha                           : 1.000000
    /// [JPEG-HDR]      Beta                            : 1.000000
    /// [JPEG-HDR]      CorrectionMethod                : 0
    /// [JPEG-HDR]      RatioImage                      : (Binary data 19 bytes, use -b option to extract)
    /// ```
    const EXIFTOOL_JPG_APP11: &[u8] = b"HDR_RI ver=11\n ln0=0.122262, ln1=2.634655, s2n=2.269635e+03 alp=1.000000 bet=1.000000 cor=0\n~\x00<dummy ratio image>";

    #[test]
    fn test_parse_exiftool_sample_matches_exiftool_output() {
        let metadata = parse_app11_jpeg_hdr(EXIFTOOL_JPG_APP11).expect("should parse");

        let get = |k: &str| metadata.get_string(k).unwrap_or_default();
        assert_eq!(get("APP11:JPEG-HDRVersion"), "11");
        assert_eq!(get("APP11:Ln0"), "0.122262");
        assert_eq!(get("APP11:Ln1"), "2.634655");
        assert_eq!(get("APP11:S2n"), "2.269635e+03");
        assert_eq!(get("APP11:Alpha"), "1.000000");
        assert_eq!(get("APP11:Beta"), "1.000000");
        assert_eq!(get("APP11:CorrectionMethod"), "0");
        assert_eq!(
            get("APP11:RatioImage"),
            "(Binary data 19 bytes, use -b option to extract)"
        );
    }

    /// `s2n` is written in exponent form and ExifTool has no PrintConv for it,
    /// so the exponent must survive: a decimal `2269.635` is a rendering
    /// ExifTool never emits for this file.
    #[test]
    fn test_s2n_keeps_exponent_form() {
        let metadata = parse_app11_jpeg_hdr(EXIFTOOL_JPG_APP11).unwrap();
        assert_eq!(
            metadata.get_string("APP11:S2n").as_deref(),
            Some("2.269635e+03")
        );
    }

    /// Trailing zeros are part of the reported value, not noise.
    #[test]
    fn test_saturation_parameters_keep_trailing_zeros() {
        let metadata = parse_app11_jpeg_hdr(EXIFTOOL_JPG_APP11).unwrap();
        assert_eq!(
            metadata.get_string("APP11:Alpha").as_deref(),
            Some("1.000000")
        );
        assert_eq!(
            metadata.get_string("APP11:Beta").as_deref(),
            Some("1.000000")
        );
    }

    #[test]
    fn test_extract_hdr_parameters_decodes_text() {
        let params = extract_hdr_parameters(EXIFTOOL_JPG_APP11).unwrap();
        assert_eq!(params.version, Some((11, 0)));
        assert_eq!(params.correction_method, Some(CorrectionMethod::None));
        assert!((params.alpha.unwrap() - 1.0).abs() < 1e-6);
        assert!((params.beta.unwrap() - 1.0).abs() < 1e-6);
        assert!((params.ln0.unwrap() - 0.122262).abs() < 1e-6);
        assert!((params.ln1.unwrap() - 2.634655).abs() < 1e-6);
        assert!((params.s2n.unwrap() - 2269.635).abs() < 1e-2);
        assert!(params.has_ratio_image);
        assert_eq!(params.ratio_image_size, Some(19));
    }

    /// A segment with no ratio image after `~\0` still reports the tag, with a
    /// zero byte count, because `HandleTag` is called unconditionally.
    #[test]
    fn test_empty_ratio_image() {
        let params = extract_hdr_parameters(b"HDR_RI ver=11 cor=0\n~\x00").unwrap();
        assert_eq!(params.ratio_image_size, Some(0));
        assert!(!params.has_ratio_image);
    }

    /// `ProcessJPEG_HDR` warns and returns 0 when the `~\0` marker is missing.
    #[test]
    fn test_missing_terminator_is_rejected() {
        assert!(parse_app11_jpeg_hdr(b"HDR_RI ver=11 cor=0").is_err());
    }

    /// APP11 also carries JUMBF (`^JP`), which this table must not claim.
    #[test]
    fn test_non_hdr_segment_is_rejected() {
        assert!(parse_app11_jpeg_hdr(b"JP\x20\x20jumbf payload~\x00").is_err());
        assert!(parse_app11_jpeg_hdr(b"HDR_RI").is_err());
        assert!(parse_app11_jpeg_hdr(b"").is_err());
    }

    /// The value runs to the next comma or whitespace and may be empty.
    #[test]
    fn test_pair_scanning_boundaries() {
        assert_eq!(
            scan_hdr_pairs("a=1, b=2 c= d=4"),
            vec![("a", "1"), ("b", "2"), ("c", ""), ("d", "4")]
        );
        // A bare word without '=' is not a tag.
        assert_eq!(scan_hdr_pairs("HDR_RI ver=11"), vec![("ver", "11")]);
    }

    #[test]
    fn test_unknown_key_naming() {
        assert_eq!(hdr_tag_name("ver"), "JPEG-HDRVersion");
        assert_eq!(hdr_tag_name("cor"), "CorrectionMethod");
        assert_eq!(hdr_tag_name("gamma"), "Gamma");
        // MakeTagName: ucfirst, then "Tag"-prefix anything under 2 chars or
        // starting with a digit.
        assert_eq!(hdr_tag_name("x"), "TagX");
        assert_eq!(hdr_tag_name("2pass"), "Tag2pass");
    }

    #[test]
    fn test_correction_method_descriptions() {
        for (byte, expected) in [
            (0u8, "None"),
            (1u8, "Multiplicative"),
            (2u8, "Additive"),
            (3u8, "Logarithmic"),
            (4u8, "Gamma"),
            (99u8, "Unknown (99)"),
        ] {
            assert_eq!(CorrectionMethod::from_byte(byte).description(), expected);
        }
    }
}
