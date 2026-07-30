//! Nikon Capture NX editing metadata (`NikonCaptureData`, MakerNote 0x0E01).
//!
//! Nikon Capture writes its edit history into a single MakerNote tag as a
//! stream of variable-length records. This is NOT an IFD: the records carry
//! 32-bit tag ids, a 22-byte header apiece, and are always little-endian
//! regardless of the enclosing file's byte order.
//!
//! Ported from ExifTool's `Image::ExifTool::NikonCapture::ProcessNikonCapture`.
//! Tag ids and PrintConvs come from `exiftool -f -listx`, which reports them
//! in decimal at full width -- deliberately not from
//! `oxidex-tags-camera/src/camera_tags.yaml`, whose NikonCapture ids were
//! truncated from 32 bits to 16 (`0x008ae85e` stored as `0xE85E`) by an
//! older generator, so every lookup by real id would miss.

use std::collections::HashMap;

/// Bytes of record header preceding each value: the 32-bit id, ten bytes this
/// port does not interpret, the 32-bit size, then four more.
const RECORD_HEADER: usize = 22;

/// Offset of the size field within a record header.
const SIZE_OFFSET: usize = 18;

/// How a record's bytes become a displayed value.
#[derive(Clone, Copy)]
enum Format {
    /// Unsigned byte, rendered through an on/off style table.
    Int8uOffOn,
    /// Unsigned byte, rendered through a no/yes style table.
    Int8uNoYes,
    /// Unsigned 16-bit.
    Int16u,
    /// Signed 16-bit.
    Int16s,
    /// IEEE 754 double.
    Double,
}

/// `NikonCapture::Main`'s scalar tags: (id, name, format).
///
/// The SubDirectory entries (CropData, UnsharpData, NoiseReduction, ...) are
/// deliberately absent -- they need their own binary-data tables, and
/// emitting their container as a scalar would be a wrong value rather than a
/// missing one.
const MAIN_TAGS: &[(u32, &str, Format)] = &[
    (0x008a_e85e, "LCHEditor", Format::Int8uOffOn),
    (0x0c89_224b, "ColorAberrationControl", Format::Int8uOffOn),
    (0x2175_eb78, "D-LightingHQ", Format::Int8uOffOn),
    (0x2fc0_8431, "StraightenAngle", Format::Double),
    (0x4163_91c6, "QuickFix", Format::Int8uOffOn),
    (0x5f0e_7d23, "ColorBooster", Format::Int8uOffOn),
    (0x6a6e_36b6, "D-LightingHQSelected", Format::Int8uNoYes),
    (0x753d_cbc0, "NoiseReduction", Format::Int8uOffOn),
    (0x76a4_3200, "UnsharpMask", Format::Int8uOffOn),
    (0x76a4_3201, "Curves", Format::Int8uOffOn),
    (0x76a4_3202, "ColorBalanceAdj", Format::Int8uOffOn),
    (0x76a4_3203, "AdvancedRaw", Format::Int8uOffOn),
    (0x76a4_3204, "WhiteBalanceAdj", Format::Int8uOffOn),
    (0x76a4_3205, "VignetteControl", Format::Int8uOffOn),
    (0x76a4_3206, "FlipHorizontal", Format::Int8uNoYes),
    (0x76a4_3207, "Rotation", Format::Int16u),
    (0xab5e_ca5e, "PhotoEffects", Format::Int8uOffOn),
    (0xac6b_d5c0, "VignetteControlIntensity", Format::Int16s),
    (0xce55_54aa, "D-LightingHS", Format::Int8uOffOn),
    (0xe217_3c47, "PictureControl", Format::Int8uOffOn),
    (0xfe28_a44f, "AutoRedEye", Format::Int8uOffOn),
    (0xfe44_3a45, "ImageDustOff", Format::Int8uOffOn),
];

fn render(value: &[u8], format: Format) -> Option<String> {
    Some(match format {
        Format::Int8uOffOn => match *value.first()? {
            0 => "Off".to_string(),
            1 => "On".to_string(),
            // An unrecognised code reports itself rather than being rounded
            // to the nearer of Off/On.
            other => other.to_string(),
        },
        Format::Int8uNoYes => match *value.first()? {
            0 => "No".to_string(),
            1 => "Yes".to_string(),
            other => other.to_string(),
        },
        Format::Int16u => {
            let b = value.get(..2)?;
            u16::from_le_bytes([b[0], b[1]]).to_string()
        }
        Format::Int16s => {
            let b = value.get(..2)?;
            i16::from_le_bytes([b[0], b[1]]).to_string()
        }
        Format::Double => {
            let b = value.get(..8)?;
            let v = f64::from_le_bytes(b.try_into().ok()?);
            // ExifTool prints a plain number here; trim a pointless ".0" so
            // an unrotated image reads 0 rather than 0.0.
            if v.fract() == 0.0 {
                format!("{}", v as i64)
            } else {
                format!("{}", v)
            }
        }
    })
}

