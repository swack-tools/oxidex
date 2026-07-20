//! Rust port of scripts/jpeg_tag_report.py: turn results.json into the two
//! committed markdown reports and ratchet the regression baseline.

use clap::Args;
use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use crate::types::{BaselineCounts, ReadonlyFile, ReadonlyTag, ResultEntry};

#[derive(Args)]
pub struct ReportArgs {
    #[arg(long, help = "write current summary counts to jpeg-tag-baseline.json")]
    pub update_baseline: bool,
    #[arg(long, help = "exit 1 if support regressed vs the committed baseline")]
    pub check_baseline: bool,
}

static READ_BUG_LABELS: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    [
        (
            "R-iptc-binary-garbage",
            "IPTC binary int16u decoded as NUL-garbage string",
        ),
        (
            "R-binary-garbage",
            "binary value decoded as NUL-garbage string",
        ),
        ("R-apex-missing", "APEX ValueConv (2^x) not applied"),
        (
            "R-namespace-blind-printconv",
            "-e compat applies EXIF enum table to XMP tag",
        ),
        (
            "R-acr-prefix",
            "ACR \"TagName: \" ValueConv prefix not stripped",
        ),
        (
            "R-undef-not-decoded",
            "undef/binary value shown as opaque (Binary, N bytes)",
        ),
        (
            "R-utf16-not-decoded",
            "XP* UTF-16 string shown as raw integer",
        ),
        ("R-float-raw-bits", "float value shown as raw IEEE-754 bits"),
        (
            "R-xmp-struct-concat",
            "XMP struct fields concatenated into garbage scalar",
        ),
    ]
    .into_iter()
    .collect()
});
static WRITE_BUG_LABELS: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    [
        (
            "I1-no-printconvinv",
            "PrintConvInv missing: human-readable stored as raw",
        ),
        (
            "I2-wrong-type-enum",
            "written as ASCII where SHORT/LONG expected",
        ),
        (
            "I3-wrong-type-numeric",
            "written as ASCII where numeric/rational expected",
        ),
        (
            "I4-wrong-type-undef",
            "written as ASCII where UNDEF expected (+NUL)",
        ),
        (
            "I5-subdir-poison",
            "junk written into subdirectory pointer tag",
        ),
        (
            "R4-registry-asymmetry",
            "writes OK; reads back under hex key (registry asymmetry)",
        ),
    ]
    .into_iter()
    .collect()
});

