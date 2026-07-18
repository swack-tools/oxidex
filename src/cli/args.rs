//! Command-line argument definitions using lexopt
//!
//! This module defines the CLI argument structure for the oxidex application.

use lexopt::prelude::*;
use std::path::PathBuf;

/// A modern, high-performance Rust reimplementation of ExifTool
#[derive(Debug)]
pub struct CliArgs {
    /// Output in JSON format
    pub json: bool,

    /// Output in CSV format
    pub csv: bool,

    /// Short output format (not yet fully implemented)
    pub short_format: bool,

    /// Display all tags (default behavior, currently has no effect)
    pub all_tags: bool,

    /// Recursive directory processing
    pub recursive: bool,

    /// Preserve original file modification time after writing metadata.
    /// When this flag is set, the file's modification timestamp (mtime) will be
    /// restored to its original value after metadata changes are written.
    pub preserve_file_times: bool,

    /// Create a backup copy of the file before modifying it.
    /// The backup file will have the same name with a .bak extension appended.
    /// For example: photo.jpg -> photo.jpg.bak
    pub backup: bool,

    /// Enable read-only mode to prevent any file modifications.
    /// When this flag is set, the tool will refuse to write any changes and
    /// return an error if write operations are attempted. Use this as a safety
    /// measure to prevent accidental modifications.
    pub readonly: bool,

    /// Format output for ExifTool compatibility.
    /// When enabled, tag values are formatted to match ExifTool's output format,
    /// including enum descriptions, unit suffixes, and precision formatting.
    /// This is useful for comparing OxiDex output with ExifTool or for scripts
    /// that expect ExifTool-compatible output.
    pub exiftool_compat: bool,

    /// Copy metadata from source file (ExifTool -TagsFromFile syntax).
    /// Use with optional tag names to copy specific tags, or without to copy all tags.
    /// Example: oxidex -TagsFromFile src.jpg dest.jpg (copy all)
    /// Example: oxidex -TagsFromFile src.jpg -EXIF:Artist -EXIF:Copyright dest.jpg
    pub tags_from_file: Option<String>,

    /// Date format string for DateTime tags in filename patterns (using chrono format).
    /// Example: -d %Y%m%d_%H%M%S
    /// Common specifiers: %Y (year), %m (month), %d (day), %H (hour), %M (minute), %S (second)
    pub date_format: Option<String>,

    /// Dry-run mode: show proposed renames without executing.
    /// Prints "old_name -> new_name" for each file without actually renaming.
    pub dry_run: bool,

    /// Tag modifications and file path. Use -TAG=VALUE to modify tags.
    /// Example: -EXIF:Artist="John Doe" -EXIF:Copyright=2025 photo.jpg
    /// The last argument must be the file path.
    pub args: Vec<String>,
}

fn normalize_exiftool_option(arg: String) -> String {
    if let Some(value) = arg.strip_prefix("-TagsFromFile=") {
        return format!("--TagsFromFile={value}");
    }

    match arg.as_str() {
        "-json" => "--json".to_string(),
        "-csv" => "--csv".to_string(),
        "-preserve-file-times" => "--preserve-file-times".to_string(),
        "-backup" => "--backup".to_string(),
        "-readonly" => "--readonly".to_string(),
        "-exiftool-compat" => "--exiftool-compat".to_string(),
        "-TagsFromFile" => "--TagsFromFile".to_string(),
        _ => arg,
    }
}

fn is_flag_short_option(ch: char) -> bool {
    matches!(ch, 'h' | 'V' | 'j' | 's' | 'a' | 'r' | 'e' | 'n')
}

fn is_lexopt_short_arg(arg: &str) -> bool {
    let Some(body) = arg.strip_prefix('-') else {
        return false;
    };
    if body.is_empty() || body.starts_with('-') {
        return false;
    }
    // Assignment-shaped args are tag modifications (e.g. -description=x), never
    // short-option clusters, even when they start with a valid cluster prefix.
    if body.contains('=') {
        return false;
    }

    for (index, ch) in body.char_indices() {
        if ch == 'd' {
            return body[..index].chars().all(is_flag_short_option);
        }
        if !is_flag_short_option(ch) {
            return false;
        }
    }

    true
}

