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

    /// Output in CSV format
    #[arg(long)]
    pub csv: bool,

    /// Short output format (not yet fully implemented)
    #[arg(short = 's')]
    pub short_format: bool,

    /// Display all tags (default behavior, currently has no effect)
    #[arg(short = 'a')]
    pub all_tags: bool,

    /// Recursive directory processing (placeholder - not yet implemented)
    #[arg(short = 'r')]
    pub recursive: bool,

    /// Preserve original file modification time after writing metadata.
    /// When this flag is set, the file's modification timestamp (mtime) will be
    /// restored to its original value after metadata changes are written.
    #[arg(long)]
    pub preserve_file_times: bool,

    /// Create a backup copy of the file before modifying it.
    /// The backup file will have the same name with a .bak extension appended.
    /// For example: photo.jpg -> photo.jpg.bak
    #[arg(long)]
    pub backup: bool,

    /// Enable read-only mode to prevent any file modifications.
    /// When this flag is set, the tool will refuse to write any changes and
    /// return an error if write operations are attempted. Use this as a safety
    /// measure to prevent accidental modifications.
    #[arg(long)]
    pub readonly: bool,

    /// Copy metadata from source file (ExifTool -TagsFromFile syntax).
    /// Use with optional tag names to copy specific tags, or without to copy all tags.
    /// Example: exiftool-rs -TagsFromFile src.jpg dest.jpg (copy all)
    /// Example: exiftool-rs -TagsFromFile src.jpg -EXIF:Artist -EXIF:Copyright dest.jpg
    #[arg(long = "TagsFromFile")]
    pub tags_from_file: Option<String>,

    /// Date format string for DateTime tags in filename patterns (using chrono format).
    /// Example: -d %Y%m%d_%H%M%S
    /// Common specifiers: %Y (year), %m (month), %d (day), %H (hour), %M (minute), %S (second)
    #[arg(short = 'd')]
    pub date_format: Option<String>,

    /// Dry-run mode: show proposed renames without executing.
    /// Prints "old_name -> new_name" for each file without actually renaming.
    #[arg(short = 'n')]
    pub dry_run: bool,

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

    /// Extracts tag names to copy when using -TagsFromFile.
    /// Returns None if -TagsFromFile is not set.
    /// Returns Some(Vec) of tag names if tags are specified (args starting with '-' but not '=').
    /// Returns Some(empty Vec) if no specific tags are specified (copy all).
    pub fn copy_tag_filters(&self) -> Option<Vec<String>> {
        // If -TagsFromFile is not set, return None
        self.tags_from_file.as_ref()?;

        // If no additional args (only destination file), copy all tags
        if self.args.len() <= 1 {
            return Some(Vec::new());
        }

        let mut tag_names = Vec::new();

        // Process all arguments except the last one (which is the destination file)
        for arg in &self.args[..self.args.len() - 1] {
            // Check if it's a tag name (starts with '-' but does NOT contain '=')
            if arg.starts_with('-') && !arg.contains('=') {
                // Extract tag name (remove leading '-')
                let tag_name = arg.trim_start_matches('-').to_string();
                tag_names.push(tag_name);
            }
        }

        // Return empty vec if no tags specified (means copy all)
        // Return vec with tag names if specific tags were specified
        Some(tag_names)
    }

    /// Extracts the filename pattern from -FileName<pattern> argument.
    /// Returns None if no -FileName argument is found.
    /// Returns Some(pattern) with the pattern after the '<' character.
    ///
    /// Example: '-FileName<DateTimeOriginal' -> Some("DateTimeOriginal")
    /// Example: '-FileName<${EXIF:Make}_${EXIF:Model}' -> Some("${EXIF:Make}_${EXIF:Model}")
    pub fn filename_pattern(&self) -> Option<String> {
        for arg in &self.args {
            // Check if this is a -FileName argument
            if arg.starts_with("-FileName") || arg.starts_with("'FileName") {
                // Find the '<' character that separates -FileName from the pattern
                if let Some(pos) = arg.find('<') {
                    // Extract everything after '<'
                    let pattern = &arg[pos + 1..];
                    // Remove trailing quote if present (from '-FileName<pattern')
                    let pattern = pattern.trim_end_matches('\'');
                    return Some(pattern.to_string());
                }
            }
        }
        None
    }

    /// Parses date shift arguments (e.g., "-AllDates+=1:0:0 0:0:0" or "-EXIF:DateTime-=0:1:0 0:0:0")
    /// Returns a vector of (tag_pattern, operation, offset_or_value) tuples
    ///
    /// # Format
    ///
    /// Date shift arguments follow the format: `-TAG_PATTERN{+= | -= | =}OFFSET`
    /// - TAG_PATTERN: "AllDates" or specific tag name (e.g., "EXIF:DateTime")
    /// - Operation: `+=` (add), `-=` (subtract), `=` (set absolute)
    /// - OFFSET: For += and -=: "Y:M:D H:M:S" format
    ///   For =: "YYYY:MM:DD HH:MM:SS" absolute datetime format
    ///
    /// # Examples
    ///
    /// - `-AllDates+=1:0:0 0:0:0` -> Add 1 year to all date tags
    /// - `-EXIF:DateTime-=0:1:0 0:0:0` -> Subtract 1 month from DateTime
    /// - `-EXIF:DateTime=2025:01:15 10:30:00` -> Set DateTime to specific value
    pub fn date_shift_operations(&self) -> Vec<(String, String, String)> {
        if self.args.len() <= 1 {
            return Vec::new();
        }

        let mut operations = Vec::new();

        // Process all arguments except the last one (which is the file)
        for arg in &self.args[..self.args.len() - 1] {
            if let Some((tag, op, value)) = Self::parse_date_shift(arg) {
                operations.push((tag, op, value));
            }
        }

        operations
    }

    /// Parses a single date shift argument
    /// Returns (tag_pattern, operation, offset_or_value) or None if not a date shift argument
    ///
    /// Supports three operation types:
    /// - `+=`: Add offset (e.g., "-AllDates+=1:0:0 0:0:0")
    /// - `-=`: Subtract offset (e.g., "-EXIF:DateTime-=0:1:0 0:0:0")
    /// - `=`: Set absolute (e.g., "-EXIF:DateTime=2025:01:15 10:30:00")
    fn parse_date_shift(arg: &str) -> Option<(String, String, String)> {
        // Date shift args must start with '-'
        if !arg.starts_with('-') {
            return None;
        }

        // Check for += operator first (must check before single =)
        if let Some(pos) = arg.find("+=") {
            let tag = arg[1..pos].to_string(); // Remove leading '-'
            let value = arg[pos + 2..].to_string();
            return Some((tag, "+=".to_string(), value));
        }

        // Check for -= operator
        if let Some(pos) = arg.find("-=") {
            let tag = arg[1..pos].to_string();
            let value = arg[pos + 2..].to_string();
            return Some((tag, "-=".to_string(), value));
        }

        // Check for = operator (but not if it's part of += or -=)
        // Also need to distinguish from regular tag modifications
        if let Some(pos) = arg.find('=') {
            let tag = arg[1..pos].to_string();
            let value = arg[pos + 1..].to_string();

            // Check if this looks like a date shift operation
            // Date shifts should have either:
            // - "AllDates" as the tag pattern (case-insensitive)
            // - A tag containing a date-related keyword (DateTime, Date, CreateDate, etc.)
            // - A value in date format (contains colons and spaces like "Y:M:D H:M:S" or "YYYY:MM:DD HH:MM:SS")

            let tag_lower = tag.to_lowercase();
            let is_date_tag = tag_lower == "alldates"
                || tag_lower.contains("date")
                || tag_lower.contains("time");

            let is_date_value = value.contains(':') && value.contains(' ');

            // Only treat as date shift if both tag and value look date-related
            if is_date_tag && is_date_value {
                return Some((tag, "=".to_string(), value));
            }
        }

        None
    }
}
