//! Tag Comparison Binary
//!
//! Command-line tool to compare tags extracted by OxiDex vs ExifTool

use clap::Parser;
use std::path::PathBuf;
use std::process::Command;

mod comparison;
mod extraction;
mod models;

use comparison::{ComparisonEngine, generate_markdown_reports};
use extraction::{ExifToolExtractor, OxiDexExtractor};
use models::ComparisonReport;

#[derive(Parser, Debug)]
#[command(name = "tag-comparison")]
#[command(about = "Compare tags extracted by OxiDex vs ExifTool", long_about = None)]
struct Args {
    /// Path to test samples directory
    #[arg(long, alias = "fixture-path", default_value = "tests/fixtures")]
    samples: PathBuf,

    /// Specific format to process (if not specified, all formats)
    #[arg(long)]
    format: Option<String>,

    /// Output file for JSON results
    #[arg(short, long, default_value = "comparison.json")]
    output: PathBuf,

    /// Path to baseline.json for regression detection
    #[arg(long)]
    baseline: Option<PathBuf>,

    /// Path to exiftool executable
    #[arg(long, default_value = "exiftool")]
    exiftool: String,

    /// Output directory for markdown reports
    #[arg(long, default_value = "docs/reference/comparison")]
    markdown_dir: PathBuf,

    /// ExifTool version string (for report metadata); auto-detected via
    /// `exiftool -ver` when omitted
    #[arg(long)]
    exiftool_version: Option<String>,

    /// OxiDex version string (for report metadata); defaults to this
    /// binary's own Cargo package version when omitted
    #[arg(long)]
    oxidex_version: Option<String>,
}

/// Runs `<exiftool> -ver` and returns its trimmed stdout, or "unknown" if
/// the binary can't be found or fails.
fn detect_exiftool_version(exiftool: &str) -> String {
    Command::new(exiftool)
        .arg("-ver")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let exiftool_version = args
        .exiftool_version
        .clone()
        .unwrap_or_else(|| detect_exiftool_version(&args.exiftool));
    let oxidex_version = args
        .oxidex_version
        .clone()
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());

    println!("🏷️  Tag Comparison Tool");
    println!("=======================\n");
    println!("ExifTool: v{}", exiftool_version);
    println!("OxiDex: v{}", oxidex_version);
    println!("Samples: {}", args.samples.display());
    println!();

    // Load baseline for regression detection
    let baseline: Option<ComparisonReport> = args.baseline.as_ref().and_then(|path| {
        if path.exists() {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|content| serde_json::from_str(&content).ok())
        } else {
            None
        }
    });

    // Create report
    let mut report = ComparisonReport::new();
    report.exiftool_version = exiftool_version.clone();
    report.oxidex_version = oxidex_version.clone();

    // Auto-detect formats from samples directory
    let formats = if let Some(format) = args.format {
        vec![format]
    } else {
        detect_formats(&args.samples)?
    };

    println!("Found {} formats to process\n", formats.len());

    // Process each format
    for format in formats {
        println!("Processing format: {}", format);

        // Extract OxiDex tags
        let t_oxidex = std::time::Instant::now();
        let mut oxidex_extractor = OxiDexExtractor::new(args.samples.clone());
        match oxidex_extractor.extract_format_tags(&format).await {
            Ok(oxidex_result) => {
                println!(
                    "  OxiDex found {} tags from {} files [{:.2}s]",
                    oxidex_result.tags.len(),
                    oxidex_result.files_processed,
                    t_oxidex.elapsed().as_secs_f64()
                );

                // Extract ExifTool tags
                let t_exiftool = std::time::Instant::now();
                let mut exiftool_extractor = ExifToolExtractor::new(args.exiftool.clone());
                match exiftool_extractor
                    .extract_format_tags(&format, &args.samples)
                    .await
                {
                    Ok(exiftool_result) => {
                        println!(
                            "  ExifTool found {} tags from {} files [{:.2}s]",
                            exiftool_result.tags.len(),
                            exiftool_result.files_processed,
                            t_exiftool.elapsed().as_secs_f64()
                        );

                        // Use the max files processed from both extractors
                        let files_tested = oxidex_result
                            .files_processed
                            .max(exiftool_result.files_processed);

                        // Compare with baseline for regression detection
                        let t_compare = std::time::Instant::now();
                        let previous = baseline.as_ref().and_then(|b| b.by_format.get(&format));
                        let comparison = ComparisonEngine::compare(
                            oxidex_result.tags,
                            exiftool_result.tags,
                            &format,
                            files_tested,
                            previous,
                        );
                        println!(
                            "  Result: {} [compare {:.2}s]",
                            comparison.summary(),
                            t_compare.elapsed().as_secs_f64()
                        );

                        report.add_format(format, comparison);
                    }
                    Err(e) => eprintln!("  Error extracting ExifTool tags: {}", e),
                }
            }
            Err(e) => eprintln!("  Error extracting OxiDex tags: {}", e),
        }
    }

    // Calculate overall coverage
    report.calculate_overall_coverage();

    println!("\n📊 Overall Results");
    println!("==================");
    println!("{}", report.summary);

    // Output results
    let t_json = std::time::Instant::now();
    let json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&args.output, json)?;
    println!(
        "\n✅ Results saved to: {} [{:.2}s]",
        args.output.display(),
        t_json.elapsed().as_secs_f64()
    );

    // Generate markdown reports
    let t_md = std::time::Instant::now();
    println!("\n📝 Generating markdown reports...");
    generate_markdown_reports(&report, &args.markdown_dir)?;
    println!(
        "✅ Markdown reports saved to: {} [{:.2}s]",
        args.markdown_dir.display(),
        t_md.elapsed().as_secs_f64()
    );

    // Save updated baseline
    if let Some(baseline_path) = &args.baseline {
        let baseline_json = serde_json::to_string_pretty(&report)?;
        if let Some(parent) = baseline_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(baseline_path, baseline_json)?;
        println!("✅ Baseline updated: {}", baseline_path.display());
    }

    Ok(())
}

