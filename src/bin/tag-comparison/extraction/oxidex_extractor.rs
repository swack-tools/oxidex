//! OxiDex tag extractor - Extract tags by running OxiDex on test fixtures
//!
//! This module extracts metadata tags from test fixture files using the OxiDex
//! library. It handles conversion of internal TagValue types to string representations
//! that match ExifTool's output format.
//!
//! # ExifTool Compatibility
//!
//! Before comparison, all metadata is passed through `format_for_exiftool()` to ensure
//! values are formatted consistently with ExifTool's output. This handles GPS references,
//! binary decoders, enum values, unit suffixes, and numeric precision.

use super::ExtractionResult;
use crate::comparison::engine::normalize_family_for_comparison;
use crate::models::TagInfo;
use oxidex::core::TagValue;
use oxidex::core::exiftool_compat::format_for_exiftool;
use oxidex::core::tag_normalization::normalize_tag_family;
// Unit suffixing must come from the same module the library itself ships --
// `exiftool_compat::format_tag_value` calls
// `core::formatters::unit_suffixes::format_with_unit`, so importing a
// second implementation here would score oxidex against a formatter it does
// not use.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// On-disk cache entry for one format's OxiDex extraction. Unlike ExifTool's
/// output (which is stable across a whole fix-loop run), OxiDex's output can
/// legitimately change every time a fix gets applied and rebuilt -- so this
/// is keyed on the currently-running binary's own content hash rather than a
/// version string: a rebuild changes that hash automatically, forcing a
/// fresh extraction exactly when (and only when) the code actually changed.
/// A round where the last diff was rejected/reverted leaves the binary
/// byte-for-byte identical, so this hits and skips re-extracting from
/// scratch every round even though nothing was actually fixed.
#[derive(Debug, Serialize, Deserialize)]
struct DiskCacheEntry {
    binary_hash: String,
    signature: String,
    result: ExtractionResult,
}

/// Per displayed `family:name` key, the sorted DISTINCT values that two
/// or more raw metadata keys produced for it within ONE source file.
/// More than one entry in the `Vec` is a duplicate emission; exactly one
/// is the benign IFD0/ExifIFD redundancy real cameras write. See
/// `OxiDexExtractor::flatten_metadata`.
type CollisionMap = HashMap<String, Vec<String>>;

/// Extract tags from OxiDex by processing test fixtures
pub struct OxiDexExtractor {
    fixture_path: PathBuf,
    cache: HashMap<String, ExtractionResult>,
    cache_dir_override: Option<PathBuf>,
}

impl OxiDexExtractor {
    /// Create a new OxiDex extractor
    pub fn new(fixture_path: PathBuf) -> Self {
        Self {
            fixture_path,
            cache: HashMap::new(),
            cache_dir_override: None,
        }
    }

    /// Pin the on-disk cache directory explicitly (wired from the
    /// `--tag-cache-dir` CLI flag), overriding the `OXIDEX_TAG_CACHE_DIR`
    /// env var and the fixture-hash-keyed temp dir default. See
    /// `cache_dir::resolve_cache_dir`.
    pub fn with_cache_dir_override(mut self, dir: PathBuf) -> Self {
        self.cache_dir_override = Some(dir);
        self
    }

    /// Extract tags from all fixtures of a specific format
    ///
    /// Plain `fn`, not `async fn`: this body is entirely blocking
    /// (per-file reads and in-memory formatting, no I/O that ever
    /// suspends), so the `async` this signature carried until 2026-08-08
    /// never actually yielded -- see `ExifToolExtractor::extract_format_tags`
    /// for the fuller version of this note. Synchronous throughout lets
    /// `main.rs` parallelize the per-format loop with `rayon` instead.
    pub fn extract_format_tags(
        &mut self,
        format: &str,
    ) -> Result<ExtractionResult, Box<dyn std::error::Error>> {
        // Check in-memory cache first
        if let Some(cached) = self.cache.get(format) {
            return Ok(cached.clone());
        }

        // Find files by extension recursively throughout the samples directory
        let files: Vec<PathBuf> = self.find_files_by_extension(format)?;

        let files_processed = files.len();

        if files.is_empty() {
            return Ok(ExtractionResult {
                tags: Vec::new(),
                files_processed: 0,
                duplicate_emissions: Vec::new(),
                all_instances: HashMap::new(),
            });
        }

        // Check the on-disk cache next -- see DiskCacheEntry's docs. Only
        // meaningful once this binary was actually built once and run from
        // disk (current_exe/hashing an in-memory-only test binary isn't
        // useful), so a hashing failure just means "treat as a miss".
        let signature = Self::compute_signature(&files);
        let binary_hash = Self::current_binary_hash();
        if let Some(hash) = &binary_hash
            && let Some(cached) = self.load_disk_cache(format, hash, &signature)
        {
            self.cache.insert(format.to_string(), cached.clone());
            return Ok(cached);
        }

        // Extract tags from each file. `all_tags` keeps ONE canonical
        // TagInfo per format-wide key (first file it's seen in wins,
        // matching the pre-existing cross-file reduction other report
        // fields -- matched/missing/extra_in_oxidex/value_differences --
        // depend on), now additionally stamped with `source_file` (spec
        // M3). `duplicate_emissions` is collected alongside it: whenever
        // `flatten_metadata` reports that a SINGLE file emitted the same
        // displayed key more than once with more than one DISTINCT value
        // (a registry/dynamic-name emitter collision -- the exact bug
        // class M3 targets, and one the literal-string diff backstop
        // can't see), that key is recorded here.
        //
        // Until 2026-07-26 this was smuggled through `tags` instead, as
        // two `tag_info.clone()`s per duplicate key, so that
        // `ComparisonEngine::compare`'s per-(source_file, key) distinct-
        // value count could find something. That could never work:
        // `tag_info` had already been through `flatten_metadata`'s
        // last-write-wins `tag_map.insert`, so the losing value was
        // destroyed before the evidence was built, both clones carried
        // the SAME surviving value, and compare()'s `values.len() > 1`
        // test saw a one-element set every single time. The gate written
        // specifically to catch double-emission was structurally
        // incapable of catching double-emission.
        //
        // Measured against the live shared cache that day: the disk
        // cache's gif.json entry held exactly 3 `GIF:BackgroundColor`
        // TagInfo entries (1 canonical +
        // the 2 clones) whose value set was the singleton {'0'} or
        // {'#00'} -- never both -- while GIF.gif genuinely emits
        // BackgroundColor twice with two different values. Ten formats in
        // that cache carried duplicate evidence (jpeg 15 keys, mp4 13,
        // raf 5, bmp 4, psd 3, gif 2, mrw 2, mp3/nef/ttf 1) and every one
        // of them reported duplicate_emissions=0.
        let mut all_tags: HashMap<String, TagInfo> = HashMap::new();
        let mut all_instances: HashMap<String, Vec<TagInfo>> = HashMap::new();
        let mut duplicate_emissions: HashSet<String> = HashSet::new();

        for file_path in &files {
            match self.extract_tags_from_file(file_path) {
                Ok((file_tags, collisions)) => {
                    let source_file = file_path.display().to_string();
                    // More than one DISTINCT value only. Two raw keys
                    // colliding on one displayed key with an IDENTICAL
                    // value is the ordinary IFD0/ExifIFD redundancy real
                    // cameras write, and stays unreported -- the same
                    // exemption compare()'s `values.len() > 1` encodes,
                    // and the reason every squad's batch check stopped
                    // false-failing.
                    for (key, values) in &collisions {
                        if values.len() > 1 {
                            duplicate_emissions.insert(key.clone());
                        }
                    }
                    for tag_info in file_tags {
                        let key = format!("{}:{}", tag_info.family, tag_info.name);
                        let stamped = tag_info.with_source_file(source_file.clone());
                        all_tags
                            .entry(key.clone())
                            .or_insert_with(|| stamped.clone());
                        all_instances.entry(key).or_default().push(stamped);
                    }
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to extract tags from {}: {}",
                        file_path.display(),
                        e
                    );
                }
            }
        }

