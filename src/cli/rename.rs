//! File renaming based on metadata tags
//!
//! This module implements the -FileName feature to rename files based on metadata values.
//! Supports variable substitution (e.g., ${EXIF:DateTimeOriginal}), date formatting with -d flag,
//! dry-run mode with -n flag, and safety checks for collision detection.

use crate::core::operations::read_metadata;
use crate::core::tag_value::TagValue;
use crate::error::{ExifToolError, Result};
use std::path::{Path, PathBuf};

/// Represents a token in a filename pattern
#[derive(Debug, Clone, PartialEq)]
enum PatternToken {
    /// A literal string (e.g., "_" or "-")
    Literal(String),
    /// A tag reference (e.g., "DateTimeOriginal" or "EXIF:Make")
    Tag(String),
    /// File extension placeholder (%%e)
    Extension,
    /// Counter placeholder for collision avoidance (%%-.c)
    Counter,
}

/// Parses a filename pattern into tokens
///
/// Supports:
/// - Simple tag names: DateTimeOriginal
/// - Qualified tag names: EXIF:DateTimeOriginal
/// - Variable syntax: ${EXIF:DateTimeOriginal}
/// - Extension placeholder: %%e or %%.e
/// - Counter placeholder: %%-.c
/// - Literal text between tags
///
/// Example: "${EXIF:Make}_${EXIF:Model}%%-.%%e"
/// -> [Tag("EXIF:Make"), Literal("_"), Tag("EXIF:Model"), Counter, Literal("."), Extension]
fn parse_pattern(pattern: &str) -> Result<Vec<PatternToken>> {
    let mut tokens = Vec::new();
    let mut current_literal = String::new();
    let mut chars = pattern.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '$' if chars.peek() == Some(&'{') => {
                // Variable syntax: ${TagName}
                chars.next(); // consume '{'

                // Save any accumulated literal
                if !current_literal.is_empty() {
                    tokens.push(PatternToken::Literal(current_literal.clone()));
                    current_literal.clear();
                }

                // Extract tag name until '}'
                let mut tag_name = String::new();
                for ch in chars.by_ref() {
                    if ch == '}' {
                        break;
                    }
                    tag_name.push(ch);
                }

                if tag_name.is_empty() {
                    return Err(ExifToolError::parse_error("Empty tag name in pattern"));
                }

                tokens.push(PatternToken::Tag(tag_name));
            }
            '%' if chars.peek() == Some(&'%') => {
                // Special placeholder: %%e or %%-.c
                chars.next(); // consume second '%'

                // Save any accumulated literal
                if !current_literal.is_empty() {
                    tokens.push(PatternToken::Literal(current_literal.clone()));
                    current_literal.clear();
                }

                // Check what follows
                let mut placeholder = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch.is_alphabetic() || ch == '-' || ch == '.' {
                        placeholder.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }

                match placeholder.as_str() {
                    "e" | ".e" => tokens.push(PatternToken::Extension),
                    "-.c" => tokens.push(PatternToken::Counter),
                    _ => {
                        // Unknown placeholder, treat as literal
                        current_literal.push_str("%%");
                        current_literal.push_str(&placeholder);
                    }
                }
            }
            _ => {
                // Regular character, add to current literal
                current_literal.push(ch);
            }
        }
    }

    // If the pattern is just a simple tag name (no special syntax)
    if tokens.is_empty() && !current_literal.is_empty() {
        // Check if it looks like a tag name (contains letters and maybe colon)
        if current_literal
            .chars()
            .all(|c| c.is_alphanumeric() || c == ':' || c == '_')
        {
            tokens.push(PatternToken::Tag(current_literal.clone()));
            current_literal.clear();
        }
    }

    // Add any remaining literal
    if !current_literal.is_empty() {
        tokens.push(PatternToken::Literal(current_literal));
    }

    if tokens.is_empty() {
        return Err(ExifToolError::parse_error("Empty pattern"));
    }

    Ok(tokens)
}

