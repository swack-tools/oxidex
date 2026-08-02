//! Tag extraction modules for both OxiDex and ExifTool

pub mod cache_dir;
pub mod exiftool_extractor;
pub mod oxidex_extractor;

pub use exiftool_extractor::ExifToolExtractor;
pub use oxidex_extractor::OxiDexExtractor;

use crate::models::TagInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result of extracting tags from files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    /// Tags extracted from files
    pub tags: Vec<TagInfo>,
    /// Number of files processed
    pub files_processed: usize,
    /// Every (file, value) instance found for each `family:name` key,
    /// NOT collapsed to one canonical value across the whole corpus the
    /// way `tags` is.
    ///
    /// `tags` keeps only the first file (in whatever order `WalkDir`/the
    /// batch extractor happens to visit) that produced each key -- fine
    /// for presence (`matched_tags`/`missing_in_oxidex`/`extra_in_oxidex`
    /// only care whether a key exists anywhere), but wrong for
    /// `value_differences`: a tag name that recurs across many different
    /// source files with legitimately different per-file values (Sony's
    /// `AFStatus*` binary-data tags are a real example -- every camera
    /// body has its own AF sensor readings) can have its corpus-wide
    /// "canonical" oxidex value come from a *different file* than the
    /// one ExifTool's canonical value came from. The comparison then
    /// reports two unrelated cameras' real values as a same-file mismatch
    /// stamped with whichever file happened to win the ExifTool side --
    /// see the `SonyDSLR-A580.jpg` / `SonySLT-A65.jpg` case fixed
    /// alongside this field (AFStatus value_differences that vanished
    /// when re-compared per file instead of per corpus).
    ///
    /// `ComparisonEngine::compare` uses this map to require BOTH sides'
    /// values come from the SAME source file before calling something a
    /// value difference; `tags` continues to drive every other stat so
    /// none of that pre-existing behavior changes.
    ///
    /// `#[serde(default)]` so on-disk cache entries written before this
    /// field existed still deserialize (they are invalidated by the
    /// binary hash / ExifTool version regardless, but a parse failure
    /// would be silent otherwise).
    #[serde(default)]
    pub all_instances: HashMap<String, Vec<TagInfo>>,
    /// Displayed `family:name` keys that a SINGLE source file emitted
    /// more than once with more than one DISTINCT value (spec M3).
    ///
    /// Reported from the extractor rather than smuggled through `tags`
    /// as duplicate clones: `tags` is what builds
    /// `ComparisonEngine::compare`'s value-lookup maps, and injecting
    /// same-keyed clones there made the value the report attributes to
    /// oxidex depend on which clone landed last, re-introducing exactly
    /// the ordering coupling this field exists to eliminate. Only the
    /// extractor can see both sides of a collision anyway -- by the time
    /// `compare` runs, `flatten_metadata` has already collapsed each key
    /// to one value.
    ///
    /// `#[serde(default)]` so on-disk cache entries written before this
    /// field existed still deserialize (they are invalidated by the
    /// binary hash regardless, but a parse failure would be silent).
    #[serde(default)]
    pub duplicate_emissions: Vec<String>,
}
