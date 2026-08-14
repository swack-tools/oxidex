//! Regression tests for six formats that `detect_format` never produced a
//! `FileFormat` variant for, so `read_metadata` fell through to
//! `add_identity_tags` and reported `File:FileType`, `FileTypeExtension`,
//! `MIMEType` and the filesystem tags -- successfully -- while extracting
//! none of their real metadata (`AGENTS.md`, "Detected is not parsed").
//!
//! Every expectation below is the pinned ExifTool 13.59 oracle's own output
//! on the real fixture named in the test, taken with
//! `exiftool-pinned.sh -a -G1 -s <file>`. The fixtures are the ones the
//! coverage census measured against; none of them is synthetic.

use oxidex::core::TagValue;
use oxidex::core::operations::read_metadata;
use std::path::Path;

const SAMPLES: &str = "/tmp/oxidex-exiftool-cache/combined-samples";

/// `Torrent.torrent`, all 21 `Torrent` tags. Covers the bencode reader's
/// three value shapes (integer, text, binary), `ExtractTags`' list-index
/// substitution on both a flat list (`AnnounceList1..3`, from a *nested*
/// list ExifTool flattens one level at a time) and a list of dictionaries
/// (`File1..4Length`/`Path`), `JoinPath` (`docs/README`), and the two
/// declared conversions -- `ConvertUnixTime` on `CreateDate` and
/// `ConvertFileSize` on `File1Length`.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn torrent_fixture_matches_pinned_oracle() {
    let m = read_metadata(Path::new(&format!("{SAMPLES}/Torrent.torrent")))
        .expect("read pinned Torrent fixture");

    assert_eq!(m.get_string("File:FileType"), Some("Torrent"));
    assert_eq!(
        m.get_string("Torrent:Announce"),
        Some("udp://tracker.bogus.com:80/announce")
    );
    assert_eq!(
        m.get_string("Torrent:AnnounceList1"),
        Some("udp://tracker.bogus.com:80/announce")
    );
    assert_eq!(
        m.get_string("Torrent:AnnounceList3"),
        Some("udp://tracker.bogus3.com:80/announce")
    );
    assert_eq!(
        m.get_string("Torrent:Comment"),
        Some("Test BitTorrent description file")
    );
    assert_eq!(m.get_string("Torrent:Creator"), Some("uTorrent/1840"));
    // `ValueConv => 'ConvertUnixTime($val,1)'` (Torrent.pm:40).
    assert_eq!(
        m.get_string("Torrent:CreateDate"),
        Some("2013:09:06 15:33:59+00:00")
    );
    assert_eq!(m.get_string("Torrent:Encoding"), Some("UTF-8"));
    assert_eq!(m.get_string("Torrent:Name"), Some("Image-ExifTool-9.35"));
    // `PrintConv => 'ConvertFileSize($val)'` (Torrent.pm:79).
    assert_eq!(m.get_string("Torrent:File1Length"), Some("3.7 MB"));
    assert_eq!(m.get_string("Torrent:File4Length"), Some("11 kB"));
    assert_eq!(
        m.get_string("Torrent:File2Path"),
        Some("Image-ExifTool-9.35.tar.gz")
    );
    // `JoinPath => 1` (Torrent.pm:81) joins the path components with '/'.
    assert_eq!(m.get_string("Torrent:File4Path"), Some("docs/README"));
    assert_eq!(m.get_integer("Torrent:PieceLength"), Some(1_048_576));
    assert_eq!(
        m.get_string("Torrent:URLList2"),
        Some("http://seed.bogus2.com/")
    );
    // Torrent.pm:170-175: 200 bytes of SHA-1 digests are not printable ASCII
    // and not valid UTF-8, so ExifTool returns them as binary.
    assert!(
        matches!(m.get("Torrent:Pieces"), Some(TagValue::Binary(b)) if b.len() == 200),
        "expected 200-byte binary Pieces, got {:?}",
        m.get("Torrent:Pieces")
    );
}

