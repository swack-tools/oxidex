//! Archive format detection
//!
//! Handles detection of ZIP-based document formats including EPUB,
//! Office Open XML (DOCX, XLSX, PPTX), and iWork formats.

use crate::core::{FileFormat, FileReader};

const EPUB_MIMETYPE_MAX_SIZE: u64 = 1024;

/// Detect ZIP-based document formats
///
/// Many document formats use ZIP containers. This function examines
/// internal structure to distinguish between:
/// - EPUB, DOCX, XLSX, PPTX (Office Open XML)
/// - Pages, Numbers, Keynote (iWork)
/// - Generic ZIP
///
/// # Arguments
///
/// * `reader` - File reader for reading ZIP contents
///
/// # Returns
///
/// `FileFormat` variant for detected format
pub fn detect_zip_variant(reader: &dyn FileReader) -> FileFormat {
    use std::io::{Cursor, Read};
    use zip::ZipArchive;

    let size = reader.size() as usize;
    if let Ok(all_data) = reader.read(0, size)
        && let Ok(mut archive) = ZipArchive::new(Cursor::new(all_data))
    {
        // Check for specific marker files in priority order

        if let Ok(mimetype_file) = archive.by_name("mimetype") {
            let declared_size = mimetype_file.size();
            if declared_size <= EPUB_MIMETYPE_MAX_SIZE {
                let mut content = Vec::with_capacity(declared_size as usize);
                let read_result = mimetype_file
                    .take(EPUB_MIMETYPE_MAX_SIZE + 1)
                    .read_to_end(&mut content);
                if read_result.is_ok() && content.len() as u64 == declared_size {
                    let mimetype = String::from_utf8_lossy(&content);
                    let mimetype = mimetype.trim_matches(|character: char| {
                        character.is_ascii_whitespace() || character == '\u{feff}'
                    });
                    if mimetype == "application/epub+zip" {
                        return FileFormat::EPUB;
                    }
                }
            }
        }

        if archive.by_name("word/document.xml").is_ok() {
            return FileFormat::DOCX;
        }

        if archive.by_name("xl/workbook.xml").is_ok() {
            return FileFormat::XLSX;
        }

        if archive.by_name("ppt/presentation.xml").is_ok() {
            return FileFormat::PPTX;
        }

        if archive.by_name("Index/Presentation.iwa").is_ok() {
            return FileFormat::Keynote;
        }

        // Numbers and Pages both have Document.iwa, check for Tables
        if archive.by_name("Index/Document.iwa").is_ok() {
            if archive
                .file_names()
                .any(|name| name.starts_with("Index/Tables/"))
            {
                return FileFormat::Numbers;
            }
            return FileFormat::Pages;
        }
    }

    FileFormat::ZIP
}
