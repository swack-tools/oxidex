//! Native ProcessMOV's direct explicit-Format path for movie-level UserData.
//!
//! Generated operands own names, groups, priorities, formats and the loaded
//! MacRoman mapping. The protocol compiler authenticates ProcessMOV, its caller
//! edges, ReadValue/IsUTF8/Decode/Charset/FoundTag and the byte registries. IText,
//! implicit Format and custom controls remain with the existing other routes.
use super::generated_itemlist_specs::{ItemListSpec, SourceFormat};
use super::generated_userdata_specs::{
    AUTO_DECODE_MAX_BYTES, COPYRIGHT_PREFIX, SINGLE_BYTE_MAP, USERDATA_SPECS,
};
use super::itemlist_reader::{decode_direct_unsigned, numeric_values};
use crate::core::{Instance, MetadataMap, TagValue};

pub(crate) fn read_item(fourcc: &[u8], payload: &[u8], metadata: &mut MetadataMap) -> bool {
    let Some(spec) = USERDATA_SPECS.iter().find(|s| fourcc == s.data.raw_fourcc) else {
        return false;
    };
    if let Some((display, raw)) = decode(&spec.data, payload) {
        metadata.insert_occurrence_with_raw(
            format!("{}:{}", spec.data.group0, spec.data.name),
            display,
            raw,
            spec.priority,
            spec.data.group,
            Instance::default(),
        );
    }
    true
}

fn decode(spec: &ItemListSpec, payload: &[u8]) -> Option<(TagValue, TagValue)> {
    match spec.source_format {
        SourceFormat::Unsigned(width) => decode_direct_unsigned(spec, payload, width),
        SourceFormat::String => {
            // ReadValue(string) truncates at the first NUL before ProcessMOV's
            // length and encoding checks; no IText header exists on this path.
            let bytes = payload.split(|b| *b == 0).next()?;
            let text = if spec.raw_fourcc[0] != COPYRIGHT_PREFIX
                && bytes.len() <= AUTO_DECODE_MAX_BYTES
                && bytes.iter().any(|b| *b >= 0x80)
                && !is_native_utf8(bytes)
            {
                bytes
                    .iter()
                    .map(|b| char::from_u32(SINGLE_BYTE_MAP[usize::from(*b)]))
                    .collect::<Option<String>>()?
            } else {
                // ExifTool retains raw bytes for the copyright bypass and
                // over-limit values. Rust Strings apply its JSON FixUTF8
                // projection here, as the shared binary-table reader does.
                crate::exiftool_tables::runtime::fix_utf8(bytes)?
            };
            let text = crate::exiftool_tables::runtime::fix_utf8(text.as_bytes())?;
            Some(numeric_values(spec, text.clone(), TagValue::String(text)))
        }
        SourceFormat::Implicit => None,
    }
}

fn is_native_utf8(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes)
        .is_ok_and(|s| !s.chars().any(|ch| ch == '\u{fffe}' || ch == '\u{ffff}'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(fourcc: &[u8; 4]) -> &'static ItemListSpec {
        &USERDATA_SPECS
            .iter()
            .find(|s| &s.data.raw_fourcc == fourcc)
            .unwrap()
            .data
    }

    fn raw(fourcc: &[u8; 4], payload: &[u8]) -> TagValue {
        decode(spec(fourcc), payload).unwrap().1
    }

    #[test]
    fn recorded_native_json_values_cover_all_direct_declarations_and_edges() {
        use sha2::{Digest, Sha256};
        let proof: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/exiftool-tables/fixtures/quicktime_userdata_native_13_59.json"
        ))
        .unwrap();
        fn unhex(text: &str) -> Vec<u8> {
            text.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }
        for row in proof["cases"].as_array().unwrap() {
            let fourcc = unhex(row["raw_fourcc"].as_str().unwrap());
            let spec = USERDATA_SPECS
                .iter()
                .find(|s| fourcc == s.data.raw_fourcc)
                .unwrap();
            let seed = &row["payload"];
            let mut payload = unhex(seed["hex"].as_str().unwrap())
                .repeat(seed["repeat"].as_u64().unwrap() as usize);
            payload.extend(unhex(seed["suffix_hex"].as_str().unwrap()));
            let (display, raw) = decode(&spec.data, &payload).unwrap();
            for (mode, value) in [("print", display), ("raw", raw)] {
                let text = match value {
                    TagValue::String(s) => s,
                    TagValue::Integer(n) => n.to_string(),
                    other => panic!("unexpected direct scalar {other:?}"),
                };
                assert_eq!(
                    text.len() as u64,
                    row["expected"][mode]["utf8_length"].as_u64().unwrap(),
                    "{} {mode}",
                    row["case"]
                );
                assert_eq!(
                    hex::encode(Sha256::digest(text.as_bytes())),
                    row["expected"][mode]["utf8_sha256"].as_str().unwrap(),
                    "{} {mode}",
                    row["case"]
                );
            }
        }
    }

    #[test]
    fn direct_width_partial_tail_and_empty_are_native_readvalue() {
        assert_eq!(raw(b"WLOC", &[0x01, 0x02, 0xff]), TagValue::Integer(258));
        assert_eq!(raw(b"WLOC", &[0xff]), TagValue::String(String::new()));
        assert_eq!(raw(b"WLOC", &[0, 1, 0, 2]), TagValue::String("1 2".into()));
        assert_eq!(raw(b"LOOP", &[0, 2]), TagValue::String(String::new()));
        assert_eq!(
            decode(spec(b"LOOP"), &[0, 0, 0, 2]).unwrap().0,
            TagValue::String("Palindromic".into())
        );
    }

    #[test]
    fn text_detects_utf8_else_uses_actual_single_byte_map() {
        assert_eq!(raw(b"CNCV", b"a\0ignored"), TagValue::String("a".into()));
        assert_eq!(raw(b"CNCV", "é".as_bytes()), TagValue::String("é".into()));
        assert_eq!(raw(b"CNCV", &[0x8e]), TagValue::String("é".into()));
        assert_eq!(
            raw(b"CNCV", &[0xef, 0xbf, 0xbe]),
            TagValue::String("Ôøæ".into())
        );
        assert_eq!(raw(b"\xa9mdl", &[0x8e]), TagValue::String("?".into()));
    }

    #[test]
    fn string_limit_is_after_nul_truncation() {
        let mut payload = vec![0x8e; AUTO_DECODE_MAX_BYTES];
        assert_eq!(
            raw(b"CNCV", &payload),
            TagValue::String("é".repeat(AUTO_DECODE_MAX_BYTES))
        );
        payload.push(0x8e);
        assert_eq!(
            raw(b"CNCV", &payload),
            TagValue::String("?".repeat(AUTO_DECODE_MAX_BYTES + 1))
        );
        payload[1] = 0;
        assert_eq!(raw(b"CNCV", &payload), TagValue::String("é".into()));
    }

    #[test]
    fn every_generated_declaration_reaches_the_runtime_with_native_group() {
        for spec in USERDATA_SPECS {
            let mut metadata = MetadataMap::new();
            assert!(read_item(&spec.data.raw_fourcc, b"ABCD", &mut metadata));
            assert!(
                metadata
                    .get(&format!("{}:{}", spec.data.group0, spec.data.name))
                    .is_some()
            );
        }
        assert!(!read_item(b"zzzz", b"value", &mut MetadataMap::new()));
    }
}