        let mut tags: Vec<TagInfo> = all_tags.into_values().collect();
        tags.sort_by_key(|a| a.key());

        let mut duplicate_emissions: Vec<String> = duplicate_emissions.into_iter().collect();
        duplicate_emissions.sort();

        let result = ExtractionResult {
            tags: tags.clone(),
            files_processed,
            duplicate_emissions,
            all_instances,
        };

        self.cache.insert(format.to_string(), result.clone());
        if let Some(hash) = &binary_hash {
            self.save_disk_cache(format, hash, &signature, &result);
        }

        Ok(result)
    }

    /// Directory the on-disk cache lives in. See
    /// `cache_dir::resolve_cache_dir` -- this is deliberately independent of
    /// `fixture_path`'s parent, which used to be the samples corpus itself
    /// whenever `fixture_path` was pointed at a vendor subdirectory (e.g.
    /// `combined-samples/Olympus`), writing the cache inside the read-only
    /// corpus.
    fn disk_cache_dir(&self) -> PathBuf {
        super::cache_dir::resolve_cache_dir(
            &self.fixture_path,
            "oxidex-tag-cache",
            self.cache_dir_override.as_deref(),
        )
    }

    fn disk_cache_path(&self, format: &str) -> PathBuf {
        self.disk_cache_dir()
            .join(format!("{}.json", format.to_lowercase()))
    }

    /// Cheap signature of the exact sample set this format's cache entry
    /// covers -- path, size, and mtime per file, hashed together. Any
    /// change to the corpus changes this, invalidating the cache.
    fn compute_signature(files: &[PathBuf]) -> String {
        let mut sorted: Vec<&PathBuf> = files.iter().collect();
        sorted.sort();
        let mut hasher_input = String::new();
        for path in sorted {
            if let Ok(meta) = std::fs::metadata(path) {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                hasher_input.push_str(&format!("{}|{}|{}\n", path.display(), meta.len(), mtime));
            } else {
                hasher_input.push_str(&format!("{}|?|?\n", path.display()));
            }
        }
        format!("{:x}", md5::compute(hasher_input.as_bytes()))
    }

    /// MD5 of the currently-running executable's own bytes -- a rebuild
    /// (new fix applied and compiled) changes this automatically, so the
    /// cache invalidates exactly when OxiDex's actual behavior could have
    /// changed. Returns None if the exe path or its bytes can't be read
    /// (e.g. sandboxed environments); callers treat that as "skip caching"
    /// rather than erroring.
    fn current_binary_hash() -> Option<String> {
        // Cache-invalidation key only (see docstring above), not a trust or
        // security decision, so current_exe's spoofability doesn't apply.
        let exe_path = std::env::current_exe().ok()?; // nosemgrep: rust.lang.security.current-exe.current-exe
        let bytes = std::fs::read(exe_path).ok()?;
        Some(format!("{:x}", md5::compute(&bytes)))
    }

    fn load_disk_cache(
        &self,
        format: &str,
        binary_hash: &str,
        signature: &str,
    ) -> Option<ExtractionResult> {
        let content = std::fs::read_to_string(self.disk_cache_path(format)).ok()?;
        let entry: DiskCacheEntry = serde_json::from_str(&content).ok()?;
        if entry.binary_hash == binary_hash && entry.signature == signature {
            Some(entry.result)
        } else {
            None
        }
    }

    /// Best-effort -- a failure to persist the cache must never fail the
    /// extraction itself, since the result was already computed correctly.
    fn save_disk_cache(
        &self,
        format: &str,
        binary_hash: &str,
        signature: &str,
        result: &ExtractionResult,
    ) {
        let dir = self.disk_cache_dir();
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let entry = DiskCacheEntry {
            binary_hash: binary_hash.to_string(),
            signature: signature.to_string(),
            result: result.clone(),
        };
        if let Ok(json) = serde_json::to_string(&entry) {
            let _ = std::fs::write(self.disk_cache_path(format), json);
        }
    }

    /// Extract tags from a single file using OxiDex
    ///
    /// This method reads raw metadata from the file and applies ExifTool-compatible
    /// formatting before flattening into TagInfo structures. The formatting ensures
    /// that GPS references, binary values, enums, and numeric precision match
    /// ExifTool's output format for accurate comparison.
    ///
    /// Returns the flattened tags plus, per displayed `family:name` key
    /// that `flatten_metadata` found more than one raw source for within
    /// this single file, every DISTINCT value those sources produced
    /// (spec M3 duplicate-emission evidence).
    fn extract_tags_from_file(
        &self,
        file_path: &Path,
    ) -> Result<(Vec<TagInfo>, CollisionMap), Box<dyn std::error::Error>> {
        // Step 1: Read raw metadata from the file
        let raw_metadata = oxidex::core::operations::read_metadata(file_path)?;

        // Step 2: Apply ExifTool-compatible formatting to all values
        // This ensures GPS refs, binary decoders, enums, units, and precision
        // match ExifTool's output before we compare the results
        let formatted_metadata = format_for_exiftool(&raw_metadata);

        // Step 3: Determine format from file extension
        let format = file_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_uppercase());

        // Step 4: Flatten the formatted metadata into TagInfo structures
        let (tags, collisions) = self.flatten_metadata(&formatted_metadata, format.as_deref());
        Ok((tags, collisions))
    }

    /// Flatten an already-formatted library value to its comparison string.
    ///
    /// `format_for_exiftool` has run before this (`extract_tags_from_file`
    /// step 2), so every ExifTool rendering rule the library knows has
    /// already been applied. This function is deliberately tag-blind: it
    /// only maps `TagValue` variants to text. Any per-tag conversion added
    /// here scores the library as matching ExifTool when the library's own
    /// rendering is wrong -- that is how the pre-#655 UserComment defect
    /// stayed invisible for months. A branch census over the 4,238-file
    /// sample corpus confirmed every tag-conditional branch this function
    /// used to carry was either dead (the library had already formatted the
    /// value), a no-op (ExposureTime ratios arrived pre-simplified), or a
    /// mask over a library gap that is now fixed in the library itself
    /// (SensitivityType-family enums, QuickTime dates, HDRGainCurve,
    /// XMP timestamp auto-conversion).
    fn format_value(value: &TagValue) -> String {
        match value {
            TagValue::String(s) => s.clone(),
            TagValue::Integer(i) => i.to_string(),
            TagValue::Float(f) => {
                let formatted = format!("{:.5}", f);
                formatted
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string()
            }
            TagValue::Rational {
                numerator,
                denominator,
            } => {
                if *denominator == 0 {
                    return "inf".to_string();
                }
                let value = *numerator as f64 / *denominator as f64;
                let formatted = format!("{:.9}", value);
                formatted
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string()
            }
            TagValue::Binary(bytes) => format!(
                "(Binary data {} bytes, use -b option to extract)",
                bytes.len()
            ),
            TagValue::DateTime(dt) => dt.format("%Y:%m:%d %H:%M:%S").to_string(),
            TagValue::Struct(_) => "[Structured data]".to_string(),
            TagValue::Array(arr) => {
                serde_json::to_string(&arr.iter().map(Self::format_value).collect::<Vec<String>>())
                    .expect("a vector of strings is JSON-serializable")
            }
        }
    }

    /// Normalize QuickTime track suffix tags for ExifTool comparison
    /// ExifTool outputs audio track tags (from track 2) without suffix,
    /// while OxiDex uses _2 suffix to distinguish tracks.
    /// This function maps _2 suffix audio tags to non-suffix versions when needed.
    fn normalize_quicktime_track_tags(tag_map: &mut HashMap<String, String>) {
        // Audio-specific tags that ExifTool shows from the audio track without suffix
        let audio_tags = [
            "AudioBitsPerSample",
            "AudioChannels",
            "AudioFormat",
            "AudioSampleRate",
            "Balance",
            "HandlerClass",
        ];

        // For audio tags, if _2 version exists and non-suffix doesn't exist or is empty, copy it
        for tag in &audio_tags {
            let key_with_suffix = format!("QuickTime:{}_2", tag);
            let key_without_suffix = format!("QuickTime:{}", tag);
            if let Some(suffix_value) = tag_map.get(&key_with_suffix).cloned() {
                // Copy if non-suffix doesn't exist OR non-suffix is empty but suffix has value
                let should_copy = match tag_map.get(&key_without_suffix) {
                    None => true,
                    Some(existing) => existing.trim().is_empty() && !suffix_value.trim().is_empty(),
                };
                if should_copy {
                    tag_map.insert(key_without_suffix, suffix_value);
                }
            }
        }

        // Special handling for MediaTimeScale: ExifTool uses audio track value
        // If MediaTimeScale_2 exists, use its value for MediaTimeScale
        let media_timescale_2 = "QuickTime:MediaTimeScale_2";
        let media_timescale = "QuickTime:MediaTimeScale";
        if let Some(audio_timescale) = tag_map.get(media_timescale_2).cloned() {
            tag_map.insert(media_timescale.to_string(), audio_timescale);
        }
    }

    /// Apply comparison-specific normalization for ExifTool compatibility reports
    /// This normalizes families for the comparison tool documentation output
    /// Check if a tag family should be skipped (pseudo-tags, not actual metadata)
    fn should_skip_family(family: &str) -> bool {
        matches!(
            family,
            "File" | "System" | "UNKNOWN"
                // Compatibility-only aliases emitted alongside canonical
                // APP10/AROT gain-curve keys. ExifTool has no HDR family-0
                // group, and folding these aliases into APP11 manufactured
                // three oxidex-only tags on every AROT sample.
                | "HDR"
        )
    }

    /// Capitalize the first letter of a string to match ExifTool naming conventions
    fn capitalize_first(s: &str) -> String {
        let mut chars = s.chars();
        match chars.next() {
            None => String::new(),
            Some(first) => first.to_uppercase().chain(chars).collect(),
        }
    }

    fn normalize_for_comparison(tag_key: &str, format: Option<&str>) -> String {
        // Handle PNG special cases first
        // PNG:tEXt:Author → PNG:Author
        // PNG:tEXt:date:create → PNG:Datecreate
        // PNG-pHYs:PixelUnits → PNG:PixelUnits
        // ExifTool capitalizes PNG text chunk keywords (comment → Comment)
        if let Some(rest) = tag_key.strip_prefix("PNG:tEXt:") {
            // Handle date:create → Datecreate format
            // ExifTool uses lowercase after "Date" (Datecreate, not DateCreate)
            if let Some(date_part) = rest.strip_prefix("date:") {
                // date:create → Datecreate, date:modify → Datemodify, date:timestamp → Datetimestamp
                return format!("PNG:Date{}", date_part);
            }
            // Capitalize the keyword to match ExifTool (comment → Comment)
            return format!("PNG:{}", Self::capitalize_first(rest));
        }
        if let Some(rest) = tag_key.strip_prefix("PNG-pHYs:") {
            return format!("PNG:{}", rest);
        }
        if let Some(rest) = tag_key.strip_prefix("PNG:iTXt:") {
            // Capitalize the keyword to match ExifTool
            return format!("PNG:{}", Self::capitalize_first(rest));
        }
        if let Some(rest) = tag_key.strip_prefix("PNG:zTXt:") {
            // Capitalize the keyword to match ExifTool
            return format!("PNG:{}", Self::capitalize_first(rest));
        }

        if let Some((family, name)) = tag_key.split_once(':') {
            let normalized_family = match family {
                // ExifIFD, IFD0, IFD1, IFD2, GPS, and InteropIFD tags are output as
                // EXIF in comparison reports. Perl ExifTool outputs GPS tags as
                // EXIF:GPSxxx, and groups the thumbnail (IFD1), the Leica-preview
                // directory (IFD2 -- see tiff_helpers.rs's parse_ifd2_preview_image),
                // and Interoperability (InteropIFD) sub-IFDs under the same
                // top-level "EXIF" family by default.
                "ExifIFD" | "IFD0" | "IFD1" | "IFD2" | "GPS" | "InteropIFD" => "EXIF",
                // MP4/QuickTime: ItemList and UserData → QuickTime for comparison
                "ItemList" | "UserData" => "QuickTime",
                // WebP tags map to RIFF family in ExifTool
                "WebP" => "RIFF",
                // EXR tags map to OpenEXR family in ExifTool
                "EXR" => "OpenEXR",
                // Keep the extractor and engine on one shared family-alias
                // table. A shorter duplicate list here used to leave Leica
                // tags uncollapsed while compare() folded them to MakerNotes;
                // mixed-vendor corpus runs then retained both keys and
                // reported the Leica spelling as a false oxidex-only tag.
                _ => normalize_family_for_comparison(family),
            };
            format!("{}:{}", normalized_family, name)
        } else if let Some(fmt) = format {
            // No family prefix - use format as family (e.g., GIF:GIFVersion)
            // Apply family normalization to format-based families
            let format_family = fmt.to_uppercase();
            let normalized_family = match format_family.as_str() {
                "EXR" => "OpenEXR",
                other => other,
            };
            format!("{}:{}", normalized_family, tag_key)
        } else {
            tag_key.to_string()
        }
    }

    /// Record one write into `tag_map`, remembering the clobbered value.
    ///
    /// `tag_map` stays last-write-wins (every downstream report field
    /// depends on exactly one displayed value per key), but when a write
    /// lands on a key that already has one, BOTH values are appended to
    /// `collisions` so the losing value survives long enough for
    /// `ComparisonEngine::compare` to see it. See `flatten_metadata`.
    fn record_write(
        tag_map: &mut HashMap<String, String>,
        collisions: &mut CollisionMap,
        normalized_key: String,
        value: String,
    ) {
        if let Some(previous) = tag_map.get(&normalized_key) {
            collisions
                .entry(normalized_key.clone())
                .or_insert_with(|| vec![previous.clone()])
                .push(value.clone());
        }
        tag_map.insert(normalized_key, value);
    }

    /// ExifTool's family-0 XMP group intentionally collapses some properties
    /// from distinct family-1 schemas onto the same displayed key. For
    /// example, one packet may legally contain both `XMP-crs:Sharpness` and
    /// `XMP-exif:Sharpness`; `exiftool -G0 -a` prints both as
    /// `XMP:Sharpness`, and `-json -G` keeps one deterministic value. Those
    /// are namespace peers, not OxiDex emitting one conceptual tag twice, so
    /// they must not trip the duplicate-emission gate used by squad batches.
    fn is_exiftool_family0_xmp_overlap(normalized_key: &str) -> bool {
        matches!(
            normalized_key,
            "XMP:NativeDigest" | "XMP:Sharpness" | "XMP:WhiteBalance"
        )
    }

    /// The seven MP Entry tag names repeat once per embedded image, each under
    /// its own indexed family-1 group (`MPImage1:MPImageStart`,
    /// `MPImage2:MPImageStart`, ... -- MPF.pm:247). ExifTool's own family-0
    /// view collapses them exactly the same way: `exiftool -G0 -a` prints
    /// `[MPF] MPImageStart` once per image and `-json -G` keeps one. They are
    /// per-image peers, not OxiDex computing one conceptual tag twice, so --
    /// like the XMP namespace peers above -- they must not trip the
    /// duplicate-emission gate.
    ///
    /// Sorted-key order makes the surviving value the highest-numbered group,
    /// which is the one ExifTool's family-0 JSON keeps, for up to nine images.
    /// The sample corpus tops out at three MP Entries (737 MPF files: 48 with
    /// one, 648 with two, 41 with three).
    fn is_exiftool_family0_mp_image_peer(normalized_key: &str) -> bool {
        matches!(
            normalized_key,
            "MPF:MPImageFlags"
                | "MPF:MPImageFormat"
                | "MPF:MPImageType"
                | "MPF:MPImageLength"
                | "MPF:MPImageStart"
                | "MPF:DependentImage1EntryNumber"
                | "MPF:DependentImage2EntryNumber"
        )
    }

    /// ExifTool's `Exif.pm` promotes a NEF SubIFD with `SubfileType = 0`
    /// (full-resolution) over IFD0's reduced-resolution thumbnail. The
    /// parser records the promoted directory under `EXIF:` already; without
    /// this filter, family-0 flattening normalizes IFD0 to the same key and
    /// its later sorted write overwrites the promoted value.
    fn nef_ifd0_field_is_superseded(key: &str, metadata: &oxidex::core::MetadataMap) -> bool {
        let Some(name) = key.strip_prefix("IFD0:") else {
            return false;
        };
        matches!(
            name,
            "BitsPerSample"
                | "Compression"
                | "ImageHeight"
                | "ImageWidth"
                | "PhotometricInterpretation"
                | "RowsPerStrip"
                | "SamplesPerPixel"
                | "StripOffsets"
                | "SubfileType"
        ) && metadata.contains_key(&format!("EXIF:{name}"))
    }

    /// Flatten MetadataMap into TagInfo vector
    ///
    /// Returns the flattened tags plus, for every displayed `family:name`
    /// key that had more than one DIFFERENT raw `metadata` key normalize
    /// down to it, the sorted DISTINCT values those raw keys produced
    /// (spec M3: a registry/dynamic-name emitter computing the same
    /// conceptual tag twice via two different raw paths, where the second
    /// write silently clobbers the first in `tag_map` below). `metadata`
    /// itself is already a `HashMap`, so a literal repeated raw key is
    /// structurally impossible here -- this only catches
    /// post-normalization collisions between genuinely distinct raw keys,
    /// which is exactly the class the literal-string diff backstop
    /// (`detect_duplicate_tag_insertion`) is blind to.
    ///
    /// DETERMINISM (2026-07-26): the raw keys are visited in sorted
    /// order, NOT `MetadataMap::iter()` order. `MetadataMap` wraps a
    /// `std::collections::HashMap` with the default `RandomState`, whose
    /// hasher is seeded per process, so its iteration order differs from
    /// run to run. Combined with `record_write`'s last-write-wins, that
    /// made the surviving value of any post-normalization collision a
    /// per-process coin flip, and every report field derived from it
    /// (matched_tags, value_differences, the whole gap list)
    /// non-reproducible on an unchanged source tree.
    ///
    /// Measured before this fix, 12 runs of one binary built from
    /// 21293fb2 over one file (GIF.gif), extraction cache cleared each
    /// time: 7 runs reported matched=34 value_differences=1
    /// (`GIF:BackgroundColor` exiftool="0" oxidex="#00"), 5 runs reported
    /// matched=35 value_differences=0. Identical binary, identical file,
    /// identical argv. Minolta.mrw flipped the same way across 15 runs
    /// (matched 35/36/37, value_differences 5/6/7, on
    /// `EXIF:ImageWidth` "3264" vs "12337"). That is what let a fleet
    /// worker record "Verified: recheck-pass gaps=1->0" against a
    /// measurement artifact rather than a real defect: the 22:17 gap and
    /// the 22:37 clean run came from the SAME source tree.
    ///
    /// A collision still collapses to one value here -- resolving it is
    /// the parser's job, not this harness's -- but it now collapses the
    /// same way every time, and the collision itself is reported through
    /// `collisions` -> `duplicate_evidence` -> `duplicate_emissions`
    /// instead of being silently swallowed.
    fn flatten_metadata(
        &self,
        metadata: &oxidex::core::MetadataMap,
        format: Option<&str>,
    ) -> (Vec<TagInfo>, CollisionMap) {
        let mut tag_map: HashMap<String, String> = HashMap::new();
        let mut collisions: CollisionMap = CollisionMap::new();

        let mut raw_entries: Vec<(&String, &TagValue)> = metadata.iter().collect();
        raw_entries.sort_by_key(|(key, _)| *key);

        for (key, value) in raw_entries {
            if matches!(format, Some("NEF" | "NRW"))
                && Self::nef_ifd0_field_is_superseded(key, metadata)
            {
                continue;
            }

            // Check if original family should be skipped (pseudo-tags)
            if let Some((original_family, _)) = key.split_once(':')
                && Self::should_skip_family(original_family)
            {
                continue;
            }

            // Normalize the tag family (core library normalization + comparison-specific)
            let normalized_key = Self::normalize_for_comparison(&normalize_tag_family(key), format);

            let family = if let Some(colon_pos) = normalized_key.find(':') {
                normalized_key[..colon_pos].to_string()
            } else {
                "UNKNOWN".to_string()
            };

            // Skip if normalized family should be skipped
            if Self::should_skip_family(&family) {
                continue;
            }
            let _family = family; // Keep for later use

            // Format the value
            let value_str = Self::format_value(value);
            if Self::is_exiftool_family0_xmp_overlap(&normalized_key)
                || Self::is_exiftool_family0_mp_image_peer(&normalized_key)
            {
                // Preserve the existing sorted, last-write-wins behavior so
                // our family-0 JSON view chooses the same schema value as
                // ExifTool. Only the false duplicate evidence is suppressed.
                tag_map.insert(normalized_key, value_str);
            } else {
                Self::record_write(&mut tag_map, &mut collisions, normalized_key, value_str);
            }
        }

        // Handle QuickTime track suffix normalization for ExifTool comparison
        // ExifTool outputs audio track tags without suffix, OxiDex uses _2 suffix
        Self::normalize_quicktime_track_tags(&mut tag_map);

        // Convert to Vec<TagInfo>
        let mut tags: Vec<TagInfo> = tag_map
            .into_iter()
            .map(|(key, value)| {
                if let Some(colon_pos) = key.find(':') {
                    let (family, name) = key.split_at(colon_pos);
                    TagInfo::new(name[1..].to_string(), family.to_string(), value)
                } else {
                    TagInfo::new(key.clone(), "UNKNOWN".to_string(), value)
                }
            })
            .collect();

        tags.sort_by_key(|a| a.key());

        // Distinct values only, in a stable order. Two raw keys that
        // collide but produce the SAME displayed value collapse to a
        // one-element list here, which keeps compare()'s deliberate
        // `values.len() > 1` exemption intact: real cameras writing an
        // identical value into both IFD0 and the ExifIFD stay unreported
        // (that exemption is why every squad's batch check stopped
        // false-failing), while two DIFFERENT values colliding on one key
        // now reaches compare() as the two-element set it always was.
        for values in collisions.values_mut() {
            values.sort();
            values.dedup();
        }

        (tags, collisions)
    }

    /// Find files by extension recursively throughout the samples directory
    fn find_files_by_extension(
        &self,
        format: &str,
    ) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        let extensions = Self::format_to_extensions(format);
        if extensions.is_empty() {
            return Ok(Vec::new());
        }

        let files: Vec<PathBuf> = WalkDir::new(&self.fixture_path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| {
                if !e.path().is_file() {
                    return false;
                }
                // Skip hidden files and directories
                if e.path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("."))
                {
                    return false;
                }
                if let Some(ext) = e.path().extension().and_then(|e| e.to_str()) {
                    extensions.iter().any(|x| x == &ext.to_lowercase())
                } else {
                    false
                }
            })
            .map(|e| e.path().to_path_buf())
            .collect();

        Ok(files)
    }

    /// Map format name to file extensions
    fn format_to_extensions(format: &str) -> Vec<String> {
        let named: &[&str] = match format.to_uppercase().as_str() {
            "JPEG" => &["jpg", "jpeg"],
            "PNG" => &["png"],
            "TIFF" => &["tif", "tiff"],
            "GIF" => &["gif"],
            "WEBP" => &["webp"],
            "HEIC" => &["heic", "heif"],
            "MP4" => &["mp4", "m4v", "mov"],
            "AVI" => &["avi"],
            "MKV" => &["mkv"],
            "MP3" => &["mp3"],
            "WAV" => &["wav"],
            "PDF" => &["pdf"],
            "PSD" => &["psd"],
            "CR2" => &["cr2", "cr3"],
            "NEF" => &["nef"],
            "ARW" => &["arw"],
            "DNG" => &["dng"],
            "RAF" => &["raf"],
            "ORF" => &["orf"],
            "RW2" => &["rw2"],
            "XMP" => &["xmp"],
            "FLAC" => &["flac"],
            "OGG" => &["ogg", "oga", "ogv"],
            "BMP" => &["bmp"],
            "ICO" => &["ico"],
            "SVG" => &["svg"],
            "EPS" => &["eps", "ps"],
            "FLIF" => &["flif"],
            "XCF" => &["xcf"],
            "EXR" => &["exr"],
            "JXL" => &["jxl"],
            "AVIF" => &["avif"],
            "3GP" => &["3gp", "3g2"],
            "M2TS" => &["mts", "m2ts", "ts"],
            "M4A" => &["m4a"],
            "FLV" => &["flv"],
            "WMV" => &["wmv", "asf"],
            "MXF" => &["mxf"],
            "WEBM" => &["webm"],
            "ICC" => &["icc", "icm"],
            "PEF" => &["pef"],
            "SRW" => &["srw"],
            "X3F" => &["x3f"],
            "DCR" => &["dcr"],
            "RWL" => &["rwl"],
            "3FR" => &["3fr"],
            "FFF" => &["fff"],
            "MEF" => &["mef"],
            "MOS" => &["mos"],
            "MRW" => &["mrw"],
            "NRW" => &["nrw"],
            "SR2" => &["sr2", "srf"],
            "KDC" => &["kdc"],
            "ERF" => &["erf"],
            "BPG" => &["bpg"],
            "AAC" => &["aac"],
            "APE" => &["ape"],
            "OPUS" => &["opus"],
            "AIFF" => &["aif", "aiff"],
            "HDR" => &["hdr"],
            "PPM" => &["ppm", "pgm", "pbm", "pnm"],
            "MPC" => &["mpc"],
            "RAW" => &[
                "raw", "3fr", "ari", "bay", "crw", "dcr", "dcs", "dng", "erf", "fff", "k25", "kdc",
                "mef", "mos", "mrw", "nrw", "pef", "ptx", "r3d", "raf", "rw2", "rwl", "sr2", "srf",
                "srw", "x3f",
            ],
            "PE" => &["exe", "dll", "sys"],
            "ELF" => &["elf", "so"],
            "MACHO" => &["dylib", "bundle", "macho"],
            "OTF" => &["otf"],
            "TTF" => &["ttf"],
            "WOFF" => &["woff"],
            "WOFF2" => &["woff2"],
            "DOCX" => &["docx"],
            "XLSX" => &["xlsx"],
            "PPTX" => &["pptx"],
            "ZIP" => &["zip"],
            "RAR" => &["rar"],
            "7Z" => &["7z"],
            "GZIP" => &["gz"],
            "TAR" => &["tar"],
            "ISO" => &["iso"],
            "OLE" => &["doc", "xls", "ppt", "msg", "vsd", "pub"],
            // Formats that had no entry here at all, which is why the harness
            // reported no row for them: an unnamed format finds no files, and
            // "no gap" and "not looked at" were indistinguishable.
            "DR4" => &["dr4"],
            "VRD" => &["vrd"],
            "LFP" => &["lfp", "lfr"],
            "FPF" => &["fpf"],
            "DJVU" => &["djvu", "djv"],
            "HTML" => &["html", "htm"],
            "LNK" => &["lnk"],
            // Unknown format: treat the format name as its own extension.
            // `detect_formats` groups any extension it has no name for under
            // that extension uppercased, so `FITS` must find `.fits` here or
            // the two halves disagree and the files vanish again -- silently,
            // as an empty list, which is what the old `vec![]` did.
            _ => return vec![format.to_lowercase()],
        };
        named.iter().map(|e| (*e).to_string()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oxidex_extractor_creation() {
        let extractor = OxiDexExtractor::new(PathBuf::from("tests/fixtures/jpeg"));
        assert_eq!(extractor.fixture_path, PathBuf::from("tests/fixtures/jpeg"));
    }

    #[test]
    fn test_fpf_format_discovers_standalone_fpf_files() {
        assert_eq!(OxiDexExtractor::format_to_extensions("FPF"), vec!["fpf"]);
    }

    /// Regression test for the corpus-pollution bug: pointing the extractor
    /// at a vendor subdirectory of a corpus (mirroring
    /// `combined-samples/Olympus`) must never write the on-disk cache
    /// anywhere under that corpus's observable parent
    /// (`combined-samples`), which is exactly what `fixture_path.parent()`
    /// used to resolve to.
    #[test]
    fn test_disk_cache_dir_never_lands_inside_fixture_parent() {
        let _env = super::super::cache_dir::lock_env();
        unsafe {
            std::env::remove_var(super::super::cache_dir::OXIDEX_TAG_CACHE_DIR_ENV);
        }
        let corpus_root = tempfile::tempdir().unwrap();
        let observable_parent = corpus_root.path().join("combined-samples");
        let vendor_subdir = observable_parent.join("Olympus");
        std::fs::create_dir_all(&vendor_subdir).unwrap();

        let extractor = OxiDexExtractor::new(vendor_subdir);
        let cache_dir = extractor.disk_cache_dir();

        assert!(
            !cache_dir.starts_with(&observable_parent),
            "cache dir {} must not be written inside the corpus at {}",
            cache_dir.display(),
            observable_parent.display()
        );
    }

    #[test]
    fn test_flatten_metadata_empty() {
        let extractor = OxiDexExtractor::new(PathBuf::from("tests/fixtures"));
        let metadata = oxidex::core::MetadataMap::new();
        let (tags, collisions) = extractor.flatten_metadata(&metadata, None);
        assert_eq!(tags.len(), 0);
        assert!(collisions.is_empty());
    }

    /// The flattener is tag-blind: whatever string the library produced is
    /// the string the comparison sees. `exiftool -json` keeps trailing EXIF
    /// padding too, and `normalize_value_for_comparison` trims both sides,
    /// so padding must NOT be scrubbed here -- an extractor-side trim is
    /// indistinguishable from library behavior in the results.
    #[test]
    fn format_value_passes_strings_through_verbatim() {
        assert_eq!(
            OxiDexExtractor::format_value(&TagValue::String(
                "OLYMPUS DIGITAL CAMERA         ".to_string()
            )),
            "OLYMPUS DIGITAL CAMERA         "
        );
        assert_eq!(
            OxiDexExtractor::format_value(&TagValue::String(
                "2025-11-09T15:06:20+00:00".to_string()
            )),
            "2025-11-09T15:06:20+00:00"
        );
    }

    #[test]
    fn nef_priority_subifd_wins_over_ifd0_thumbnail_fields() {
        let extractor = OxiDexExtractor::new(PathBuf::from("tests/fixtures"));
        let mut metadata = oxidex::core::MetadataMap::new();
        metadata.insert("IFD0:ImageWidth".to_string(), TagValue::Integer(160));
        metadata.insert("EXIF:ImageWidth".to_string(), TagValue::Integer(3040));
        metadata.insert(
            "IFD0:Compression".to_string(),
            TagValue::String("Uncompressed".to_string()),
        );
        metadata.insert(
            "EXIF:Compression".to_string(),
            TagValue::String("Nikon NEF Compressed".to_string()),
        );

        let (tags, collisions) = extractor.flatten_metadata(&metadata, Some("NEF"));
        let tag_values: HashMap<_, _> =
            tags.into_iter().map(|tag| (tag.key(), tag.value)).collect();
        assert_eq!(tag_values.get("EXIF:ImageWidth"), Some(&"3040".to_string()));
        assert_eq!(
            tag_values.get("EXIF:Compression"),
            Some(&"Nikon NEF Compressed".to_string())
        );
        assert!(collisions.is_empty());
    }

    #[test]
    fn test_extractor_uses_shared_family_aliases() {
        assert_eq!(
            OxiDexExtractor::normalize_for_comparison("Leica:Contrast", Some("JPEG")),
            "MakerNotes:Contrast"
        );
        assert_eq!(
            OxiDexExtractor::normalize_for_comparison("PhaseOne:SensorWidth", Some("IIQ")),
            "MakerNotes:SensorWidth"
        );
        assert_eq!(
            OxiDexExtractor::normalize_for_comparison("GoPro:MetadataVersion", Some("JPEG")),
            "APP6:MetadataVersion"
        );
        assert_eq!(
            OxiDexExtractor::normalize_for_comparison("AROT:HDRGainCurveSize", Some("JPEG")),
            "APP10:HDRGainCurveSize"
        );
    }

    #[test]
    fn test_flatten_metadata_uses_arot_and_skips_hdr_compatibility_aliases() {
        let extractor = OxiDexExtractor::new(PathBuf::from("tests/fixtures"));
        let mut metadata = oxidex::core::MetadataMap::new();
        let curve = "17707 36099 54906";
        metadata.insert(
            "AROT:HDRGainCurve".to_string(),
            TagValue::String(curve.to_string()),
        );
        metadata.insert("AROT:HDRGainCurveSize".to_string(), TagValue::Integer(3));
        metadata.insert(
            "HDR:GainCurve".to_string(),
            TagValue::String(curve.to_string()),
        );
        metadata.insert("HDR:GainCurveSize".to_string(), TagValue::Integer(3));
        metadata.insert(
            "HDR:Format".to_string(),
            TagValue::String("AROT".to_string()),
        );

        // Production applies format_for_exiftool before flattening
        // (extract_tags_from_file step 2); the HDRGainCurve binary-placeholder
        // rendering lives there now, not in the tag-blind flattener.
        let metadata = format_for_exiftool(&metadata);
        let (tags, collisions) = extractor.flatten_metadata(&metadata, Some("JPEG"));
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].key(), "APP10:HDRGainCurve");
        assert_eq!(
            tags[0].value,
            format!(
                "(Binary data {} bytes, use -b option to extract)",
                curve.len()
            )
        );
        assert_eq!(tags[1].key(), "APP10:HDRGainCurveSize");
        assert_eq!(tags[1].value, "3");
        assert!(collisions.is_empty());
    }

    /// Canon:FileNumber arrives from the Canon parser already rendered as
    /// `directory-file` (`format_canon_file_number`, Canon.pm:1260); the
    /// extractor must not re-derive it. The old bit-shift re-derivation here
    /// disagreed with ExifTool's decimal-digit split for any value it ever
    /// touched, so it only worked by never firing on the String the parser
    /// actually emits.
    #[test]
    fn test_canon_file_number_passes_through_parser_rendering() {
        let extractor = OxiDexExtractor::new(PathBuf::from("tests/fixtures"));
        let mut metadata = oxidex::core::MetadataMap::new();
        metadata.insert(
            "Canon:FileNumber".to_string(),
            TagValue::String("117-1771".to_string()),
        );
        let (tags, collisions) = extractor.flatten_metadata(&metadata, None);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].value, "117-1771");
        assert!(collisions.is_empty());
    }

    /// Spec M3: two DIFFERENT raw keys that normalize to the same
    /// displayed `family:name` must be reported as a duplicate, even
    /// though `MetadataMap` itself (a `HashMap`) makes a literal repeated
    /// raw key structurally impossible.
    #[test]
    fn test_flatten_metadata_detects_normalization_collision() {
        let extractor = OxiDexExtractor::new(PathBuf::from("tests/fixtures"));
        let mut metadata = oxidex::core::MetadataMap::new();
        // Two distinct raw keys ExifTool-family-normalization collapses
        // onto the same "MakerNotes:Sharpness" displayed key.
        metadata.insert(
            "Canon:Sharpness".to_string(),
            TagValue::String("Normal".to_string()),
        );
        metadata.insert(
            "Nikon:Sharpness".to_string(),
            TagValue::String("Hard".to_string()),
        );
        let (tags, collisions) = extractor.flatten_metadata(&metadata, None);
        assert_eq!(tags.len(), 1);
        assert!(collisions.contains_key("MakerNotes:Sharpness"));
    }

    #[test]
    fn test_exiftool_family0_xmp_namespace_overlaps_are_not_duplicates() {
        let extractor = OxiDexExtractor::new(PathBuf::from("tests/fixtures"));
        let mut metadata = oxidex::core::MetadataMap::new();
        metadata.insert(
            "XMP-exif:NativeDigest".to_string(),
            TagValue::String("exif digest".to_string()),
        );
        metadata.insert(
            "XMP-tiff:NativeDigest".to_string(),
            TagValue::String("tiff digest".to_string()),
        );
        metadata.insert(
            "XMP-exif:Sharpness".to_string(),
            TagValue::String("Normal".to_string()),
        );
        metadata.insert(
            "XMP:Sharpness".to_string(),
            TagValue::String("25".to_string()),
        );
        metadata.insert(
            "XMP-exif:WhiteBalance".to_string(),
            TagValue::String("Manual".to_string()),
        );
        metadata.insert(
            "XMP:WhiteBalance".to_string(),
            TagValue::String("Custom".to_string()),
        );

        let (tags, collisions) = extractor.flatten_metadata(&metadata, Some("JPEG"));
        assert_eq!(tags.len(), 3);
        assert!(collisions.is_empty());
        assert_eq!(
            tags.iter()
                .find(|tag| tag.key() == "XMP:NativeDigest")
                .map(|tag| tag.value.as_str()),
            Some("tiff digest")
        );
        assert_eq!(
            tags.iter()
                .find(|tag| tag.key() == "XMP:Sharpness")
                .map(|tag| tag.value.as_str()),
            Some("25")
        );
        assert_eq!(
            tags.iter()
                .find(|tag| tag.key() == "XMP:WhiteBalance")
                .map(|tag| tag.value.as_str()),
            Some("Custom")
        );
    }

    /// Two unrelated tags that don't collide must never be flagged.
    #[test]
    fn test_flatten_metadata_no_false_positive_duplicates() {
        let extractor = OxiDexExtractor::new(PathBuf::from("tests/fixtures"));
        let mut metadata = oxidex::core::MetadataMap::new();
        metadata.insert(
            "EXIF:Make".to_string(),
            TagValue::String("Canon".to_string()),
        );
        metadata.insert(
            "EXIF:Model".to_string(),
            TagValue::String("EOS 5D".to_string()),
        );
        let (tags, collisions) = extractor.flatten_metadata(&metadata, None);
        assert_eq!(tags.len(), 2);
        assert!(collisions.is_empty());
    }

    /// The GIF.gif collision that produced the 2026-07-26 phantom gap,
    /// reduced to its two raw keys: `BackgroundColor` (bare, so
    /// `normalize_for_comparison` prepends the format family) and
    /// `GIF:BackgroundColor` (already prefixed, family left alone). Both
    /// normalize to `GIF:BackgroundColor`, so one clobbers the other.
    fn gif_background_color_collision() -> oxidex::core::MetadataMap {
        let mut metadata = oxidex::core::MetadataMap::new();
        metadata.insert("BackgroundColor".to_string(), TagValue::Integer(0));
        metadata.insert(
            "GIF:BackgroundColor".to_string(),
            TagValue::String("#00".to_string()),
        );
        metadata
    }

    /// A post-normalization collision must resolve the SAME WAY on every
    /// call. `MetadataMap` wraps a `std::collections::HashMap`, and each
    /// freshly-constructed `HashMap` gets its own `RandomState` instance,
    /// so iteration order varies between maps even inside one process
    /// (measured 2026-07-26: 200 fresh two-key `HashMap`s yielded both
    /// possible orders). Before `flatten_metadata` sorted its raw keys,
    /// that order decided which value survived last-write-wins, and the
    /// whole gap list rode on it -- 12 runs of one binary over GIF.gif
    /// split 7x "matched=34 value_differences=1" / 5x "matched=35
    /// value_differences=0" from an unchanged source tree.
    ///
    /// 200 iterations, not a handful: a single iteration would pass ~50%
    /// of the time even with the bug present.
    #[test]
    fn test_flatten_metadata_collision_resolves_identically_every_call() {
        let extractor = OxiDexExtractor::new(PathBuf::from("tests/fixtures"));
        let mut survivors: HashSet<String> = HashSet::new();
        for _ in 0..200 {
            let metadata = gif_background_color_collision();
            let (tags, _) = extractor.flatten_metadata(&metadata, Some("GIF"));
            assert_eq!(tags.len(), 1, "both raw keys must collapse to one");
            survivors.insert(tags[0].value.clone());
        }
        assert_eq!(
            survivors.len(),
            1,
            "collision resolved inconsistently across 200 calls: {:?} -- \
             the surviving value must not depend on HashMap iteration order",
            survivors
        );
    }

    /// The losing value must survive as far as the caller, or
    /// `duplicate_emissions` can never fire. This is the assertion the
    /// pre-2026-07-26 code could not satisfy: it reported the colliding
    /// KEY but had already destroyed one of the two VALUES, so
    /// `ComparisonEngine::compare`'s `values.len() > 1` gate saw a
    /// one-element set and stayed silent on every duplicate emission in
    /// the corpus.
    #[test]
    fn test_flatten_metadata_reports_both_colliding_values() {
        let extractor = OxiDexExtractor::new(PathBuf::from("tests/fixtures"));
        let metadata = gif_background_color_collision();
        let (_tags, collisions) = extractor.flatten_metadata(&metadata, Some("GIF"));
        let values = collisions
            .get("GIF:BackgroundColor")
            .expect("collision on GIF:BackgroundColor must be recorded");
        assert_eq!(
            values,
            &vec!["#00".to_string(), "0".to_string()],
            "both the surviving and the clobbered value must be reported"
        );
    }

    /// The deliberate exemption must hold: two raw keys colliding on one
    /// displayed key with an IDENTICAL value is the ordinary
    /// IFD0/ExifIFD redundancy real cameras write (confirmed across
    /// Samsung/Canon/Nikon/Olympus/Panasonic/FujiFilm/Leica samples), and
    /// flagging it false-failed every squad's batch full-corpus check.
    /// gif.rs emits FrameCount twice this way. One distinct value means
    /// `extract_format_tags`'s `values.len() > 1` test does not fire.
    #[test]
    fn test_identical_value_collision_is_not_a_duplicate_emission() {
        let extractor = OxiDexExtractor::new(PathBuf::from("tests/fixtures"));
        let mut metadata = oxidex::core::MetadataMap::new();
        metadata.insert("FrameCount".to_string(), TagValue::Integer(1));
        metadata.insert("GIF:FrameCount".to_string(), TagValue::Integer(1));
        let (_tags, collisions) = extractor.flatten_metadata(&metadata, Some("GIF"));
        let values = collisions
            .get("GIF:FrameCount")
            .expect("the collision itself is still recorded");
        assert_eq!(
            values,
            &vec!["1".to_string()],
            "identical colliding values must collapse to one, keeping the \
             IFD0/ExifIFD redundancy exemption intact"
        );
        assert!(
            values.len() <= 1,
            "must not be reported as a duplicate emission"
        );
    }
}
