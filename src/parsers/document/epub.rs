//! EPUB e-book format parser

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::io::{Cursor, Read};
use zip::ZipArchive;

/// Parser for EPUB (Electronic Publication) e-book files
///
/// Extracts metadata from EPUB files including title, creator, publisher,
/// language, and other Dublin Core metadata elements from the OPF package file.
pub struct EpubParser;

impl FormatParser for EpubParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();

        // Read as ZIP
        let size = reader.size() as usize;
        let file_data = reader.read(0, size)?;
        let cursor = Cursor::new(file_data);
        let mut archive = ZipArchive::new(cursor)
            .map_err(|e| ExifToolError::parse_error(format!("Not a valid EPUB: {}", e)))?;

        // EPUB must contain mimetype file with "application/epub+zip"
        if let Ok(mut mimetype_file) = archive.by_name("mimetype") {
            let mut content = String::new();
            mimetype_file.read_to_string(&mut content).ok();
            if !content.starts_with("application/epub+zip") {
                return Err(ExifToolError::parse_error("Not a valid EPUB file"));
            }
        } else {
            return Err(ExifToolError::parse_error(
                "Not a valid EPUB file: missing mimetype",
            ));
        }

        // Find the OPF file location from container.xml
        let opf_path = if let Ok(mut container_file) = archive.by_name("META-INF/container.xml") {
            let mut content = String::new();
            container_file.read_to_string(&mut content).map_err(|e| {
                ExifToolError::parse_error(format!("Failed to read container.xml: {}", e))
            })?;

            extract_opf_path(&content)?
        } else {
            return Err(ExifToolError::parse_error(
                "Not a valid EPUB: missing META-INF/container.xml",
            ));
        };

        // Parse the OPF file for metadata
        if let Ok(mut opf_file) = archive.by_name(&opf_path) {
            let mut content = String::new();
            opf_file.read_to_string(&mut content).map_err(|e| {
                ExifToolError::parse_error(format!("Failed to read OPF file: {}", e))
            })?;

            parse_opf_metadata(&content, &mut metadata)?;
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::EPUB)
    }
}

/// Extract the path to the OPF file from container.xml
fn extract_opf_path(xml: &str) -> Result<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) if e.local_name().as_ref() == b"rootfile" => {
                for attr in e.attributes().flatten() {
                    if attr.key.local_name().as_ref() == b"full-path" {
                        let path = String::from_utf8_lossy(&attr.value).to_string();
                        return Ok(path);
                    }
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

    Err(ExifToolError::parse_error(
        "Could not find OPF path in container.xml",
    ))
}

/// Parse OPF metadata (Dublin Core and custom metadata)
fn parse_opf_metadata(xml: &str, metadata: &mut MetadataMap) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut current_element = String::new();
    let mut in_metadata = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if name == "metadata" {
                    in_metadata = true;
                } else if in_metadata {
                    current_element = name;
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if name == "metadata" {
                    in_metadata = false;
                }
            }
            Ok(Event::Text(e)) if in_metadata => {
                if let Ok(text) = e.xml10_content()
                    && !text.is_empty()
                    && !current_element.is_empty()
                {
                    let tag_name = match current_element.as_str() {
                        "title" => "EPUB:Title",
                        "creator" => "EPUB:Creator",
                        "subject" => "EPUB:Subject",
                        "description" => "EPUB:Description",
                        "publisher" => "EPUB:Publisher",
                        "date" => "EPUB:Date",
                        "language" => "EPUB:Language",
                        "identifier" => "EPUB:Identifier",
                        "rights" => "EPUB:Rights",
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

/// Standalone function to parse EPUB metadata
///
/// This function provides a convenient way to parse EPUB metadata without
/// directly instantiating the EpubParser struct.
pub fn parse_epub_metadata(
    reader: &dyn crate::core::FileReader,
) -> std::result::Result<MetadataMap, String> {
    let parser = EpubParser;
    parser
        .parse(reader)
        .map_err(|e| format!("EPUB parse error: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_opf_path() {
        let xml = r#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
    <rootfiles>
        <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
    </rootfiles>
</container>"#;

        let result = extract_opf_path(xml);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "OEBPS/content.opf");
    }

    #[test]
    fn test_parse_opf_metadata() {
        let xml = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf">
    <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
        <dc:title>Test Book</dc:title>
        <dc:creator>John Doe</dc:creator>
        <dc:language>en</dc:language>
        <dc:identifier>123456789</dc:identifier>
    </metadata>
</package>"#;

        let mut metadata = MetadataMap::new();
        let result = parse_opf_metadata(xml, &mut metadata);
        assert!(result.is_ok());
        assert!(metadata.contains_key("EPUB:Title"));
        assert!(metadata.contains_key("EPUB:Creator"));
    }
}
