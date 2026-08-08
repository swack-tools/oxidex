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
use crate::core::formatters::perl_number;
use crate::error::{ExifToolError, Result};
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
    let Some(def) = TAG_REGISTRY.iter().find(|t| t.signature == signature) else {
        return;
    };

    // ExifTool checks the PAYLOAD type before the tag's own type: any
    // non-subdirectory tag whose data starts with 'mluc' goes through the
    // multiLocalizedUnicode branch of `ProcessICC_Profile`, which emits one
    // entry per language record instead of a single value. That is how
    // `desc`, `cprt` and `dscm` are read out of v4 / ColorSync profiles.
    if !def.tag_type.is_subdirectory() && size > 4 && data.len() >= 4 && &data[0..4] == b"mluc" {
        decode_multi_localized(def.name, data, size, metadata);
        return;
    }

    // Decode based on tag type
    let result = match def.tag_type {
        TagType::TextDescription => parse_text_description_type(data)
            .ok()
            .map(TagValue::new_string),
        TagType::Text => parse_text_type(data).ok().map(TagValue::new_string),
        TagType::Xyz => parse_xyz_type(data).ok().map(|(x, y, z)| {
            let value = if def.name == "BlueMatrixColumn" {
                format!(
                    "{} {} {}",
                    format_fixed32s(x),
                    format_fixed32s(y),
                    format_fixed32s(z)
                )
            } else {
                format!("{} {} {}", x, y, z)
            };
            TagValue::new_string(value)
        }),
        TagType::Curve | TagType::Binary => Some(binary_placeholder(size)),
        TagType::S15Fixed16Array => Some(
            parse_s15fixed16_array(data)
                .map(TagValue::new_string)
                // FormatICCTag returns undef for a payload that is not really
                // an 'sf32' array, and ExifTool then stores the raw bytes.
                .unwrap_or_else(|_| binary_placeholder(size)),
        ),
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

/// Renders ExifTool's placeholder for a payload it has no formatter for.
///
/// When `FormatICCTag` returns undef, `ProcessICC_Profile` stores the raw tag
/// bytes and they print as `(Binary data N bytes, use -b option to extract)`,
/// where N is the size the tag table declares - not the length of any decoded
/// sub-structure.
fn binary_placeholder(size: usize) -> TagValue {
    TagValue::new_string(format!(
        "(Binary data {} bytes, use -b option to extract)",
        size
    ))
}

/// Decodes an ICC multiLocalizedUnicodeType (`mluc`) payload into one metadata
/// entry per language record.
///
/// Mirrors the `$fmt eq 'mluc'` branch of ExifTool's `ProcessICC_Profile`: a
/// record whose language code is `en-US`, or is not a well-formed pair of
/// two-letter codes at all, lands on the bare tag name; every other record gets
/// a `-<lang>-<COUNTRY>` suffix. Records are applied in file order and a later
/// one overwrites an earlier one on the same key, which is how ExifTool's
/// duplicate-tag priority resolves two records that both map to the bare name.
fn decode_multi_localized(
    base_name: &str,
    data: &[u8],
    size: usize,
    metadata: &mut HashMap<String, TagValue>,
) {
    // ExifTool: `next if $size < 28` - too small to hold a single record.
    if size < 28 {
        return;
    }
    let (Ok(count), Ok(record_len)) = (read_u32_be(data, 8), read_u32_be(data, 12)) else {
        return;
    };
    // ExifTool: `next if $recLen < 12`.
    if record_len < 12 {
        return;
    }
    let record_len = record_len as usize;

    for index in 0..count as usize {
        // Record table starts 16 bytes into the payload.
        let Some(record_pos) = index
            .checked_mul(record_len)
            .and_then(|offset| offset.checked_add(16))
        else {
            break;
        };
        match record_pos.checked_add(record_len) {
            Some(record_end) if record_end <= size && record_end <= data.len() => {}
            _ => break,
        }

        let (Ok(str_len), Ok(str_pos)) = (
            read_u32_be(data, record_pos + 4),
            read_u32_be(data, record_pos + 8),
        ) else {
            break;
        };
        // String offsets are relative to the start of the tag payload.
        let (str_len, str_pos) = (str_len as usize, str_pos as usize);
        let Some(str_end) = str_pos.checked_add(str_len) else {
            break;
        };
        if str_end > size || str_end > data.len() {
            break;
        }

        let name = match language_suffix(&data[record_pos..record_pos + 4]) {
            Some(suffix) => format!("{}-{}", base_name, suffix),
            None => base_name.to_string(),
        };
        metadata.insert(
            name,
            TagValue::new_string(decode_utf16_be(&data[str_pos..str_end])),
        );
    }
}

/// Builds ExifTool's language suffix from an `mluc` record's 4-byte code.
///
/// ExifTool applies `s/^([a-z]{2})([A-Z]{2})$/\L$1-\U$2/i` to the raw bytes, so
/// the country code comes out of the file verbatim: ColorSync's misspelled
/// `frFU` stays `fr-FU` and is not corrected to `fr-FR`. A code that is not four
/// ASCII letters (Apple writes a bare `fr\0\0` in some profiles), or that
/// normalizes to `en-US`, carries no suffix and lands on the bare tag name.
fn language_suffix(code: &[u8]) -> Option<String> {
    if code.len() != 4 || !code.iter().all(u8::is_ascii_alphabetic) {
        return None;
    }
    let suffix = format!(
        "{}-{}",
        String::from_utf8_lossy(&code[0..2]).to_ascii_lowercase(),
        String::from_utf8_lossy(&code[2..4]).to_ascii_uppercase()
    );
    if suffix == "en-US" {
        None
    } else {
        Some(suffix)
    }
}

/// Decodes big-endian UTF-16 the way ExifTool's `Decode($val, 'UTF16')` does.
///
/// ExifTool recomposes to UTF-8 through `Charset::Recompose`, which truncates at
/// the first NUL (`$outVal =~ s/\0.*//s`). That is what drops the terminator
/// ColorSync counts inside `strLen` on profiles such as Google's sRGB, where the
/// record holds `"sRGB IEC61966-2.1\0"`.
fn decode_utf16_be(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    let text = String::from_utf16_lossy(&units);
    match text.find('\0') {
        Some(nul) => text[..nul].to_string(),
        None => text,
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
    }

    Err(ExifToolError::parse_error("Invalid text description type"))
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
    }

    Err(ExifToolError::parse_error("Invalid text type"))
}

