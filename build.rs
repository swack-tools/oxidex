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

/// Parses tag definitions from ExifTool Perl modules
fn parse_exiftool_tags(source_dir: &Path) -> Result<Vec<TagDefinition>> {
    let lib_dir = source_dir.join("lib/Image/ExifTool");
    if !lib_dir.exists() {
        anyhow::bail!("ExifTool lib directory not found: {:?}", lib_dir);
    }

    let mut all_tags = Vec::new();

    // Parse key modules for different format families
    let modules = vec![
        ("EXIF.pm", "EXIF"),
        ("GPS.pm", "GPS"),
        ("XMP.pm", "XMP"),
        ("IPTC.pm", "IPTC"),
        ("PDF.pm", "PDF"),
        ("QuickTime.pm", "QuickTime"),
        ("Photoshop.pm", "Photoshop"),
        ("PNG.pm", "PNG"),
        ("JFIF.pm", "JFIF"),
        ("JPEG.pm", "JPEG"),
        ("TIFF.pm", "TIFF"),
        ("ICC_Profile.pm", "ICC_Profile"),
        ("PostScript.pm", "PostScript"),
        ("RIFF.pm", "RIFF"),
        ("MakerNotes.pm", "MakerNotes"),
    ];

    for (module_file, format_family) in modules {
        let module_path = lib_dir.join(module_file);
        if module_path.exists() {
            match parse_perl_module(&module_path, format_family) {
                Ok(mut tags) => {
                    println!(
                        "cargo:warning=Parsed {} tags from {}",
                        tags.len(),
                        module_file
                    );
                    all_tags.append(&mut tags);
                }
                Err(e) => {
                    eprintln!("cargo:warning=Failed to parse {}: {}", module_file, e);
                }
            }
        }
    }

    if all_tags.is_empty() {
        anyhow::bail!("No tags parsed from ExifTool source");
    }

    Ok(all_tags)
}

