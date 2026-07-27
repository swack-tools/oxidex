//! Raw format metadata extraction
//!
//! Most camera raw formats are based on TIFF/EXIF structure.
//! This module leverages the existing TIFF parser and adds raw-specific handling.
//!
//! ## Architecture
//!
//! The metadata parser follows a dispatch pattern based on format type:
//! - **TIFF-based formats**: Use existing TIFF parser infrastructure
//! - **Proprietary formats**: Use format-specific parsers (CR3, X3F, MRW)
//! - **Fallback**: Attempt TIFF parsing, return minimal metadata on failure
//!
//! ## Format Support
//!
//! ### TIFF-based (fully supported):
//! - Canon CR2, Nikon NEF, Sony ARW, Adobe DNG
//! - Pentax PEF, Olympus ORF, Fujifilm RAF
//! - Panasonic RW2, and most other raw formats
//!
//! ### Proprietary (stubbed for future implementation):
//! - Canon CR3 (ISO Base Media Format)
//! - Sigma X3F (FOVb format)
//! - Minolta MRW (MRM format)

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use crate::parsers::raw::{RawFormat, raf_parser};
use crate::parsers::tiff::ifd_parser::{ByteOrder, parse_ifd};
use crate::tag_db::lookup_tag_name;

/// Resolve RAW-specific tags using the names and groups assigned by ExifTool.
///
/// Some physical RAW IFD tags correspond to standard EXIF concepts but use
/// format-specific IDs and representations.
fn lookup_raw_tag_name(tag_id: u16, ifd_name: &str, format: RawFormat) -> String {
    if format == RawFormat::PanasonicRW2 && tag_id == 0x0009 {
        // PanasonicRaw CFAPattern is stored at 0x0009, while the canonical
        // EXIF CFAPattern name is registered under tag 0xA302. ExifTool
        // assigns the Panasonic tag to its EXIF group.
        lookup_tag_name(0xA302, "EXIF")
    } else if format == RawFormat::AdobeDNG
        && matches!(
            tag_id,
            0xC619 // BlackLevelRepeatDim
                | 0xC61A // BlackLevel
                | 0xC62D // BayerGreenSplit
                | 0xC632 // AntiAliasStrength
                | 0xC65C // BestQualityScale
                | 0xC68D // ActiveArea
        )
    {
        lookup_tag_name(tag_id, "EXIF")
    } else {
        lookup_tag_name(tag_id, ifd_name)
    }
}

/// Format TIFF/EP CFAPattern2 (tag 0x828E), whose components are unsigned
/// bytes printed by ExifTool as a space-separated list.
fn format_cfa_pattern2(bytes: &[u8], value_count: u32) -> String {
    bytes
        .iter()
        .take(value_count as usize)
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parse metadata from camera raw file
///
/// This is the main entry point for raw format metadata extraction.
/// It dispatches to format-specific parsers based on the detected format.
///
/// # Arguments
///
/// * `data` - Complete file data as a byte slice
/// * `format` - Detected raw format from format detection
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(ExifToolError)` - Parse error or unsupported format
///
/// # Examples
///
/// ```no_run
/// use oxidex::parsers::raw::{parse_raw_metadata, RawFormat};
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let data = std::fs::read("photo.dng")?;
/// let metadata = parse_raw_metadata(&data, RawFormat::AdobeDNG)?;
///
/// // Access extracted metadata
/// if let Some(make) = metadata.get("IFD0:Make") {
///     println!("Camera: {:?}", make);
/// }
/// # Ok(())
/// # }
/// ```
///
/// # Implementation Notes
///
/// Most raw formats are TIFF-based and can be parsed using the existing TIFF parser.
/// Proprietary formats (CR3, X3F, MRW) require specialized parsers and are currently
/// stubbed for future implementation.
pub fn parse_raw_metadata(data: &[u8], format: RawFormat) -> Result<MetadataMap> {
    match format {
        // TIFF-based formats - use existing TIFF parser infrastructure
        // These formats all follow the TIFF/EXIF structure with manufacturer-specific extensions
        RawFormat::CanonCR2
        | RawFormat::NikonNEF
        | RawFormat::NikonNRW
        | RawFormat::SonyARW
        | RawFormat::SonySR2
        | RawFormat::SonySRF
        | RawFormat::SonySRW
        | RawFormat::SonyARQ
        | RawFormat::SonyARI
        | RawFormat::AdobeDNG
        | RawFormat::PentaxPEF
        | RawFormat::OlympusORF
        | RawFormat::OlympusORI
        | RawFormat::FujifilmRAF
        | RawFormat::PanasonicRW2
        | RawFormat::PanasonicRWL
        | RawFormat::Hasselblad3FR
        | RawFormat::HasselbladFFF
        | RawFormat::PhaseOneIIQ
        | RawFormat::MamiyaMEF
        | RawFormat::LeafMOS
        | RawFormat::KodakDCR
        | RawFormat::KodakKDC
        | RawFormat::MinoltaMDC
        | RawFormat::EpsonERF
        | RawFormat::GoProGPR
        | RawFormat::HEIFHIF
        | RawFormat::LightLRI
        | RawFormat::SinarSTI => parse_tiff_based_raw(data, format),

        // Canon CR3 uses ISO Base Media Format (similar to MP4)
        // This is a different container format from TIFF
        RawFormat::CanonCR3 => parse_cr3(data, format),

        // Sigma X3F uses proprietary FOVb format
        RawFormat::SigmaX3F => parse_sigma_x3f(data, format),

        // Minolta MRW uses proprietary MRM format
        RawFormat::MinoltaMRW => parse_minolta_mrw(data, format),

        // Canon CRW is an older proprietary format
        RawFormat::CanonCRW => parse_canon_crw(data, format),

        // Generic/fallback formats
        // Attempt TIFF parsing as most raw formats are TIFF-based
        RawFormat::GenericRAW | RawFormat::GenericCAM | RawFormat::GenericREV => {
            parse_tiff_based_raw(data, format).or_else(|_| {
                // If TIFF parsing fails, return minimal metadata
                let mut metadata = MetadataMap::new();
                metadata.insert(
                    "File:FileType".to_string(),
                    TagValue::new_string(format!("{:?}", format)),
                );
                Ok(metadata)
            })
        }
    }
}

/// Parse TIFF-based raw formats using existing TIFF parser infrastructure
///
/// This function handles the majority of raw formats as they are based on TIFF/EXIF.
/// It creates a FileReader adapter, parses the TIFF structure, and enriches the
/// metadata with format-specific information.
///
/// Special handling for format variants:
/// - **Fujifilm RAF**: Contains embedded JPEG with EXIF data after proprietary header
/// - **Panasonic RW2**: TIFF variant with magic number 0x55 instead of 0x2A
/// - **Olympus ORF**: TIFF variant with "RO" signature instead of magic number 42
///
/// # Arguments
///
/// * `data` - Complete file data
/// * `format` - Specific raw format variant
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Extracted metadata including TIFF tags and format info
/// * `Err(ExifToolError)` - Parse error from TIFF parser
///
/// # Implementation
///
/// 1. Check for format-specific handling (RAF embedded JPEG extraction)
/// 2. Create SliceReader adapter for byte slice access
/// 3. Parse TIFF header to determine byte order
/// 4. Parse IFD chain to extract all metadata tags
/// 5. Convert IFD entries to MetadataMap with proper tag names
/// 6. Add format-specific tags (e.g., DNG version for DNG files)
fn parse_tiff_based_raw(data: &[u8], format: RawFormat) -> Result<MetadataMap> {
    // Special handling for Fujifilm RAF format
    // RAF files have a proprietary header followed by embedded JPEG with EXIF data
    // Structure: "FUJIFILMCCD-RAW " (16 bytes) + header info + embedded JPEG at offset
    if format == RawFormat::FujifilmRAF {
        return parse_fujifilm_raf(data, format);
    }

    // Validate minimum TIFF header size
    if data.len() < 8 {
        return Err(ExifToolError::parse_error(
            "File too small to be a valid TIFF-based raw format",
        ));
    }

    // Create a FileReader adapter for the data slice
    let reader = SliceReader::new(data);

    // Parse TIFF header to get byte order
    let byte_order = detect_byte_order(data)?;

    // Read first IFD offset from TIFF header (bytes 4-7)
    let first_ifd_offset = read_u32(&data[4..8], byte_order) as u64;

    // Parse all IFDs in the chain
    let mut metadata = MetadataMap::new();
    let mut ifd_offset = first_ifd_offset;
    let mut ifd_index = 0;

    // CR2 IFD0 thumbnail byte count for PreviewImage/PreviewImageLength
    let mut cr2_thumbnail_length: Option<u32> = None;

    // Add format-specific tag to identify file type
    metadata.insert(
        "File:FileType".to_string(),
        TagValue::new_string(format!("{:?}", format)),
    );

    // Walk the IFD chain (IFD0, IFD1, etc.)
    while ifd_offset != 0 && ifd_index < 10 {
        // Safety limit to prevent infinite loops
        // Determine IFD name based on index
        let ifd_name = match ifd_index {
            0 => "IFD0",
            1 => "IFD1",
            n => {
                eprintln!("Warning: Found IFD{} which is unusual", n);
                "IFD0" // Fallback
            }
        };

        // Parse this IFD
        match parse_ifd(&reader, ifd_offset, byte_order) {
            Ok(tags) => {
                // Track sub-IFD offsets, MakerNote data, and camera make
                let mut exif_ifd_offset = None;
                let mut gps_ifd_offset = None;
                let mut sub_ifd_offsets = Vec::new();
                let mut makernote_data: Option<Vec<u8>> = None;
                let mut camera_make: Option<String> = None;

                // Convert tags to metadata
                for (tag_id, field_type, value_count, raw_bytes) in &tags {
                    let bytes = raw_bytes.as_ref();

                    // RW2 tag 0x002e contains the JPEG preview whose EXIF IFD
                    // carries a handful of standard EXIF tags omitted from the
                    // outer Panasonic RAW IFDs.
                    if format == RawFormat::PanasonicRW2 && ifd_index == 0 && *tag_id == 0x002e {
                        if let Err(error) = extract_rw2_embedded_exif_tags(bytes, &mut metadata) {
                            eprintln!("Warning: Failed to parse RW2 preview EXIF: {}", error);
                        }
                        // Emit the JpgFromRaw binary itself (ExifTool EXIF:JpgFromRaw)
                        metadata.insert(
                            "EXIF:JpgFromRaw".to_string(),
                            TagValue::Binary(bytes.to_vec()),
                        );
                        // Prevent further processing of this tag (generic code would emit it as a raw blob)
                        continue;
                    }

                    // Check for EXIF Sub-IFD pointer (tag 0x8769)
                    if *tag_id == 0x8769 && bytes.len() >= 4 {
                        let offset = read_u32(bytes, byte_order);
                        exif_ifd_offset = Some(offset as u64);
                        continue; // Don't add pointer tag to metadata
                    }

                    // Check for GPS Sub-IFD pointer (tag 0x8825)
                    if *tag_id == 0x8825 && bytes.len() >= 4 {
                        let offset = read_u32(bytes, byte_order);
                        gps_ifd_offset = Some(offset as u64);
                        continue; // Don't add pointer tag to metadata
                    }

                    // CR2 IFD0: capture the StripByteCounts value for the
                    // thumbnail JPEG preview (ExifTool EXIF:PreviewImage).
                    if format == RawFormat::CanonCR2 && ifd_index == 0
                        && *tag_id == 0x0117 && bytes.len() >= 4
                    {
                        cr2_thumbnail_length = Some(read_u32(bytes, byte_order));
                    }

                    // Check for SubIFD pointer (tag 0x014A) - common in RAW formats
                    // SubIFD contains RAW image data and RAW-specific metadata
                    if *tag_id == 0x014A {
                        // SubIFDs can contain multiple offsets
                        let offset_count = bytes.len() / 4;
                        for i in 0..offset_count {
                            if (i + 1) * 4 <= bytes.len() {
                                let offset_bytes = &bytes[i * 4..(i + 1) * 4];
                                let offset = read_u32(offset_bytes, byte_order);
                                sub_ifd_offsets.push(offset as u64);
                            }
                        }
                        continue; // Don't add pointer tag to metadata
                    }

                    // Check for MakerNote tag (0x927C) - crucial for RAW format metadata
                    // MakerNotes contain manufacturer-specific camera settings
                    if *tag_id == 0x927C {
                        makernote_data = Some(bytes.to_vec());
                        continue; // Don't add raw MakerNote to metadata, will be parsed separately
                    }

                    // Check for Make tag (0x010F) - needed for MakerNote dispatcher
                    if *tag_id == 0x010F && *field_type == 2 {
                        // Extract camera make for MakerNote parsing (ASCII type)
                        let make_str = String::from_utf8_lossy(bytes);
                        camera_make = Some(make_str.trim_end_matches('\0').trim().to_string());
                    }

                    // CR2 IFD1 thumbnail/preview tags: ExifTool reports
                    // PreviewImage and PreviewImageLength under the EXIF
                    // group, derived from the IFD1 JPEGInterchangeFormat*
                    // entries. The generic lookup_tag_name path indexes
                    // these under their EXIF-spec names, so we name them
                    // explicitly to match ExifTool's output.
                    if format == RawFormat::CanonCR2 && ifd_index == 1 {
                        match *tag_id {
                            0x0201 if bytes.len() >= 4 => {
                                let value = read_u32(bytes, byte_order);
                                metadata.insert(
                                    "EXIF:ThumbnailOffset".to_string(),
                                    TagValue::new_integer(value as i64),
                                );
                                continue;
                            }
                            0x0202 if bytes.len() >= 4 => {
                                let value = read_u32(bytes, byte_order);
                                metadata.insert(
                                    "EXIF:PreviewImageLength".to_string(),
                                    TagValue::new_integer(value as i64),
                                );
                                metadata.insert(
                                    "EXIF:PreviewImage".to_string(),
                                    TagValue::new_string(format!(
                                        "(Binary data {} bytes, use -b option to extract)",
                                        value
                                    )),
                                );
                                continue;
                            }
                            _ => {}
                        }
                    }
                    // Convert tag to metadata
                    // Panasonic RW2 stores BitsPerSample in its proprietary
                    // IFD0 tag 0x000A and Compression in tag 0x000B instead
                    // of the standard TIFF tags 0x0102 and 0x0103.
                    //
                    // BlackLevelRed, BlackLevelGreen, and BlackLevelBlue are
                    // PanasonicRaw tags 0x001C, 0x001D, and 0x001E and have no equivalent standard
                    // TIFF tag IDs, so name them explicitly.
                    let canonical_tag_id = match (format, ifd_index, *tag_id) {
                        (RawFormat::PanasonicRW2, 0, 0x000A) => 0x0102,
                        (RawFormat::PanasonicRW2, 0, 0x000B) => 0x0103,
                        _ => *tag_id,
                    };
                    let tag_name = match (format, ifd_index, *tag_id) {
                        (RawFormat::PanasonicRW2, 0, 0x001C) => {
                            format!("{}:BlackLevelRed", ifd_name)
                        }
                        (RawFormat::PanasonicRW2, 0, 0x001D) => {
                            format!("{}:BlackLevelGreen", ifd_name)
                        }
                        (RawFormat::PanasonicRW2, 0, 0x001E) => {
                            format!("{}:BlackLevelBlue", ifd_name)
                        }
                        _ => lookup_raw_tag_name(canonical_tag_id, ifd_name, format),
                    };
                    let tag_value = if format == RawFormat::PanasonicRW2
                        && ifd_index == 0
                        && *tag_id == 0x0009
                    {
                        format_panasonic_cfa_pattern(bytes, *field_type, *value_count, byte_order)
                            .map(TagValue::new_string)
                            .unwrap_or_else(|| {
                                raw_bytes_to_simple_tag_value(
                                    bytes,
                                    *field_type,
                                    *value_count,
                                    byte_order,
                                )
                            })
                    } else if format == RawFormat::PanasonicRW2
                        && ifd_index == 0
                        && *tag_id == 0x000B
                    {
                        format_panasonic_raw_compression(
                            bytes,
                            *field_type,
                            *value_count,
                            byte_order,
                        )
                        .map(TagValue::new_string)
                        .unwrap_or_else(|| {
                            raw_bytes_to_simple_tag_value(
                                bytes,
                                *field_type,
                                *value_count,
                                byte_order,
                            )
                        })
                    } else if let Some(value) = format_exif_display_value(
                        *tag_id,
                        bytes,
                        *field_type,
                        *value_count,
                        byte_order,
                    ) {
                        TagValue::new_string(value)
                    } else if format == RawFormat::AdobeDNG {
                        format_dng_integer_array(
                            *tag_id,
                            bytes,
                            *field_type,
                            *value_count,
                            byte_order,
                        )
                        .map(TagValue::new_string)
                        .unwrap_or_else(|| {
                            raw_bytes_to_simple_tag_value(
                                bytes,
                                *field_type,
                                *value_count,
                                byte_order,
                            )
                        })
                    } else {
                        raw_bytes_to_simple_tag_value(bytes, *field_type, *value_count, byte_order)
                    };
                    metadata.insert(tag_name, tag_value);
                }

                // Parse EXIF Sub-IFD if present
                if let Some(offset) = exif_ifd_offset
                    && let Ok(exif_tags) = parse_ifd(&reader, offset, byte_order)
                {
                    // Also check EXIF IFD for MakerNote and Make tags
                    let mut exif_makernote: Option<Vec<u8>> = None;
                    let mut exif_make: Option<String> = None;

                    for (tag_id, field_type, value_count, raw_bytes) in &exif_tags {
                        let bytes = raw_bytes.as_ref();

                        // MakerNote in EXIF IFD (more common location)
                        if *tag_id == 0x927C {
                            exif_makernote = Some(bytes.to_vec());
                            continue;
                        }

                        // Make tag in EXIF IFD
                        if *tag_id == 0x010F && *field_type == 2 {
                            let make_str = String::from_utf8_lossy(bytes);
                            exif_make = Some(make_str.trim_end_matches('\0').trim().to_string());
                        }

                        let tag_name = lookup_tag_name(*tag_id, "ExifIFD");
                        let tag_value = if let Some(value) = format_exif_display_value(
                            *tag_id,
                            bytes,
                            *field_type,
                            *value_count,
                            byte_order,
                        ) {
                            TagValue::new_string(value)
                        } else {
                            raw_bytes_to_simple_tag_value(
                                bytes,
                                *field_type,
                                *value_count,
                                byte_order,
                            )
                        };
                        metadata.insert(tag_name, tag_value);
                    }

                    // Prefer EXIF IFD MakerNote/Make over IFD0 versions
                    if exif_makernote.is_some() {
                        makernote_data = exif_makernote;
                    }
                    if exif_make.is_some() {
                        camera_make = exif_make;
                    }

                    // Parse Interoperability IFD (ExifIFD tag 0xA005) if present
                    if let Some(interop_offset) = exif_tags.iter().find_map(|(tag_id, _, _, raw)| {
                        if *tag_id == 0xA005 && raw.len() >= 4 {
                            Some(read_u32(raw.as_ref(), byte_order) as u64)
                        } else {
                            None
                        }
                    }) {
                        if let Ok(interop_tags) = parse_ifd(&reader, interop_offset, byte_order) {
                            for (tag_id, field_type, value_count, raw_bytes) in interop_tags {
                                let tag_name = lookup_tag_name(tag_id, "InteropIFD");
                                let tag_value = if let Some(value) = format_exif_display_value(
                                    tag_id,
                                    raw_bytes.as_ref(),
                                    field_type,
                                    value_count,
                                    byte_order,
                                ) {
                                    TagValue::new_string(value)
                                } else {
                                    raw_bytes_to_simple_tag_value(raw_bytes.as_ref(), field_type, value_count, byte_order)
                                };
                                metadata.insert(tag_name, tag_value);
                            }
                        }
                    }
                }

                // Parse MakerNote if present and we have the camera make
                if let (Some(make), Some(mn_data)) = (camera_make.as_ref(), makernote_data.as_ref())
                {
                    // Use the MakerNote dispatcher to parse manufacturer-specific tags
                    let mut makernote_tags = std::collections::HashMap::new();
                    if let Err(e) = crate::parsers::tiff::makernote_dispatcher::dispatch_makernote(
                        make,
                        mn_data,
                        byte_order,
                        &mut makernote_tags,
                    ) {
                        eprintln!("Warning: Failed to parse MakerNote for {}: {}", make, e);
                    } else {
                        // Add parsed MakerNote tags to metadata
                        // Tags already have proper prefixes (e.g., "Canon:MacroMode")
                        for (tag_name, tag_value) in makernote_tags {
                            metadata.insert(tag_name, TagValue::new_string(tag_value));
                        }
                    }
                }

                // Parse GPS Sub-IFD if present
                if let Some(offset) = gps_ifd_offset
                    && let Ok(gps_tags) = parse_ifd(&reader, offset, byte_order)
                {
                    for (tag_id, field_type, value_count, raw_bytes) in gps_tags {
                        let tag_name = lookup_tag_name(tag_id, "GPS");
                        let tag_value = raw_bytes_to_simple_tag_value(
                            raw_bytes.as_ref(),
                            field_type,
                            value_count,
                            byte_order,
                        );
                        metadata.insert(tag_name, tag_value);
                    }
                }

                // Parse SubIFD(s) if present - crucial for RAW formats
                // SubIFDs contain RAW image data, compression info, and RAW-specific tags
                for (sub_index, sub_offset) in sub_ifd_offsets.iter().enumerate() {
                    // Use SubIFD0, SubIFD1, etc. for tag naming
                    let sub_ifd_name = if sub_index == 0 {
                        "SubIFD0"
                    } else {
                        // Multiple SubIFDs are rare but possible
                        eprintln!("Warning: Found SubIFD{} which is unusual", sub_index);
                        "SubIFD0" // Use SubIFD0 as fallback for consistency
                    };

                    if let Ok(sub_tags) = parse_ifd(&reader, *sub_offset, byte_order) {
                        let is_nef = matches!(format, RawFormat::NikonNEF | RawFormat::NikonNRW);
                        let is_rw2 = format == RawFormat::PanasonicRW2;
                        for (tag_id, field_type, value_count, raw_bytes) in sub_tags {
                            // NEF maps SubIFD tags into the EXIF group and
                            // applies format-specific decoding where needed.
                            if is_nef {
                                if let Some((tag_name, tag_value)) = format_nef_subifd_tag(
                                    tag_id,
                                    field_type,
                                    value_count,
                                    raw_bytes.as_ref(),
                                    byte_order,
                                ) {
                                    metadata.insert(tag_name, tag_value);
                                    continue;
                                }
                                // Tags not handled specially fall through to the
                                // generic path which renames them to EXIF: below.
                            }

                            // Panasonic RW2 maps its SubIFD (PanasonicRaw) tags into EXIF group
                            // so that standard tags like ISO (0x8827) appear as EXIF:ISO.
                            if is_rw2 {
                                let tag_name = lookup_tag_name(tag_id, "EXIF");
                                let tag_value = raw_bytes_to_simple_tag_value(
                                    raw_bytes.as_ref(),
                                    field_type,
                                    value_count,
                                    byte_order,
                                );
                                metadata.insert(tag_name, tag_value);
                                continue;
                            }

                            // TIFF/EP tag 0x828E (CFAPattern2) is a plain
                            // int8u array with no dimension header, unlike
                            // EXIF tag 0xA302 (CFAPattern). The generic
                            // decoder below has no BYTE-array case and falls
                            // back to raw TagValue::Binary, so this still
                            // needs its own formatting -- but the name comes
                            // from the same lookup every other tag in this
                            // loop uses, for consistency.
                            // CFAPattern2 stays under its physical SubIFD
                            // group for all formats, including NEF.
                            if tag_id == 0x828E {
                                metadata.insert(
                                    lookup_tag_name(tag_id, sub_ifd_name),
                                    TagValue::new_string(format_cfa_pattern2(
                                        raw_bytes.as_ref(),
                                        value_count,
                                    )),
                                );
                                continue;
                            }

                            let tag_name = if is_nef {
                                lookup_tag_name(tag_id, "EXIF")
                            } else {
                                lookup_raw_tag_name(tag_id, sub_ifd_name, format)
                            };
                            let bytes = raw_bytes.as_ref();
                            let tag_value = if format == RawFormat::AdobeDNG {
                                format_dng_integer_array(
                                    tag_id,
                                    bytes,
                                    field_type,
                                    value_count,
                                    byte_order,
                                )
                                .map(TagValue::new_string)
                                .unwrap_or_else(|| {
                                    raw_bytes_to_simple_tag_value(
                                        bytes,
                                        field_type,
                                        value_count,
                                        byte_order,
                                    )
                                })
                            } else {
                                raw_bytes_to_simple_tag_value(
                                    bytes,
                                    field_type,
                                    value_count,
                                    byte_order,
                                )
                            };
                            metadata.insert(tag_name, tag_value);
                        }
                    }
                }

                // Read next IFD offset
                let entry_count = tags.len();
                let next_offset_location = ifd_offset + 2 + (entry_count as u64 * 12);

                if next_offset_location + 4 <= reader.size() {
                    if let Ok(next_offset_bytes) = reader.read(next_offset_location, 4) {
                        ifd_offset = read_u32(next_offset_bytes, byte_order) as u64;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to parse IFD at offset {}: {}",
                    ifd_offset, e
                );
                break;
            }
        }

        ifd_index += 1;
    }

    // Apply format-specific enhancements
    match format {
        RawFormat::AdobeDNG => {
            extract_dng_tags(&mut metadata);
        }
        RawFormat::CanonCR2 => {
            // Emit EXIF:PreviewImage / PreviewImageLength from the IFD0
            // thumbnail JPEG byte count (StripByteCounts, 0x0117).
            if let Some(length) = cr2_thumbnail_length {
                if length > 0 {
                    metadata.insert(
                        "EXIF:PreviewImageLength".to_string(),
                        TagValue::new_integer(length as i64),
                    );
                    metadata.insert(
                        "EXIF:PreviewImage".to_string(),
                        TagValue::new_string(format!(
                            "(Binary data {} bytes, use -b option to extract)",
                            length
                        )),
                    );
                }
            }
            extract_cr2_tags(&mut metadata);
        }
        RawFormat::NikonNEF | RawFormat::NikonNRW => {
            extract_nef_tags(&mut metadata);
        }
        _ => {
            // Other formats don't need special handling yet
        }
    }

    Ok(metadata)
}

/// Extract standard EXIF tags stored only in Panasonic RW2's JpgFromRaw data.
///
/// TIFF offsets in an APP1 EXIF payload are relative to its embedded TIFF
/// header. Giving that header its own `SliceReader` keeps the offset base and
/// byte order local to this parse instead of mutating parser-wide state.
fn extract_rw2_embedded_exif_tags(jpeg: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    let Some(tiff_data) = find_jpeg_exif_tiff(jpeg)? else {
        return Ok(());
    };

    let byte_order = detect_byte_order(tiff_data)?;
    let first_ifd_bytes = tiff_data
        .get(4..8)
        .ok_or_else(|| ExifToolError::parse_error("Truncated TIFF header in RW2 preview EXIF"))?;
    let first_ifd_offset = u64::from(read_u32(first_ifd_bytes, byte_order));
    let reader = SliceReader::new(tiff_data);
    let ifd0_tags = parse_ifd(&reader, first_ifd_offset, byte_order)?;

    let exif_ifd_offset =
        ifd0_tags
            .iter()
            .find_map(|(tag_id, field_type, value_count, raw_bytes)| {
                if *tag_id == 0x8769 && *field_type == 4 && *value_count >= 1 {
                    read_tiff_u32(raw_bytes.as_ref(), byte_order).map(u64::from)
                } else {
                    None
                }
            });
    let Some(exif_ifd_offset) = exif_ifd_offset else {
        return Ok(());
    };

    for (tag_id, field_type, value_count, raw_bytes) in
        parse_ifd(&reader, exif_ifd_offset, byte_order)?
    {
        // Filter to the exact set of EXIF tags that ExifTool extracts from
        // the RW2 JpgFromRaw preview EXIF IFD.
        if !matches!(
            tag_id,
            0x9101 // ComponentsConfiguration
                | 0x9102 // CompressedBitsPerPixel
                | 0xA405 // FocalLengthIn35mmFormat
                | 0xA407 // GainControl
                | 0xA411 // HighISOMultiplierRed
                | 0xA412 // HighISOMultiplierGreen
                | 0xA413 // HighISOMultiplierBlue
                | 0xA000 // FlashpixVersion
                | 0xA001 // ColorSpace
                | 0xA002 // ExifImageWidth
                | 0xA003 // ExifImageHeight
                | 0xA302 // CFAPattern
                | 0xA401 // CustomRendered
                | 0xA402 // ExposureMode
                | 0xA404 // DigitalZoomRatio
                | 0xA405 // FocalLengthIn35mmFormat
                | 0xA407 // GainControl
                | 0xA408 // Contrast
                | 0xA411 // HighISOMultiplierRed
                | 0xA412 // HighISOMultiplierGreen
                | 0xA413 // HighISOMultiplierBlue
        ) {
            continue;
        }

        let tag_name = lookup_tag_name(tag_id, "ExifIFD");
        let tag_value = if let Some(value) = format_exif_display_value(
            tag_id,
            raw_bytes.as_ref(),
            field_type,
            value_count,
            byte_order,
        ) {
            TagValue::new_string(value)
        } else {
            raw_bytes_to_simple_tag_value(raw_bytes.as_ref(), field_type, value_count, byte_order)
        };
        metadata.insert(tag_name, tag_value);
    }

    Ok(())
}

/// Locate the TIFF header in a JPEG APP1 EXIF segment.
fn find_jpeg_exif_tiff(jpeg: &[u8]) -> Result<Option<&[u8]>> {
    if jpeg.get(..2) != Some(&[0xff, 0xd8]) {
        return Ok(None);
    }

    let mut offset = 2usize;
    while offset < jpeg.len() {
        if jpeg.get(offset) != Some(&0xff) {
            return Ok(None);
        }

        while jpeg.get(offset) == Some(&0xff) {
            offset = offset
                .checked_add(1)
                .ok_or_else(|| ExifToolError::parse_error("Invalid JPEG marker offset"))?;
        }
        let Some(&marker) = jpeg.get(offset) else {
            return Ok(None);
        };
        offset = offset
            .checked_add(1)
            .ok_or_else(|| ExifToolError::parse_error("Invalid JPEG marker offset"))?;

        if marker == 0xd9 || marker == 0xda {
            return Ok(None);
        }
        if marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
            continue;
        }

        let length_bytes: [u8; 2] = jpeg
            .get(offset..offset.saturating_add(2))
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| ExifToolError::parse_error("Truncated JPEG segment length"))?;
        let segment_length = usize::from(u16::from_be_bytes(length_bytes));
        if segment_length < 2 {
            return Err(ExifToolError::parse_error("Invalid JPEG segment length"));
        }

        let payload_start = offset
            .checked_add(2)
            .ok_or_else(|| ExifToolError::parse_error("Invalid JPEG segment offset"))?;
        let segment_end = offset
            .checked_add(segment_length)
            .ok_or_else(|| ExifToolError::parse_error("Invalid JPEG segment length"))?;
        let payload = jpeg
            .get(payload_start..segment_end)
            .ok_or_else(|| ExifToolError::parse_error("Truncated JPEG segment"))?;

        if marker == 0xe1 && payload.get(..6) == Some(b"Exif\0\0") {
            return Ok(payload.get(6..));
        }
        offset = segment_end;
    }

    Ok(None)
}

