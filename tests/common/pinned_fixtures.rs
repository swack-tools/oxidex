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
use std::path::{Path, PathBuf};

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
    cache_dir: PathBuf,
    required: bool,
}

impl FixtureConfig {
    pub fn new(source_tree: Option<PathBuf>, cache_dir: PathBuf, required: bool) -> Self {
        Self {
            source_tree,
            cache_dir,
            required,
        }
    }

    pub fn from_environment(repo_pin: &str) -> Self {
        Self::from_explicit_environment_values(
            repo_pin,
            env::var_os("EXIFTOOL").as_deref(),
            env::var_os("EXIFTOOL_CACHE_DIR").as_deref(),
            durable_cache_dir_from_environment(repo_pin),
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
        let source_tree = exiftool
            .and_then(normalized_path)
            .and_then(|binary| binary.parent().map(Path::to_path_buf));
        if let Some(tree) = &source_tree {
            assert_pinned_source_tree(tree, repo_pin, "EXIFTOOL");
        }
        let explicit_cache = cache.and_then(normalized_path);
        if let Some(cache) = &explicit_cache {
            assert_pinned_cache_dir(cache, repo_pin, "EXIFTOOL_CACHE_DIR");
        }
        let cache_dir = explicit_cache.unwrap_or(fallback_cache);
        Self::new(source_tree, cache_dir, required)
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
        self.directory_for_mode("combined-samples directory", vec![self.combined_dir()])
    }

    pub fn candidates(&self, name: &str, population: FixturePopulation) -> Vec<PathBuf> {
        match population {
            FixturePopulation::Any => self
                .t_images_directories()
                .into_iter()
                .map(|directory| directory.join(name))
                .chain(std::iter::once(self.combined_dir().join(name)))
                .collect(),
            FixturePopulation::TImages => self
                .t_images_directories()
                .into_iter()
                .map(|directory| directory.join(name))
                .collect(),
            FixturePopulation::Combined => vec![self.combined_dir().join(name)],
        }
    }

    fn t_images_directories(&self) -> Vec<PathBuf> {
        self.source_tree
            .iter()
            .map(|tree| tree.join("t/images"))
            .chain(std::iter::once(self.cache_dir.join("exiftool/t/images")))
            .collect()
    }

    fn combined_dir(&self) -> PathBuf {
        self.cache_dir.join("combined-samples")
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
    let value = value.to_string_lossy();
    let value = value.trim();
    (!value.is_empty()).then(|| PathBuf::from(value))
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

fn durable_cache_dir_from_environment(repo_pin: &str) -> PathBuf {
    let ops_dir = env::var_os("OXIDEX_OPS_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let home_dir = if ops_dir.is_some() {
        None
    } else {
        env::var_os("HOME").map(PathBuf::from)
    };
    durable_cache_dir_from_values(ops_dir.as_deref(), home_dir.as_deref(), repo_pin)
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
