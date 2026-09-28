//! A bare `-TAG` answers with the copy ExifTool 13.59 answers with.
//!
//! When a file carries one tag name in several groups, `exiftool -TAG`
//! prints the copy `FoundTag` left under the bare key (`SetFoundTags`,
//! ExifTool.pm 13.59:5389-5395), and `FoundTag` (ExifTool.pm:9518-9591)
//! decides that by priority -- the tag's own `Priority`, the table's
//! `PRIORITY`, 0 for `Avoid`, 0 in a `LOW_PRIORITY_DIR`, promoted in the
//! `PRIORITY_DIR` -- then by the order the copies were found, the later
//! winning a tie, and never across sub-documents (`DOC_NUM`). Since #957
//! `-TagsFromFile`/`copy_metadata` copies each tag by that same name, so a
//! wrong winner is also a wrong value written into the destination.
//!
//! Every case is one `t/images` file of the pinned tree, graded only by
//! [`exiftool_oracle::graded`] (pinned `-ver` and the `OOXML.docx` probe):
//! the oracle must itself report the name under at least two family-1
//! groups (`-a -G1`), so a case cannot pass by the file having lost its
//! duplicate, and then oxidex's `-s3 -TAG` and `-j -TAG` must equal the
//! oracle's. `tools/exiftool-tables/bare_name_breadth.py` measures the same
//! question over every multi-group name of every `t/images` file.

use oxidex::exiftool_oracle;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

struct Case {
    file: &'static str,
    tag: &'static str,
    /// The rule that picks ExifTool's copy, for the failure message.
    rule: &'static str,
}

/// The seven reported divergences.
const REPORTED: &[Case] = &[
    Case {
        file: "Kodak.jpg",
        tag: "MeteringMode",
        rule: "Kodak::Main is read (all 25 fields) and found after ExifIFD 0x9207",
    },
    Case {
        file: "Kodak.jpg",
        tag: "ExposureTime",
        rule: "Kodak::Main is read and found after ExifIFD 0x829a",
    },
    Case {
        file: "Kodak.jpg",
        tag: "FNumber",
        rule: "Kodak::Main is read and found after ExifIFD 0x829d",
    },
    Case {
        file: "Google.jpg",
        tag: "CreateDate",
        rule: "Google HDRP CreateDate is Priority => 0 (Google.pm:539-546)",
    },
    Case {
        file: "Casio2.jpg",
        tag: "WhiteBalance",
        rule: "Casio::Type2 0x2012 is found at 0x927C, before ExifIFD 0xa403 (Priority => 0)",
    },
    Case {
        file: "ExifTool.jpg",
        tag: "Copyright",
        rule: "MIE trailer is found last; Ducky is Priority => 0; JUMBF is a sub-document",
    },
];

/// A sample of the breadth set, one or two per rule.
const BREADTH: &[Case] = &[
    Case {
        file: "Pentax.jpg",
        tag: "Contrast",
        rule: "ExifIFD 0xa408 is found after the MakerNote at 0x927C",
    },
    Case {
        file: "NikonD70.jpg",
        tag: "Sharpness",
        rule: "ExifIFD 0xa40a is found after the MakerNote at 0x927C",
    },
    Case {
        file: "Olympus2.jpg",
        tag: "ExposureMode",
        rule: "ExifIFD 0xa402 is found after the MakerNote at 0x927C",
    },
    Case {
        file: "FujiFilm.raf",
        tag: "Saturation",
        rule: "RAF: ExifIFD 0xa409 is found after the MakerNote at 0x927C",
    },
    Case {
        file: "SigmaDP2.x3f",
        tag: "ExposureMode",
        rule: "X3F: ExifIFD 0xa402 is found after the MakerNote at 0x927C",
    },
    Case {
        file: "SigmaDP2.x3f",
        tag: "Contrast",
        rule: "Sigma::Main numeric Contrast is Priority => 0",
    },
    Case {
        file: "SigmaDP2.x3f",
        tag: "Software",
        rule: "Sigma::Main Software is Priority => 0",
    },
    Case {
        file: "SigmaDP2.x3f",
        tag: "DriveMode",
        rule: "SigmaRaw::Properties is PRIORITY => 0",
    },
    Case {
        file: "Real.rm",
        tag: "StreamMimeType",
        rule: "Real::MediaProps is PRIORITY => 0: the first stream wins",
    },
    Case {
        file: "Panasonic.rw2",
        tag: "WBRedLevel",
        rule: "the RW2 JpgFromRaw is sub-document Doc1",
    },
    Case {
        file: "PhaseOne.iiq",
        tag: "ImageWidth",
        rule: "IFD1 is not PRIORITY_DIR: its Priority => 0 ImageWidth stays 0",
    },
    Case {
        file: "DNG.dng",
        tag: "ImageWidth",
        rule: "the full-resolution SubIFD is PRIORITY_DIR",
    },
    Case {
        file: "DNG.dng",
        tag: "RowsPerStrip",
        rule: "reduced SubIFDs keep Priority => 0",
    },
    Case {
        file: "MWG.jpg",
        tag: "City",
        rule: "APP13 precedes the XMP APP1 in this file",
    },
    Case {
        file: "ExifTool.jpg",
        tag: "CropLeft",
        rule: "trailers are read from the end inwards: FotoStation last",
    },
    Case {
        file: "ExifTool.jpg",
        tag: "ProfileID",
        rule: "SPIFF APP8 is found before the ICC APP2",
    },
    Case {
        file: "XMP.svg",
        tag: "Title",
        rule: "the C2PA manifest's JSON is a JUMBF sub-document",
    },
    Case {
        file: "XMP.xml",
        tag: "FileType",
        rule: "an XMP tag no table defines is Priority => 0 (XMP.pm:3589-3596)",
    },
    Case {
        file: "CanonRaw.cr3",
        tag: "CreateDate",
        rule: "the Canon uuid box precedes mvhd in moov",
    },
];

