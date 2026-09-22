//! Test-only resolution of pinned ExifTool fixture data.
//!
//! This file is path-included by both the crate unit-test support and the
//! integration-test fixture wrapper. Keeping it outside the library API makes
//! the release-fixture contract portable without changing production behavior.

#![allow(dead_code)] // Each test target imports only the population it exercises.

use std::env;
use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};

pub const REQUIRED_ENV: &str = "OXIDEX_RELEASE_REQUIRE_PINNED_FIXTURES";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixturePopulation {
    /// Preserve the historical source-before-cache-before-combined fallback.
    Any,
    /// Only the pinned ExifTool source tree's `t/images` population.
    TImages,
    /// Only the separately assembled combined-sample population.
    Combined,
}

#[derive(Clone, Debug)]
pub struct FixtureConfig {
    source_tree: Option<PathBuf>,
    cache_dir: Option<PathBuf>,
    required: bool,
}

impl FixtureConfig {
    pub fn new(source_tree: Option<PathBuf>, cache_dir: PathBuf, required: bool) -> Self {
        Self::with_optional_cache(source_tree, Some(cache_dir), required)
    }

    fn with_optional_cache(
        source_tree: Option<PathBuf>,
        cache_dir: Option<PathBuf>,
        required: bool,
    ) -> Self {
        Self {
            source_tree,
            cache_dir,
            required,
        }
    }

    pub fn from_environment(repo_pin: &str) -> Self {
        let exiftool = env::var_os("EXIFTOOL");
        let cache = env::var_os("EXIFTOOL_CACHE_DIR");
        let explicit_cache = cache.as_deref().and_then(normalized_path);
        let fallback_cache = explicit_cache
            .is_none()
            .then(|| durable_cache_dir_from_environment(repo_pin))
            .flatten();
        Self::from_optional_environment_values(
            repo_pin,
            exiftool.as_deref(),
            explicit_cache,
            fallback_cache,
            env::var(REQUIRED_ENV).is_ok_and(|value| value == "1"),
        )
    }

    /// Test-only constructor for the environment boundary. It keeps path
    /// trimming and pinned-tree validation observable without mutating the
    /// process environment shared by parallel test targets.
    pub fn from_explicit_environment_values(
        repo_pin: &str,
        exiftool: Option<&OsStr>,
        cache: Option<&OsStr>,
        fallback_cache: PathBuf,
        required: bool,
    ) -> Self {
        Self::from_optional_environment_values(
            repo_pin,
            exiftool,
            cache.and_then(normalized_path),
            Some(fallback_cache),
            required,
        )
    }

    fn from_optional_environment_values(
        repo_pin: &str,
        exiftool: Option<&OsStr>,
        explicit_cache: Option<PathBuf>,
        fallback_cache: Option<PathBuf>,
        required: bool,
    ) -> Self {
        let source_tree = exiftool
            .and_then(normalized_path)
            .and_then(|binary| binary.parent().map(Path::to_path_buf))
            .and_then(|tree| source_tree_if_pinned(&tree, repo_pin));
        if let Some(cache) = &explicit_cache {
            assert_pinned_cache_dir(cache, repo_pin, "EXIFTOOL_CACHE_DIR");
        }
        if let Some(cache) = fallback_cache.as_ref().filter(|cache| cache.exists()) {
            assert_pinned_cache_dir(cache, repo_pin, "durable fixture cache");
        }
        Self::with_optional_cache(source_tree, explicit_cache.or(fallback_cache), required)
    }

    pub fn required(&self) -> bool {
        self.required
    }

    pub fn resolve_optional(&self, name: &str, population: FixturePopulation) -> Option<PathBuf> {
        self.candidates(name, population)
            .into_iter()
            .find(|path| path_is_present(path))
    }

