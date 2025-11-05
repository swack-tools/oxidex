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

/// Map tag table name to domain crate
///
/// This function routes tag tables to their appropriate domain crate based on
/// the table name. This enables splitting the monolithic tag database into
/// domain-specific crates for faster parallel compilation.
///
/// Table names often include module prefixes like "Canon::AFConfig" or "EXIF::Main",
/// so we extract the base module name (first part before "::") for routing.
///
/// # Arguments
/// * `table_name` - The name of the tag table (e.g., "Canon::AFConfig", "EXIF", "QuickTime::Main")
///
/// # Returns
/// The domain name as a static string: "core", "camera", "media", "image", "document", or "specialty"
fn get_domain_for_table(table_name: &str) -> &'static str {
    // Extract base module name (part before ::)
    let base_name = table_name.split("::").next().unwrap_or(table_name);

    match base_name {
        // Core - universal standards
        "EXIF" | "XMP" | "IPTC" | "GPS" | "ICC_Profile" | "MWG" |
        "Photoshop" | "FlashPix" | "GeoTiff" | "Composite" | "Trailer" |
        "MakerNotes" => "core",

        // Camera manufacturers
        "Canon" | "CanonCustom" | "CanonRaw" | "Nikon" | "NikonCapture" |
        "NikonCustom" | "NikonSettings" | "Sony" | "SonyIDC" | "Panasonic" |
        "PanasonicRaw" | "Olympus" | "FujiFilm" | "Pentax" | "Casio" |
        "Minolta" | "MinoltaRaw" | "Ricoh" | "Sigma" | "SigmaRaw" |
        "PhaseOne" | "Kodak" | "KyoceraRaw" | "Samsung" | "Sanyo" |
        "HP" | "GE" | "Reconyx" | "JVC" | "Motorola" | "Apple" |
        "DJI" | "GoPro" | "Parrot" | "Infiray" | "FLIR" => "camera",

        // Media formats
        "QuickTime" | "Matroska" | "MPEG" | "M2TS" | "MXF" | "FLAC" |
        "AAC" | "AIFF" | "Vorbis" | "Opus" | "ID3" | "APE" | "ASF" |
        "Flash" | "Real" | "Theora" | "H264" | "WavPack" | "MPC" |
        "DSF" | "WTV" => "media",

        // Image formats
        "PNG" | "GIF" | "JPEG" | "JPEG2000" | "BMP" | "TIFF" | "DNG" |
        "MNG" | "PGF" | "PICT" | "OpenEXR" | "FLIF" | "BPG" | "WebP" |
        "DPX" | "PSP" | "PCX" | "MIFF" | "PhotoCD" | "ICO" | "Palm" => "image",

        // Document formats
        "PDF" | "PostScript" | "Font" | "PLIST" | "HTML" | "Torrent" |
        "ZIP" | "TNEF" | "VCard" | "Microsoft" | "MacOS" | "EXE" |
        "Lnk" | "RSRC" | "FotoStation" | "PhotoMechanic" | "ITC" |
        "GIMP" | "GM" | "Google" => "document",

        // Specialty/scientific
        "DICOM" | "FITS" | "MRC" | "STIM" | "PCAP" | "XISF" | "MISB" |
        "DjVu" | "ISO" | "Nintendo" => "specialty",

        // Default to core for unknown
        _ => "core",
    }
}

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
    let modules = discover_all_modules(&lib_dir).context("Failed to discover ExifTool modules")?;

    println!(
        "cargo:warning=Parsing {} modules for tag definitions...",
        modules.len()
    );

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
            table_declaration: Regex::new(r"%Image::ExifTool::(\w+(?:::\w+)*)\s*=\s*\(")?,
            hash_tag_def: Regex::new(r"^\s*(0x[0-9a-fA-F]+|'[^']*'|\d+)\s*=>\s*\{")?,
            simple_tag_def: Regex::new(r#"^\s*(0x[0-9a-fA-F]+|\d+)\s*=>\s*'([^']+)'[\s,]*$"#)?,
            name_field: Regex::new(r#"Name\s*=>\s*'([^']+)'"#)?,
            writable_field: Regex::new(r#"Writable\s*=>\s*'?([^',}\s]+)"#)?,
            description_field: Regex::new(r#"Description\s*=>\s*'([^']+)'"#)?,
            format_field: Regex::new(r#"Format\s*=>\s*'([^']+)'"#)?,
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
    s.bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32))
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

/// Generate YAML for all tags in a domain
///
/// This function creates a compact YAML representation of all tags belonging to a specific
/// domain. The YAML is structured as a list of tables, each containing a list of tags.
///
/// # Arguments
/// * `domain` - The domain name ("core", "camera", "media", "image", "document", "specialty")
/// * `tags` - Vector of all parsed tag definitions
///
/// # Returns
/// YAML string containing all tags for the specified domain
///
/// # YAML Structure
/// ```yaml
/// tables:
///   - name: Canon
///     tags:
///       - id: "0x0001"
///         name: CanonCameraSettings
///         writable: true
///         type: int16u
///         description: "Camera settings"
/// ```
fn generate_domain_yaml(domain: &str, tags: &[TagDefinition]) -> Result<String> {
    let mut yaml = String::from("tables:\n");

    // Group tags by table name within this domain
    let mut tables_map: HashMap<String, Vec<&TagDefinition>> = HashMap::new();
    for tag in tags {
        // Only include tags that belong to this domain
        if get_domain_for_table(&tag.table_name) == domain {
            tables_map
                .entry(tag.table_name.clone())
                .or_default()
                .push(tag);
        }
    }

    // If no tables for this domain, return minimal valid YAML
    if tables_map.is_empty() {
        return Ok(yaml);
    }

    // Sort table names for consistent output
    let mut table_names: Vec<_> = tables_map.keys().collect();
    table_names.sort();

    // Generate YAML for each table
    for table_name in table_names {
        let table_tags = tables_map.get(table_name).unwrap();

        yaml.push_str(&format!("  - name: {}\n", table_name));
        yaml.push_str("    tags:\n");

        for tag in table_tags {
            // Generate tag entry
            // Tag ID - convert to string representation
            let tag_id_str = match &tag.tag_id {
                TagId::Numeric(n) => format!("0x{:04X}", n),
                TagId::Named(s) => s.clone(),
            };

            yaml.push_str(&format!("      - id: \"{}\"\n", tag_id_str));

            // Tag name - use only the name part without the format family prefix
            let tag_name = if let Some(colon_pos) = tag.tag_name.find(':') {
                &tag.tag_name[colon_pos + 1..]
            } else {
                &tag.tag_name
            };
            // Quote the tag name to handle special YAML characters (colons, quotes, etc.)
            let escaped_name = tag_name.replace('\\', "\\\\").replace('"', "\\\"");
            yaml.push_str(&format!("        name: \"{}\"\n", escaped_name));

            // Writable flag
            yaml.push_str(&format!("        writable: {}\n", tag.writable));

            // Optional type field
            if let Some(ref type_name) = tag.writable_type {
                yaml.push_str(&format!("        type: {}\n", type_name));
            }

            // Optional description field
            if !tag.description.is_empty() {
                // Escape YAML special characters in description
                let escaped = tag.description
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', " ")
                    .replace('\r', "");
                yaml.push_str(&format!("        description: \"{}\"\n", escaped));
            }
        }
    }

    Ok(yaml)
}

/// Generates YAML files for each domain crate
///
/// This function replaces the old Rust code generation approach with YAML generation.
/// Each domain crate gets its own YAML file containing only the tags for that domain.
/// This approach provides:
/// - Faster compilation (YAML parsing vs Rust compilation)
/// - Better parallel builds (6 independent crates)
/// - Easier debugging (human-readable YAML)
///
/// # Arguments
/// * `tags` - Vector of all parsed tag definitions from ExifTool
///
/// # Returns
/// Ok(()) on success, Error if file writing fails
fn generate_rust_code(tags: &[TagDefinition]) -> Result<()> {
    println!(
        "cargo:warning=Generating YAML files for {} tags across 6 domain crates",
        tags.len()
    );

    // Define all domains
    let domains = ["core", "camera", "media", "image", "document", "specialty"];

    let mut total_generated = 0;

    // Generate YAML for each domain
    for domain in &domains {
        let yaml_content = generate_domain_yaml(domain, tags)?;

        // Determine output path for this domain
        let output_path = format!("exiftool-tags-{}/src/{}_tags.yaml", domain, domain);

        // Write YAML file
        fs::write(&output_path, &yaml_content)
            .with_context(|| format!("Failed to write YAML to {}", output_path))?;

        // Count tags in this domain for reporting
        let tag_count = tags
            .iter()
            .filter(|tag| get_domain_for_table(&tag.table_name) == *domain)
            .count();

        total_generated += tag_count;

        println!(
            "cargo:warning=  {:12} -> {} ({} tags, {} bytes)",
            domain,
            output_path,
            tag_count,
            yaml_content.len()
        );
    }

    // Still generate the legacy generated_tags.rs for backward compatibility
    // This ensures existing code continues to work during migration
    generate_legacy_rust_stub(tags)?;

    println!(
        "cargo:warning=Successfully generated YAML for {} total tags",
        total_generated
    );

    Ok(())
}

/// Generates a minimal stub for legacy generated_tags.rs
///
/// This provides backward compatibility during the migration to YAML-based tags.
/// The stub maintains the same API but logs a deprecation warning.
fn generate_legacy_rust_stub(tags: &[TagDefinition]) -> Result<()> {
    let output_path = Path::new(GENERATED_TAGS_PATH);

    // Create parent directory if needed
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = File::create(output_path)?;

    // Write minimal stub that maintains API compatibility
    writeln!(file, "//! Legacy stub for generated tag database")?;
    writeln!(file, "//!")?;
    writeln!(file, "//! THIS FILE IS AUTO-GENERATED BY build.rs")?;
    writeln!(file, "//! Tag data is now in YAML files in domain crates.")?;
    writeln!(file, "//!")?;
    writeln!(file, "//! Generated from ExifTool: {}", EXIFTOOL_REPO_URL)?;
    writeln!(file, "//! Total tags: {}", tags.len())?;
    writeln!(file)?;
    writeln!(file, "#![allow(dead_code)]")?;
    writeln!(file)?;
    writeln!(file, "use crate::core::TagDescriptor;")?;
    writeln!(file)?;
    writeln!(
        file,
        "/// Legacy lookup function (now delegates to manual registry)"
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
        "/// Returns the total number of tags (from ExifTool parse)"
    )?;
    writeln!(file, "pub fn generated_tag_count() -> usize {{")?;
    writeln!(file, "    {}", tags.len())?;
    writeln!(file, "}}")?;

    println!(
        "cargo:warning=Generated legacy stub at {}",
        GENERATED_TAGS_PATH
    );
    Ok(())
}

// Removed unused Rust code generation functions:
// - generate_family_module (YAML generation replaced Rust codegen)
// - sanitize_module_name (YAML generation replaced Rust codegen)
// - generate_tag_array_entry (YAML generation replaced Rust codegen)
// - escape_string (YAML generation replaced Rust codegen)

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
    writeln!(file, "use crate::core::TagDescriptor;")?;
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
    #[allow(dead_code)] // Used in old Rust codegen, kept for ExifTool parsing
    format_family: String,
    table_name: String, // NEW: track which table this tag came from
    writable: bool,
    writable_type: Option<String>, // NEW: track writable type (int16u, string, etc)
    #[allow(dead_code)] // Used in old Rust codegen, kept for ExifTool parsing
    value_type: ValueType,
    description: String,
}

/// Tag ID enum (matches src/core/tag_descriptor.rs)
#[derive(Debug, Clone)]
enum TagId {
    Numeric(u16),
    #[allow(dead_code)] // Used in old Rust codegen, kept for future extensibility
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
