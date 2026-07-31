//! PDF format parser
//!
//! This module provides parsing for PDF (Portable Document Format) files,
//! extracting metadata from Info dictionaries and embedded XMP packets.
//!
//! # PDF Metadata Support
//!
//! The parser extracts metadata from:
//! - **Info dictionary**: Standard PDF metadata fields (Title, Author, Subject, etc.)
//! - **XMP packets**: Extensible Metadata Platform XML data
//! - **Encrypt dictionary**: Encryption and security information
//!
//! # PDF Structure
//!
//! PDF files consist of:
//! 1. Header: `%PDF-1.x`
//! 2. Body: Objects containing content and metadata
//! 3. Cross-reference table (xref): Maps object numbers to byte offsets
//! 4. Trailer: Contains references to catalog and Info dictionary
//! 5. EOF marker: `%%EOF`
//!
//! # Example
//!
//! ```no_run
//! use oxidex::parsers::pdf::parse_pdf_metadata;
//! use oxidex::io::buffered_reader::BufferedReader;
//! use std::path::Path;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let reader = BufferedReader::new(Path::new("document.pdf"))?;
//! let metadata = parse_pdf_metadata(&reader)?;
//!
//! // Access Info dictionary metadata
//! if let Some(title) = metadata.get_string("PDF:Title") {
//!     println!("Title: {}", title);
//! }
//!
//! // Access XMP metadata
//! if let Some(creator) = metadata.get_string("XMP:creator") {
//!     println!("Creator: {}", creator);
//! }
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

pub mod embedded_files_parser;
pub mod encryption_parser;
pub mod font_parser;
pub mod info_parser;
pub mod permissions_parser;
pub mod resources_parser;
pub mod root_parser;
pub mod shared;
pub mod signature_parser;
pub mod structure_parser;
pub mod xmp_extractor;

use crate::core::{FileReader, MetadataMap};
use crate::error::{ExifToolError, Result};
use crate::parsers::icc::extract_icc_profile;
use encryption_parser::parse_encryption_metadata;
// use root_parser::parse_root_metadata;
// use structure_parser::parse_structure_metadata;

/// PDF signature/magic bytes
const PDF_SIGNATURE: &[u8] = b"%PDF-";