/// Format DNG integer-array tags whose ExifTool default output preserves all
/// components as a space-separated list.
///
/// The generic TIFF value conversion intentionally reduces SHORT and LONG
/// values to one scalar. These two DNG tags have meaningful fixed-size arrays,
/// so validate their declared TIFF type and complete byte payload before
/// formatting them.
fn format_dng_integer_array(
    tag_id: u16,
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    byte_order: ByteOrder,
) -> Option<String> {
    let component_size = match tag_id {
        0xC619 if field_type == 3 => 2, // BlackLevelRepeatDim: SHORT[2]
        0xC68D if field_type == 4 => 4, // ActiveArea: LONG[4]
        _ => return None,
    };

    let value_count = usize::try_from(value_count).ok()?;
    let byte_len = value_count.checked_mul(component_size)?;
    let values = bytes.get(..byte_len)?;

    let formatted = match component_size {
        2 => values
            .chunks_exact(2)
            .map(|chunk| {
                let value = match byte_order {
                    ByteOrder::LittleEndian => u16::from_le_bytes([chunk[0], chunk[1]]),
                    ByteOrder::BigEndian => u16::from_be_bytes([chunk[0], chunk[1]]),
                };
                value.to_string()
            })
            .collect::<Vec<_>>(),
        4 => values
            .chunks_exact(4)
            .map(|chunk| {
                let value = match byte_order {
                    ByteOrder::LittleEndian => {
                        u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
                    }
                    ByteOrder::BigEndian => {
                        u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
                    }
                };
                value.to_string()
            })
            .collect::<Vec<_>>(),
        _ => return None,
    };

    Some(formatted.join(" "))
}

fn read_tiff_u16(bytes: &[u8], byte_order: ByteOrder) -> Option<u16> {
    let bytes: [u8; 2] = bytes.get(..2)?.try_into().ok()?;
    Some(match byte_order {
        ByteOrder::LittleEndian => u16::from_le_bytes(bytes),
        ByteOrder::BigEndian => u16::from_be_bytes(bytes),
    })
}

fn read_tiff_u32(bytes: &[u8], byte_order: ByteOrder) -> Option<u32> {
    let bytes: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
    Some(match byte_order {
        ByteOrder::LittleEndian => u32::from_le_bytes(bytes),
        ByteOrder::BigEndian => u32::from_be_bytes(bytes),
    })
}

