//! Generate all six internal tag databases from one freshly captured, pinned
//! `dump_tables.pl` document. `-listx` remains an independent test oracle.

use anyhow::{Context, Result, bail, ensure};
use oxidex::tag_sync::{
    DOMAINS, count_ids_in_yaml, generate_domain_yaml, tag_records_from_dump_document,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Authenticate the complete loaded source closure before projecting any
/// registry rows. ExifTool.pm alone cannot identify a dump: most tag tables
/// live in separate module files, and a stale module dump must never replace
/// the six domain registries.
fn verify_dump_source_modules(doc: &Value, source_lib: &Path) -> Result<()> {
    let selected_root = source_lib
        .canonicalize()
        .with_context(|| format!("canonicalizing {}", source_lib.display()))?;
    let loaded = doc["native_capture_context"]["loaded_closure"]["modules"]
        .as_array()
        .context("table dump lacks loaded module source provenance")?;
    ensure!(
        !loaded.is_empty(),
        "table dump has no loaded module provenance"
    );
    let mut verified = HashMap::new();
    for fact in loaded {
        let rel = fact["source_file"]
            .as_str()
            .context("loaded module lacks a source file")?;
        let inc = fact["inc"]
            .as_str()
            .context("loaded module lacks an include name")?;
        ensure!(rel == inc, "loaded module source and include path disagree");
        let rel_path = Path::new(rel);
        ensure!(
            !rel_path.is_absolute()
                && rel_path
                    .components()
                    .all(|component| matches!(component, std::path::Component::Normal(_)))
                && (rel == "Image/ExifTool.pm" || rel.starts_with("Image/ExifTool/"))
                && (rel.ends_with(".pm") || rel.ends_with(".pl")),
            "loaded module has an unsafe source path: {rel}"
        );
        let expected = fact["source_sha256"]
            .as_str()
            .context("loaded module lacks a source SHA-256")?;
        ensure!(
            expected.len() == 64 && expected.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "loaded module has an invalid source SHA-256: {rel}"
        );
        let actual_path = selected_root
            .join(rel_path)
            .canonicalize()
            .with_context(|| {
                format!(
                    "resolving selected module source {}",
                    selected_root.join(rel).display()
                )
            })?;
        ensure!(
            actual_path.starts_with(&selected_root),
            "loaded module source escapes selected library: {rel}"
        );
        let actual = fs::read(&actual_path)
            .with_context(|| format!("reading selected module source {}", actual_path.display()))?;
        ensure!(
            sha256(&actual) == expected,
            "selected module source does not match table dump provenance: {rel}"
        );
        ensure!(
            verified.insert(rel.to_string(), expected).is_none(),
            "duplicate loaded module source: {rel}"
        );
    }
    let modules = doc["modules"]
        .as_object()
        .context("table dump lacks modules")?;
    for name in modules.keys() {
        ensure!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
            "unsafe table module name: {name}"
        );
        let rel = format!("Image/ExifTool/{name}.pm");
        ensure!(
            verified.contains_key(&rel),
            "table module lacks loaded source provenance: {rel}"
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let dump_path = PathBuf::from(
        args.next()
            .context("usage: gen_tag_registry <dump.json> <selected ExifTool lib>")?,
    );
    let source_lib = PathBuf::from(args.next().context("missing selected ExifTool lib")?);
    ensure!(args.next().is_none(), "unexpected extra argument");

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let pin = fs::read_to_string(root.join(".exiftool-version")).context("missing ExifTool pin")?;
    let pin = pin.trim();
    ensure!(!pin.is_empty(), "empty ExifTool pin");
    let dump_bytes =
        fs::read(&dump_path).with_context(|| format!("reading {}", dump_path.display()))?;
    let dump_sha = sha256(&dump_bytes);
    let doc: Value = serde_json::from_slice(&dump_bytes).context("invalid table dump JSON")?;
    drop(dump_bytes);
    ensure!(
        doc["exiftool_version"].as_str() == Some(pin),
        "table dump version does not match {pin}"
    );
    ensure!(
        doc["modules_ok"].as_u64().is_some_and(|count| count > 0)
            && doc["modules_failed"].as_u64() == Some(0),
        "incomplete table dump"
    );
    ensure!(
        doc["source_provenance"]["library_relative_path"].as_str() == Some("Image/ExifTool.pm"),
        "table dump lacks canonical source provenance"
    );
    let claimed_sha = doc["source_provenance"]["sha256"]
        .as_str()
        .context("table dump lacks source SHA-256")?
        .to_owned();
    let source_path = source_lib.join("Image/ExifTool.pm");
    let source_bytes =
        fs::read(&source_path).with_context(|| format!("reading {}", source_path.display()))?;
    ensure!(
        sha256(&source_bytes) == claimed_sha,
        "selected source does not match table dump provenance"
    );
    verify_dump_source_modules(&doc, &source_lib)?;

    let records = tag_records_from_dump_document(&doc)?;
    drop(doc);
    ensure!(!records.is_empty(), "empty tag registry");
    let mut staged = Vec::new();
    for domain in DOMAINS {
        let path = root.join(format!("oxidex-tags-{domain}/src/{domain}_tags.yaml"));
        let yaml = generate_domain_yaml(domain, &records);
        let count = count_ids_in_yaml(&yaml);
        if count == 0 {
            bail!("{domain} registry is empty");
        }
        // Every domain is computed and checked before the first replacement.
        staged.push((domain, path, yaml, count));
    }

    let stage_root = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    fs::create_dir_all(&stage_root)?;
    let stage_dir = stage_root.join(format!("registry-stage-{}", std::process::id()));
    fs::create_dir(&stage_dir).context("creating registry staging directory")?;
    let mut pending = Vec::new();
    for (domain, _, yaml, count) in &staged {
        let tmp = stage_dir.join(format!("{domain}.yaml"));
        if let Err(error) = fs::write(&tmp, yaml) {
            let _ = fs::remove_dir_all(&stage_dir);
            return Err(error).with_context(|| format!("staging {domain} registry"));
        }
        pending.push((*count, tmp));
    }
    for ((domain, path, _, count), (_, tmp)) in staged.iter().zip(&pending) {
        fs::rename(tmp, path).with_context(|| format!("replacing {domain} registry"))?;
        println!("{domain}: {count} rows; {}", sha256(&fs::read(path)?));
    }
    fs::remove_dir(&stage_dir)?;
    println!(
        "ExifTool {pin}; source {} sha256 {claimed_sha}; dump {} sha256 {}",
        source_path.display(),
        dump_path.display(),
        dump_sha
    );
    Ok(())
}
