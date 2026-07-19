# JPEG Tag Matrix Rust Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the three Python scripts that drive the JPEG tag comparison pipeline (`scripts/generate_exiftool_manifest.py`, `scripts/jpeg_tag_matrix.py`, `scripts/jpeg_tag_report.py`) with a single Rust binary, behavior-preserving, so `.github/workflows/jpeg-tag-matrix.yml` no longer needs Python/uv at all.

**Architecture:** One new feature-gated binary, `jpeg-tag-matrix`, with three subcommands (`manifest`, `run`, `report`) mirroring the three scripts 1:1. Each subcommand reads/writes the exact same JSON files at the exact same paths as its Python predecessor, so the CI workflow only needs its Python/uv setup steps deleted and its four `uv run scripts/...` lines swapped for `cargo run --bin jpeg-tag-matrix`. No behavior changes: same sample synthesis, same value-comparison leniency, same bug classification, same ratchet semantics.

**Tech Stack:** Rust (workspace binary), `quick-xml` (with `serialize` feature) for parsing `exiftool -listx` XML, `regex` (already a build-dependency, promoted to a runtime dependency here), `rayon` for the parallel read/write phases (replacing Python's `ThreadPoolExecutor`), `serde`/`serde_json` for all JSON I/O, `clap` for subcommand parsing (already a dependency, used the same way as `tag-comparison`).

## Global Constraints

- Every JSON/markdown file the pipeline reads or writes must stay at its current path, driven by the same environment variables as today: `EXIFTOOL` (default `exiftool`), `OXIDEX` (default `target/release/oxidex`), `TAGMATRIX_WORK` (default `<tmp>/oxidex-tagmap`), `TAGMATRIX_BASE` (default `tests/fixtures/jpeg/tag_matrix_base.jpg`).
- `docs/reference/jpeg-tag-baseline.json` must be written with **1-space indent** (`json.dumps(counts, indent=1) + "\n"` today) — not serde_json's default 2-space pretty printer — and with the same key order: `total_tested, readable, writable_cli, full, full_nonstandard, read_only, read_broken, write_broken, unsupported, untestable`.
- Ratchet semantics (`report.rs`, ported from `jpeg_tag_report.py:305-349`): `--check-baseline` fails only if `readable`, `writable_cli`, or `full` **decrease**, or `read_broken`/`write_broken` **increase**. `--update-baseline` unconditionally overwrites the baseline with current counts — it has no "only if strictly improved" gate of its own; it only runs after `--check-baseline` would have already non-zero-exited on a real regression. Do not add a strict-improvement check that isn't in the source.
- `--flag-noops` (`manifest.rs`, ported from `generate_exiftool_manifest.py:183-211`) performs **real write tests** against the base fixture for MakerNote*/Photoshop/JFIF-family tags — it is not static `-listx` parsing. Preserve the actual `exiftool -TAG=value` write + "1 image files updated" check.
- The pipeline's three artifact JSONs (`exiftool_jpeg_tags.json`, `exiftool_jpeg_readonly_tags.json`, `results.json`) are **not committed** — only uploaded as a 90-day workflow artifact — so their exact byte formatting doesn't matter, only that `manifest` → `run` → `report` can round-trip through them.
- `docs/reference/jpeg-tag-support.md` and `docs/reference/jpeg-tag-matrix.md` **are committed**; their Markdown structure (headings, table columns, the `KNOWN_BUGS` section content) must match today's output closely enough that the generated diff on first Rust-driven run is a content refresh, not a wholesale reformat.
- Out of scope, do not touch: `scripts/generate_tag_coverage.py`, `.github/workflows/update-coverage-docs.yml`, and the pre-existing race condition between that workflow and this one (deferred separately).
- Out of scope, do not touch: the pinned ExifTool sourcing policy in `jpeg-tag-matrix.yml` (git clone of tag `13.55`) — sourcing-policy unification across workflows was explicitly deferred.
- New binary must be feature-gated the same way `tag-comparison` is (`required-features = ["jpeg-tag-matrix-binary"]`) so it's excluded from default `cargo build`/`cargo test`, but must still compile cleanly under `cargo clippy --all-features -- -D warnings` (ci.yml's lint job already runs `--all-features`, so this feature is exercised there).
- Don't delete the Python scripts until Task 12 (parity verification) passes — they stay as the source of truth for behavior until then.

---

## File Structure

```
src/bin/jpeg-tag-matrix/
  main.rs        — clap CLI, subcommand dispatch (Manifest/Run/Report), env var resolution
  types.rs        — shared serde structs: ManifestTag, ManifestFile, ReadonlyTag, ReadonlyFile,
                     ResultEntry, BaselineCounts
  manifest.rs     — port of generate_exiftool_manifest.py (listx parsing, sample synthesis,
                     flag-noops)
  matrix.rs       — port of jpeg_tag_matrix.py (value comparison, bug classification, key
                     mapping, read/write phases)
  report.rs       — port of jpeg_tag_report.py (classify(), KNOWN_BUGS, markdown generation,
                     baseline ratchet)
```

Cargo.toml changes:
- Add `regex = "1.13"` under `[dependencies]` (currently only a `[build-dependencies]` entry — needs promoting since `matrix.rs`/`manifest.rs` use it at runtime, same version already resolved in `Cargo.lock` via the build-dep).
- Add `quick-xml = { version = "0.41", features = ["serialize"] }` — currently declared with default features only; the serde-based deserialization this plan uses needs `serialize`. Confirm no other crate/module rendered inoperable by only-default-features today (grep `quick_xml::` usages first — likely all manual `Reader` usage that also works fine under `serialize`).
- Add `jpeg-tag-matrix-binary = []` under `[features]`.
- Add a `[[bin]]` entry:
  ```toml
  [[bin]]
  name = "jpeg-tag-matrix"
  path = "src/bin/jpeg-tag-matrix/main.rs"
  required-features = ["jpeg-tag-matrix-binary"]
  ```

---

### Task 1: Cargo scaffolding and shared types

**Files:**
- Modify: `Cargo.toml`
- Create: `src/bin/jpeg-tag-matrix/main.rs`
- Create: `src/bin/jpeg-tag-matrix/types.rs`

**Interfaces:**
- Produces: `types::ManifestTag`, `types::ManifestFile`, `types::ReadonlyTag`, `types::ReadonlyFile`, `types::ResultEntry`, `types::BaselineCounts` — every later task serializes/deserializes through these.

- [ ] **Step 1: Add the feature, dependency, and bin entry to Cargo.toml**

```toml
[dependencies]
# ... existing deps ...
regex = "1.13"
quick-xml = { version = "0.41", features = ["serialize"] }
```

Update the existing `quick-xml = "0.41"` line to the `features = ["serialize"]` form above (don't duplicate the key).

```toml
[features]
exiftool-comparison = []
tag-comparison-binary = []
jpeg-tag-matrix-binary = []
```

```toml
[[bin]]
name = "jpeg-tag-matrix"
path = "src/bin/jpeg-tag-matrix/main.rs"
required-features = ["jpeg-tag-matrix-binary"]
```

- [ ] **Step 2: Write types.rs**

Field order matches each Python dict's construction order (JSON key order isn't semantically load-bearing since these files aren't committed, but matching it keeps diffs readable during Task 12's parity check).

```rust
//! Shared JSON schema for the JPEG tag matrix pipeline (manifest -> run -> report).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestTag {
    pub group: String,
    pub name: String,
    pub family0: String,
    pub writable: bool,
    #[serde(rename = "type")]
    pub vtype: String,
    pub protected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample: Option<String>,
    #[serde(rename = "sample_is_file", skip_serializing_if = "Option::is_none")]
    pub sample_is_file: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub noop: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GroupCounts {
    pub writable: u32,
    pub readonly: u32,
    pub protected_writable: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestFile {
    pub generated_by: String,
    pub description: String,
    pub groups: std::collections::BTreeMap<String, GroupCounts>,
    pub tag_count: usize,
    pub tags: Vec<ManifestTag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub noop_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadonlyTag {
    pub group: String,
    pub name: String,
    pub family0: String,
    #[serde(rename = "type")]
    pub vtype: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadonlyFile {
    pub generated_by: String,
    pub description: String,
    pub tag_count: usize,
    pub tags: Vec<ReadonlyTag>,
}

/// One tag's accumulated read+write result. Mirrors the Python `results[key]`
/// dict, which is built incrementally across the read and write phases, so
/// every field is optional except the manifest-derived ones attached last.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResultEntry {
    pub group: String,
    pub name: String,
    pub sample: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub vtype: Option<String>,
    #[serde(default)]
    pub protected: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_batch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_bug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ox_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ox_val: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub et_val: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub write: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_ox_val: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_et_val: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_ox_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bug_cluster: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_quality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_warnings: Option<String>,
}

/// docs/reference/jpeg-tag-baseline.json — key order matters for readable diffs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineCounts {
    pub total_tested: u32,
    pub readable: u32,
    pub writable_cli: u32,
    pub full: u32,
    pub full_nonstandard: u32,
    pub read_only: u32,
    pub read_broken: u32,
    pub write_broken: u32,
    pub unsupported: u32,
    pub untestable: u32,
}
```

- [ ] **Step 3: Write a minimal main.rs that compiles**

```rust
//! JPEG tag matrix pipeline: manifest generation, empirical read/write testing,
//! and report generation against a committed regression baseline.
//! Rust port of scripts/{generate_exiftool_manifest,jpeg_tag_matrix,jpeg_tag_report}.py.

mod manifest;
mod matrix;
mod report;
mod types;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "jpeg-tag-matrix")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Port of generate_exiftool_manifest.py
    Manifest(manifest::ManifestArgs),
    /// Port of jpeg_tag_matrix.py
    Run(matrix::RunArgs),
    /// Port of jpeg_tag_report.py
    Report(report::ReportArgs),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Manifest(args) => manifest::run(args),
        Command::Run(args) => matrix::run(args),
        Command::Report(args) => report::run(args),
    }
}
```

Create empty stub modules so this compiles:
```rust
// src/bin/jpeg-tag-matrix/manifest.rs
use clap::Args;
#[derive(Args)]
pub struct ManifestArgs {
    #[arg(long)]
    pub flag_noops: bool,
}
pub fn run(_args: ManifestArgs) -> anyhow::Result<()> { Ok(()) }
```
```rust
// src/bin/jpeg-tag-matrix/matrix.rs
use clap::Args;
#[derive(Args)]
pub struct RunArgs {
    #[arg(long)]
    pub only_group: Option<String>,
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long)]
    pub skip_write: bool,
    #[arg(long)]
    pub reread: bool,
    #[arg(long, default_value_t = 8)]
    pub workers: usize,
}
pub fn run(_args: RunArgs) -> anyhow::Result<()> { Ok(()) }
```
```rust
// src/bin/jpeg-tag-matrix/report.rs
use clap::Args;
#[derive(Args)]
pub struct ReportArgs {
    #[arg(long)]
    pub update_baseline: bool,
    #[arg(long)]
    pub check_baseline: bool,
}
pub fn run(_args: ReportArgs) -> anyhow::Result<()> { Ok(()) }
```

`anyhow` is currently only a `[build-dependencies]` entry (used in `build.rs`) — add it to `[dependencies]` too in this same Cargo.toml edit (same promote-from-build-dep pattern as `regex`).

- [ ] **Step 4: Verify it builds**

Run: `cargo build --features jpeg-tag-matrix-binary --bin jpeg-tag-matrix`
Expected: builds successfully, `target/debug/jpeg-tag-matrix` exists.

- [ ] **Step 5: Verify default build is unaffected**

Run: `cargo build`
Expected: succeeds, and `jpeg-tag-matrix` binary is NOT built (feature not enabled by default) — confirm via `ls target/debug/jpeg-tag-matrix` failing.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/bin/jpeg-tag-matrix/
git commit -m "scaffold jpeg-tag-matrix Rust binary behind feature flag"
```

---

### Task 2: manifest.rs — listx XML parsing and sample synthesis

**Files:**
- Modify: `src/bin/jpeg-tag-matrix/manifest.rs`

**Interfaces:**
- Consumes: `types::{ManifestTag, ManifestFile, ReadonlyTag, ReadonlyFile}` (Task 1)
- Produces: `manifest::make_sample(family0: &str, name: &str, vtype: &str, tag: &ListxTag, group1: &str) -> String`, plus the `Listx*` XML schema structs and `first_en_value()` — consumed by Task 3's `dump_listx()` and `main()` assembly. `dump_listx()` itself is NOT produced by this task; it's implemented in Task 3 Step 1, which consumes the schema structs defined here.

This is the trickiest single function to port faithfully: `make_sample()` (`generate_exiftool_manifest.py:129-166`) has ~15 ordered special cases before falling back to type-based synthesis. Port the `if`/`elif` chain in the exact same order — reordering changes which branch wins for tags that could match multiple rules.

- [ ] **Step 1: Define the listx XML shape via quick-xml serde**

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ListxRoot {
    #[serde(rename = "table", default)]
    pub tables: Vec<ListxTable>,
}

#[derive(Debug, Deserialize)]
pub struct ListxTable {
    #[serde(rename = "@name", default)]
    pub name: String,
    #[serde(rename = "@g0", default)]
    pub g0: String,
    #[serde(rename = "@g1", default)]
    pub g1: String,
    #[serde(rename = "tag", default)]
    pub tags: Vec<ListxTag>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ListxTag {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@g1", default)]
    pub g1: String,
    #[serde(rename = "@type", default)]
    pub vtype: String,
    #[serde(rename = "@writable", default)]
    pub writable: String,
    #[serde(rename = "@flags", default)]
    pub flags: String,
    #[serde(rename = "@count", default)]
    pub count: String,
    #[serde(rename = "values", default)]
    pub values: Option<ListxValues>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ListxValues {
    #[serde(rename = "key", default)]
    pub keys: Vec<ListxKey>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ListxKey {
    #[serde(rename = "val", default)]
    pub vals: Vec<ListxVal>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ListxVal {
    #[serde(rename = "@lang", default)]
    pub lang: String,
    #[serde(rename = "$text", default)]
    pub text: String,
}
```

- [ ] **Step 2: Port first_en_value (generate_exiftool_manifest.py:109-126)**

```rust
/// First English enum label, preferring a distinctive one over a bare
/// "None"/"Unknown" sentinel: those are frequently a tag's own unset
/// default, so writing that exact value as the sample makes a genuine
/// write indistinguishable from a no-op that left the default untouched.
fn first_en_value(tag: &ListxTag) -> Option<String> {
    let values = tag.values.as_ref()?;
    let labels: Vec<&str> = values
        .keys
        .iter()
        .flat_map(|k| k.vals.iter())
        .filter(|v| v.lang == "en")
        .map(|v| v.text.as_str())
        .collect();
    labels
        .iter()
        .find(|l| **l != "None" && **l != "Unknown")
        .or_else(|| labels.first())
        .map(|s| s.to_string())
}
```

