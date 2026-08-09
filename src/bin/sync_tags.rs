//! Regenerates `oxidex-tags-*/src/*_tags.yaml` from a locally-installed
//! `exiftool` binary's own `-f -listx` tag dump.
//!
//! Usage: `cargo run --release --bin sync_tags`
//!
//! Requires `exiftool` on `PATH` (override with the `EXIFTOOL` env var).
//! Never invoked from `build.rs` or `cargo build` — this tool is run
//! explicitly by a developer or by CI.

use anyhow::{Context, Result, bail};
use oxidex::tag_sync::{
    DOMAINS, TagRecord, count_ids_in_yaml, generate_domain_yaml, parse_domain_yaml, parse_listx,
};
use std::collections::HashSet;
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

    // `.exiftool-version` is the repo's single source of truth for which
    // ExifTool release everything is graded against (see AGENTS.md). A sync
    // must run against that pin, not quietly move it to whatever `exiftool`
    // happens to resolve to on this machine -- PATH skew is exactly the
    // failure mode the pin exists to prevent. Check before any work happens
    // so a mismatched binary can never touch the YAML files.
    let pin_path = Path::new(".exiftool-version");
    let pin_missing = match fs::read_to_string(pin_path) {
        Ok(pinned) if pinned.trim() != version => bail!(
            ".exiftool-version pins {} but this exiftool is {version}. Refusing to sync \
             against an unpinned release: if the upgrade is intentional, update \
             .exiftool-version first and re-run against that release.",
            pinned.trim()
        ),
        Ok(_) => false,
        Err(_) => true,
    };

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

        let existing = if path.exists() {
            fs::read_to_string(path)
                .with_context(|| format!("failed to read existing {path_str}"))?
        } else {
            String::new()
        };
        let previous_count = count_ids_in_yaml(&existing);

        // `-listx` omits every tag with no printable value, so regenerating
        // from it alone DELETES them. That includes the SubDirectory pointers
        // oxidex needs to find anything at all -- ExifOffset (0x8769),
        // GPSInfo (0x8825), InteropOffset (0xA005) -- and tags ExifTool names
        // at runtime. Carry forward anything the fresh parse does not cover,
        // keyed by (table, id, name), so a sync adds without destroying.
        //
        // The id MUST be part of the key: real -listx output routinely lists
        // one (table, name) pair under several ids (Canon binary tables ship
        // the same display name at five different byte offsets), and a
        // (table, name)-only key made both this filter and the `lost` check
        // below blind to a dropped id variant -- `covered` claimed the name
        // was still present while a differently-numbered record vanished.
        let previous = parse_domain_yaml(&existing);
        let covered: HashSet<(&str, &str, &str)> = tags
            .iter()
            .map(|t| (t.table.as_str(), t.id.as_str(), t.name.as_str()))
            .collect();
        let preserved: Vec<TagRecord> = previous
            .iter()
            .filter(|r| !covered.contains(&(r.table.as_str(), r.id.as_str(), r.name.as_str())))
            .cloned()
            .collect();

        let mut merged = tags.clone();
        merged.extend(preserved.iter().cloned());
        let new_yaml = generate_domain_yaml(domain, &merged);
        let new_count = count_ids_in_yaml(&new_yaml);

        // A count check cannot see this: regeneration RAISES the total while
        // dropping the few structural tags that matter most. Name the losses.
        // This guard is only meaningful because generate_domain_yaml can
        // deduplicate or refuse records: with the (table, id, name) key above,
        // every `previous` record is either covered by the fresh parse or
        // carried into `merged` verbatim, so a loss here means round-tripping
        // through generate/parse dropped it.
        let reparsed = parse_domain_yaml(&new_yaml);
        let now: HashSet<(&str, &str, &str)> = reparsed
            .iter()
            .map(|t| (t.table.as_str(), t.id.as_str(), t.name.as_str()))
            .collect();
        let lost: Vec<String> = previous
            .iter()
            .filter(|r| !now.contains(&(r.table.as_str(), r.id.as_str(), r.name.as_str())))
            .map(|r| format!("{}:{}:{}", r.table, r.id, r.name))
            .collect();
        if !lost.is_empty() {
            bail!(
                "domain '{domain}' would drop {} existing tag(s) that the new parse does not \
                 cover, e.g. {:?} — refusing to write",
                lost.len(),
                &lost[..lost.len().min(8)]
            );
        }
        if !preserved.is_empty() {
            println!(
                "  {domain:12} carrying forward {} tag(s) exiftool -listx does not report",
                preserved.len()
            );
        }

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

    // The pin was checked against this binary before any work began; record
    // it only when the repo had none at all.
    if pin_missing {
        fs::write(pin_path, format!("{version}\n")).context("failed to write .exiftool-version")?;
        println!("Recorded exiftool version {version} in .exiftool-version");
    }

    Ok(())
}