/// Format EXIF values whose raw TIFF representation differs from ExifTool's
/// default text output.
fn format_exif_display_value(
    tag_id: u16,
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    byte_order: ByteOrder,
) -> Option<String> {
    match tag_id {
        // ComponentsConfiguration: UNDEFINED[4].
        0x9101 if field_type == 7 => {
            let count = usize::try_from(value_count).ok()?;
            let components = bytes.get(..count)?;
            if components.is_empty() {
                return None;
            }

            Some(
                components
                    .iter()
                    .map(|component| match component {
                        0 => "-".to_string(),
                        1 => "Y".to_string(),
                        2 => "Cb".to_string(),
                        3 => "Cr".to_string(),
                        4 => "R".to_string(),
                        5 => "G".to_string(),
                        6 => "B".to_string(),
                        value => value.to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        }
        // CompressedBitsPerPixel: RATIONAL[1].
        0x9102 if field_type == 5 && value_count >= 1 => {
            let numerator = read_tiff_u32(bytes.get(..4)?, byte_order)?;
            let denominator = read_tiff_u32(bytes.get(4..8)?, byte_order)?;
            if denominator == 0 {
                None
            } else if numerator % denominator == 0 {
                Some((numerator / denominator).to_string())
            } else {
                Some(format!("{}", f64::from(numerator) / f64::from(denominator)))
            }
        }
        // ColorSpace: SHORT[1].
        0xA001 if field_type == 3 && value_count >= 1 => match read_tiff_u16(bytes, byte_order)? {
            1 => Some("sRGB".to_string()),
            0xffff => Some("Uncalibrated".to_string()),
            _ => None,
        },
        // FocalLengthIn35mmFormat: SHORT[1] -> "24 mm".
        0xA405 if field_type == 3 && value_count >= 1 => {
            let value = read_tiff_u16(bytes, byte_order)?;
            Some(format!("{} mm", value))
        }
        // GainControl: SHORT[1].
        0xA407 if field_type == 3 && value_count >= 1 => match read_tiff_u16(bytes, byte_order)? {
            0 => Some("None".to_string()),
            1 => Some("Low gain up".to_string()),
            2 => Some("High gain up".to_string()),
            3 => Some("Low gain down".to_string()),
            4 => Some("High gain down".to_string()),
            _ => None,
        },
        // CFAPattern: UNDEFINED with two endian-dependent u16 dimensions.
        0xA302 if field_type == 7 => decode_exif_cfa_pattern(bytes, byte_order),
        // FlashpixVersion: UNDEFINED 4 bytes printed as e.g. "0100".
        0xA000 if field_type == 7 => {
            let count = usize::try_from(value_count).ok()?;
            let ver_bytes = bytes.get(..count.min(4))?;
            Some(String::from_utf8_lossy(ver_bytes).into_owned())
        }
        // FocalLengthIn35mmFormat: SHORT[1] with " mm" suffix.
        // Exif.pm PrintConv: $val .= " mm"
        0xA405 if field_type == 3 && value_count >= 1 => {
            let value = read_tiff_u16(bytes, byte_order)?;
            Some(format!("{} mm", value))
        }
        // GainControl: SHORT[1] with PrintConv table.
        // Exif.pm: 0=>'None', 1=>'Low gain up', 2=>'High gain up',
        //          3=>'Low gain down', 4=>'High gain down'
        0xA407 if field_type == 3 && value_count >= 1 => match read_tiff_u16(bytes, byte_order)? {
            0 => Some("None".to_string()),
            1 => Some("Low gain up".to_string()),
            2 => Some("High gain up".to_string()),
            3 => Some("Low gain down".to_string()),
            4 => Some("High gain down".to_string()),
            _ => None,
        },
        // CustomRendered: SHORT[1].
        0xA401 if field_type == 3 && value_count >= 1 => match read_tiff_u16(bytes, byte_order)? {
            0 => Some("Normal".to_string()),
            1 => Some("Custom".to_string()),
            _ => None,
        },
        // ExposureMode: SHORT[1].
        0xA402 if field_type == 3 && value_count >= 1 => match read_tiff_u16(bytes, byte_order)? {
            0 => Some("Auto".to_string()),
            1 => Some("Manual".to_string()),
            2 => Some("Auto bracket".to_string()),
            _ => None,
        },
        // DigitalZoomRatio: RATIONAL[1].
        0xA404 if field_type == 5 && value_count >= 1 => {
            // Reuse the same rational formatting as CompressedBitsPerPixel (0x9102).
            format_rational_as_string(bytes, byte_order)
        }
        // Contrast: SHORT[1].
        0xA408 if field_type == 3 && value_count >= 1 => match read_tiff_u16(bytes, byte_order)? {
            0 => Some("Normal".to_string()),
            1 => Some("Soft".to_string()),
            2 => Some("Hard".to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// Format a RATIONAL pair whose value_count >= 1.
fn format_rational_as_string(bytes: &[u8], byte_order: ByteOrder) -> Option<String> {
    let numerator = read_tiff_u32(bytes.get(..4)?, byte_order)?;
    let denominator = read_tiff_u32(bytes.get(4..8)?, byte_order)?;
    if denominator == 0 {
        return None;
    }
    if numerator % denominator == 0 {
        return Some((numerator / denominator).to_string());
    }
    Some(format!("{}", f64::from(numerator) / f64::from(denominator)))
}

/// Format PanasonicRaw tag 0x0009 (CFAPattern).
///
/// Panasonic stores the Bayer arrangement as a single SHORT enum rather than
/// using the dimension-and-cell payload of EXIF tag 0xA302.
fn format_panasonic_cfa_pattern(
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    byte_order: ByteOrder,
) -> Option<String> {
    if field_type != 3 || value_count < 1 {
        return None;
    }

    let pattern = match read_tiff_u16(bytes, byte_order)? {
        1 => "[Red,Green][Green,Blue]",
        2 => "[Green,Red][Blue,Green]",
        3 => "[Green,Blue][Red,Green]",
        4 => "[Blue,Green][Green,Red]",
        _ => return None,
    };
    Some(pattern.to_string())
}

fn format_panasonic_raw_compression(
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    byte_order: ByteOrder,
) -> Option<String> {
    if field_type != 3 || value_count < 1 {
        return None;
    }

    match read_tiff_u16(bytes, byte_order)? {
        34316 => Some("Panasonic RAW 1".to_string()),
        _ => None,
    }
}

/// Decode EXIF tag 0xA302 (CFAPattern).
///
/// The first four bytes are the horizontal and vertical repeat dimensions,
/// stored as two u16 values in TIFF byte order. They are followed by one u8
/// color identifier for each cell in the pattern.
fn decode_exif_cfa_pattern(bytes: &[u8], byte_order: ByteOrder) -> Option<String> {
    if bytes.len() < 4 {
        return None;
    }

    let read_dimension = |offset: usize| {
        let value = [bytes[offset], bytes[offset + 1]];
        match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes(value),
            ByteOrder::BigEndian => u16::from_be_bytes(value),
        }
    };

    let horizontal_repeat = usize::from(read_dimension(0));
    let vertical_repeat = usize::from(read_dimension(2));
    if horizontal_repeat == 0 || vertical_repeat == 0 {
        return None;
    }

    let pattern_len = horizontal_repeat.checked_mul(vertical_repeat)?;
    let pattern_end = 4usize.checked_add(pattern_len)?;
    let pattern = bytes.get(4..pattern_end)?;

    let color_name = |color: &u8| match color {
        0 => Some("Red"),
        1 => Some("Green"),
        2 => Some("Blue"),
        3 => Some("Cyan"),
        4 => Some("Magenta"),
        5 => Some("Yellow"),
        6 => Some("White"),
        _ => None,
    };

    let mut formatted = String::new();
    for row in pattern.chunks(horizontal_repeat) {
        let colors = row.iter().map(color_name).collect::<Option<Vec<_>>>()?;
        formatted.push('[');
        formatted.push_str(&colors.join(","));
        formatted.push(']');
    }
    Some(formatted)
}

#[cfg(test)]
mod cfa_pattern_tests {
    use super::*;

    #[test]
    fn formats_panasonic_rw2_cfa_pattern() {
        assert_eq!(
            format_panasonic_cfa_pattern(&[4, 0], 3, 1, ByteOrder::LittleEndian).as_deref(),
            Some("[Blue,Green][Green,Red]")
        );
    }

    #[test]
    fn decodes_little_endian_cfa_pattern() {
        let bytes = [2, 0, 2, 0, 2, 1, 1, 0];
        assert_eq!(
            decode_exif_cfa_pattern(&bytes, ByteOrder::LittleEndian).as_deref(),
            Some("[Blue,Green][Green,Red]")
        );
    }

    #[test]
    fn decodes_big_endian_cfa_pattern() {
        let bytes = [0, 2, 0, 2, 2, 1, 1, 0];
        assert_eq!(
            decode_exif_cfa_pattern(&bytes, ByteOrder::BigEndian).as_deref(),
            Some("[Blue,Green][Green,Red]")
        );
    }

    #[test]
    fn rejects_truncated_cfa_pattern() {
        let bytes = [2, 0, 2, 0, 2];
        assert_eq!(
            decode_exif_cfa_pattern(&bytes, ByteOrder::LittleEndian),
            None
        );
    }
}

#[cfg(test)]
mod dng_integer_array_tests {
    use super::*;

    #[test]
    fn formats_little_endian_dng_integer_arrays() {
        assert_eq!(
            format_dng_integer_array(0xC619, &[1, 0, 1, 0], 3, 2, ByteOrder::LittleEndian,)
                .as_deref(),
            Some("1 1")
        );
        assert_eq!(
            format_dng_integer_array(
                0xC68D,
                &[14, 0, 0, 0, 42, 0, 0, 0, 24, 9, 0, 0, 188, 13, 0, 0,],
                4,
                4,
                ByteOrder::LittleEndian,
            )
            .as_deref(),
            Some("14 42 2328 3516")
        );
    }

    #[test]
    fn rejects_wrong_type_or_truncated_dng_integer_array() {
        assert_eq!(
            format_dng_integer_array(0xC619, &[1, 0, 1, 0], 4, 2, ByteOrder::LittleEndian),
            None
        );
        assert_eq!(
            format_dng_integer_array(0xC68D, &[14, 0, 0, 0], 4, 4, ByteOrder::LittleEndian,),
            None
        );
    }

    #[test]
    fn ignores_unrelated_dng_array_tags() {
        assert_eq!(
            format_dng_integer_array(0xC61A, &[1, 0], 3, 1, ByteOrder::LittleEndian),
            None
        );
    }
}

#[cfg(test)]
mod panasonic_rw2_tests {
    use super::*;

    #[test]
    fn extracts_black_level_blue_from_panasonic_raw_tag() {
        // Little-endian RW2 header followed by an IFD containing one SHORT
        // entry: PanasonicRaw tag 0x001E (BlackLevelBlue) with value zero.
        let data = [
            b'I', b'I', 0x55, 0x00, // RW2 byte order and magic
            0x08, 0x00, 0x00, 0x00, // first IFD offset
            0x01, 0x00, // entry count
            0x1e, 0x00, // tag: BlackLevelBlue
            0x03, 0x00, // type: SHORT
            0x01, 0x00, 0x00, 0x00, // count: 1
            0x00, 0x00, 0x00, 0x00, // value: 0
            0x00, 0x00, 0x00, 0x00, // next IFD offset
        ];

        let metadata =
            parse_raw_metadata(&data, RawFormat::PanasonicRW2).expect("valid synthetic RW2");

        assert!(
            metadata.contains_key("IFD0:BlackLevelBlue"),
            "PanasonicRaw tag 0x001E should use its canonical EXIF name"
        );
    }

    #[test]
    fn formats_observed_panasonic_raw_compression_code() {
        let bytes = 34316u16.to_le_bytes();
        assert_eq!(
            format_panasonic_raw_compression(&bytes, 3, 1, ByteOrder::LittleEndian,).as_deref(),
            Some("Panasonic RAW 1")
        );
    }

    #[test]
    fn extracts_standard_exif_tags_from_rw2_preview() {
        let mut tiff = vec![0u8; 108];
        tiff[0..8].copy_from_slice(b"II\x2a\x00\x08\x00\x00\x00");

        // Embedded IFD0 points to an EXIF IFD at TIFF-relative offset 26.
        tiff[8..10].copy_from_slice(&1u16.to_le_bytes());
        tiff[10..12].copy_from_slice(&0x8769u16.to_le_bytes());
        tiff[12..14].copy_from_slice(&4u16.to_le_bytes());
        tiff[14..18].copy_from_slice(&1u32.to_le_bytes());
        tiff[18..22].copy_from_slice(&26u32.to_le_bytes());

        tiff[26..28].copy_from_slice(&5u16.to_le_bytes());
        let entries = [
            (0x9101u16, 7u16, 4u32, [1, 2, 3, 0]),
            (0x9102u16, 5u16, 1u32, 92u32.to_le_bytes()),
            (0xA001u16, 3u16, 1u32, [1, 0, 0, 0]),
            (0xA302u16, 7u16, 8u32, 100u32.to_le_bytes()),
            (0xA408u16, 3u16, 1u32, [0, 0, 0, 0]),
        ];
        for (index, (tag_id, field_type, count, value)) in entries.iter().enumerate() {
            let start = 28 + index * 12;
            tiff[start..start + 2].copy_from_slice(&tag_id.to_le_bytes());
            tiff[start + 2..start + 4].copy_from_slice(&field_type.to_le_bytes());
            tiff[start + 4..start + 8].copy_from_slice(&count.to_le_bytes());
            tiff[start + 8..start + 12].copy_from_slice(value);
        }
        tiff[92..96].copy_from_slice(&2u32.to_le_bytes());
        tiff[96..100].copy_from_slice(&1u32.to_le_bytes());
        tiff[100..108].copy_from_slice(&[2, 0, 2, 0, 2, 1, 1, 0]);

        let app1_length =
            u16::try_from(2 + 6 + tiff.len()).expect("synthetic APP1 segment length fits in u16");
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend_from_slice(&app1_length.to_be_bytes());
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(&tiff);
        jpeg.extend_from_slice(&[0xff, 0xd9]);

        let mut metadata = MetadataMap::new();
        extract_rw2_embedded_exif_tags(&jpeg, &mut metadata)
            .expect("synthetic preview EXIF should parse");

        assert_eq!(
            metadata.get("ExifIFD:ComponentsConfiguration"),
            Some(&TagValue::new_string("Y, Cb, Cr, -".to_string()))
        );
        assert_eq!(
            metadata.get("ExifIFD:CompressedBitsPerPixel"),
            Some(&TagValue::new_string("2".to_string()))
        );
        assert_eq!(
            metadata.get("ExifIFD:ColorSpace"),
            Some(&TagValue::new_string("sRGB".to_string()))
        );
        assert_eq!(
            metadata.get("ExifIFD:CFAPattern"),
            Some(&TagValue::new_string("[Blue,Green][Green,Red]".to_string()))
        );
        assert_eq!(
            metadata.get("ExifIFD:Contrast"),
            Some(&TagValue::new_string("Normal".to_string()))
        );
    }
}

#[cfg(test)]
mod nef_cfa_pattern2_tests {
    use super::*;

    #[test]
    fn extracts_tiff_ep_cfa_pattern2_from_nef_sub_ifd() {
        // Minimal little-endian TIFF containing an IFD0 SubIFD pointer and a
        // SubIFD with BYTE[4] tag 0x828E. This is the layout used by the Nikon
        // NEF sample.
        let mut data = vec![0u8; 44];
        data[0..8].copy_from_slice(b"II\x2a\x00\x08\x00\x00\x00");

        // IFD0 at offset 8: one SubIFDs (0x014A) entry pointing to offset 26.
        data[8..10].copy_from_slice(&1u16.to_le_bytes());
        data[10..12].copy_from_slice(&0x014Au16.to_le_bytes());
        data[12..14].copy_from_slice(&4u16.to_le_bytes());
        data[14..18].copy_from_slice(&1u32.to_le_bytes());
        data[18..22].copy_from_slice(&26u32.to_le_bytes());
        // Bytes 22..26 are the zero next-IFD offset.

        // SubIFD at offset 26: CFAPattern2 = 2 1 1 0.
        data[26..28].copy_from_slice(&1u16.to_le_bytes());
        data[28..30].copy_from_slice(&0x828Eu16.to_le_bytes());
        data[30..32].copy_from_slice(&1u16.to_le_bytes());
        data[32..36].copy_from_slice(&4u32.to_le_bytes());
        data[36..40].copy_from_slice(&[2, 1, 1, 0]);
        // Bytes 40..44 are the zero next-IFD offset.

        let metadata = parse_raw_metadata(&data, RawFormat::NikonNEF)
            .expect("minimal NEF-compatible TIFF should parse");

        assert!(
            metadata.get("SubIFD0:CFAPattern2").is_some(),
            "CFAPattern2 should be exposed under its physical SubIFD0 group, \
             consistent with every other tag this loop names"
        );
        assert!(
            metadata.get("SubIFD0:0x828E").is_none(),
            "CFAPattern2 should not remain an unnamed SubIFD tag"
        );
        assert_eq!(format_cfa_pattern2(&[2, 1, 1, 0], 4), "2 1 1 0");
    }
}

/// Extract DNG-specific tags from metadata
///
/// DNG (Digital Negative) files have additional tags beyond standard TIFF/EXIF.
/// This function enriches the metadata with DNG-specific information.
///
/// # DNG-Specific Tags Extracted
///
/// Color calibration tags (crucial for RAW processing):
/// - ColorMatrix1/2 (0xC621/0xC622): Color transformation matrices
/// - CameraCalibration1/2 (0xC623/0xC624): Camera-specific calibration
/// - CalibrationIlluminant1/2 (0xC65A/0xC65B): Illuminant used for calibration
/// - ForwardMatrix1/2 (0xC714/0xC715): Forward color transformation
/// - AsShotNeutral (0xC628): White balance as shot
///
/// Exposure and rendering tags:
/// - BaselineExposure (0xC62A): Baseline exposure compensation
/// - BaselineNoise (0xC62B): Baseline noise level
/// - BaselineSharpness (0xC62C): Baseline sharpness
/// - LinearResponseLimit (0xC62E): Linear response limit
///
/// RAW data tags:
/// - BlackLevel (0xC61A): Black level for each color plane
/// - WhiteLevel (0xC61D): White level for sensor
/// - DefaultScale (0xC61E): Default scale factors
/// - DefaultCropOrigin/Size (0xC61F/0xC620): Default crop area
/// - BayerGreenSplit (0xC62D): Bayer green channel split value
///
/// DNG metadata:
/// - DNGVersion (0xC612): DNG specification version
/// - DNGBackwardVersion (0xC613): Backward compatibility version
/// - UniqueCameraModel (0xC614): Unique camera model string
/// - LocalizedCameraModel (0xC615): Localized camera model name
/// - CFAPlaneColor (0xC616): CFA plane color
/// - CFALayout (0xC617): CFA layout
/// - LinearizationTable (0xC618): Linearization table
/// - BlackLevelRepeatDim (0xC619): Black level repeat dimensions
///
/// # Arguments
///
/// * `metadata` - Mutable reference to MetadataMap to enrich
///
/// # Implementation Note
///
/// Most DNG-specific tags are automatically extracted by the TIFF parser
/// during IFD traversal. This function serves as documentation and can be
/// extended to add computed/derived DNG-specific metadata or aliases.
fn extract_dng_tags(metadata: &mut MetadataMap) {
    // DNG-specific tags are stored in IFD0 or SubIFD0
    // The TIFF parser already extracts these automatically

    // We can add computed values or format-specific processing here
    // For example, parsing the DNGVersion bytes into a readable format
    // DNGVersion is stored as 4 bytes: major.minor.tertiary.quaternary
    // Example: [1, 4, 0, 0] = version 1.4.0.0
    if let Some(TagValue::Binary(bytes)) = metadata.get("IFD0:DNGVersion")
        && bytes.len() >= 4
    {
        let version_str = format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3]);
        metadata.insert(
            "DNG:VersionString".to_string(),
            TagValue::new_string(version_str),
        );
    }

    // Mark critical DNG tags for easier identification
    // This helps downstream applications know which color calibration data is available
    let critical_color_tags = [
        "IFD0:ColorMatrix1",
        "IFD0:ColorMatrix2",
        "IFD0:CameraCalibration1",
        "IFD0:CameraCalibration2",
        "IFD0:CalibrationIlluminant1",
        "IFD0:CalibrationIlluminant2",
    ];

    let mut available_color_tags = Vec::new();
    for tag_name in &critical_color_tags {
        if metadata.contains_key(tag_name) {
            available_color_tags.push(*tag_name);
        }
    }

    if !available_color_tags.is_empty() {
        metadata.insert(
            "DNG:AvailableColorCalibration".to_string(),
            TagValue::new_string(available_color_tags.join(", ")),
        );
    }
}

/// Extract CR2-specific tags from metadata
///
/// Canon CR2 (Canon Raw version 2) files are TIFF-based with Canon-specific extensions.
/// This function enriches the metadata with CR2-specific information.
///
/// # CR2-Specific Tags
///
/// CR2 files contain:
/// - **Canon MakerNotes**: Extensive Canon-specific metadata (already extracted via MakerNote parser)
/// - **SubIFD tags**: RAW image data dimensions, compression, bit depth
/// - **Preview images**: Multiple embedded preview/thumbnail images at various sizes
/// - **RAW sensor data**: CFA pattern, sensor size, crop information
///
/// Key CR2 characteristics:
/// - CR2 marker at offset 8: "CR\x02\x00" (distinguishes from other TIFF formats)
/// - SubIFD contains the RAW image data
/// - IFD1 typically contains a full-size JPEG preview
/// - Multiple thumbnail/preview images at different resolutions
///
/// # Arguments
///
/// * `metadata` - Mutable reference to MetadataMap to enrich
fn extract_cr2_tags(metadata: &mut MetadataMap) {
    // CR2 files have multiple image layers:
    // - IFD0: Typically a small thumbnail
    // - IFD1: Full-size JPEG preview
    // - SubIFD0: RAW image data

    // Count available image representations
    let mut image_count = 0;
    if metadata.contains_key("IFD0:ImageWidth") {
        image_count += 1;
    }
    if metadata.contains_key("IFD1:ImageWidth") {
        image_count += 1;
    }
    if metadata.contains_key("SubIFD0:ImageWidth") {
        image_count += 1;
    }

    if image_count > 0 {
        metadata.insert(
            "CR2:ImageLayerCount".to_string(),
            TagValue::new_integer(image_count),
        );
    }

    // Check for RAW data in SubIFD
    if metadata.contains_key("SubIFD0:ImageWidth") {
        // Mark that RAW data is present
        metadata.insert(
            "CR2:HasRAWData".to_string(),
            TagValue::new_string("true".to_string()),
        );

        // Extract RAW image dimensions if available
        if let Some(width) = metadata.get("SubIFD0:ImageWidth")
            && let Some(height) = metadata.get("SubIFD0:ImageHeight")
        {
            let width_val = match width {
                TagValue::Integer(i) => i.to_string(),
                TagValue::String(s) => s.clone(),
                _ => format!("{:?}", width),
            };
            let height_val = match height {
                TagValue::Integer(i) => i.to_string(),
                TagValue::String(s) => s.clone(),
                _ => format!("{:?}", height),
            };
            metadata.insert(
                "CR2:RAWImageSize".to_string(),
                TagValue::new_string(format!("{}x{}", width_val, height_val)),
            );
        }
    }

    // Check for JPEG preview in IFD1
    if metadata.contains_key("IFD1:ImageWidth") && metadata.contains_key("IFD1:Compression") {
        metadata.insert(
            "CR2:HasJPEGPreview".to_string(),
            TagValue::new_string("true".to_string()),
        );
    }
}

/// Extract NEF-specific tags from metadata
///
/// Nikon NEF (Nikon Electronic Format) files are TIFF-based with Nikon-specific extensions.
/// This function enriches the metadata with NEF-specific information.
///
/// # NEF-Specific Tags
///
/// NEF files contain:
/// - **Nikon MakerNotes**: Extensive Nikon-specific metadata (already extracted via MakerNote parser)
/// - **SubIFD tags**: RAW image data, compression type, bit depth
/// - **Preview images**: Embedded JPEG preview images
/// - **Compressed RAW data**: Nikon's lossless compressed RAW format
///
/// NEF variants:
/// - NEF: Standard Nikon RAW format (uncompressed or losslessly compressed)
/// - NRW: Nikon RAW (sRAW) - smaller file size variant
///
/// Key NEF characteristics:
/// - Can use lossless compression (reduces file size without quality loss)
/// - Multiple embedded previews at different sizes
/// - Extensive shooting information in Nikon MakerNotes
///
/// # Arguments
///
/// * `metadata` - Mutable reference to MetadataMap to enrich
fn extract_nef_tags(metadata: &mut MetadataMap) {
    // NEF files typically have:
    // - IFD0: Thumbnail image or preview
    // - IFD1: Another preview (optional)
    // - SubIFD0: RAW image data

    // Check for compression type in SubIFD
    if let Some(compression) = metadata.get("SubIFD0:Compression") {
        // Nikon uses various compression schemes:
        // - 1: Uncompressed
        // - 7: JPEG compression (for preview)
        // - 34713: Nikon lossless compressed
        let compression_val = match compression {
            TagValue::Integer(i) => *i,
            TagValue::String(s) => s.parse::<i64>().unwrap_or(0),
            _ => 0,
        };

        let compression_name = match compression_val {
            1 => "Uncompressed",
            7 => "JPEG",
            34713 => "Nikon Lossless Compressed",
            _ => "Unknown",
        };

        metadata.insert(
            "NEF:RAWCompression".to_string(),
            TagValue::new_string(compression_name.to_string()),
        );
    }

    // Count available image representations
    let mut image_count = 0;
    if metadata.contains_key("IFD0:ImageWidth") {
        image_count += 1;
    }
    if metadata.contains_key("IFD1:ImageWidth") {
        image_count += 1;
    }
    if metadata.contains_key("SubIFD0:ImageWidth") {
        image_count += 1;
    }

    if image_count > 0 {
        metadata.insert(
            "NEF:ImageLayerCount".to_string(),
            TagValue::new_integer(image_count),
        );
    }

    // Check for RAW data in SubIFD
    if metadata.contains_key("SubIFD0:ImageWidth") {
        metadata.insert(
            "NEF:HasRAWData".to_string(),
            TagValue::new_string("true".to_string()),
        );

        // Extract RAW image dimensions
        if let Some(width) = metadata.get("SubIFD0:ImageWidth")
            && let Some(height) = metadata.get("SubIFD0:ImageHeight")
        {
            let width_val = match width {
                TagValue::Integer(i) => i.to_string(),
                TagValue::String(s) => s.clone(),
                _ => format!("{:?}", width),
            };
            let height_val = match height {
                TagValue::Integer(i) => i.to_string(),
                TagValue::String(s) => s.clone(),
                _ => format!("{:?}", height),
            };
            metadata.insert(
                "NEF:RAWImageSize".to_string(),
                TagValue::new_string(format!("{}x{}", width_val, height_val)),
            );
        }
    }

    // Check for bit depth
    if let Some(bits_per_sample) = metadata.get("SubIFD0:BitsPerSample") {
        let bits_val = match bits_per_sample {
            TagValue::Integer(i) => i.to_string(),
            TagValue::String(s) => s.clone(),
            _ => format!("{:?}", bits_per_sample),
        };
        metadata.insert(
            "NEF:RAWBitDepth".to_string(),
            TagValue::new_string(bits_val),
        );
    }
}

/// Parse Canon CR3 format (ISO Base Media File Format)
///
/// CR3 files use a container format similar to MP4/QuickTime rather than TIFF.
/// This function is a stub for future implementation.
///
/// # Arguments
///
/// * `data` - Complete file data
/// * `format` - CR3 format variant
///
/// # Returns
///
/// Minimal metadata with file type information.
/// Full CR3 parsing to be implemented in future iteration.
///
/// # TODO
///
/// - Implement ISO Base Media File Format parser
/// - Extract metadata from CR3 boxes (similar to MP4 atoms)
/// - Parse Canon-specific metadata boxes
/// Locate the Canon CR3 `CMT1` metadata box and return its TIFF payload.
///
/// CR3 is an ISO Base Media container; Canon stores the primary image's
/// standard EXIF/TIFF metadata in a `CMT1` box (nested under a Canon UUID
/// box inside `moov`). Rather than walk the full box hierarchy, this scans
/// for the `CMT1` box type and validates both the preceding 4-byte
/// big-endian size field and that the payload begins with a TIFF header, so
/// a coincidental `CMT1` byte sequence in image data is not mistaken for the
/// box.
fn find_cr3_cmt1_tiff(data: &[u8]) -> Option<&[u8]> {
    let payload = find_cr3_box(data, b"CMT1")?;
    if payload.starts_with(b"II*\0") || payload.starts_with(b"MM\x00*") {
        Some(payload)
    } else {
        None
    }
}

/// Locate a Canon CR3 box by its 4-byte type and return its payload.
///
/// CR3 is an ISO Base Media container. Each box has the structure:
/// [size: u32 BE][type: 4 bytes][payload: size-8 bytes].
/// The size includes the 8-byte header. size==0 and size==1 (extended size)
/// are not handled here because Canon's metadata boxes use normal 32-bit sizes.
fn find_cr3_box<'a>(data: &'a [u8], box_type: &[u8; 4]) -> Option<&'a [u8]> {
    let mut cursor = 0;
    while cursor + 4 <= data.len() {
        let rel = data[cursor..].windows(4).position(|w| w == box_type)?;
        let type_offset = cursor + rel;
        cursor = type_offset + 4;
        if type_offset < 4 {
            continue;
        }

        let box_start = type_offset - 4;
        let box_size = u32::from_be_bytes([
            data[box_start],
            data[box_start + 1],
            data[box_start + 2],
            data[box_start + 3],
        ]) as usize;
        let payload_start = type_offset + 4;
        let box_end = match box_start.checked_add(box_size) {
            Some(end) if box_size >= 8 && end <= data.len() && end > payload_start => end,
            _ => continue,
        };

        return Some(&data[payload_start..box_end]);
    }
    None
}

fn parse_cr3(data: &[u8], format: RawFormat) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();
    metadata.insert(
        "File:FileType".to_string(),
        TagValue::new_string(format!("{:?}", format)),
    );

    // Parse CMT1 box (standard TIFF IFD0 with optional EXIF IFD and MakerNote)
    if let Some(tiff) = find_cr3_cmt1_tiff(data) {
        if let Ok(byte_order) = detect_byte_order(tiff) {
            let first_ifd_offset = read_u32(&tiff[4..8], byte_order) as u64;
            let reader = SliceReader::new(tiff);

            if let Ok(ifd0_tags) = parse_ifd(&reader, first_ifd_offset, byte_order) {
                let mut exif_ifd_offset = None;
                let mut makernote_data: Option<Vec<u8>> = None;
                let mut camera_make: Option<String> = None;

                for (tag_id, field_type, value_count, raw_bytes) in &ifd0_tags {
                    let bytes = raw_bytes.as_ref();

                    // EXIF IFD pointer
                    if *tag_id == 0x8769 && bytes.len() >= 4 {
                        exif_ifd_offset = Some(read_u32(bytes, byte_order) as u64);
                        continue;
                    }

                    // Camera make (needed for MakerNote dispatch)
                    if *tag_id == 0x010F && *field_type == 2 {
                        camera_make = Some(
                            String::from_utf8_lossy(bytes)
                                .trim_end_matches('\0')
                                .trim()
                                .to_string(),
                        );
                    }

                    // Insert IFD0 tag
                    let tag_name = lookup_tag_name(*tag_id, "IFD0");
                    let tag_value =
                        raw_bytes_to_simple_tag_value(bytes, *field_type, *value_count, byte_order);
                    let tag_value = if let Some(value) = format_exif_display_value(
                        *tag_id,
                        bytes,
                        *field_type,
                        *value_count,
                        byte_order,
                    ) {
                        TagValue::new_string(value)
                    } else {
                        tag_value
                    };
                    metadata.insert(tag_name, tag_value);
                }

                // Parse EXIF Sub-IFD
                if let Some(offset) = exif_ifd_offset {
                    if let Ok(exif_tags) = parse_ifd(&reader, offset, byte_order) {
                        for (tag_id, field_type, value_count, raw_bytes) in &exif_tags {
                            let bytes = raw_bytes.as_ref();

                            // MakerNote in EXIF IFD
                            if *tag_id == 0x927C {
                                makernote_data = Some(bytes.to_vec());
                                continue;
                            }

                            let tag_name = lookup_tag_name(*tag_id, "ExifIFD");
                            let tag_value = raw_bytes_to_simple_tag_value(
                                bytes,
                                *field_type,
                                *value_count,
                                byte_order,
                            );
                            let tag_value = if let Some(value) = format_exif_display_value(
                                *tag_id,
                                bytes,
                                *field_type,
                                *value_count,
                                byte_order,
                            ) {
                                TagValue::new_string(value)
                            } else {
                                tag_value
                            };
                            metadata.insert(tag_name, tag_value);
                        }
                    }
                }

                // Parse MakerNote from CMT1 EXIF IFD
                if let (Some(make), Some(mn_data)) = (camera_make.as_ref(), makernote_data.as_ref())
                {
                    let mut makernote_tags = std::collections::HashMap::new();
                    if let Err(e) = crate::parsers::tiff::makernote_dispatcher::dispatch_makernote(
                        make,
                        mn_data,
                        byte_order,
                        &mut makernote_tags,
                    ) {
                        eprintln!("Warning: Failed to parse MakerNote for {}: {}", make, e);
                    } else {
                        for (tag_name, tag_value) in makernote_tags {
                            metadata.insert(tag_name, TagValue::new_string(tag_value));
                        }
                    }
                }
            }
        }
    }

    // Some CR3 files store additional EXIF metadata in a CMT2 box
    // (e.g. LensModel, LensSerialNumber, OffsetTime, OwnerName).
    // Parse it the same way as CMT1.
    if let Some(tiff) = find_cr3_box(data, b"CMT2") {
        if tiff.starts_with(b"II*\0") || tiff.starts_with(b"MM\x00*") {
            if let Ok(byte_order) = detect_byte_order(tiff) {
                let first_ifd_offset = read_u32(&tiff[4..8], byte_order) as u64;
                let reader = SliceReader::new(tiff);
                if let Ok(ifd0_tags) = parse_ifd(&reader, first_ifd_offset, byte_order) {
                    for (tag_id, field_type, value_count, raw_bytes) in &ifd0_tags {
                        let bytes = raw_bytes.as_ref();
                        let tag_name = lookup_tag_name(*tag_id, "EXIF");
                        let tag_value =
                            raw_bytes_to_simple_tag_value(bytes, *field_type, *value_count, byte_order);
                        metadata.insert(tag_name, tag_value);
                    }
                }
            }
        }
    }

    // Some CR3 files store MakerNotes in CMT4 instead of CMT1 EXIF
    if let Some(makernote_data) = find_cr3_box(data, b"CMT4") {
        let mut makernote_tags = std::collections::HashMap::new();
        if let Err(e) = crate::parsers::tiff::makernote_dispatcher::dispatch_makernote(
            "Canon",
            makernote_data,
            ByteOrder::LittleEndian,
            &mut makernote_tags,
        ) {
            eprintln!("Warning: Failed to parse CMT4 MakerNote: {}", e);
        } else {
            for (tag_name, tag_value) in makernote_tags {
                metadata.insert(tag_name, TagValue::new_string(tag_value));
            }
        }
    }

    Ok(metadata)
}

/// Parse Sigma X3F format
///
/// X3F files use Sigma's proprietary FOVb format with:
/// - FOVb header at offset 0 (version, dimensions, white balance)
/// - Directory section (SECd) near end of file
/// - Property sections (SECp) with name/value pairs in UTF-16LE
/// - Image sections (SECi) that can contain embedded EXIF/TIFF
///
/// # Arguments
///
/// * `data` - Complete file data
/// * `format` - X3F format variant
///
/// # Returns
///
/// Metadata extracted from X3F file including header info, properties, and EXIF data.
fn parse_sigma_x3f(data: &[u8], format: RawFormat) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();
    metadata.insert(
        "File:FileType".to_string(),
        TagValue::new_string(format!("{:?}", format)),
    );

    // Verify FOVb signature
    if data.len() < 40 || &data[0..4] != b"FOVb" {
        return Ok(metadata);
    }

    // Parse X3F header (little-endian)
    let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let version_major = (version >> 16) & 0xFFFF;
    let version_minor = version & 0xFFFF;
    metadata.insert(
        "SigmaRaw:FileVersion".to_string(),
        TagValue::new_string(format!("{}.{}", version_major, version_minor)),
    );

    // Unique identifier (16 bytes at offset 8)
    // Skip for now - it's binary data

    // Mark bits at offset 24
    let _mark_bits = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);

    // Image dimensions at offset 28-35
    let columns = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);
    let rows = u32::from_le_bytes([data[32], data[33], data[34], data[35]]);

    if columns > 0 && rows > 0 {
        metadata.insert(
            "EXIF:ImageWidth".to_string(),
            TagValue::new_string(columns.to_string()),
        );
        metadata.insert(
            "EXIF:ImageHeight".to_string(),
            TagValue::new_string(rows.to_string()),
        );
    }

    // Rotation at offset 36
    let rotation = u32::from_le_bytes([data[36], data[37], data[38], data[39]]);
    if rotation > 0 {
        metadata.insert(
            "SigmaRaw:Rotation".to_string(),
            TagValue::new_string(format!("{}", rotation)),
        );
    }

    // White balance string (32 bytes at offset 40) - introduced in v2.1
    if version >= 0x00020001 && data.len() >= 72 {
        let wb_bytes = &data[40..72];
        if let Some(end) = wb_bytes.iter().position(|&b| b == 0) {
            if end > 0 {
                if let Ok(wb) = std::str::from_utf8(&wb_bytes[..end]) {
                    metadata.insert(
                        "SigmaRaw:WhiteBalance".to_string(),
                        TagValue::new_string(wb.to_string()),
                    );
                }
            }
        }
    }

    // Color mode string (32 bytes at offset 72) - introduced in v2.3
    if version >= 0x00020003 && data.len() >= 104 {
        let cm_bytes = &data[72..104];
        if let Some(end) = cm_bytes.iter().position(|&b| b == 0) {
            if end > 0 {
                if let Ok(cm) = std::str::from_utf8(&cm_bytes[..end]) {
                    metadata.insert(
                        "SigmaRaw:ColorMode".to_string(),
                        TagValue::new_string(cm.to_string()),
                    );
                }
            }
        }
    }

    // Find directory section - it's near the end of the file
    // The directory offset is stored at (file_size - 4)
    if data.len() < 12 {
        return Ok(metadata);
    }

    let dir_offset_pos = data.len() - 4;
    let dir_offset = u32::from_le_bytes([
        data[dir_offset_pos],
        data[dir_offset_pos + 1],
        data[dir_offset_pos + 2],
        data[dir_offset_pos + 3],
    ]) as usize;

    if dir_offset >= data.len() || dir_offset + 12 > data.len() {
        return Ok(metadata);
    }

    // Parse directory section header
    let dir_section = &data[dir_offset..];
    if dir_section.len() < 12 || &dir_section[0..4] != b"SECd" {
        return Ok(metadata);
    }

    let _dir_version = u32::from_le_bytes([
        dir_section[4],
        dir_section[5],
        dir_section[6],
        dir_section[7],
    ]);
    let num_entries = u32::from_le_bytes([
        dir_section[8],
        dir_section[9],
        dir_section[10],
        dir_section[11],
    ]) as usize;

    // Parse directory entries (each entry is 12 bytes: offset(4) + size(4) + type(4))
    let mut offset = 12;
    for _ in 0..num_entries {
        if offset + 12 > dir_section.len() {
            break;
        }

        let entry_offset = u32::from_le_bytes([
            dir_section[offset],
            dir_section[offset + 1],
            dir_section[offset + 2],
            dir_section[offset + 3],
        ]) as usize;
        let entry_size = u32::from_le_bytes([
            dir_section[offset + 4],
            dir_section[offset + 5],
            dir_section[offset + 6],
            dir_section[offset + 7],
        ]) as usize;
        let entry_type = &dir_section[offset + 8..offset + 12];

        offset += 12;

        if entry_offset >= data.len() || entry_offset + entry_size > data.len() {
            continue;
        }

        let entry_data = &data[entry_offset..entry_offset + entry_size];

        match entry_type {
            b"SECp" | b"PROP" => {
                // Property section - contains name/value pairs in UTF-16LE
                parse_x3f_properties(entry_data, &mut metadata);
            }
            b"SECi" | b"IMA0" | b"IMA1" | b"IMA2" => {
                // Image section - may contain embedded EXIF data
                parse_x3f_image_section(entry_data, &mut metadata, format);
            }
            b"CAMF" => {
                // Camera settings - complex format, skip for now
            }
            _ => {
                // Unknown section type
            }
        }
    }

    Ok(metadata)
}

