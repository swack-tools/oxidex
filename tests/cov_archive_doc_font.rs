//! Coverage-focused integration tests for archive (OLE), OOXML documents,
//! fonts (TTF/OTF/WOFF/WOFF2), and remaining text parsers.
//!
//! These tests drive the public parser APIs with synthetic byte buffers crafted
//! to be valid enough to exercise deep code paths, plus production-path coverage
//! through `read_metadata` on real tempfiles with correct extensions.

#[path = "common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::TagValue;
use oxidex::core::operations::read_metadata;

use std::io::Write;

// Target parsers (public API)
use oxidex::parsers::archive::ole::{OLEParser, VBAAnalyzer, parse_ole_metadata};
use oxidex::parsers::document::ooxml::{
    DocxParser, PptxParser, XlsxParser, parse_docx_metadata, parse_pptx_metadata,
    parse_xlsx_metadata,
};
use oxidex::parsers::font::otf::{OTFParser, parse_otf_metadata};
use oxidex::parsers::font::ttf::{TTFParser, parse_ttf_metadata};
use oxidex::parsers::font::woff::{WOFFParser, parse_woff_metadata};
use oxidex::parsers::font::woff2::{WOFF2Parser, parse_woff2_metadata};
use oxidex::parsers::text::txt::{Encoding, LineEnding, TXTParser, parse_txt_metadata};

use oxidex::core::FormatParser;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// OLE compound file magic.
const OLE_MAGIC: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Write a UTF-16LE directory-entry name into a 128-byte directory entry buffer.
/// `entry_type`: 0=invalid, 1=storage, 2=stream, 5=root.
fn make_dir_entry(name: &str, entry_type: u8, start_sector: u32, size: u32) -> [u8; 128] {
    let mut entry = [0u8; 128];

    // Name as UTF-16LE in the first 64 bytes.
    let utf16: Vec<u16> = name.encode_utf16().collect();
    for (i, ch) in utf16.iter().enumerate() {
        if i * 2 + 1 >= 64 {
            break;
        }
        let bytes = ch.to_le_bytes();
        entry[i * 2] = bytes[0];
        entry[i * 2 + 1] = bytes[1];
    }
    // Name length in bytes including the terminating NUL (offset 64, u16 LE).
    let name_len = ((utf16.len() + 1) * 2) as u16;
    entry[64..66].copy_from_slice(&name_len.to_le_bytes());

    // entry type at offset 66
    entry[66] = entry_type;

    // sibling/child DIDs at 68/72/76 (leave as 0)
    // start sector at offset 116 (u32 LE)
    entry[116..120].copy_from_slice(&start_sector.to_le_bytes());
    // size at offset 120 (u32 LE)
    entry[120..124].copy_from_slice(&size.to_le_bytes());

    entry
}

/// Build a minimal OLE file with a 512-byte sector size.
///
/// Layout:
/// - 512 byte header (signature + sector shift = 9 -> 512 byte sectors)
/// - directory sector at first_dir_sector (default 0 -> offset 512)
/// - directory holds the supplied 128-byte entries (4 per 512-byte sector)
fn build_ole(entries: &[[u8; 128]], extra_tail: usize) -> Vec<u8> {
    let mut header = vec![0u8; 512];
    header[0..8].copy_from_slice(OLE_MAGIC);

    // sector shift (offset 30) = 9 -> sector size 512
    header[30..32].copy_from_slice(&9u16.to_le_bytes());
    // mini sector shift (offset 32) = 6 -> mini sector size 64
    header[32..34].copy_from_slice(&6u16.to_le_bytes());

    // total sectors (offset 44)
    header[44..48].copy_from_slice(&4u32.to_le_bytes());
    // first dir sector (offset 48) = 0
    header[48..52].copy_from_slice(&0u32.to_le_bytes());
    // first mini fat sector (offset 60)
    header[60..64].copy_from_slice(&0u32.to_le_bytes());
    // mini fat sectors (offset 64)
    header[64..68].copy_from_slice(&1u32.to_le_bytes());
    // first difat sector (offset 68)
    header[68..72].copy_from_slice(&0u32.to_le_bytes());
    // difat sectors (offset 72)
    header[72..76].copy_from_slice(&0u32.to_le_bytes());
    // fat sectors (offset 76)
    header[76..80].copy_from_slice(&1u32.to_le_bytes());

    let mut data = header;

    // Directory sector (512 bytes -> 4 entries).
    let mut dir = vec![0u8; 512];
    for (i, e) in entries.iter().enumerate() {
        if i >= 4 {
            break;
        }
        dir[i * 128..(i + 1) * 128].copy_from_slice(e);
    }
    data.extend_from_slice(&dir);

    // Extra tail so streams referencing sectors >0 can be read.
    if extra_tail > 0 {
        data.extend(vec![0u8; extra_tail]);
    }

    data
}

/// Build an OLE file where one stream entry points at an in-file sector
/// containing the supplied `payload` bytes (used to trigger pattern scanning).
fn build_ole_with_stream(name: &str, payload: &[u8], extra_entries: &[[u8; 128]]) -> Vec<u8> {
    // Stream sector index 1 -> offset 512 + 1*512 = 1024.
    let stream_sector: u32 = 1;
    let size = payload.len() as u32;

    let mut entries: Vec<[u8; 128]> = Vec::new();
    entries.push(make_dir_entry("Root Entry", 5, 0, 0));
    entries.push(make_dir_entry(name, 2, stream_sector, size));
    for e in extra_entries {
        entries.push(*e);
    }

    // Header + directory sector (sector 0) + stream sector (sector 1).
    let mut data = build_ole(&entries, 0);
    // Ensure the file extends to include the stream sector (offset 1024).
    while data.len() < 512 + (stream_sector as usize + 1) * 512 {
        data.push(0);
    }
    let stream_off = 512 + stream_sector as usize * 512;
    let end = stream_off + payload.len();
    if end > data.len() {
        data.resize(end, 0);
    }
    data[stream_off..end].copy_from_slice(payload);

    data
}

/// Build a ZIP container from (name, bytes) entries.
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

fn temp_with(suffix: &str, bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new()
        .suffix(suffix)
        .tempfile()
        .expect("create tempfile");
    f.write_all(bytes).expect("write tempfile");
    f.flush().expect("flush tempfile");
    f
}

