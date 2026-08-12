//! Recursive directory processing for batch operations
//!
//! This module handles batch processing of multiple files and directories.
//! It provides parallel processing capabilities using rayon for efficient
//! metadata operations on large file collections.

use crate::cli::args::CliArgs;
use crate::cli::output_formatter::{
    CsvFormatter, HumanReadableFormatter, JsonFormatter, OutputFormatter, ShortFormatter,
};
use crate::cli::tag_resolution::{ResolvedFileOutput, resolve_file_output};
use crate::cli::value_parser::parse_cli_tag_value;
use crate::core::MetadataMap;
use crate::core::operations::{modify_tag, read_metadata_with_detector_and_options};
use crate::error::{ExifToolError, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use walkdir::WalkDir;

/// Statistics collected during batch processing
#[derive(Debug, Clone)]
pub struct BatchStats {
    /// Number of files successfully read
    pub files_read: usize,
    /// Number of files successfully updated (for write operations)
    pub files_updated: usize,
    /// Number of files that encountered errors
    pub errors: usize,
}

impl BatchStats {
    /// Creates a new BatchStats with zero counts
    fn new() -> Self {
        Self {
            files_read: 0,
            files_updated: 0,
            errors: 0,
        }
    }

    /// Prints the statistics in ExifTool-compatible format
    pub fn print(&self) {
        if self.files_read > 0 {
            println!("    {} image files read", self.files_read);
        }
        if self.files_updated > 0 {
            println!("    {} image files updated", self.files_updated);
        }
        if self.errors > 0 {
            println!("    {} files could not be read", self.errors);
        }
    }
}

/// Supported image and media file extensions
const SUPPORTED_EXTENSIONS: &[&str] = &[
    // JPEG
    "jpg", "jpeg", "jpe", "jfif", // TIFF
    "tif", "tiff", // PNG
    "png",  // Video
    "mp4", "m4v", "m4a", "m4b", "mov", // PDF
    "pdf", // Camera Raw - Canon
    "cr2", "cr3", "crw", // Camera Raw - Nikon
    "nef", "nrw", // Camera Raw - Sony
    "arw", "arq", "ari", "sr2", "srf", "srw", // Camera Raw - Fujifilm
    "raf", // Camera Raw - Olympus
    "orf", "ori", // Camera Raw - Pentax
    "pef", // Camera Raw - Panasonic
    "rw2", "rwl", // Camera Raw - Hasselblad
    "3fr", "fff", // Camera Raw - Phase One
    "iiq", // Camera Raw - Mamiya
    "mef", // Camera Raw - Leaf
    "mos", // Camera Raw - Kodak
    "dcr", "kdc", // Camera Raw - Minolta
    "mdc", "mrw", // Camera Raw - Epson
    "erf", // Camera Raw - Sigma
    "x3f", // Camera Raw - GoPro
    "gpr", // Camera Raw - DNG (Adobe Digital Negative)
    "dng", // Camera Raw - HEIF
    "hif", // Camera Raw - Light
    "lri", // Camera Raw - Sinar
    "sti", // Camera Raw - Generic/Other
    "raw", "cam", "rev",
];

/// Main entry point for batch processing operations.
///
/// This function handles both recursive directory traversal and batch file processing.
/// It automatically detects whether to perform read or write operations based on
/// the CLI arguments.
///
/// # Arguments
///
/// * `path` - Root path to start processing (file or directory)
/// * `args` - CLI arguments containing flags and tag modifications
///
/// # Returns
///
/// * `Ok(BatchStats)` - Processing completed with statistics
/// * `Err(ExifToolError)` - Fatal error occurred (e.g., invalid path)
///
/// # Processing Modes
///
/// - **Read mode**: No tag modifications specified - reads and outputs metadata
/// - **Write mode**: Tag modifications present - applies changes to all files
///
/// # Parallelization
///
/// Uses rayon's parallel iterators to process files concurrently across CPU cores.
/// Thread-safe atomic counters track statistics during parallel execution.
///
/// # Error Handling
///
/// Individual file errors are logged to stderr but do not stop batch processing.
/// All errors are counted and reported in the final statistics.
pub fn batch_process(path: &Path, args: &CliArgs) -> Result<BatchStats> {
    // Validate that the path exists
    if !path.exists() {
        return Err(ExifToolError::from(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Path does not exist: {}", path.display()),
        )));
    }

    // Collect all files to process
    let files = collect_files(path, args.recursive)?;

    if files.is_empty() {
        eprintln!(
            "Warning: No supported image files found in {}",
            path.display()
        );
        return Ok(BatchStats::new());
    }

    // Determine operation mode
    let modifications = args.tag_modifications();
    let is_write_mode = !modifications.is_empty();

    // Validate readonly flag for write operations
    if is_write_mode && args.readonly {
        return Err(ExifToolError::from(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Cannot modify files in read-only mode. Remove --readonly flag or remove tag modifications.",
        )));
    }

    // Process files based on mode
    if is_write_mode {
        batch_write(files, &modifications, args)
    } else {
        batch_read(files, args)
    }
}

