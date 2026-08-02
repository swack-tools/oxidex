//! EXIF enum value formatters for ExifTool-compatible output
//!
//! This module contains formatters for common EXIF enum tags that convert
//! integer values to human-readable strings matching ExifTool's output.

/// Formats ColorSpace (EXIF tag 0xA001).
///
/// ExifTool's `%Exif::Main{0xa001}` carries `PrintHex => 1`, so a value the
/// table does not name prints its code in hex -- `Unknown (0x0)`, not
/// `Unknown (0)`. Thirteen files in the sample corpus report exactly that.
pub fn format_color_space(value: i64) -> String {
    match value {
        1 => "sRGB".to_string(),
        2 => "Adobe RGB".to_string(),
        65533 => "Wide Gamut RGB".to_string(),
        65534 => "ICC Profile".to_string(),
        65535 => "Uncalibrated".to_string(),
        _ => format!("Unknown (0x{:x})", value),
    }
}

/// Format MeteringMode enum value
/// EXIF tag 0x9207
pub fn format_metering_mode(value: i64) -> String {
    match value {
        0 => "Unknown".to_string(),
        1 => "Average".to_string(),
        2 => "Center-weighted average".to_string(),
        3 => "Spot".to_string(),
        4 => "Multi-spot".to_string(),
        5 => "Multi-segment".to_string(),
        6 => "Partial".to_string(),
        255 => "Other".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format LightSource enum value
/// EXIF tag 0x9208
pub fn format_light_source(value: i64) -> String {
    match value {
        0 => "Unknown".to_string(),
        1 => "Daylight".to_string(),
        2 => "Fluorescent".to_string(),
        3 => "Tungsten (Incandescent)".to_string(),
        4 => "Flash".to_string(),
        9 => "Fine Weather".to_string(),
        10 => "Cloudy".to_string(),
        11 => "Shade".to_string(),
        12 => "Daylight Fluorescent".to_string(),
        13 => "Day White Fluorescent".to_string(),
        14 => "Cool White Fluorescent".to_string(),
        15 => "White Fluorescent".to_string(),
        16 => "Warm White Fluorescent".to_string(),
        17 => "Standard Light A".to_string(),
        18 => "Standard Light B".to_string(),
        19 => "Standard Light C".to_string(),
        20 => "D55".to_string(),
        21 => "D65".to_string(),
        22 => "D75".to_string(),
        23 => "D50".to_string(),
        24 => "ISO Studio Tungsten".to_string(),
        255 => "Other".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// `%Image::ExifTool::Exif::flash` (Exif.pm:175), transcribed verbatim.
///
/// Flash is **not** a bitfield PrintConv. Exif.pm 0x9209 is
/// `PrintConv => \%flash` -- a flat 27-key hash -- and `Flags => 'PrintHex'`,
/// so a code the hash does not name prints as `Unknown (0x38)`. The bit layout
/// (fired / strobe return / mode / function present / red-eye) explains how the
/// 27 codes were *chosen*; it is not how ExifTool renders them, and 229 of the
/// 256 byte values are simply not valid Flash codes.
///
/// `core::exif_enums::decode_flash` synthesised a label from those bit fields
/// instead. Scored against the hash over all 256 inputs it was wrong on **236**:
/// it named codes ExifTool leaves unknown (`0x02` -> `No Flash`, ExifTool
/// `Unknown (0x2)`), and mis-worded ones it did know (`0x0d` -> `On, Fired,
/// Return not detected`, ExifTool `On, Return not detected`). Two verbatim
/// copies of the real table already existed, in `parsers::pdf` and
/// `parsers::raw::metadata`; this is the one both now share.
const FLASH: &[(i64, &str)] = &[
    (0x00, "No Flash"),
    (0x01, "Fired"),
    (0x05, "Fired, Return not detected"),
    (0x07, "Fired, Return detected"),
    (0x08, "On, Did not fire"),
    (0x09, "On, Fired"),
    (0x0d, "On, Return not detected"),
    (0x0f, "On, Return detected"),
    (0x10, "Off, Did not fire"),
    (0x14, "Off, Did not fire, Return not detected"),
    (0x18, "Auto, Did not fire"),
    (0x19, "Auto, Fired"),
    (0x1d, "Auto, Fired, Return not detected"),
    (0x1f, "Auto, Fired, Return detected"),
    (0x20, "No flash function"),
    (0x30, "Off, No flash function"),
    (0x41, "Fired, Red-eye reduction"),
    (0x45, "Fired, Red-eye reduction, Return not detected"),
    (0x47, "Fired, Red-eye reduction, Return detected"),
    (0x49, "On, Red-eye reduction"),
    (0x4d, "On, Red-eye reduction, Return not detected"),
    (0x4f, "On, Red-eye reduction, Return detected"),
    (0x50, "Off, Red-eye reduction"),
    (0x58, "Auto, Did not fire, Red-eye reduction"),
    (0x59, "Auto, Fired, Red-eye reduction"),
    (0x5d, "Auto, Fired, Red-eye reduction, Return not detected"),
    (0x5f, "Auto, Fired, Red-eye reduction, Return detected"),
];

/// Looks up EXIF `Flash` (0x9209) in ExifTool's `%flash`.
///
/// Returns `None` for a code ExifTool does not name, so callers keep their own
/// behaviour for it -- `format_flash` renders `Unknown (0x38)` the way
/// `PrintHex` does, while the RAW and PDF paths leave the tag alone.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::exif_enums::flash_label;
///
/// assert_eq!(flash_label(0x0d), Some("On, Return not detected"));
/// assert_eq!(flash_label(0x02), None);
/// ```
pub fn flash_label(value: i64) -> Option<&'static str> {
    FLASH
        .iter()
        .find(|&&(id, _)| id == value)
        .map(|&(_, label)| label)
}

/// Format Flash enum value
/// EXIF tag 0x9209
///
/// `Flags => 'PrintHex'` on Exif.pm 0x9209, so unnamed codes print their value
/// in lowercase hex: `Unknown (0x38)`, not `Unknown (56)`.
pub fn format_flash(value: i64) -> String {
    match flash_label(value) {
        Some(label) => label.to_string(),
        None => format!("Unknown (0x{:x})", value),
    }
}

/// Format ExposureMode enum value
/// EXIF tag 0xA402
pub fn format_exposure_mode(value: i64) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "Manual".to_string(),
        2 => "Auto bracket".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format WhiteBalance enum value
/// EXIF tag 0xA403
pub fn format_white_balance(value: i64) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "Manual".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format SceneCaptureType enum value
/// EXIF tag 0xA406
pub fn format_scene_capture_type(value: i64) -> String {
    match value {
        0 => "Standard".to_string(),
        1 => "Landscape".to_string(),
        2 => "Portrait".to_string(),
        3 => "Night".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format Contrast enum value
/// EXIF tag 0xA408
pub fn format_contrast(value: i64) -> String {
    match value {
        0 => "Normal".to_string(),
        1 => "Low".to_string(),
        2 => "High".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format Saturation enum value
/// EXIF tag 0xA409
pub fn format_saturation(value: i64) -> String {
    match value {
        0 => "Normal".to_string(),
        1 => "Low".to_string(),
        2 => "High".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format Sharpness enum value
/// EXIF tag 0xA40A
pub fn format_sharpness(value: i64) -> String {
    match value {
        0 => "Normal".to_string(),
        1 => "Soft".to_string(),
        2 => "Hard".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format GainControl enum value
/// EXIF tag 0xA407
pub fn format_gain_control(value: i64) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Low gain up".to_string(),
        2 => "High gain up".to_string(),
        3 => "Low gain down".to_string(),
        4 => "High gain down".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format FileSource enum value
/// EXIF tag 0xA300
pub fn format_file_source(value: i64) -> String {
    match value {
        1 => "Film Scanner".to_string(),
        2 => "Reflection Print Scanner".to_string(),
        3 => "Digital Camera".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format SensingMethod enum value
/// EXIF tag 0xA217
pub fn format_sensing_method(value: i64) -> String {
    match value {
        1 => "Not defined".to_string(),
        2 => "One-chip color area".to_string(),
        3 => "Two-chip color area".to_string(),
        4 => "Three-chip color area".to_string(),
        5 => "Color sequential area".to_string(),
        7 => "Trilinear".to_string(),
        8 => "Color sequential linear".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// `%Image::ExifTool::Exif::compression` (Exif.pm:213), transcribed verbatim.
///
/// This is the single copy. Three divergent transcriptions used to exist -- this
/// one, the `0x0103` arm of `parsers::tiff::tiff_enums::tiff_enum_to_string`,
/// and `COMPRESSION_LABELS` in `parsers::pdf` -- carrying 50, 40 and 53 ids
/// respectively. Dumping `%Image::ExifTool::Exif::compression` straight out of
/// the Perl symbol table and scoring all three over every code in 0..=70000
/// gave 10, 15 and 1 wrong answers; this table gives 0. The lines it is built
/// from, quoted from ExifTool 13.59:
///
/// ```text
///     9 => 'JBIG B&W or VC-5', #3 / github411
///     32766 => 'NeXt or Sony ARW Compressed 2', #3/Milos
///     33003 => 'Aperio JPEG 2000 YCbCr', #https://openslide.org/formats/aperio/
///     33005 => 'Aperio JPEG 2000 RGB', #https://openslide.org/formats/aperio/
///     34926 => 'Zstd (old)', #LibTiff
///     34927 => 'WebP (old)', #LibTiff
///     50000 => 'Zstd', #LibTiff 4.7
///     50001 => 'WebP', #LibTiff 4.7
///     50002 => 'JPEG XL (old)', #LibTiff 4.7
///     52546 => 'JPEG XL', # (DNG 1.7)
/// ```
///
/// Every one of those ten lines was previously wrong or absent in at least one
/// copy: `9` read `JBIG B&W`, `32766` read `Next`, `33003`/`33004`/`33005` all
/// collapsed to `Aperio JPEG 2000 YCbCr` (and `33004` is not an ExifTool key at
/// all -- it prints `Unknown (33004)`), `34926`/`34927` dropped the ` (old)`
/// suffix that distinguishes them from the LibTiff 4.7 codes, and 50000, 50001,
/// 50002 and 52546 were absent everywhere but the PDF copy.
const COMPRESSION: &[(i64, &str)] = &[
    (1, "Uncompressed"),
    (2, "CCITT 1D"),
    (3, "T4/Group 3 Fax"),
    (4, "T6/Group 4 Fax"),
    (5, "LZW"),
    (6, "JPEG (old-style)"),
    (7, "JPEG"),
    (8, "Adobe Deflate"),
    (9, "JBIG B&W or VC-5"),
    (10, "JBIG Color"),
    (99, "JPEG"),
    (262, "Kodak 262"),
    (32766, "NeXt or Sony ARW Compressed 2"),
    (32767, "Sony ARW Compressed"),
    (32769, "Packed RAW"),
    (32770, "Samsung SRW Compressed"),
    (32771, "CCIRLEW"),
    (32772, "Samsung SRW Compressed 2"),
    (32773, "PackBits"),
    (32809, "Thunderscan"),
    (32867, "Kodak KDC Compressed"),
    (32895, "IT8CTPAD"),
    (32896, "IT8LW"),
    (32897, "IT8MP"),
    (32898, "IT8BL"),
    (32908, "PixarFilm"),
    (32909, "PixarLog"),
    (32946, "Deflate"),
    (32947, "DCS"),
    (33003, "Aperio JPEG 2000 YCbCr"),
    (33005, "Aperio JPEG 2000 RGB"),
    (34661, "JBIG"),
    (34676, "SGILog"),
    (34677, "SGILog24"),
    (34712, "JPEG 2000"),
    (34713, "Nikon NEF Compressed"),
    (34715, "JBIG2 TIFF FX"),
    (34718, "Microsoft Document Imaging (MDI) Binary Level Codec"),
    (
        34719,
        "Microsoft Document Imaging (MDI) Progressive Transform Codec",
    ),
    (34720, "Microsoft Document Imaging (MDI) Vector"),
    (34887, "ESRI Lerc"),
    (34892, "Lossy JPEG"),
    (34925, "LZMA2"),
    (34926, "Zstd (old)"),
    (34927, "WebP (old)"),
    (34933, "PNG"),
    (34934, "JPEG XR"),
    (50000, "Zstd"),
    (50001, "WebP"),
    (50002, "JPEG XL (old)"),
    (52546, "JPEG XL"),
    (65000, "Kodak DCR Compressed"),
    (65535, "Pentax PEF Compressed"),
];

/// Looks up EXIF/TIFF `Compression` (0x0103) in ExifTool's `%compression`.
///
/// Returns `None` for a code ExifTool does not name, so callers keep their own
/// established behaviour for unknown values -- `format_compression` renders
/// `Unknown (N)` the way `PrintConv` does, while the TIFF and PDF paths leave
/// the tag alone rather than invent a label ExifTool never prints.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::exif_enums::compression_label;
///
/// assert_eq!(compression_label(6), Some("JPEG (old-style)"));
/// assert_eq!(compression_label(32766), Some("NeXt or Sony ARW Compressed 2"));
/// assert_eq!(compression_label(50000), Some("Zstd"));
/// assert_eq!(compression_label(33004), None);
/// ```
pub fn compression_label(value: i64) -> Option<&'static str> {
    COMPRESSION
        .iter()
        .find(|&&(id, _)| id == value)
        .map(|&(_, label)| label)
}

/// Format Compression enum value
/// EXIF/TIFF tag 0x0103
pub fn format_compression(value: i64) -> String {
    match compression_label(value) {
        Some(label) => label.to_string(),
        None => format!("Unknown ({})", value),
    }
}

/// Format Orientation enum value
/// EXIF/TIFF tag 0x0112
pub fn format_orientation(value: i64) -> String {
    match value {
        1 => "Horizontal (normal)".to_string(),
        2 => "Mirror horizontal".to_string(),
        3 => "Rotate 180".to_string(),
        4 => "Mirror vertical".to_string(),
        5 => "Mirror horizontal and rotate 270 CW".to_string(),
        6 => "Rotate 90 CW".to_string(),
        7 => "Mirror horizontal and rotate 90 CW".to_string(),
        8 => "Rotate 270 CW".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format ResolutionUnit enum value
/// EXIF/TIFF tag 0x0128
pub fn format_resolution_unit(value: i64) -> String {
    match value {
        1 => "None".to_string(),
        2 => "inches".to_string(),
        3 => "cm".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format YCbCrPositioning enum value
/// EXIF tag 0x0213
pub fn format_ycbcr_positioning(value: i64) -> String {
    match value {
        1 => "Centered".to_string(),
        2 => "Co-sited".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format ComponentsConfiguration binary data
/// EXIF tag 0x9101 - 4 bytes representing Y, Cb, Cr, - or R, G, B, -
pub fn format_components_configuration(data: &[u8]) -> String {
    if data.len() < 4 {
        return format!("(Binary data {} bytes)", data.len());
    }

    let component_names: Vec<&str> = data
        .iter()
        .take(4)
        .map(|&b| match b {
            0 => "-",
            1 => "Y",
            2 => "Cb",
            3 => "Cr",
            4 => "R",
            5 => "G",
            6 => "B",
            _ => "?",
        })
        .collect();

    component_names.join(", ")
}

/// Format CustomRendered enum value
/// EXIF tag 0xA401
pub fn format_custom_rendered(value: i64) -> String {
    match value {
        0 => "Normal".to_string(),
        1 => "Custom".to_string(),
        2 => "HDR (no original saved)".to_string(),
        3 => "HDR (original saved)".to_string(),
        4 => "Original (for HDR)".to_string(),
        6 => "Panorama".to_string(),
        7 => "Portrait HDR".to_string(),
        8 => "Portrait".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format DigitalZoomRatio - converts 0 to "Digital zoom not used"
pub fn format_digital_zoom_ratio(value: f64) -> String {
    if value == 0.0 {
        "Digital zoom not used".to_string()
    } else {
        format!("{}", value)
    }
}

/// Format SubjectDistanceRange enum value
/// EXIF tag 0xA40C
pub fn format_subject_distance_range(value: i64) -> String {
    match value {
        0 => "Unknown".to_string(),
        1 => "Macro".to_string(),
        2 => "Close".to_string(),
        3 => "Distant".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Format InteropIndex value
/// EXIF Interop tag 0x0001
pub fn format_interop_index(value: &str) -> String {
    match value.trim() {
        "R98" => "R98 - DCF basic file (sRGB)".to_string(),
        "THM" => "THM - DCF thumbnail file".to_string(),
        "R03" => "R03 - DCF option file (Adobe RGB)".to_string(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_space() {
        assert_eq!(format_color_space(1), "sRGB");
        assert_eq!(format_color_space(65535), "Uncalibrated");
    }

    #[test]
    fn test_metering_mode() {
        assert_eq!(format_metering_mode(5), "Multi-segment");
        assert_eq!(format_metering_mode(1), "Average");
    }

    #[test]
    fn test_flash() {
        assert_eq!(format_flash(0), "No Flash");
        assert_eq!(format_flash(1), "Fired");
    }

    #[test]
    fn test_orientation() {
        assert_eq!(format_orientation(1), "Horizontal (normal)");
        assert_eq!(format_orientation(6), "Rotate 90 CW");
    }

    #[test]
    fn test_compression() {
        assert_eq!(format_compression(1), "Uncompressed");
        assert_eq!(format_compression(6), "JPEG (old-style)");
        assert_eq!(format_compression(7), "JPEG");
    }

    /// The ten codes the pre-consolidation table got wrong.
    ///
    /// `test_compression` above asserted 1, 6 and 7 only -- three codes that
    /// three divergent tables all agreed on -- so it stayed green while `32766`
    /// printed `Next` and `50000` printed `Unknown (50000)`. Each assertion
    /// here is a branch that was measurably wrong against
    /// `%Image::ExifTool::Exif::compression` (Exif.pm:213, ExifTool 13.59).
    #[test]
    fn compression_codes_the_old_table_got_wrong() {
        // was "JBIG B&W"
        assert_eq!(format_compression(9), "JBIG B&W or VC-5");
        // was "Next"
        assert_eq!(format_compression(32766), "NeXt or Sony ARW Compressed 2");
        // 33003/33004/33005 all collapsed to the YCbCr label
        assert_eq!(format_compression(33003), "Aperio JPEG 2000 YCbCr");
        assert_eq!(format_compression(33005), "Aperio JPEG 2000 RGB");
        // 33004 is not an ExifTool key at all
        assert_eq!(format_compression(33004), "Unknown (33004)");
        assert_eq!(compression_label(33004), None);
        // the " (old)" suffix separates these from the LibTiff 4.7 codes
        assert_eq!(format_compression(34926), "Zstd (old)");
        assert_eq!(format_compression(34927), "WebP (old)");
        // absent entirely -- printed "Unknown (N)"
        assert_eq!(format_compression(50000), "Zstd");
        assert_eq!(format_compression(50001), "WebP");
        assert_eq!(format_compression(50002), "JPEG XL (old)");
        assert_eq!(format_compression(52546), "JPEG XL");
    }

    /// Codes ExifTool does not name still print `Unknown (N)`, and the id space
    /// between the named LibTiff codes is not silently filled in.
    #[test]
    fn compression_unknown_codes_are_not_invented() {
        assert_eq!(compression_label(0), None);
        assert_eq!(format_compression(0), "Unknown (0)");
        assert_eq!(format_compression(1536), "Unknown (1536)");
        assert_eq!(format_compression(50003), "Unknown (50003)");
        assert_eq!(format_compression(52545), "Unknown (52545)");
        // 32910/32911 are "Pixar reserved" in Exif.pm and carry no label
        assert_eq!(compression_label(32910), None);
        assert_eq!(compression_label(32911), None);
        // 34888/34889 are "ESRI reserved"
        assert_eq!(compression_label(34888), None);
        assert_eq!(compression_label(34889), None);
    }

    /// The table holds exactly the 53 keys of `%compression`, no more.
    #[test]
    fn compression_table_has_exactly_exiftools_key_count() {
        assert_eq!(COMPRESSION.len(), 53);
        let mut ids: Vec<i64> = COMPRESSION.iter().map(|&(id, _)| id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), COMPRESSION.len(), "duplicate id in table");
        assert_eq!(*ids.first().unwrap(), 1);
        assert_eq!(*ids.last().unwrap(), 65535);
    }

    #[test]
    fn test_components_configuration() {
        assert_eq!(
            format_components_configuration(&[1, 2, 3, 0]),
            "Y, Cb, Cr, -"
        );
        assert_eq!(format_components_configuration(&[4, 5, 6, 0]), "R, G, B, -");
    }

    #[test]
    fn test_custom_rendered() {
        assert_eq!(format_custom_rendered(0), "Normal");
        assert_eq!(format_custom_rendered(1), "Custom");
    }

    #[test]
    fn test_interop_index() {
        assert_eq!(format_interop_index("R98"), "R98 - DCF basic file (sRGB)");
    }
}
