//! ICC Profile header parsing
//!
//! This module contains functions for extracting metadata fields from the
//! 128-byte ICC profile header.

use super::binary::{read_s15fixed16, read_signature, read_u16_be, read_u32_be, read_u64_be};
use super::registries::{
    CMM_TYPES, HeaderField, MANUFACTURERS, PLATFORMS, PROFILE_CLASSES, RENDERING_INTENTS,
    lookup_in_table,
};
use crate::core::TagValue;
use crate::error::Result;
use std::collections::HashMap;

// ============================================================================
// HEADER FIELD REGISTRY
// ============================================================================

/// Header field definitions for structured parsing
///
/// Each entry defines the offset, name, and extractor function for a
/// specific field in the 128-byte ICC header.
pub static HEADER_FIELDS: &[HeaderField] = &[
    HeaderField {
        offset: 4,
        name: "ProfileCMMType",
        extract: extract_cmm_type,
    },
    HeaderField {
        offset: 8,
        name: "ProfileVersion",
        extract: extract_version,
    },
    HeaderField {
        offset: 12,
        name: "ProfileClass",
        extract: extract_profile_class,
    },
    HeaderField {
        offset: 16,
        name: "ColorSpaceData",
        extract: extract_color_space,
    },
    HeaderField {
        offset: 20,
        name: "ProfileConnectionSpace",
        extract: extract_pcs,
    },
    HeaderField {
        offset: 24,
        name: "ProfileDateTime",
        extract: extract_datetime,
    },
    HeaderField {
        offset: 36,
        name: "ProfileFileSignature",
        extract: extract_signature_field,
    },
    HeaderField {
        offset: 40,
        name: "PrimaryPlatform",
        extract: extract_platform,
    },
    HeaderField {
        offset: 44,
        name: "CMMFlags",
        extract: extract_flags,
    },
    HeaderField {
        offset: 48,
        name: "DeviceManufacturer",
        extract: extract_manufacturer,
    },
    HeaderField {
        offset: 52,
        name: "DeviceModel",
        extract: extract_model,
    },
    HeaderField {
        offset: 56,
        name: "DeviceAttributes",
        extract: extract_attributes,
    },
    HeaderField {
        offset: 64,
        name: "RenderingIntent",
        extract: extract_rendering_intent,
    },
    HeaderField {
        offset: 68,
        name: "ConnectionSpaceIlluminant",
        extract: extract_illuminant,
    },
    HeaderField {
        offset: 80,
        name: "ProfileCreator",
        extract: extract_creator,
    },
    HeaderField {
        offset: 84,
        name: "ProfileID",
        extract: extract_profile_id,
    },
];

/// Parses ICC header using the header field registry
///
/// This function iterates through the header field registry and extracts
/// each field using its associated extractor function.
pub fn parse_header_registry(data: &[u8], metadata: &mut HashMap<String, TagValue>) -> Result<()> {
    for field in HEADER_FIELDS {
        if data.len() >= field.offset + 4 {
            // Call the extractor function for this field
            (field.extract)(data, field.offset, metadata)?;
        }
    }
    Ok(())
}

// ============================================================================
// HEADER FIELD EXTRACTORS
// ============================================================================

/// Extracts CMM type from header
///
/// Reads the 4-character CMM signature and converts it to a human-readable
/// name using the CMM_TYPES lookup table.
fn extract_cmm_type(
    data: &[u8],
    offset: usize,
    metadata: &mut HashMap<String, TagValue>,
) -> Result<()> {
    let cmm_type = read_signature(data, offset)?;
    let trimmed = cmm_type.trim();
    if !trimmed.is_empty() {
        // Look up the CMM type in the registry to get a human-readable name
        let cmm_name = lookup_in_table(CMM_TYPES, trimmed);
        metadata.insert(
            "ProfileCMMType".to_string(),
            TagValue::new_string(cmm_name.to_string()),
        );
    }
    Ok(())
}

/// Extracts profile version from header
fn extract_version(
    data: &[u8],
    offset: usize,
    metadata: &mut HashMap<String, TagValue>,
) -> Result<()> {
    let version = format!(
        "{}.{}.{}",
        data.get(offset).copied().unwrap_or(0),
        (data.get(offset + 1).copied().unwrap_or(0) >> 4) & 0x0F,
        data.get(offset + 1).copied().unwrap_or(0) & 0x0F
    );
    metadata.insert("ProfileVersion".to_string(), TagValue::new_string(version));
    Ok(())
}