/// Resolves a tag name in metadata, trying both qualified and unqualified forms
///
/// Tries in order:
/// 1. Exact tag name (e.g., "EXIF:DateTimeOriginal")
/// 2. Unqualified name with EXIF prefix (e.g., "DateTimeOriginal" -> "EXIF:DateTimeOriginal")
fn resolve_tag<'a>(metadata: &'a crate::core::MetadataMap, tag_name: &str) -> Option<&'a TagValue> {
    // Try exact match first
    if let Some(value) = metadata.get(tag_name) {
        return Some(value);
    }

    // If tag doesn't have a family prefix, try adding "EXIF:"
    if !tag_name.contains(':') {
        let qualified_name = format!("EXIF:{}", tag_name);
        if let Some(value) = metadata.get(&qualified_name) {
            return Some(value);
        }
    }

    None
}

/// Formats a TagValue as a string for use in filenames
///
/// If date_format is provided and the value is a DateTime, applies the format.
/// Otherwise, converts the value to its string representation.
fn format_tag_value(value: &TagValue, date_format: Option<&str>) -> Result<String> {
    match value {
        TagValue::DateTime(dt) => {
            if let Some(format) = date_format {
                // Apply chrono format string
                Ok(dt.format(format).to_string())
            } else {
                // Default ISO format
                Ok(dt.format("%Y:%m:%d %H:%M:%S").to_string())
            }
        }
        TagValue::String(s) => Ok(s.clone()),
        TagValue::Integer(i) => Ok(i.to_string()),
        TagValue::Float(f) => Ok(f.to_string()),
        TagValue::Rational {
            numerator,
            denominator,
        } => {
            if *denominator == 1 {
                Ok(numerator.to_string())
            } else {
                Ok(format!("{}/{}", numerator, denominator))
            }
        }
        TagValue::Binary(_) => Err(ExifToolError::parse_error(
            "Cannot use binary tag in filename",
        )),
        TagValue::Struct(_) => Err(ExifToolError::parse_error(
            "Cannot use struct tag in filename",
        )),
        TagValue::Array(values) => {
            // For arrays, use the first value
            values.first().map_or_else(
                || Err(ExifToolError::parse_error("Empty array tag")),
                |v| format_tag_value(v, date_format),
            )
        }
    }
}

/// Substitutes pattern tokens with actual values from metadata
///
/// Builds the new filename by replacing tags with their values.
fn substitute_pattern(
    tokens: &[PatternToken],
    metadata: &crate::core::MetadataMap,
    original_path: &Path,
    date_format: Option<&str>,
) -> Result<String> {
    let mut result = String::new();

    for token in tokens {
        match token {
            PatternToken::Literal(s) => {
                result.push_str(s);
            }
            PatternToken::Tag(tag_name) => {
                // Resolve tag in metadata
                let value = resolve_tag(metadata, tag_name).ok_or_else(|| {
                    ExifToolError::parse_error(format!("Tag '{}' not found in metadata", tag_name))
                })?;

                // Format value for filename
                let formatted = format_tag_value(value, date_format)?;
                result.push_str(&formatted);
            }
            PatternToken::Extension => {
                // Get original file extension
                if let Some(ext) = original_path.extension() {
                    result.push('.');
                    result.push_str(ext.to_str().unwrap_or(""));
                }
            }
            PatternToken::Counter => {
                // Counter is not implemented yet, just add nothing for now
                // In full implementation, this would be handled by collision detection
            }
        }
    }

    Ok(result)
}

/// Sanitizes a filename to remove invalid characters
///
/// Replaces characters that are invalid on most file systems:
/// - Path separators: / \
/// - Reserved characters: : * ? " < > |
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

/// Builds a new filename from a pattern and metadata
///
/// This is the main entry point for filename generation.
pub fn build_new_filename(
    pattern: &str,
    metadata: &crate::core::MetadataMap,
    original_path: &Path,
    date_format: Option<&str>,
) -> Result<String> {
    // Parse pattern into tokens
    let tokens = parse_pattern(pattern)?;

    // Substitute tokens with metadata values
    let new_name = substitute_pattern(&tokens, metadata, original_path, date_format)?;

    // Sanitize filename
    let sanitized = sanitize_filename(&new_name);

    if sanitized.is_empty() {
        return Err(ExifToolError::parse_error("Resulting filename is empty"));
    }

    Ok(sanitized)
}

