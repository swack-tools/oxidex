//! Sony `LensSpec`: eight bytes describing focal range, aperture range and a
//! bitfield of lens features.
//!
//! Transcribed from `ConvLensSpec` / `PrintLensSpec` / `@lensFeatures` in
//! `Image::ExifTool::Sony`.

use super::binary::print_float;

/// Lens features in the order ExifTool adds them to the string.
///
/// `mask` selects bits out of the 16-bit flag word built from byte 0 (high) and
/// byte 7 (low); `prefix` says whether the matched name goes before the focal
/// length rather than after it.
struct LensFeature {
    mask: u16,
    names: &'static [(u16, &'static str)],
    prefix: bool,
}

static LENS_FEATURES: &[LensFeature] = &[
    LensFeature {
        mask: 0x4000,
        names: &[(0x4000, "PZ")],
        prefix: true,
    },
    LensFeature {
        mask: 0x0300,
        names: &[(0x0100, "DT"), (0x0200, "FE"), (0x0300, "E")],
        prefix: true,
    },
    LensFeature {
        mask: 0x00e0,
        names: &[
            (0x0020, "STF"),
            (0x0040, "Reflex"),
            (0x0060, "Macro"),
            (0x0080, "Fisheye"),
        ],
        prefix: false,
    },
    LensFeature {
        mask: 0x000c,
        names: &[(0x0004, "ZA"), (0x0008, "G")],
        prefix: false,
    },
    LensFeature {
        mask: 0x0003,
        names: &[(0x0001, "SSM"), (0x0002, "SAM")],
        prefix: false,
    },
    LensFeature {
        mask: 0x8000,
        names: &[(0x8000, "OSS")],
        prefix: false,
    },
    LensFeature {
        mask: 0x2000,
        names: &[(0x2000, "LE")],
        prefix: false,
    },
    LensFeature {
        mask: 0x0800,
        names: &[(0x0800, "II")],
        prefix: false,
    },
];

/// ExifTool's `ConvLensSpec`: the six-field intermediate value, e.g.
/// `"01 16 50 2.8 0 01"`.
///
/// Fields are `flags1, focal-short(2 bytes), focal-long(2 bytes),
/// aperture-short, aperture-long, flags2`. Every numeric field is
/// binary-coded decimal: ExifTool unpacks the bytes to a hex *string* and then
/// uses it as a number, so the two bytes `00 16` are the focal length 16mm,
/// not 0x0016 = 22mm, and the aperture byte 0xb0 is 110 -> f/11.
fn conv_lens_spec(bytes: &[u8]) -> Option<[String; 6]> {
    if bytes.len() != 8 {
        return None;
    }
    Some([
        format!("{:02x}", bytes[0]),
        bcd_pair(bytes[1], bytes[2]).to_string(),
        bcd_pair(bytes[3], bytes[4]).to_string(),
        print_float(bcd_byte(bytes[5]) as f64 / 10.0),
        print_float(bcd_byte(bytes[6]) as f64 / 10.0),
        format!("{:02x}", bytes[7]),
    ])
}

/// Reads a byte's two hex digits as decimal digits: 0x28 is 28, 0xb0 is 110.
fn bcd_byte(byte: u8) -> u32 {
    (byte >> 4) as u32 * 10 + (byte & 0x0f) as u32
}

/// The same across a two-byte field: `00 16` is 16, `01 20` is 120.
fn bcd_pair(high: u8, low: u8) -> u32 {
    bcd_byte(high) * 100 + bcd_byte(low)
}

/// ExifTool's `PrintLensSpec`, e.g. `"DT 16-50mm F2.8 SSM"`.
///
/// Returns `None` only when the blob is not eight bytes; a blob that fails
/// ExifTool's focal/aperture sanity check prints as `Unknown (...)`, exactly as
/// ExifTool does for the all-zero LensSpec the A-mount DSLRs write when no
/// electronic lens data is available.
pub fn print_lens_spec(bytes: &[u8]) -> Option<String> {
    let fields = conv_lens_spec(bytes)?;
    let raw = fields.join(" ");

    let short_focal: f64 = fields[1].parse().ok()?;
    let long_focal: f64 = fields[2].parse().ok()?;
    let short_aperture: f64 = fields[3].parse().ok()?;
    let long_aperture: f64 = fields[4].parse().ok()?;

    // ExifTool's "crude validation": without it the flag bits are not trusted
    // either, and the whole value prints as Unknown.
    let plausible = short_focal != 0.0
        && short_aperture != 0.0
        && (long_focal == 0.0 || long_focal >= short_focal)
        && (long_aperture == 0.0 || long_aperture >= short_aperture);
    if !plausible {
        return Some(format!("Unknown ({})", raw));
    }

    let mut focal = print_float(short_focal);
    if long_focal != short_focal && long_focal != 0.0 {
        focal = format!("{}-{}", focal, print_float(long_focal));
    }
    let mut aperture = print_float(short_aperture);
    if long_aperture != short_aperture && long_aperture != 0.0 {
        aperture = format!("{}-{}", aperture, print_float(long_aperture));
    }
    let mut result = format!("{}mm F{}", focal, aperture);

    let flags = u16::from_be_bytes([bytes[0], bytes[7]]);
    for feature in LENS_FEATURES {
        let bits = feature.mask & flags;
        let named = feature.names.iter().find(|(b, _)| *b == bits);
        if bits == 0 && named.is_none() {
            continue;
        }
        let name = match named {
            Some((_, name)) => (*name).to_string(),
            None => format!("Unknown({:04x})", bits),
        };
        result = if feature.prefix {
            format!("{} {}", name, result)
        } else {
            format!("{} {}", result, name)
        };
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ilca_77m2_kit_lens() {
        // SonyILCA-77M2.jpg: exiftool reports "DT 16-50mm F2.8 SSM" from the
        // intermediate value "01 16 50 2.8 0 01".
        let bytes = [0x01, 0x00, 0x16, 0x00, 0x50, 0x28, 0x00, 0x01];
        assert_eq!(
            print_lens_spec(&bytes),
            Some("DT 16-50mm F2.8 SSM".to_string())
        );
    }

    #[test]
    fn all_zero_spec_prints_as_unknown() {
        // What DSLR-A350 and SLT-A77 write; exiftool prints the raw six fields.
        let bytes = [0u8; 8];
        assert_eq!(
            print_lens_spec(&bytes),
            Some("Unknown (00 0 0 0 0 00)".to_string())
        );
    }

    #[test]
    fn numeric_fields_are_binary_coded_decimal() {
        // 0xb0 is f/11, not f/17.6.
        assert_eq!(bcd_byte(0xb0), 110);
        assert_eq!(bcd_byte(0x28), 28);
        // The ILCA-77M2 kit lens is 16-50mm, not 22-80mm.
        assert_eq!(bcd_pair(0x00, 0x16), 16);
        assert_eq!(bcd_pair(0x00, 0x50), 50);
        assert_eq!(bcd_pair(0x01, 0x20), 120);
    }

    #[test]
    fn wrong_length_is_rejected() {
        assert_eq!(print_lens_spec(&[0u8; 4]), None);
    }
}
