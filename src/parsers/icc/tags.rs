//! ICC Profile tag parsing
//!
//! This module contains functions for decoding ICC profile tags from
//! the tag table section of an ICC profile.

use super::binary::{read_s15fixed16, read_signature, read_u16fixed16, read_u32_be};
use super::registries::{
    GEOMETRY_TYPES, ILLUMINANT_TYPES, OBSERVER_TYPES, TAG_REGISTRY, TECHNOLOGIES, TagType,
    lookup_in_table,
};
use crate::core::TagValue;
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use std::collections::HashMap;

/// Parses ICC tags using the tag registry
///
/// This function reads the tag table and dispatches each tag to its
/// appropriate decoder based on the tag type in the registry.
pub fn parse_tags_registry(data: &[u8], metadata: &mut HashMap<String, TagValue>) -> Result<()> {
    if data.len() < 132 {
        return Ok(());
    }

    let tag_count = read_u32_be(data, 128)?;

    for i in 0..tag_count {
        let entry_offset = 132 + (i * 12) as usize;
        if entry_offset + 12 > data.len() {
            break;
        }

        let tag_signature = read_signature(data, entry_offset)?;
        let tag_offset = read_u32_be(data, entry_offset + 4)? as usize;
        let tag_size = read_u32_be(data, entry_offset + 8)? as usize;

        if tag_offset >= data.len() || tag_offset + tag_size > data.len() {
            continue;
        }

        let tag_data = &data[tag_offset..tag_offset + tag_size];

        // Look up tag in registry and decode
        decode_tag(tag_signature.trim(), tag_data, tag_size, metadata);
    }

    Ok(())
}

/// Decodes a single tag using the tag registry
///
/// This function looks up the tag signature in the registry and calls
/// the appropriate decoder based on the tag type.
fn decode_tag(signature: &str, data: &[u8], size: usize, metadata: &mut HashMap<String, TagValue>) {
    // Find tag in registry
    let tag_def = TAG_REGISTRY.iter().find(|t| t.signature == signature);

    if let Some(def) = tag_def {
        // Decode based on tag type
        let result = match def.tag_type {
            TagType::TextDescription => parse_text_description_type(data)
                .ok()
                .map(TagValue::new_string),
            TagType::Text => parse_text_type(data).ok().map(TagValue::new_string),
            TagType::Xyz => parse_xyz_type(data)
                .ok()
                .map(|(x, y, z)| TagValue::new_string(format!("{} {} {}", x, y, z))),
            TagType::Curve => Some(TagValue::new_string(format!(
                "(Binary data {} bytes, use -b option to extract)",
                size
            ))),
            TagType::ViewingConditions => parse_viewing_conditions(data)
                .ok()
                .and_then(|vc| decode_viewing_conditions(vc, metadata)),
            TagType::Measurement => parse_measurement(data)
                .ok()
                .and_then(|m| decode_measurement(m, metadata)),
            TagType::Signature => parse_signature_type(data).ok().map(|sig| {
                let name = lookup_in_table(TECHNOLOGIES, &sig);
                TagValue::new_string(name.to_string())
            }),
        };

        if let Some(value) = result {
            metadata.insert(def.name.to_string(), value);
        }
    }
}

/// Decodes viewing conditions into multiple metadata entries
fn decode_viewing_conditions(
    vc: HashMap<String, String>,
    metadata: &mut HashMap<String, TagValue>,
) -> Option<TagValue> {
    if let Some(illuminant) = vc.get("illuminant") {
        metadata.insert(
            "ViewingCondIlluminant".to_string(),
            TagValue::new_string(illuminant.clone()),
        );
    }
    if let Some(surround) = vc.get("surround") {
        metadata.insert(
            "ViewingCondSurround".to_string(),
            TagValue::new_string(surround.clone()),
        );
    }
    if let Some(illum_type) = vc.get("illuminant_type") {
        metadata.insert(
            "ViewingCondIlluminantType".to_string(),
            TagValue::new_string(illum_type.clone()),
        );
    }
    None // This tag produces multiple entries, not a single value
}

