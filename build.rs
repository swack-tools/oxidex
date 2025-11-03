//! Build script for exiftool-rs
//!
//! This script automatically generates the tag database from ExifTool Perl source
//! during the build process. It downloads ExifTool source from GitHub, parses
//! tag definitions from Perl modules, and generates Rust code.
//!
//! If generation fails (network issues, parse errors), it falls back to using
//! the manually curated tag registry to ensure builds always succeed.

use anyhow::{Context, Result};
use regex::Regex;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// ExifTool GitHub repository URL for downloading source
const EXIFTOOL_REPO_URL: &str = "https://github.com/exiftool/exiftool";
const EXIFTOOL_ARCHIVE_URL: &str =
    "https://github.com/exiftool/exiftool/archive/refs/heads/master.zip";

/// Output file path for generated tag database
const GENERATED_TAGS_PATH: &str = "src/tag_db/generated_tags.rs";

/// Minimum required tag count (matching manual registry)
const MIN_TAG_COUNT: usize = 500;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/tag_db/tag_registry.rs");

    // Skip generation if the file already exists (for crates.io publishing)
    if Path::new(GENERATED_TAGS_PATH).exists() {
        println!("cargo:warning=Using existing generated_tags.rs (file already exists)");
        return;
    }

    match generate_tag_database() {
        Ok(tag_count) => {
            if tag_count >= MIN_TAG_COUNT {
                println!(
                    "cargo:warning=Successfully generated tag database with {} tags",
                    tag_count
                );
            } else {
                eprintln!("cargo:warning=Generated tag database has only {} tags (expected {}+), using fallback",
                    tag_count, MIN_TAG_COUNT);
                create_fallback_generated_tags().unwrap_or_else(|e| {
                    panic!("Failed to create fallback tag database: {}", e);
                });
            }
        }
        Err(e) => {
            eprintln!(
                "cargo:warning=Tag generation failed: {}. Using fallback to manual registry.",
                e
            );
            create_fallback_generated_tags().unwrap_or_else(|e| {
                panic!("Failed to create fallback tag database: {}", e);
            });
        }
    }
}

/// Main tag generation function
fn generate_tag_database() -> Result<usize> {
    println!("cargo:warning=Starting tag database generation from ExifTool source...");

    // Step 1: Download ExifTool source
    let source_dir = download_exiftool_source().context("Failed to download ExifTool source")?;

    // Step 2: Parse tag definitions from Perl modules
    let tags = parse_exiftool_tags(&source_dir).context("Failed to parse ExifTool tags")?;

    println!(
        "cargo:warning=Parsed {} tags from ExifTool source",
        tags.len()
    );

    // Step 3: Generate Rust code
    generate_rust_code(&tags).context("Failed to generate Rust code")?;

    Ok(tags.len())
}

/// Downloads ExifTool source from GitHub
fn download_exiftool_source() -> Result<PathBuf> {
    let out_dir = std::env::var("OUT_DIR").context("OUT_DIR not set")?;
    let cache_dir = Path::new(&out_dir).join("exiftool-source");

    // Check if already cached
    if cache_dir.exists() && cache_dir.join("lib/Image/ExifTool").exists() {
        println!("cargo:warning=Using cached ExifTool source");
        return Ok(cache_dir);
    }

    println!("cargo:warning=Downloading ExifTool source from GitHub...");

    // Create cache directory
    fs::create_dir_all(&cache_dir)?;

    // Download archive
    let response = ureq::get(EXIFTOOL_ARCHIVE_URL)
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .context("Failed to download ExifTool archive")?;

    // Save to temporary file
    let zip_path = cache_dir.join("exiftool.zip");
    let mut zip_file = File::create(&zip_path)?;
    std::io::copy(&mut response.into_reader(), &mut zip_file)?;

    println!("cargo:warning=Extracting ExifTool source...");

    // Extract using unzip command (simpler than adding zip dependencies)
    let output = std::process::Command::new("unzip")
        .arg("-q")
        .arg("-o")
        .arg(&zip_path)
        .arg("-d")
        .arg(&cache_dir)
        .output();

    match output {
        Ok(result) if result.status.success() => {
            // Find the extracted directory (usually exiftool-master)
            let extracted = cache_dir.join("exiftool-master");
            if extracted.exists() {
                println!("cargo:warning=ExifTool source downloaded successfully");
                Ok(extracted)
            } else {
                anyhow::bail!("Extracted directory not found")
            }
        }
        _ => {
            // Fallback: try manual extraction if unzip not available
            anyhow::bail!("Failed to extract archive (unzip command not available)")
        }
    }
}

