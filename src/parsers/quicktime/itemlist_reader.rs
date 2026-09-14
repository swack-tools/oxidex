//! Generated ItemList `data` atom executor.
//!
//! This consumes the source-derived ItemList declarations without making any
//! hand-maintained FourCC-to-name decisions.  The `country` and `language`
//! fields in a `data` atom are intentionally not used to manufacture the
//! alternate-language identities that ExifTool's `GetLangInfoQT` protocol
//! creates: that protocol remains outside this ItemList-only executor.

use super::generated_itemlist_specs::{ITEMLIST_SPECS, ItemListSpec, SourceFormat};
use crate::core::{Instance, MetadataMap, SHIM_DEFAULT_PRIORITY, TagValue};
use encoding_rs::{SHIFT_JIS, UTF_16BE};

/// Read one ItemList `data` atom payload.
///
/// The payload starts with the `data` atom's flags, country, and language
/// fields. A known source declaration returns `true` even when its payload is
/// malformed, because the caller must distinguish an omitted value from an
/// unrecognized FourCC.
pub(crate) fn read_item(
    raw_fourcc: &[u8],
    data_atom_payload: &[u8],
    metadata: &mut MetadataMap,
) -> bool {
    let Some(spec) = ITEMLIST_SPECS
        .iter()
        .find(|spec| raw_fourcc == spec.raw_fourcc.as_slice())
    else {
        return false;
    };

    let Some((display, raw)) = decode_data_atom(spec, data_atom_payload) else {
        return true;
    };

    // ItemList is the generated family-1 group; QuickTime is the ItemList
    // table's source-declared family-0 group and therefore the canonical key.
    metadata.insert_occurrence_with_raw(
        format!("{}:{}", spec.group0, spec.name),
        display,
        raw,
        SHIM_DEFAULT_PRIORITY,
        spec.group,
        Instance::default(),
    );
    true
}

fn decode_data_atom(spec: &ItemListSpec, data: &[u8]) -> Option<(TagValue, TagValue)> {
    let flags = u32::from_be_bytes(data.get(..4)?.try_into().ok()?);
    // The country and language fields are included in this fixed header but
    // are handled only by ExifTool's separate language-tag protocol.
    let value = data.get(8..)?;
    // Non-default locales require source-derived language identities. Omit
    // those values until that protocol is implemented, rather than mislabeling.
    if data.get(4..8)? != [0; 4] {
        return None;
    }

    // ProcessMOV decodes the known text flags before considering a table's
    // Format, so explicit numeric rows also follow an on-disk text type.
    if matches!(flags, 1..=5) {
        let text = decode_text_flag(flags, value)?;
        return Some(numeric_values(spec, text.clone(), TagValue::String(text)));
    }
    if matches!(spec.source_format, SourceFormat::String) {
        let text = std::str::from_utf8(value.split(|byte| *byte == 0).next()?)
            .ok()?
            .to_owned();
        return Some((TagValue::String(text.clone()), TagValue::String(text)));
    }

    match spec.source_format {
        SourceFormat::Unsigned(width) => {
            let width = adjusted_width(width / 8, value.len())?;
            decode_array(spec, value, width, |chunk| {
                decode_unsigned(chunk, width).map(unsigned_raw)
            })
        }
        SourceFormat::Implicit => decode_implicit(spec, flags, value),
        SourceFormat::String => unreachable!("string rows return before numeric decoding"),
    }
}

fn decode_text_flag(flags: u32, value: &[u8]) -> Option<String> {
    let decoded = match flags {
        1 | 4 => std::str::from_utf8(value).ok()?.to_owned(),
        2 | 5 => {
            if !value.len().is_multiple_of(2) {
                return None;
            }
            let (text, _, had_errors) = UTF_16BE.decode(value);
            if had_errors {
                return None;
            }
            text.into_owned()
        }
        3 => {
            let (text, _, had_errors) = SHIFT_JIS.decode(value);
            if had_errors {
                return None;
            }
            text.into_owned()
        }
        _ => return None,
    };
    Some(trim_one_nul(&decoded).to_owned())
}

fn trim_one_nul(value: &str) -> &str {
    value.strip_suffix('\0').unwrap_or(value)
}

