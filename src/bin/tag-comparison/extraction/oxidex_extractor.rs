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
use crate::models::TagInfo;
use oxidex::core::TagValue;
use oxidex::core::exiftool_compat::format_for_exiftool;
use oxidex::core::tag_normalization::normalize_tag_family;
use oxidex::core::value_formatter::{
    format_date_exif_style, format_rational_as_decimal, format_with_unit, is_decimal_rational_tag,
    needs_unit_suffix,
};
use oxidex::parsers::tiff::tiff_enums::tiff_enum_to_string;
use rayon::prelude::*;
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
}

impl OxiDexExtractor {
    /// Create a new OxiDex extractor
    pub fn new(fixture_path: PathBuf) -> Self {
        Self {
            fixture_path,
            cache: HashMap::new(),
        }
    }

    /// Extract tags from all fixtures of a specific format
    pub async fn extract_format_tags(
        &mut self,
        format: &str,
    ) -> Result<ExtractionResult, Box<dyn std::error::Error>> {
        // Check in-memory cache first
        if let Some(cached) = self.cache.get(format) {
            return Ok(cached.clone());
        }

        // Find files by extension recursively throughout the samples directory
        let mut files: Vec<PathBuf> = self.find_files_by_extension(format)?;
        // WalkDir order is filesystem-dependent. Sorting is both a cache
        // stability requirement and the tie-breaker for the canonical value
        // retained when several files emit the same displayed key.
        files.sort();

        let files_processed = files.len();

        if files.is_empty() {
            return Ok(ExtractionResult {
                tags: Vec::new(),
                files_processed: 0,
                duplicate_emissions: Vec::new(),
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
        // Measured against the live shared cache that day:
        // /tmp/oxidex-exiftool-cache/oxidex-tag-cache/gif.json held
        // exactly 3 `GIF:BackgroundColor` TagInfo entries (1 canonical +
        // the 2 clones) whose value set was the singleton {'0'} or
        // {'#00'} -- never both -- while GIF.gif genuinely emits
        // BackgroundColor twice with two different values. Ten formats in
        // that cache carried duplicate evidence (jpeg 15 keys, mp4 13,
        // raf 5, bmp 4, psd 3, gif 2, mrw 2, mp3/nef/ttf 1) and every one
        // of them reported duplicate_emissions=0.
        let mut all_tags: HashMap<String, TagInfo> = HashMap::new();
        let mut duplicate_emissions: HashSet<String> = HashSet::new();

        // Parsing individual files is independent. Parallelize this expensive
        // phase, then fold results in sorted path order so the report remains
        // byte-for-byte reproducible. This avoids the usual parallel-parser
        // trap where whichever worker finishes first wins a collision.
        let mut extracted: Vec<(PathBuf, Vec<TagInfo>, CollisionMap)> = files
            .par_iter()
            .filter_map(|file_path| match self.extract_tags_from_file(file_path) {
                Ok((file_tags, collisions)) => Some((file_path.clone(), file_tags, collisions)),
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to extract tags from {}: {}",
                        file_path.display(),
                        e
                    );
                    None
                }
            })
            .collect();
        extracted.sort_by(|a, b| a.0.cmp(&b.0));

        for (file_path, file_tags, collisions) in extracted {
            let source_file = file_path.display().to_string();
            for (key, values) in &collisions {
                if values.len() > 1 {
                    duplicate_emissions.insert(key.clone());
                }
            }
            for tag_info in file_tags {
                let key = format!("{}:{}", tag_info.family, tag_info.name);
                all_tags
                    .entry(key)
                    .or_insert_with(|| tag_info.with_source_file(source_file.clone()));
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
        };

        self.cache.insert(format.to_string(), result.clone());
        if let Some(hash) = &binary_hash {
            self.save_disk_cache(format, hash, &signature, &result);
        }

        Ok(result)
    }

    /// Directory the on-disk cache lives in: a sibling of the samples dir
    /// itself, keeping it alongside ExifTool's own disk cache dir rather
    /// than inside the samples tree.
    fn disk_cache_dir(&self) -> PathBuf {
        self.fixture_path
            .parent()
            .map(|p| p.join("oxidex-tag-cache"))
            .unwrap_or_else(|| self.fixture_path.join(".oxidex-tag-cache"))
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

    /// Format a tag value to match ExifTool's output format
    fn format_value(&self, key: &str, name: &str, value: &TagValue) -> String {
        match value {
            TagValue::String(s) => {
                // ColorMap is a large array of color values stored as space-separated string
                // ExifTool shows it as "(Binary data N bytes, use -b option to extract)"
                if name == "ColorMap" {
                    // Count entries to estimate byte size (each value is 2 bytes for SHORT)
                    let entry_count = s.split_whitespace().count();
                    if entry_count > 10 {
                        let byte_size = entry_count * 2;
                        return format!(
                            "(Binary data {} bytes, use -b option to extract)",
                            byte_size
                        );
                    }
                }

                // Copyright and similar text tags - trim whitespace and null bytes to match ExifTool
                // ExifTool trims empty copyright strings to empty
                if name == "Copyright" || name == "Artist" || name == "ImageDescription" {
                    // Trim null bytes and whitespace
                    let trimmed = s
                        .trim_end_matches('\0')
                        .trim()
                        .trim_end_matches('\0')
                        .trim();
                    if trimmed.is_empty() {
                        return String::new();
                    }
                    return trimmed.to_string();
                }

                // ExposureTime might come as a string ratio like "10/2500" - simplify to "1/250"
                if name == "ExposureTime"
                    && let Some(slash_pos) = s.find('/')
                    && let (Ok(num), Ok(den)) = (
                        s[..slash_pos].parse::<i64>(),
                        s[slash_pos + 1..].parse::<i64>(),
                    )
                    && den > 0
                    && num > 0
                {
                    // Find GCD to simplify the fraction
                    fn gcd(a: i64, b: i64) -> i64 {
                        if b == 0 { a } else { gcd(b, a % b) }
                    }
                    let g = gcd(num, den);
                    let simplified_num = num / g;
                    let simplified_den = den / g;
                    if simplified_num == 1 {
                        return format!("1/{}", simplified_den);
                    } else if simplified_den == 1 {
                        return simplified_num.to_string();
                    }
                    return format!("{}/{}", simplified_num, simplified_den);
                }

                // Try to format dates in EXIF style
                if (key.contains("Date") || key.contains("Time"))
                    && (s.contains('T') || s.contains('-'))
                {
                    return format_date_exif_style(s, false);
                }
                s.clone()
            }
            TagValue::Integer(i) => {
                // Try enum decoding for known tags
                if let Some(decoded) = self.decode_enum(name, *i as u32) {
                    return decoded;
                }
                i.to_string()
            }
            TagValue::Float(f) => {
                // ExposureTime should be formatted as a fraction (e.g., "1/250") for sub-second values
                if name == "ExposureTime" && *f > 0.0 && *f < 1.0 {
                    // Convert to fraction: find closest 1/N form
                    let denominator = (1.0 / f).round() as i64;
                    return format!("1/{}", denominator);
                }

                // Format floats with reasonable precision
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

                // Special handling for FocalLength - round to 1 decimal
                if name == "FocalLength" {
                    let value = *numerator as f64 / *denominator as f64;
                    return format!("{:.1} mm", value);
                }

                // Handle APEX (Additive System of Photographic Exposure) tags
                // These require conversion from APEX units to human-readable values
                // ApertureValue/MaxApertureValue: F-number = 2^(APEX/2)
                if name == "ApertureValue" || name == "MaxApertureValue" {
                    let apex = *numerator as f64 / *denominator as f64;
                    let f_number = (2.0_f64).powf(apex / 2.0);
                    return format!("{:.1}", f_number);
                }

                // ShutterSpeedValue: Exposure time = 2^(-APEX)
                // Format as fraction (e.g., "1/501") for times < 1 second
                if name == "ShutterSpeedValue" {
                    let apex = *numerator as f64 / *denominator as f64;
                    let exposure_time = (2.0_f64).powf(-apex);
                    // Format as fraction for sub-second exposures
                    if exposure_time < 1.0 {
                        let denominator = (1.0 / exposure_time).round() as i64;
                        return format!("1/{}", denominator);
                    } else {
                        return format!("{:.1}", exposure_time);
                    }
                }

                // ExposureTime: format as simplified fraction (e.g., "1/250") for times < 1 second
                if name == "ExposureTime" {
                    let value = *numerator as f64 / *denominator as f64;
                    if value < 1.0 && value > 0.0 {
                        // Find GCD to simplify first
                        fn gcd_i32(a: i32, b: i32) -> i32 {
                            if b == 0 { a.abs() } else { gcd_i32(b, a % b) }
                        }
                        let g = gcd_i32(*numerator, *denominator);
                        let simplified_num = numerator / g;
                        let simplified_den = denominator / g;
                        if simplified_num == 1 {
                            return format!("1/{}", simplified_den);
                        } else {
                            // Approximate to 1/N form like ExifTool does
                            let approx_denom = (1.0 / value).round() as i64;
                            return format!("1/{}", approx_denom);
                        }
                    } else if value >= 1.0 {
                        return format!("{:.1}", value);
                    }
                }

                // For tags that should be decimal values
                if is_decimal_rational_tag(key) || is_decimal_rational_tag(name) {
                    let decimal =
                        format_rational_as_decimal(*numerator as i64, *denominator as i64);
                    // Add unit if needed
                    if needs_unit_suffix(key) || needs_unit_suffix(name) {
                        return format_with_unit(name, &decimal);
                    }
                    return decimal;
                }

                // Default: compute decimal value
                // Use 9 decimal places for ExifTool compatibility
                let value = *numerator as f64 / *denominator as f64;
                let formatted = format!("{:.9}", value);
                let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');

                // Add unit suffix if needed
                if needs_unit_suffix(key) || needs_unit_suffix(name) {
                    format_with_unit(name, trimmed)
                } else {
                    trimmed.to_string()
                }
            }
            TagValue::Binary(bytes) => {
                // FileSource - single byte value indicating the source device
                // Values: 1=Film Scanner, 2=Reflection Print Scanner, 3=Digital Camera
                if name == "FileSource" && bytes.len() == 1 {
                    return match bytes[0] {
                        1 => "Film Scanner".to_string(),
                        2 => "Reflection Print Scanner".to_string(),
                        3 => "Digital Camera".to_string(),
                        _ => format!("Unknown ({})", bytes[0]),
                    };
                }

                // FlashpixVersion - 4 ASCII bytes representing version (e.g., "0100")
                if name == "FlashpixVersion"
                    && bytes.len() == 4
                    && let Ok(s) = std::str::from_utf8(bytes)
                {
                    return s.to_string();
                }

                // ExifVersion - 4 ASCII bytes representing version (e.g., "0232")
                if name == "ExifVersion"
                    && bytes.len() == 4
                    && let Ok(s) = std::str::from_utf8(bytes)
                {
                    return s.to_string();
                }

                // ComponentsConfiguration - 4 bytes indicating component order
                // Values: 0=doesn't exist, 1=Y, 2=Cb, 3=Cr, 4=R, 5=G, 6=B
                if name == "ComponentsConfiguration" && bytes.len() == 4 {
                    let components: Vec<&str> = bytes
                        .iter()
                        .map(|&b| match b {
                            0 => "-",
                            1 => "Y",
                            2 => "Cb",
                            3 => "Cr",
                            4 => "R",
                            5 => "G",
                            6 => "B",
                            _ => "?",
                        })
                        .collect();
                    return components.join(", ");
                }

                // SRATIONAL tags stored as binary (8 bytes = numerator + denominator, both i32)
                // BrightnessValue, ExposureCompensation, ShutterSpeedValue
                if (name == "BrightnessValue"
                    || name == "ExposureCompensation"
                    || name == "ShutterSpeedValue"
                    || name == "ExposureBiasValue")
                    && bytes.len() == 8
                {
                    // Try both little-endian and big-endian
                    let num_le = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    let den_le = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
                    let num_be = i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    let den_be = i32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);

                    // Use whichever gives a reasonable denominator (positive, non-zero)
                    let (num, den) = if den_le > 0 && den_le < 1_000_000 {
                        (num_le, den_le)
                    } else if den_be > 0 && den_be < 1_000_000 {
                        (num_be, den_be)
                    } else {
                        // Fallback to default binary display
                        return format!(
                            "(Binary data {} bytes, use -b option to extract)",
                            bytes.len()
                        );
                    };

                    if den != 0 {
                        // ShutterSpeedValue requires APEX conversion
                        if name == "ShutterSpeedValue" {
                            let apex = num as f64 / den as f64;
                            let exposure_time = (2.0_f64).powf(-apex);
                            if exposure_time < 1.0 {
                                let denominator = (1.0 / exposure_time).round() as i64;
                                return format!("1/{}", denominator);
                            } else {
                                return format!("{:.1}", exposure_time);
                            }
                        }

                        // Other tags: just format as decimal
                        let value = num as f64 / den as f64;
                        let formatted = format!("{:.9}", value);
                        return formatted
                            .trim_end_matches('0')
                            .trim_end_matches('.')
                            .to_string();
                    }
                }

                // UserComment - starts with 8-byte encoding identifier followed by data
                // Encoding prefixes: "ASCII\0\0\0", "UNICODE\0", "JIS\0\0\0\0\0", etc.
                if name == "UserComment" && bytes.len() > 8 {
                    let encoding = &bytes[0..8];
                    let data = &bytes[8..];

                    // Check for ASCII encoding
                    if encoding.starts_with(b"ASCII\0\0\0") {
                        return String::from_utf8_lossy(data)
                            .trim_end_matches('\0')
                            .trim()
                            .to_string();
                    }

                    // Check for Unicode encoding (UTF-16)
                    if encoding.starts_with(b"UNICODE\0") {
                        // Decode as UTF-16 little-endian
                        let u16_data: Vec<u16> = data
                            .chunks_exact(2)
                            .map(|c| u16::from_le_bytes([c[0], c[1]]))
                            .collect();
                        return String::from_utf16_lossy(&u16_data)
                            .trim_end_matches('\0')
                            .trim()
                            .to_string();
                    }

                    // Empty or null-padded data - return empty string
                    if data.iter().all(|&b| b == 0) {
                        return String::new();
                    }
                }

                // Default fallback for unrecognized binary data
                // Format to match ExifTool: "(Binary data N bytes, use -b option to extract)"
                format!(
                    "(Binary data {} bytes, use -b option to extract)",
                    bytes.len()
                )
            }
            TagValue::DateTime(dt) => {
                // Format in EXIF style: YYYY:MM:DD HH:MM:SS
                dt.format("%Y:%m:%d %H:%M:%S").to_string()
            }
            TagValue::Struct(_) => "[Structured data]".to_string(),
            TagValue::Array(arr) => {
                // ColorMap and similar large numeric arrays are shown as binary data by ExifTool
                // ColorMap is 256 entries × 3 colors × 2 bytes = 1536 bytes
                if name == "ColorMap" {
                    // Calculate the size: each value is 2 bytes (SHORT)
                    let byte_size = arr.len() * 2;
                    return format!(
                        "(Binary data {} bytes, use -b option to extract)",
                        byte_size
                    );
                }

                // Format array elements
                let parts: Vec<String> = arr
                    .iter()
                    .map(|v| self.format_value(key, name, v))
                    .collect();
                parts.join(" ")
            }
        }
    }

    /// Decode enum values for known EXIF tags
    fn decode_enum(&self, tag_name: &str, value: u32) -> Option<String> {
        // Map tag names to TIFF tag IDs for enum lookup
        let tag_id = match tag_name {
            "ColorSpace" => 0xA001,
            "MeteringMode" => 0x9207,
            "ExposureMode" => 0xA402,
            "WhiteBalance" => 0xA403,
            "SceneCaptureType" => 0xA406,
            "Contrast" => 0xA408,
            "Saturation" => 0xA409,
            "Sharpness" => 0xA40A,
            "SubjectDistanceRange" => 0xA40C,
            "SensingMethod" => 0xA217,
            "CustomRendered" => 0xA401,
            "FocalPlaneResolutionUnit" | "ResolutionUnit" => 0x0128,
            "Orientation" => 0x0112,
            "YCbCrPositioning" => 0x0213,
            "Compression" => 0x0103,
            "ExposureProgram" => 0x8822,
            "LightSource" => 0x9208,
            "Flash" => 0x9209,
            "GainControl" => 0xA407,
            "ExtraSamples" => 0x0152,
            "FillOrder" => 0x010A,
            "PlanarConfiguration" => 0x011C,
            "Predictor" => 0x013D,
            "SubfileType" => 0x00FE,
            "SceneType" => 0xA301,
            "SensitivityType" => 0x8830,
            "CompositeImage" => 0xA460,
            "MakerNoteSafety" => 0xC635,
            "PhotometricInterpretation" => 0x0106,
            _ => return None,
        };

        // Special handling for Flash tag (bitmask)
        if tag_name == "Flash" {
            return Some(oxidex::core::exif_enums::decode_flash(value));
        }

        // Use TIFF enum decoder
        tiff_enum_to_string(tag_id, value as i64)
    }

    /// Add computed Composite tags
    fn add_composite_tags(&self, tag_map: &mut HashMap<String, String>) {
        // ImageSize
        if let (Some(w), Some(h)) = (
            tag_map
                .get("EXIF:ImageWidth")
                .or(tag_map.get("File:ImageWidth")),
            tag_map
                .get("EXIF:ImageHeight")
                .or(tag_map.get("File:ImageHeight")),
        ) {
            tag_map.insert("Composite:ImageSize".to_string(), format!("{}x{}", w, h));
        }

        // Megapixels
        if let (Some(w), Some(h)) = (
            tag_map
                .get("EXIF:ImageWidth")
                .or(tag_map.get("File:ImageWidth")),
            tag_map
                .get("EXIF:ImageHeight")
                .or(tag_map.get("File:ImageHeight")),
        ) && let (Ok(width), Ok(height)) = (w.parse::<f64>(), h.parse::<f64>())
        {
            let mp = (width * height) / 1_000_000.0;
            tag_map.insert("Composite:Megapixels".to_string(), format!("{:.3}", mp));
        }

        // Aperture - copy from FNumber
        if let Some(f) = tag_map.get("EXIF:FNumber") {
            tag_map.insert("Composite:Aperture".to_string(), f.clone());
        }

        // ShutterSpeed - copy from ExposureTime
        if let Some(e) = tag_map.get("EXIF:ExposureTime") {
            tag_map.insert("Composite:ShutterSpeed".to_string(), e.clone());
        }

        // ISO
        if let Some(iso) = tag_map.get("EXIF:ISO") {
            tag_map.insert("Composite:ISO".to_string(), iso.clone());
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
        matches!(family, "File" | "System" | "UNKNOWN")
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
                // ExifIFD, IFD0, IFD1, GPS, and InteropIFD tags are output as EXIF in
                // comparison reports. Perl ExifTool outputs GPS tags as EXIF:GPSxxx,
                // and groups the thumbnail (IFD1) and Interoperability (InteropIFD)
                // sub-IFDs under the same top-level "EXIF" family by default.
                "ExifIFD" | "IFD0" | "IFD1" | "GPS" | "InteropIFD" => "EXIF",
                // Manufacturer maker notes are output as MakerNotes in comparison reports
                "Canon" | "Nikon" | "Sony" | "Fujifilm" | "Panasonic" | "Olympus" | "Pentax"
                | "Samsung" => "MakerNotes",
                // MP4/QuickTime: ItemList and UserData → QuickTime for comparison
                "ItemList" | "UserData" => "QuickTime",
                // WebP tags map to RIFF family in ExifTool
                "WebP" => "RIFF",
                // EXR tags map to OpenEXR family in ExifTool
                "EXR" => "OpenEXR",
                // Keep other families unchanged
                _ => family,
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
            // Check if original family should be skipped (pseudo-tags)
            if let Some((original_family, _)) = key.split_once(':')
                && Self::should_skip_family(original_family)
            {
                continue;
            }

            // Normalize the tag family (core library normalization + comparison-specific)
            let normalized_key = Self::normalize_for_comparison(&normalize_tag_family(key), format);

            let (family, name) = if let Some(colon_pos) = normalized_key.find(':') {
                let (fam, nam) = normalized_key.split_at(colon_pos);
                (fam.to_string(), nam[1..].to_string())
            } else {
                ("UNKNOWN".to_string(), normalized_key.clone())
            };

            // Skip if normalized family should be skipped
            if Self::should_skip_family(&family) {
                continue;
            }
            let _family = family; // Keep for later use

            // Special handling for Canon FileNumber (check original key since family is normalized)
            if name == "FileNumber" && key.starts_with("Canon:") {
                let formatted = match value {
                    TagValue::Integer(val) => {
                        let directory = (*val >> 16) & 0xFFFF;
                        let file = *val & 0xFFFF;
                        format!("{}-{}", directory, file)
                    }
                    TagValue::String(s) => {
                        if let Ok(val) = s.parse::<i64>() {
                            let directory = (val >> 16) & 0xFFFF;
                            let file = val & 0xFFFF;
                            format!("{}-{}", directory, file)
                        } else {
                            s.clone()
                        }
                    }
                    _ => continue,
                };
                Self::record_write(&mut tag_map, &mut collisions, normalized_key, formatted);
                continue;
            }

            // Format the value
            let value_str = self.format_value(&normalized_key, &name, value);
            Self::record_write(&mut tag_map, &mut collisions, normalized_key, value_str);
        }

        // Add composite tags
        self.add_composite_tags(&mut tag_map);

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
                    extensions.contains(&ext.to_lowercase().as_str())
                } else {
                    false
                }
            })
            .map(|e| e.path().to_path_buf())
            .collect();