/// Discovers all .pm Perl modules in ExifTool source
fn discover_all_modules(lib_dir: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut modules = Vec::new();

    // Recursively walk the lib directory
    fn visit_dirs(dir: &Path, modules: &mut Vec<(PathBuf, String)>) -> Result<()> {
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_dir() {
                    visit_dirs(&path, modules)?;
                } else if path.extension().and_then(|s| s.to_str()) == Some("pm") {
                    // Extract module name from file path
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        modules.push((path.clone(), stem.to_string()));
                    }
                }
            }
        }
        Ok(())
    }

    visit_dirs(lib_dir, &mut modules)?;

    println!("cargo:warning=Discovered {} Perl modules", modules.len());
    Ok(modules)
}

/// Parses tag definitions from ExifTool Perl modules
fn parse_exiftool_tags(source_dir: &Path) -> Result<Vec<TagDefinition>> {
    let lib_dir = source_dir.join("lib/Image/ExifTool");
    if !lib_dir.exists() {
        anyhow::bail!("ExifTool lib directory not found: {:?}", lib_dir);
    }

    let mut all_tags = Vec::new();

    // Discover all modules
    let modules = discover_all_modules(&lib_dir)
        .context("Failed to discover ExifTool modules")?;

    println!("cargo:warning=Parsing {} modules for tag definitions...", modules.len());

    // Parse each module
    for (module_path, module_name) in modules {
        match parse_perl_module(&module_path, &module_name) {
            Ok(mut tags) => {
                if !tags.is_empty() {
                    println!(
                        "cargo:warning=  {:30} -> {:5} tags",
                        module_name,
                        tags.len()
                    );
                    all_tags.append(&mut tags);
                }
            }
            Err(e) => {
                // Don't fail on individual module parse errors
                eprintln!("cargo:warning=  {:30} -> ERROR: {}", module_name, e);
            }
        }
    }

    println!("cargo:warning=Total tags parsed: {}", all_tags.len());

    if all_tags.is_empty() {
        anyhow::bail!("No tags parsed from ExifTool source");
    }

    Ok(all_tags)
}

/// Comprehensive regex patterns for parsing Perl tag definitions
struct TagPatterns {
    /// Matches: %Image::ExifTool::ModuleName::TableName = (
    table_declaration: Regex,

    /// Matches: 0x0100 => { Name => 'ImageWidth', ... }
    hash_tag_def: Regex,

    /// Matches: 0x0100 => 'ImageWidth',
    simple_tag_def: Regex,

    /// Matches: Name => 'ImageWidth',
    name_field: Regex,

    /// Matches: Writable => 'int16u',
    writable_field: Regex,

    /// Matches: Description => 'Image Width',
    description_field: Regex,

    /// Matches: Format => 'int16u',
    format_field: Regex,
}

impl TagPatterns {
    fn new() -> Result<Self> {
        Ok(TagPatterns {
            table_declaration: Regex::new(
                r"%Image::ExifTool::(\w+(?:::\w+)*)\s*=\s*\("
            )?,
            hash_tag_def: Regex::new(
                r"^\s*(0x[0-9a-fA-F]+|'[^']*'|\d+)\s*=>\s*\{"
            )?,
            simple_tag_def: Regex::new(
                r#"^\s*(0x[0-9a-fA-F]+|\d+)\s*=>\s*'([^']+)'[\s,]*$"#
            )?,
            name_field: Regex::new(
                r#"Name\s*=>\s*'([^']+)'"#
            )?,
            writable_field: Regex::new(
                r#"Writable\s*=>\s*'?([^',}\s]+)"#
            )?,
            description_field: Regex::new(
                r#"Description\s*=>\s*'([^']+)'"#
            )?,
            format_field: Regex::new(
                r#"Format\s*=>\s*'([^']+)'"#
            )?,
        })
    }
}

