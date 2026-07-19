# ExifTool Tag Sync Tool Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the broken Perl-regex tag generator in `build.rs` with a standalone `sync-tags` binary that regenerates `oxidex-tags-*/src/*_tags.yaml` from `exiftool -f -listx`'s authoritative, fully-resolved tag dump — capturing real per-tag type data (currently populated for only 1.1% of tags) instead of throwing it away.

**Architecture:** A new library module `src/tag_sync/mod.rs` holds pure, unit-testable logic (XML parsing, domain routing, YAML generation, count sanity-checking). A thin binary `src/bin/sync_tags.rs` wires that logic to the real `exiftool` process and the filesystem. `build.rs`'s entire download/parse/generate apparatus (~700 lines) is deleted, since it is provably dead code today (it only runs if `src/tag_db/generated_tags.rs` is deleted, which last happened once, silently failed, and was never merged) and has no other build responsibilities.

**Tech Stack:** Rust, `quick-xml` (already a workspace dependency, used elsewhere for XMP parsing — same idiom reused here), `anyhow` (already a workspace dependency).

## Global Constraints

- The sync tool must never run as a side effect of `cargo build` — it is invoked explicitly only (`cargo run --release --bin sync-tags`).
- No network access from the tool itself — it shells out to a locally-installed `exiftool` binary; installing/upgrading that binary is a separate, external step.
- On any failure (missing `exiftool`, unparseable output, tag count regression), the tool must exit non-zero with a clear message — no silent fallback that reports success with no real change.
- The YAML `type:` field stores ExifTool's raw type string verbatim (e.g. `int16u`, `rational64s`, `string`, `?`) — no coercion to oxidex's `ValueType` enum at generation time (that belongs to a later phase).
- `.exiftool-version` stores a plain release version string (e.g. `13.55`), read via `exiftool -ver` — not a git commit SHA.

---

## Task 1: XML parsing — `TagRecord` and `parse_listx`

**Files:**
- Create: `src/tag_sync/mod.rs`
- Modify: `src/lib.rs` (add `pub mod tag_sync;`)

**Interfaces:**
- Produces: `pub struct TagRecord { pub table: String, pub id: String, pub name: String, pub writable: bool, pub type_name: Option<String>, pub description: Option<String> }` (all fields `pub`, struct derives `Debug, Clone, PartialEq`)
- Produces: `pub fn parse_listx(xml: &str) -> anyhow::Result<Vec<TagRecord>>`

- [ ] **Step 1: Write the failing test**

Create `src/tag_sync/mod.rs` with just the test module first:

```rust
//! ExifTool tag database sync: parses `exiftool -f -listx` XML output into
//! `TagRecord`s and regenerates the `oxidex-tags-*` YAML tag databases.

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_LISTX: &str = r#"<?xml version='1.0' encoding='UTF-8'?>
<taginfo>
<table name='Exif::Main' g0='EXIF' g1='IFD0' g2='Image'>
 <desc lang='en'>Exif</desc>
 <tag id='271' name='Make' type='string' writable='true' g1='IFD0'>
  <desc lang='en'>Manufacturer</desc>
  <desc lang='de'>Hersteller</desc>
 </tag>
 <tag id='37500' name='MakerNotes' type='undef' writable='false' g1='ExifIFD'>
  <desc lang='en'>Maker Notes</desc>
 </tag>
</table>
<table name='Composite' g0='Composite' g1='Composite' g2='Other'>
 <tag id='Exif-ThumbnailImage' name='ThumbnailImage' type='?' writable='true' g0='EXIF' g1='All' g2='Preview'>
  <desc lang='en'>Thumbnail Image</desc>
 </tag>
</table>
</taginfo>
"#;

    #[test]
    fn parses_hash_form_tags_with_type_and_description() {
        let tags = parse_listx(SAMPLE_LISTX).expect("valid listx XML must parse");
        assert_eq!(tags.len(), 3);

        let make = tags
            .iter()
            .find(|t| t.name == "Make")
            .expect("Make tag must be present");
        assert_eq!(make.table, "Exif::Main");
        assert_eq!(make.id, "271");
        assert!(make.writable);
        assert_eq!(make.type_name.as_deref(), Some("string"));
        assert_eq!(make.description.as_deref(), Some("Manufacturer"));
    }

    #[test]
    fn parses_non_writable_tags_and_non_numeric_ids() {
        let tags = parse_listx(SAMPLE_LISTX).expect("valid listx XML must parse");

        let maker_notes = tags
            .iter()
            .find(|t| t.name == "MakerNotes")
            .expect("MakerNotes tag must be present");
        assert!(!maker_notes.writable);

        let thumb = tags
            .iter()
            .find(|t| t.name == "ThumbnailImage")
            .expect("ThumbnailImage tag must be present");
        assert_eq!(thumb.id, "Exif-ThumbnailImage");
        assert_eq!(thumb.table, "Composite");
        assert_eq!(thumb.type_name.as_deref(), Some("?"));
    }

    #[test]
    fn rejects_malformed_xml() {
        let result = parse_listx("<taginfo><table name='X'><tag id='1'");
        assert!(result.is_err(), "truncated XML must return an error, not panic");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib tag_sync:: 2>&1 | head -30`
