//! The tenth round of Codex review threads on the beta.1 roll-up (#957) at
//! ebe82593: an `EXIF:all` copy that skipped SubIFD rows, write shortcuts
//! other than `AllDates`, and a hyphen-separated GPSDateStamp. Every case
//! runs the real CLI and pinned ExifTool 13.59 (`perl5.38.2 -I<pinned>/lib
//! <pinned>/exiftool`; probes `-ver` = 13.59, `OOXML.docx` FileType = DOCX,
//! through `exiftool_oracle::graded()`) on two copies of one source and
//! compares the oracle's own `-a -G1 -s` read-back of both.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::exiftool_oracle::{self, Oracle};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";

fn oxidex(args: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(path)
        .output()
        .expect("run oxidex")
}

fn oracle_write(oracle: &Oracle, args: &[&str], path: &Path) -> Output {
    let o = oracle
        .command()
        .args(["-overwrite_original"])
        .args(args)
        .arg(path)
        .output()
        .expect("run oracle");
    assert!(o.status.success(), "oracle {args:?}: {o:?}");
    o
}

/// The oracle's `-a -G1 -s` rows of `tags` in `path`, one per line.
fn rows(oracle: &Oracle, path: &Path, tags: &[&str]) -> Vec<String> {
    let o = oracle
        .command()
        .args(["-a", "-G1", "-s"])
        .args(tags.iter().map(|tag| format!("-{tag}")))
        .arg(path)
        .output()
        .expect("run oracle read");
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

fn copy_into(dir: &TempDir, source: &Path, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::copy(source, &path).expect("copy fixture");
    path
}

fn unchanged(path: &Path, before: &[u8], metadata: &fs::Metadata) {
    assert_eq!(fs::read(path).unwrap(), before);
    #[cfg(unix)]
    assert_eq!(fs::metadata(path).unwrap().ino(), metadata.ino());
    #[cfg(not(unix))]
    let _ = metadata;
    assert!(!PathBuf::from(format!("{}_original", path.display())).exists());
}

// --- 4112788430: an EXIF:all copy selects SubIFD rows ----------------------

/// t/images/DNG.dng with a text tag the oracle writes into its SubIFD:
/// oxidex reads it as `SubIFD0:ImageDescription`, which `EXIF:all` did not
/// select, so `-TagsFromFile SRC -EXIF:all` copied nothing of it where 13.59
/// copies it by name into the destination's IFD0 (and its `SubIFD:Artist`,
/// found after IFD0's, is the Artist it copies).
#[test]
fn an_exif_all_copy_selects_subifd_rows() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let Some(dng) = fixtures::pinned_t_images_fixture_path("DNG.dng") else {
        eprintln!("skipping: pinned t/images/DNG.dng not available");
        return;
    };
    let dir = TempDir::new().unwrap();
    let source = copy_into(&dir, &dng, "source.dng");
    oracle_write(
        oracle,
        &[
            "-SubIFD:ImageDescription=SubIFD text",
            "-SubIFD:Artist=SubIFD artist",
        ],
        &source,
    );
    assert_eq!(
        rows(oracle, &source, &["SubIFD:ImageDescription"]),
        ["[SubIFD] ImageDescription : SubIFD text"],
        "the oracle wrote the source's SubIFD"
    );
    let theirs = copy_into(&dir, Path::new(JPEG), "theirs.jpg");
    let ours = copy_into(&dir, Path::new(JPEG), "ours.jpg");
    let before = fs::read(&ours).unwrap();
    let metadata = fs::metadata(&ours).unwrap();
    let source_arg = source.to_str().unwrap();
    oracle_write(oracle, &["-TagsFromFile", source_arg, "-EXIF:all"], &theirs);
    let o = oxidex(
        &[
            "-overwrite_original",
            "-TagsFromFile",
            source_arg,
            "-EXIF:all",
        ],
        &ours,
    );
    assert_eq!(o.status.code(), Some(1), "{o:?}");
    assert!(
        String::from_utf8_lossy(&o.stderr).contains(
            "Cannot write tag 'ExifIFD:MakerNoteCanon': the selected physical maker note block cannot be copied by oxidex"
        ),
        "{o:?}"
    );
    unchanged(&ours, &before, &metadata);
    let tags = ["ImageDescription", "Artist"];
    let expected = rows(oracle, &theirs, &tags);
    assert!(
        expected.contains(&"[IFD0] ImageDescription : SubIFD text".to_string()),
        "13.59 copies the SubIFD's ImageDescription: {expected:?}"
    );

    // Excluding only the unsupported physical block still copies the SubIFD
    // scalars. Compare both successful destinations through the native reader.
    let theirs = copy_into(&dir, Path::new(JPEG), "theirs-no-block.jpg");
    let ours = copy_into(&dir, Path::new(JPEG), "ours-no-block.jpg");
    oracle_write(
        oracle,
        &["-TagsFromFile", source_arg, "-EXIF:all", "--MakerNoteCanon"],
        &theirs,
    );
    let o = oxidex(
        &[
            "-overwrite_original",
            "-TagsFromFile",
            source_arg,
            "-EXIF:all",
            "--MakerNoteCanon",
        ],
        &ours,
    );
    assert_eq!(o.status.code(), Some(0), "{o:?}");
    assert_eq!(rows(oracle, &ours, &tags), rows(oracle, &theirs, &tags));
    assert_eq!(rows(oracle, &ours, &tags), expected);
}

