//! Coverage-focused integration tests for the REMAINING uncovered code in the
//! document / font / archive / icc / text parser families after wave 1.
//!
//! Wave 1 (`tests/cov_archive_doc_font.rs`) already covered OLE, OOXML
//! (DOCX/XLSX/PPTX), TTF/OTF/WOFF/WOFF2 happy paths, and TXT. This file targets
//! the parsers and branches that wave 1 did not touch:
//!   - document: eml, ics, epub, iwork (pages/numbers/keynote)
//!   - text: vcf, eps
//!   - archive: zip (forensic branches), tar, iso, rar, sevenz (7z), gz
//!   - icc: header field extractors + tag table decoders + standalone file parse
//!   - font: extra TTF/OTF name-table records (Mac/Unicode platforms, many nameIDs)
//!
//! Strategy: build minimal-but-valid byte containers and drive the public parser
//! APIs directly, plus error/malformed-input branches.

#[path = "common/mod.rs"]
mod common;

use common::TestReader;

use oxidex::core::FormatParser;
use oxidex::core::{FileFormat, TagValue};

use std::io::Write;

// Archive parsers
use oxidex::parsers::archive::gz::{GZParser, parse_gz_metadata};
use oxidex::parsers::archive::iso::{ISOParser, parse_iso_metadata};
use oxidex::parsers::archive::rar::{RARParser, parse_rar_metadata};
use oxidex::parsers::archive::sevenz::{SevenZParser, parse_7z_metadata};
use oxidex::parsers::archive::tar::{TARParser, parse_tar_metadata};
use oxidex::parsers::archive::zip::{ZipParser, parse_zip_metadata};

// Document parsers
use oxidex::parsers::document::eml::{EmlParser, parse_eml_metadata};
use oxidex::parsers::document::epub::{EpubParser, parse_epub_metadata};
use oxidex::parsers::document::ics::{ICSParser, parse_ics_metadata};
use oxidex::parsers::document::iwork::{
    KeynoteParser, NumbersParser, PagesParser, parse_keynote_metadata, parse_numbers_metadata,
    parse_pages_metadata,
};

// Text parsers
use oxidex::parsers::text::eps::{EPSParser, parse_eps_metadata};
use oxidex::parsers::text::vcf::{VCFParser, parse_vcf_metadata};

// ICC
use oxidex::parsers::icc::{TagDef, TagType, parse_icc_file, parse_icc_profile_data};

// Font
use oxidex::parsers::font::otf::parse_otf_metadata;
use oxidex::parsers::font::ttf::parse_ttf_metadata;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a ZIP container from (name, bytes) entries using the `zip` crate.
fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::FileOptions::default();
        for (name, data) in entries {
            zip.start_file(*name, options).expect("start zip entry");
            zip.write_all(data).expect("write zip entry");
        }
        zip.finish().expect("finish zip");
    }
    buf
}

/// Build a ZIP whose first entry is `mimetype` stored uncompressed, matching the
/// EPUB OCF requirement.
fn build_epub_zip(mimetype: &str, rest: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let stored =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("mimetype", stored).expect("mimetype");
        zip.write_all(mimetype.as_bytes()).expect("write mimetype");
        let options = zip::write::FileOptions::default();
        for (name, data) in rest {
            zip.start_file(*name, options).expect("start zip entry");
            zip.write_all(data).expect("write zip entry");
        }
        zip.finish().expect("finish zip");
    }
    buf
}

// ===========================================================================
// EML (email) tests
// ===========================================================================

const EML_FULL: &[u8] = b"From: Alice <alice@example.com>\r\n\
To: Bob <bob@example.com>\r\n\
Cc: carol@example.com\r\n\
Bcc: dave@example.com\r\n\
Subject: Quarterly Report\r\n\
Date: Mon, 1 Jan 2024 12:00:00 +0000\r\n\
Message-ID: <abc123@example.com>\r\n\
In-Reply-To: <prev@example.com>\r\n\
References: <r1@example.com> <r2@example.com>\r\n\
Thread-Index: AQHabc\r\n\
Thread-Topic: Topic\r\n\
Received: from a by b\r\n\
Received: from c by d\r\n\
Return-Path: <alice@example.com>\r\n\
X-Originating-IP: 10.0.0.1\r\n\
Authentication-Results: example.com; spf=pass\r\n\
DKIM-Signature: v=1; a=rsa-sha256\r\n\
Received-SPF: pass\r\n\
User-Agent: Thunderbird\r\n\
X-Mailer: Outlook 16\r\n\
Content-Type: multipart/mixed; boundary=xyz; name=\"attach.pdf\"\r\n\
MIME-Version: 1.0\r\n\
Content-Transfer-Encoding: base64\r\n\
Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
X-MS-Exchange-Organization-AuthAs: Internal\r\n\
X-Google-Original-From: alice@gmail.com\r\n\
X-Custom-Header: custom-value\r\n\
\r\n\
Body of the email.";

#[test]
fn eml_full_header_extraction() {
    let reader = TestReader::new(EML_FULL.to_vec());
    let md = parse_eml_metadata(&reader).expect("eml parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("EML".to_string()))
    );
    assert_eq!(
        md.get("EML:From").and_then(|v| v.as_string()),
        Some("Alice <alice@example.com>")
    );
    assert_eq!(
        md.get("EML:Cc").and_then(|v| v.as_string()),
        Some("carol@example.com")
    );
    assert_eq!(
        md.get("EML:Bcc").and_then(|v| v.as_string()),
        Some("dave@example.com")
    );
    assert!(md.contains_key("EML:DateTime")); // parsed RFC 5322 date
    assert_eq!(
        md.get("EML:OriginatingIP").and_then(|v| v.as_string()),
        Some("10.0.0.1")
    );
    // Microsoft Exchange + Google headers are keyed by the lowercased name;
    // the generic X- branch preserves the original key casing.
    assert!(md.contains_key("EML:x-ms-exchange-organization-authas"));
    assert!(md.contains_key("EML:x-google-original-from"));
    assert!(md.contains_key("EML:X-Custom-Header"));
    // Attachment filename from Content-Disposition / Content-Type
    assert!(md.contains_key("EML:AttachmentFilename"));
    // Received headers array
    if let Some(TagValue::Array(rec)) = md.get("EML:Received") {
        assert_eq!(rec.len(), 2);
    } else {
        panic!("expected Received array");
    }
}

#[test]
fn eml_parser_trait_and_supports_format() {
    let parser = EmlParser;
    let reader = TestReader::new(EML_FULL.to_vec());
    assert!(parser.parse(&reader).is_ok());
    assert!(parser.supports_format(FileFormat::EML));
    assert!(!parser.supports_format(FileFormat::ICS));
    // verify_signature true / false
    assert!(EmlParser::verify_signature(&reader).unwrap());
    let bad = TestReader::new(b"hello world not an email".to_vec());
    assert!(!EmlParser::verify_signature(&bad).unwrap());
    let tiny = TestReader::new(b"x".to_vec());
    assert!(!EmlParser::verify_signature(&tiny).unwrap());
}

#[test]
fn eml_no_blank_line_treats_all_as_headers() {
    // No body separator -> whole content is headers.
    let data = b"From: a@b.com\r\nTo: c@d.com\r\nSubject: NoBody";
    let reader = TestReader::new(data.to_vec());
    let md = parse_eml_metadata(&reader).expect("eml parse");
    assert_eq!(
        md.get("EML:Subject").and_then(|v| v.as_string()),
        Some("NoBody")
    );
}

#[test]
fn eml_invalid_signature_errors() {
    let reader = TestReader::new(b"random bytes with no headers here".to_vec());
    assert!(parse_eml_metadata(&reader).is_err());
}

// ===========================================================================
// ICS (iCalendar) tests
// ===========================================================================

const ICS_FULL: &str = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Example//Cal//EN\r\n\
CALSCALE:GREGORIAN\r\n\
METHOD:REQUEST\r\n\
BEGIN:VEVENT\r\n\
DTSTART:20240101T120000Z\r\n\
DTEND:20240101T130000Z\r\n\
SUMMARY:First\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
DTSTART:20240202T090000Z\r\n\
SUMMARY:Second\r\n\
END:VEVENT\r\n\
BEGIN:VTODO\r\n\
SUMMARY:Task\r\n\
END:VTODO\r\n\
END:VCALENDAR";

