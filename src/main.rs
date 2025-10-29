//! ExifTool-RS Command Line Interface
//!
//! This is the main entry point for the exiftool-rs command-line application.

use clap::Parser;
use exiftool_rs::cli::args::CliArgs;
use exiftool_rs::cli::batch_processor;
use exiftool_rs::cli::output_formatter::{HumanReadableFormatter, JsonFormatter, OutputFormatter};
use exiftool_rs::core::operations::{copy_metadata, modify_tag, read_metadata};
use exiftool_rs::core::tag_value::TagValue;
use std::process;

fn main() {
    // Parse command-line arguments using clap
    let args = CliArgs::parse();

    // Display warning for unimplemented features
    if args.short_format {
        eprintln!("Warning: Short format output (-s) is not yet fully implemented");
    }

    // Extract file path from arguments
    let file = match args.file() {
        Some(path) => path,
        None => {
            eprintln!("Error: No file or directory specified");
            eprintln!("Usage: exiftool-rs [OPTIONS] [-TAG=VALUE ...] FILE|DIRECTORY");
            process::exit(1);
        }
    };

    // Check if this is a copy operation (-TagsFromFile)
    if args.tags_from_file.is_some() {
        // Copy metadata mode
        handle_copy_operation(&file, &args);
    } else if file.is_dir() || args.recursive {
        // Batch processing mode
        handle_batch_processing(&file, &args);
    } else {
        // Single file processing mode
        let modifications = args.tag_modifications();

        if !modifications.is_empty() {
            // Write mode: modify tags
            handle_write_operation(&file, &args);
        } else {
            // Read mode: display metadata
            handle_read_operation(&file, args.json);
        }
    }
}

