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

use crate::core::formatters::numeric_precision::perl_number;
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

// =============================================================================
// SubDirectory tables
// =============================================================================

/// One field inside a binary-data sub-table: (key, name, format).
///
/// `key` is ExifTool's table key. It is a BYTE offset only when the table's
/// FORMAT is int8u; for a table declaring `FORMAT => 'int32u'` the keys are
/// indices in units of that format, so key 2 means byte 8. Getting this
/// backwards makes DLightingHQ's three fields overlap at bytes 0,1,2 and
/// yields garbage -- see `SubTable::stride`.
struct Field {
    key: usize,
    name: &'static str,
    format: SubFormat,
}

#[derive(Clone, Copy, PartialEq)]
enum SubFormat {
    U8,
    U8Enum(&'static [(u8, &'static str)]),
    U16,
    U16Enum(&'static [(u16, &'static str)]),
    I16,
    U32,
    Double,
    /// int16s with `ValueConv => '$val / 100'` (ExposureAdj).
    I16Hundredths,
    /// A double printed via `sprintf("%.4f")` (ExposureAdj2).
    Double4dp,
    /// A double carrying `ValueConv => '$val / 2'`. Nikon stores the crop
    /// rectangle and its source resolution at twice their reported size, so
    /// a 3008-wide crop is written 6016.
    DoubleHalved,
}

struct SubTable {
    /// Bytes per table key. 1 for `FORMAT => 'int8u'` (the default), 4 for
    /// `FORMAT => 'int32u'`.
    stride: usize,
    fields: &'static [Field],
}

const OFF_ON: &[(u8, &str)] = &[(0, "Off"), (1, "On")];

const CROP_DATA: &[Field] = &[
    Field {
        key: 30,
        name: "CropLeft",
        format: SubFormat::DoubleHalved,
    },
    Field {
        key: 38,
        name: "CropTop",
        format: SubFormat::DoubleHalved,
    },
    Field {
        key: 46,
        name: "CropRight",
        format: SubFormat::DoubleHalved,
    },
    Field {
        key: 54,
        name: "CropBottom",
        format: SubFormat::DoubleHalved,
    },
    Field {
        key: 142,
        name: "CropOutputWidthInches",
        format: SubFormat::Double,
    },
    Field {
        key: 150,
        name: "CropOutputHeightInches",
        format: SubFormat::Double,
    },
    Field {
        key: 158,
        name: "CropScaledResolution",
        format: SubFormat::Double,
    },
    Field {
        key: 174,
        name: "CropSourceResolution",
        format: SubFormat::DoubleHalved,
    },
    Field {
        key: 182,
        name: "CropOutputResolution",
        format: SubFormat::Double,
    },
    Field {
        key: 190,
        name: "CropOutputScale",
        format: SubFormat::Double,
    },
    Field {
        key: 198,
        name: "CropOutputWidth",
        format: SubFormat::Double,
    },
    Field {
        key: 206,
        name: "CropOutputHeight",
        format: SubFormat::Double,
    },
    Field {
        key: 214,
        name: "CropOutputPixels",
        format: SubFormat::Double,
    },
];

const NOISE_REDUCTION: &[Field] = &[
    Field {
        key: 4,
        name: "EdgeNoiseReduction",
        format: SubFormat::U8Enum(OFF_ON),
    },
    Field {
        key: 5,
        name: "ColorMoireReductionMode",
        format: SubFormat::U8Enum(&[(0, "Off"), (1, "Low"), (2, "Medium"), (3, "High")]),
    },
    Field {
        key: 9,
        name: "NoiseReductionIntensity",
        format: SubFormat::U32,
    },
    Field {
        key: 13,
        name: "NoiseReductionSharpness",
        format: SubFormat::U32,
    },
    Field {
        key: 17,
        name: "NoiseReductionMethod",
        format: SubFormat::U16Enum(&[
            (0, "Faster"),
            (1, "Better Quality"),
            (2, "Better Quality 2013"),
        ]),
    },
];

const UNSHARP_DATA: &[Field] = &[
    Field {
        key: 0,
        name: "UnsharpCount",
        format: SubFormat::U8,
    },
    Field {
        key: 19,
        name: "Unsharp1Color",
        format: SubFormat::U16Enum(&[
            (0, "RGB"),
            (1, "Red"),
            (2, "Green"),
            (3, "Blue"),
            (4, "Yellow"),
            (5, "Magenta"),
            (6, "Cyan"),
        ]),
    },
    Field {
        key: 23,
        name: "Unsharp1Intensity",
        format: SubFormat::U16,
    },
    Field {
        key: 25,
        name: "Unsharp1HaloWidth",
        format: SubFormat::U16,
    },
    Field {
        key: 27,
        name: "Unsharp1Threshold",
        format: SubFormat::U8,
    },
];

