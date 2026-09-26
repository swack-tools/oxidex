//! Command-line arguments that are not valid UTF-8, run through the real
//! `oxidex` binary.
//!
//! Before this suite the CLI collected its arguments with `std::env::args()`,
//! which panics on the first argument that is not UTF-8 -- a typed tag value
//! carrying CESU-8 bytes for a lone surrogate (`ed a0 80`), or a filename in
//! a legacy encoding. No file was written, but the process died with a
//! backtrace. Every test here asserts first that no invocation panics, then
//! what it does instead.
//!
//! Every expected byte string and rendering below is the pinned ExifTool
//! 13.59 oracle's own output for the same argv bytes, recorded verbatim
//! (`perl5.38.2 -I.../13.59/exiftool/lib .../13.59/exiftool/exiftool`, `-ver`
//! 13.59 and the `OOXML.docx` capability probe `DOCX` asserted), except the
//! filename renderings, which need a filesystem that accepts non-UTF-8 names:
//! those were recorded on Linux with the same pinned 13.59 library tree
//! (system perl 5.40.1, `-ver` 13.59; that perl lacks `Archive::Zip`, which
//! no JPEG read touches).
//!
//! What the oracle does, per case:
//!
//! * **Paths** are bytes. It reads and writes `n\xff.jpg`, prints the name's
//!   raw bytes in `======== ` headers, messages and CSV, and replaces each
//!   malformed byte with `?` in JSON (`SourceFile: "n?.jpg"`).
//! * **XP\* values** (`Writable => 'int8u'`, `ValueConvInv =>
//!   Encode($val,"UCS2","II") . "\0\0"`): the typed bytes are decoded as
//!   Perl's lax UTF-8, which accepts a surrogate code point, and each code
//!   point is packed as its low 16 bits. `A ed a0 80 B` writes `41 00 00 d8
//!   42 00 00 00`. Bytes Perl calls malformed (`ff`, a lone `e9`, a stray
//!   continuation, an overlong or truncated sequence) are refused: `Warning:
//!   Malformed UTF-8 character ... Nothing to do.`, exit 1, file untouched.
//! * **Other values** are handled per format in ways oxidex's writers cannot
//!   reproduce yet (EXIF ASCII keeps the raw bytes; XMP replaces each bad
//!   byte with `?`; IPTC truncates; numeric tags refuse). oxidex refuses all
//!   of them with a clear error and a non-zero exit rather than approximate.
//! * **Tag and option names** that are not UTF-8 are refused (`Invalid tag
//!   name`, `Unknown option`); oxidex refuses them too.
#![cfg(unix)]

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use oxidex::exiftool_oracle;

/// An 8x8 baseline JPEG with no metadata segment: decodable, so both tools
/// agree to write it. (The same image `tests/xp_string_write.rs` uses.)
const BASE_JPEG_HEX: &str = "ffd8ffdb0084001410101912192717172732261f26322e262626262e3e35353535353e44414141414141444444444444444444444444444444444444444444444444444444444401151919201c2026181826362620263644362b2b364444444235424444444444444444444444444444444444444444444444444444444444444444444444444444ffc00011080008000803012200021101031101ffc4004b00010100000000000000000000000000000006010100000000000000000000000000000000100100000000000000000000000000000000110100000000000000000000000000000000ffda000c03010002110311003f00b3001fffd9";

/// A repository JPEG with an IFD0 (`Make: TestCamera`), always present.
const REPO_FIXTURE: &str = "tests/fixtures/jpeg/sample_with_exif.jpg";

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn os(bytes: &[u8]) -> OsString {
    OsString::from_vec(bytes.to_vec())
}

fn run(args: &[OsString]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run oxidex {args:?}: {e}"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.code().is_some() && output.status.code() != Some(101),
        "oxidex {args:?} died: {:?}\nstderr: {stderr}",
        output.status
    );
    assert!(
        !stderr.contains("panicked"),
        "oxidex {args:?} panicked:\n{stderr}"
    );
    output
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn write(dir: &Path, name: &[u8], bytes: &[u8]) -> PathBuf {
    let path = dir.join(OsStr::from_bytes(name));
    std::fs::write(&path, bytes).unwrap();
    path
}

