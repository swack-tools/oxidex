//! A generated, oracle-graded matrix of request orderings (#957 round 5).
//!
//! Five rounds of Codex review on #957 found request-ordering and
//! interaction bugs in `core::write_transaction` one at a time: a later
//! `-all=` against an earlier set, a group deletion against an earlier or
//! later set (empty group, non-empty group, covering or not), a PDF Info
//! field's two spellings in call order, a same-value set beside a no-op
//! deletion. This test closes the class by enumerating the orderings.
//!
//! Every case is a sequence of two or three operations -- a set, a
//! same-value set, a deletion, a `<group>:All` deletion (EXIF, IFD0, IFD1,
//! GPS, XMP, IPTC, PNG, PDF, `-all=`), a PDF alias spelling -- over one of
//! five files (JPEG with EXIF and XMP, JPEG without GPS, PNG, PDF, TIFF). It
//! is run both as one CLI command in argument order and as FFI calls on one
//! handle in call order, and graded against pinned ExifTool 13.59 running
//! the same operations as one command (`exiftool_oracle::graded()`):
//!
//! * the final tags: the oracle's `-a -G1 -s` read of both output files
//!   (so reader differences cannot count), without the `System`, `File`,
//!   `ExifTool` and `Composite` rows that describe the file, not its
//!   metadata;
//! * the outcome: updated or unchanged;
//! * whether the write was refused.
//!
//! A case is a **match** when all three agree. A case oxidex refuses where
//! ExifTool writes is a **named refusal** only if the refusal is typed and
//! names the tag -- the CLI's `... tag '<TAG>' ...` error and an untouched
//! file; the C ABI's `EXIFTOOL_ERR_TAG_NOT_WRITTEN` with the tag list --
//! and its reason is one the table below lists. Anything else is a
//! **mismatch**, and the test fails on the first.
//!
//! `OXIDEX_ORDER_MATRIX_REPORT=<path>` writes every case's verdict as JSON.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::exiftool_oracle::{self, Oracle};
use oxidex::ffi::{
    EXIFTOOL_ERR_TAG_NOT_WRITTEN, EXIFTOOL_OK, EXIFTOOL_WRITE_UPDATED, exiftool_create,
    exiftool_destroy, exiftool_get_last_error, exiftool_get_last_error_tag,
    exiftool_get_last_error_tag_count, exiftool_read_file, exiftool_remove_tag,
    exiftool_set_tag_string, exiftool_write_file_with_outcome,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, CString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// `[IFD0]` Make/Model and XMP (`XMP-dc:Title`), no ExifIFD, no GPS.
const JPEG_EXIF_XMP: &str = "tests/fixtures/jpeg/sample_with_exif_xmp.jpg";
/// `[IFD0]` Make/Model/Artist, `[ExifIFD]`, JFIF; no GPS, no XMP.
const JPEG_NO_GPS: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
/// IHDR/text rows plus an EXIF block (`[IFD0] Artist: PNG Artist 1`).
const PNG: &str = "tests/fixtures/png/sample.png";
/// Info dictionary with Title and both dates.
const PDF: &str = "tests/fixtures/pdf/sample.pdf";
/// IFD0 Make, ExifIFD, IFD1 Model.
const TIFF: &str = "tests/fixtures/tiff/sample.tif";

/// Reasons oxidex refuses what 13.59 writes, each by name: every refusal
/// in the matrix must carry one of these (a substring of its message), be
/// typed, leave the file untouched, and name no request that a later
/// deletion cancels in 13.59.
const NAMED_REFUSALS: &[(&str, &str)] = &[
    (
        "cannot write the XMP",
        "oxidex writes no XMP packet (JPEG, PNG, PDF, TIFF)",
    ),
    ("cannot write the IPTC", "oxidex writes no IPTC record"),
    (
        "cannot write the IFD1 group",
        "oxidex's JPEG and TIFF writers do not create or edit IFD1 entries",
    ),
    (
        "does not delete a whole XMP group",
        "oxidex cannot delete an XMP packet the file holds",
    ),
    (
        "does not delete a whole PNG group",
        "oxidex cannot delete the PNG group (every PNG holds IHDR rows)",
    ),
    (
        "does not delete a whole PDF group",
        "oxidex cannot delete the PDF Info group",
    ),
    (
        "cannot shrink an IFD table",
        "the in-place TIFF writer cannot remove an entry from an IFD",
    ),
    (
        "cannot delete a directory",
        "the in-place TIFF writer cannot delete an IFD",
    ),
    (
        "Clearing all metadata from a TIFF-structured file",
        "-all= on TIFF: the in-place TIFF writer never rebuilds the file",
    ),
];

/// EXIF directories (family 1) whose tags have family-0 group `EXIF`.
const EXIF_FAMILY: &[&str] = &[
    "ifd0",
    "ifd1",
    "exififd",
    "gps",
    "interopifd",
    "subifd",
    "exif",
];

/// Whether 13.59's `-<group>:All=` (or `-all=`) removes a value set earlier
/// in `tag_group` (Writer.pl `SetNewValue` -> `RemoveNewValuesForGroup`,
/// with its `%removeGroups`: IFD0 also removes EXIF and MakerNotes values,
/// ExifIFD also MakerNotes and InteropIFD; XMP every XMP-* namespace).
fn cancels(group: &str, tag_group: &str) -> bool {
    let (g, t) = (group.to_ascii_lowercase(), tag_group.to_ascii_lowercase());
    g == "all"
        || g == t
        || ((g == "exif" || g == "ifd0") && EXIF_FAMILY.contains(&t.as_str()))
        || (g == "exififd" && t == "interopifd")
        || (g == "xmp" && t.starts_with("xmp-"))
}

#[derive(Clone)]
enum Call {
    Set(&'static str, String),
    Remove(&'static str),
}

/// One operation: its CLI argument (`None`: not a CLI request), its C ABI
/// call (`None`: no FFI equivalent, `-all=`), and the oracle's argument.
#[derive(Clone)]
struct Op {
    cli: Option<String>,
    ffi: Option<Call>,
    oracle: String,
}

impl Op {
    /// The tag this operation names (`all` for `-all=`).
    fn tag(&self) -> &str {
        match (&self.ffi, &self.cli) {
            (Some(Call::Set(tag, _) | Call::Remove(tag)), _) => tag,
            (None, _) => "all",
        }
    }

    /// The group this operation deletes, if it is a group deletion.
    fn deleted_group(&self) -> Option<&str> {
        let tag = self.tag();
        if tag == "all" {
            return Some("all");
        }
        match &self.ffi {
            Some(Call::Remove(_)) => tag
                .rsplit_once(':')
                .filter(|(_, name)| name.eq_ignore_ascii_case("all"))
                .map(|(group, _)| group),
            _ => None,
        }
    }
}

fn set(tag: &'static str, value: &str) -> Op {
    Op {
        cli: Some(format!("-{tag}={value}")),
        ffi: Some(Call::Set(tag, value.to_string())),
        oracle: format!("-{tag}={value}"),
    }
}

fn delete(tag: &'static str) -> Op {
    Op {
        cli: Some(format!("-{tag}=")),
        ffi: Some(Call::Remove(tag)),
        oracle: format!("-{tag}="),
    }
}

/// A group deletion; `-all=` has no C ABI call.
fn group(group: &'static str) -> Op {
    if group == "all" {
        return Op {
            cli: Some("-all=".into()),
            ffi: None,
            oracle: "-all=".into(),
        };
    }
    let tag: &'static str = Box::leak(format!("{group}:All").into_boxed_str());
    delete(tag)
}

/// A removal of the reader's other spelling of a PDF Info date, which only
/// the map API can name (13.59 knows no `PDF:CreationDate` tag): the oracle
/// deletes the field by its tag name.
fn remove_alias(alias: &'static str, field: &'static str) -> Op {
    Op {
        cli: None,
        ffi: Some(Call::Remove(alias)),
        oracle: format!("-{field}="),
    }
}

struct Fixture {
    path: &'static str,
    fields: Vec<Op>,
    groups: Vec<&'static str>,
    extra: Vec<Vec<Op>>,
}

fn fixtures() -> Vec<Fixture> {
    let date = "2020:01:02 03:04:05";
    vec![
        Fixture {
            path: JPEG_EXIF_XMP,
            fields: vec![
                set("IFD0:Artist", "x"),
                set("IFD0:Make", "TestCamera"),
                delete("IFD0:Model"),
                set("XMP-dc:Title", "x"),
                delete("XMP-dc:Title"),
            ],
            groups: vec!["EXIF", "IFD0", "IFD1", "GPS", "XMP", "IPTC", "all"],
            extra: vec![],
        },
        Fixture {
            path: JPEG_NO_GPS,
            fields: vec![
                set("IFD0:Artist", "x"),
                set("IFD0:Artist", "Synthetic Artist 1"),
                delete("IFD0:Artist"),
                set("IFD1:Make", "x"),
                set("ExifIFD:LensModel", "L1"),
            ],
            groups: vec!["EXIF", "IFD0", "IFD1", "GPS", "XMP", "IPTC", "all"],
            extra: vec![
                vec![
                    set("IFD0:Artist", "x"),
                    group("EXIF"),
                    set("ExifIFD:LensModel", "L1"),
                ],
                vec![set("IFD0:Artist", "x"), group("GPS"), delete("IFD0:Artist")],
                vec![
                    delete("IFD0:Artist"),
                    group("EXIF"),
                    set("IFD0:Artist", "y"),
                ],
                vec![
                    set("IFD1:Make", "x"),
                    group("IFD1"),
                    set("IFD0:Artist", "y"),
                ],
                vec![group("EXIF"), set("IFD0:Artist", "x"), group("IFD0")],
                vec![
                    set("IFD0:Artist", "Synthetic Artist 1"),
                    group("GPS"),
                    group("XMP"),
                ],
            ],
        },
        Fixture {
            path: PNG,
            fields: vec![
                set("IFD0:Artist", "x"),
                set("IFD0:Artist", "PNG Artist 1"),
                delete("IFD0:Artist"),
                set("PNG:Title", "x"),
            ],
            groups: vec!["EXIF", "IFD0", "GPS", "XMP", "PNG", "all"],
            extra: vec![],
        },
        Fixture {
            path: PDF,
            fields: vec![
                set("PDF:Title", "x"),
                set("PDF:Title", "Sample PDF for Testing"),
                delete("PDF:Title"),
                set("PDF:CreateDate", date),
                delete("PDF:CreateDate"),
            ],
            groups: vec!["EXIF", "XMP", "PDF", "all"],
            extra: vec![
                vec![
                    set("PDF:CreateDate", date),
                    remove_alias("PDF:CreationDate", "PDF:CreateDate"),
                ],
                vec![
                    remove_alias("PDF:CreationDate", "PDF:CreateDate"),
                    set("PDF:CreateDate", date),
                ],
                vec![
                    set("PDF:ModifyDate", date),
                    remove_alias("PDF:ModDate", "PDF:ModifyDate"),
                ],
                vec![
                    remove_alias("PDF:ModDate", "PDF:ModifyDate"),
                    set("PDF:ModifyDate", date),
                ],
                vec![set("PDF:CreateDate", date), delete("PDF:CreateDate")],
                vec![delete("PDF:CreateDate"), set("PDF:CreateDate", date)],
                vec![remove_alias("PDF:CreationDate", "PDF:CreateDate")],
                vec![set("PDF:Title", "x"), group("EXIF"), set("PDF:Author", "y")],
            ],
        },
        Fixture {
            path: TIFF,
            fields: vec![
                set("IFD0:Artist", "x"),
                set("IFD0:Make", "TestCamera"),
                delete("IFD0:Make"),
                set("IFD1:Model", "x"),
                set("XMP-dc:Title", "x"),
            ],
            groups: vec!["EXIF", "IFD0", "IFD1", "GPS", "XMP", "IPTC", "all"],
            extra: vec![],
        },
    ]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Cli,
    Ffi,
}

struct Case {
    fixture: &'static str,
    ops: Vec<Op>,
}

impl Case {
    fn label(&self, mode: Mode) -> String {
        let ops: Vec<String> = self
            .ops
            .iter()
            .map(|op| match (mode, &op.cli, &op.ffi) {
                (Mode::Cli, Some(cli), _) => cli.clone(),
                (Mode::Ffi, _, Some(Call::Set(tag, value))) => format!("set({tag},{value})"),
                (Mode::Ffi, _, Some(Call::Remove(tag))) => format!("remove({tag})"),
                _ => "?".into(),
            })
            .collect();
        format!(
            "{mode:?} {} [{}]",
            Path::new(self.fixture)
                .file_name()
                .unwrap()
                .to_string_lossy(),
            ops.join(" ")
        )
    }

    /// The first tag in `named` whose request 13.59 never performs: a later
    /// group deletion cancels it.
    fn cancelled(&self, named: &[String]) -> Option<String> {
        named.iter().find_map(|tag| {
            let at = self
                .ops
                .iter()
                .position(|op| op.tag().eq_ignore_ascii_case(tag))?;
            let (group, _) = tag.rsplit_once(':')?;
            self.ops[at + 1..]
                .iter()
                .filter_map(Op::deleted_group)
                .any(|deleted| cancels(deleted, group))
                .then(|| tag.clone())
        })
    }

    fn runs_in(&self, mode: Mode) -> bool {
        self.ops.iter().all(|op| match mode {
            Mode::Cli => op.cli.is_some(),
            Mode::Ffi => op.ffi.is_some(),
        })
    }
}

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for fixture in fixtures() {
        for field in &fixture.fields {
            for name in &fixture.groups {
                cases.push(Case {
                    fixture: fixture.path,
                    ops: vec![field.clone(), group(name)],
                });
                cases.push(Case {
                    fixture: fixture.path,
                    ops: vec![group(name), field.clone()],
                });
            }
        }
        for ops in fixture.extra {
            cases.push(Case {
                fixture: fixture.path,
                ops,
            });
        }
    }
    cases
}

/// What one write did, as the tool reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Updated,
    Unchanged,
    Refused(String),
}

fn cli_outcome(status: Option<i32>, stdout: &str, stderr: &str) -> Outcome {
    let count = |what: &str| {
        stdout
            .lines()
            .find(|line| line.trim_end().ends_with(what))
            .and_then(|line| line.split_whitespace().next()?.parse::<u32>().ok())
            .unwrap_or(0)
    };
    if status != Some(0) || count("image files updated") + count("image files unchanged") == 0 {
        return Outcome::Refused(stderr.trim().to_string());
    }
    if count("image files updated") > 0 {
        Outcome::Updated
    } else {
        Outcome::Unchanged
    }
}

fn sha(path: &Path) -> Vec<u8> {
    Sha256::digest(fs::read(path).unwrap()).to_vec()
}

fn run_ffi(ops: &[Op], path: &Path) -> (Outcome, bool, Vec<String>) {
    let c_path = CString::new(path.to_str().unwrap()).unwrap();
    let handle = exiftool_create();
    assert_eq!(exiftool_read_file(handle, c_path.as_ptr()), EXIFTOOL_OK);
    for op in ops {
        match op.ffi.as_ref().unwrap() {
            Call::Set(tag, value) => {
                let (tag, value) = (
                    CString::new(*tag).unwrap(),
                    CString::new(value.as_str()).unwrap(),
                );
                exiftool_set_tag_string(handle, tag.as_ptr(), value.as_ptr());
            }
            Call::Remove(tag) => {
                let tag = CString::new(*tag).unwrap();
                exiftool_remove_tag(handle, tag.as_ptr());
            }
        }
    }
    let mut written = 0;
    let code = exiftool_write_file_with_outcome(handle, c_path.as_ptr(), &mut written);
    let outcome = if code == EXIFTOOL_OK {
        if written == EXIFTOOL_WRITE_UPDATED {
            Outcome::Updated
        } else {
            Outcome::Unchanged
        }
    } else {
        let error = exiftool_get_last_error();
        let message = if error.is_null() {
            String::new()
        } else {
            // SAFETY: a non-NULL last error is a NUL-terminated string the
            // C ABI owns until the next call on this thread.
            unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned()
        };
        Outcome::Refused(format!("code {code}: {message}"))
    };
    let typed = code == EXIFTOOL_ERR_TAG_NOT_WRITTEN && exiftool_get_last_error_tag_count() > 0;
    let named = (0..exiftool_get_last_error_tag_count())
        .filter_map(|index| {
            let tag = exiftool_get_last_error_tag(index);
            // SAFETY: a non-NULL tag is a NUL-terminated string the C ABI
            // owns until the next call on this thread.
            (!tag.is_null()).then(|| {
                unsafe { CStr::from_ptr(tag) }
                    .to_string_lossy()
                    .into_owned()
            })
        })
        .collect();
    exiftool_destroy(handle);
    (outcome, typed, named)
}

/// The tags a CLI error names (`... tag 'IFD1:Make' ...`).
fn cli_named(stderr: &str) -> Vec<String> {
    stderr
        .split("tag '")
        .skip(1)
        .filter_map(|rest| rest.split_once('\'').map(|(tag, _)| tag.to_string()))
        .collect()
}

/// One of oxidex's runs of a case.
struct Ours {
    mode: Mode,
    outcome: Outcome,
    /// A typed refusal: the C ABI's `EXIFTOOL_ERR_TAG_NOT_WRITTEN` with its
    /// tag list, or a CLI error naming the tag (or `-all=`'s own refusal).
    typed: bool,
    /// The tags the refusal names.
    named: Vec<String>,
    path: PathBuf,
    before: Vec<u8>,
}

struct Written {
    oracle: (Outcome, PathBuf),
    ours: Vec<Ours>,
}

fn write_case(oracle: &Oracle, case: &Case, dir: &Path) -> Written {
    let ext = Path::new(case.fixture)
        .extension()
        .unwrap()
        .to_str()
        .unwrap();
    let copy = |name: &str| {
        let path = dir.join(format!("{name}.{ext}"));
        fs::copy(case.fixture, &path).unwrap();
        path
    };
    let theirs = copy("theirs");
    let o = oracle
        .command()
        .args(case.ops.iter().map(|op| op.oracle.as_str()))
        .arg(&theirs)
        .output()
        .unwrap();
    let oracle_outcome = cli_outcome(
        o.status.code(),
        &String::from_utf8_lossy(&o.stdout),
        &String::from_utf8_lossy(&o.stderr),
    );
    let mut ours = Vec::new();
    if case.runs_in(Mode::Cli) {
        let path = copy("cli");
        let before = sha(&path);
        let o = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(case.ops.iter().map(|op| op.cli.clone().unwrap()))
            .arg(&path)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
        let outcome = cli_outcome(
            o.status.code(),
            &String::from_utf8_lossy(&o.stdout),
            &stderr,
        );
        let named = cli_named(&stderr);
        let clears = case.ops.iter().any(|op| op.tag() == "all");
        let typed = !named.is_empty() || (clears && stderr.contains("Failed to clear metadata"));
        ours.push(Ours {
            mode: Mode::Cli,
            outcome,
            typed,
            named,
            path,
            before,
        });
    }
    if case.runs_in(Mode::Ffi) {
        let path = copy("ffi");
        let before = sha(&path);
        let (outcome, typed, named) = run_ffi(&case.ops, &path);
        ours.push(Ours {
            mode: Mode::Ffi,
            outcome,
            typed,
            named,
            path,
            before,
        });
    }
    Written {
        oracle: (oracle_outcome, theirs),
        ours,
    }
}

/// The oracle's `-a -G1 -s` rows of each file, without the rows that
/// describe the file rather than its metadata.
fn oracle_rows(oracle: &Oracle, files: &[PathBuf]) -> BTreeMap<PathBuf, BTreeSet<String>> {
    let mut rows = BTreeMap::new();
    for chunk in files.chunks(120) {
        let o = oracle
            .command()
            .args(["-a", "-G1", "-s"])
            .args(chunk)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        let mut current: Option<PathBuf> = if chunk.len() == 1 {
            Some(chunk[0].clone())
        } else {
            None
        };
        for line in text.lines() {
            if let Some(name) = line.strip_prefix("======== ") {
                current = Some(PathBuf::from(name));
                continue;
            }
            let Some(file) = current.clone() else {
                continue;
            };
            let group = line
                .strip_prefix('[')
                .and_then(|rest| rest.split_once(']'))
                .map(|(group, _)| group);
            match group {
                Some("System" | "File" | "ExifTool" | "Composite") | None => {}
                Some(_) => {
                    let normalized: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
                    rows.entry(file)
                        .or_insert_with(BTreeSet::new)
                        .insert(normalized);
                }
            }
        }
        for file in chunk {
            rows.entry(file.clone()).or_insert_with(BTreeSet::new);
        }
    }
    rows
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Verdict {
    Match,
    NamedRefusal,
    Mismatch,
}

#[test]
fn request_orderings_match_pinned_exiftool() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let cases = cases();
    let root = tempfile::tempdir().unwrap();
    let written: Vec<Mutex<Option<Written>>> = cases.iter().map(|_| Mutex::new(None)).collect();
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get().min(6));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(case) = cases.get(index) else { break };
                    let dir = root.path().join(index.to_string());
                    fs::create_dir(&dir).unwrap();
                    *written[index].lock().unwrap() = Some(write_case(oracle, case, &dir));
                }
            });
        }
    });
    let written: Vec<Written> = written
        .into_iter()
        .map(|slot| slot.into_inner().unwrap().unwrap())
        .collect();
    let mut files: Vec<PathBuf> = written
        .iter()
        .flat_map(|w| {
            std::iter::once(w.oracle.1.clone()).chain(w.ours.iter().map(|o| o.path.clone()))
        })
        .collect();
    // The fixtures themselves, for the C ABI's outcome (below).
    let originals: BTreeSet<PathBuf> = cases
        .iter()
        .map(|case| fs::canonicalize(case.fixture).unwrap())
        .collect();
    files.extend(originals.iter().cloned());
    let rows = oracle_rows(oracle, &files);

    let mut verdicts: Vec<(String, Verdict, String)> = Vec::new();
    for (case, written) in cases.iter().zip(&written) {
        let (their_outcome, theirs) = &written.oracle;
        for ours in &written.ours {
            let label = case.label(ours.mode);
            // The C ABI's outcome is whether the file's bytes changed
            // (ExifTool WriteInfo's 1 / 2 by the bytes, the approved library
            // contract), where the CLI reports 13.59's count (a same-value
            // set is `1 image files updated`). So the C ABI is graded by the
            // tags: where 13.59 changed them it must say `UPDATED`; where it
            // did not (a same-value set, a no-op deletion), what it says must
            // be true of its own bytes -- a same-value set rewrites nothing in
            // a JPEG but appends a PDF revision, as 13.59's does, while 13.59
            // may also re-lay out a JPEG or TIFF the tags of which it leaves
            // alone.
            let original = &rows[&fs::canonicalize(case.fixture).unwrap()];
            let changed_bytes = sha(&ours.path) != ours.before;
            let expected = match (ours.mode, their_outcome) {
                (Mode::Ffi, Outcome::Updated | Outcome::Unchanged) => {
                    if rows[theirs] != *original || changed_bytes {
                        Outcome::Updated
                    } else {
                        Outcome::Unchanged
                    }
                }
                (_, outcome) => outcome.clone(),
            };
            let untouched = sha(&ours.path) == ours.before;
            let (verdict, detail) = match (&ours.outcome, &expected) {
                (Outcome::Refused(why), Outcome::Refused(_)) => {
                    if untouched {
                        (Verdict::Match, format!("both refused: {why}"))
                    } else {
                        (
                            Verdict::Mismatch,
                            format!("refused but changed the file: {why}"),
                        )
                    }
                }
                (Outcome::Refused(why), _) => {
                    let named = NAMED_REFUSALS
                        .iter()
                        .find(|(needle, _)| why.contains(needle));
                    if !ours.typed {
                        (Verdict::Mismatch, format!("untyped refusal: {why}"))
                    } else if !untouched {
                        (
                            Verdict::Mismatch,
                            format!("refused but changed the file: {why}"),
                        )
                    } else if let Some(tag) = case.cancelled(&ours.named) {
                        (
                            Verdict::Mismatch,
                            format!(
                                "refused {tag}, which a later deletion cancels in 13.59: {why}"
                            ),
                        )
                    } else if let Some((_, reason)) = named {
                        (Verdict::NamedRefusal, format!("{reason}: {why}"))
                    } else {
                        (Verdict::Mismatch, format!("unlisted refusal: {why}"))
                    }
                }
                (_, Outcome::Refused(why)) => (
                    Verdict::Mismatch,
                    format!("13.59 refuses ({why}) but oxidex wrote"),
                ),
                (outcome, expected) => {
                    let (ours_rows, their_rows) = (&rows[&ours.path], &rows[theirs]);
                    if outcome != expected {
                        (
                            Verdict::Mismatch,
                            format!("outcome {outcome:?}, expected {expected:?}"),
                        )
                    } else if ours_rows != their_rows {
                        let only_ours: Vec<_> = ours_rows.difference(their_rows).collect();
                        let only_theirs: Vec<_> = their_rows.difference(ours_rows).collect();
                        (
                            Verdict::Mismatch,
                            format!(
                                "tags differ: oxidex only {only_ours:?}; 13.59 only {only_theirs:?}"
                            ),
                        )
                    } else {
                        (Verdict::Match, String::new())
                    }
                }
            };
            verdicts.push((label, verdict, detail));
        }
    }

    let count = |v: Verdict| {
        verdicts
            .iter()
            .filter(|(_, verdict, _)| *verdict == v)
            .count()
    };
    let (matched, refused, mismatched) = (
        count(Verdict::Match),
        count(Verdict::NamedRefusal),
        count(Verdict::Mismatch),
    );
    eprintln!(
        "request order matrix: {} cases ({} oracle runs): {matched} match, {refused} named \
         refusal, {mismatched} mismatch",
        verdicts.len(),
        cases.len()
    );
    let mut reasons: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, verdict, detail) in &verdicts {
        if *verdict == Verdict::NamedRefusal {
            *reasons
                .entry(detail.split(": ").next().unwrap())
                .or_default() += 1;
        }
    }
    for (reason, n) in &reasons {
        eprintln!("  named refusal x{n}: {reason}");
    }
    for (label, verdict, detail) in &verdicts {
        if *verdict == Verdict::Mismatch {
            eprintln!("  MISMATCH {label}: {detail}");
        }
    }
    if let Some(report) = std::env::var_os("OXIDEX_ORDER_MATRIX_REPORT") {
        let json: Vec<serde_json::Value> = verdicts
            .iter()
            .map(|(label, verdict, detail)| {
                serde_json::json!({"case": label, "verdict": format!("{verdict:?}"), "detail": detail})
            })
            .collect();
        fs::write(
            report,
            serde_json::to_string_pretty(&serde_json::json!({
                "cases": verdicts.len(), "match": matched, "named_refusal": refused,
                "mismatch": mismatched, "verdicts": json,
            }))
            .unwrap(),
        )
        .unwrap();
    }
    assert!(verdicts.len() >= 150, "only {} cases", verdicts.len());
    assert_eq!(
        mismatched, 0,
        "{mismatched} orderings differ from pinned 13.59 (listed above)"
    );
}