/// Renames a single file based on metadata
///
/// If dry_run is true, prints the proposed rename without executing.
/// Returns the new path if successful.
pub fn rename_file(
    path: &Path,
    pattern: &str,
    date_format: Option<&str>,
    dry_run: bool,
) -> Result<PathBuf> {
    // Read metadata from file
    let metadata = read_metadata(path)?;

    // Build new filename
    let new_filename = build_new_filename(pattern, &metadata, path, date_format)?;

    // Get parent directory (file stays in same directory)
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let new_path = parent.join(&new_filename);

    // Check for collision (don't overwrite existing files)
    if new_path.exists() && new_path != path {
        return Err(ExifToolError::parse_error(format!(
            "Target file already exists: {}",
            new_path.display()
        )));
    }

    if dry_run {
        // Dry-run mode: just print what would happen
        println!("{} -> {}", path.display(), new_path.display());
    } else {
        // Actually rename the file
        std::fs::rename(path, &new_path)?;
    }

    Ok(new_path)
}

/// Processes rename operation for multiple files
///
/// Renames each file according to the pattern.
/// Continues processing on error (graceful degradation).
pub fn process_rename_operation(
    files: &[PathBuf],
    pattern: &str,
    date_format: Option<&str>,
    dry_run: bool,
) -> Result<usize> {
    let mut success_count = 0;
    let mut error_count = 0;

    for file in files {
        match rename_file(file, pattern, date_format, dry_run) {
            Ok(_) => {
                success_count += 1;
            }
            Err(e) => {
                eprintln!("Error renaming '{}': {}", file.display(), e);
                error_count += 1;
            }
        }
    }

    if error_count > 0 {
        eprintln!("{} files renamed, {} errors", success_count, error_count);
    } else if !dry_run {
        println!("{} image files renamed", success_count);
    }

    Ok(success_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pattern_simple_tag() {
        let tokens = parse_pattern("DateTimeOriginal").unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0], PatternToken::Tag("DateTimeOriginal".to_string()));
    }

    #[test]
    fn test_parse_pattern_variable_syntax() {
        let tokens = parse_pattern("${EXIF:Make}").unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0], PatternToken::Tag("EXIF:Make".to_string()));
    }

    #[test]
    fn test_parse_pattern_multiple_tags() {
        let tokens = parse_pattern("${EXIF:Make}_${EXIF:Model}").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], PatternToken::Tag("EXIF:Make".to_string()));
        assert_eq!(tokens[1], PatternToken::Literal("_".to_string()));
        assert_eq!(tokens[2], PatternToken::Tag("EXIF:Model".to_string()));
    }

    #[test]
    fn test_parse_pattern_with_extension() {
        let tokens = parse_pattern("${EXIF:DateTimeOriginal}%%e").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[0],
            PatternToken::Tag("EXIF:DateTimeOriginal".to_string())
        );
        assert_eq!(tokens[1], PatternToken::Extension);
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(
            sanitize_filename("2025:01:15 10:30:00"),
            "2025_01_15 10_30_00"
        );
        assert_eq!(sanitize_filename("Canon/EOS"), "Canon_EOS");
        assert_eq!(sanitize_filename("test<file>.jpg"), "test_file_.jpg");
    }

    #[test]
    fn test_format_tag_value_string() {
        let value = TagValue::new_string("Canon");
        let result = format_tag_value(&value, None).unwrap();
        assert_eq!(result, "Canon");
    }

    #[test]
    fn test_format_tag_value_integer() {
        let value = TagValue::new_integer(100);
        let result = format_tag_value(&value, None).unwrap();
        assert_eq!(result, "100");
    }

    #[test]
    fn test_format_tag_value_datetime_with_format() {
        use chrono::TimeZone;
        let dt = chrono::Utc
            .with_ymd_and_hms(2025, 1, 15, 10, 30, 0)
            .unwrap();
        let value = TagValue::new_datetime(dt);
        let result = format_tag_value(&value, Some("%Y%m%d_%H%M%S")).unwrap();
        assert_eq!(result, "20250115_103000");
    }
}
