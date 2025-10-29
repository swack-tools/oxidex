//! Command-line argument definitions using clap
//!
//! This module defines the CLI argument structure for the exiftool-rs application.

use clap::Parser;
use std::path::PathBuf;

/// A modern, high-performance Rust reimplementation of ExifTool
#[derive(Parser, Debug)]
#[command(name = "exiftool-rs")]
#[command(version, about, long_about = None)]
pub struct CliArgs {
    /// Input file to process
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    /// Output in JSON format
    #[arg(short, long)]
    pub json: bool,

    /// Short output format (not yet fully implemented)
    #[arg(short = 's')]
    pub short_format: bool,

    /// Display all tags (default behavior, currently has no effect)
    #[arg(short = 'a')]
    pub all_tags: bool,

    /// Recursive directory processing (placeholder - not yet implemented)
    #[arg(short = 'r')]
    pub recursive: bool,
}
