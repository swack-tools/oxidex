//! Office Open XML (DOCX, XLSX, PPTX) format parsers

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::io::{Cursor, Read};
use zip::ZipArchive;

/// DOCX parser
pub struct DocxParser;

impl FormatParser for DocxParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();

        // Read as ZIP
        let size = reader.size() as usize;
        let file_data = reader.read(0, size)?;
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
            core_file.read_to_string(&mut xml_content).map_err(|e| {
                ExifToolError::parse_error(format!("Failed to read core.xml: {}", e))
            })?;

            parse_core_properties(&xml_content, &mut metadata)?;
        }

        // Parse app.xml for application properties
        if let Ok(mut app_file) = archive.by_name("docProps/app.xml") {
            let mut xml_content = String::new();
            app_file.read_to_string(&mut xml_content).map_err(|e| {
                ExifToolError::parse_error(format!("Failed to read app.xml: {}", e))
            })?;

            parse_app_properties(&xml_content, &mut metadata)?;
        }

        // Parse custom.xml for custom properties
        if let Ok(mut custom_file) = archive.by_name("docProps/custom.xml") {
            let mut xml_content = String::new();
            custom_file.read_to_string(&mut xml_content).map_err(|e| {
                ExifToolError::parse_error(format!("Failed to read custom.xml: {}", e))
            })?;

            parse_custom_properties(&xml_content, &mut metadata)?;
        }

        // Parse [Content_Types].xml
        if let Ok(mut content_types_file) = archive.by_name("[Content_Types].xml") {
            let mut xml_content = String::new();
            content_types_file
                .read_to_string(&mut xml_content)
                .map_err(|e| {
                    ExifToolError::parse_error(format!("Failed to read [Content_Types].xml: {}", e))
                })?;

            parse_content_types(&xml_content, &mut metadata)?;
        }

        // Parse DOCX-specific properties
        parse_docx_specific(&mut archive, &mut metadata)?;

        // Add DOCX-specific tag aliases for Worker 20 requirements
        add_docx_tag_aliases(&mut metadata);

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::DOCX)
    }
}

/// XLSX parser
pub struct XlsxParser;

impl FormatParser for XlsxParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();
        let size = reader.size() as usize;
        let file_data = reader.read(0, size)?;
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

        if let Ok(mut app_file) = archive.by_name("docProps/app.xml") {
            let mut xml_content = String::new();
            app_file.read_to_string(&mut xml_content).ok();
            parse_app_properties(&xml_content, &mut metadata)?;
        }

        if let Ok(mut custom_file) = archive.by_name("docProps/custom.xml") {
            let mut xml_content = String::new();
            custom_file.read_to_string(&mut xml_content).ok();
            parse_custom_properties(&xml_content, &mut metadata)?;
        }

        // Parse [Content_Types].xml
        if let Ok(mut content_types_file) = archive.by_name("[Content_Types].xml") {
            let mut xml_content = String::new();
            content_types_file.read_to_string(&mut xml_content).ok();
            parse_content_types(&xml_content, &mut metadata)?;
        }

        // Parse XLSX-specific properties
        parse_xlsx_specific(&mut archive, &mut metadata)?;

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::XLSX)
    }
}

/// PPTX parser
pub struct PptxParser;

impl FormatParser for PptxParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();
        let size = reader.size() as usize;
        let file_data = reader.read(0, size)?;
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

        if let Ok(mut app_file) = archive.by_name("docProps/app.xml") {
            let mut xml_content = String::new();
            app_file.read_to_string(&mut xml_content).ok();
            parse_app_properties(&xml_content, &mut metadata)?;
        }

        if let Ok(mut custom_file) = archive.by_name("docProps/custom.xml") {
            let mut xml_content = String::new();
            custom_file.read_to_string(&mut xml_content).ok();
            parse_custom_properties(&xml_content, &mut metadata)?;
        }

        // Parse [Content_Types].xml
        if let Ok(mut content_types_file) = archive.by_name("[Content_Types].xml") {
            let mut xml_content = String::new();
            content_types_file.read_to_string(&mut xml_content).ok();
            parse_content_types(&xml_content, &mut metadata)?;
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::PPTX)
    }
}