fn decode_implicit(spec: &ItemListSpec, flags: u32, value: &[u8]) -> Option<(TagValue, TagValue)> {
    match flags {
        21 if matches!(value.len(), 1 | 2 | 4 | 8) => decode_signed(value)
            .map(|number| numeric_values(spec, number.to_string(), TagValue::Integer(number))),
        22 if matches!(value.len(), 1 | 2 | 4 | 8) => decode_unsigned(value, value.len() as u8)
            .map(|number| numeric_values(spec, number.to_string(), unsigned_raw(number))),
        23 => decode_array(spec, value, 4, |chunk| {
            let number = f32::from_be_bytes(chunk.try_into().ok()?) as f64;
            number.is_finite().then_some(TagValue::Float(number))
        }),
        24 => decode_array(spec, value, 8, |chunk| {
            let number = f64::from_be_bytes(chunk.try_into().ok()?);
            number.is_finite().then_some(TagValue::Float(number))
        }),
        0 if matches!(value.len(), 1 | 2) => decode_unsigned(value, value.len() as u8)
            .map(|number| numeric_values(spec, number.to_string(), unsigned_raw(number))),
        _ => Some((
            TagValue::Binary(value.to_vec()),
            TagValue::Binary(value.to_vec()),
        )),
    }
}

fn scalar_text(value: &TagValue) -> String {
    match value {
        TagValue::Integer(number) => number.to_string(),
        TagValue::Float(number) => crate::exiftool_tables::exprs::perl_num(*number),
        TagValue::String(text) => text.clone(),
        _ => unreachable!("numeric ReadValue produces only scalar values"),
    }
}

// ExifTool ReadValue with no Count reads every complete element, ignoring a
// partial tail. Zero elements yield an empty string; multiple elements become
// a space-separated scalar, not an array.
fn decode_array(
    spec: &ItemListSpec,
    value: &[u8],
    width: u8,
    decode: impl Fn(&[u8]) -> Option<TagValue>,
) -> Option<(TagValue, TagValue)> {
    let mut values = value
        .chunks_exact(usize::from(width))
        .map(decode)
        .collect::<Option<Vec<_>>>()?;
    let raw = if values.len() == 1 {
        values.pop()?
    } else {
        TagValue::String(values.iter().map(scalar_text).collect::<Vec<_>>().join(" "))
    };
    Some(numeric_values(spec, scalar_text(&raw), raw))
}

fn adjusted_width(declared: u8, value_len: usize) -> Option<u8> {
    match value_len {
        1 | 2 | 4 => Some(value_len as u8),
        _ if matches!(declared, 1 | 2 | 4 | 8) => Some(declared),
        _ => None,
    }
}

fn decode_unsigned(value: &[u8], width: u8) -> Option<u64> {
    match width {
        1 if value.len() == 1 => Some(value[0] as u64),
        2 if value.len() == 2 => Some(u16::from_be_bytes(value.try_into().ok()?) as u64),
        4 if value.len() == 4 => Some(u32::from_be_bytes(value.try_into().ok()?) as u64),
        8 if value.len() == 8 => Some(u64::from_be_bytes(value.try_into().ok()?)),
        _ => None,
    }
}

fn decode_signed(value: &[u8]) -> Option<i64> {
    match value.len() {
        1 => Some(i8::from_be_bytes(value.try_into().ok()?) as i64),
        2 => Some(i16::from_be_bytes(value.try_into().ok()?) as i64),
        4 => Some(i32::from_be_bytes(value.try_into().ok()?) as i64),
        8 => Some(i64::from_be_bytes(value.try_into().ok()?)),
        _ => None,
    }
}

fn unsigned_raw(value: u64) -> TagValue {
    match i64::try_from(value) {
        Ok(value) => TagValue::Integer(value),
        Err(_) => TagValue::String(value.to_string()),
    }
}