- [ ] **Step 3: Write the failing tests for make_sample's special cases**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn tag(name: &str, vtype: &str, count: &str) -> ListxTag {
        ListxTag {
            name: name.to_string(),
            g1: String::new(),
            vtype: vtype.to_string(),
            writable: "true".to_string(),
            flags: String::new(),
            count: count.to_string(),
            values: None,
        }
    }

    #[test]
    fn override_wins_over_type() {
        let t = tag("IPTCDigest", "string", "1");
        assert_eq!(make_sample("Photoshop", "IPTCDigest", "string", &t, "Photoshop"), "new");
    }

    #[test]
    fn gps_sample_table_wins() {
        let t = tag("GPSLatitude", "rational64u", "1");
        assert_eq!(make_sample("EXIF", "GPSLatitude", "rational64u", &t, "GPS"), "37.7749");
    }

    #[test]
    fn exif_undef_version_tag() {
        let t = tag("ExifVersion", "undef", "4");
        assert_eq!(make_sample("EXIF", "ExifVersion", "undef", &t, "ExifIFD"), "0100");
    }

    #[test]
    fn offset_time_tag() {
        let t = tag("OffsetTimeOriginal", "string", "1");
        assert_eq!(make_sample("EXIF", "OffsetTimeOriginal", "string", &t, "ExifIFD"), "+05:30");
    }

    #[test]
    fn boolean_type() {
        let t = tag("SomeFlag", "boolean", "1");
        assert_eq!(make_sample("XMP", "SomeFlag", "boolean", &t, "XMP-x"), "True");
    }

    #[test]
    fn int_type_repeats_scalar_for_count() {
        let t = tag("SomeInts", "int16u", "3");
        assert_eq!(make_sample("EXIF", "SomeInts", "int16u", &t, "ExifIFD"), "3 3 3");
    }

    #[test]
    fn rational_type_single_count() {
        let t = tag("SomeRational", "rational64u", "1");
        assert_eq!(make_sample("EXIF", "SomeRational", "rational64u", &t, "ExifIFD"), "1.5");
    }

    #[test]
    fn fallback_generic_string() {
        let t = tag("SomeWeirdTag", "unknowntype", "1");
        assert_eq!(make_sample("EXIF", "SomeWeirdTag", "unknowntype", &t, "ExifIFD"), "OxTest");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin jpeg-tag-matrix --features jpeg-tag-matrix-binary make_sample`
Expected: FAIL — `make_sample` not defined yet (only the test module referencing it exists).

- [ ] **Step 3: Port make_sample (generate_exiftool_manifest.py:129-166)**

```rust
use std::collections::HashSet;
use once_cell::sync::Lazy;

const DT: &str = "2024:01:15 10:30:00";
const D: &str = "2024:01:15";
const T: &str = "10:30:00";

static INT_TYPES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    ["int8u", "int8s", "int16u", "int16s", "int32u", "int32s", "int64u", "int64s",
     "integer", "digits"].into_iter().collect()
});
static RAT_TYPES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    ["rational32u", "rational32s", "rational64u", "rational64s", "rational",
     "real", "float", "double", "fixed16u", "fixed16s", "fixed32u", "fixed32s"]
        .into_iter().collect()
});
static STRINGISH: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    ["string", "undef", "?", "var_ustr32", "var_string", "lang-alt", "binary"]
        .into_iter().collect()
});

fn override_sample(group1: &str, name: &str) -> Option<&'static str> {
    match (group1, name) {
        ("Photoshop", "IPTCDigest") => Some("new"),
        ("GPS", "GPSVersionID") => Some("2.3.0.0"),
        // PhotoshopThumbnail/PhotoshopBGRThumbnail resolved to BASE_FIXTURE
        // path by the caller (main()), not here -- see gps/file sample handling.
        _ => None,
    }
}

fn gps_sample(name: &str) -> Option<&'static str> {
    match name {
        "GPSLatitude" | "GPSDestLatitude" => Some("37.7749"),
        "GPSLatitudeRef" | "GPSDestLatitudeRef" => Some("N"),
        "GPSLongitude" | "GPSDestLongitude" => Some("122.4194"),
        "GPSLongitudeRef" | "GPSDestLongitudeRef" => Some("W"),
        "GPSAltitude" => Some("10.5"),
        "GPSDestDistance" => Some("1.5"),
        "GPSTimeStamp" => Some("10:30:00"),
        "GPSDateStamp" => Some("2024:01:15"),
        "GPSDateTime" => Some("2024:01:15 10:30:00"),
        _ => None,
    }
}

pub fn make_sample(family0: &str, name: &str, vtype: &str, tag: &ListxTag, group1: &str) -> String {
    if let Some(s) = override_sample(group1, name) {
        return s.to_string();
    }
    if let Some(s) = gps_sample(name) {
        return s.to_string();
    }
    if family0 == "EXIF" && vtype == "undef" && name.contains("Version") {
        return "0100".to_string();
    }
    if name.starts_with("OffsetTime") {
        return "+05:30".to_string();
    }
    if vtype == "boolean" {
        return "True".to_string();
    }
    if let Some(ev) = first_en_value(tag) {
        return ev;
    }
    if vtype == "date" {
        return DT.to_string();
    }
    if vtype == "struct" {
        return "{}".to_string();
    }
    if STRINGISH.contains(vtype) || vtype == "digits" {
        if name.starts_with("SubSec") {
            return "3".to_string();
        }
        if name.contains("Date") {
            if family0 == "IPTC" || vtype == "digits" {
                return D.to_string();
            }
            return DT.to_string();
        }
        if name.contains("Time") && family0 == "IPTC" {
            return T.to_string();
        }
    }
    if INT_TYPES.contains(vtype) || RAT_TYPES.contains(vtype) {
        let scalar = if INT_TYPES.contains(vtype) { "3" } else { "1.5" };
        let n: usize = tag.count.parse().unwrap_or(1);
        if n > 1 {
            return vec![scalar; n].join(" ");
        }
        return scalar.to_string();
    }
    "OxTest".to_string()
}
```

Note: the two file-path overrides (`PhotoshopThumbnail`, `PhotoshopBGRThumbnail` → `BASE_FIXTURE` path) and the `sample_is_file` flag they trigger are handled where `make_sample` is called from `main()` in Task 3, not inside this function — same separation of concerns as the Python (`OVERRIDES`/`FILE_SAMPLES` are both module-level, but `FILE_SAMPLES` is checked separately at `generate_exiftool_manifest.py:260-261` after `make_sample()` returns).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin jpeg-tag-matrix --features jpeg-tag-matrix-binary make_sample`
Expected: all 8 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/bin/jpeg-tag-matrix/manifest.rs
git commit -m "port make_sample and listx XML schema to Rust"
```

---

### Task 3: manifest.rs — dump_listx, main assembly, JSON output

**Files:**
- Modify: `src/bin/jpeg-tag-matrix/manifest.rs`

**Interfaces:**
- Consumes: `manifest::make_sample` (Task 2), `types::{ManifestTag, ManifestFile, ReadonlyTag, ReadonlyFile, GroupCounts}` (Task 1)
- Produces: `manifest::run(args: ManifestArgs) -> anyhow::Result<()>` — this is the subcommand entry point wired into `main.rs` (Task 1), no other task calls into it directly.

- [ ] **Step 1: Port dump_listx (generate_exiftool_manifest.py:169-180)**

```rust
use std::path::Path;
use std::process::Command;

fn dump_listx(exiftool: &str, group: &str, work: &Path) -> anyhow::Result<ListxRoot> {
    let out = Command::new(exiftool)
        .args(["-f", "-listx", &format!("-{group}:all")])
        .output()?;
    let xml = String::from_utf8_lossy(&out.stdout).to_string();
    std::fs::write(work.join(format!("listx_{group}.xml")), &xml)?;
    let root: ListxRoot = quick_xml::de::from_str(&xml)
        .map_err(|e| anyhow::anyhow!("empty or malformed -listx dump for {group}: {e}"))?;
    Ok(root)
}
```

- [ ] **Step 2: Port the SOURCES table and per-source table predicate (generate_exiftool_manifest.py:56-68)**

```rust
struct Source {
    group_arg: &'static str,
    family0: &'static str,
    table_pred: fn(&ListxTable) -> bool,
}

const SOURCES: &[Source] = &[
    Source { group_arg: "EXIF", family0: "EXIF",
             table_pred: |t| t.name == "Exif::Main" || t.name == "GPS::Main" },
    Source { group_arg: "XMP", family0: "XMP",
             table_pred: |t| t.g0 == "XMP" },
    Source { group_arg: "IPTC", family0: "IPTC",
             table_pred: |t| t.name.starts_with("IPTC::") },
    Source { group_arg: "JFIF", family0: "JFIF",
             table_pred: |t| t.name.starts_with("JFIF::") },
    Source { group_arg: "Photoshop", family0: "Photoshop",
             table_pred: |t| t.name.starts_with("Photoshop::") },
    Source { group_arg: "ICC_Profile", family0: "ICC_Profile",
             table_pred: |t| t.name.starts_with("ICC_Profile::") },
    Source { group_arg: "File", family0: "File",
             table_pred: |t| t.name == "Extra" },
];
```

- [ ] **Step 3: Port the entry-building and precedence-merge loop (generate_exiftool_manifest.py:213-271)**

```rust
use std::collections::HashMap;

const FILE_SAMPLES: &[(&str, &str)] = &[
    ("Photoshop", "PhotoshopThumbnail"),
    ("Photoshop", "PhotoshopBGRThumbnail"),
];

pub fn run(args: ManifestArgs) -> anyhow::Result<()> {
    let exiftool = std::env::var("EXIFTOOL").unwrap_or_else(|_| "exiftool".into());
    let work = std::env::var("TAGMATRIX_WORK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("oxidex-tagmap"));
    let repo = std::env::current_dir()?;
    let base_fixture = repo.join("tests/fixtures/jpeg/tag_matrix_base.jpg");
    std::fs::create_dir_all(&work)?;

    let ver_out = Command::new(&exiftool).arg("-ver").output()?;
    let ver = String::from_utf8_lossy(&ver_out.stdout).trim().to_string();
    println!("exiftool {ver}; work dir {}", work.display());

    let mut all_entries: HashMap<(String, String), ManifestTag> = HashMap::new();

    for source in SOURCES {
        let root = dump_listx(&exiftool, source.group_arg, &work)?;
        for table in root.tables.iter().filter(|t| (source.table_pred)(t)) {
            for tag_el in &table.tags {
                if source.family0 == "File" && tag_el.name != "Comment" {
                    continue;
                }
                let g1 = if source.family0 == "File" {
                    "File".to_string()
                } else if !tag_el.g1.is_empty() {
                    tag_el.g1.clone()
                } else {
                    table.g1.clone()
                };
                let writable = tag_el.writable == "true";
                let flagset: std::collections::HashSet<&str> =
                    tag_el.flags.split(',').filter(|s| !s.is_empty()).collect();
                let protected = flagset.contains("Protected")
                    || flagset.contains("Unsafe")
                    || flagset.contains("Avoid");

                let mut entry = ManifestTag {
                    group: g1.clone(),
                    name: tag_el.name.clone(),
                    family0: source.family0.to_string(),
                    writable,
                    vtype: tag_el.vtype.clone(),
                    protected,
                    flags: if tag_el.flags.is_empty() { None } else { Some(tag_el.flags.clone()) },
                    count: tag_el.count.parse().ok(),
                    sample: None,
                    sample_is_file: None,
                    noop: None,
                };

                if writable {
                    let mut sample = make_sample(source.family0, &tag_el.name, &tag_el.vtype, tag_el, &g1);
                    let is_file_sample = FILE_SAMPLES.contains(&(g1.as_str(), tag_el.name.as_str()));
                    if is_file_sample {
                        sample = base_fixture.display().to_string();
                        entry.sample_is_file = Some(true);
                    }
                    entry.sample = Some(sample);
                }

                let key = (g1.clone(), tag_el.name.clone());
                match all_entries.get(&key) {
                    None => { all_entries.insert(key, entry); }
                    Some(prev) => {
                        // prefer writable over not, then non-protected
                        let prev_rank = (prev.writable, !prev.protected);
                        let new_rank = (entry.writable, !entry.protected);
                        if new_rank > prev_rank {
                            all_entries.insert(key, entry);
                        }
                    }
                }
            }
        }
    }

    let mut entries: Vec<ManifestTag> = all_entries.into_values().collect();
    entries.sort_by(|a, b| (&a.family0, &a.group, &a.name).cmp(&(&b.family0, &b.group, &b.name)));

    let writable_tags: Vec<ManifestTag> = entries.iter().filter(|e| e.writable).cloned().collect();
    let readonly_tags: Vec<ReadonlyTag> = entries.iter().filter(|e| !e.writable)
        .map(|e| ReadonlyTag { group: e.group.clone(), name: e.name.clone(),
                               family0: e.family0.clone(), vtype: e.vtype.clone() })
        .collect();

    let mut groups: std::collections::BTreeMap<String, types::GroupCounts> = Default::default();
    for e in &entries {
        let g = groups.entry(e.family0.clone()).or_default();
        if e.writable {
            g.writable += 1;
            if e.protected { g.protected_writable += 1; }
        } else {
            g.readonly += 1;
        }
    }

    let mut manifest = ManifestFile {
        generated_by: format!("exiftool {ver}"),
        description: "ExifTool tags writable in JPEG files (testable universe for a \
                       read/write support matrix). group = ExifTool family-1 group.".into(),
        groups: groups.clone(),
        tag_count: writable_tags.len(),
        tags: writable_tags,
        noop_note: None,
    };

    if args.flag_noops {
        flag_noops(&mut manifest, &ver, &exiftool, &base_fixture, &work)?;
    }

    std::fs::write(work.join("exiftool_jpeg_tags.json"),
                   serde_json::to_string_pretty(&manifest)?)?;

    let readonly = ReadonlyFile {
        generated_by: format!("exiftool {ver}"),
        description: "JPEG-relevant ExifTool tags that are read-only (writable=false); \
                       not testable via synthesis.".into(),
        tag_count: readonly_tags.len(),
        tags: readonly_tags,
    };
    std::fs::write(work.join("exiftool_jpeg_readonly_tags.json"),
                   serde_json::to_string_pretty(&readonly)?)?;

    println!("{:<12} {:>8} {:>11} {:>8}", "family0", "writable", "(protected)", "readonly");
    let mut total = types::GroupCounts::default();
    for (g, c) in &groups {
        println!("{g:<12} {:>8} {:>11} {:>8}", c.writable, c.protected_writable, c.readonly);
        total.writable += c.writable;
        total.protected_writable += c.protected_writable;
        total.readonly += c.readonly;
    }
    println!("{:<12} {:>8} {:>11} {:>8}", "TOTAL", total.writable, total.protected_writable, total.readonly);

    Ok(())
}
```

Add `flag_noops: bool` field to `ManifestArgs` (already stubbed in Task 1) — confirm the clap `#[arg(long)]` flag name serializes to `--flag-noops` (clap's default kebab-case rename does this automatically for a `flag_noops` field; no extra attribute needed).

- [ ] **Step 2: Write an integration test using a recorded `-listx` fixture**

Real `exiftool` may not be installed in every dev/CI environment that runs `cargo test`, so this test uses a small canned XML fixture instead of shelling out. Add:

```
tests/fixtures/jpeg-tag-matrix/listx_sample.xml
```
with a minimal but real `<taginfo><table name="Exif::Main" g1="ExifIFD"><tag name="ISO" g1="ExifIFD" type="int16u" writable="true" count="1"/></table></taginfo>` fixture, and a test in `manifest.rs`:

```rust
#[test]
fn parses_minimal_listx_fixture() {
    let xml = std::fs::read_to_string("tests/fixtures/jpeg-tag-matrix/listx_sample.xml").unwrap();
    let root: ListxRoot = quick_xml::de::from_str(&xml).unwrap();
    assert_eq!(root.tables.len(), 1);
    assert_eq!(root.tables[0].tags[0].name, "ISO");
}
```

- [ ] **Step 3: Run the test**

Run: `cargo test --bin jpeg-tag-matrix --features jpeg-tag-matrix-binary parses_minimal_listx_fixture`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/bin/jpeg-tag-matrix/manifest.rs tests/fixtures/jpeg-tag-matrix/
git commit -m "port manifest assembly and JSON output to Rust"
```

---

### Task 4: manifest.rs — flag_noops

**Files:**
- Modify: `src/bin/jpeg-tag-matrix/manifest.rs`

**Interfaces:**
- Consumes: `types::ManifestFile` (Task 1)
- Produces: `manifest::flag_noops(manifest: &mut ManifestFile, exiftool_ver: &str, exiftool: &str, base_fixture: &Path, work: &Path) -> anyhow::Result<()>` — called from Task 3's `run()`.

This is one of the two fidelity-risk items flagged in the earlier investigation: it does a **real write test**, not static parsing.

- [ ] **Step 1: Port flag_noops (generate_exiftool_manifest.py:183-211)**

```rust
fn flag_noops(manifest: &mut ManifestFile, exiftool_ver: &str, exiftool: &str,
              base_fixture: &Path, work: &Path) -> anyhow::Result<()> {
    let suspects: Vec<usize> = manifest.tags.iter().enumerate()
        .filter(|(_, t)| (t.name.starts_with("MakerNote") && t.family0 == "EXIF")
                       || t.family0 == "Photoshop" || t.family0 == "JFIF")
        .map(|(i, _)| i)
        .collect();

    let mut noop_count = 0;
    for &i in &suspects {
        let (group, name, sample, sample_is_file) = {
            let t = &manifest.tags[i];
            (t.group.clone(), t.name.clone(), t.sample.clone().unwrap_or_default(),
             t.sample_is_file.unwrap_or(false))
        };
        let dst = work.join("noop_tmp.jpg");
        std::fs::copy(base_fixture, &dst)?;
        let op = if sample_is_file { "<=" } else { "=" };
        let spec = format!("-{group}:{name}{op}{sample}");
        let out = Command::new(exiftool)
            .args(["-overwrite_original", &spec])
            .arg(&dst)
            .output()?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        if out.status.success() && stdout.contains("1 image files updated") {
            manifest.tags[i].noop = None;
        } else {
            manifest.tags[i].noop = Some(true);
            noop_count += 1;
        }
    }
    manifest.noop_note = Some(format!(
        "Tags with noop:true are listed writable by exiftool -listx but were \
         behaviorally verified to be silent no-ops when written to a bare JPEG \
         (exiftool {exiftool_ver})."
    ));
    println!("flag-noops: {} suspects tested, {noop_count} no-ops", suspects.len());
    Ok(())
}
```

- [ ] **Step 2: Write a test that stubs `exiftool` via PATH**

Because this genuinely shells out, test it with a fake `exiftool` script rather than mocking `Command` (no mocking framework is in the dependency tree, and adding one for a single test isn't warranted — YAGNI).

```
tests/fixtures/jpeg-tag-matrix/fake-exiftool-noop.sh
```
```bash
#!/usr/bin/env bash
# Fake exiftool for flag_noops tests: always reports success ("1 image files
# updated"), so any tag list run through it should come out with noop:None.
if [[ "$*" == *"-ver"* ]]; then echo "13.55"; exit 0; fi
echo "    1 image files updated"
exit 0
```

```rust
#[test]
fn flag_noops_marks_failing_writes_as_noop() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = dir.path().join("base.jpg");
    std::fs::write(&fixture, b"fake jpeg bytes").unwrap();
    let fake_exiftool = "tests/fixtures/jpeg-tag-matrix/fake-exiftool-fail.sh";

    let mut manifest = ManifestFile {
        generated_by: "test".into(), description: "test".into(),
        groups: Default::default(), tag_count: 1,
        tags: vec![ManifestTag {
            group: "MakerNotes".into(), name: "MakerNoteFoo".into(),
            family0: "EXIF".into(), writable: true, vtype: "string".into(),
            protected: false, flags: None, count: None,
            sample: Some("x".into()), sample_is_file: None, noop: None,
        }],
        noop_note: None,
    };
    flag_noops(&mut manifest, "13.55", fake_exiftool, &fixture, dir.path()).unwrap();
    assert_eq!(manifest.tags[0].noop, Some(true));
}
```

Add a second fake script, `fake-exiftool-fail.sh`, that exits 0 but doesn't print "1 image files updated" (simulating a silent no-op write):
```bash
#!/usr/bin/env bash
if [[ "$*" == *"-ver"* ]]; then echo "13.55"; exit 0; fi
echo "    0 image files updated"
exit 0
```
Make both scripts executable: `chmod +x tests/fixtures/jpeg-tag-matrix/fake-exiftool-*.sh`.

- [ ] **Step 3: Run the test**

Run: `cargo test --bin jpeg-tag-matrix --features jpeg-tag-matrix-binary flag_noops_marks_failing_writes_as_noop`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/bin/jpeg-tag-matrix/manifest.rs tests/fixtures/jpeg-tag-matrix/
git commit -m "port flag_noops empirical write-testing to Rust"
```