fn images(oracle: &exiftool_oracle::Oracle) -> std::path::PathBuf {
    exiftool_oracle::capability_sample(oracle)
        .and_then(|docx| docx.parent().map(Path::to_path_buf))
        .expect("the graded oracle's t/images")
}

fn run(mut command: Command, args: &[&str], file: &Path) -> String {
    let out = command.args(args).arg(file).output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn oxidex() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
}

/// The family-1 groups the oracle's `-a -G1 -s -TAG` reports the name under.
fn oracle_groups(oracle: &exiftool_oracle::Oracle, tag: &str, file: &Path) -> BTreeSet<String> {
    run(
        oracle.command(),
        &["-a", "-G1", "-s", &format!("-{tag}")],
        file,
    )
    .lines()
    .filter_map(|line| {
        let group = line.strip_prefix('[')?.split_once(']')?.0;
        (line.split_whitespace().nth(1) == Some(tag)).then(|| group.to_string())
    })
    .collect()
}

/// The single value `-j -TAG` reports, whatever the key's group prefix:
/// what is graded is which copy was chosen, not the key's spelling.
fn json_value(text: &str, tag: &str) -> Option<Value> {
    let parsed: Value = serde_json::from_str(text).ok()?;
    parsed[0].as_object()?.iter().find_map(|(key, value)| {
        let name = key.rsplit_once(':').map_or(key.as_str(), |(_, name)| name);
        (name == tag).then(|| value.clone())
    })
}

