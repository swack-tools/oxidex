//! Baseline Generation Tool for ExifTool-RS Integration Tests
//!
//! This tool generates baseline metadata outputs for all test fixtures by:
//! 1. Executing Perl ExifTool on all test images to get JSON outputs
//! 2. Executing ExifTool-RS on all test images to get JSON outputs
//! 3. Comparing outputs and calculating match rates
//! 4. Generating a baseline_metadata.json file with results
//!
//! ## Usage
//!
//! ```bash
//! # Generate initial baseline
//! cargo run --bin generate_baseline -- --input tests/fixtures/ --output tests/baselines/
//!
//! # Update existing baseline
//! cargo run --bin generate_baseline -- --update
//!
//! # Generate baseline for specific format
//! cargo run --bin generate_baseline -- --input tests/fixtures/jpeg --output tests/baselines/jpeg
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Serialize, Deserialize)]
struct BaselineMetadata {
    version: String,
    exiftool_version: String,
    exiftool_rs_version: String,
    generated_at: String,
    images: Vec<ImageBaseline>,
    overall_match_rate: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ImageBaseline {
    path: String,
    perl_tags: usize,
    rust_tags: usize,
    match_rate: f64,
    discrepancies: Vec<Discrepancy>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Discrepancy {
    tag: String,
    perl_value: String,
    rust_value: String,
    reason: Option<String>,
}

/// Checks if Perl ExifTool is available
fn is_exiftool_available() -> bool {
    Command::new("exiftool")
        .arg("-ver")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Gets Perl ExifTool version
fn get_exiftool_version() -> Result<String, String> {
    let output = Command::new("exiftool")
        .arg("-ver")
        .output()
        .map_err(|e| format!("Failed to get ExifTool version: {}", e))?;

    if !output.status.success() {
        return Err("ExifTool -ver command failed".to_string());
    }

    String::from_utf8(output.stdout)
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("Invalid UTF-8 in ExifTool version: {}", e))
}

/// Executes Perl ExifTool and captures JSON output
fn get_perl_exiftool_output(file_path: &Path) -> Result<String, String> {
    let output = Command::new("exiftool")
        .arg("-json")
        .arg("-a")
        .arg("-G1")
        .arg("-struct")
        .arg(file_path)
        .output()
        .map_err(|e| format!("Failed to execute Perl ExifTool: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Perl ExifTool failed on {:?}: {}",
            file_path, stderr
        ));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8 in Perl ExifTool output: {}", e))
}

/// Executes ExifTool-RS and captures JSON output
fn get_exiftool_rs_output(file_path: &Path) -> Result<String, String> {
    // Find the exiftool-rs binary in target directory
    let cargo_target_dir =
        std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());

    let binary_path = PathBuf::from(&cargo_target_dir)
        .join("release")
        .join("exiftool-rs");

    if !binary_path.exists() {
        return Err(format!(
            "ExifTool-RS binary not found at {:?}. Run 'cargo build --release' first.",
            binary_path
        ));
    }

    let output = Command::new(&binary_path)
        .arg("--json")
        .arg(file_path)
        .output()
        .map_err(|e| format!("Failed to execute ExifTool-RS: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ExifTool-RS failed on {:?}: {}", file_path, stderr));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8 in ExifTool-RS output: {}", e))
}

/// Compares two JSON outputs and calculates match rate
fn compare_outputs(
    perl_json: &str,
    rust_json: &str,
) -> Result<(usize, usize, f64, Vec<Discrepancy>), String> {
    let perl_data: Vec<HashMap<String, Value>> =
        serde_json::from_str(perl_json).map_err(|e| format!("Failed to parse Perl JSON: {}", e))?;

    let rust_data: Vec<HashMap<String, Value>> =
        serde_json::from_str(rust_json).map_err(|e| format!("Failed to parse Rust JSON: {}", e))?;

    if perl_data.is_empty() || rust_data.is_empty() {
        return Ok((0, 0, 0.0, Vec::new()));
    }

    let perl_tags = &perl_data[0];
    let rust_tags = &rust_data[0];

    // Filter out system/file/composite tags
    let perl_filtered: HashMap<_, _> = perl_tags
        .iter()
        .filter(|(k, _)| !should_skip_tag(k))
        .collect();

    let rust_filtered: HashMap<_, _> = rust_tags
        .iter()
        .filter(|(k, _)| !should_skip_tag(k))
        .collect();

    let mut matched = 0;
    let mut discrepancies = Vec::new();

    for (key, perl_value) in &perl_filtered {
        if let Some(rust_value) = rust_filtered.get(key) {
            if values_match(perl_value, rust_value) {
                matched += 1;
            } else {
                discrepancies.push(Discrepancy {
                    tag: (*key).to_string(),
                    perl_value: format!("{:?}", perl_value),
                    rust_value: format!("{:?}", rust_value),
                    reason: None,
                });
            }
        } else {
            discrepancies.push(Discrepancy {
                tag: (*key).to_string(),
                perl_value: format!("{:?}", perl_value),
                rust_value: "MISSING".to_string(),
                reason: None,
            });
        }
    }

    let total = perl_filtered.len();
    let match_rate = if total > 0 {
        (matched as f64 / total as f64) * 100.0
    } else {
        100.0
    };

    Ok((total, matched, match_rate, discrepancies))
}

/// Determines if a tag should be skipped
fn should_skip_tag(tag_name: &str) -> bool {
    tag_name.starts_with("System:")
        || tag_name.starts_with("File:")
        || tag_name.starts_with("ExifTool:")
        || tag_name.starts_with("Composite:")
        || tag_name == "SourceFile"
}

/// Simple value comparison
fn values_match(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::String(s1), Value::String(s2)) => s1 == s2,
        (Value::Number(n1), Value::Number(n2)) => {
            if let (Some(i1), Some(i2)) = (n1.as_i64(), n2.as_i64()) {
                i1 == i2
            } else if let (Some(f1), Some(f2)) = (n1.as_f64(), n2.as_f64()) {
                (f1 - f2).abs() < 0.0001
            } else {
                false
            }
        }
        (Value::Bool(b1), Value::Bool(b2)) => b1 == b2,
        (Value::Array(a1), Value::Array(a2)) => {
            a1.len() == a2.len()
                && a1
                    .iter()
                    .zip(a2.iter())
                    .all(|(v1, v2)| values_match(v1, v2))
        }
        (Value::Object(o1), Value::Object(o2)) => {
            o1.len() == o2.len()
                && o1
                    .iter()
                    .all(|(k, v1)| o2.get(k).map(|v2| values_match(v1, v2)).unwrap_or(false))
        }
        (Value::Null, Value::Null) => true,
        _ => false,
    }
}