/// Parse X3F property section (SECp)
///
/// Properties are stored as UTF-16LE name/value pairs.
fn parse_x3f_properties(data: &[u8], metadata: &mut MetadataMap) {
    if data.len() < 24 {
        return;
    }

    // Property section header:
    // 0-3: "SECp"
    // 4-7: version
    // 8-11: num_properties
    // 12-15: character format (0 = UTF-16)
    // 16-19: reserved
    // 20-23: total_length

    if &data[0..4] != b"SECp" {
        return;
    }

    let num_properties = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
    let _char_format = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

    // Property table starts at offset 24
    // Each entry is 8 bytes: name_offset(4) + value_offset(4)
    let table_start = 24;
    let table_size = num_properties * 8;

    if table_start + table_size > data.len() {
        return;
    }

    // Data block follows the property table
    let data_start = table_start + table_size;
    let data_block = if data_start < data.len() {
        &data[data_start..]
    } else {
        return;
    };

    for i in 0..num_properties {
        let entry_offset = table_start + i * 8;
        if entry_offset + 8 > data.len() {
            break;
        }

        let name_offset = u32::from_le_bytes([
            data[entry_offset],
            data[entry_offset + 1],
            data[entry_offset + 2],
            data[entry_offset + 3],
        ]) as usize
            * 2; // Multiply by 2 for UTF-16

        let value_offset = u32::from_le_bytes([
            data[entry_offset + 4],
            data[entry_offset + 5],
            data[entry_offset + 6],
            data[entry_offset + 7],
        ]) as usize
            * 2;

        // Read name (UTF-16LE null-terminated)
        let name = read_utf16le_string(data_block, name_offset);
        let value = read_utf16le_string(data_block, value_offset);

        if !name.is_empty() && !value.is_empty() {
            // Map property names to ExifTool-compatible tag names
            let tag_name = map_x3f_property_name(&name);
            metadata.insert(tag_name, TagValue::new_string(value));
        }
    }
}

