//! Archive format detection
//!
//! Handles detection of ZIP-based document formats including EPUB,
//! Office Open XML (DOCX, XLSX, PPTX), and iWork formats.

use crate::core::{FileFormat, FileReader};

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
    use std::io::Cursor;
    use zip::ZipArchive;

    let size = reader.size() as usize;
    if let Ok(all_data) = reader.read(0, size)
        && let Ok(mut archive) = ZipArchive::new(Cursor::new(all_data))
    {
        // Check for specific marker files in priority order

        if archive.by_name("mimetype").is_ok() {
            return FileFormat::EPUB;
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

        // Numbers and Pages both have Document.iwa. Numbers additionally stores
        // spreadsheet data under the "Index/Tables/" prefix. Some archives do not
        // emit an explicit "Index/Tables/" directory entry, so scan entry names
        // for the prefix rather than relying on a bare directory entry.
        if archive.by_name("Index/Document.iwa").is_ok() {
            let has_tables = archive.by_name("Index/Tables/").is_ok()
                || (0..archive.len()).any(|i| {
                    archive
                        .by_index(i)
                        .map(|entry| entry.name().starts_with("Index/Tables/"))
                        .unwrap_or(false)
                });
            if has_tables {
                return FileFormat::Numbers;
            }
            return FileFormat::Pages;
        }
    }

    FileFormat::ZIP
}