/// `Palm.mobi`, all 21 `Palm`/`MOBI` tags. Covers `Palm::Main` and
/// `Palm::MOBI` read through the generated binary tables, the `%dateTimeInfo`
/// re-base + `ConvertUnixTime` the generator refused, `ConvertFileSize` on
/// the `conv_dropped` `UncompressedTextLength`, the `BookName` string that
/// replaces its own offset, and cp1252 decoding of the `EXTH` text driven by
/// `CodePage`.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn mobi_fixture_matches_pinned_oracle() {
    let m = read_metadata(Path::new(&format!("{SAMPLES}/Palm.mobi")))
        .expect("read pinned MOBI fixture");

    assert_eq!(m.get_string("File:FileType"), Some("MOBI"));
    assert_eq!(
        m.get_string("Palm:DatabaseName"),
        Some("El_Diezmo_Continua_Vigente")
    );
    assert_eq!(
        m.get_string("Palm:CreateDate"),
        Some("2014:05:28 00:00:51+00:00")
    );
    assert_eq!(
        m.get_string("Palm:ModifyDate"),
        Some("2014:05:28 00:00:51+00:00")
    );
    // ExifTool.pm:6787: a Unix time of exactly zero short-circuits.
    assert_eq!(
        m.get_string("Palm:LastBackupDate"),
        Some("0000:00:00 00:00:00")
    );
    assert_eq!(m.get_integer("Palm:ModificationNumber"), Some(0));
    // `Format => 'undef[8]'` against a string-keyed `PrintConv` hash
    // (Palm.pm:95-99).
    assert_eq!(m.get_string("Palm:PalmFileType"), Some("Mobipocket"));

    assert_eq!(m.get_string("MOBI:Compression"), Some("PalmDOC"));
    // `PrintConv => \&ConvertFileSize` (Palm.pm:123), hand-applied because a
    // Perl code ref is not something the transcription can carry.
    assert_eq!(m.get_string("MOBI:UncompressedTextLength"), Some("172 kB"));
    assert_eq!(m.get_string("MOBI:Encryption"), Some("None"));
    assert_eq!(m.get_string("MOBI:MobiType"), Some("Mobipocket Book"));
    assert_eq!(
        m.get_string("MOBI:CodePage"),
        Some("Windows Latin 1 (Western European)")
    );
    assert_eq!(m.get_integer("MOBI:MobiVersion"), Some(6));
    assert_eq!(m.get_integer("MOBI:MinimumVersion"), Some(6));
    // Palm.pm:326-330: the table's index-21 value is an offset, replaced with
    // the string it points at.
    assert_eq!(
        m.get_string("MOBI:BookName"),
        Some("El Diezmo Continua Vigente")
    );
    assert_eq!(m.get_string("MOBI:Author"), Some("Mike Peralta"));
    assert_eq!(m.get_string("MOBI:Contributor"), Some("Smashwords, Inc."));
    assert_eq!(
        m.get_string("MOBI:CreatorSoftware"),
        Some("Kindlegen (Linux)")
    );
    assert_eq!(m.get_integer("MOBI:CreatorMajorVersion"), Some(1));
    assert_eq!(m.get_integer("MOBI:CreatorMinorVersion"), Some(1));
    assert_eq!(m.get_integer("MOBI:CreatorBuildNumber"), Some(98));
    // The EXTH `Description` is cp1252, not UTF-8: 0x93/0x94 are curly
    // quotes and 0xed/0xfa/0xe1 are i/u/a-acute. Decoding it as UTF-8 would
    // mangle every one of them.
    let description = m
        .get_string("MOBI:Description")
        .expect("MOBI:Description present");
    assert!(
        description.starts_with("Hebreos 7:8  \u{201c}Y aqu\u{ed} ciertamente reciben"),
        "cp1252 decoding failed: {description}"
    );
    assert!(
        description.contains("Jes\u{fa}s ahora recibe"),
        "{description}"
    );
    // Palm.pm:204-227 comments these EXTH IDs out; the fixture carries four
    // of them, and ExifTool reports none.
    for absent in [
        "MOBI:CoverOffset",
        "MOBI:ThumbOffset",
        "MOBI:HasFakeCover",
        "MOBI:FontSignature",
    ] {
        assert_eq!(m.get(absent), None, "{absent} should not be extracted");
    }
}