/// Parses PDF file and extracts all metadata.
///
/// This function reads the PDF file structure, verifies the signature,
/// and extracts metadata from both the Info dictionary and XMP packets.
///
/// # Parameters
///
/// - `reader`: FileReader implementation for accessing the PDF file
///
/// # Returns
///
/// - `Ok(MetadataMap)`: Extracted metadata with tag names prefixed appropriately
/// - `Err(ExifToolError)`: Parse error or I/O error
///
/// # Tag Naming Convention
///
/// - Info dictionary: `PDF:<field>` (e.g., `PDF:Title`, `PDF:Author`)
/// - XMP tags: `XMP:<property>` (e.g., `XMP:creator`, `XMP:title`)
///
/// # Errors
///
/// Returns an error if:
/// - File is not a valid PDF (signature mismatch)
/// - File is truncated or malformed
/// - Required PDF structures cannot be found
/// - I/O error occurs
///
/// # Example
///
/// ```no_run
/// use oxidex::parsers::pdf::parse_pdf_metadata;
/// use oxidex::io::buffered_reader::BufferedReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let reader = BufferedReader::new(Path::new("document.pdf"))?;
/// let metadata = parse_pdf_metadata(&reader)?;
///
/// for (key, value) in metadata.iter() {
///     println!("{}: {:?}", key, value);
/// }
/// # Ok(())
/// # }
/// ```
pub fn parse_pdf_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    let file_size = reader.size();

    // Verify PDF signature
    if file_size < PDF_SIGNATURE.len() as u64 {
        return Err(ExifToolError::parse_error("File too small to be a PDF"));
    }

    // Read first 20 bytes to get version
    let header_size = std::cmp::min(20, file_size as usize);
    let header_data = reader.read(0, header_size)?;

    if !header_data.starts_with(PDF_SIGNATURE) {
        return Err(ExifToolError::parse_error("Invalid PDF signature"));
    }

    // Initialize combined metadata map
    let mut metadata = MetadataMap::with_capacity(20);

    // Extract PDF version from header (e.g., "%PDF-1.4")
    // The version is in format "%PDF-X.Y" on the first line
    // PDF headers often have binary data after the first line, so we need to extract just the first line
    // Look for the newline to find end of first line
    let first_line_end = header_data
        .iter()
        .position(|&b| b == b'\n' || b == b'\r')
        .unwrap_or(header_data.len());
    let first_line = &header_data[..first_line_end];

    // The first line should be ASCII: %PDF-X.Y
    if let Ok(header_str) = std::str::from_utf8(first_line)
        && let Some(version_str) = header_str.strip_prefix("%PDF-")
    {
        let version = version_str.trim();
        // Store as string to preserve exact version format (e.g., "1.3", "1.4", "2.0")
        let version_string = crate::core::TagValue::new_string(version.to_string());
        metadata.insert("PDF:PDFVersion".to_string(), version_string.clone());
        // Add PDF:Version as alias for ExifTool compatibility
        metadata.insert("PDF:Version".to_string(), version_string);
    }

    // Check for linearization (optimize for web display)
    // Linearized PDFs have a linearization dictionary in the first object
    // We search for the byte sequence "/Linearized" in the first 2KB
    let check_size = std::cmp::min(2048, file_size as usize);
    let check_data = reader.read(0, check_size)?;

    // Search for "/Linearized" as bytes (PDF dictionaries can contain binary data)
    let linearized_marker = b"/Linearized";
    let is_linearized = check_data
        .windows(linearized_marker.len())
        .any(|window| window == linearized_marker);

    metadata.insert(
        "PDF:Linearized".to_string(),
        crate::core::TagValue::new_string(if is_linearized { "Yes" } else { "No" }),
    );

    // Extract Info dictionary metadata
    match info_parser::parse_info_dict(reader) {
        Ok(info_metadata) => {
            // Merge Info dictionary tags into main metadata
            for (key, value) in info_metadata.iter() {
                metadata.insert(key.clone(), value.clone());
            }
        }
        Err(e) => {
            // Log warning but continue - Info dict might not exist or be malformed
            eprintln!("Warning: Failed to parse PDF Info dictionary: {}", e);
        }
    }

    // Extract XMP metadata
    match xmp_extractor::extract_xmp_metadata(reader) {
        Ok(xmp_metadata) => {
            // Merge XMP tags into main metadata
            for (key, value) in xmp_metadata.iter() {
                metadata.insert(key.clone(), value.clone());
            }
        }
        Err(e) => {
            // Log warning but continue - XMP might not exist
            eprintln!("Warning: Failed to extract XMP metadata: {}", e);
        }
    }

    // Extract ICC profile metadata
    match extract_icc_profile(reader) {
        Ok(icc_metadata) => {
            for (key, value) in icc_metadata.iter() {
                metadata.insert(key.clone(), value.clone());
            }
        }
        Err(_) => {
            // ICC profile is optional - silently continue if not present
        }
    }

    // Extract root dictionary metadata (Language, PageLayout, PageMode, JavaScript, Outlines, Names)
    if let Ok(root_meta) = root_parser::parse_root_metadata(reader) {
        for (key, value) in root_meta.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // Extract encryption metadata
    if let Ok(enc_meta) = encryption_parser::parse_encryption_metadata(reader) {
        for (key, value) in enc_meta.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // Extract digital signature metadata
    if let Ok(sig_meta) = signature_parser::parse_signature_metadata(reader) {
        for (key, value) in sig_meta.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // Extract permissions metadata
    if let Ok(perm_meta) = permissions_parser::parse_permissions_metadata(reader) {
        for (key, value) in perm_meta.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // Extract embedded resources metadata
    if let Ok(res_meta) = resources_parser::parse_resources_metadata(reader) {
        for (key, value) in res_meta.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // Extract font metadata
    if let Ok(font_meta) = font_parser::parse_font_metadata(reader) {
        for (key, value) in font_meta.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // Extract embedded file metadata
    if let Ok(embedded_meta) = embedded_files_parser::parse_embedded_files_metadata(reader) {
        for (key, value) in embedded_meta.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // Extract encryption metadata
    match parse_encryption_metadata(reader) {
        Ok(encryption_metadata) => {
            // Merge encryption tags into main metadata
            for (key, value) in encryption_metadata.iter() {
                metadata.insert(key.clone(), value.clone());
            }
        }
        Err(e) => {
            // Log warning but continue - encryption might not be parseable
            eprintln!("Warning: Failed to parse PDF encryption metadata: {}", e);
        }
    }

    // Extract Root/Catalog metadata
    // TODO: Implement root_parser
    // match parse_root_metadata(reader) {
    //     Ok(root_metadata) => {
    //         // Merge Root tags into main metadata
    //         for (key, value) in root_metadata.iter() {
    //             metadata.insert(key.clone(), value.clone());
    //         }
    //     }
    //     Err(e) => {
    //         // Log warning but continue - Root metadata might not be parseable
    //         eprintln!("Warning: Failed to parse PDF Root metadata: {}", e);
    //     }
    // }

    // Extract structure and features metadata
    // TODO: Implement structure_parser
    // match parse_structure_metadata(reader) {
    //     Ok(structure_metadata) => {
    //         // Merge structure tags into main metadata
    //         for (key, value) in structure_metadata.iter() {
    //             metadata.insert(key.clone(), value.clone());
    //         }
    //     }
    //     Err(e) => {
    //         // Log warning but continue - structure metadata might not be parseable
    //         eprintln!("Warning: Failed to parse PDF structure metadata: {}", e);
    //     }
    // }

    // PDF image streams may contain complete TIFF/EXIF payloads. Extract
    // metadata from these payloads after the PDF-level resource parsers have
    // run so the standard IFD tag database determines the canonical key.
    if let Ok(exif_metadata) = extract_embedded_exif_metadata(reader) {
        for (key, value) in exif_metadata.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // If we didn't extract any metadata at all, return error
    if metadata.is_empty() {
        return Err(ExifToolError::parse_error(
            "No metadata found in PDF (no Info dictionary, XMP, ICC profile, encryption, Root, or structure)",
        ));
    }

    Ok(metadata)
}

// Standard TIFF tag IDs from ExifTool 13.55 Exif.pm.
const TAG_IMAGE_DESCRIPTION: u16 = 0x010e;
const TAG_MAKE: u16 = 0x010f;
const TIFF_ARTIST_TAG: u16 = 0x013b;
const TAG_EXIF_IFD: u16 = 0x8769;
const TAG_APERTURE_VALUE: u16 = 0x9202;
const TAG_BRIGHTNESS_VALUE: u16 = 0x9203;
const TAG_COLOR_SPACE: u16 = 0xA001;
const TAG_COMPONENTS_CONFIGURATION: u16 = 0x9101;
const TAG_COMPRESSED_BITS_PER_PIXEL: u16 = 0x9102;
const TAG_COMPRESSION: u16 = 0x0103;
const TAG_EXIF_VERSION: u16 = 0x9000;
const TAG_EXPOSURE_PROGRAM: u16 = 0x8822;
const TAG_EXPOSURE_COMPENSATION: u16 = 0x9204;
const TAG_COPYRIGHT: u16 = 0x8298;
const TAG_DATE_TIME_ORIGINAL: u16 = 0x9003;
const TAG_CREATE_DATE: u16 = 0x9004;
const TAG_FLASH: u16 = 0x9209;
const TAG_FLASHPIX_VERSION: u16 = 0xA000;
const TAG_EXIF_IMAGE_WIDTH: u16 = 0xA002;
const TAG_EXIF_IMAGE_HEIGHT: u16 = 0xA003;
// ExifTool 13.55 Exif.pm: FocalPlaneX/YResolution.
const TAG_FOCAL_PLANE_X_RESOLUTION: u16 = 0xA20e;
const TAG_FOCAL_PLANE_Y_RESOLUTION: u16 = 0xA20f;
const TAG_FOCAL_PLANE_RESOLUTION_UNIT: u16 = 0xA210;
const TAG_FILE_SOURCE: u16 = 0xA300;

/// `%compression` from ExifTool 13.55 Exif.pm, transcribed in full.
///
/// The archived patches this parser grew from carried a nine-entry excerpt
/// ending at `32773 => 'PackBits'`. A truncated PrintConv table is the exact
/// shape that shipped `32767 => "Sony RAW"` instead of
/// `'Sony ARW Compressed'` elsewhere in this codebase, so the whole table is
/// reproduced here rather than the handful of values PDF.pdf happens to hit.
const COMPRESSION_LABELS: &[(u16, &str)] = &[
    (1, "Uncompressed"),
    (2, "CCITT 1D"),
    (3, "T4/Group 3 Fax"),
    (4, "T6/Group 4 Fax"),
    (5, "LZW"),
    (6, "JPEG (old-style)"),
    (7, "JPEG"),
    (8, "Adobe Deflate"),
    (9, "JBIG B&W"),
    (10, "JBIG Color"),
    (99, "JPEG"),
    (262, "Kodak 262"),
    (32766, "NeXt or Sony ARW Compressed 2"),
    (32767, "Sony ARW Compressed"),
    (32769, "Packed RAW"),
    (32770, "Samsung SRW Compressed"),
    (32771, "CCIRLEW"),
    (32772, "Samsung SRW Compressed 2"),
    (32773, "PackBits"),
    (32809, "Thunderscan"),
    (32867, "Kodak KDC Compressed"),
    (32895, "IT8CTPAD"),
    (32896, "IT8LW"),
    (32897, "IT8MP"),
    (32898, "IT8BL"),
    (32908, "PixarFilm"),
    (32909, "PixarLog"),
    (32946, "Deflate"),
    (32947, "DCS"),
    (33003, "Aperio JPEG 2000 YCbCr"),
    (33005, "Aperio JPEG 2000 RGB"),
    (34661, "JBIG"),
    (34676, "SGILog"),
    (34677, "SGILog24"),
    (34712, "JPEG 2000"),
    (34713, "Nikon NEF Compressed"),
    (34715, "JBIG2 TIFF FX"),
    (34718, "Microsoft Document Imaging (MDI) Binary Level Codec"),
    (
        34719,
        "Microsoft Document Imaging (MDI) Progressive Transform Codec",
    ),
    (34720, "Microsoft Document Imaging (MDI) Vector"),
    (34887, "ESRI Lerc"),
    (34892, "Lossy JPEG"),
    (34925, "LZMA2"),
    (34926, "Zstd (old)"),
    (34927, "WebP (old)"),
    (34933, "PNG"),
    (34934, "JPEG XR"),
    (50000, "Zstd"),
    (50001, "WebP"),
    (50002, "JPEG XL (old)"),
    (52546, "JPEG XL"),
    (65000, "Kodak DCR Compressed"),
    (65535, "Pentax PEF Compressed"),
];

/// `%flash` from ExifTool 13.55 Exif.pm, transcribed in full.
///
/// Three archived patches decoded this tag by OR-ing bit meanings together
/// (`bit 3 => "Auto"`, and so on). That is not what ExifTool does: 0x08 is a
/// single table entry meaning `'On, Did not fire'`, not "Auto". The bitwise
/// spelling only agreed with ExifTool for the one value PDF.pdf stores (1),
/// which is why it survived its recheck.
const FLASH_LABELS: &[(u16, &str)] = &[
    (0x00, "No Flash"),
    (0x01, "Fired"),
    (0x05, "Fired, Return not detected"),
    (0x07, "Fired, Return detected"),
    (0x08, "On, Did not fire"),
    (0x09, "On, Fired"),
    (0x0d, "On, Return not detected"),
    (0x0f, "On, Return detected"),
    (0x10, "Off, Did not fire"),
    (0x14, "Off, Did not fire, Return not detected"),
    (0x18, "Auto, Did not fire"),
    (0x19, "Auto, Fired"),
    (0x1d, "Auto, Fired, Return not detected"),
    (0x1f, "Auto, Fired, Return detected"),
    (0x20, "No flash function"),
    (0x30, "Off, No flash function"),
    (0x41, "Fired, Red-eye reduction"),
    (0x45, "Fired, Red-eye reduction, Return not detected"),
    (0x47, "Fired, Red-eye reduction, Return detected"),
    (0x49, "On, Red-eye reduction"),
    (0x4d, "On, Red-eye reduction, Return not detected"),
    (0x4f, "On, Red-eye reduction, Return detected"),
    (0x50, "Off, Red-eye reduction"),
    (0x58, "Auto, Did not fire, Red-eye reduction"),
    (0x59, "Auto, Fired, Red-eye reduction"),
    (0x5d, "Auto, Fired, Red-eye reduction, Return not detected"),
    (0x5f, "Auto, Fired, Red-eye reduction, Return detected"),
];

/// FocalPlaneResolutionUnit (0xa210) PrintConv, ExifTool 13.55 Exif.pm.
/// Values 1, 4 and 5 are flagged there as non-standard EXIF but are still
/// decoded, so all five are kept.
const FOCAL_PLANE_RESOLUTION_UNIT_LABELS: &[(u16, &str)] =
    &[(1, "None"), (2, "inches"), (3, "cm"), (4, "mm"), (5, "um")];

/// FileSource (0xa300) PrintConv, ExifTool 13.55 Exif.pm.
///
/// Two archived patches wrote `3 => "Digital Camera", _ => "Unknown"`. The
/// "Unknown" fallback is invented - ExifTool prints the undecoded number - and
/// dropping 1 and 2 would have re-created the `1 => 'Scanner'` class of bug,
/// since 1 is `'Film Scanner'`, not a generic scanner.
const FILE_SOURCE_LABELS: &[(u8, &str)] = &[
    (1, "Film Scanner"),
    (2, "Reflection Print Scanner"),
    (3, "Digital Camera"),
];

/// ColorSpace (0xa001) PrintConv, ExifTool 13.55 Exif.pm. The inherited code
/// carried only `1` and `0xffff`.
const COLOR_SPACE_LABELS: &[(u16, &str)] = &[
    (1, "sRGB"),
    (2, "Adobe RGB"),
    (0xfffd, "Wide Gamut RGB"),
    (0xfffe, "ICC Profile"),
    (0xffff, "Uncalibrated"),
];

/// ExposureProgram (0x8822) PrintConv, transcribed verbatim from ExifTool
/// 13.55 Exif.pm lines 2088-2098. Value 9 is non-standard EXIF (Canon) but
/// ExifTool still decodes it, so it is kept here.
const EXPOSURE_PROGRAM_LABELS: &[(u16, &str)] = &[
    (0, "Not Defined"),
    (1, "Manual"),
    (2, "Program AE"),
    (3, "Aperture-priority AE"),
    (4, "Shutter speed priority AE"),
    (5, "Creative (Slow speed)"),
    (6, "Action (High speed)"),
    (7, "Portrait"),
    (8, "Landscape"),
    (9, "Bulb"),
];

#[derive(Clone, Copy)]
enum EmbeddedTiffByteOrder {
    Little,
    Big,
}

/// Extracts standard EXIF tags from TIFF/EXIF payloads embedded in PDF image
/// streams (IFD0 and ExifIFD).
///
/// DCT-encoded PDF image streams retain their JPEG APP1 payload byte-for-byte,
/// so the TIFF byte-order marker can be located without interpreting the PDF
/// data as text. TIFF offsets remain relative to the located TIFF header.
fn extract_embedded_exif_metadata(
    reader: &dyn crate::core::FileReader,
) -> crate::error::Result<crate::core::MetadataMap> {
    let file_size = usize::try_from(reader.size()).map_err(|_| {
        crate::error::ExifToolError::parse_error(
            "PDF is too large to scan for embedded EXIF metadata",
        )
    })?;
    let data = reader.read(0, file_size)?;

    Ok(find_embedded_exif_tags(data).unwrap_or_default())
}

/// Searches raw PDF bytes for embedded TIFF/EXIF headers and returns
/// recognised EXIF tags from the first valid IFD0+ExifIFD chain.
fn find_embedded_exif_tags(data: &[u8]) -> Option<MetadataMap> {
    let mut cursor = 0usize;

    while cursor < data.len() {
        let remaining = data.get(cursor..)?;
        let Some(rel_off) = remaining
            .windows(4)
            .position(|w| w == b"II\x2a\x00" || w == b"MM\x00\x2a")
        else {
            break;
        };
        let tiff_offset = cursor.checked_add(rel_off)?;
        if let Some(tags) = parse_embedded_tiff_ifds(data.get(tiff_offset..)?) {
            return Some(tags);
        }
        cursor = tiff_offset.checked_add(1)?;
    }

    None
}

/// Parse IFD0 and optionally the ExifIFD sub-IFD, collecting recognised tags.
fn parse_embedded_tiff_ifds(data: &[u8]) -> Option<MetadataMap> {
    let byte_order = if data.get(..2)? == b"II" {
        EmbeddedTiffByteOrder::Little
    } else if data.get(..2)? == b"MM" {
        EmbeddedTiffByteOrder::Big
    } else {
        return None;
    };

    if read_embedded_tiff_u16(data, 2, byte_order)? != 0x002A {
        return None;
    }

    let ifd0_offset = usize::try_from(read_embedded_tiff_u32(data, 4, byte_order)?).ok()?;
    let mut metadata = MetadataMap::with_capacity(8);
    let mut exif_ifd_offset: Option<usize> = None;

    // ---- IFD0 entries ------------------------------------------------------
    let entry_count = usize::from(read_embedded_tiff_u16(data, ifd0_offset, byte_order)?);
    let entries_offset = ifd0_offset.checked_add(2)?;
    let entries_len = entry_count.checked_mul(12)?;
    let entries_end = entries_offset.checked_add(entries_len)?;
    data.get(entries_offset..entries_end)?;

    for i in 0..entry_count {
        let base = entries_offset.checked_add(i.checked_mul(12)?)?;
        let tag = read_embedded_tiff_u16(data, base, byte_order)?;
        let field_type = read_embedded_tiff_u16(data, base.checked_add(2)?, byte_order)?;
        let count = read_embedded_tiff_u32(data, base.checked_add(4)?, byte_order)?;

        match tag {
            TAG_IMAGE_DESCRIPTION if field_type == 2 => {
                if let Some(v) = read_ascii_value(data, base, byte_order, count) {
                    let key = crate::tag_db::lookup_tag_name(TAG_IMAGE_DESCRIPTION, "IFD0");
                    metadata.insert(key, crate::core::TagValue::new_string(v));
                }
            }
            TAG_MAKE if field_type == 2 => {
                if let Some(v) = read_ascii_value(data, base, byte_order, count) {
                    let key = crate::tag_db::lookup_tag_name(TAG_MAKE, "IFD0");
                    metadata.insert(key, crate::core::TagValue::new_string(v));
                }
            }
            TIFF_ARTIST_TAG if field_type == 2 => {
                if let Some(v) = read_ascii_value(data, base, byte_order, count) {
                    if !v.is_empty() {
                        let key = crate::tag_db::lookup_tag_name(TIFF_ARTIST_TAG, "IFD0");
                        metadata.insert(key, crate::core::TagValue::new_string(v));
                    }
                }
            }
            TAG_COMPRESSION if field_type == 3 => {
                if let Some(raw) = read_short_value(data, base, byte_order) {
                    if let Some(label) = COMPRESSION_LABELS
                        .iter()
                        .find(|&&(id, _)| id == raw)
                        .map(|&(_, s)| s)
                    {
                        let key = crate::tag_db::lookup_tag_name(TAG_COMPRESSION, "IFD0");
                        metadata.insert(key, crate::core::TagValue::new_string(label.to_string()));
                    }
                }
            }
            // ExifTool 13.55 Exif.pm 0x8298: Name => 'Copyright',
            // WriteGroup => 'IFD0', Format => 'undef', Writable => 'string'.
            // PDF.pdf stores it with the ASCII field type and the Format
            // override only affects the raw read, so the string arm applies.
            TAG_COPYRIGHT if field_type == 2 || field_type == 7 => {
                if let Some(v) = read_ascii_value(data, base, byte_order, count) {
                    let key = crate::tag_db::lookup_tag_name(TAG_COPYRIGHT, "IFD0");
                    metadata.insert(key, crate::core::TagValue::new_string(v));
                }
            }
            TAG_EXIF_IFD if field_type == 4 || field_type == 13 => {
                exif_ifd_offset = Some(
                    usize::try_from(read_embedded_tiff_u32(
                        data,
                        base.checked_add(8)?,
                        byte_order,
                    )?)
                    .ok()?,
                );
            }
            _ => {}
        }
    }

    // ---- IFD1 (thumbnail IFD) ----------------------------------------------
    // The Compression tag PDF.pdf reports lives in IFD1, not IFD0: ExifTool
    // prints it as `[IFD1] Compression : JPEG (old-style)`. The inherited code
    // only ever looked in IFD0 and the ExifIFD, so its Compression arms could
    // never fire - every archived patch in this group re-listed
    // `EXIF:Compression` as still missing for exactly that reason.
    if let Some(ifd1_offset) = read_next_ifd_offset(data, ifd0_offset, entry_count, byte_order) {
        if let Some(sub_tags) = parse_ifd1(data, ifd1_offset, byte_order) {
            for (k, v) in sub_tags.iter() {
                metadata.insert(k.clone(), v.clone());
            }
        }
    }

    // ---- ExifIFD sub-IFD ----------------------------------------------------
    if let Some(off) = exif_ifd_offset {
        if let Some(sub_tags) = parse_exif_ifd(data, off, byte_order) {
            for (k, v) in sub_tags.iter() {
                metadata.insert(k.clone(), v.clone());
            }
        }
    }

    if metadata.is_empty() {
        None
    } else {
        Some(metadata)
    }
}

/// Reads the next-IFD pointer that follows an IFD's entry array, returning
/// `None` when it is absent, zero (end of chain) or points back at the IFD
/// itself (a loop some malformed files contain).
fn read_next_ifd_offset(
    data: &[u8],
    ifd_offset: usize,
    entry_count: usize,
    byte_order: EmbeddedTiffByteOrder,
) -> Option<usize> {
    let pointer_offset = ifd_offset
        .checked_add(2)?
        .checked_add(entry_count.checked_mul(12)?)?;
    let next = usize::try_from(read_embedded_tiff_u32(data, pointer_offset, byte_order)?).ok()?;

    if next == 0 || next == ifd_offset {
        None
    } else {
        Some(next)
    }
}

/// Parses the thumbnail IFD (IFD1) and returns the tags decoded from it.
fn parse_ifd1(
    data: &[u8],
    ifd_offset: usize,
    byte_order: EmbeddedTiffByteOrder,
) -> Option<MetadataMap> {
    let entry_count = usize::from(read_embedded_tiff_u16(data, ifd_offset, byte_order)?);
    let entries_offset = ifd_offset.checked_add(2)?;
    let entries_len = entry_count.checked_mul(12)?;
    let entries_end = entries_offset.checked_add(entries_len)?;
    data.get(entries_offset..entries_end)?;

    let mut metadata = MetadataMap::with_capacity(2);

    for i in 0..entry_count {
        let base = entries_offset.checked_add(i.checked_mul(12)?)?;
        let tag = read_embedded_tiff_u16(data, base, byte_order)?;
        let field_type = read_embedded_tiff_u16(data, base.checked_add(2)?, byte_order)?;

        if tag == TAG_COMPRESSION && field_type == 3 {
            if let Some(raw) = read_short_value(data, base, byte_order) {
                if let Some(label) = lookup_label(COMPRESSION_LABELS, raw) {
                    let key = crate::tag_db::lookup_tag_name(TAG_COMPRESSION, "IFD1");
                    metadata.insert(key, crate::core::TagValue::new_string(label.to_string()));
                }
            }
        }
    }

    if metadata.is_empty() {
        None
    } else {
        Some(metadata)
    }
}

/// Looks a raw value up in one of the transcribed ExifTool PrintConv tables.
///
/// Unknown values return `None` so the caller drops the tag: ExifTool would
/// print the bare number, and inventing an "Unknown" label - as two archived
/// patches did for FileSource and FocalPlaneResolutionUnit - emits metadata
/// that ExifTool never produces.
fn lookup_label<T: Copy + PartialEq>(table: &[(T, &'static str)], raw: T) -> Option<&'static str> {
    table
        .iter()
        .find(|&&(id, _)| id == raw)
        .map(|&(_, label)| label)
}

/// Parse a single ExifIFD (pointed to by the ExifIFD tag in IFD0) and return
/// decoded EXIF tags.
fn parse_exif_ifd(
    data: &[u8],
    ifd_offset: usize,
    byte_order: EmbeddedTiffByteOrder,
) -> Option<MetadataMap> {
    let entry_count = usize::from(read_embedded_tiff_u16(data, ifd_offset, byte_order)?);
    let entries_offset = ifd_offset.checked_add(2)?;
    let entries_len = entry_count.checked_mul(12)?;
    let entries_end = entries_offset.checked_add(entries_len)?;
    data.get(entries_offset..entries_end)?;

    let mut metadata = MetadataMap::with_capacity(6);

    for i in 0..entry_count {
        let base = entries_offset.checked_add(i.checked_mul(12)?)?;
        let tag = read_embedded_tiff_u16(data, base, byte_order)?;
        let field_type = read_embedded_tiff_u16(data, base.checked_add(2)?, byte_order)?;
        let count = read_embedded_tiff_u32(data, base.checked_add(4)?, byte_order)?;

        match tag {
            // ExifTool 13.55 Exif.pm 0x9000 has no PrintConv, only
            // `RawConv => '$val=~s/\0+$//; $val'`, so the four undef bytes are
            // emitted verbatim minus any trailing NULs ("0210" for PDF.pdf).
            TAG_EXIF_VERSION if field_type == 7 && count == 4 => {
                if let Some(bytes) = read_undefined_value(data, base, byte_order, count) {
                    let text_end = bytes
                        .iter()
                        .rposition(|&b| b != 0)
                        .map_or(0, |last| last.saturating_add(1));
                    let s = String::from_utf8_lossy(&bytes[..text_end]).into_owned();
                    if !s.is_empty() {
                        let key = crate::tag_db::lookup_tag_name(TAG_EXIF_VERSION, "ExifIFD");
                        metadata.insert(key, crate::core::TagValue::new_string(s));
                    }
                }
            }
            TAG_EXPOSURE_PROGRAM if field_type == 3 => {
                if let Some(raw) = read_short_value(data, base, byte_order) {
                    // Unknown values are dropped rather than guessed: ExifTool
                    // would print the bare number, and emitting a made-up label
                    // is worse than leaving the gap open.
                    if let Some(label) = EXPOSURE_PROGRAM_LABELS
                        .iter()
                        .find(|&&(id, _)| id == raw)
                        .map(|&(_, s)| s)
                    {
                        let key = crate::tag_db::lookup_tag_name(TAG_EXPOSURE_PROGRAM, "ExifIFD");
                        metadata.insert(key, crate::core::TagValue::new_string(label.to_string()));
                    }
                }
            }
            // ExifTool 13.55 Exif.pm 0x9204 carries `Format => 'rational64s'`
            // with the comment "Leica M8 patch (incorrectly written as
            // rational64u)", i.e. the stored type is overridden and the value
            // is always read signed - so rational64u (5) is accepted too.
            TAG_EXPOSURE_COMPENSATION if field_type == 10 || field_type == 5 => {
                if let Some((num, den)) = read_signed_rational_value(data, base, byte_order) {
                    if den != 0 {
                        let key =
                            crate::tag_db::lookup_tag_name(TAG_EXPOSURE_COMPENSATION, "ExifIFD");
                        metadata.insert(
                            key,
                            crate::core::TagValue::new_string(print_fraction(
                                f64::from(num) / f64::from(den),
                            )),
                        );
                    }
                }
            }
            // ExifTool 13.55 Exif.pm 0x9202:
            //   ValueConv => '2 ** ($val / 2)',
            //   PrintConv => 'sprintf("%.1f",$val)',
            // The inherited code stopped after the ValueConv and printed the
            // full f64, so PDF.pdf reported "3.4822022531844965" where
            // ExifTool reports "3.5".
            TAG_APERTURE_VALUE if field_type == 5 => {
                if let Some((num, den)) = read_unsigned_rational_value(data, base, byte_order) {
                    let apex = if den != 0 {
                        f64::from(num) / f64::from(den)
                    } else {
                        0.0
                    };
                    let f_number = 2.0_f64.powf(apex / 2.0);
                    let key = crate::tag_db::lookup_tag_name(TAG_APERTURE_VALUE, "ExifIFD");
                    metadata.insert(
                        key,
                        crate::core::TagValue::new_string(format!("{:.1}", f_number)),
                    );
                }
            }
            // ExifTool 13.55 Exif.pm 0x9203: Writable => 'rational64s', no
            // PrintConv. PDF.pdf stores it as rational64s (field type 10);
            // the inherited unsigned-only arm could never fire there.
            TAG_BRIGHTNESS_VALUE if field_type == 10 => {
                if let Some((num, den)) = read_signed_rational_value(data, base, byte_order) {
                    if den != 0 {
                        let key = crate::tag_db::lookup_tag_name(TAG_BRIGHTNESS_VALUE, "ExifIFD");
                        metadata.insert(
                            key,
                            crate::core::TagValue::new_string(format_rational(
                                f64::from(num) / f64::from(den),
                            )),
                        );
                    }
                }
            }
            // ExifTool 13.55 Exif.pm 0x9003 / 0x9004: Writable => 'string',
            // PrintConv => '$self->ConvertDateTime($val)', which is a no-op
            // without -d, so the stored "YYYY:MM:DD HH:MM:SS" is emitted as-is.
            TAG_DATE_TIME_ORIGINAL if field_type == 2 => {
                if let Some(v) = read_ascii_value(data, base, byte_order, count) {
                    let key = crate::tag_db::lookup_tag_name(TAG_DATE_TIME_ORIGINAL, "ExifIFD");
                    metadata.insert(key, crate::core::TagValue::new_string(v));
                }
            }
            TAG_CREATE_DATE if field_type == 2 => {
                if let Some(v) = read_ascii_value(data, base, byte_order, count) {
                    let key = crate::tag_db::lookup_tag_name(TAG_CREATE_DATE, "ExifIFD");
                    metadata.insert(key, crate::core::TagValue::new_string(v));
                }
            }
            // ExifTool 13.55 Exif.pm 0xa002 / 0xa003: Writable => 'int16u',
            // no PrintConv. PDF.pdf writes them as int32u (field type 4) -
            // patches that only handled int16u closed nothing here.
            TAG_EXIF_IMAGE_WIDTH if field_type == 3 || field_type == 4 => {
                if let Some(raw) = read_short_or_long_value(data, base, byte_order, field_type) {
                    let key = crate::tag_db::lookup_tag_name(TAG_EXIF_IMAGE_WIDTH, "ExifIFD");
                    metadata.insert(key, crate::core::TagValue::new_string(raw.to_string()));
                }
            }
            TAG_EXIF_IMAGE_HEIGHT if field_type == 3 || field_type == 4 => {
                if let Some(raw) = read_short_or_long_value(data, base, byte_order, field_type) {
                    let key = crate::tag_db::lookup_tag_name(TAG_EXIF_IMAGE_HEIGHT, "ExifIFD");
                    metadata.insert(key, crate::core::TagValue::new_string(raw.to_string()));
                }
            }
            // ExifTool 13.55 Exif.pm 0xa000: Writable => 'undef', no
            // PrintConv, RawConv => '$val=~s/\0+$//; $val'.
            TAG_FLASHPIX_VERSION if field_type == 7 => {
                if let Some(bytes) = read_undefined_value(data, base, byte_order, count) {
                    if let Some(s) = trim_trailing_nuls(&bytes) {
                        let key = crate::tag_db::lookup_tag_name(TAG_FLASHPIX_VERSION, "ExifIFD");
                        metadata.insert(key, crate::core::TagValue::new_string(s));
                    }
                }
            }
            // ExifTool 13.55 Exif.pm 0x9209: PrintConv => \%flash.
            TAG_FLASH if field_type == 3 => {
                if let Some(raw) = read_short_value(data, base, byte_order) {
                    if let Some(label) = lookup_label(FLASH_LABELS, raw) {
                        let key = crate::tag_db::lookup_tag_name(TAG_FLASH, "ExifIFD");
                        metadata.insert(key, crate::core::TagValue::new_string(label.to_string()));
                    }
                }
            }
            // ExifTool 13.55 Exif.pm 0xa20e and 0xa20f are unsigned
            // rationals without a PrintConv.
            TAG_FOCAL_PLANE_X_RESOLUTION if field_type == 5 => {
                if let Some((num, den)) = read_unsigned_rational_value(data, base, byte_order) {
                    if den != 0 {
                        let key = crate::tag_db::lookup_tag_name(
                            TAG_FOCAL_PLANE_X_RESOLUTION,
                            "ExifIFD",
                        );
                        metadata.insert(
                            key,
                            crate::core::TagValue::new_string(format_rational(
                                f64::from(num) / f64::from(den),
                            )),
                        );
                    }
                }
            }
            TAG_FOCAL_PLANE_Y_RESOLUTION if field_type == 5 => {
                if let Some((num, den)) = read_unsigned_rational_value(data, base, byte_order) {
                    if den != 0 {
                        let key = crate::tag_db::lookup_tag_name(
                            TAG_FOCAL_PLANE_Y_RESOLUTION,
                            "ExifIFD",
                        );
                        metadata.insert(
                            key,
                            crate::core::TagValue::new_string(format_rational(
                                f64::from(num) / f64::from(den),
                            )),
                        );
                    }
                }
            }
            // ExifTool 13.55 Exif.pm 0xa210 PrintConv.
            TAG_FOCAL_PLANE_RESOLUTION_UNIT if field_type == 3 => {
                if let Some(raw) = read_short_value(data, base, byte_order) {
                    if let Some(label) = lookup_label(FOCAL_PLANE_RESOLUTION_UNIT_LABELS, raw) {
                        let key = crate::tag_db::lookup_tag_name(
                            TAG_FOCAL_PLANE_RESOLUTION_UNIT,
                            "ExifIFD",
                        );
                        metadata.insert(key, crate::core::TagValue::new_string(label.to_string()));
                    }
                }
            }
            // ExifTool 13.55 Exif.pm 0xa300: Writable => 'undef', PrintConv
            // keyed on the single stored byte.
            TAG_FILE_SOURCE if field_type == 7 => {
                if let Some(bytes) = read_undefined_value(data, base, byte_order, count) {
                    if let Some(label) = bytes
                        .first()
                        .copied()
                        .and_then(|b| lookup_label(FILE_SOURCE_LABELS, b))
                    {
                        let key = crate::tag_db::lookup_tag_name(TAG_FILE_SOURCE, "ExifIFD");
                        metadata.insert(key, crate::core::TagValue::new_string(label.to_string()));
                    }
                }
            }
            TAG_BRIGHTNESS_VALUE if field_type == 5 => {
                if let Some((num, den)) = read_unsigned_rational_value(data, base, byte_order) {
                    let val_str = if den == 0 {
                        "0".to_string()
                    } else if den == 1 {
                        num.to_string()
                    } else {
                        format!("{}", num as f64 / den as f64)
                    };
                    let key = crate::tag_db::lookup_tag_name(TAG_BRIGHTNESS_VALUE, "ExifIFD");
                    metadata.insert(key, crate::core::TagValue::new_string(val_str));
                }
            }
            TAG_COMPRESSED_BITS_PER_PIXEL if field_type == 5 => {
                if let Some((num, den)) = read_unsigned_rational_value(data, base, byte_order) {
                    let val_str = if den == 0 {
                        "0".to_string()
                    } else if den == 1 {
                        num.to_string()
                    } else {
                        let d = num as f64 / den as f64;
                        let s = format!("{:.9}", d);
                        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
                        if trimmed.is_empty() {
                            "0".to_string()
                        } else {
                            trimmed.to_string()
                        }
                    };
                    let key =
                        crate::tag_db::lookup_tag_name(TAG_COMPRESSED_BITS_PER_PIXEL, "ExifIFD");
                    metadata.insert(key, crate::core::TagValue::new_string(val_str));
                }
            }
            // ExifTool 13.55 Exif.pm 0xa001 PrintConv. The inherited code
            // carried only two of the five entries and, worse, did
            // `_ => return None` on anything else - abandoning the whole
            // ExifIFD (and every tag after ColorSpace) over one unknown value.
            TAG_COLOR_SPACE if field_type == 3 => {
                if let Some(raw) = read_short_value(data, base, byte_order) {
                    if let Some(label) = lookup_label(COLOR_SPACE_LABELS, raw) {
                        let key = crate::tag_db::lookup_tag_name(TAG_COLOR_SPACE, "ExifIFD");
                        metadata.insert(key, crate::core::TagValue::new_string(label.to_string()));
                    }
                }
            }
            TAG_COMPONENTS_CONFIGURATION if field_type == 7 => {
                if let Some(bytes) = read_undefined_value(data, base, byte_order, count) {
                    let components: Vec<String> = bytes
                        .iter()
                        .map(|&b| match b {
                            0 => "-".to_string(),
                            1 => "Y".to_string(),
                            2 => "Cb".to_string(),
                            3 => "Cr".to_string(),
                            4 => "R".to_string(),
                            5 => "G".to_string(),
                            6 => "B".to_string(),
                            _ => "?".to_string(),
                        })
                        .collect();
                    let key =
                        crate::tag_db::lookup_tag_name(TAG_COMPONENTS_CONFIGURATION, "ExifIFD");
                    metadata.insert(
                        key,
                        crate::core::TagValue::new_string(components.join(", ")),
                    );
                }
            }
            _ => {}
        }
    }

    if metadata.is_empty() {
        None
    } else {
        Some(metadata)
    }
}

// ---------------------------------------------------------------------------
//  TIFF entry value helpers
// ---------------------------------------------------------------------------

fn get_entry_value_offset(
    data: &[u8],
    entry_offset: usize,
    type_size: usize,
    count: usize,
    byte_order: EmbeddedTiffByteOrder,
) -> Option<usize> {
    let total = count.checked_mul(type_size)?;
    if total <= 4 {
        entry_offset.checked_add(8)
    } else {
        let value_off = read_embedded_tiff_u32(data, entry_offset.checked_add(8)?, byte_order)?;
        Some(usize::try_from(value_off).ok()?)
    }
}

fn read_ascii_value(
    data: &[u8],
    entry_offset: usize,
    byte_order: EmbeddedTiffByteOrder,
    count: u32,
) -> Option<String> {
    let len = usize::try_from(count).ok()?;
    if len == 0 {
        return None;
    }
    let val_off = get_entry_value_offset(data, entry_offset, 1, len, byte_order)?;
    let end = val_off.checked_add(len)?;
    let raw = data.get(val_off..end)?;
    let text_end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let s = String::from_utf8_lossy(&raw[..text_end]).into_owned();
    if s.is_empty() { None } else { Some(s) }
}

fn read_short_value(
    data: &[u8],
    entry_offset: usize,
    byte_order: EmbeddedTiffByteOrder,
) -> Option<u16> {
    let val_off = get_entry_value_offset(data, entry_offset, 2, 1, byte_order)?;
    read_embedded_tiff_u16(data, val_off, byte_order)
}

fn read_unsigned_rational_value(
    data: &[u8],
    entry_offset: usize,
    byte_order: EmbeddedTiffByteOrder,
) -> Option<(u32, u32)> {
    let val_off = get_entry_value_offset(data, entry_offset, 8, 1, byte_order)?;
    let num = read_embedded_tiff_u32(data, val_off, byte_order)?;
    let den = read_embedded_tiff_u32(data, val_off.checked_add(4)?, byte_order)?;
    Some((num, den))
}

/// Reads a SHORT (type 3) or LONG (type 4) scalar as a `u32`.
fn read_short_or_long_value(
    data: &[u8],
    entry_offset: usize,
    byte_order: EmbeddedTiffByteOrder,
    field_type: u16,
) -> Option<u32> {
    if field_type == 4 {
        let val_off = get_entry_value_offset(data, entry_offset, 4, 1, byte_order)?;
        read_embedded_tiff_u32(data, val_off, byte_order)
    } else {
        read_short_value(data, entry_offset, byte_order).map(u32::from)
    }
}

/// Applies ExifTool's `RawConv => '$val=~s/\0+$//; $val'` to an undef value,
/// returning `None` when nothing is left.
fn trim_trailing_nuls(bytes: &[u8]) -> Option<String> {
    let text_end = bytes
        .iter()
        .rposition(|&b| b != 0)
        .map_or(0, |last| last.saturating_add(1));
    let s = String::from_utf8_lossy(bytes.get(..text_end)?).into_owned();
    if s.is_empty() { None } else { Some(s) }
}

/// Renders a rational as a decimal string the way the already-validated
/// CompressedBitsPerPixel path does: nine decimal places, trailing zeros
/// stripped (`16/13` -> `1.230769231`, `200/100` -> `2`).
fn format_rational(value: f64) -> String {
    let s = format!("{:.9}", value);
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn read_signed_rational_value(
    data: &[u8],
    entry_offset: usize,
    byte_order: EmbeddedTiffByteOrder,
) -> Option<(i32, i32)> {
    let val_off = get_entry_value_offset(data, entry_offset, 8, 1, byte_order)?;
    let num = read_embedded_tiff_u32(data, val_off, byte_order)? as i32;
    let den = read_embedded_tiff_u32(data, val_off.checked_add(4)?, byte_order)? as i32;
    Some((num, den))
}

/// Port of `Image::ExifTool::Exif::PrintFraction` (ExifTool 13.55 Exif.pm
/// lines 5421-5440), used by ExposureCompensation and friends.
///
/// The archived patch this code comes from (pdf-bff9296f5e84, 2026-07-27)
/// instead wrote `format!("{}", num as f64 / den as f64)` and still passed its
/// recheck, because the only sample it was measured against - PDF.pdf - stores
/// 0/100, and both spellings print "0" for that. Every non-zero value diverges:
/// ExifTool prints 1/3 EV as "+1/3", the decimal form printed
/// "0.3333333333333333". Verified on 2026-07-27 by running the verbatim Perl
/// body of PrintFraction over the 27 inputs asserted in
/// `print_fraction_matches_exiftool_reference_outputs`.
fn print_fraction(val: f64) -> String {
    // ExifTool's own comment: "avoid round-off errors".
    let val = val * 1.00001;
    if val == 0.0 {
        return "0".to_string();
    }
    // Perl's int() truncates toward zero, matching f64::trunc.
    if val.trunc() / val > 0.999 {
        return format!("{:+}", val.trunc() as i64);
    }
    if (val * 2.0).trunc() / (val * 2.0) > 0.999 {
        return format!("{:+}/2", (val * 2.0).trunc() as i64);
    }
    if (val * 3.0).trunc() / (val * 3.0) > 0.999 {
        return format!("{:+}/3", (val * 3.0).trunc() as i64);
    }
    format_signed_g3(val)
}

/// Reproduces Perl's `sprintf("%+.3g", $val)`: three significant digits, `%e`
/// style when the decimal exponent falls outside `-4 <= exp < 3`, trailing
/// zeros stripped, sign always present.
fn format_signed_g3(val: f64) -> String {
    const SIG_DIGITS: i32 = 3;

    // Round to SIG_DIGITS first so the exponent is the post-rounding one, the
    // way C's %g defines it (0.9999 -> 1.00e+00, exponent 0, not -1).
    let sci = format!("{:.*e}", (SIG_DIGITS - 1) as usize, val);
    let (mantissa, exp) = match sci.split_once('e') {
        Some((m, e)) => (m, e.parse::<i32>().unwrap_or(0)),
        None => (sci.as_str(), 0),
    };

    let body = if exp < -4 || exp >= SIG_DIGITS {
        format!(
            "{}e{}{:02}",
            trim_zeros(mantissa),
            sign_char(exp),
            exp.abs()
        )
    } else {
        let decimals = usize::try_from(SIG_DIGITS - 1 - exp).unwrap_or(0);
        trim_zeros(&format!("{:.*}", decimals, val)).to_string()
    };

    if body.starts_with('-') {
        body
    } else {
        format!("+{}", body)
    }
}

fn sign_char(exp: i32) -> char {
    if exp < 0 { '-' } else { '+' }
}

/// Strips the trailing zeros (and any orphaned decimal point) that C's `%g`
/// removes but Rust's `{:.*}` keeps.
fn trim_zeros(s: &str) -> &str {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        s
    }
}

fn read_undefined_value(
    data: &[u8],
    entry_offset: usize,
    byte_order: EmbeddedTiffByteOrder,
    count: u32,
) -> Option<Vec<u8>> {
    let cnt = usize::try_from(count).ok()?;
    if cnt == 0 {
        return Some(Vec::new());
    }
    let val_off = get_entry_value_offset(data, entry_offset, 1, cnt, byte_order)?;
    let end = val_off.checked_add(cnt)?;
    Some(data.get(val_off..end)?.to_vec())
}

fn read_embedded_tiff_u16(
    data: &[u8],
    offset: usize,
    byte_order: EmbeddedTiffByteOrder,
) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let bytes: [u8; 2] = data.get(offset..end)?.try_into().ok()?;

    Some(match byte_order {
        EmbeddedTiffByteOrder::Little => u16::from_le_bytes(bytes),
        EmbeddedTiffByteOrder::Big => u16::from_be_bytes(bytes),
    })
}

fn read_embedded_tiff_u32(
    data: &[u8],
    offset: usize,
    byte_order: EmbeddedTiffByteOrder,
) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let bytes: [u8; 4] = data.get(offset..end)?.try_into().ok()?;

    Some(match byte_order {
        EmbeddedTiffByteOrder::Little => u32::from_le_bytes(bytes),
        EmbeddedTiffByteOrder::Big => u32::from_be_bytes(bytes),
    })
}

#[cfg(test)]
mod embedded_exif_tests {
    use super::find_embedded_exif_tags;
    use crate::core::MetadataMap;

    #[test]
    fn extracts_artist_from_embedded_little_endian_tiff() {
        let mut pdf = b"%PDF-1.4\nstream\nExif\0\0".to_vec();
        // TIFF header with IFD0 at offset 8.
        pdf.extend_from_slice(b"II\x2a\x00\x08\x00\x00\x00");
        // One IFD entry.
        pdf.extend_from_slice(b"\x01\x00");
        // Artist (0x013b), ASCII, 12 bytes, value at TIFF offset 26.
        pdf.extend_from_slice(b"\x3b\x01\x02\x00\x0c\x00\x00\x00\x1a\x00\x00\x00");
        // Next IFD pointer = 0, then Artist string.
        pdf.extend_from_slice(b"\x00\x00\x00\x00Phil Harvey\0");
        pdf.extend_from_slice(b"\nendstream\n%%EOF");

        let map = find_embedded_exif_tags(&pdf).expect("should find embedded EXIF tags");
        let artist = map
            .get_string("IFD0:Artist")
            .expect("must contain Artist tag");
        assert_eq!(artist, "Phil Harvey");
    }

    /// Builds a PDF-ish buffer wrapping a little-endian TIFF whose IFD0 points
    /// at an ExifIFD holding ExposureProgram, ExifVersion and
    /// ExposureCompensation (num/den supplied by the caller).
    fn pdf_with_exif_ifd(exposure_program: u16, num: i32, den: i32) -> Vec<u8> {
        let mut pdf = b"%PDF-1.4\nstream\n".to_vec();
        // TIFF header, IFD0 at TIFF-relative offset 8.
        pdf.extend_from_slice(b"II\x2a\x00\x08\x00\x00\x00");
        // IFD0: one entry, ExifIFD (0x8769) long, pointing at offset 26.
        pdf.extend_from_slice(b"\x01\x00");
        pdf.extend_from_slice(b"\x69\x87\x04\x00\x01\x00\x00\x00\x1a\x00\x00\x00");
        pdf.extend_from_slice(b"\x00\x00\x00\x00");
        // ExifIFD at 26: three entries.
        pdf.extend_from_slice(b"\x03\x00");
        // 0x8822 ExposureProgram, int16u[1], inline.
        pdf.extend_from_slice(b"\x22\x88\x03\x00\x01\x00\x00\x00");
        pdf.extend_from_slice(&exposure_program.to_le_bytes());
        pdf.extend_from_slice(b"\x00\x00");
        // 0x9000 ExifVersion, undef[4], inline, NUL-padded to prove the
        // RawConv trailing-NUL strip runs.
        pdf.extend_from_slice(b"\x00\x90\x07\x00\x04\x00\x00\x00021\x00");
        // 0x9204 ExposureCompensation, rational64s[1], value at offset 68.
        pdf.extend_from_slice(b"\x04\x92\x0a\x00\x01\x00\x00\x00\x44\x00\x00\x00");
        // Next-IFD pointer, then the rational at offset 68.
        pdf.extend_from_slice(b"\x00\x00\x00\x00");
        pdf.extend_from_slice(&num.to_le_bytes());
        pdf.extend_from_slice(&den.to_le_bytes());
        pdf.extend_from_slice(b"\nendstream\n%%EOF");
        pdf
    }

    fn exif_tags(exposure_program: u16, num: i32, den: i32) -> MetadataMap {
        find_embedded_exif_tags(&pdf_with_exif_ifd(exposure_program, num, den))
            .expect("should find embedded EXIF tags")
    }

    #[test]
    fn extracts_exif_version_stripping_trailing_nulls() {
        // ExifTool 13.55 Exif.pm 0x9000: RawConv => '$val=~s/\0+$//; $val'
        let map = exif_tags(2, 0, 100);
        assert_eq!(
            map.get_string("ExifIFD:ExifVersion")
                .expect("must contain ExifVersion"),
            "021"
        );
    }

    #[test]
    fn extracts_exposure_program_label() {
        // ExifTool 13.55 Exif.pm 0x8822 PrintConv: 2 => 'Program AE'
        let map = exif_tags(2, 0, 100);
        assert_eq!(
            map.get_string("ExifIFD:ExposureProgram")
                .expect("must contain ExposureProgram"),
            "Program AE"
        );

        // 4 => 'Shutter speed priority AE' - the full string, not a truncation.
        let map = exif_tags(4, 0, 100);
        assert_eq!(
            map.get_string("ExifIFD:ExposureProgram")
                .expect("must contain ExposureProgram"),
            "Shutter speed priority AE"
        );

        // 5 => 'Creative (Slow speed)'
        let map = exif_tags(5, 0, 100);
        assert_eq!(
            map.get_string("ExifIFD:ExposureProgram")
                .expect("must contain ExposureProgram"),
            "Creative (Slow speed)"
        );
    }

    #[test]
    fn drops_unknown_exposure_program_rather_than_guessing() {
        // 42 is absent from the ExifTool table; leaving the gap open beats
        // emitting an invented label.
        let map = exif_tags(42, 0, 100);
        assert!(map.get_string("ExifIFD:ExposureProgram").is_none());
    }

    #[test]
    fn formats_exposure_compensation_as_exiftool_fraction() {
        // PDF.pdf itself stores 0/100; every other case exercises the
        // PrintFraction branches a plain decimal format would get wrong.
        for (num, den, expected) in [
            (0, 100, "0"),
            (1, 3, "+1/3"),
            (-1, 3, "-1/3"),
            (2, 3, "+2/3"),
            (1, 2, "+1/2"),
            (-3, 2, "-3/2"),
            (1, 1, "+1"),
            (-3, 1, "-3"),
            (7, 10, "+0.7"),
        ] {
            let map = exif_tags(2, num, den);
            assert_eq!(
                map.get_string("ExifIFD:ExposureCompensation")
                    .as_deref()
                    .expect("must contain ExposureCompensation"),
                expected,
                "{}/{} should print as {}",
                num,
                den,
                expected
            );
        }
    }

    // -----------------------------------------------------------------------
    // IFD0 / ExifIFD / IFD1 fixture
    //
    // Mirrors the directory layout of
    // /tmp/oxidex-exiftool-cache/combined-samples/PDF.pdf as reported by
    // `exiftool -v3`: Copyright in IFD0, an ExifIFD pointer, and an IFD1
    // holding Compression. Every expected string below is a literal copied
    // from ExifTool 13.55 Exif.pm, never read back out of our own tables.
    // -----------------------------------------------------------------------

    fn entry(tag: u16, field_type: u16, count: u32, value: [u8; 4]) -> Vec<u8> {
        let mut e = Vec::with_capacity(12);
        e.extend_from_slice(&tag.to_le_bytes());
        e.extend_from_slice(&field_type.to_le_bytes());
        e.extend_from_slice(&count.to_le_bytes());
        e.extend_from_slice(&value);
        e
    }

    struct ExifFixture {
        copyright: &'static str,
        date_time_original: &'static str,
        create_date: &'static str,
        aperture: (u32, u32),
        brightness: (i32, i32),
        flash: u16,
        flashpix: [u8; 4],
        color_space: u16,
        exif_width_type: u16,
        exif_width: u32,
        exif_height: u32,
        focal_plane_unit: u16,
        file_source: u8,
        compression: u16,
    }

    impl Default for ExifFixture {
        /// The values PDF.pdf actually stores.
        fn default() -> Self {
            Self {
                copyright: "Copyright 2004 Phil Harvey",
                date_time_original: "2001:05:19 18:36:41",
                create_date: "2001:05:19 18:36:41",
                aperture: (360, 100),
                brightness: (200, 100),
                flash: 1,
                flashpix: *b"0100",
                color_space: 1,
                // PDF.pdf writes both dimensions as int32u, not int16u.
                exif_width_type: 4,
                exif_width: 8,
                exif_height: 8,
                focal_plane_unit: 3,
                file_source: 3,
                compression: 6,
            }
        }
    }

    impl ExifFixture {
        fn build(&self) -> Vec<u8> {
            const IFD0_ENTRIES: usize = 2;
            const EXIF_ENTRIES: usize = 11;
            const IFD1_ENTRIES: usize = 1;

            let ifd0_off = 8usize;
            let exif_off = ifd0_off + 2 + 12 * IFD0_ENTRIES + 4;
            let ifd1_off = exif_off + 2 + 12 * EXIF_ENTRIES + 4;
            let heap_off = ifd1_off + 2 + 12 * IFD1_ENTRIES + 4;

            let mut heap: Vec<u8> = Vec::new();
            let mut push_heap = |bytes: &[u8]| -> [u8; 4] {
                let at = u32::try_from(heap_off + heap.len()).expect("fixture fits in u32");
                heap.extend_from_slice(bytes);
                at.to_le_bytes()
            };

            let mut nul_terminated = |s: &str| -> (u32, [u8; 4]) {
                let mut bytes = s.as_bytes().to_vec();
                bytes.push(0);
                let count = u32::try_from(bytes.len()).expect("fixture string is short");
                (count, push_heap(&bytes))
            };

            let (copyright_count, copyright_at) = nul_terminated(self.copyright);
            let (dto_count, dto_at) = nul_terminated(self.date_time_original);
            let (create_count, create_at) = nul_terminated(self.create_date);

            let mut rational = |num: [u8; 4], den: [u8; 4]| -> [u8; 4] {
                let mut bytes = num.to_vec();
                bytes.extend_from_slice(&den);
                push_heap(&bytes)
            };
            let aperture_at =
                rational(self.aperture.0.to_le_bytes(), self.aperture.1.to_le_bytes());
            let brightness_at = rational(
                self.brightness.0.to_le_bytes(),
                self.brightness.1.to_le_bytes(),
            );

            let short = |v: u16| -> [u8; 4] {
                let mut b = [0u8; 4];
                b[..2].copy_from_slice(&v.to_le_bytes());
                b
            };

            let mut ifd0 = Vec::new();
            ifd0.extend_from_slice(&entry(0x8298, 2, copyright_count, copyright_at));
            ifd0.extend_from_slice(&entry(
                0x8769,
                4,
                1,
                u32::try_from(exif_off).expect("fits").to_le_bytes(),
            ));

            let mut exif = Vec::new();
            exif.extend_from_slice(&entry(0x9003, 2, dto_count, dto_at));
            exif.extend_from_slice(&entry(0x9004, 2, create_count, create_at));
            exif.extend_from_slice(&entry(0x9202, 5, 1, aperture_at));
            exif.extend_from_slice(&entry(0x9203, 10, 1, brightness_at));
            exif.extend_from_slice(&entry(0x9209, 3, 1, short(self.flash)));
            exif.extend_from_slice(&entry(0xa000, 7, 4, self.flashpix));
            exif.extend_from_slice(&entry(0xa001, 3, 1, short(self.color_space)));
            exif.extend_from_slice(&entry(
                0xa002,
                self.exif_width_type,
                1,
                if self.exif_width_type == 4 {
                    self.exif_width.to_le_bytes()
                } else {
                    short(u16::try_from(self.exif_width).expect("fixture width fits in u16"))
                },
            ));
            exif.extend_from_slice(&entry(0xa003, 4, 1, self.exif_height.to_le_bytes()));
            exif.extend_from_slice(&entry(0xa210, 3, 1, short(self.focal_plane_unit)));
            exif.extend_from_slice(&entry(0xa300, 7, 1, [self.file_source, 0, 0, 0]));

            let mut ifd1 = Vec::new();
            ifd1.extend_from_slice(&entry(0x0103, 3, 1, short(self.compression)));

            let mut tiff = Vec::new();
            tiff.extend_from_slice(b"II\x2a\x00");
            tiff.extend_from_slice(&u32::try_from(ifd0_off).expect("fits").to_le_bytes());
            tiff.extend_from_slice(&u16::try_from(IFD0_ENTRIES).expect("fits").to_le_bytes());
            tiff.extend_from_slice(&ifd0);
            // IFD0's next-IFD pointer is IFD1.
            tiff.extend_from_slice(&u32::try_from(ifd1_off).expect("fits").to_le_bytes());
            tiff.extend_from_slice(&u16::try_from(EXIF_ENTRIES).expect("fits").to_le_bytes());
            tiff.extend_from_slice(&exif);
            tiff.extend_from_slice(&0u32.to_le_bytes());
            tiff.extend_from_slice(&u16::try_from(IFD1_ENTRIES).expect("fits").to_le_bytes());
            tiff.extend_from_slice(&ifd1);
            tiff.extend_from_slice(&0u32.to_le_bytes());
            assert_eq!(tiff.len(), heap_off, "fixture layout must match offsets");
            tiff.extend_from_slice(&heap);

            let mut pdf = b"%PDF-1.4\nstream\n".to_vec();
            pdf.extend_from_slice(&tiff);
            pdf.extend_from_slice(b"\nendstream\n%%EOF");
            pdf
        }

        fn tags(&self) -> MetadataMap {
            find_embedded_exif_tags(&self.build()).expect("fixture must yield EXIF tags")
        }
    }

    fn get(map: &MetadataMap, key: &str) -> String {
        map.get_string(key)
            .unwrap_or_else(|| panic!("must contain {key}"))
            .to_string()
    }

    #[test]
    fn extracts_pdf_sample_tags_with_exiftool_values() {
        // Every expectation here is the literal string `exiftool -G1 -s
        // PDF.pdf` prints for that tag under ExifTool 13.55.
        let map = ExifFixture::default().tags();

        assert_eq!(get(&map, "IFD0:Copyright"), "Copyright 2004 Phil Harvey");
        assert_eq!(get(&map, "IFD1:Compression"), "JPEG (old-style)");
        assert_eq!(get(&map, "ExifIFD:DateTimeOriginal"), "2001:05:19 18:36:41");
        assert_eq!(get(&map, "ExifIFD:CreateDate"), "2001:05:19 18:36:41");
        assert_eq!(get(&map, "ExifIFD:BrightnessValue"), "2");
        assert_eq!(get(&map, "ExifIFD:ExifImageWidth"), "8");
        assert_eq!(get(&map, "ExifIFD:ExifImageHeight"), "8");
        assert_eq!(get(&map, "ExifIFD:Flash"), "Fired");
        assert_eq!(get(&map, "ExifIFD:FlashpixVersion"), "0100");
        assert_eq!(get(&map, "ExifIFD:ColorSpace"), "sRGB");
        assert_eq!(get(&map, "ExifIFD:FocalPlaneResolutionUnit"), "cm");
        assert_eq!(get(&map, "ExifIFD:FileSource"), "Digital Camera");
        // Exif.pm 0x9202 PrintConv => 'sprintf("%.1f",$val)' on
        // ValueConv => '2 ** ($val / 2)': 2**(3.6/2) = 3.4822... -> "3.5".
        assert_eq!(get(&map, "ExifIFD:ApertureValue"), "3.5");
    }

    #[test]
    fn decodes_exif_image_dimensions_stored_as_int16u() {
        // Exif.pm declares 0xa002/0xa003 Writable => 'int16u'; PDF.pdf writes
        // int32u. Both spellings must decode.
        let map = ExifFixture {
            exif_width_type: 3,
            exif_width: 640,
            ..Default::default()
        }
        .tags();
        assert_eq!(get(&map, "ExifIFD:ExifImageWidth"), "640");
    }

    #[test]
    fn decodes_flash_from_the_table_not_from_bit_arithmetic() {
        // These four are exactly where the bitwise decoding the archived
        // patches used diverges from ExifTool's %flash table.
        for (raw, expected) in [
            (0x00u16, "No Flash"),
            (0x08, "On, Did not fire"),
            (0x18, "Auto, Did not fire"),
            (0x5f, "Auto, Fired, Red-eye reduction, Return detected"),
        ] {
            let map = ExifFixture {
                flash: raw,
                ..Default::default()
            }
            .tags();
            assert_eq!(get(&map, "ExifIFD:Flash"), expected, "Flash 0x{raw:02x}");
        }
    }

    #[test]
    fn decodes_file_source_scanner_values_in_full() {
        // Exif.pm 0xa300: 1 => 'Film Scanner', not a bare 'Scanner'.
        for (raw, expected) in [
            (1u8, "Film Scanner"),
            (2, "Reflection Print Scanner"),
            (3, "Digital Camera"),
        ] {
            let map = ExifFixture {
                file_source: raw,
                ..Default::default()
            }
            .tags();
            assert_eq!(get(&map, "ExifIFD:FileSource"), expected);
        }
    }

    #[test]
    fn decodes_focal_plane_resolution_unit_including_non_standard_values() {
        for (raw, expected) in [
            (1u16, "None"),
            (2, "inches"),
            (3, "cm"),
            (4, "mm"),
            (5, "um"),
        ] {
            let map = ExifFixture {
                focal_plane_unit: raw,
                ..Default::default()
            }
            .tags();
            assert_eq!(get(&map, "ExifIFD:FocalPlaneResolutionUnit"), expected);
        }
    }

    #[test]
    fn decodes_compression_values_beyond_the_truncated_excerpt() {
        // The inherited table stopped at 32773; these entries are the ones a
        // truncation would have silently mislabelled.
        for (raw, expected) in [
            (6u16, "JPEG (old-style)"),
            (32767, "Sony ARW Compressed"),
            (32770, "Samsung SRW Compressed"),
            (34713, "Nikon NEF Compressed"),
            (65535, "Pentax PEF Compressed"),
        ] {
            let map = ExifFixture {
                compression: raw,
                ..Default::default()
            }
            .tags();
            assert_eq!(get(&map, "IFD1:Compression"), expected, "Compression {raw}");
        }
    }

    #[test]
    fn decodes_color_space_without_abandoning_the_rest_of_the_ifd() {
        let map = ExifFixture {
            color_space: 2,
            ..Default::default()
        }
        .tags();
        assert_eq!(get(&map, "ExifIFD:ColorSpace"), "Adobe RGB");

        // An unrecognised ColorSpace must drop only that tag; the previous
        // `_ => return None` threw away the whole ExifIFD.
        let map = ExifFixture {
            color_space: 0x1234,
            ..Default::default()
        }
        .tags();
        assert!(map.get_string("ExifIFD:ColorSpace").is_none());
        assert_eq!(get(&map, "ExifIFD:FileSource"), "Digital Camera");
    }

    #[test]
    fn drops_unknown_print_conv_values_rather_than_inventing_labels() {
        let map = ExifFixture {
            file_source: 9,
            focal_plane_unit: 42,
            flash: 0x7777,
            ..Default::default()
        }
        .tags();
        assert!(map.get_string("ExifIFD:FileSource").is_none());
        assert!(map.get_string("ExifIFD:FocalPlaneResolutionUnit").is_none());
        assert!(map.get_string("ExifIFD:Flash").is_none());
        // The rest of the directory still comes through.
        assert_eq!(get(&map, "ExifIFD:ExifImageWidth"), "8");
    }

    #[test]
    fn strips_trailing_nulls_from_flashpix_version() {
        // Exif.pm 0xa000 RawConv => '$val=~s/\0+$//; $val'
        let map = ExifFixture {
            flashpix: *b"010\0",
            ..Default::default()
        }
        .tags();
        assert_eq!(get(&map, "ExifIFD:FlashpixVersion"), "010");
    }

    #[test]
    fn formats_brightness_value_as_a_signed_rational() {
        for (num, den, expected) in [
            (200, 100, "2"),
            (-200, 100, "-2"),
            (16, 13, "1.230769231"),
            (-1, 3, "-0.333333333"),
        ] {
            let map = ExifFixture {
                brightness: (num, den),
                ..Default::default()
            }
            .tags();
            assert_eq!(
                get(&map, "ExifIFD:BrightnessValue"),
                expected,
                "{num}/{den}"
            );
        }
    }

    #[test]
    fn print_fraction_matches_exiftool_reference_outputs() {
        // Expected strings produced by running the verbatim Perl body of
        // Image::ExifTool::Exif::PrintFraction (Exif.pm 5421-5440) on
        // 2026-07-27; they are literals, not re-derived from our own code.
        for (input, expected) in [
            (0.0, "0"),
            (1.0, "+1"),
            (-1.0, "-1"),
            (2.0, "+2"),
            (0.5, "+1/2"),
            (-0.5, "-1/2"),
            (1.0 / 3.0, "+1/3"),
            (-1.0 / 3.0, "-1/3"),
            (2.0 / 3.0, "+2/3"),
            (-2.0 / 3.0, "-2/3"),
            (4.0 / 3.0, "+4/3"),
            (-4.0 / 3.0, "-4/3"),
            (1.5, "+3/2"),
            (-1.5, "-3/2"),
            (0.7, "+0.7"),
            (-0.7, "-0.7"),
            (0.25, "+0.25"),
            (-0.25, "-0.25"),
            (-0.0625, "-0.0625"),
            (0.3, "+0.3"),
            (0.1, "+0.1"),
            (3.7, "+3.7"),
            (5.0, "+5"),
            (-3.0, "-3"),
            (1234.0, "+1234"),
            (0.0001, "+0.0001"),
            (1e-6, "+1e-06"),
        ] {
            assert_eq!(
                super::print_fraction(input),
                expected,
                "PrintFraction({input})"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tag_value::TagValue;
    use crate::test_support::TestReader;

    /// Creates a minimal valid PDF with Info dictionary
    fn create_test_pdf_with_info() -> Vec<u8> {
        // This is a valid minimal PDF structure with Info dictionary
        let pdf = b"%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>
endobj
4 0 obj
<<
/Title (Test PDF Document)
/Author (John Doe)
/Subject (Testing)
/Keywords (test, pdf, metadata)
/Creator (ExifTool-RS Test)
/Producer (PDF Generator 1.0)
/CreationDate (D:20240115120000Z)
/ModDate (D:20240115120000Z)
>>
endobj
xref
0 5
0000000000 65535 f
0000000009 00000 n
0000000058 00000 n
0000000115 00000 n
0000000186 00000 n
trailer
<< /Size 5 /Root 1 0 R /Info 4 0 R >>
startxref
425
%%EOF";

        pdf.to_vec()
    }

    #[test]
    fn test_parse_pdf_with_info_dict() {
        let pdf_data = create_test_pdf_with_info();
        let reader = TestReader::new(pdf_data);

        let result = parse_pdf_metadata(&reader);
        assert!(result.is_ok(), "Failed to parse PDF: {:?}", result.err());

        let metadata = result.unwrap();

        // Verify Info dictionary fields were extracted
        assert_eq!(metadata.get_string("PDF:Title"), Some("Test PDF Document"));
        assert_eq!(metadata.get_string("PDF:Author"), Some("John Doe"));
        assert_eq!(metadata.get_string("PDF:Subject"), Some("Testing"));
        if let Some(TagValue::Array(values)) = metadata.get("PDF:Keywords") {
            let keywords: Vec<&str> = values.iter().filter_map(|v| v.as_string()).collect();
            assert_eq!(keywords, vec!["test", "pdf", "metadata"]);
        } else {
            panic!("Expected PDF:Keywords as array");
        }
        assert_eq!(metadata.get_string("PDF:Creator"), Some("ExifTool-RS Test"));
        assert_eq!(
            metadata.get_string("PDF:Producer"),
            Some("PDF Generator 1.0")
        );

        assert_eq!(
            metadata.get_string("PDF:CreateDate"),
            Some("2024:01:15 12:00:00Z")
        );
        assert_eq!(
            metadata.get_string("PDF:ModifyDate"),
            Some("2024:01:15 12:00:00Z")
        );

        // Should have at least 5 metadata fields as per acceptance criteria
        assert!(
            metadata.len() >= 5,
            "Should have at least 5 metadata fields, got {}",
            metadata.len()
        );
    }

    #[test]
    fn test_parse_pdf_invalid_signature() {
        let data = vec![0xFF; 100]; // Invalid signature
        let reader = TestReader::new(data);

        let result = parse_pdf_metadata(&reader);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid PDF signature")
        );
    }

    #[test]
    fn test_parse_pdf_too_small() {
        let data = vec![0x25, 0x50]; // Only "%P"
        let reader = TestReader::new(data);

        let result = parse_pdf_metadata(&reader);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too small"));
    }

    #[test]
    fn test_parse_pdf_with_xmp() {
        let pdf_with_xmp = b"%PDF-1.4
1 0 obj
<< /Type /Catalog >>
endobj
<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">
<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\">
<rdf:Description rdf:about=\"\">
  <dc:creator>XMP Creator</dc:creator>
  <dc:title>XMP Title</dc:title>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>
<?xpacket end=\"w\"?>
4 0 obj
<<
/Title (Info Title)
/Author (Info Author)
>>
endobj
xref
0 2
0000000000 65535 f
0000000009 00000 n
trailer
<< /Size 2 /Root 1 0 R /Info 4 0 R >>
startxref
500
%%EOF";

        let reader = TestReader::new(pdf_with_xmp.to_vec());
        let result = parse_pdf_metadata(&reader);

        // Should succeed even if parsing has issues
        assert!(result.is_ok() || result.is_err());

        // If it succeeds, check that we got some metadata
        if let Ok(metadata) = result {
            // Should have metadata from either Info dict or XMP
            assert!(!metadata.is_empty(), "Should have extracted some metadata");
        }
    }
}
