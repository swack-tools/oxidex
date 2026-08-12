//! OxiDex Command Line Interface
//!
//! Main entry point for the oxidex command-line application.

use oxidex::cli::args::CliArgs;
use oxidex::cli::batch_processor;
use oxidex::cli::output_formatter::{
    CsvFormatter, HumanReadableFormatter, JsonFormatter, OutputFormatter, ShortFormatter,
};
use oxidex::cli::rename;
use oxidex::cli::value_parser::parse_cli_tag_value;
use oxidex::core::date_shift::{ShiftOperation, shift_metadata_dates};
use oxidex::core::operations::{
    clear_all_metadata, copy_metadata, modify_tag, read_metadata_report_with_detector_and_options,
    remove_tag,
};
use oxidex::core::read_report::ParseStatus;
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
        // args.file() only ever returns the *last* positional argument, so a
        // plain-read invocation with more than one path -- `oxidex -j a.jpg
        // b.jpg` -- fell through this whole dispatch chain looking at "b.jpg"
        // alone: "a.jpg" was silently dropped, and every output formatter
        // (JSON/CSV/human) emitted a single result with no SourceFile,
        // rather than one result per input file. Route explicit multi-file
        // invocations through the same batch machinery a directory uses,
        // which already emits one tagged result per file.
        let files = args.files();

        if files.len() > 1 {
            handle_multi_file_processing(&files, &args);
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
}

/// Handles multiple explicit file arguments (e.g. `oxidex -j a.jpg b.jpg`).
///
/// Unlike `handle_batch_processing`, this does not walk a directory or
/// filter by extension -- the files were named explicitly on the command
/// line, so every one of them is processed as given.
fn handle_multi_file_processing(files: &[std::path::PathBuf], args: &CliArgs) {
    let modifications = args.tag_modifications();
    let result = if !modifications.is_empty() {
        batch_processor::batch_write(files.to_vec(), &modifications, args)
    } else {
        batch_processor::batch_read(files.to_vec(), args)
    };

    match result {
        Ok(stats) => {
            let is_read_mode = modifications.is_empty();
            if !(is_read_mode && (args.json || args.csv || args.short_format)) {
                stats.print();
            }

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
            // Non-empty value = modify tag, typed as the tag's registry entry
            // declares. Wrapping every value as a String here is what made
            // Integer/Rational/DateTime tags unsettable from the CLI.
            let tag_value = match parse_cli_tag_value(tag_name, value) {
                Ok(tag_value) => tag_value,
                Err(e) => {
                    eprintln!("Error: Invalid value for {}: {}", tag_name, e);
                    process::exit(1);
                }
            };

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
    // Step 21: build request-awareness (`ReadOptions`) from the specific
    // tags requested and `--extended-output` *before* reading, so a
    // specifically-requested `-JPEGQualityEstimate` (or `--extended-output`)
    // actually reaches the JPEG parser instead of being computed
    // unconditionally or not at all. See `core::read_options`.
    let tag_filter = args.specific_tags();
    let read_options =
        oxidex::core::ReadOptions::new(tag_filter.as_deref().unwrap_or(&[]), args.extended_output);
    match read_metadata_report_with_detector_and_options(file, args.detector, &read_options) {
        Ok(report) => {
            // `--strict` opts back into the old fail-fast behavior: a read
            // that didn't fully parse is an error, not partial output. See
            // `CliArgs::strict` and `read_metadata_report_with_detector`'s
            // doc comment for why the default is otherwise to degrade
            // gracefully (ExifTool.pm:8483's `Warn('JPEG format error')`
            // model) rather than discard the whole read.
            if args.strict
                && !matches!(
                    report.status,
                    ParseStatus::Parsed | ParseStatus::IdentifiedOnly
                )
            {
                let reason = report
                    .diagnostics
                    .first()
                    .map(|d| d.message.clone())
                    .unwrap_or_else(|| report.status.to_string());
                eprintln!(
                    "Error: Failed to read metadata from '{}': {} (status: {})",
                    file.display(),
                    reason,
                    report.status
                );
                process::exit(1);
            }

            let status = report.status;
            let raw_metadata = report.metadata;

            // Check if any metadata was found
            if raw_metadata.is_empty() {
                println!("No metadata found in file: {}", file.display());
                return;
            }

            // A specific `-TAG` request goes through Step 20's group/priority-
            // aware resolution (`cli::tag_resolution::resolve_requested_tags`)
            // instead of the exact/suffix match `tag_matches_filter` used to
            // apply; the unfiltered default listing applies Step 21's
            // extended-namespace filter. See
            // `cli::tag_resolution::resolve_file_output`'s doc comment for
            // the full rationale, including why `-Gn` + human/short output
            // is rendered directly rather than through a formatter.
            //
            // Shared with batch mode (`cli::batch_processor`) via
            // `cli::tag_resolution::resolve_file_output`, so the two CLI
            // paths agree on `-a`/`-G*`/`--no-print-conv`/the extended
            // namespace for the same file (Step 21 closes the Step 20 gap
            // where batch bypassed this module entirely). `show_header`
            // (the human-readable `"File: ..."`/`"Found N ..."` preamble)
            // only ever applies to the unfiltered full listing.
            match oxidex::cli::tag_resolution::resolve_file_output(&raw_metadata, args) {
                oxidex::cli::tag_resolution::ResolvedFileOutput::Lines(output) => {
                    print!("{}", output);
                }
                oxidex::cli::tag_resolution::ResolvedFileOutput::Metadata(metadata) => {
                    print_resolved_metadata(file, &metadata, args, status, tag_filter.is_none());
                }
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

/// Dispatches an already display-ready `metadata` (PrintConv applied or
/// not, filtered to exactly what should show) to whichever formatter
/// `args` selects. Shared tail of [`handle_read_operation`]'s two paths --
/// a resolved specific-tag request and the full-listing default -- which
/// differ only in how `metadata` was built, not in how it is printed.
/// `show_header` gates the human-readable `"File: ..." / "Found N ..."`
/// preamble, which only ever applied to the unfiltered full listing.
fn print_resolved_metadata(
    file: &std::path::Path,
    metadata: &oxidex::core::MetadataMap,
    args: &CliArgs,
    status: ParseStatus,
    show_header: bool,
) {
    if args.csv {
        let formatter = CsvFormatter;
        let output = formatter.format(metadata, None);
        print!("{}", output);
    } else if args.json {
        // Carries `Status` for any read that didn't fully parse (see
        // `JsonFormatter::format_with_status`); identical to plain `format`
        // for a healthy `Parsed` read.
        let formatter = JsonFormatter;
        let output = formatter.format_with_status(metadata, None, Some(status));
        println!("{}", output);
    } else if args.short_format {
        let formatter = ShortFormatter;
        let output = formatter.format(metadata, None);
        print!("{}", output);
    } else {
        if show_header {
            println!("File: {}", file.display());
            println!("Found {} metadata tag(s):", metadata.len());
            println!();
        }
        let formatter = HumanReadableFormatter;
        let output = formatter.format(metadata, None);
        print!("{}", output);
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