#[test]
fn ics_full_metadata() {
    let reader = TestReader::new(ICS_FULL.as_bytes().to_vec());
    let md = parse_ics_metadata(&reader).expect("ics parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("ICS".to_string()))
    );
    assert_eq!(
        md.get("MIMEType"),
        Some(&TagValue::String("text/calendar".to_string()))
    );
    assert_eq!(
        md.get("ICS:Version").and_then(|v| v.as_string()),
        Some("2.0")
    );
    assert_eq!(
        md.get("ICS:ProductID").and_then(|v| v.as_string()),
        Some("-//Example//Cal//EN")
    );
    assert_eq!(
        md.get("ICS:CalScale").and_then(|v| v.as_string()),
        Some("GREGORIAN")
    );
    assert_eq!(
        md.get("ICS:Method").and_then(|v| v.as_string()),
        Some("REQUEST")
    );
    assert_eq!(
        md.get("ICS:EventCount").and_then(|v| v.as_integer()),
        Some(2)
    );
    assert_eq!(
        md.get("ICS:TodoCount").and_then(|v| v.as_integer()),
        Some(1)
    );
    // first/last date extraction
    assert!(md.contains_key("ICS:FirstDate"));
    assert!(md.contains_key("ICS:LastDate"));
}

#[test]
fn ics_signature_helpers_and_trait() {
    assert!(ICSParser::verify_signature(ICS_FULL.as_bytes()));
    assert!(!ICSParser::verify_signature(b"not a calendar"));
    // invalid utf8 -> false
    assert!(!ICSParser::verify_signature(&[0xFF, 0xFE, 0x00]));

    let parser = ICSParser;
    assert!(parser.supports_format(FileFormat::ICS));
    assert!(!parser.supports_format(FileFormat::EML));
}

#[test]
fn ics_invalid_errors() {
    let reader = TestReader::new(b"BEGIN:SOMETHING\r\nNOPE".to_vec());
    assert!(parse_ics_metadata(&reader).is_err());
}

// ===========================================================================
// EPUB tests
// ===========================================================================

const CONTAINER_XML: &str = r#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

const OPF_XML: &str = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Adventures</dc:title>
    <dc:creator>Jane Author</dc:creator>
    <dc:subject>Fiction</dc:subject>
    <dc:description>A tale.</dc:description>
    <dc:publisher>Pub House</dc:publisher>
    <dc:date>2024-01-01</dc:date>
    <dc:language>en</dc:language>
    <dc:identifier>urn:isbn:123</dc:identifier>
    <dc:rights>All rights reserved</dc:rights>
  </metadata>
</package>"#;

#[test]
fn epub_full_metadata() {
    let zip = build_epub_zip(
        "application/epub+zip",
        &[
            ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
            ("OEBPS/content.opf", OPF_XML.as_bytes()),
        ],
    );
    let reader = TestReader::new(zip);
    let md = parse_epub_metadata(&reader).expect("epub parse");

    assert_eq!(
        md.get("EPUB:Title").and_then(|v| v.as_string()),
        Some("Adventures")
    );
    assert_eq!(
        md.get("EPUB:Creator").and_then(|v| v.as_string()),
        Some("Jane Author")
    );
    assert_eq!(
        md.get("EPUB:Publisher").and_then(|v| v.as_string()),
        Some("Pub House")
    );
    assert_eq!(
        md.get("EPUB:Language").and_then(|v| v.as_string()),
        Some("en")
    );
    assert!(md.contains_key("EPUB:Subject"));
    assert!(md.contains_key("EPUB:Description"));
    assert!(md.contains_key("EPUB:Date"));
    assert!(md.contains_key("EPUB:Identifier"));
    assert!(md.contains_key("EPUB:Rights"));

    let parser = EpubParser;
    assert!(parser.supports_format(FileFormat::EPUB));
    assert!(!parser.supports_format(FileFormat::ZIP));
}

#[test]
fn epub_wrong_mimetype_errors() {
    let zip = build_epub_zip(
        "application/zip",
        &[("META-INF/container.xml", CONTAINER_XML.as_bytes())],
    );
    let reader = TestReader::new(zip);
    assert!(parse_epub_metadata(&reader).is_err());
}

#[test]
fn epub_missing_mimetype_errors() {
    // A regular zip with no mimetype entry.
    let zip = build_zip(&[("META-INF/container.xml", CONTAINER_XML.as_bytes())]);
    let reader = TestReader::new(zip);
    assert!(parse_epub_metadata(&reader).is_err());
}

#[test]
fn epub_not_a_zip_errors() {
    let reader = TestReader::new(b"definitely not a zip file".to_vec());
    assert!(parse_epub_metadata(&reader).is_err());
}

// ===========================================================================
// iWork (Pages / Numbers / Keynote) tests
// ===========================================================================

const IWORK_META_PLIST: &str = r#"<?xml version="1.0"?>
<plist><dict>
<key>Author</key><string>Steve</string>
<key>Title</key><string>My Doc</string>
</dict></plist>"#;

const IWORK_BUILD_PLIST: &str = r#"<?xml version="1.0"?>
<plist><dict>
<key>BuildVersion</key><string>7029.0.1</string>
</dict></plist>"#;

#[test]
fn pages_full_metadata() {
    let zip = build_zip(&[
        ("Index/Document.iwa", b"\x00\x01binary-iwa"),
        ("Index/Metadata.plist", IWORK_META_PLIST.as_bytes()),
        ("buildVersionHistory.plist", IWORK_BUILD_PLIST.as_bytes()),
    ]);
    let reader = TestReader::new(zip);
    let md = parse_pages_metadata(&reader).expect("pages parse");
    assert_eq!(
        md.get("iWork:Application").and_then(|v| v.as_string()),
        Some("Pages")
    );
    assert_eq!(
        md.get("iWork:Author").and_then(|v| v.as_string()),
        Some("Steve")
    );
    assert_eq!(
        md.get("iWork:Title").and_then(|v| v.as_string()),
        Some("My Doc")
    );
    assert_eq!(
        md.get("iWork:BuildVersion").and_then(|v| v.as_string()),
        Some("7029.0.1")
    );

    let parser = PagesParser;
    assert!(parser.supports_format(FileFormat::Pages));
    assert!(!parser.supports_format(FileFormat::Numbers));
}

#[test]
fn numbers_minimal_metadata() {
    let zip = build_zip(&[("Index/Document.iwa", b"data")]);
    let reader = TestReader::new(zip);
    let md = parse_numbers_metadata(&reader).expect("numbers parse");
    assert_eq!(
        md.get("iWork:Application").and_then(|v| v.as_string()),
        Some("Numbers")
    );
    let parser = NumbersParser;
    assert!(parser.supports_format(FileFormat::Numbers));
}

#[test]
fn keynote_minimal_metadata() {
    let zip = build_zip(&[("Index/Presentation.iwa", b"data")]);
    let reader = TestReader::new(zip);
    let md = parse_keynote_metadata(&reader).expect("keynote parse");
    assert_eq!(
        md.get("iWork:Application").and_then(|v| v.as_string()),
        Some("Keynote")
    );
    let parser = KeynoteParser;
    assert!(parser.supports_format(FileFormat::Keynote));
}

#[test]
fn iwork_missing_index_errors() {
    // A valid zip but missing the expected iWork structure file.
    let zip = build_zip(&[("random.txt", b"nope")]);
    let reader = TestReader::new(zip);
    assert!(parse_pages_metadata(&reader).is_err());
    let reader2 = TestReader::new(build_zip(&[("random.txt", b"nope")]));
    assert!(parse_keynote_metadata(&reader2).is_err());
}

#[test]
fn iwork_not_a_zip_errors() {
    let reader = TestReader::new(b"not zip data at all here".to_vec());
    assert!(parse_numbers_metadata(&reader).is_err());
}

// ===========================================================================
// VCF (vCard) tests
// ===========================================================================

const VCF_FULL: &str = "BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
FN:John Smith\r\n\
EMAIL:john@example.com\r\n\
TEL:+1-555-1234\r\n\
ORG:Acme Inc\r\n\
ADR:;;123 Main St;Town;;12345;USA\r\n\
URL:https://example.com\r\n\
PHOTO:https://example.com/p.jpg\r\n\
END:VCARD\r\n\
BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
NICKNAME:Second\r\n\
END:VCARD";

#[test]
fn vcf_full_metadata() {
    let reader = TestReader::new(VCF_FULL.as_bytes().to_vec());
    let md = parse_vcf_metadata(&reader).expect("vcf parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("vCard".to_string()))
    );
    assert_eq!(
        md.get("VCardVersion").and_then(|v| v.as_string()),
        Some("3.0")
    );
    assert_eq!(
        md.get("VCF:Version").and_then(|v| v.as_string()),
        Some("3.0")
    );
    assert_eq!(
        md.get("FullName").and_then(|v| v.as_string()),
        Some("John Smith")
    );
    assert_eq!(
        md.get("Email").and_then(|v| v.as_string()),
        Some("john@example.com")
    );
    assert_eq!(
        md.get("Telephone").and_then(|v| v.as_string()),
        Some("+1-555-1234")
    );
    // Two BEGIN:VCARD blocks -> count of 2
    assert_eq!(md.get("VCF:Count").and_then(|v| v.as_integer()), Some(2));
    // Feature flags
    assert_eq!(
        md.get("VCF:HasPhoto").and_then(|v| v.as_string()),
        Some("true")
    );
    assert_eq!(
        md.get("VCF:HasOrganization").and_then(|v| v.as_string()),
        Some("true")
    );
    assert_eq!(
        md.get("VCF:HasEmail").and_then(|v| v.as_string()),
        Some("true")
    );
    assert_eq!(
        md.get("VCF:HasPhone").and_then(|v| v.as_string()),
        Some("true")
    );
    assert_eq!(
        md.get("VCF:HasAddress").and_then(|v| v.as_string()),
        Some("true")
    );
    assert_eq!(
        md.get("VCF:HasURL").and_then(|v| v.as_string()),
        Some("true")
    );
}