const WB_ADJ: &[Field] = &[
    Field {
        key: 0,
        name: "WBAdjRedBalance",
        format: SubFormat::Double,
    },
    Field {
        key: 8,
        name: "WBAdjBlueBalance",
        format: SubFormat::Double,
    },
    Field {
        key: 16,
        name: "WBAdjMode",
        format: SubFormat::U8Enum(&[
            (1, "Use Gray Point"),
            (2, "Recorded Value"),
            (3, "Use Temperature"),
            (4, "Calculate Automatically"),
            (5, "Auto2"),
            (6, "Underwater"),
            (7, "Auto1"),
        ]),
    },
    Field {
        key: 20,
        name: "WBAdjLighting",
        format: SubFormat::U16Enum(&[
            (0, "None"),
            (256, "Incandescent"),
            (512, "Daylight (direct sunlight)"),
            (513, "Daylight (shade)"),
            (514, "Daylight (cloudy)"),
            (768, "Standard Fluorescent (warm white)"),
            (769, "Standard Fluorescent (3700K)"),
            (770, "Standard Fluorescent (cool white)"),
            (771, "Standard Fluorescent (5000K)"),
            (772, "Standard Fluorescent (daylight)"),
            (773, "Standard Fluorescent (high temperature mercury vapor)"),
            (1024, "High Color Rendering Fluorescent (warm white)"),
            (1025, "High Color Rendering Fluorescent (3700K)"),
            (1026, "High Color Rendering Fluorescent (cool white)"),
            (1027, "High Color Rendering Fluorescent (5000K)"),
            (1028, "High Color Rendering Fluorescent (daylight)"),
            (1280, "Flash"),
            (1281, "Flash (FL-G1 filter)"),
            (1282, "Flash (FL-G2 filter)"),
            (1283, "Flash (TN-A1 filter)"),
            (1284, "Flash (TN-A2 filter)"),
            (1536, "Sodium Vapor Lamps"),
        ]),
    },
    Field {
        key: 24,
        name: "WBAdjTemperature",
        format: SubFormat::U16,
    },
];

const PHOTO_EFFECTS: &[Field] = &[
    Field {
        key: 0,
        name: "PhotoEffectsType",
        format: SubFormat::U8Enum(&[(0, "None"), (1, "B&W"), (2, "Sepia"), (3, "Tinted")]),
    },
    Field {
        key: 4,
        name: "PhotoEffectsRed",
        format: SubFormat::I16,
    },
    Field {
        key: 6,
        name: "PhotoEffectsGreen",
        format: SubFormat::I16,
    },
    Field {
        key: 8,
        name: "PhotoEffectsBlue",
        format: SubFormat::I16,
    },
];

// FORMAT => 'int32u': keys are int32u INDICES, so 0,1,2 are bytes 0,4,8.
const D_LIGHTING_HQ: &[Field] = &[
    Field {
        key: 0,
        name: "D-LightingHQShadow",
        format: SubFormat::U32,
    },
    Field {
        key: 1,
        name: "D-LightingHQHighlight",
        format: SubFormat::U32,
    },
    Field {
        key: 2,
        name: "D-LightingHQColorBoost",
        format: SubFormat::U32,
    },
];

const D_LIGHTING_HS: &[Field] = &[
    Field {
        key: 0,
        name: "D-LightingHSAdjustment",
        format: SubFormat::U32,
    },
    Field {
        key: 1,
        name: "D-LightingHSColorBoost",
        format: SubFormat::U32,
    },
];

const BRIGHTNESS: &[Field] = &[
    Field {
        key: 0,
        name: "BrightnessAdj",
        format: SubFormat::Double,
    },
    Field {
        key: 8,
        name: "EnhanceDarkTones",
        format: SubFormat::U8Enum(OFF_ON),
    },
];

const COLOR_BOOST: &[Field] = &[
    Field {
        key: 0,
        name: "ColorBoostType",
        format: SubFormat::U8Enum(&[(0, "Nature"), (1, "People")]),
    },
    Field {
        key: 1,
        name: "ColorBoostLevel",
        format: SubFormat::U32,
    },
];

const EXPOSURE: &[Field] = &[
    Field {
        key: 0,
        name: "ExposureAdj",
        format: SubFormat::I16Hundredths,
    },
    Field {
        key: 18,
        name: "ExposureAdj2",
        format: SubFormat::Double4dp,
    },
];

const RED_EYE: &[Field] = &[Field {
    key: 0,
    name: "RedEyeCorrection",
    format: SubFormat::U8Enum(&[(0, "Off"), (1, "Automatic"), (2, "Click on Eyes")]),
}];

