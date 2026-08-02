//! Office Open XML (DOCX, XLSX, PPTX) format parsers

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};
use std::io::{Cursor, Read};
use zip::ZipArchive;

/// Extract the general-purpose bit flag from the first ZIP local file header.
///
/// ExifTool exposes this header field as `ZIP:ZipBitFlag`.
fn extract_zip_bit_flag(data: &[u8]) -> Option<u16> {
    let header_offset = data.windows(4).position(|window| window == b"PK\x03\x04")?;
    let flag_bytes = data.get(header_offset + 6..header_offset + 8)?;

    Some(u16::from_le_bytes([flag_bytes[0], flag_bytes[1]]))
}

/// Render the bit flag the way ExifTool's ZIP.pm does.
///
/// `PrintConv => '$val ? sprintf("0x%.4x",$val) : $val'`: a set flag word is
/// 4-digit hex, a zero one is a plain `0`. Most OOXML writers set bit 11
/// (UTF-8 names), so this is the usual case rather than an edge one.
fn zip_bit_flag_value(bit_flag: u16) -> TagValue {
    if bit_flag == 0 {
        TagValue::new_integer(0)
    } else {
        TagValue::new_string(format!("0x{:04x}", bit_flag))
    }
}

/// Extract the CRC-32 from the first ZIP local file header.
///
/// ExifTool exposes this header field as `ZIP:ZipCRC`.
fn extract_zip_crc(data: &[u8]) -> Option<u32> {
    let header_offset = data.windows(4).position(|window| window == b"PK\x03\x04")?;
    let crc_bytes = data.get(header_offset + 14..header_offset + 18)?;

    Some(u32::from_le_bytes([
        crc_bytes[0],
        crc_bytes[1],
        crc_bytes[2],
        crc_bytes[3],
    ]))
}

/// Extract the compressed size from the first ZIP local file header.
///
/// ExifTool exposes this header field as `ZIP:ZipCompressedSize`.
fn extract_zip_compressed_size(data: &[u8]) -> Option<u32> {
    let header_offset = data.windows(4).position(|window| window == b"PK\x03\x04")?;
    let size_bytes = data.get(header_offset + 18..header_offset + 22)?;

    Some(u32::from_le_bytes([
        size_bytes[0],
        size_bytes[1],
        size_bytes[2],
        size_bytes[3],
    ]))
}

/// Extract ExifTool ZIP tags from the first ZIP local file header.
fn extract_zip_local_header_tags(data: &[u8]) -> Option<(String, String, String, u16, u32)> {
    let header_offset = data.windows(4).position(|window| window == b"PK\x03\x04")?;
    let header = data.get(header_offset..)?;

    let required_version = u16::from_le_bytes(header.get(4..6)?.try_into().ok()?);
    let compression_method = u16::from_le_bytes(header.get(8..10)?.try_into().ok()?);
    let dos_time = u16::from_le_bytes(header.get(10..12)?.try_into().ok()?);
    let dos_date = u16::from_le_bytes(header.get(12..14)?.try_into().ok()?);
    let uncompressed_size = u32::from_le_bytes(header.get(22..26)?.try_into().ok()?);
    let filename_length = usize::from(u16::from_le_bytes(header.get(26..28)?.try_into().ok()?));
    let filename_end = 30usize.checked_add(filename_length)?;
    let filename = header.get(30..filename_end)?;

    let compression = match compression_method {
        0 => "Stored",
        8 => "Deflated",
        _ => "Unknown",
    };

    let year = 1980 + i32::from(dos_date >> 9);
    let month = (dos_date >> 5) & 0x0f;
    let day = dos_date & 0x1f;
    let hour = dos_time >> 11;
    let minute = (dos_time >> 5) & 0x3f;
    let second = (dos_time & 0x1f) * 2;
    if month > 12 || day > 31 || hour > 23 || minute > 59 || second > 59 {
        return None;
    }

    // ZIP permits zero month/day values; ExifTool renders these as 1.
    let month = month.max(1);
    let day = day.max(1);
    let modify_date = format!("{year:04}:{month:02}:{day:02} {hour:02}:{minute:02}:{second:02}");

    Some((
        compression.to_string(),
        String::from_utf8_lossy(filename).into_owned(),
        modify_date,
        required_version,
        uncompressed_size,
    ))
}