/// Decodes measurement data into multiple metadata entries
fn decode_measurement(
    m: HashMap<String, String>,
    metadata: &mut HashMap<String, TagValue>,
) -> Option<TagValue> {
    if let Some(observer) = m.get("observer") {
        metadata.insert(
            "MeasurementObserver".to_string(),
            TagValue::new_string(observer.clone()),
        );
    }
    if let Some(backing) = m.get("backing") {
        metadata.insert(
            "MeasurementBacking".to_string(),
            TagValue::new_string(backing.clone()),
        );
    }
    if let Some(geometry) = m.get("geometry") {
        metadata.insert(
            "MeasurementGeometry".to_string(),
            TagValue::new_string(geometry.clone()),
        );
    }
    if let Some(flare) = m.get("flare") {
        metadata.insert(
            "MeasurementFlare".to_string(),
            TagValue::new_string(flare.clone()),
        );
    }
    if let Some(illuminant) = m.get("illuminant") {
        metadata.insert(
            "MeasurementIlluminant".to_string(),
            TagValue::new_string(illuminant.clone()),
        );
    }
    None // This tag produces multiple entries, not a single value
}

// ============================================================================
// ICC DATA TYPE PARSERS
// ============================================================================

/// Parses ICC textDescriptionType (old-style text)
fn parse_text_description_type(data: &[u8]) -> Result<String> {
    if data.len() < 12 {
        return Err(ExifToolError::parse_error("textDescriptionType too small"));
    }

    let type_sig = read_signature(data, 0)?;

    if type_sig.trim() == "desc" {
        let ascii_count = read_u32_be(data, 8)? as usize;
        if ascii_count > 0 && data.len() >= 12 + ascii_count {
            let text_bytes = &data[12..12 + ascii_count];
            let text_len = text_bytes
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(text_bytes.len());
            return Ok(String::from_utf8_lossy(&text_bytes[..text_len]).to_string());
        }
    } else if type_sig.trim() == "mluc" {
        return parse_mluc_type(data);
    }

    Err(ExifToolError::parse_error("Invalid text description type"))
}

/// Parses ICC multiLocalizedUnicodeType (modern text format)
fn parse_mluc_type(data: &[u8]) -> Result<String> {
    if data.len() < 16 {
        return Err(ExifToolError::parse_error("mluc type too small"));
    }

    let num_records = read_u32_be(data, 8)? as usize;
    if num_records == 0 {
        return Err(ExifToolError::parse_error("mluc has no records"));
    }

    if data.len() < 16 + 12 {
        return Err(ExifToolError::parse_error("mluc record table too small"));
    }

    let str_length = read_u32_be(data, 16 + 8)? as usize;
    let str_offset = read_u32_be(data, 16 + 12)? as usize;

    if str_offset + str_length > data.len() {
        return Err(ExifToolError::parse_error("mluc string out of bounds"));
    }

    let utf16_bytes = &data[str_offset..str_offset + str_length];
    let reader = EndianReader::big_endian(utf16_bytes);
    let u16_vec: Vec<u16> = (0..utf16_bytes.len())
        .step_by(2)
        .filter_map(|offset| reader.u16_at(offset))
        .collect();

    String::from_utf16(&u16_vec)
        .map_err(|_| ExifToolError::parse_error("Invalid UTF-16 in mluc string"))
}

/// Parses ICC textType (simple text)
fn parse_text_type(data: &[u8]) -> Result<String> {
    if data.len() < 8 {
        return Err(ExifToolError::parse_error("textType too small"));
    }

    let type_sig = read_signature(data, 0)?;

    if type_sig.trim() == "text" {
        let text_bytes = &data[8..];
        let text_len = text_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(text_bytes.len());
        return Ok(String::from_utf8_lossy(&text_bytes[..text_len]).to_string());
    } else if type_sig.trim() == "mluc" {
        return parse_mluc_type(data);
    }

    Err(ExifToolError::parse_error("Invalid text type"))
}