Expected: compile error — `parse_listx` and `TagRecord` not found.

- [ ] **Step 3: Write minimal implementation**

Add above the `#[cfg(test)]` block in `src/tag_sync/mod.rs`:

```rust
use anyhow::{Context, Result};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

/// A single ExifTool tag as reported by `exiftool -f -listx`.
#[derive(Debug, Clone, PartialEq)]
pub struct TagRecord {
    pub table: String,
    pub id: String,
    pub name: String,
    pub writable: bool,
    pub type_name: Option<String>,
    pub description: Option<String>,
}

fn attr_value(e: &BytesStart, key: &str) -> Option<String> {
    e.attributes().flatten().find_map(|attr| {
        if attr.key.as_ref() == key.as_bytes() {
            std::str::from_utf8(&attr.value).ok().map(|s| s.to_string())
        } else {
            None
        }
    })
}

/// Parses `exiftool -f -listx` XML output into a flat list of `TagRecord`s.
///
/// ExifTool has already resolved table-level `WRITABLE` inheritance by the
/// time it emits this XML, so no inheritance logic is needed here — every
/// `<tag>` element carries its own fully-resolved `writable` attribute.
pub fn parse_listx(xml: &str) -> Result<Vec<TagRecord>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut tags = Vec::new();
    let mut current_table = String::new();
    let mut in_tag = false;
    let mut capturing_en_desc = false;
    let mut buf = Vec::new();

    loop {
        match reader
            .read_event_into(&mut buf)
            .context("failed to read XML event from exiftool -listx output")?
        {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == b"table" => {
                current_table = attr_value(&e, "name").unwrap_or_default();
            }
            Event::Start(e) if e.name().as_ref() == b"tag" => {
                let id = attr_value(&e, "id").unwrap_or_default();
                let name = attr_value(&e, "name").unwrap_or_default();
                let writable = matches!(attr_value(&e, "writable").as_deref(), Some("true"));
                let type_name = attr_value(&e, "type");

                tags.push(TagRecord {
                    table: current_table.clone(),
                    id,
                    name,
                    writable,
                    type_name,
                    description: None,
                });
                in_tag = true;
            }
            Event::Empty(e) if e.name().as_ref() == b"tag" => {
                let id = attr_value(&e, "id").unwrap_or_default();
                let name = attr_value(&e, "name").unwrap_or_default();
                let writable = matches!(attr_value(&e, "writable").as_deref(), Some("true"));
                let type_name = attr_value(&e, "type");

                tags.push(TagRecord {
                    table: current_table.clone(),
                    id,
                    name,
                    writable,
                    type_name,
                    description: None,
                });
            }
            Event::Start(e) if in_tag && e.name().as_ref() == b"desc" => {
                capturing_en_desc = attr_value(&e, "lang").as_deref() == Some("en");
            }
            Event::Text(t) if capturing_en_desc => {
                if let Some(last) = tags.last_mut() {
                    // `decode()` only handles byte-encoding, not XML entities
                    // (e.g. `&#39;`, `&amp;`) — `escape::unescape` does that,
                    // matching the pattern already used in
                    // src/parsers/xmp/rdf_parser.rs.
                    let decoded = t
                        .decode()
                        .context("invalid text content in <desc> element")?;
                    let text = quick_xml::escape::unescape(&decoded)
                        .unwrap_or_else(|_| decoded.clone())
                        .into_owned();
                    last.description = Some(text);
                }
                capturing_en_desc = false;
            }
            Event::End(e) if e.name().as_ref() == b"tag" => {
                in_tag = false;
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(tags)
}
```

Add to `src/lib.rs` (alongside the other `pub mod` declarations):

```rust
pub mod tag_sync;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib tag_sync:: 2>&1 | tail -20`
Expected: `test result: ok. 3 passed`

- [ ] **Step 5: Commit**

```bash
git add src/tag_sync/mod.rs src/lib.rs
git commit -m "feat: add exiftool -listx XML parser for tag sync"
```

---

## Task 2: Domain routing — `get_domain_for_table`

**Files:**
- Modify: `src/tag_sync/mod.rs`

**Interfaces:**
- Consumes: nothing new (pure string function)
- Produces: `pub fn get_domain_for_table(table_name: &str) -> &'static str`

Real `-listx` table names are mixed-case (`Exif::Main`, not `EXIF::Main`; `Jpeg2000`, not `JPEG2000`), unlike `build.rs`'s old Perl-derived names. Matching case-insensitively (uppercasing both the input and every match key) avoids silently mis-routing tags due to casing instead of requiring an exhaustive case-by-case audit of ~140 names.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/tag_sync/mod.rs`:

```rust
    #[test]
    fn routes_core_standards_case_insensitively() {
        assert_eq!(get_domain_for_table("Exif::Main"), "core");
        assert_eq!(get_domain_for_table("GPS::Main"), "core");
        assert_eq!(get_domain_for_table("Composite"), "core");
        assert_eq!(get_domain_for_table("ICC_Profile::Main"), "core");
    }

    #[test]
    fn routes_camera_makernotes() {
        assert_eq!(get_domain_for_table("Canon::AFConfig"), "camera");
        assert_eq!(get_domain_for_table("Nikon::Main"), "camera");
    }

    #[test]
    fn routes_media_and_image_and_document_and_specialty() {
        assert_eq!(get_domain_for_table("QuickTime::Main"), "media");
        assert_eq!(get_domain_for_table("Jpeg2000::Main"), "image");
        assert_eq!(get_domain_for_table("PDF::Main"), "document");
        assert_eq!(get_domain_for_table("DICOM::Main"), "specialty");
    }

    #[test]
    fn routes_unknown_tables_to_core_by_default() {
        assert_eq!(get_domain_for_table("SomeBrandNewVendor::Main"), "core");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib tag_sync:: 2>&1 | head -30`
Expected: compile error — `get_domain_for_table` not found.

- [ ] **Step 3: Write minimal implementation**

Add to `src/tag_sync/mod.rs` (above the `tests` module):

```rust
/// Routes an ExifTool table name (e.g. `Canon::AFConfig`, `Exif::Main`) to
/// the `oxidex-tags-*` domain crate that should own it. Matching is
/// case-insensitive: `-listx` table names use ExifTool's own mixed casing
/// (`Exif`, `Jpeg2000`), which does not consistently match any single case
/// convention.
pub fn get_domain_for_table(table_name: &str) -> &'static str {
    let base_name = table_name.split("::").next().unwrap_or(table_name);
    match base_name.to_ascii_uppercase().as_str() {
        "EXIF" | "XMP" | "IPTC" | "GPS" | "ICC_PROFILE" | "MWG" | "PHOTOSHOP" | "FLASHPIX"
        | "GEOTIFF" | "COMPOSITE" | "TRAILER" | "MAKERNOTES" => "core",

        "CANON" | "CANONCUSTOM" | "CANONRAW" | "NIKON" | "NIKONCAPTURE" | "NIKONCUSTOM"
        | "NIKONSETTINGS" | "SONY" | "SONYIDC" | "PANASONIC" | "PANASONICRAW" | "OLYMPUS"
        | "FUJIFILM" | "PENTAX" | "CASIO" | "MINOLTA" | "MINOLTARAW" | "RICOH" | "SIGMA"
        | "SIGMARAW" | "PHASEONE" | "KODAK" | "KYOCERARAW" | "SAMSUNG" | "SANYO" | "HP" | "GE"
        | "RECONYX" | "JVC" | "MOTOROLA" | "APPLE" | "DJI" | "GOPRO" | "PARROT" | "INFIRAY"
        | "FLIR" => "camera",

        "QUICKTIME" | "MATROSKA" | "MPEG" | "M2TS" | "MXF" | "FLAC" | "AAC" | "AIFF" | "VORBIS"
        | "OPUS" | "ID3" | "APE" | "ASF" | "FLASH" | "REAL" | "THEORA" | "H264" | "WAVPACK"
        | "MPC" | "DSF" | "WTV" => "media",

        "PNG" | "GIF" | "JPEG" | "JPEG2000" | "BMP" | "TIFF" | "DNG" | "MNG" | "PGF" | "PICT"
        | "OPENEXR" | "FLIF" | "BPG" | "WEBP" | "DPX" | "PSP" | "PCX" | "MIFF" | "PHOTOCD"
        | "ICO" | "PALM" => "image",

        "PDF" | "POSTSCRIPT" | "FONT" | "PLIST" | "HTML" | "TORRENT" | "ZIP" | "TNEF" | "VCARD"
        | "MICROSOFT" | "MACOS" | "EXE" | "LNK" | "RSRC" | "FOTOSTATION" | "PHOTOMECHANIC"
        | "ITC" | "GIMP" | "GM" | "GOOGLE" => "document",

        "DICOM" | "FITS" | "MRC" | "STIM" | "PCAP" | "XISF" | "MISB" | "DJVU" | "ISO"
        | "NINTENDO" => "specialty",

        _ => "core",
    }
}

/// The six `oxidex-tags-*` domain crates, in the order YAML files are
/// written.
pub const DOMAINS: [&str; 6] = ["core", "camera", "media", "image", "document", "specialty"];
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib tag_sync:: 2>&1 | tail -20`
Expected: `test result: ok. 7 passed`

- [ ] **Step 5: Commit**

```bash
git add src/tag_sync/mod.rs
git commit -m "feat: add case-insensitive domain routing for tag sync"
```