#[test]
fn vcf_minimal_flags_false() {
    let data = "BEGIN:VCARD\r\nVERSION:2.1\r\nFN:Only Name\r\nEND:VCARD";
    let reader = TestReader::new(data.as_bytes().to_vec());
    let md = parse_vcf_metadata(&reader).expect("vcf parse");
    assert_eq!(
        md.get("VCF:HasPhoto").and_then(|v| v.as_string()),
        Some("false")
    );
    assert_eq!(
        md.get("VCF:HasEmail").and_then(|v| v.as_string()),
        Some("false")
    );
    assert_eq!(md.get("VCF:Count").and_then(|v| v.as_integer()), Some(1));
}

#[test]
fn vcf_signature_helpers_and_trait() {
    let reader = TestReader::new(VCF_FULL.as_bytes().to_vec());
    assert!(VCFParser::verify_signature(&reader).unwrap());
    let tiny = TestReader::new(b"BEGIN".to_vec());
    assert!(!VCFParser::verify_signature(&tiny).unwrap());
    let bad = TestReader::new(b"NOTAVCARDHEADER!!".to_vec());
    assert!(!VCFParser::verify_signature(&bad).unwrap());

    let parser = VCFParser;
    assert!(parser.supports_format(FileFormat::VCF));
    assert!(!parser.supports_format(FileFormat::EML));
}

#[test]
fn vcf_invalid_signature_errors() {
    let reader = TestReader::new(b"NOTAVCARD000000000".to_vec());
    assert!(parse_vcf_metadata(&reader).is_err());
}

// ===========================================================================
// EPS tests
// ===========================================================================

const EPS_FULL: &str = "%!PS-Adobe-3.0 EPSF-3.0\n\
%%Creator: Adobe Illustrator\n\
%%Title: (Sample Art)\n\
%%CreationDate: 2024/03/15\n\
%%For: Designer\n\
%%BoundingBox: 0 0 612 792\n\
%%HiResBoundingBox: 0.0 0.0 612.0 792.0\n\
%%DocumentData: Clean7Bit\n\
%%LanguageLevel: 2\n\
%%Pages: 1\n\
%%ImageData: 100 100 8 3\n\
%%EndComments\n\
showpage\n";

#[test]
fn eps_full_dsc_metadata() {
    let reader = TestReader::new(EPS_FULL.as_bytes().to_vec());
    let md = parse_eps_metadata(&reader).expect("eps parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("EPS".to_string()))
    );
    assert_eq!(
        md.get("MIMEType"),
        Some(&TagValue::String("application/postscript".to_string()))
    );
    assert_eq!(
        md.get("PostScript:Creator").and_then(|v| v.as_string()),
        Some("Adobe Illustrator")
    );
    assert_eq!(
        md.get("EPS:Creator").and_then(|v| v.as_string()),
        Some("Adobe Illustrator")
    );
    // Title parentheses stripped
    assert_eq!(
        md.get("PostScript:Title").and_then(|v| v.as_string()),
        Some("Sample Art")
    );
    assert_eq!(
        md.get("EPS:Title").and_then(|v| v.as_string()),
        Some("Sample Art")
    );
    assert_eq!(
        md.get("EPS:For").and_then(|v| v.as_string()),
        Some("Designer")
    );
    assert!(md.contains_key("PostScript:BoundingBox"));
    assert!(md.contains_key("EPS:BoundingBox"));
    assert!(md.contains_key("PostScript:HiResBoundingBox"));
    assert!(md.contains_key("PostScript:DocumentData"));
    assert!(md.contains_key("PostScript:LanguageLevel"));
    assert!(md.contains_key("PostScript:ImageData"));
    // Pages parsed to integer
    assert_eq!(md.get("EPS:Pages").and_then(|v| v.as_integer()), Some(1));
}

#[test]
fn eps_atend_values_skipped() {
    let data = "%!PS-Adobe-3.0 EPSF-3.0\n\
%%BoundingBox: (atend)\n\
%%Pages: (atend)\n\
%%EndComments\n";
    let reader = TestReader::new(data.as_bytes().to_vec());
    let md = parse_eps_metadata(&reader).expect("eps parse");
    // (atend) values are not stored.
    assert!(!md.contains_key("PostScript:BoundingBox"));
    assert!(!md.contains_key("EPS:Pages"));
}

#[test]
fn eps_binary_dos_header() {
    // DOS EPS binary container: 0xC5D0D3C6 magic + offsets to a PS section.
    let ps_section = b"%!PS-Adobe-3.0 EPSF-3.0\n%%Creator: BinaryTool\n%%EndComments\n";
    let header_len = 30usize;
    let ps_start = header_len as u32;
    let ps_len = ps_section.len() as u32;

    let mut data = vec![0u8; header_len];
    data[0] = 0xC5;
    data[1] = 0xD0;
    data[2] = 0xD3;
    data[3] = 0xC6;
    // PS section offset (LE) at bytes 4..8
    data[4..8].copy_from_slice(&ps_start.to_le_bytes());
    // PS section length (LE) at bytes 8..12
    data[8..12].copy_from_slice(&ps_len.to_le_bytes());
    data.extend_from_slice(ps_section);

    let reader = TestReader::new(data);
    let md = parse_eps_metadata(&reader).expect("binary eps parse");
    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("EPS".to_string()))
    );
    assert_eq!(
        md.get("PostScript:Creator").and_then(|v| v.as_string()),
        Some("BinaryTool")
    );
}

#[test]
fn eps_signature_helpers_and_trait() {
    assert!(EPSParser::verify_signature(b"%!PS-Adobe-3.0"));
    assert!(EPSParser::verify_signature(&[0xC5, 0xD0, 0xD3, 0xC6, 0x00]));
    assert!(!EPSParser::verify_signature(b"not eps"));

    let parser = EPSParser;
    assert!(parser.supports_format(FileFormat::EPS));
    assert!(!parser.supports_format(FileFormat::TXT));
}

#[test]
fn eps_invalid_errors() {
    let reader = TestReader::new(b"not an eps file at all".to_vec());
    assert!(parse_eps_metadata(&reader).is_err());
}

// ===========================================================================
// ZIP (forensic branches not hit by wave 1's helper) tests
// ===========================================================================

#[test]
fn zip_forensic_fields() {
    // Build a non-trivial zip via the zip crate to exercise per-file + summary.
    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut zip = zip::ZipWriter::new(cursor);
        zip.set_comment("forensic test");
        let stored = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .last_modified_time(zip::DateTime::from_date_and_time(2020, 6, 1, 8, 0, 0).unwrap());
        zip.start_file("a.bin", stored).unwrap();
        zip.write_all(b"stored payload").unwrap();
        let deflated = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(
                zip::DateTime::from_date_and_time(2022, 12, 31, 23, 59, 59).unwrap(),
            );
        zip.start_file("b.txt", deflated).unwrap();
        zip.write_all(&b"X".repeat(2000)).unwrap();
        zip.finish().unwrap();
    }

    let reader = TestReader::new(buf);
    let md = parse_zip_metadata(&reader).expect("zip parse");

    assert_eq!(
        md.get("ZIP:FileCount").and_then(|v| v.as_integer()),
        Some(2)
    );
    assert_eq!(
        md.get("ZIP:Comment").and_then(|v| v.as_string()),
        Some("forensic test")
    );
    assert!(md.contains_key("ZIP:TotalCompressedSize"));
    assert!(md.contains_key("ZIP:TotalUncompressedSize"));
    assert!(md.contains_key("ZIP:CompressedSize"));
    assert!(md.contains_key("ZIP:UncompressedSize"));
    assert!(md.contains_key("ZIP:CompressionMethod"));
    assert!(md.contains_key("ZIP:CompressionRatio"));
    assert!(md.contains_key("ZIP:CreationDate"));
    // Date range across the two distinct file timestamps.
    assert_eq!(
        md.get("ZIP:OldestFileDate").and_then(|v| v.as_string()),
        Some("2020-06-01T08:00:00")
    );
    // DOS timestamps have 2-second resolution, so :59 rounds down to :58.
    assert_eq!(
        md.get("ZIP:NewestFileDate").and_then(|v| v.as_string()),
        Some("2022-12-31T23:59:58")
    );
    // Not self-extracting (starts with PK), not zip64.
    assert_eq!(
        md.get("ZIP:SelfExtractingArchive")
            .and_then(|v| v.as_string()),
        Some("false")
    );
    assert!(!md.contains_key("ZIP:IsZIP64"));

    let parser = ZipParser;
    assert!(parser.supports_format(FileFormat::ZIP));
    assert!(!parser.supports_format(FileFormat::TAR));
}