/// `Font.pfb`, all 16 `PostScript`/`Font`/`File` tags. Covers the six-byte
/// PFB segment header, the DSC comment branch, the `/FontInfo` dictionary
/// (both bracketed and bare values), `UnescapePostScript`'s octal escapes,
/// and the leading-comment accumulator -- including the `$comment = 1` seed
/// that ExifTool stringifies into the front of `File:Comment`.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn pfb_fixture_matches_pinned_oracle() {
    let m =
        read_metadata(Path::new(&format!("{SAMPLES}/Font.pfb"))).expect("read pinned PFB fixture");

    assert_eq!(m.get_string("File:FileType"), Some("PFB"));
    assert_eq!(
        m.get_string("PostScript:Title"),
        Some("NimbusSanL-ReguCondItal")
    );
    assert_eq!(
        m.get_string("PostScript:CreateDate"),
        Some("Sat Sep  4 16:12:41 2004")
    );
    assert_eq!(m.get_string("PostScript:Creator"), Some("frob"));

    assert_eq!(m.get_string("Font:FontType"), Some("1"));
    assert_eq!(
        m.get_string("Font:FontName"),
        Some("NimbusSanL-ReguCondItal")
    );
    // `version` -> `Version` via `AddTagToTable`'s `ucfirst`.
    assert_eq!(m.get_string("Font:Version"), Some("1.06"));
    // `\050`/`\051` are octal escapes for '(' and ')'.
    assert_eq!(
        m.get_string("Font:Notice"),
        Some(
            "Copyright (URW)++,Copyright 1999 by (URW)++ Design & Development; \
             Cyrillic glyphs added by Valek Filippov (C) 2001-2004"
        )
    );
    assert_eq!(
        m.get_string("Font:FullName"),
        Some("Nimbus Sans L Condensed Regular Italic")
    );
    assert_eq!(
        m.get_string("Font:FontFamily"),
        Some("Nimbus Sans L Condensed")
    );
    assert_eq!(m.get_string("Font:Weight"), Some("Regular"));
    assert_eq!(m.get_string("Font:FSType"), Some("12"));
    assert_eq!(m.get_string("Font:ItalicAngle"), Some("-9.9"));
    // `isFixedPitch` -> `IsFixedPitch`, again via `ucfirst`.
    assert_eq!(m.get_string("Font:IsFixedPitch"), Some("false"));
    assert_eq!(m.get_string("Font:UnderlinePosition"), Some("-100"));
    assert_eq!(m.get_string("Font:UnderlineThickness"), Some("50"));

    // PostScript.pm:456 seeds the accumulator with the Perl truth value `1`,
    // which then gets `.=`'d into the string. The leading "1" is ExifTool's,
    // not the file's, and the oracle prints it.
    assert_eq!(
        m.get_string("File:Comment"),
        Some(
            "1\nCopyright (URW)++,Copyright 1999 by (URW)++ Design & Development; Cyri\n\
             Generated by FontForge 20040824 (http://fontforge.sf.net/)"
        )
    );
    // `/Encoding StandardEncoding def` sits inside the font program but is
    // not in `Font::PSInfo`, and `currentdict end` stops the walk.
    assert_eq!(m.get("Font:Encoding"), None);
}

/// `InDesign.indd`, all 9 XMP tags. Covers the two-master-page validation,
/// the sequence-number tiebreak, the object-database offset and the
/// contiguous-object walk down to the XMP stream.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn indd_fixture_matches_pinned_oracle() {
    let m = read_metadata(Path::new(&format!("{SAMPLES}/InDesign.indd")))
        .expect("read pinned INDD fixture");

    assert_eq!(m.get_string("File:FileType"), Some("INDD"));
    // The `XMP-x`/`XMP-rdf`/`XMP-dc`/`XMP-xmp` family-1 spellings the oracle
    // prints are this crate's existing XMP group1 modelling, shared with the
    // standalone `.xmp` sidecar reader -- not something this parser chooses.
    // `conformance.py` scores the two as a match.
    assert_eq!(
        m.get_string("XMP:XMPToolkit"),
        Some("XMP toolkit 3.0-29, framework 1.6")
    );
    assert_eq!(
        m.get_string("XMP:About"),
        Some("d5d09d4b-2831-11dc-bfa2-d89eae7bab84")
    );
    assert_eq!(m.get_string("XMP:CreateDate"), Some("2007:06:30 00:19:02Z"));
    assert_eq!(m.get_string("XMP:CreatorTool"), Some("Adobe InDesign 3.0"));
    assert_eq!(
        m.get_string("XMP:MetadataDate"),
        Some("2007:06:30 00:19:17Z")
    );
    assert_eq!(m.get_string("XMP:ModifyDate"), Some("2007:06:30 00:19:17Z"));
    assert_eq!(
        m.get_string("XMP-xmpMM:DocumentID"),
        Some("adobe:docid:indd:d5d09d4a-2831-11dc-bfa2-d89eae7bab84")
    );
    assert_eq!(m.get_string("XMP-xmpMM:RenditionClass"), Some("default"));
    assert_eq!(m.get_string("XMP:Format"), Some("application/x-indesign"));
}