---

## Task 3: YAML generation — `generate_domain_yaml`

**Files:**
- Modify: `src/tag_sync/mod.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: `TagRecord` (Task 1), `get_domain_for_table`, `DOMAINS` (Task 2)
- Produces: `pub fn generate_domain_yaml(domain: &str, tags: &[TagRecord]) -> String`

Matches the existing schema `oxidex-tags-shared::types::TagDatabase` deserializes today (`tables: [{name, tags: [{id, name, writable, type, description}]}]`), so no downstream consumer changes are needed.

This task's tests parse the generated YAML back with `serde_yaml` to catch real invalid-YAML output (not just string matching) — the `oxidex-tags-core` crate already depends on `serde_yaml = "0.9"` for its own build step, so this adds the same version as a root-crate dev-dependency, test-only.

- [ ] **Step 1: Add `serde_yaml` as a dev-dependency**

In `Cargo.toml`, find the `[dev-dependencies]` section and add:

```toml
serde_yaml = "0.9"
```

Run: `cargo build --tests 2>&1 | tail -10`
Expected: builds successfully with the new dev-dependency resolved (no test code uses it yet).

Commit this alone before moving on:

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add serde_yaml dev-dependency for tag sync YAML validation tests"
```

- [ ] **Step 2: Write the failing test**

Add to the `tests` module:

```rust
    #[test]
    fn generates_expected_yaml_shape_for_a_domain() {
        let tags = vec![
            TagRecord {
                table: "Exif::Main".to_string(),
                id: "271".to_string(),
                name: "Make".to_string(),
                writable: true,
                type_name: Some("string".to_string()),
                description: Some("Manufacturer".to_string()),
            },
            TagRecord {
                table: "Exif::Main".to_string(),
                id: "37500".to_string(),
                name: "MakerNotes".to_string(),
                writable: false,
                type_name: Some("undef".to_string()),
                description: None,
            },
            TagRecord {
                table: "Canon::Main".to_string(),
                id: "1".to_string(),
                name: "CanonImageType".to_string(),
                writable: true,
                type_name: None,
                description: Some("with \"quotes\" and \\backslash".to_string()),
            },
        ];

        let core_yaml = generate_domain_yaml("core", &tags);
        assert!(core_yaml.contains("  - name: Exif::Main\n"));
        assert!(core_yaml.contains("      - id: \"271\"\n"));
        assert!(core_yaml.contains("        name: \"Make\"\n"));
        assert!(core_yaml.contains("        writable: true\n"));
        assert!(core_yaml.contains("        type: \"string\"\n"));
        assert!(core_yaml.contains("        description: \"Manufacturer\"\n"));
        // MakerNotes has no description: field must be omitted, not empty-stringed.
        assert!(!core_yaml.contains("37500\"\n        name: \"MakerNotes\"\n        writable: false\n        type: \"undef\"\n        description"));
        // Canon tag must not appear in the "core" domain output.
        assert!(!core_yaml.contains("CanonImageType"));

        let camera_yaml = generate_domain_yaml("camera", &tags);
        assert!(camera_yaml.contains("CanonImageType"));
        // Escaping: embedded quotes and backslashes must not break the YAML string.
        assert!(camera_yaml.contains("description: \"with \\\"quotes\\\" and \\\\backslash\"\n"));
        // No `type:` field written for tags without a type.
        assert!(!camera_yaml.contains("CanonImageType\"\n        writable: true\n        type:"));
    }

    #[test]
    fn empty_domain_produces_minimal_valid_yaml() {
        let yaml = generate_domain_yaml("specialty", &[]);
        assert_eq!(yaml, "tables:\n");
    }

    #[test]
    fn question_mark_type_is_quoted_to_stay_valid_yaml() {
        // ExifTool reports type '?' for composite/calculated tags (e.g. real
        // exiftool 13.55: AFCP::Main's PreviewImage). An unquoted `?` is
        // YAML's explicit complex-mapping-key indicator — left unquoted,
        // `serde_yaml`/any YAML parser fails with "mapping keys are not
        // allowed in this context". Verified against a real exiftool dump
        // during planning; this test guards against regressing the fix.
        let tags = vec![TagRecord {
            table: "Composite".to_string(),
            id: "Exif-PreviewImage".to_string(),
            name: "PreviewImage".to_string(),
            writable: true,
            type_name: Some("?".to_string()),
            description: None,
        }];

        let yaml = generate_domain_yaml("core", &tags);
        assert!(yaml.contains("        type: \"?\"\n"));

        let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(&yaml);
        assert!(
            parsed.is_ok(),
            "generated YAML with a '?' type must remain parseable: {:?}",
            parsed.err()
        );
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib tag_sync:: 2>&1 | head -30`
Expected: compile error — `generate_domain_yaml` not found.

- [ ] **Step 4: Write minimal implementation**