    pub fn resolve_required(
        &self,
        name: &str,
        population: FixturePopulation,
    ) -> Result<PathBuf, MissingFixture> {
        self.resolve_optional(name, population)
            .ok_or_else(|| MissingFixture {
                requested: format!("fixture {name}"),
                checked: self.candidates(name, population),
            })
    }

    pub fn resolve_for_mode(
        &self,
        name: &str,
        population: FixturePopulation,
    ) -> Result<Option<PathBuf>, MissingFixture> {
        if self.required {
            self.resolve_required(name, population).map(Some)
        } else {
            Ok(self.resolve_optional(name, population))
        }
    }

    pub fn t_images_dir_for_mode(&self) -> Result<Option<PathBuf>, MissingFixture> {
        self.directory_for_mode("t/images directory", self.t_images_directories())
    }

    pub fn combined_dir_for_mode(&self) -> Result<Option<PathBuf>, MissingFixture> {
        self.directory_for_mode(
            "combined-samples directory",
            self.combined_dir().into_iter().collect(),
        )
    }

    pub fn candidates(&self, name: &str, population: FixturePopulation) -> Vec<PathBuf> {
        match population {
            FixturePopulation::Any => self
                .t_images_directories()
                .into_iter()
                .map(|directory| directory.join(name))
                .chain(
                    self.combined_dir()
                        .into_iter()
                        .map(|directory| directory.join(name)),
                )
                .collect(),
            FixturePopulation::TImages => self
                .t_images_directories()
                .into_iter()
                .map(|directory| directory.join(name))
                .collect(),
            FixturePopulation::Combined => self
                .combined_dir()
                .into_iter()
                .map(|directory| directory.join(name))
                .collect(),
        }
    }

    fn t_images_directories(&self) -> Vec<PathBuf> {
        self.source_tree
            .iter()
            .map(|tree| tree.join("t/images"))
            .chain(
                self.cache_dir
                    .iter()
                    .map(|cache| cache.join("exiftool/t/images")),
            )
            .collect()
    }

    fn combined_dir(&self) -> Option<PathBuf> {
        self.cache_dir
            .as_ref()
            .map(|cache| cache.join("combined-samples"))
    }

    fn directory_for_mode(
        &self,
        requested: &str,
        checked: Vec<PathBuf>,
    ) -> Result<Option<PathBuf>, MissingFixture> {
        for path in &checked {
            match std::fs::symlink_metadata(path) {
                Ok(_) if path.is_dir() => return Ok(Some(path.clone())),
                Ok(_) if self.required => {
                    return Err(MissingFixture {
                        requested: format!("{requested} is unusable at {}", path.display()),
                        checked,
                    });
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) if self.required => {
                    return Err(MissingFixture {
                        requested: format!(
                            "{requested} could not be inspected at {}: {error}",
                            path.display()
                        ),
                        checked,
                    });
                }
                Err(error) => panic!(
                    "could not inspect pinned fixture directory {}: {error}",
                    path.display()
                ),
            }
        }
        if self.required {
            Err(MissingFixture {
                requested: requested.to_owned(),
                checked,
            })
        } else {
            Ok(None)
        }
    }
}

fn normalized_path(value: &OsStr) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let bytes = value.as_bytes();
        let first = bytes.iter().position(|byte| !byte.is_ascii_whitespace())?;
        let last = bytes.iter().rposition(|byte| !byte.is_ascii_whitespace())?;
        Some(PathBuf::from(std::ffi::OsString::from_vec(
            bytes[first..=last].to_vec(),
        )))
    }
    #[cfg(not(unix))]
    {
        let value = value
            .to_str()?
            .trim_matches(|character: char| character.is_ascii_whitespace());
        (!value.is_empty()).then(|| PathBuf::from(value))
    }
}

fn assert_pinned_cache_dir(cache_dir: &Path, repo_pin: &str, variable: &str) {
    assert_pinned_source_tree(&cache_dir.join("exiftool"), repo_pin, variable);
}