/// Counts one CLI write run reported: (updated, unchanged, errors).
fn cli_counts(stdout: &str) -> (u32, u32, u32) {
    let count = |what: &str| {
        stdout
            .lines()
            .find(|line| line.trim_end().ends_with(what))
            .and_then(|line| line.split_whitespace().next()?.parse::<u32>().ok())
            .unwrap_or(0)
    };
    (
        count("image files updated"),
        count("image files unchanged"),
        count("files weren't updated due to errors"),
    )
}

/// One CLI path's run of a case: the counts it printed, its exit code, and
/// the bytes each file ended with.
#[derive(Debug, PartialEq)]
struct PathRun {
    counts: (u32, u32, u32),
    exit: Option<i32>,
    files: Vec<Vec<u8>>,
}

fn run_oxidex(args: &[String], targets: &[PathBuf], files: &[PathBuf]) -> PathRun {
    let o = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .args(targets)
        .output()
        .unwrap();
    PathRun {
        counts: cli_counts(&String::from_utf8_lossy(&o.stdout)),
        exit: o.status.code(),
        files: files.iter().map(|file| fs::read(file).unwrap()).collect(),
    }
}

/// The same requests through the CLI's three write paths -- one file
/// (`write_plan_file`), several files (`batch_write` over the list) and a
/// directory (`batch_process`) -- give every file the same outcome: the
/// same bytes, and the per-file count the single-file run reports, times
/// the number of files. Graded against pinned 13.59 running the same
/// command over the same two files and the same directory (its counts; the
/// tags themselves are graded by `request_orderings_match_pinned_exiftool`),
/// except where oxidex refuses a request by name there.
///
/// The cases: every CLI case of the matrix that the list and directory
/// paths take (`-all=` is a single-file or file-list write only), and
/// defined sets beside an undefined name, which ExifTool warns about and
/// drops (#957, PRRT_kwDOQNbr5M6mR8dA).
#[test]
fn single_file_list_and_directory_writes_agree() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let mut cases: Vec<(&'static str, Vec<String>)> = cases()
        .into_iter()
        .filter(|case| case.runs_in(Mode::Cli) && !case.ops.iter().any(|op| op.tag() == "all"))
        .map(|case| {
            let args = case.ops.iter().map(|op| op.cli.clone().unwrap()).collect();
            (case.fixture, args)
        })
        .collect();
    for (fixture, set) in [
        (JPEG_NO_GPS, "-IFD0:Artist=x"),
        (JPEG_EXIF_XMP, "-IFD0:Artist=x"),
        (PNG, "-IFD0:Artist=x"),
        (PDF, "-PDF:Title=x"),
        (TIFF, "-IFD0:Artist=x"),
    ] {
        for args in [
            vec![set.to_string(), "-NoSuchTag=y".to_string()],
            vec!["-NoSuchTag=y".to_string(), set.to_string()],
            vec![
                set.to_string(),
                "-NoSuchTag=".to_string(),
                "-GPS:All=".to_string(),
            ],
        ] {
            cases.push((fixture, args));
        }
    }
    let root = tempfile::tempdir().unwrap();
    let mismatches = Mutex::new(Vec::new());
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get().min(6));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some((fixture, args)) = cases.get(index) else {
                        break;
                    };
                    let ext = Path::new(fixture).extension().unwrap().to_str().unwrap();
                    let base = root.path().join(index.to_string());
                    let place = |dir: &str, names: &[&str]| -> Vec<PathBuf> {
                        let dir = base.join(dir);
                        fs::create_dir_all(&dir).unwrap();
                        names
                            .iter()
                            .map(|name| {
                                let path = dir.join(format!("{name}.{ext}"));
                                fs::copy(fixture, &path).unwrap();
                                path
                            })
                            .collect()
                    };
                    let single = place("single", &["a"]);
                    let listed = place("list", &["a", "b"]);
                    let walked = place("dir", &["a"]);
                    let single = run_oxidex(args, &single, &single);
                    let listed = run_oxidex(args, &listed, &listed);
                    let walked = run_oxidex(args, &[base.join("dir")], &walked);
                    let label = format!(
                        "{} {args:?}",
                        Path::new(fixture).file_name().unwrap().to_string_lossy()
                    );
                    let mut problems = Vec::new();
                    let times = |(u, n, e): (u32, u32, u32), k: u32| (u * k, n * k, e * k);
                    let bytes = &single.files[0];
                    if listed.files.iter().any(|file| file != bytes) {
                        problems.push("file-list bytes differ from the single-file write".into());
                    }
                    if walked.files[0] != *bytes {
                        problems.push("directory bytes differ from the single-file write".into());
                    }
                    // A single-file refusal is an error count of one in a batch.
                    let per_file = match single.counts {
                        (0, 0, 0) if single.exit != Some(0) => (0, 0, 1),
                        counts => counts,
                    };
                    if listed.counts != times(per_file, 2) || listed.exit != single.exit {
                        problems.push(format!(
                            "file list {:?} exit {:?}; single file {:?} exit {:?}",
                            listed.counts, listed.exit, single.counts, single.exit
                        ));
                    }
                    if walked.counts != per_file || walked.exit != single.exit {
                        problems.push(format!(
                            "directory {:?} exit {:?}; single file {:?} exit {:?}",
                            walked.counts, walked.exit, single.counts, single.exit
                        ));
                    }
                    // The oracle's counts over the same list and directory,
                    // where oxidex wrote (its refusals are graded above).
                    if per_file.2 == 0 {
                        let theirs = place("theirs-list", &["a", "b"]);
                        let o = oracle
                            .command()
                            .arg("-overwrite_original")
                            .args(args)
                            .args(&theirs)
                            .output()
                            .unwrap();
                        let counts = cli_counts(&String::from_utf8_lossy(&o.stdout));
                        if counts != listed.counts {
                            problems
                                .push(format!("file list {:?}, 13.59 {counts:?}", listed.counts));
                        }
                        place("theirs-dir", &["a"]);
                        let o = oracle
                            .command()
                            .arg("-overwrite_original")
                            .args(args)
                            .arg(base.join("theirs-dir"))
                            .output()
                            .unwrap();
                        let counts = cli_counts(&String::from_utf8_lossy(&o.stdout));
                        if counts != walked.counts {
                            problems
                                .push(format!("directory {:?}, 13.59 {counts:?}", walked.counts));
                        }
                    }
                    if !problems.is_empty() {
                        mismatches
                            .lock()
                            .unwrap()
                            .push(format!("{label}: {}", problems.join("; ")));
                    }
                }
            });
        }
    });
    let mismatches = mismatches.into_inner().unwrap();
    eprintln!(
        "write paths: {} cases x 3 CLI paths: {} agree, {} differ",
        cases.len(),
        cases.len() - mismatches.len(),
        mismatches.len()
    );
    for mismatch in &mismatches {
        eprintln!("  PATHS DIFFER {mismatch}");
    }
    assert!(cases.len() >= 150, "only {} cases", cases.len());
    assert!(
        mismatches.is_empty(),
        "{} cases differ by write path",
        mismatches.len()
    );
}