/// Parses a single Perl module file for tag definitions
fn parse_perl_module(module_path: &Path, format_family: &str) -> Result<Vec<TagDefinition>> {
    let file = File::open(module_path)?;
    let reader = BufReader::new(file);

    let mut tags = Vec::new();
    let mut in_tag_table = false;
    let mut current_tag_id: Option<String> = None;
    let mut current_tag_data: HashMap<String, String> = HashMap::new();

    // Regex patterns for parsing Perl hash structures
    // Updated to handle: hex IDs (0x010F), quoted strings ('Name'), integers (1, 2, 3)
    let tag_id_regex = Regex::new(r"^\s*(0x[0-9A-Fa-f]+|'[^']+'|\d+)\s*=>\s*\{?\s*$")?;
    let name_regex = Regex::new(r#"^\s*Name\s*=>\s*'([^']+)'"#)?;
    let writable_regex = Regex::new(r#"^\s*Writable\s*=>\s*'?([^',\s]+)"#)?;
    let desc_regex = Regex::new(r#"^\s*Description\s*=>\s*'([^']+)'"#)?;
    let format_regex = Regex::new(r#"^\s*Format\s*=>\s*'([^']+)'"#)?;
    let table_start_regex = Regex::new(r"%Image::ExifTool::\w+::\w+\s*=\s*\(")?;

    for line in reader.lines() {
        let line = line?;

        // Detect tag table start
        if table_start_regex.is_match(&line) {
            in_tag_table = true;
            continue;
        }

        // Detect tag table end
        if in_tag_table && line.trim() == ");" {
            in_tag_table = false;
            // Save any pending tag
            if let Some(tag_id) = current_tag_id.take() {
                if let Some(tag) = create_tag_definition(&tag_id, &current_tag_data, format_family)
                {
                    tags.push(tag);
                }
                current_tag_data.clear();
            }
            continue;
        }

        if !in_tag_table {
            continue;
        }

        // Parse tag ID line (e.g., "0x010F => {" or "'Creator' => {")
        if let Some(_caps) = tag_id_regex.captures(&line) {
            // Save previous tag if exists
            if let Some(tag_id) = current_tag_id.take() {
                if let Some(tag) = create_tag_definition(&tag_id, &current_tag_data, format_family)
                {
                    tags.push(tag);
                }
                current_tag_data.clear();
            }

            // Extract tag ID
            let id_str = line.split("=>").next().unwrap_or("").trim();
            if !id_str.is_empty() && id_str != "GROUPS" {
                current_tag_id = Some(id_str.to_string());
            }
            continue;
        }

        // Parse tag properties
        if current_tag_id.is_some() {
            if let Some(caps) = name_regex.captures(&line) {
                current_tag_data.insert("Name".to_string(), caps[1].to_string());
            } else if let Some(caps) = writable_regex.captures(&line) {
                current_tag_data.insert("Writable".to_string(), caps[1].to_string());
            } else if let Some(caps) = desc_regex.captures(&line) {
                current_tag_data.insert("Description".to_string(), caps[1].to_string());
            } else if let Some(caps) = format_regex.captures(&line) {
                current_tag_data.insert("Format".to_string(), caps[1].to_string());
            }

            // Check for closing brace (end of tag definition)
            if line.trim() == "}," || line.trim() == "}" {
                if let Some(tag_id) = current_tag_id.take() {
                    if let Some(tag) =
                        create_tag_definition(&tag_id, &current_tag_data, format_family)
                    {
                        tags.push(tag);
                    }
                    current_tag_data.clear();
                }
            }
        }
    }

    Ok(tags)
}

/// Creates a TagDefinition from parsed Perl data
fn create_tag_definition(
    tag_id_str: &str,
    data: &HashMap<String, String>,
    format_family: &str,
) -> Option<TagDefinition> {
    // Get tag name (required)
    let name = data.get("Name")?;

    // Parse tag ID
    let tag_id = if tag_id_str.starts_with("0x") {
        // Hex ID (e.g., 0x010F)
        let hex_str = tag_id_str.trim_start_matches("0x").trim_end_matches(',');
        if let Ok(num) = u16::from_str_radix(hex_str, 16) {
            TagId::Numeric(num)
        } else {
            return None;
        }
    } else if tag_id_str
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        // Integer ID (e.g., 1, 2, 3)
        let num_str = tag_id_str.trim_end_matches(',').trim();
        if let Ok(num) = num_str.parse::<u16>() {
            TagId::Numeric(num)
        } else {
            return None;
        }
    } else {
        // String ID (remove quotes, e.g., 'Creator', "Title")
        TagId::Named(tag_id_str.trim_matches('\'').trim_matches('"').to_string())
    };

    // Determine if writable
    let writable = data
        .get("Writable")
        .map(|w| w != "no" && w != "0")
        .unwrap_or(false);

    // Determine value type from Writable field
    let value_type = data
        .get("Writable")
        .or_else(|| data.get("Format"))
        .map(|w| map_perl_type_to_value_type(w))
        .unwrap_or(ValueType::String);

    // Get description (use name as fallback)
    let description = data
        .get("Description")
        .cloned()
        .unwrap_or_else(|| format!("{} tag", name));

    // Create full tag name with family prefix
    let full_name = format!("{}:{}", format_family, name);

    Some(TagDefinition {
        tag_id,
        tag_name: full_name,
        format_family: format_family.to_string(),
        writable,
        value_type,
        description,
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

/// Generates Rust source code from tag definitions
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
    writeln!(file, "pub static GENERATED_TAG_REGISTRY: Lazy<HashMap<&'static str, TagDescriptor>> = Lazy::new(|| {{")?;
    writeln!(
        file,
        "    let mut registry = HashMap::with_capacity({});",
        tags.len()
    )?;
    writeln!(file)?;

    // Group tags by format family for organized output
    let mut tags_by_family: HashMap<String, Vec<&TagDefinition>> = HashMap::new();
    for tag in tags {
        tags_by_family
            .entry(tag.format_family.clone())
            .or_default()
            .push(tag);
    }

    // Generate tag insertions grouped by family
    for (family, family_tags) in tags_by_family.iter() {
        writeln!(file, "    // =============================")?;
        writeln!(file, "    // {} TAGS ({} total)", family, family_tags.len())?;
        writeln!(file, "    // =============================")?;
        writeln!(file)?;

        for tag in family_tags {
            generate_tag_insertion(&mut file, tag)?;
        }

        writeln!(file)?;
    }

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
    writeln!(file, "    GENERATED_TAG_REGISTRY.len()")?;
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

/// Generates a single tag insertion statement
fn generate_tag_insertion(file: &mut File, tag: &TagDefinition) -> Result<()> {
    writeln!(file, "    registry.insert(")?;
    writeln!(file, "        \"{}\",", tag.tag_name)?;
    writeln!(file, "        TagDescriptor::new(")?;

    // Tag ID
    match &tag.tag_id {
        TagId::Numeric(n) => writeln!(file, "            TagId::new_numeric(0x{:04X}),", n)?,
        TagId::Named(s) => writeln!(
            file,
            "            TagId::new_named(\"{}\".to_string()),",
            escape_string(s)
        )?,
    }

    // Tag name
    writeln!(
        file,
        "            \"{}\".to_string(),",
        escape_string(&tag.tag_name)
    )?;

    // Format family
    let family_variant = match tag.format_family.as_str() {
        "ICC_Profile" => "ICCProfile",
        other => other,
    };
    writeln!(file, "            FormatFamily::{},", family_variant)?;

    // Writable
    writeln!(file, "            {},", tag.writable)?;

    // Value type
    writeln!(file, "            ValueType::{:?},", tag.value_type)?;

    // Description
    writeln!(
        file,
        "            \"{}\".to_string(),",
        escape_string(&tag.description)
    )?;

    // Example values (generate some based on type)
    // Use single-line vec for single elements to match rustfmt formatting
    let example_value = match tag.value_type {
        ValueType::String => "\"Example\".to_string()",
        ValueType::Integer => "\"100\".to_string()",
        ValueType::Float => "\"1.5\".to_string()",
        ValueType::Rational => "\"1/100\".to_string()",
        ValueType::DateTime => "\"2024:01:01 12:00:00\".to_string()",
        _ => "\"Value\".to_string()",
    };
    writeln!(file, "            vec![{}],", example_value)?;
    writeln!(file, "        ),")?;
    writeln!(file, "    );")?;
    writeln!(file)?;

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
    writable: bool,
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