fn assert_pinned_source_tree(tree: &Path, repo_pin: &str, variable: &str) {
    let version_file = tree.join("lib/Image/ExifTool.pm");
    let contents = std::fs::read_to_string(&version_file).unwrap_or_else(|error| {
        panic!(
            "{variable} does not name a readable pinned ExifTool {repo_pin} tree ({}: {error})",
            version_file.display()
        )
    });
    let single = format!("$VERSION = '{repo_pin}'");
    let double = format!("$VERSION = \"{repo_pin}\"");
    assert!(
        contents.lines().any(|line| {
            let line = line.trim();
            line.starts_with("$VERSION") && (line.contains(&single) || line.contains(&double))
        }),
        "{variable} does not name pinned ExifTool {repo_pin}: {} has no matching $VERSION declaration",
        version_file.display()
    );
}

fn source_tree_if_pinned(tree: &Path, repo_pin: &str) -> Option<PathBuf> {
    if !tree.join("lib/Image/ExifTool.pm").is_file() {
        return None;
    }
    assert_pinned_source_tree(tree, repo_pin, "EXIFTOOL");
    Some(tree.to_path_buf())
}

/// Test-only adapter for precedence tests that model an explicit ExifTool
/// binary and a cache root. Both wrapper test suites use this instead of
/// restating fixture search order.
pub fn resolve_optional_from_binary_and_cache(
    name: &str,
    binary: Option<&Path>,
    cache_dir: &Path,
) -> Option<PathBuf> {
    FixtureConfig::new(
        binary.and_then(Path::parent).map(Path::to_path_buf),
        cache_dir.to_path_buf(),
        false,
    )
    .resolve_optional(name, FixturePopulation::Any)
}

pub fn durable_cache_dir(ops_dir: Option<&Path>, home_dir: &Path, repo_pin: &str) -> PathBuf {
    durable_cache_dir_from_values(ops_dir, Some(home_dir), repo_pin)
}

pub fn durable_cache_dir_from_values(
    ops_dir: Option<&Path>,
    home_dir: Option<&Path>,
    repo_pin: &str,
) -> PathBuf {
    ops_dir
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| {
            home_dir
                .expect("fixture resolution needs OXIDEX_OPS_DIR or HOME for its cache root")
                .join("oxidex-ops")
        })
        .join("cache/exiftool")
        .join(repo_pin.trim())
}

fn durable_cache_dir_from_environment(repo_pin: &str) -> Option<PathBuf> {
    let ops_dir = env::var_os("OXIDEX_OPS_DIR")
        .as_deref()
        .and_then(normalized_path);
    let home_dir = if ops_dir.is_some() {
        None
    } else {
        env::var_os("HOME").as_deref().and_then(normalized_path)
    };
    ops_dir
        .map(expand_tilde)
        .or_else(|| home_dir.map(|home| home.join("oxidex-ops")))
        .map(|root| durable_ops_root(&root))
        .map(|root| root.join("cache/exiftool").join(repo_pin.trim()))
}

/// Mirror `scripts/ops_paths.py` for the test-only durable fallback. This
/// keeps fixture qualification from accepting a cache outside the operational
/// root contract used by the release tools.
fn durable_ops_root(path: &Path) -> PathBuf {
    assert!(
        path.is_absolute(),
        "OXIDEX_OPS_DIR must name an absolute durable directory"
    );
    reject_symlink_components(path);
    let resolved = normalize_absolute_path(path);
    let temporary_roots = [
        PathBuf::from("/tmp"),
        PathBuf::from("/private/tmp"),
        PathBuf::from("/var/tmp"),
        env::temp_dir(),
    ];
    assert!(
        !temporary_roots
            .iter()
            .map(|root| normalize_absolute_path(root))
            .any(|root| resolved.starts_with(root)),
        "OXIDEX_OPS_DIR must be durable, not temporary: {}",
        path.display()
    );
    assert!(
        resolved.parent().is_some(),
        "OXIDEX_OPS_DIR must not be a filesystem root"
    );
    resolved
}