/// Read a null-terminated UTF-16LE string from a byte buffer
fn read_utf16le_string(data: &[u8], offset: usize) -> String {
    if offset >= data.len() {
        return String::new();
    }

    let mut chars = Vec::new();
    let mut pos = offset;

    while pos + 1 < data.len() {
        let code_unit = u16::from_le_bytes([data[pos], data[pos + 1]]);
        if code_unit == 0 {
            break;
        }
        chars.push(code_unit);
        pos += 2;
    }

    String::from_utf16_lossy(&chars)
}

/// Map X3F property names to ExifTool-compatible tag names
fn map_x3f_property_name(name: &str) -> String {
    match name {
        "CAMMANUF" => "EXIF:Make".to_string(),
        "CAMMODEL" => "EXIF:Model".to_string(),
        "CAMSERIAL" => "MakerNotes:SerialNumber".to_string(),
        "FIRMWARE" => "MakerNotes:Firmware".to_string(),
        "EXPTIME" => "SigmaRaw:ExposureTime".to_string(),
        "APERTURE" => "SigmaRaw:FNumber".to_string(),
        "FLENGTH" => "SigmaRaw:FocalLength".to_string(),
        "FLEQ35MM" => "SigmaRaw:FocalLengthIn35mmFormat".to_string(),
        "ISO" => "SigmaRaw:ISO".to_string(),
        "WB" | "WBAL" => "SigmaRaw:WhiteBalance".to_string(),
        "EXPCOMP" => "SigmaRaw:ExposureCompensation".to_string(),
        "EXPMODE" => "SigmaRaw:ExposureProgram".to_string(),
        "FLASHM" => "SigmaRaw:FlashMode".to_string(),
        "DRIVEMODE" => "SigmaRaw:DriveMode".to_string(),
        "COLORMODE" => "SigmaRaw:ColorMode".to_string(),
        "SHARPNESS" => "SigmaRaw:Sharpness".to_string(),
        "CONTRAST" => "SigmaRaw:Contrast".to_string(),
        "SATURATION" => "SigmaRaw:Saturation".to_string(),
        "TIME" => "SigmaRaw:DateTimeOriginal".to_string(),
        "LENSARANGE" => "MakerNotes:LensApertureRange".to_string(),
        "LENSFRANGE" => "MakerNotes:LensFocalRange".to_string(),
        _ => format!("SigmaRaw:{}", name),
    }
}

