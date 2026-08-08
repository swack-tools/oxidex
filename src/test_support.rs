//! Shared test utilities for unit tests within the crate.
//!
//! This module provides common test infrastructure like `TestReader`
//! that can be shared across all unit tests in the crate.
//!
//! # Usage
//!
//! In test modules within `src/`:
//! ```ignore
//! #[cfg(test)]
//! mod tests {
//!     use crate::test_support::TestReader;
//!     // ...
//! }
//! ```

use crate::core::{FileReader, MetadataMap, TagValue};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

/// Resolves a sample-corpus fixture, or `None` when the corpus is absent.
///
/// The corpus (~4200 files, thirteen manufacturer tarballs pulled from
/// exiftool.org) lives outside the repository under
/// `<DEFAULT_CACHE_DIR>/combined-samples` and is populated by
/// `just compare-exiftool-full` / the sample-download recipe in the justfile.
/// CI never downloads it: the fetch is large, and the recipe explicitly
/// tolerates a manufacturer being unavailable, so a corpus-backed assertion
/// could not be trusted there anyway.
///
/// A test that hardcodes a corpus path therefore passes locally and panics in
/// CI on a missing file. Because `cargo nextest` fail-fasts, one such panic
/// aborts the whole run -- which is how a green-looking suite locally became a
/// `Build & Test` job that only ever executed 143 of 4654 tests. Callers use
///
/// ```ignore
/// let Some(path) = corpus_fixture("Nikon.nef") else { return };
/// ```
///
/// so the assertion still runs in full wherever the corpus exists, and the
/// test no-ops (loudly) where it does not.
pub fn corpus_fixture(relative: &str) -> Option<PathBuf> {
    let path = PathBuf::from(crate::exiftool_oracle::DEFAULT_CACHE_DIR)
        .join("combined-samples")
        .join(relative);
    if path.exists() {
        return Some(path);
    }
    eprintln!(
        "skipping corpus-backed assertion: `{relative}` is not present under \
         {}/combined-samples",
        crate::exiftool_oracle::DEFAULT_CACHE_DIR
    );
    None
}

/// In-memory FileReader implementation for unit testing.
///
/// Wraps a `Vec<u8>` and implements the `FileReader` trait,
/// allowing tests to create virtual files from byte arrays.
pub struct TestReader {
    data: Vec<u8>,
}

impl TestReader {
    /// Creates a new TestReader from a Vec<u8>.
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }

    /// Creates a new TestReader from a byte slice.
    pub fn from_slice(data: &[u8]) -> Self {
        Self {
            data: data.to_vec(),
        }
    }
}

impl FileReader for TestReader {
    fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
        let start = offset as usize;
        let end = start.saturating_add(length).min(self.data.len());

        if start > self.data.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "offset beyond end of data",
            ));
        }

        Ok(&self.data[start..end])
    }

    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

/// Renders a `TagValue` the way a consumer comparing against ExifTool text
/// output would see it, so that a benign `String("8")` / `Integer(8)` pair does
/// not read as a divergence.
fn rendered_value(value: &TagValue) -> String {
    match value {
        TagValue::String(s) => s.clone(),
        TagValue::Integer(n) => n.to_string(),
        TagValue::Float(f) => f.to_string(),
        other => format!("{:?}", other),
    }
}

/// Asserts that no two emitted tag keys that are equal after stripping a leading
/// `"<Group>:"` prefix carry different rendered values.
///
/// This pins the defect class found by the 2026-07-26 duplicate-emission audit.
/// oxidex has a convention of emitting a tag twice -- once under ExifTool's own
/// key and once under a `"<Group>:"` alias -- which is only safe when both
/// inserts carry the same value. Eight sites had drifted, the sharpest being
/// GIF, where `BackgroundColor: 0` was emitted alongside
/// `GIF:BackgroundColor: #00` from the same local screen descriptor.
///
/// This is invisible to the fleet's `duplicate_emissions` detector, which keys
/// on the exact tag string (see
/// `docs/plans/specs/2026-07-24-fleet-knowledge-and-scaling-design.md:449`):
/// `"BackgroundColor"` and `"GIF:BackgroundColor"` are two distinct strings each
/// emitted once, so it scores 0. The comparison layer, which strips the group
/// prefix before matching, then picks between the two emissions
/// non-deterministically -- the same source tree reported GIF BackgroundColor as
/// a value difference at 22:17 and as 35/35 with `value_differences=0` at 22:37
/// on 2026-07-26.
pub fn assert_no_divergent_prefixed_duplicates(metadata: &MetadataMap) {
    let mut by_bare_name: BTreeMap<&str, Vec<(&str, String)>> = BTreeMap::new();
    for (key, value) in metadata.iter() {
        let bare = key
            .split_once(':')
            .map_or(key.as_str(), |(_group, rest)| rest);
        by_bare_name
            .entry(bare)
            .or_default()
            .push((key.as_str(), rendered_value(value)));
    }

    let divergent: Vec<String> = by_bare_name
        .iter()
        .filter(|(_, emissions)| {
            emissions.len() > 1
                && emissions
                    .iter()
                    .any(|(_, rendered)| *rendered != emissions[0].1)
        })
        .map(|(bare, emissions)| {
            let rendered: Vec<String> = emissions
                .iter()
                .map(|(key, rendered)| format!("{key}={rendered:?}"))
                .collect();
            format!("{bare}: {}", rendered.join(" vs "))
        })
        .collect();

    assert!(
        divergent.is_empty(),
        "one logical tag emitted under >1 key with different values, which makes \
         the ExifTool comparison harness non-deterministic:\n  {}",
        divergent.join("\n  ")
    );
}
