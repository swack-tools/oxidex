//! OxiDex Command Line Interface
//!
//! Main entry point for the oxidex command-line application.

use oxidex::cli::args::CliArgs;
use oxidex::cli::batch_processor;
use oxidex::cli::output_formatter::{
    CsvFormatter, HumanReadableFormatter, JsonFormatter, OutputFormatter, ShortFormatter,
};
use oxidex::cli::rename;
use oxidex::core::date_shift::{ShiftOperation, shift_metadata_dates};
use oxidex::core::exiftool_compat::format_for_exiftool;
use oxidex::core::operations::{
    clear_all_metadata, copy_metadata, modify_tag, read_metadata, remove_tag,
};
use oxidex::core::tag_value::TagValue;
use std::process;

fn main() {
    // Parse command-line arguments after normalizing supported ExifTool-style options.
    let args = match CliArgs::parse() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    // Extract file path from arguments
    let file = match args.file() {
        Some(path) => path,
        None => {
            eprintln!("Error: No file or directory specified");
            eprintln!("Usage: oxidex [OPTIONS] [-TAG=VALUE ...] FILE|DIRECTORY");
            process::exit(1);
        }
    };

    // Check if this is a clear all metadata operation (-all=)
    if args.is_clear_all_metadata() {
        handle_clear_all_operation(&file, &args);
        return;
    }

    // Check if this is a date shift operation
    let date_shifts = args.date_shift_operations();
    if !date_shifts.is_empty() {
        // Date shift mode
        handle_date_shift_operation(&file, &args);
    } else if let Some(pattern) = args.filename_pattern() {
        // Rename mode
        handle_rename_operation(&file, &pattern, &args);
    } else if args.tags_from_file.is_some() {
        // Copy metadata mode
        handle_copy_operation(&file, &args);
    } else if file.is_dir() {
        // Batch processing mode (directory)
        handle_batch_processing(&file, &args);
    } else {
        // Single file processing mode
        let modifications = args.tag_modifications();

        if !modifications.is_empty() {
            // Write mode: modify tags
            handle_write_operation(&file, &args);
        } else {
            // Read mode: display metadata
            handle_read_operation(&file, &args);
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
        if value.is_empty() {
            // Empty value = delete tag (ExifTool -TAG= syntax)
            if let Err(e) = remove_tag(file, tag_name) {
                eprintln!("Error: Failed to remove tag '{}': {}", tag_name, e);
                process::exit(1);
            }
        } else {
            // Non-empty value = modify tag
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
fn handle_read_operation(file: &std::path::Path, args: &CliArgs) {
    match read_metadata(file) {
        Ok(metadata) => {
            // Check if any metadata was found
            if metadata.is_empty() {
                println!("No metadata found in file: {}", file.display());
                return;
            }

            // Apply ExifTool-compatible formatting if requested
            let metadata = if args.exiftool_compat() {
                format_for_exiftool(&metadata)
            } else {
                metadata
            };

            // Get specific tags filter if provided (e.g., oxidex -Make -Model photo.jpg)
            let tag_filter = args.specific_tags();
            let filter_slice = tag_filter.as_deref();

            // Output based on requested format using formatters
            // Check CSV first, then JSON, then short, then default to human-readable
            if args.csv {
                // CSV output format
                let formatter = CsvFormatter;
                let output = formatter.format(&metadata, filter_slice);
                print!("{}", output);
            } else if args.json {
                // JSON output format
                let formatter = JsonFormatter;
                let output = formatter.format(&metadata, filter_slice);
                println!("{}", output);
            } else if args.short_format {
                // Short format output (-s flag)
                let formatter = ShortFormatter;
                let output = formatter.format(&metadata, filter_slice);
                print!("{}", output);
            } else {
                // Human-readable output format
                if filter_slice.is_none() {
                    // Only show header when showing all tags
                    println!("File: {}", file.display());
                    println!("Found {} metadata tag(s):", metadata.len());
                    println!();
                }

                // Use HumanReadableFormatter
                let formatter = HumanReadableFormatter;
                let output = formatter.format(&metadata, filter_slice);
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
            let is_read_mode = args.tag_modifications().is_empty();
            if !(is_read_mode && (args.json || args.csv || args.short_format)) {
                stats.print();
            }

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
                eprintln!(
                    "Error: Destination file is read-only: {}",
                    dest_file.display()
                );
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
    if let Err(e) = copy_metadata(&src_file, dest_file, tags_to_copy.as_deref()) {
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
    if let Some(tags) = tags_to_copy.as_ref() {
        println!("    1 image files updated ({} tags copied)", tags.len());
    } else {
        println!("    1 image files updated");
    }
}

/// Handles rename operations (renaming files based on metadata)
fn handle_rename_operation(file: &std::path::Path, pattern: &str, args: &CliArgs) {
    // Verify file exists
    if !file.exists() {
        eprintln!("Error: File not found: {}", file.display());
        process::exit(1);
    }

    // If it's a directory, we could support batch rename in the future
    // For now, only support single files
    if file.is_dir() {
        eprintln!("Error: Directory renaming not yet supported");
        eprintln!("Please specify a single file for renaming");
        process::exit(1);
    }

    // Extract date format if provided
    let date_format = args.date_format.as_deref();

    // Perform rename
    match rename::rename_file(file, pattern, date_format, args.dry_run) {
        Ok(_new_path) => {
            if !args.dry_run {
                // Print success message
                println!("    1 image files renamed");
            }
        }
        Err(e) => {
            eprintln!("Error: Failed to rename file '{}': {}", file.display(), e);
            process::exit(1);
        }
    }
}

/// Handles date shift operations (shifting date/time tags by offset)
fn handle_date_shift_operation(file: &std::path::Path, args: &CliArgs) {
    // Extract date shift operations
    let date_shifts = args.date_shift_operations();

    // Check readonly flag FIRST - if set, prevent any writes
    if args.readonly {
        eprintln!("Error: Cannot shift dates in read-only mode (--readonly flag set)");
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

    // Apply each date shift operation
    for (tag_pattern, op_str, offset_or_value) in &date_shifts {
        // Parse operation type
        let operation = match op_str.as_str() {
            "+=" => ShiftOperation::Add,
            "-=" => ShiftOperation::Subtract,
            "=" => ShiftOperation::Set,
            _ => {
                eprintln!("Error: Invalid date shift operation '{}'", op_str);
                eprintln!("Supported operations: +=, -=, =");
                process::exit(1);
            }
        };

        // Apply the date shift
        if let Err(e) = shift_metadata_dates(file, tag_pattern, offset_or_value, operation) {
            eprintln!("Error: Failed to shift dates for '{}': {}", tag_pattern, e);
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
                // Don't exit - the shift succeeded, only mtime restoration failed
            }
        }
    }

    // Print success message (matching ExifTool format)
    println!("    1 image files updated");
}

/// Handles clear all metadata operation (-all=)
fn handle_clear_all_operation(file: &std::path::Path, args: &CliArgs) {
    // Check readonly flag FIRST - if set, prevent any writes
    if args.readonly {
        eprintln!("Error: Cannot clear metadata in read-only mode (--readonly flag set)");
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

    // Clear all metadata
    if let Err(e) = clear_all_metadata(file) {
        eprintln!(
            "Error: Failed to clear metadata from '{}': {}",
            file.display(),
            e
        );
        process::exit(1);
    }

    // Restore original modification time if requested
    if let Some(mtime) = original_mtime {
        use std::fs::File;
        match File::open(file).and_then(|f| f.set_modified(mtime)) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Warning: Could not restore file modification time: {}", e);
            }
        }
    }

    // Print success message (matching ExifTool format)
    println!("    1 image files updated");
}