// Common OOXML XML payloads -------------------------------------------------

const CONTENT_TYPES_XML: &str = r#"<?xml version="1.0"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Override PartName="/word/media/image1.png" ContentType="image/png"/>
  <Override PartName="/word/charts/chart1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/>
</Types>"#;

const CORE_XML: &str = r#"<?xml version="1.0"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"
                   xmlns:dc="http://purl.org/dc/elements/1.1/"
                   xmlns:dcterms="http://purl.org/dc/terms/">
  <dc:title>Cov Title</dc:title>
  <dc:creator>Cov Creator</dc:creator>
  <dc:subject>Cov Subject</dc:subject>
  <dc:description>Cov Description</dc:description>
  <dc:keywords>a, b, c</dc:keywords>
  <cp:lastModifiedBy>Editor</cp:lastModifiedBy>
  <cp:revision>7</cp:revision>
  <dcterms:created>2024-01-01T00:00:00Z</dcterms:created>
  <dcterms:modified>2024-02-02T00:00:00Z</dcterms:modified>
  <cp:category>Cat</cp:category>
  <cp:contentStatus>Final</cp:contentStatus>
</cp:coreProperties>"#;

const APP_XML: &str = r#"<?xml version="1.0"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">
  <Application>Microsoft Word</Application>
  <Pages>12</Pages>
  <Words>1500</Words>
  <Characters>8000</Characters>
  <Company>Acme</Company>
  <Manager>Mgr</Manager>
  <Template>Normal.dotm</Template>
  <TotalTime>90</TotalTime>
  <AppVersion>16.0</AppVersion>
</Properties>"#;

const CUSTOM_XML: &str = r#"<?xml version="1.0"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties"
            xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">
  <property fmtid="{X}" pid="2" name="ProjectID"><vt:lpwstr>P-1</vt:lpwstr></property>
  <property fmtid="{X}" pid="3" name="Reviewers"><vt:i4>4</vt:i4></property>
</Properties>"#;

const STYLES_XML: &str = r#"<?xml version="1.0"?>
<styles><style w:styleId="A"/><style w:styleId="B"/><style w:styleId="C"/></styles>"#;

const COMMENTS_XML: &str = r#"<?xml version="1.0"?>
<comments><comment id="1">hi</comment><comment id="2">there</comment></comments>"#;

const WORKBOOK_XML: &str = r#"<?xml version="1.0"?>
<workbook><sheets><sheet name="Sheet1"/><sheet name="Sheet2"/><sheet name="Data"/></sheets></workbook>"#;

const PRESENTATION_XML: &str = r#"<?xml version="1.0"?><presentation/>"#;

const DOCUMENT_XML: &str = r#"<?xml version="1.0"?><document><body/></document>"#;

// ===========================================================================
// OLE tests
// ===========================================================================

#[test]
fn ole_too_small_errors() {
    let reader = TestReader::new(vec![0u8; 100]);
    assert!(parse_ole_metadata(&reader).is_err());
}

#[test]
fn ole_bad_signature_errors() {
    let reader = TestReader::new(vec![0u8; 600]);
    assert!(parse_ole_metadata(&reader).is_err());
}

#[test]
fn ole_minimal_no_macros() {
    // Root entry + a benign stream that is not VBA-related.
    let entries = [
        make_dir_entry("Root Entry", 5, 0, 0),
        make_dir_entry("WordDocument", 2, 0, 0),
    ];
    let data = build_ole(&entries, 1024);
    let reader = TestReader::new(data);
    let md = parse_ole_metadata(&reader).expect("ole parse");

    assert_eq!(md.get("OLE:SectorSize"), Some(&TagValue::Integer(512)));
    assert!(md.contains_key("OLE:TotalSectors"));
    assert!(md.contains_key("OLE:DirectoryEntryCount"));
    // No VBA directory -> HasVBAMacros = No
    assert_eq!(
        md.get("OLE:HasVBAMacros"),
        Some(&TagValue::String("No".to_string()))
    );
}