---

### Task 5: matrix.rs — value comparison and bug classification tables

**Files:**
- Modify: `src/bin/jpeg-tag-matrix/matrix.rs`

**Interfaces:**
- Produces: `matrix::values_match(expected: &str, actual: &str) -> bool`, `matrix::classify_read_mismatch(r: &ResultEntry) -> Option<&'static str>`, `matrix::write_bug_cluster_for(name: &str) -> Option<&'static str>` — consumed by Task 7 (read phase) and Task 8 (write phase, `apply_bug_classification`).

This is the other fidelity-risk item: `values_match`'s lenient comparison (exact / numeric incl. rationals / date / unit-suffix / single-letter-enum) is what makes the whole matrix's OK/MISMATCH split meaningful. Port every branch — dropping one silently changes pass/fail counts for potentially dozens of tags.

- [ ] **Step 1: Write the failing tests (from jpeg_tag_matrix.py:91-148's own worked examples)**

```rust
#[cfg(test)]
mod value_match_tests {
    use super::*;

    #[test]
    fn exact_match() {
        assert!(values_match("37.7749", "37.7749"));
    }

    #[test]
    fn rational_vs_decimal() {
        assert!(values_match("3/2", "1.5"));
    }

    #[test]
    fn numeric_tolerance() {
        assert!(values_match("1.500001", "1.5"));
    }

    #[test]
    fn unit_suffix() {
        assert!(values_match("10.5", "10.5 m"));
    }

    #[test]
    fn single_letter_enum_abbreviation() {
        assert!(values_match("N", "North"));
        assert!(values_match("North", "N"));
    }

    #[test]
    fn date_separator_normalization() {
        assert!(values_match("2024:01:15 10:30:00", "2024-01-15T10:30:00"));
    }

    #[test]
    fn date_drops_subseconds_and_timezone() {
        assert!(values_match("2024:01:15 10:30:00", "2024:01:15 10:30:00.500+05:00"));
    }

    #[test]
    fn whitespace_and_case_normalized() {
        assert!(values_match("Foo  Bar", "foo bar"));
    }

    #[test]
    fn genuinely_different_values_do_not_match() {
        assert!(!values_match("North", "South"));
        assert!(!values_match("3", "4"));
    }

    #[test]
    fn none_never_matches() {
        assert!(!values_match_opt(None, Some("x")));
        assert!(!values_match_opt(Some("x"), None));
    }
}
```

`values_match` in Python takes `expected: Optional[str]` / returns `False` if either is `None` (`jpeg_tag_matrix.py:113`). Since call sites in Rust will pass `Option<&str>` at the boundary (read/write results are frequently absent), model this as two functions: `values_match(expected: &str, actual: &str) -> bool` (the core string-level comparison, always called with two real strings) plus a thin `values_match_opt(expected: Option<&str>, actual: Option<&str>) -> bool` wrapper used at call sites where either side may be missing — this keeps the core function's signature simple for the 90% of call sites that already have two strings in hand.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin jpeg-tag-matrix --features jpeg-tag-matrix-binary value_match_tests`
Expected: FAIL — `values_match` not defined.

- [ ] **Step 3: Port values_match (jpeg_tag_matrix.py:91-148)**

```rust
use regex::Regex;
use once_cell::sync::Lazy;

static RATIONAL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(-?\d+)/(-?\d+)$").unwrap());
static UNIT_SUFFIX_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(-?[\d.]+(?:/\d+)?)\s*\D*$").unwrap());
static DATE_LIKE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\d{4}[:-]\d{2}[:-]\d{2}").unwrap());

fn as_float(s: &str) -> Option<f64> {
    let s = s.trim();
    if let Some(caps) = RATIONAL_RE.captures(s) {
        let num: i64 = caps[1].parse().ok()?;
        let den: i64 = caps[2].parse().ok()?;
        if den != 0 {
            return Some(num as f64 / den as f64);
        }
    }
    s.parse::<f64>().ok()
}

fn norm_str(s: &str) -> String {
    let collapsed: String = s.trim().split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.to_lowercase()
}

fn dnorm(s: &str) -> String {
    let replaced: String = s.chars().map(|c| match c {
        '-' | ':' | 't' | 'T' | ' ' => ':',
        other => other,
    }).collect();
    let no_tz = replaced.split('+').next().unwrap_or(&replaced);
    no_tz.split('.').next().unwrap_or(no_tz).trim().to_string()
}

pub fn values_match(expected: &str, actual: &str) -> bool {
    let (es, as_) = (expected.trim(), actual.trim());
    if es == as_ {
        return true;
    }
    if norm_str(es) == norm_str(as_) {
        return true;
    }
    let (ef, af) = (as_float(es), as_float(as_));
    if let (Some(ef), Some(af)) = (ef, af) {
        if ef == af {
            return true;
        }
        let denom = ef.abs().max(af.abs()).max(1e-9);
        if (ef - af).abs() / denom < 1e-3 {
            return true;
        }
    }
    // numeric with unit suffix, e.g. "10.5 m" vs "10.5"
    if let Some(ef) = ef {
        if let Some(caps) = UNIT_SUFFIX_RE.captures(as_) {
            if let Some(af2) = as_float(&caps[1]) {
                if (ef - af2).abs() / ef.abs().max(1e-9) < 1e-3 {
                    return true;
                }
            }
        }
    }
    if let Some(af) = af {
        if let Some(caps) = UNIT_SUFFIX_RE.captures(es) {
            if let Some(ef2) = as_float(&caps[1]) {
                if (af - ef2).abs() / af.abs().max(1e-9) < 1e-3 {
                    return true;
                }
            }
        }
    }
    // single-letter enum abbreviation vs PrintConv expansion ("N" <-> "North")
    if es.chars().count() == 1 && !as_.is_empty()
        && as_.chars().next().unwrap().to_ascii_uppercase() == es.chars().next().unwrap().to_ascii_uppercase() {
        return true;
    }
    if as_.chars().count() == 1 && !es.is_empty()
        && es.chars().next().unwrap().to_ascii_uppercase() == as_.chars().next().unwrap().to_ascii_uppercase() {
        return true;
    }
    // dates: normalize separators (incl. T vs space), drop subseconds/timezone
    if DATE_LIKE_RE.is_match(es) && dnorm(es) == dnorm(as_) {
        return true;
    }
    false
}

pub fn values_match_opt(expected: Option<&str>, actual: Option<&str>) -> bool {
    match (expected, actual) {
        (Some(e), Some(a)) => values_match(e, a),
        _ => false,
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin jpeg-tag-matrix --features jpeg-tag-matrix-binary value_match_tests`
Expected: all 10 tests PASS.

- [ ] **Step 5: Write failing tests for classify_read_mismatch and the write bug clusters (jpeg_tag_matrix.py:163-237)**

```rust
#[cfg(test)]
mod bug_classification_tests {
    use super::*;

    fn result(name: &str, group: &str, ox_val: &str, sample: &str, vtype: &str) -> ResultEntry {
        ResultEntry {
            name: name.into(), group: group.into(), ox_val: Some(ox_val.into()),
            sample: sample.into(), vtype: Some(vtype.into()), read: Some("MISMATCH".into()),
            ..Default::default()
        }
    }

    #[test]
    fn apex_tag_names_flagged() {
        let r = result("ApertureValue", "ExifIFD", "4.0", "4.0", "rational64u");
        assert_eq!(classify_read_mismatch(&r), Some("R-apex-missing"));
    }

    #[test]
    fn nul_byte_in_iptc_flags_binary_garbage() {
        let r = result("SomeIptcTag", "IPTC", "foo\u{0}bar", "x", "string");
        assert_eq!(classify_read_mismatch(&r), Some("R-iptc-binary-garbage"));
    }

    #[test]
    fn xp_utf16_not_decoded() {
        let r = result("XPComment", "ExifIFD", "1234567", "x", "string");
        assert_eq!(classify_read_mismatch(&r), Some("R-utf16-not-decoded"));
    }

    #[test]
    fn unrecognized_mismatch_returns_none() {
        let r = result("SomeNewTag", "EXIF", "totally different", "x", "string");
        assert_eq!(classify_read_mismatch(&r), None);
    }

    #[test]
    fn write_bug_cluster_lookup() {
        assert_eq!(write_bug_cluster_for("GPSSpeedRef"), Some("I1-no-printconvinv"));
        assert_eq!(write_bug_cluster_for("DNGVersion"), Some("I3-wrong-type-numeric"));
        assert_eq!(write_bug_cluster_for("SomeUnclusteredTag"), None);
    }
}
```

- [ ] **Step 6: Port the bug-classification constants and classify_read_mismatch (jpeg_tag_matrix.py:163-237)**

```rust
static APEX_TAG_NAMES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    ["ApertureValue", "MaxApertureValue", "ShutterSpeedValue", "FlashEnergy"]
        .into_iter().collect()
});
static IPTC_BINARY_TAG_NAMES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    ["ARMIdentifier", "ARMVersion", "FileFormat", "FileVersion", "ObjectPreviewFileFormat"]
        .into_iter().collect()
});
static NAMESPACE_BLIND_ENUM_NAMES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    ["Contrast", "Saturation", "Sharpness", "SensingMethod", "CustomRendered"]
        .into_iter().collect()
});
static XP_INT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\d{6,}$").unwrap());
static FLOAT_RAW_BITS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^-?\d{7,}$").unwrap());

pub fn classify_read_mismatch(r: &ResultEntry) -> Option<&'static str> {
    let name = r.name.as_str();
    let group = r.group.as_str();
    let oxs = r.ox_val.clone().unwrap_or_default();
    let sample = r.sample.as_str();
    let vtype = r.vtype.clone().unwrap_or_default();

    if oxs.contains('\u{0}') {
        return Some(if group == "IPTC" { "R-iptc-binary-garbage" } else { "R-binary-garbage" });
    }
    if IPTC_BINARY_TAG_NAMES.contains(name) && group == "IPTC" {
        return Some("R-iptc-binary-garbage");
    }
    if APEX_TAG_NAMES.contains(name) {
        return Some("R-apex-missing");
    }
    if group.starts_with("XMP") && (oxs.starts_with("Unknown (") || NAMESPACE_BLIND_ENUM_NAMES.contains(name)) {
        return Some("R-namespace-blind-printconv");
    }
    if oxs.starts_with(&format!("{name}: ")) {
        return Some("R-acr-prefix");
    }
    if oxs.starts_with("(Binary,") {
        return Some("R-undef-not-decoded");
    }
    if name.starts_with("XP") && XP_INT_RE.is_match(&oxs) {
        return Some("R-utf16-not-decoded");
    }
    if (vtype.starts_with("float") || vtype.starts_with("double")) && FLOAT_RAW_BITS_RE.is_match(&oxs) {
        return Some("R-float-raw-bits");
    }
    if !sample.is_empty() && oxs.matches(sample).count() >= 2 {
        return Some("R-xmp-struct-concat");
    }
    None
}

static WRITE_BUG_CLUSTER_TAG_NAMES: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let clusters: &[(&str, &[&str])] = &[
        ("I1-no-printconvinv", &["GPSSpeedRef", "GPSStatus", "GPSMeasureMode", "GPSDestBearingRef",
                                  "GPSDestDistanceRef", "GPSImgDirectionRef", "GPSTrackRef",
                                  "SecurityClassification"]),
        ("I2-wrong-type-enum", &["CalibrationIlluminant1", "CalibrationIlluminant2",
                                  "CalibrationIlluminant3", "ColorimetricReference",
                                  "DefaultBlackRender", "DepthFormat", "DepthMeasureType",
                                  "DepthUnits", "MakerNoteSafety", "OldSubfileType",
                                  "PreviewColorSpace", "ProfileEmbedPolicy",
                                  "ProfileHueSatMapEncoding", "ProfileLookTableEncoding",
                                  "Thresholding"]),
        ("I3-wrong-type-numeric", &["DNGVersion", "DNGBackwardVersion", "RawImageDigest",
                                     "NewRawImageDigest", "OriginalRawFileDigest",
                                     "RawDataUniqueID", "TimeCodes", "ExposureCompensation",
                                     "DNGLensInfo", "GeoTiffDoubleParams"]),
        ("I4-wrong-type-undef", &["Padding", "GooglePlusUploadCode",
                                   "CompositeImageExposureTimes", "RGBTables", "ImageStats",
                                   "ProfileGainTableMap2", "GeoTiffAsciiParams"]),
        ("I5-subdir-poison", &["CurrentICCProfile", "AsShotICCProfile", "XiaomiSettings",
                                "ImageSequenceInfo", "OriginalRawFileData",
                                "ProfileDynamicRange", "SEAL"]),
    ];
    let mut map = HashMap::new();
    for (cluster, names) in clusters {
        for name in *names {
            map.insert(*name, *cluster);
        }
    }
    map
});

pub fn write_bug_cluster_for(name: &str) -> Option<&'static str> {
    WRITE_BUG_CLUSTER_TAG_NAMES.get(name).copied()
}
```

`BATCH_POISON` (`jpeg_tag_matrix.py:47`, the single `IFD0:GeoTiffDoubleParams` exclusion) belongs with the read-phase batching logic — port it in Task 7, not here.

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --bin jpeg-tag-matrix --features jpeg-tag-matrix-binary bug_classification_tests`
Expected: all 5 tests PASS.

- [ ] **Step 8: Commit**

```bash
git add src/bin/jpeg-tag-matrix/matrix.rs
git commit -m "port values_match and bug classification tables to Rust"
```

---

### Task 6: matrix.rs — key mapping between ExifTool and oxidex tag names

**Files:**
- Modify: `src/bin/jpeg-tag-matrix/matrix.rs`

**Interfaces:**
- Consumes: `types::ManifestTag` (Task 1), `matrix::values_match` (Task 5)
- Produces: `matrix::oxidex_read_keys`, `matrix::oxidex_write_keys`, `matrix::find_in_json`, `matrix::find_in_exiftool_json`, `matrix::find_same_group_fallback` — all consumed by Task 7 (read phase) and Task 8 (write phase).

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod key_mapping_tests {
    use super::*;
    use serde_json::json;

    fn tag(group: &str, name: &str) -> ManifestTag {
        ManifestTag {
            group: group.into(), name: name.into(), family0: "EXIF".into(),
            writable: true, vtype: "string".into(), protected: false,
            flags: None, count: None, sample: Some("x".into()),
            sample_is_file: None, noop: None,
        }
    }

    #[test]
    fn interop_ifd_gets_exif_prefixed_first() {
        let keys = oxidex_read_keys(&tag("InteropIFD", "InteropIndex"));
        assert_eq!(keys, vec!["EXIF:InteropIndex", "InteropIFD:InteropIndex", "InteropIndex"]);
    }

    #[test]
    fn xmp_group_gets_flattened_and_full_variants() {
        let keys = oxidex_read_keys(&tag("XMP-dc", "Creator"));
        assert_eq!(keys, vec!["XMP:Creator", "XMP-dc:Creator", "Creator"]);
    }

    #[test]
    fn photoshop_falls_back_to_iptc() {
        let keys = oxidex_read_keys(&tag("Photoshop", "IPTCDigest"));
        assert_eq!(keys, vec!["Photoshop:IPTCDigest", "IPTC:IPTCDigest", "IPTCDigest"]);
    }

    #[test]
    fn exif_group_write_key_uses_exact_family1_prefix() {
        let keys = oxidex_write_keys(&tag("ExifIFD", "ISO"));
        assert_eq!(keys, vec!["ExifIFD:ISO"]);
    }

    #[test]
    fn find_in_json_returns_first_present_key() {
        let data = json!({"InteropIFD:InteropIndex": "R98"});
        let (k, v) = find_in_json(&data, &["EXIF:InteropIndex".into(), "InteropIFD:InteropIndex".into()]);
        assert_eq!(k.as_deref(), Some("InteropIFD:InteropIndex"));
        assert_eq!(v, Some(&json!("R98")));
    }

    #[test]
    fn find_in_exiftool_json_strict_group_has_no_bare_name_fallback() {
        let data = json!({"ExifIFD:ColorSpace": "1"});
        let t = tag("XMP-exif", "ColorSpace");
        assert_eq!(find_in_exiftool_json(&data, &t, true), None);
        assert_eq!(find_in_exiftool_json(&data, &t, false), Some(&json!("1")));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin jpeg-tag-matrix --features jpeg-tag-matrix-binary key_mapping_tests`
Expected: FAIL — functions not defined.

- [ ] **Step 3: Port the key-mapping functions (jpeg_tag_matrix.py:270-351)**

```rust
use serde_json::Value;

const EXIF_GROUPS: &[&str] = &["IFD0", "IFD1", "ExifIFD", "GPS", "InteropIFD", "SubIFD"];

pub fn oxidex_read_keys(tag: &ManifestTag) -> Vec<String> {
    let (g, n) = (tag.group.as_str(), tag.name.as_str());
    let mut keys = Vec::new();
    if g == "InteropIFD" {
        keys.push(format!("EXIF:{n}"));
        keys.push(format!("InteropIFD:{n}"));
    } else if EXIF_GROUPS.contains(&g) {
        keys.push(format!("{g}:{n}"));
    } else if g.starts_with("XMP") {
        keys.push(format!("XMP:{n}"));
        keys.push(format!("{g}:{n}"));
    } else if g == "IPTC" {
        keys.push(format!("IPTC:{n}"));
    } else if g == "Photoshop" {
        keys.push(format!("Photoshop:{n}"));
        keys.push(format!("IPTC:{n}"));
    } else if g == "JFIF" {
        keys.push(format!("JFIF:{n}"));
    } else {
        keys.push(format!("{g}:{n}"));
    }
    keys.push(n.to_string());
    keys
}

/// Write routing (validator.rs separate_by_ifd) only honors IFD0:/IFD1:/
/// ExifIFD:/GPS:/EXIF: prefixes; EXIF: lands in IFD0 (wrong IFD for ExifIFD
/// tags) so we use the exact family-1 prefix only. Other families are
/// dropped silently -- one spelling suffices to prove NOT_WRITTEN.
pub fn oxidex_write_keys(tag: &ManifestTag) -> Vec<String> {
    let (g, n) = (tag.group.as_str(), tag.name.as_str());
    if EXIF_GROUPS.contains(&g) {
        vec![format!("{g}:{n}")]
    } else if g.starts_with("XMP") {
        vec![format!("XMP:{n}")]
    } else {
        vec![format!("{g}:{n}")]
    }
}

pub fn find_in_json<'a>(data: &'a Value, keys: &[String]) -> (Option<String>, Option<&'a Value>) {
    for k in keys {
        if let Some(v) = data.get(k) {
            return (Some(k.clone()), Some(v));
        }
    }
    (None, None)
}

/// strict_group: require the exact family-1 group, with no bare-name
/// fallback to a different group at all. Used for write-test read-back:
/// without this, a tag we never actually wrote can spuriously "match" an
/// unrelated pre-existing tag of the same bare name in a different group.
pub fn find_in_exiftool_json<'a>(data: &'a Value, tag: &ManifestTag, strict_group: bool) -> Option<&'a Value> {
    let k = format!("{}:{}", tag.group, tag.name);
    if let Some(v) = data.get(&k) {
        return Some(v);
    }
    if strict_group {
        return None;
    }
    data.as_object()?.iter()
        .find(|(key, _)| key.splitn(2, ':').last() == Some(tag.name.as_str()))
        .map(|(_, v)| v)
}

/// Scan for `sample` under any key sharing this tag's group prefix. Catches
/// write/read registry asymmetries without hardcoding specific tag names.
pub fn find_same_group_fallback<'a>(data: &'a Value, tag: &ManifestTag, sample: &str) -> (Option<String>, Option<&'a Value>) {
    let prefix = format!("{}:", tag.group);
    if let Some(obj) = data.as_object() {
        for (key, v) in obj {
            if key.starts_with(&prefix) && v.as_str().map(|s| values_match(sample, s)).unwrap_or(false) {
                return (Some(key.clone()), Some(v));
            }
        }
    }
    (None, None)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin jpeg-tag-matrix --features jpeg-tag-matrix-binary key_mapping_tests`
Expected: all 6 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/bin/jpeg-tag-matrix/matrix.rs
git commit -m "port exiftool<->oxidex key mapping to Rust"
```

---

### Task 7: matrix.rs — read phase (batch + individual retest)

**Files:**
- Modify: `src/bin/jpeg-tag-matrix/matrix.rs`

**Interfaces:**
- Consumes: `matrix::{oxidex_read_keys, find_in_json, find_in_exiftool_json, find_same_group_fallback, values_match_opt}` (Task 6), `types::ManifestTag`
- Produces: `matrix::read_test_group(tags: &[ManifestTag], ...) -> HashMap<String, ResultEntry>`, `matrix::read_test_single(tag: &ManifestTag, ...) -> ResultEntry` — consumed by Task 8's `run()` orchestration.

- [ ] **Step 1: Port the subprocess wrappers (jpeg_tag_matrix.py:70-88)**

```rust
use std::time::Duration;
use std::path::Path;

pub struct Tools<'a> {
    pub exiftool: &'a str,
    pub oxidex: &'a str,
}

