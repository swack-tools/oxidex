//! ExifTool tag extractor - Extract tags by running exiftool -json on fixtures
//!
//! OPTIMIZED: Uses batch mode to process multiple files at once (much faster than
//! spawning exiftool for each file individually).

use super::ExtractionResult;
use crate::models::TagInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use walkdir::WalkDir;

/// On-disk cache entry for one format's ExifTool extraction. ExifTool's own
/// output for a given sample corpus never changes round-to-round (only
/// OxiDex's binary changes as fixes land), so this persists across process
/// invocations -- unlike ExifToolExtractor's in-memory `cache` field, which
/// is rebuilt from scratch every time main.rs constructs a fresh extractor
/// (once per format, every single comparison run). Invalidated by either an
/// ExifTool version change or the sample corpus itself changing (tracked
/// via `signature`, a hash of every matched file's path/size/mtime).
#[derive(Debug, Serialize, Deserialize)]
struct DiskCacheEntry {
    exiftool_version: String,
    signature: String,
    result: ExtractionResult,
}

/// Batch size for exiftool invocations
/// ExifTool handles batches efficiently, but we limit batch size to avoid
/// command line length limits on some systems
const BATCH_SIZE: usize = 100;

/// Extract tags from ExifTool by running exiftool CLI
pub struct ExifToolExtractor {
    exiftool_path: String,
    cache: HashMap<String, ExtractionResult>,
}

impl ExifToolExtractor {
    /// Create a new ExifTool extractor
    pub fn new(exiftool_path: String) -> Self {
        Self {
            exiftool_path,
            cache: HashMap::new(),
        }
    }