#[test]
fn ole_with_vba_macros_and_modules() {
    // Macros storage + _VBA_PROJECT stream + a module stream named "Module1".
    let entries = [
        make_dir_entry("Root Entry", 5, 0, 0),
        make_dir_entry("Macros", 1, 0, 0),
        make_dir_entry("_VBA_PROJECT", 2, 0, 0),
        make_dir_entry("Module1", 2, 0, 0),
    ];
    let data = build_ole(&entries, 2048);
    let reader = TestReader::new(data);
    let md = parse_ole_metadata(&reader).expect("ole parse");

    assert_eq!(
        md.get("OLE:HasVBAMacros"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert_eq!(
        md.get("OLE:VBAProjectName"),
        Some(&TagValue::String("VBA Project".to_string()))
    );
    assert!(md.contains_key("OLE:VBAModuleCount"));
    assert!(md.contains_key("OLE:VBAModuleNames"));
}

#[test]
fn ole_with_suspicious_stream_payload() {
    // Stream contains macro source with multiple suspicious patterns; this
    // drives read_stream + check_suspicious_patterns + the category reporting.
    let payload = b"Sub Auto_Open()\r\n  Shell \"powershell.exe -encodedcommand AAA\"\r\n  Set h = CreateObject(\"MSXML2.XMLHTTP\")\r\n  x = Chr(65) & Chr(66)\r\n  Open \"c:\\f\" For Output As #1\r\nEnd Sub";
    // Add a VBA marker dir so the analyzer proceeds past the early return.
    let macros = make_dir_entry("VBA", 1, 0, 0);
    let data = build_ole_with_stream("ThisDocument", payload, &[macros]);
    let reader = TestReader::new(data);
    let md = parse_ole_metadata(&reader).expect("ole parse");

    assert_eq!(
        md.get("OLE:HasVBAMacros"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert_eq!(
        md.get("OLE:HasAutoExec"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert_eq!(
        md.get("OLE:HasShellExecution"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert_eq!(
        md.get("OLE:HasNetworkAccess"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert_eq!(
        md.get("OLE:HasPowerShell"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert!(md.contains_key("OLE:SuspiciousPatterns"));
    // A module with readable code should yield a code preview.
    assert!(md.contains_key("OLE:VBACodePreview"));
}

#[test]
fn ole_formatparser_trait_and_supports_format() {
    let entries = [make_dir_entry("Root Entry", 5, 0, 0)];
    let data = build_ole(&entries, 512);
    let reader = TestReader::new(data);
    let parser = OLEParser;
    assert!(parser.parse(&reader).is_ok());
    assert!(parser.supports_format(oxidex::core::FileFormat::OLE));
    assert!(!parser.supports_format(oxidex::core::FileFormat::TXT));
}

#[test]
fn ole_vba_pattern_checker_categories() {
    // Direct exercise of the public pattern checker across categories.
    let auto = VBAAnalyzer::check_suspicious_patterns(b"Workbook_Open Document_Close AutoClose");
    assert!(auto.iter().any(|p| p.contains("AutoExec")));

    let net = VBAAnalyzer::check_suspicious_patterns(b"WinHttp URLDownloadToFile InternetOpen");
    assert!(net.iter().any(|p| p.contains("Network")));

    let file =
        VBAAnalyzer::check_suspicious_patterns(b"FileSystemObject CreateTextFile OpenTextFile");
    assert!(file.iter().any(|p| p.contains("File")));

    let obf = VBAAnalyzer::check_suspicious_patterns(b"ChrW(65) Chr$(66)");
    assert!(obf.iter().any(|p| p.contains("Obfuscation")));

    // Excessive concatenation path.
    let mut concat = String::from("x = \"a\"");
    for _ in 0..30 {
        concat.push_str(" & \"b\"");
    }
    let cc = VBAAnalyzer::check_suspicious_patterns(concat.as_bytes());
    assert!(cc.iter().any(|p| p.contains("concatenation")));

    // Clean input yields nothing dangerous.
    let clean = VBAAnalyzer::check_suspicious_patterns(b"Dim total As Long\r\ntotal = 1 + 2");
    assert!(clean.is_empty() || clean.len() <= 1);
}

#[test]
fn ole_production_path_via_read_metadata() {
    let entries = [
        make_dir_entry("Root Entry", 5, 0, 0),
        make_dir_entry("Macros", 1, 0, 0),
        make_dir_entry("Module1", 2, 0, 0),
    ];
    let data = build_ole(&entries, 2048);
    let f = temp_with(".doc", &data);
    let md = read_metadata(f.path()).expect("read_metadata ole");
    assert!(md.contains_key("OLE:SectorSize"));
    assert_eq!(
        md.get("OLE:HasVBAMacros"),
        Some(&TagValue::String("Yes".to_string()))
    );
}

// ===========================================================================
// OOXML (DOCX / XLSX / PPTX) tests
// ===========================================================================

#[test]
fn docx_full_metadata() {
    let zip = build_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES_XML.as_bytes()),
        ("word/document.xml", DOCUMENT_XML.as_bytes()),
        ("docProps/core.xml", CORE_XML.as_bytes()),
        ("docProps/app.xml", APP_XML.as_bytes()),
        ("docProps/custom.xml", CUSTOM_XML.as_bytes()),
        ("word/styles.xml", STYLES_XML.as_bytes()),
        ("word/comments.xml", COMMENTS_XML.as_bytes()),
    ]);
    let reader = TestReader::new(zip);
    let md = parse_docx_metadata(&reader).expect("docx parse");

    assert_eq!(
        md.get("OOXML:Title"),
        Some(&TagValue::String("Cov Title".to_string()))
    );
    assert_eq!(
        md.get("OOXML:Creator"),
        Some(&TagValue::String("Cov Creator".to_string()))
    );
    assert_eq!(
        md.get("OOXML:LastModifiedBy"),
        Some(&TagValue::String("Editor".to_string()))
    );
    assert_eq!(
        md.get("OOXML:RevisionNumber"),
        Some(&TagValue::String("7".to_string()))
    );
    // app.xml fields
    assert_eq!(
        md.get("OOXML:Application"),
        Some(&TagValue::String("Microsoft Word".to_string()))
    );
    assert_eq!(
        md.get("OOXML:TotalEditTime"),
        Some(&TagValue::String("1 hour 30 minutes".to_string()))
    );
    // custom.xml
    assert_eq!(
        md.get("OOXML:Custom:ProjectID"),
        Some(&TagValue::String("P-1".to_string()))
    );
    assert_eq!(
        md.get("OOXML:Custom:Reviewers"),
        Some(&TagValue::String("4".to_string()))
    );
    // content types
    assert!(md.contains_key("OOXML:EmbeddedContentTypes"));
    // docx-specific
    assert_eq!(
        md.get("OOXML:HasComments"),
        Some(&TagValue::String("true".to_string()))
    );
    assert_eq!(
        md.get("OOXML:CommentsCount"),
        Some(&TagValue::String("2".to_string()))
    );
    assert_eq!(
        md.get("OOXML:StylesCount"),
        Some(&TagValue::String("3".to_string()))
    );
    // DOCX aliases
    assert_eq!(
        md.get("DOCX:Title"),
        Some(&TagValue::String("Cov Title".to_string()))
    );
    assert_eq!(
        md.get("DOCX:WordCount"),
        Some(&TagValue::String("1500".to_string()))
    );
    assert_eq!(
        md.get("DOCX:PageCount"),
        Some(&TagValue::String("12".to_string()))
    );
}

#[test]
fn docx_missing_required_parts_errors() {
    // ZIP missing word/document.xml -> not a valid DOCX.
    let zip = build_zip(&[("[Content_Types].xml", CONTENT_TYPES_XML.as_bytes())]);
    let reader = TestReader::new(zip);
    assert!(parse_docx_metadata(&reader).is_err());
}

#[test]
fn docx_not_a_zip_errors() {
    let reader = TestReader::new(b"this is not a zip at all".to_vec());
    assert!(parse_docx_metadata(&reader).is_err());
}

#[test]
fn docx_formatparser_supports_format() {
    let parser = DocxParser;
    assert!(parser.supports_format(oxidex::core::FileFormat::DOCX));
    assert!(!parser.supports_format(oxidex::core::FileFormat::XLSX));
}

#[test]
fn xlsx_full_metadata() {
    let zip = build_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES_XML.as_bytes()),
        ("xl/workbook.xml", WORKBOOK_XML.as_bytes()),
        ("docProps/core.xml", CORE_XML.as_bytes()),
        ("docProps/app.xml", APP_XML.as_bytes()),
        ("docProps/custom.xml", CUSTOM_XML.as_bytes()),
    ]);
    let reader = TestReader::new(zip);
    let md = parse_xlsx_metadata(&reader).expect("xlsx parse");

    assert_eq!(
        md.get("OOXML:SheetCount"),
        Some(&TagValue::String("3".to_string()))
    );
    let sheets = md
        .get("OOXML:SheetNames")
        .and_then(|v| v.as_string())
        .unwrap_or("");
    assert!(sheets.contains("Sheet1"));
    assert!(sheets.contains("Data"));
    assert_eq!(
        md.get("OOXML:Creator"),
        Some(&TagValue::String("Cov Creator".to_string()))
    );

    let parser = XlsxParser;
    assert!(parser.supports_format(oxidex::core::FileFormat::XLSX));
}

#[test]
fn xlsx_missing_workbook_errors() {
    let zip = build_zip(&[("[Content_Types].xml", CONTENT_TYPES_XML.as_bytes())]);
    let reader = TestReader::new(zip);
    assert!(parse_xlsx_metadata(&reader).is_err());
}

#[test]
fn pptx_full_metadata() {
    let zip = build_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES_XML.as_bytes()),
        ("ppt/presentation.xml", PRESENTATION_XML.as_bytes()),
        ("docProps/core.xml", CORE_XML.as_bytes()),
        ("docProps/app.xml", APP_XML.as_bytes()),
        ("docProps/custom.xml", CUSTOM_XML.as_bytes()),
    ]);
    let reader = TestReader::new(zip);
    let md = parse_pptx_metadata(&reader).expect("pptx parse");

    assert_eq!(
        md.get("OOXML:Title"),
        Some(&TagValue::String("Cov Title".to_string()))
    );
    assert!(md.contains_key("OOXML:EmbeddedContentTypes"));

    let parser = PptxParser;
    assert!(parser.supports_format(oxidex::core::FileFormat::PPTX));
}

#[test]
fn pptx_missing_presentation_errors() {
    let zip = build_zip(&[("[Content_Types].xml", CONTENT_TYPES_XML.as_bytes())]);
    let reader = TestReader::new(zip);
    assert!(parse_pptx_metadata(&reader).is_err());
}

#[test]
fn docx_production_path_via_read_metadata() {
    let zip = build_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES_XML.as_bytes()),
        ("word/document.xml", DOCUMENT_XML.as_bytes()),
        ("docProps/core.xml", CORE_XML.as_bytes()),
        ("docProps/app.xml", APP_XML.as_bytes()),
    ]);
    let f = temp_with(".docx", &zip);
    let md = read_metadata(f.path()).expect("read_metadata docx");
    assert_eq!(
        md.get("OOXML:Title"),
        Some(&TagValue::String("Cov Title".to_string()))
    );
}