fn run_cmd(prog: &str, args: &[String], timeout_secs: u64) -> (i32, String, String) {
    // wait_timeout avoids adding a new dependency: spawn + poll in a loop.
    use std::process::{Command, Stdio};
    let mut child = match Command::new(prog).args(args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(e) => return (-2, String::new(), e.to_string()),
    };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                use std::io::Read;
                let mut out = String::new();
                let mut err = String::new();
                child.stdout.take().unwrap().read_to_string(&mut out).ok();
                child.stderr.take().unwrap().read_to_string(&mut err).ok();
                return (status.code().unwrap_or(-1), out, err);
            }
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(timeout_secs) {
                    let _ = child.kill();
                    return (-1, String::new(), "TIMEOUT".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return (-2, String::new(), e.to_string()),
        }
    }
}

pub fn exiftool_json(tools: &Tools, path: &Path) -> Value {
    let (code, out, _) = run_cmd(tools.exiftool,
        &["-j".into(), "-G1".into(), "-charset".into(), "utf8".into(), path.display().to_string()], 30);
    if code != 0 || out.trim().is_empty() {
        return Value::Object(Default::default());
    }
    serde_json::from_str::<Vec<Value>>(&out).ok()
        .and_then(|mut v| v.pop())
        .unwrap_or_else(|| Value::Object(Default::default()))
}

pub fn oxidex_json(tools: &Tools, path: &Path) -> (Option<Value>, Option<String>) {
    // -e (exiftool-compat) gives PrintConv-style values closest to exiftool -j -G1
    let (code, out, err) = run_cmd(tools.oxidex, &["-j".into(), "-e".into(), path.display().to_string()], 30);
    if code != 0 || out.trim().is_empty() {
        return (None, Some(err));
    }
    match serde_json::from_str::<Vec<Value>>(&out) {
        Ok(mut v) => (v.pop(), None),
        Err(_) => (None, Some("unparseable JSON".into())),
    }
}
```

- [ ] **Step 2: Port _resolve_read and read_test_single (jpeg_tag_matrix.py:357-390)**

```rust
fn value_to_str(v: &Value) -> String {
    match v { Value::String(s) => s.clone(), other => other.to_string() }
}

fn resolve_read(ox: &Value, tag: &ManifestTag, et_val: &Value) -> ResultEntry {
    let keys = oxidex_read_keys(tag);
    let (k, v) = find_in_json(ox, &keys);
    let et_str = value_to_str(et_val);
    match (k, v) {
        (None, _) => {
            let sample = tag.sample.clone().unwrap_or_default();
            let (fk, fv) = find_same_group_fallback(ox, tag, &sample);
            if let (Some(fk), Some(fv)) = (fk, fv) {
                return ResultEntry {
                    read: Some("OK".into()), ox_key: Some(fk), ox_val: Some(value_to_str(fv)),
                    et_val: Some(et_str), bug_cluster: Some("R4-registry-asymmetry".into()),
                    ..Default::default()
                };
            }
            ResultEntry { read: Some("MISSING".into()), et_val: Some(et_str), ..Default::default() }
        }
        (Some(k), Some(v)) => {
            let vs = value_to_str(v);
            let sample = tag.sample.clone().unwrap_or_default();
            if values_match(&et_str, &vs) || values_match(&sample, &vs) {
                ResultEntry { read: Some("OK".into()), ox_key: Some(k), ox_val: Some(vs),
                              et_val: Some(et_str), ..Default::default() }
            } else {
                ResultEntry { read: Some("MISMATCH".into()), ox_key: Some(k), ox_val: Some(vs),
                              et_val: Some(et_str), ..Default::default() }
            }
        }
        _ => unreachable!("find_in_json returns matching Some/Some or None/None"),
    }
}

pub fn read_test_single(tools: &Tools, base: &Path, tag: &ManifestTag) -> ResultEntry {
    let td = tempfile::tempdir().unwrap();
    let img = td.path().join("t.jpg");
    std::fs::copy(base, &img).unwrap();
    let spec = format!("-{}:{}={}", tag.group, tag.name, tag.sample.as_deref().unwrap_or(""));
    run_cmd(tools.exiftool, &["-m".into(), "-q".into(), "-overwrite_original".into(),
                              spec, img.display().to_string()], 60);
    let et = exiftool_json(tools, &img);
    let et_val = find_in_exiftool_json(&et, tag, false);
    let Some(et_val) = et_val else {
        return ResultEntry { read: Some("NO_SAMPLE".into()), ..Default::default() };
    };
    let (ox, oxerr) = oxidex_json(tools, &img);
    let Some(ox) = ox else {
        return ResultEntry { read: Some("OXIDEX_PARSE_FAIL".into()),
                              et_val: Some(value_to_str(et_val)),
                              read_detail: oxerr.map(|e| e.chars().take(200).collect()),
                              ..Default::default() };
    };
    resolve_read(&ox, tag, et_val)
}
```

- [ ] **Step 3: Port read_test_group with the BATCH_POISON exclusion (jpeg_tag_matrix.py:47, 393-420)**

```rust
const BATCH_POISON: &[(&str, &str)] = &[("IFD0", "GeoTiffDoubleParams")];

fn key_of(t: &ManifestTag) -> String {
    format!("{}:{}", t.group, t.name)
}