fn expand_tilde(path: PathBuf) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(remainder) => {
            PathBuf::from(env::var_os("HOME").expect("OXIDEX_OPS_DIR ~ expansion needs HOME"))
                .join(remainder)
        }
        Err(_) => path,
    }
}

fn reject_symlink_components(path: &Path) {
    let mut cursor = PathBuf::new();
    for component in path.components() {
        cursor.push(component.as_os_str());
        if matches!(component, Component::CurDir | Component::ParentDir) {
            continue;
        }
        match std::fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                panic!(
                    "OXIDEX_OPS_DIR must not contain a symlink: {}",
                    cursor.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => panic!(
                "could not inspect OXIDEX_OPS_DIR component {}: {error}",
                cursor.display()
            ),
        }
    }
}

fn normalize_absolute_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(component) => normalized.push(component),
        }
    }
    normalized
}

fn path_is_present(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => panic!(
            "could not inspect pinned fixture {}: {error}",
            path.display()
        ),
    }
}

#[cfg(test)]
mod environment_tests {
    use super::*;

    const CHILD_ENV: &str = "OXIDEX_PINNED_FIXTURE_EXPLICIT_CACHE_CHILD";
    const WRAPPER_CHILD_ENV: &str = "OXIDEX_PINNED_FIXTURE_WRAPPER_CHILD";
    const NO_FALLBACK_CHILD_ENV: &str = "OXIDEX_PINNED_FIXTURE_NO_FALLBACK_CHILD";
    const WRONG_FALLBACK_CHILD_ENV: &str = "OXIDEX_PINNED_FIXTURE_WRONG_FALLBACK_CHILD";
    const OPS_ROOT_CHILD_ENV: &str = "OXIDEX_PINNED_FIXTURE_OPS_ROOT_CHILD";

    #[test]
    fn explicit_cache_does_not_require_fallback_environment() {
        if env::var_os(CHILD_ENV).is_some() {
            let cache = PathBuf::from(env::var_os("EXIFTOOL_CACHE_DIR").unwrap());
            assert_eq!(
                FixtureConfig::from_environment("13.59").cache_dir,
                Some(cache),
                "a validated explicit cache must not consult HOME or OXIDEX_OPS_DIR"
            );
            return;
        }

        let temp = tempfile::tempdir().expect("temporary explicit cache");
        let cache = temp.path().join("cache");
        let version = cache.join("exiftool/lib/Image/ExifTool.pm");
        std::fs::create_dir_all(version.parent().unwrap()).unwrap();
        std::fs::write(&version, "$VERSION = '13.59';\n").unwrap();

        let status = std::process::Command::new(env::current_exe().unwrap())
            .arg("explicit_cache_does_not_require_fallback_environment")
            .env(CHILD_ENV, "1")
            .env("EXIFTOOL_CACHE_DIR", &cache)
            .env_remove("HOME")
            .env_remove("OXIDEX_OPS_DIR")
            .env_remove("EXIFTOOL")
            .env_remove(REQUIRED_ENV)
            .status()
            .expect("run isolated environment-selection control");
        assert!(
            status.success(),
            "isolated environment-selection control failed"
        );
    }

