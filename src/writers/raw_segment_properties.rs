//! Generic source-selected raw scalar property reader for a segment payload.
//!
//! The supplied payload is the JPEG parser's segment data, excluding framing.
//! Every format-specific marker, prefix, offset, width and property name is a
//! generated operand. Missing fields remain absent; a raw zero remains zero.
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RawByteOrder {
    Big,
    Little,
}

/// Native creation occurs before processing the current JPEG segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RawCreationTiming {
    BeforeCurrentSegment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RawField {
    pub property: &'static str,
    pub offset: usize,
    pub width: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RawSegmentRecipe {
    pub source_core_sha256: &'static str,
    pub source_writer_sha256: &'static str,
    pub marker: u8,
    pub signature: &'static [u8],
    pub skip: usize,
    pub byte_order: RawByteOrder,
    pub fields: &'static [RawField],
    /// Every consecutive matching marker delays fresh directory creation.
    pub creation_skip_markers: &'static [u8],
    /// Existing native directories postpone creation until they are processed.
    pub creation_wait_for_directories: &'static [&'static str],
    pub creation_timing: RawCreationTiming,
}

/// Execute the generated DATAMEMBER pass on one parser-provided payload.
/// The caller merges present properties across matching segments in order.
pub(crate) fn decode_raw_segment(
    recipe: &RawSegmentRecipe,
    marker: u8,
    payload: &[u8],
) -> Result<Option<BTreeMap<String, i64>>, String> {
    if marker != recipe.marker || !payload.starts_with(recipe.signature) {
        return Ok(None);
    }
    let raw = payload.get(recipe.skip..).unwrap_or_default();
    let mut properties = BTreeMap::new();
    for field in recipe.fields {
        // Native ProcessBinaryData ends this ordered pass at a field whose
        // start is beyond the directory, rather than trying subsequent ids.
        if field.offset >= raw.len() {
            break;
        }
        if !matches!(field.width, 1 | 2) {
            return Err("raw segment field width is outside the generated unsigned grammar".into());
        }
        let end = field
            .offset
            .checked_add(field.width)
            .ok_or_else(|| "raw segment field offset overflows address space".to_string())?;
        let Some(bytes) = raw.get(field.offset..end) else {
            // Native ReadValue returns undef without one complete scalar.
            continue;
        };
        let value = match (field.width, recipe.byte_order) {
            (1, _) => i64::from(bytes[0]),
            (2, RawByteOrder::Big) => i64::from(u16::from_be_bytes([bytes[0], bytes[1]])),
            (2, RawByteOrder::Little) => i64::from(u16::from_le_bytes([bytes[0], bytes[1]])),
            _ => unreachable!("width was validated above"),
        };
        properties.insert(field.property.to_string(), value);
    }
    Ok(Some(properties))
}