Add to `src/tag_sync/mod.rs`:

```rust
use std::collections::HashMap;

fn escape_yaml_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Renders the YAML tag database for one `oxidex-tags-*` domain crate from
/// the full set of parsed tags, filtering to tags whose table routes to
/// `domain`. Table and tag ordering is sorted for deterministic,
/// diffable output.
pub fn generate_domain_yaml(domain: &str, tags: &[TagRecord]) -> String {
    let mut by_table: HashMap<&str, Vec<&TagRecord>> = HashMap::new();
    for tag in tags {
        if get_domain_for_table(&tag.table) == domain {
            by_table.entry(tag.table.as_str()).or_default().push(tag);
        }
    }

    let mut yaml = String::from("tables:\n");
    if by_table.is_empty() {
        return yaml;
    }

    let mut table_names: Vec<&str> = by_table.keys().copied().collect();
    table_names.sort_unstable();

    for table_name in table_names {
        let mut table_tags = by_table[table_name].clone();
        table_tags.sort_by(|a, b| a.name.cmp(&b.name));

        yaml.push_str(&format!("  - name: {}\n", table_name));
        yaml.push_str("    tags:\n");

        for tag in table_tags {
            yaml.push_str(&format!("      - id: \"{}\"\n", escape_yaml_string(&tag.id)));
            yaml.push_str(&format!(
                "        name: \"{}\"\n",
                escape_yaml_string(&tag.name)
            ));
            yaml.push_str(&format!("        writable: {}\n", tag.writable));

            if let Some(ref type_name) = tag.type_name {
                // Must be quoted: ExifTool's own "unknown/composite" type
                // string is a bare `?`, which YAML treats as the explicit
                // complex-mapping-key indicator when unquoted, breaking the
                // parser (verified against a real exiftool 13.55 -f -listx
                // dump during planning — AFCP::Main's PreviewImage tag has
                // exactly this type value).
                yaml.push_str(&format!(
                    "        type: \"{}\"\n",
                    escape_yaml_string(type_name)
                ));
            }

            if let Some(ref description) = tag.description {
                if !description.is_empty() {
                    yaml.push_str(&format!(
                        "        description: \"{}\"\n",
                        escape_yaml_string(description)
                    ));
                }
            }
        }
    }

    yaml
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib tag_sync:: 2>&1 | tail -20`
Expected: `test result: ok. 10 passed`

- [ ] **Step 6: Commit**

```bash
git add src/tag_sync/mod.rs
git commit -m "feat: add YAML generation for tag sync domains"
```

---

## Task 4: Regression guard — `count_ids_in_yaml`

**Files:**
- Modify: `src/tag_sync/mod.rs`

**Interfaces:**
- Produces: `pub fn count_ids_in_yaml(yaml_content: &str) -> usize`

Counts tag entries the same way `sync-exiftool-tags.yml`'s existing bash step already does (`grep -hE '^[[:space:]]+- id:'`), so the sync binary can compare a freshly generated domain's tag count against what's already committed before overwriting it.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:

```rust
    #[test]
    fn counts_id_lines_regardless_of_indentation() {
        let yaml = "tables:\n  - name: Exif::Main\n    tags:\n      - id: \"271\"\n        name: \"Make\"\n      - id: \"272\"\n        name: \"Model\"\n";
        assert_eq!(count_ids_in_yaml(yaml), 2);
    }

    #[test]
    fn counts_zero_for_empty_yaml() {
        assert_eq!(count_ids_in_yaml("tables:\n"), 0);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib tag_sync:: 2>&1 | head -30`
Expected: compile error — `count_ids_in_yaml` not found.

- [ ] **Step 3: Write minimal implementation**

Add to `src/tag_sync/mod.rs`:

```rust
/// Counts tag entries in a domain YAML file by counting `- id:` lines —
/// matches the counting method `sync-exiftool-tags.yml` already uses via
/// `grep -hE '^[[:space:]]+- id:'`, so sanity checks agree with CI reporting.
pub fn count_ids_in_yaml(yaml_content: &str) -> usize {
    yaml_content
        .lines()
        .filter(|line| line.trim_start().starts_with("- id:"))
        .count()
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib tag_sync:: 2>&1 | tail -20`
Expected: `test result: ok. 12 passed`

- [ ] **Step 5: Commit**

```bash
git add src/tag_sync/mod.rs
git commit -m "feat: add tag-count sanity check helper for tag sync"
```

---

## Task 5: `sync-tags` binary

**Files:**
- Create: `src/bin/sync_tags.rs`

**Interfaces:**
- Consumes: `oxidex::tag_sync::{parse_listx, generate_domain_yaml, count_ids_in_yaml, DOMAINS}`

This task has no unit tests of its own — it is thin glue code (process invocation and file I/O) around the already-tested logic in `src/tag_sync/mod.rs`. It is exercised end-to-end by the smoke test in Task 8. Cargo auto-discovers any `.rs` file directly under `src/bin/` as a binary target (matching the existing `src/bin/generate_baseline.rs`, which needs no `Cargo.toml` entry), so no `Cargo.toml` change is needed.