/// The TIFF block of a JPEG's `Exif` APP1 segment.
fn tiff_block(file: &[u8]) -> Option<&[u8]> {
    let mut i = 2;
    while i + 4 <= file.len() && file[i] == 0xFF {
        let marker = file[i + 1];
        if marker == 0xDA || marker == 0xD9 {
            return None;
        }
        let len = u16::from_be_bytes([file[i + 2], file[i + 3]]) as usize;
        let seg = &file[i + 4..i + 2 + len];
        if marker == 0xE1 && seg.starts_with(b"Exif\0\0") {
            return Some(&seg[6..]);
        }
        i += 2 + len;
    }
    None
}

/// The raw IFD0 entry for `tag`: (field type, value bytes).
fn ifd0_entry(path: &Path, tag: u16) -> Option<(u16, Vec<u8>)> {
    let file = std::fs::read(path).unwrap();
    let t = tiff_block(&file)?;
    let le = t.starts_with(b"II");
    let r16 = |o: usize| {
        let b = [t[o], t[o + 1]];
        if le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        }
    };
    let r32 = |o: usize| {
        let b = [t[o], t[o + 1], t[o + 2], t[o + 3]];
        if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    };
    let ifd = r32(4) as usize;
    for k in 0..r16(ifd) as usize {
        let p = ifd + 2 + 12 * k;
        let (id, typ, count) = (r16(p), r16(p + 2), r32(p + 4) as usize);
        if id != tag {
            continue;
        }
        let size = count
            * match typ {
                3 | 8 => 2,
                4 | 9 | 11 => 4,
                5 | 10 | 12 => 8,
                _ => 1,
            };
        let at = if size <= 4 {
            p + 8
        } else {
            r32(p + 8) as usize
        };
        return Some((typ, t[at..at + size].to_vec()));
    }
    None
}

const XP_TITLE: u16 = 0x9c9b;

/// Typed XP values the oracle accepts, with the entry it writes (always a new
/// `int8u` entry here: the base JPEG has no EXIF). `-IFD0:XPTitle=<value>`.
const XP_ACCEPTED: &[(&str, &str, &str)] = &[
    // (label, typed value bytes, oracle's entry bytes)
    ("lone high surrogate", "41eda08042", "410000d842000000"),
    ("lone low surrogate", "41edb08042", "410000dc42000000"),
    (
        "CESU-8 surrogate pair",
        "41eda0bdedb88042",
        "41003dd800de42000000",
    ),
    (
        "surrogate among BMP text",
        "c3a9eda080e4b8ad",
        "e90000d82d4e0000",
    ),
    // U+1F38C is packed as its low 16 bits, F38C, as every typed code point
    // above U+FFFF is (`pack('v*')`, Charset.pm:387-390).
    ("astral then surrogate", "f09f8e8ceda080", "8cf300d80000"),
    ("last low surrogate", "edbfbf5a", "ffdf5a000000"),
];

/// A non-UTF-8 XP value is written as ExifTool's UCS-2LE bytes, as `int8u`.
#[test]
fn xp_value_with_a_lone_surrogate_writes_exiftools_ucs2_bytes() {
    for (label, value, oracle) in XP_ACCEPTED {
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), b"a.jpg", &hex(BASE_JPEG_HEX));
        let mut arg = b"-IFD0:XPTitle=".to_vec();
        arg.extend_from_slice(&hex(value));
        let out = run(&[os(&arg), file.clone().into()]);
        assert!(
            out.status.success(),
            "{label}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "    1 image files updated\n",
            "{label}"
        );
        assert_eq!(
            ifd0_entry(&file, XP_TITLE),
            Some((1, hex(oracle))),
            "{label}: -IFD0:XPTitle={value}"
        );
    }
}

/// All five XP tags take the same encoding under `IFD0:` and `EXIF:`. The
/// bare name is refused: oxidex's writer does not reach the file for an
/// ungrouped XP name (`-XPTitle=Hi` reports an update and writes nothing),
/// so accepting bytes for one would report a write that never happens.
#[test]
fn every_xp_tag_and_group_spelling_takes_the_encoding() {
    for (id, name) in [
        (0x9c9b, "XPTitle"),
        (0x9c9c, "XPComment"),
        (0x9c9d, "XPAuthor"),
        (0x9c9e, "XPKeywords"),
        (0x9c9f, "XPSubject"),
    ] {
        for group in ["IFD0:", "EXIF:"] {
            let dir = tempfile::tempdir().unwrap();
            let file = write(dir.path(), b"a.jpg", &hex(BASE_JPEG_HEX));
            let mut arg = format!("-{group}{name}=").into_bytes();
            arg.extend_from_slice(b"A\xed\xa0\x80B");
            let out = run(&[os(&arg), file.clone().into()]);
            assert!(
                out.status.success(),
                "-{group}{name}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(
                ifd0_entry(&file, id),
                Some((1, hex("410000d842000000"))),
                "-{group}{name}="
            );
        }
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), b"a.jpg", &hex(BASE_JPEG_HEX));
        let before = std::fs::read(&file).unwrap();
        let mut arg = format!("-{name}=").into_bytes();
        arg.extend_from_slice(b"A\xed\xa0\x80B");
        let out = run(&[os(&arg), file.clone().into()]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "-{name}: {stderr}");
        assert!(stderr.contains(&format!("-IFD0:{name}=")), "{stderr}");
        assert_eq!(std::fs::read(&file).unwrap(), before, "-{name}=");
    }
}