pub fn read_test_group(tools: &Tools, base: &Path, tags: &[ManifestTag]) -> HashMap<String, ResultEntry> {
    let mut results = HashMap::new();
    let td = tempfile::tempdir().unwrap();
    let img = td.path().join("t.jpg");
    std::fs::copy(base, &img).unwrap();

    let chunk = 80;
    let batch_tags: Vec<&ManifestTag> = tags.iter()
        .filter(|t| !BATCH_POISON.contains(&(t.group.as_str(), t.name.as_str())))
        .collect();
    for group in batch_tags.chunks(chunk) {
        let mut args = vec!["-m".to_string(), "-q".to_string(), "-overwrite_original".to_string()];
        for t in group {
            args.push(format!("-{}:{}={}", t.group, t.name, t.sample.as_deref().unwrap_or("")));
        }
        args.push(img.display().to_string());
        run_cmd(tools.exiftool, &args, 120);
    }

    let et = exiftool_json(tools, &img);
    let (ox, oxerr) = oxidex_json(tools, &img);

    for t in tags {
        let et_val = find_in_exiftool_json(&et, t, false);
        let Some(et_val) = et_val else {
            results.insert(key_of(t), ResultEntry { read: Some("NO_SAMPLE".into()), ..Default::default() });
            continue;
        };
        match &ox {
            None => {
                results.insert(key_of(t), ResultEntry {
                    read: Some("OXIDEX_PARSE_FAIL".into()), et_val: Some(value_to_str(et_val)),
                    read_detail: oxerr.clone().map(|e| e.chars().take(200).collect()),
                    ..Default::default()
                });
            }
            Some(ox) => {
                results.insert(key_of(t), resolve_read(ox, t, et_val));
            }
        }
    }
    results
}
```

- [ ] **Step 4: Port the two-phase read orchestration (jpeg_tag_matrix.py:580-613) as a function callable from Task 8's run()**

```rust
use rayon::prelude::*;

pub fn run_read_phase(tools: &Tools, base: &Path, tags: &[ManifestTag]) -> HashMap<String, ResultEntry> {
    let mut by_group: HashMap<String, Vec<ManifestTag>> = HashMap::new();
    for t in tags {
        by_group.entry(t.group.clone()).or_default().push(t.clone());
    }
    println!("Testing {} tags across {} groups", tags.len(), by_group.len());

    // READ phase 1: one batch per group, groups in parallel
    let group_results: Vec<HashMap<String, ResultEntry>> = by_group.par_iter()
        .map(|(_, ts)| read_test_group(tools, base, ts))
        .collect();
    let mut read_res: HashMap<String, ResultEntry> = HashMap::new();
    for gr in group_results {
        read_res.extend(gr);
    }
    println!("READ batch phase done");

    // READ phase 2: individually retest every non-OK tag so one poison tag /
    // aborted chunk / mandatory-tag interaction can't contaminate a group.
    let retest: Vec<&ManifestTag> = tags.iter().filter(|t| {
        matches!(read_res.get(&key_of(t)).and_then(|r| r.read.as_deref()),
                 Some("MISSING") | Some("MISMATCH") | Some("NO_SAMPLE") | Some("OXIDEX_PARSE_FAIL"))
    }).collect();
    println!("READ retest phase: {} tags individually", retest.len());

    let retested: Vec<(String, ResultEntry, Option<String>)> = retest.par_iter()
        .map(|t| {
            let mut single = read_test_single(tools, base, t);
            let batch_status = read_res.get(&key_of(t)).and_then(|r| r.read.clone());
            if single.read != batch_status {
                single.read_batch = batch_status.clone();
            }
            (key_of(t), single, batch_status)
        })
        .collect();
    for (k, single, _) in retested {
        read_res.insert(k, single);
    }
    println!("READ phase done");
    read_res
}
```

- [ ] **Step 5: Write an integration test using the fake-exiftool/fake-oxidex fixture pattern from Task 4**

```rust
#[test]
fn read_test_group_marks_readable_tag_ok() {
    let td = tempfile::tempdir().unwrap();
    let base = td.path().join("base.jpg");
    std::fs::write(&base, b"fake").unwrap();
    // fake-exiftool-read.sh: `-j -G1 ...` prints a fixed JSON with the tag present.
    // fake-oxidex-read.sh: `-j -e ...` prints matching JSON.
    let tools = Tools {
        exiftool: "tests/fixtures/jpeg-tag-matrix/fake-exiftool-read.sh",
        oxidex: "tests/fixtures/jpeg-tag-matrix/fake-oxidex-read.sh",
    };
    let tag = ManifestTag {
        group: "ExifIFD".into(), name: "ISO".into(), family0: "EXIF".into(),
        writable: true, vtype: "int16u".into(), protected: false,
        flags: None, count: None, sample: Some("200".into()), sample_is_file: None, noop: None,
    };
    let res = read_test_group(&tools, &base, std::slice::from_ref(&tag));
    assert_eq!(res["ExifIFD:ISO"].read.as_deref(), Some("OK"));
}
```

Add the two fake scripts:
```bash
# tests/fixtures/jpeg-tag-matrix/fake-exiftool-read.sh
#!/usr/bin/env bash
echo '[{"ExifIFD:ISO": "200"}]'
```
```bash
# tests/fixtures/jpeg-tag-matrix/fake-oxidex-read.sh
#!/usr/bin/env bash
echo '[{"ExifIFD:ISO": "200"}]'
```
`chmod +x` both.

- [ ] **Step 6: Run the test**

Run: `cargo test --bin jpeg-tag-matrix --features jpeg-tag-matrix-binary read_test_group_marks_readable_tag_ok`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/bin/jpeg-tag-matrix/matrix.rs tests/fixtures/jpeg-tag-matrix/
git commit -m "port read phase (batch + individual retest) to Rust"
```

---

### Task 8: matrix.rs — write phase and bug-classification post-processing

**Files:**
- Modify: `src/bin/jpeg-tag-matrix/matrix.rs`