#[test]
fn zip_self_extracting_detection() {
    // Prepend an executable stub before a valid zip; signature check still passes
    // (the parser reads "PK" only for the first-two-byte gate, but the SFX flag
    // is computed against the first 4 bytes, which here do not start with PK).
    // To keep ZipArchive happy we instead test the plain invalid path below; here
    // we confirm a normal archive reports SelfExtractingArchive=false.
    let zip = build_zip(&[("only.txt", b"data")]);
    let reader = TestReader::new(zip);
    let md = parse_zip_metadata(&reader).expect("zip parse");
    assert_eq!(
        md.get("ZIP:SelfExtractingArchive")
            .and_then(|v| v.as_string()),
        Some("false")
    );
}

#[test]
fn zip_too_small_and_bad_signature_error() {
    assert!(parse_zip_metadata(&TestReader::new(vec![0x01, 0x02])).is_err());
    assert!(parse_zip_metadata(&TestReader::new(b"NOTPKZIPDATA".to_vec())).is_err());
}

// ===========================================================================
// TAR tests
// ===========================================================================

/// Build a single 512-byte ustar header block.
fn tar_header(
    name: &str,
    size: u64,
    mtime: u64,
    typeflag: u8,
    uname: &str,
    gname: &str,
) -> Vec<u8> {
    let mut h = vec![0u8; 512];
    let nb = name.as_bytes();
    h[..nb.len().min(100)].copy_from_slice(&nb[..nb.len().min(100)]);
    h[100..107].copy_from_slice(b"0000644");
    h[124..136].copy_from_slice(format!("{:011o}\0", size).as_bytes());
    h[136..148].copy_from_slice(format!("{:011o}\0", mtime).as_bytes());
    h[148..156].copy_from_slice(b"        ");
    h[156] = typeflag;
    h[257..262].copy_from_slice(b"ustar");
    // version "00" sits immediately after the 5-byte "ustar" magic (offset 262)
    h[262..264].copy_from_slice(b"00");
    let ub = uname.as_bytes();
    h[265..265 + ub.len().min(32)].copy_from_slice(&ub[..ub.len().min(32)]);
    let gb = gname.as_bytes();
    h[297..297 + gb.len().min(32)].copy_from_slice(&gb[..gb.len().min(32)]);
    let checksum: u32 = h.iter().map(|&b| b as u32).sum();
    h[148..156].copy_from_slice(format!("{:06o}\0 ", checksum).as_bytes());
    h
}

#[test]
fn tar_full_metadata_and_helpers() {
    let mut data = Vec::new();
    // regular file (1024 bytes content -> 2 blocks)
    data.extend_from_slice(&tar_header(
        "file1.txt",
        1024,
        1_609_459_200,
        b'0',
        "alice",
        "staff",
    ));
    data.extend(vec![0u8; 1024]);
    // directory entry
    data.extend_from_slice(&tar_header(
        "subdir/",
        0,
        1_609_459_200,
        b'5',
        "alice",
        "staff",
    ));
    // symlink entry
    data.extend_from_slice(&tar_header("link", 0, 1_609_459_300, b'2', "bob", "admin"));
    // second regular file (512 bytes content -> 1 block)
    data.extend_from_slice(&tar_header(
        "subdir/f2",
        512,
        1_609_459_400,
        b'0',
        "bob",
        "admin",
    ));
    data.extend(vec![0u8; 512]);
    // end-of-archive zero blocks
    data.extend(vec![0u8; 1024]);

    let reader = TestReader::new(data);
    let md = parse_tar_metadata(&reader).expect("tar parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("TAR".to_string()))
    );
    assert_eq!(
        md.get("TAR:FileFormat").and_then(|v| v.as_string()),
        Some("POSIX")
    );
    assert_eq!(
        md.get("TAR:BlockSize").and_then(|v| v.as_integer()),
        Some(512)
    );
    assert_eq!(md.get("FileCount").and_then(|v| v.as_integer()), Some(4));
    assert_eq!(
        md.get("TAR:FileCount").and_then(|v| v.as_integer()),
        Some(4)
    );
    assert_eq!(
        md.get("RegularFileCount").and_then(|v| v.as_integer()),
        Some(2)
    );
    assert_eq!(
        md.get("DirectoryCount").and_then(|v| v.as_integer()),
        Some(1)
    );
    assert_eq!(md.get("SymLinkCount").and_then(|v| v.as_integer()), Some(1));
    assert_eq!(
        md.get("TotalUncompressedSize").and_then(|v| v.as_integer()),
        Some(1536)
    );
    assert_eq!(
        md.get("TAR:TotalSize").and_then(|v| v.as_integer()),
        Some(1536)
    );
    assert_eq!(
        md.get("TAR:CompressionMethod").and_then(|v| v.as_string()),
        Some("None")
    );
    assert!(md.contains_key("TAR:Permissions"));
    assert!(md.contains_key("TAR:OwnerID"));
    assert!(md.contains_key("TAR:GroupID"));
    assert_eq!(
        md.get("FirstFileName").and_then(|v| v.as_string()),
        Some("file1.txt")
    );
    assert_eq!(
        md.get("FirstFileSize").and_then(|v| v.as_integer()),
        Some(1024)
    );
    assert_eq!(
        md.get("FirstFileOwner").and_then(|v| v.as_string()),
        Some("alice")
    );
    assert_eq!(
        md.get("FirstFileGroup").and_then(|v| v.as_string()),
        Some("staff")
    );
    assert!(md.contains_key("FirstFileModifyDate"));
}

#[test]
fn tar_signature_and_version_helpers() {
    // POSIX ("00")
    let mut posix = vec![0u8; 264];
    posix[257..262].copy_from_slice(b"ustar");
    posix[262..264].copy_from_slice(b"00");
    let r = TestReader::new(posix);
    assert!(TARParser::verify_signature(&r).unwrap());
    assert_eq!(TARParser::read_version(&r).unwrap(), "POSIX");

    // GNU ("\0\0")
    let mut gnu = vec![0u8; 264];
    gnu[257..262].copy_from_slice(b"ustar");
    // bytes 262,263 stay zero -> GNU
    let rg = TestReader::new(gnu);
    assert_eq!(TARParser::read_version(&rg).unwrap(), "GNU");

    // too small -> false / Unknown
    let small = TestReader::new(vec![0u8; 100]);
    assert!(!TARParser::verify_signature(&small).unwrap());
    assert_eq!(TARParser::read_version(&small).unwrap(), "Unknown");

    let parser = TARParser;
    assert!(parser.supports_format(FileFormat::TAR));
    assert!(!parser.supports_format(FileFormat::ZIP));
}

#[test]
fn tar_invalid_signature_errors() {
    let reader = TestReader::new(vec![0u8; 600]);
    assert!(parse_tar_metadata(&reader).is_err());
}

// ===========================================================================
// ISO 9660 tests
// ===========================================================================

fn build_iso() -> Vec<u8> {
    let mut data = vec![0u8; 33_700];
    data[32768] = 0x01; // primary volume descriptor
    data[32769..32774].copy_from_slice(b"CD001");
    // System ID @ +8
    data[32776..32781].copy_from_slice(b"LINUX");
    // Volume ID @ +40
    data[32808..32824].copy_from_slice(b"TEST_DISC_VOLUME");
    // Volume space size (both-endian) @ +80
    data[32848..32852].copy_from_slice(&10_000u32.to_le_bytes());
    data[32852..32856].copy_from_slice(&10_000u32.to_be_bytes());
    // Block size (both-endian) @ +128
    data[32896..32900].copy_from_slice(&2048u32.to_le_bytes());
    data[32900..32904].copy_from_slice(&2048u32.to_be_bytes());
    // Publisher ID @ +318
    data[33086..33100].copy_from_slice(b"TEST PUBLISHER");
    // Application ID @ +574
    data[33342..33349].copy_from_slice(b"MKISOFS");
    // Creation date @ +813
    data[33581..33598].copy_from_slice(b"20240315143045000");
    data
}