- [ ] **Step 1: Write the binary**

Create `src/bin/sync_tags.rs`:

```rust
//! Regenerates `oxidex-tags-*/src/*_tags.yaml` from a locally-installed
//! `exiftool` binary's own `-f -listx` tag dump.
//!
//! Usage: `cargo run --release --bin sync-tags`
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

        fs::write(path, &new_yaml).with_context(|| format!("failed to write {path_str}"))?;
        println!("  {domain:12} -> {path_str} ({previous_count} -> {new_count} tags)");
    }

    fs::write(".exiftool-version", format!("{version}\n"))
        .context("failed to write .exiftool-version")?;
    println!("Recorded exiftool version {version} in .exiftool-version");

    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build --bin sync_tags 2>&1 | tail -30`
Expected: builds with no errors (warnings about unused items in `tag_sync` are fine at this point if any remain — none are expected since all four functions are now used).

- [ ] **Step 3: Commit**

```bash
git add src/bin/sync_tags.rs
git commit -m "feat: add sync-tags binary to regenerate tag YAML from exiftool"
```

---

## Task 6: Delete the dead Perl-parsing pipeline in `build.rs`

**Files:**
- Delete: `build.rs`
- Modify: `src/tag_db/tag_registry.rs:1-6`

`build.rs` today has exactly one job — a download/parse/generate pipeline that is gated behind "skip entirely if `src/tag_db/generated_tags.rs` already exists." That file is checked into git and its content (a static two-function compatibility facade delegating to `tag_registry::get_tag_descriptor`/`tag_count`) no longer depends on any parsed tag data — so `build.rs` has no remaining reason to exist. There is no `build = "build.rs"` line in `Cargo.toml`; Cargo auto-detects and removes the build-script step the moment the file is gone.

- [ ] **Step 1: Confirm `build.rs` has no other responsibilities**

Run: `grep -n 'cargo:rustc\|cargo:rerun\|bindgen\|VERGEN\|env!' build.rs`
Expected: only the two `cargo:rerun-if-changed` lines and `cargo:warning` lines already read during design — no linker flags, no codegen for anything besides the tag database. If this turns up anything unexpected, stop and re-scope this task before deleting.

- [ ] **Step 2: Delete the file**

```bash
git rm build.rs
```

- [ ] **Step 3: Update the stale migration note in `tag_registry.rs`**

Read `src/tag_db/tag_registry.rs:1-6`:

```rust
//! Tag Registry - 500+ Metadata Tags
//!
//! This module provides a static registry of 500+ metadata tags covering EXIF (300+),
//! GPS (30+), XMP (100+), IPTC (50+), PDF (10+), and QuickTime (10+) formats.
//! This is a manual implementation that will later be replaced by automated tag
//! generation in build.rs (task I5.T5).
```

Replace with:

```rust
//! Tag Registry - 500+ Metadata Tags
//!
//! This module provides a static registry of 500+ metadata tags covering EXIF (300+),
//! GPS (30+), XMP (100+), IPTC (50+), PDF (10+), and QuickTime (10+) formats.
//! This is a manual implementation. Automated tag generation now lives in
//! `src/tag_sync/` and `src/bin/sync_tags.rs` (run explicitly via
//! `cargo run --bin sync-tags`, not as part of the build).
```

- [ ] **Step 4: Verify the workspace still builds without `build.rs`**

Run: `cargo build --release 2>&1 | tail -30`
Expected: builds successfully. `src/tag_db/generated_tags.rs` is unchanged (still checked into git) and still compiles as a plain source file.

- [ ] **Step 5: Commit**

```bash
git add src/tag_db/tag_registry.rs
git commit -m "chore: delete dead Perl-parsing pipeline in build.rs

Tag generation moved to src/tag_sync/ + src/bin/sync_tags.rs, run
explicitly rather than as a build.rs side effect. build.rs had no
other responsibility and was already dead code in practice: it only
ran when src/tag_db/generated_tags.rs was deleted, which happened
once, produced a silent no-op sync, and was never merged."
```

---

## Task 7: Update `tests/tag_sync_automation.rs` regression assertions

**Files:**
- Modify: `tests/tag_sync_automation.rs`

This file guards against regressing to old broken paths by asserting specific strings exist in `build.rs`. Since `build.rs` is deleted (Task 6), these assertions must move to the new source of truth so the regression guard keeps working instead of failing to compile.

- [ ] **Step 1: Read the current file**

The current content (captured during planning):

```rust
//! Regression tests for tag sync automation wiring.

use std::fs;
use std::path::Path;

fn repo_file(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
}

#[test]
fn generated_tags_stub_delegates_count_to_active_registry() {
    let build_rs = repo_file("build.rs");

    assert!(
        build_rs.contains(r#"writeln!(file, "    crate::tag_db::tag_registry::tag_count()")"#),
        "build.rs should generate a compatibility facade that delegates counts to the active registry"
    );
    assert!(
        !build_rs.contains(r#"writeln!(file, "    {}", tags.len())"#),
        "build.rs must not regenerate generated_tag_count() with a stale parsed constant"
    );
}

#[test]
fn tag_sync_targets_active_domain_crates_and_counts_yaml_sources() {
    let build_rs = repo_file("build.rs");
    let workflow = repo_file(".github/workflows/sync-exiftool-tags.yml");

    assert!(
        build_rs.contains(r#"format!("oxidex-tags-{}/src/{}_tags.yaml", domain, domain)"#),
        "build.rs should regenerate YAML in the active oxidex-tags-* domain crates"
    );
    assert!(
        !build_rs.contains("exiftool-tags-{}/src/{}_tags.yaml"),
        "build.rs should not target obsolete exiftool-tags-* crate paths"
    );
    assert!(
        workflow.contains("oxidex-tags-*/src/*_tags.yaml"),
        "sync workflow should count tags from active YAML domain crates"
    );
    assert!(
        !workflow.contains("grep -A 1 \"pub fn generated_tag_count\" src/tag_db/generated_tags.rs"),
        "sync workflow must not scrape generated_tags.rs as the tag-count source"
    );
}
```

- [ ] **Step 2: Replace it to assert against the new source of truth**

Overwrite `tests/tag_sync_automation.rs`:

```rust
//! Regression tests for tag sync automation wiring.

use std::fs;
use std::path::Path;

fn repo_file(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
}

#[test]
fn generated_tags_stub_still_delegates_to_active_registry() {
    let generated = repo_file("src/tag_db/generated_tags.rs");

    assert!(
        generated.contains("crate::tag_db::tag_registry::get_tag_descriptor(name)"),
        "generated_tags.rs facade should delegate lookups to the active registry"
    );
    assert!(
        generated.contains("crate::tag_db::tag_registry::tag_count()"),
        "generated_tags.rs facade should delegate counts to the active registry"
    );
}

#[test]
fn build_rs_no_longer_exists() {
    let build_rs = Path::new(env!("CARGO_MANIFEST_DIR")).join("build.rs");
    assert!(
        !build_rs.exists(),
        "build.rs should stay deleted — tag generation lives in src/tag_sync/ + \
         src/bin/sync_tags.rs, run explicitly rather than as a build.rs side effect"
    );
}

#[test]
fn sync_tags_binary_targets_active_domain_crates() {
    let sync_tags = repo_file("src/bin/sync_tags.rs");

    assert!(
        sync_tags.contains(r#"format!("oxidex-tags-{domain}/src/{domain}_tags.yaml")"#),
        "sync_tags.rs should regenerate YAML in the active oxidex-tags-* domain crates"
    );
    assert!(
        !sync_tags.contains("exiftool-tags-{}/src/{}_tags.yaml"),
        "sync_tags.rs should not target obsolete exiftool-tags-* crate paths"
    );
}
```

- [ ] **Step 3: Run the test**

Run: `cargo test --test tag_sync_automation 2>&1 | tail -20`
Expected: `test result: ok. 3 passed`

- [ ] **Step 4: Commit**

```bash
git add tests/tag_sync_automation.rs
git commit -m "test: point tag sync regression guards at src/tag_sync and sync_tags.rs"
```

---

## Task 8: End-to-end smoke test against the real `exiftool` binary

**Files:**
- Create: `tests/tag_sync_smoke.rs`

Gated on `exiftool` being present on `PATH` (skipped, not failed, otherwise — matches how format-comparison tests in this repo already handle an optional external dependency). Proves the whole pipeline is healthy: real `exiftool` output parses, routes, and yields materially better type coverage than today's 1.1% baseline.

- [ ] **Step 1: Write the test**

Create `tests/tag_sync_smoke.rs`:

```rust
//! End-to-end smoke test for the exiftool-listx-based tag sync pipeline.
//!
//! Skipped (not failed) when `exiftool` isn't on PATH, matching how other
//! exiftool-comparison tests in this repo handle the optional dependency.

use oxidex::tag_sync::{DOMAINS, generate_domain_yaml, parse_listx};
use std::process::Command;

fn exiftool_available() -> bool {
    Command::new("exiftool")
        .arg("-ver")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn real_exiftool_listx_parses_and_beats_the_current_type_coverage_baseline() {
    if !exiftool_available() {
        eprintln!("skipping: exiftool not found on PATH");
        return;
    }

    let output = Command::new("exiftool")
        .args(["-f", "-listx"])
        .output()
        .expect("failed to run exiftool -f -listx");
    assert!(output.status.success(), "exiftool -f -listx must succeed");

    let xml = String::from_utf8(output.stdout).expect("exiftool output must be valid UTF-8");
    let tags = parse_listx(&xml).expect("real exiftool -listx output must parse");

    // Baseline from the committed YAML as of 2026-07-19: 32,684 tags, 366
    // (1.1%) with a populated `type` field. The new pipeline must clear
    // both bars by a wide margin, since ExifTool resolves every tag's
    // writable/type attributes before emitting -listx.
    assert!(
        tags.len() > 30_000,
        "expected >30,000 tags from a real exiftool -listx dump, got {}",
        tags.len()
    );

    let typed = tags.iter().filter(|t| t.type_name.is_some()).count();
    let typed_fraction = typed as f64 / tags.len() as f64;
    assert!(
        typed_fraction > 0.5,
        "expected over 50% of tags to carry a type (old pipeline: 1.1%), got {:.1}%",
        typed_fraction * 100.0
    );

    // Every domain must route at least one tag for a full parse.
    for domain in DOMAINS {
        let yaml = generate_domain_yaml(domain, &tags);
        assert!(
            yaml.lines().count() > 1,
            "domain '{domain}' produced no tags from a real exiftool dump"
        );
    }
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test --test tag_sync_smoke -- --nocapture 2>&1 | tail -30`
Expected: `test result: ok. 1 passed` (assuming `exiftool` is installed locally, as it is in this environment — version 13.55).

- [ ] **Step 3: Commit**

```bash
git add tests/tag_sync_smoke.rs
git commit -m "test: add exiftool-gated smoke test proving tag sync coverage gains"
```

---

## Task 9: Fix the stale pipeline description in `docs/architecture/tag-database.md`

**Files:**
- Modify: `docs/architecture/tag-database.md`

The doc's "Tag Generation Pipeline" section currently describes the deleted Perl-source-download-and-regex-parse pipeline as if it were live and working. Leaving it as-is would actively mislead the next person who reads it.

- [ ] **Step 1: Replace the stale section**

Find this block in `docs/architecture/tag-database.md`:

```markdown
## Tag Generation Pipeline

Tags are automatically generated during build from Perl ExifTool source:

```
1. Download → Fetches latest ExifTool master from GitHub
2. Discover → Finds all 140+ .pm Perl modules recursively
3. Parse    → Extracts tag definitions using regex patterns
4. Resolve  → Follows subdirectory references for nested tables
5. Generate → Creates Rust code organized by format family
```
```

Replace with:

```markdown
## Tag Generation Pipeline

Tags are regenerated by explicitly running the `sync-tags` binary against a
locally-installed `exiftool` — never as a side effect of `cargo build`:

```bash
cargo run --release --bin sync_tags
```

```
1. Run     → `exiftool -f -listx` dumps ExifTool's own resolved tag database as XML
2. Parse   → src/tag_sync/mod.rs::parse_listx reads id/name/writable/type/description
3. Route   → each tag's table name maps to one of 6 oxidex-tags-* domain crates
4. Generate → writes oxidex-tags-{domain}/src/{domain}_tags.yaml directly
5. Record  → `.exiftool-version` is updated with the exiftool release used
```

Because ExifTool has already resolved table-level `WRITABLE` inheritance by
the time it emits `-listx` output, this captures per-tag type data that the
old Perl-regex parser missed for the vast majority of tags.
```

- [ ] **Step 2: Also fix the "Rebuilding" section**

Find:

```markdown
## Rebuilding

To force regeneration:

```bash
# Remove generated files
rm -rf oxidex-tags-*/src/generated/