fn lexopt_arg_requires_next_value(arg: &str) -> bool {
    if arg == "--TagsFromFile" {
        return true;
    }

    let Some(body) = arg.strip_prefix('-') else {
        return false;
    };
    !body.starts_with('-')
        && body.ends_with('d')
        && body[..body.len() - 1].chars().all(is_flag_short_option)
}

impl CliArgs {
    /// Parse command-line arguments from the environment.
    ///
    /// This method uses lexopt to parse arguments in a way that's compatible with
    /// the original ExifTool syntax, including support for:
    /// - Single-dash long options (e.g., `-json` alongside `--json`)
    /// - Tag modification syntax (e.g., `-TAG=VALUE`)
    /// - Trailing variable arguments for files and tag modifications
    ///
    /// # Returns
    ///
    /// Returns `Ok(CliArgs)` if parsing succeeds, or `Err` if invalid arguments are provided.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - An unknown option is encountered that doesn't look like a tag modification
    /// - A required value for an option is missing
    /// - Help (`--help`, `-h`) or version (`--version`, `-V`) is requested (exits immediately)
    pub fn parse() -> Result<Self, lexopt::Error> {
        // Initialize with default values
        let mut json = false;
        let mut csv = false;
        let mut short_format = false;
        let mut all_tags = false;
        let mut recursive = false;
        let mut preserve_file_times = false;
        let mut backup = false;
        let mut readonly = false;
        let mut exiftool_compat = false;
        let mut tags_from_file = None;
        let mut date_format = None;
        let mut dry_run = false;
        let mut args = Vec::new();

        // Pre-process arguments to handle tag modifications that look like flags
        // e.g., "-EXIF:Artist=value" starts with '-' but isn't a regular flag
        let raw_args: Vec<String> = std::env::args().skip(1).collect();
        let mut lexopt_args = Vec::new();
        let mut tag_modifications = Vec::new();
        let mut next_arg_is_lexopt_value = false;

        for raw_arg in raw_args {
            if next_arg_is_lexopt_value {
                lexopt_args.push(raw_arg);
                next_arg_is_lexopt_value = false;
                continue;
            }

            let arg = normalize_exiftool_option(raw_arg);

            // Keep tag modifications, date shifts, and specific tag extraction out of lexopt.
            // Supported short options and clusters still flow through lexopt.
            if arg.starts_with('-')
                && !arg.starts_with("--")
                && !is_lexopt_short_arg(&arg)
                && (arg.contains('=')
                    || arg.ends_with("+=")
                    || arg.ends_with("-=")
                    || arg.contains(':')
                    || arg.len() > 1)
            {
                // This is a tag modification, date shift, or specific tag - don't pass to lexopt
                tag_modifications.push(arg);
            } else {
                next_arg_is_lexopt_value = lexopt_arg_requires_next_value(&arg);
                // Regular argument - pass to lexopt
                lexopt_args.push(arg);
            }
        }

        // Create parser from filtered arguments
        let mut parser = lexopt::Parser::from_args(lexopt_args);

        // Add pre-identified tag modifications to args list
        args.extend(tag_modifications);

        // Process each argument
        loop {
            // Handle parser.next() errors specially for tag modifications
            let arg = match parser.next() {
                Ok(Some(arg)) => arg,
                Ok(None) => break, // No more arguments
                Err(e) => {
                    // Handle lexopt errors - these might be tag modifications or date shifts
                    // that lexopt tries to parse as flags
                    let error_msg = e.to_string();
                    if let Some(arg_str) = extract_arg_from_error(&error_msg) {
                        args.push(arg_str);
                    } else {
                        // If we can't extract the argument, return the error
                        return Err(e);
                    }
                    // Collect remaining arguments
                    match parser.raw_args() {
                        Ok(raw) => {
                            for remaining_arg in raw {
                                if let Ok(s) = remaining_arg.string() {
                                    args.push(s);
                                }
                            }
                        }
                        Err(_) => {
                            // raw_args() can fail, but we already collected the main arg
                            // so we can continue
                        }
                    }
                    break;
                }
            };

            match arg {
                // Help flag
                Short('h') | Long("help") => {
                    print_help();
                    std::process::exit(0);
                }
                // Version flag
                Short('V') | Long("version") => {
                    print_version();
                    std::process::exit(0);
                }
                // JSON output
                Short('j') | Long("json") => {
                    json = true;
                }
                // CSV output
                Long("csv") => {
                    csv = true;
                }
                // Short format
                Short('s') => {
                    short_format = true;
                }
                // All tags
                Short('a') => {
                    all_tags = true;
                }
                // Recursive
                Short('r') => {
                    recursive = true;
                }
                // Preserve file times
                Long("preserve-file-times") => {
                    preserve_file_times = true;
                }
                // Backup
                Long("backup") => {
                    backup = true;
                }
                // Readonly
                Long("readonly") => {
                    readonly = true;
                }
                // ExifTool compatibility mode
                Short('e') | Long("exiftool-compat") => {
                    exiftool_compat = true;
                }
                // TagsFromFile (copy metadata from source file)
                Long("TagsFromFile") => {
                    tags_from_file = Some(parser.value()?.string()?);
                }
                // Date format
                Short('d') => {
                    date_format = Some(parser.value()?.string()?);
                }
                // Dry-run
                Short('n') => {
                    dry_run = true;
                }
                // Value argument (file path or positional argument)
                Value(val) => {
                    args.push(val.string()?);
                }
                // Unknown short or long option
                // This could be a tag modification like -EXIF:Artist=value
                // or a date shift operation like -AllDates+=1:0:0
                // Collect it as a trailing argument by accessing the raw value
                Short(_) | Long(_) => {
                    // Get the raw argument by using parser.raw_args()
                    // Since we can't go back, we need to handle this differently
                    // We'll use the unexpected error to extract the option

                    // For unknown options, we want to collect them as tag modifications
                    // This is a bit tricky with lexopt, so we need to handle it specially
                    // The arg.unexpected() will give us an error, but we want to collect
                    // the raw string instead

                    // Unfortunately, lexopt doesn't give us direct access to the raw string
                    // in the error case, so we need a different approach
                    // We'll collect remaining arguments using raw_args()

                    // Collect all remaining raw arguments (including this one)
                    // First, we need to reconstruct the current argument
                    let current_arg = format!("{}", arg.unexpected());

                    // Extract the actual argument from the error message
                    // Error format is typically "unexpected argument '--option'"
                    // or "unexpected option '-o'"
                    if let Some(arg_str) = extract_arg_from_error(&current_arg) {
                        args.push(arg_str);
                    }

                    // Collect all remaining arguments
                    for remaining_arg in parser.raw_args()? {
                        args.push(remaining_arg.string()?);
                    }

                    // Break out of the loop since we've consumed all arguments
                    break;
                }
            }
        }

        Ok(CliArgs {
            json,
            csv,
            short_format,
            all_tags,
            recursive,
            preserve_file_times,
            backup,
            readonly,
            exiftool_compat,
            tags_from_file,
            date_format,
            dry_run,
            args,
        })
    }

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