/// Parses a single Perl module file for tag definitions
fn parse_perl_module(module_path: &Path, format_family: &str) -> Result<Vec<TagDefinition>> {
    let file = File::open(module_path)?;
    let reader = BufReader::new(file);
    let patterns = TagPatterns::new()?;

    let mut tags = Vec::new();
    let mut in_table = false;
    let mut current_table_name = String::new();
    let mut brace_depth = 0;
    let mut current_tag_def = String::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        // Skip comments
        if trimmed.starts_with('#') {
            continue;
        }

        // Detect table declaration
        if let Some(captures) = patterns.table_declaration.captures(&line) {
            current_table_name = captures[1].to_string();
            in_table = true;
            brace_depth = 0;
            continue;
        }

        if !in_table {
            continue;
        }

        // Track brace depth to know when table ends
        brace_depth += line.matches('(').count() as i32;
        brace_depth += line.matches('{').count() as i32;
        brace_depth -= line.matches(')').count() as i32;
        brace_depth -= line.matches('}').count() as i32;

        if brace_depth < 0 {
            in_table = false;
            continue;
        }

        // Accumulate multi-line tag definitions
        current_tag_def.push_str(&line);
        current_tag_def.push('\n');

        // Check if we have a complete tag definition
        if let Some(tag) = try_parse_tag_definition(
            &current_tag_def,
            format_family,
            &current_table_name,
            &patterns,
        )? {
            tags.push(tag);
            current_tag_def.clear();
        }

        // Clear if line ends with comma or closing brace (definition complete)
        if trimmed.ends_with(',') || (trimmed.ends_with('}') && brace_depth >= 0) {
            current_tag_def.clear();
        }
    }

    Ok(tags)
}

/// Attempts to parse a complete tag definition from accumulated text
fn try_parse_tag_definition(
    def: &str,
    format_family: &str,
    table_name: &str,
    patterns: &TagPatterns,
) -> Result<Option<TagDefinition>> {
    // Try simple definition first: 0x0100 => 'ImageWidth',
    if let Some(captures) = patterns.simple_tag_def.captures(def) {
        let tag_id = parse_tag_id(&captures[1])?;
        let tag_name = captures[2].to_string();

        return Ok(Some(TagDefinition {
            tag_id: TagId::Numeric(tag_id as u16),
            tag_name: format!("{}:{}", format_family, tag_name),
            format_family: format_family.to_string(),
            table_name: table_name.to_string(),
            writable: false,
            writable_type: None,
            value_type: ValueType::String,
            description: format!("{} tag", tag_name),
        }));
    }

    // Try hash-based definition: 0x0100 => { Name => 'ImageWidth', ... }
    if let Some(captures) = patterns.hash_tag_def.captures(def) {
        let tag_id_str = &captures[1];

        // Extract tag ID (hex or decimal)
        let tag_id = parse_tag_id(tag_id_str)?;

        // Extract Name field
        if let Some(name_cap) = patterns.name_field.captures(def) {
            let tag_name = name_cap[1].to_string();

            // Extract optional Writable field
            let writable_type = patterns
                .writable_field
                .captures(def)
                .map(|c| c[1].to_string());

            // Extract optional Description field
            let description = patterns
                .description_field
                .captures(def)
                .map(|c| c[1].to_string())
                .unwrap_or_else(|| format!("{} tag", tag_name));

            // Extract optional Format field
            let format = patterns
                .format_field
                .captures(def)
                .map(|c| c[1].to_string());

            // Determine if writable
            let writable = writable_type
                .as_ref()
                .map(|w| w != "no" && w != "0")
                .unwrap_or(false);

            // Determine value type from Writable or Format field
            let value_type = writable_type
                .as_ref()
                .or(format.as_ref())
                .map(|t| map_perl_type_to_value_type(t))
                .unwrap_or(ValueType::String);

            return Ok(Some(TagDefinition {
                tag_id: TagId::Numeric(tag_id as u16),
                tag_name: format!("{}:{}", format_family, tag_name),
                format_family: format_family.to_string(),
                table_name: table_name.to_string(),
                writable,
                writable_type,
                value_type,
                description,
            }));
        }
    }

    Ok(None)
}