fn detect_formats(
    samples_path: &std::path::Path,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use std::collections::HashSet;
    let mut formats = HashSet::new();

    // Recursively scan all files to detect formats by extension
    fn scan_directory(dir: &std::path::Path, formats: &mut HashSet<String>) -> std::io::Result<()> {
        if dir.is_dir() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_dir() {
                    // Skip hidden directories
                    if !path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("."))
                    {
                        scan_directory(&path, formats)?;
                    }
                } else if path.is_file()
                    && let Some(ext) = path.extension().and_then(|e| e.to_str())
                    && let Some(format) = extension_to_format(ext)
                {
                    formats.insert(format.to_string());
                }
            }
        }
        Ok(())
    }

    scan_directory(samples_path, &mut formats)?;

    let mut sorted: Vec<_> = formats.into_iter().collect();
    sorted.sort();
    Ok(sorted)
}

/// Map file extension to format name
fn extension_to_format(ext: &str) -> Option<&'static str> {
    match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" => Some("JPEG"),
        "png" => Some("PNG"),
        "tif" | "tiff" => Some("TIFF"),
        "gif" => Some("GIF"),
        "webp" => Some("WEBP"),
        "heic" | "heif" => Some("HEIC"),
        "mp4" | "m4v" | "mov" => Some("MP4"),
        "avi" => Some("AVI"),
        "mkv" => Some("MKV"),
        "mp3" => Some("MP3"),
        "wav" => Some("WAV"),
        "pdf" => Some("PDF"),
        "psd" => Some("PSD"),
        "cr2" | "cr3" => Some("CR2"),
        "nef" => Some("NEF"),
        "arw" => Some("ARW"),
        "dng" => Some("DNG"),
        "raf" => Some("RAF"),
        "orf" => Some("ORF"),
        "rw2" => Some("RW2"),
        "xmp" => Some("XMP"),
        "flac" => Some("FLAC"),
        "ogg" | "oga" | "ogv" => Some("OGG"),
        "bmp" => Some("BMP"),
        "ico" => Some("ICO"),
        "svg" => Some("SVG"),
        "eps" | "ps" => Some("EPS"),
        "exr" => Some("EXR"),
        "jxl" => Some("JXL"),
        "avif" => Some("AVIF"),
        "3gp" | "3g2" => Some("3GP"),
        "flv" => Some("FLV"),
        "wmv" | "asf" => Some("WMV"),
        "mxf" => Some("MXF"),
        "webm" => Some("WEBM"),
        "icc" | "icm" => Some("ICC"),
        "pef" => Some("PEF"),
        "srw" => Some("SRW"),
        "x3f" => Some("X3F"),
        "dcr" => Some("DCR"),
        "rwl" => Some("RWL"),
        "3fr" => Some("3FR"),
        "fff" => Some("FFF"),
        "mef" => Some("MEF"),
        "mos" => Some("MOS"),
        "mrw" => Some("MRW"),
        "nrw" => Some("NRW"),
        "sr2" | "srf" => Some("SR2"),
        "kdc" => Some("KDC"),
        "erf" => Some("ERF"),
        // Executables/libraries/fonts/documents/archives -- detection for
        // all of these is magic-byte-based in src/parsers/detection, not
        // extension-based, so these mappings only serve this comparison
        // tool's own file discovery; oxidex would recognize any of these
        // formats regardless of what extension the file actually has.
        "exe" | "dll" | "sys" => Some("PE"),
        "elf" | "so" => Some("ELF"),
        // Tag group prefix oxidex actually emits is "MachO" (no hyphen),
        // unlike FileFormat::MachO.name()'s display string "Mach-O" --
        // using the tag-prefix spelling here so this stays the identity
        // tag-comparison groups its extracted tags under.
        "dylib" | "bundle" | "macho" => Some("MachO"),
        "otf" => Some("OTF"),
        "ttf" => Some("TTF"),
        "woff" => Some("WOFF"),
        "woff2" => Some("WOFF2"),
        "docx" => Some("DOCX"),
        "xlsx" => Some("XLSX"),
        "pptx" => Some("PPTX"),
        "zip" => Some("ZIP"),
        "rar" => Some("RAR"),
        "7z" => Some("7z"),
        "gz" => Some("GZIP"),
        "tar" => Some("TAR"),
        "iso" => Some("ISO"),
        "doc" | "xls" | "ppt" | "msg" | "vsd" | "pub" => Some("OLE"),
        _ => None,
    }
}