/// Bytes Perl's decoder calls malformed are refused by the oracle
/// (`Malformed UTF-8 character`, `Nothing to do.`, exit 1); oxidex refuses
/// them the same way and leaves the file untouched.
#[test]
fn malformed_xp_values_are_refused_like_exiftool() {
    for (label, value) in [
        ("0xff", "41ff42"),
        ("0xfe", "41fe42"),
        ("latin-1 e9 at the end", "636166e9"),
        ("latin-1 e9 mid-word", "636166e973"),
        ("stray continuation", "418042"),
        ("overlong c0 80", "41c08042"),
        ("overlong c1 bf", "41c1bf42"),
        ("overlong e0 80 80", "41e0808042"),
        ("overlong f0 80 80 80", "41f080808042"),
        ("truncated at the end", "41e282"),
        ("truncated mid-string", "41e28242"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), b"a.jpg", &hex(BASE_JPEG_HEX));
        let before = std::fs::read(&file).unwrap();
        let mut arg = b"-IFD0:XPTitle=".to_vec();
        arg.extend_from_slice(&hex(value));
        let out = run(&[os(&arg), file.clone().into()]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{label}: {stderr}");
        assert!(
            stderr.contains("Malformed UTF-8"),
            "{label}: expected a malformed-UTF-8 error, got: {stderr}"
        );
        assert!(!contains(&out.stdout, b"updated"), "{label}");
        assert_eq!(std::fs::read(&file).unwrap(), before, "{label}");
    }
}

/// Values the oracle accepts but whose bytes oxidex cannot reproduce through
/// its writer are refused, never approximated: a code point above U+10FFFF
/// (the oracle packs its low 16 bits, here `0000`, a NUL inside the value
/// that the writer's UCS-2 round trip would end the string at), and a first
/// code unit that reads back as a byte-order mark.
#[test]
fn xp_values_oxidex_cannot_reproduce_are_refused() {
    for (label, value) in [
        ("above U+10FFFF (f4 90 80 80)", "41f490808042"),
        ("Perl 5-byte extended (f8)", "41f88880808042"),
        ("leading U+FFFE then a surrogate", "efbfbeeda080"),
        ("leading U+FEFF then a surrogate", "efbbbfeda080"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), b"a.jpg", &hex(BASE_JPEG_HEX));
        let before = std::fs::read(&file).unwrap();
        let mut arg = b"-IFD0:XPTitle=".to_vec();
        arg.extend_from_slice(&hex(value));
        let out = run(&[os(&arg), file.clone().into()]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{label}: {stderr}");
        assert!(stderr.starts_with("Error: "), "{label}: {stderr}");
        assert_eq!(std::fs::read(&file).unwrap(), before, "{label}");
    }
}

