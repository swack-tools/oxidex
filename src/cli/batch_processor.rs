//! Recursive directory processing for batch operations
//!
//! This module handles batch processing of multiple files and directories.
//! It provides parallel processing capabilities using rayon for efficient
//! metadata operations on large file collections.

use crate::cli::args::CliArgs;
use crate::cli::non_utf8::{PathLine, json_path, os_bytes};
use crate::cli::output_formatter::{
    CsvFormatter, HumanReadableFormatter, JsonFormatter, JsonNode, OutputFormatter, ShortFormatter,
};
use crate::cli::tag_resolution::{ResolvedFileOutput, resolve_file_output};
use crate::cli::value_parser::parse_cli_tag_value_os;
use crate::core::MetadataMap;
use crate::core::operations::{modify_tag, read_metadata_with_detector_and_options};
use crate::error::{ExifToolError, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
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
    /// Number of files a directory walk found but never attempted to read,
    /// because their extension is not one `crate::filetype`'s table (see
    /// [`is_supported_file`]) recognizes at all.
    ///
    /// This is the count that used to not exist: a directory walk gated on
    /// a hand-maintained extension allow-list dropped every file with an
    /// extension missing from that list with no error, no warning and no
    /// number anywhere in the output -- a run over a directory containing
    /// only such files reported success having read zero of them. Every
    /// file this field counts was positively identified as unreadable (an
    /// extension absent from ExifTool's own `%fileTypeLookup`-derived
    /// table), not merely omitted from a list someone forgot to extend.
    pub unidentified: usize,
}

impl BatchStats {
    /// Creates a new BatchStats with zero counts
    fn new() -> Self {
        Self {
            files_read: 0,
            files_updated: 0,
            errors: 0,
            unidentified: 0,
        }
    }

    /// Prints the statistics in ExifTool-compatible format
    pub fn print(&self) {
        // ExifTool's summary is `printf("%5d image files read\n", ...)`
        // (`exiftool`:2071-2077, 13.59): the count is right-aligned in five
        // columns, so ten or more files print `   12 ...`, not `    12 ...`.
        if self.files_read > 0 {
            println!("{:5} image files read", self.files_read);
        }
        if self.files_updated > 0 {
            println!("{:5} image files updated", self.files_updated);
        }
        if self.errors > 0 {
            println!("{:5} files could not be read", self.errors);
        }
        if self.unidentified > 0 {
            println!(
                "{:5} files skipped (extension not recognized)",
                self.unidentified
            );
        }
    }
}

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
    let (files, unidentified) = collect_files(path, args.recursive)?;

    if files.is_empty() {
        PathLine::new("Warning: No supported image files found in ")
            .path(path)
            .eprint();
        let mut stats = BatchStats::new();
        stats.unidentified = unidentified;
        return Ok(stats);
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
    let mut stats = if is_write_mode {
        batch_write(files, &modifications, args)?
    } else {
        batch_read(files, args)?
    };
    stats.unidentified = unidentified;
    Ok(stats)
}

/// Collects all identifiable files from the given path, and counts (without
/// reading) every file it skips.
///
/// # Arguments
///
/// * `path` - Starting path (file or directory)
/// * `recursive` - Whether to recursively traverse subdirectories
///
/// # Returns
///
/// `(files, unidentified)`: the files to attempt, and a count of files this
/// walk declined to queue because [`is_supported_file`] could not recognize
/// their extension. That count is never dropped -- see
/// [`BatchStats::unidentified`] -- it travels back up through
/// [`batch_process`] into the stats the caller prints.
fn collect_files(path: &Path, recursive: bool) -> Result<(Vec<PathBuf>, usize)> {
    let mut files = Vec::new();
    let mut unidentified = 0usize;

    if path.is_file() {
        // Single file - check if supported
        if is_supported_file(path) {
            files.push(path.to_path_buf());
        } else {
            PathLine::new("Warning: File type not supported: ")
                .path(path)
                .eprint();
            unidentified += 1;
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
                    if !entry.file_type().is_file() {
                        continue;
                    }
                    if is_supported_file(entry.path()) {
                        files.push(entry.path().to_path_buf());
                    } else {
                        unidentified += 1;
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Error accessing path: {}", e);
                }
            }
        }
    }

    Ok((files, unidentified))
}

