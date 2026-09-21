//! Test-only resolution of pinned ExifTool fixture data.
//!
//! This file is path-included by both the crate unit-test support and the
//! integration-test fixture wrapper. Keeping it outside the library API makes
//! the release-fixture contract portable without changing production behavior.

#![allow(dead_code)] // Each test target imports only the population it exercises.

use std::env;
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
        let source_tree = env::var_os("EXIFTOOL")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .and_then(|binary| binary.parent().map(Path::to_path_buf));
        let cache_dir = env::var_os("EXIFTOOL_CACHE_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| durable_cache_dir_from_environment(repo_pin));
        let required = env::var(REQUIRED_ENV).is_ok_and(|value| value == "1");
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
        let found = checked.iter().find(|path| path.is_dir()).cloned();
        if self.required {
            found
                .ok_or_else(|| MissingFixture {
                    requested: requested.to_owned(),
                    checked,
                })
                .map(Some)
        } else {
            Ok(found)
        }
    }
}

pub fn durable_cache_dir(ops_dir: Option<&Path>, home_dir: &Path, repo_pin: &str) -> PathBuf {
    ops_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home_dir.join("oxidex-ops"))
        .join("cache/exiftool")
        .join(repo_pin.trim())
}

fn durable_cache_dir_from_environment(repo_pin: &str) -> PathBuf {
    let ops_dir = env::var_os("OXIDEX_OPS_DIR").map(PathBuf::from);
    let home_dir = env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| {
        panic!("fixture resolution needs OXIDEX_OPS_DIR or HOME for its cache root")
    });
    durable_cache_dir(ops_dir.as_deref(), &home_dir, repo_pin)
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