/// Parses tag ID from string (hex, decimal, or string)
fn parse_tag_id(id_str: &str) -> Result<u32> {
    let id_str = id_str.trim().trim_matches('\'').trim_matches('"');

    if let Some(hex_str) = id_str.strip_prefix("0x") {
        u32::from_str_radix(hex_str, 16)
            .with_context(|| format!("Failed to parse hex tag ID: {}", id_str))
    } else if let Ok(num) = id_str.parse::<u32>() {
        Ok(num)
    } else {
        // String-based tag ID - hash the string to get a numeric ID
        Ok(hash_string_tag_id(id_str))
    }
}

/// Hashes string tag IDs to numeric values for consistent mapping
fn hash_string_tag_id(s: &str) -> u32 {
    // Simple hash function for string tag IDs
    s.bytes().fold(0u32, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(b as u32)
    })
}


/// Maps Perl type strings to ValueType enum
fn map_perl_type_to_value_type(perl_type: &str) -> ValueType {
    match perl_type {
        "string" | "lang-alt" => ValueType::String,
        "int8u" | "int8s" | "int16u" | "int16s" | "int32u" | "int32s" | "int64u" | "int64s" => {
            ValueType::Integer
        }
        "rational32u" | "rational32s" | "rational64u" | "rational64s" => ValueType::Rational,
        "float" | "double" => ValueType::Float,
        "binary" | "undef" => ValueType::Binary,
        _ if perl_type.contains("date") || perl_type.contains("time") => ValueType::DateTime,
        _ => ValueType::String,
    }
}

/// Generates Rust source code from tag definitions using optimized static array approach
fn generate_rust_code(tags: &[TagDefinition]) -> Result<()> {
    let output_path = Path::new(GENERATED_TAGS_PATH);
    let mut file = File::create(output_path)?;

    // Write file header
    writeln!(file, "//! Auto-generated tag database from ExifTool source")?;
    writeln!(file, "//!")?;
    writeln!(file, "//! THIS FILE IS AUTO-GENERATED BY build.rs")?;
    writeln!(
        file,
        "//! DO NOT EDIT MANUALLY - CHANGES WILL BE OVERWRITTEN"
    )?;
    writeln!(file, "//!")?;
    writeln!(
        file,
        "//! Generated from ExifTool GitHub repository: {}",
        EXIFTOOL_REPO_URL
    )?;
    writeln!(file, "//! Total tags: {}", tags.len())?;
    writeln!(file)?;
    writeln!(file, "#![allow(dead_code)]")?;
    writeln!(file)?;
    writeln!(
        file,
        "use crate::core::tag_descriptor::{{FormatFamily, TagDescriptor, TagId, ValueType}};"
    )?;
    writeln!(file, "use once_cell::sync::Lazy;")?;
    writeln!(file, "use std::collections::HashMap;")?;
    writeln!(file)?;

    // Generate lazy tag array (allows heap allocations)
    writeln!(file, "/// Lazily initialized array of all generated tags")?;
    writeln!(file, "static TAG_ARRAY: Lazy<Vec<TagDescriptor>> = Lazy::new(|| {{")?;
    writeln!(file, "    vec![")?;

    for tag in tags {
        generate_tag_array_entry(&mut file, tag)?;
    }

    writeln!(file, "    ]")?;
    writeln!(file, "}});")?;
    writeln!(file)?;

    // Generate HashMap for O(1) lookup
    writeln!(
        file,
        "/// Auto-generated tag registry from ExifTool source."
    )?;
    writeln!(
        file,
        "/// This registry is populated during the build process by parsing"
    )?;
    writeln!(
        file,
        "/// ExifTool Perl modules and extracting tag metadata."
    )?;
    writeln!(file, "pub static GENERATED_TAG_REGISTRY: Lazy<HashMap<String, TagDescriptor>> = Lazy::new(|| {{")?;
    writeln!(
        file,
        "    let mut registry = HashMap::with_capacity({});",
        tags.len()
    )?;
    writeln!(file, "    for tag in TAG_ARRAY.iter() {{")?;
    writeln!(file, "        registry.insert(tag.tag_name.clone(), tag.clone());")?;
    writeln!(file, "    }}")?;
    writeln!(file, "    registry")?;
    writeln!(file, "}});")?;
    writeln!(file)?;

    // Generate lookup function
    writeln!(file, "/// Looks up a tag descriptor by its canonical name.")?;
    writeln!(file, "///")?;
    writeln!(file, "/// # Arguments")?;
    writeln!(
        file,
        "/// * `name` - The full tag name (e.g., \"EXIF:Make\", \"XMP-dc:Creator\")"
    )?;
    writeln!(file, "///")?;
    writeln!(file, "/// # Returns")?;
    writeln!(
        file,
        "/// * `Some(&TagDescriptor)` if the tag is registered"
    )?;
    writeln!(file, "/// * `None` if the tag is not found")?;
    writeln!(
        file,
        "pub fn get_generated_tag_descriptor(name: &str) -> Option<&TagDescriptor> {{"
    )?;
    writeln!(file, "    GENERATED_TAG_REGISTRY.get(name)")?;
    writeln!(file, "}}")?;
    writeln!(file)?;

    // Generate tag count function
    writeln!(
        file,
        "/// Returns the total number of registered tags in the generated registry."
    )?;
    writeln!(file, "pub fn generated_tag_count() -> usize {{")?;
    writeln!(file, "    TAG_ARRAY.len()")?;
    writeln!(file, "}}")?;
    writeln!(file)?;

    // Generate tests
    writeln!(file, "#[cfg(test)]")?;
    writeln!(file, "mod tests {{")?;
    writeln!(file, "    use super::*;")?;
    writeln!(file)?;
    writeln!(file, "    #[test]")?;
    writeln!(file, "    fn test_generated_registry_not_empty() {{")?;
    writeln!(
        file,
        "        assert!(generated_tag_count() > 0, \"Generated registry should not be empty\");"
    )?;
    writeln!(file, "    }}")?;
    writeln!(file)?;
    writeln!(file, "    #[test]")?;
    writeln!(file, "    fn test_generated_registry_min_count() {{")?;
    writeln!(
        file,
        "        assert!(generated_tag_count() >= {}, \"Expected at least {} tags\");",
        MIN_TAG_COUNT, MIN_TAG_COUNT
    )?;
    writeln!(file, "    }}")?;
    writeln!(file, "}}")?;

    println!(
        "cargo:warning=Generated Rust code written to {}",
        GENERATED_TAGS_PATH
    );
    Ok(())
}