/// Main-table ids that carry a sub-table rather than a scalar.
///
/// These ids are absent from `exiftool -f -listx` -- a SubDirectory has no
/// printable value of its own, the same reason ExifOffset and GPSInfo are
/// missing from it -- so they come from NikonCapture.pm directly.
const SUBDIRS: &[(u32, &str, SubTable)] = &[
    (
        0x3742_33e0,
        "CropData",
        SubTable {
            stride: 1,
            fields: CROP_DATA,
        },
    ),
    (
        0x926f_13e0,
        "NoiseReductionData",
        SubTable {
            stride: 1,
            fields: NOISE_REDUCTION,
        },
    ),
    (
        0xe42b_5161,
        "UnsharpData",
        SubTable {
            stride: 1,
            fields: UNSHARP_DATA,
        },
    ),
    (
        0xbf3c_6c20,
        "WBAdjData",
        SubTable {
            stride: 1,
            fields: WB_ADJ,
        },
    ),
    (
        0xb038_4e1e,
        "PhotoEffectsData",
        SubTable {
            stride: 1,
            fields: PHOTO_EFFECTS,
        },
    ),
    (
        0x890f_f591,
        "D-LightingHQData",
        SubTable {
            stride: 4,
            fields: D_LIGHTING_HQ,
        },
    ),
    (
        0xe37b_4337,
        "D-LightingHSData",
        SubTable {
            stride: 4,
            fields: D_LIGHTING_HS,
        },
    ),
    (
        0x8458_9434,
        "BrightnessData",
        SubTable {
            stride: 1,
            fields: BRIGHTNESS,
        },
    ),
    (
        0xb999_a36f,
        "ColorBoostData",
        SubTable {
            stride: 1,
            fields: COLOR_BOOST,
        },
    ),
    (
        0x56a5_4260,
        "Exposure",
        SubTable {
            stride: 1,
            fields: EXPOSURE,
        },
    ),
    (
        0x3cfc_73c6,
        "RedEyeData",
        SubTable {
            stride: 1,
            fields: RED_EYE,
        },
    ),
];

fn render_sub(bytes: &[u8], format: SubFormat) -> Option<String> {
    fn u16le(b: &[u8]) -> Option<u16> {
        Some(u16::from_le_bytes([*b.first()?, *b.get(1)?]))
    }
    Some(match format {
        SubFormat::U8 => bytes.first()?.to_string(),
        SubFormat::U8Enum(t) => {
            let v = *bytes.first()?;
            // An unlisted code reports itself rather than being mapped to a
            // neighbouring label.
            t.iter()
                .find(|(k, _)| *k == v)
                .map(|(_, s)| s.to_string())
                .unwrap_or_else(|| v.to_string())
        }
        SubFormat::U16 => u16le(bytes)?.to_string(),
        SubFormat::U16Enum(t) => {
            let v = u16le(bytes)?;
            t.iter()
                .find(|(k, _)| *k == v)
                .map(|(_, s)| s.to_string())
                .unwrap_or_else(|| v.to_string())
        }
        SubFormat::I16 => i16::from_le_bytes([*bytes.first()?, *bytes.get(1)?]).to_string(),
        SubFormat::I16Hundredths => {
            let v = i16::from_le_bytes([*bytes.first()?, *bytes.get(1)?]);
            perl_number(f64::from(v) / 100.0)
        }
        SubFormat::Double4dp => {
            format!(
                "{:.4}",
                f64::from_le_bytes(bytes.get(..8)?.try_into().ok()?)
            )
        }
        SubFormat::U32 => u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?).to_string(),
        SubFormat::Double | SubFormat::DoubleHalved => {
            let mut v = f64::from_le_bytes(bytes.get(..8)?.try_into().ok()?);
            if format == SubFormat::DoubleHalved {
                v /= 2.0;
            }
            perl_number(v)
        }
    })
}

fn parse_sub_table(data: &[u8], table: &SubTable, tags: &mut HashMap<String, String>) {
    for f in table.fields {
        let at = f.key * table.stride;
        if at >= data.len() {
            continue;
        }
        if let Some(v) = render_sub(&data[at..], f.format) {
            tags.insert(format!("Nikon:{}", f.name), v);
        }
    }
}

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

        if let Some((_, name, format)) = MAIN_TAGS.iter().find(|(tid, _, _)| *tid == id) {
            if let Some(rendered) = render(&data[pos..pos + size], *format) {
                tags.insert(format!("Nikon:{}", name), rendered);
            }
        } else if let Some((_, _, table)) = SUBDIRS.iter().find(|(tid, _, _)| *tid == id) {
            parse_sub_table(&data[pos..pos + size], table, tags);
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
