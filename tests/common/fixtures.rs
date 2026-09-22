//! Genuine pinned-source fixtures for integration tests.
//!
//! CI stages ExifTool outside the default developer cache. Resolve its sample
//! files through the same explicit script/cache settings as the pinned oracle.

#![allow(dead_code)] // Standalone integration targets use different fixture routes.

use oxidex::exiftool_oracle;
use std::path::PathBuf;

#[path = "pinned_fixtures.rs"]
mod pinned_fixtures;
pub use pinned_fixtures::{FixtureConfig, FixturePopulation};

/// Find optional sample data without turning present but unreadable files into
/// a successful skip. The caller must report any subsequent read/parse error.
pub fn pinned_fixture_path(name: &str) -> Option<PathBuf> {
    let config = FixtureConfig::from_environment(exiftool_oracle::repo_pin());
    let path = pinned_fixture_path_with_config(name, &config);
    if path.is_none() {
        eprintln!(
            "note: skipping pinned-fixture case -- {name} absent from configured fixture roots"
        );
    }
    path
}

pub fn pinned_fixture_path_with_config(name: &str, config: &FixtureConfig) -> Option<PathBuf> {
    config
        .resolve_for_mode(name, FixturePopulation::Any)
        .unwrap_or_else(|error| panic!("{error}"))
}

/// Resolve only a `t/images` source fixture; never substitute a combined sample.
pub fn pinned_t_images_fixture_path(name: &str) -> Option<PathBuf> {
    let config = FixtureConfig::from_environment(exiftool_oracle::repo_pin());
    config
        .resolve_for_mode(name, FixturePopulation::TImages)
        .unwrap_or_else(|error| panic!("{error}"))
}

/// Resolve only a combined-corpus fixture; never substitute a source fixture.
pub fn pinned_combined_fixture_path(name: &str) -> Option<PathBuf> {
    let config = FixtureConfig::from_environment(exiftool_oracle::repo_pin());
    config
        .resolve_for_mode(name, FixturePopulation::Combined)
        .unwrap_or_else(|error| panic!("{error}"))
}

pub fn pinned_t_images_dir() -> Option<PathBuf> {
    let config = FixtureConfig::from_environment(exiftool_oracle::repo_pin());
    config
        .t_images_dir_for_mode()
        .unwrap_or_else(|error| panic!("{error}"))
}

pub fn pinned_combined_corpus_dir() -> Option<PathBuf> {
    let config = FixtureConfig::from_environment(exiftool_oracle::repo_pin());
    config
        .combined_dir_for_mode()
        .unwrap_or_else(|error| panic!("{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integration_wrapper_routes_injected_required_resolution() {
        let temp = tempfile::tempdir().expect("temporary fixture layout");
        let cache = temp.path().join("cache");
        let path = cache.join("exiftool/t/images/fixture.bin");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"synthetic integration-wrapper bytes").unwrap();
        let config = FixtureConfig::new(None, cache, true);

        assert_eq!(
            pinned_fixture_path_with_config("fixture.bin", &config),
            Some(path)
        );
    }

    #[test]
    fn genuine_fixture_resolves_from_each_source_and_respects_precedence() {
        let Some(path) = pinned_fixture_path("RIFF.avi") else {
            return;
        };
        let bytes = std::fs::read(path).expect("read genuine RIFF fixture");
        let temp = tempfile::tempdir().expect("temporary fixture layout");
        let binary = temp.path().join("source/exiftool");
        let cache = temp.path().join("cache");
        // A present but incomplete source directory must not shadow a sample
        // available in the cache; resolution is per file, never per directory.
        std::fs::create_dir_all(temp.path().join("source/t/images")).unwrap();
        // Add candidates from lowest to highest priority, keeping all earlier
        // ones present so every assertion also checks the lookup precedence.
        for relative in [
            "cache/combined-samples/RIFF.avi",
            "cache/exiftool/t/images/RIFF.avi",
            "source/t/images/RIFF.avi",
        ] {
            let expected = temp.path().join(relative);
            std::fs::create_dir_all(expected.parent().unwrap()).unwrap();
            std::fs::write(&expected, &bytes).unwrap();
            assert_eq!(
                pinned_fixtures::resolve_optional_from_binary_and_cache(
                    "RIFF.avi",
                    Some(&binary),
                    &cache,
                ),
                Some(expected)
            );
        }
    }

    #[test]
    fn absent_optional_fixture_has_no_path() {
        let temp = tempfile::tempdir().expect("empty fixture layout");
        assert_eq!(
            pinned_fixtures::resolve_optional_from_binary_and_cache(
                "RIFF.avi",
                Some(&temp.path().join("source/exiftool")),
                &temp.path().join("cache")
            ),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn dangling_fixture_is_selected_and_read_fails() {
        let temp = tempfile::tempdir().expect("temporary fixture layout");
        let path = temp.path().join("source/t/images/RIFF.avi");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(temp.path().join("missing-target"), &path).unwrap();
        assert_eq!(
            pinned_fixtures::resolve_optional_from_binary_and_cache(
                "RIFF.avi",
                Some(&temp.path().join("source/exiftool")),
                &temp.path().join("cache")
            ),
            Some(path.clone())
        );
        assert!(std::fs::read(path).is_err());
    }
}
