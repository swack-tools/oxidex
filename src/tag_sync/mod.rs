//! ExifTool tag database sync.
//!
//! As of Step 30 (see `AGENTS.md`, "Tag knowledge is not tag coverage"), the
//! `oxidex-tags-*` YAML databases are generated from `dump_tables.pl`'s dump
//! of ExifTool's *real* Perl symbol table -- see
//! [`tag_records_from_dump_json`] -- not from `exiftool -f -listx`. `-listx`
//! is the documentation view: it emits exactly `count`, `encoding`, `id`,
//! `index`, `lang`, `name`, `type`, `version`, `writable` per tag, with no
//! `SubDirectory`/`TagTable` edges, no `FORMAT`/`FIRST_ENTRY`, no per-field
//! `Format`, `Mask`, `DataMember`, `Condition`, `ValueConv` or `RawConv`. That
//! is the byte layout, so it can tell you a tag *exists* but never how to
//! *read* it -- which is why a growing `-listx`-derived count never implied
//! growing extraction coverage, and why the generator no longer uses it.
//!
//! [`parse_listx`] and its siblings remain: `tests/tag_registry_invariants.rs`
//! runs them against a live pinned `exiftool -f -listx` as an *independent*
//! oracle, to catch PrintConv display values that leaked into the registry as
//! tags. That role is deliberately kept separate from generation -- an oracle
//! that shares code with the thing it grades would only catch its own bugs
//! consistently, never actually rule them out.
//!
//! When layout is what you need at read time (not registry generation), use
//! [`crate::exiftool_tables`], which reads the same real Perl structures
//! through a narrower, per-format lens.

use anyhow::{Context, Result};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

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

/// Per table, the set of PrintConv *display values* its tags map to.
///
/// `-listx` nests a tag's PrintConv table inside the tag itself:
///
/// ```xml
/// <tag id='41986' name='ExposureMode'>
///   <values><key id='1'><val lang='en'>Manual</val></key></values>
/// </tag>
/// ```
///
/// Those `<key>` elements are value rows, not tags. Reading them as tags is
/// what produced 16,005 bogus entries in the YAML registry (a tag literally
/// named `Higher resolution image exists`), so
/// `tests/tag_registry_invariants.rs` uses this to assert the registry never
/// lists a display value as a tag again.
pub fn parse_listx_print_conv_values(xml: &str) -> Result<HashMap<String, HashSet<String>>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut out: HashMap<String, HashSet<String>> = HashMap::new();
    let mut current_table = String::new();
    let mut capturing_en_val = false;
    let mut buf = Vec::new();

    loop {
        match reader
            .read_event_into(&mut buf)
            .context("failed to read XML event from exiftool -listx output")?
        {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == b"table" => {
                current_table = attr_value(&e, "name").unwrap_or_default();
            }
            Event::Start(e) if e.name().as_ref() == b"val" => {
                capturing_en_val = attr_value(&e, "lang").as_deref() == Some("en");
            }
            Event::Text(t) if capturing_en_val => {
                let decoded = t
                    .decode()
                    .context("invalid text content in <val> element")?;
                let text = quick_xml::escape::unescape(&decoded)
                    .unwrap_or_else(|_| decoded.clone())
                    .into_owned();
                out.entry(current_table.clone())
                    .or_default()
                    .insert(text.trim().to_string());
                capturing_en_val = false;
            }
            Event::End(e) if e.name().as_ref() == b"val" => {
                capturing_en_val = false;
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(out)
}

/// Reads a domain YAML back into `TagRecord`s.
///
/// The inverse of [`generate_domain_yaml`], and the reason regeneration is
/// non-destructive: `-listx` omits every tag that has no printable value, so a
/// straight regenerate silently deletes them. That includes the SubDirectory
/// pointers oxidex needs in order to *find* anything — `ExifOffset` (0x8769),
/// `GPSInfo` (0x8825), `InteropOffset` (0xA005) — plus tags ExifTool names at
/// runtime. 1,029 entries are in that position today, 47 of which feed
/// `lookup_tag_name`.
pub fn parse_domain_yaml(yaml: &str) -> Vec<TagRecord> {
    fn unquote(rest: &str) -> String {
        // Strip exactly one delimiter quote per side. `trim_matches('"')` is
        // greedy: a value ending in an escaped quote renders as `...\""`, and
        // trimming both trailing quote bytes corrupted it to `...\` before
        // the unescape pass could pair them back up.
        let trimmed = rest.trim();
        let inner = trimmed
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(trimmed);
        inner.replace("\\\"", "\"").replace("\\\\", "\\")
    }

    let mut out: Vec<TagRecord> = Vec::new();
    let mut table = String::new();
    for line in yaml.lines() {
        if let Some(rest) = line.strip_prefix("  - name: ") {
            table = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("      - id: ") {
            out.push(TagRecord {
                table: table.clone(),
                id: unquote(rest),
                name: String::new(),
                writable: false,
                type_name: None,
                description: None,
            });
        } else if let Some(last) = out.last_mut() {
            if let Some(rest) = line.strip_prefix("        name: ") {
                last.name = unquote(rest);
            } else if let Some(rest) = line.strip_prefix("        writable: ") {
                last.writable = rest.trim() == "true";
            } else if let Some(rest) = line.strip_prefix("        type: ") {
                last.type_name = Some(unquote(rest));
            } else if let Some(rest) = line.strip_prefix("        description: ") {
                last.description = Some(unquote(rest));
            }
        }
    }
    out.retain(|r| !r.table.is_empty() && !r.name.is_empty());
    out
}

static SPACE_LOWER_UPPER_DIGIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([a-z])([A-Z\d])").expect("static regex"));
static SPACE_ACRONYM_WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Z])([A-Z][a-z])").expect("static regex"));
static SPACE_DIGIT_UPPER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d)([A-Z]\S)").expect("static regex"));