    /// Extracts specific tag names to display when reading metadata.
    /// Returns None if no specific tags are requested (show all tags).
    /// Returns Some(Vec) of tag names if specific tags are requested.
    ///
    /// This enables `-TAG` syntax for filtering output:
    /// - `oxidex -Make photo.jpg` → shows only Make tag
    /// - `oxidex -Make -Model photo.jpg` → shows Make and Model tags
    ///
    /// # Examples
    ///
    /// ```
    /// # use oxidex::cli::args::CliArgs;
    /// // If args are: ["-Make", "-Model", "photo.jpg"]
    /// // Returns: Some(vec!["Make", "Model"])
    /// ```
    pub fn specific_tags(&self) -> Option<Vec<String>> {
        // Don't apply in copy mode (tags_from_file handles its own filtering)
        if self.tags_from_file.is_some() {
            return None;
        }

        // Don't apply in write mode (has tag modifications with '=')
        let has_modifications = self.args.iter().any(|arg| arg.contains('='));
        if has_modifications {
            return None;
        }

        // If only file argument present, show all tags
        if self.args.len() <= 1 {
            return None;
        }

        let mut tag_names = Vec::new();

        // Process all arguments except the last one (file path)
        for arg in &self.args[..self.args.len() - 1] {
            // Tag extraction: starts with '-', does NOT contain '='
            if arg.starts_with('-') && !arg.contains('=') {
                let tag_name = arg.trim_start_matches('-').to_string();
                tag_names.push(tag_name);
            }
        }

        if tag_names.is_empty() {
            None
        } else {
            Some(tag_names)
        }
    }

