//! Source-derived resolver for the `moov/meta/keys` -> `ilst` protocol.
//!
//! This deliberately implements only direct QuickTime::Keys lookups. Native
//! ProcessKeys also falls back to ItemList/UserData and can construct unknown
//! names; neither is a generated claim here.
use super::generated_keys_specs::{KEYS_SPECS, KeySpec};
use super::itemlist_reader;
use crate::core::MetadataMap;

pub(crate) fn resolve_keys(data: &[u8]) -> Vec<Option<&'static KeySpec>> {
    if data.len() < 8 {
        return Vec::new();
    }
    let count = u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize;
    let mut out = Vec::with_capacity(count);
    let mut pos = 8;
    for _ in 0..count {
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
        out.push(resolve_name(namespace, full));
        pos += len;
    }
    out
}

fn resolve_name(namespace: &[u8], full: &[u8]) -> Option<&'static KeySpec> {
    let short = if namespace == b"mdta" {
        full.strip_prefix(b"com.apple.quicktime.")
            .or_else(|| full.strip_prefix(b"com."))
            .unwrap_or(full)
    } else {
        full
    };
    find(short).or_else(|| (short != full).then(|| find(full)).flatten())
}
fn find(key: &[u8]) -> Option<&'static KeySpec> {
    KEYS_SPECS
        .iter()
        .find(|spec| spec.source_key.as_bytes() == key)
}
pub(crate) fn read_indexed(
    index: u32,
    resolved: &[Option<&'static KeySpec>],
    data: &[u8],
    metadata: &mut MetadataMap,
) -> bool {
    let Some(spec) = index
        .checked_sub(1)
        .and_then(|i| resolved.get(i as usize))
        .and_then(|x| *x)
    else {
        return false;
    };
    itemlist_reader::read_spec(&spec.data, data, metadata);
    true
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