/// Parse core.xml properties (Dublin Core metadata)
fn parse_core_properties(xml: &str, metadata: &mut MetadataMap) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut current_element = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                current_element = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
            }
            Ok(Event::Text(e)) => {
                if let Ok(text) = e.xml10_content()
                    && !text.is_empty()
                {
                    let tag_name = match current_element.as_str() {
                        "title" => "OOXML:Title",
                        "creator" => "OOXML:Creator",
                        "subject" => "OOXML:Subject",
                        "description" => "OOXML:Description",
                        "keywords" => "OOXML:Keywords",
                        "created" => "OOXML:CreateDate",
                        "modified" => "OOXML:ModifyDate",
                        "lastModifiedBy" => "OOXML:LastModifiedBy",
                        "revision" => "OOXML:RevisionNumber",
                        "lastPrinted" => "OOXML:LastPrinted",
                        "category" => "OOXML:Category",
                        "contentStatus" => "OOXML:ContentStatus",
                        _ => {
                            buf.clear();
                            continue;
                        }
                    };
                    metadata.insert(tag_name.to_string(), TagValue::new_string(text.to_string()));
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(ExifToolError::parse_error(format!(
                    "XML parse error: {}",
                    e
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(())
}

/// Parse app.xml properties
fn parse_app_properties(xml: &str, metadata: &mut MetadataMap) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut current_element = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                current_element = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
            }
            Ok(Event::Text(e)) => {
                if let Ok(text) = e.xml10_content()
                    && !text.is_empty()
                {
                    let tag_name = match current_element.as_str() {
                        "Application" => "OOXML:Application",
                        "Pages" => "OOXML:Pages",
                        "Words" => "OOXML:Words",
                        "Characters" => "OOXML:Characters",
                        "CharactersWithSpaces" => "OOXML:CharactersWithSpaces",
                        "Lines" => "OOXML:Lines",
                        "Paragraphs" => "OOXML:Paragraphs",
                        "Company" => "OOXML:Company",
                        "Manager" => "OOXML:Manager",
                        "Template" => "OOXML:Template",
                        "HyperlinkBase" => "OOXML:HyperlinkBase",
                        "HiddenSlides" => "OOXML:HiddenSlides",
                        "PresentationFormat" => "OOXML:PresentationFormat",
                        "AppVersion" => "OOXML:AppVersion",
                        "DocSecurity" => "OOXML:DocSecurity",
                        "ScaleCrop" => "OOXML:ScaleCrop",
                        "LinksUpToDate" => "OOXML:LinksUpToDate",
                        "SharedDoc" => "OOXML:SharedDoc",
                        "HyperlinksChanged" => "OOXML:HyperlinksChanged",
                        "TotalTime" => {
                            // Convert minutes to human-readable format
                            if let Ok(minutes) = text.parse::<u64>() {
                                let formatted = format_edit_time(minutes);
                                metadata.insert(
                                    "OOXML:TotalEditTime".to_string(),
                                    TagValue::new_string(formatted),
                                );
                            }
                            buf.clear();
                            continue;
                        }
                        _ => {
                            buf.clear();
                            continue;
                        }
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

/// Parse custom.xml properties (user-defined metadata)
fn parse_custom_properties(xml: &str, metadata: &mut MetadataMap) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut current_property_name = String::new();
    let mut in_property = false;
    let mut in_value = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();

                if element_name == "property" {
                    in_property = true;
                    // Extract property name from attribute
                    for attr in e.attributes().flatten() {
                        let key_bytes = attr.key.local_name();
                        let key = String::from_utf8_lossy(key_bytes.as_ref());
                        if key == "name" {
                            current_property_name =
                                String::from_utf8_lossy(&attr.value).to_string();
                        }
                    }
                } else if in_property
                    && (element_name == "lpwstr" || element_name == "i4" || element_name == "bool")
                {
                    in_value = true;
                }
            }
            Ok(Event::Text(e)) => {
                if in_value
                    && !current_property_name.is_empty()
                    && let Ok(text) = e.xml10_content()
                {
                    let tag_name = format!("OOXML:Custom:{}", current_property_name);
                    metadata.insert(tag_name, TagValue::new_string(text.to_string()));
                }
            }
            Ok(Event::End(e)) => {
                let element_name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if element_name == "property" {
                    in_property = false;
                    current_property_name.clear();
                } else if element_name == "lpwstr" || element_name == "i4" || element_name == "bool"
                {
                    in_value = false;
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

/// Format edit time from minutes to human-readable string
fn format_edit_time(minutes: u64) -> String {
    if minutes == 0 {
        return "0 minutes".to_string();
    }

    let hours = minutes / 60;
    let remaining_minutes = minutes % 60;

    match (hours, remaining_minutes) {
        (0, m) => format!("{} minute{}", m, if m == 1 { "" } else { "s" }),
        (h, 0) => format!("{} hour{}", h, if h == 1 { "" } else { "s" }),
        (h, m) => format!(
            "{} hour{} {} minute{}",
            h,
            if h == 1 { "" } else { "s" },
            m,
            if m == 1 { "" } else { "s" }
        ),
    }
}

/// Parse [Content_Types].xml to detect content types and embedded objects
fn parse_content_types(xml: &str, metadata: &mut MetadataMap) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut content_types = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                let element_name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();

                if element_name == "Override" || element_name == "Default" {
                    for attr in e.attributes().flatten() {
                        let key_bytes = attr.key.local_name();
                        let key = String::from_utf8_lossy(key_bytes.as_ref());
                        if key == "ContentType" {
                            let content_type = String::from_utf8_lossy(&attr.value).to_string();
                            // Track interesting content types (embedded objects, images, etc.)
                            if content_type.contains("image/")
                                || content_type.contains("ole")
                                || content_type.contains("drawing")
                                || content_type.contains("chart")
                            {
                                content_types.push(content_type);
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    if !content_types.is_empty() {
        let unique_types: std::collections::HashSet<_> = content_types.into_iter().collect();
        let types_list = unique_types.into_iter().collect::<Vec<_>>().join(", ");
        metadata.insert(
            "OOXML:EmbeddedContentTypes".to_string(),
            TagValue::new_string(types_list),
        );
    }

    Ok(())
}

/// Parse DOCX-specific properties (styles count, comments presence)
fn parse_docx_specific<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    metadata: &mut MetadataMap,
) -> Result<()> {
    // Check for comments
    if archive.by_name("word/comments.xml").is_ok() {
        metadata.insert(
            "OOXML:HasComments".to_string(),
            TagValue::new_string("true".to_string()),
        );

        // Try to count comments
        if let Ok(mut comments_file) = archive.by_name("word/comments.xml") {
            let mut xml_content = String::new();
            if comments_file.read_to_string(&mut xml_content).is_ok() {
                let comment_count = count_xml_elements(&xml_content, "comment");
                if comment_count > 0 {
                    metadata.insert(
                        "OOXML:CommentsCount".to_string(),
                        TagValue::new_string(comment_count.to_string()),
                    );
                }
            }
        }
    }

    // Check for styles
    if let Ok(mut styles_file) = archive.by_name("word/styles.xml") {
        let mut xml_content = String::new();
        if styles_file.read_to_string(&mut xml_content).is_ok() {
            let styles_count = count_xml_elements(&xml_content, "style");
            if styles_count > 0 {
                metadata.insert(
                    "OOXML:StylesCount".to_string(),
                    TagValue::new_string(styles_count.to_string()),
                );
            }
        }
    }

    Ok(())
}

/// Parse XLSX-specific properties (sheet names and count)
fn parse_xlsx_specific<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    metadata: &mut MetadataMap,
) -> Result<()> {
    if let Ok(mut workbook_file) = archive.by_name("xl/workbook.xml") {
        let mut xml_content = String::new();
        if workbook_file.read_to_string(&mut xml_content).is_ok() {
            let mut reader = Reader::from_str(&xml_content);
            reader.config_mut().trim_text(true);

            let mut buf = Vec::new();
            let mut sheet_names = Vec::new();

            loop {
                match reader.read_event_into(&mut buf) {
                    Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                        let element_name =
                            String::from_utf8_lossy(e.local_name().as_ref()).to_string();

                        if element_name == "sheet" {
                            for attr in e.attributes().flatten() {
                                let key_bytes = attr.key.local_name();
                                let key = String::from_utf8_lossy(key_bytes.as_ref());
                                if key == "name" {
                                    let name = String::from_utf8_lossy(&attr.value).to_string();
                                    sheet_names.push(name);
                                }
                            }
                        }
                    }
                    Ok(Event::Eof) => break,
                    Err(_) => break,
                    _ => {}
                }
                buf.clear();
            }

            if !sheet_names.is_empty() {
                metadata.insert(
                    "OOXML:SheetCount".to_string(),
                    TagValue::new_string(sheet_names.len().to_string()),
                );
                metadata.insert(
                    "OOXML:SheetNames".to_string(),
                    TagValue::new_string(sheet_names.join(", ")),
                );
            }
        }
    }

    Ok(())
}

/// Helper function to count occurrences of XML elements
fn count_xml_elements(xml: &str, element_name: &str) -> usize {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut count = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name_bytes = e.local_name();
                let name = String::from_utf8_lossy(name_bytes.as_ref());
                if name == element_name {
                    count += 1;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    count
}

/// Standalone function to parse DOCX metadata
///
/// This function provides a convenient way to parse DOCX metadata without
/// directly instantiating the DocxParser struct.
pub fn parse_docx_metadata(
    reader: &dyn crate::core::FileReader,
) -> std::result::Result<MetadataMap, String> {
    let parser = DocxParser;
    parser
        .parse(reader)
        .map_err(|e| format!("DOCX parse error: {}", e))
}

/// Standalone function to parse XLSX metadata
///
/// This function provides a convenient way to parse XLSX metadata without
/// directly instantiating the XlsxParser struct.
pub fn parse_xlsx_metadata(
    reader: &dyn crate::core::FileReader,
) -> std::result::Result<MetadataMap, String> {
    let parser = XlsxParser;
    parser
        .parse(reader)
        .map_err(|e| format!("XLSX parse error: {}", e))
}

/// Standalone function to parse PPTX metadata
///
/// This function provides a convenient way to parse PPTX metadata without
/// directly instantiating the PptxParser struct.
pub fn parse_pptx_metadata(
    reader: &dyn crate::core::FileReader,
) -> std::result::Result<MetadataMap, String> {
    let parser = PptxParser;
    parser
        .parse(reader)
        .map_err(|e| format!("PPTX parse error: {}", e))
}

/// Adds DOCX-specific tag aliases to metadata (Worker 20 requirements)
///
/// Maps OOXML generic tags to DOCX-specific tags for ExifTool compatibility
fn add_docx_tag_aliases(metadata: &mut MetadataMap) {
    // Worker 20 requires: DOCX:Title, DOCX:Subject, DOCX:Creator, DOCX:Keywords,
    // DOCX:Description, DOCX:Modified, DOCX:WordCount, DOCX:PageCount

    // Create a list of mappings from OOXML tags to DOCX tags
    let mappings = [
        ("OOXML:Title", "DOCX:Title"),
        ("OOXML:Subject", "DOCX:Subject"),
        ("OOXML:Creator", "DOCX:Creator"),
        ("OOXML:Keywords", "DOCX:Keywords"),
        ("OOXML:Description", "DOCX:Description"),
        ("OOXML:ModifyDate", "DOCX:Modified"),
        ("OOXML:Words", "DOCX:WordCount"),
        ("OOXML:Pages", "DOCX:PageCount"),
    ];

    // Clone existing tags and create aliases with DOCX prefix
    let mut docx_tags = Vec::new();
    for (ooxml_tag, docx_tag) in &mappings {
        if let Some(value) = metadata.get(ooxml_tag) {
            docx_tags.push((docx_tag.to_string(), value.clone()));
        }
    }

    // Insert the DOCX aliases
    for (key, value) in docx_tags {
        metadata.insert(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_core_properties() {
        let xml = r#"<?xml version="1.0"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties">
    <dc:title>Test Document</dc:title>
    <dc:creator>John Doe</dc:creator>
    <dc:subject>Testing</dc:subject>
</cp:coreProperties>"#;

        let mut metadata = MetadataMap::new();
        let result = parse_core_properties(xml, &mut metadata);
        assert!(result.is_ok());
        assert!(metadata.contains_key("OOXML:Title"));
    }

    #[test]
    fn test_parse_core_properties_forensic() {
        let xml = r#"<?xml version="1.0"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"
                   xmlns:dc="http://purl.org/dc/elements/1.1/"
                   xmlns:dcterms="http://purl.org/dc/terms/">
    <dc:title>Forensic Test</dc:title>
    <dc:creator>John Doe</dc:creator>
    <cp:lastModifiedBy>Jane Smith</cp:lastModifiedBy>
    <cp:revision>42</cp:revision>
    <dcterms:created>2024-01-15T10:30:00Z</dcterms:created>
    <dcterms:modified>2024-01-20T15:45:00Z</dcterms:modified>
    <cp:lastPrinted>2024-01-18T09:00:00Z</cp:lastPrinted>
    <cp:category>Confidential</cp:category>
    <cp:contentStatus>Draft</cp:contentStatus>
</cp:coreProperties>"#;

        let mut metadata = MetadataMap::new();
        let result = parse_core_properties(xml, &mut metadata);
        assert!(result.is_ok());

        assert_eq!(
            metadata.get("OOXML:LastModifiedBy").unwrap().as_string(),
            Some("Jane Smith")
        );
        assert_eq!(
            metadata.get("OOXML:RevisionNumber").unwrap().as_string(),
            Some("42")
        );
        assert_eq!(
            metadata.get("OOXML:Category").unwrap().as_string(),
            Some("Confidential")
        );
        assert_eq!(
            metadata.get("OOXML:ContentStatus").unwrap().as_string(),
            Some("Draft")
        );
    }

    #[test]
    fn test_parse_app_properties() {
        let xml = r#"<?xml version="1.0"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">
    <Application>Microsoft Word</Application>
    <Pages>10</Pages>
</Properties>"#;

        let mut metadata = MetadataMap::new();
        let result = parse_app_properties(xml, &mut metadata);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_app_properties_forensic() {
        let xml = r#"<?xml version="1.0"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">
    <Application>Microsoft Office Word</Application>
    <AppVersion>16.0000</AppVersion>
    <Company>Acme Corp</Company>
    <Manager>Bob Johnson</Manager>
    <Template>Normal.dotm</Template>
    <TotalTime>45</TotalTime>
    <HyperlinkBase>http://example.com</HyperlinkBase>
    <DocSecurity>0</DocSecurity>
</Properties>"#;

        let mut metadata = MetadataMap::new();
        let result = parse_app_properties(xml, &mut metadata);
        assert!(result.is_ok());

        assert_eq!(
            metadata.get("OOXML:Application").unwrap().as_string(),
            Some("Microsoft Office Word")
        );
        assert_eq!(
            metadata.get("OOXML:AppVersion").unwrap().as_string(),
            Some("16.0000")
        );
        assert_eq!(
            metadata.get("OOXML:Company").unwrap().as_string(),
            Some("Acme Corp")
        );
        assert_eq!(
            metadata.get("OOXML:Manager").unwrap().as_string(),
            Some("Bob Johnson")
        );
        assert_eq!(
            metadata.get("OOXML:Template").unwrap().as_string(),
            Some("Normal.dotm")
        );
        assert_eq!(
            metadata.get("OOXML:TotalEditTime").unwrap().as_string(),
            Some("45 minutes")
        );
        assert_eq!(
            metadata.get("OOXML:HyperlinkBase").unwrap().as_string(),
            Some("http://example.com")
        );
        assert_eq!(
            metadata.get("OOXML:DocSecurity").unwrap().as_string(),
            Some("0")
        );
    }

    #[test]
    fn test_parse_app_properties_powerpoint() {
        let xml = r#"<?xml version="1.0"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">
    <Application>Microsoft Office PowerPoint</Application>
    <HiddenSlides>3</HiddenSlides>
    <PresentationFormat>On-screen Show (4:3)</PresentationFormat>
</Properties>"#;

        let mut metadata = MetadataMap::new();
        let result = parse_app_properties(xml, &mut metadata);
        assert!(result.is_ok());

        assert_eq!(
            metadata.get("OOXML:HiddenSlides").unwrap().as_string(),
            Some("3")
        );
        assert_eq!(
            metadata
                .get("OOXML:PresentationFormat")
                .unwrap()
                .as_string(),
            Some("On-screen Show (4:3)")
        );
    }

    #[test]
    fn test_parse_custom_properties() {
        let xml = r#"<?xml version="1.0"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties">
    <property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="2" name="ProjectID">
        <vt:lpwstr>PROJ-12345</vt:lpwstr>
    </property>
    <property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="3" name="Classification">
        <vt:lpwstr>Internal Use Only</vt:lpwstr>
    </property>
    <property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="4" name="ReviewCount">
        <vt:i4>5</vt:i4>
    </property>
</Properties>"#;

        let mut metadata = MetadataMap::new();
        let result = parse_custom_properties(xml, &mut metadata);
        assert!(result.is_ok());

        assert_eq!(
            metadata.get("OOXML:Custom:ProjectID").unwrap().as_string(),
            Some("PROJ-12345")
        );
        assert_eq!(
            metadata
                .get("OOXML:Custom:Classification")
                .unwrap()
                .as_string(),
            Some("Internal Use Only")
        );
        assert_eq!(
            metadata
                .get("OOXML:Custom:ReviewCount")
                .unwrap()
                .as_string(),
            Some("5")
        );
    }

    #[test]
    fn test_format_edit_time() {
        assert_eq!(format_edit_time(0), "0 minutes");
        assert_eq!(format_edit_time(1), "1 minute");
        assert_eq!(format_edit_time(5), "5 minutes");
        assert_eq!(format_edit_time(45), "45 minutes");
        assert_eq!(format_edit_time(60), "1 hour");
        assert_eq!(format_edit_time(90), "1 hour 30 minutes");
        assert_eq!(format_edit_time(120), "2 hours");
        assert_eq!(format_edit_time(150), "2 hours 30 minutes");
        assert_eq!(format_edit_time(301), "5 hours 1 minute");
    }

    #[test]
    fn test_parse_core_properties_with_keywords() {
        let xml = r#"<?xml version="1.0"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"
                   xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Document</dc:title>
    <dc:creator>John Doe</dc:creator>
    <dc:keywords>forensics, metadata, testing</dc:keywords>
    <dc:subject>Testing Keywords</dc:subject>
</cp:coreProperties>"#;

        let mut metadata = MetadataMap::new();
        let result = parse_core_properties(xml, &mut metadata);
        assert!(result.is_ok());
        assert_eq!(
            metadata.get("OOXML:Keywords").unwrap().as_string(),
            Some("forensics, metadata, testing")
        );
    }

    #[test]
    fn test_parse_app_properties_extended() {
        let xml = r#"<?xml version="1.0"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">
    <Application>Microsoft Office Word</Application>
    <Words>1500</Words>
    <Characters>8500</Characters>
    <CharactersWithSpaces>10000</CharactersWithSpaces>
    <Lines>75</Lines>
    <Paragraphs>50</Paragraphs>
    <ScaleCrop>false</ScaleCrop>
    <LinksUpToDate>true</LinksUpToDate>
    <SharedDoc>false</SharedDoc>
    <HyperlinksChanged>true</HyperlinksChanged>
</Properties>"#;

        let mut metadata = MetadataMap::new();
        let result = parse_app_properties(xml, &mut metadata);
        assert!(result.is_ok());

        assert_eq!(
            metadata.get("OOXML:Words").unwrap().as_string(),
            Some("1500")
        );
        assert_eq!(
            metadata.get("OOXML:Characters").unwrap().as_string(),
            Some("8500")
        );
        assert_eq!(
            metadata
                .get("OOXML:CharactersWithSpaces")
                .unwrap()
                .as_string(),
            Some("10000")
        );
        assert_eq!(metadata.get("OOXML:Lines").unwrap().as_string(), Some("75"));
        assert_eq!(
            metadata.get("OOXML:Paragraphs").unwrap().as_string(),
            Some("50")
        );
        assert_eq!(
            metadata.get("OOXML:ScaleCrop").unwrap().as_string(),
            Some("false")
        );
        assert_eq!(
            metadata.get("OOXML:LinksUpToDate").unwrap().as_string(),
            Some("true")
        );
        assert_eq!(
            metadata.get("OOXML:SharedDoc").unwrap().as_string(),
            Some("false")
        );
        assert_eq!(
            metadata.get("OOXML:HyperlinksChanged").unwrap().as_string(),
            Some("true")
        );
    }

    #[test]
    fn test_parse_content_types() {
        let xml = r#"<?xml version="1.0"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
    <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
    <Default Extension="xml" ContentType="application/xml"/>
    <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
    <Override PartName="/word/media/image1.png" ContentType="image/png"/>
    <Override PartName="/word/media/image2.jpeg" ContentType="image/jpeg"/>
    <Override PartName="/word/charts/chart1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/>
</Types>"#;

        let mut metadata = MetadataMap::new();
        let result = parse_content_types(xml, &mut metadata);
        assert!(result.is_ok());

        let embedded_types = metadata
            .get("OOXML:EmbeddedContentTypes")
            .unwrap()
            .as_string()
            .unwrap();
        assert!(embedded_types.contains("image/png"));
        assert!(embedded_types.contains("image/jpeg"));
        assert!(embedded_types.contains("chart"));
    }

    #[test]
    fn test_count_xml_elements() {
        let xml = r#"<?xml version="1.0"?>
<root>
    <item>One</item>
    <item>Two</item>
    <other>Other</other>
    <item>Three</item>
</root>"#;

        assert_eq!(count_xml_elements(xml, "item"), 3);
        assert_eq!(count_xml_elements(xml, "other"), 1);
        assert_eq!(count_xml_elements(xml, "nonexistent"), 0);
    }
}