/// Every other destination refuses a non-UTF-8 value with a clear error and
/// a non-zero exit, writing nothing -- including when a valid modification
/// comes first on the command line.
#[test]
fn non_utf8_values_for_other_tags_are_refused() {
    for arg in [
        &b"-IFD0:Artist=A\xffB"[..],
        b"-Artist=A\xed\xa0\x80B",
        b"-IFD0:Make=caf\xe9",
        b"-XMP-dc:Title=A\xffB",
        b"-IPTC:Caption-Abstract=A\xffB",
        b"-ExifIFD:UserComment=A\xffB",
        b"-IFD0:XResolution=7\xff",
        b"-ExifIFD:ISO=1\xff",
        b"-ExifIFD:DateTimeOriginal=2020:01:01 00:00:0\xff",
        b"-AllDates+=1:0:0 0:0:0\xff",
        b"-Comment=A\xffB",
        b"-XPTitleX=A\xed\xa0\x80B",
        b"-XMP:XPTitle=A\xed\xa0\x80B",
    ] {
        let label = String::from_utf8_lossy(arg).into_owned();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.jpg");
        std::fs::copy(REPO_FIXTURE, &file).unwrap();
        let before = std::fs::read(&file).unwrap();
        for argv in [
            vec![os(arg), file.clone().into()],
            vec!["-IFD0:Model=X".into(), os(arg), file.clone().into()],
        ] {
            let out = run(&argv);
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert_eq!(out.status.code(), Some(1), "{label}: {stderr}");
            assert!(
                stderr.starts_with("Error: ") && stderr.contains("not valid UTF-8"),
                "{label}: expected a clear non-UTF-8 error, got: {stderr}"
            );
            assert!(!contains(&out.stdout, b"updated"), "{label}");
            assert_eq!(std::fs::read(&file).unwrap(), before, "{label}");
        }
    }
}

