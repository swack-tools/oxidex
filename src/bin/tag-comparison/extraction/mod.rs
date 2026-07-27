//! Tag extraction modules for both OxiDex and ExifTool

pub mod exiftool_extractor;
pub mod oxidex_extractor;

pub use exiftool_extractor::ExifToolExtractor;
pub use oxidex_extractor::OxiDexExtractor;

use crate::models::TagInfo;
use serde::{Deserialize, Serialize};

/// Result of extracting tags from files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    /// Tags extracted from files
    pub tags: Vec<TagInfo>,
    /// Number of files processed
    pub files_processed: usize,
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