fn grade(cases: &[Case]) {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping bare-name priority parity: no ExifTool oracle may grade output");
        return;
    };
    let images = images(oracle);
    let mut failures = Vec::new();
    for case in cases {
        let file = images.join(case.file);
        let groups = oracle_groups(oracle, case.tag, &file);
        assert!(
            groups.len() >= 2,
            "{} -{}: the oracle reports it in {groups:?}, not in two groups",
            case.file,
            case.tag
        );
        let bare = format!("-{}", case.tag);
        let theirs = run(oracle.command(), &["-s3", &bare], &file);
        let ours = run(oxidex(), &["-s3", &bare], &file);
        if ours != theirs {
            failures.push(format!(
                "{} -s3 -{} ({}): oracle {theirs:?}, oxidex {ours:?}",
                case.file, case.tag, case.rule
            ));
        }
        let theirs = json_value(&run(oracle.command(), &["-j", &bare], &file), case.tag);
        let ours = json_value(&run(oxidex(), &["-j", &bare], &file), case.tag);
        if ours != theirs {
            failures.push(format!(
                "{} -j -{} ({}): oracle {theirs:?}, oxidex {ours:?}",
                case.file, case.tag, case.rule
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "graded against {}:\n{}",
        oracle.provenance(),
        failures.join("\n")
    );
}

#[test]
fn reported_bare_names_answer_as_exiftool_does() {
    grade(REPORTED);
}

#[test]
fn breadth_sample_bare_names_answer_as_exiftool_does() {
    grade(BREADTH);
}

/// ExifTool reads a rational with a zero denominator as `inf` (non-zero
/// numerator) or `undef` (zero numerator) -- `GetRational64u` and its
/// siblings, ExifTool.pm 13.59:6090-6119 -- and a tag with no conversion,
/// like `DigitalZoomRatio` (Exif.pm:2886-2890), prints that string in every
/// output mode. `Casio2.jpg` stores 0/0.
#[test]
fn a_zero_over_zero_rational_is_undef_in_every_output_mode() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping zero-denominator rational parity: no oracle may grade output");
        return;
    };
    let file = images(oracle).join("Casio2.jpg");
    let tag = "-DigitalZoomRatio";
    let mut failures = Vec::new();
    for mode in [&["-s3"][..], &["-s3", "-n"][..]] {
        let args: Vec<&str> = mode.iter().copied().chain([tag]).collect();
        let theirs = run(oracle.command(), &args, &file);
        let ours = run(oxidex(), &args, &file);
        assert_eq!(theirs, "undef\n", "the oracle's own {mode:?} reading");
        if ours != theirs {
            failures.push(format!("{mode:?}: oracle {theirs:?}, oxidex {ours:?}"));
        }
    }
    for mode in [&["-j"][..], &["-j", "-n"][..]] {
        let args: Vec<&str> = mode.iter().copied().chain([tag]).collect();
        let theirs = json_value(&run(oracle.command(), &args, &file), "DigitalZoomRatio");
        let ours = json_value(&run(oxidex(), &args, &file), "DigitalZoomRatio");
        assert_eq!(theirs, Some(Value::String("undef".into())), "{mode:?}");
        if ours != theirs {
            failures.push(format!("{mode:?}: oracle {theirs:?}, oxidex {ours:?}"));
        }
    }
    // The grouped listing renders the same value.
    let theirs = run(oracle.command(), &["-G1", "-s", tag], &file);
    let ours = run(oxidex(), &["-G1", "-s", tag], &file);
    if ours != theirs {
        failures.push(format!("-G1 -s: oracle {theirs:?}, oxidex {ours:?}"));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Each packet occupies its own APP1 position, including packets on both
/// sides of IPTC. Compare the actual crafted bytes with the pinned oracle.
#[test]
fn interleaved_xmp_and_iptc_follow_actual_packet_order() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    fn xmp(city: &str) -> Vec<u8> {
        let mut data = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        data.extend(format!(r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/"><photoshop:City>{city}</photoshop:City></rdf:Description></rdf:RDF>"#).as_bytes());
        data
    }
    fn iptc() -> Vec<u8> {
        let city = b"IPTC-City";
        let mut iim = vec![0x1c, 2, 90];
        iim.extend((city.len() as u16).to_be_bytes());
        iim.extend(city);
        let mut data = b"Photoshop 3.0\0".to_vec();
        data.extend(b"8BIM\x04\x04\x00\x00");
        data.extend((iim.len() as u32).to_be_bytes());
        data.extend(&iim);
        if iim.len() % 2 != 0 {
            data.push(0);
        }
        data
    }
    let packets = [
        (0xe1, xmp("XMP-A-City")),
        (0xed, iptc()),
        (0xe1, xmp("XMP-B-City")),
    ];
    let dir = tempfile::tempdir().unwrap();
    for (order, expected) in [
        ([0, 1, 2], "XMP-B-City\n"),
        ([2, 1, 0], "XMP-A-City\n"),
        ([0, 2, 1], "IPTC-City\n"),
        ([1, 0, 2], "XMP-B-City\n"),
    ] {
        let mut jpeg = vec![0xff, 0xd8];
        for index in order {
            let (marker, data) = &packets[index];
            jpeg.extend([0xff, *marker]);
            jpeg.extend(((data.len() + 2) as u16).to_be_bytes());
            jpeg.extend(data);
        }
        jpeg.extend([0xff, 0xd9]);
        let path = dir.path().join(format!("order-{order:?}.jpg"));
        std::fs::write(&path, jpeg).unwrap();
        let theirs = run(oracle.command(), &["-s3", "-City"], &path);
        assert_eq!(theirs, expected, "oracle packet order {order:?}");
        assert_eq!(
            run(oxidex(), &["-s3", "-City"], &path),
            theirs,
            "packet order {order:?}"
        );
    }
}

#[test]
fn extended_xmp_is_read_once_after_standard_packets() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let xml = |properties: &str| {
        format!(r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/" xmlns:xmpNote="http://ns.adobe.com/xmp/note/" xmlns:pdf="http://ns.adobe.com/pdf/1.3/">{properties}</rdf:Description></rdf:RDF>"#).into_bytes()
    };
    let extension =
        xml("<pdf:Producer>EXT-ONCE</pdf:Producer><photoshop:City>EXT-City</photoshop:City>");
    let guid = format!("{:X}", md5::compute(&extension));
    let main = |city: &str| {
        let mut data = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        data.extend(xml(&format!("<photoshop:City>{city}</photoshop:City><xmpNote:HasExtendedXMP>{guid}</xmpNote:HasExtendedXMP>")));
        data
    };
    let mut extended = b"http://ns.adobe.com/xmp/extension/\0".to_vec();
    extended.extend(guid.as_bytes());
    extended.extend((extension.len() as u32).to_be_bytes());
    extended.extend(0u32.to_be_bytes());
    extended.extend(extension);
    let packets = [main("MAIN-A"), extended, main("MAIN-B")];
    let writer = std::fs::read(images(oracle).join("Writer.jpg")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    for order in [[0, 1, 2], [2, 1, 0]] {
        let mut jpeg = writer[..2].to_vec();
        for index in order {
            let packet = &packets[index];
            jpeg.extend([0xff, 0xe1]);
            jpeg.extend(((packet.len() + 2) as u16).to_be_bytes());
            jpeg.extend(packet);
        }
        jpeg.extend(&writer[2..]);
        let path = dir.path().join(format!("extension-{order:?}.jpg"));
        std::fs::write(&path, jpeg).unwrap();
        let expected = run(
            oracle.command(),
            &["-api", "ExtendedXMP=1", "-a", "-G1", "-s", "-Producer"],
            &path,
        );
        assert_eq!(expected.matches("EXT-ONCE").count(), 1);
        assert_eq!(
            run(oxidex(), &["-a", "-G1", "-s", "-Producer"], &path),
            expected
        );
        let expected = run(
            oracle.command(),
            &["-api", "ExtendedXMP=1", "-s3", "-City"],
            &path,
        );
        assert_eq!(expected, "EXT-City\n");
        assert_eq!(run(oxidex(), &["-s3", "-City"], &path), expected);
    }
}

#[test]
fn cr3_canon_uuid_arbitrates_at_its_moov_child_position() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let original = std::fs::read(images(oracle).join("CanonRaw.cr3")).unwrap();
    fn boxes(bytes: &[u8], start: usize, end: usize) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        let mut at = start;
        while at + 8 <= end {
            let size = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            assert!(size >= 8 && at + size <= end);
            ranges.push((at, at + size));
            at += size;
        }
        assert_eq!(at, end);
        ranges
    }
    let moov = 24; // pinned CanonRaw.cr3 ftyp length, verified below
    assert_eq!(&original[moov + 4..moov + 8], b"moov");
    let end = moov + u32::from_be_bytes(original[moov..moov + 4].try_into().unwrap()) as usize;
    let children = boxes(&original, moov + 8, end);
    let canon = children
        .iter()
        .position(|(a, _)| &original[a + 4..a + 8] == b"uuid")
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    for position in 0..children.len() {
        let mut order: Vec<_> = (0..children.len()).filter(|i| *i != canon).collect();
        order.insert(position, canon);
        let mut data = original[..moov + 8].to_vec();
        for index in order {
            let (a, b) = children[index];
            data.extend(&original[a..b]);
        }
        data.extend(&original[end..]);
        assert_eq!(data.len(), original.len());
        let path = dir.path().join(format!("canon-position-{position}.cr3"));
        std::fs::write(&path, data).unwrap();
        for flags in [
            &["-s3", "-CreateDate"][..],
            &["-a", "-s3", "-CreateDate"][..],
        ] {
            let expected = run(oracle.command(), flags, &path);
            assert!(!expected.is_empty());
            let actual = run(oxidex(), flags, &path);
            if flags.contains(&"-a") {
                let mut actual: Vec<_> = actual.lines().collect();
                let mut expected: Vec<_> = expected.lines().collect();
                actual.sort_unstable();
                expected.sort_unstable();
                assert_eq!(
                    actual, expected,
                    "duplicate values at Canon position {position}"
                );
            } else {
                assert_eq!(actual, expected, "Canon position {position}, {flags:?}");
            }
        }
    }
}