fn numeric_values(spec: &ItemListSpec, raw_text: String, raw: TagValue) -> (TagValue, TagValue) {
    let display = if spec.safe_enum_operands.is_empty() {
        raw.clone()
    } else if let Some(operand) = spec
        .safe_enum_operands
        .iter()
        .find(|operand| operand.raw == raw_text)
    {
        TagValue::String(operand.rendered.to_owned())
    } else {
        TagValue::String(format!("Unknown ({raw_text})"))
    };
    (display, raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(flags: u32, value: &[u8]) -> Vec<u8> {
        let mut bytes = flags.to_be_bytes().to_vec();
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(value);
        bytes
    }

    #[test]
    fn text_enum_still_applies_printconv() {
        let mut metadata = MetadataMap::new();
        assert!(read_item(b"cpil", &payload(1, b"1"), &mut metadata));
        assert_eq!(metadata.get_string("QuickTime:Compilation"), Some("Yes"));
    }

    #[test]
    fn invalid_text_is_not_reinterpreted_as_numeric() {
        let mut metadata = MetadataMap::new();
        assert!(read_item(b"cpil", &payload(1, &[0xff]), &mut metadata));
        assert!(metadata.is_empty());
    }

    #[test]
    fn numeric_formats_consume_complete_elements() {
        let mut metadata = MetadataMap::new();
        let mut floats = 1.25_f32.to_be_bytes().to_vec();
        floats.extend_from_slice(&2.5_f32.to_be_bytes());
        assert!(read_item(b"\xa9nam", &payload(23, &floats), &mut metadata));
        assert_eq!(metadata.get_string("QuickTime:Title"), Some("1.25 2.5"));
        assert!(read_item(b"plID", &payload(0, &[0; 9]), &mut metadata));
        assert_eq!(
            metadata.get("QuickTime:AlbumID"),
            Some(&TagValue::Integer(0))
        );
        assert!(read_item(b"plID", &payload(0, &[0; 3]), &mut metadata));
        assert_eq!(metadata.get_string("QuickTime:AlbumID"), Some(""));
    }

    #[test]
    fn unrecognized_implicit_integer_width_retains_binary() {
        let mut metadata = MetadataMap::new();
        assert!(read_item(
            b"\xa9nam",
            &payload(21, &[1, 2, 3]),
            &mut metadata
        ));
        assert_eq!(
            metadata.get("QuickTime:Title"),
            Some(&TagValue::Binary(vec![1, 2, 3]))
        );
    }

    #[test]
    fn untranslated_locale_is_not_labeled_as_default() {
        let mut bytes = payload(1, b"Title");
        bytes[6..8].copy_from_slice(&0x15c7_u16.to_be_bytes());
        let mut metadata = MetadataMap::new();
        assert!(read_item(b"\xa9nam", &bytes, &mut metadata));
        assert!(metadata.is_empty());
    }

    #[test]
    fn unknown_fourcc_is_not_handled() {
        let mut metadata = MetadataMap::new();
        assert!(!read_item(b"nope", &payload(1, b"ignored"), &mut metadata));
        assert!(metadata.is_empty());
    }

    #[test]
    fn malformed_known_item_is_handled_but_omitted() {
        let mut metadata = MetadataMap::new();
        assert!(read_item(b"cpil", &[0; 7], &mut metadata));
        assert!(metadata.is_empty());
    }

    #[test]
    fn enum_display_preserves_numeric_no_print_conv_form() {
        let mut metadata = MetadataMap::new();
        assert!(read_item(b"cpil", &payload(21, &[1]), &mut metadata));
        assert_eq!(
            metadata.get("QuickTime:Compilation"),
            Some(&TagValue::String("Yes".into()))
        );
        assert_eq!(
            metadata.without_print_conv().get("QuickTime:Compilation"),
            Some(&TagValue::Integer(1))
        );
    }

    #[test]
    fn text_flag_trims_exactly_one_trailing_nul() {
        let mut metadata = MetadataMap::new();
        assert!(read_item(
            b"\xa9nam",
            &payload(1, b"Title\0\0"),
            &mut metadata
        ));
        assert_eq!(
            metadata.get("QuickTime:Title"),
            Some(&TagValue::String("Title\0".into()))
        );
    }

    #[test]
    fn shift_jis_flag_decodes_with_the_pinned_encoding() {
        let mut metadata = MetadataMap::new();
        assert!(read_item(
            b"\xa9nam",
            &payload(3, &[0x82, 0xa0]),
            &mut metadata
        ));
        assert_eq!(
            metadata.get("QuickTime:Title"),
            Some(&TagValue::String("あ".into()))
        );
    }

    #[test]
    fn string_format_overrides_a_numeric_flag() {
        let mut metadata = MetadataMap::new();
        assert!(read_item(
            b"gshh",
            &payload(22, b"host.example"),
            &mut metadata
        ));
        assert_eq!(
            metadata.get("QuickTime:GoogleHostHeader"),
            Some(&TagValue::String("host.example".into()))
        );
    }

    #[test]
    fn unsigned_64_above_i64_is_a_decimal_string() {
        let mut metadata = MetadataMap::new();
        assert!(read_item(
            b"plID",
            &payload(22, &u64::MAX.to_be_bytes()),
            &mut metadata
        ));
        let expected = TagValue::String(u64::MAX.to_string());
        assert_eq!(metadata.get("QuickTime:AlbumID"), Some(&expected));
        assert_eq!(
            metadata.without_print_conv().get("QuickTime:AlbumID"),
            Some(&expected)
        );
    }

    #[test]
    fn explicit_unsigned_format_uses_a_short_payload_width() {
        let mut metadata = MetadataMap::new();
        assert!(read_item(
            b"plID",
            &payload(22, &[0x12, 0x34]),
            &mut metadata
        ));
        assert_eq!(
            metadata.get("QuickTime:AlbumID"),
            Some(&TagValue::Integer(0x1234))
        );
    }
}