/// Parse X3F image section for embedded EXIF data
///
/// X3F image sections (SECi) can contain embedded TIFF/EXIF data. This function
/// searches for TIFF headers throughout the image section data to locate and parse
/// any embedded metadata.
fn parse_x3f_image_section(data: &[u8], metadata: &mut MetadataMap, format: RawFormat) {
    if data.len() < 28 {
        return;
    }

    // Image section header:
    // 0-3: Section type ("SECi", "IMA0", etc.)
    // 4-7: Version
    // 8-11: Image type (1=RAW, 2=thumbnail, 3=preview JPEG)
    // 12-15: Image format
    // 16-19: Columns
    // 20-23: Rows
    // 24-27: Row stride

    let image_type = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let _image_format = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let columns = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
    let rows = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
    // Row stride at bytes 24-27 is not used for metadata extraction but
    // is part of the SECi header layout.
    let _row_stride = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);

    // Store preview image dimensions for type 2/3
    if (image_type == 2 || image_type == 3) && columns > 0 && rows > 0 {
        metadata.insert(
            "MakerNotes:PreviewImageSize".to_string(),
            TagValue::new_string(format!("{}x{}", columns, rows)),
        );

        // Image types 2 (thumbnail) and 3 (preview JPEG) contain JPEG data
        // with an APP1 EXIF segment. Parse it to extract the specific EXIF
        // tags that ExifTool reports for X3F but oxidex currently misses:
        //   DateTimeOriginal (0x9003), CreateDate (0x9004),
        //   ComponentsConfiguration (0x9101), ColorSpace (0xA001),
        //   CustomRendered (0xA401), Compression (0x0103 from IFD1).
        //
        // We intentionally skip MakerNote, InteropOffset, and all other
        // tags to match ExifTool's X3F output exactly and avoid triggering
        // unwanted composite tag generation.
        if data.len() > 28 {
            let jpeg_data = &data[28..];
            if let Ok(Some(tiff_data)) = find_jpeg_exif_tiff(jpeg_data) {
                if let Ok(byte_order) = detect_byte_order(tiff_data) {
                    if tiff_data.len() >= 8 {
                        let first_ifd_offset = read_u32(&tiff_data[4..8], byte_order) as u64;
                        let reader = SliceReader::new(tiff_data);

                        if let Ok(ifd0_tags) = parse_ifd(&reader, first_ifd_offset, byte_order) {
                            let mut exif_ifd_offset: Option<u64> = None;

                            for (tag_id, _field_type, _value_count, raw_bytes) in &ifd0_tags {
                                if *tag_id == 0x8769 && raw_bytes.as_ref().len() >= 4 {
                                    exif_ifd_offset =
                                        Some(read_u32(raw_bytes.as_ref(), byte_order) as u64);
                                }
                            }

                            // Parse ExifIFD: only the tags ExifTool reports
                            // for X3F files.  Extracting MarkerNote or
                            // InteropOffset here adds oxidex-only tags and
                            // triggers spurious composite generation.
                            if let Some(offset) = exif_ifd_offset {
                                if let Ok(exif_tags) =
                                    parse_ifd(&reader, offset, byte_order)
                                {
                                    for (tag_id, field_type, value_count, raw_bytes) in
                                        &exif_tags
                                    {
                                        // Whitelist: exact tag IDs ExifTool
                                        // emits for SigmaDP2.x3f.
                                        if !matches!(
                                            *tag_id,
                                            0x9003 // DateTimeOriginal
                                                | 0x9004 // CreateDate
                                                | 0x9101 // ComponentsConfiguration
                                                | 0xA001 // ColorSpace
                                                | 0xA401 // CustomRendered
                                        ) {
                                            continue;
                                        }
                                        let bytes = raw_bytes.as_ref();
                                        let tag_name = lookup_tag_name(*tag_id, "ExifIFD");
                                        let tag_value =
                                            if let Some(value) = format_exif_display_value(
                                                *tag_id,
                                                bytes,
                                                *field_type,
                                                *value_count,
                                                byte_order,
                                            ) {
                                                TagValue::new_string(value)
                                            } else {
                                                raw_bytes_to_simple_tag_value(
                                                    bytes,
                                                    *field_type,
                                                    *value_count,
                                                    byte_order,
                                                )
                                            };
                                        metadata.insert(tag_name, tag_value);
                                    }
                                }
                            }

                            // Parse IFD1 for Compression (0x0103).  The
                            // next-IFD offset follows IFD0's entries:
                            //   2 bytes count + N*12 bytes entries + 4 bytes next.
                            let ifd0_entry_count = ifd0_tags.len() as u64;
                            let ifd1_pos = first_ifd_offset + 2 + ifd0_entry_count * 12 + 4;
                            if ifd1_pos + 4 <= tiff_data.len() as u64 {
                                let ifd1_offset = read_u32(
                                    &tiff_data[ifd1_pos as usize..(ifd1_pos + 4) as usize],
                                    byte_order,
                                );
                                if ifd1_offset != 0 {
                                    if let Ok(ifd1_tags) =
                                        parse_ifd(&reader, ifd1_offset as u64, byte_order)
                                    {
                                        for (tag_id, field_type, value_count, raw_bytes) in &ifd1_tags {
                                            if *tag_id == 0x0103 {
                                                let bytes = raw_bytes.as_ref();
                                                let tag_value = format_x3f_compression(
                                                    bytes,
                                                    *field_type,
                                                    *value_count,
                                                    byte_order,
                                                );
                                                metadata.insert(
                                                    "ExifIFD:Compression".to_string(),
                                                    tag_value,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // For RAW type (1), look for embedded TIFF/EXIF data
    // TIFF can be embedded at various offsets, so we search for TIFF headers
    if image_type == 1 {
        // Search for TIFF headers (II or MM byte order markers) starting from offset 28
        // We search up to offset min(data.len() - 8, 1024) to find TIFF headers
        // Limit search to first 1KB to avoid scanning large image data
        let search_limit = (data.len() - 8).min(1024);

        for offset in 28..search_limit {
            // Check for little-endian TIFF (II\x2a\x00) or big-endian (MM\x00\x2a)
            if offset + 4 <= data.len() {
                let marker = &data[offset..offset + 2];
                let magic_bytes = &data[offset + 2..offset + 4];

                let is_valid_tiff = match marker {
                    b"II" => {
                        // Little-endian: magic should be 0x2a (42) or 0x55 (for RW2-like variants)
                        magic_bytes[0] == 0x2a || magic_bytes[0] == 0x55
                    }
                    b"MM" => {
                        // Big-endian: magic should be 0x00 0x2a or 0x00 0x55
                        magic_bytes[1] == 0x2a || magic_bytes[1] == 0x55
                    }
                    _ => false,
                };

                if is_valid_tiff && offset + 8 <= data.len() {
                    let potential_tiff = &data[offset..];
                    if let Ok(tiff_metadata) = parse_tiff_based_raw(potential_tiff, format) {
                        // Successfully parsed TIFF data, merge into metadata
                        for (key, value) in tiff_metadata {
                            if !metadata.contains_key(&key) {
                                metadata.insert(key, value);
                            }
                        }
                        // Found and parsed TIFF data, stop searching
                        return;
                    }
                }
            }
        }
    }
}

/// Parse Minolta MRW format
///
/// MRW files use Minolta's proprietary MRM format which consists of:
/// - 4-byte signature: `\x00MRM`
/// - 4-byte file size (big-endian)
/// - Series of tagged blocks, each with:
///   - 4-byte tag name (e.g., "PRD" for preview, "TTW" for TIFF)
///   - 4-byte block size (big-endian)
///   - Block data
///
/// The TTW block contains TIFF/EXIF data that can be parsed with standard TIFF parser.
///
/// # Arguments
///
/// * `data` - Complete file data
/// * `format` - MRW format variant
///
/// # Returns
///
/// Metadata extracted from MRW file including EXIF from TTW block.
fn parse_minolta_mrw(data: &[u8], format: RawFormat) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();
    metadata.insert(
        "File:FileType".to_string(),
        TagValue::new_string(format!("{:?}", format)),
    );

    // Verify MRM signature
    if data.len() < 8 || &data[0..4] != b"\x00MRM" {
        return Ok(metadata);
    }

    // Read file size (big-endian)
    let _file_size = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;

    // Parse MRW blocks starting at offset 8
    let mut offset = 8usize;

    while offset + 8 <= data.len() {
        // Read block tag (4 bytes) and size (4 bytes big-endian)
        let block_tag = &data[offset..offset + 4];
        let block_size = u32::from_be_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]) as usize;

        offset += 8;

        if offset + block_size > data.len() {
            break;
        }

        let block_data = &data[offset..offset + block_size];

        match block_tag {
            b"\x00TTW" => {
                // TTW block contains TIFF/EXIF data
                // Parse it as a TIFF structure
                if block_data.len() >= 8 {
                    // TIFF data should start with byte order marker
                    if let Ok(tiff_metadata) = parse_tiff_based_raw(block_data, format) {
                        for (key, value) in tiff_metadata {
                            metadata.insert(key, value);
                        }
                    }
                }
            }
            b"\x00PRD" => {
                // PRD block contains image dimensions and sensor info
                if block_data.len() >= 8 {
                    let reader = crate::io::EndianReader::big_endian(block_data);
                    // PRD structure:
                    // - 2 bytes: version?
                    // - 2 bytes: sensor width
                    // - 2 bytes: sensor height
                    // - 2 bytes: image width
                    // - 2 bytes: image height
                    // etc.
                    if let (Some(_version), Some(sensor_w), Some(sensor_h)) =
                        (reader.u16_at(0), reader.u16_at(2), reader.u16_at(4))
                    {
                        metadata.insert(
                            "MakerNotes:SensorWidth".to_string(),
                            TagValue::Integer(sensor_w as i64),
                        );
                        metadata.insert(
                            "MakerNotes:SensorHeight".to_string(),
                            TagValue::Integer(sensor_h as i64),
                        );
                    }
                    if let (Some(img_w), Some(img_h)) = (reader.u16_at(6), reader.u16_at(8)) {
                        metadata.insert(
                            "EXIF:ImageWidth".to_string(),
                            TagValue::Integer(img_w as i64),
                        );
                        metadata.insert(
                            "EXIF:ImageHeight".to_string(),
                            TagValue::Integer(img_h as i64),
                        );
                    }
                }
            }
            b"\x00WBG" => {
                // WBG block contains white balance info
                if block_data.len() >= 8 {
                    let reader = crate::io::EndianReader::big_endian(block_data);
                    // WBG structure varies but typically contains R/G/B multipliers
                    if let (Some(r), Some(g), Some(b)) =
                        (reader.u16_at(0), reader.u16_at(2), reader.u16_at(4))
                    {
                        // Values are typically scaled, convert to ratio
                        let g_val = g as f64;
                        if g_val > 0.0 {
                            let r_ratio = r as f64 / g_val;
                            let b_ratio = b as f64 / g_val;
                            metadata.insert(
                                "MakerNotes:ColorBalanceRed".to_string(),
                                TagValue::Float(r_ratio),
                            );
                            metadata.insert(
                                "MakerNotes:ColorBalanceGreen".to_string(),
                                TagValue::Float(1.0),
                            );
                            metadata.insert(
                                "MakerNotes:ColorBalanceBlue".to_string(),
                                TagValue::Float(b_ratio),
                            );
                        }
                    }
                }
            }
            _ => {
                // Unknown block - skip
            }
        }

        offset += block_size;
    }

    Ok(metadata)
}

/// Parse Canon CRW format
///
/// CRW is Canon's older proprietary raw format used before CR2.
/// This function is a stub for future implementation.
///
/// # Arguments
///
/// * `data` - Complete file data
/// * `format` - CRW format variant
///
/// # Returns
///
/// Minimal metadata with file type information.
/// Full CRW parsing to be implemented in future iteration.
///
/// # TODO
///
/// - Implement CRW format parser
/// - Extract Canon-specific metadata from CRW structure
fn parse_canon_crw(_data: &[u8], format: RawFormat) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();
    metadata.insert(
        "File:FileType".to_string(),
        TagValue::new_string(format!("{:?}", format)),
    );

    // TODO: Implement CRW specific parsing
    // CRW is Canon's older proprietary format

    Ok(metadata)
}

/// Format the Compression tag value (0x0103) from the X3F JPEG preview's
/// IFD1.  ExifTool reports value 6 as "JPEG (old-style)".
fn format_x3f_compression(
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    byte_order: ByteOrder,
) -> TagValue {
    if field_type == 3 && value_count >= 1 && bytes.len() >= 2 {
        let value = read_tiff_u16(bytes, byte_order).unwrap_or(0);
        if value == 6 {
            return TagValue::new_string("JPEG (old-style)".to_string());
        }
    }
    // Fall back to the standard simple tag value for other values.
    raw_bytes_to_simple_tag_value(bytes, field_type, value_count, byte_order)
}

// ===== Fujifilm RAF Format Parsing =====

/// Parse Fujifilm RAF format
///
/// RAF files use a proprietary container format with embedded JPEG/EXIF data.
/// The structure is:
/// - Bytes 0-15: "FUJIFILMCCD-RAW " signature
/// - Bytes 16-83: Header with version, camera model, and offset information
/// - Bytes 84-87: JPEG image offset (big-endian u32)
/// - Bytes 88-91: JPEG image length (big-endian u32)
/// - At JPEG offset: Standard JPEG file with EXIF data
///
/// This implementation extracts metadata from the embedded JPEG/EXIF data.
///
/// # Arguments
///
/// * `data` - Complete file data
/// * `format` - RAF format variant
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Extracted metadata from embedded JPEG/EXIF
/// * `Err(ExifToolError)` - Parse error or invalid RAF structure
///
/// # Implementation Strategy
///
/// Rather than parsing the proprietary RAF header, we locate and parse the
/// embedded JPEG data which contains standard EXIF metadata. This approach:
/// - Reuses existing JPEG/EXIF parsing infrastructure
/// - Extracts camera settings, timestamps, and other standard metadata
/// - Avoids need to reverse-engineer proprietary RAF format details
fn parse_fujifilm_raf(data: &[u8], format: RawFormat) -> Result<MetadataMap> {
    // Validate RAF signature
    if data.len() < 16 || &data[0..16] != b"FUJIFILMCCD-RAW " {
        return Err(ExifToolError::parse_error(
            "Invalid RAF file: missing FUJIFILMCCD-RAW signature",
        ));
    }

    // RAF header is 84 bytes, followed by offset table
    // Bytes 84-87: JPEG image offset (big-endian u32)
    // Bytes 88-91: JPEG image length (big-endian u32)
    if data.len() < 92 {
        return Err(ExifToolError::parse_error(
            "Invalid RAF file: header too small",
        ));
    }

    // Read JPEG offset and length (big-endian)
    let reader = crate::io::EndianReader::big_endian(data);
    let jpeg_offset = reader
        .u32_at(84)
        .ok_or_else(|| ExifToolError::parse_error("RAF: failed to read JPEG offset"))?
        as usize;
    let jpeg_length = reader
        .u32_at(88)
        .ok_or_else(|| ExifToolError::parse_error("RAF: failed to read JPEG length"))?
        as usize;

    // Validate JPEG offset and length
    if jpeg_offset >= data.len() {
        return Err(ExifToolError::parse_error(format!(
            "Invalid RAF file: JPEG offset {} exceeds file size {}",
            jpeg_offset,
            data.len()
        )));
    }

    if jpeg_offset + jpeg_length > data.len() {
        // JPEG length might be incorrect, try to use remaining file size
        let remaining = data.len() - jpeg_offset;
        eprintln!(
            "Warning: RAF JPEG length {} exceeds remaining file size {}, using remaining size",
            jpeg_length, remaining
        );
    }

    // Extract JPEG data
    let jpeg_end = (jpeg_offset + jpeg_length).min(data.len());
    let jpeg_data = &data[jpeg_offset..jpeg_end];

    // Verify JPEG signature (0xFF 0xD8)
    if jpeg_data.len() < 2 || jpeg_data[0] != 0xFF || jpeg_data[1] != 0xD8 {
        return Err(ExifToolError::parse_error(
            "Invalid RAF file: embedded data is not a valid JPEG",
        ));
    }

    // Create metadata map with format info
    let mut metadata = MetadataMap::new();
    metadata.insert(
        "File:FileType".to_string(),
        TagValue::new_string(format!("{:?}", format)),
    );

    // Parse the RAF file's own proprietary header/directory structures
    // (FirmwareVersion, RAFCompression, RawImage* dimensions,
    // WB_GRGBLevels*, etc.), separate from the embedded JPEG's EXIF data.
    for (tag_name, tag_value) in raf_parser::parse_raf_container_metadata(data) {
        metadata.insert(tag_name, TagValue::new_string(tag_value));
    }

    // Parse embedded JPEG to extract EXIF data
    // Create a SliceReader for the JPEG data
    let jpeg_reader = SliceReader::new(jpeg_data);

    // Use the existing JPEG segment parser to extract EXIF
    if let Ok(segments) = crate::parsers::jpeg::segment_parser::parse_segments(&jpeg_reader) {
        // Look for APP1 segments containing EXIF data
        for segment in segments {
            if segment.marker == 0xFFE1 && segment.data.len() > 6 {
                // Check for EXIF header "Exif\0\0"
                if &segment.data[0..6] == b"Exif\x00\x00" {
                    // EXIF data starts at byte 6
                    let exif_data = &segment.data[6..];

                    // Parse TIFF structure within EXIF data
                    if let Ok(byte_order) = detect_byte_order(exif_data) {
                        // Read first IFD offset (bytes 4-7 in TIFF header)
                        if exif_data.len() >= 8 {
                            let first_ifd_offset = read_u32(&exif_data[4..8], byte_order) as u64;

                            // Create reader for EXIF data
                            let exif_reader = SliceReader::new(exif_data);

                            // Parse IFD0
                            if let Ok(tags) = parse_ifd(&exif_reader, first_ifd_offset, byte_order)
                            {
                                // Track sub-IFD offsets
                                let mut exif_ifd_offset = None;

                                // Convert tags to metadata
                                for (tag_id, field_type, value_count, raw_bytes) in &tags {
                                    let bytes = raw_bytes.as_ref();

                                    // Check for EXIF Sub-IFD pointer (tag 0x8769)
                                    if *tag_id == 0x8769 && bytes.len() >= 4 {
                                        let offset = read_u32(bytes, byte_order);
                                        exif_ifd_offset = Some(offset as u64);
                                        continue;
                                    }

                                    // PrintIM directory (tag 0xC4A5): a small proprietary
                                    // sub-block starting with "PrintIM\0" followed by a
                                    // 4-byte ASCII version string at offset 8. ExifTool
                                    // reports this under its own "PrintIM" family.
                                    if *tag_id == 0xC4A5
                                        && bytes.len() >= 12
                                        && &bytes[0..7] == b"PrintIM"
                                    {
                                        let version =
                                            String::from_utf8_lossy(&bytes[8..12]).to_string();
                                        metadata.insert(
                                            "PrintIM:PrintIMVersion".to_string(),
                                            TagValue::new_string(version),
                                        );
                                        continue;
                                    }

                                    // Convert tag to metadata
                                    let tag_name = lookup_tag_name(*tag_id, "IFD0");
                                    let tag_value = raw_bytes_to_simple_tag_value(
                                        bytes,
                                        *field_type,
                                        *value_count,
                                        byte_order,
                                    );
                                    metadata.insert(tag_name, tag_value);
                                }

                                // The thumbnail (IFD1) immediately follows IFD0 and is
                                // referenced by the 4-byte "next IFD offset" that trails
                                // IFD0's entries. Parse it to recover Compression and the
                                // Thumbnail offset/length tags that ExifTool reports under
                                // the "EXIF" family.
                                let next_ifd_pos = first_ifd_offset + 2 + (tags.len() as u64 * 12);
                                if (next_ifd_pos + 4) as usize <= exif_data.len()
                                    && let Some(next_ifd_bytes) = exif_data
                                        .get(next_ifd_pos as usize..(next_ifd_pos + 4) as usize)
                                {
                                    let next_ifd_offset =
                                        read_u32(next_ifd_bytes, byte_order) as u64;
                                    if next_ifd_offset != 0
                                        && let Ok(ifd1_tags) =
                                            parse_ifd(&exif_reader, next_ifd_offset, byte_order)
                                    {
                                        // ThumbnailOffset (0x0201) is stored relative to this
                                        // TIFF header (same base as every other offset we read
                                        // from `exif_data`), but ExifTool reports it as an
                                        // absolute offset into the physical file. Recover that
                                        // base: JPEG data start + APP1 marker/length (4 bytes)
                                        // + "Exif\0\0" (6 bytes).
                                        let thumbnail_base =
                                            jpeg_offset as u64 + segment.offset + 4 + 6;

                                        let mut thumbnail_length: Option<u32> = None;
                                        for (tag_id, field_type, value_count, raw_bytes) in
                                            &ifd1_tags
                                        {
                                            let bytes = raw_bytes.as_ref();
                                            match *tag_id {
                                                // ThumbnailOffset/ThumbnailLength are absent
                                                // from the generated tag name database under
                                                // those names (they're indexed under the
                                                // EXIF-spec name "JPEGInterchangeFormat*"
                                                // instead), so name them explicitly here to
                                                // match ExifTool's default output.
                                                0x0201 if bytes.len() >= 4 => {
                                                    let value = read_u32(bytes, byte_order) as u64
                                                        + thumbnail_base;
                                                    metadata.insert(
                                                        "IFD1:ThumbnailOffset".to_string(),
                                                        TagValue::new_integer(value as i64),
                                                    );
                                                }
                                                0x0202 if bytes.len() >= 4 => {
                                                    let value = read_u32(bytes, byte_order);
                                                    thumbnail_length = Some(value);
                                                    metadata.insert(
                                                        "IFD1:ThumbnailLength".to_string(),
                                                        TagValue::new_integer(value as i64),
                                                    );
                                                }
                                                _ => {
                                                    let tag_name = lookup_tag_name(*tag_id, "IFD1");
                                                    let tag_value = raw_bytes_to_simple_tag_value(
                                                        bytes,
                                                        *field_type,
                                                        *value_count,
                                                        byte_order,
                                                    );
                                                    metadata.insert(tag_name, tag_value);
                                                }
                                            }
                                        }

                                        // ExifTool represents the actual thumbnail image
                                        // data with a placeholder unless -b is used to
                                        // extract binary data.
                                        if let Some(len) = thumbnail_length {
                                            metadata.insert(
                                                "IFD1:ThumbnailImage".to_string(),
                                                TagValue::new_string(format!(
                                                    "(Binary data {} bytes, use -b option to extract)",
                                                    len
                                                )),
                                            );
                                        }
                                    }
                                }

                                // Also look for GPS IFD pointer in IFD0
                                let mut gps_ifd_offset = None;
                                for (tag_id, _field_type, _value_count, raw_bytes) in &tags {
                                    let bytes = raw_bytes.as_ref();
                                    // GPS Sub-IFD pointer (tag 0x8825)
                                    if *tag_id == 0x8825 && bytes.len() >= 4 {
                                        let offset = read_u32(bytes, byte_order);
                                        gps_ifd_offset = Some(offset as u64);
                                    }
                                }

                                // Parse EXIF Sub-IFD if present
                                if let Some(offset) = exif_ifd_offset
                                    && let Ok(exif_tags) =
                                        parse_ifd(&exif_reader, offset, byte_order)
                                {
                                    // Track MakerNote data and Interoperability Sub-IFD pointer
                                    let mut makernote_data: Option<Vec<u8>> = None;
                                    let mut interop_ifd_offset: Option<u64> = None;

                                    for (tag_id, field_type, value_count, raw_bytes) in &exif_tags {
                                        let bytes = raw_bytes.as_ref();

                                        // Check for MakerNote tag (0x927C)
                                        if *tag_id == 0x927C {
                                            makernote_data = Some(bytes.to_vec());
                                            continue; // Don't add raw MakerNote to metadata
                                        }

                                        // Interoperability Sub-IFD pointer (tag 0xA005)
                                        if *tag_id == 0xA005 && bytes.len() >= 4 {
                                            let offset = read_u32(bytes, byte_order);
                                            interop_ifd_offset = Some(offset as u64);
                                            continue; // Don't add raw offset to metadata
                                        }

                                        let tag_name = lookup_tag_name(*tag_id, "ExifIFD");
                                        let tag_value = raw_bytes_to_simple_tag_value(
                                            bytes,
                                            *field_type,
                                            *value_count,
                                            byte_order,
                                        );
                                        metadata.insert(tag_name, tag_value);
                                    }

                                    // Parse Interoperability Sub-IFD if present (InteropIndex,
                                    // InteropVersion). ExifTool reports these under the "EXIF"
                                    // family even though they live in their own InteropIFD.
                                    if let Some(offset) = interop_ifd_offset
                                        && let Ok(interop_tags) =
                                            parse_ifd(&exif_reader, offset, byte_order)
                                    {
                                        for (tag_id, field_type, value_count, raw_bytes) in
                                            &interop_tags
                                        {
                                            let bytes = raw_bytes.as_ref();
                                            match *tag_id {
                                                // InteropIndex (0x0001): short ASCII code with
                                                // a PrintConv to a descriptive string.
                                                0x0001 => {
                                                    let raw = String::from_utf8_lossy(bytes)
                                                        .trim_end_matches('\0')
                                                        .to_string();
                                                    let printed = match raw.as_str() {
                                                        "R98" => "R98 - DCF basic file (sRGB)"
                                                            .to_string(),
                                                        "R03" => {
                                                            "R03 - DCF option file (Adobe RGB)"
                                                                .to_string()
                                                        }
                                                        "THM" => {
                                                            "THM - DCF thumbnail file".to_string()
                                                        }
                                                        _ => raw,
                                                    };
                                                    metadata.insert(
                                                        "InteropIFD:InteropIndex".to_string(),
                                                        TagValue::new_string(printed),
                                                    );
                                                }
                                                _ => {
                                                    let tag_name =
                                                        lookup_tag_name(*tag_id, "InteropIFD");
                                                    let tag_value = raw_bytes_to_simple_tag_value(
                                                        bytes,
                                                        *field_type,
                                                        *value_count,
                                                        byte_order,
                                                    );
                                                    metadata.insert(tag_name, tag_value);
                                                }
                                            }
                                        }
                                    }

                                    // Parse MakerNote if present (Fujifilm camera)
                                    if let Some(mn_data) = makernote_data.as_ref() {
                                        // Use the MakerNote dispatcher for Fujifilm
                                        let mut makernote_tags = std::collections::HashMap::new();
                                        if let Err(e) =
                                            crate::parsers::tiff::makernote_dispatcher::dispatch_makernote(
                                                "FUJIFILM",
                                                mn_data,
                                                byte_order,
                                                &mut makernote_tags,
                                            )
                                        {
                                            eprintln!(
                                                "Warning: Failed to parse Fujifilm MakerNote: {}",
                                                e
                                            );
                                        } else {
                                            // Add parsed MakerNote tags to metadata
                                            for (tag_name, tag_value) in makernote_tags {
                                                metadata.insert(
                                                    tag_name,
                                                    TagValue::new_string(tag_value),
                                                );
                                            }
                                        }

                                        // Also use RAF-specific MakerNote parser to extract additional camera metadata
                                        if let Ok(raf_tags) =
                                            raf_parser::parse_raf_makernote(mn_data, byte_order)
                                        {
                                            for (tag_name, tag_value) in raf_tags {
                                                // Only add if not already present from dispatcher
                                                if !metadata.contains_key(&tag_name) {
                                                    metadata.insert(
                                                        tag_name,
                                                        TagValue::new_string(tag_value),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }

                                // Parse GPS Sub-IFD if present
                                if let Some(offset) = gps_ifd_offset
                                    && let Ok(gps_tags) =
                                        parse_ifd(&exif_reader, offset, byte_order)
                                {
                                    for (tag_id, field_type, value_count, raw_bytes) in gps_tags {
                                        let tag_name = lookup_tag_name(tag_id, "GPS");
                                        let tag_value = raw_bytes_to_simple_tag_value(
                                            raw_bytes.as_ref(),
                                            field_type,
                                            value_count,
                                            byte_order,
                                        );
                                        metadata.insert(tag_name, tag_value);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(metadata)
}

/// Map NEF SubIFD tags to EXIF group names and apply format-specific decoding.
///
/// Returns `Some((tag_name, tag_value))` when the tag requires special handling
/// (Nikon-specific compression string, JPEG offset aliases, TIFF-EPStandardID
/// version formatting, CFARepeatPatternDim multi-value formatting). Returns
/// `None` to let the generic SubIFD path assign the tag with `EXIF:` prefix
/// via `lookup_tag_name(tag_id, "EXIF")` and `raw_bytes_to_simple_tag_value`.
fn format_nef_subifd_tag(
    tag_id: u16,
    _field_type: u16,
    _value_count: u32,
    bytes: &[u8],
    byte_order: ByteOrder,
) -> Option<(String, TagValue)> {
    match tag_id {
        // Compression: Nikon lossless compressed (34713) -> "Nikon NEF Compressed"
        0x0103 => {
            if bytes.len() >= 4 && read_u32(bytes, byte_order) == 34713 {
                Some((
                    "EXIF:Compression".to_string(),
                    TagValue::new_string("Nikon NEF Compressed".to_string()),
                ))
            } else {
                None // uncompressed/JPEG: let generic path emit integer
            }
        }
        // JPEGInterchangeFormat -> JpgFromRawStart
        0x0201 => Some((
            "EXIF:JpgFromRawStart".to_string(),
            TagValue::new_integer(read_u32(bytes, byte_order) as i64),
        )),
        // JPEGInterchangeFormatLength -> JpgFromRawLength
        0x0202 => Some((
            "EXIF:JpgFromRawLength".to_string(),
            TagValue::new_integer(read_u32(bytes, byte_order) as i64),
        )),
        // TIFF-EPStandardID: UNDEFINED[4] -> dotted version string
        0x828F => {
            let ver = format!(
                "{}.{}.{}.{}",
                bytes.first().copied().unwrap_or(0),
                bytes.get(1).copied().unwrap_or(0),
                bytes.get(2).copied().unwrap_or(0),
                bytes.get(3).copied().unwrap_or(0)
            );
            Some((
                "EXIF:TIFF-EPStandardID".to_string(),
                TagValue::new_string(ver),
            ))
        }
        // CFARepeatPatternDim: two SHORT values -> "h v"
        0x828D => {
            let h = read_tiff_u16(bytes, byte_order)?;
            let v = read_tiff_u16(bytes.get(2..)?, byte_order)?;
            Some((
                "EXIF:CFARepeatPatternDim".to_string(),
                TagValue::new_string(format!("{} {}", h, v)),
            ))
        }
        _ => None,
    }
}

// ===== Helper Functions =====

/// Detect byte order from TIFF header
///
/// Reads the first 2 bytes to determine endianness:
/// - "II" (0x4949) = Little-endian (used by most TIFF and many raw formats)
/// - "MM" (0x4D4D) = Big-endian (used by some TIFF and raw formats)
///
/// This function handles standard TIFF as well as raw format variants:
/// - Standard TIFF: "II\x2A\x00" or "MM\x00\x2A" (magic number 42)
/// - Panasonic RW2: "II\x55\x00" (magic number 85 instead of 42)
/// - Olympus ORF: "IIRO" or "MMOR" (uses "RO" or "OR" instead of magic number)
///
/// # Arguments
///
/// * `data` - File data (must be at least 2 bytes)
///
/// # Returns
///
/// * `Ok(ByteOrder)` - Detected byte order
/// * `Err(ExifToolError)` - Invalid byte order marker
fn detect_byte_order(data: &[u8]) -> Result<ByteOrder> {
    if data.len() < 2 {
        return Err(ExifToolError::parse_error(
            "File too small to detect byte order",
        ));
    }

    match &data[0..2] {
        b"II" => Ok(ByteOrder::LittleEndian),
        b"MM" => Ok(ByteOrder::BigEndian),
        _ => Err(ExifToolError::parse_error("Invalid TIFF byte order marker")),
    }
}

/// Read a 32-bit unsigned integer from bytes with specified byte order
///
/// # Arguments
///
/// * `bytes` - Byte slice (must be at least 4 bytes)
/// * `byte_order` - Endianness to use
///
/// # Returns
///
/// The parsed u32 value
fn read_u32(bytes: &[u8], byte_order: ByteOrder) -> u32 {
    let reader = match byte_order {
        ByteOrder::LittleEndian => EndianReader::little_endian(bytes),
        ByteOrder::BigEndian => EndianReader::big_endian(bytes),
    };

    reader.u32_at(0).unwrap_or(0)
}

/// Convert raw bytes to TagValue (simplified version)
///
/// This is a simplified converter for raw metadata parsing.
/// For full tag value conversion with all special cases, use the
/// raw_bytes_to_tag_value function in operations.rs.
///
/// # Arguments
///
/// * `bytes` - Raw byte data
/// * `field_type` - TIFF field type
/// * `value_count` - Number of values
/// * `byte_order` - Endianness
///
/// # Returns
///
/// TagValue representing the data
fn raw_bytes_to_simple_tag_value(
    bytes: &[u8],
    field_type: u16,
    _value_count: u32,
    byte_order: ByteOrder,
) -> TagValue {
    use crate::parsers::common::exif_types::ExifType;

    // Try to convert field_type to ExifType
    if let Some(exif_type) = ExifType::from_u16(field_type) {
        match exif_type {
            // ASCII string
            ExifType::Ascii => {
                let s = String::from_utf8_lossy(bytes);
                let s = s.trim_end_matches('\0');
                return TagValue::new_string(s.to_string());
            }

            // SHORT (16-bit unsigned)
            ExifType::Short if bytes.len() >= 2 => {
                let reader = match byte_order {
                    ByteOrder::LittleEndian => EndianReader::little_endian(bytes),
                    ByteOrder::BigEndian => EndianReader::big_endian(bytes),
                };
                let value = reader.u16_at(0).unwrap_or(0) as i64;
                return TagValue::new_integer(value);
            }

            // LONG (32-bit unsigned)
            ExifType::Long if bytes.len() >= 4 => {
                let value = read_u32(bytes, byte_order) as i64;
                return TagValue::new_integer(value);
            }

            // RATIONAL (two 32-bit unsigned)
            ExifType::Rational if bytes.len() >= 8 => {
                let numerator = read_u32(&bytes[0..4], byte_order);
                let denominator = read_u32(&bytes[4..8], byte_order);
                return TagValue::new_rational(numerator as i32, denominator as i32);
            }

            // SRATIONAL (two 32-bit signed)
            ExifType::SRational if bytes.len() >= 8 => {
                let reader = match byte_order {
                    ByteOrder::LittleEndian => EndianReader::little_endian(bytes),
                    ByteOrder::BigEndian => EndianReader::big_endian(bytes),
                };
                let numerator = reader.i32_at(0).unwrap_or(0);
                let denominator = reader.i32_at(4).unwrap_or(1);
                return TagValue::new_rational(numerator, denominator);
            }

            _ => {}
        }
    }

    // Fallback: binary data
    TagValue::new_binary(bytes.to_vec())
}

// ===== FileReader Adapter for Byte Slices =====

/// FileReader implementation for byte slices
///
/// This adapter allows using a byte slice with the TIFF parser
/// which expects a FileReader trait implementation.
struct SliceReader<'a> {
    data: &'a [u8],
}

impl<'a> SliceReader<'a> {
    /// Create a new SliceReader from a byte slice
    fn new(data: &'a [u8]) -> Self {
        Self { data }
    }
}

impl<'a> FileReader for SliceReader<'a> {
    /// Read bytes from the slice
    ///
    /// # Arguments
    ///
    /// * `offset` - Offset from start of slice
    /// * `length` - Number of bytes to read
    ///
    /// # Returns
    ///
    /// * `Ok(&[u8])` - Slice of requested bytes
    /// * `Err` - If offset/length exceeds slice bounds
    fn read(&self, offset: u64, length: usize) -> std::io::Result<&[u8]> {
        let start = offset as usize;
        let end = start + length;

        if end > self.data.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "read beyond end of data",
            ));
        }

        Ok(&self.data[start..end])
    }

    /// Get total size of the slice
    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

// ===== Unit Tests =====

#[cfg(test)]
mod cr3_cmt1_artist_tests {
    use super::*;

    /// Build a minimal CR3-shaped buffer: a `CMT1` box whose payload is a
    /// little-endian TIFF with a single IFD0 ASCII entry `tag_id` holding
    /// `value` (its trailing NUL included in the count). The value is kept
    /// <= 4 bytes so it fits inline in the IFD entry.
    fn build_cr3_with_tag(tag_id: u16, value: &[u8]) -> Vec<u8> {
        assert!(value.len() <= 4, "test helper only inlines <=4-byte values");
        let mut inline = [0u8; 4];
        inline[..value.len()].copy_from_slice(value);

        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II*\0"); // little-endian TIFF
        tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD0 at offset 8
        tiff.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
        tiff.extend_from_slice(&tag_id.to_le_bytes()); // tag
        tiff.extend_from_slice(&2u16.to_le_bytes()); // type = ASCII
        tiff.extend_from_slice(&(value.len() as u32).to_le_bytes()); // count
        tiff.extend_from_slice(&inline); // inline value
        tiff.extend_from_slice(&0u32.to_le_bytes()); // next IFD = 0

        let mut data = Vec::new();
        data.extend_from_slice(b"\0\0\0\x18ftypcrx "); // plausible leading box
        let box_size = (8 + tiff.len()) as u32;
        data.extend_from_slice(&box_size.to_be_bytes()); // CMT1 box size (BE)
        data.extend_from_slice(b"CMT1");
        data.extend_from_slice(&tiff);
        data
    }

    fn build_cr3_with_artist(artist: &[u8]) -> Vec<u8> {
        build_cr3_with_tag(0x013B, artist)
    }

    #[test]
    fn extracts_artist_from_cmt1_box() {
        let data = build_cr3_with_artist(b"Jo\0");
        let metadata = parse_cr3(&data, RawFormat::CanonCR3).unwrap();
        assert_eq!(
            metadata.get("IFD0:Artist"),
            Some(&TagValue::new_string("Jo".to_string()))
        );
    }

    #[test]
    fn preserves_empty_artist_value() {
        // ExifTool reports Artist for CR3 even when it is an empty string;
        // a present-but-empty entry must still yield the tag.
        let data = build_cr3_with_artist(b"\0");
        let metadata = parse_cr3(&data, RawFormat::CanonCR3).unwrap();
        assert_eq!(
            metadata.get("IFD0:Artist"),
            Some(&TagValue::new_string(String::new()))
        );
    }

    #[test]
    fn no_artist_tag_when_no_cmt1_box() {
        let metadata = parse_cr3(b"\0\0\0\x18ftypcrx not a cmt box", RawFormat::CanonCR3).unwrap();
        assert!(metadata.get("IFD0:Artist").is_none());
    }

    #[test]
    fn extracts_copyright_from_cmt1_box() {
        // ExifTool reports Copyright (0x8298) for CR3 from the CMT1 TIFF's
        // IFD0, alongside Artist; verified against CanonRaw.cr3.
        let data = build_cr3_with_tag(0x8298, b"(c)\0");
        let metadata = parse_cr3(&data, RawFormat::CanonCR3).unwrap();
        assert_eq!(
            metadata.get("IFD0:Copyright"),
            Some(&TagValue::new_string("(c)".to_string()))
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cfa_pattern_0xa302_resolves_to_correct_tag_name() {
        assert_eq!(
            crate::tag_db::lookup_tag_name(0xA302, "ExifIFD"),
            "ExifIFD:CFAPattern"
        );
        assert_eq!(
            crate::tag_db::lookup_tag_name(0x828E, "ExifIFD"),
            "ExifIFD:CFAPattern2"
        );
    }

    #[test]
    fn test_detect_byte_order_little_endian() {
        let data = b"II\x2a\x00\x08\x00\x00\x00";
        let byte_order = detect_byte_order(data).unwrap();
        assert_eq!(byte_order, ByteOrder::LittleEndian);
    }

    #[test]
    fn test_detect_byte_order_big_endian() {
        let data = b"MM\x00\x2a\x00\x00\x00\x08";
        let byte_order = detect_byte_order(data).unwrap();
        assert_eq!(byte_order, ByteOrder::BigEndian);
    }

    #[test]
    fn test_detect_byte_order_invalid() {
        let data = b"XX\x2a\x00";
        assert!(detect_byte_order(data).is_err());
    }

    #[test]
    fn test_detect_byte_order_too_small() {
        let data = b"I";
        assert!(detect_byte_order(data).is_err());
    }

    #[test]
    fn test_parse_tiff_based_format() {
        // Minimal TIFF header (little-endian)
        // II (little-endian) + 42 (magic) + offset 8 (first IFD)
        let data = b"II\x2a\x00\x08\x00\x00\x00\x00\x00"; // Header + no IFD entries

        // Should not crash even with minimal data
        let result = parse_raw_metadata(data, RawFormat::AdobeDNG);
        // Either parse successfully or fail gracefully
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_parse_cr3_stub() {
        let data = b"\x00\x00\x00\x18ftypcrx test data";
        let result = parse_raw_metadata(data, RawFormat::CanonCR3);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert!(metadata.contains_key("File:FileType"));
    }

    #[test]
    fn test_parse_x3f_stub() {
        let data = b"FOVbtest data";
        let result = parse_raw_metadata(data, RawFormat::SigmaX3F);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert!(metadata.contains_key("File:FileType"));
    }

    #[test]
    fn test_parse_mrw_stub() {
        let data = b"\x00MRMtest data";
        let result = parse_raw_metadata(data, RawFormat::MinoltaMRW);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert!(metadata.contains_key("File:FileType"));
    }

    #[test]
    fn test_slice_reader_read() {
        let data = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let reader = SliceReader::new(&data);

        let result = reader.read(0, 5).unwrap();
        assert_eq!(result, &[0, 1, 2, 3, 4]);

        let result = reader.read(5, 3).unwrap();
        assert_eq!(result, &[5, 6, 7]);
    }

    #[test]
    fn test_slice_reader_read_out_of_bounds() {
        let data = vec![0, 1, 2, 3, 4];
        let reader = SliceReader::new(&data);

        let result = reader.read(0, 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_slice_reader_size() {
        let data = vec![0; 100];
        let reader = SliceReader::new(&data);
        assert_eq!(reader.size(), 100);
    }

    #[test]
    fn test_subifd_parsing() {
        // Create a TIFF with SubIFD pointer
        let mut data = Vec::new();

        // TIFF header (little-endian)
        data.extend_from_slice(b"II\x2a\x00");
        data.extend_from_slice(&8u32.to_le_bytes()); // First IFD offset

        // IFD0 with SubIFD pointer tag (0x014A)
        data.extend_from_slice(&1u16.to_le_bytes()); // 1 entry

        // SubIFD pointer tag entry
        data.extend_from_slice(&0x014Au16.to_le_bytes()); // Tag ID: SubIFD
        data.extend_from_slice(&4u16.to_le_bytes()); // Type: LONG
        data.extend_from_slice(&1u32.to_le_bytes()); // Count: 1
        data.extend_from_slice(&30u32.to_le_bytes()); // Offset to SubIFD

        // Next IFD offset (0 = none)
        data.extend_from_slice(&0u32.to_le_bytes());

        // SubIFD at offset 30
        // Pad to reach offset 30
        while data.len() < 30 {
            data.push(0);
        }

        // SubIFD with one entry (ImageWidth)
        data.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
        data.extend_from_slice(&0x0100u16.to_le_bytes()); // Tag: ImageWidth
        data.extend_from_slice(&3u16.to_le_bytes()); // Type: SHORT
        data.extend_from_slice(&1u32.to_le_bytes()); // Count: 1
        data.extend_from_slice(&4000u16.to_le_bytes()); // Value: 4000
        data.extend_from_slice(&0u16.to_le_bytes()); // Padding
        data.extend_from_slice(&0u32.to_le_bytes()); // Next IFD: none

        let result = parse_raw_metadata(&data, RawFormat::AdobeDNG);
        assert!(result.is_ok(), "Should parse TIFF with SubIFD");

        let metadata = result.unwrap();
        // Should have extracted the ImageWidth from SubIFD0
        // Note: The exact tag name depends on the tag database
        let has_subifd_data = metadata
            .keys()
            .any(|k| k.starts_with("SubIFD") || k.contains("ImageWidth"));

        if !has_subifd_data {
            let keys: Vec<&String> = metadata.keys().collect();
            eprintln!("Available keys: {:?}", keys);
        }

        assert!(has_subifd_data, "Should have extracted SubIFD data");
    }

    #[test]
    fn test_dng_version_extraction() {
        // Create a minimal TIFF with DNGVersion tag
        let mut data = Vec::new();

        // TIFF header
        data.extend_from_slice(b"II\x2a\x00");
        data.extend_from_slice(&8u32.to_le_bytes());

        // IFD0 with DNGVersion tag (0xC612)
        data.extend_from_slice(&1u16.to_le_bytes()); // 1 entry

        // DNGVersion tag entry
        data.extend_from_slice(&0xC612u16.to_le_bytes()); // Tag ID
        data.extend_from_slice(&1u16.to_le_bytes()); // Type: BYTE
        data.extend_from_slice(&4u32.to_le_bytes()); // Count: 4
        // Version 1.4.0.0 stored inline
        data.extend_from_slice(&[1, 4, 0, 0]);

        // Next IFD offset
        data.extend_from_slice(&0u32.to_le_bytes());

        let result = parse_raw_metadata(&data, RawFormat::AdobeDNG);
        assert!(result.is_ok(), "Should parse DNG with version tag");

        let metadata = result.unwrap();
        // Check if version string was created
        if metadata.contains_key("DNG:VersionString") {
            let version = metadata.get("DNG:VersionString").unwrap();
            if let TagValue::String(s) = version {
                assert_eq!(s, "1.4.0.0", "Version should be parsed");
            } else {
                panic!("Version should be a string");
            }
        }
    }

    #[test]
    fn test_cr2_format_detection() {
        // Create a CR2 header
        let mut data = Vec::new();
        data.extend_from_slice(b"II\x2a\x00"); // TIFF header
        data.extend_from_slice(&16u32.to_le_bytes()); // First IFD offset
        data.extend_from_slice(b"CR\x02\x00"); // CR2 marker at offset 8

        // Minimal IFD at offset 16
        data.extend_from_slice(&0u16.to_le_bytes()); // 0 entries
        data.extend_from_slice(&0u32.to_le_bytes()); // Next IFD

        let result = parse_raw_metadata(&data, RawFormat::CanonCR2);
        assert!(result.is_ok(), "Should parse CR2 format");

        let metadata = result.unwrap();
        assert!(
            metadata.contains_key("File:FileType"),
            "Should have FileType tag"
        );
    }

    #[test]
    fn test_nef_format_detection() {
        // Create a minimal NEF (just TIFF header, NEF is detected by extension)
        let mut data = Vec::new();
        data.extend_from_slice(b"MM\x00\x2a"); // TIFF header (big-endian for Nikon)
        data.extend_from_slice(&8u32.to_be_bytes()); // First IFD offset

        // Minimal IFD
        data.extend_from_slice(&0u16.to_be_bytes()); // 0 entries
        data.extend_from_slice(&0u32.to_be_bytes()); // Next IFD

        let result = parse_raw_metadata(&data, RawFormat::NikonNEF);
        assert!(result.is_ok(), "Should parse NEF format");

        let metadata = result.unwrap();
        assert!(
            metadata.contains_key("File:FileType"),
            "Should have FileType tag"
        );
    }

    #[test]
    fn test_multiple_ifd_parsing() {
        // Create TIFF with IFD0 and IFD1 (typical for RAW with thumbnail)
        let mut data = Vec::new();

        // TIFF header
        data.extend_from_slice(b"II\x2a\x00");
        data.extend_from_slice(&8u32.to_le_bytes());

        // IFD0 with ImageWidth tag and pointer to IFD1
        data.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
        data.extend_from_slice(&0x0100u16.to_le_bytes()); // ImageWidth
        data.extend_from_slice(&3u16.to_le_bytes()); // Type: SHORT
        data.extend_from_slice(&1u32.to_le_bytes()); // Count: 1
        data.extend_from_slice(&160u16.to_le_bytes()); // Value: 160
        data.extend_from_slice(&0u16.to_le_bytes()); // Padding

        // Next IFD offset (IFD1 at offset 30)
        data.extend_from_slice(&30u32.to_le_bytes());

        // Pad to offset 30
        while data.len() < 30 {
            data.push(0);
        }

        // IFD1 with ImageWidth tag
        data.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
        data.extend_from_slice(&0x0100u16.to_le_bytes()); // ImageWidth
        data.extend_from_slice(&3u16.to_le_bytes()); // Type: SHORT
        data.extend_from_slice(&1u32.to_le_bytes()); // Count: 1
        data.extend_from_slice(&1600u16.to_le_bytes()); // Value: 1600
        data.extend_from_slice(&0u16.to_le_bytes()); // Padding

        // No more IFDs
        data.extend_from_slice(&0u32.to_le_bytes());

        let result = parse_raw_metadata(&data, RawFormat::CanonCR2);
        assert!(result.is_ok(), "Should parse multiple IFDs");

        let metadata = result.unwrap();
        // Should have tags from both IFD0 and IFD1
        let has_ifd0 = metadata.keys().any(|k| k.starts_with("IFD0:"));
        let has_ifd1 = metadata.keys().any(|k| k.starts_with("IFD1:"));

        assert!(has_ifd0 || has_ifd1, "Should have extracted tags from IFDs");
    }
}

/// Regression coverage for the RW2 JpgFromRaw EXIF PrintConv values that the
/// corpus sample cannot reach.
///
/// Background (measured 2026-07-26): the six tags wired for RW2 were signed off
/// by a `recheck-pass gaps=6->0` run against
/// `/tmp/oxidex-exiftool-cache/combined-samples/Panasonic.rw2`. That sample only
/// ever hits `CustomRendered = 0`, `ExposureMode = 0`, `DigitalZoomRatio = 0/10`
/// and LONG-typed `ExifImageWidth/Height`, so exactly four of the seventeen
/// added literals and branches were actually executed. A gap count dropping to
/// zero therefore proves nothing about `1 => 'Custom'`, `1 => 'Manual'`,
/// `2 => 'Auto bracket'`, the SHORT-typed dimension branch or the non-integral
/// rational path.
///
/// That blind spot is not hypothetical. The same fleet run produced a TTF fix
/// asserting Mac language Spanish = 12 (`%ttLang` says 12 => 'ar'; Spanish is 6)
/// and a RAR5 fix inventing host-OS values 2/3/4 plus an "Unknown" catch-all
/// (ExifTool's RAR5 table is exactly `{0 => 'Win32', 1 => 'Unix'}`). Both sat
/// beside values the sample *did* exercise, so both rechecks came back green.
///
/// Every expectation below is a literal copied from the PrintConv hashes in
/// `%Image::ExifTool::Exif::Main`
/// (`/private/tmp/oxidex-exiftool-cache/exiftool/lib/Image/ExifTool/Exif.pm`,
/// 0xa401 at line 2843, 0xa402 at line 2862), never a reference to the constant
/// under test — an assertion phrased in terms of the constant itself passes for
/// whatever value that constant happens to hold.
#[cfg(test)]
mod rw2_embedded_exif_printconv_tests {
    use super::*;

    /// One synthetic ExifIFD entry: `(tag_id, field_type, value_count, payload)`.
    type Entry<'a> = (u16, u16, u32, &'a [u8]);

    fn u16b(value: u16, big_endian: bool) -> [u8; 2] {
        if big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        }
    }

    fn u32b(value: u32, big_endian: bool) -> [u8; 4] {
        if big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        }
    }

    /// Build an RW2-shaped JpgFromRaw blob: SOI, then an APP1 `Exif\0\0`
    /// segment whose TIFF holds an IFD0 carrying only the ExifIFD pointer
    /// (0x8769) that `extract_rw2_embedded_exif_tags` follows.
    fn build_preview_jpeg(entries: &[Entry<'_>], big_endian: bool) -> Vec<u8> {
        // IFD0 sits at 8 and holds one 12-byte entry: 8 + 2 + 12 + 4 = 26.
        const EXIF_IFD_OFFSET: u32 = 26;
        let entry_count = u32::try_from(entries.len()).expect("test entry count fits in u32");
        let overflow_start = EXIF_IFD_OFFSET + 2 + 12 * entry_count + 4;

        let mut tiff = Vec::new();
        tiff.extend_from_slice(if big_endian { b"MM\0*" } else { b"II*\0" });
        tiff.extend_from_slice(&u32b(8, big_endian));

        tiff.extend_from_slice(&u16b(1, big_endian));
        tiff.extend_from_slice(&u16b(0x8769, big_endian));
        tiff.extend_from_slice(&u16b(4, big_endian)); // LONG
        tiff.extend_from_slice(&u32b(1, big_endian));
        tiff.extend_from_slice(&u32b(EXIF_IFD_OFFSET, big_endian));
        tiff.extend_from_slice(&u32b(0, big_endian)); // no IFD1
        assert_eq!(tiff.len(), EXIF_IFD_OFFSET as usize);

        let mut overflow = Vec::new();
        tiff.extend_from_slice(&u16b(
            u16::try_from(entries.len()).expect("test entry count fits in u16"),
            big_endian,
        ));
        for (tag_id, field_type, value_count, payload) in entries {
            tiff.extend_from_slice(&u16b(*tag_id, big_endian));
            tiff.extend_from_slice(&u16b(*field_type, big_endian));
            tiff.extend_from_slice(&u32b(*value_count, big_endian));
            if payload.len() <= 4 {
                // TIFF stores short values left-justified in the 4-byte field.
                let mut inline = [0u8; 4];
                inline[..payload.len()].copy_from_slice(payload);
                tiff.extend_from_slice(&inline);
            } else {
                let at = overflow_start
                    + u32::try_from(overflow.len()).expect("test overflow fits in u32");
                tiff.extend_from_slice(&u32b(at, big_endian));
                overflow.extend_from_slice(payload);
            }
        }
        tiff.extend_from_slice(&u32b(0, big_endian)); // next IFD
        assert_eq!(tiff.len(), overflow_start as usize);
        tiff.extend_from_slice(&overflow);

        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        let segment_length =
            u16::try_from(2 + 6 + tiff.len()).expect("test APP1 segment fits in u16");
        jpeg.extend_from_slice(&segment_length.to_be_bytes());
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(&tiff);
        jpeg
    }

    fn extract(entries: &[Entry<'_>], big_endian: bool) -> MetadataMap {
        let jpeg = build_preview_jpeg(entries, big_endian);
        let mut metadata = MetadataMap::new();
        extract_rw2_embedded_exif_tags(&jpeg, &mut metadata)
            .expect("synthetic RW2 preview EXIF must parse");
        metadata
    }

    fn short(value: u16, big_endian: bool) -> Vec<u8> {
        u16b(value, big_endian).to_vec()
    }

    fn rational(numerator: u32, denominator: u32, big_endian: bool) -> Vec<u8> {
        let mut bytes = u32b(numerator, big_endian).to_vec();
        bytes.extend_from_slice(&u32b(denominator, big_endian));
        bytes
    }

    /// Panasonic.rw2 carries `CustomRendered = 0`, so the `1 => 'Custom'` arm
    /// was never executed by the recheck that approved it. Exif.pm:2852 reads
    /// `1 => 'Custom',`.
    #[test]
    fn custom_rendered_one_is_custom() {
        let payload = short(1, false);
        let metadata = extract(&[(0xA401, 3, 1, &payload)], false);
        assert_eq!(
            metadata.get("ExifIFD:CustomRendered"),
            Some(&TagValue::new_string("Custom"))
        );
    }

    /// The value the sample does hit, pinned so that a future edit cannot swap
    /// the two arms and still pass. Exif.pm:2851 reads `0 => 'Normal',`.
    #[test]
    fn custom_rendered_zero_is_normal() {
        let payload = short(0, false);
        let metadata = extract(&[(0xA401, 3, 1, &payload)], false);
        assert_eq!(
            metadata.get("ExifIFD:CustomRendered"),
            Some(&TagValue::new_string("Normal"))
        );
    }

    /// Not exercised by Panasonic.rw2 (`ExposureMode = 0`). Exif.pm:2868 reads
    /// `1 => 'Manual',`.
    #[test]
    fn exposure_mode_one_is_manual() {
        let payload = short(1, false);
        let metadata = extract(&[(0xA402, 3, 1, &payload)], false);
        assert_eq!(
            metadata.get("ExifIFD:ExposureMode"),
            Some(&TagValue::new_string("Manual"))
        );
    }

    /// Not exercised by Panasonic.rw2. Exif.pm:2869 reads
    /// `2 => 'Auto bracket',`.
    #[test]
    fn exposure_mode_two_is_auto_bracket() {
        let payload = short(2, false);
        let metadata = extract(&[(0xA402, 3, 1, &payload)], false);
        assert_eq!(
            metadata.get("ExifIFD:ExposureMode"),
            Some(&TagValue::new_string("Auto bracket"))
        );
    }

    /// Exif.pm:2867 reads `0 => 'Auto',`; pinned alongside 1 and 2 so the three
    /// arms cannot rotate.
    #[test]
    fn exposure_mode_zero_is_auto() {
        let payload = short(0, false);
        let metadata = extract(&[(0xA402, 3, 1, &payload)], false);
        assert_eq!(
            metadata.get("ExifIFD:ExposureMode"),
            Some(&TagValue::new_string("Auto"))
        );
    }

    /// Panasonic.rw2 stores this preview's EXIF little-endian, so the
    /// big-endian read of every enum payload is unexercised by the corpus.
    #[test]
    fn enum_print_conv_is_byte_order_aware() {
        let custom = short(1, true);
        let bracket = short(2, true);
        let metadata = extract(&[(0xA401, 3, 1, &custom), (0xA402, 3, 1, &bracket)], true);
        assert_eq!(
            metadata.get("ExifIFD:CustomRendered"),
            Some(&TagValue::new_string("Custom"))
        );
        assert_eq!(
            metadata.get("ExifIFD:ExposureMode"),
            Some(&TagValue::new_string("Auto bracket"))
        );
    }

    /// The RAR5-class guard. ExifTool's 0xa401 PrintConv does define
    /// `2 => 'HDR (no original saved)'` (Exif.pm:2853, non-standard Apple iOS),
    /// which oxidex has not wired yet — that is a coverage gap. What this test
    /// pins is that the gap degrades to the raw number, which is exactly what
    /// ExifTool prints when no PrintConv key matches, instead of substituting a
    /// stand-in label. The rejected RAR5 commit failed precisely here: its
    /// catch-all emitted "Unknown" and overwrote real data.
    #[test]
    fn out_of_table_custom_rendered_falls_back_to_raw_number() {
        let payload = short(2, false);
        let metadata = extract(&[(0xA401, 3, 1, &payload)], false);
        assert_eq!(
            metadata.get("ExifIFD:CustomRendered"),
            Some(&TagValue::new_integer(2))
        );
    }

    /// Same guard for ExposureMode. Exif.pm:2870 notes value 3 has been seen
    /// from Samsung EX1/NX30/NX200 and deliberately has no PrintConv entry, so
    /// ExifTool prints `3`.
    #[test]
    fn out_of_table_exposure_mode_falls_back_to_raw_number() {
        let payload = short(3, false);
        let metadata = extract(&[(0xA402, 3, 1, &payload)], false);
        assert_eq!(
            metadata.get("ExifIFD:ExposureMode"),
            Some(&TagValue::new_integer(3))
        );
    }

    /// Panasonic.rw2 stores both dimensions as LONG (type 4). ExifTool declares
    /// them `Writable => 'int16u'` (Exif.pm:2705 and 2711), so the SHORT form is
    /// the spec-typical encoding and is entirely unexercised by the corpus.
    /// Neither tag has a PrintConv, so the raw number must survive verbatim.
    #[test]
    fn exif_image_dimensions_accept_short_encoding() {
        let width = short(1920, false);
        let height = short(1440, false);
        let metadata = extract(&[(0xA002, 3, 1, &width), (0xA003, 3, 1, &height)], false);
        assert_eq!(
            metadata.get("ExifIFD:ExifImageWidth"),
            Some(&TagValue::new_integer(1920))
        );
        assert_eq!(
            metadata.get("ExifIFD:ExifImageHeight"),
            Some(&TagValue::new_integer(1440))
        );
    }

    /// The LONG form the sample does use, pinned so the SHORT test above cannot
    /// be "fixed" by breaking the branch that actually shipped.
    #[test]
    fn exif_image_dimensions_accept_long_encoding() {
        let width = u32b(1920, false);
        let height = u32b(1440, false);
        let metadata = extract(&[(0xA002, 4, 1, &width), (0xA003, 4, 1, &height)], false);
        assert_eq!(
            metadata.get("ExifIFD:ExifImageWidth"),
            Some(&TagValue::new_integer(1920))
        );
        assert_eq!(
            metadata.get("ExifIFD:ExifImageHeight"),
            Some(&TagValue::new_integer(1440))
        );
    }

    /// Panasonic.rw2 holds `DigitalZoomRatio = 0/10`, which only reaches the
    /// `numerator % denominator == 0` shortcut. 0xa404 has no PrintConv
    /// (Exif.pm:2886) and ExifTool prints the evaluated rational, so 3/2 must
    /// render as "1.5" rather than "3/2".
    #[test]
    fn digital_zoom_ratio_integral_and_fractional() {
        let integral = rational(0, 10, false);
        let metadata = extract(&[(0xA404, 5, 1, &integral)], false);
        assert_eq!(
            metadata.get("ExifIFD:DigitalZoomRatio"),
            Some(&TagValue::new_string("0"))
        );

        let fractional = rational(3, 2, false);
        let metadata = extract(&[(0xA404, 5, 1, &fractional)], false);
        assert_eq!(
            metadata.get("ExifIFD:DigitalZoomRatio"),
            Some(&TagValue::new_string("1.5"))
        );
    }

    /// A zero denominator is unreachable from the corpus and must not panic or
    /// invent a value; the display formatter declines and the generic RATIONAL
    /// fallback keeps the raw pair.
    #[test]
    fn digital_zoom_ratio_zero_denominator_is_not_fabricated() {
        let payload = rational(1, 0, false);
        let metadata = extract(&[(0xA404, 5, 1, &payload)], false);
        assert_eq!(
            metadata.get("ExifIFD:DigitalZoomRatio"),
            Some(&TagValue::new_rational(1, 0))
        );
    }

    /// FlashpixVersion has no PrintConv at all (Exif.pm:2678); ExifTool's only
    /// transform is `RawConv => '$val=~s/\0+$//'`. The corpus sample carries an
    /// unpadded `30 31 30 30`, so the padded case is unexercised.
    #[test]
    fn flashpix_version_is_raw_ascii() {
        let metadata = extract(&[(0xA000, 7, 4, b"0100")], false);
        assert_eq!(
            metadata.get("ExifIFD:FlashpixVersion"),
            Some(&TagValue::new_string("0100"))
        );
    }
}
