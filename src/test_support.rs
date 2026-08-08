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

/// Root of the pinned ExifTool sample corpus that a number of unit tests read
/// real image files from.
///
/// This is a local developer cache (populated by `just compare-exiftool-full`),
/// **not** a committed fixture, so it is absent on CI runners and in fresh
/// clones. Tests that read from it must gate on [`pinned_corpus_available`].
pub const PINNED_CORPUS_ROOT: &str = "/tmp/oxidex-exiftool-cache/combined-samples";

/// True when the pinned sample corpus is present on this machine.
///
/// Tests reading real files out of [`PINNED_CORPUS_ROOT`] must call this and
/// return early when it is false. Letting them panic instead turns every CI run
/// red -- and because the runner is fail-fast, the first such panic also stops
/// the other ~3.9k tests from running at all, which is how a corpus-only
/// dependency masqueraded as a repo-wide test failure.
pub fn pinned_corpus_available() -> bool {
    use std::sync::Once;
    static NOTED: Once = Once::new();
    let present = std::path::Path::new(PINNED_CORPUS_ROOT).is_dir();
    if !present {
        NOTED.call_once(|| {
            eprintln!(
                "note: skipping pinned-corpus tests -- {PINNED_CORPUS_ROOT} is absent \
                 (populate it with `just compare-exiftool-full`)"
            );
        });
    }
    present
}