    /// Extract tags from all fixtures of a specific format
    ///
    /// # Arguments
    /// * `format` - Format name (e.g., "JPEG", "PNG")
    ///
    /// # Returns
    /// ExtractionResult with tags and file count
    ///
    /// OPTIMIZED: Uses batch mode to process multiple files per exiftool invocation
    pub async fn extract_format_tags(
        &mut self,
        format: &str,
        fixture_path: &Path,
    ) -> Result<ExtractionResult, Box<dyn std::error::Error>> {
        // Check in-memory cache first (survives within this one process,
        // e.g. a repeat call for the same format within a single run)
        if let Some(cached) = self.cache.get(format) {
            return Ok(cached.clone());
        }

        // Find files by extension recursively throughout the samples directory
        let files: Vec<PathBuf> = Self::find_files_by_extension(fixture_path, format)?;

        let files_processed = files.len();

        if files.is_empty() {
            return Ok(ExtractionResult {
                tags: Vec::new(),
                files_processed: 0,
            });
        }

        // Check the on-disk cache next -- ExifTool's own output for this
        // sample corpus never changes between rounds of a fix-loop, only
        // OxiDex's binary does, so this is the expensive part actually
        // worth persisting across process invocations.
        let signature = Self::compute_signature(&files);
        let exiftool_version = self.get_exiftool_version();
        if let Some(cached) =
            self.load_disk_cache(fixture_path, format, &exiftool_version, &signature)
        {
            self.cache.insert(format.to_string(), cached.clone());
            return Ok(cached);
        }

        // OPTIMIZATION: Process files in batches using exiftool's batch mode
        // This is MUCH faster than spawning exiftool for each file individually
        let mut all_tags: HashMap<String, (TagInfo, usize)> = HashMap::new();

        // Process in batches
        for batch in files.chunks(BATCH_SIZE) {
            match self.run_exiftool_batch(batch) {
                Ok(batch_results) => {
                    for file_tags in batch_results {
                        for tag_info in file_tags {
                            all_tags
                                .entry(format!("{}:{}", tag_info.family, tag_info.name))
                                .and_modify(|(_info, count)| *count += 1)
                                .or_insert((tag_info.clone(), 1));
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Batch extraction failed: {}", e);
                    // Fall back to individual file processing for this batch
                    for file_path in batch {
                        if let Ok(file_tags) = self.run_exiftool_on_file(file_path) {
                            for tag_info in file_tags {
                                all_tags
                                    .entry(format!("{}:{}", tag_info.family, tag_info.name))
                                    .and_modify(|(_info, count)| *count += 1)
                                    .or_insert((tag_info.clone(), 1));
                            }
                        }
                    }
                }
            }
        }

        // Convert to final format
        let mut tags: Vec<TagInfo> = all_tags
            .into_values()
            .map(|(tag_info, _count)| tag_info)
            .collect();

        // Sort by key for consistency
        tags.sort_by_key(|a| a.key());

        let result = ExtractionResult {
            tags,
            files_processed,
        };

        // Cache the result in memory and on disk
        self.cache.insert(format.to_string(), result.clone());
        self.save_disk_cache(fixture_path, format, &exiftool_version, &signature, &result);

        Ok(result)
    }

    /// Directory the on-disk cache lives in: a sibling of the samples dir
    /// itself (fixture_path is e.g. `<cache_dir>/combined-samples`, so this
    /// resolves to `<cache_dir>/exiftool-tag-cache`), keeping it alongside
    /// the rest of the ExifTool cache machinery rather than inside the
    /// samples tree.
    fn disk_cache_dir(fixture_path: &Path) -> PathBuf {
        fixture_path
            .parent()
            .map(|p| p.join("exiftool-tag-cache"))
            .unwrap_or_else(|| fixture_path.join(".exiftool-tag-cache"))
    }

    fn disk_cache_path(fixture_path: &Path, format: &str) -> PathBuf {
        Self::disk_cache_dir(fixture_path).join(format!("{}.json", format.to_lowercase()))
    }

    /// Cheap signature of the exact sample set this format's cache entry
    /// covers -- path, size, and mtime per file, hashed together. Any
    /// change to the corpus (a sample added/removed/modified) changes this,
    /// which invalidates the cache without needing to re-run ExifTool just
    /// to find out.
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

    /// Runs `<exiftool> -ver`. Falls back to "unknown" on failure (rather
    /// than erroring out) -- a version we can't determine still invalidates
    /// any stale disk cache safely, since "unknown" simply never matches a
    /// real version string recorded by a prior successful run.
    fn get_exiftool_version(&self) -> String {
        match Command::new(&self.exiftool_path).arg("-ver").output() {
            Ok(o) if o.status.success() => String::from_utf8(o.stdout)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "unknown".to_string()),
            Ok(o) => {
                eprintln!(
                    "Warning: `{} -ver` exited non-zero ({}); ExifTool disk cache disabled this run",
                    self.exiftool_path, o.status
                );
                "unknown".to_string()
            }
            Err(e) => {
                eprintln!(
                    "Warning: failed to run `{} -ver` ({e}); ExifTool disk cache disabled this run",
                    self.exiftool_path
                );
                "unknown".to_string()
            }
        }
    }

    fn load_disk_cache(
        &self,
        fixture_path: &Path,
        format: &str,
        exiftool_version: &str,
        signature: &str,
    ) -> Option<ExtractionResult> {
        let path = Self::disk_cache_path(fixture_path, format);
        let content = std::fs::read_to_string(path).ok()?;
        let entry: DiskCacheEntry = serde_json::from_str(&content).ok()?;
        if entry.exiftool_version == exiftool_version && entry.signature == signature {
            Some(entry.result)
        } else {
            None
        }
    }

    /// Best-effort -- a failure to persist the cache (e.g. read-only
    /// filesystem) must never fail the extraction itself, since the result
    /// was already computed correctly; it just means next round pays the
    /// same ExifTool cost again. Also refuses to write when exiftool_version
    /// is "unknown" (get_exiftool_version's failure sentinel) -- writing
    /// under that key would clobber a previously good cache entry with one
    /// that can never validate against a future successful run.
    fn save_disk_cache(
        &self,
        fixture_path: &Path,
        format: &str,
        exiftool_version: &str,
        signature: &str,
        result: &ExtractionResult,
    ) {
        if exiftool_version == "unknown" {
            return;
        }
        let dir = Self::disk_cache_dir(fixture_path);
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let entry = DiskCacheEntry {
            exiftool_version: exiftool_version.to_string(),
            signature: signature.to_string(),
            result: result.clone(),
        };
        if let Ok(json) = serde_json::to_string(&entry) {
            let _ = std::fs::write(Self::disk_cache_path(fixture_path, format), json);
        }
    }

    /// Run exiftool on multiple files at once (batch mode)
    /// Returns a Vec of tag results, one per file
    fn run_exiftool_batch(
        &self,
        files: &[PathBuf],
    ) -> Result<Vec<Vec<TagInfo>>, Box<dyn std::error::Error>> {
        if files.is_empty() {
            return Ok(vec![]);
        }

        // Use -@ to read filenames from stdin (avoids command line length limits)
        let mut child = Command::new(&self.exiftool_path)
            .arg("-json")
            .arg("-G") // Include group name prefix (e.g., "EXIF:Make")
            .arg("-@")
            .arg("-") // Read filenames from stdin
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Write filenames to stdin
        if let Some(mut stdin) = child.stdin.take() {
            for file in files {
                writeln!(stdin, "{}", file.display())?;
            }
        }

        let output = child.wait_with_output()?;

        if !output.status.success() {
            // Non-zero exit is common when some files fail - check if we got any output
            if output.stdout.is_empty() {
                return Err(format!(
                    "ExifTool failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                )
                .into());
            }
            // We have output despite errors, continue parsing
        }

        let stdout = String::from_utf8(output.stdout)?;
        if stdout.trim().is_empty() {
            return Ok(vec![]);
        }

        let json: serde_json::Value = serde_json::from_str(&stdout)?;
        let results = self.parse_exiftool_batch_json(&json);

        Ok(results)
    }

    /// Run exiftool on a single file and parse JSON output (fallback)
    fn run_exiftool_on_file(
        &self,
        file_path: &Path,
    ) -> Result<Vec<TagInfo>, Box<dyn std::error::Error>> {
        let output = Command::new(&self.exiftool_path)
            .arg("-json")
            .arg("-G") // Include group name prefix (e.g., "EXIF:Make")
            .arg(file_path)
            .output()?;

        if !output.status.success() {
            return Err(format!(
                "ExifTool failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }

        let stdout = String::from_utf8(output.stdout)?;
        let json: serde_json::Value = serde_json::from_str(&stdout)?;
        let tags = self.parse_exiftool_json(&json);

        Ok(tags)
    }

    /// Parse batch JSON output from ExifTool (array of file results)
    fn parse_exiftool_batch_json(&self, json: &serde_json::Value) -> Vec<Vec<TagInfo>> {
        let mut results = Vec::new();

        if let Some(array) = json.as_array() {
            for file_data in array {
                let tags = self.parse_single_file_json(file_data);
                results.push(tags);
            }
        }

        results
    }

    /// Check if a tag family should be skipped in comparison
    /// These are pseudo-tags computed by ExifTool, not actual extracted metadata
    fn should_skip_family(family: &str) -> bool {
        matches!(
            family,
            // Composite tags are calculated/derived from other tags
            "Composite"
            // ExifTool version info
            | "ExifTool"
            // File system metadata (varies by environment)
            | "System"
            | "File"
        )
    }

    /// Parse a single file's JSON data into TagInfo vector
    fn parse_single_file_json(&self, file_data: &serde_json::Value) -> Vec<TagInfo> {
        let mut tags = Vec::new();

        if let Some(obj) = file_data.as_object() {
            // ExifTool's own JSON always includes this per entry regardless
            // of -G grouping -- reading it directly here is far more
            // robust than trying to zip batch results back up against the
            // input file list positionally (which breaks the moment
            // ExifTool skips or reorders an entry for a failed file).
            let source_file = obj
                .get("SourceFile")
                .and_then(|v| v.as_str())
                .map(str::to_string);

            for (key, value) in obj.iter() {
                let (family, name) = self.parse_tag_name(key);
                // Skip pseudo-tags and computed values
                if family != "UNKNOWN" && !Self::should_skip_family(&family) {
                    let value_str = match value {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Array(_) => value.to_string(),
                        serde_json::Value::Object(_) => value.to_string(),
                        serde_json::Value::Null => "null".to_string(),
                    };
                    let mut tag_info = TagInfo::new(name, family, value_str);
                    if let Some(sf) = &source_file {
                        tag_info = tag_info.with_source_file(sf.clone());
                    }
                    tags.push(tag_info);
                }
            }
        }

        tags
    }

    /// Parse ExifTool JSON output into TagInfo (for single-file output)
    fn parse_exiftool_json(&self, json: &serde_json::Value) -> Vec<TagInfo> {
        // ExifTool returns an array of objects, one per file
        if let Some(array) = json.as_array()
            && let Some(file_data) = array.first()
        {
            return self.parse_single_file_json(file_data);
        }

        Vec::new()
    }

    /// Parse tag name to extract family and tag name
    /// "EXIF:Make" → ("EXIF", "Make")
    /// "ExifTool:Version" → ("ExifTool", "Version")
    fn parse_tag_name(&self, exiftool_name: &str) -> (String, String) {
        if let Some(colon_pos) = exiftool_name.find(':') {
            let (family, name) = exiftool_name.split_at(colon_pos);
            (family.to_string(), name[1..].to_string()) // Skip the ':'
        } else {
            ("UNKNOWN".to_string(), exiftool_name.to_string())
        }
    }

    /// Find files by extension recursively throughout the samples directory
    fn find_files_by_extension(
        fixture_path: &Path,
        format: &str,
    ) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        let extensions = Self::format_to_extensions(format);
        if extensions.is_empty() {
            return Ok(Vec::new());
        }

        let files: Vec<PathBuf> = WalkDir::new(fixture_path)
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
            "EXR" => vec!["exr"],
            "JXL" => vec!["jxl"],
            "AVIF" => vec!["avif"],
            "3GP" => vec!["3gp", "3g2"],
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
    fn test_exiftool_extractor_creation() {
        let extractor = ExifToolExtractor::new("exiftool".to_string());
        assert_eq!(extractor.exiftool_path, "exiftool");
    }

    #[test]
    fn test_parse_tag_name_with_colon() {
        let extractor = ExifToolExtractor::new("exiftool".to_string());
        let (family, name) = extractor.parse_tag_name("EXIF:Make");
        assert_eq!(family, "EXIF");
        assert_eq!(name, "Make");
    }

    #[test]
    fn test_parse_tag_name_without_colon() {
        let extractor = ExifToolExtractor::new("exiftool".to_string());
        let (family, name) = extractor.parse_tag_name("SourceFile");
        assert_eq!(family, "UNKNOWN");
        assert_eq!(name, "SourceFile");
    }

    #[test]
    fn test_parse_tag_name_xmp() {
        let extractor = ExifToolExtractor::new("exiftool".to_string());
        let (family, name) = extractor.parse_tag_name("XMP:Creator");
        assert_eq!(family, "XMP");
        assert_eq!(name, "Creator");
    }

    #[test]
    fn test_parse_exiftool_json_empty() {
        let extractor = ExifToolExtractor::new("exiftool".to_string());
        let json = serde_json::json!([]);
        let tags = extractor.parse_exiftool_json(&json);
        assert_eq!(tags.len(), 0);
    }

    #[test]
    fn test_parse_exiftool_json_with_data() {
        let extractor = ExifToolExtractor::new("exiftool".to_string());
        let json = serde_json::json!([{
            "EXIF:Make": "Canon",
            "EXIF:Model": "Canon EOS 5D",
            "XMP:Creator": "John Doe"
        }]);
        let tags = extractor.parse_exiftool_json(&json);
        assert_eq!(tags.len(), 3);
        assert!(tags.iter().any(|t| t.name == "Make" && t.family == "EXIF"));
        assert!(
            tags.iter()
                .any(|t| t.name == "Creator" && t.family == "XMP")
        );
    }

    #[test]
    fn test_parse_single_file_json_populates_source_file_from_exiftool_own_field() {
        let extractor = ExifToolExtractor::new("exiftool".to_string());
        let json = serde_json::json!({
            "SourceFile": "/samples/JPEG/Sony/camera.jpg",
            "EXIF:Make": "Sony",
        });
        let tags = extractor.parse_single_file_json(&json);
        assert_eq!(tags.len(), 1);
        assert_eq!(
            tags[0].source_file,
            Some("/samples/JPEG/Sony/camera.jpg".to_string())
        );
    }

    #[test]
    fn test_parse_single_file_json_source_file_none_when_absent() {
        let extractor = ExifToolExtractor::new("exiftool".to_string());
        let json = serde_json::json!({"EXIF:Make": "Sony"});
        let tags = extractor.parse_single_file_json(&json);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].source_file, None);
    }
}
