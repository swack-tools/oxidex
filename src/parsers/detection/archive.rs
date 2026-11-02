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
    use crate::parsers::archive::zip_reader::ZipReader;

    let size = reader.size() as usize;
    if let Ok(all_data) = reader.read(0, size)
        && let Ok(archive) = ZipReader::new(all_data)
    {
        // Check for specific marker files in priority order

        if let Some(mimetype_entry) = archive.by_name("mimetype") {
            // The declared size is attacker-controlled central directory data,
            // so it is screened before any read. ZipReader then enforces the
            // declared length itself, erroring on a stream that inflates to a
            // different size, which is what the explicit take()/compare here
            // used to do for the zip crate.
            if mimetype_entry.size() <= EPUB_MIMETYPE_MAX_SIZE
                && let Ok(content) = archive.read_entry(mimetype_entry)
            {
                let mimetype = String::from_utf8_lossy(&content);
                let mimetype = mimetype.trim_matches(|character: char| {
                    character.is_ascii_whitespace() || character == '\u{feff}'
                });
                if mimetype == "application/epub+zip" {
                    return FileFormat::EPUB;
                }
            }
        }

        // Detection only needs the entry to be listed, not readable: a member
        // compressed with a method this reader cannot inflate still identifies
        // the container's format.
        if archive.contains("word/document.xml") {
            return FileFormat::DOCX;
        }

        if archive.contains("xl/workbook.xml") {
            return FileFormat::XLSX;
        }

        if archive.contains("ppt/presentation.xml") {
            return FileFormat::PPTX;
        }

        if archive.contains("Index/Presentation.iwa") {
            return FileFormat::Keynote;
        }

        // Numbers and Pages both have Document.iwa, check for Tables
        if archive.contains("Index/Document.iwa") {
            if archive
                .entries()
                .iter()
                .any(|entry| entry.name().starts_with("Index/Tables/"))
            {
                return FileFormat::Numbers;
            }
            return FileFormat::Pages;
        }
    }

    FileFormat::ZIP
}