#[test]
fn xlsx_production_path_via_read_metadata() {
    let zip = build_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES_XML.as_bytes()),
        ("xl/workbook.xml", WORKBOOK_XML.as_bytes()),
        ("docProps/core.xml", CORE_XML.as_bytes()),
    ]);
    let f = temp_with(".xlsx", &zip);
    let md = read_metadata(f.path()).expect("read_metadata xlsx");
    assert!(md.contains_key("OOXML:SheetCount"));
}

#[test]
fn pptx_production_path_via_read_metadata() {
    let zip = build_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES_XML.as_bytes()),
        ("ppt/presentation.xml", PRESENTATION_XML.as_bytes()),
        ("docProps/core.xml", CORE_XML.as_bytes()),
    ]);
    let f = temp_with(".pptx", &zip);
    let md = read_metadata(f.path()).expect("read_metadata pptx");
    assert_eq!(
        md.get("OOXML:Creator"),
        Some(&TagValue::String("Cov Creator".to_string()))
    );
}

// ===========================================================================
// TTF tests
// ===========================================================================

/// Build a TTF with a `name` table (one Windows record) and a `head` table.
fn build_ttf() -> Vec<u8> {
    // Offset table (12) + 2 table directory entries (32) = 44.
    let mut data = vec![
        0x00, 0x01, 0x00, 0x00, // sfnt version
        0x00, 0x02, // numTables = 2
        0x00, 0x10, // searchRange
        0x00, 0x00, // entrySelector
        0x00, 0x00, // rangeShift
        // name table dir entry
        b'n', b'a', b'm', b'e', 0x00, 0x00, 0x00, 0x00, // checksum
        0x00, 0x00, 0x00, 0x2C, // offset = 44
        0x00, 0x00, 0x00, 0x1A, // length = 26
        // head table dir entry
        b'h', b'e', b'a', b'd', 0x00, 0x00, 0x00, 0x00, // checksum
        0x00, 0x00, 0x00, 0x46, // offset = 70
        0x00, 0x00, 0x00, 0x36, // length = 54
    ];

    // name table at offset 44: format(2) count(2) stringOffset(2) + record(12) + string(8)
    data.extend_from_slice(&[
        0x00, 0x00, // format
        0x00, 0x01, // count = 1
        0x00, 0x12, // stringOffset = 18
        0x00, 0x03, // platformID = 3 (Windows)
        0x00, 0x01, // encodingID = 1
        0x00, 0x09, // languageID
        0x00, 0x01, // nameID = 1 (FontFamily)
        0x00, 0x08, // length = 8
        0x00, 0x00, // offset = 0
        0x00, b'T', 0x00, b'e', 0x00, b's', 0x00, b't', // "Test" UTF-16BE
    ]);

    // head table at offset 70 (54 bytes).
    data.extend_from_slice(&[
        0x00, 0x01, 0x00, 0x00, // version
        0x00, 0x00, 0x00, 0x00, // fontRevision
        0x00, 0x00, 0x00, 0x00, // checksumAdjustment
        0x5F, 0x0F, 0x3C, 0xF5, // magic
        0x00, 0x00, // flags
        0x08, 0x00, // unitsPerEm = 2048
        0x00, 0x00, 0x00, 0x00, 0xD4, 0x36, 0x5E, 0x80, // created
        0x00, 0x00, 0x00, 0x00, 0xD4, 0x36, 0x5E, 0x80, // modified
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // bbox
        0x00, 0x00, // macStyle
        0x00, 0x08, // lowestRecPPEM
        0x00, 0x00, // fontDirectionHint
        0x00, 0x00, // indexToLocFormat
        0x00, 0x00, // glyphDataFormat
    ]);

    data
}

