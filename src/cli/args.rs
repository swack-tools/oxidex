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

    /// Tag modifications and file path. Use -TAG=VALUE to modify tags.
    /// Example: -EXIF:Artist="John Doe" -EXIF:Copyright=2025 photo.jpg
    /// The last argument must be the file path.
    #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

impl CliArgs {
    /// Extracts the file path from the arguments (last argument)
    pub fn file(&self) -> Option<PathBuf> {
        self.args.last().map(PathBuf::from)
    }

    /// Parses tag modification arguments (all args except the last one)
    /// Returns a vector of (tag_name, value) tuples
    pub fn tag_modifications(&self) -> Vec<(String, String)> {
        if self.args.len() <= 1 {
            return Vec::new();
        }

        let mut modifications = Vec::new();
        // Process all arguments except the last one (which is the file)
        for arg in &self.args[..self.args.len() - 1] {
            if let Some((tag, value)) = Self::parse_modification(arg) {
                modifications.push((tag, value));
            }
        }
        modifications
    }

    /// Parses a single modification argument in the form -TAG=VALUE
    fn parse_modification(arg: &str) -> Option<(String, String)> {
        // Check if it starts with '-' and contains '='
        if !arg.starts_with('-') || !arg.contains('=') {
            return None;
        }

        // Split on first '=' to handle values that contain '='
        let parts: Vec<&str> = arg.splitn(2, '=').collect();
        if parts.len() != 2 {
            return None;
        }

        // Extract tag name (remove leading '-')
        let tag_name = parts[0].trim_start_matches('-').to_string();

        // Extract value and remove surrounding quotes if present
        let value = Self::unquote(parts[1]);

        Some((tag_name, value))
    }

    /// Removes surrounding quotes from a string if present
    fn unquote(s: &str) -> String {
        let trimmed = s.trim();
        if (trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        {
            trimmed[1..trimmed.len() - 1].to_string()
        } else {
            s.to_string()
        }
    }
}