/// Collects all supported image files from the given path.
///
/// # Arguments
///
/// * `path` - Starting path (file or directory)
/// * `recursive` - Whether to recursively traverse subdirectories
///
/// # Returns
///
/// Vector of PathBuf objects for all supported image files found
fn collect_files(path: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    if path.is_file() {
        // Single file - check if supported
        if is_supported_file(path) {
            files.push(path.to_path_buf());
        } else {
            eprintln!("Warning: File type not supported: {}", path.display());
        }
    } else if path.is_dir() {
        // Directory - walk and collect files
        let walker = if recursive {
            WalkDir::new(path)
                .follow_links(false) // Avoid symlink loops
                .into_iter()
        } else {
            WalkDir::new(path)
                .max_depth(1)
                .follow_links(false)
                .into_iter()
        };

        for entry in walker {
            match entry {
                Ok(entry) => {
                    if entry.file_type().is_file() && is_supported_file(entry.path()) {
                        files.push(entry.path().to_path_buf());
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Error accessing path: {}", e);
                }
            }
        }
    }

    Ok(files)
}

/// Checks if a file has a supported extension.
///
/// This function is exposed publicly for testing purposes.
///
/// # Arguments
///
/// * `path` - File path to check
///
/// # Returns
///
/// `true` if the file has a supported image/media extension, `false` otherwise
pub fn is_supported_file(path: &Path) -> bool {
    if let Some(ext) = path.extension()
        && let Some(ext_str) = ext.to_str()
    {
        let ext_lower = ext_str.to_lowercase();
        return SUPPORTED_EXTENSIONS.contains(&ext_lower.as_str());
    }
    false
}

/// Performs batch read operations on a collection of files.
///
/// Reads metadata from all files in parallel and outputs results.
/// Supports both JSON and human-readable output formats.
///
/// # Arguments
///
/// * `files` - Vector of file paths to process
/// * `args` - CLI arguments containing output format flags
///
/// # Returns
///
/// BatchStats with counts of successful reads and errors
pub fn batch_read(files: Vec<PathBuf>, args: &CliArgs) -> Result<BatchStats> {
    let file_count = files.len();

    // Create progress bar
    let progress = create_progress_bar(file_count, "Reading");

    // Atomic counters for thread-safe statistics
    let success_count = AtomicUsize::new(0);
    let error_count = AtomicUsize::new(0);

    // Step 21: the same `ReadOptions` single-file mode builds
    // (`main.rs::handle_read_operation`), from the same two inputs -- the
    // specific tags requested and `--extended-output` -- built once outside
    // the per-file loop since `args` is shared across every file in the
    // batch. Without this, a batch `-JPEGQualityEstimate` or
    // `--extended-output` request never reached the JPEG parser at all.
    let tag_filter = args.specific_tags();
    let read_options =
        crate::core::ReadOptions::new(tag_filter.as_deref().unwrap_or(&[]), args.extended_output);

    // Process files in parallel
    let results: Vec<_> = files
        .par_iter()
        .map(|path| {
            let result =
                read_metadata_with_detector_and_options(path, args.detector, &read_options);

            match &result {
                Ok(_) => {
                    success_count.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    error_count.fetch_add(1, Ordering::Relaxed);
                    eprintln!("Error reading {}: {}", path.display(), e);
                }
            }

            progress.inc(1);

            (path.clone(), result)
        })
        .collect();

    progress.finish_and_clear();

    // Output results. Each file's raw read is resolved through
    // `cli::tag_resolution::resolve_file_output` -- the same function
    // `main.rs::handle_read_operation` uses -- so `-a`/`-G*`/
    // `--no-print-conv`/the unfiltered default listing behave identically
    // between batch and single-file mode for the same file (Step 21 closes
    // the Step 20 gap where batch instead fed `args.specific_tags()`
    // straight into each formatter's own exact/suffix `filter_tags`
    // matching, bypassing Step 20's group/priority-aware resolution
    // entirely).
    if args.csv {
        output_csv_results(&results, args)?;
    } else if args.json {
        output_json_results(&results, args)?;
    } else if args.short_format {
        output_short_results(&results, args);
    } else {
        output_human_readable_results(&results, args);
    }

    Ok(BatchStats {
        files_read: success_count.load(Ordering::Relaxed),
        files_updated: 0,
        errors: error_count.load(Ordering::Relaxed),
    })
}

/// Performs batch write operations on a collection of files.
///
/// Applies the same tag modifications to all files in parallel.
///
/// # Arguments
///
/// * `files` - Vector of file paths to process
/// * `modifications` - Tag modifications to apply (tag_name, value pairs)
/// * `args` - CLI arguments containing file preservation flags
///
/// # Returns
///
/// BatchStats with counts of successful updates and errors
pub fn batch_write(
    files: Vec<PathBuf>,
    modifications: &[(String, String)],
    args: &CliArgs,
) -> Result<BatchStats> {
    let file_count = files.len();

    // Create progress bar
    let progress = create_progress_bar(file_count, "Writing");

    // Atomic counters for thread-safe statistics
    let success_count = AtomicUsize::new(0);
    let error_count = AtomicUsize::new(0);

    // Process files in parallel
    files.par_iter().for_each(|path| {
        let result = apply_modifications(path, modifications, args);

        match result {
            Ok(_) => {
                success_count.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                error_count.fetch_add(1, Ordering::Relaxed);
                eprintln!("Error writing {}: {}", path.display(), e);
            }
        }

        progress.inc(1);
    });

    progress.finish_and_clear();

    Ok(BatchStats {
        files_read: file_count,
        files_updated: success_count.load(Ordering::Relaxed),
        errors: error_count.load(Ordering::Relaxed),
    })
}

/// Applies tag modifications to a single file.
///
/// Handles file preservation options (backup, preserve timestamps).
///
/// # Arguments
///
/// * `path` - File to modify
/// * `modifications` - Tag modifications to apply
/// * `args` - CLI arguments for preservation options
fn apply_modifications(
    path: &Path,
    modifications: &[(String, String)],
    args: &CliArgs,
) -> Result<()> {
    // Preserve original file times if requested
    let original_metadata = if args.preserve_file_times {
        Some(fs::metadata(path)?)
    } else {
        None
    };

    // Create backup if requested
    if args.backup {
        let backup_path = path.with_extension(format!(
            "{}.bak",
            path.extension().and_then(|e| e.to_str()).unwrap_or("")
        ));
        fs::copy(path, &backup_path)?;
    }

    // Apply all modifications
    for (tag_name, value_str) in modifications {
        // Parse the string into the type the tag declares, not into whatever
        // type the value's own shape suggests.
        let tag_value = parse_cli_tag_value(tag_name, value_str)?;
        modify_tag(path, tag_name, tag_value)?;
    }

    // Restore file times if requested
    if let Some(metadata) = original_metadata
        && let Ok(mtime) = metadata.modified()
    {
        use std::fs::File;
        if let Err(_e) = File::open(path).and_then(|f| f.set_modified(mtime)) {
            // Silently ignore errors - the write succeeded, only mtime restoration failed
            // Errors are expected on some filesystems or when permissions are restricted
        }
    }

    Ok(())
}

/// Creates a progress bar for batch processing.
///
/// # Arguments
///
/// * `total` - Total number of files to process
/// * `action` - Action being performed ("Reading" or "Writing")
fn create_progress_bar(total: usize, action: &str) -> ProgressBar {
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{bar:40.cyan/blue}] {pos}/{len} files ({msg})")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb.set_message(action.to_string());
    pb
}