/// The tag a CLI argument names (`-EXIF:All=` -> `EXIF:All`, `-AllDates+=1`
/// -> `AllDates`), and whether it deletes a group.
fn arg_tag(arg: &str) -> Option<(&str, bool)> {
    let body = arg.strip_prefix('-')?;
    let end = body.find(['=', '<', '>']).unwrap_or(body.len());
    let tag = body[..end].trim_end_matches(['+', '-']);
    let group = tag
        .rsplit_once(':')
        .is_some_and(|(_, name)| name.eq_ignore_ascii_case("all"))
        || tag.eq_ignore_ascii_case("all") && body[end..].starts_with('=');
    Some((tag, group && body[end..].starts_with('=')))
}

/// Whether a later argument of `args` deletes a group that cancels the
/// request of argument `at` (see [`cancels`]).
fn cancelled_arg(args: &[String], at: usize) -> bool {
    let Some((tag, false)) = arg_tag(&args[at]) else {
        return false;
    };
    let group = tag.rsplit_once(':').map_or("", |(group, _)| group);
    args[at + 1..].iter().any(|later| match arg_tag(later) {
        Some((deleted, true)) => {
            let deleted = deleted.rsplit_once(':').map_or("all", |(group, _)| group);
            // An ungrouped date lands in EXIF where the file has it.
            cancels(deleted, if group.is_empty() { "exififd" } else { group })
        }
        _ => false,
    })
}

