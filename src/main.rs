//! ExifTool-RS Command Line Interface
//!
//! This is the main entry point for the exiftool-rs command-line application.

use clap::Parser;
use exiftool_rs::cli::args::CliArgs;
use exiftool_rs::cli::output_formatter::{HumanReadableFormatter, JsonFormatter, OutputFormatter};
use exiftool_rs::core::operations::{modify_tag, read_metadata};
use exiftool_rs::core::tag_value::TagValue;
use std::process;

fn main() {
    // Parse command-line arguments using clap
    let args = CliArgs::parse();

    // Display warning for unimplemented features
    if args.recursive {
        eprintln!("Warning: Recursive directory processing (-r) is not yet implemented");
    }
    if args.short_format {
        eprintln!("Warning: Short format output (-s) is not yet fully implemented");
    }

    // Extract file path from arguments
    let file = match args.file() {
        Some(path) => path,
        None => {
            eprintln!("Error: No file specified");
            eprintln!("Usage: exiftool-rs [OPTIONS] [-TAG=VALUE ...] FILE");
            process::exit(1);
        }
    };

    // Check if this is a write operation (tag modifications present)
    let modifications = args.tag_modifications();

    if !modifications.is_empty() {
        // Write mode: modify tags
        handle_write_operation(&file, &modifications);
    } else {
        // Read mode: display metadata
        handle_read_operation(&file, args.json);
    }
}

/// Handles write operations (tag modifications)
fn handle_write_operation(file: &std::path::Path, modifications: &[(String, String)]) {
    // Verify file exists
    if !file.exists() {
        eprintln!("Error: File not found: {}", file.display());
        process::exit(1);
    }

    // Check if file is writable
    match std::fs::metadata(file) {
        Ok(metadata) => {
            if metadata.permissions().readonly() {
                eprintln!("Error: File is read-only: {}", file.display());
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error: Cannot access file '{}': {}", file.display(), e);
            process::exit(1);
        }
    }

    // Apply each modification
    for (tag_name, value) in modifications {
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
