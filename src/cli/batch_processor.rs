//! Recursive directory processing for batch operations
//!
//! This module handles batch processing of multiple files and directories.
//! It provides parallel processing capabilities using rayon for efficient
//! metadata operations on large file collections.

use crate::cli::args::CliArgs;
use crate::core::operations::{modify_tag, read_metadata};
use crate::core::tag_value::TagValue;
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
    "jpg", "jpeg", "jpe", "jfif", // JPEG
    "tif", "tiff", // TIFF
    "png",  // PNG
    "mp4", "m4v", "m4a", "m4b", "mov", // MP4/QuickTime
    "pdf", // PDF
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
/// # Arguments
///
/// * `path` - File path to check
///
/// # Returns
///
/// `true` if the file has a supported image/media extension, `false` otherwise
fn is_supported_file(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        if let Some(ext_str) = ext.to_str() {
            let ext_lower = ext_str.to_lowercase();
            return SUPPORTED_EXTENSIONS.contains(&ext_lower.as_str());
        }
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
fn batch_read(files: Vec<PathBuf>, args: &CliArgs) -> Result<BatchStats> {
    let file_count = files.len();

    // Create progress bar
    let progress = create_progress_bar(file_count, "Reading");

    // Atomic counters for thread-safe statistics
    let success_count = AtomicUsize::new(0);
    let error_count = AtomicUsize::new(0);

    // Process files in parallel
    let results: Vec<_> = files
        .par_iter()
        .map(|path| {
            let result = read_metadata(path);

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

    // Output results
    if args.json {
        output_json_results(&results)?;
    } else {
        output_human_readable_results(&results);
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
fn batch_write(
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
        // Parse string value to TagValue
        let tag_value = parse_tag_value(value_str);
        modify_tag(path, tag_name, tag_value)?;
    }

    // Restore file times if requested
    if let Some(metadata) = original_metadata {
        if let Ok(mtime) = metadata.modified() {
            use std::fs::File;
            if let Err(_e) = File::open(path).and_then(|f| f.set_modified(mtime)) {
                // Silently ignore errors - the write succeeded, only mtime restoration failed
                // Errors are expected on some filesystems or when permissions are restricted
            }
        }
    }

    Ok(())
}

/// Parses a string value into a TagValue.
///
/// Attempts to detect the appropriate type:
/// - Integers (e.g., "100", "200")
/// - Floats (e.g., "5.6", "3.14")
/// - Strings (everything else)
fn parse_tag_value(value: &str) -> TagValue {
    // Try to parse as integer
    if let Ok(int_val) = value.parse::<i64>() {
        return TagValue::new_integer(int_val);
    }

    // Try to parse as float
    if let Ok(float_val) = value.parse::<f64>() {
        return TagValue::new_float(float_val);
    }

    // Default to string
    TagValue::new_string(value.to_string())
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

/// Outputs results in JSON format.
///
/// Creates a JSON array with one object per file containing:
/// - SourceFile: file path
/// - All metadata tags (for successful reads)
/// - Error message (for failed reads)
fn output_json_results(results: &[(PathBuf, Result<crate::core::MetadataMap>)]) -> Result<()> {
    use serde_json::{json, Value};

    let json_array: Vec<Value> = results
        .iter()
        .filter_map(|(path, result)| {
            match result {
                Ok(metadata) => {
                    let mut obj = serde_json::Map::new();
                    obj.insert("SourceFile".to_string(), json!(path.display().to_string()));

                    // Add all metadata tags
                    for (tag_name, tag_value) in metadata.iter() {
                        let value = match tag_value {
                            TagValue::String(s) => json!(s),
                            TagValue::Integer(i) => json!(i),
                            TagValue::Float(f) => json!(f),
                            TagValue::Rational {
                                numerator,
                                denominator,
                            } => {
                                json!(format!("{}/{}", numerator, denominator))
                            }
                            TagValue::Binary(b) => {
                                json!(format!("(Binary data, {} bytes)", b.len()))
                            }
                            TagValue::DateTime(dt) => json!(dt.to_rfc3339()),
                            TagValue::Struct(_) => json!("(Structured data)"),
                            TagValue::Array(values) => {
                                let array: Vec<serde_json::Value> = values.iter().map(|v| match v {
                                    TagValue::String(s) => json!(s),
                                    TagValue::Integer(i) => json!(i),
                                    TagValue::Float(f) => json!(f),
                                    _ => json!(format!("{:?}", v)),
                                }).collect();
                                json!(array)
                            }
                        };
                        obj.insert(tag_name.clone(), value);
                    }

                    Some(Value::Object(obj))
                }
                Err(_) => {
                    // Errors already printed to stderr, skip in JSON output
                    None
                }
            }
        })
        .collect();

    match serde_json::to_string_pretty(&json_array) {
        Ok(json_str) => {
            println!("{}", json_str);
            Ok(())
        }
        Err(e) => Err(ExifToolError::parse_error(format!(
            "Failed to serialize JSON: {}",
            e
        ))),
    }
}

/// Outputs results in human-readable format.
///
/// Prints each file's metadata with "File:" header separating files.
fn output_human_readable_results(results: &[(PathBuf, Result<crate::core::MetadataMap>)]) {
    use crate::cli::output_formatter::{HumanReadableFormatter, OutputFormatter};

    let formatter = HumanReadableFormatter;

    for (path, result) in results {
        match result {
            Ok(metadata) => {
                println!("\n======== {} ========", path.display());
                let output = formatter.format(metadata, None);
                print!("{}", output);
            }
            Err(_) => {
                // Error already printed to stderr during processing
            }
        }
    }
}
