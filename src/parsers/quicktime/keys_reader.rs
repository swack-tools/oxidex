//! Source-derived resolver for the `moov/meta/keys` -> `ilst` protocol.
//!
//! This deliberately implements only direct QuickTime::Keys lookups. Native
//! ProcessKeys also falls back to ItemList/UserData and can construct unknown
//! names; neither is a generated claim here.
use super::generated_keys_specs::{KEYS_SPECS, KeySpec, REFUSED_SOURCE_KEYS};
use super::itemlist_reader;
use crate::core::MetadataMap;

pub(crate) struct ResolvedKey {
    pub spec: Option<&'static KeySpec>,
    pub refused: Option<String>,
}

pub(crate) fn resolve_keys(data: &[u8]) -> Vec<ResolvedKey> {
    if data.len() < 8 {
        return Vec::new();
    }
    // ProcessKeys ignores the declared entry count and walks valid trailing records.
    let mut out = Vec::new();
    let mut pos = 8;
    while pos < data.len().saturating_sub(4) {
        if pos + 8 > data.len() {
            break;
        }
        let len = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        if len < 8 || pos + len > data.len() {
            break;
        }
        let namespace = &data[pos + 4..pos + 8];
        let full = &data[pos + 8..pos + len];
        let full = &full[..full.iter().position(|b| *b == 0).unwrap_or(full.len())];
        let spec = resolve_name(namespace, full);
        let normalized = normalized_name(namespace, full);
        let refused = (spec.is_none()
            && REFUSED_SOURCE_KEYS
                .iter()
                .any(|key| key.as_bytes() == normalized))
        .then(|| String::from_utf8_lossy(full).into_owned());
        out.push(ResolvedKey { spec, refused });
        pos += len;
    }
    out
}

fn normalized_name<'a>(namespace: &[u8], full: &'a [u8]) -> &'a [u8] {
    if namespace == b"mdta" {
        full.strip_prefix(b"com.apple.quicktime.")
            .or_else(|| full.strip_prefix(b"com."))
            .unwrap_or(full)
    } else {
        full
    }
}
fn resolve_name(namespace: &[u8], full: &[u8]) -> Option<&'static KeySpec> {
    let short = normalized_name(namespace, full);
    find(short).or_else(|| (short != full).then(|| find(full)).flatten())
}
fn find(key: &[u8]) -> Option<&'static KeySpec> {
    KEYS_SPECS
        .iter()
        .find(|spec| spec.source_key.as_bytes() == key)
}
pub(crate) fn read_indexed(
    index: u32,
    resolved: &[ResolvedKey],
    data: &[u8],
    metadata: &mut MetadataMap,
) -> bool {
    let Some(spec) = index
        .checked_sub(1)
        .and_then(|i| resolved.get(i as usize))
        .and_then(|x| x.spec)
    else {
        return false;
    };
    itemlist_reader::read_spec(&spec.data, data, metadata);
    true
}
pub(crate) fn legacy_refused(index: u32, resolved: &[ResolvedKey]) -> Option<&str> {
    index
        .checked_sub(1)
        .and_then(|i| resolved.get(i as usize))
        .and_then(|x| x.refused.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::MetadataMap;
    fn keys(entries: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut x = vec![0; 4];
        x.extend_from_slice(&(entries.len() as u32).to_be_bytes());
        for (n, k) in entries {
            let l = (8 + k.len()) as u32;
            x.extend_from_slice(&l.to_be_bytes());
            x.extend_from_slice(*n);
            x.extend_from_slice(k);
        }
        x
    }
    fn data(s: &[u8]) -> Vec<u8> {
        let mut x = vec![0, 0, 0, 1, 0, 0, 0, 0];
        x.extend_from_slice(s);
        x
    }
    #[test]
    fn mdta_prefix_and_ordinal_are_source_resolved() {
        let r = resolve_keys(&keys(&[
            (b"mdta", b"com.apple.quicktime.artist"),
            (b"mdta", b"location.role"),
        ]));
        assert_eq!(r.len(), 2);
        let mut m = MetadataMap::new();
        assert!(read_indexed(1, &r, &data(b"Ada"), &mut m));
        assert_eq!(m.get_string("QuickTime:Artist"), Some("Ada"));
        assert!(!read_indexed(3, &r, &data(b"no"), &mut m));
        // Native walks valid trailing records even with count zero.
        let mut zero = keys(&[(b"mdta", b"artist")]);
        zero[4..8].copy_from_slice(&0u32.to_be_bytes());
        assert!(read_indexed(
            1,
            &resolve_keys(&zero),
            &data(b"Count ignored"),
            &mut m
        ));
    }
    #[test]
    fn full_retry_after_mdta_prefix_strip_and_unknown_are_not_invented() {
        let r = resolve_keys(&keys(&[
            (b"mdta", b"com.android.model"),
            (b"mdta", b"not.source.defined\0ignored"),
        ]));
        let mut m = MetadataMap::new();
        assert!(read_indexed(1, &r, &data(b"Pixel"), &mut m));
        assert_eq!(m.get_string("QuickTime:AndroidModel"), Some("Pixel"));
        assert!(!read_indexed(2, &r, &data(b"x"), &mut m));
    }
    #[test]
    fn malformed_key_directory_has_no_ordinal_claim() {
        assert!(resolve_keys(&[0; 7]).is_empty());
        let mut bad = keys(&[(b"mdta", b"artist")]);
        bad[8..12].copy_from_slice(&7u32.to_be_bytes());
        assert!(resolve_keys(&bad).is_empty());
    }
}