#[test]
fn ttf_verify_signatures() {
    let v1 = TestReader::new(vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x10]);
    assert!(TTFParser::verify_signature(&v1).unwrap());
    let mut t = b"true".to_vec();
    t.extend_from_slice(&[0x00, 0x10]);
    let tr = TestReader::new(t);
    assert!(TTFParser::verify_signature(&tr).unwrap());
    let bad = TestReader::new(vec![0xFF, 0xFF, 0xFF, 0xFF]);
    assert!(!TTFParser::verify_signature(&bad).unwrap());
    let tiny = TestReader::new(vec![0x00]);
    assert!(!TTFParser::verify_signature(&tiny).unwrap());
}

#[test]
fn ttf_read_num_tables() {
    let data = build_ttf();
    let reader = TestReader::new(data);
    assert_eq!(TTFParser::read_num_tables(&reader).unwrap(), 2);
}

#[test]
fn ttf_full_parse_name_and_head() {
    let data = build_ttf();
    let reader = TestReader::new(data);
    let md = parse_ttf_metadata(&reader).expect("ttf parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("TTF".to_string()))
    );
    assert_eq!(
        md.get("NumTables"),
        Some(&TagValue::String("2".to_string()))
    );
    assert_eq!(
        md.get("UnitsPerEm"),
        Some(&TagValue::String("2048".to_string()))
    );
    assert_eq!(
        md.get("FontFamily"),
        Some(&TagValue::String("Test".to_string()))
    );
    assert!(md.contains_key("FontCreated"));
    assert!(md.contains_key("FontModified"));
    // alias
    assert_eq!(
        md.get("TTF:FamilyName"),
        Some(&TagValue::String("Test".to_string()))
    );
    assert_eq!(
        md.get("TTF:UnitsPerEm"),
        Some(&TagValue::String("2048".to_string()))
    );
}

#[test]
fn ttf_invalid_signature_errors() {
    let reader = TestReader::new(vec![0xAB, 0xCD, 0xEF, 0x00, 0x00, 0x00]);
    assert!(parse_ttf_metadata(&reader).is_err());
}

#[test]
fn ttf_production_path_via_read_metadata() {
    let data = build_ttf();
    let f = temp_with(".ttf", &data);
    let md = read_metadata(f.path()).expect("read_metadata ttf");
    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("TTF".to_string()))
    );
}

// ===========================================================================
// OTF tests
// ===========================================================================

fn build_otf() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"OTTO"); // signature
    data.extend_from_slice(&[0x00, 0x03]); // numTables = 3
    data.extend_from_slice(&[0x00, 0x20]); // searchRange
    data.extend_from_slice(&[0x00, 0x01]); // entrySelector
    data.extend_from_slice(&[0x00, 0x00]); // rangeShift

    // Table dir: CFF, name, head. Dir = 12 + 3*16 = 60 bytes.
    // CFF table
    data.extend_from_slice(b"CFF ");
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // checksum
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0xC8]); // offset = 200
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x10]); // length = 16
    // name table
    data.extend_from_slice(b"name");
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x3C]); // offset = 60
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x1A]); // length = 26
    // head table
    data.extend_from_slice(b"head");
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x86]); // offset = 134
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x36]); // length = 54

    // Pad to offset 60 (already at 60 after 12 + 48).
    while data.len() < 60 {
        data.push(0);
    }

    // name table at offset 60: format(2) count(2) stringOffset(2) record(12) string(8)
    data.extend_from_slice(&[
        0x00, 0x00, // format
        0x00, 0x01, // count = 1
        0x00, 0x12, // stringOffset = 18
        0x00, 0x03, // platformID = 3 (Windows)
        0x00, 0x01, // encodingID = 1
        0x00, 0x09, // languageID
        0x00, 0x01, // nameID = 1 (FontFamily)
        0x00, 0x08, // length = 8
        0x00, 0x00, // offset = 0
        0x00, b'O', 0x00, b'p', 0x00, b'e', 0x00, b'n', // "Open"
    ]);

    // Pad to offset 134.
    while data.len() < 134 {
        data.push(0);
    }

    // head table (54 bytes) at offset 134.
    data.extend_from_slice(&[
        0x00, 0x01, 0x00, 0x00, // version
        0x00, 0x00, 0x00, 0x00, // fontRevision
        0x00, 0x00, 0x00, 0x00, // checksumAdjustment
        0x5F, 0x0F, 0x3C, 0xF5, // magic
        0x00, 0x00, // flags
        0x04, 0x00, // unitsPerEm = 1024
        0x00, 0x00, 0x00, 0x00, 0xD4, 0x36, 0x5E, 0x80, // created
        0x00, 0x00, 0x00, 0x00, 0xD4, 0x36, 0x5E, 0x80, // modified
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // bbox
        0x00, 0x00, // macStyle
        0x00, 0x08, // lowestRecPPEM
        0x00, 0x00, // fontDirectionHint
        0x00, 0x00, // indexToLocFormat
        0x00, 0x00, // glyphDataFormat
    ]);

    // Pad up to CFF table at offset 200 + 16.
    while data.len() < 216 {
        data.push(0);
    }

    data
}

#[test]
fn otf_verify_signature() {
    let mut d = b"OTTO".to_vec();
    d.extend_from_slice(&[0x00, 0x10]);
    let reader = TestReader::new(d);
    assert!(OTFParser::verify_signature(&reader).unwrap());
    let bad = TestReader::new(b"NOPE".to_vec());
    assert!(!OTFParser::verify_signature(&bad).unwrap());
    let tiny = TestReader::new(vec![0x4F]);
    assert!(!OTFParser::verify_signature(&tiny).unwrap());
}