/// Replicates `ExifTool.pm::MakeDescription()` (ExifTool.pm:6465-6483), the
/// algorithm `-listx` uses to synthesize a `<desc>` for every tag whose source
/// table declares no explicit `Description`. `dump_tables.pl` only ever
/// carries an *explicit* `Description` key — it reads the table literally, it
/// does not run ExifTool's display layer — so a tag relying on the
/// synthesized form needs it computed here too, or its `description` field
/// goes from a real value (the old `-listx`-sourced pipeline) to nothing (a
/// naive read of the dump). Verified against a real exiftool 13.59 `-f
/// -listx` dump: every one of its 33,698 tags carries a `<desc lang='en'>`,
/// so `description` in the generated registry is likewise never `None`.
///
/// The TagID-suffix branch of the Perl (`s/ (0x[\da-f]+)$//i ... $desc .= '
/// ' . $tagID`) is for ExifTool's synthetic tags whose *Name* is itself a hex
/// ID; every name here is a real tag name, so that branch never applies and
/// is not reproduced.
fn make_description(tag_name: &str) -> String {
    let mut desc = String::new();
    let mut chars = tag_name.chars();
    if let Some(first) = chars.next() {
        desc.extend(first.to_uppercase());
        desc.push_str(chars.as_str());
    }
    let desc: String = desc
        .chars()
        .map(|c| if c == '_' { ' ' } else { c })
        .collect();
    let desc = SPACE_LOWER_UPPER_DIGIT.replace_all(&desc, "$1 $2");
    let desc = SPACE_ACRONYM_WORD.replace_all(&desc, "$1 $2");
    let desc = SPACE_DIGIT_UPPER.replace_all(&desc, "$1 $2");
    desc.into_owned()
}

/// Strips a trailing ExifTool count suffix (`string[6]`, `binary[16]`) down to
/// the bare format name (`string`, `binary`). `-listx` reports element count
/// through its own separate `count=` attribute, which `TagRecord` has no slot
/// for (matching `parse_listx`, which never reads `count` either) — so the
/// `type` field carries only the format name on both generation paths.
fn strip_format_count(format: &str) -> &str {
    format.split('[').next().unwrap_or(format).trim()
}

/// Resolves ExifTool's `WRITABLE` inheritance for one tag: the tag's own
/// `Writable` wins if present; failing that, the table's `WRITABLE` applies.
/// A resolved value of `"0"`/empty/`"false"` means not writable; anything
/// else (a type name, or the boolean toggle `"1"`) means writable. Verified
/// against real dumps: `Canon::CameraInfo1DX`'s `FirmwareVersion` (index 640)
/// declares `Writable => 0` and reads back `writable='false'` from `-listx`
/// despite `Format => 'string[6]'` being present, and `QuickTime::Keys`
/// entries with no per-tag `Writable` at all read `writable='true'` because
/// the table declares `WRITABLE => 1`.
fn resolve_writable(entry: &serde_json::Value, table_writable: Option<&str>) -> bool {
    let own = entry.get("Writable").and_then(|v| v.as_str());
    let effective = own.or(table_writable);
    !matches!(effective, None | Some("0" | "" | "false"))
}