/// Walks the NikonCaptureData record stream, inserting every recognised tag.
///
/// Mirrors ExifTool's loop: start 22 bytes in, read the id at the record
/// start and the size at +18 (which counts four bytes more than the value),
/// then step over header and value together. A record claiming more bytes
/// than remain ends the walk rather than erroring, matching ExifTool's
/// `last if ... $pos + $size > $dirEnd`.
pub fn parse_nikon_capture_data(data: &[u8], tags: &mut HashMap<String, String>) {
    let mut pos = RECORD_HEADER;
    while pos + RECORD_HEADER < data.len() {
        let id = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        let declared = u32::from_le_bytes([
            data[pos + SIZE_OFFSET],
            data[pos + SIZE_OFFSET + 1],
            data[pos + SIZE_OFFSET + 2],
            data[pos + SIZE_OFFSET + 3],
        ]);
        // The stored size counts four bytes beyond the value itself.
        let Some(size) = (declared as usize).checked_sub(4) else {
            break;
        };
        pos += RECORD_HEADER;
        if pos + size > data.len() {
            break;
        }

        if let Some((_, name, format)) = MAIN_TAGS.iter().find(|(tid, _, _)| *tid == id)
            && let Some(rendered) = render(&data[pos..pos + size], *format)
        {
            tags.insert(format!("Nikon:{}", name), rendered);
        }

        pos += size;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a record stream: 22 bytes of preamble, then one 22-byte header
    /// plus value per entry.
    fn stream(records: &[(u32, &[u8])]) -> Vec<u8> {
        let mut d = vec![0u8; RECORD_HEADER];
        for (id, value) in records {
            let mut hdr = vec![0u8; RECORD_HEADER];
            hdr[..4].copy_from_slice(&id.to_le_bytes());
            hdr[SIZE_OFFSET..SIZE_OFFSET + 4]
                .copy_from_slice(&((value.len() + 4) as u32).to_le_bytes());
            d.extend_from_slice(&hdr);
            d.extend_from_slice(value);
        }
        d
    }

    #[test]
    fn decodes_main_table_scalars() {
        let data = stream(&[
            (0x76a4_3203, &[1]),          // AdvancedRaw = On
            (0xfe28_a44f, &[1]),          // AutoRedEye = On
            (0x0c89_224b, &[0]),          // ColorAberrationControl = Off
            (0x76a4_3207, &[0x5a, 0x00]), // Rotation = 90
            (0xac6b_d5c0, &[0xf6, 0xff]), // VignetteControlIntensity = -10
        ]);
        let mut tags = HashMap::new();
        parse_nikon_capture_data(&data, &mut tags);

        assert_eq!(
            tags.get("Nikon:AdvancedRaw").map(String::as_str),
            Some("On")
        );
        assert_eq!(tags.get("Nikon:AutoRedEye").map(String::as_str), Some("On"));
        assert_eq!(
            tags.get("Nikon:ColorAberrationControl").map(String::as_str),
            Some("Off")
        );
        assert_eq!(tags.get("Nikon:Rotation").map(String::as_str), Some("90"));
        assert_eq!(
            tags.get("Nikon:VignetteControlIntensity")
                .map(String::as_str),
            Some("-10")
        );
    }

    /// The ids are 32-bit. A build that truncated them to 16 -- as the
    /// generated YAML did -- would match nothing here.
    #[test]
    fn ids_are_full_width() {
        let data = stream(&[(0x008a_e85e, &[1])]);
        let mut tags = HashMap::new();
        parse_nikon_capture_data(&data, &mut tags);
        assert_eq!(tags.get("Nikon:LCHEditor").map(String::as_str), Some("On"));

        // The low 16 bits alone must NOT be accepted as the same tag.
        let truncated = stream(&[(0x0000_e85e, &[1])]);
        let mut other = HashMap::new();
        parse_nikon_capture_data(&truncated, &mut other);
        assert!(other.is_empty());
    }

    /// A record claiming more bytes than remain ends the walk instead of
    /// panicking, and whatever was read before it survives.
    #[test]
    fn truncated_record_stops_the_walk() {
        let mut data = stream(&[(0x76a4_3203, &[1])]);
        let mut hdr = vec![0u8; RECORD_HEADER];
        hdr[..4].copy_from_slice(&0x76a4_3201u32.to_le_bytes());
        hdr[SIZE_OFFSET..SIZE_OFFSET + 4].copy_from_slice(&9999u32.to_le_bytes());
        data.extend_from_slice(&hdr);

        let mut tags = HashMap::new();
        parse_nikon_capture_data(&data, &mut tags);
        assert_eq!(
            tags.get("Nikon:AdvancedRaw").map(String::as_str),
            Some("On")
        );
        assert!(!tags.contains_key("Nikon:Curves"));
    }
}