    #[test]
    fn wrapper_exiftool_uses_validated_explicit_cache() {
        if env::var_os(WRAPPER_CHILD_ENV).is_some() {
            let cache = PathBuf::from(env::var_os("EXIFTOOL_CACHE_DIR").unwrap());
            let config = FixtureConfig::from_environment("13.59");
            assert_eq!(config.source_tree, None, "a wrapper is not a source tree");
            assert_eq!(config.cache_dir, Some(cache));
            return;
        }

        let temp = tempfile::tempdir().expect("temporary wrapper cache");
        let cache = temp.path().join("cache");
        let version = cache.join("exiftool/lib/Image/ExifTool.pm");
        std::fs::create_dir_all(version.parent().unwrap()).unwrap();
        std::fs::write(&version, "$VERSION = '13.59';\n").unwrap();
        let wrapper = temp.path().join("exiftool-pinned.sh");
        std::fs::write(&wrapper, "#!/bin/sh\n").unwrap();

        let status = std::process::Command::new(env::current_exe().unwrap())
            .arg("wrapper_exiftool_uses_validated_explicit_cache")
            .env(WRAPPER_CHILD_ENV, "1")
            .env("EXIFTOOL", &wrapper)
            .env("EXIFTOOL_CACHE_DIR", &cache)
            .env_remove("HOME")
            .env_remove("OXIDEX_OPS_DIR")
            .env_remove(REQUIRED_ENV)
            .status()
            .expect("run isolated wrapper environment-selection control");
        assert!(status.success(), "isolated wrapper control failed");
    }

    #[test]
    fn absent_fallback_is_optional_only_until_required_resolution() {
        if env::var_os(NO_FALLBACK_CHILD_ENV).is_some() {
            let required = env::var_os(REQUIRED_ENV).is_some();
            let config = FixtureConfig::from_environment("13.59");
            let outcome = config.resolve_for_mode("missing.bin", FixturePopulation::Any);
            if required {
                let error = outcome.expect_err("required lookup must fail without a fixture root");
                assert!(error.to_string().contains("missing.bin"));
            } else {
                assert_eq!(
                    outcome.unwrap(),
                    None,
                    "optional lookup must not invent a cache root"
                );
            }
            return;
        }

        for required in [false, true] {
            let mut command = std::process::Command::new(env::current_exe().unwrap());
            command
                .arg("absent_fallback_is_optional_only_until_required_resolution")
                .env(NO_FALLBACK_CHILD_ENV, "1")
                .env_remove("EXIFTOOL")
                .env_remove("EXIFTOOL_CACHE_DIR")
                .env_remove("HOME")
                .env_remove("OXIDEX_OPS_DIR");
            if required {
                command.env(REQUIRED_ENV, "1");
            } else {
                command.env_remove(REQUIRED_ENV);
            }
            assert!(
                command
                    .status()
                    .expect("run isolated no-fallback environment-selection control")
                    .success(),
                "isolated no-fallback control failed with required={required}"
            );
        }
    }

    #[test]
    fn existing_durable_fallback_must_identify_the_repo_pin() {
        if env::var_os(WRONG_FALLBACK_CHILD_ENV).is_some() {
            let failure = std::panic::catch_unwind(|| FixtureConfig::from_environment("13.59"));
            assert!(
                failure.is_err(),
                "an existing durable fallback must reject a mismatched ExifTool pin"
            );
            return;
        }

        let temp = tempfile::tempdir().expect("temporary durable fallback");
        let version = temp
            .path()
            .join("cache/exiftool/13.59/exiftool/lib/Image/ExifTool.pm");
        std::fs::create_dir_all(version.parent().unwrap()).unwrap();
        std::fs::write(&version, "$VERSION = '0.00';\n").unwrap();

        let status = std::process::Command::new(env::current_exe().unwrap())
            .arg("existing_durable_fallback_must_identify_the_repo_pin")
            .env(WRONG_FALLBACK_CHILD_ENV, "1")
            .env("OXIDEX_OPS_DIR", temp.path())
            .env_remove("EXIFTOOL")
            .env_remove("EXIFTOOL_CACHE_DIR")
            .env_remove("HOME")
            .env_remove(REQUIRED_ENV)
            .status()
            .expect("run isolated durable-fallback validation control");
        assert!(status.success(), "isolated durable-fallback control failed");
    }