/// Finds all test image files recursively
fn find_test_images(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut images = Vec::new();
    let extensions = [
        "jpg", "jpeg", "png", "tif", "tiff", "pdf", "mp4", "webp", "heic", "heif", "avif",
    ];

    fn visit_dirs(
        dir: &Path,
        images: &mut Vec<PathBuf>,
        extensions: &[&str],
    ) -> std::io::Result<()> {
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    visit_dirs(&path, images, extensions)?;
                } else if let Some(ext) = path.extension() {
                    if extensions.contains(&ext.to_str().unwrap_or("").to_lowercase().as_str()) {
                        images.push(path);
                    }
                }
            }
        }
        Ok(())
    }

    visit_dirs(dir, &mut images, &extensions)
        .map_err(|e| format!("Failed to traverse directory: {}", e))?;

    Ok(images)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    // Parse command line arguments
    let input_dir = if let Some(idx) = args.iter().position(|a| a == "--input") {
        args.get(idx + 1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("tests/fixtures"))
    } else if args.contains(&"--update".to_string()) {
        PathBuf::from("tests/fixtures")
    } else {
        PathBuf::from("tests/fixtures")
    };

    let output_dir = if let Some(idx) = args.iter().position(|a| a == "--output") {
        args.get(idx + 1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("tests/baselines"))
    } else if args.contains(&"--update".to_string()) {
        PathBuf::from("tests/baselines")
    } else {
        PathBuf::from("tests/baselines")
    };

    println!("ExifTool-RS Baseline Generation Tool");
    println!("====================================\n");

    // Check for Perl ExifTool
    if !is_exiftool_available() {
        eprintln!("ERROR: Perl ExifTool not found in PATH");
        eprintln!("Please install it first:");
        eprintln!("  - Ubuntu/Debian: sudo apt-get install libimage-exiftool-perl");
        eprintln!("  - macOS: brew install exiftool");
        eprintln!("  - Windows: choco install exiftool");
        std::process::exit(1);
    }

    let exiftool_version = get_exiftool_version()?;
    println!("Perl ExifTool version: {}", exiftool_version);

    // Get ExifTool-RS version
    let exiftool_rs_version = env!("CARGO_PKG_VERSION").to_string();
    println!("ExifTool-RS version: {}", exiftool_rs_version);

    // Create output directory
    fs::create_dir_all(&output_dir)?;

    // Find all test images
    println!("\nScanning for test images in: {}", input_dir.display());
    let test_images = find_test_images(&input_dir)?;
    println!("Found {} test images", test_images.len());

    if test_images.is_empty() {
        eprintln!("\nERROR: No test images found in {}", input_dir.display());
        std::process::exit(1);
    }

    // Process each image
    let mut image_baselines = Vec::new();
    let mut total_match_rate = 0.0;

    for (idx, image_path) in test_images.iter().enumerate() {
        let relative_path = image_path
            .strip_prefix(&input_dir)
            .unwrap_or(image_path)
            .to_string_lossy()
            .to_string();

        print!(
            "[{}/{}] Processing: {} ... ",
            idx + 1,
            test_images.len(),
            relative_path
        );
        std::io::stdout().flush()?;

        match process_image(image_path, &output_dir, &relative_path) {
            Ok((perl_tags, rust_tags, match_rate, discrepancies)) => {
                println!(
                    "{:.1}% match ({}/{} tags)",
                    match_rate,
                    perl_tags - discrepancies.len(),
                    perl_tags
                );

                total_match_rate += match_rate;

                image_baselines.push(ImageBaseline {
                    path: relative_path,
                    perl_tags,
                    rust_tags,
                    match_rate,
                    discrepancies,
                });
            }
            Err(e) => {
                println!("FAILED: {}", e);
            }
        }
    }

    // Calculate overall match rate
    let overall_match_rate = if !image_baselines.is_empty() {
        total_match_rate / image_baselines.len() as f64
    } else {
        0.0
    };

    // Generate baseline metadata
    let baseline = BaselineMetadata {
        version: "1.0.0".to_string(),
        exiftool_version,
        exiftool_rs_version,
        generated_at: chrono::Utc::now().to_rfc3339(),
        images: image_baselines,
        overall_match_rate,
    };

    // Write baseline_metadata.json
    let metadata_path = output_dir.join("baseline_metadata.json");
    let metadata_file = File::create(&metadata_path)?;
    serde_json::to_writer_pretty(metadata_file, &baseline)?;

    println!("\n====================================");
    println!("Baseline generation complete!");
    println!("Overall match rate: {:.2}%", overall_match_rate);
    println!("Baseline metadata: {}", metadata_path.display());
    println!("====================================");

    Ok(())
}

