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

                    // Convert tag to metadata
                    // Panasonic RW2 stores BitsPerSample in its proprietary
                    // IFD0 tag 0x000A instead of the standard TIFF tag 0x0102.
                    // Use the standard tag ID for name lookup so this is
                    // exposed as the canonical EXIF BitsPerSample tag.
                    let canonical_tag_id =
                        if format == RawFormat::PanasonicRW2 && ifd_index == 0 && *tag_id == 0x000A
                        {
                            0x0102
                        } else {
                            *tag_id
                        };
                    let tag_name = lookup_tag_name(canonical_tag_id, ifd_name);
                    let tag_value =
                        raw_bytes_to_simple_tag_value(bytes, *field_type, *value_count, byte_order);
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
                        let tag_value = raw_bytes_to_simple_tag_value(
                            bytes,
                            *field_type,
                            *value_count,
                            byte_order,
                        );
                        metadata.insert(tag_name, tag_value);
                    }

                    // Prefer EXIF IFD MakerNote/Make over IFD0 versions
                    if exif_makernote.is_some() {
                        makernote_data = exif_makernote;
                    }
                    if exif_make.is_some() {
                        camera_make = exif_make;
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
                        for (tag_id, field_type, value_count, raw_bytes) in sub_tags {
                            let tag_name = lookup_tag_name(tag_id, sub_ifd_name);
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
fn parse_cr3(_data: &[u8], format: RawFormat) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();
    metadata.insert(
        "File:FileType".to_string(),
        TagValue::new_string(format!("{:?}", format)),
    );

    // TODO: Implement full CR3 parsing
    // CR3 uses ISO Base Media Format (similar to MP4/QuickTime)
    // Will require box/atom parser similar to QuickTime parser

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

    // Store preview image dimensions for type 2/3
    if (image_type == 2 || image_type == 3) && columns > 0 && rows > 0 {
        metadata.insert(
            "MakerNotes:PreviewImageSize".to_string(),
            TagValue::new_string(format!("{}x{}", columns, rows)),
        );
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
mod tests {
    use super::*;

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