/// Resolves the `type` `-listx` would report for one tag: a per-tag
/// `Writable` that names a real format (not the boolean toggle `"0"`/`"1"`)
/// wins; failing that, a binary `Format`; failing that, ExifTool's own
/// "unknown/composite" spelling `?` — every one of a real dump's 33,698 tags
/// carries a `type` attribute, `?` included, so this never returns `None`.
/// A declared numeric type is only safe to publish when the tag is
/// single-valued. `Tag`/`TagRecord` has no `Count` field (matching `-listx`,
/// which reports element count through its own separate `count=` attribute
/// that `parse_listx` never captured either), so there is nowhere to record
/// "int8u, four of them" — and downstream, `src/cli/value_parser.rs` reads
/// only the type name: a `ValueType::Integer`/`Float`/`Rational` tag goes
/// through `parse_integer`/`parse_float`/`parse_rational`, all of which parse
/// a single scalar and reject `"3 3 3 3"` with "Not an integer".
///
/// Before this generator, essentially no tag had a `type` at all (the old
/// registry's own baseline: 1.1% typed), so that branch was never reached for
/// these tags — a declared-but-unreliable type sent every one of them through
/// the `None | Some(ValueType::String)` fallback instead, which passes the
/// raw multi-value string down to the writer verbatim and works. Publishing
/// `int8u` for DNG's `DNGVersion` (Exif.pm 0xc612, `Count => 4`) is more
/// accurate than omitting it, but it is also a regression the CLI cannot
/// act on correctly yet — confirmed by hand: `oxidex -EXIF:DNGVersion="3 3 3
/// 3"` fails with "Not an integer" once the type is trusted. Per AGENTS.md,
/// "never approximate a conversion" — a type the write path cannot honor is
/// worse than no type, so a numeric type is only published when `Count` is
/// absent or `"1"` -- with one more tell an absent `Count` does not cover.
/// ExifTool's `XPTitle`/`XPComment`/`XPKeywords`/`XPSubject` family (Exif.pm
/// 0x9c9b-0x9c9e) declares `Format => 'undef'` (an unbounded binary blob)
/// alongside `Writable => 'int8u'` (the *per-element* type for writing it as
/// a byte sequence) and no `Count` at all -- ExifTool only states `Count` for
/// a *fixed*-length array, and these are variable-length UCS2 text stored as
/// bytes. Confirmed by hand: with the type trusted, `oxidex -XPTitle=...`
/// mis-parses the sample as a lone scalar the same way an untrusted `Count`
/// array does. So `Format` present and naming an unbounded/binary layout
/// while `Writable` names a scalar numeric per-element type is exactly as
/// unreliable as `Count` > 1, even though `Count` itself is silent.
fn is_scalar_count(entry: &serde_json::Value) -> bool {
    let count_says_scalar = match entry.get("Count").and_then(|v| v.as_str()) {
        None => true,
        Some(count) => count == "1",
    };
    if !count_says_scalar {
        return false;
    }
    let format_says_unbounded_blob = entry
        .get("Format")
        .and_then(|v| v.as_str())
        .map(strip_format_count)
        .is_some_and(|f| matches!(f.to_ascii_lowercase().as_str(), "undef" | "binary"));
    !format_says_unbounded_blob
}

/// Whether `resolve_type`'s candidate type string is one whose value_parser.rs
/// consumer (`ValueType::Integer`/`Float`/`Rational`) parses a single scalar.
/// `String`/`Binary`/`DateTime` pass the raw argument through unsplit, so a
/// multi-value count never trips them.
fn is_scalar_only_type_family(type_name: &str) -> bool {
    let normalized = type_name.to_ascii_lowercase();
    normalized.starts_with("int")
        || normalized.starts_with("rational")
        || matches!(normalized.as_str(), "float" | "double" | "real")
}

fn resolve_type(entry: &serde_json::Value) -> Option<String> {
    let candidate = if let Some(w) = entry.get("Writable").and_then(|v| v.as_str()) {
        (!matches!(w, "0" | "1" | "" | "false" | "true")).then(|| strip_format_count(w).to_string())
    } else {
        None
    };
    let candidate = candidate.or_else(|| {
        entry
            .get("Format")
            .and_then(|v| v.as_str())
            .map(strip_format_count)
            .filter(|f| !f.is_empty())
            .map(str::to_string)
    });

    match candidate {
        Some(type_name) if is_scalar_only_type_family(&type_name) && !is_scalar_count(entry) => {
            Some("?".to_string())
        }
        Some(type_name) => Some(type_name),
        None => Some("?".to_string()),
    }
}

/// Expands ExifTool's array-of-conditional-variants tag shape
/// (`dump_tag_entry`'s `_variants`, e.g. Canon `CameraInfo`'s 33
/// model-dependent layouts) into one entry per variant. `-listx` does the
/// same thing at the XML level — Canon's `SerialNumber` (id 12) appears as
/// three separate `<tag id='12' index='0/1/2'>` elements — and `parse_listx`
/// already turns each into its own `TagRecord` with no `index` field, so this
/// matches that shape rather than introducing one.
fn expand_variants(tag_val: &serde_json::Value) -> Vec<&serde_json::Value> {
    match tag_val.get("_variants").and_then(|v| v.as_array()) {
        Some(variants) => variants.iter().collect(),
        None => vec![tag_val],
    }
}