fn process_image(
    image_path: &Path,
    output_dir: &Path,
    relative_path: &str,
) -> Result<(usize, usize, f64, Vec<Discrepancy>), String> {
    // Get Perl ExifTool output
    let perl_json = get_perl_exiftool_output(image_path)?;

    // Get ExifTool-RS output
    let rust_json = get_exiftool_rs_output(image_path)?;

    // Save outputs to baseline directory
    let image_output_dir =
        output_dir.join(Path::new(relative_path).parent().unwrap_or(Path::new("")));
    fs::create_dir_all(&image_output_dir)
        .map_err(|e| format!("Failed to create output directory: {}", e))?;

    let base_name = Path::new(relative_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    let perl_output_path = image_output_dir.join(format!("{}.perl.json", base_name));
    let rust_output_path = image_output_dir.join(format!("{}.rust.json", base_name));

    fs::write(&perl_output_path, &perl_json)
        .map_err(|e| format!("Failed to write Perl output: {}", e))?;
    fs::write(&rust_output_path, &rust_json)
        .map_err(|e| format!("Failed to write Rust output: {}", e))?;

    // Compare outputs
    let (perl_tags, _matched, match_rate, discrepancies) = compare_outputs(&perl_json, &rust_json)?;

    // Count rust tags
    let rust_data: Vec<HashMap<String, Value>> = serde_json::from_str(&rust_json)
        .map_err(|e| format!("Failed to parse Rust JSON: {}", e))?;
    let rust_tags = rust_data.get(0).map(|m| m.len()).unwrap_or(0);

    Ok((perl_tags, rust_tags, match_rate, discrepancies))
}