/// DOCX parser
pub struct DocxParser;

impl FormatParser for DocxParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();

        // Read as ZIP
        let size = reader.size() as usize;
        let file_data = reader.read(0, size)?;
        let zip_bit_flag = extract_zip_bit_flag(file_data);
        let zip_crc = extract_zip_crc(file_data);
        let zip_compressed_size = extract_zip_compressed_size(file_data);
        let zip_local_header_tags = extract_zip_local_header_tags(file_data);
        let cursor = Cursor::new(file_data);
        let mut archive = ZipArchive::new(cursor)
            .map_err(|e| ExifToolError::parse_error(format!("Not a valid DOCX: {}", e)))?;

        if let Some((compression, filename, modify_date, required_version, uncompressed_size)) =
            zip_local_header_tags
        {
            metadata.insert(
                "ZIP:ZipCompression".to_string(),
                TagValue::new_string(compression),
            );
            metadata.insert(
                "ZIP:ZipFileName".to_string(),
                TagValue::new_string(filename),
            );
            metadata.insert(
                "ZIP:ZipModifyDate".to_string(),
                TagValue::new_string(modify_date),
            );
            metadata.insert(
                "ZIP:ZipRequiredVersion".to_string(),
                TagValue::new_integer(i64::from(required_version)),
            );
            metadata.insert(
                "ZIP:ZipUncompressedSize".to_string(),
                TagValue::new_integer(i64::from(uncompressed_size)),
            );
        }

        if let Some(crc) = zip_crc {
            metadata.insert(
                "ZIP:ZipCRC".to_string(),
                TagValue::new_string(format!("0x{crc:08x}")),
            );
        }

        // Check for DOCX-specific files
        let has_content_types = archive.by_name("[Content_Types].xml").is_ok();
        let has_word_doc = archive.by_name("word/document.xml").is_ok();

        if !has_content_types || !has_word_doc {
            return Err(ExifToolError::parse_error("Not a valid DOCX file"));
        }

        if let Some(bit_flag) = zip_bit_flag {
            metadata.insert("ZIP:ZipBitFlag".to_string(), zip_bit_flag_value(bit_flag));
        }

        if let Some(compressed_size) = zip_compressed_size {
            metadata.insert(
                "ZIP:ZipCompressedSize".to_string(),
                TagValue::new_integer(i64::from(compressed_size)),
            );
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
            parse_docx_xml_custom_properties(&xml_content, &mut metadata)?;
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
        add_docx_xml_tags(&mut metadata);

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::DOCX)
    }
}

#[cfg(test)]
mod zip_bit_flag_tests {
    use super::{
        extract_zip_bit_flag, extract_zip_compressed_size, extract_zip_crc,
        extract_zip_local_header_tags,
    };

    #[test]
    fn extracts_zip_bit_flag_from_local_header() {
        let unflagged = b"PK\x03\x04\x14\x00\x00\x00\x08\x00";
        assert_eq!(extract_zip_bit_flag(unflagged), Some(0));

        let flagged = b"prefixPK\x03\x04\x14\x00\x08\x08\x08\x00";
        assert_eq!(extract_zip_bit_flag(flagged), Some(0x0808));
    }

    #[test]
    fn extracts_zip_crc_from_local_header() {
        let header = b"PK\x03\x04\x14\x00\x00\x00\x08\x00\x00\x00\x00\x00\x31\x44\x5b\x81";

        assert_eq!(extract_zip_crc(header), Some(0x815b4431));
    }

    #[test]
    fn rejects_truncated_crc() {
        assert_eq!(extract_zip_crc(b"PK\x03\x04\x14\x00\x00\x00\x08\x00"), None);
    }

    #[test]
    fn extracts_zip_local_header_tags() {
        let header = b"PK\x03\x04\x14\x00\x00\x00\x08\x00\x00\x00\x00\x00\x00\x00\x00\x00\
                       \x00\x00\x00\x00\xd1\x05\x00\x00\x13\x00\x00\x00[Content_Types].xml";

        assert_eq!(
            extract_zip_local_header_tags(header),
            Some((
                "Deflated".to_string(),
                "[Content_Types].xml".to_string(),
                "1980:01:01 00:00:00".to_string(),
                20,
                1489,
            ))
        );
    }