        Ok(files)
    }

    /// Map format name to file extensions
    fn format_to_extensions(format: &str) -> Vec<&'static str> {
        match format.to_uppercase().as_str() {
            "JPEG" => vec!["jpg", "jpeg"],
            "PNG" => vec!["png"],
            "TIFF" => vec!["tif", "tiff"],
            "GIF" => vec!["gif"],
            "WEBP" => vec!["webp"],
            "HEIC" => vec!["heic", "heif"],
            "MP4" => vec!["mp4", "m4v", "mov"],
            "AVI" => vec!["avi"],
            "MKV" => vec!["mkv"],
            "MP3" => vec!["mp3"],
            "WAV" => vec!["wav"],
            "PDF" => vec!["pdf"],
            "PSD" => vec!["psd"],
            "CR2" => vec!["cr2", "cr3"],
            "NEF" => vec!["nef"],
            "ARW" => vec!["arw"],
            "DNG" => vec!["dng"],
            "RAF" => vec!["raf"],
            "ORF" => vec!["orf"],
            "RW2" => vec!["rw2"],
            "XMP" => vec!["xmp"],
            "FLAC" => vec!["flac"],
            "OGG" => vec!["ogg", "oga", "ogv"],
            "BMP" => vec!["bmp"],
            "ICO" => vec!["ico"],
            "SVG" => vec!["svg"],
            "EPS" => vec!["eps", "ps"],
            "FLIF" => vec!["flif"],
            "EXR" => vec!["exr"],
            "JXL" => vec!["jxl"],
            "AVIF" => vec!["avif"],
            "3GP" => vec!["3gp", "3g2"],
            "M2TS" => vec!["mts", "m2ts", "ts"],
            "M4A" => vec!["m4a"],
            "FLV" => vec!["flv"],
            "WMV" => vec!["wmv", "asf"],
            "MXF" => vec!["mxf"],
            "WEBM" => vec!["webm"],
            "ICC" => vec!["icc", "icm"],
            "PEF" => vec!["pef"],
            "SRW" => vec!["srw"],
            "X3F" => vec!["x3f"],
            "DCR" => vec!["dcr"],
            "RWL" => vec!["rwl"],
            "3FR" => vec!["3fr"],
            "FFF" => vec!["fff"],
            "MEF" => vec!["mef"],
            "MOS" => vec!["mos"],
            "MRW" => vec!["mrw"],
            "NRW" => vec!["nrw"],
            "SR2" => vec!["sr2", "srf"],
            "KDC" => vec!["kdc"],
            "ERF" => vec!["erf"],
            "BPG" => vec!["bpg"],
            "AAC" => vec!["aac"],
            "APE" => vec!["ape"],
            "OPUS" => vec!["opus"],
            "AIFF" => vec!["aif", "aiff"],
            "HDR" => vec!["hdr"],
            "PPM" => vec!["ppm", "pgm", "pbm", "pnm"],
            "MPC" => vec!["mpc"],
            "RAW" => vec![
                "raw", "3fr", "ari", "bay", "crw", "dcr", "dcs", "dng", "erf", "fff", "k25", "kdc",
                "mef", "mos", "mrw", "nrw", "pef", "ptx", "r3d", "raf", "rw2", "rwl", "sr2", "srf",
                "srw", "x3f",
            ],
            "PE" => vec!["exe", "dll", "sys"],
            "ELF" => vec!["elf", "so"],
            "MACHO" => vec!["dylib", "bundle", "macho"],
            "OTF" => vec!["otf"],
            "TTF" => vec!["ttf"],
            "WOFF" => vec!["woff"],
            "WOFF2" => vec!["woff2"],
            "DOCX" => vec!["docx"],
            "XLSX" => vec!["xlsx"],
            "PPTX" => vec!["pptx"],
            "ZIP" => vec!["zip"],
            "RAR" => vec!["rar"],
            "7Z" => vec!["7z"],
            "GZIP" => vec!["gz"],
            "TAR" => vec!["tar"],
            "ISO" => vec!["iso"],
            "OLE" => vec!["doc", "xls", "ppt", "msg", "vsd", "pub"],
            _ => vec![],
        }
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
    fn test_flatten_metadata_empty() {
        let extractor = OxiDexExtractor::new(PathBuf::from("tests/fixtures"));
        let metadata = oxidex::core::MetadataMap::new();
        let (tags, collisions) = extractor.flatten_metadata(&metadata, None);
        assert_eq!(tags.len(), 0);
        assert!(collisions.is_empty());
    }

    #[test]
    fn test_canon_file_number_formatting() {
        let extractor = OxiDexExtractor::new(PathBuf::from("tests/fixtures"));
        let mut metadata = oxidex::core::MetadataMap::new();
        metadata.insert("Canon:FileNumber".to_string(), TagValue::Integer(7669483));
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