# Rebuild (triggers build.rs)
cargo build
```
```

Replace with:

```markdown
## Rebuilding

To regenerate the tag database from a locally-installed `exiftool`:

```bash
cargo run --release --bin sync_tags
```

This overwrites `oxidex-tags-*/src/*_tags.yaml` and `.exiftool-version`
directly — review the resulting `git diff` before committing.
```

- [ ] **Step 3: Commit**

```bash
git add docs/architecture/tag-database.md
git commit -m "docs: describe the sync-tags binary instead of the deleted build.rs pipeline"
```

---

## Final verification

- [ ] Run the full workspace test suite: `cargo test --workspace --release 2>&1 | tail -50`. Expected: all tests pass, including the new `tag_sync` unit tests, `tag_sync_automation`, and `tag_sync_smoke`.
- [ ] Run `cargo clippy --workspace --all-targets 2>&1 | tail -50`. Expected: no new warnings introduced by `src/tag_sync/mod.rs` or `src/bin/sync_tags.rs`.
- [ ] Run `cargo run --release --bin sync_tags` for real and inspect `git diff --stat` — confirm all six `oxidex-tags-*/src/*_tags.yaml` files changed, `.exiftool-version` now contains `13.55` (or whatever is locally installed), and the diff is plausible (large increase in `type:` field coverage, no wholesale deletion of previously-present tags). Do not commit this regeneration as part of this plan unless the user asks — it's a large, reviewable diff that deserves its own look.
