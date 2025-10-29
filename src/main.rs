//! ExifTool-RS Command Line Interface
//!
//! This is the main entry point for the exiftool-rs command-line application.

use clap::Parser;
use exiftool_rs::cli::args::CliArgs;
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

            // Output based on requested format
            if args.json {
                // JSON output format
                match serde_json::to_string_pretty(&metadata) {
                    Ok(json) => println!("{}", json),
                    Err(e) => {
                        eprintln!("Error: Failed to serialize metadata to JSON: {}", e);
                        process::exit(1);
                    }
                }
            } else {
                // Human-readable output format
                println!("File: {}", args.file.display());
                println!("Found {} metadata tag(s):", metadata.len());
                println!();

                // Display each tag with its value
                let mut tags: Vec<_> = metadata.iter().collect();
                tags.sort_by_key(|(name, _)| *name);

                for (tag_name, tag_value) in tags {
                    println!("  {}: {:?}", tag_name, tag_value);
                }
            }
        }
        Err(e) => {
            eprintln!("Error: Failed to read metadata from '{}': {}", args.file.display(), e);
            process::exit(1);
        }
    }
}