**Interfaces:**
- Consumes: everything from Tasks 5-7
- Produces: `matrix::write_test_tag`, `matrix::apply_bug_classification`, `matrix::run(args: RunArgs) -> anyhow::Result<()>` (the subcommand entry point, replacing Task 1's stub).

This is the largest and most state-heavy function in the whole pipeline (`write_test_tag`, `jpeg_tag_matrix.py:447-545`, ~100 lines with base-value comparison to detect silent no-ops). Port it as a single function rather than decomposing further — the Python keeps it as one function precisely because its internal state (base values, per-attempt results) doesn't factor cleanly, and splitting it in Rust would just require threading the same state through extra parameters.

- [ ] **Step 1: Port exiftool_validate_warnings (jpeg_tag_matrix.py:440-444)**

```rust
static NONSTANDARD_WARNING_MARKERS: &[&str] =
    &["Non-standard format", "Non-standard count", "Missing required"];

fn exiftool_validate_warnings(tools: &Tools, path: &Path) -> std::collections::HashSet<String> {
    let (_, out, _) = run_cmd(tools.exiftool,
        &["-validate".into(), "-warning".into(), "-a".into(), path.display().to_string()], 30);
    out.lines()
        .filter_map(|ln| {
            let (prefix, rest) = ln.split_once(':')?;
            if prefix.contains("Validate") { return None; }
            Some(rest.trim().to_string())
        })
        .collect()
}
```

- [ ] **Step 2: Port write_test_tag (jpeg_tag_matrix.py:447-545)**

```rust
pub struct WriteContext<'a> {
    pub base_ox: &'a Value,
    pub base_et: &'a Value,
    pub base_validate_warnings: &'a std::collections::HashSet<String>,
}

pub fn write_test_tag(tools: &Tools, base: &Path, tag: &ManifestTag, ctx: &WriteContext) -> ResultEntry {
    let base_ox_val = find_in_json(ctx.base_ox, &oxidex_read_keys(tag)).1.map(value_to_str);
    let base_et_val = find_in_exiftool_json(ctx.base_et, tag, true).map(value_to_str);
    let sample = tag.sample.clone().unwrap_or_default();

    let mut res = ResultEntry { write: Some("ERROR".into()), detail: Some(String::new()), ..Default::default() };

    for wkey in oxidex_write_keys(tag) {
        let td = tempfile::tempdir().unwrap();
        let img = td.path().join("t.jpg");
        std::fs::copy(base, &img).unwrap();
        let spec = format!("-{wkey}={sample}");
        let (code, out, err) = run_cmd(tools.oxidex, &[spec, img.display().to_string()], 30);
        let errtext = format!("{err}{out}").trim().to_string();
        if code != 0 || errtext.contains("Error:") {
            res = ResultEntry { write: Some("ERROR".into()), wkey: Some(wkey),
                                 detail: Some(errtext.chars().take(200).collect()), ..Default::default() };
            continue;
        }
        let ox = oxidex_json(tools, &img).0;
        let et = exiftool_json(tools, &img);
        if et.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            res = ResultEntry { write: Some("CORRUPTS_FILE".into()), wkey: Some(wkey),
                                 detail: Some("exiftool cannot parse output file".into()), ..Default::default() };
            continue;
        }
        let et_val = find_in_exiftool_json(&et, tag, true).map(value_to_str);
        let ox_val = ox.as_ref().and_then(|ox| find_in_json(ox, &oxidex_read_keys(tag)).1).map(value_to_str);
        let mut ox_key_used: Option<String> = None;

        let sample_eq_base_ox = base_ox_val.as_deref().map(|b| b.trim() == sample.trim()).unwrap_or(false);
        let sample_eq_base_et = base_et_val.as_deref().map(|b| b.trim() == sample.trim()).unwrap_or(false);
        let ox_unchanged = ox_val.is_some() && base_ox_val.is_some()
            && ox_val.as_deref().unwrap().trim() == base_ox_val.as_deref().unwrap().trim()
            && !sample_eq_base_ox;
        let et_unchanged = et_val.is_some() && base_et_val.is_some()
            && et_val.as_deref().unwrap().trim() == base_et_val.as_deref().unwrap().trim()
            && !sample_eq_base_et;

        let mut ox_ok = ox_val.is_some() && !ox_unchanged && values_match(&sample, ox_val.as_deref().unwrap_or(""));
        let et_ok = et_val.is_some() && !et_unchanged && values_match(&sample, et_val.as_deref().unwrap_or(""));

        // Registry asymmetry: oxidex has no display name for this tag, but the
        // value landed correctly under a raw/hex key in the same group.
        let mut ox_val = ox_val;
        if !ox_ok && et_ok {
            if let Some(ox_ref) = &ox {
                let (fk, fv) = find_same_group_fallback(ox_ref, tag, &sample);
                if let (Some(fk), Some(fv)) = (fk, fv) {
                    ox_key_used = Some(fk);
                    ox_val = Some(value_to_str(fv));
                    ox_ok = true;
                }
            }
        }

        if ox_ok && et_ok {
            let mut result = ResultEntry {
                write: Some("OK".into()), wkey: Some(wkey),
                write_ox_val: ox_val.clone(), write_et_val: et_val.clone(),
                ..Default::default()
            };
            if let Some(k) = ox_key_used {
                result.write_ox_key = Some(k);
                result.bug_cluster = Some("R4-registry-asymmetry".into());
            }
            let new_warnings: std::collections::HashSet<String> =
                exiftool_validate_warnings(tools, &img)
                    .difference(ctx.base_validate_warnings).cloned().collect();
            let real_warnings: Vec<&String> = new_warnings.iter()
                .filter(|w| NONSTANDARD_WARNING_MARKERS.iter().any(|m| w.contains(m)))
                .collect();
            if !real_warnings.is_empty() {
                result.write_quality = Some("nonstandard".into());
                let mut sorted: Vec<&str> = real_warnings.iter().map(|s| s.as_str()).collect();
                sorted.sort();
                result.write_warnings = Some(sorted.join("; ").chars().take(200).collect());
            }
            return result;
        }

        if ox_unchanged && et_unchanged {
            res = ResultEntry { write: Some("NOT_WRITTEN".into()), wkey: Some(wkey),
                                 detail: Some("silent no-op: value unchanged from pristine base fixture".into()),
                                 ..Default::default() };
            continue;
        }
        if et_ok && !ox_ok {
            res = ResultEntry { write: Some("READBACK_BROKEN".into()), wkey: Some(wkey),
                                 detail: Some(format!("exiftool sees {et_val:?}, oxidex sees {ox_val:?}")),
                                 ..Default::default() };
        } else if ox_ok && !et_ok {
            res = ResultEntry { write: Some("INTEROP_BROKEN".into()), wkey: Some(wkey),
                                 detail: Some(format!("oxidex reads back {ox_val:?} but exiftool sees {et_val:?}")),
                                 ..Default::default() };
        } else if ox_val.is_some() || et_val.is_some() {
            res = ResultEntry { write: Some("VALUE_MISMATCH".into()), wkey: Some(wkey),
                                 detail: Some(format!("wrote {sample:?}; oxidex={ox_val:?} exiftool={et_val:?}")),
                                 ..Default::default() };
        } else {
            res = ResultEntry { write: Some("NOT_WRITTEN".into()), wkey: Some(wkey),
                                 detail: Some(format!("exit 0 but tag absent on read-back; stderr: {}",
                                                       errtext.chars().take(150).collect::<String>())),
                                 ..Default::default() };
        }
    }
    res
}
```

- [ ] **Step 3: Port apply_bug_classification (jpeg_tag_matrix.py:240-267)**

```rust
pub fn apply_bug_classification(results: &mut HashMap<String, ResultEntry>) {
    for r in results.values_mut() {
        // Independent axes -- a tag can be both read=MISMATCH and
        // write=INTEROP_BROKEN at once, so these must not be if/else'd.
        if r.read.as_deref() == Some("MISMATCH") {
            if let Some(bug) = classify_read_mismatch(r) {
                r.read_bug = Some(bug.to_string());
            } else {
                r.read = Some("MISMATCH_FORMAT".into());
                r.read_note = Some("value equivalent; oxidex shows stored/raw form, \
                                    exiftool applies PrintConv".into());
            }
        }
        if r.write.as_deref() == Some("INTEROP_BROKEN") && r.bug_cluster.is_none() {
            if let Some(cluster) = write_bug_cluster_for(&r.name) {
                r.bug_cluster = Some(cluster.to_string());
            }
        }
    }
}
```

- [ ] **Step 4: Wire up run() — replaces the stub from Task 1 (jpeg_tag_matrix.py:552-648)**

```rust
use clap::Args;

#[derive(Args)]
pub struct RunArgs {
    #[arg(long)]
    pub only_group: Option<String>,
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long)]
    pub skip_write: bool,
    #[arg(long, help = "redo READ phase only; merge into existing results.json")]
    pub reread: bool,
    #[arg(long, default_value_t = 8)]
    pub workers: usize,
}

pub fn run(args: RunArgs) -> anyhow::Result<()> {
    let exiftool = std::env::var("EXIFTOOL").unwrap_or_else(|_| "exiftool".into());
    let repo = std::env::current_dir()?;
    let oxidex = std::env::var("OXIDEX")
        .unwrap_or_else(|_| repo.join("target/release/oxidex").display().to_string());
    let work = std::env::var("TAGMATRIX_WORK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("oxidex-tagmap"));
    let base = std::env::var("TAGMATRIX_BASE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| repo.join("tests/fixtures/jpeg/tag_matrix_base.jpg"));
    let results_path = work.join("results.json");
    let tools = Tools { exiftool: &exiftool, oxidex: &oxidex };

    rayon::ThreadPoolBuilder::new().num_threads(args.workers).build_global().ok();

    let manifest: ManifestFile = serde_json::from_str(&std::fs::read_to_string(work.join("exiftool_jpeg_tags.json"))?)?;
    let mut tags: Vec<ManifestTag> = manifest.tags.into_iter().filter(|t| t.writable).collect();
    if let Some(g) = &args.only_group {
        tags.retain(|t| &t.group == g);
    }
    if let Some(limit) = args.limit {
        tags.truncate(limit);
    }

    let mut results: HashMap<String, ResultEntry> = HashMap::new();
    let mut skip_write = args.skip_write;
    if args.reread && results_path.exists() {
        results = serde_json::from_str(&std::fs::read_to_string(&results_path)?)?;
        skip_write = true;
    }

    let read_res = run_read_phase(&tools, &base, &tags);
    for t in &tags {
        let entry = results.entry(key_of(t)).or_default();
        // drop stale read fields before merging fresh read results
        *entry = ResultEntry { write: entry.write.clone(), wkey: entry.wkey.clone(),
                                detail: entry.detail.clone(), write_ox_val: entry.write_ox_val.clone(),
                                write_et_val: entry.write_et_val.clone(), write_ox_key: entry.write_ox_key.clone(),
                                bug_cluster: entry.bug_cluster.clone(), write_quality: entry.write_quality.clone(),
                                write_warnings: entry.write_warnings.clone(), ..Default::default() };
        if let Some(r) = read_res.get(&key_of(t)) {
            entry.read = r.read.clone();
            entry.read_batch = r.read_batch.clone();
            entry.read_detail = r.read_detail.clone();
            entry.read_bug = r.read_bug.clone();
            entry.read_note = r.read_note.clone();
            entry.ox_key = r.ox_key.clone();
            entry.ox_val = r.ox_val.clone();
            entry.et_val = r.et_val.clone();
        }
    }

    if !skip_write {
        let base_ox = oxidex_json(&tools, &base).0.unwrap_or_else(|| Value::Object(Default::default()));
        let base_et = exiftool_json(&tools, &base);
        let base_validate_warnings = exiftool_validate_warnings(&tools, &base);
        let ctx = WriteContext { base_ox: &base_ox, base_et: &base_et,
                                  base_validate_warnings: &base_validate_warnings };
        let write_results: Vec<(String, ResultEntry)> = tags.par_iter()
            .map(|t| (key_of(t), write_test_tag(&tools, &base, t, &ctx)))
            .collect();
        for (k, wr) in write_results {
            let entry = results.entry(k).or_default();
            entry.write = wr.write; entry.wkey = wr.wkey; entry.detail = wr.detail;
            entry.write_ox_val = wr.write_ox_val; entry.write_et_val = wr.write_et_val;
            entry.write_ox_key = wr.write_ox_key; entry.bug_cluster = wr.bug_cluster;
            entry.write_quality = wr.write_quality; entry.write_warnings = wr.write_warnings;
        }
    }

    for t in &tags {
        let r = results.entry(key_of(t)).or_default();
        r.group = t.group.clone();
        r.name = t.name.clone();
        r.sample = t.sample.clone().unwrap_or_default();
        r.vtype = Some(t.vtype.clone());
        r.protected = t.protected;
    }

    apply_bug_classification(&mut results);

    std::fs::write(&results_path, serde_json::to_string_pretty(&results)?)?;
    let mut counts: HashMap<(Option<String>, Option<String>), u32> = HashMap::new();
    for r in results.values() {
        *counts.entry((r.read.clone(), r.write.clone())).or_insert(0) += 1;
    }
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    for ((rd, wr), n) in sorted {
        println!("  read={:<18} write={:<18} {n}", rd.unwrap_or_default(), wr.unwrap_or_default());
    }
    println!("Results: {}", results_path.display());
    Ok(())
}
```

Note on `--reread`'s stale-field-clearing: the Python drops exactly `read, read_batch, read_detail, read_bug, read_note, ox_key, ox_val, et_val` before merging (`jpeg_tag_matrix.py:609-611`) — the Rust version above rebuilds the entry keeping only the write-side fields, which is equivalent since `ResultEntry`'s only fields are the read-side ones (dropped) and write-side ones (kept) plus the manifest-attached ones (re-set unconditionally a few lines later anyway).

- [ ] **Step 5: Run the full matrix.rs test suite**

Run: `cargo test --bin jpeg-tag-matrix --features jpeg-tag-matrix-binary`
Expected: all tests from Tasks 5-8 PASS.

- [ ] **Step 6: Commit**

```bash
git add src/bin/jpeg-tag-matrix/matrix.rs
git commit -m "port write phase and run() orchestration to Rust"
```

---

### Task 9: report.rs — classification, KNOWN_BUGS, markdown generation

**Files:**
- Modify: `src/bin/jpeg-tag-matrix/report.rs`

**Interfaces:**
- Consumes: `types::{ResultEntry, ReadonlyFile}` (Task 1)
- Produces: `report::classify(r: &ResultEntry) -> (String, String, String)`, `report::run(args: ReportArgs) -> anyhow::Result<()>` (replaces Task 1's stub).

- [ ] **Step 1: Write failing tests for classify() (jpeg_tag_report.py:54-110)**

```rust
#[cfg(test)]
mod classify_tests {
    use super::*;

    fn entry(read: &str, write: &str) -> ResultEntry {
        ResultEntry { read: Some(read.into()), write: Some(write.into()), ..Default::default() }
    }

    #[test]
    fn full_read_and_write() {
        let (head, rd, wr) = classify(&entry("OK", "OK"));
        assert_eq!(head, "✅ Full (read + write)");
        assert_eq!(rd, "✅ ok");
        assert_eq!(wr, "✅ ok");
    }

    #[test]
    fn write_nonstandard_encoding() {
        let mut r = entry("OK", "OK");
        r.write_quality = Some("nonstandard".into());
        let (head, _, wr) = classify(&r);
        assert_eq!(head, "⚠️ Full (write non-standard encoding)");
        assert_eq!(wr, "⚠️ writes, but non-standard encoding (exiftool tolerates)");
    }

    #[test]
    fn read_only() {
        let (head, _, _) = classify(&entry("OK", "NOT_WRITTEN"));
        assert_eq!(head, "📖 Read only");
    }

    #[test]
    fn read_ok_write_broken() {
        let (head, _, _) = classify(&entry("OK", "INTEROP_BROKEN"));
        assert_eq!(head, "🐛 Read OK, write broken");
    }

    #[test]
    fn read_broken() {
        let (head, rd, _) = classify(&entry("MISMATCH", "NOT_WRITTEN"));
        assert_eq!(head, "🐛 Read broken");
        assert_eq!(rd, "🐛 broken: wrong value decoded");
    }

    #[test]
    fn unsupported() {
        let (head, _, _) = classify(&entry("MISSING", "NOT_WRITTEN"));
        assert_eq!(head, "❌ Unsupported");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin jpeg-tag-matrix --features jpeg-tag-matrix-binary classify_tests`
Expected: FAIL — `classify` not defined.

- [ ] **Step 3: Port READ_BUG_LABELS/WRITE_BUG_LABELS and classify() (jpeg_tag_report.py:33-110)**

```rust
static READ_BUG_LABELS: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    [("R-iptc-binary-garbage", "IPTC binary int16u decoded as NUL-garbage string"),
     ("R-binary-garbage", "binary value decoded as NUL-garbage string"),
     ("R-apex-missing", "APEX ValueConv (2^x) not applied"),
     ("R-namespace-blind-printconv", "-e compat applies EXIF enum table to XMP tag"),
     ("R-acr-prefix", "ACR \"TagName: \" ValueConv prefix not stripped"),
     ("R-undef-not-decoded", "undef/binary value shown as opaque (Binary, N bytes)"),
     ("R-utf16-not-decoded", "XP* UTF-16 string shown as raw integer"),
     ("R-float-raw-bits", "float value shown as raw IEEE-754 bits"),
     ("R-xmp-struct-concat", "XMP struct fields concatenated into garbage scalar")]
        .into_iter().collect()
});
static WRITE_BUG_LABELS: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    [("I1-no-printconvinv", "PrintConvInv missing: human-readable stored as raw"),
     ("I2-wrong-type-enum", "written as ASCII where SHORT/LONG expected"),
     ("I3-wrong-type-numeric", "written as ASCII where numeric/rational expected"),
     ("I4-wrong-type-undef", "written as ASCII where UNDEF expected (+NUL)"),
     ("I5-subdir-poison", "junk written into subdirectory pointer tag"),
     ("R4-registry-asymmetry", "writes OK; reads back under hex key (registry asymmetry)")]
        .into_iter().collect()
});

pub fn classify(r: &ResultEntry) -> (String, String, String) {
    let rd = r.read.as_deref();
    let wr = r.write.as_deref();
    let detail = r.detail.clone().unwrap_or_default().to_lowercase();

    let wr_c = match wr {
        Some("OK") => {
            if r.write_quality.as_deref() == Some("nonstandard") {
                "⚠️ writes, but non-standard encoding (exiftool tolerates)".to_string()
            } else if r.bug_cluster.as_deref() == Some("R4-registry-asymmetry") {
                "✅ ok (reads back under hex key)".to_string()
            } else {
                "✅ ok".to_string()
            }
        }
        Some("ERROR") => {
            if detail.contains("type mismatch") {
                "🐛 broken: type-validation rejects CLI string values".to_string()
            } else if detail.contains("shift dates") {
                "🐛 broken: datetime write misrouted to date-shift path".to_string()
            } else {
                "🐛 broken: error".to_string()
            }
        }
        Some("INTEROP_BROKEN") => format!("🐛 broken: {}",
            r.bug_cluster.as_deref().and_then(|c| WRITE_BUG_LABELS.get(c)).copied()
                .unwrap_or("interop (exiftool can't read it)")),
        Some("NOT_WRITTEN") => "— unsupported (silent no-op)".to_string(),
        other => other.unwrap_or("").to_lowercase(),
    };

    let rd_c = match rd {
        Some("OK") => "✅ ok".to_string(),
        Some("MISMATCH_FORMAT") => "✅ ok (formatting differs from exiftool)".to_string(),
        Some("MISMATCH") => format!("🐛 broken: {}",
            r.read_bug.as_deref().and_then(|b| READ_BUG_LABELS.get(b)).copied()
                .unwrap_or("wrong value decoded")),
        Some("MISSING") => "— unsupported".to_string(),
        Some("NO_SAMPLE") => "❔ untestable (exiftool could not synthesize a sample)".to_string(),
        other => other.unwrap_or("").to_lowercase(),
    };

    let read_okish = matches!(rd, Some("OK") | Some("MISMATCH_FORMAT"));
    let head = if read_okish && wr == Some("OK") {
        if r.write_quality.as_deref() == Some("nonstandard") {
            "⚠️ Full (write non-standard encoding)".to_string()
        } else {
            "✅ Full (read + write)".to_string()
        }
    } else if read_okish && matches!(wr, Some("ERROR") | Some("INTEROP_BROKEN")) {
        "🐛 Read OK, write broken".to_string()
    } else if read_okish {
        "📖 Read only".to_string()
    } else if rd == Some("MISMATCH") {
        "🐛 Read broken".to_string()
    } else if wr == Some("OK") {
        "✍️ Write only".to_string()
    } else if rd == Some("NO_SAMPLE") {
        "❔ Untestable".to_string()
    } else {
        "❌ Unsupported".to_string()
    };

    (head, rd_c, wr_c)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin jpeg-tag-matrix --features jpeg-tag-matrix-binary classify_tests`
Expected: all 6 tests PASS.

- [ ] **Step 5: Port the KNOWN_BUGS constant verbatim (jpeg_tag_report.py:138-172)**

This is committed documentation content — copy it byte-for-byte into a Rust raw string, only translating Python's `"""..."""` to Rust's `r#"..."#`.

```rust
pub const KNOWN_BUGS: &str = r#"
## Known bugs (empirically confirmed, with root cause)

All were reproduced with the release binary against exiftool 13.55; file:line
references are into this repo.

### Write path

| # | Bug | Impact (tags) | Root cause |
|---|---|---|---|
| W1 | CLI cannot write any non-String EXIF tag: values reach the writer as `TagValue::String` and strict type validation rejects them ("Type mismatch: expected Integer/Rational/... but got String") | 191 | `src/main.rs:146` wraps all CLI values as strings; `src/core/validation.rs:109`. A fix exists on the `fix/wiring` branch (commit `2433c79`) but is not on this branch |
| W2 | Datetime tags cannot be *created* on a JPEG with no pre-existing value for that tag: `-ExifIFD:DateTimeOriginal=...` is misrouted to the date-*shift* path (`src/core/date_shift.rs`), which requires a matching tag to already exist ("No date/time tags matching ... found in EXIF data" otherwise). This is what the empirical matrix measures, since the base fixture has no pre-existing date tags. **Update-in-place now works** (fixed by [#18](https://github.com/swack-tools/oxidex/pull/18)): `-ExifIFD:DateTimeOriginal=...` against a file that already has the tag set succeeds and round-trips correctly through both oxidex and exiftool | 82 (fresh-creation case only) | `src/cli/args.rs` `parse_date_shift` treats any `date/time`-named tag with a `Y:M:D H:M:S`-shaped value as a shift; the shift path itself only rewrites existing entries |
| W3 | Silent no-op with false success: XMP, IPTC, JFIF, Photoshop, Comment, InteropIFD and bare tag names are dropped, yet the CLI prints "1 image files updated" and rewrites the file | ~4,300 (incl. 1,297 readable tags) | `src/writers/tiff_writer/tiff/validator.rs:110` `separate_by_ifd` routes only IFD0/IFD1/ExifIFD/GPS/EXIF; `ifd_builder.rs:93` skips silently; `src/main.rs:175` unconditional success message |
| W4 | Type-blind serialization: registry-unknown tags are written as TIFF ASCII regardless of expected SHORT/LONG/RATIONAL/UNDEF type — produces non-standard files (`exiftool -validate` flags every one) | 32 | `src/writers/tiff_writer/tiff/ifd_entry.rs:95` picks TIFF type from the Rust value variant, never from the tag's spec |
| W5 | No PrintConv inversion: human-readable values stored raw (e.g. `GPSSpeedRef=km/h` stores "km/h" instead of "K"; exiftool then reads "Unknown (km/h)") | 8 | no inverse-conversion layer exists (`src/core/operations.rs:291` inserts CLI strings untouched) |
| W6 | Subdirectory-pointer poisoning: writing text into pointer tags (CurrentICCProfile, AsShotICCProfile, ...) corrupts downstream parsing ("Bad length ICC_Profile" on every later read) | 7 | `validator.rs:48` checks only family + numeric ID, not writability/subdirectory flags |
| W7 | Every write flips EXIF byte order MM→II (big- to little-endian) even when only one tag changes | all writes | EXIF segment fully rebuilt by `src/writers/tiff_writer/mod.rs` in II order |
| W8 | Write/read registry asymmetry: `IFD0:TargetPrinter` writes correctly but reads back as `IFD0:0x0151` | ≥1 | write resolves via manual `TAG_REGISTRY` (`src/tag_db/tag_registry.rs:1281`), read via YAML `TAG_ID_TO_NAME_INDEX` (`src/tag_db/mod.rs:302`) which lacks the ID |
| W9 | Some write errors print `Error: ...` but exit 0 (e.g. GPS rational type mismatch) — scripts can't trust the exit code | — | inconsistent error propagation in `src/main.rs` write handling |
| W10 | 57 of the 122 "successful" CLI writes actually serialize a non-standard encoding (string where numeric expected, wrong count) that exiftool tolerates on read but `-validate` flags — same root cause as W4 | 57 | `src/writers/tiff_writer/tiff/ifd_entry.rs:95` |
| W11 | Creating a GPS IFD does not add the mandatory `GPSVersionID` tag (exiftool adds it automatically) — `exiftool -validate` flags every oxidex-created GPS IFD | GPS writes | GPS IFD builder in `src/writers/tiff_writer/` has no mandatory-tag logic |

### Read path

| # | Bug | Impact (tags) | Root cause |
|---|---|---|---|
| R1 | One malformed IFD entry silently discards the **entire EXIF block** (IFD0 + ExifIFD + GPS). Trigger found in the wild: exiftool 13.55 itself writes `IFD0:GeoTiffDoubleParams` with a bad offset | whole file | `src/parsers/tiff/ifd_parser.rs:274` aborts `parse_ifd` on one bad entry; `src/core/jpeg_helpers.rs:161` swallows the error (`if let Ok`) |
| R2 | IPTC binary int16u datasets (FileFormat, FileVersion, ARMIdentifier, ARMVersion, ObjectPreviewFileFormat) decoded as strings → NUL-byte garbage | 5 | `src/parsers/jpeg/iptc_parser.rs:330-374`; a correct `parse_binary_u16` exists in `iptc_record1.rs:383` but isn't on the live path |
| R3 | `-e` compat mode applies EXIF enum tables to XMP tags by bare name (namespace-blind): e.g. XMP-exif:SensingMethod `1` renders "Not defined" instead of "Monochrome area"; plain `-j` output is correct | 7 | `src/core/exiftool_compat.rs:172` strips the group before enum lookup |
| R4 | APEX conversion missing: ApertureValue/MaxApertureValue/ShutterSpeedValue show the raw APEX number (or raw rational for XMP) instead of `2^(v/2)` / `2^(-v)` | 6 | no APEX code anywhere (`src/core/value_formatter.rs:425` only divides num/den) |
| R5 | XMP struct properties concatenated into a single garbage scalar (e.g. Flash → "TrueTrue0True0"), flattened child tags dropped | 4+ | `src/parsers/xmp/rdf_parser.rs:181-191` appends nested text nodes without separators |
| R6 | ACR-style `"TagName: value"` ValueConv prefix not stripped (Brightness, Shadows, ...) | 6+ | no equivalent of exiftool's ValueConv for these tags |
| R7 | undef values undecoded: FileSource shows "(Binary, 1 bytes)" instead of "Film Scanner"; GPSAreaInformation opaque; XP* Windows tags show raw integers instead of UTF-16 text; float-typed DNG tags show raw IEEE-754 bits (e.g. 1069547520 for 1.5) | 24 | `src/core/exiftool_compat.rs:435` (FileSource requires as_integer), no UTF-16/float decode paths |
| R8 | **Fixed by [#22](https://github.com/swack-tools/oxidex/pull/22).** Previously: JPEG COM comment, SPIFF and DQT-quality parsers existed but were never invoked from `parse_jpeg_metadata`, and multi-chunk ICC profiles were dropped with a warning. COM comment reading is now verified working (`File:Comment` reads correctly); SPIFF, DQT-derived quality, multi-chunk ICC, and APP6/GoPro are also wired in per #22 but aren't ExifTool-writable tags (or fall outside the EXIF/XMP/IPTC/JFIF/Photoshop/ICC_Profile groups this matrix synthesizes samples for), so this empirical harness can't independently confirm them | Comment (confirmed); SPIFF/DQT/ICC/APP6 (out of this harness's testable scope) | was `src/core/operations.rs:483-497`; see #22 for the fix |
"#;
```

**Important:** this table describes bugs in `oxidex`'s parsers/writers as of when it was last hand-written — it is not derived from the current run's data. If any of these bugs get fixed between now and when this task is done (check `git log` on the referenced files before porting), update the table to match reality rather than copying stale claims forward. Cross-check row R8's "Fixed by #22" framing and W2's "Fixed by #18" framing especially, since those are the two rows already mid-transition.

- [ ] **Step 6: Port autogen_callout and md_escape (jpeg_tag_report.py:113-135)**

```rust
fn md_escape(s: &str, n: usize) -> String {
    let escaped = s.replace('|', "\\|").replace('\n', " ");
    escaped.chars().take(n).collect()
}

fn autogen_callout(work: &Path) -> String {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let manifest_path = work.join("exiftool_jpeg_tags.json");
    let ver = std::fs::read_to_string(&manifest_path).ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("generated_by").and_then(|g| g.as_str()).map(String::from))
        .unwrap_or_else(|| "ExifTool".to_string());
    format!(
        "::: info Auto-Generated\n\
         This report is regenerated by the \
         [JPEG Tag Matrix workflow]\
         (https://github.com/swack-tools/oxidex/actions/workflows/\
         jpeg-tag-matrix.yml) \
         (`scripts/jpeg_tag_matrix.py`) against **{ver}**. \
         Last updated: **{today}**\n\
         :::\n"
    )
}
```

Leave the `scripts/jpeg_tag_matrix.py` reference in the callout text as-is for this task — it's committed doc *content*, and updating it to reference the new Rust binary belongs in Task 13 alongside the workflow file changes, not buried in this port.

- [ ] **Step 7: Port the two markdown-generation blocks (jpeg_tag_report.py:175-299) and baseline handling (jpeg_tag_report.py:305-349)**

```rust
use std::collections::BTreeMap;

#[derive(Args)]
pub struct ReportArgs {
    #[arg(long, help = "write current summary counts to jpeg-tag-baseline.json")]
    pub update_baseline: bool,
    #[arg(long, help = "exit 1 if support regressed vs the committed baseline")]
    pub check_baseline: bool,
}

struct Row {
    key: String, group: String, name: String, sample: String, vtype: String,
    head: String, rd: String, wr: String, ox_key: String, wkey: String,
    raw_read: Option<String>, raw_write: Option<String>, wq: Option<String>,
}

pub fn run(args: ReportArgs) -> anyhow::Result<()> {
    let repo = std::env::current_dir()?;
    let work = std::env::var("TAGMATRIX_WORK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("oxidex-tagmap"));
    let results_path = work.join("results.json");
    let readonly_path = work.join("exiftool_jpeg_readonly_tags.json");
    let out_support = repo.join("docs/reference/jpeg-tag-support.md");
    let out_matrix = repo.join("docs/reference/jpeg-tag-matrix.md");
    let baseline_path = repo.join("docs/reference/jpeg-tag-baseline.json");

    let results: HashMap<String, ResultEntry> = serde_json::from_str(&std::fs::read_to_string(&results_path)?)?;
    let mut rows: Vec<Row> = results.iter().map(|(key, r)| {
        let (head, rd, wr) = classify(r);
        Row {
            key: key.clone(),
            group: if r.group.is_empty() { key.split(':').next().unwrap_or("").to_string() } else { r.group.clone() },
            name: if r.name.is_empty() { key.rsplit(':').next().unwrap_or("").to_string() } else { r.name.clone() },
            sample: r.sample.clone(), vtype: r.vtype.clone().unwrap_or_default(),
            head, rd, wr,
            ox_key: r.ox_key.clone().unwrap_or_default(), wkey: r.wkey.clone().unwrap_or_default(),
            raw_read: r.read.clone(), raw_write: r.write.clone(), wq: r.write_quality.clone(),
        }
    }).collect();
    rows.sort_by(|a, b| (&a.group, &a.name).cmp(&(&b.group, &b.name)));

    let mut heads: BTreeMap<String, u32> = BTreeMap::new();
    for x in &rows { *heads.entry(x.head.clone()).or_insert(0) += 1; }
    let mut by_group: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for x in &rows { by_group.entry(x.group.clone()).or_default().push(x); }

    const READ_OKISH: &[&str] = &["OK", "MISMATCH_FORMAT"];
    let n_read = rows.iter().filter(|x| x.raw_read.as_deref().map(|r| READ_OKISH.contains(&r)).unwrap_or(false)).count();
    let n_write = rows.iter().filter(|x| x.raw_write.as_deref() == Some("OK")).count();

    // ---------------------------------------------------- supported mapping doc
    let mut md = vec![
        "# JPEG Tag Support\n".to_string(),
        autogen_callout(&work),
        "Empirical OxiDex ↔ ExifTool tag mapping for JPEG: for each tag, ExifTool writes a \
         sample into a clean JPEG and `oxidex -j -e` must read it back; writability additionally \
         requires `oxidex -KEY=VALUE` to round-trip through both OxiDex and ExifTool.\n".to_string(),
        "Only tags OxiDex can **read** from JPEG are listed here (including those whose value \
         formatting differs from ExifTool). The full classification of all tested tags — \
         including unsupported and broken ones — is in the \
         [JPEG Tag Matrix](/reference/jpeg-tag-matrix). See also \
         [ExifTool Coverage](/reference/tag-coverage-analysis) for the tag-database view and the \
         [Compatibility overview](/reference/comparison/) for fixture-based comparisons across \
         formats.\n".to_string(),
    ];
    md.push(format!("\n**{n_read}** ExifTool tags readable, **{n_write}** writable via the CLI \
                      (of {} ExifTool-writable JPEG tags tested).\n", rows.len()));
    for (g, xs) in &by_group {
        let sup: Vec<&&Row> = xs.iter().filter(|x| x.raw_read.as_deref().map(|r| READ_OKISH.contains(&r)).unwrap_or(false)).collect();
        if sup.is_empty() { continue; }
        md.push(format!("\n## {g} ({} readable tags)\n", sup.len()));
        md.push("| ExifTool tag | OxiDex key | OxiDex write | Example value |".to_string());
        md.push("|---|---|---|---|".to_string());
        for x in &sup {
            let wr_cell = if x.raw_write.as_deref() == Some("OK") {
                let mark = if x.wq.as_deref() == Some("nonstandard") { "⚠️" } else { "✅" };
                format!("{mark} `-{}=`", x.wkey)
            } else {
                "—".to_string()
            };
            let note = if x.raw_read.as_deref() == Some("MISMATCH_FORMAT") { " *" } else { "" };
            let ox_key_display = if x.ox_key.is_empty() { &x.key } else { &x.ox_key };
            md.push(format!("| `{}` | `{ox_key_display}`{note} | {wr_cell} | `{}` |",
                             x.key, md_escape(&x.sample, 60)));
        }
    }
    md.push("\n\\* value formatting differs from ExifTool (same underlying value, missing PrintConv).\n".to_string());
    std::fs::write(&out_support, md.join("\n") + "\n")?;

    // ------------------------------------------------------- full matrix doc
    let mut md = vec![
        "---".to_string(), "outline: 2".to_string(), "---\n".to_string(),
        "# JPEG Tag Matrix\n".to_string(),
        autogen_callout(&work),
        "Every ExifTool-writable JPEG tag, classified by empirical test: read support \
         (ExifTool writes → OxiDex reads), write support (OxiDex writes → both read back), \
         and known bugs with root causes. Readable tags with their working write keys are \
         summarized in [JPEG Tag Support](/reference/jpeg-tag-support).\n".to_string(),
        "\n## Summary\n".to_string(), "| Classification | Tags |".to_string(), "|---|---|".to_string(),
    ];
    let mut heads_sorted: Vec<(&String, &u32)> = heads.iter().collect();
    heads_sorted.sort_by(|a, b| b.1.cmp(a.1));
    for (h, n) in &heads_sorted {
        md.push(format!("| {h} | {n} |"));
    }
    let readonly_tags: Vec<ReadonlyTag> = if readonly_path.exists() {
        let ro: ReadonlyFile = serde_json::from_str(&std::fs::read_to_string(&readonly_path)?)?;
        md.push(format!("| 🚫 Not writable in ExifTool (no synthetic sample possible; untested) | {} |",
                         ro.tags.len()));
        ro.tags
    } else {
        vec![]
    };

    md.push("\n## Per-group breakdown\n".to_string());
    md.push("| Group | Full | Read-only | Write-broken | Read-broken | Write-only | Unsupported | Untestable | Total |".to_string());
    md.push("|---|---|---|---|---|---|---|---|---|".to_string());
    for (g, xs) in &by_group {
        let mut c: BTreeMap<&str, u32> = BTreeMap::new();
        for x in xs { *c.entry(x.head.as_str()).or_insert(0) += 1; }
        md.push(format!("| {g} | {} | {} | {} | {} | {} | {} | {} | {} |",
            c.get("✅ Full (read + write)").unwrap_or(&0), c.get("📖 Read only").unwrap_or(&0),
            c.get("🐛 Read OK, write broken").unwrap_or(&0), c.get("🐛 Read broken").unwrap_or(&0),
            c.get("✍️ Write only").unwrap_or(&0), c.get("❌ Unsupported").unwrap_or(&0),
            c.get("❔ Untestable").unwrap_or(&0), xs.len()));
    }

    md.push(KNOWN_BUGS.to_string());

    md.push("\n## Full matrix\n".to_string());
    for (g, xs) in &by_group {
        md.push(format!("\n### {g}\n"));
        md.push("| ExifTool tag | Read | Write | Example |".to_string());
        md.push("|---|---|---|---|".to_string());
        for x in xs {
            md.push(format!("| `{}` | {} | {} | `{}` |", x.name, x.rd, x.wr, md_escape(&x.sample, 60)));
        }
    }
    if !readonly_tags.is_empty() {
        md.push("\n### Not writable in ExifTool (read-only universe, untested)\n".to_string());
        md.push("These exist in JPEG-relevant groups but ExifTool itself cannot write them, \
                  so no synthetic test file could be produced.\n".to_string());
        let mut by_g: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for t in &readonly_tags { by_g.entry(&t.group).or_default().push(&t.name); }
        for (g, names) in &by_g {
            let mut sorted = names.clone();
            sorted.sort();
            let shown: Vec<String> = sorted.iter().take(40).map(|n| format!("`{n}`")).collect();
            let extra = if sorted.len() > 40 { format!(" … +{} more", sorted.len() - 40) } else { String::new() };
            md.push(format!("- **{g}** ({}): {}{extra}", sorted.len(), shown.join(", ")));
        }
    }
    std::fs::write(&out_matrix, md.join("\n") + "\n")?;

    println!("Wrote {}\nWrote {}\n", out_support.display(), out_matrix.display());
    for (h, n) in &heads_sorted {
        println!("  {h}: {n}");
    }

    // ------------------------------------------------------ baseline handling
    let counts = BaselineCounts {
        total_tested: rows.len() as u32,
        readable: n_read as u32,
        writable_cli: n_write as u32,
        full: *heads.get("✅ Full (read + write)").unwrap_or(&0),
        full_nonstandard: *heads.get("⚠️ Full (write non-standard encoding)").unwrap_or(&0),
        read_only: *heads.get("📖 Read only").unwrap_or(&0),
        read_broken: *heads.get("🐛 Read broken").unwrap_or(&0),
        write_broken: *heads.get("🐛 Read OK, write broken").unwrap_or(&0),
        unsupported: *heads.get("❌ Unsupported").unwrap_or(&0),
        untestable: *heads.get("❔ Untestable").unwrap_or(&0),
    };

    if args.update_baseline {
        write_baseline_one_space_indent(&baseline_path, &counts)?;
        println!("Baseline updated: {}", baseline_path.display());
    }

    if args.check_baseline {
        if !baseline_path.exists() {
            println!("No baseline committed yet; skipping check");
            return Ok(());
        }
        let base: BaselineCounts = serde_json::from_str(&std::fs::read_to_string(&baseline_path)?)?;
        let mut failures = Vec::new();
        if counts.readable < base.readable { failures.push(format!("readable regressed: {} -> {}", base.readable, counts.readable)); }
        if counts.writable_cli < base.writable_cli { failures.push(format!("writable_cli regressed: {} -> {}", base.writable_cli, counts.writable_cli)); }
        if counts.full < base.full { failures.push(format!("full regressed: {} -> {}", base.full, counts.full)); }
        if counts.read_broken > base.read_broken { failures.push(format!("read_broken regressed: {} -> {}", base.read_broken, counts.read_broken)); }
        if counts.write_broken > base.write_broken { failures.push(format!("write_broken regressed: {} -> {}", base.write_broken, counts.write_broken)); }

        print_deltas(&base, &counts);

        if !failures.is_empty() {
            println!("\nBASELINE REGRESSION:");
            for f in &failures { println!("  ✗ {f}"); }
            println!("If intentional, rerun with --update-baseline and commit jpeg-tag-baseline.json.");
            std::process::exit(1);
        }
        println!("Baseline check passed");
    }
    Ok(())
}

fn print_deltas(base: &BaselineCounts, counts: &BaselineCounts) {
    macro_rules! delta { ($field:ident) => {
        if base.$field != counts.$field {
            println!("  delta {}: {} -> {}", stringify!($field), base.$field, counts.$field);
        }
    }}
    delta!(total_tested); delta!(readable); delta!(writable_cli); delta!(full);
    delta!(full_nonstandard); delta!(read_only); delta!(read_broken); delta!(write_broken);
    delta!(unsupported); delta!(untestable);
}
```

Note: the Python's delta-printing iterates `sorted(set(base) | set(counts))` over dict keys — since `BaselineCounts` is a fixed-shape struct (not a dynamic dict), the `print_deltas` macro above enumerates the same fixed field set in the same alphabetical-ish order the struct declares them; this is equivalent because both sides always have exactly these 10 keys (no dynamic/missing keys can occur in the Rust version, unlike Python's `dict.get` which tolerated a schema drift).

- [ ] **Step 8: Port the 1-space-indent baseline JSON writer**

serde_json's `to_string_pretty` defaults to 2-space indent; the committed `jpeg-tag-baseline.json` uses 1-space (`json.dumps(counts, indent=1)`). Use a custom formatter to match:

```rust
fn write_baseline_one_space_indent(path: &Path, counts: &BaselineCounts) -> anyhow::Result<()> {
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    counts.serialize(&mut ser)?;
    buf.push(b'\n');
    std::fs::write(path, buf)?;
    Ok(())
}
```

(`use serde::Serialize;` needed for the `.serialize()` call.)

- [ ] **Step 9: Write an end-to-end test with a small canned results.json**

```rust
#[test]
fn report_end_to_end_produces_matching_baseline_shape() {
    let td = tempfile::tempdir().unwrap();
    std::env::set_var("TAGMATRIX_WORK", td.path());
    let mut results = HashMap::new();
    results.insert("ExifIFD:ISO".to_string(), ResultEntry {
        group: "ExifIFD".into(), name: "ISO".into(), sample: "200".into(),
        vtype: Some("int16u".into()), read: Some("OK".into()), write: Some("OK".into()),
        ..Default::default()
    });
    std::fs::write(td.path().join("results.json"), serde_json::to_string(&results).unwrap()).unwrap();

    let repo_docs = td.path().join("docs/reference");
    std::fs::create_dir_all(&repo_docs).unwrap();
    std::env::set_current_dir(td.path()).unwrap();

    run(ReportArgs { update_baseline: true, check_baseline: false }).unwrap();

    let baseline = std::fs::read_to_string(repo_docs.join("jpeg-tag-baseline.json")).unwrap();
    assert!(baseline.starts_with("{\n \"total_tested\": 1"));
}
```

- [ ] **Step 10: Run the full report.rs test suite**

Run: `cargo test --bin jpeg-tag-matrix --features jpeg-tag-matrix-binary report`
Expected: all tests PASS.

- [ ] **Step 11: Commit**

```bash
git add src/bin/jpeg-tag-matrix/report.rs
git commit -m "port report generation and baseline ratchet to Rust"
```

---

### Task 10: Full-pipeline dry run against a real ExifTool + built oxidex

**Files:**
- None created — this is a manual verification task, not a code task.

**Interfaces:**
- Consumes: the full `jpeg-tag-matrix` binary from Tasks 1-9.

- [ ] **Step 1: Build the release binary and oxidex itself**

Run: `cargo build --release --features jpeg-tag-matrix-binary --bin jpeg-tag-matrix && cargo build --release --bin oxidex`
Expected: both binaries build.

- [ ] **Step 2: Run the three subcommands against a locally installed ExifTool**

```bash
export TAGMATRIX_WORK=/tmp/oxidex-tagmap-rust-check
rm -rf "$TAGMATRIX_WORK"
./target/release/jpeg-tag-matrix manifest --flag-noops
./target/release/jpeg-tag-matrix run --workers 8
./target/release/jpeg-tag-matrix report --check-baseline
```

Expected: each command exits 0 and prints the same summary-table shape as its Python predecessor (family0/writable/readonly counts; READ/WRITE phase progress lines; per-group breakdown). `report --check-baseline` should print `Baseline check passed` if run against a fixture/ExifTool version matching the committed baseline, or print deltas otherwise (not a hard failure at this stage — the point here is confirming the pipeline runs end-to-end without crashing).

- [ ] **Step 3: Spot-check a handful of known-tricky tags in the generated docs**

```bash
grep -A2 "ApertureValue" docs/reference/jpeg-tag-matrix.md
grep -A2 "GPSSpeedRef" docs/reference/jpeg-tag-matrix.md
```

Expected: `ApertureValue` shows the `R-apex-missing` read bug classification; `GPSSpeedRef` shows the `I1-no-printconvinv` write bug classification — confirms the bug-classification tables from Task 5 are wired correctly end-to-end, not just passing in isolation.

- [ ] **Step 4: No commit** — this task only exercises the built binaries; if anything fails, return to the relevant task above and fix it there.

---

### Task 11: Parity verification against the Python pipeline

**Files:**
- Create (temporary, not committed): a small comparison script or manual `jq` invocation — whichever is more expedient; delete when done.

**Interfaces:**
- Consumes: both the Python scripts (still present) and the Rust binary (Tasks 1-10).

- [ ] **Step 1: Run the Python pipeline into one work dir, the Rust pipeline into another, same ExifTool/oxidex**

```bash
export EXIFTOOL_VERSION_CHECK=$(exiftool -ver)
echo "Testing against exiftool $EXIFTOOL_VERSION_CHECK"

TAGMATRIX_WORK=/tmp/oxidex-tagmap-py uv run scripts/generate_exiftool_manifest.py --flag-noops
TAGMATRIX_WORK=/tmp/oxidex-tagmap-py uv run scripts/jpeg_tag_matrix.py --workers 8
TAGMATRIX_WORK=/tmp/oxidex-tagmap-py uv run scripts/jpeg_tag_report.py --check-baseline

TAGMATRIX_WORK=/tmp/oxidex-tagmap-rs ./target/release/jpeg-tag-matrix manifest --flag-noops
TAGMATRIX_WORK=/tmp/oxidex-tagmap-rs ./target/release/jpeg-tag-matrix run --workers 8
TAGMATRIX_WORK=/tmp/oxidex-tagmap-rs ./target/release/jpeg-tag-matrix report --check-baseline
```

Note: both runs regenerate the same committed `docs/reference/jpeg-tag-*.md` files in place — run the Python pass first, `git stash` its output, run the Rust pass, then diff against the stash, so neither run's output silently overwrites the other before you can compare them.

- [ ] **Step 2: Structurally diff the intermediate JSON (order-independent, since these aren't committed)**

```bash
jq --sort-keys . /tmp/oxidex-tagmap-py/exiftool_jpeg_tags.json > /tmp/py_manifest.sorted.json
jq --sort-keys . /tmp/oxidex-tagmap-rs/exiftool_jpeg_tags.json > /tmp/rs_manifest.sorted.json
diff /tmp/py_manifest.sorted.json /tmp/rs_manifest.sorted.json

jq --sort-keys . /tmp/oxidex-tagmap-py/results.json > /tmp/py_results.sorted.json
jq --sort-keys . /tmp/oxidex-tagmap-rs/results.json > /tmp/rs_results.sorted.json
diff /tmp/py_results.sorted.json /tmp/rs_results.sorted.json
```

Expected: no diff, or only differences attributable to non-determinism already present in the Python version (e.g. HashMap iteration order affecting which duplicate entry wins a tie in `all_entries` — check `jpeg_tag_matrix.py:263-271`'s precedence-merge logic is a strict total order, which it is: `(writable, not protected)` — so this should NOT actually be a source of nondeterminism; if a diff shows up here, it's a real bug in the port, not expected variance).

- [ ] **Step 3: Byte-diff the committed markdown and baseline files**

```bash
git diff --stat docs/reference/jpeg-tag-support.md docs/reference/jpeg-tag-matrix.md docs/reference/jpeg-tag-baseline.json
```

Expected: either no diff (content identical) or a diff limited to the `Last updated: **YYYY-MM-DD**` line in the autogen callout (today's date — expected to differ run-to-run regardless of language) and the `generated_by`/exiftool-version string if the two runs used different installed ExifTool binaries. Any other diff (table row content, classification labels, baseline counts) is a real behavioral divergence — stop and fix the relevant task before proceeding.

- [ ] **Step 4: Document the outcome**

If parity is confirmed, note it in the PR description for this migration (not a new doc file — YAGNI). If there are intentional, documented differences (e.g. a bug fixed in Rust that the Python version doesn't have), call them out explicitly rather than letting them ride as "probably fine."

- [ ] **Step 5: No commit** — this is a verification gate; proceed to Task 12 only once it passes.

---

### Task 12: Update jpeg-tag-matrix.yml to run the Rust binary

**Files:**
- Modify: `.github/workflows/jpeg-tag-matrix.yml`

**Interfaces:**
- Consumes: the `jpeg-tag-matrix` binary (Tasks 1-11, parity-verified).

- [ ] **Step 1: Remove the Python/uv setup steps**

Delete these two steps from `.github/workflows/jpeg-tag-matrix.yml` (currently lines 64-70):
```yaml
      - name: Setup Python
        uses: actions/setup-python@v5
        with:
          python-version: '3.12'

      - name: Install uv
        uses: astral-sh/setup-uv@e4db8464a088ece1b920f60402e813ea4de65b8f # v4
```

- [ ] **Step 2: Replace the build step and the four `uv run` invocations**

Replace:
```yaml
      - name: Build oxidex (release)
        run: cargo build --release --bin oxidex

      - name: Generate ExifTool tag manifest
        run: uv run scripts/generate_exiftool_manifest.py --flag-noops

      - name: Run tag matrix (read + write round-trips)
        run: uv run scripts/jpeg_tag_matrix.py --workers 8

      - name: Generate reports and check baseline
        run: uv run scripts/jpeg_tag_report.py --check-baseline

      - name: Ratchet baseline on improvement
        run: uv run scripts/jpeg_tag_report.py --update-baseline
```
with:
```yaml
      - name: Build oxidex and jpeg-tag-matrix (release)
        run: |
          cargo build --release --bin oxidex
          cargo build --release --features jpeg-tag-matrix-binary --bin jpeg-tag-matrix

      - name: Generate ExifTool tag manifest
        run: ./target/release/jpeg-tag-matrix manifest --flag-noops

      - name: Run tag matrix (read + write round-trips)
        run: ./target/release/jpeg-tag-matrix run --workers 8

      - name: Generate reports and check baseline
        run: ./target/release/jpeg-tag-matrix report --check-baseline

      - name: Ratchet baseline on improvement
        run: ./target/release/jpeg-tag-matrix report --update-baseline
```

- [ ] **Step 3: Update the commit message body's script reference**

The existing "Commit and push docs updates" step's commit message says nothing script-specific (only the workflow name), so no change needed there. But re-check the `autogen_callout()` text ported in Task 9 Step 6 — update its `scripts/jpeg_tag_matrix.py` reference now:

```rust
// in report.rs, autogen_callout()
"(`scripts/jpeg_tag_matrix.py`) against **{ver}**. \
```
becomes
```rust
"(`jpeg-tag-matrix` binary) against **{ver}**. \
```

- [ ] **Step 4: Leave everything else in the workflow untouched**

Do not touch: the pinned ExifTool git-clone step, the `concurrency`/`permissions` blocks, the `rust-cache` step, the weekly cron trigger, or the artifact-upload step's paths (still point at `${{ env.TAGMATRIX_WORK }}/results.json` etc. — unchanged since the Rust binary writes to the same `TAGMATRIX_WORK` location).

- [ ] **Step 5: Validate the workflow YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/jpeg-tag-matrix.yml'))"` (or any YAML linter available) to catch indentation mistakes before pushing — a bad workflow file fails silently with "zero jobs scheduled" rather than a clear parse error, per the existing comment already in this file about `runner.temp`.

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/jpeg-tag-matrix.yml src/bin/jpeg-tag-matrix/report.rs
git commit -m "run jpeg-tag-matrix pipeline via Rust binary instead of Python/uv in CI"
```

- [ ] **Step 7: Push the branch and let the workflow run once for real confirmation**

This step needs your explicit go-ahead before pushing, since it pushes to a remote and triggers a real CI run — don't push automatically as part of executing this plan.

---

### Task 13: Remove the Python scripts

**Files:**
- Delete: `scripts/generate_exiftool_manifest.py`, `scripts/jpeg_tag_matrix.py`, `scripts/jpeg_tag_report.py`

**Interfaces:**
- Consumes: a green CI run from Task 12 (confirms the Rust binary works in the actual GitHub Actions environment, not just locally).

- [ ] **Step 1: Confirm no other script or doc references the three files**

Run: `grep -rn "generate_exiftool_manifest\|jpeg_tag_matrix\.py\|jpeg_tag_report\.py" --include="*.py" --include="*.md" --include="*.yml" --include="*.toml" .`
Expected: no remaining references outside the files themselves (the `justfile` doesn't reference these three — only `docs-coverage`/`compare-exiftool*` recipes, which call `generate_tag_coverage.py`, out of scope for this migration).

- [ ] **Step 2: Delete the three scripts**

```bash
git rm scripts/generate_exiftool_manifest.py scripts/jpeg_tag_matrix.py scripts/jpeg_tag_report.py
```

- [ ] **Step 3: Confirm CI is still green after removal**

Run: `just check` (or the equivalent local lint+test invocation) to confirm nothing in the Rust build depends on the Python files still existing (it shouldn't — they were never a build input, only a CI runtime step — but this is cheap to confirm).

- [ ] **Step 4: Commit**

```bash
git commit -m "remove Python jpeg-tag-matrix pipeline scripts, superseded by Rust binary"
```

---

## Self-Review Notes

- **Spec coverage:** all three in-scope scripts are covered — `generate_exiftool_manifest.py` (Tasks 2-4), `jpeg_tag_matrix.py` (Tasks 5-8), `jpeg_tag_report.py` (Task 9). `generate_tag_coverage.py` and sourcing-policy unification are both explicitly out of scope per the deferred decisions and are not touched anywhere in this plan.
- **Fidelity-risk items called out during investigation are each given their own explicit step:** the ratchet's "overwrite on any non-regression" semantics (Task 9 Step 7, `--update-baseline` has no strict-improvement gate), and `--flag-noops`'s real write-testing (Task 4, tested against a fake-exiftool fixture that distinguishes a real write from a silent no-op).
- **Type consistency check:** `ResultEntry` (Task 1) is used identically across `matrix.rs` (Tasks 5-8, populated) and `report.rs` (Task 9, consumed) — field names (`read`, `write`, `bug_cluster`, `write_quality`, etc.) match in both directions.
- **No placeholder scan:** all code steps contain complete, real translations of specific line ranges in the Python source; the one deliberately-deferred item (KNOWN_BUGS's #18/#22 "fixed by" framing potentially going stale) is flagged as a live check to do at port time, not left as a TODO in code.
