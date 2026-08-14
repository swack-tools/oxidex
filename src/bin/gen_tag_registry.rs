//! Regenerates `oxidex-tags-*/src/*_tags.yaml` from `dump_tables.pl`'s JSON
//! dump of ExifTool's real Perl tag tables.
//!
//! Usage: `cargo run --release --bin gen_tag_registry -- <dump.json>`
//!
//! Replaces the retired `sync_tags` binary, which shelled out to `exiftool -f
//! -listx` (the documentation view — see `AGENTS.md`, "Tag knowledge is not
//! tag coverage", and the module doc on `oxidex::tag_sync`). This tool takes
//! the JSON `tools/exiftool-tables/regen.sh` already produces from
//! `dump_tables.pl` for the binary-table generator, so there is one
//! extraction pass and one pin gate, not two.
//!
//! Never invoked from `build.rs` or `cargo build` — run explicitly by
//! `regen.sh`, or by a developer.

use anyhow::{Context, Result, bail};
use oxidex::tag_sync::{
    DOMAINS, count_ids_in_yaml, generate_domain_yaml, tag_records_from_dump_json,
};
use std::fs;
use std::path::Path;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let json_path = args
        .next()
        .context("usage: gen_tag_registry <dump_tables.pl JSON path>")?;

    let json =
        fs::read_to_string(&json_path).with_context(|| format!("failed to read {json_path}"))?;

    let version = serde_json::from_str::<serde_json::Value>(&json)
        .ok()
        .and_then(|v| {
            v.get("exiftool_version")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        });
    if let Some(version) = &version {
        println!("dump JSON is from ExifTool {version}");
    }

    // The dump JSON is a byproduct of `regen.sh`, which already refuses to
    // run against anything but `.exiftool-version`'s pin before it ever calls
    // `dump_tables.pl` (see regen.sh's own PIN check). Re-deriving the pin
    // path here and refusing on mismatch keeps that guarantee even if this
    // binary is ever invoked by hand against a stale cached JSON.
    let pin_path = Path::new(".exiftool-version");
    if let Ok(pinned) = fs::read_to_string(pin_path) {
        let pinned = pinned.trim();
        if let Some(version) = &version
            && version != pinned
        {
            bail!(
                ".exiftool-version pins {pinned} but {json_path} was dumped from ExifTool \
                 {version}. Refusing to regenerate the tag registry from an unpinned dump."
            );
        }
    }

    let records = tag_records_from_dump_json(&json)
        .with_context(|| format!("failed to parse {json_path} as a dump_tables.pl JSON dump"))?;
    if records.is_empty() {
        bail!("parsed zero tag records from {json_path} — refusing to overwrite YAML files");
    }
    println!("Parsed {} tag records from the dump", records.len());

    let mut writes: Vec<(&str, String, String, usize, usize)> = Vec::new();
    for domain in DOMAINS {
        let path_str = format!("oxidex-tags-{domain}/src/{domain}_tags.yaml");
        let path = Path::new(&path_str);
        let previous_count = if path.exists() {
            count_ids_in_yaml(
                &fs::read_to_string(path)
                    .with_context(|| format!("failed to read existing {path_str}"))?,
            )
        } else {
            0
        };

        let new_yaml = generate_domain_yaml(domain, &records);
        let new_count = count_ids_in_yaml(&new_yaml);
        writes.push((domain, path_str, new_yaml, previous_count, new_count));
    }

    for (domain, path_str, new_yaml, previous_count, new_count) in writes {
        let path = Path::new(&path_str);
        fs::write(path, &new_yaml).with_context(|| format!("failed to write {path_str}"))?;
        let delta = new_count as i64 - previous_count as i64;
        println!("  {domain:12} -> {path_str} ({previous_count} -> {new_count} tags, {delta:+})");
    }

    Ok(())
}