#[test]
fn iso_full_metadata() {
    let reader = TestReader::new(build_iso());
    let md = parse_iso_metadata(&reader).expect("iso parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("ISO".to_string()))
    );
    assert_eq!(
        md.get("VolumeDescriptorType").and_then(|v| v.as_string()),
        Some("1")
    );
    assert_eq!(
        md.get("VolumeID").and_then(|v| v.as_string()),
        Some("TEST_DISC_VOLUME")
    );
    assert_eq!(
        md.get("SystemID").and_then(|v| v.as_string()),
        Some("LINUX")
    );
    assert_eq!(
        md.get("BlockSize").and_then(|v| v.as_string()),
        Some("2048")
    );
    assert_eq!(
        md.get("VolumeSize").and_then(|v| v.as_string()),
        Some("20480000")
    );
    assert_eq!(
        md.get("PublisherID").and_then(|v| v.as_string()),
        Some("TEST PUBLISHER")
    );
    assert_eq!(
        md.get("ApplicationID").and_then(|v| v.as_string()),
        Some("MKISOFS")
    );
    assert_eq!(
        md.get("CreationDate").and_then(|v| v.as_string()),
        Some("2024:03:15 14:30:45")
    );
}

#[test]
fn iso_signature_and_descriptor_helpers() {
    let reader = TestReader::new(build_iso());
    assert!(ISOParser::verify_signature(&reader).unwrap());
    assert_eq!(ISOParser::read_descriptor_type(&reader).unwrap(), 1);

    let small = TestReader::new(vec![0u8; 100]);
    assert!(!ISOParser::verify_signature(&small).unwrap());
    assert_eq!(ISOParser::read_descriptor_type(&small).unwrap(), 0);

    let parser = ISOParser;
    assert!(parser.supports_format(FileFormat::ISO));
    assert!(!parser.supports_format(FileFormat::TAR));
}

#[test]
fn iso_invalid_signature_errors() {
    let reader = TestReader::new(vec![0u8; 33_000]);
    assert!(parse_iso_metadata(&reader).is_err());
}

// ===========================================================================
// RAR tests (RAR4 + RAR5)
// ===========================================================================

#[test]
fn rar4_full_metadata() {
    let mut data = b"Rar!".to_vec();
    data.extend_from_slice(&[0x1A, 0x07, 0x00]); // RAR4 signature
    // Archive header (type 0x73) with solid + volume flags (0x09)
    data.extend_from_slice(&[0x33, 0x92, 0x73, 0x09, 0x00, 0x0D, 0x00]);
    data.extend_from_slice(&[0x00; 6]);
    // a file header block (type 0x74)
    data.extend_from_slice(&[0x33, 0x92, 0x74, 0x00, 0x00, 0x20, 0x00]);
    data.extend_from_slice(&[0x00; 25]);

    let reader = TestReader::new(data);
    let md = parse_rar_metadata(&reader).expect("rar parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("RAR".to_string()))
    );
    assert_eq!(
        md.get("RARVersion").and_then(|v| v.as_string()),
        Some("4.x")
    );
    assert_eq!(md.get("IsSolid").and_then(|v| v.as_string()), Some("true"));
    assert_eq!(md.get("IsVolume").and_then(|v| v.as_string()), Some("true"));
    assert!(md.contains_key("IsEncrypted"));
    assert!(md.contains_key("HasRecoveryRecord"));
    assert!(md.contains_key("HasComment"));
    assert!(md.contains_key("IsLocked"));
    // standardized RAR: tags
    assert!(md.contains_key("RAR:SolidArchive"));
    assert!(md.contains_key("RAR:CompressionMethod"));
    assert_eq!(
        md.get("RAR:CreateDate").and_then(|v| v.as_string()),
        Some("Unknown")
    );
    assert_eq!(
        md.get("RAR:CompressedSize").and_then(|v| v.as_integer()),
        Some(0)
    );
    assert!(md.contains_key("RAR:HeaderCRC"));
    assert!(md.contains_key("FileCount"));
}

#[test]
fn rar5_metadata() {
    // RAR5 is detected when byte[7] == 0x01.
    let mut data = b"Rar!".to_vec();
    data.extend_from_slice(&[0x1A, 0x07, 0x01, 0x01]);
    // Main archive header: CRC(4) + vint sizes/types/flags.
    // header_crc (4 bytes), header_size vint, header_type vint=1 (main), flags vint=0
    data.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // crc
    data.push(0x08); // header_size vint = 8
    data.push(0x01); // header_type vint = 1 (main)
    data.push(0x00); // flags vint = 0
    // pad so parse_rar5_metadata can read 32 bytes from offset 8 (needs >= 40 total)
    data.extend_from_slice(&[0x00; 40]);

    let reader = TestReader::new(data);
    let md = parse_rar_metadata(&reader).expect("rar5 parse");
    assert_eq!(
        md.get("RARVersion").and_then(|v| v.as_string()),
        Some("5.0")
    );
    assert!(md.contains_key("IsVolume"));
    assert!(md.contains_key("IsSolid"));
    assert!(md.contains_key("IsEncrypted"));
}

#[test]
fn rar_signature_and_version_helpers() {
    let mut r4 = b"Rar!".to_vec();
    r4.extend_from_slice(&[0x1A, 0x07, 0x00, 0x00]);
    let reader4 = TestReader::new(r4);
    assert!(RARParser::verify_signature(&reader4).unwrap());
    assert_eq!(RARParser::detect_version(&reader4).unwrap(), "4.x");

    let mut r5 = b"Rar!".to_vec();
    r5.extend_from_slice(&[0x1A, 0x07, 0x01, 0x01]);
    let reader5 = TestReader::new(r5);
    assert_eq!(RARParser::detect_version(&reader5).unwrap(), "5.0");

    let bad = TestReader::new(b"NOTRAR!".to_vec());
    assert!(!RARParser::verify_signature(&bad).unwrap());
    let tiny = TestReader::new(vec![0x52, 0x61]);
    assert!(!RARParser::verify_signature(&tiny).unwrap());
    assert_eq!(RARParser::detect_version(&tiny).unwrap(), "Unknown");

    let parser = RARParser;
    assert!(parser.supports_format(FileFormat::RAR));
    assert!(!parser.supports_format(FileFormat::ZIP));
}

#[test]
fn rar_invalid_signature_errors() {
    let reader = TestReader::new(b"NOTARAR12345".to_vec());
    assert!(parse_rar_metadata(&reader).is_err());
}

// ===========================================================================
// 7z (SevenZ) tests
// ===========================================================================

fn build_7z(major: u8, minor: u8, next_off: u64, next_size: u64) -> Vec<u8> {
    let mut data = vec![0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, major, minor];
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // start header crc
    data.extend_from_slice(&next_off.to_le_bytes()); // next header offset (8)
    data.extend_from_slice(&next_size.to_le_bytes()); // next header size (8)
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // next header crc
    data
}

#[test]
fn sevenz_full_metadata() {
    let data = build_7z(0, 4, 32, 64);
    let reader = TestReader::new(data);
    let md = parse_7z_metadata(&reader).expect("7z parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("7z".to_string()))
    );
    assert_eq!(md.get("7zVersion").and_then(|v| v.as_string()), Some("0.4"));
    assert_eq!(
        md.get("NextHeaderOffset").and_then(|v| v.as_string()),
        Some("32")
    );
    assert_eq!(
        md.get("NextHeaderSize").and_then(|v| v.as_string()),
        Some("64")
    );
    assert!(md.contains_key("StartHeaderCRC"));
    assert!(md.contains_key("NextHeaderCRC"));
    assert!(md.contains_key("DataOffset"));
    assert!(md.contains_key("HeaderSize"));
    assert!(md.contains_key("HeaderOverhead"));
    assert!(md.contains_key("HeaderCRCValid"));
    assert_eq!(
        md.get("HasEncodedHeader").and_then(|v| v.as_string()),
        Some("true")
    );
}

#[test]
fn sevenz_no_encoded_header() {
    // next_header_size == 0 -> HasEncodedHeader false.
    let data = build_7z(1, 0, 0, 0);
    let reader = TestReader::new(data);
    let md = parse_7z_metadata(&reader).expect("7z parse");
    assert_eq!(md.get("7zVersion").and_then(|v| v.as_string()), Some("1.0"));
    assert_eq!(
        md.get("HasEncodedHeader").and_then(|v| v.as_string()),
        Some("false")
    );
}

#[test]
fn sevenz_signature_helpers_and_errors() {
    let data = build_7z(0, 4, 0, 0);
    let reader = TestReader::new(data);
    assert!(SevenZParser::verify_signature(&reader).unwrap());

    let tiny = TestReader::new(vec![0x37, 0x7A]);
    assert!(!SevenZParser::verify_signature(&tiny).unwrap());
    let bad = TestReader::new(b"NOT7Z!".to_vec());
    assert!(!SevenZParser::verify_signature(&bad).unwrap());

    // valid magic but too short for the 32-byte start header -> parse error
    let short = TestReader::new(vec![0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, 0x00, 0x04]);
    assert!(parse_7z_metadata(&short).is_err());

    assert!(parse_7z_metadata(&TestReader::new(b"NOTASEVENZ".to_vec())).is_err());

    let parser = SevenZParser;
    assert!(parser.supports_format(FileFormat::SevenZ));
    assert!(!parser.supports_format(FileFormat::ZIP));
}