/// Parses ICC s15Fixed16ArrayType (`sf32`) into a space-separated value list.
///
/// ExifTool's `FormatICCTag` reads `($size - 8) / 4` `fixed32s` values and joins
/// them with single spaces; the 5-decimal rounding ExifTool applies in
/// `GetFixed32s` is done downstream by the ICC matrix-tag formatter, the same
/// way `MediaWhitePoint` and the XYZ colorant tags are handled here.
fn parse_s15fixed16_array(data: &[u8]) -> Result<String> {
    if data.len() < 12 {
        return Err(ExifToolError::parse_error("s15Fixed16Array too small"));
    }
    if read_signature(data, 0)? != "sf32" {
        return Err(ExifToolError::parse_error("Not an s15Fixed16Array"));
    }

    let count = (data.len() - 8) / 4;
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        values.push(read_s15fixed16(data, 8 + index * 4)?.to_string());
    }

    Ok(values.join(" "))
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

/// ExifTool's `GetFixed32s` removes insignificant digits at five decimal
/// places before rendering the value.
fn format_fixed32s(value: f64) -> String {
    let adjustment = if value > 0.0 { 0.5 } else { -0.5 };
    perl_number((value * 1e5 + adjustment).trunc() / 1e5)
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
        // ICC_Profile.pm:874-878 declares MeasurementFlare as
        // `Format => 'fixed32u'` with `PrintConv => '$val*100 . "%"'`, and the
        // fixed32u reader drops the insignificant digits before the PrintConv
        // ever sees them: `int((Get32u(...) / 0x10000) * 1e5 + 0.5) / 1e5`
        // (ExifTool.pm:6116-6121). ExifTool.tif stores 0x0000028f, so
        // 655/65536 = 0.0099945068... rounds to 0.00999 and prints `0.999%`;
        // carrying five decimals of the ALREADY-multiplied percentage instead
        // printed `0.99945%`.
        //
        // Round at the fixed32u stage, then multiply, then let the number
        // print as Perl would (trailing zeros suppressed).
        let rounded = (flare * 1e5 + 0.5).floor() / 1e5;
        result.insert("flare".to_string(), perl_number(rounded * 100.0));
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

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a parseable ICC profile body: a zeroed 128-byte header, a tag
    /// table, then the payloads laid out end to end.
    fn build_profile(tags: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut profile = vec![0u8; 128];
        profile.extend_from_slice(&(tags.len() as u32).to_be_bytes());

        let mut offset = 132 + tags.len() * 12;
        for (signature, payload) in tags {
            profile.extend_from_slice(signature.as_bytes());
            profile.extend_from_slice(&(offset as u32).to_be_bytes());
            profile.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            offset += payload.len();
        }
        for (_, payload) in tags {
            profile.extend_from_slice(payload);
        }
        profile
    }

    /// Builds a multiLocalizedUnicode payload from (language code, text) pairs.
    fn build_mluc(records: &[(&str, &str)]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(b"mluc");
        payload.extend_from_slice(&0u32.to_be_bytes());
        payload.extend_from_slice(&(records.len() as u32).to_be_bytes());
        payload.extend_from_slice(&12u32.to_be_bytes());

        let strings: Vec<Vec<u8>> = records
            .iter()
            .map(|(_, text)| text.encode_utf16().flat_map(u16::to_be_bytes).collect())
            .collect();

        let mut string_offset = 16 + records.len() * 12;
        for ((language, _), encoded) in records.iter().zip(&strings) {
            payload.extend_from_slice(language.as_bytes());
            payload.extend_from_slice(&(encoded.len() as u32).to_be_bytes());
            payload.extend_from_slice(&(string_offset as u32).to_be_bytes());
            string_offset += encoded.len();
        }
        for encoded in &strings {
            payload.extend_from_slice(encoded);
        }
        payload
    }

    fn parse(profile: &[u8]) -> HashMap<String, TagValue> {
        let mut metadata = HashMap::new();
        parse_tags_registry(profile, &mut metadata).expect("tag table should parse");
        metadata
    }

    fn value<'a>(metadata: &'a HashMap<String, TagValue>, name: &str) -> Option<&'a str> {
        metadata.get(name).and_then(TagValue::as_string)
    }

    #[test]
    fn mluc_records_are_split_across_language_suffixed_names() {
        let metadata = parse(&build_profile(&[(
            "dscm",
            build_mluc(&[
                ("enUS", "Camera RGB Profile"),
                ("frFU", "Profil RVB de l'appareil-photo"),
                ("zhCN", "\u{76f8}\u{673a} RGB"),
            ]),
        )]));

        // en-US carries no suffix, so it lands on the bare tag name.
        assert_eq!(
            value(&metadata, "ProfileDescriptionML"),
            Some("Camera RGB Profile")
        );
        // ExifTool takes the country code from the file verbatim: ColorSync's
        // misspelled `frFU` stays `fr-FU` and is NOT corrected to `fr-FR`.
        assert_eq!(
            value(&metadata, "ProfileDescriptionML-fr-FU"),
            Some("Profil RVB de l'appareil-photo")
        );
        assert_eq!(
            value(&metadata, "ProfileDescriptionML-zh-CN"),
            Some("\u{76f8}\u{673a} RGB")
        );
        assert!(!metadata.contains_key("ProfileDescriptionML-en-US"));
    }

    #[test]
    fn mluc_record_with_a_malformed_language_code_takes_the_bare_name() {
        // Apple's Generic RGB profile writes a bare `fr\0\0`. ExifTool's
        // language regex rejects it, so the record lands on the bare tag name
        // and - coming after the en-US record - is the value that survives.
        let metadata = parse(&build_profile(&[(
            "dscm",
            build_mluc(&[
                ("enUS", "Generic RGB Profile"),
                ("fr\0\0", "Profil generique RVB"),
            ]),
        )]));

        assert_eq!(
            value(&metadata, "ProfileDescriptionML"),
            Some("Profil generique RVB")
        );
    }

    #[test]
    fn mluc_text_is_truncated_at_the_first_nul() {
        // ColorSync counts the terminator inside strLen; ExifTool's charset
        // recomposition drops it.
        let metadata = parse(&build_profile(&[(
            "desc",
            build_mluc(&[("enUS", "sRGB IEC61966-2.1\0")]),
        )]));

        assert_eq!(
            value(&metadata, "ProfileDescription"),
            Some("sRGB IEC61966-2.1")
        );
    }

    #[test]
    fn mluc_payload_too_short_for_a_record_emits_nothing() {
        let mut short = Vec::new();
        short.extend_from_slice(b"mluc");
        short.extend_from_slice(&0u32.to_be_bytes());
        short.extend_from_slice(&1u32.to_be_bytes());
        short.extend_from_slice(&12u32.to_be_bytes());

        let metadata = parse(&build_profile(&[("desc", short)]));
        assert!(metadata.is_empty());
    }

    #[test]
    fn chad_decodes_as_an_s15fixed16_array() {
        let raw: [i32; 9] = [68690, 1502, -3290, 1939, 64912, -1118, -605, 988, 49258];
        let mut payload = Vec::new();
        payload.extend_from_slice(b"sf32");
        payload.extend_from_slice(&0u32.to_be_bytes());
        for value in raw {
            payload.extend_from_slice(&value.to_be_bytes());
        }

        let metadata = parse(&build_profile(&[("chad", payload)]));
        let decoded = value(&metadata, "ChromaticAdaptation").expect("chad should decode");

        let numbers: Vec<f64> = decoded
            .split(' ')
            .map(|part| part.parse().expect("each element should be numeric"))
            .collect();
        assert_eq!(numbers.len(), 9);
        for (decoded, expected) in numbers.iter().zip(raw) {
            assert!((decoded - f64::from(expected) / 65536.0).abs() < 1e-12);
        }
    }

    #[test]
    fn blue_matrix_column_matches_exiftool_fixed32_rounding() {
        let mut payload = Vec::new();
        payload.extend_from_slice(b"XYZ ");
        payload.extend_from_slice(&0u32.to_be_bytes());
        for value in [9778i32, 4143, 48800] {
            payload.extend_from_slice(&value.to_be_bytes());
        }

        let metadata = parse(&build_profile(&[("bXYZ", payload)]));

        assert_eq!(
            value(&metadata, "BlueMatrixColumn"),
            Some("0.1492 0.06322 0.74463")
        );
    }

    /// Builds a `curv` tone-reproduction-curve payload with `points` 16-bit
    /// entries: 4-byte signature, 4 reserved, a 4-byte count, then the table.
    /// Apple's Display P3 writes `curv` with 1024 points (2060 bytes) and
    /// ExifTool.jpg writes one with a single point (14 bytes).
    fn build_curv(points: usize) -> Vec<u8> {
        let mut payload = Vec::with_capacity(12 + points * 2);
        payload.extend_from_slice(b"curv");
        payload.extend_from_slice(&0u32.to_be_bytes());
        payload.extend_from_slice(&(points as u32).to_be_bytes());
        payload.extend(std::iter::repeat_n(0u8, points * 2));
        payload
    }

    /// Builds a `para` parametric-curve payload of function type 3: 4-byte
    /// signature, 4 reserved, a 2-byte function type, 2 reserved, then five
    /// s15Fixed16 parameters - 32 bytes, the shape every recent iPhone/iPad
    /// Display P3 profile carries.
    fn build_para() -> Vec<u8> {
        let mut payload = Vec::with_capacity(32);
        payload.extend_from_slice(b"para");
        payload.extend_from_slice(&0u32.to_be_bytes());
        payload.extend_from_slice(&3u16.to_be_bytes());
        payload.extend_from_slice(&0u16.to_be_bytes());
        payload.extend(std::iter::repeat_n(0u8, 20));
        payload
    }

    /// rTRC/gTRC/bTRC are `Name => 'RedTRC'/'GreenTRC'/'BlueTRC'` in
    /// ICC_Profile.pm (449-452, 421-424, 361-364); the long
    /// `Red Tone Reproduction Curve` spelling is only the `Description` and
    /// `-s` never prints it. Emitting the long form hid a byte-exact value
    /// behind a key ExifTool never writes, on all 135 corpus files.
    ///
    /// Ground truth, ExifTool 13.59 (the pin in `.exiftool-version`):
    ///
    /// ```text
    /// $ exiftool -a -G1 -s Apple/Apple_iPadAir_3rd_generation.jpg
    /// [ICC_Profile]   RedTRC     : (Binary data 32 bytes, use -b option to extract)
    /// [ICC_Profile]   GreenTRC   : (Binary data 32 bytes, use -b option to extract)
    /// [ICC_Profile]   BlueTRC    : (Binary data 32 bytes, use -b option to extract)
    ///
    /// $ exiftool -a -G1 -s Apple/Apple_iPadPro.jpg
    /// [ICC_Profile]   RedTRC     : (Binary data 2060 bytes, use -b option to extract)
    /// [ICC_Profile]   GreenTRC   : (Binary data 2060 bytes, use -b option to extract)
    /// [ICC_Profile]   BlueTRC    : (Binary data 2060 bytes, use -b option to extract)
    /// ```
    ///
    /// Both payload shapes are asserted because the count is the tag's own
    /// declared size, not a constant: the iPadAir profile stores a 32-byte
    /// `para` curve and the iPadPro profile a 2060-byte `curv` with 1024
    /// points, and a hardcoded number would satisfy exactly one of them.
    #[test]
    fn colour_trc_tags_use_exiftools_short_names_and_their_own_byte_counts() {
        // Apple_iPadAir_3rd_generation.jpg: `para`, 32 bytes.
        let para = parse(&build_profile(&[
            ("rTRC", build_para()),
            ("gTRC", build_para()),
            ("bTRC", build_para()),
        ]));
        for name in ["RedTRC", "GreenTRC", "BlueTRC"] {
            assert_eq!(
                value(&para, name),
                Some("(Binary data 32 bytes, use -b option to extract)"),
                "{name} should carry the 32-byte para payload's own size"
            );
        }

        // Apple_iPadPro.jpg: `curv` with 1024 points, 12 + 2048 = 2060 bytes.
        let curv = parse(&build_profile(&[
            ("rTRC", build_curv(1024)),
            ("gTRC", build_curv(1024)),
            ("bTRC", build_curv(1024)),
        ]));
        for name in ["RedTRC", "GreenTRC", "BlueTRC"] {
            assert_eq!(
                value(&curv, name),
                Some("(Binary data 2060 bytes, use -b option to extract)"),
                "{name} should carry the 2060-byte curv payload's own size"
            );
        }

        // The long `Description` spelling is never a key.
        for metadata in [&para, &curv] {
            for name in [
                "RedToneReproductionCurve",
                "GreenToneReproductionCurve",
                "BlueToneReproductionCurve",
            ] {
                assert!(
                    !metadata.contains_key(name),
                    "{name} is a Description, not a tag Name"
                );
            }
        }
    }

    #[test]
    fn opaque_payloads_report_the_size_from_the_tag_table() {
        // kTRC/vcgt/ndin have no ExifTool formatter, so they print the tag's
        // declared byte count rather than any decoded sub-structure.
        let metadata = parse(&build_profile(&[
            ("kTRC", vec![0u8; 14]),
            ("vcgt", vec![0u8; 48]),
            ("ndin", vec![0u8; 56]),
        ]));

        assert_eq!(
            value(&metadata, "GrayTRC"),
            Some("(Binary data 14 bytes, use -b option to extract)")
        );
        assert_eq!(
            value(&metadata, "VideoCardGamma"),
            Some("(Binary data 48 bytes, use -b option to extract)")
        );
        assert_eq!(
            value(&metadata, "NativeDisplayInfo"),
            Some("(Binary data 56 bytes, use -b option to extract)")
        );
    }
}