/// Roll a raw read/write result up into (headline, read-desc, write-desc).
/// Port of jpeg_tag_report.py:54-110.
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
        Some("INTEROP_BROKEN") => format!(
            "🐛 broken: {}",
            r.bug_cluster
                .as_deref()
                .and_then(|c| WRITE_BUG_LABELS.get(c))
                .copied()
                .unwrap_or("interop (exiftool can't read it)")
        ),
        Some("NOT_WRITTEN") => "— unsupported (silent no-op)".to_string(),
        other => other.unwrap_or("").to_lowercase(),
    };

    let rd_c = match rd {
        Some("OK") => "✅ ok".to_string(),
        Some("MISMATCH_FORMAT") => "✅ ok (formatting differs from exiftool)".to_string(),
        Some("MISMATCH") => format!(
            "🐛 broken: {}",
            r.read_bug
                .as_deref()
                .and_then(|b| READ_BUG_LABELS.get(b))
                .copied()
                .unwrap_or("wrong value decoded")
        ),
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

// NOTE on staleness (checked 2026-07-19 against this branch's HEAD, see
// task-9-report.md for the full trail):
//   - W1 (`fix/wiring` commit 2433c79): confirmed still NOT merged onto this
//     branch (`git merge-base --is-ancestor 2433c79 HEAD` fails) and
//     empirically reproduced (`-ExifIFD:ISO=200` still errors with "Type
//     mismatch: expected Integer but got String"). Left as-is.
//   - W2 (#18, 0f5b7f5): confirmed merged and empirically verified — writing
//     a *pre-existing* ExifIFD:DateTimeOriginal now round-trips correctly;
//     writing one with *no* pre-existing value is still misrouted to the
//     date-shift path and still fails. Left as-is (matches the Python
//     source's already-updated wording).
//   - R8 (#22, b596476): confirmed merged; wording already reflects the fix.
//     Left as-is.
//   - W7: found STALE while spot-checking neighboring rows. PR #24
//     (bcc1d29) replaced the write path with a surgical raw-carry-over
//     writer (`src/writers/exif_surgical.rs`) that explicitly preserves the
//     original TIFF byte order. Empirically verified: writing a tag into
//     `tests/fixtures/jpeg/tag_matrix_base.jpg` (a big-endian/MM fixture)
//     leaves the EXIF byte-order marker as `MM` in the output. Updated the
//     row below to describe the fix instead of the old bug.
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
| W7 | **Fixed.** Previously every write flipped EXIF byte order MM→II (big- to little-endian) even when only one tag changed. The write path now goes through a surgical raw-carry-over rewriter (`src/writers/exif_surgical.rs`, added by [#24](https://github.com/swack-tools/oxidex/pull/24)) that preserves the original byte order; empirically verified against a big-endian (`MM`) fixture, which still reads back `MM` after a write | 0 (was: all writes) | was: EXIF segment fully rebuilt in II order; now: `src/writers/exif_surgical.rs` carries `scan.byte_order` through to the rewritten segment |
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

fn md_escape(s: &str, n: usize) -> String {
    let escaped = s.replace('|', "\\|").replace('\n', " ");
    escaped.chars().take(n).collect()
}

fn autogen_callout(work: &Path) -> String {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let manifest_path = work.join("exiftool_jpeg_tags.json");
    let ver = std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| {
            v.get("generated_by")
                .and_then(|g| g.as_str())
                .map(String::from)
        })
        .unwrap_or_else(|| "ExifTool".to_string());
    format!(
        "::: info Auto-Generated\n\
         This report is regenerated by the \
         [JPEG Tag Matrix workflow]\
         (https://github.com/swack-tools/oxidex/actions/workflows/\
         jpeg-tag-matrix.yml) \
         (`jpeg-tag-matrix` binary) against **{ver}**. \
         Last updated: **{today}**\n\
         :::\n"
    )
}

struct Row {
    key: String,
    group: String,
    name: String,
    sample: String,
    #[allow(dead_code)]
    vtype: String,
    head: String,
    rd: String,
    wr: String,
    ox_key: String,
    wkey: String,
    raw_read: Option<String>,
    raw_write: Option<String>,
    wq: Option<String>,
}

/// Subcommand entry point: load results.json, classify every tag, write
/// docs/reference/jpeg-tag-{support,matrix}.md, and (optionally) ratchet the
/// committed baseline. Port of jpeg_tag_report.py's `main`.
pub fn run(args: ReportArgs) -> anyhow::Result<()> {
    // `TAGMATRIX_REPO` is a test-only escape hatch: the end-to-end test below
    // needs `run()` to treat a scratch tempdir as the repo root (so it never
    // writes over the real committed docs/reference/*.md), but calling
    // `std::env::set_current_dir` would mutate process-wide state shared by
    // every test in this binary — including matrix.rs's write-phase tests,
    // which resolve fixture scripts via a *relative* `FIXTURE_DIR` path and
    // would break under a changed CWD when tests run in parallel (confirmed
    // empirically: `cargo test` intermittently failed
    // `matrix::write_phase_tests::*` once a CWD-changing test ran
    // concurrently). Production runs never set this var, so `repo` is
    // `current_dir()` exactly as the Python's caller-invokes-from-repo-root
    // convention (and Tasks 1-8's `manifest::run`/`matrix::run`) expect.
    let repo = match std::env::var("TAGMATRIX_REPO") {
        Ok(p) => std::path::PathBuf::from(p),
        Err(_) => std::env::current_dir()?,
    };
    let work = std::env::var("TAGMATRIX_WORK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("oxidex-tagmap"));
    let results_path = work.join("results.json");
    let readonly_path = work.join("exiftool_jpeg_readonly_tags.json");
    let out_support = repo.join("docs/reference/jpeg-tag-support.md");
    let out_matrix = repo.join("docs/reference/jpeg-tag-matrix.md");
    let baseline_path = repo.join("docs/reference/jpeg-tag-baseline.json");

    let results: HashMap<String, ResultEntry> =
        serde_json::from_str(&std::fs::read_to_string(&results_path)?)?;
    let mut rows: Vec<Row> = results
        .iter()
        .map(|(key, r)| {
            let (head, rd, wr) = classify(r);
            Row {
                key: key.clone(),
                group: if r.group.is_empty() {
                    key.split(':').next().unwrap_or("").to_string()
                } else {
                    r.group.clone()
                },
                name: if r.name.is_empty() {
                    key.rsplit(':').next().unwrap_or("").to_string()
                } else {
                    r.name.clone()
                },
                sample: r.sample.clone(),
                vtype: r.vtype.clone().unwrap_or_default(),
                head,
                rd,
                wr,
                ox_key: r.ox_key.clone().unwrap_or_default(),
                wkey: r.wkey.clone().unwrap_or_default(),
                raw_read: r.read.clone(),
                raw_write: r.write.clone(),
                wq: r.write_quality.clone(),
            }
        })
        .collect();
    rows.sort_by(|a, b| (&a.group, &a.name).cmp(&(&b.group, &b.name)));

    let mut heads: BTreeMap<String, u32> = BTreeMap::new();
    for x in &rows {
        *heads.entry(x.head.clone()).or_insert(0) += 1;
    }
    let mut by_group: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for x in &rows {
        by_group.entry(x.group.clone()).or_default().push(x);
    }

    const READ_OKISH: &[&str] = &["OK", "MISMATCH_FORMAT"];
    let n_read = rows
        .iter()
        .filter(|x| {
            x.raw_read
                .as_deref()
                .map(|r| READ_OKISH.contains(&r))
                .unwrap_or(false)
        })
        .count();
    let n_write = rows
        .iter()
        .filter(|x| x.raw_write.as_deref() == Some("OK"))
        .count();

    // ---------------------------------------------------- supported mapping doc
    let mut md = vec![
        "# JPEG Tag Support\n".to_string(),
        autogen_callout(&work),
        "Empirical OxiDex ↔ ExifTool tag mapping for JPEG: for each tag, ExifTool writes a \
         sample into a clean JPEG and `oxidex -j -e` must read it back; writability additionally \
         requires `oxidex -KEY=VALUE` to round-trip through both OxiDex and ExifTool.\n"
            .to_string(),
        "Only tags OxiDex can **read** from JPEG are listed here (including those whose value \
         formatting differs from ExifTool). The full classification of all tested tags — \
         including unsupported and broken ones — is in the \
         [JPEG Tag Matrix](/reference/jpeg-tag-matrix). See also \
         [ExifTool Coverage](/reference/tag-coverage-analysis) for the tag-database view and the \
         [Compatibility overview](/reference/comparison/) for fixture-based comparisons across \
         formats.\n"
            .to_string(),
    ];
    md.push(format!(
        "\n**{n_read}** ExifTool tags readable, **{n_write}** writable via the CLI \
         (of {} ExifTool-writable JPEG tags tested).\n",
        rows.len()
    ));
    for (g, xs) in &by_group {
        let sup: Vec<&&Row> = xs
            .iter()
            .filter(|x| {
                x.raw_read
                    .as_deref()
                    .map(|r| READ_OKISH.contains(&r))
                    .unwrap_or(false)
            })
            .collect();
        if sup.is_empty() {
            continue;
        }
        md.push(format!("\n## {g} ({} readable tags)\n", sup.len()));
        md.push("| ExifTool tag | OxiDex key | OxiDex write | Example value |".to_string());
        md.push("|---|---|---|---|".to_string());
        for x in &sup {
            let wr_cell = if x.raw_write.as_deref() == Some("OK") {
                let mark = if x.wq.as_deref() == Some("nonstandard") {
                    "⚠️"
                } else {
                    "✅"
                };
                format!("{mark} `-{}=`", x.wkey)
            } else {
                "—".to_string()
            };
            let note = if x.raw_read.as_deref() == Some("MISMATCH_FORMAT") {
                " *"
            } else {
                ""
            };
            let ox_key_display = if x.ox_key.is_empty() {
                &x.key
            } else {
                &x.ox_key
            };
            md.push(format!(
                "| `{}` | `{ox_key_display}`{note} | {wr_cell} | `{}` |",
                x.key,
                md_escape(&x.sample, 60)
            ));
        }
    }
    md.push("\n\\* value formatting differs from ExifTool (same underlying value, missing PrintConv).\n".to_string());
    std::fs::write(&out_support, md.join("\n") + "\n")?;

    // ------------------------------------------------------- full matrix doc
    let mut md = vec![
        "---".to_string(),
        "outline: 2".to_string(),
        "---\n".to_string(),
        "# JPEG Tag Matrix\n".to_string(),
        autogen_callout(&work),
        "Every ExifTool-writable JPEG tag, classified by empirical test: read support \
         (ExifTool writes → OxiDex reads), write support (OxiDex writes → both read back), \
         and known bugs with root causes. Readable tags with their working write keys are \
         summarized in [JPEG Tag Support](/reference/jpeg-tag-support).\n"
            .to_string(),
        "\n## Summary\n".to_string(),
        "| Classification | Tags |".to_string(),
        "|---|---|".to_string(),
    ];
    let mut heads_sorted: Vec<(&String, &u32)> = heads.iter().collect();
    heads_sorted.sort_by(|a, b| b.1.cmp(a.1));
    for (h, n) in &heads_sorted {
        md.push(format!("| {h} | {n} |"));
    }
    let readonly_tags: Vec<ReadonlyTag> = if readonly_path.exists() {
        let ro: ReadonlyFile = serde_json::from_str(&std::fs::read_to_string(&readonly_path)?)?;
        md.push(format!(
            "| 🚫 Not writable in ExifTool (no synthetic sample possible; untested) | {} |",
            ro.tags.len()
        ));
        ro.tags
    } else {
        vec![]
    };

    md.push("\n## Per-group breakdown\n".to_string());
    md.push("| Group | Full | Read-only | Write-broken | Read-broken | Write-only | Unsupported | Untestable | Total |".to_string());
    md.push("|---|---|---|---|---|---|---|---|---|".to_string());
    for (g, xs) in &by_group {
        let mut c: BTreeMap<&str, u32> = BTreeMap::new();
        for x in xs {
            *c.entry(x.head.as_str()).or_insert(0) += 1;
        }
        md.push(format!(
            "| {g} | {} | {} | {} | {} | {} | {} | {} | {} |",
            c.get("✅ Full (read + write)").unwrap_or(&0),
            c.get("📖 Read only").unwrap_or(&0),
            c.get("🐛 Read OK, write broken").unwrap_or(&0),
            c.get("🐛 Read broken").unwrap_or(&0),
            c.get("✍️ Write only").unwrap_or(&0),
            c.get("❌ Unsupported").unwrap_or(&0),
            c.get("❔ Untestable").unwrap_or(&0),
            xs.len()
        ));
    }

    md.push(KNOWN_BUGS.to_string());

    md.push("\n## Full matrix\n".to_string());
    for (g, xs) in &by_group {
        md.push(format!("\n### {g}\n"));
        md.push("| ExifTool tag | Read | Write | Example |".to_string());
        md.push("|---|---|---|---|".to_string());
        for x in xs {
            md.push(format!(
                "| `{}` | {} | {} | `{}` |",
                x.name,
                x.rd,
                x.wr,
                md_escape(&x.sample, 60)
            ));
        }
    }
    if !readonly_tags.is_empty() {
        md.push("\n### Not writable in ExifTool (read-only universe, untested)\n".to_string());
        md.push(
            "These exist in JPEG-relevant groups but ExifTool itself cannot write them, \
             so no synthetic test file could be produced.\n"
                .to_string(),
        );
        let mut by_g: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for t in &readonly_tags {
            by_g.entry(&t.group).or_default().push(&t.name);
        }
        for (g, names) in &by_g {
            let mut sorted = names.clone();
            sorted.sort();
            let shown: Vec<String> = sorted.iter().take(40).map(|n| format!("`{n}`")).collect();
            let extra = if sorted.len() > 40 {
                format!(" … +{} more", sorted.len() - 40)
            } else {
                String::new()
            };
            md.push(format!(
                "- **{g}** ({}): {}{extra}",
                sorted.len(),
                shown.join(", ")
            ));
        }
    }
    std::fs::write(&out_matrix, md.join("\n") + "\n")?;

    println!(
        "Wrote {}\nWrote {}\n",
        out_support.display(),
        out_matrix.display()
    );
    for (h, n) in &heads_sorted {
        println!("  {h}: {n}");
    }

    // ------------------------------------------------------ baseline handling
    let counts = BaselineCounts {
        total_tested: rows.len() as u32,
        readable: n_read as u32,
        writable_cli: n_write as u32,
        full: *heads.get("✅ Full (read + write)").unwrap_or(&0),
        full_nonstandard: *heads
            .get("⚠️ Full (write non-standard encoding)")
            .unwrap_or(&0),
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
        if counts.readable < base.readable {
            failures.push(format!(
                "readable regressed: {} -> {}",
                base.readable, counts.readable
            ));
        }
        if counts.writable_cli < base.writable_cli {
            failures.push(format!(
                "writable_cli regressed: {} -> {}",
                base.writable_cli, counts.writable_cli
            ));
        }
        if counts.full < base.full {
            failures.push(format!("full regressed: {} -> {}", base.full, counts.full));
        }
        if counts.read_broken > base.read_broken {
            failures.push(format!(
                "read_broken regressed: {} -> {}",
                base.read_broken, counts.read_broken
            ));
        }
        if counts.write_broken > base.write_broken {
            failures.push(format!(
                "write_broken regressed: {} -> {}",
                base.write_broken, counts.write_broken
            ));
        }

        print_deltas(&base, &counts);

        if !failures.is_empty() {
            println!("\nBASELINE REGRESSION:");
            for f in &failures {
                println!("  ✗ {f}");
            }
            println!(
                "If intentional, rerun with --update-baseline and commit jpeg-tag-baseline.json."
            );
            std::process::exit(1);
        }
        println!("Baseline check passed");
    }
    Ok(())
}

fn print_deltas(base: &BaselineCounts, counts: &BaselineCounts) {
    macro_rules! delta {
        ($field:ident) => {
            if base.$field != counts.$field {
                println!(
                    "  delta {}: {} -> {}",
                    stringify!($field),
                    base.$field,
                    counts.$field
                );
            }
        };
    }
    delta!(total_tested);
    delta!(readable);
    delta!(writable_cli);
    delta!(full);
    delta!(full_nonstandard);
    delta!(read_only);
    delta!(read_broken);
    delta!(write_broken);
    delta!(unsupported);
    delta!(untestable);
}

/// serde_json's `to_string_pretty` defaults to 2-space indent; the committed
/// `jpeg-tag-baseline.json` uses 1-space (Python's `json.dumps(counts, indent=1)`).
fn write_baseline_one_space_indent(path: &Path, counts: &BaselineCounts) -> anyhow::Result<()> {
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    counts.serialize(&mut ser)?;
    buf.push(b'\n');
    std::fs::write(path, buf)?;
    Ok(())
}

#[cfg(test)]
mod classify_tests {
    use super::*;

    fn entry(read: &str, write: &str) -> ResultEntry {
        ResultEntry {
            read: Some(read.into()),
            write: Some(write.into()),
            ..Default::default()
        }
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
        assert_eq!(
            wr,
            "⚠️ writes, but non-standard encoding (exiftool tolerates)"
        );
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

#[cfg(test)]
mod report_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn report_end_to_end_produces_matching_baseline_shape() {
        let td = tempfile::tempdir().unwrap();
        // SAFETY: test-only. Neither var is read by any other test in this
        // binary (matrix.rs/manifest.rs tests call their helper functions
        // directly, never their top-level `run()`, so they never observe
        // these), so setting them here cannot race with concurrently-running
        // tests. Deliberately NOT using `std::env::set_current_dir`: that
        // mutates process-wide CWD, which matrix.rs's write-phase tests
        // resolve their fixture scripts against via a relative path, and
        // does race across parallel test threads (see the comment on
        // `TAGMATRIX_REPO` in `run()` above for how this was discovered).
        unsafe {
            std::env::set_var("TAGMATRIX_WORK", td.path());
            std::env::set_var("TAGMATRIX_REPO", td.path());
        }
        let mut results = HashMap::new();
        results.insert(
            "ExifIFD:ISO".to_string(),
            ResultEntry {
                group: "ExifIFD".into(),
                name: "ISO".into(),
                sample: "200".into(),
                vtype: Some("int16u".into()),
                read: Some("OK".into()),
                write: Some("OK".into()),
                ..Default::default()
            },
        );
        std::fs::write(
            td.path().join("results.json"),
            serde_json::to_string(&results).unwrap(),
        )
        .unwrap();

        let repo_docs = td.path().join("docs/reference");
        std::fs::create_dir_all(&repo_docs).unwrap();

        run(ReportArgs {
            update_baseline: true,
            check_baseline: false,
        })
        .unwrap();

        let baseline = std::fs::read_to_string(repo_docs.join("jpeg-tag-baseline.json")).unwrap();
        assert!(baseline.starts_with("{\n \"total_tested\": 1"));
    }
}