/// The tags or selectors a CLI refusal names: `tag '<T>'`, `copy '<S>'`,
/// `dates for '<T>'`.
fn cli_refusal_names(stderr: &str) -> Vec<String> {
    ["tag '", "copy '", "for '"]
        .iter()
        .flat_map(|marker| {
            stderr
                .split(marker)
                .skip(1)
                .filter_map(|rest| rest.split_once('\'').map(|(name, _)| name.to_string()))
        })
        .collect()
}

/// Refusals of a whole command line that name what they refuse (no tag to
/// quote): the combinations the CLI does not apply.
const PLAN_REFUSALS: &[&str] = &["Error: Combining ", "is both set and shifted"];

/// Date and `-TagsFromFile` cases: (label, fixture, CLI arguments).
fn date_and_copy_cases() -> Vec<(String, PathBuf, Vec<String>)> {
    let mut cases = Vec::new();
    let date = "2025:01:02 03:04:05";
    let canon = fixtures::pinned_t_images_fixture_path("Canon.jpg");
    let owned = |args: &[&str]| args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
    // Dates: each operation alone, then beside a group deletion or a field
    // request, in both orders.
    let mut date_fixtures: Vec<(PathBuf, Vec<String>)> = vec![
        (
            PathBuf::from(JPEG_NO_GPS),
            vec![
                format!("-DateTimeOriginal={date}"),
                format!("-ModifyDate={date}"),
                format!("-AllDates={date}"),
                "-DateTimeOriginal=2024:01:01 12:00:00".into(),
                "-DateTimeOriginal+=1:0:0 0:0:0".into(),
                "-AllDates+=1:0:0 0:0:0".into(),
                "-AllDates-=0:1:0 0:0:0".into(),
                "-ModifyDate+=0:0:1 0:0:0".into(),
            ],
        ),
        (
            PathBuf::from(PNG),
            vec![
                format!("-DateTimeOriginal={date}"),
                "-DateTimeOriginal=2024:03:01 10:00:00".into(),
                "-DateTimeOriginal+=1:0:0 0:0:0".into(),
                "-AllDates+=1:0:0 0:0:0".into(),
            ],
        ),
        (
            PathBuf::from(PDF),
            vec![
                format!("-PDF:CreateDate={date}"),
                "-PDF:CreateDate=2024:01:15 14:30:00+00:00".into(),
                "-DateTimeOriginal+=1:0:0 0:0:0".into(),
            ],
        ),
    ];
    if let Some(canon) = &canon {
        date_fixtures.push((
            canon.clone(),
            vec![
                format!("-DateTimeOriginal={date}"),
                format!("-ModifyDate={date}"),
                format!("-AllDates={date}"),
                "-AllDates=2003:12:04 06:46:52".into(),
                "-ModifyDate=2003:12:04 06:46:52".into(),
                "-AllDates+=1:0:0 0:0:0".into(),
                "-ExifIFD:CreateDate-=0:0:1 0:0:0".into(),
                "-ModifyDate+=0:0:1 0:0:0".into(),
            ],
        ));
    }
    let partners = [
        "-EXIF:All=",
        "-IFD0:All=",
        "-ExifIFD:All=",
        "-GPS:All=",
        "-IFD0:Artist=x",
        "-IFD0:Artist=",
    ];
    for (fixture, dates) in &date_fixtures {
        let name = fixture.file_name().unwrap().to_string_lossy().into_owned();
        for op in dates {
            cases.push((format!("date {name}"), fixture.clone(), vec![op.clone()]));
            for partner in partners {
                for args in [
                    vec![op.clone(), partner.to_string()],
                    vec![partner.to_string(), op.clone()],
                ] {
                    cases.push((format!("date {name}"), fixture.clone(), args));
                }
            }
        }
    }
    // -TagsFromFile selectors, alone and with a set or deletion before or
    // after the copy.
    let selectors: Vec<Vec<&str>> = vec![
        vec!["-all"],
        vec!["-all", "-Make"],
        vec!["-all", "--Make"],
        vec!["--Make"],
        vec!["-Make"],
        vec!["-IFD0:all"],
        vec!["-EXIF:all"],
        vec!["-XMP:all"],
        vec!["-XMP-dc:Title"],
        vec!["-XMP-dc:Title>IFD0:Artist"],
        vec!["-Make>Artist"],
        vec!["-IFD0:Model>IFD0:Artist", "-Make"],
        vec!["-*Model"],
        vec!["-IFD0:*"],
        vec!["-all", "--IFD0:Model"],
        vec!["-IFD0:all", "--Make"],
    ];
    // Selector semantics, from a JPEG with EXIF and XMP into JPEGs without
    // XMP, where a copy maps EXIF to EXIF and oxidex names the XMP it skips.
    // Which other groups 13.59's by-name, preferred-group copy also writes
    // (a JPEG's File:ImageWidth into XMP-tiff, a PDF's Info into XMP, EXIF
    // into a PNG's text chunks) and a camera file's maker notes are the
    // copy's destination mapping, not its selectors -- a separate class.
    let pairs: Vec<(String, PathBuf)> = vec![
        (JPEG_EXIF_XMP.into(), PathBuf::from(JPEG_NO_GPS)),
        (
            JPEG_EXIF_XMP.into(),
            PathBuf::from("tests/fixtures/jpeg/sample_with_exif.jpg"),
        ),
    ];
    for (index, (source, destination)) in pairs.iter().enumerate() {
        let name = destination
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        for selector in &selectors {
            let mut copy = owned(&["-TagsFromFile", source]);
            copy.extend(owned(selector));
            cases.push((format!("copy {name}"), destination.clone(), copy.clone()));
            if index != 0 {
                continue;
            }
            for request in ["-IFD0:Artist=z", "-IFD0:Make=", "-EXIF:All="] {
                let mut after = copy.clone();
                after.push(request.to_string());
                cases.push((format!("copy {name}"), destination.clone(), after));
                let mut before = vec![request.to_string()];
                before.extend(copy.clone());
                cases.push((format!("copy {name}"), destination.clone(), before));
            }
        }
    }
    // The copy's destination mapping (#957 round 8): 13.59 copies each tag by
    // name (Writer.pl `SetNewValuesFromFile` -> `SetNewValue`), to the
    // preferred group and to every other group already carrying the name --
    // a JPEG's File:ImageWidth into XMP-tiff, a PDF's Info into XMP-dc/-pdf/
    // -xmp, EXIF into a PNG's text chunks, a camera file's maker notes and
    // its maker-note values into ExifIFD -- over JPEG, PNG, PDF and TIFF
    // destinations.
    let mut mapping_pairs: Vec<(String, PathBuf)> = vec![
        (
            JPEG_NO_GPS.into(),
            PathBuf::from("tests/fixtures/jpeg/sample_with_exif.jpg"),
        ),
        (JPEG_NO_GPS.into(), PathBuf::from(PNG)),
        (PDF.into(), PathBuf::from(PDF)),
        (JPEG_EXIF_XMP.into(), PathBuf::from(TIFF)),
        (JPEG_NO_GPS.into(), PathBuf::from(TIFF)),
        (JPEG_NO_GPS.into(), PathBuf::from(PDF)),
        (JPEG_EXIF_XMP.into(), PathBuf::from(PNG)),
        (PNG.into(), PathBuf::from(JPEG_NO_GPS)),
    ];
    if let Some(canon) = &canon {
        mapping_pairs.push((
            canon.to_string_lossy().into_owned(),
            PathBuf::from(JPEG_NO_GPS),
        ));
        mapping_pairs.push((canon.to_string_lossy().into_owned(), PathBuf::from(PNG)));
    }
    let mapping_selectors: Vec<Vec<&str>> = vec![
        vec!["-all"],
        vec!["-Make"],
        vec!["-Artist"],
        vec!["-Title"],
        vec!["-*Model"],
        vec!["-all", "--Make"],
        vec!["-EXIF:all"],
    ];
    for (source, destination) in &mapping_pairs {
        let name = destination
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        for selector in &mapping_selectors {
            let mut copy = owned(&["-TagsFromFile", source]);
            copy.extend(owned(selector));
            cases.push((format!("mapping {name}"), destination.clone(), copy));
        }
    }
    cases
}