    #[test]
    fn ops_root_uses_the_canonical_durable_root_contract() {
        if let Some(mode) = env::var_os(OPS_ROOT_CHILD_ENV) {
            if mode == OsStr::new("reject") {
                let failure = std::panic::catch_unwind(|| FixtureConfig::from_environment("13.59"));
                assert!(
                    failure.is_err(),
                    "an invalid OXIDEX_OPS_DIR must not supply a fixture fallback"
                );
                return;
            }

            let expected_root = PathBuf::from(env::var_os("EXPECTED_OPS_ROOT").unwrap());
            let config = FixtureConfig::from_environment("13.59");
            assert_eq!(
                config.cache_dir,
                Some(expected_root.join("cache/exiftool/13.59")),
                "configured root must resolve through the canonical durable root"
            );
            return;
        }

        let home = PathBuf::from(env::var_os("HOME").expect("test needs HOME"));
        let durable_parent = home.join("oxidex-ops");
        std::fs::create_dir_all(&durable_parent).expect("create durable test parent");
        let durable = tempfile::Builder::new()
            .prefix("pinned-fixture-ops-root-")
            .tempdir_in(&durable_parent)
            .expect("create canonical durable root");
        let scratch = tempfile::tempdir().expect("create scratch root");
        let mut invalid = vec![PathBuf::from("relative-ops"), scratch.path().to_path_buf()];
        #[cfg(unix)]
        {
            let linked = durable_parent.join("pinned-fixture-ops-root-link");
            let _ = std::fs::remove_file(&linked);
            std::os::unix::fs::symlink(durable.path(), &linked).expect("create ops-root symlink");
            invalid.push(linked);
        }

        for invalid_root in invalid {
            let child_result = std::process::Command::new(env::current_exe().unwrap())
                .arg("ops_root_uses_the_canonical_durable_root_contract")
                .env(OPS_ROOT_CHILD_ENV, "reject")
                .env("OXIDEX_OPS_DIR", invalid_root)
                .env_remove("EXIFTOOL")
                .env_remove("EXIFTOOL_CACHE_DIR")
                .env_remove(REQUIRED_ENV)
                .status()
                .expect("run invalid ops-root control");
            assert!(child_result.success(), "invalid ops-root control failed");
        }

        for (mode, configured_root) in [
            ("canonical", durable.path().to_path_buf()),
            (
                "tilde",
                PathBuf::from("~/oxidex-ops").join(durable.path().file_name().unwrap()),
            ),
        ] {
            let child_result = std::process::Command::new(env::current_exe().unwrap())
                .arg("ops_root_uses_the_canonical_durable_root_contract")
                .env(OPS_ROOT_CHILD_ENV, mode)
                .env("OXIDEX_OPS_DIR", configured_root)
                .env("EXPECTED_OPS_ROOT", durable.path())
                .env_remove("EXIFTOOL")
                .env_remove("EXIFTOOL_CACHE_DIR")
                .env_remove(REQUIRED_ENV)
                .status()
                .expect("run valid ops-root control");
            assert!(
                child_result.success(),
                "valid ops-root control failed for {mode}"
            );
        }
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(durable_parent.join("pinned-fixture-ops-root-link"));
        }
    }
}

#[cfg(unix)]
#[test]
fn normalized_path_preserves_non_utf8_unix_bytes_while_trimming_ascii_whitespace() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let value = std::ffi::OsString::from_vec(vec![b' ', b'\t', 0xff, b' ', b'x', b'\n']);
    let normalized = normalized_path(&value).expect("nonblank path");
    assert_eq!(normalized.as_os_str().as_bytes(), &[0xff, b' ', b'x']);
}

#[cfg(not(unix))]
#[test]
fn normalized_path_trims_ascii_whitespace_portably() {
    assert_eq!(
        normalized_path(OsStr::new(" \tfixture path\n ")),
        Some(PathBuf::from("fixture path"))
    );
}

#[derive(Debug)]
pub struct MissingFixture {
    requested: String,
    checked: Vec<PathBuf>,
}

impl fmt::Display for MissingFixture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "required pinned {} is absent; checked: {}",
            self.requested,
            self.checked
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl std::error::Error for MissingFixture {}