/// A tag or option name that is not UTF-8 is a clear error (the oracle:
/// `Invalid tag name`, `Unknown option`), never a panic, and nothing is
/// written.
#[test]
fn non_utf8_tag_and_option_names_are_clear_errors() {
    for argv in [
        vec![&b"-Art\xffist=x"[..]],
        vec![b"-IFD\xff:Artist=x"],
        vec![b"-Mak\xffe"],
        vec![b"-\xff"],
        vec![b"--js\xffon"],
        vec![b"-FileName<\xff"],
        vec![b"--detector", b"sig\xffnature"],
        vec![b"-d", b"%Y\xff", b"-FileName<DateTimeOriginal"],
    ] {
        let label = format!("{:?}", argv.iter().map(|a| os(a)).collect::<Vec<_>>());
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.jpg");
        std::fs::copy(REPO_FIXTURE, &file).unwrap();
        let before = std::fs::read(&file).unwrap();
        let mut args: Vec<OsString> = argv.iter().map(|a| os(a)).collect();
        args.push(file.clone().into());
        let out = run(&args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{label}: {stderr}");
        assert!(stderr.starts_with("Error: "), "{label}: {stderr}");
        assert!(out.stdout.is_empty(), "{label}: {:?}", out.stdout);
        assert_eq!(std::fs::read(&file).unwrap(), before, "{label}");
    }
}

/// A non-UTF-8 path to a file that is not there is an ordinary error, and
/// the message carries the path's own bytes, as the oracle's `Error: File not
/// found - n\xff.jpg` does -- not U+FFFD.
#[test]
fn missing_non_utf8_path_is_a_clean_error_naming_its_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join(OsStr::from_bytes(b"n\xff.jpg"));
    let raw = missing.as_os_str().as_bytes().to_vec();
    for argv in [
        vec![os(b"-s2"), os(b"-Make"), missing.clone().into()],
        vec![os(b"-IFD0:Artist=x"), missing.clone().into()],
        vec![
            os(b"-TagsFromFile"),
            missing.clone().into(),
            REPO_FIXTURE.into(),
        ],
    ] {
        let out = run(&argv);
        assert_eq!(out.status.code(), Some(1), "{argv:?}");
        assert!(
            contains(&out.stderr, &raw),
            "{argv:?}: stderr does not name the path's bytes: {:?}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!contains(&out.stderr, "\u{FFFD}".as_bytes()), "{argv:?}");
    }
}

/// Creates a file whose name is not UTF-8, or reports that this filesystem
/// refuses such names (APFS and HFS+ return `EILSEQ`): the round trips below
/// then have nothing to run on here, and say so rather than pass silently.
fn non_utf8_file(dir: &Path, name: &[u8], bytes: &[u8]) -> Option<PathBuf> {
    let path = dir.join(OsStr::from_bytes(name));
    match std::fs::write(&path, bytes) {
        Ok(()) => Some(path),
        Err(e) if e.raw_os_error() == Some(libc::EILSEQ) => {
            eprintln!(
                "note: skipping non-UTF-8 filename round trip -- this filesystem refuses {:?}: {e}",
                path
            );
            None
        }
        Err(e) => panic!("cannot create {path:?}: {e}"),
    }
}

/// A file whose name is not UTF-8 reads and writes as a path, end to end, and
/// every rendering of its name matches the oracle's: raw bytes in `======== `
/// headers and CSV, `?` for each malformed byte in JSON.
#[test]
fn non_utf8_filename_read_write_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = std::fs::read(REPO_FIXTURE).unwrap();
    let Some(file) = non_utf8_file(dir.path(), b"n\xff.jpg", &fixture) else {
        return;
    };
    let plain = write(dir.path(), b"plain.jpg", &fixture);

    // Read.
    let out = run(&["-s2".into(), "-Make".into(), file.clone().into()]);
    assert!(out.status.success());
    assert_eq!(out.stdout, b"Make: TestCamera\n");

    // Write, then read the write back.
    let out = run(&["-IFD0:Artist=Z".into(), file.clone().into()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"    1 image files updated\n");
    let out = run(&["-s2".into(), "-IFD0:Artist".into(), file.clone().into()]);
    assert_eq!(out.stdout, b"Artist: Z\n");

    // A non-UTF-8 value into a non-UTF-8 path.
    let out = run(&[os(b"-IFD0:XPTitle=A\xed\xa0\x80B"), file.clone().into()]);
    assert!(out.status.success());
    assert_eq!(
        ifd0_entry(&file, XP_TITLE),
        Some((1, hex("410000d842000000")))
    );

    // Multi-file headers carry the name's raw bytes (oracle, `-s2 -Make
    // base.jpg n\xff.jpg`: `======== n\xff.jpg`).
    let out = run(&[
        "-s2".into(),
        "-Make".into(),
        plain.clone().into(),
        file.clone().into(),
    ]);
    assert!(out.status.success());
    let mut header = b"======== ".to_vec();
    header.extend_from_slice(file.as_os_str().as_bytes());
    header.push(b'\n');
    assert!(
        contains(&out.stdout, &header),
        "{:?}",
        String::from_utf8_lossy(&out.stdout)
    );

    // JSON replaces each malformed byte with `?` (oracle: `"SourceFile":
    // "n?.jpg"`), so the output stays valid JSON.
    let out = run(&[
        "-j".into(),
        "-Make".into(),
        plain.clone().into(),
        file.clone().into(),
    ]);
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let expected = file.to_string_lossy().replace('\u{FFFD}', "?");
    assert_eq!(json[1]["SourceFile"], serde_json::json!(expected));

    // A non-UTF-8 source for -TagsFromFile.
    let out = run(&[
        "-TagsFromFile".into(),
        file.clone().into(),
        "-IFD0:Artist".into(),
        plain.clone().into(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = run(&["-s2".into(), "-IFD0:Artist".into(), plain.clone().into()]);
    assert_eq!(out.stdout, b"Artist: Z\n");

    // After `--`, and inside a directory whose name is not UTF-8.
    let out = run(&[
        "-s2".into(),
        "-Make".into(),
        "--".into(),
        file.clone().into(),
    ]);
    assert_eq!(out.stdout, b"Make: TestCamera\n");
    let sub = dir.path().join(OsStr::from_bytes(b"d\xfe"));
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("in.jpg"), &fixture).unwrap();
    let out = run(&["-s2".into(), "-Make".into(), sub.clone().into()]);
    assert!(out.status.success());
    let mut header = b"======== ".to_vec();
    header.extend_from_slice(sub.join("in.jpg").as_os_str().as_bytes());
    header.push(b'\n');
    assert!(
        contains(&out.stdout, &header),
        "{:?}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// The accepted XP values graded live against the pinned oracle: it writes a
/// reference file from the same argv bytes, and the XP entries must match.
#[test]
fn oracle_writes_the_same_xp_bytes_for_non_utf8_values() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    for (label, value, _) in XP_ACCEPTED {
        let dir = tempfile::tempdir().unwrap();
        let mut arg = b"-IFD0:XPTitle=".to_vec();
        arg.extend_from_slice(&hex(value));
        let reference = write(dir.path(), b"ref.jpg", &hex(BASE_JPEG_HEX));
        let out = oracle
            .command()
            .args([
                os(b"-q"),
                os(b"-overwrite_original"),
                os(&arg),
                reference.clone().into(),
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{} {label}: {}",
            oracle.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        let ours = write(dir.path(), b"ours.jpg", &hex(BASE_JPEG_HEX));
        let out = run(&[os(&arg), ours.clone().into()]);
        assert!(out.status.success(), "{label}");
        assert_eq!(
            ifd0_entry(&ours, XP_TITLE),
            ifd0_entry(&reference, XP_TITLE),
            "{label}"
        );
    }
}