/// `MacOS.macos`, all 8 `MacOS` tags. Covers the AppleDouble entry table,
/// `ProcessATTR`'s absolute-to-relative offset rebase, `ReadXAttrValue`'s
/// dynamic naming, the `kMDLabel` ID rewrite, the `quarantine` and
/// `lastuseddate#PS` conversions, and four binary-plist values -- one of
/// which is a `CFDate`.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn macos_fixture_matches_pinned_oracle() {
    let m = read_metadata(Path::new(&format!("{SAMPLES}/MacOS.macos")))
        .expect("read pinned MacOS fixture");

    assert_eq!(m.get_string("File:FileType"), Some("MacOS"));
    // MacOS.pm:303-309's `PrintConv`, including `ConvertUnixTime(hex $a[1])`.
    assert_eq!(
        m.get_string("MacOS:XAttrQuarantine"),
        Some("Flags=0082 set at 2020:11:12 12:27:26 by Safari")
    );
    // MacOS.pm:344's `RawConv => 'ConvertUnixTime(unpack("V",$$val))'` --
    // UTC, and no zone suffix, unlike the plist date below.
    assert_eq!(
        m.get_string("MacOS:XAttrLastUsedDate"),
        Some("2020:11:12 12:27:26")
    );
    // A binary plist holding a one-element array of a `CFDate`, which
    // PLIST.pm:279 renders with `ConvertUnixTime($val + 11323*86400, 1)`.
    assert_eq!(
        m.get("MacOS:XAttrMDItemDownloadedDate"),
        Some(&TagValue::Array(vec![TagValue::new_string(
            "2020:11:12 12:27:26+00:00"
        )]))
    );
    assert_eq!(
        m.get("MacOS:XAttrMDItemWhereFroms"),
        Some(&TagValue::Array(vec![TagValue::new_string(
            "https://exiftool.org/test/sample.jpg"
        )]))
    );
    assert_eq!(
        m.get_string("MacOS:XAttrMDItemFinderComment"),
        Some("A Finder comment")
    );
    // A four-element plist array stays a list; ExifTool's JSON output emits
    // it as one, and the newlines inside the first and third items are real.
    assert_eq!(
        m.get("MacOS:XAttrMDItemUserTags"),
        Some(&TagValue::Array(vec![
            TagValue::new_string("Red\n6"),
            TagValue::new_string("Custom1"),
            TagValue::new_string("Yellow\n5"),
            TagValue::new_string("Custom2"),
        ]))
    );
    // MacOS.pm:681 strips the random suffix off `kMDLabel_ooibp6bluksucqtdhkawpzukiy`
    // so the ID matches the table's `Binary => 1` entry.
    assert!(
        matches!(m.get("MacOS:XAttrMDLabel"), Some(TagValue::Binary(b)) if b.len() == 89),
        "expected 89-byte binary XAttrMDLabel, got {:?}",
        m.get("MacOS:XAttrMDLabel")
    );
    // `org.exiftool.metadata:TestTag` is not a `com.apple.` attribute, so it
    // goes down `ReadXAttrValue`'s else branch and then loses its ':' to
    // `AddTagToTable`'s `tr/-_a-zA-Z0-9//dc`.
    assert_eq!(
        m.get_string("MacOS:XAttrOrgExiftoolMetadataTestTag"),
        Some("this is a test tag")
    );
}

/// `BigTIFF.btf`, all 8 `IFD0` tags plus the two composites they feed.
/// Covers the 64-bit entry count, the 20-byte entries with `u64` count and
/// value fields, and the 8-byte inline threshold -- `BitsPerSample` is three
/// `SHORT`s, six bytes, which is inline in BigTIFF and would be an offset in
/// a 4-byte-field TIFF.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn bigtiff_fixture_matches_pinned_oracle() {
    let m = read_metadata(Path::new(&format!("{SAMPLES}/BigTIFF.btf")))
        .expect("read pinned BigTIFF fixture");

    assert_eq!(m.get_string("File:FileType"), Some("BTF"));
    assert_eq!(m.get_integer("IFD0:ImageWidth"), Some(8));
    assert_eq!(m.get_integer("IFD0:ImageHeight"), Some(8));
    assert_eq!(m.get_string("IFD0:BitsPerSample"), Some("8 8 8"));
    // The map holds the raw EXIF value; the `RGB` the oracle prints is the
    // shared TIFF display layer's `PrintConv`, not something this reader
    // applies -- the ordinary TIFF chain stores it the same way.
    assert_eq!(m.get_integer("IFD0:PhotometricInterpretation"), Some(2));
    assert_eq!(m.get_integer("IFD0:StripOffsets"), Some(192));
    assert_eq!(m.get_integer("IFD0:SamplesPerPixel"), Some(3));
    assert_eq!(m.get_integer("IFD0:RowsPerStrip"), Some(8));
    assert_eq!(m.get_integer("IFD0:StripByteCounts"), Some(192));
    assert_eq!(m.get_string("Composite:ImageSize"), Some("8x8"));

    // ExifTool.pm:8661-8667 returns from the BigTIFF branch before the
    // `FoundTag('ExifByteOrder', ...)` at :8702, so -- unlike every other
    // TIFF-based file -- a BigTIFF has none.
    assert_eq!(m.get("File:ExifByteOrder"), None);
}
