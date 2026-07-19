//! Regenerates `oxidex-tags-*/src/*_tags.yaml` from a locally-installed
//! `exiftool` binary's own `-f -listx` tag dump.
//!
//! Usage: `cargo run --release --bin sync_tags`
//!
//! Requires `exiftool` on `PATH` (override with the `EXIFTOOL` env var).
//! Never invoked from `build.rs` or `cargo build` — this tool is run
//! explicitly by a developer or by CI.

use anyhow::{Context, Result, bail};
use oxidex::tag_sync::{DOMAINS, count_ids_in_yaml, generate_domain_yaml, parse_listx};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Below this fraction of the previous tag count for a domain, refuse to
/// write — likely signals a parsing regression rather than a genuine drop
/// in ExifTool's own tag count.
const MIN_RETENTION_FRACTION: f64 = 0.9;

fn exiftool_bin() -> String {
    std::env::var("EXIFTOOL").unwrap_or_else(|_| "exiftool".to_string())
}

fn run_exiftool(args: &[&str]) -> Result<String> {
    let bin = exiftool_bin();
    let output = Command::new(&bin)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute `{bin}` (is it on PATH?)"))?;

    if !output.status.success() {
        bail!(
            "`{bin} {}` exited with {}: {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    String::from_utf8(output.stdout).context("exiftool output was not valid UTF-8")
}

fn main() -> Result<()> {
    let version = run_exiftool(&["-ver"])?.trim().to_string();
    if version.is_empty() {
        bail!("exiftool -ver returned an empty version string");
    }
    println!("Using exiftool {version}");

    let listx = run_exiftool(&["-f", "-listx"])?;
    let tags = parse_listx(&listx).context("failed to parse exiftool -listx output")?;
    if tags.is_empty() {
        bail!("parsed zero tags from exiftool -listx output — refusing to overwrite YAML files");
    }
    println!("Parsed {} tags from exiftool -listx", tags.len());

    // First pass: generate and check all domains, collecting results in memory.
    // If any domain fails its retention check, we bail here before writing anything to disk.
    let mut writes: Vec<(&str, String, String, usize, usize)> = Vec::new();

    for domain in DOMAINS {
        let path_str = format!("oxidex-tags-{domain}/src/{domain}_tags.yaml");
        let path = Path::new(&path_str);

        let previous_count = if path.exists() {
            let existing = fs::read_to_string(path)
                .with_context(|| format!("failed to read existing {path_str}"))?;
            count_ids_in_yaml(&existing)
        } else {
            0
        };

        let new_yaml = generate_domain_yaml(domain, &tags);
        let new_count = count_ids_in_yaml(&new_yaml);

        if previous_count > 0 {
            let retention = new_count as f64 / previous_count as f64;
            if retention < MIN_RETENTION_FRACTION {
                bail!(
                    "domain '{domain}' would drop from {previous_count} to {new_count} tags \
                     ({:.1}% retained, below the {:.0}% floor) — refusing to write, this looks \
                     like a parsing regression",
                    retention * 100.0,
                    MIN_RETENTION_FRACTION * 100.0
                );
            }
        }

        writes.push((domain, path_str, new_yaml, previous_count, new_count));
    }

    // Second pass: write all domains to disk only after every domain has passed its check.
    for (domain, path_str, new_yaml, previous_count, new_count) in writes {
        let path = Path::new(&path_str);
        fs::write(path, &new_yaml).with_context(|| format!("failed to write {path_str}"))?;
        println!("  {domain:12} -> {path_str} ({previous_count} -> {new_count} tags)");
    }

    fs::write(".exiftool-version", format!("{version}\n"))
        .context("failed to write .exiftool-version")?;
    println!("Recorded exiftool version {version} in .exiftool-version");

    Ok(())
}