/// Resolves one file's raw read the same way `main.rs::handle_read_operation`
/// does (`cli::tag_resolution::resolve_file_output`) for a caller that only
/// ever wants the `MetadataMap` form. Both callers of this helper
/// (CSV and JSON output) already gate on `args.csv`/`args.json`, so
/// `resolve_file_output` never takes its `-Gn` + human/short `Lines` branch
/// here (that branch requires `!args.json && !args.csv`) -- the `Lines` arm
/// below is unreachable in practice and returns an empty map rather than
/// panicking, so a future change to that invariant fails loud (empty tags)
/// instead of crashing a batch run.
fn resolved_metadata_for_structured_output(metadata: &MetadataMap, args: &CliArgs) -> MetadataMap {
    match resolve_file_output(metadata, args) {
        ResolvedFileOutput::Metadata(m) => m,
        ResolvedFileOutput::Lines(_) => MetadataMap::new(),
    }
}

fn output_csv_results(results: &[(PathBuf, Result<MetadataMap>)], args: &CliArgs) -> Result<()> {
    let formatter = CsvFormatter;
    let mut writer = csv::Writer::from_writer(Vec::new());

    writer
        .write_record(["SourceFile", "Tag", "Value"])
        .map_err(|e| ExifToolError::parse_error(format!("CSV formatting failed: {e}")))?;

    for (path, result) in results {
        if let Ok(metadata) = result {
            let metadata = resolved_metadata_for_structured_output(metadata, args);
            let rendered = formatter.format(&metadata, None);
            let source_file = path.display().to_string();
            // Parse without implicit header handling and skip the formatter's
            // "Tag,Value" header row explicitly, so a formatter change cannot
            // silently swallow each file's first data row.
            let mut reader = csv::ReaderBuilder::new()
                .has_headers(false)
                .from_reader(rendered.as_bytes());

            for record in reader.records() {
                let record = record
                    .map_err(|e| ExifToolError::parse_error(format!("CSV parsing failed: {e}")))?;
                if record.get(0) == Some("Tag") && record.get(1) == Some("Value") {
                    continue;
                }
                writer
                    .write_record([
                        source_file.as_str(),
                        record.get(0).unwrap_or_default(),
                        record.get(1).unwrap_or_default(),
                    ])
                    .map_err(|e| {
                        ExifToolError::parse_error(format!("CSV formatting failed: {e}"))
                    })?;
            }
        }
    }

    writer
        .flush()
        .map_err(|e| ExifToolError::parse_error(format!("CSV formatting failed: {e}")))?;
    let bytes = writer
        .into_inner()
        .map_err(|e| ExifToolError::parse_error(format!("CSV formatting failed: {e}")))?;
    let output = String::from_utf8(bytes)
        .map_err(|e| ExifToolError::parse_error(format!("CSV formatting failed: {e}")))?;
    print!("{}", output);

    Ok(())
}