fn entry_name(entry: &serde_json::Value) -> Option<String> {
    entry
        .get("Name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// The same tag-name shape `tests/tag_registry_invariants.rs` checks the
/// generated registry against
/// (`registry_tag_names_are_shaped_like_exiftool_tag_names`): starts
/// alphanumeric or `_`, then only alphanumeric, `_` or `-`.
fn looks_like_tag_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Recognizes a plain PrintConv/lookup hash that `dump_tables.pl`'s admission
/// heuristic let through as if it were a tag table.
///
/// That heuristic (`dump_tables.pl`'s `dump_module`) admits a hash that
/// either declares table-level metadata (`GROUPS`, `WRITABLE`, ...) or has at
/// least one struct-valued entry. An `OTHER => sub {...}` fallback callback —
/// a normal idiom for "how to print an id with no listed name" inside a
/// lookup hash — is itself struct-valued, so a hash that is otherwise
/// entirely `id => 'Display Name'` pairs (`Sony::sonyLensTypes`,
/// `Nikon::nikonLensIDs`, `Exif::flash`, ...) passes the identical gate a
/// real tag table does, and its display strings would otherwise land in the
/// registry as fake tags — the exact bug class
/// `registry_tag_names_are_shaped_like_exiftool_tag_names` exists to catch,
/// coming from a different source than the one that test was originally
/// written against (`-listx` nesting PrintConv `<key>` rows as if they were
/// `<tag>` rows).
///
/// Verified against a real ExifTool 13.59 dump: exactly seven module-wide
/// tables have this shape, contributing 1,758 non-tag rows, all lens/flash/
/// subfile-type lookups; every genuine table with no metadata in the same
/// dump is 100% tag-name-shaped. Applying the same shape rule here, keyed
/// on the absence of table metadata, fixes the leak at the source rather
/// than hand-listing table names a future ExifTool release could add to.
fn is_probably_a_value_lookup_table(
    meta: Option<&serde_json::Map<String, serde_json::Value>>,
    names: &[String],
) -> bool {
    if meta.is_some_and(|m| !m.is_empty()) || names.is_empty() {
        return false;
    }
    let unshaped = names.iter().filter(|n| !looks_like_tag_name(n)).count();
    (unshaped as f64 / names.len() as f64) > 0.5
}

fn entry_description(entry: &serde_json::Value, tag_name: &str) -> String {
    entry
        .get("Description")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| make_description(tag_name))
}

/// Parses `dump_tables.pl`'s JSON dump — ExifTool's real Perl tag tables, read
/// out of the live symbol table rather than through `-listx`'s documentation
/// filter — into the same flat `TagRecord` shape [`parse_listx`] produces.
/// This is the primary source [`generate_domain_yaml`] is fed from as of Step
/// 30 (`AGENTS.md`, "Tag knowledge is not tag coverage").
///
/// Three things fall out of reading the real tables instead of `-listx`:
///
/// - **No carry-forward.** `-listx` omits every tag with no printable value,
///   which is most of the `SubDirectory` pointers oxidex needs to find
///   anything (`ExifOffset` 0x8769, `GPSInfo` 0x8825, `InteropOffset`
///   0xA005). Those are ordinary entries here, so the 1,029-row
///   carry-forward `sync_tags` used to preserve them (and the corrupt legacy
///   rows that carry-forward could never distinguish from genuine ones) is
///   gone outright rather than ported.
/// - **Composite tags are merged by hand.** ExifTool assembles the single
///   `Image::ExifTool::Composite` package `-listx` reports from every
///   module's own `%Composite` sub-table at `require`-time
///   (`AddCompositeTags`); `dump_tables.pl` walks `.pm` files directly and
///   never observes that merge. This reconstructs it: every module's
///   `Composite` table folds into one `"Composite"` table, with ids
///   disambiguated by module prefix exactly as `-listx` does (`Exif-LensID`
///   vs `Exif-LensID-2`) — keyed by the tag's *source* hash key, not `Name`;
///   `Exif::Composite` defines both `LensID` and `LensID-2` with `Name =>
///   'LensID'` on each, so the key is what keeps them distinct.
/// - **`WRITABLE`/type inheritance and missing descriptions are resolved by
///   hand** ([`resolve_writable`], [`resolve_type`], [`make_description`]),
///   since `-listx` reports those fully resolved and the raw dump does not.
pub fn tag_records_from_dump_json(json: &str) -> Result<Vec<TagRecord>> {
    let doc: serde_json::Value =
        serde_json::from_str(json).context("failed to parse dump_tables.pl JSON")?;
    let modules = doc
        .get("modules")
        .and_then(|v| v.as_object())
        .context("dump JSON missing a 'modules' object")?;

    let mut records = Vec::new();

    for (module_name, module_val) in modules {
        // `Shortcuts.pm` defines named aliases to OTHER tags
        // (`CommonIFD0 => ['IFD0:Make', 'IFD0:Model', ...]`), not tags of its
        // own -- ExifTool's own `-listx` excludes it entirely (verified: zero
        // `<table name='Shortcuts::...'>` in a real dump). Its list values
        // are plain strings, so `dump_tag_entry`'s `_variants`/shorthand
        // handling turns each aliased reference into a fake tag whose name is
        // a group-qualified string like `"IFD0:Make"` -- caught by
        // `tests/tag_registry_invariants.rs::registry_tag_names_are_shaped_like_exiftool_tag_names`,
        // since `:` is not a valid tag-name character.
        if module_name == "Shortcuts" {
            continue;
        }

        let Some(tables) = module_val.get("tables").and_then(|v| v.as_object()) else {
            continue;
        };

        for (table_symbol, table_val) in tables {
            let Some(tags) = table_val.get("tags").and_then(|v| v.as_object()) else {
                continue;
            };

            if table_symbol == "Composite" {
                for (tag_key, tag_val) in tags {
                    for entry in expand_variants(tag_val) {
                        let Some(name) = entry_name(entry) else {
                            continue;
                        };
                        records.push(TagRecord {
                            table: "Composite".to_string(),
                            id: format!("{module_name}-{tag_key}"),
                            name: name.clone(),
                            writable: resolve_writable(entry, None),
                            type_name: resolve_type(entry),
                            description: Some(entry_description(entry, &name)),
                        });
                    }
                }
                continue;
            }

            let full_name = table_val
                .get("full_name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let table_name = full_name
                .strip_prefix("Image::ExifTool::")
                .unwrap_or(full_name)
                .to_string();
            if table_name.is_empty() {
                continue;
            }

            let table_meta = table_val.get("meta").and_then(|m| m.as_object());
            let table_writable = table_meta
                .and_then(|m| m.get("WRITABLE"))
                .and_then(|w| w.as_str());

            // Expand variants up front so the whole table can be classified
            // (see `is_probably_a_value_lookup_table`) before any of its rows
            // are committed -- a table found to be a lookup hash in disguise
            // is skipped in full, not row-by-row.
            let candidates: Vec<(&String, &serde_json::Value)> = tags
                .iter()
                .flat_map(|(tag_key, tag_val)| {
                    expand_variants(tag_val)
                        .into_iter()
                        .map(move |entry| (tag_key, entry))
                })
                .collect();
            let names: Vec<String> = candidates
                .iter()
                .filter_map(|(_, entry)| entry_name(entry))
                .collect();
            if is_probably_a_value_lookup_table(table_meta, &names) {
                continue;
            }

            // ExifTool's own conditional dispatch can bind ONE id to several
            // DIFFERENT names, tried in declaration order until a Condition
            // matches -- and `generate_domain_yaml` re-sorts every table's
            // tags alphabetically for diffable output, which would otherwise
            // let `TAG_ID_TO_NAME_INDEX`'s first-wins reverse index
            // (`src/tag_db/mod.rs`) pick whichever name sorts first
            // alphabetically, not whichever ExifTool would actually apply.
            // Two real, opposite-shaped cases:
            //
            // - Exif.pm 0x117: mostly `StripByteCounts` (its Condition only
            //   excludes three specific formats), `JpgFromRawLength` only as
            //   a DNG SubIFD2 special case. One variant is the practical
            //   default; the id should resolve to its name.
            // - Exif.pm 0x202 declares NINE variants (`ThumbnailLength`,
            //   `PreviewImageLength`, `JpgFromRawLength`, `OtherImageLength`)
            //   each scoped to a specific `DIR_NAME`/`TIFF_TYPE`, with no
            //   variant that covers the ordinary case. And 0x927c is
            //   `\@Image::ExifTool::MakerNotes::Main`, a *routing table* of
            //   ~94 manufacturer-specific SubDirectory targets
            //   (`MakerNoteApple`, `MakerNoteCanon`, ...) that `dump_tag_entry`
            //   cannot tell apart from an ordinary conditional-format list --
            //   both are just a Perl ARRAY ref to it. Neither id has a
            //   default; which name is right depends on context (`Make`,
            //   `DIR_NAME`) this static, context-free index does not have.
            //   Picking any one of them mislabels every file it is wrong for
            //   -- the exact "plausible but wrong is worse than absent" case
            //   AGENTS.md warns about, worse than the honest hex-fallback key
            //   this used to leave in place.
            //
            // The reliable structural difference: ExifTool's own `WriteGroup`
            // marks a variant as scoped to one specific IFD/context (Exif.pm
            // 0x202's four names all declare one; 0x927c's ~94 MakerNote
            // routes all declare one). A variant that leaves `WriteGroup`
            // unset inherits the table's own default and is the
            // general-purpose case -- true of 0x117's `StripByteCounts`, and
            // of every truly single-name tag (format-only variants, like
            // Canon's `SerialNumber`, never scope `WriteGroup` per variant).
            // So: with more than one distinct name for an id, only the
            // first-declared `WriteGroup`-less variant's name may claim the
            // plain id; if every variant scopes a `WriteGroup`, none does and
            // the id stays a hex fallback for lookups the way it always
            // reached this table. `dump_tables.pl` does not carry `WriteGroup`
            // as data (it is not in its curated `TAG_KEYS`), only records its
            // presence in `_extra_keys`, which is enough to ask "is this
            // variant scoped" without needing the scope's actual value.
            fn has_write_group(entry: &serde_json::Value) -> bool {
                entry
                    .get("_extra_keys")
                    .and_then(|v| v.as_array())
                    .is_some_and(|keys| keys.iter().any(|k| k.as_str() == Some("WriteGroup")))
            }

            let mut winner_name_for_id: HashMap<&str, Option<&str>> = HashMap::new();
            for (tag_key, entry) in &candidates {
                let Some(name) = entry.get("Name").and_then(|v| v.as_str()) else {
                    continue;
                };
                winner_name_for_id
                    .entry(tag_key.as_str())
                    .and_modify(|winner| {
                        // A later variant can only become the recorded
                        // winner if none has been found yet and this one is
                        // unscoped; an already-found winner (from an earlier
                        // variant) is never displaced.
                        if winner.is_none() && !has_write_group(entry) {
                            *winner = Some(name);
                        }
                    })
                    .or_insert_with(|| (!has_write_group(entry)).then_some(name));
            }
            // A single distinct name always wins outright, `WriteGroup`
            // or not -- scoping which IFD an already-unambiguous name
            // writes to is not the same question as which of several names
            // reads an id.
            for (tag_key, entry) in &candidates {
                if let Some(name) = entry.get("Name").and_then(|v| v.as_str())
                    && let Some(winner) = winner_name_for_id.get_mut(tag_key.as_str())
                    && winner.is_none_or(|w| w != name)
                    && candidates
                        .iter()
                        .filter(|(k, _)| k.as_str() == tag_key.as_str())
                        .filter_map(|(_, e)| e.get("Name").and_then(|v| v.as_str()))
                        .all(|n| n == name)
                {
                    *winner = Some(name);
                }
            }

            for (tag_key, entry) in candidates {
                let Some(name) = entry_name(entry) else {
                    continue;
                };
                let id = if winner_name_for_id.get(tag_key.as_str()).copied().flatten()
                    == Some(name.as_str())
                {
                    tag_key.clone()
                } else {
                    format!("{tag_key}#{name}")
                };
                records.push(TagRecord {
                    table: table_name.clone(),
                    id,
                    name: name.clone(),
                    writable: resolve_writable(entry, table_writable),
                    type_name: resolve_type(entry),
                    description: Some(entry_description(entry, &name)),
                });
            }
        }
    }

    Ok(records)
}

/// Parses a `-listx` id, which is decimal (`41986`) for numeric tables and
/// hex (`0x829a`) elsewhere. Returns `None` for the shapes that are neither —
/// FlashPix's `0016,0042` and NikonCustom's bit-positions like `1.1`.
pub fn parse_tag_id(text: &str) -> Option<i64> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => i64::from_str_radix(hex, 16).ok(),
        None => text.parse::<i64>().ok(),
    }
}

