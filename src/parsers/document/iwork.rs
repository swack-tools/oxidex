//! Apple iWork (Pages, Numbers, Keynote) format parsers

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use std::io::{Cursor, Read};
use zip::ZipArchive;

/// Pages parser
pub struct PagesParser;

impl FormatParser for PagesParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        parse_iwork(reader, "Pages", "Index/Document.iwa")
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::Pages)
    }
}

/// Numbers parser
pub struct NumbersParser;

impl FormatParser for NumbersParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        parse_iwork(reader, "Numbers", "Index/Document.iwa")
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::Numbers)
    }
}

/// Keynote parser
pub struct KeynoteParser;

impl FormatParser for KeynoteParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        parse_iwork(reader, "Keynote", "Index/Presentation.iwa")
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::Keynote)
    }
}

pub fn parse_pages_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = PagesParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

pub fn parse_numbers_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = NumbersParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

pub fn parse_keynote_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = KeynoteParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

/// Common iWork parsing logic
fn parse_iwork(
    reader: &dyn FileReader,
    app_name: &str,
    expected_file: &str,
) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Read as ZIP
    let size = reader.size() as usize;
    let file_data = reader.read(0, size)?;
    let cursor = Cursor::new(file_data);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| ExifToolError::parse_error(format!("Not a valid {} file: {}", app_name, e)))?;

    // Check for iWork-specific structure
    if archive.by_name(expected_file).is_err() {
        return Err(ExifToolError::parse_error(format!(
            "Not a valid {} file: missing {}",
            app_name, expected_file
        )));
    }

    // Add basic metadata
    metadata.insert(
        "iWork:Application".to_string(),
        TagValue::new_string(app_name.to_string()),
    );

    // Try to find metadata in Index/Metadata.plist if it exists
    if let Ok(mut metadata_file) = archive.by_name("Index/Metadata.plist") {
        let mut content = String::new();
        if metadata_file.read_to_string(&mut content).is_ok() {
            // Basic plist parsing - look for common keys
            extract_plist_metadata(&content, &mut metadata);
        }
    }

    // Extract build version if available
    if let Ok(mut buildversion_file) = archive.by_name("buildVersionHistory.plist") {
        let mut content = String::new();
        if buildversion_file.read_to_string(&mut content).is_ok() {
            extract_build_version(&content, &mut metadata);
        }
    }

    Ok(metadata)
}

/// Extract metadata from plist content (simple text-based extraction)
fn extract_plist_metadata(content: &str, metadata: &mut MetadataMap) {
    // Look for author information
    if let Some(author_start) = content.find("<key>Author</key>")
        && let Some(string_start) = content[author_start..].find("<string>")
    {
        let value_start = author_start + string_start + 8;
        if let Some(string_end) = content[value_start..].find("</string>") {
            let author = &content[value_start..value_start + string_end];
            metadata.insert(
                "iWork:Author".to_string(),
                TagValue::new_string(author.to_string()),
            );
        }
    }

    // Look for title
    if let Some(title_start) = content.find("<key>Title</key>")
        && let Some(string_start) = content[title_start..].find("<string>")
    {
        let value_start = title_start + string_start + 8;
        if let Some(string_end) = content[value_start..].find("</string>") {
            let title = &content[value_start..value_start + string_end];
            metadata.insert(
                "iWork:Title".to_string(),
                TagValue::new_string(title.to_string()),
            );
        }
    }
}

/// Extract build version from buildVersionHistory.plist
fn extract_build_version(content: &str, metadata: &mut MetadataMap) {
    if let Some(version_start) = content.find("<key>BuildVersion</key>")
        && let Some(string_start) = content[version_start..].find("<string>")
    {
        let value_start = version_start + string_start + 8;
        if let Some(string_end) = content[value_start..].find("</string>") {
            let version = &content[value_start..value_start + string_end];
            metadata.insert(
                "iWork:BuildVersion".to_string(),
                TagValue::new_string(version.to_string()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_plist_metadata() {
        let plist = r#"<?xml version="1.0"?>
<plist>
    <dict>
        <key>Author</key>
        <string>John Doe</string>
        <key>Title</key>
        <string>Test Document</string>
    </dict>
</plist>"#;

        let mut metadata = MetadataMap::new();
        extract_plist_metadata(plist, &mut metadata);

        assert!(metadata.contains_key("iWork:Author"));
        assert!(metadata.contains_key("iWork:Title"));
    }

    #[test]
    fn test_extract_build_version() {
        let plist = r#"<?xml version="1.0"?>
<plist>
    <dict>
        <key>BuildVersion</key>
        <string>7029</string>
    </dict>
</plist>"#;

        let mut metadata = MetadataMap::new();
        extract_build_version(plist, &mut metadata);

        assert!(metadata.contains_key("iWork:BuildVersion"));
    }
}