/// Extracts profile class from header
fn extract_profile_class(
    data: &[u8],
    offset: usize,
    metadata: &mut HashMap<String, TagValue>,
) -> Result<()> {
    let class = read_signature(data, offset)?;
    let class_name = lookup_in_table(PROFILE_CLASSES, &class);
    metadata.insert(
        "ProfileClass".to_string(),
        TagValue::new_string(class_name.to_string()),
    );
    Ok(())
}

/// Extracts color space from header
fn extract_color_space(
    data: &[u8],
    offset: usize,
    metadata: &mut HashMap<String, TagValue>,
) -> Result<()> {
    let color_space = read_signature(data, offset)?.trim().to_string();
    metadata.insert(
        "ColorSpaceData".to_string(),
        TagValue::new_string(color_space),
    );
    Ok(())
}

/// Extracts Profile Connection Space from header
fn extract_pcs(data: &[u8], offset: usize, metadata: &mut HashMap<String, TagValue>) -> Result<()> {
    let pcs = read_signature(data, offset)?.trim().to_string();
    metadata.insert(
        "ProfileConnectionSpace".to_string(),
        TagValue::new_string(pcs),
    );
    Ok(())
}

/// Extracts date/time from header
fn extract_datetime(
    data: &[u8],
    offset: usize,
    metadata: &mut HashMap<String, TagValue>,
) -> Result<()> {
    if data.len() < offset + 12 {
        return Ok(());
    }

    let year = read_u16_be(data, offset)?;
    let month = read_u16_be(data, offset + 2)?;
    let day = read_u16_be(data, offset + 4)?;
    let hour = read_u16_be(data, offset + 6)?;
    let minute = read_u16_be(data, offset + 8)?;
    let second = read_u16_be(data, offset + 10)?;

    let datetime = format!(
        "{}:{:02}:{:02} {:02}:{:02}:{:02}",
        year, month, day, hour, minute, second
    );
    metadata.insert(
        "ProfileDateTime".to_string(),
        TagValue::new_string(datetime),
    );
    Ok(())
}

/// Extracts signature from header
fn extract_signature_field(
    data: &[u8],
    offset: usize,
    metadata: &mut HashMap<String, TagValue>,
) -> Result<()> {
    let signature = read_signature(data, offset)?;
    metadata.insert(
        "ProfileFileSignature".to_string(),
        TagValue::new_string(signature),
    );
    Ok(())
}

/// Extracts primary platform from header
fn extract_platform(
    data: &[u8],
    offset: usize,
    metadata: &mut HashMap<String, TagValue>,
) -> Result<()> {
    let platform = read_signature(data, offset)?;
    let platform_name = lookup_in_table(PLATFORMS, &platform);
    if !platform_name.is_empty() {
        metadata.insert(
            "PrimaryPlatform".to_string(),
            TagValue::new_string(platform_name.to_string()),
        );
    }
    Ok(())
}

/// Extracts CMM flags from header
fn extract_flags(
    data: &[u8],
    offset: usize,
    metadata: &mut HashMap<String, TagValue>,
) -> Result<()> {
    let flags = read_u32_be(data, offset)?;
    let embedded = if flags & 0x01 == 0 {
        "Not Embedded"
    } else {
        "Embedded"
    };
    let independent = if flags & 0x02 == 0 {
        "Independent"
    } else {
        "Dependent"
    };
    metadata.insert(
        "CMMFlags".to_string(),
        TagValue::new_string(format!("{}, {}", embedded, independent)),
    );
    Ok(())
}

/// Extracts device manufacturer from header
///
/// Reads the 4-character manufacturer signature and converts it to a
/// human-readable name using the MANUFACTURERS lookup table.
fn extract_manufacturer(
    data: &[u8],
    offset: usize,
    metadata: &mut HashMap<String, TagValue>,
) -> Result<()> {
    if data.len() >= offset + 4 {
        let manufacturer = read_signature(data, offset)?;
        // Look up the manufacturer in the registry to get a human-readable name
        // "none" code returns "none" to match ExifTool behavior
        let manufacturer_name = lookup_in_table(MANUFACTURERS, &manufacturer);
        if !manufacturer_name.is_empty() {
            metadata.insert(
                "DeviceManufacturer".to_string(),
                TagValue::new_string(manufacturer_name.to_string()),
            );
        }
    }
    Ok(())
}

