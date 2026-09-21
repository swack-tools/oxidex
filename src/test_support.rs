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

#[path = "../tests/common/pinned_fixtures.rs"]
mod pinned_fixtures;
pub use pinned_fixtures::{FixtureConfig, FixturePopulation};

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

/// Locate a genuine sample beside the configured pinned ExifTool script, then
/// in its configured cache. Optional sample data is absent in fresh clones;
/// metadata or read failures for a present sample must still fail the test.
pub fn pinned_fixture_path(name: &str) -> Option<std::path::PathBuf> {
    let config = FixtureConfig::from_environment(crate::exiftool_oracle::repo_pin());
    let path = pinned_fixture_path_with_config(name, &config);
    if path.is_none() {
        eprintln!(
            "note: skipping pinned-fixture test -- {name} absent from configured fixture roots"
        );
    }
    path
}

pub fn pinned_fixture_path_with_config(
    name: &str,
    config: &FixtureConfig,
) -> Option<std::path::PathBuf> {
    config
        .resolve_for_mode(name, FixturePopulation::Any)
        .unwrap_or_else(|error| panic!("{error}"))
}

/// Resolve only an ExifTool source-tree `t/images` fixture.
pub fn pinned_t_images_fixture_path(name: &str) -> Option<std::path::PathBuf> {
    let config = FixtureConfig::from_environment(crate::exiftool_oracle::repo_pin());
    config
        .resolve_for_mode(name, FixturePopulation::TImages)
        .unwrap_or_else(|error| panic!("{error}"))
}

/// Resolve only a combined-corpus fixture.
pub fn pinned_combined_fixture_path(name: &str) -> Option<std::path::PathBuf> {
    let config = FixtureConfig::from_environment(crate::exiftool_oracle::repo_pin());
    config
        .resolve_for_mode(name, FixturePopulation::Combined)
        .unwrap_or_else(|error| panic!("{error}"))
}

/// Resolve the combined-corpus directory for directory-iteration tests.
pub fn pinned_combined_corpus_dir() -> Option<std::path::PathBuf> {
    let config = FixtureConfig::from_environment(crate::exiftool_oracle::repo_pin());
    config
        .combined_dir_for_mode()
        .unwrap_or_else(|error| panic!("{error}"))
}

/// Resolve the first configured `t/images` directory for directory iteration.
pub fn pinned_t_images_dir() -> Option<std::path::PathBuf> {
    let config = FixtureConfig::from_environment(crate::exiftool_oracle::repo_pin());
    config
        .t_images_dir_for_mode()
        .unwrap_or_else(|error| panic!("{error}"))
}

fn fixture_path_in(
    name: &str,
    exiftool: Option<&std::path::Path>,
    cache: &std::path::Path,
) -> Option<std::path::PathBuf> {
    use std::path::Path;
    let cached_binary = crate::exiftool_oracle::pinned_binary(cache);
    let candidates = [exiftool, Some(cached_binary.as_path())]
        .into_iter()
        .flatten()
        .filter_map(Path::parent)
        .map(|tree| tree.join("t/images").join(name))
        .chain([cache.join("combined-samples").join(name)]);
    candidates
        .into_iter()
        .find(|path| match std::fs::symlink_metadata(path) {
            Ok(_) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => panic!(
                "could not inspect pinned fixture {}: {error}",
                path.display()
            ),
        })
}

/// Open an optional pinned sample without hiding a present file's read error.
pub fn pinned_fixture_reader(name: &str) -> Option<crate::io::MMapReader> {
    pinned_fixture_path(name).map(|path| {
        crate::io::MMapReader::new(&path).unwrap_or_else(|error| {
            panic!("could not read pinned fixture {}: {error}", path.display())
        })
    })
}

#[cfg(test)]
mod fixture_tests {
    use super::*;

    #[test]
    fn injected_configuration_prefers_source_then_cache_then_combined_per_file() {
        let temp = tempfile::tempdir().expect("temporary fixture layout");
        let source = temp.path().join("source");
        let cache = temp.path().join("cache");
        let config = FixtureConfig::new(Some(source.clone()), cache.clone(), false);

        let combined = cache.join("combined-samples/fixture.bin");
        std::fs::create_dir_all(combined.parent().unwrap()).unwrap();
        std::fs::write(&combined, b"synthetic combined layout bytes").unwrap();
        assert_eq!(
            pinned_fixture_path_with_config("fixture.bin", &config),
            Some(combined.clone())
        );

        let cached = cache.join("exiftool/t/images/fixture.bin");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, b"synthetic cached layout bytes").unwrap();
        assert_eq!(
            pinned_fixture_path_with_config("fixture.bin", &config),
            Some(cached.clone())
        );

