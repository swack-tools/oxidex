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

/// Resolve a named source fixture for an explicitly requested qualification.
pub fn required_t_images_fixture_path(name: &str) -> PathBuf {
    let config = FixtureConfig::from_environment(exiftool_oracle::repo_pin());
    config
        .resolve_required(name, FixturePopulation::TImages)
        .unwrap_or_else(|error| panic!("{error}"))
}

/// Resolve a named combined fixture for an explicitly requested qualification.
pub fn required_combined_fixture_path(name: &str) -> PathBuf {
    let config = FixtureConfig::from_environment(exiftool_oracle::repo_pin());
    config
        .resolve_required(name, FixturePopulation::Combined)
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

/// t/images/ExifTool.jpg with its MIE trailer cut out, or `None` when the
/// pinned fixture is absent: a JPEG with the original's full EXIF and every
/// other trailer (AFCP, FotoStation x2, CanonVRD, PhotoMechanic, Samsung,
/// Vivo) and no MIE, which pinned ExifTool 13.59 reads exactly as the
/// original less its two MIE rows (`[MIE-Doc] Copyright`, `[MIE-Main]
/// TrailerSignature`). The MIE trailer (`~\x10\x04\xfe0MIE` .. the `zmie`
/// footer, 90 bytes) lies after the AFCP trailer and nothing locates it by
/// absolute offset, so cutting it moves no offset any reader follows.
///
/// For tests of writes the original's MIE would refuse: pinned 13.59 also
/// writes every EXIF set into that trailer's EXIF directory, which oxidex
/// does not write, so oxidex refuses them there.
pub fn exiftool_jpg_without_mie() -> Option<Vec<u8>> {
    let path = pinned_t_images_fixture_path("ExifTool.jpg")?;
    let file = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let find = |needle: &[u8]| {
        let hits: Vec<usize> = file
            .windows(needle.len())
            .enumerate()
            .filter_map(|(at, window)| (window == needle).then_some(at))
            .collect();
        assert_eq!(hits.len(), 1, "ExifTool.jpg: one {needle:?}");
        hits[0]
    };
    let start = find(b"~\x10\x04\xfe0MIE");
    let footer = b"~\0\x04\0zmie~\0\0\x06";
    let end = find(footer) + footer.len() + 6;
    assert_eq!(end - start, 90, "ExifTool.jpg: its 90-byte MIE trailer");
    Some([&file[..start], &file[end..]].concat())
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
    fn explicit_qualification_resolution_ignores_optional_mode() {
        let temp = tempfile::tempdir().expect("temporary fixture layout");
        let config = FixtureConfig::new(None, temp.path().join("cache"), false);
        let failure = std::panic::catch_unwind(|| {
            config
                .resolve_required("required.bin", FixturePopulation::Combined)
                .unwrap_or_else(|error| panic!("{error}"));
        });
        assert!(
            failure.is_err(),
            "qualification must not become an optional skip"
        );
    }

    #[test]
    fn explicit_cache_must_identify_the_repo_pin() {
        let temp = tempfile::tempdir().expect("temporary fixture layout");
        let cache = temp.path().join("other-release");
        let version = cache.join("exiftool/lib/Image/ExifTool.pm");
        std::fs::create_dir_all(version.parent().unwrap()).unwrap();
        std::fs::write(&version, "$VERSION = '0.00';\n").unwrap();
        let failure = std::panic::catch_unwind(|| {
            FixtureConfig::from_explicit_environment_values(
                "13.59",
                None,
                Some(cache.as_os_str()),
                temp.path().join("fallback"),
                false,
            )
        });
        assert!(
            failure.is_err(),
            "a mismatched explicit cache must be rejected"
        );
    }

    #[test]
    fn padded_exiftool_path_is_trimmed_and_blank_does_not_become_cwd() {
        let temp = tempfile::tempdir().expect("temporary fixture layout");
        let source = temp.path().join("source");
        let version = source.join("lib/Image/ExifTool.pm");
        std::fs::create_dir_all(version.parent().unwrap()).unwrap();
        std::fs::write(&version, "$VERSION = '13.59';\n").unwrap();
        let binary = source.join("exiftool");
        let padded = format!("  {}  ", binary.display());
        let config = FixtureConfig::from_explicit_environment_values(
            "13.59",
            Some(std::ffi::OsStr::new(&padded)),
            None,
            temp.path().join("fallback"),
            false,
        );
        assert_eq!(
            config.candidates("fixture.bin", FixturePopulation::TImages)[0],
            source.join("t/images/fixture.bin")
        );
        let blank = FixtureConfig::from_explicit_environment_values(
            "13.59",
            Some(std::ffi::OsStr::new("   \t")),
            None,
            temp.path().join("fallback"),
            false,
        );
        assert_eq!(
            blank.candidates("fixture.bin", FixturePopulation::TImages)[0],
            temp.path().join("fallback/exiftool/t/images/fixture.bin")
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