// --- 4112788433: every write shortcut expands --------------------------------

/// `Shortcuts::Main` keys other than `AllDates` were accepted as defined and
/// then written as one unknown tag. 13.59's `-CommonIFD0=` deletes every
/// IFD0 field of the shortcut (Make, Model and ModifyDate in
/// t/images/Canon.jpg), in any letter case; `-AllDates=` still sets the
/// three dates.
#[test]
fn every_write_shortcut_expands_as_exiftool_does() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let Some(canon) = fixtures::pinned_t_images_fixture_path("Canon.jpg") else {
        eprintln!("skipping: pinned t/images/Canon.jpg not available");
        return;
    };
    let dir = TempDir::new().unwrap();
    for arg in [
        "-CommonIFD0=",
        "-commonifd0=",
        "-AllDates=2021:02:03 04:05:06",
    ] {
        let theirs = copy_into(&dir, &canon, "theirs.jpg");
        let ours = copy_into(&dir, &canon, "ours.jpg");
        oracle_write(oracle, &[arg], &theirs);
        let o = oxidex(&[arg], &ours);
        assert_eq!(
            (o.status.code(), String::from_utf8_lossy(&o.stdout).as_ref()),
            (Some(0), "    1 image files updated\n"),
            "{arg}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        let tags = ["CommonIFD0", "AllDates"];
        assert_eq!(
            rows(oracle, &ours, &tags),
            rows(oracle, &theirs, &tags),
            "{arg}"
        );
    }
}

// --- 4112862931: a hyphen separates GPSDateStamp's parts ---------------------

/// GPS.pm 13.59's GPSDateStamp PrintConvInv takes the date from any value
/// (`(\d{4}).*?(\d{2}).*?(\d{2})`) and adjusts to UTC only a full date/time
/// `GetUnixTime` parses; oxidex refused every value with a hyphen. A zoned
/// date/time is still refused (the UTC adjustment is not ported), with the
/// file left untouched.
#[test]
fn a_gps_date_stamp_takes_hyphens_as_separators() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let dir = TempDir::new().unwrap();
    for value in ["2024-01-02", "2024-01-02T10:11:12", "2024:01:02 10:11:12"] {
        let arg = format!("-GPSDateStamp={value}");
        let theirs = copy_into(&dir, Path::new(JPEG), "theirs.jpg");
        let ours = copy_into(&dir, Path::new(JPEG), "ours.jpg");
        oracle_write(oracle, &[&arg], &theirs);
        let o = oxidex(&[&arg], &ours);
        assert_eq!(
            o.status.code(),
            Some(0),
            "{arg}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        let expected = rows(oracle, &theirs, &["GPSDateStamp"]);
        assert_eq!(expected, ["[GPS] GPSDateStamp : 2024:01:02"], "{arg}");
        assert_eq!(rows(oracle, &ours, &["GPSDateStamp"]), expected, "{arg}");
    }
    let ours = copy_into(&dir, Path::new(JPEG), "zoned.jpg");
    let before = fs::read(&ours).unwrap();
    let o = oxidex(&["-GPSDateStamp=2024:01:02 23:30:00-05:00"], &ours);
    assert_eq!(o.status.code(), Some(1), "a zoned date/time is refused");
    assert_eq!(fs::read(&ours).unwrap(), before, "file untouched");
}