// ===========================================================================
// GZIP tests
// ===========================================================================

/// Build a gzip stream with FNAME + FCOMMENT + FEXTRA + FHCRC optional fields.
fn build_gz_with_options() -> Vec<u8> {
    let mut d = Vec::new();
    d.push(0x1F);
    d.push(0x8B);
    d.push(0x08); // method DEFLATE
    // flags: FEXTRA|FNAME|FCOMMENT|FHCRC
    d.push(0x04 | 0x08 | 0x10 | 0x02);
    d.extend_from_slice(&1_700_000_000u32.to_le_bytes()); // mtime
    d.push(0x02); // XFL = max compression
    d.push(0x03); // OS = Unix
    // FEXTRA: xlen(2) + payload
    let extra = b"AB\x01\x00\x00"; // subfield id + len + 0
    d.extend_from_slice(&(extra.len() as u16).to_le_bytes());
    d.extend_from_slice(extra);
    // FNAME (null-terminated)
    d.extend_from_slice(b"original.txt\0");
    // FCOMMENT (null-terminated)
    d.extend_from_slice(b"a comment\0");
    // FHCRC (2 bytes)
    d.extend_from_slice(&[0xAB, 0xCD]);
    // some deflate-ish body
    d.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
    // trailer: CRC32(4) + ISIZE(4)
    d.extend_from_slice(&0xDEADBEEFu32.to_le_bytes());
    d.extend_from_slice(&12345u32.to_le_bytes());
    d
}

#[test]
fn gz_full_with_optional_fields() {
    let reader = TestReader::new(build_gz_with_options());
    let md = parse_gz_metadata(&reader).expect("gz parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("GZIP".to_string()))
    );
    assert_eq!(
        md.get("CompressionMethod").and_then(|v| v.as_string()),
        Some("DEFLATE")
    );
    assert_eq!(
        md.get("CompressionLevel").and_then(|v| v.as_string()),
        Some("Maximum compression")
    );
    assert_eq!(
        md.get("OperatingSystem").and_then(|v| v.as_string()),
        Some("Unix")
    );
    assert!(md.contains_key("ModificationTime"));
    assert_eq!(
        md.get("OriginalFileName").and_then(|v| v.as_string()),
        Some("original.txt")
    );
    assert_eq!(
        md.get("Comment").and_then(|v| v.as_string()),
        Some("a comment")
    );
    assert_eq!(
        md.get("CRC32").and_then(|v| v.as_string()),
        Some("0xDEADBEEF")
    );
    assert_eq!(
        md.get("OriginalSize").and_then(|v| v.as_string()),
        Some("12345")
    );
}

#[test]
fn gz_minimal_header() {
    // Minimal 10-byte header (no optional fields) + 8-byte trailer.
    let mut d = vec![0x1F, 0x8B, 0x08, 0x00];
    d.extend_from_slice(&0u32.to_le_bytes()); // mtime = 0 (skip ModificationTime)
    d.push(0x04); // XFL = fastest
    d.push(0xFF); // OS = unknown
    d.extend_from_slice(&0u32.to_le_bytes()); // CRC
    d.extend_from_slice(&0u32.to_le_bytes()); // ISIZE
    let reader = TestReader::new(d);
    let md = parse_gz_metadata(&reader).expect("gz parse");
    assert_eq!(
        md.get("CompressionLevel").and_then(|v| v.as_string()),
        Some("Fastest compression")
    );
    assert_eq!(
        md.get("OperatingSystem").and_then(|v| v.as_string()),
        Some("Unknown")
    );
    assert!(!md.contains_key("ModificationTime"));
}

#[test]
fn gz_signature_and_errors() {
    let reader = TestReader::new(vec![0x1F, 0x8B, 0x08, 0x00, 0, 0, 0, 0, 0, 0]);
    assert!(GZParser::verify_signature(&reader).unwrap());
    let bad = TestReader::new(vec![0x00, 0x00]);
    assert!(!GZParser::verify_signature(&bad).unwrap());
    let tiny = TestReader::new(vec![0x1F]);
    assert!(!GZParser::verify_signature(&tiny).unwrap());

    // Valid signature but header too short -> parse error.
    let short = TestReader::new(vec![0x1F, 0x8B, 0x08]);
    assert!(parse_gz_metadata(&short).is_err());
    assert!(parse_gz_metadata(&TestReader::new(b"NO".to_vec())).is_err());

    let parser = GZParser;
    assert!(parser.supports_format(FileFormat::GZ));
    assert!(!parser.supports_format(FileFormat::TAR));
}

// ===========================================================================
// ICC profile tests
// ===========================================================================

/// Build a minimal but well-formed ICC profile: 128-byte header + tag table.
///
/// `tags` is a list of (4-char signature, tag-data bytes). The tag data is laid
/// out after the tag table; offsets/sizes are computed automatically.
fn build_icc(profile_class: &[u8; 4], color_space: &[u8; 4], tags: &[(&str, Vec<u8>)]) -> Vec<u8> {
    // Header (128 bytes).
    let mut header = vec![0u8; 128];
    // profile size @0 filled later
    // CMM type @4
    header[4..8].copy_from_slice(b"appl");
    // version @8 -> 4.0.0 (major=4, minor/bugfix in high/low nibble of byte 9)
    header[8] = 0x04;
    header[9] = 0x30;
    // profile/device class @12
    header[12..16].copy_from_slice(profile_class);
    // color space @16
    header[16..20].copy_from_slice(color_space);
    // PCS @20
    header[20..24].copy_from_slice(b"XYZ ");
    // datetime @24 (12 bytes): 2024-03-15 14:30:45
    header[24..26].copy_from_slice(&2024u16.to_be_bytes());
    header[26..28].copy_from_slice(&3u16.to_be_bytes());
    header[28..30].copy_from_slice(&15u16.to_be_bytes());
    header[30..32].copy_from_slice(&14u16.to_be_bytes());
    header[32..34].copy_from_slice(&30u16.to_be_bytes());
    header[34..36].copy_from_slice(&45u16.to_be_bytes());
    // 'acsp' file signature @36
    header[36..40].copy_from_slice(b"acsp");
    // primary platform @40
    header[40..44].copy_from_slice(b"APPL");
    // CMM flags @44
    header[44..48].copy_from_slice(&0u32.to_be_bytes());
    // device manufacturer @48
    header[48..52].copy_from_slice(b"appl");
    // device model @52
    header[52..56].copy_from_slice(b"    ");
    // device attributes @56 (8 bytes) = 0
    // rendering intent @64
    header[64..68].copy_from_slice(&1u32.to_be_bytes()); // Media-Relative
    // illuminant @68 (12 bytes, s15Fixed16): D50 ~ (0.9642, 1.0, 0.8249)
    header[68..72].copy_from_slice(&((0.9642f64 * 65536.0) as i32).to_be_bytes());
    header[72..76].copy_from_slice(&(65536i32).to_be_bytes());
    header[76..80].copy_from_slice(&((0.8249f64 * 65536.0) as i32).to_be_bytes());
    // creator @80
    header[80..84].copy_from_slice(b"appl");
    // profile id @84 (16 bytes) -> all zero -> "0"

    // Tag table: count(4) + count * 12-byte entries.
    let tag_count = tags.len() as u32;
    let table_start = 128usize;
    let entries_len = 4 + tags.len() * 12;
    let mut data_section: Vec<u8> = Vec::new();
    let mut entries: Vec<u8> = Vec::new();
    entries.extend_from_slice(&tag_count.to_be_bytes());

    let data_base = table_start + entries_len;
    for (sig, tag_data) in tags {
        let offset = data_base + data_section.len();
        let size = tag_data.len();
        let sig_bytes = sig.as_bytes();
        let mut s = [b' '; 4];
        for (i, b) in sig_bytes.iter().take(4).enumerate() {
            s[i] = *b;
        }
        entries.extend_from_slice(&s);
        entries.extend_from_slice(&(offset as u32).to_be_bytes());
        entries.extend_from_slice(&(size as u32).to_be_bytes());
        data_section.extend_from_slice(tag_data);
    }

    let mut out = header;
    out.extend_from_slice(&entries);
    out.extend_from_slice(&data_section);
    // Set profile size @0.
    let total = out.len() as u32;
    out[0..4].copy_from_slice(&total.to_be_bytes());
    out
}