    #[test]
    fn extracts_zip_compressed_size_from_local_header() {
        let header =
            b"PK\x03\x04\x14\x00\x00\x00\x08\x00\x00\x00\x00\x00\x00\x00\x00\x00\x6a\x01\x00\x00";

        assert_eq!(extract_zip_compressed_size(header), Some(362));
    }

    #[test]
    fn rejects_truncated_compressed_size() {
        assert_eq!(
            extract_zip_compressed_size(b"PK\x03\x04\x14\x00\x00\x00\x08\x00"),
            None
        );
    }

    #[test]
    fn rejects_truncated_or_missing_local_header() {
        assert_eq!(extract_zip_bit_flag(b"not a zip file"), None);
        assert_eq!(extract_zip_bit_flag(b"PK\x03\x04\x14\x00"), None);
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
    let mut in_heading_pairs = false;
    let mut heading_pairs = Vec::new();
    let mut in_titles_of_parts = false;
    let mut titles_of_parts = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                current_element = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if current_element == "HeadingPairs" {
                    in_heading_pairs = true;
                    heading_pairs.clear();
                } else if current_element == "TitlesOfParts" {
                    in_titles_of_parts = true;
                    titles_of_parts.clear();
                }
            }
            Ok(Event::Text(e)) => {
                if let Ok(text) = e.xml10_content()
                    && !text.is_empty()
                {
                    if in_heading_pairs {
                        let value = match current_element.as_str() {
                            "i1" | "i2" | "i4" | "i8" | "int" | "ui1" | "ui2" | "ui4" | "ui8"
                            | "uint" => text
                                .parse::<i64>()
                                .map(TagValue::new_integer)
                                .unwrap_or_else(|_| TagValue::new_string(text.to_string())),
                            "lpstr" | "lpwstr" | "bstr" => TagValue::new_string(text.to_string()),
                            _ => {
                                buf.clear();
                                continue;
                            }
                        };
                        heading_pairs.push(value);
                        buf.clear();
                        continue;
                    }

                    if in_titles_of_parts {
                        if matches!(current_element.as_str(), "lpstr" | "lpwstr" | "bstr") {
                            titles_of_parts.push(TagValue::new_string(text.to_string()));
                        }
                        buf.clear();
                        continue;
                    }

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
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"HeadingPairs" {
                    in_heading_pairs = false;
                    if !heading_pairs.is_empty() {
                        metadata.insert(
                            "XML:HeadingPairs".to_string(),
                            TagValue::new_array(std::mem::take(&mut heading_pairs)),
                        );
                    }
                } else if e.local_name().as_ref() == b"TitlesOfParts" {
                    in_titles_of_parts = false;
                    if titles_of_parts.len() == 1 {
                        metadata.insert(
                            "XML:TitlesOfParts".to_string(),
                            titles_of_parts.pop().expect("length checked"),
                        );
                    } else if !titles_of_parts.is_empty() {
                        metadata.insert(
                            "XML:TitlesOfParts".to_string(),
                            TagValue::new_array(std::mem::take(&mut titles_of_parts)),
                        );
                    }
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

/// Extract DOCX custom properties under the normalized XML-group names used by
/// ExifTool. Office permits arbitrary user-defined property names here.
fn parse_docx_xml_custom_properties(xml: &str, metadata: &mut MetadataMap) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut current_tag: Option<String> = None;
    let mut current_value_is_filetime = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if e.local_name().as_ref() == b"property" {
                    current_tag = None;
                    current_value_is_filetime = false;

                    for attribute in e.attributes() {
                        let attribute = attribute.map_err(|e| {
                            ExifToolError::parse_error(format!(
                                "Invalid custom property attribute: {}",
                                e
                            ))
                        })?;

                        if attribute.key.as_ref() == b"name" {
                            let name = attribute
                                .decoded_and_normalized_value(
                                    XmlVersion::Implicit1_0,
                                    reader.decoder(),
                                )
                                .map_err(|e| {
                                    ExifToolError::parse_error(format!(
                                        "Invalid custom property name: {}",
                                        e
                                    ))
                                })?;

                            let normalized = normalize_xml_property_name(name.as_ref());
                            if !normalized.is_empty() {
                                current_tag = Some(format!("XML:{normalized}"));
                            }
                        }
                    }
                } else if current_tag.is_some() && e.local_name().as_ref() == b"filetime" {
                    current_value_is_filetime = true;
                }
            }
            Ok(Event::Text(e)) => {
                if let Some(tag_name) = current_tag.as_ref() {
                    let text = e.xml10_content().map_err(|e| {
                        ExifToolError::parse_error(format!("Invalid custom property value: {}", e))
                    })?;

                    if !text.is_empty() {
                        let value = if current_value_is_filetime {
                            format_xml_date_for_exiftool(text.as_ref())
                        } else {
                            text.into_owned()
                        };
                        metadata.insert(tag_name.clone(), TagValue::new_string(value));
                    }
                }
            }
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"property" {
                    current_tag = None;
                    current_value_is_filetime = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(ExifToolError::parse_error(format!(
                    "XML custom properties parse error: {}",
                    e
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(())
}

fn normalize_xml_property_name(name: &str) -> String {
    let mut normalized = String::with_capacity(name.len());
    let mut capitalize_next = false;

    for character in name.chars() {
        if character.is_alphanumeric() {
            if capitalize_next {
                normalized.extend(character.to_uppercase());
                capitalize_next = false;
            } else {
                normalized.push(character);
            }
        } else if !normalized.is_empty() {
            capitalize_next = true;
        }
    }

    normalized
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
        ("OOXML:Title", "XMP:Title"),
        ("OOXML:Subject", "XMP:Subject"),
        ("OOXML:Creator", "XMP:Creator"),
        ("OOXML:Description", "XMP:Description"),
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

/// Add the standard XML-group names emitted by ExifTool for DOCX properties.
fn add_docx_xml_tags(metadata: &mut MetadataMap) {
    for (source, destination) in [
        ("OOXML:Application", "XML:Application"),
        ("OOXML:AppVersion", "XML:AppVersion"),
        ("OOXML:Category", "XML:Category"),
        ("OOXML:Characters", "XML:Characters"),
        ("OOXML:CharactersWithSpaces", "XML:CharactersWithSpaces"),
        ("OOXML:Company", "XML:Company"),
        ("OOXML:HyperlinkBase", "XML:HyperlinkBase"),
        ("OOXML:Keywords", "XML:Keywords"),
        ("OOXML:LastModifiedBy", "XML:LastModifiedBy"),
        ("OOXML:Lines", "XML:Lines"),
        ("OOXML:Manager", "XML:Manager"),
        ("OOXML:Pages", "XML:Pages"),
        ("OOXML:Paragraphs", "XML:Paragraphs"),
        ("OOXML:RevisionNumber", "XML:RevisionNumber"),
        ("OOXML:Template", "XML:Template"),
        ("OOXML:TotalEditTime", "XML:TotalEditTime"),
        ("OOXML:Words", "XML:Words"),
    ] {
        if let Some(value) = metadata.get(source).cloned() {
            metadata.insert(destination.to_string(), value);
        }
    }

    for (source, destination) in [
        ("OOXML:CreateDate", "XML:CreateDate"),
        ("OOXML:ModifyDate", "XML:ModifyDate"),
    ] {
        if let Some(date) = metadata.get(source).and_then(TagValue::as_string) {
            metadata.insert(
                destination.to_string(),
                TagValue::new_string(format_xml_date_for_exiftool(date)),
            );
        }
    }

    for (source, destination) in [
        ("OOXML:HyperlinksChanged", "XML:HyperlinksChanged"),
        ("OOXML:LinksUpToDate", "XML:LinksUpToDate"),
        ("OOXML:ScaleCrop", "XML:ScaleCrop"),
        ("OOXML:SharedDoc", "XML:SharedDoc"),
    ] {
        if let Some(value) = metadata.get(source).and_then(TagValue::as_string) {
            metadata.insert(
                destination.to_string(),
                TagValue::new_string(format_xml_yes_no(value)),
            );
        }
    }

    if let Some(value) = metadata
        .get("OOXML:DocSecurity")
        .and_then(TagValue::as_string)
    {
        metadata.insert(
            "XML:DocSecurity".to_string(),
            TagValue::new_string(if value == "0" { "None" } else { value }),
        );
    }
}

fn format_xml_yes_no(value: &str) -> &str {
    match value {
        "true" | "1" => "Yes",
        "false" | "0" => "No",
        value => value,
    }
}

/// Convert an ISO OOXML timestamp to ExifTool's default XML date rendering.
///
/// For example, `2009-10-24T01:48:00Z` becomes
/// `2009:10:24 01:48:00Z`.
fn format_xml_date_for_exiftool(value: &str) -> String {
    let Some((date, time)) = value.split_once('T') else {
        return value.to_string();
    };

    let mut date_parts = date.split('-');
    let (Some(year), Some(month), Some(day), None) = (
        date_parts.next(),
        date_parts.next(),
        date_parts.next(),
        date_parts.next(),
    ) else {
        return value.to_string();
    };

    format!("{}:{}:{} {}", year, month, day, time)
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

    #[test]
    fn test_docx_xml_standard_property_aliases() {
        let mut metadata = MetadataMap::new();
        for (name, value) in [
            ("Application", "Microsoft Macintosh Word"),
            ("AppVersion", "12.0000"),
            ("Category", "category goes here"),
            ("Characters", "42"),
            ("CharactersWithSpaces", "45"),
            ("Company", "Company - ExifTool"),
            ("DocSecurity", "0"),
            ("LastModifiedBy", "Jeff"),
            ("Lines", "7"),
            ("LinksUpToDate", "false"),
            ("Manager", "Manager: Self"),
            ("RevisionNumber", "3"),
            ("ScaleCrop", "false"),
            ("SharedDoc", "false"),
            ("Template", "Normal"),
            ("TotalEditTime", "7 minutes"),
            ("Words", "7"),
        ] {
            metadata.insert(
                format!("OOXML:{name}"),
                TagValue::new_string(value.to_string()),
            );
        }
        metadata.insert(
            "OOXML:CreateDate".to_string(),
            TagValue::new_string("2009-10-24T01:41:00Z".to_string()),
        );
        metadata.insert(
            "OOXML:ModifyDate".to_string(),
            TagValue::new_string("2009-10-24T01:48:00Z".to_string()),
        );
        metadata.insert(
            "OOXML:Pages".to_string(),
            TagValue::new_string("1".to_string()),
        );
        metadata.insert(
            "OOXML:Paragraphs".to_string(),
            TagValue::new_string("4".to_string()),
        );

        add_docx_xml_tags(&mut metadata);

        for (name, expected) in [
            ("Application", "Microsoft Macintosh Word"),
            ("AppVersion", "12.0000"),
            ("Category", "category goes here"),
            ("Characters", "42"),
            ("CharactersWithSpaces", "45"),
            ("Company", "Company - ExifTool"),
            ("CreateDate", "2009:10:24 01:41:00Z"),
            ("DocSecurity", "None"),
            ("LastModifiedBy", "Jeff"),
            ("Lines", "7"),
            ("LinksUpToDate", "No"),
            ("Manager", "Manager: Self"),
            ("RevisionNumber", "3"),
            ("ScaleCrop", "No"),
            ("SharedDoc", "No"),
            ("Template", "Normal"),
            ("TotalEditTime", "7 minutes"),
            ("Words", "7"),
        ] {
            assert_eq!(
                metadata
                    .get(&format!("XML:{name}"))
                    .and_then(TagValue::as_string),
                Some(expected),
                "XML:{name}"
            );
        }
        assert_eq!(
            metadata.get("XML:ModifyDate").and_then(TagValue::as_string),
            Some("2009:10:24 01:48:00Z")
        );
        assert_eq!(
            metadata.get("XML:Pages").and_then(TagValue::as_string),
            Some("1")
        );
        assert_eq!(
            metadata.get("XML:Paragraphs").and_then(TagValue::as_string),
            Some("4")
        );
    }

    #[test]
    fn test_docx_xml_selected_custom_properties() {
        let xml = r#"<?xml version="1.0"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties"
            xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">
    <property name="Matter"><vt:lpwstr>Matter</vt:lpwstr></property>
    <property name="Office"><vt:lpwstr>Office</vt:lpwstr></property>
    <property name="Owner"><vt:lpwstr>Owner</vt:lpwstr></property>
    <property name="Forward to"><vt:lpwstr>Forward to</vt:lpwstr></property>
    <property name="Group"><vt:lpwstr>Group</vt:lpwstr></property>
    <property name="A Custom Field"><vt:lpwstr>Custom value</vt:lpwstr></property>
    <property name="Date completed"><vt:filetime>2009-10-23T07:00:00Z</vt:filetime></property>
</Properties>"#;

        let mut metadata = MetadataMap::new();
        let result = parse_docx_xml_custom_properties(xml, &mut metadata);

        assert!(result.is_ok());
        assert_eq!(
            metadata.get("XML:Matter").and_then(TagValue::as_string),
            Some("Matter")
        );
        assert_eq!(
            metadata.get("XML:Office").and_then(TagValue::as_string),
            Some("Office")
        );
        assert_eq!(
            metadata.get("XML:Owner").and_then(TagValue::as_string),
            Some("Owner")
        );
        assert_eq!(
            metadata.get("XML:ForwardTo").and_then(TagValue::as_string),
            Some("Forward to")
        );
        assert_eq!(
            metadata.get("XML:Group").and_then(TagValue::as_string),
            Some("Group")
        );
        assert_eq!(
            metadata
                .get("XML:ACustomField")
                .and_then(TagValue::as_string),
            Some("Custom value")
        );
        assert_eq!(
            metadata
                .get("XML:DateCompleted")
                .and_then(TagValue::as_string),
            Some("2009:10:23 07:00:00Z")
        );
    }

    #[test]
    fn test_docx_xml_remaining_standard_properties() {
        let core_xml =
            r#"<cp:coreProperties><cp:keywords>one, two</cp:keywords></cp:coreProperties>"#;
        let app_xml = r#"
<Properties xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">
    <HeadingPairs>
        <vt:vector size="2" baseType="variant">
            <vt:variant><vt:lpstr>Title</vt:lpstr></vt:variant>
            <vt:variant><vt:i4>1</vt:i4></vt:variant>
        </vt:vector>
    </HeadingPairs>
    <TitlesOfParts>
        <vt:vector size="1" baseType="lpstr">
            <vt:lpstr>The document title</vt:lpstr>
        </vt:vector>
    </TitlesOfParts>
    <HyperlinkBase>https://example.test/</HyperlinkBase>
    <HyperlinksChanged>false</HyperlinksChanged>
</Properties>"#;
        let mut metadata = MetadataMap::new();

        parse_core_properties(core_xml, &mut metadata).unwrap();
        parse_app_properties(app_xml, &mut metadata).unwrap();
        add_docx_xml_tags(&mut metadata);

        assert_eq!(
            metadata.get("XML:Keywords").and_then(TagValue::as_string),
            Some("one, two")
        );
        assert_eq!(
            metadata
                .get("XML:HyperlinkBase")
                .and_then(TagValue::as_string),
            Some("https://example.test/")
        );
        assert_eq!(
            metadata
                .get("XML:HyperlinksChanged")
                .and_then(TagValue::as_string),
            Some("No")
        );
        assert_eq!(
            metadata.get("XML:HeadingPairs"),
            Some(&TagValue::new_array(vec![
                TagValue::new_string("Title"),
                TagValue::new_integer(1),
            ]))
        );
        assert_eq!(
            metadata
                .get("XML:TitlesOfParts")
                .and_then(TagValue::as_string),
            Some("The document title")
        );
    }

    #[test]
    fn test_docx_core_properties_have_xmp_aliases() {
        let mut metadata = MetadataMap::new();
        for (name, value) in [
            ("Title", "The document title"),
            ("Subject", "the subject"),
            ("Creator", "Author: Jeff"),
            ("Description", "here are my comments"),
        ] {
            metadata.insert(
                format!("OOXML:{name}"),
                TagValue::new_string(value.to_string()),
            );
        }

        add_docx_tag_aliases(&mut metadata);

        for (name, expected) in [
            ("Title", "The document title"),
            ("Subject", "the subject"),
            ("Creator", "Author: Jeff"),
            ("Description", "here are my comments"),
        ] {
            assert_eq!(
                metadata
                    .get(&format!("XMP:{name}"))
                    .and_then(TagValue::as_string),
                Some(expected),
                "XMP:{name}"
            );
        }
    }
}