/// Whether a file's extension is one that identification can recognize at
/// all.
///
/// This is a fast pre-filter, not the identification itself. A file that
/// passes still goes through the exact same magic-number pipeline
/// single-file mode uses --
/// [`read_metadata_with_detector_and_options`](crate::core::operations::read_metadata_with_detector_and_options)
/// -- which is what actually decides whether the file can be read; a
/// recognized extension whose header disagrees, or whose format has no
/// parser, is still counted correctly afterward (as a read error or, per
/// `add_identity_tags`'s "detected is not parsed" fallback, as a success
/// carrying only identity tags). This function exists only to avoid
/// mmap'ing and probing every stray file (`.DS_Store`, a `.git` object) in
/// a large tree before it can find that out.
///
/// It used to be a hand-maintained ~90-entry list, which is exactly the
/// defect AGENTS.md warns against: it silently fell behind the ~40+
/// extensions OxiDex added real parsers for later (MP3, ZIP, DOCX, TXT,
/// HTML, EPUB, and more all have working parsers in
/// `crate::core::format_dispatch` yet were absent from that list), so
/// `oxidex -r` skipped them with no error, no warning and no count -- a
/// run over a directory of nothing but such files reported success having
/// read zero of them.
///
/// This is backed instead by `crate::filetype`'s extension table, which is
/// generated from ExifTool's own `%fileTypeLookup`/`%fileTypeExt` (see
/// `crate::core::operations::add_identity_tags`'s doc comment) and so
/// cannot drift the same way. ExifTool's own default recursive scan
/// applies the identical filter for the identical reason: `ScanDir`
/// (`exiftool:4370-4378`) skips a file when `GetFileType($file)`
/// (`ExifTool.pm:4214`, keyed on `%fileTypeLookup`) comes back empty,
/// unless `-ext` was given.
///
/// Files this returns `false` for are never queued for a read, but they
/// are never silently dropped either: [`collect_files`] counts every one
/// into `BatchStats::unidentified`, which is printed in the final summary.
pub fn is_supported_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| crate::filetype::identify_by_extension(ext).is_some())
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
                    PathLine::new("Error reading ")
                        .path(path)
                        .text(&format!(": {e}"))
                        .eprint();
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
    } else if args.short_level > 0 {
        output_short_results(&results, args);
    } else {
        output_human_readable_results(&results, args);
    }

    Ok(BatchStats {
        files_read: success_count.load(Ordering::Relaxed),
        files_updated: 0,
        errors: error_count.load(Ordering::Relaxed),
        unidentified: 0,
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
    modifications: &[(String, OsString)],
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
                PathLine::new("Error writing ")
                    .path(path)
                    .text(&format!(": {e}"))
                    .eprint();
            }
        }

        progress.inc(1);
    });

    progress.finish_and_clear();

    Ok(BatchStats {
        files_read: file_count,
        files_updated: success_count.load(Ordering::Relaxed),
        errors: error_count.load(Ordering::Relaxed),
        unidentified: 0,
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
    modifications: &[(String, OsString)],
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
        // `<ext>.bak`, built on the `OsStr`: a `to_str()` here dropped an
        // extension that is not UTF-8, and `n.\xff` backed up to `n.bak`.
        let backup_path = match path.extension() {
            Some(extension) => {
                let mut extension = extension.to_os_string();
                extension.push(".bak");
                path.with_extension(extension)
            }
            None => path.with_extension(".bak"),
        };
        fs::copy(path, &backup_path)?;
    }

    // Apply all modifications
    for (tag_name, value_str) in modifications {
        // Parse the string into the type the tag declares, not into whatever
        // type the value's own shape suggests.
        let tag_value = parse_cli_tag_value_os(tag_name, value_str)?;
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
            let rendered = formatter.format_with_mode(&metadata, None, !args.exiftool_compat());
            // The path's own bytes, as ExifTool's `-csv` prints them.
            let source_file = os_bytes(path.as_os_str());
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
                        source_file,
                        record.get(0).unwrap_or_default().as_bytes(),
                        record.get(1).unwrap_or_default().as_bytes(),
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
    // Bytes, not a `String`: a path need not be UTF-8.
    std::io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(ExifToolError::from)?;

    Ok(())
}

/// Outputs results at one of ExifTool's short levels (`-s`, `-s2`, `-s3`).
///
/// Each file read is introduced by ExifTool's own `======== FILE` line
/// (`exiftool`:2328-2332, printed whenever more than one file is processed),
/// including a file that has none of the requested tags; the caller prints
/// the `%5d image files read` summary after the last one.
fn output_short_results(results: &[(PathBuf, Result<MetadataMap>)], args: &CliArgs) {
    let formatter = ShortFormatter;

    for (path, result) in results {
        if let Ok(metadata) = result {
            PathLine::new("======== ").path(path).print();
            match resolve_file_output(metadata, args) {
                ResolvedFileOutput::Lines(lines) => print!("{}", lines),
                ResolvedFileOutput::Metadata(metadata) => {
                    let output =
                        formatter.format_with_mode(&metadata, None, !args.exiftool_compat());
                    print!("{}", output);
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
    let formatter = JsonFormatter;

    // Build each object as a `JsonNode` tree directly. Rendering to text and
    // parsing it back through `serde_json::Value` (as this once did) would
    // re-render every bare numeric token through f64 -- `2.00` back to `2.0`
    // -- undoing the verbatim spelling `EscapeJSON` preserves.
    let objects: Vec<BTreeMap<String, JsonNode>> = results
        .iter()
        .map(|(path, result)| {
            let mut map = match result {
                Ok(metadata) => {
                    let metadata = resolved_metadata_for_structured_output(metadata, args);
                    formatter.build_json_map(&metadata, None, !args.exiftool_compat())
                }
                Err(e) => BTreeMap::from([("Error".to_string(), JsonNode::String(e.to_string()))]),
            };
            map.insert(
                "SourceFile".to_string(),
                // ExifTool's `-j` replaces each malformed byte of a path with
                // `?` (`FixUTF8`), keeping the output valid JSON.
                JsonNode::String(json_path(path)),
            );
            map
        })
        .collect();

    // The object map iterates keys alphabetically, so inserting "SourceFile"
    // cannot make it serialize first the way ExifTool's `-j` does. Render
    // the array by hand so SourceFile leads each object.
    println!("{}", json_array_with_source_file_first(&objects));
    Ok(())
}

/// Renders `objects` as a pretty-printed JSON array, printing each object's
/// `SourceFile` key first (as ExifTool's `-j` does) and every other key in
/// its existing (alphabetical) order after it.
fn json_array_with_source_file_first(objects: &[BTreeMap<String, JsonNode>]) -> String {
    let mut out = String::from("[\n");
    for (i, map) in objects.iter().enumerate() {
        out.push_str("  ");
        let entries = map
            .get_key_value("SourceFile")
            .into_iter()
            .chain(map.iter().filter(|(k, _)| k.as_str() != "SourceFile"))
            .map(|(k, v)| (k.as_str(), v));
        JsonNode::write_object(entries, &mut out, 2);
        if i + 1 < objects.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push(']');
    out
}

#[cfg(test)]
mod json_ordering_tests {
    use super::*;
    use serde_json::json;

    fn object(pairs: &[(&str, JsonNode)]) -> BTreeMap<String, JsonNode> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn source_file_is_always_first_key() {
        let objects = vec![object(&[
            ("Zebra", JsonNode::String("z".into())),
            ("Apple", JsonNode::String("a".into())),
            ("SourceFile", JsonNode::String("photo.jpg".into())),
        ])];

        let rendered = json_array_with_source_file_first(&objects);
        let source_pos = rendered.find("\"SourceFile\"").unwrap();
        let apple_pos = rendered.find("\"Apple\"").unwrap();
        let zebra_pos = rendered.find("\"Zebra\"").unwrap();
        assert!(source_pos < apple_pos);
        assert!(source_pos < zebra_pos);
    }

    #[test]
    fn reindented_array_value_parses_back_identically() {
        let objects = vec![object(&[
            ("SourceFile", JsonNode::String("photo.jpg".into())),
            (
                "IPTC:Keywords",
                JsonNode::Array(vec![
                    JsonNode::String("ExifTool".into()),
                    JsonNode::String("Test".into()),
                ]),
            ),
        ])];

        let out = json_array_with_source_file_first(&objects);
        let reparsed: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
        assert_eq!(reparsed[0]["IPTC:Keywords"], json!(["ExifTool", "Test"]));
        assert_eq!(reparsed[0]["SourceFile"], json!("photo.jpg"));
    }

    /// The batch writer must keep ExifTool's verbatim numeric spelling; it
    /// used to parse the formatter's text back through `serde_json::Value`,
    /// which printed `2.00` as `2.0`.
    #[test]
    fn batch_writer_keeps_numeric_literal_spelling() {
        let objects = vec![object(&[
            ("SourceFile", JsonNode::String("a.ntf".into())),
            ("NITF:NITFVersion", JsonNode::Literal("2.00".into())),
            ("Test:Exp", JsonNode::Literal("1.000e-06".into())),
        ])];
        assert_eq!(
            json_array_with_source_file_first(&objects),
            "[\n  {\n    \"SourceFile\": \"a.ntf\",\n    \"NITF:NITFVersion\": 2.00,\n    \"Test:Exp\": 1.000e-06\n  }\n]"
        );
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
                PathLine::new("File: ").path(path).print();
                match resolve_file_output(metadata, args) {
                    ResolvedFileOutput::Lines(lines) => print!("{}", lines),
                    ResolvedFileOutput::Metadata(metadata) => {
                        let output =
                            formatter.format_with_mode(&metadata, None, !args.exiftool_compat());
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