/// Handles write operations (tag modifications)
fn handle_write_operation(file: &std::path::Path, args: &CliArgs) {
    // Extract tag modifications
    let modifications = args.tag_modifications();

    // Check readonly flag FIRST - if set, prevent any writes
    if args.readonly {
        eprintln!("Error: Cannot modify file in read-only mode (--readonly flag set)");
        process::exit(1);
    }

    // Verify file exists
    if !file.exists() {
        eprintln!("Error: File not found: {}", file.display());
        process::exit(1);
    }

    // Check if file is writable
    let file_metadata = match std::fs::metadata(file) {
        Ok(metadata) => {
            if metadata.permissions().readonly() {
                eprintln!("Error: File is read-only: {}", file.display());
                process::exit(1);
            }
            metadata
        }
        Err(e) => {
            eprintln!("Error: Cannot access file '{}': {}", file.display(), e);
            process::exit(1);
        }
    };

    // Save original modification time if preserve_file_times is enabled
    let original_mtime = if args.preserve_file_times {
        match file_metadata.modified() {
            Ok(mtime) => Some(mtime),
            Err(e) => {
                eprintln!("Warning: Could not read file modification time: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Create backup if requested
    if args.backup {
        // Create backup by appending .bak to the original filename
        // Example: photo.jpg -> photo.jpg.bak
        let mut backup_path = file.as_os_str().to_owned();
        backup_path.push(".bak");
        let backup_path = std::path::PathBuf::from(backup_path);

        if let Err(e) = std::fs::copy(file, &backup_path) {
            eprintln!(
                "Error: Failed to create backup file '{}': {}",
                backup_path.display(),
                e
            );
            process::exit(1);
        }
    }

    // Apply each modification
    for (tag_name, value) in &modifications {
        // Convert value to TagValue (currently only supporting strings)
        let tag_value = TagValue::new_string(value.clone());

        // Call modify_tag from core operations
        if let Err(e) = modify_tag(file, tag_name, tag_value) {
            // Format error message based on error type
            let error_msg = format!("{}", e);
            if error_msg.contains("invalid") || error_msg.contains("Invalid") {
                eprintln!("Error: Invalid value for {}: {}", tag_name, e);
            } else {
                eprintln!("Error: Failed to modify tag '{}': {}", tag_name, e);
            }
            process::exit(1);
        }
    }

    // Restore original modification time if requested
    if let Some(mtime) = original_mtime {
        use std::fs::File;
        match File::open(file).and_then(|f| f.set_modified(mtime)) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Warning: Could not restore file modification time: {}", e);
                // Don't exit - the write succeeded, only mtime restoration failed
            }
        }
    }

    // Print success message (matching ExifTool format)
    println!("    1 image files updated");
}

/// Handles read operations (displaying metadata)
fn handle_read_operation(file: &std::path::Path, json_output: bool) {
    match read_metadata(file) {
        Ok(metadata) => {
            // Check if any metadata was found
            if metadata.is_empty() {
                println!("No metadata found in file: {}", file.display());
                return;
            }

            // Output based on requested format using formatters
            if json_output {
                // JSON output format
                let formatter = JsonFormatter;
                let output = formatter.format(&metadata, None);
                println!("{}", output);
            } else {
                // Human-readable output format
                println!("File: {}", file.display());
                println!("Found {} metadata tag(s):", metadata.len());
                println!();

                // Use HumanReadableFormatter
                let formatter = HumanReadableFormatter;
                let output = formatter.format(&metadata, None);
                print!("{}", output);
            }
        }
        Err(e) => {
            eprintln!(
                "Error: Failed to read metadata from '{}': {}",
                file.display(),
                e
            );
            process::exit(1);
        }
    }
}

/// Handles batch processing (multiple files or directories)
fn handle_batch_processing(path: &std::path::Path, args: &CliArgs) {
    match batch_processor::batch_process(path, args) {
        Ok(stats) => {
            // Print statistics
            stats.print();

            // Exit with error code if there were any errors
            if stats.errors > 0 {
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error: Batch processing failed: {}", e);
            process::exit(1);
        }
    }
}

/// Handles copy operations (copying metadata from one file to another)
fn handle_copy_operation(dest_file: &std::path::Path, args: &CliArgs) {
    // Extract source file path from tags_from_file option
    let src_file = match &args.tags_from_file {
        Some(path) => std::path::PathBuf::from(path),
        None => {
            eprintln!("Error: No source file specified for -TagsFromFile");
            process::exit(1);
        }
    };

    // Check readonly flag FIRST - if set, prevent any writes
    if args.readonly {
        eprintln!("Error: Cannot copy metadata in read-only mode (--readonly flag set)");
        process::exit(1);
    }

    // Verify source file exists
    if !src_file.exists() {
        eprintln!("Error: Source file not found: {}", src_file.display());
        process::exit(1);
    }

    // Verify destination file exists
    if !dest_file.exists() {
        eprintln!("Error: Destination file not found: {}", dest_file.display());
        process::exit(1);
    }

    // Check if destination file is writable
    let file_metadata = match std::fs::metadata(dest_file) {
        Ok(metadata) => {
            if metadata.permissions().readonly() {
                eprintln!("Error: Destination file is read-only: {}", dest_file.display());
                process::exit(1);
            }
            metadata
        }
        Err(e) => {
            eprintln!(
                "Error: Cannot access destination file '{}': {}",
                dest_file.display(),
                e
            );
            process::exit(1);
        }
    };

    // Save original modification time if preserve_file_times is enabled
    let original_mtime = if args.preserve_file_times {
        match file_metadata.modified() {
            Ok(mtime) => Some(mtime),
            Err(e) => {
                eprintln!("Warning: Could not read file modification time: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Create backup if requested
    if args.backup {
        // Create backup by appending .bak to the original filename
        let mut backup_path = dest_file.as_os_str().to_owned();
        backup_path.push(".bak");
        let backup_path = std::path::PathBuf::from(backup_path);

        if let Err(e) = std::fs::copy(dest_file, &backup_path) {
            eprintln!(
                "Error: Failed to create backup file '{}': {}",
                backup_path.display(),
                e
            );
            process::exit(1);
        }
    }

    // Extract tag filters (if specified)
    let tag_filters = args.copy_tag_filters();
    let tags_to_copy = match tag_filters {
        Some(filters) if !filters.is_empty() => Some(filters),
        _ => None, // Copy all tags
    };

    // Perform the copy operation
    if let Err(e) = copy_metadata(
        &src_file,
        dest_file,
        tags_to_copy.as_deref(),
    ) {
        eprintln!(
            "Error: Failed to copy metadata from '{}' to '{}': {}",
            src_file.display(),
            dest_file.display(),
            e
        );
        process::exit(1);
    }

    // Restore original modification time if requested
    if let Some(mtime) = original_mtime {
        use std::fs::File;
        match File::open(dest_file).and_then(|f| f.set_modified(mtime)) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Warning: Could not restore file modification time: {}", e);
                // Don't exit - the copy succeeded, only mtime restoration failed
            }
        }
    }

    // Print success message (matching ExifTool format)
    if tags_to_copy.is_some() {
        println!(
            "    1 image files updated ({} tags copied)",
            tags_to_copy.as_ref().unwrap().len()
        );
    } else {
        println!("    1 image files updated");
    }
}