/// Extracts device model from header
/// Empty, null-byte, or non-printable models are reported as empty string to match ExifTool.
fn extract_model(
    data: &[u8],
    offset: usize,
    metadata: &mut HashMap<String, TagValue>,
) -> Result<()> {
    if data.len() >= offset + 4 {
        let model = read_signature(data, offset)?;
        // If the model contains non-printable characters or is empty, report as empty
        let is_printable = model.chars().all(|c| c.is_ascii_graphic() || c == ' ');
        let value = if model.is_empty() || !is_printable {
            String::new()
        } else {
            model
        };
        metadata.insert("DeviceModel".to_string(), TagValue::new_string(value));
    }
    Ok(())
}

/// Extracts device attributes from header
fn extract_attributes(
    data: &[u8],
    offset: usize,
    metadata: &mut HashMap<String, TagValue>,
) -> Result<()> {
    if data.len() < offset + 8 {
        return Ok(());
    }

    let attrs = read_u64_be(data, offset)?;
    let reflective = if attrs & 0x01 == 0 {
        "Reflective"
    } else {
        "Transparency"
    };
    let glossy = if attrs & 0x02 == 0 { "Glossy" } else { "Matte" };
    let positive = if attrs & 0x04 == 0 {
        "Positive"
    } else {
        "Negative"
    };
    let color = if attrs & 0x08 == 0 { "Color" } else { "B&W" };

    metadata.insert(
        "DeviceAttributes".to_string(),
        TagValue::new_string(format!(
            "{}, {}, {}, {}",
            reflective, glossy, positive, color
        )),
    );
    Ok(())
}

/// Extracts rendering intent from header
fn extract_rendering_intent(
    data: &[u8],
    offset: usize,
    metadata: &mut HashMap<String, TagValue>,
) -> Result<()> {
    if data.len() < offset + 4 {
        return Ok(());
    }

    let intent = read_u32_be(data, offset)?;
    let intent_name = RENDERING_INTENTS
        .get(intent as usize)
        .copied()
        .unwrap_or("Unknown");
    metadata.insert(
        "RenderingIntent".to_string(),
        TagValue::new_string(intent_name.to_string()),
    );
    Ok(())
}

/// Extracts connection space illuminant from header
fn extract_illuminant(
    data: &[u8],
    offset: usize,
    metadata: &mut HashMap<String, TagValue>,
) -> Result<()> {
    if data.len() >= offset + 12 {
        let x = read_s15fixed16(data, offset)?;
        let y = read_s15fixed16(data, offset + 4)?;
        let z = read_s15fixed16(data, offset + 8)?;
        metadata.insert(
            "ConnectionSpaceIlluminant".to_string(),
            TagValue::new_string(format!("{} {} {}", x, y, z)),
        );
    }
    Ok(())
}

/// Extracts profile creator from header
///
/// Reads the 4-character creator signature and converts it to a
/// human-readable name using the MANUFACTURERS lookup table.
/// Empty or null-byte creators are reported as empty string to match ExifTool.
fn extract_creator(
    data: &[u8],
    offset: usize,
    metadata: &mut HashMap<String, TagValue>,
) -> Result<()> {
    if data.len() >= offset + 4 {
        let creator = read_signature(data, offset)?;
        let trimmed = creator.trim();
        if trimmed.is_empty() {
            // Empty creator - ExifTool shows empty string
            metadata.insert(
                "ProfileCreator".to_string(),
                TagValue::new_string(String::new()),
            );
        } else {
            // Look up the creator in the registry to get a human-readable name
            let creator_name = lookup_in_table(MANUFACTURERS, trimmed);
            metadata.insert(
                "ProfileCreator".to_string(),
                TagValue::new_string(creator_name.to_string()),
            );
        }
    }
    Ok(())
}

/// Extracts profile ID (MD5 hash) from header
fn extract_profile_id(
    data: &[u8],
    offset: usize,
    metadata: &mut HashMap<String, TagValue>,
) -> Result<()> {
    if data.len() >= offset + 16 {
        let id_bytes = &data[offset..offset + 16];
        if id_bytes.iter().all(|&b| b == 0) {
            metadata.insert(
                "ProfileID".to_string(),
                TagValue::new_string("0".to_string()),
            );
        } else {
            let id_hex = id_bytes
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();
            metadata.insert("ProfileID".to_string(), TagValue::new_string(id_hex));
        }
    }
    Ok(())
}