#[test]
fn otf_full_parse() {
    let data = build_otf();
    let reader = TestReader::new(data);
    let md = parse_otf_metadata(&reader).expect("otf parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("OTF".to_string()))
    );
    assert_eq!(
        md.get("NumTables"),
        Some(&TagValue::String("3".to_string()))
    );
    assert_eq!(
        md.get("OutlineFormat"),
        Some(&TagValue::String("CFF".to_string()))
    );
    assert_eq!(
        md.get("FontFamily"),
        Some(&TagValue::String("Open".to_string()))
    );
    assert_eq!(
        md.get("UnitsPerEm"),
        Some(&TagValue::String("1024".to_string()))
    );
    assert!(md.contains_key("CreatedDate"));
    assert!(md.contains_key("ModifiedDate"));
    // aliases
    assert_eq!(
        md.get("OTF:FamilyName"),
        Some(&TagValue::String("Open".to_string()))
    );
    assert_eq!(
        md.get("OTF:ScalerType"),
        Some(&TagValue::String("CFF".to_string()))
    );
    assert_eq!(
        md.get("OTF:TableCount"),
        Some(&TagValue::String("3".to_string()))
    );
}

#[test]
fn otf_invalid_signature_errors() {
    let reader = TestReader::new(b"TTTT0000".to_vec());
    assert!(parse_otf_metadata(&reader).is_err());
}

#[test]
fn otf_production_path_via_read_metadata() {
    let data = build_otf();
    let f = temp_with(".otf", &data);
    let md = read_metadata(f.path()).expect("read_metadata otf");
    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("OTF".to_string()))
    );
}

// ===========================================================================
// WOFF tests
// ===========================================================================

/// Minimal 44-byte WOFF header (no tables, no metadata).
fn build_woff_header(num_tables: u16, flavor: &[u8; 4], version: (u16, u16)) -> Vec<u8> {
    let mut h = Vec::new();
    h.extend_from_slice(b"wOFF"); // signature
    h.extend_from_slice(flavor); // flavor
    h.extend_from_slice(&0u32.to_be_bytes()); // length
    h.extend_from_slice(&num_tables.to_be_bytes()); // numTables
    h.extend_from_slice(&[0x00, 0x00]); // reserved
    h.extend_from_slice(&0x1000u32.to_be_bytes()); // totalSfntSize
    h.extend_from_slice(&version.0.to_be_bytes()); // majorVersion
    h.extend_from_slice(&version.1.to_be_bytes()); // minorVersion
    h.extend_from_slice(&0u32.to_be_bytes()); // metaOffset
    h.extend_from_slice(&0u32.to_be_bytes()); // metaLength
    h.extend_from_slice(&0u32.to_be_bytes()); // metaOrigLength
    h.extend_from_slice(&0u32.to_be_bytes()); // privOffset
    h.extend_from_slice(&0u32.to_be_bytes()); // privLength
    h
}

#[test]
fn woff_signature_and_flavor() {
    let mut d = b"wOFF".to_vec();
    d.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
    let tt = TestReader::new(d);
    assert!(WOFFParser::verify_signature(&tt).unwrap());
    assert_eq!(WOFFParser::read_flavor(&tt).unwrap(), "TrueType");

    let mut c = b"wOFF".to_vec();
    c.extend_from_slice(b"OTTO");
    let cff = TestReader::new(c);
    assert_eq!(WOFFParser::read_flavor(&cff).unwrap(), "CFF");

    let mut u = b"wOFF".to_vec();
    u.extend_from_slice(&[0x12, 0x34, 0x56, 0x78]);
    let unk = TestReader::new(u);
    assert_eq!(WOFFParser::read_flavor(&unk).unwrap(), "Unknown");

    let bad = TestReader::new(b"NOPE".to_vec());
    assert!(!WOFFParser::verify_signature(&bad).unwrap());
}

#[test]
fn woff_full_header_parse() {
    let h = build_woff_header(3, &[0x00, 0x01, 0x00, 0x00], (1, 2));
    let reader = TestReader::new(h);
    let md = parse_woff_metadata(&reader).expect("woff parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("WOFF".to_string()))
    );
    assert_eq!(
        md.get("FontFlavor"),
        Some(&TagValue::String("TrueType".to_string()))
    );
    assert_eq!(
        md.get("NumTables"),
        Some(&TagValue::String("3".to_string()))
    );
    assert_eq!(
        md.get("FontVersion"),
        Some(&TagValue::String("1.2".to_string()))
    );
    assert_eq!(
        md.get("TotalSfntSize"),
        Some(&TagValue::String("4096".to_string()))
    );
    assert_eq!(
        md.get("HasMetadata"),
        Some(&TagValue::String("No".to_string()))
    );
    assert_eq!(
        md.get("HasPrivateData"),
        Some(&TagValue::String("No".to_string()))
    );
    assert!(md.contains_key("CompressionRatio"));
}