/// Generates a compact tag array entry (single line for efficiency)
fn generate_tag_array_entry(file: &mut File, tag: &TagDefinition) -> Result<()> {
    // Format family - map to known FormatFamily enum variants
    let family_variant = match tag.format_family.as_str() {
        "ICC_Profile" => "ICCProfile",
        "EXIF" => "EXIF",
        "XMP" => "XMP",
        "IPTC" => "IPTC",
        "GPS" => "GPS",
        "Photoshop" => "Photoshop",
        "MakerNotes" => "MakerNotes",
        "JFIF" => "JFIF",
        "JPEG" => "JPEG",
        "PNG" => "PNG",
        "PDF" => "PDF",
        "QuickTime" => "QuickTime",
        "TIFF" => "TIFF",
        "RIFF" => "RIFF",
        "PostScript" => "PostScript",
        // Map all maker note modules to MakerNotes family
        "Canon" | "Nikon" | "Sony" | "Olympus" | "Panasonic" | "Pentax" | "FujiFilm"
        | "Samsung" | "Minolta" | "Kodak" | "Casio" | "Ricoh" | "Sanyo"
        | "CanonCustom" | "NikonCapture" | "KyoceraRaw" | "MinoltaRaw"
        | "SigmaRaw" | "Leaf" | "PhaseOne" => "MakerNotes",
        // Map video/audio formats to QuickTime for now
        "Matroska" | "Flash" | "ASF" | "MPEG" | "H264" | "FLAC" | "Ogg"
        | "Vorbis" | "AAC" | "APE" => "QuickTime",
        // Map other formats to appropriate families
        "GIF" | "BMP" | "PSD" | "DjVu" | "MNG" | "BPG" | "FLIF" | "ICO" => "PNG",
        "HTML" | "XML" | "SVG" => "PDF",
        // Default: unknown formats map to MakerNotes as a catch-all
        _ => "MakerNotes",
    };

    // Example value based on type
    let example_value = match tag.value_type {
        ValueType::String => "\"Example\".to_string()",
        ValueType::Integer => "\"100\".to_string()",
        ValueType::Float => "\"1.5\".to_string()",
        ValueType::Rational => "\"1/100\".to_string()",
        ValueType::DateTime => "\"2024:01:01 12:00:00\".to_string()",
        _ => "\"Value\".to_string()",
    };

    // Generate compact single-line entry
    match &tag.tag_id {
        TagId::Numeric(n) => {
            writeln!(
                file,
                "    TagDescriptor::new(TagId::new_numeric(0x{:04X}), \"{}\".to_string(), FormatFamily::{}, {}, ValueType::{:?}, \"{}\".to_string(), vec![{}]),",
                n,
                escape_string(&tag.tag_name),
                family_variant,
                tag.writable,
                tag.value_type,
                escape_string(&tag.description),
                example_value
            )?;
        }
        TagId::Named(s) => {
            writeln!(
                file,
                "    TagDescriptor::new(TagId::new_named(\"{}\".to_string()), \"{}\".to_string(), FormatFamily::{}, {}, ValueType::{:?}, \"{}\".to_string(), vec![{}]),",
                escape_string(s),
                escape_string(&tag.tag_name),
                family_variant,
                tag.writable,
                tag.value_type,
                escape_string(&tag.description),
                example_value
            )?;
        }
    }

    Ok(())
}

