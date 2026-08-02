//! Shared resolution of the on-disk tag-cache directory for both
//! extractors.
//!
//! Never derive this location by walking up from a caller-supplied fixture
//! path. The corpus contains vendor subdirectories (`combined-samples/Canon`,
//! `combined-samples/Olympus`, ...), so pointing the harness at one of them
//! makes `fixture_path.parent()` resolve to `combined-samples` itself -- the
//! read-only sample corpus -- and the cache gets written inside it. A guard
//! that refuses a cache dir *inside* `fixture_path` does not catch this: the
//! bad location is the fixture's *parent*, not its child.

use std::path::{Path, PathBuf};

pub const OXIDEX_TAG_CACHE_DIR_ENV: &str = "OXIDEX_TAG_CACHE_DIR";

/// Resolve the on-disk cache directory for `cache_kind` (e.g.
/// `"oxidex-tag-cache"` or `"exiftool-tag-cache"`).
///
/// Priority:
/// 1. `explicit_override` (wired from a CLI flag)
/// 2. the `OXIDEX_TAG_CACHE_DIR` env var
/// 3. a stable location under the system temp dir, keyed by a hash of the
///    canonicalized `fixture_path` so different corpora don't collide.
///
/// This never inspects `fixture_path`'s parent -- deriving a write location
/// from a caller-supplied read location is exactly the defect this function
/// replaces.
pub fn resolve_cache_dir(
    fixture_path: &Path,
    cache_kind: &str,
    explicit_override: Option<&Path>,
) -> PathBuf {
    if let Some(dir) = explicit_override {
        return dir.join(cache_kind);
    }
    if let Ok(dir) = std::env::var(OXIDEX_TAG_CACHE_DIR_ENV) {
        return PathBuf::from(dir).join(cache_kind);
    }
    let canonical =
        std::fs::canonicalize(fixture_path).unwrap_or_else(|_| fixture_path.to_path_buf());
    let hash = format!("{:x}", md5::compute(canonical.to_string_lossy().as_bytes()));
    std::env::temp_dir()
        .join(format!("oxidex-tag-comparison-cache-{hash}"))
        .join(cache_kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The regression this module exists to fix: point `fixture_path` at a
    /// subdirectory of a corpus (mirroring `combined-samples/Olympus`) and
    /// assert the resolved cache dir never lands under its observable
    /// parent (mirroring `combined-samples` itself, the read-only corpus).
    #[test]
    fn resolved_cache_dir_never_lands_under_fixture_parent() {
        // SAFETY: single-threaded assertion within this test; no other test
        // in this process reads OXIDEX_TAG_CACHE_DIR concurrently.
        unsafe {
            std::env::remove_var(OXIDEX_TAG_CACHE_DIR_ENV);
        }

        let corpus_root = tempfile::tempdir().unwrap();
        let observable_parent = corpus_root.path().join("combined-samples");
        let vendor_subdir = observable_parent.join("Olympus");
        std::fs::create_dir_all(&vendor_subdir).unwrap();

        let resolved = resolve_cache_dir(&vendor_subdir, "oxidex-tag-cache", None);

        assert!(
            !resolved.starts_with(&observable_parent),
            "cache dir {} must not be written inside the corpus at {}",
            resolved.display(),
            observable_parent.display()
        );
        assert!(
            !resolved.starts_with(corpus_root.path()),
            "cache dir {} must not be written anywhere inside the corpus root {}",
            resolved.display(),
            corpus_root.path().display()
        );
    }

    #[test]
    fn explicit_override_wins_over_everything() {
        unsafe {
            std::env::set_var(OXIDEX_TAG_CACHE_DIR_ENV, "/should/not/be/used");
        }
        let fixture = tempfile::tempdir().unwrap();
        let override_dir = tempfile::tempdir().unwrap();

        let resolved = resolve_cache_dir(
            fixture.path(),
            "oxidex-tag-cache",
            Some(override_dir.path()),
        );

        assert_eq!(resolved, override_dir.path().join("oxidex-tag-cache"));

        unsafe {
            std::env::remove_var(OXIDEX_TAG_CACHE_DIR_ENV);
        }
    }

    #[test]
    fn env_var_wins_over_fixture_derived_default() {
        let fixture = tempfile::tempdir().unwrap();
        let env_dir = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var(OXIDEX_TAG_CACHE_DIR_ENV, env_dir.path());
        }

        let resolved = resolve_cache_dir(fixture.path(), "exiftool-tag-cache", None);

        assert_eq!(resolved, env_dir.path().join("exiftool-tag-cache"));

        unsafe {
            std::env::remove_var(OXIDEX_TAG_CACHE_DIR_ENV);
        }
    }

    #[test]
    fn same_fixture_path_resolves_deterministically() {
        unsafe {
            std::env::remove_var(OXIDEX_TAG_CACHE_DIR_ENV);
        }
        let fixture = tempfile::tempdir().unwrap();
        let a = resolve_cache_dir(fixture.path(), "oxidex-tag-cache", None);
        let b = resolve_cache_dir(fixture.path(), "oxidex-tag-cache", None);
        assert_eq!(a, b);
    }
}
