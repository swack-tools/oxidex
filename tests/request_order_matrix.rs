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
