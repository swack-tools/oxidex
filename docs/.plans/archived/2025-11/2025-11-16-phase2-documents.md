# Phase 2: Document Formats Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add metadata extraction for document formats (DOCX, XLSX, PPTX, Pages, Numbers, Keynote, EPUB)

**Architecture:** ZIP-based document parsers. Office formats are ZIP archives containing XML metadata. Parse ZIP structure, extract XML files, parse Office/iWork metadata.

**Tech Stack:** Rust, zip crate (archive handling), quick-xml (XML parsing), nom (binary parsing)

**Timeline:** 2-3 months

**Reference Design:** `docs/plans/2025-11-16-comprehensive-format-support-design.md`

---

## Prerequisites

### Task 0: Setup ZIP Foundation

**Files:**
- Create: `src/parsers/archive/mod.rs`
- Create: `src/parsers/archive/zip.rs`
- Create: `src/parsers/document/mod.rs`
- Modify: `src/core/file_format.rs`
- Modify: `Cargo.toml`

**Step 1: Add zip dependency**

In `Cargo.toml`, add:

```toml
[dependencies]
zip = "0.6"
```

**Step 2: Create archive parser module**

Create `src/parsers/archive/mod.rs`:

```rust
//! Archive format parsers

pub mod zip;
pub use zip::ZipParser;
```

**Step 3: Create document parser module**

Create `src/parsers/document/mod.rs`:

```rust
//! Document format parsers

pub mod ooxml;
pub mod iwork;
pub mod epub;

pub use ooxml::{DocxParser, XlsxParser, PptxParser};
pub use iwork::{PagesParser, NumbersParser, KeynoteParser};
pub use epub::EpubParser;
```

**Step 4: Add FileFormat enum variants**

In `src/core/file_format.rs`:

```rust
// Phase 2: Document formats
/// ZIP archive format (.zip)
ZIP,

/// DOCX document format (.docx)
DOCX,

/// XLSX spreadsheet format (.xlsx)
XLSX,

/// PPTX presentation format (.pptx)
PPTX,

/// Apple Pages document (.pages)
Pages,

/// Apple Numbers spreadsheet (.numbers)
Numbers,

/// Apple Keynote presentation (.key)
Keynote,

/// EPUB e-book format (.epub)
EPUB,
```

**Step 5: Commit**

```bash
git add src/parsers/archive src/parsers/document src/core/file_format.rs Cargo.toml
git commit -m "feat: add Phase 2 document format infrastructure

- Add ZIP parser foundation
- Create document parser modules
- Add 8 new FileFormat variants (ZIP, DOCX, XLSX, PPTX, Pages, Numbers, Keynote, EPUB)
- Add zip crate dependency"
```

---

## Parser 1: ZIP

**Reference:** ExifTool `lib/Image/ExifTool/ZIP.pm`
**Magic Bytes:** `50 4B 03 04` or `50 4B 05 06` ("PK")
**Spec:** PKZIP specification

### Task 1.1: ZIP Parser

**Files:**
- Create: `src/parsers/archive/zip.rs`
- Create: `tests/unit/archive/zip_tests.rs`

**Step 1: Write failing test**

```rust
use oxidex::parsers::archive::zip::ZipParser;
use oxidex::core::FormatParser;

#[test]
fn test_zip_magic_bytes() {
    let data = b"PK\x03\x04...";
    let reader = BufferedReader::from_bytes(data);
    let parser = ZipParser;
    assert!(parser.parse(&reader).is_ok());
}
```

**Step 2: Implement ZIP parser**

Create `src/parsers/archive/zip.rs`:

```rust
//! ZIP archive format parser

use crate::core::{FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use std::io::{Cursor, Read};
use zip::ZipArchive;

const ZIP_SIGNATURE: &[u8] = b"PK";

pub struct ZipParser;

impl FormatParser for ZipParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify ZIP signature
        if reader.size() < 4 {
            return Err(ExifToolError::parse_error("File too small to be ZIP"));
        }

        let header = reader.read(0, 2)?;
        if header != ZIP_SIGNATURE {
            return Err(ExifToolError::parse_error("Invalid ZIP signature"));
        }

        let mut metadata = MetadataMap::new();

        // Read entire file into memory for zip crate
        let file_data = reader.read_all()?;
        let cursor = Cursor::new(file_data);
        let mut archive = ZipArchive::new(cursor)
            .map_err(|e| ExifToolError::parse_error(format!("Failed to read ZIP: {}", e)))?;

        // Extract basic metadata
        metadata.insert(
            "ZIP:FileCount".to_string(),
            TagValue::new_integer(archive.len() as i64),
        );

        // List files
        let mut file_names = Vec::new();
        for i in 0..archive.len() {
            if let Ok(file) = archive.by_index(i) {
                file_names.push(file.name().to_string());
            }
        }

        if !file_names.is_empty() {
            metadata.insert(
                "ZIP:Files".to_string(),
                TagValue::new_string(file_names.join(", ")),
            );
        }

        Ok(metadata)
    }
}
```

**Step 3: Commit**

```bash
git add src/parsers/archive/zip.rs tests/unit/archive/zip_tests.rs
git commit -m "feat: implement ZIP archive parser

- Parse ZIP file structure using zip crate
- Extract file count and file list
- Foundation for Office format parsers"
```

---

## Parser 2: DOCX (Office Open XML)

**Reference:** ExifTool `lib/Image/ExifTool/OOXML.pm`
**Detection:** ZIP file containing `[Content_Types].xml` and `word/document.xml`

### Task 2.1: DOCX Parser

**Files:**
- Create: `src/parsers/document/ooxml.rs`
- Create: `tests/unit/document/docx_tests.rs`

**Step 1: Implement OOXML parser**

Create `src/parsers/document/ooxml.rs`:

```rust
//! Office Open XML (DOCX, XLSX, PPTX) format parsers

use crate::core::{FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::io::Cursor;
use zip::ZipArchive;

/// DOCX parser
pub struct DocxParser;

impl FormatParser for DocxParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();

        // Read as ZIP
        let file_data = reader.read_all()?;
        let cursor = Cursor::new(file_data);
        let mut archive = ZipArchive::new(cursor)
            .map_err(|e| ExifToolError::parse_error(format!("Not a valid DOCX: {}", e)))?;

        // Check for DOCX-specific files
        let has_content_types = archive.by_name("[Content_Types].xml").is_ok();
        let has_word_doc = archive.by_name("word/document.xml").is_ok();

        if !has_content_types || !has_word_doc {
            return Err(ExifToolError::parse_error("Not a valid DOCX file"));
        }

        // Parse core.xml for metadata
        if let Ok(mut core_file) = archive.by_name("docProps/core.xml") {
            let mut xml_content = String::new();
            core_file.read_to_string(&mut xml_content)
                .map_err(|e| ExifToolError::parse_error(format!("Failed to read core.xml: {}", e)))?;

            parse_core_properties(&xml_content, &mut metadata)?;
        }

        // Parse app.xml for application properties
        if let Ok(mut app_file) = archive.by_name("docProps/app.xml") {
            let mut xml_content = String::new();
            app_file.read_to_string(&mut xml_content)
                .map_err(|e| ExifToolError::parse_error(format!("Failed to read app.xml: {}", e)))?;

            parse_app_properties(&xml_content, &mut metadata)?;
        }

        Ok(metadata)
    }
}

/// XLSX parser
pub struct XlsxParser;

impl FormatParser for XlsxParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Similar to DOCX, but check for xl/workbook.xml
        let mut metadata = MetadataMap::new();
        let file_data = reader.read_all()?;
        let cursor = Cursor::new(file_data);
        let mut archive = ZipArchive::new(cursor)
            .map_err(|e| ExifToolError::parse_error(format!("Not a valid XLSX: {}", e)))?;

        if archive.by_name("xl/workbook.xml").is_err() {
            return Err(ExifToolError::parse_error("Not a valid XLSX file"));
        }

        // Parse metadata from docProps
        if let Ok(mut core_file) = archive.by_name("docProps/core.xml") {
            let mut xml_content = String::new();
            core_file.read_to_string(&mut xml_content).ok();
            parse_core_properties(&xml_content, &mut metadata)?;
        }

        Ok(metadata)
    }
}

/// PPTX parser
pub struct PptxParser;

impl FormatParser for PptxParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Similar to DOCX, but check for ppt/presentation.xml
        let mut metadata = MetadataMap::new();
        let file_data = reader.read_all()?;
        let cursor = Cursor::new(file_data);
        let mut archive = ZipArchive::new(cursor)
            .map_err(|e| ExifToolError::parse_error(format!("Not a valid PPTX: {}", e)))?;

        if archive.by_name("ppt/presentation.xml").is_err() {
            return Err(ExifToolError::parse_error("Not a valid PPTX file"));
        }

        // Parse metadata
        if let Ok(mut core_file) = archive.by_name("docProps/core.xml") {
            let mut xml_content = String::new();
            core_file.read_to_string(&mut xml_content).ok();
            parse_core_properties(&xml_content, &mut metadata)?;
        }

        Ok(metadata)
    }
}

/// Parse core.xml properties (Dublin Core metadata)
fn parse_core_properties(xml: &str, metadata: &mut MetadataMap) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);

    let mut buf = Vec::new();
    let mut current_element = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                current_element = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
            }
            Ok(Event::Text(e)) => {
                let text = e.unescape().unwrap_or_default();
                if !text.is_empty() {
                    let tag_name = match current_element.as_str() {
                        "title" => "OOXML:Title",
                        "creator" => "OOXML:Creator",
                        "subject" => "OOXML:Subject",
                        "description" => "OOXML:Description",
                        "created" => "OOXML:CreateDate",
                        "modified" => "OOXML:ModifyDate",
                        _ => continue,
                    };
                    metadata.insert(tag_name.to_string(), TagValue::new_string(text.to_string()));
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(ExifToolError::parse_error(format!("XML parse error: {}", e))),
            _ => {}
        }
        buf.clear();
    }

    Ok(())
}

/// Parse app.xml properties
fn parse_app_properties(xml: &str, metadata: &mut MetadataMap) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);

    let mut buf = Vec::new();
    let mut current_element = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                current_element = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
            }
            Ok(Event::Text(e)) => {
                let text = e.unescape().unwrap_or_default();
                if !text.is_empty() {
                    let tag_name = match current_element.as_str() {
                        "Application" => "OOXML:Application",
                        "Pages" => "OOXML:Pages",
                        "Words" => "OOXML:Words",
                        "Characters" => "OOXML:Characters",
                        "Company" => "OOXML:Company",
                        _ => continue,
                    };
                    metadata.insert(tag_name.to_string(), TagValue::new_string(text.to_string()));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(())
}
```

**Step 2: Commit**

```bash
git add src/parsers/document/ooxml.rs tests/unit/document/docx_tests.rs
git commit -m "feat: implement DOCX, XLSX, PPTX parsers

- Parse Office Open XML ZIP structure
- Extract Dublin Core metadata from core.xml
- Extract application properties from app.xml
- Support Word, Excel, PowerPoint formats"
```

---

## Remaining Parsers (Abbreviated)

### Parser 3-5: iWork (Pages, Numbers, Keynote)

**Files:**
- Create: `src/parsers/document/iwork.rs`

Similar ZIP-based approach, parse Index/Metadata.iwa files.

### Parser 6: EPUB

**Files:**
- Create: `src/parsers/document/epub.rs`

ZIP-based, parse `META-INF/container.xml` and `content.opf`.

---

## Testing & Integration

### Task 10: Integration Tests

Test all document formats with ExifTool parity.

### Task 11: Benchmarks

Ensure <20ms per document.

### Task 12: Documentation

Update user guide with document format examples.

---

## Success Criteria

- [ ] All 8 parsers implemented
- [ ] 100% ExifTool parity
- [ ] Tests passing
- [ ] Performance targets met
- [ ] Documentation complete

**Estimated Tasks:** ~40-50 tasks
**Timeline:** 2-3 months