/// ICC textType tag: 'text' + reserved(4) + ascii + NUL.
fn icc_text_tag(s: &str) -> Vec<u8> {
    let mut v = b"text".to_vec();
    v.extend_from_slice(&[0, 0, 0, 0]);
    v.extend_from_slice(s.as_bytes());
    v.push(0);
    v
}

/// ICC textDescriptionType ('desc') tag.
fn icc_desc_tag(s: &str) -> Vec<u8> {
    let mut v = b"desc".to_vec();
    v.extend_from_slice(&[0, 0, 0, 0]); // reserved
    let count = (s.len() + 1) as u32; // includes NUL
    v.extend_from_slice(&count.to_be_bytes());
    v.extend_from_slice(s.as_bytes());
    v.push(0);
    v
}

/// ICC XYZType tag: 'XYZ ' + reserved(4) + 3 * s15Fixed16.
fn icc_xyz_tag(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut v = b"XYZ ".to_vec();
    v.extend_from_slice(&[0, 0, 0, 0]);
    v.extend_from_slice(&((x * 65536.0) as i32).to_be_bytes());
    v.extend_from_slice(&((y * 65536.0) as i32).to_be_bytes());
    v.extend_from_slice(&((z * 65536.0) as i32).to_be_bytes());
    v
}

/// ICC signatureType tag: 'sig ' + reserved(4) + 4-char signature.
fn icc_sig_tag(sig: &str) -> Vec<u8> {
    let mut v = b"sig ".to_vec();
    v.extend_from_slice(&[0, 0, 0, 0]);
    let mut s = [b' '; 4];
    for (i, b) in sig.as_bytes().iter().take(4).enumerate() {
        s[i] = *b;
    }
    v.extend_from_slice(&s);
    v
}

#[test]
fn icc_header_and_tags_full() {
    let tags = vec![
        ("desc", icc_desc_tag("sRGB IEC61966-2.1")),
        ("cprt", icc_text_tag("Copyright Notice")),
        ("wtpt", icc_xyz_tag(0.9505, 1.0, 1.089)),
        ("bkpt", icc_xyz_tag(0.0, 0.0, 0.0)),
        ("rXYZ", icc_xyz_tag(0.4361, 0.2225, 0.0139)),
        ("gXYZ", icc_xyz_tag(0.3851, 0.7169, 0.0971)),
        ("bXYZ", icc_xyz_tag(0.1431, 0.0606, 0.7141)),
        ("rTRC", vec![b'c', b'u', b'r', b'v', 0, 0, 0, 0, 0, 0, 0, 0]),
        ("tech", icc_sig_tag("CRT ")),
    ];
    let data = build_icc(b"mntr", b"RGB ", &tags);

    let md = parse_icc_profile_data(&data).expect("icc parse");

    // Header fields.
    assert_eq!(
        md.get("ProfileClass").and_then(|v| v.as_string()),
        Some("Display Device Profile")
    );
    assert_eq!(
        md.get("ColorSpaceData").and_then(|v| v.as_string()),
        Some("RGB")
    );
    assert_eq!(
        md.get("ProfileConnectionSpace").and_then(|v| v.as_string()),
        Some("XYZ")
    );
    assert!(md.contains_key("ProfileVersion"));
    assert!(md.contains_key("ProfileDateTime"));
    assert_eq!(
        md.get("ProfileFileSignature").and_then(|v| v.as_string()),
        Some("acsp")
    );
    assert!(md.contains_key("PrimaryPlatform"));
    assert!(md.contains_key("CMMFlags"));
    assert!(md.contains_key("DeviceManufacturer"));
    assert!(md.contains_key("RenderingIntent"));
    assert!(md.contains_key("ConnectionSpaceIlluminant"));
    assert!(md.contains_key("ProfileID"));

    // Tag decoders.
    assert_eq!(
        md.get("ProfileDescription").and_then(|v| v.as_string()),
        Some("sRGB IEC61966-2.1")
    );
    assert_eq!(
        md.get("ProfileCopyright").and_then(|v| v.as_string()),
        Some("Copyright Notice")
    );
    assert!(md.contains_key("MediaWhitePoint"));
    assert!(md.contains_key("MediaBlackPoint"));
    assert!(md.contains_key("RedMatrixColumn"));
    assert!(md.contains_key("GreenMatrixColumn"));
    assert!(md.contains_key("BlueMatrixColumn"));
    // Curve tag -> binary placeholder.
    assert!(md.contains_key("RedToneReproductionCurve"));
    // Signature (technology) tag.
    assert!(md.contains_key("Technology"));
}

#[test]
fn icc_standalone_file_parse_prefixes() {
    let tags = vec![("desc", icc_desc_tag("Profile X"))];
    let data = build_icc(b"prtr", b"CMYK", &tags);
    let reader = TestReader::new(data);
    let md = parse_icc_file(&reader).expect("icc file parse");

    // parse_icc_file adds the ICC_Profile: prefix.
    assert_eq!(
        md.get("ICC_Profile:ProfileClass")
            .and_then(|v| v.as_string()),
        Some("Output Device Profile")
    );
    assert_eq!(
        md.get("ICC_Profile:ProfileDescription")
            .and_then(|v| v.as_string()),
        Some("Profile X")
    );
}

#[test]
fn icc_too_small_errors() {
    // < 128 bytes -> error from the core parser.
    assert!(parse_icc_profile_data(&[0u8; 64]).is_err());
    let reader = TestReader::new(vec![0u8; 10]);
    assert!(parse_icc_file(&reader).is_err());
}

#[test]
fn icc_header_only_no_tag_table() {
    // Exactly 128 bytes: header parses, tag table is skipped.
    let mut header = vec![0u8; 128];
    header[12..16].copy_from_slice(b"scnr"); // Input Device Profile
    header[16..20].copy_from_slice(b"Gray");
    header[36..40].copy_from_slice(b"acsp");
    header[0..4].copy_from_slice(&128u32.to_be_bytes());
    let md = parse_icc_profile_data(&header).expect("icc header-only");
    assert_eq!(
        md.get("ProfileClass").and_then(|v| v.as_string()),
        Some("Input Device Profile")
    );
}

#[test]
fn icc_tagtype_enum_and_tagdef_public_api() {
    // Exercise the re-exported public types directly.
    let def = TagDef {
        signature: "desc",
        name: "ProfileDescription",
        tag_type: TagType::TextDescription,
    };
    assert_eq!(def.signature, "desc");
    assert_eq!(def.name, "ProfileDescription");
    assert_eq!(def.tag_type, TagType::TextDescription);
    assert_ne!(TagType::Text, TagType::Xyz);
    // Copy/Clone/Debug derived impls.
    let copied = def.tag_type;
    assert_eq!(copied, TagType::TextDescription);
    let _ = format!("{:?}", TagType::Curve);
}

// ===========================================================================
// Font name-table coverage (extra nameIDs + Mac/Unicode platform decode)
// ===========================================================================

/// Build a TTF with a `name` table containing the supplied records and a `head`
/// table. Each record is (platform_id, encoding_id, language_id, name_id, &str).
/// Windows (3) strings are encoded UTF-16BE; Mac (1)/Unicode (0) as raw bytes.
fn build_ttf_with_names(records: &[(u16, u16, u16, u16, &str)]) -> Vec<u8> {
    // Encode each record's string and lay out the string storage.
    let mut string_storage: Vec<u8> = Vec::new();
    let mut encoded: Vec<(u16, u16, u16, u16, u16, u16)> = Vec::new(); // pid,eid,lid,nid,len,off
    for (pid, eid, lid, nid, s) in records {
        let bytes: Vec<u8> = if *pid == 3 {
            s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
        } else {
            s.as_bytes().to_vec()
        };
        let off = string_storage.len() as u16;
        let len = bytes.len() as u16;
        string_storage.extend_from_slice(&bytes);
        encoded.push((*pid, *eid, *lid, *nid, len, off));
    }

    let count = records.len() as u16;
    // name table = header(6) + count*12 + storage
    let name_record_area = 6 + (count as usize) * 12;
    let name_table_len = name_record_area + string_storage.len();

    // Layout: offset table (12) + 2 dir entries (32) = 44.
    let name_off = 44u32;
    let head_off = name_off + name_table_len as u32;
    let head_len = 54u32;

    let mut data: Vec<u8> = vec![
        0x00, 0x01, 0x00, 0x00, // sfnt version 1.0
        0x00, 0x02, // numTables = 2
        0x00, 0x10, 0x00, 0x00, 0x00, 0x00, // searchRange/entrySelector/rangeShift
    ];
    // name dir entry
    data.extend_from_slice(b"name");
    data.extend_from_slice(&0u32.to_be_bytes()); // checksum
    data.extend_from_slice(&name_off.to_be_bytes());
    data.extend_from_slice(&(name_table_len as u32).to_be_bytes());
    // head dir entry
    data.extend_from_slice(b"head");
    data.extend_from_slice(&0u32.to_be_bytes());
    data.extend_from_slice(&head_off.to_be_bytes());
    data.extend_from_slice(&head_len.to_be_bytes());

    // name table.
    data.extend_from_slice(&0u16.to_be_bytes()); // format 0
    data.extend_from_slice(&count.to_be_bytes()); // count
    data.extend_from_slice(&(name_record_area as u16).to_be_bytes()); // stringOffset
    for (pid, eid, lid, nid, len, off) in &encoded {
        data.extend_from_slice(&pid.to_be_bytes());
        data.extend_from_slice(&eid.to_be_bytes());
        data.extend_from_slice(&lid.to_be_bytes());
        data.extend_from_slice(&nid.to_be_bytes());
        data.extend_from_slice(&len.to_be_bytes());
        data.extend_from_slice(&off.to_be_bytes());
    }
    data.extend_from_slice(&string_storage);

    // head table (54 bytes), unitsPerEm = 1000 @ offset 18.
    let mut head = vec![0u8; 54];
    head[0..4].copy_from_slice(&[0x00, 0x01, 0x00, 0x00]); // version
    head[12..16].copy_from_slice(&[0x5F, 0x0F, 0x3C, 0xF5]); // magic
    head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
    data.extend_from_slice(&head);

    data
}