fn output_short_results(results: &[(PathBuf, Result<MetadataMap>)], args: &CliArgs) {
    let formatter = ShortFormatter;

    for (path, result) in results {
        if let Ok(metadata) = result {
            match resolve_file_output(metadata, args) {
                ResolvedFileOutput::Lines(lines) => {
                    if !lines.is_empty() {
                        println!("SourceFile: {}", path.display());
                        print!("{}", lines);
                    }
                }
                ResolvedFileOutput::Metadata(metadata) => {
                    let output = formatter.format(&metadata, None);
                    if !output.is_empty() {
                        println!("SourceFile: {}", path.display());
                        print!("{}", output);
                    }
                }
            }
        }
    }
}

/// Outputs results in JSON format.
///
/// Creates a JSON array with one object per file containing:
/// - SourceFile: file path
/// - All metadata tags (for successful reads)
/// - Error message (for failed reads)
fn output_json_results(results: &[(PathBuf, Result<MetadataMap>)], args: &CliArgs) -> Result<()> {
    use serde_json::{Value, json};

    let formatter = JsonFormatter;

    let json_array: Vec<Value> = results
        .iter()
        .map(|(path, result)| -> Result<Value> {
            match result {
                Ok(metadata) => {
                    let metadata = resolved_metadata_for_structured_output(metadata, args);
                    let formatted = formatter.format(&metadata, None);
                    let mut values: Vec<Value> = serde_json::from_str(&formatted).map_err(|e| {
                        ExifToolError::parse_error(format!("Failed to parse formatted JSON: {}", e))
                    })?;
                    let mut obj = values.pop().ok_or_else(|| {
                        ExifToolError::parse_error("Formatted JSON contained no objects")
                    })?;
                    let map = obj.as_object_mut().ok_or_else(|| {
                        ExifToolError::parse_error("Formatted JSON entry was not an object")
                    })?;
                    map.insert("SourceFile".to_string(), json!(path.display().to_string()));
                    Ok(obj)
                }
                Err(e) => Ok(json!({
                    "SourceFile": path.display().to_string(),
                    "Error": e.to_string(),
                })),
            }
        })
        .collect::<Result<Vec<_>>>()?;

    // serde_json's Map is a plain BTreeMap here (this crate doesn't enable the
    // "preserve_order" feature), so it always iterates keys alphabetically --
    // inserting "SourceFile" cannot make it serialize first the way ExifTool's
    // `-j` does. Render the array by hand so SourceFile leads each object.
    print_json_array_with_source_file_first(&json_array);
    Ok(())
}