    /// Checks if the user wants to clear all metadata (`-all=` syntax).
    ///
    /// This implements the ExifTool `-all=` command for removing all metadata
    /// from a file for privacy purposes.
    ///
    /// # Examples
    ///
    /// - `oxidex -all= photo.jpg` → clears all metadata
    /// - `oxidex -ALL= photo.jpg` → clears all metadata (case-insensitive)
    pub fn is_clear_all_metadata(&self) -> bool {
        self.args.iter().any(|arg| {
            let lower = arg.to_lowercase();
            lower == "-all=" || lower == "--all="
        })
    }

    /// Returns whether ExifTool compatibility mode is enabled.
    ///
    /// When enabled, tag values are formatted to match ExifTool's output format,
    /// including enum descriptions, unit suffixes, and precision formatting.
    ///
    /// # Examples
    ///
    /// - `oxidex -e photo.jpg` → ExifTool-compatible output
    /// - `oxidex --exiftool-compat photo.jpg` → ExifTool-compatible output
    pub fn exiftool_compat(&self) -> bool {
        self.exiftool_compat
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
            let is_date_tag =
                tag_lower == "alldates" || tag_lower.contains("date") || tag_lower.contains("time");

            let is_date_value = value.contains(':') && value.contains(' ');

            // Only treat as date shift if both tag and value look date-related
            if is_date_tag && is_date_value {
                return Some((tag, "=".to_string(), value));
            }
        }

        None
    }
}

/// Helper function to extract the actual argument from a lexopt error message
///
/// Lexopt error messages have the format: "unexpected argument '--option'" or "unexpected option '-o'"
/// or "unexpected argument for option '-E': \"XIF:Artist=TestValue\"" (when a tag looks like a flag)
/// This function extracts the actual argument string from the error message.
///
/// # Arguments
///
/// * `error_msg` - The error message from lexopt
///
/// # Returns
///
/// The extracted argument string, or the original string if parsing fails
fn extract_arg_from_error(error_msg: &str) -> Option<String> {
    // Handle the "unexpected argument for option '-X': \"value\"" format
    // This occurs when lexopt parses "-EXIF:Artist=value" as "-E" with unexpected value
    if error_msg.contains("unexpected argument for option") {
        // Extract the option part (e.g., '-E')
        if let Some(start) = error_msg.find('\'')
            && let Some(end) = error_msg[start + 1..].find('\'')
        {
            let option = &error_msg[start + 1..start + 1 + end];

            // Extract the value part (after the colon and space, between quotes)
            if let Some(value_start) = error_msg.find(": \"")
                && let Some(value_end) = error_msg[value_start + 3..].find('"')
            {
                let value = &error_msg[value_start + 3..value_start + 3 + value_end];
                // Reconstruct the full argument by combining option and value
                // e.g., '-E' + 'XIF:Artist=value' = '-EXIF:Artist=value'
                return Some(format!("{}{}", option, value));
            }
        }
    }

    // Try to find quoted text in the error message
    if let Some(start) = error_msg.find('\'')
        && let Some(end) = error_msg[start + 1..].find('\'')
    {
        return Some(error_msg[start + 1..start + 1 + end].to_string());
    }

    // Try double quotes as fallback
    if let Some(start) = error_msg.find('"')
        && let Some(end) = error_msg[start + 1..].find('"')
    {
        return Some(error_msg[start + 1..start + 1 + end].to_string());
    }

    None
}