/// Parses ICC XYZType (XYZ color values)
fn parse_xyz_type(data: &[u8]) -> Result<(f64, f64, f64)> {
    if data.len() < 20 {
        return Err(ExifToolError::parse_error("XYZType too small"));
    }

    let x = read_s15fixed16(data, 8)?;
    let y = read_s15fixed16(data, 12)?;
    let z = read_s15fixed16(data, 16)?;

    Ok((x, y, z))
}

/// Parses ICC signatureType (4-byte signature)
fn parse_signature_type(data: &[u8]) -> Result<String> {
    if data.len() < 12 {
        return Err(ExifToolError::parse_error("signatureType too small"));
    }

    let sig = read_signature(data, 8)?;
    Ok(sig)
}

/// Parses ICC viewing conditions structure
fn parse_viewing_conditions(data: &[u8]) -> Result<HashMap<String, String>> {
    let mut result = HashMap::new();

    if data.len() < 36 {
        return Err(ExifToolError::parse_error("Viewing conditions too small"));
    }

    let illum_x = read_s15fixed16(data, 8)?;
    let illum_y = read_s15fixed16(data, 12)?;
    let illum_z = read_s15fixed16(data, 16)?;
    result.insert(
        "illuminant".to_string(),
        format!("{} {} {}", illum_x, illum_y, illum_z),
    );

    let surr_x = read_s15fixed16(data, 20)?;
    let surr_y = read_s15fixed16(data, 24)?;
    let surr_z = read_s15fixed16(data, 28)?;
    result.insert(
        "surround".to_string(),
        format!("{} {} {}", surr_x, surr_y, surr_z),
    );

    if data.len() >= 36 {
        let illum_type = read_u32_be(data, 32)?;
        let illum_name = ILLUMINANT_TYPES
            .get(illum_type as usize)
            .copied()
            .unwrap_or("Unknown");
        result.insert("illuminant_type".to_string(), illum_name.to_string());
    }

    Ok(result)
}

/// Parses ICC measurement structure
fn parse_measurement(data: &[u8]) -> Result<HashMap<String, String>> {
    let mut result = HashMap::new();

    if data.len() < 36 {
        return Err(ExifToolError::parse_error("Measurement data too small"));
    }

    let observer = read_u32_be(data, 8)?;
    let observer_name = OBSERVER_TYPES
        .get(observer as usize)
        .copied()
        .unwrap_or("Unknown");
    result.insert("observer".to_string(), observer_name.to_string());

    let back_x = read_s15fixed16(data, 12)?;
    let back_y = read_s15fixed16(data, 16)?;
    let back_z = read_s15fixed16(data, 20)?;
    result.insert(
        "backing".to_string(),
        format!("{} {} {}", back_x, back_y, back_z),
    );

    let geometry = read_u32_be(data, 24)?;
    let geometry_name = GEOMETRY_TYPES
        .get(geometry as usize)
        .copied()
        .unwrap_or("Unknown");
    result.insert("geometry".to_string(), geometry_name.to_string());

    if data.len() >= 32 {
        let flare = read_u16fixed16(data, 28)?;
        // Store raw percentage value with 5 decimal places; % suffix is applied
        // by exiftool_compat when output formatting is requested
        result.insert("flare".to_string(), format!("{:.5}", flare * 100.0));
    }

    if data.len() >= 36 {
        let illuminant = read_u32_be(data, 32)?;
        let illuminant_name = ILLUMINANT_TYPES
            .get(illuminant as usize)
            .copied()
            .unwrap_or("Unknown");
        result.insert("illuminant".to_string(), illuminant_name.to_string());
    }

    Ok(result)
}