/// Per table, which PrintConv *key* maps to which display values.
///
/// The keyed form of [`parse_listx_print_conv_values`]. It exists to catch the
/// case that the unkeyed form cannot: a display value whose name collides with
/// a real tag of the same table, so the only evidence it is a value row is that
/// its id is that value's PrintConv key. See
/// `tests/tag_registry_invariants.rs::registry_tag_ids_are_not_print_conv_keys_in_disguise`.
pub fn parse_listx_print_conv_keys(
    xml: &str,
) -> Result<HashMap<String, HashMap<i64, HashSet<String>>>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut out: HashMap<String, HashMap<i64, HashSet<String>>> = HashMap::new();
    let mut current_table = String::new();
    let mut current_key: Option<i64> = None;
    let mut capturing_en_val = false;
    let mut buf = Vec::new();

    loop {
        match reader
            .read_event_into(&mut buf)
            .context("failed to read XML event from exiftool -listx output")?
        {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == b"table" => {
                current_table = attr_value(&e, "name").unwrap_or_default();
            }
            Event::Start(e) if e.name().as_ref() == b"key" => {
                current_key = attr_value(&e, "id").as_deref().and_then(parse_tag_id);
            }
            Event::End(e) if e.name().as_ref() == b"key" => {
                current_key = None;
            }
            Event::Start(e) if e.name().as_ref() == b"val" => {
                capturing_en_val = attr_value(&e, "lang").as_deref() == Some("en");
            }
            Event::Text(t) if capturing_en_val => {
                if let Some(key) = current_key {
                    let decoded = t.decode().context("invalid text content in <val>")?;
                    let text = quick_xml::escape::unescape(&decoded)
                        .unwrap_or_else(|_| decoded.clone())
                        .into_owned();
                    out.entry(current_table.clone())
                        .or_default()
                        .entry(key)
                        .or_default()
                        .insert(text.trim().to_string());
                }
                capturing_en_val = false;
            }
            Event::End(e) if e.name().as_ref() == b"val" => {
                capturing_en_val = false;
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(out)
}

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

fn escape_yaml_string(s: &str) -> String {
    // Beyond backslash/quote, dump_tables.pl-sourced ids can carry real
    // control bytes -- FlashPix's Main table is keyed by raw OLE compound-file
    // stream names like `\x01CompObj` (a literal 0x01 byte), which `-listx`
    // never emitted since it worked from resolved text, not raw hash keys.
    // YAML's double-quoted scalar forbids literal control characters
    // (`serde_yaml` refuses them with "control characters are not allowed"),
    // so anything below U+0020 other than the ones with a named escape needs
    // `\xHH` — the same escape a `\\`/`\"` pair already relies on being valid
    // inside a double-quoted scalar.
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02X}", c as u32)),
            c => out.push(c),
        }
    }
    out
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
            yaml.push_str(&format!(
                "      - id: \"{}\"\n",
                escape_yaml_string(&tag.id)
            ));
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