        let sourced = source.join("t/images/fixture.bin");
        std::fs::create_dir_all(sourced.parent().unwrap()).unwrap();
        std::fs::write(&sourced, b"synthetic source layout bytes").unwrap();
        assert_eq!(
            pinned_fixture_path_with_config("fixture.bin", &config),
            Some(sourced)
        );
    }

    #[test]
    fn injected_configuration_keeps_t_images_and_combined_populations_separate() {
        let temp = tempfile::tempdir().expect("temporary fixture layout");
        let source = temp.path().join("source");
        let cache = temp.path().join("cache");
        let config = FixtureConfig::new(Some(source.clone()), cache.clone(), false);

        let t_images = source.join("t/images");
        let combined = cache.join("combined-samples");
        std::fs::create_dir_all(&t_images).unwrap();
        std::fs::create_dir_all(&combined).unwrap();
        std::fs::write(t_images.join("only-source.bin"), b"synthetic source bytes").unwrap();
        std::fs::write(
            combined.join("only-combined.bin"),
            b"synthetic combined bytes",
        )
        .unwrap();

        assert_eq!(
            config.resolve_optional("only-source.bin", FixturePopulation::TImages),
            Some(t_images.join("only-source.bin"))
        );
        assert_eq!(
            config.resolve_optional("only-source.bin", FixturePopulation::Combined),
            None
        );
        assert_eq!(
            config.resolve_optional("only-combined.bin", FixturePopulation::TImages),
            None
        );
        assert_eq!(
            config.resolve_optional("only-combined.bin", FixturePopulation::Combined),
            Some(combined.join("only-combined.bin"))
        );
    }

    #[test]
    fn injected_required_mode_names_missing_fixture_and_checked_paths() {
        let temp = tempfile::tempdir().expect("empty fixture layout");
        let source = temp.path().join("source");
        let cache = temp.path().join("cache");
        let optional = FixtureConfig::new(Some(source.clone()), cache.clone(), false);
        let required = FixtureConfig::new(Some(source.clone()), cache.clone(), true);

        assert_eq!(
            optional.resolve_optional("missing.bin", FixturePopulation::Any),
            None
        );
        let error = required
            .resolve_required("missing.bin", FixturePopulation::Any)
            .expect_err("required mode must reject an absent requested fixture")
            .to_string();
        assert!(error.contains("missing.bin"), "{error}");
        assert!(error.contains(&source.join("t/images/missing.bin").display().to_string()));
        assert!(
            error.contains(
                &cache
                    .join("exiftool/t/images/missing.bin")
                    .display()
                    .to_string()
            )
        );
        assert!(
            error.contains(
                &cache
                    .join("combined-samples/missing.bin")
                    .display()
                    .to_string()
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn injected_broken_candidate_is_selected_so_open_failure_is_not_a_skip() {
        let temp = tempfile::tempdir().expect("temporary fixture layout");
        let source = temp.path().join("source");
        let cache = temp.path().join("cache");
        let config = FixtureConfig::new(Some(source.clone()), cache, true);
        let path = source.join("t/images/broken.bin");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(temp.path().join("missing-target"), &path).unwrap();

        assert_eq!(
            config
                .resolve_required("broken.bin", FixturePopulation::TImages)
                .expect("present broken candidate must not be reported missing"),
            path.clone()
        );
        assert!(
            std::fs::read(&path).is_err(),
            "selected candidate must fail on open"
        );
    }

    #[test]
    fn resolves_real_fixture_from_nondefault_source_and_cache_locations() {
        let Some(path) = pinned_fixture_path("Real.ra") else {
            return;
        };
        // Relocate genuine sample bytes unchanged, never a fabricated oracle
        // executable or hand-authored media pretending to be its fixture.
        let bytes = std::fs::read(path).expect("read genuine fixture");
        for relative in [
            "source/t/images/Real.ra",
            "cache/exiftool/t/images/Real.ra",
            "cache/combined-samples/Real.ra",
        ] {
            let temp = tempfile::tempdir().expect("temporary fixture layout");
            let expected = temp.path().join(relative);
            std::fs::create_dir_all(expected.parent().unwrap()).unwrap();
            std::fs::write(&expected, &bytes).unwrap();
            assert_eq!(
                fixture_path_in(
                    "Real.ra",
                    Some(&temp.path().join("source/exiftool")),
                    &temp.path().join("cache")
                ),
                Some(expected)
            );
        }
    }

    #[test]
    fn configured_source_precedes_cached_samples() {
        let Some(path) = pinned_fixture_path("Real.ra") else {
            return;
        };
        let bytes = std::fs::read(path).expect("read genuine fixture");
        let temp = tempfile::tempdir().expect("temporary fixture layout");
        for relative in [
            "source/t/images/Real.ra",
            "cache/exiftool/t/images/Real.ra",
            "cache/combined-samples/Real.ra",
        ] {
            let target = temp.path().join(relative);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, &bytes).unwrap();
        }
        assert_eq!(
            fixture_path_in(
                "Real.ra",
                Some(&temp.path().join("source/exiftool")),
                &temp.path().join("cache")
            ),
            Some(temp.path().join("source/t/images/Real.ra"))
        );
        assert_eq!(
            fixture_path_in("Real.ra", None, &temp.path().join("cache")),
            Some(temp.path().join("cache/exiftool/t/images/Real.ra"))
        );
    }

    #[test]
    fn missing_optional_fixture_has_no_reader_path() {
        let temp = tempfile::tempdir().expect("empty fixture layout");
        assert_eq!(
            fixture_path_in(
                "Real.ra",
                Some(&temp.path().join("source/exiftool")),
                &temp.path().join("cache")
            ),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn dangling_fixture_link_is_present_and_fails_to_open() {
        let temp = tempfile::tempdir().expect("temporary fixture layout");
        let path = temp.path().join("source/t/images/Real.ra");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(temp.path().join("missing-target"), &path).unwrap();
        assert_eq!(
            fixture_path_in(
                "Real.ra",
                Some(&temp.path().join("source/exiftool")),
                &temp.path().join("cache")
            ),
            Some(path.clone())
        );
        assert!(crate::io::MMapReader::new(&path).is_err());
    }
}