#[test]
fn woff_with_embedded_zlib_metadata_block() {
    // Build a WOFF whose metadata block is a zlib-compressed XML payload, so the
    // parser exercises decompress_zlib + parse_xml_metadata internally.
    use flate2::Compression;
    use flate2::write::ZlibEncoder;

    let xml = r#"<metadata><vendor>Test Vendor</vendor><description>Desc</description><license>MIT</license></metadata>"#;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(xml.as_bytes()).unwrap();
    let compressed = encoder.finish().unwrap();

    // Header is 44 bytes; place the metadata block right after.
    let meta_offset = 44u32;
    let meta_length = compressed.len() as u32;

    let mut h = Vec::new();
    h.extend_from_slice(b"wOFF");
    h.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]); // flavor TTF
    h.extend_from_slice(&0u32.to_be_bytes()); // length
    h.extend_from_slice(&0u16.to_be_bytes()); // numTables = 0
    h.extend_from_slice(&[0x00, 0x00]); // reserved
    h.extend_from_slice(&0x1000u32.to_be_bytes()); // totalSfntSize
    h.extend_from_slice(&1u16.to_be_bytes()); // majorVersion
    h.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
    h.extend_from_slice(&meta_offset.to_be_bytes()); // metaOffset
    h.extend_from_slice(&meta_length.to_be_bytes()); // metaLength
    h.extend_from_slice(&(xml.len() as u32).to_be_bytes()); // metaOrigLength
    h.extend_from_slice(&0u32.to_be_bytes()); // privOffset
    h.extend_from_slice(&0u32.to_be_bytes()); // privLength
    h.extend_from_slice(&compressed); // metadata block payload

    let reader = TestReader::new(h);
    let md = parse_woff_metadata(&reader).expect("woff parse with metadata");

    assert_eq!(
        md.get("HasMetadata"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert_eq!(
        md.get("WOFFVendor"),
        Some(&TagValue::String("Test Vendor".to_string()))
    );
    assert_eq!(
        md.get("WOFFLicense"),
        Some(&TagValue::String("MIT".to_string()))
    );
}

#[test]
fn woff_header_too_short_errors() {
    let reader = TestReader::new(b"wOFF\x00\x01\x00\x00".to_vec());
    assert!(parse_woff_metadata(&reader).is_err());
}

#[test]
fn woff_production_path_via_read_metadata() {
    let h = build_woff_header(2, b"OTTO", (1, 0));
    let f = temp_with(".woff", &h);
    let md = read_metadata(f.path()).expect("read_metadata woff");
    assert_eq!(
        md.get("FontFlavor"),
        Some(&TagValue::String("CFF".to_string()))
    );
}

// ===========================================================================
// WOFF2 tests
// ===========================================================================

#[allow(clippy::too_many_arguments)]
fn build_woff2_header(
    flavor: &[u8; 4],
    num_tables: u16,
    total_sfnt_size: u32,
    total_compressed_size: u32,
    version: (u16, u16),
    has_meta: bool,
    has_priv: bool,
) -> Vec<u8> {
    let mut d = Vec::with_capacity(48);
    d.extend_from_slice(b"wOF2");
    d.extend_from_slice(flavor);
    d.extend_from_slice(&48u32.to_be_bytes()); // length
    d.extend_from_slice(&num_tables.to_be_bytes());
    d.extend_from_slice(&[0, 0]); // reserved
    d.extend_from_slice(&total_sfnt_size.to_be_bytes());
    d.extend_from_slice(&total_compressed_size.to_be_bytes());
    d.extend_from_slice(&version.0.to_be_bytes());
    d.extend_from_slice(&version.1.to_be_bytes());
    d.extend_from_slice(&(if has_meta { 100u32 } else { 0 }).to_be_bytes());
    d.extend_from_slice(&(if has_meta { 50u32 } else { 0 }).to_be_bytes());
    d.extend_from_slice(&(if has_meta { 100u32 } else { 0 }).to_be_bytes());
    d.extend_from_slice(&(if has_priv { 200u32 } else { 0 }).to_be_bytes());
    d.extend_from_slice(&(if has_priv { 75u32 } else { 0 }).to_be_bytes());
    d
}

#[test]
fn woff2_signature() {
    let d = build_woff2_header(
        &[0x00, 0x01, 0x00, 0x00],
        10,
        10000,
        5000,
        (1, 0),
        false,
        false,
    );
    let reader = TestReader::new(d);
    assert!(WOFF2Parser::verify_signature(&reader).unwrap());
    let bad = TestReader::new(b"NOPE".to_vec());
    assert!(!WOFF2Parser::verify_signature(&bad).unwrap());
    let tiny = TestReader::new(vec![0x77]);
    assert!(!WOFF2Parser::verify_signature(&tiny).unwrap());
}

#[test]
fn woff2_full_header_with_blocks() {
    let d = build_woff2_header(
        &[0x00, 0x01, 0x00, 0x00],
        12,
        20000,
        8000,
        (1, 5),
        true,
        true,
    );
    let reader = TestReader::new(d);
    let md = parse_woff2_metadata(&reader).expect("woff2 parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("WOFF2".to_string()))
    );
    assert_eq!(
        md.get("FontFlavor"),
        Some(&TagValue::String("TrueType".to_string()))
    );
    assert_eq!(md.get("NumTables"), Some(&TagValue::Integer(12)));
    assert_eq!(md.get("TotalSfntSize"), Some(&TagValue::Integer(20000)));
    assert_eq!(
        md.get("TotalCompressedSize"),
        Some(&TagValue::Integer(8000))
    );
    assert_eq!(
        md.get("FontVersion"),
        Some(&TagValue::String("1.5".to_string()))
    );
    assert_eq!(
        md.get("HasMetadata"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert!(md.contains_key("MetadataOffset"));
    assert!(md.contains_key("MetadataLength"));
    assert!(md.contains_key("MetadataOrigLength"));
    assert_eq!(
        md.get("HasPrivateData"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert!(md.contains_key("PrivateDataOffset"));
    assert!(md.contains_key("PrivateDataLength"));
}

#[test]
fn woff2_cff_flavor_and_ratio_no_blocks() {
    let d = build_woff2_header(b"OTTO", 10, 10000, 4000, (2, 0), false, false);
    let reader = TestReader::new(d);
    let md = parse_woff2_metadata(&reader).expect("woff2 parse");
    assert_eq!(
        md.get("FontFlavor"),
        Some(&TagValue::String("CFF".to_string()))
    );
    assert_eq!(
        md.get("CompressionRatio"),
        Some(&TagValue::String("40.0%".to_string()))
    );
    assert_eq!(
        md.get("HasMetadata"),
        Some(&TagValue::String("No".to_string()))
    );
    assert!(md.get("MetadataOffset").is_none());
    assert!(md.get("PrivateDataOffset").is_none());
}

#[test]
fn woff2_header_too_short_errors() {
    let reader = TestReader::new(b"wOF2\x00\x01\x00\x00".to_vec());
    assert!(parse_woff2_metadata(&reader).is_err());
}

#[test]
fn woff2_production_path_via_read_metadata() {
    let d = build_woff2_header(
        &[0x00, 0x01, 0x00, 0x00],
        8,
        15000,
        6000,
        (1, 0),
        false,
        false,
    );
    let f = temp_with(".woff2", &d);
    let md = read_metadata(f.path()).expect("read_metadata woff2");
    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("WOFF2".to_string()))
    );
}

// ===========================================================================
// TXT tests
// ===========================================================================

#[test]
fn txt_encoding_detection_all_variants() {
    assert_eq!(
        TXTParser::detect_encoding(b"plain ascii"),
        (Encoding::ASCII, false)
    );
    assert_eq!(
        TXTParser::detect_encoding("utf8 \u{2122}".as_bytes()),
        (Encoding::UTF8, false)
    );
    assert_eq!(
        TXTParser::detect_encoding(b"\xEF\xBB\xBFwith bom"),
        (Encoding::UTF8, true)
    );
    assert_eq!(
        TXTParser::detect_encoding(b"\xFF\xFEH\x00i\x00"),
        (Encoding::UTF16LE, true)
    );
    assert_eq!(
        TXTParser::detect_encoding(b"\xFE\xFF\x00H\x00i"),
        (Encoding::UTF16BE, true)
    );
    assert_eq!(
        TXTParser::detect_encoding(b"\xFF\xFE\x00\x00abc"),
        (Encoding::UTF32LE, true)
    );
    assert_eq!(
        TXTParser::detect_encoding(b"\x00\x00\xFE\xFFabc"),
        (Encoding::UTF32BE, true)
    );
    // Invalid UTF-8, high bytes -> Unknown
    assert_eq!(
        TXTParser::detect_encoding(&[0x80, 0xFE, 0x41, 0x42]),
        (Encoding::Unknown, false)
    );
}

#[test]
fn txt_encoding_mime_names() {
    assert_eq!(Encoding::ASCII.mime_name(), "us-ascii");
    assert_eq!(Encoding::UTF8.mime_name(), "utf-8");
    assert_eq!(Encoding::UTF16LE.mime_name(), "utf-16le");
    assert_eq!(Encoding::UTF16BE.mime_name(), "utf-16be");
    assert_eq!(Encoding::UTF32LE.mime_name(), "utf-32le");
    assert_eq!(Encoding::UTF32BE.mime_name(), "utf-32be");
    assert_eq!(Encoding::Unknown.mime_name(), "unknown");
}

#[test]
fn txt_line_ending_detection() {
    assert_eq!(TXTParser::detect_line_endings("a\nb\nc"), LineEnding::LF);
    assert_eq!(
        TXTParser::detect_line_endings("a\r\nb\r\n"),
        LineEnding::CRLF
    );
    assert_eq!(TXTParser::detect_line_endings("a\rb\rc"), LineEnding::CR);
    assert_eq!(
        TXTParser::detect_line_endings("noendings"),
        LineEnding::None
    );
    assert_eq!(TXTParser::detect_line_endings("a\nb\rc"), LineEnding::Mixed);
    // display names
    assert_eq!(LineEnding::LF.display_name(), "Unix LF");
    assert_eq!(LineEnding::CRLF.display_name(), "Windows CRLF");
    assert_eq!(LineEnding::CR.display_name(), "Mac CR");
    assert_eq!(LineEnding::Mixed.display_name(), "Mixed");
    assert_eq!(LineEnding::None.display_name(), "(none)");
}

#[test]
fn txt_compute_stats() {
    let s = TXTParser::compute_stats("Hello World\nSecond line here");
    assert_eq!(s.line_count, 2);
    assert_eq!(s.word_count, 5);
    assert!(s.char_count > 0);

    let empty = TXTParser::compute_stats("");
    assert_eq!(empty.line_count, 0);
    assert_eq!(empty.word_count, 0);

    let one = TXTParser::compute_stats("just one line");
    assert_eq!(one.line_count, 1);
    assert_eq!(one.word_count, 3);

    let trailing = TXTParser::compute_stats("a\nb\n");
    assert_eq!(trailing.line_count, 2);
}

#[test]
fn txt_parse_ascii_full_metadata() {
    let content = b"Line one\nLine two\nLine three\n";
    let reader = TestReader::new(content.to_vec());
    let md = parse_txt_metadata(&reader).expect("txt parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("TXT".to_string()))
    );
    assert_eq!(
        md.get("MIMEType"),
        Some(&TagValue::String("text/plain".to_string()))
    );
    assert_eq!(
        md.get("MIMEEncoding"),
        Some(&TagValue::String("us-ascii".to_string()))
    );
    assert_eq!(
        md.get("ByteOrderMark"),
        Some(&TagValue::String("No".to_string()))
    );
    assert_eq!(
        md.get("Newlines"),
        Some(&TagValue::String("Unix LF".to_string()))
    );
    assert!(md.contains_key("LineCount"));
    assert!(md.contains_key("WordCount"));
    // aliases
    assert_eq!(
        md.get("TEXT:Encoding"),
        Some(&TagValue::String("us-ascii".to_string()))
    );
    assert!(md.contains_key("TEXT:LineEnding"));
    assert!(md.contains_key("TEXT:HasBOM"));
}

#[test]
fn txt_parse_utf8_with_bom_crlf() {
    let mut content = vec![0xEF, 0xBB, 0xBF];
    content.extend_from_slice("héllo\r\nwörld\r\n".as_bytes());
    let reader = TestReader::new(content);
    let md = parse_txt_metadata(&reader).expect("txt parse");
    assert_eq!(
        md.get("MIMEEncoding"),
        Some(&TagValue::String("utf-8".to_string()))
    );
    assert_eq!(
        md.get("ByteOrderMark"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert_eq!(
        md.get("Newlines"),
        Some(&TagValue::String("Windows CRLF".to_string()))
    );
}

#[test]
fn txt_parse_utf16_returns_early() {
    // UTF-16LE BOM: parser inserts encoding + BOM then returns early.
    let content = b"\xFF\xFEH\x00e\x00l\x00l\x00o\x00".to_vec();
    let reader = TestReader::new(content);
    let md = parse_txt_metadata(&reader).expect("txt parse");
    assert_eq!(
        md.get("MIMEEncoding"),
        Some(&TagValue::String("utf-16le".to_string()))
    );
    assert_eq!(
        md.get("ByteOrderMark"),
        Some(&TagValue::String("Yes".to_string()))
    );
    // No newline/linecount analysis for non-UTF8/ASCII.
    assert!(!md.contains_key("Newlines"));
}

#[test]
fn txt_formatparser_trait() {
    let parser = TXTParser;
    let reader = TestReader::new(b"hello world".to_vec());
    let md = parser.parse(&reader).expect("txt format parse");
    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("TXT".to_string()))
    );
    assert!(parser.supports_format(oxidex::core::FileFormat::TXT));
    assert!(!parser.supports_format(oxidex::core::FileFormat::OLE));
}

#[test]
fn txt_production_path_via_read_metadata() {
    let content = b"Production path text file.\nWith two lines.\n";
    let f = temp_with(".txt", content);
    let md = read_metadata(f.path()).expect("read_metadata txt");
    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("TXT".to_string()))
    );
    assert!(md.contains_key("WordCount"));
}