/// Escapes special characters in strings for Rust code generation
fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Creates a fallback generated_tags.rs that references the manual registry
fn create_fallback_generated_tags() -> Result<()> {
    let output_path = Path::new(GENERATED_TAGS_PATH);
    let mut file = File::create(output_path)?;

    writeln!(file, "//! Fallback tag database")?;
    writeln!(file, "//!")?;
    writeln!(
        file,
        "//! Tag generation from ExifTool source failed during build."
    )?;
    writeln!(
        file,
        "//! This file provides a fallback that delegates to the manually"
    )?;
    writeln!(
        file,
        "//! curated tag registry to ensure the build succeeds."
    )?;
    writeln!(file)?;
    writeln!(file, "#![allow(dead_code)]")?;
    writeln!(file)?;
    writeln!(file, "use crate::core::tag_descriptor::TagDescriptor;")?;
    writeln!(file)?;
    writeln!(
        file,
        "/// Fallback lookup function (delegates to manual registry)"
    )?;
    writeln!(
        file,
        "pub fn get_generated_tag_descriptor(name: &str) -> Option<&TagDescriptor> {{"
    )?;
    writeln!(
        file,
        "    crate::tag_db::tag_registry::get_tag_descriptor(name)"
    )?;
    writeln!(file, "}}")?;
    writeln!(file)?;
    writeln!(
        file,
        "/// Fallback tag count function (delegates to manual registry)"
    )?;
    writeln!(file, "pub fn generated_tag_count() -> usize {{")?;
    writeln!(file, "    crate::tag_db::tag_registry::tag_count()")?;
    writeln!(file, "}}")?;
    writeln!(file)?;

    println!("cargo:warning=Created fallback generated_tags.rs using manual registry");
    Ok(())
}

/// Represents a parsed tag definition
#[derive(Debug, Clone)]
struct TagDefinition {
    tag_id: TagId,
    tag_name: String,
    format_family: String,
    table_name: String,        // NEW: track which table this tag came from
    writable: bool,
    writable_type: Option<String>, // NEW: track writable type (int16u, string, etc)
    value_type: ValueType,
    description: String,
}

/// Tag ID enum (matches src/core/tag_descriptor.rs)
#[derive(Debug, Clone)]
enum TagId {
    Numeric(u16),
    Named(String),
}

/// Value type enum (matches src/core/tag_descriptor.rs)
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum ValueType {
    String,
    Integer,
    Float,
    Rational,
    Binary,
    DateTime,
    Struct,
}