/// Serializes `objects` as a pretty-printed JSON array, printing each
/// object's `SourceFile` key first (as ExifTool's `-j` does) and every other
/// key in its existing (alphabetical) order after it.
fn print_json_array_with_source_file_first(objects: &[serde_json::Value]) {
    let mut out = String::from("[\n");
    for (i, obj) in objects.iter().enumerate() {
        out.push_str("  ");
        match obj.as_object() {
            Some(map) => out.push_str(&ordered_object_to_json(map, 2)),
            None => out.push_str(&obj.to_string()),
        }
        if i + 1 < objects.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push(']');
    println!("{}", out);
}

/// Renders a JSON object with `SourceFile` (if present) as the first key,
/// at the given indent level (spaces before each `"key": value` line).
fn ordered_object_to_json(
    map: &serde_json::Map<String, serde_json::Value>,
    indent: usize,
) -> String {
    if map.is_empty() {
        return "{}".to_string();
    }

    let inner_indent = indent + 2;
    let inner_pad = " ".repeat(inner_indent);

    let mut ordered: Vec<(&str, &serde_json::Value)> = Vec::with_capacity(map.len());
    if let Some(v) = map.get("SourceFile") {
        ordered.push(("SourceFile", v));
    }
    for (k, v) in map.iter() {
        if k != "SourceFile" {
            ordered.push((k.as_str(), v));
        }
    }

    let mut out = String::from("{\n");
    for (i, (key, value)) in ordered.iter().enumerate() {
        out.push_str(&inner_pad);
        out.push_str(&serde_json::to_string(key).unwrap_or_default());
        out.push_str(": ");
        out.push_str(&reindent_pretty_json(value, inner_indent));
        if i + 1 < ordered.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(&" ".repeat(indent));
    out.push('}');
    out
}

/// Pretty-prints a JSON value, shifting every line after the first over by
/// `indent` spaces so a multi-line value (an array or nested object) lines
/// up under the key that introduces it.
fn reindent_pretty_json(value: &serde_json::Value, indent: usize) -> String {
    let rendered = serde_json::to_string_pretty(value).unwrap_or_default();
    let mut lines = rendered.lines();
    let Some(first) = lines.next() else {
        return rendered;
    };
    let pad = " ".repeat(indent);
    let mut result = first.to_string();
    for line in lines {
        result.push('\n');
        result.push_str(&pad);
        result.push_str(line);
    }
    result
}

#[cfg(test)]
mod json_ordering_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_file_is_always_first_key() {
        let objects = vec![json!({
            "Zebra": "z",
            "Apple": "a",
            "SourceFile": "photo.jpg",
        })];

        let map = objects[0].as_object().unwrap();
        let rendered = ordered_object_to_json(map, 2);
        let source_pos = rendered.find("\"SourceFile\"").unwrap();
        let apple_pos = rendered.find("\"Apple\"").unwrap();
        let zebra_pos = rendered.find("\"Zebra\"").unwrap();
        assert!(source_pos < apple_pos);
        assert!(source_pos < zebra_pos);
    }

    #[test]
    fn reindented_array_value_parses_back_identically() {
        let objects = vec![json!({
            "SourceFile": "photo.jpg",
            "IPTC:Keywords": ["ExifTool", "Test"],
        })];

        let mut out = String::from("[\n  ");
        out.push_str(&ordered_object_to_json(objects[0].as_object().unwrap(), 2));
        out.push_str("\n]");

        let reparsed: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
        assert_eq!(reparsed[0]["IPTC:Keywords"], json!(["ExifTool", "Test"]));
        assert_eq!(reparsed[0]["SourceFile"], json!("photo.jpg"));
    }
}

/// Outputs results in human-readable format.
///
/// Prints each file's metadata with a file path header.
fn output_human_readable_results(results: &[(PathBuf, Result<MetadataMap>)], args: &CliArgs) {
    let formatter = HumanReadableFormatter;

    for (path, result) in results {
        match result {
            Ok(metadata) => {
                println!("File: {}", path.display());
                match resolve_file_output(metadata, args) {
                    ResolvedFileOutput::Lines(lines) => print!("{}", lines),
                    ResolvedFileOutput::Metadata(metadata) => {
                        let output = formatter.format(&metadata, None);
                        print!("{}", output);
                    }
                }
            }
            Err(_) => {
                // Error already printed to stderr during processing
            }
        }
    }
}