/// A `-TagsFromFile SRC SELECTOR...` command with nothing else: (source,
/// the filters `copy_metadata_report` takes -- each selector with the one
/// `-` the CLI strips).
fn pure_copy(args: &[String]) -> Option<(PathBuf, Vec<String>)> {
    let [flag, source, selectors @ ..] = args else {
        return None;
    };
    if !flag.eq_ignore_ascii_case("-TagsFromFile")
        || selectors
            .iter()
            .any(|arg| arg.contains('=') || !arg.starts_with('-'))
    {
        return None;
    }
    let filters = selectors.iter().map(|arg| arg[1..].to_string()).collect();
    Some((PathBuf::from(source), filters))
}

/// Every tag name (lower-case) the oracle reads from `path`, file-level
/// groups included.
fn oracle_names(oracle: &Oracle, path: &Path) -> BTreeSet<String> {
    let o = oracle
        .command()
        .args(["-a", "-G1", "-s"])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .map(|line| row_tag(line).1.to_ascii_lowercase())
        .collect()
}

/// `(family-1 group, tag name)` of an `-a -G1 -s` row.
fn row_tag(row: &str) -> (String, String) {
    let (group, rest) = row
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .unwrap_or(("", row));
    let name = rest.trim_start().split(' ').next().unwrap_or_default();
    (group.to_string(), name.to_string())
}

