//! Genuine pinned-source fixtures for integration tests.
//!
//! CI stages ExifTool outside the default developer cache. Resolve its sample
//! files through the same explicit script/cache settings as the pinned oracle.

use oxidex::exiftool_oracle;
use std::io;
use std::path::{Path, PathBuf};

/// Find optional sample data without turning present but unreadable files into
/// a successful skip. The caller must report any subsequent read/parse error.
pub fn pinned_fixture_path(name: &str) -> Option<PathBuf> {
    let named = std::env::var(exiftool_oracle::BINARY_ENV).ok();
    let binary = named
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(Path::new);
    let cache = exiftool_oracle::cache_dir();
    let path = fixture_path_in(name, binary, &cache);
    if path.is_none() {
        eprintln!(
            "note: skipping pinned-fixture case -- {name} absent beside EXIFTOOL or under {}",
            cache.display()
        );
    }
    path
}

fn fixture_path_in(name: &str, exiftool: Option<&Path>, cache: &Path) -> Option<PathBuf> {
    let cached_binary = exiftool_oracle::pinned_binary(cache);
    [exiftool, Some(cached_binary.as_path())]
        .into_iter()
        .flatten()
        .filter_map(Path::parent)
        .map(|tree| tree.join("t/images").join(name))
        .chain([cache.join("combined-samples").join(name)])
        .find(|path| match std::fs::symlink_metadata(path) {
            Ok(_) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => panic!(
                "could not inspect pinned fixture {}: {error}",
                path.display()
            ),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

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
                fixture_path_in("RIFF.avi", Some(&binary), &cache),
                Some(expected)
            );
        }
    }

    #[test]
    fn absent_optional_fixture_has_no_path() {
        let temp = tempfile::tempdir().expect("empty fixture layout");
        assert_eq!(
            fixture_path_in(
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
            fixture_path_in(
                "RIFF.avi",
                Some(&temp.path().join("source/exiftool")),
                &temp.path().join("cache")
            ),
            Some(path.clone())
        );
        assert!(std::fs::read(path).is_err());
    }
}