#[test]
fn ttf_many_name_ids_windows_platform() {
    let records = [
        (3u16, 1u16, 0x409u16, 0u16, "(c) 2024 Foundry"), // Copyright
        (3, 1, 0x409, 1, "MyFamily"),                     // FontFamily
        (3, 1, 0x409, 2, "Bold"),                         // Subfamily
        (3, 1, 0x409, 4, "MyFamily Bold"),                // Full name
        (3, 1, 0x409, 5, "Version 2.000"),                // Version
        (3, 1, 0x409, 6, "MyFamily-Bold"),                // PostScript name
        (3, 1, 0x409, 9, "Famous Designer"),              // Designer
        (3, 1, 0x409, 11, "https://foundry.example"),     // Vendor URL
        (3, 1, 0x409, 13, "OFL-1.1"),                     // License
    ];
    let data = build_ttf_with_names(&records);
    let reader = TestReader::new(data);
    let md = parse_ttf_metadata(&reader).expect("ttf parse");

    assert_eq!(
        md.get("Copyright").and_then(|v| v.as_string()),
        Some("(c) 2024 Foundry")
    );
    assert_eq!(
        md.get("FontFamily").and_then(|v| v.as_string()),
        Some("MyFamily")
    );
    assert_eq!(
        md.get("FontSubfamily").and_then(|v| v.as_string()),
        Some("Bold")
    );
    assert_eq!(
        md.get("FontName").and_then(|v| v.as_string()),
        Some("MyFamily Bold")
    );
    assert_eq!(
        md.get("FontVersion").and_then(|v| v.as_string()),
        Some("Version 2.000")
    );
    assert_eq!(
        md.get("PostScriptName").and_then(|v| v.as_string()),
        Some("MyFamily-Bold")
    );
    assert_eq!(
        md.get("Designer").and_then(|v| v.as_string()),
        Some("Famous Designer")
    );
    assert_eq!(
        md.get("VendorURL").and_then(|v| v.as_string()),
        Some("https://foundry.example")
    );
    assert_eq!(
        md.get("License").and_then(|v| v.as_string()),
        Some("OFL-1.1")
    );
    assert_eq!(
        md.get("UnitsPerEm").and_then(|v| v.as_string()),
        Some("1000")
    );
}

#[test]
fn ttf_mac_and_unicode_platform_decode() {
    // Mac (platform 1) + Unicode (platform 0) records use the UTF-8/raw decode
    // branch. Provide them for distinct name IDs so both records survive.
    let records = [
        (1u16, 0u16, 0u16, 1u16, "MacFamily"), // Mac platform FontFamily
        (0u16, 3u16, 0u16, 4u16, "UnicodeFull"), // Unicode platform Full name
    ];
    let data = build_ttf_with_names(&records);
    let reader = TestReader::new(data);
    let md = parse_ttf_metadata(&reader).expect("ttf parse");
    assert_eq!(
        md.get("FontFamily").and_then(|v| v.as_string()),
        Some("MacFamily")
    );
    assert_eq!(
        md.get("FontName").and_then(|v| v.as_string()),
        Some("UnicodeFull")
    );
}

#[test]
fn otf_name_table_records() {
    // OTF reuses the SFNT name table; build an OTTO sfnt with a name + head + CFF.
    // Reuse the TTF builder's name/head layout but with an OTTO signature and a
    // CFF table entry so OutlineFormat=CFF is reported.
    let records = [
        (3u16, 1u16, 0x409u16, 1u16, "OpenFamily"),
        (3, 1, 0x409, 6, "OpenFamily-Reg"),
    ];

    // Build the name + head portion via the helper, then re-wrap as OTTO with CFF.
    // Simpler: construct directly.
    let mut string_storage: Vec<u8> = Vec::new();
    let mut encoded: Vec<(u16, u16, u16, u16, u16, u16)> = Vec::new();
    for (pid, eid, lid, nid, s) in records.iter() {
        let bytes: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
        let off = string_storage.len() as u16;
        let len = bytes.len() as u16;
        string_storage.extend_from_slice(&bytes);
        encoded.push((*pid, *eid, *lid, *nid, len, off));
    }
    let count = records.len() as u16;
    let name_record_area = 6 + (count as usize) * 12;
    let name_table_len = name_record_area + string_storage.len();

    // 3 tables: CFF, name, head. offset table(12) + 3*16 = 60.
    let name_off = 60u32;
    let head_off = name_off + name_table_len as u32;
    let head_len = 54u32;
    let cff_off = head_off + head_len;
    let cff_len = 4u32;

    let mut data: Vec<u8> = Vec::new();
    data.extend_from_slice(b"OTTO");
    data.extend_from_slice(&3u16.to_be_bytes()); // numTables
    data.extend_from_slice(&[0x00, 0x20, 0x00, 0x01, 0x00, 0x00]); // search/entry/range
    // CFF dir entry
    data.extend_from_slice(b"CFF ");
    data.extend_from_slice(&0u32.to_be_bytes());
    data.extend_from_slice(&cff_off.to_be_bytes());
    data.extend_from_slice(&cff_len.to_be_bytes());
    // name dir entry
    data.extend_from_slice(b"name");
    data.extend_from_slice(&0u32.to_be_bytes());
    data.extend_from_slice(&name_off.to_be_bytes());
    data.extend_from_slice(&(name_table_len as u32).to_be_bytes());
    // head dir entry
    data.extend_from_slice(b"head");
    data.extend_from_slice(&0u32.to_be_bytes());
    data.extend_from_slice(&head_off.to_be_bytes());
    data.extend_from_slice(&head_len.to_be_bytes());

    // name table
    data.extend_from_slice(&0u16.to_be_bytes());
    data.extend_from_slice(&count.to_be_bytes());
    data.extend_from_slice(&(name_record_area as u16).to_be_bytes());
    for (pid, eid, lid, nid, len, off) in &encoded {
        data.extend_from_slice(&pid.to_be_bytes());
        data.extend_from_slice(&eid.to_be_bytes());
        data.extend_from_slice(&lid.to_be_bytes());
        data.extend_from_slice(&nid.to_be_bytes());
        data.extend_from_slice(&len.to_be_bytes());
        data.extend_from_slice(&off.to_be_bytes());
    }
    data.extend_from_slice(&string_storage);

    // head table
    let mut head = vec![0u8; 54];
    head[0..4].copy_from_slice(&[0x00, 0x01, 0x00, 0x00]);
    head[12..16].copy_from_slice(&[0x5F, 0x0F, 0x3C, 0xF5]);
    head[18..20].copy_from_slice(&2048u16.to_be_bytes());
    data.extend_from_slice(&head);

    // CFF table (4 bytes of filler).
    data.extend_from_slice(&[0x01, 0x00, 0x04, 0x01]);

    let reader = TestReader::new(data);
    let md = parse_otf_metadata(&reader).expect("otf parse");
    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("OTF".to_string()))
    );
    assert_eq!(
        md.get("OutlineFormat").and_then(|v| v.as_string()),
        Some("CFF")
    );
    assert_eq!(
        md.get("FontFamily").and_then(|v| v.as_string()),
        Some("OpenFamily")
    );
    assert_eq!(
        md.get("PostScriptName").and_then(|v| v.as_string()),
        Some("OpenFamily-Reg")
    );
    assert_eq!(
        md.get("UnitsPerEm").and_then(|v| v.as_string()),
        Some("2048")
    );
}
