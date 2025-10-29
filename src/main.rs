//! ExifTool-RS Command Line Interface
//!
//! This is the main entry point for the exiftool-rs command-line application.

use clap::Parser;
use exiftool_rs::cli::args::CliArgs;
use exiftool_rs::cli::output_formatter::{HumanReadableFormatter, JsonFormatter, OutputFormatter};
use exiftool_rs::core::operations::read_metadata;
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

    // Call read_metadata from core operations module
    match read_metadata(&args.file) {
        Ok(metadata) => {
            // Check if any metadata was found
            if metadata.is_empty() {
                println!("No metadata found in file: {}", args.file.display());
                return;
            }

            // Output based on requested format using formatters
            if args.json {
                // JSON output format
                let formatter = JsonFormatter;
                let output = formatter.format(&metadata, None);
                println!("{}", output);
            } else {
                // Human-readable output format
                println!("File: {}", args.file.display());
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
                args.file.display(),
                e
            );
            process::exit(1);
        }
    }
}