/// Grades a library copy (`copy_metadata_report`) against 13.59's copy of
/// the same file: every row 13.59 wrote is in oxidex's output unchanged or
/// named -- by family-1 group and tag -- in `CopyReport::uncopied_tags`, with
/// its group in `uncopied_groups`; oxidex writes no row 13.59 did not; and
/// every tag oxidex names as uncopied is one 13.59 did write.
fn grade_library_copy(
    report: &Result<oxidex::core::operations::CopyReport, oxidex::error::ExifToolError>,
    source_names: &BTreeSet<String>,
    before: &BTreeSet<String>,
    ours: &BTreeSet<String>,
    theirs: &BTreeSet<String>,
    their_outcome: &Outcome,
    untouched: bool,
) -> (Verdict, String) {
    let report = match (report, their_outcome) {
        (Err(err), Outcome::Refused(_)) if untouched => {
            return (Verdict::Match, format!("both refused: {err}"));
        }
        (Err(err), _) => {
            let typed = matches!(
                err,
                oxidex::error::ExifToolError::TagsNotWritten { .. }
                    | oxidex::error::ExifToolError::UnsupportedFormat { .. }
            );
            return if typed && untouched && err.to_string().contains('\'') {
                (Verdict::NamedRefusal, format!("library refused: {err}"))
            } else {
                (Verdict::Mismatch, format!("library error: {err}"))
            };
        }
        (Ok(report), _) => report,
    };
    let uncopied: BTreeSet<(String, String)> = report
        .uncopied_tags
        .iter()
        .filter_map(|tag| tag.tag.split_once(':'))
        .map(|(group, name)| (group.to_ascii_lowercase(), name.to_ascii_lowercase()))
        .collect();
    // A tag named "where its own conversion accepts the value": oxidex does
    // not model the conversions of the groups it cannot write, so 13.59 may
    // reject the value (a label its PrintConv lacks) and write nothing.
    let conditional: BTreeSet<(String, String)> = report
        .uncopied_tags
        .iter()
        .filter(|tag| {
            tag.reason
                .contains("where its own conversion accepts the value")
        })
        .filter_map(|tag| tag.tag.split_once(':'))
        .map(|(group, name)| (group.to_ascii_lowercase(), name.to_ascii_lowercase()))
        .collect();
    let xmp_named = report
        .uncopied_groups
        .iter()
        .any(|group| group.to_ascii_lowercase().starts_with("xmp-"));
    let named = |row: &String| {
        let (group, name) = row_tag(row);
        // ExifTool adds its XMPToolkit to any XMP it writes.
        if xmp_named && group == "XMP-x" && name == "XMPToolkit" {
            return true;
        }
        let key = (group.to_ascii_lowercase(), name.to_ascii_lowercase());
        uncopied.contains(&key)
            && report
                .uncopied_groups
                .iter()
                .any(|listed| listed.eq_ignore_ascii_case(&group))
    };
    let their_tags: BTreeSet<(String, String)> = theirs
        .iter()
        .map(|row| {
            let (group, name) = row_tag(row);
            (group.to_ascii_lowercase(), name.to_ascii_lowercase())
        })
        .collect();
    // A claim about a file-level group cannot be checked against the rows
    // (which leave those groups out); one about a tag 13.59 does not read
    // from the source at all comes from a row only oxidex's reader reports
    // (graded by the read conformance instrument, not here).
    let false_claims: Vec<_> = uncopied
        .difference(&their_tags)
        .filter(|(group, name)| {
            !matches!(group.as_str(), "file" | "system" | "composite" | "exiftool")
                && source_names.contains(name)
                && !conditional.contains(&(group.clone(), name.clone()))
        })
        .collect();
    if !false_claims.is_empty() {
        return (
            Verdict::Mismatch,
            format!("names as uncopied what 13.59 does not write: {false_claims:?}"),
        );
    }
    let written: Vec<&String> = theirs.difference(before).collect();
    let unexplained_theirs: Vec<&&String> = written
        .iter()
        .filter(|row| !ours.contains(**row) && !named(row))
        .collect();
    let unexplained_ours: Vec<&String> = ours
        .difference(theirs)
        .filter(|row| {
            // A row oxidex left as it was, because the tag 13.59 rewrote is
            // named as uncopied.
            !(before.contains(*row) && named(row))
        })
        .collect();
    if !unexplained_theirs.is_empty() || !unexplained_ours.is_empty() {
        return (
            Verdict::Mismatch,
            format!(
                "library copy: 13.59 only {unexplained_theirs:?}; oxidex only \
                 {unexplained_ours:?}; uncopied {:?}",
                report.uncopied_groups
            ),
        );
    }
    if ours == theirs {
        (Verdict::Match, String::new())
    } else {
        (
            Verdict::NamedRefusal,
            format!(
                "a library copy names what it cannot write: {:?}",
                report.uncopied_groups
            ),
        )
    }
}

