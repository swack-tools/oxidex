//! ExifTool tag extractor - Extract tags by running exiftool -json on fixtures
//!
//! OPTIMIZED: Uses batch mode to process multiple files at once (much faster than
//! spawning exiftool for each file individually).

use super::ExtractionResult;
use crate::models::TagInfo;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use walkdir::WalkDir;

/// One file's entry from `exiftool -json`, with every value left as the
/// exact source text ExifTool wrote. `BTreeMap` matches the key ordering
/// `serde_json::Map` gave before (serde_json's default Map is a BTreeMap),
/// so nothing downstream sees a different iteration order.
///
/// The point of `RawValue` here is numbers -- see `exiftool_value_text`.
type RawFileEntry = BTreeMap<String, Box<RawValue>>;

/// Bumped whenever this extractor changes how it renders ExifTool's JSON
/// into the comparison's value text. A cache written under an older
/// revision holds strings this code would no longer produce (revision 1
/// stored "0.0" where ExifTool printed "0.00"), and re-serving them would
/// make a rendering fix look like it had no effect until someone
/// remembered to `rm -rf` the cache directory by hand.
const VALUE_RENDERING_REVISION: u32 = 2;

/// On-disk cache entry for one format's ExifTool extraction. ExifTool's own
/// output for a given sample corpus never changes round-to-round (only
/// OxiDex's binary changes as fixes land), so this persists across process
/// invocations -- unlike ExifToolExtractor's in-memory `cache` field, which
/// is rebuilt from scratch every time main.rs constructs a fresh extractor
/// (once per format, every single comparison run). Invalidated by an
/// ExifTool version change, the sample corpus itself changing (tracked via
/// `signature`, a hash of every matched file's path/size/mtime), or a bump
/// of `VALUE_RENDERING_REVISION`.
#[derive(Debug, Serialize, Deserialize)]
struct DiskCacheEntry {
    exiftool_version: String,
    signature: String,
    /// Absent in caches written before this field existed, which
    /// `#[serde(default)]` reads as 0 -- never equal to a real revision,
    /// so those entries are correctly treated as stale.
    #[serde(default)]
    rendering_revision: u32,
    result: ExtractionResult,
}

/// Render one value from ExifTool's `-json` output as the text ExifTool
/// itself printed for that tag.
///
/// The number branch is the entire reason this reads `RawValue` instead of
/// `serde_json::Value`. ExifTool does not *serialize* numbers -- it emits
/// the PrintConv result verbatim, unquoted, whenever that result happens to
/// look like a JSON number. From the exiftool script's `EscapeJSON`
/// (13.59, line 3810):
///
/// ```text
/// return $str if $str =~ /^-?(\d|[1-9]\d{1,14})(\.\d{1,16})?(e[-+]?\d{1,3})?$/i;
/// ```
///
/// So a PrintConv that deliberately produced `0.00` reaches us as the bare
/// token `0.00`, and that token *is* ExifTool's human-readable output --
/// `exiftool -G1 -s` prints exactly `RawBrightnessAdj : 0.00`. Parsing it
/// through `f64` and re-rendering (what this extractor did through
/// rendering revision 1) yielded "0.0", a string ExifTool never printed,
/// and the harness then reported OxiDex's correct "0.00" as a value
/// difference. Trailing zeros, exponent spelling and any precision past
/// f64's shortest round-trip form were all destroyed the same way -- and
/// in the other direction two genuinely different values could collapse
/// onto one `f64` string and silently *match*.
///
/// Strings, booleans, nulls, arrays and objects keep byte-for-byte the
/// rendering revision 1 produced, so this changes nothing except the one
/// case that was wrong.
fn exiftool_value_text(raw: &RawValue) -> String {
    let text = raw.get().trim();
    // JSON grammar: a value whose first byte is '-' or a digit can only be
    // a number, so this needs no lookahead and cannot misfire on a string
    // (a string always starts with '"').
    if matches!(text.as_bytes().first(), Some(b'-' | b'0'..=b'9')) {
        return text.to_string();
    }
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(serde_json::Value::Bool(b)) => b.to_string(),
        Ok(serde_json::Value::Null) => "null".to_string(),
        // Arrays/objects: compact JSON, which is what
        // `normalize_value_for_comparison`'s `["a","b"]` handling expects.
        Ok(other) => other.to_string(),
        Err(_) => text.to_string(),
    }
}