/// Prints help text for the CLI application
///
/// This function displays comprehensive usage information including:
/// - Application description
/// - Usage syntax
/// - Available options with short and long forms
/// - Examples of common use cases
fn print_help() {
    println!("oxidex {}", env!("CARGO_PKG_VERSION"));
    println!("A modern, high-performance Rust reimplementation of ExifTool");
    println!();
    println!("USAGE:");
    println!("    oxidex [OPTIONS] [-TAG=VALUE ...] FILE|DIRECTORY");
    println!();
    println!("OPTIONS:");
    println!("    -h, --help                  Print help information");
    println!("    -V, --version               Print version information");
    println!("    -j, --json                  Output in JSON format");
    println!("        --csv                   Output in CSV format");
    println!("    -s                          Short output format (not yet fully implemented)");
    println!("    -a                          Display all tags (default behavior)");
    println!("    -r                          Recursive directory processing");
    println!(
        "        --preserve-file-times   Preserve original file modification time after writing"
    );
    println!(
        "        --backup                Create backup copy before modifying file (.bak extension)"
    );
    println!("        --readonly              Enable read-only mode to prevent file modifications");
    println!("    -e, --exiftool-compat       Format output for ExifTool compatibility");
    println!("        --TagsFromFile VALUE    Copy metadata from source file");
    println!(
        "    -d VALUE                    Date format string for DateTime tags in filename patterns"
    );
    println!(
        "    -n                          Dry-run mode: show proposed renames without executing"
    );
    println!();
    println!("EXAMPLES:");
    println!("    # Read metadata from a file");
    println!("    oxidex photo.jpg");
    println!();
    println!("    # Output metadata in JSON format");
    println!("    oxidex -j photo.jpg");
    println!();
    println!("    # Modify a single tag");
    println!("    oxidex -EXIF:Artist=\"John Doe\" photo.jpg");
    println!();
    println!("    # Copy metadata from one file to another");
    println!("    oxidex --TagsFromFile source.jpg destination.jpg");
    println!();
    println!("    # Rename file based on metadata");
    println!("    oxidex '-FileName<DateTimeOriginal' -d %Y%m%d_%H%M%S photo.jpg");
    println!();
    println!("For more information, visit: https://github.com/oxidex/oxidex");
}

/// Prints version information for the CLI application
///
/// Displays the application name and version number from Cargo package metadata.
fn print_version() {
    println!("oxidex {}", env!("CARGO_PKG_VERSION"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_option_clusters_are_lexopt_args() {
        assert!(is_lexopt_short_arg("-s"));
        assert!(is_lexopt_short_arg("-sr"));
        assert!(is_lexopt_short_arg("-d"));
        // Attached date-format values stay with lexopt's -d option.
        assert!(is_lexopt_short_arg("-d%Y%m%d"));
        assert!(is_lexopt_short_arg("-srd%Y"));
    }

    #[test]
    fn assignment_shaped_args_are_never_lexopt_short_args() {
        // Regression: these were consumed as `-d` with an attached value,
        // silently dropping the requested tag write.
        assert!(!is_lexopt_short_arg("-description=test"));
        assert!(!is_lexopt_short_arg(
            "-datetimeoriginal=2020:01:01 10:00:00"
        ));
        assert!(!is_lexopt_short_arg("-d=broken"));
    }

    #[test]
    fn non_cluster_args_are_not_lexopt_short_args() {
        assert!(!is_lexopt_short_arg("--json"));
        assert!(!is_lexopt_short_arg("-EXIF:Model"));
        assert!(!is_lexopt_short_arg("photo.jpg"));
        assert!(!is_lexopt_short_arg("-"));
    }
}