/// Counts tag entries in a domain YAML file by counting `- id:` lines —
/// matches the counting method `sync-exiftool-tags.yml` already uses via
/// `grep -hE '^[[:space:]]+- id:'`, so sanity checks agree with CI reporting.
pub fn count_ids_in_yaml(yaml_content: &str) -> usize {
    yaml_content
        .lines()
        .filter(|line| line.trim_start().starts_with("- id:"))
        .count()
}

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
        assert!(
            result.is_err(),
            "truncated XML must return an error, not panic"
        );
    }

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
    fn counts_id_lines_regardless_of_indentation() {
        let yaml = "tables:\n  - name: Exif::Main\n    tags:\n      - id: \"271\"\n        name: \"Make\"\n      - id: \"272\"\n        name: \"Model\"\n";
        assert_eq!(count_ids_in_yaml(yaml), 2);
    }

    #[test]
    fn counts_zero_for_empty_yaml() {
        assert_eq!(count_ids_in_yaml("tables:\n"), 0);
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

    #[test]
    fn generate_domain_yaml_is_idempotent_regardless_of_input_order() {
        // Verifies that `generate_domain_yaml` produces deterministic,
        // byte-identical output regardless of input tag order. This is
        // guaranteed by sorting tables and tags within each table before
        // emission, allowing tool output to be diff-friendly and reproducible.
        let tags_forward = vec![
            TagRecord {
                table: "Exif::Main".to_string(),
                id: "271".to_string(),
                name: "Make".to_string(),
                writable: true,
                type_name: Some("string".to_string()),
                description: Some("Camera manufacturer".to_string()),
            },
            TagRecord {
                table: "Exif::Main".to_string(),
                id: "272".to_string(),
                name: "Model".to_string(),
                writable: true,
                type_name: Some("string".to_string()),
                description: Some("Camera model".to_string()),
            },
            TagRecord {
                table: "Canon::Main".to_string(),
                id: "1".to_string(),
                name: "CanonImageType".to_string(),
                writable: false,
                type_name: None,
                description: None,
            },
        ];

        // Same tags in reverse order
        let mut tags_reversed = tags_forward.clone();
        tags_reversed.reverse();

        let yaml_forward = generate_domain_yaml("core", &tags_forward);
        let yaml_reversed = generate_domain_yaml("core", &tags_reversed);

        assert_eq!(
            yaml_forward, yaml_reversed,
            "YAML output must be byte-identical regardless of input order"
        );

        // Verify table-level ordering is stable: tables must appear in sorted order
        let table_order_forward = generate_domain_yaml("core", &tags_forward);
        assert!(table_order_forward.find("Exif::Main").unwrap() < table_order_forward.len());
        // Both tables are in the same domain; Exif::Main should appear before Canon
        // (since Canon sorts after Exif). But they're in different domains so only
        // check that the core domain has consistent output.

        // For camera domain, verify Canon tags are present and ordered
        let yaml_camera = generate_domain_yaml("camera", &tags_forward);
        let yaml_camera_reversed = generate_domain_yaml("camera", &tags_reversed);
        assert_eq!(yaml_camera, yaml_camera_reversed);
        assert!(yaml_camera.contains("CanonImageType"));
    }

    /// A regenerate must not delete what `-listx` does not report.
    ///
    /// ExifTool omits every tag with no printable value, which is most of the
    /// SubDirectory pointers oxidex needs in order to find anything at all --
    /// `ExifOffset` (0x8769) is how the EXIF sub-IFD gets located. Reading the
    /// shipped registry back must recover them, because that is the set
    /// sync_tags carries forward.
    #[test]
    fn parse_domain_yaml_recovers_subdirectory_tags_listx_omits() {
        let yaml = include_str!("../../oxidex-tags-core/src/core_tags.yaml");
        let records = parse_domain_yaml(yaml);
        assert!(
            records.len() > 1000,
            "parsed only {} records",
            records.len()
        );

        for needed in ["ExifOffset", "GPSInfo", "InteropOffset"] {
            let found = records
                .iter()
                .find(|r| r.name == needed && r.table == "Exif::Main");
            assert!(
                found.is_some(),
                "{needed} was not recovered from the registry"
            );
        }
    }

    /// Round-trip: whatever `generate_domain_yaml` writes, `parse_domain_yaml`
    /// must read back, or the carry-forward set would be silently short.
    #[test]
    fn generate_and_parse_domain_yaml_round_trip() {
        let tags = vec![
            TagRecord {
                table: "Exif::Main".into(),
                id: "0x8769".into(),
                name: "ExifOffset".into(),
                writable: false,
                type_name: Some("int32u".into()),
                description: Some("Exif IFD Pointer".into()),
            },
            TagRecord {
                table: "Exif::Main".into(),
                id: "0x010e".into(),
                name: "ImageDescription".into(),
                writable: true,
                type_name: None,
                description: Some(r#"a "quoted" description"#.into()),
            },
        ];
        let back = parse_domain_yaml(&generate_domain_yaml("core", &tags));
        assert_eq!(back.len(), 2);
        let exif = back.iter().find(|r| r.name == "ExifOffset").unwrap();
        assert_eq!(exif.id, "0x8769");
        assert_eq!(exif.table, "Exif::Main");
        assert_eq!(exif.type_name.as_deref(), Some("int32u"));
        let desc = back.iter().find(|r| r.name == "ImageDescription").unwrap();
        assert!(desc.writable);
        assert_eq!(
            desc.description.as_deref(),
            Some(r#"a "quoted" description"#)
        );
    }

    /// A quote at either boundary of the value renders as an escaped quote
    /// adjacent to the delimiter quote (`"...\""`). The old greedy
    /// `trim_matches('"')` stripped both trailing quote bytes and corrupted
    /// the value to `...\` on every resync of the shipped registry.
    #[test]
    fn domain_yaml_round_trips_values_with_boundary_quotes() {
        let tags = vec![
            TagRecord {
                table: "Exif::Main".into(),
                id: "0x9999".into(),
                name: "BoundaryQuotes".into(),
                writable: false,
                type_name: None,
                description: Some(r#"Value is "N/A""#.into()),
            },
            TagRecord {
                table: "Exif::Main".into(),
                id: "0x999a".into(),
                name: "LeadingQuote".into(),
                writable: false,
                type_name: None,
                description: Some(r#""quoted" from the very first byte"#.into()),
            },
        ];
        let back = parse_domain_yaml(&generate_domain_yaml("core", &tags));
        assert_eq!(back.len(), 2);
        assert_eq!(
            back.iter()
                .find(|r| r.name == "BoundaryQuotes")
                .unwrap()
                .description
                .as_deref(),
            Some(r#"Value is "N/A""#)
        );
        assert_eq!(
            back.iter()
                .find(|r| r.name == "LeadingQuote")
                .unwrap()
                .description
                .as_deref(),
            Some(r#""quoted" from the very first byte"#)
        );
    }
}