/// Batch size for exiftool invocations
/// ExifTool handles batches efficiently, but we limit batch size to avoid
/// command line length limits on some systems
const BATCH_SIZE: usize = 100;

/// Extract tags from ExifTool by running exiftool CLI
pub struct ExifToolExtractor {
    /// Argv prefix from `oxidex::exiftool_oracle`: program plus any leading
    /// arguments. It is a vector, not a path, because the pinned oracle runs
    /// as `<perl> -I<tree>/lib <tree>/exiftool` -- the tree's own
    /// `#!/usr/bin/env perl` cannot be trusted to find a perl with
    /// `Archive::Zip`, and without that module every ZIP-container format
    /// silently degrades.
    exiftool_argv: Vec<String>,
    cache: HashMap<String, ExtractionResult>,
    cache_dir_override: Option<PathBuf>,
}

impl ExifToolExtractor {
    /// Create a new ExifTool extractor from an oracle argv prefix.
    pub fn new(exiftool_argv: Vec<String>) -> Self {
        Self {
            exiftool_argv,
            cache: HashMap::new(),
            cache_dir_override: None,
        }
    }

    /// A `Command` preloaded with the oracle's argv prefix.
    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.exiftool_argv[0]);
        cmd.args(&self.exiftool_argv[1..]);
        cmd
    }

    /// The invocation as one string, for messages.
    fn invocation(&self) -> String {
        self.exiftool_argv.join(" ")
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
    /// # Arguments
    /// * `format` - Format name (e.g., "JPEG", "PNG")
    ///
    /// # Returns
    /// ExtractionResult with tags and file count
    ///
    /// OPTIMIZED: Uses batch mode to process multiple files per exiftool invocation
    ///
    /// Plain `fn`, not `async fn`: every operation in this body (subprocess
    /// spawn/wait, `std::fs` reads/writes) is already blocking, so the
    /// `async` this signature carried until 2026-08-08 never actually
    /// yielded -- `main`'s single `.await` per call ran it to completion
    /// inline, same as a direct call. Synchronous throughout lets the
    /// per-format loop in `main.rs` parallelize with `rayon` (each format's
    /// extraction runs on its own OS thread) without dragging a second
    /// (unused) concurrency model -- tokio -- along for the ride.
    pub fn extract_format_tags(
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
                // ExifTool is the reference implementation; a duplicate
                // emission is only ever an oxidex-side defect.
                duplicate_emissions: Vec::new(),
                all_instances: HashMap::new(),
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
        let mut all_instances: HashMap<String, Vec<TagInfo>> = HashMap::new();

        // Process in batches
        for batch in files.chunks(BATCH_SIZE) {
            match self.run_exiftool_batch(batch) {
                Ok(batch_results) => {
                    for file_tags in batch_results {
                        for tag_info in file_tags {
                            let key = format!("{}:{}", tag_info.family, tag_info.name);
                            all_tags
                                .entry(key.clone())
                                .and_modify(|(_info, count)| *count += 1)
                                .or_insert_with(|| (tag_info.clone(), 1));
                            all_instances.entry(key).or_default().push(tag_info);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Batch extraction failed: {}", e);
                    // Fall back to individual file processing for this batch
                    for file_path in batch {
                        if let Ok(file_tags) = self.run_exiftool_on_file(file_path) {
                            for tag_info in file_tags {
                                let key = format!("{}:{}", tag_info.family, tag_info.name);
                                all_tags
                                    .entry(key.clone())
                                    .and_modify(|(_info, count)| *count += 1)
                                    .or_insert_with(|| (tag_info.clone(), 1));
                                all_instances.entry(key).or_default().push(tag_info);
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
            // ExifTool is the reference implementation; a duplicate
            // emission is only ever an oxidex-side defect.
            duplicate_emissions: Vec::new(),
            all_instances,
        };

        // Cache the result in memory and on disk
        self.cache.insert(format.to_string(), result.clone());
        self.save_disk_cache(fixture_path, format, &exiftool_version, &signature, &result);

        Ok(result)
    }

    /// Directory the on-disk cache lives in. See
    /// `cache_dir::resolve_cache_dir` -- this is deliberately independent of
    /// `fixture_path`'s parent, which used to be the samples corpus itself
    /// whenever `fixture_path` was pointed at a vendor subdirectory (e.g.
    /// `combined-samples/Olympus`), writing the cache inside the read-only
    /// corpus.
    fn disk_cache_dir(&self, fixture_path: &Path) -> PathBuf {
        super::cache_dir::resolve_cache_dir(
            fixture_path,
            "exiftool-tag-cache",
            self.cache_dir_override.as_deref(),
        )
    }

    fn disk_cache_path(&self, fixture_path: &Path, format: &str) -> PathBuf {
        self.disk_cache_dir(fixture_path)
            .join(format!("{}.json", format.to_lowercase()))
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
        match self.command().arg("-ver").output() {
            Ok(o) if o.status.success() => String::from_utf8(o.stdout)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "unknown".to_string()),
            Ok(o) => {
                eprintln!(
                    "Warning: `{} -ver` exited non-zero ({}); ExifTool disk cache disabled this run",
                    self.invocation(),
                    o.status
                );
                "unknown".to_string()
            }
            Err(e) => {
                eprintln!(
                    "Warning: failed to run `{} -ver` ({e}); ExifTool disk cache disabled this run",
                    self.invocation()
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
        let path = self.disk_cache_path(fixture_path, format);
        let content = std::fs::read_to_string(path).ok()?;
        let entry: DiskCacheEntry = serde_json::from_str(&content).ok()?;
        if entry.exiftool_version == exiftool_version
            && entry.signature == signature
            && entry.rendering_revision == VALUE_RENDERING_REVISION
        {
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
        let dir = self.disk_cache_dir(fixture_path);
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let entry = DiskCacheEntry {
            exiftool_version: exiftool_version.to_string(),
            signature: signature.to_string(),
            rendering_revision: VALUE_RENDERING_REVISION,
            result: result.clone(),
        };
        if let Ok(json) = serde_json::to_string(&entry) {
            let _ = std::fs::write(self.disk_cache_path(fixture_path, format), json);
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
        let mut child = self
            .command()
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

        Ok(self.parse_exiftool_batch_json(&stdout)?)
    }

    /// Run exiftool on a single file and parse JSON output (fallback)
    fn run_exiftool_on_file(
        &self,
        file_path: &Path,
    ) -> Result<Vec<TagInfo>, Box<dyn std::error::Error>> {
        let output = self
            .command()
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
        let tags = self.parse_exiftool_json(&stdout)?;

        Ok(tags)
    }

    /// Parse batch JSON output from ExifTool (array of file results)
    fn parse_exiftool_batch_json(
        &self,
        stdout: &str,
    ) -> Result<Vec<Vec<TagInfo>>, serde_json::Error> {
        let entries: Vec<RawFileEntry> = serde_json::from_str(stdout)?;
        Ok(entries
            .iter()
            .map(|entry| self.parse_single_file_json(entry))
            .collect())
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
    fn parse_single_file_json(&self, file_data: &RawFileEntry) -> Vec<TagInfo> {
        let mut tags = Vec::new();

        // ExifTool's own JSON always includes this per entry regardless
        // of -G grouping -- reading it directly here is far more
        // robust than trying to zip batch results back up against the
        // input file list positionally (which breaks the moment
        // ExifTool skips or reorders an entry for a failed file).
        let source_file = file_data
            .get("SourceFile")
            .map(|raw| exiftool_value_text(raw));

        for (key, value) in file_data.iter() {
            let (family, name) = self.parse_tag_name(key);
            // Skip pseudo-tags and computed values
            if family != "UNKNOWN" && !Self::should_skip_family(&family) {
                let mut tag_info = TagInfo::new(name, family, exiftool_value_text(value));
                if let Some(sf) = &source_file {
                    tag_info = tag_info.with_source_file(sf.clone());
                }
                tags.push(tag_info);
            }
        }

        tags
    }

    /// Parse ExifTool JSON output into TagInfo (for single-file output)
    fn parse_exiftool_json(&self, stdout: &str) -> Result<Vec<TagInfo>, serde_json::Error> {
        // ExifTool returns an array of objects, one per file
        let entries: Vec<RawFileEntry> = serde_json::from_str(stdout)?;
        Ok(entries
            .first()
            .map(|entry| self.parse_single_file_json(entry))
            .unwrap_or_default())
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

        let extractor = ExifToolExtractor::new(vec!["exiftool".to_string()]);
        let cache_dir = extractor.disk_cache_dir(&vendor_subdir);

        assert!(
            !cache_dir.starts_with(&observable_parent),
            "cache dir {} must not be written inside the corpus at {}",
            cache_dir.display(),
            observable_parent.display()
        );
    }

    #[test]
    fn test_exiftool_extractor_creation() {
        let extractor = ExifToolExtractor::new(vec!["exiftool".to_string()]);
        assert_eq!(extractor.exiftool_argv, vec!["exiftool".to_string()]);
    }

    #[test]
    fn test_fpf_format_discovers_standalone_fpf_files() {
        assert_eq!(ExifToolExtractor::format_to_extensions("FPF"), vec!["fpf"]);
    }

    /// An unmapped format must still find its files. This returned `vec![]`
    /// before, so a format `detect_formats` had grouped under its bare
    /// extension matched nothing and dropped out of the run without a word --
    /// 83 of the 194 files in ExifTool's t/images, the hard formats first.
    #[test]
    fn unmapped_format_falls_back_to_its_own_extension() {
        for format in ["FITS", "DICOM", "MIE", "CRW", "R3D"] {
            assert_eq!(
                ExifToolExtractor::format_to_extensions(format),
                vec![format.to_lowercase()],
                "{format} must resolve to .{} and not an empty list",
                format.to_lowercase(),
            );
        }
    }

    #[test]
    fn test_parse_tag_name_with_colon() {
        let extractor = ExifToolExtractor::new(vec!["exiftool".to_string()]);
        let (family, name) = extractor.parse_tag_name("EXIF:Make");
        assert_eq!(family, "EXIF");
        assert_eq!(name, "Make");
    }

    #[test]
    fn test_parse_tag_name_without_colon() {
        let extractor = ExifToolExtractor::new(vec!["exiftool".to_string()]);
        let (family, name) = extractor.parse_tag_name("SourceFile");
        assert_eq!(family, "UNKNOWN");
        assert_eq!(name, "SourceFile");
    }

    #[test]
    fn test_parse_tag_name_xmp() {
        let extractor = ExifToolExtractor::new(vec!["exiftool".to_string()]);
        let (family, name) = extractor.parse_tag_name("XMP:Creator");
        assert_eq!(family, "XMP");
        assert_eq!(name, "Creator");
    }

    fn entry(json: &str) -> RawFileEntry {
        serde_json::from_str(json).expect("test fixture must be valid JSON")
    }

    fn value_of(tags: &[TagInfo], name: &str) -> String {
        tags.iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("tag {name} not extracted"))
            .value
            .clone()
    }

    #[test]
    fn test_parse_exiftool_json_empty() {
        let extractor = ExifToolExtractor::new(vec!["exiftool".to_string()]);
        let tags = extractor.parse_exiftool_json("[]").unwrap();
        assert_eq!(tags.len(), 0);
    }

    #[test]
    fn test_parse_exiftool_json_with_data() {
        let extractor = ExifToolExtractor::new(vec!["exiftool".to_string()]);
        let tags = extractor
            .parse_exiftool_json(
                r#"[{
                    "EXIF:Make": "Canon",
                    "EXIF:Model": "Canon EOS 5D",
                    "XMP:Creator": "John Doe"
                }]"#,
            )
            .unwrap();
        assert_eq!(tags.len(), 3);
        assert!(tags.iter().any(|t| t.name == "Make" && t.family == "EXIF"));
        assert!(
            tags.iter()
                .any(|t| t.name == "Creator" && t.family == "XMP")
        );
    }

    #[test]
    fn test_parse_single_file_json_populates_source_file_from_exiftool_own_field() {
        let extractor = ExifToolExtractor::new(vec!["exiftool".to_string()]);
        let tags = extractor.parse_single_file_json(&entry(
            r#"{
                "SourceFile": "/samples/JPEG/Sony/camera.jpg",
                "EXIF:Make": "Sony"
            }"#,
        ));
        assert_eq!(tags.len(), 1);
        assert_eq!(
            tags[0].source_file,
            Some("/samples/JPEG/Sony/camera.jpg".to_string())
        );
    }

    #[test]
    fn test_parse_single_file_json_source_file_none_when_absent() {
        let extractor = ExifToolExtractor::new(vec!["exiftool".to_string()]);
        let tags = extractor.parse_single_file_json(&entry(r#"{"EXIF:Make": "Sony"}"#));
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].source_file, None);
    }

    /// The defect this rendering revision exists to remove, reduced to the
    /// tag it was proven on. `exiftool -G1 -s` prints
    /// `RawBrightnessAdj : 0.00` and `-json` carries that text through as
    /// the bare token `0.00`; going through `f64` turned it into "0.0",
    /// and the harness then reported OxiDex's correct "0.00" as a value
    /// difference against a string ExifTool never printed.
    #[test]
    fn test_number_keeps_exiftools_own_text_not_an_f64_round_trip() {
        let extractor = ExifToolExtractor::new(vec!["exiftool".to_string()]);
        let tags = extractor.parse_single_file_json(&entry(
            r#"{
                "MakerNotes:RawBrightnessAdj": 0.00,
                "MakerNotes:TrailingZero": 1.50,
                "MakerNotes:Exponent": 1e3,
                "MakerNotes:LongPrecision": 1.100000000000000,
                "MakerNotes:NegativeZero": -0,
                "EXIF:PlainInteger": 400,
                "EXIF:AlreadyShortest": 2.8
            }"#,
        ));
        assert_eq!(value_of(&tags, "RawBrightnessAdj"), "0.00");
        assert_eq!(value_of(&tags, "TrailingZero"), "1.50");
        assert_eq!(value_of(&tags, "Exponent"), "1e3");
        assert_eq!(value_of(&tags, "LongPrecision"), "1.100000000000000");
        assert_eq!(value_of(&tags, "NegativeZero"), "-0");
        // Unchanged cases must stay byte-identical to rendering revision 1.
        assert_eq!(value_of(&tags, "PlainInteger"), "400");
        assert_eq!(value_of(&tags, "AlreadyShortest"), "2.8");
    }

    /// A genuinely different value must still read as different: this is
    /// not "make the numbers agree", it is "report the text ExifTool
    /// actually printed". `0.00` and `0.000` are two distinct PrintConv
    /// outputs and stay two distinct strings.
    #[test]
    fn test_distinct_number_texts_stay_distinct() {
        let extractor = ExifToolExtractor::new(vec!["exiftool".to_string()]);
        let tags = extractor.parse_single_file_json(&entry(
            r#"{"MakerNotes:A": 0.00, "MakerNotes:B": 0.000, "MakerNotes:C": 0}"#,
        ));
        assert_eq!(value_of(&tags, "A"), "0.00");
        assert_eq!(value_of(&tags, "B"), "0.000");
        assert_eq!(value_of(&tags, "C"), "0");
    }

    /// Everything that is not a bare number keeps rendering revision 1's
    /// output byte-for-byte -- strings unescaped, JSON booleans lowercase
    /// (ExifTool's own `EscapeJSON` lowercased them), lists compact.
    #[test]
    fn test_non_numeric_rendering_is_unchanged() {
        let extractor = ExifToolExtractor::new(vec!["exiftool".to_string()]);
        let tags = extractor.parse_single_file_json(&entry(
            r#"{
                "EXIF:Make": "Canon",
                "Photoshop:CopyrightFlag": false,
                "XMP:Subject": [
                    "a",
                    "b"
                ],
                "XMP:Escaped": "line\none",
                "XMP:Struct": {"k": "v"}
            }"#,
        ));
        assert_eq!(value_of(&tags, "Make"), "Canon");
        assert_eq!(value_of(&tags, "CopyrightFlag"), "false");
        assert_eq!(value_of(&tags, "Subject"), r#"["a","b"]"#);
        assert_eq!(value_of(&tags, "Escaped"), "line\none");
        assert_eq!(value_of(&tags, "Struct"), r#"{"k":"v"}"#);
    }

    /// A cache written before this rendering revision holds strings this
    /// code would no longer produce, so it must not be re-served. Without
    /// this the fix appears to do nothing on any machine that has already
    /// run the harness once.
    #[test]
    fn test_disk_cache_entry_without_revision_is_stale() {
        let old = r#"{
            "exiftool_version": "13.59",
            "signature": "abc",
            "result": {"tags": [], "files_processed": 0}
        }"#;
        let entry: DiskCacheEntry = serde_json::from_str(old).unwrap();
        assert_eq!(entry.rendering_revision, 0);
        assert_ne!(entry.rendering_revision, VALUE_RENDERING_REVISION);
    }
}