/// Date operations (absolute sets, `+=`/`-=` shifts, `AllDates`, same-value
/// sets) beside group deletions and field requests in both orders, and
/// `-TagsFromFile` selector combinations (`all`, `all` with a tag, a
/// hyphenated group, `GROUP:all`, `--TAG` exclusions, redirection,
/// wildcards) alone and with a set or deletion before or after the copy
/// (#957 round 7). Graded like the ordering matrix, against pinned 13.59
/// running the same command: the oracle's read of both outputs, the
/// updated/unchanged count, and whether the command is refused -- a
/// refusal counts only when it names what it refuses (a tag, a selector,
/// or a listed combination), leaves the file untouched, names no request a
/// later deletion cancels, and carries a listed reason.
#[test]
fn dates_and_copies_match_pinned_exiftool() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let cases = date_and_copy_cases();
    let root = tempfile::tempdir().unwrap();
    type Library = (
        PathBuf,
        oxidex::error::Result<oxidex::core::operations::CopyReport>,
        bool,
    );
    type Run = (
        Outcome,
        PathBuf,
        Outcome,
        PathBuf,
        Vec<u8>,
        String,
        String,
        Option<Library>,
    );
    let runs: Vec<Mutex<Option<Run>>> = cases.iter().map(|_| Mutex::new(None)).collect();
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get().min(6));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some((_, fixture, args)) = cases.get(index) else {
                        break;
                    };
                    let dir = root.path().join(index.to_string());
                    fs::create_dir(&dir).unwrap();
                    let ext = fixture.extension().unwrap().to_str().unwrap();
                    let theirs = dir.join(format!("theirs.{ext}"));
                    let ours = dir.join(format!("ours.{ext}"));
                    fs::copy(fixture, &theirs).unwrap();
                    fs::copy(fixture, &ours).unwrap();
                    let o = oracle
                        .command()
                        .arg("-overwrite_original")
                        .args(args)
                        .arg(&theirs)
                        .output()
                        .unwrap();
                    let their_stderr = String::from_utf8_lossy(&o.stderr).into_owned();
                    let their_outcome = cli_outcome(
                        o.status.code(),
                        &String::from_utf8_lossy(&o.stdout),
                        &their_stderr,
                    );
                    // The library copy, beside the CLI's, for a command that
                    // is a copy alone.
                    let library = pure_copy(args).map(|(source, filters)| {
                        let path = dir.join(format!("library.{ext}"));
                        fs::copy(fixture, &path).unwrap();
                        let report = oxidex::core::operations::copy_metadata_report(
                            &source,
                            &path,
                            Some(&filters),
                        );
                        let untouched = fs::read(&path).unwrap() == fs::read(fixture).unwrap();
                        (path, report, untouched)
                    });
                    let before = fs::read(&ours).unwrap();
                    let o = Command::new(env!("CARGO_BIN_EXE_oxidex"))
                        .args(args)
                        .arg(&ours)
                        .output()
                        .unwrap();
                    let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
                    let our_outcome = cli_outcome(
                        o.status.code(),
                        &String::from_utf8_lossy(&o.stdout),
                        &stderr,
                    );
                    *runs[index].lock().unwrap() = Some((
                        their_outcome,
                        theirs,
                        our_outcome,
                        ours,
                        before,
                        stderr,
                        their_stderr,
                        library,
                    ));
                }
            });
        }
    });
    let runs: Vec<Run> = runs
        .into_iter()
        .map(|slot| slot.into_inner().unwrap().unwrap())
        .collect();
    let mut files: Vec<PathBuf> = runs
        .iter()
        .flat_map(|run| {
            [run.1.clone(), run.3.clone()]
                .into_iter()
                .chain(run.7.as_ref().map(|library| library.0.clone()))
        })
        .collect();
    let fixtures_read: BTreeSet<PathBuf> = cases.iter().map(|case| case.1.clone()).collect();
    files.extend(fixtures_read);
    let rows = oracle_rows(oracle, &files);
    let mut verdicts: Vec<(String, Verdict, String)> = Vec::new();
    let mut source_names: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    for (
        (kind, fixture, args),
        (theirs_outcome, theirs, ours_outcome, ours, before, stderr, their_stderr, library),
    ) in cases.iter().zip(&runs)
    {
        if let Some((path, report, untouched)) = library {
            let source = pure_copy(args).unwrap().0;
            let source_names = source_names
                .entry(source.clone())
                .or_insert_with(|| oracle_names(oracle, &source));
            let (verdict, detail) = grade_library_copy(
                report,
                source_names,
                &rows[fixture],
                &rows[path],
                &rows[theirs],
                theirs_outcome,
                *untouched,
            );
            let (verdict, detail) = match report {
                Ok(report)
                    if their_stderr.contains("No writable tags set from")
                        != (report.requested == 0) =>
                {
                    (
                        Verdict::Mismatch,
                        format!(
                            "library requested {} where 13.59 says {:?}",
                            report.requested,
                            their_stderr.trim()
                        ),
                    )
                }
                Ok(report) if report.copied > 0 && *theirs_outcome == Outcome::Unchanged => (
                    Verdict::Mismatch,
                    format!("library copied {} where 13.59 is unchanged", report.copied),
                ),
                Ok(report)
                    if report.copied == 0
                        && report.uncopied_tags.is_empty()
                        && *theirs_outcome == Outcome::Updated =>
                {
                    (
                        Verdict::Mismatch,
                        "library copied nothing and names nothing where 13.59 updated".to_string(),
                    )
                }
                _ => (verdict, detail),
            };
            verdicts.push((format!("library {kind} {args:?}"), verdict, detail));
        }
        let label = format!("{kind} {args:?}");
        let untouched = fs::read(ours).unwrap() == *before;
        let (verdict, detail) = match (ours_outcome, theirs_outcome) {
            (Outcome::Refused(why), Outcome::Refused(_)) => {
                if untouched {
                    (Verdict::Match, format!("both refused: {why}"))
                } else {
                    (
                        Verdict::Mismatch,
                        format!("refused but changed the file: {why}"),
                    )
                }
            }
            (Outcome::Refused(why), _) => {
                let names = cli_refusal_names(stderr);
                let planned = PLAN_REFUSALS.iter().any(|needle| stderr.contains(needle));
                let cancelled = names.iter().any(|name| {
                    args.iter().enumerate().any(|(at, arg)| {
                        arg_tag(arg).is_some_and(|(tag, _)| tag.eq_ignore_ascii_case(name))
                            && cancelled_arg(args, at)
                    })
                });
                let named = NAMED_REFUSALS
                    .iter()
                    .find(|(needle, _)| why.contains(needle));
                if names.is_empty() && !planned {
                    (Verdict::Mismatch, format!("untyped refusal: {why}"))
                } else if !untouched {
                    (
                        Verdict::Mismatch,
                        format!("refused but changed the file: {why}"),
                    )
                } else if cancelled {
                    (
                        Verdict::Mismatch,
                        format!("refused a request a later deletion cancels: {why}"),
                    )
                } else if let Some((_, reason)) = named {
                    (Verdict::NamedRefusal, format!("{reason}: {why}"))
                } else {
                    (Verdict::Mismatch, format!("unlisted refusal: {why}"))
                }
            }
            (_, Outcome::Refused(why)) => (
                Verdict::Mismatch,
                format!("13.59 refuses ({why}) but oxidex wrote"),
            ),
            (outcome, expected) => {
                let (ours_rows, their_rows) = (&rows[ours], &rows[theirs]);
                let skipped_all = skipped_groups(stderr);
                let only_skipped = |ours: &BTreeSet<String>, theirs: &BTreeSet<String>| {
                    ours.difference(theirs)
                        .all(|row| rows[fixture].contains(row) && row_in_groups(row, &skipped_all))
                        && !skipped_all.is_empty()
                        && theirs
                            .difference(ours)
                            .all(|row| row_in_groups(row, &skipped_all))
                };
                if outcome != expected
                    && *outcome == Outcome::Unchanged
                    && only_skipped(ours_rows, their_rows)
                {
                    // Everything 13.59 wrote was in groups oxidex skips by
                    // name, so oxidex wrote nothing.
                    (
                        Verdict::NamedRefusal,
                        format!(
                            "a best-effort copy skips the groups oxidex cannot write, and \
                             names them: {skipped_all:?}"
                        ),
                    )
                } else if outcome != expected {
                    (
                        Verdict::Mismatch,
                        format!("outcome {outcome:?}, 13.59 {expected:?}"),
                    )
                } else if ours_rows != their_rows {
                    let only_ours: Vec<_> = ours_rows.difference(their_rows).collect();
                    let only_theirs: Vec<_> = their_rows.difference(ours_rows).collect();
                    // A best-effort copy (`all`, `GROUP:all`, a wildcard)
                    // skips the groups oxidex cannot write -- and names them
                    // (`Warning: Not copied from SRC: oxidex cannot write the
                    // XMP group(s) here`): what 13.59 alone wrote must be
                    // exactly those groups.
                    let skipped: Vec<&str> = stderr
                        .lines()
                        .filter_map(|line| line.split_once("oxidex cannot write the "))
                        .filter_map(|(_, rest)| rest.split_once(" group(s)"))
                        .flat_map(|(groups, _)| groups.split(", "))
                        .collect();
                    let skipped: Vec<String> = skipped.iter().map(|s| s.to_string()).collect();
                    let in_skipped = |row: &&String| row_in_groups(row, &skipped);
                    // A row oxidex left as it was because its group is named
                    // as skipped (13.59 rewrote it by name).
                    let kept = |row: &&String| rows[fixture].contains(*row) && in_skipped(row);
                    if only_ours.iter().all(kept)
                        && !skipped.is_empty()
                        && only_theirs.iter().all(in_skipped)
                    {
                        (
                            Verdict::NamedRefusal,
                            format!(
                                "a best-effort copy skips the groups oxidex cannot write, and \
                                 names them: {skipped:?}"
                            ),
                        )
                    } else {
                        (
                            Verdict::Mismatch,
                            format!(
                                "tags differ: oxidex only {only_ours:?}; 13.59 only \
                                 {only_theirs:?}"
                            ),
                        )
                    }
                } else {
                    (Verdict::Match, String::new())
                }
            }
        };
        verdicts.push((label, verdict, detail));
    }
    let count = |v: Verdict| {
        verdicts
            .iter()
            .filter(|(_, verdict, _)| *verdict == v)
            .count()
    };
    let (matched, refused, mismatched) = (
        count(Verdict::Match),
        count(Verdict::NamedRefusal),
        count(Verdict::Mismatch),
    );
    eprintln!(
        "date and copy matrix: {} cases: {matched} match, {refused} named refusal, {mismatched} mismatch",
        verdicts.len()
    );
    let mut reasons: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, verdict, detail) in &verdicts {
        if *verdict == Verdict::NamedRefusal {
            *reasons
                .entry(detail.split(": ").next().unwrap())
                .or_default() += 1;
        }
    }
    for (reason, n) in &reasons {
        eprintln!("  named refusal x{n}: {reason}");
    }
    for (label, verdict, detail) in &verdicts {
        if *verdict == Verdict::Mismatch {
            eprintln!("  MISMATCH {label}: {detail}");
        }
    }
    if let Some(report) = std::env::var_os("OXIDEX_DATE_COPY_MATRIX_REPORT") {
        let json: Vec<serde_json::Value> = verdicts
            .iter()
            .map(|(label, verdict, detail)| {
                serde_json::json!({"case": label, "verdict": format!("{verdict:?}"), "detail": detail})
            })
            .collect();
        fs::write(
            report,
            serde_json::to_string_pretty(&serde_json::json!({
                "cases": verdicts.len(), "match": matched, "named_refusal": refused,
                "mismatch": mismatched, "verdicts": json,
            }))
            .unwrap(),
        )
        .unwrap();
    }
    assert!(verdicts.len() >= 150, "only {} cases", verdicts.len());
    assert_eq!(
        mismatched, 0,
        "{mismatched} date/copy cases differ from pinned 13.59 (listed above)"
    );
}

/// The groups a best-effort copy said it skipped (`Warning: Not copied from
/// SRC: oxidex cannot write the XMP group(s) here`).
fn skipped_groups(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .filter_map(|line| line.split_once("oxidex cannot write the "))
        .filter_map(|(_, rest)| rest.split_once(" group(s)"))
        .flat_map(|(groups, _)| groups.split(", ").map(str::to_string))
        .collect()
}

/// Whether an `-a -G1 -s` row belongs to one of `groups` (an `XMP-*`
/// family-1 group to `XMP`).
fn row_in_groups(row: &str, groups: &[String]) -> bool {
    let group = row
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .map_or("", |(group, _)| group);
    // ExifTool adds its XMPToolkit to any XMP it writes.
    if group == "XMP-x"
        && row_tag(row).1 == "XMPToolkit"
        && groups
            .iter()
            .any(|skipped| skipped.to_ascii_lowercase().starts_with("xmp"))
    {
        return true;
    }
    let family = group.split('-').next().unwrap_or(group);
    groups
        .iter()
        .any(|skipped| family.eq_ignore_ascii_case(skipped) || group.eq_ignore_ascii_case(skipped))
}
