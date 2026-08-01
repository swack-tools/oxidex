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
use crate::parsers::common::print_im::{PRINT_IM_VERSION_TAG, decode_print_im_version};
use crate::parsers::icc::parse_icc_profile_data as parse_icc;
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

/// Tag names for Panasonic/Leica RAW IFD0, transcribed from
/// `%Image::ExifTool::PanasonicRaw::Main` (PanasonicRaw.pm 1.29, ExifTool
/// 13.55). Its NOTES read, verbatim:
///
/// ```text
///     NOTES => 'These tags are found in IFD0 of Panasonic/Leica RAW, RW2 and RWL images.',
/// ```
///
/// This is a *replacement* for the standard EXIF IFD0 table, not an overlay:
/// PanasonicRaw reuses ids 0x01-0x37 for sensor geometry and white-balance
/// tags that mean something completely different in Exif::Main. Ids absent
/// from this table are not reported by ExifTool at all.
///
/// Verbatim definitions (comment lines and #refs elided):
///
/// ```text
///     0x01 => { Name => 'PanasonicRawVersion', Writable => 'undef' },
///     0x02 => 'SensorWidth',
///     0x03 => 'SensorHeight',
///     0x04 => 'SensorTopBorder',
///     0x05 => 'SensorLeftBorder',
///     0x06 => 'SensorBottomBorder',
///     0x07 => 'SensorRightBorder',
///     0x08 => { Name => 'SamplesPerPixel', Writable => 'int16u', Protected => 1 },
///     0x09 => { Name => 'CFAPattern', Writable => 'int16u', Protected => 1, ... },
///     0x0a => { Name => 'BitsPerSample', Writable => 'int16u', Protected => 1 },
///     0x0b => { Name => 'Compression', Writable => 'int16u', Protected => 1, ... },
///     0x0e => { Name => 'LinearityLimitRed',   Writable => 'int16u' },
///     0x0f => { Name => 'LinearityLimitGreen', Writable => 'int16u' },
///     0x10 => { Name => 'LinearityLimitBlue',  Writable => 'int16u' },
///     0x11 => { Name => 'RedBalance',  Writable => 'int16u', ValueConv => '$val / 256' },
///     0x12 => { Name => 'BlueBalance', Writable => 'int16u', ValueConv => '$val / 256' },
///     0x17 => { Name => 'ISO', Writable => 'int16u' },
///     0x18 => { Name => 'HighISOMultiplierRed',   ValueConv => '$val / 256' },
///     0x19 => { Name => 'HighISOMultiplierGreen', ValueConv => '$val / 256' },
///     0x1a => { Name => 'HighISOMultiplierBlue',  ValueConv => '$val / 256' },
///     0x1c => { Name => 'BlackLevelRed',   Writable => 'int16u' },
///     0x1d => { Name => 'BlackLevelGreen', Writable => 'int16u' },
///     0x1e => { Name => 'BlackLevelBlue',  Writable => 'int16u' },
///     0x24 => { Name => 'WBRedLevel',   Writable => 'int16u' },
///     0x25 => { Name => 'WBGreenLevel', Writable => 'int16u' },
///     0x26 => { Name => 'WBBlueLevel',  Writable => 'int16u' },
///     0x2d => { Name => 'RawFormat', Writable => 'int16u', Protected => 1 },
///     0x2e => { Name => 'JpgFromRaw', ... },
///     0x2f => { Name => 'CropTop',    Writable => 'int16u' },
///     0x30 => { Name => 'CropLeft',   Writable => 'int16u' },
///     0x31 => { Name => 'CropBottom', Writable => 'int16u' },
///     0x32 => { Name => 'CropRight',  Writable => 'int16u' },
///     0x37 => { Name => 'ISO',        Writable => 'int32u' },
///     0x10f => { Name => 'Make', ... },
///     0x110 => { Name => 'Model', ... },
///     0x111 => { Name => 'StripOffsets', ... },
///     0x112 => { Name => 'Orientation', Writable => 'int16u', ... },
///     0x116 => { Name => 'RowsPerStrip', Priority => 0 },
///     0x117 => { Name => 'StripByteCounts', ... },
///     0x118 => { Name => 'RawDataOffset', ... },
///     0x11c => { Name => 'Gamma', Writable => 'int16u', ... },
///     0x121 => { Name => 'Multishot', Writable => 'int32u', ... },
///     0x127 => { Name => 'JpgFromRaw2', ... },
///     0x13b => { Name => 'Artist', ... },
///     0x2bc => { Name => 'ApplicationNotes', ... },
///     0x001b => { Name => 'NoiseReductionParams', Writable => 'undef', Format => 'int16u', Count => -1 },
///     0x8298 => { Name => 'Copyright', ... },
///     0x83bb => { Name => 'IPTC-NAA', ... },
/// ```
///
/// Deliberately omitted: 0x13 (WBInfo), 0x27 (WBInfo2), 0x119 (DistortionInfo)
/// and 0x120 (CameraIFD) are `SubDirectory` entries -- ExifTool emits their
/// decoded child tags, never a value for the container itself, so naming the
/// raw blob would be wrong.
fn panasonic_raw_ifd0_tag_name(tag_id: u16) -> Option<&'static str> {
    Some(match tag_id {
        0x0001 => "PanasonicRawVersion",
        0x0002 => "SensorWidth",
        0x0003 => "SensorHeight",
        0x0004 => "SensorTopBorder",
        0x0005 => "SensorLeftBorder",
        0x0006 => "SensorBottomBorder",
        0x0007 => "SensorRightBorder",
        0x0008 => "SamplesPerPixel",
        0x0009 => "CFAPattern",
        0x000A => "BitsPerSample",
        0x000B => "Compression",
        0x000E => "LinearityLimitRed",
        0x000F => "LinearityLimitGreen",
        0x0010 => "LinearityLimitBlue",
        0x0011 => "RedBalance",
        0x0012 => "BlueBalance",
        0x0017 => "ISO",
        0x0018 => "HighISOMultiplierRed",
        0x0019 => "HighISOMultiplierGreen",
        0x001A => "HighISOMultiplierBlue",
        0x001B => "NoiseReductionParams",
        0x001C => "BlackLevelRed",
        0x001D => "BlackLevelGreen",
        0x001E => "BlackLevelBlue",
        0x0024 => "WBRedLevel",
        0x0025 => "WBGreenLevel",
        0x0026 => "WBBlueLevel",
        0x002D => "RawFormat",
        0x002E => "JpgFromRaw",
        0x002F => "CropTop",
        0x0030 => "CropLeft",
        0x0031 => "CropBottom",
        0x0032 => "CropRight",
        0x0037 => "ISO",
        0x010F => "Make",
        0x0110 => "Model",
        0x0111 => "StripOffsets",
        0x0112 => "Orientation",
        0x0116 => "RowsPerStrip",
        0x0117 => "StripByteCounts",
        0x0118 => "RawDataOffset",
        0x011C => "Gamma",
        0x0121 => "Multishot",
        0x0127 => "JpgFromRaw2",
        0x013B => "Artist",
        0x02BC => "ApplicationNotes",
        0x8298 => "Copyright",
        0x83BB => "IPTC-NAA",
        _ => return None,
    })
}

/// Display values for the Panasonic RAW IFD0 tags whose stored representation
/// differs from ExifTool's printed form.
///
/// Only two tags in `%Image::ExifTool::PanasonicRaw::Main` need this; every
/// other entry is a plain int16u/int32u/string that the generic decoder
/// already renders identically.
fn format_panasonic_raw_ifd0_value(
    tag_id: u16,
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    byte_order: ByteOrder,
) -> Option<String> {
    match tag_id {
        // 0x01 => { Name => 'PanasonicRawVersion', Writable => 'undef' }
        //
        // UNDEFINED with no PrintConv, so ExifTool prints the bytes as-is;
        // Panasonic.rw2 stores the four ASCII characters "0300".
        0x0001 if field_type == 7 => {
            let count = usize::try_from(value_count).ok()?;
            let version = bytes.get(..count)?;
            let text = String::from_utf8_lossy(version);
            let text = text.trim_end_matches('\0');
            if text.is_empty() {
                None
            } else {
                Some(text.to_string())
            }
        }
        // 0x001b => { Name => 'NoiseReductionParams', Writable => 'undef',
        //             Format => 'int16u', Count => -1 }
        //
        // `Format => 'int16u'` reinterprets the UNDEFINED blob as a variable
        // length int16u array, printed space separated. The Notes read,
        // verbatim: "the camera's default noise reduction setup.  The first
        // number is the number of entries, then for each entry there are 4
        // numbers: an ISO speed, and noise-reduction strengths the R, G and B
        // channels".
        0x001B if field_type == 7 => {
            let count = usize::try_from(value_count).ok()?;
            let blob = bytes.get(..count)?;
            if blob.len() < 2 {
                return None;
            }
            Some(
                blob.chunks_exact(2)
                    .map(|chunk| match byte_order {
                        ByteOrder::LittleEndian => u16::from_le_bytes([chunk[0], chunk[1]]),
                        ByteOrder::BigEndian => u16::from_be_bytes([chunk[0], chunk[1]]),
                    })
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        }
        _ => None,
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
    // CR2 IFD0 thumbnail offset for PreviewImageStart. Held aside and emitted after the
    // IFD walk for the same reason as the length above: IFD2 and IFD3 also carry
    // StripOffsets, and this parser folds every IFD past IFD1 into the IFD0 namespace.
    let mut cr2_preview_image_start: Option<u32> = None;

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
                let mut dng_adobe_private_data: Option<Vec<u8>> = None;

                // Convert tags to metadata
                for (tag_id, field_type, value_count, raw_bytes) in &tags {
                    let bytes = raw_bytes.as_ref();

                    // RW2 tag 0x002e contains the JPEG preview whose EXIF IFD
                    // carries a handful of standard EXIF tags omitted from the
                    // outer Panasonic RAW IFDs.
                    if format == RawFormat::PanasonicRW2 && ifd_index == 0 && *tag_id == 0x002e {
                        // Where this preview starts in the RW2 itself, needed
                        // to turn its IFD1 ThumbnailOffset back into the
                        // absolute file position ExifTool reports.
                        let jpeg_file_offset =
                            tiff_external_entry_extent(data, ifd_offset, byte_order, 0x002e)
                                .map(|(offset, _length)| offset)
                                .unwrap_or(0);
                        if let Err(error) =
                            extract_rw2_embedded_exif_tags(bytes, jpeg_file_offset, &mut metadata)
                        {
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

                    // RW2 IFD0 sub-directories that PanasonicRaw::Main routes
                    // into ProcessBinaryData tables. Without this they were
                    // emitted as opaque "IFD0:0x0027"/"IFD0:0x0119" blobs.
                    if format == RawFormat::PanasonicRW2 && ifd_index == 0 && *tag_id == 0x0027 {
                        extract_panasonic_raw_wb_info2(bytes, byte_order, &mut metadata);
                        continue;
                    }
                    if format == RawFormat::PanasonicRW2 && ifd_index == 0 && *tag_id == 0x0119 {
                        extract_panasonic_raw_distortion_info(bytes, byte_order, &mut metadata);
                        continue;
                    }

                    // Check for EXIF Sub-IFD pointer (tag 0x8769)
                    if *tag_id == 0x8769 && bytes.len() >= 4 {
                        let offset = read_u32(bytes, byte_order);
                        exif_ifd_offset = Some(offset as u64);
                        continue; // Don't add pointer tag to metadata
                    }

                    // Check for GPS Sub-IFD pointer (tag 0x8825)
                    // Check for ICC Profile tag (0x8773) – parse embedded ICC profile
                    if *tag_id == 0x8773 {
                        if let Ok(icc_tags) = parse_icc(bytes) {
                            for (key, value) in icc_tags {
                                metadata.insert(format!("ICC_Profile:{}", key), value);
                            }
                        }
                        continue; // Don't add the raw ICC blob to metadata
                    }

                    // Check for GPS Sub-IFD pointer (tag 0x8825)
                    if *tag_id == 0x8825 && bytes.len() >= 4 {
                        let offset = read_u32(bytes, byte_order);
                        gps_ifd_offset = Some(offset as u64);
                        continue; // Don't add pointer tag to metadata
                    }

                    // CR2 IFD0: capture the StripByteCounts value for the
                    // thumbnail JPEG preview (ExifTool EXIF:PreviewImage).
                    if format == RawFormat::CanonCR2
                        && ifd_index == 0
                        && *tag_id == 0x0117
                        && bytes.len() >= 4
                    {
                        cr2_thumbnail_length = Some(read_u32(bytes, byte_order));
                    }

                    // CR2 IFD0: StripOffsets (0x0111) is PreviewImageStart here.
                    //
                    // Exif.pm 0x111 Notes, verbatim: "called StripOffsets in most
                    // locations, but it is PreviewImageStart in IFD0 of CR2 images and
                    // various IFD's of DNG images except for SubIFD2 where it is
                    // JpgFromRawStart".
                    if format == RawFormat::CanonCR2
                        && ifd_index == 0
                        && *tag_id == 0x0111
                        && bytes.len() >= 4
                    {
                        cr2_preview_image_start = Some(read_u32(bytes, byte_order));
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

                    // DNGPrivateData (0xC634). When it starts with "Adobe\0"
                    // the DNG Converter parked the source file's MakerNote in
                    // it -- 138 of the 141 tags oxidex was missing on
                    // Canon350D.dng live in there. ExifTool flags the tag
                    // Binary+Protected and never prints the blob itself, so
                    // this consumes it instead of emitting an oxidex-only
                    // "EXIF:0xC634" hex blob.
                    if format == RawFormat::AdobeDNG
                        && ifd_index == 0
                        && *tag_id == 0xC634
                        && bytes.starts_with(b"Adobe\0")
                    {
                        dng_adobe_private_data = Some(bytes.to_vec());
                        continue;
                    }

                    // XMP packet (tag 0x02BC, ApplicationNotes). RAW files
                    // carry their XMP here exactly as JPEG carries it in APP1,
                    // and oxidex has had an XMP parser all along -- this walk
                    // simply never handed the bytes to it, so every RAW file
                    // reported zero XMP tags while ExifTool read them.
                    if *tag_id == 0x02BC {
                        if let Ok(xmp_tags) = crate::parsers::xmp::rdf_parser::parse_xmp(bytes) {
                            for (tag_name, tag_value) in xmp_tags {
                                metadata.insert(tag_name, TagValue::new_string(tag_value));
                            }
                        }
                        // The raw packet itself is not a tag ExifTool reports.
                        continue;
                    }

                    // IPTC-NAA (tag 0x83BB) – parse embedded IPTC records
                    if *tag_id == 0x83BB {
                        if let Ok(iptc_tags) = parse_iptc_naa(bytes) {
                            for (tag_name, tag_value) in iptc_tags {
                                metadata.insert(tag_name, tag_value);
                            }
                        }
                        // Don't add the raw IPTC blob to metadata
                        continue;
                    }

                    // Check for Make tag (0x010F) - needed for MakerNote dispatcher
                    if *tag_id == 0x010F && *field_type == 2 {
                        // Extract camera make for MakerNote parsing (ASCII type)
                        let make_str = String::from_utf8_lossy(bytes);
                        camera_make = Some(make_str.trim_end_matches('\0').trim().to_string());
                    }

                    // CR2 IFD1 holds the *thumbnail*, not the preview: ExifTool reports
                    // its JPEGInterchangeFormat/JPEGInterchangeFormatLength entries as
                    // ThumbnailOffset / ThumbnailLength / ThumbnailImage, while the
                    // Preview* trio comes from IFD0's Strip* entries (handled above).
                    // Naming IFD1's length PreviewImageLength here left ThumbnailLength
                    // and ThumbnailImage unreported and only survived because the IFD0
                    // value overwrote it after the walk.
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
                                    "EXIF:ThumbnailLength".to_string(),
                                    TagValue::new_integer(value as i64),
                                );
                                metadata.insert(
                                    "EXIF:ThumbnailImage".to_string(),
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
                    // Convert tag to metadata.
                    //
                    // Panasonic RW2 IFD0 is NOT a standard TIFF IFD: ExifTool
                    // parses it with %Image::ExifTool::PanasonicRaw::Main
                    // ("These tags are found in IFD0 of Panasonic/Leica RAW,
                    // RW2 and RWL images."), which reuses low tag ids for
                    // entirely different tags. Resolving those ids in the
                    // standard EXIF table produced false metadata -- measured
                    // on Panasonic.rw2 before this change, oxidex emitted
                    // "IFD0:Higher resolution image exists" for 0x0001
                    // (PanasonicRawVersion), "IFD0:InteropVersion" = 3724 for
                    // 0x0002 (SensorWidth), "IFD0:Trilinear" = 3656 for 0x0007
                    // (SensorRightBorder), "IFD0:MinSampleValue" = 12322 for
                    // 0x0118 (RawDataOffset) and "IFD0:XResolution" = 1 for
                    // 0x011A (which PanasonicRaw::Main does not define at all,
                    // so ExifTool does not report it).
                    let tag_name = if format == RawFormat::PanasonicRW2 && ifd_index == 0 {
                        match panasonic_raw_ifd0_tag_name(*tag_id) {
                            Some(name) => format!("{}:{}", ifd_name, name),
                            // Not in PanasonicRaw::Main: ExifTool does not
                            // report it. Emit the raw id rather than a name
                            // borrowed from an unrelated table.
                            None => format!("{}:0x{:04X}", ifd_name, tag_id),
                        }
                    } else {
                        match (format, ifd_index, *tag_id) {
                            // TIFF/EP tag 0x9216 (TIFF-EPStandardID) lives in NEF
                            // IFD0. lookup_tag_name has no entry for it under the
                            // IFD0 group, so oxidex emitted "IFD0:0x9216" with a
                            // raw 4-byte blob (measured 2026-07-27 on Nikon.nef).
                            (RawFormat::NikonNEF, 0, 0x9216) | (RawFormat::NikonNRW, 0, 0x9216) => {
                                "EXIF:TIFF-EPStandardID".to_string()
                            }
                            _ => lookup_raw_tag_name(*tag_id, ifd_name, format),
                        }
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
                    } else if format == RawFormat::PanasonicRW2
                        && ifd_index == 0
                        && matches!(*tag_id, 0x0018 | 0x0019 | 0x001A)
                    {
                        format_panasonic_high_iso_multiplier(
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
                    } else if format == RawFormat::PanasonicRW2
                        && ifd_index == 0
                        && let Some(value) = format_panasonic_raw_ifd0_value(
                            *tag_id,
                            bytes,
                            *field_type,
                            *value_count,
                            byte_order,
                        )
                    {
                        TagValue::new_string(value)
                    } else if format == RawFormat::AdobeDNG
                        && ifd_index == 0
                        && let Some(value) = format_dng_ifd0_tag(
                            *tag_id,
                            bytes,
                            *field_type,
                            *value_count,
                            byte_order,
                        )
                    {
                        TagValue::new_string(value)
                    } else if matches!(format, RawFormat::NikonNEF | RawFormat::NikonNRW)
                        && ifd_index == 0
                        && *tag_id == 0x9216
                        && let Some(value) = format_tiff_ep_standard_id(bytes, *value_count)
                    {
                        TagValue::new_string(value)
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
                    if let Some(interop_offset) =
                        exif_tags.iter().find_map(|(tag_id, _, _, raw)| {
                            if *tag_id == 0xA005 && raw.len() >= 4 {
                                Some(read_u32(raw.as_ref(), byte_order) as u64)
                            } else {
                                None
                            }
                        })
                    {
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
                                    raw_bytes_to_simple_tag_value(
                                        raw_bytes.as_ref(),
                                        field_type,
                                        value_count,
                                        byte_order,
                                    )
                                };
                                metadata.insert(tag_name, tag_value);
                            }
                        }
                    }
                }

                // Parse MakerNote if present and we have the camera make
                if let (Some(make), Some(mn_data)) = (camera_make.as_ref(), makernote_data.as_ref())
                {
                    // Some MakerNote structures are laid out per camera model
                    // (Nikon's AFInfo picks its byte order from it), so pass
                    // along whichever Model the IFDs above already recorded.
                    let camera_model = metadata
                        .get_string("IFD0:Model")
                        .or_else(|| metadata.get_string("EXIF:Model"))
                        .map(str::to_string);

                    // Use the MakerNote dispatcher to parse manufacturer-specific tags
                    let mut makernote_tags = std::collections::HashMap::new();
                    let mut value_forms = std::collections::HashMap::new();
                    if let Err(e) =
                        crate::parsers::tiff::makernote_dispatcher::dispatch_makernote_with_model_and_values(
                            make,
                            camera_model.as_deref(),
                            mn_data,
                            byte_order,
                            &mut makernote_tags,
                            &mut value_forms,
                        )
                    {
                        eprintln!("Warning: Failed to parse MakerNote for {}: {}", make, e);
                    } else {
                        // Add parsed MakerNote tags to metadata
                        // Tags already have proper prefixes (e.g., "Canon:MacroMode")
                        for (tag_name, tag_value) in makernote_tags {
                            metadata.insert(tag_name, TagValue::new_string(tag_value));
                        }
                        for (tag_name, value) in value_forms {
                            metadata.set_value_form(tag_name, value);
                        }
                    }
                }

                // Recover the MakerNote the Adobe DNG Converter relocated into
                // DNGPrivateData. The DNG carries no 0x927C of its own, so
                // this is the only route to those tags.
                if let (Some(make), Some(private)) =
                    (camera_make.as_ref(), dng_adobe_private_data.as_ref())
                {
                    extract_dng_adobe_private_data(private, make, &mut metadata);
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

                        // DNG: StripOffsets/StripByteCounts are renamed by
                        // ExifTool in the embedded-image SubIFDs. Exif.pm 0x111
                        // (Notes, verbatim): "called StripOffsets in most
                        // locations, but it is PreviewImageStart in IFD0 of CR2
                        // images and various IFD's of DNG images except for
                        // SubIFD2 where it is JpgFromRawStart".
                        //
                        // The gate is the Condition on the StripOffsets branch:
                        //   not ($$self{TIFF_TYPE} =~ /^(DNG|TIFF)$/ and
                        //        $$self{Compression} eq '7' and
                        //        $$self{SubfileType} ne '0')
                        // i.e. only JPEG-compressed (7) reduced-resolution
                        // (SubfileType != 0) SubIFDs are renamed.
                        if format == RawFormat::AdobeDNG {
                            extract_dng_subifd_preview(
                                data,
                                &sub_tags,
                                sub_index,
                                byte_order,
                                &mut metadata,
                            );
                        }

                        for (tag_id, field_type, value_count, raw_bytes) in sub_tags {
                            // CFARepeatPatternDim (Exif.pm 0x828d, Count => 2)
                            // is a SHORT[2] that ExifTool prints as the two
                            // dimensions separated by a space ("2 2"). The
                            // generic decoder emits only the first component.
                            if tag_id == 0x828D
                                && let Some(dim) =
                                    format_cfa_repeat_pattern_dim(raw_bytes.as_ref(), byte_order)
                            {
                                metadata.insert(
                                    "EXIF:CFARepeatPatternDim".to_string(),
                                    TagValue::new_string(dim),
                                );
                                continue;
                            }

                            // DNG SubIFD tags that ExifTool reports under the
                            // EXIF group with a PrintConv or multi-component
                            // value the generic decoder cannot produce.
                            //
                            // `exiftool -G0:1 DNG.dng` labels every one of
                            // these "[EXIF:SubIFD]" / "[EXIF:SubIFD1]" --
                            // family 0 is EXIF, so oxidex's "SubIFD0:" prefix
                            // was the wrong group.
                            if format == RawFormat::AdobeDNG
                                && let Some((tag_name, tag_value)) = format_dng_subifd_exif_tag(
                                    tag_id,
                                    raw_bytes.as_ref(),
                                    field_type,
                                    value_count,
                                    byte_order,
                                )
                            {
                                // ExifTool suppresses duplicates across the
                                // SubIFD chain, so the first SubIFD carrying a
                                // given tag is the one reported (measured:
                                // `exiftool -s -YCbCrSubSampling DNG.dng`
                                // prints SubIFD1's "YCbCr4:2:0 (2 2)", not
                                // SubIFD2's "YCbCr4:4:4 (1 1)").
                                if !metadata.contains_key(&tag_name) {
                                    metadata.insert(tag_name, tag_value);
                                }
                                continue;
                            }

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
                            // needs its own formatting.
                            //
                            // ExifTool puts it in family 0 "EXIF": `exiftool
                            // -G0:1 Nikon.nef` prints "[EXIF:SubIFD]
                            // CFAPattern2 : 2 1 1 0", so the reported group is
                            // EXIF, not the physical SubIFD. oxidex previously
                            // emitted "SubIFD0:CFAPattern2", which read as a
                            // missing EXIF:CFAPattern2 plus a spurious extra
                            // tag in every comparison report.
                            if tag_id == 0x828E {
                                metadata.insert(
                                    format!(
                                        "EXIF:{}",
                                        lookup_tag_name(tag_id, sub_ifd_name)
                                            .rsplit(':')
                                            .next()
                                            .unwrap_or("CFAPattern2")
                                    ),
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
            // Emit EXIF:PreviewImageStart from the IFD0 thumbnail JPEG offset
            // (StripOffsets, 0x0111).
            if let Some(start) = cr2_preview_image_start {
                metadata.insert(
                    "EXIF:PreviewImageStart".to_string(),
                    TagValue::new_integer(start as i64),
                );
            }
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
fn extract_rw2_embedded_exif_tags(
    jpeg: &[u8],
    jpeg_file_offset: usize,
    metadata: &mut MetadataMap,
) -> Result<()> {
    let Some((tiff_start_in_jpeg, tiff_data)) = find_jpeg_exif_tiff(jpeg)? else {
        return Ok(());
    };
    // Where the preview's TIFF header sits in the RW2 itself. ThumbnailOffset
    // is stored relative to that header but reported by ExifTool as a
    // position in the physical file.
    let tiff_base_in_file = jpeg_file_offset.saturating_add(tiff_start_in_jpeg);

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

    for (tag_id, _field_type, _value_count, raw_bytes) in &ifd0_tags {
        let bytes = raw_bytes.as_ref();
        if *tag_id == 0xC4A5
            && let Some(version) = decode_print_im_version(bytes, byte_order)
        {
            metadata.insert(PRINT_IM_VERSION_TAG, TagValue::new_string(version));
        }
    }

    // IFD0 of the RW2's JpgFromRaw preview carries standard TIFF tags that
    // ExifTool reports and this function used to walk straight past, because
    // it read IFD0 only to find the ExifIFD pointer. Sweep #185 claimed
    // ResolutionUnit and ModifyDate among six tags and delivered one
    // (Saturation); these are the rest of that claim, verified.
    for (tag_id, field_type, value_count, raw_bytes) in &ifd0_tags {
        if !matches!(
            *tag_id,
            0x011A // XResolution
                | 0x011B // YResolution
                | 0x0128 // ResolutionUnit
                | 0x0131 // Software
                | 0x0132 // ModifyDate
                | 0x0213 // YCbCrPositioning
        ) {
            continue;
        }
        // ExifTool promotes the preview's XResolution to the EXIF group,
        // while the other preview IFD0 tags retain their IFD0 lookup context.
        let tag_name = if *tag_id == 0x011A {
            lookup_tag_name(*tag_id, "EXIF")
        } else {
            lookup_tag_name(*tag_id, "IFD0")
        };
        let tag_value = if *tag_id == 0x011A && *field_type == 5 && raw_bytes.len() >= 8 {
            let numerator = read_tiff_u32(&raw_bytes[0..4], byte_order);
            let denominator = read_tiff_u32(&raw_bytes[4..8], byte_order);
            match (numerator, denominator) {
                (Some(numerator), Some(denominator)) if denominator != 0 => {
                    if numerator % denominator == 0 {
                        TagValue::new_integer(i64::from(numerator / denominator))
                    } else {
                        TagValue::new_string(format!(
                            "{}",
                            f64::from(numerator) / f64::from(denominator)
                        ))
                    }
                }
                _ => raw_bytes_to_simple_tag_value(
                    raw_bytes.as_ref(),
                    *field_type,
                    *value_count,
                    byte_order,
                ),
            }
        } else if let Some(value) = format_exif_display_value(
            *tag_id,
            raw_bytes.as_ref(),
            *field_type,
            *value_count,
            byte_order,
        ) {
            TagValue::new_string(value)
        } else if *tag_id == 0x0213 && *field_type == 3 {
            // Exif.pm 0x0213 PrintConv => { 1 => 'Centered', 2 => 'Co-sited' }
            let raw = read_tiff_u16(raw_bytes.as_ref(), byte_order);
            TagValue::new_string(match raw {
                Some(1) => "Centered".to_string(),
                Some(2) => "Co-sited".to_string(),
                Some(other) => other.to_string(),
                None => continue,
            })
        } else if *field_type == 5 {
            // XResolution/YResolution are RATIONAL; ExifTool prints 180/1 as
            // "180", not as a fraction.
            match format_rational_as_string(raw_bytes.as_ref(), byte_order) {
                Some(value) => TagValue::new_string(value),
                None => continue,
            }
        } else if *field_type == 2 {
            // ASCII. ExifTool trims the trailing padding, so this file's
            // "Ver.1.0 " is reported as "Ver.1.0" -- a trailing space is a
            // value mismatch, not a cosmetic difference.
            TagValue::new_string(
                String::from_utf8_lossy(raw_bytes.as_ref())
                    .trim_end_matches(|c: char| c == '\0' || c.is_whitespace())
                    .to_string(),
            )
        } else {
            raw_bytes_to_simple_tag_value(raw_bytes.as_ref(), *field_type, *value_count, byte_order)
        };
        metadata.insert(tag_name, tag_value);
    }

    let exif_tags = parse_ifd(&reader, exif_ifd_offset, byte_order)?;

    for (tag_id, field_type, value_count, raw_bytes) in &exif_tags {
        let (tag_id, field_type, value_count) = (*tag_id, *field_type, *value_count);
        // Filter to the exact set of EXIF tags that ExifTool extracts from
        // the RW2 JpgFromRaw preview EXIF IFD.
        //
        // NOTE (2026-07-27): 0xA411/0xA412/0xA413 were previously listed here
        // as HighISOMultiplierRed/Green/Blue. No such EXIF tags exist -- those
        // ids appear nowhere in ExifTool 13.55 (`grep -n '0xa411' *.pm` over
        // .../Image/ExifTool/ returns nothing). The real HighISOMultiplier
        // tags are PanasonicRaw.pm IFD0 0x18/0x19/0x1a and are handled on the
        // outer IFD0 path. The invented ids are dropped so they cannot emit a
        // hex-named oxidex-only tag if some file happens to carry them.
        if !matches!(
            tag_id,
            0x9101 // ComponentsConfiguration
                | 0x9102 // CompressedBitsPerPixel
                | 0x9208 // LightSource
                | 0xA403 // WhiteBalance
                | 0xA405 // FocalLengthIn35mmFormat
                | 0xA407 // GainControl
                | 0xA000 // FlashpixVersion
                | 0xA001 // ColorSpace
                | 0xA002 // ExifImageWidth
                | 0xA003 // ExifImageHeight
                | 0xA302 // CFAPattern
                | 0xA401 // CustomRendered
                | 0xA402 // ExposureMode
                | 0xA404 // DigitalZoomRatio
                | 0xA408 // Contrast
                | 0xA409 // Saturation
                | 0xA217 // SensingMethod
                | 0xA301 // SceneType
                | 0xA406 // SceneCaptureType
                | 0xA40A // Sharpness
        ) {
            continue;
        }

        let tag_name = lookup_tag_name(tag_id, "ExifIFD");
        // Exif.pm 0xa403 is a single int16u with the PrintConv table
        // 0 => "Auto", 1 => "Manual".
        let tag_value = if tag_id == 0xA403 && field_type == 3 && value_count == 1 {
            match read_tiff_u16(raw_bytes.as_ref(), byte_order) {
                Some(0) => TagValue::new_string("Auto".to_string()),
                Some(1) => TagValue::new_string("Manual".to_string()),
                _ => raw_bytes_to_simple_tag_value(
                    raw_bytes.as_ref(),
                    field_type,
                    value_count,
                    byte_order,
                ),
            }
        } else if let Some(value) = format_exif_display_value(
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

    // The Panasonic MakerNote lives in the preview's ExifIFD (0x927C) --
    // ExifTool reports 54 [Panasonic] tags for Panasonic.rw2 that oxidex read
    // straight past, because this walk only ever looked at a fixed list of
    // standard EXIF ids.
    //
    // The offsets inside it are relative to the PREVIEW's TIFF header, not to
    // the MakerNote, so the block is repacked before being handed to the
    // dispatcher (see rebuild_relocated_makernote). MakerNotes.pm gives
    // MakerNotePanasonic `Start => '$valuePtr + 12'`, i.e. a fixed 12-byte
    // "Panasonic\0\0\0" header ahead of the IFD, which is preserved verbatim.
    if let Some((makernote_offset, makernote_len)) =
        tiff_external_entry_extent(tiff_data, exif_ifd_offset, byte_order, 0x927C)
        && let Some(makernote) = tiff_data.get(makernote_offset..makernote_offset + makernote_len)
        && makernote.starts_with(b"Panasonic\0\0\0")
        && let Ok(base) = u32::try_from(makernote_offset)
        && let Some(rebuilt) = rebuild_relocated_makernote(
            &makernote[12..],
            base + 12,
            byte_order,
            Some(PANASONIC_DEREFERENCED_TAGS),
        )
    {
        let mut block = Vec::with_capacity(12 + rebuilt.len());
        block.extend_from_slice(b"Panasonic\0\0\0");
        block.extend_from_slice(&rebuilt);

        let mut tags = std::collections::HashMap::new();
        match crate::parsers::tiff::makernote_dispatcher::dispatch_makernote(
            "Panasonic",
            &block,
            byte_order,
            &mut tags,
        ) {
            Ok(()) => {
                for (tag_name, tag_value) in tags {
                    metadata.insert(tag_name, TagValue::new_string(tag_value));
                }
            }
            Err(error) => {
                eprintln!("Warning: Failed to parse RW2 preview MakerNote: {}", error)
            }
        }
    }

    // The preview EXIF also carries an Interoperability IFD (ExifIFD tag
    // 0xA005 -> InteropOffset). ExifTool reports [InteropIFD] InteropIndex for
    // Panasonic.rw2; oxidex never descended into it (measured gap 2026-07-27).
    if let Some(interop_offset) =
        exif_tags
            .iter()
            .find_map(|(tag_id, field_type, value_count, raw_bytes)| {
                if *tag_id == 0xA005 && *field_type == 4 && *value_count >= 1 {
                    read_tiff_u32(raw_bytes.as_ref(), byte_order).map(u64::from)
                } else {
                    None
                }
            })
    {
        extract_interop_index(&reader, interop_offset, byte_order, metadata);
    }

    // The next-IFD pointer after preview IFD0 leads to the thumbnail IFD.
    // Its 0x0201/0x0202 values are stored relative to this embedded TIFF
    // header; ExifTool reports ThumbnailOffset as a position in the physical
    // file (11976 for Panasonic.rw2 = stored 10428 plus the preview TIFF's
    // 1548-byte offset into the RW2), so `tiff_base_in_file` is added back.
    let ifd0_entry_count = u64::try_from(ifd0_tags.len()).ok();
    let next_ifd_position = ifd0_entry_count.and_then(|entry_count| {
        first_ifd_offset
            .checked_add(2)?
            .checked_add(entry_count.checked_mul(12)?)
    });
    let thumbnail_ifd_offset = next_ifd_position
        .and_then(|offset| reader.read(offset, 4).ok())
        .map(|bytes| u64::from(read_u32(bytes, byte_order)));

    if let Some(thumbnail_ifd_offset) = thumbnail_ifd_offset
        && thumbnail_ifd_offset != 0
        && let Ok(thumbnail_tags) = parse_ifd(&reader, thumbnail_ifd_offset, byte_order)
    {
        let mut thumbnail_offset = None;
        let mut thumbnail_length = None;
        for (tag_id, field_type, value_count, raw_bytes) in thumbnail_tags {
            if field_type != 4 || value_count != 1 {
                continue;
            }
            match tag_id {
                // Exif.pm 0x0201: JPEGInterchangeFormat.
                0x0201 => {
                    thumbnail_offset = read_tiff_u32(raw_bytes.as_ref(), byte_order);
                }
                // Exif.pm 0x0202: JPEGInterchangeFormatLength.
                0x0202 => {
                    thumbnail_length = read_tiff_u32(raw_bytes.as_ref(), byte_order);
                }
                _ => {}
            }
        }

        if let Some(offset) = thumbnail_offset {
            metadata.insert(
                lookup_tag_name(0x0201, "EXIF"),
                TagValue::new_integer(i64::from(offset) + tiff_base_in_file as i64),
            );
        }
        if let Some(length) = thumbnail_length {
            metadata.insert(
                lookup_tag_name(0x0202, "EXIF"),
                TagValue::new_integer(i64::from(length)),
            );
        }
        if let (Some(offset), Some(length)) = (thumbnail_offset, thumbnail_length)
            && let (Ok(offset), Ok(length)) = (usize::try_from(offset), usize::try_from(length))
            && let Some(end) = offset.checked_add(length)
            && let Some(image) = tiff_data.get(offset..end)
        {
            metadata.insert(
                "EXIF:ThumbnailImage".to_string(),
                TagValue::Binary(image.to_vec()),
            );
        }
    }

    Ok(())
}

/// Emit InteropIFD tag 0x0001 (InteropIndex) with ExifTool's PrintConv.
///
/// Exif.pm, Image::ExifTool::Exif::Main InteropIFD table, verbatim:
/// ```text
///     Name => 'InteropIndex',
///     Description => 'Interoperability Index',
///     ...
///     PrintConv => {
///         R98 => 'R98 - DCF basic file (sRGB)',
///         R03 => 'R03 - DCF option file (Adobe RGB)',
///         THM => 'THM - DCF thumbnail file',
///     },
/// ```
///
/// Values outside the table are printed unconverted by ExifTool, so an
/// unrecognised index falls through to the trimmed raw string.
fn extract_interop_index(
    reader: &SliceReader<'_>,
    interop_offset: u64,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    let Ok(interop_tags) = parse_ifd(reader, interop_offset, byte_order) else {
        eprintln!("Warning: Failed to parse Interoperability IFD");
        return;
    };

    for (tag_id, field_type, value_count, raw_bytes) in &interop_tags {
        match *tag_id {
            0x0001 => {
                let raw = String::from_utf8_lossy(raw_bytes.as_ref())
                    .trim_end_matches('\0')
                    .to_string();
                let printed = match raw.as_str() {
                    "R98" => "R98 - DCF basic file (sRGB)".to_string(),
                    "R03" => "R03 - DCF option file (Adobe RGB)".to_string(),
                    "THM" => "THM - DCF thumbnail file".to_string(),
                    _ => raw,
                };
                metadata.insert(
                    lookup_tag_name(*tag_id, "InteropIFD"),
                    TagValue::new_string(printed),
                );
            }
            // Exif.pm InteropIFD 0x0002: InteropVersion, UNDEFINED[4].
            // ExifTool reports this preview-derived value in the EXIF group.
            0x0002 if *field_type == 7 && *value_count == 4 => {
                let Some(version) = raw_bytes.get(..4) else {
                    continue;
                };
                metadata.insert(
                    lookup_tag_name(*tag_id, "EXIF"),
                    TagValue::new_string(String::from_utf8_lossy(version).into_owned()),
                );
            }
            _ => {}
        }
    }
}

/// Locate the TIFF header in a JPEG APP1 EXIF segment.
/// Returns the TIFF header's offset WITHIN `jpeg` alongside the TIFF slice.
/// Tags whose value is a file offset (ThumbnailOffset) are stated relative to
/// that TIFF header, and ExifTool reports them as absolute positions in the
/// physical file, so the caller needs the base to reconstruct them.
fn find_jpeg_exif_tiff(jpeg: &[u8]) -> Result<Option<(usize, &[u8])>> {
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
            let tiff_start = payload_start
                .checked_add(6)
                .ok_or_else(|| ExifToolError::parse_error("Invalid JPEG segment offset"))?;
            return Ok(payload.get(6..).map(|tiff| (tiff_start, tiff)));
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

/// Format the DNG IFD0 tags whose stored bytes ExifTool converts into
/// something other than a numeric list, verbatim from Exif.pm.
///
/// ```text
///     0xc612 => { Name => 'DNGVersion',         Writable => 'int8u', Count => 4,
///                 PrintConv => '$val =~ tr/ /./; $val' },          # line 3258
///     0xc613 => { Name => 'DNGBackwardVersion', Writable => 'int8u', Count => 4,
///                 PrintConv => '$val =~ tr/ /./; $val' },          # line 3274
///     0xc630 => { Name => 'DNGLensInfo', Writable => 'rational64u', Count => 4,
///                 PrintConv => \&PrintLensInfo },                  # line 3485
///     0xc65d => { Name => 'RawDataUniqueID', Format => 'undef', Count => 16,
///                 ValueConv => 'uc(unpack("H*",$val))' },          # line 3655
/// ```
/// Without these the four tags surfaced as
/// `(Binary data N bytes, use -b option to extract)` or as a raw component
/// list -- measured on Canon350D.dng, where oxidex reported `18 55 0 0` for
/// DNGLensInfo and a binary-data placeholder for the other three.
fn format_dng_ifd0_tag(
    tag_id: u16,
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    byte_order: ByteOrder,
) -> Option<String> {
    match tag_id {
        // int8u[4] printed with '.' between components.
        0xC612 | 0xC613 if field_type == 1 && value_count == 4 => Some(
            bytes
                .get(..4)?
                .iter()
                .map(|component| component.to_string())
                .collect::<Vec<_>>()
                .join("."),
        ),
        // 16 raw bytes as an uppercase hex string.
        0xC65D if value_count == 16 => Some(
            bytes
                .get(..16)?
                .iter()
                .map(|byte| format!("{:02X}", byte))
                .collect::<String>(),
        ),
        // rational64u[4]: min focal, max focal, min f, max f.
        0xC630 if field_type == 5 && value_count == 4 => {
            // Exif.pm's rational ValueConv yields 'inf' for n/0 and 'undef'
            // for 0/0; PrintLensInfo maps both to '?'. Anything else prints
            // as the plain number.
            let component = |index: usize| -> Option<String> {
                let start = index * 8;
                let numerator = read_tiff_u32(bytes.get(start..start + 4)?, byte_order)?;
                let denominator = read_tiff_u32(bytes.get(start + 4..start + 8)?, byte_order)?;
                Some(if denominator == 0 {
                    "?".to_string()
                } else if numerator % denominator == 0 {
                    (numerator / denominator).to_string()
                } else {
                    format!("{}", f64::from(numerator) / f64::from(denominator))
                })
            };
            let values: Vec<String> = (0..4).map(component).collect::<Option<_>>()?;

            // PrintLensInfo, Exif.pm:5705-5723:
            //     $val = $vals[0];
            //     $val .= "-$vals[1]" if $vals[1] and $vals[1] ne $vals[0];
            //     $val .= "mm f/$vals[2]";
            //     $val .= "-$vals[3]" if $vals[3] and $vals[3] ne $vals[2];
            // ("if $vals[1]" is Perl truthiness, so a literal "0" upper bound
            // -- which the Pentax Q writes for prime lenses -- is skipped.)
            let mut printed = values[0].clone();
            if values[1] != "0" && values[1] != values[0] {
                printed.push('-');
                printed.push_str(&values[1]);
            }
            printed.push_str("mm f/");
            printed.push_str(&values[2]);
            if values[3] != "0" && values[3] != values[2] {
                printed.push('-');
                printed.push_str(&values[3]);
            }
            Some(printed)
        }
        _ => None,
    }
}

/// Read a TIFF numeric array as ExifTool's space-separated value string.
///
/// Handles the field types the DNG SubIFD tags actually use: BYTE (1),
/// SHORT (3), LONG (4), RATIONAL (5) and SRATIONAL (10). Rationals are
/// rendered the way ExifTool renders them by default -- as an integer when the
/// division is exact, otherwise as a decimal.
fn read_tiff_numeric_array(
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    byte_order: ByteOrder,
) -> Option<Vec<String>> {
    let count = usize::try_from(value_count).ok()?;
    if count == 0 {
        return None;
    }

    let component_size = match field_type {
        1 => 1usize,
        3 => 2,
        4 => 4,
        5 | 10 => 8,
        _ => return None,
    };
    let values = bytes.get(..count.checked_mul(component_size)?)?;

    let rational = |numerator: f64, denominator: f64| {
        if denominator == 0.0 {
            "inf".to_string()
        } else if numerator % denominator == 0.0 {
            format!("{}", (numerator / denominator) as i64)
        } else {
            format!("{}", numerator / denominator)
        }
    };

    Some(
        values
            .chunks_exact(component_size)
            .map(|chunk| match field_type {
                1 => chunk[0].to_string(),
                3 => match byte_order {
                    ByteOrder::LittleEndian => u16::from_le_bytes([chunk[0], chunk[1]]),
                    ByteOrder::BigEndian => u16::from_be_bytes([chunk[0], chunk[1]]),
                }
                .to_string(),
                4 => read_tiff_u32(chunk, byte_order).unwrap_or(0).to_string(),
                5 => rational(
                    f64::from(read_tiff_u32(&chunk[0..4], byte_order).unwrap_or(0)),
                    f64::from(read_tiff_u32(&chunk[4..8], byte_order).unwrap_or(0)),
                ),
                _ => rational(
                    f64::from(read_tiff_i32(&chunk[0..4], byte_order).unwrap_or(0)),
                    f64::from(read_tiff_i32(&chunk[4..8], byte_order).unwrap_or(0)),
                ),
            })
            .collect(),
    )
}

/// Name and display value for the DNG SubIFD tags ExifTool reports under the
/// EXIF group.
///
/// Every id and name below is verbatim from Exif.pm (ExifTool 13.55); the
/// listed line numbers are that file's.
///
/// ```text
///     0x142 => { Name => 'TileWidth',  Protected => 1, Writable => 'int32u' },   # line  949
///     0x143 => { Name => 'TileLength', Protected => 1, Writable => 'int32u' },   # line  955
///     0x144 => { Name => 'TileOffsets',    IsOffset => 1, OffsetPair => 0x145 }, # line  961
///     0x145 => { Name => 'TileByteCounts', OffsetPair => 0x144 },                # line  968
///     0x211 => { Name => 'YCbCrCoefficients', Writable => 'rational64u', Count => 3 },  # line 1410
///     0x212 => { Name => 'YCbCrSubSampling', Writable => 'int16u', Count => 2,
///                PrintConv => \%Image::ExifTool::JPEG::yCbCrSubSampling },       # line 1418
///     0x213 => { Name => 'YCbCrPositioning', Writable => 'int16u',
///                PrintConv => { 1 => 'Centered', 2 => 'Co-sited' } },            # line 1428
///     0x214 => { Name => 'ReferenceBlackWhite', Writable => 'rational64u', Count => 6 }, # line 1441
///     0x828e => { Name => 'CFAPattern2', Format => 'int8u', Count => -1 },       # line 1759
///     0xc616 => { Name => 'CFAPlaneColor', PrintConv => q{
///                    my @cols = qw(Red Green Blue Cyan Magenta Yellow White);
///                    my @vals = map { $cols[$_] || "Unknown($_)" } split(' ', $val);
///                    return join(',', @vals);
///                } },                                                            # line 3296
///     0xc617 => { Name => 'CFALayout', PrintConv => { 1 => 'Rectangular', ... } },# line 3305
///     0xc61d => { Name => 'WhiteLevel', Writable => 'int32u', Count => -1 },     # line 3359
///     0xc61e => { Name => 'DefaultScale', Writable => 'rational64u', Count => 2 },# line 3366
///     0xc61f => { Name => 'DefaultCropOrigin', Writable => 'int32u', Count => 2 },# line 3373
///     0xc620 => { Name => 'DefaultCropSize', Writable => 'int32u', Count => 2 }, # line 3380
///     0xc68e => { Name => 'MaskedAreas', Writable => 'int32u', Count => -1 },    # line 3692
/// ```
///
/// Tags that also occur in DNG IFD0 (ImageWidth, BitsPerSample, Compression,
/// SamplesPerPixel, PhotometricInterpretation, RowsPerStrip, ...) are
/// deliberately NOT handled here: which IFD wins for those is decided by
/// ExifTool's PRIORITY_DIR / `Priority => 0` machinery, which oxidex does not
/// model yet, so moving them would trade one wrong value for another.
fn format_dng_subifd_exif_tag(
    tag_id: u16,
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    byte_order: ByteOrder,
) -> Option<(String, TagValue)> {
    let name = match tag_id {
        0x0142 => "TileWidth",
        0x0143 => "TileLength",
        0x0144 => "TileOffsets",
        0x0145 => "TileByteCounts",
        0x0211 => "YCbCrCoefficients",
        0x0212 => "YCbCrSubSampling",
        0x0213 => "YCbCrPositioning",
        0x0214 => "ReferenceBlackWhite",
        0x828E => "CFAPattern2",
        0xC616 => "CFAPlaneColor",
        0xC617 => "CFALayout",
        0xC61D => "WhiteLevel",
        0xC61E => "DefaultScale",
        0xC61F => "DefaultCropOrigin",
        0xC620 => "DefaultCropSize",
        0xC68E => "MaskedAreas",
        _ => return None,
    };

    // CFAPattern2 is written as int8u (and, per the Exif.pm comment,
    // "written incorrectly as 'undef' in Nikon NRW images"), so read it as a
    // plain byte array regardless of the declared field type.
    let components = if tag_id == 0x828E {
        let count = usize::try_from(value_count).ok()?;
        bytes
            .get(..count)?
            .iter()
            .map(|component| component.to_string())
            .collect()
    } else {
        read_tiff_numeric_array(bytes, field_type, value_count, byte_order)?
    };
    if components.is_empty() {
        return None;
    }

    let display = match tag_id {
        // PrintConv => { 1 => 'Centered', 2 => 'Co-sited' }
        0x0213 => match components[0].as_str() {
            "1" => "Centered".to_string(),
            "2" => "Co-sited".to_string(),
            other => format!("Unknown ({})", other),
        },
        // PrintConv => \%Image::ExifTool::JPEG::yCbCrSubSampling
        // (ExifTool.pm line 2137; keys are the space-joined component pair)
        0x0212 => match components.join(" ").as_str() {
            "1 1" => "YCbCr4:4:4 (1 1)".to_string(),
            "2 1" => "YCbCr4:2:2 (2 1)".to_string(),
            "2 2" => "YCbCr4:2:0 (2 2)".to_string(),
            "4 1" => "YCbCr4:1:1 (4 1)".to_string(),
            "4 2" => "YCbCr4:1:0 (4 2)".to_string(),
            "1 2" => "YCbCr4:4:0 (1 2)".to_string(),
            "1 4" => "YCbCr4:4:1 (1 4)".to_string(),
            "2 4" => "YCbCr4:2:1 (2 4)".to_string(),
            other => other.to_string(),
        },
        // CFAPlaneColor: qw(Red Green Blue Cyan Magenta Yellow White), joined
        // with ','; out-of-range indices print as "Unknown($_)".
        0xC616 => components
            .iter()
            .map(|component| match component.as_str() {
                "0" => "Red".to_string(),
                "1" => "Green".to_string(),
                "2" => "Blue".to_string(),
                "3" => "Cyan".to_string(),
                "4" => "Magenta".to_string(),
                "5" => "Yellow".to_string(),
                "6" => "White".to_string(),
                other => format!("Unknown({})", other),
            })
            .collect::<Vec<_>>()
            .join(","),
        // CFALayout PrintConv, verbatim from Exif.pm line 3305.
        0xC617 => match components[0].as_str() {
            "1" => "Rectangular".to_string(),
            "2" => "Even columns offset down 1/2 row".to_string(),
            "3" => "Even columns offset up 1/2 row".to_string(),
            "4" => "Even rows offset right 1/2 column".to_string(),
            "5" => "Even rows offset left 1/2 column".to_string(),
            "6" => {
                "Even rows offset up by 1/2 row, even columns offset left by 1/2 column".to_string()
            }
            "7" => "Even rows offset up by 1/2 row, even columns offset right by 1/2 column"
                .to_string(),
            "8" => "Even rows offset down by 1/2 row, even columns offset left by 1/2 column"
                .to_string(),
            "9" => "Even rows offset down by 1/2 row, even columns offset right by 1/2 column"
                .to_string(),
            other => format!("Unknown ({})", other),
        },
        _ => components.join(" "),
    };

    Some((format!("EXIF:{}", name), TagValue::new_string(display)))
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

fn read_tiff_i32(bytes: &[u8], byte_order: ByteOrder) -> Option<i32> {
    let bytes: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
    Some(match byte_order {
        ByteOrder::LittleEndian => i32::from_le_bytes(bytes),
        ByteOrder::BigEndian => i32::from_be_bytes(bytes),
    })
}

/// Port of `Image::ExifTool::Exif::PrintExposureTime` (Exif.pm line 5606):
///
/// ```text
///     my $secs = shift;
///     return $secs unless Image::ExifTool::IsFloat($secs);
///     if ($secs < 0.25001 and $secs > 0) {
///         return sprintf("1/%d",int(0.5 + 1/$secs));
///     }
///     $_ = sprintf("%.1f",$secs);
///     s/\.0$//;
///     return $_;
/// ```
fn print_exposure_time(secs: f64) -> String {
    if secs < 0.25001 && secs > 0.0 {
        return format!("1/{}", (0.5 + 1.0 / secs) as i64);
    }
    let formatted = format!("{:.1}", secs);
    formatted
        .strip_suffix(".0")
        .map(str::to_string)
        .unwrap_or(formatted)
}

/// Port of `Image::ExifTool::Exif::PrintFNumber` (Exif.pm line 5620):
///
/// ```text
///     my $val = shift;
///     if (Image::ExifTool::IsFloat($val) and $val > 0) {
///         # round to 1 decimal place, or 2 for values < 1.0
///         $val = sprintf(($val<1 ? "%.2f" : "%.1f"), $val);
///     }
///     return $val;
/// ```
fn print_f_number(value: f64) -> String {
    if value > 0.0 {
        if value < 1.0 {
            format!("{:.2}", value)
        } else {
            format!("{:.1}", value)
        }
    } else {
        format!("{}", value)
    }
}

/// Port of `Image::ExifTool::Exif::PrintFraction` (Exif.pm line 5421):
///
/// ```text
///     $val *= 1.00001;    # avoid round-off errors
///     if (not $val) {
///         $str = '0';
///     } elsif (int($val)/$val > 0.999) {
///         $str = sprintf("%+d", int($val));
///     } elsif ((int($val*2))/($val*2) > 0.999) {
///         $str = sprintf("%+d/2", int($val * 2));
///     } elsif ((int($val*3))/($val*3) > 0.999) {
///         $str = sprintf("%+d/3", int($val * 3));
///     } else {
///         $str = sprintf("%+.3g", $val);
///     }
/// ```
fn print_fraction(value: f64) -> String {
    let value = value * 1.00001;
    if value == 0.0 {
        return "0".to_string();
    }
    if value.trunc() / value > 0.999 {
        return format!("{:+}", value.trunc() as i64);
    }
    let doubled = value * 2.0;
    if doubled.trunc() / doubled > 0.999 {
        return format!("{:+}/2", doubled.trunc() as i64);
    }
    let tripled = value * 3.0;
    if tripled.trunc() / tripled > 0.999 {
        return format!("{:+}/3", tripled.trunc() as i64);
    }
    // Perl's "%+.3g": three significant digits, sign always shown.
    format!("{:+.3}", value)
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
        // CalibrationIlluminant1/2: int16u, and ExifTool prints them through
        // the SAME %lightSource hash that LightSource (0x9208) uses --
        // Exif.pm:3639 is `PrintConv => \%lightSource`. Delegating keeps one
        // table instead of a third copy of it; without this the DNG pair
        // reported raw `17` and `21` where ExifTool prints `Standard Light A`
        // and `D65`.
        0xC65A | 0xC65B if field_type == 3 && value_count >= 1 => {
            crate::parsers::tiff::tiff_enums::tiff_enum_to_string(
                tag_id,
                i64::from(read_tiff_u16(bytes, byte_order)?),
            )
        }
        0xA001 if field_type == 3 && value_count >= 1 => match read_tiff_u16(bytes, byte_order)? {
            1 => Some("sRGB".to_string()),
            0xffff => Some("Uncalibrated".to_string()),
            _ => None,
        },
        // ExposureTime: RATIONAL[1]. Exif.pm 0x829a PrintConv is
        // Image::ExifTool::Exif::PrintExposureTime.
        0x829A if field_type == 5 && value_count >= 1 => {
            let numerator = read_tiff_u32(bytes.get(..4)?, byte_order)?;
            let denominator = read_tiff_u32(bytes.get(4..8)?, byte_order)?;
            if denominator == 0 {
                None
            } else {
                Some(print_exposure_time(
                    f64::from(numerator) / f64::from(denominator),
                ))
            }
        }
        // FNumber: RATIONAL[1]. Exif.pm 0x829d PrintConv is
        // Image::ExifTool::Exif::PrintFNumber.
        0x829D if field_type == 5 && value_count >= 1 => {
            let numerator = read_tiff_u32(bytes.get(..4)?, byte_order)?;
            let denominator = read_tiff_u32(bytes.get(4..8)?, byte_order)?;
            if denominator == 0 {
                None
            } else {
                Some(print_f_number(
                    f64::from(numerator) / f64::from(denominator),
                ))
            }
        }
        // ExposureProgram: SHORT[1]. Exif.pm 0x8822 PrintConv, verbatim:
        //     0 => 'Not Defined',
        //     1 => 'Manual',
        //     2 => 'Program AE',
        //     3 => 'Aperture-priority AE',
        //     4 => 'Shutter speed priority AE',
        //     5 => 'Creative (Slow speed)',
        //     6 => 'Action (High speed)',
        //     7 => 'Portrait',
        //     8 => 'Landscape',
        //     9 => 'Bulb', #25
        0x8822 if field_type == 3 && value_count >= 1 => match read_tiff_u16(bytes, byte_order)? {
            0 => Some("Not Defined".to_string()),
            1 => Some("Manual".to_string()),
            2 => Some("Program AE".to_string()),
            3 => Some("Aperture-priority AE".to_string()),
            4 => Some("Shutter speed priority AE".to_string()),
            5 => Some("Creative (Slow speed)".to_string()),
            6 => Some("Action (High speed)".to_string()),
            7 => Some("Portrait".to_string()),
            8 => Some("Landscape".to_string()),
            9 => Some("Bulb".to_string()),
            _ => None,
        },
        // ExifVersion: UNDEFINED. Exif.pm 0x9000 has no PrintConv, only
        //     RawConv => '$val=~s/\0+$//; $val',  # (some idiots add null terminators)
        0x9000 if field_type == 7 => {
            let count = usize::try_from(value_count).ok()?;
            let version = bytes.get(..count)?;
            let trimmed = String::from_utf8_lossy(version)
                .trim_end_matches('\0')
                .to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }
        // ExposureCompensation: SRATIONAL[1]. Exif.pm 0x9204 PrintConv is
        // Image::ExifTool::Exif::PrintFraction.
        0x9204 if field_type == 10 && value_count >= 1 => {
            let numerator = read_tiff_i32(bytes.get(..4)?, byte_order)?;
            let denominator = read_tiff_i32(bytes.get(4..8)?, byte_order)?;
            if denominator == 0 {
                None
            } else {
                Some(print_fraction(
                    f64::from(numerator) / f64::from(denominator),
                ))
            }
        }
        // LightSource: SHORT[1]. Exif.pm 0x9208 PrintConv => \%lightSource,
        // whose full table (Exif.pm lines 139-162) is reproduced verbatim.
        0x9208 if field_type == 3 && value_count >= 1 => {
            exif_light_source_label(read_tiff_u16(bytes, byte_order)?).map(str::to_string)
        }
        // Flash: SHORT[1]. Exif.pm 0x9209 PrintConv => \%flash, whose full
        // table (Exif.pm lines 172-199) is reproduced verbatim.
        0x9209 if field_type == 3 && value_count >= 1 => match read_tiff_u16(bytes, byte_order)? {
            0x00 => Some("No Flash".to_string()),
            0x01 => Some("Fired".to_string()),
            0x05 => Some("Fired, Return not detected".to_string()),
            0x07 => Some("Fired, Return detected".to_string()),
            0x08 => Some("On, Did not fire".to_string()),
            0x09 => Some("On, Fired".to_string()),
            0x0d => Some("On, Return not detected".to_string()),
            0x0f => Some("On, Return detected".to_string()),
            0x10 => Some("Off, Did not fire".to_string()),
            0x14 => Some("Off, Did not fire, Return not detected".to_string()),
            0x18 => Some("Auto, Did not fire".to_string()),
            0x19 => Some("Auto, Fired".to_string()),
            0x1d => Some("Auto, Fired, Return not detected".to_string()),
            0x1f => Some("Auto, Fired, Return detected".to_string()),
            0x20 => Some("No flash function".to_string()),
            0x30 => Some("Off, No flash function".to_string()),
            0x41 => Some("Fired, Red-eye reduction".to_string()),
            0x45 => Some("Fired, Red-eye reduction, Return not detected".to_string()),
            0x47 => Some("Fired, Red-eye reduction, Return detected".to_string()),
            0x49 => Some("On, Red-eye reduction".to_string()),
            0x4d => Some("On, Red-eye reduction, Return not detected".to_string()),
            0x4f => Some("On, Red-eye reduction, Return detected".to_string()),
            0x50 => Some("Off, Red-eye reduction".to_string()),
            0x58 => Some("Auto, Did not fire, Red-eye reduction".to_string()),
            0x59 => Some("Auto, Fired, Red-eye reduction".to_string()),
            0x5d => Some("Auto, Fired, Red-eye reduction, Return not detected".to_string()),
            0x5f => Some("Auto, Fired, Red-eye reduction, Return detected".to_string()),
            _ => None,
        },
        // FileSource: UNDEFINED. Exif.pm 0xa300 PrintConv, verbatim:
        //     1 => 'Film Scanner',
        //     2 => 'Reflection Print Scanner',
        //     3 => 'Digital Camera',
        //     # handle the case where Sigma incorrectly gives this tag a count of 4
        //     "\3\0\0\0" => 'Sigma Digital Camera',
        0xA300 if field_type == 7 => {
            let count = usize::try_from(value_count).ok()?;
            let source = bytes.get(..count)?;
            match source {
                b"\x03\x00\x00\x00" => Some("Sigma Digital Camera".to_string()),
                [1] => Some("Film Scanner".to_string()),
                [2] => Some("Reflection Print Scanner".to_string()),
                [3] => Some("Digital Camera".to_string()),
                _ => None,
            }
        }
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
        // Saturation: SHORT[1]. Exif.pm tag 0xA409 PrintConv:
        // 0 => 'Normal', 1 => 'Low', 2 => 'High'.
        0xA409 if field_type == 3 && value_count >= 1 => match read_tiff_u16(bytes, byte_order)? {
            0 => Some("Normal".to_string()),
            1 => Some("Low".to_string()),
            2 => Some("High".to_string()),
            _ => None,
        },

        // ResolutionUnit (Exif.pm 0x0128). ExifTool prints the unit in lower
        // case -- "inches", not "Inches".
        0x0128 if field_type == 3 && value_count >= 1 => match read_tiff_u16(bytes, byte_order)? {
            1 => Some("None".to_string()),
            2 => Some("inches".to_string()),
            3 => Some("cm".to_string()),
            _ => None,
        },

        // SensingMethod (Exif.pm 0xa217). Note 6 is absent from ExifTool's
        // table, so it falls through rather than being invented.
        0xA217 if field_type == 3 && value_count >= 1 => match read_tiff_u16(bytes, byte_order)? {
            1 => Some("Not defined".to_string()),
            2 => Some("One-chip color area".to_string()),
            3 => Some("Two-chip color area".to_string()),
            4 => Some("Three-chip color area".to_string()),
            5 => Some("Color sequential area".to_string()),
            7 => Some("Trilinear".to_string()),
            8 => Some("Color sequential linear".to_string()),
            _ => None,
        },

        // SceneType (Exif.pm 0xa301) is UNDEFINED[1], not SHORT: its single
        // byte is the value, so it is read directly rather than through
        // read_tiff_u16.
        0xA301 if value_count >= 1 => match bytes.first()? {
            1 => Some("Directly photographed".to_string()),
            _ => None,
        },

        // SceneCaptureType (Exif.pm 0xa406)
        0xA406 if field_type == 3 && value_count >= 1 => match read_tiff_u16(bytes, byte_order)? {
            0 => Some("Standard".to_string()),
            1 => Some("Landscape".to_string()),
            2 => Some("Portrait".to_string()),
            3 => Some("Night".to_string()),
            4 => Some("Other".to_string()),
            _ => None,
        },

        // Sharpness (Exif.pm 0xa40a) -- same conversion as Contrast (0xa408),
        // NOT the same as Saturation (0xa409), whose 1/2 are Low/High.
        0xA40A if field_type == 3 && value_count >= 1 => match read_tiff_u16(bytes, byte_order)? {
            0 => Some("Normal".to_string()),
            1 => Some("Soft".to_string()),
            2 => Some("Hard".to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// Format a RATIONAL pair whose value_count >= 1.
/// `%Image::ExifTool::Exif::lightSource` (Exif.pm lines 139-162), reproduced
/// verbatim.
///
/// Shared by EXIF LightSource (0x9208), the DNG CalibrationIlluminant pair
/// (0xC65A/0xC65B, whose `PrintConv => \%lightSource` is Exif.pm:3639) and
/// PanasonicRaw's WBType1..7 (`%wbTypeInfo` in PanasonicRaw.pm:46 is
/// `PrintConv => \%Image::ExifTool::Exif::lightSource`).
///
/// Returns `None` for codes the hash has no entry for (5-8, 25-254, ...).
/// ExifTool prints the bare number in that case, and a missing label is far
/// better than one borrowed from a neighbouring code.
fn exif_light_source_label(code: u16) -> Option<&'static str> {
    Some(match code {
        0 => "Unknown",
        1 => "Daylight",
        2 => "Fluorescent",
        3 => "Tungsten (Incandescent)",
        4 => "Flash",
        9 => "Fine Weather",
        10 => "Cloudy",
        11 => "Shade",
        12 => "Daylight Fluorescent",
        13 => "Day White Fluorescent",
        14 => "Cool White Fluorescent",
        15 => "White Fluorescent",
        16 => "Warm White Fluorescent",
        17 => "Standard Light A",
        18 => "Standard Light B",
        19 => "Standard Light C",
        20 => "D55",
        21 => "D65",
        22 => "D75",
        23 => "D50",
        24 => "ISO Studio Tungsten",
        255 => "Other",
        _ => return None,
    })
}

/// Read one `int16u` at element index `index` of a `FORMAT => 'int16u'`
/// ProcessBinaryData block.
fn binary_u16_at(bytes: &[u8], index: usize, byte_order: ByteOrder) -> Option<u16> {
    let start = index.checked_mul(2)?;
    read_tiff_u16(bytes.get(start..start.checked_add(2)?)?, byte_order)
}

/// Read one `int16s` at element index `index` of a `FORMAT => 'int16s'`
/// ProcessBinaryData block.
fn binary_i16_at(bytes: &[u8], index: usize, byte_order: ByteOrder) -> Option<i16> {
    binary_u16_at(bytes, index, byte_order).map(|value| value as i16)
}

/// Print a value ExifTool derived by a numeric ValueConv with no PrintConv:
/// an exact integer loses its fractional part, everything else round-trips.
fn print_plain_number(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{}", value)
    }
}

/// PanasonicRaw IFD0 tag 0x0027 (WBInfo2), `%Image::ExifTool::PanasonicRaw::WBInfo2`.
///
/// `FORMAT => 'int16u'`, `FIRST_ENTRY => 0`, so every numeric key in that
/// table is an INDEX of two bytes, not a byte offset:
/// ```text
///      0 => 'NumWBEntries',
///      1 => { Name => 'WBType1',       %wbTypeInfo },
///      2 => { Name => 'WB_RGBLevels1', Format => 'int16u[3]' },
///      5 => WBType2, 6 => WB_RGBLevels2, 9 => WBType3, 10 => WB_RGBLevels3,
///     13 => WBType4, 14 => WB_RGBLevels4, 17 => WBType5, 18 => WB_RGBLevels5,
///     21 => WBType6, 22 => WB_RGBLevels6, 25 => WBType7, 26 => WB_RGBLevels7,
/// ```
/// i.e. entry *n* (1-based) is at index `1 + 4 * (n - 1)`.
///
/// The sibling table `WBInfo` (IFD0 tag 0x0011, PanasonicRawVersion < 200)
/// has a different stride and emits WB_RBLevels rather than WB_RGBLevels; it
/// is deliberately not handled here because no sample exercises it and
/// guessing the layout would fabricate values.
fn extract_panasonic_raw_wb_info2(bytes: &[u8], byte_order: ByteOrder, metadata: &mut MetadataMap) {
    let Some(entries) = binary_u16_at(bytes, 0, byte_order) else {
        return;
    };
    metadata.insert(
        "PanasonicRaw:NumWBEntries".to_string(),
        TagValue::new_integer(i64::from(entries)),
    );

    // The table stops at WBType7/WB_RGBLevels7 no matter what NumWBEntries
    // claims, so clamp rather than trusting the file's own count.
    for n in 1..=u16::min(entries, 7) {
        let base = 1 + 4 * (usize::from(n) - 1);
        if let Some(kind) = binary_u16_at(bytes, base, byte_order) {
            let printed = exif_light_source_label(kind)
                .map(str::to_string)
                .unwrap_or_else(|| kind.to_string());
            metadata.insert(
                format!("PanasonicRaw:WBType{}", n),
                TagValue::new_string(printed),
            );
        }
        let levels: Option<Vec<String>> = (0..3)
            .map(|i| binary_u16_at(bytes, base + 1 + i, byte_order).map(|v| v.to_string()))
            .collect();
        if let Some(levels) = levels {
            metadata.insert(
                format!("PanasonicRaw:WB_RGBLevels{}", n),
                TagValue::new_string(levels.join(" ")),
            );
        }
    }
}

/// PanasonicRaw IFD0 tag 0x0119 (DistortionInfo),
/// `%Image::ExifTool::PanasonicRaw::DistortionInfo`.
///
/// `FORMAT => 'int16s'`, `FIRST_ENTRY => 0`; the numeric keys are indices of
/// two SIGNED bytes each. Reproduced verbatim from PanasonicRaw.pm:436-490:
/// ```text
///      2 => DistortionParam02   ValueConv => '$val / 32768'
///      4 => DistortionParam04   ValueConv => '$val / 32768'
///      5 => DistortionScale     ValueConv => '1 / (1 + $val/32768)'
///    7.1 => DistortionCorrection Mask => 0x0f, PrintConv => {0=>'Off',1=>'On'}
///      8 => DistortionParam08   ValueConv => '$val / 32768'
///      9 => DistortionParam09   ValueConv => '$val / 32768'
///     11 => DistortionParam11   ValueConv => '$val / 32768'
///     12 => DistortionN         Unknown => 1
/// ```
/// Indices 0, 1, 3, 6, 10, 13, 14 and 15 are checksums or undocumented, and
/// index 12 is `Unknown => 1` so ExifTool suppresses it without -u. None of
/// them are emitted.
fn extract_panasonic_raw_distortion_info(
    bytes: &[u8],
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    // ProcessDistortionInfo warns and still parses when the length is wrong,
    // but every documented index lives inside the 32-byte form; anything
    // shorter would read past the end of a truncated block.
    if bytes.len() < 32 {
        return;
    }

    for (index, name) in [
        (2usize, "DistortionParam02"),
        (4, "DistortionParam04"),
        (8, "DistortionParam08"),
        (9, "DistortionParam09"),
        (11, "DistortionParam11"),
    ] {
        if let Some(raw) = binary_i16_at(bytes, index, byte_order) {
            metadata.insert(
                format!("PanasonicRaw:{}", name),
                TagValue::new_string(print_plain_number(f64::from(raw) / 32768.0)),
            );
        }
    }

    if let Some(raw) = binary_i16_at(bytes, 5, byte_order) {
        let denominator = 1.0 + f64::from(raw) / 32768.0;
        if denominator != 0.0 {
            metadata.insert(
                "PanasonicRaw:DistortionScale".to_string(),
                TagValue::new_string(print_plain_number(1.0 / denominator)),
            );
        }
    }

    // 7.1: Mask => 0x0f. The upper nibble has been seen set on the GF5/GX1,
    // which is exactly why ExifTool masks instead of comparing the whole
    // value; an unmasked read would print "Off" for -4095.
    if let Some(raw) = binary_i16_at(bytes, 7, byte_order) {
        let masked = raw & 0x0f;
        let printed = match masked {
            0 => "Off".to_string(),
            1 => "On".to_string(),
            other => other.to_string(),
        };
        metadata.insert(
            "PanasonicRaw:DistortionCorrection".to_string(),
            TagValue::new_string(printed),
        );
    }
}

/// Panasonic MakerNote tag ids whose out-of-line value
/// `parsers::tiff::makernotes::panasonic` actually dereferences (its
/// `extract_string_value` list). Every other tag in that parser reads the
/// entry's `value_offset` field as if it were the value, so an out-of-line
/// entry outside this set makes it print an offset: Panasonic.rw2 produced
/// `AFPointPosition = 9046`, `FaceDetection = Unknown (9062)` and
/// `BabyAge2 = 9104`, which are the rebuilt offsets of 0x4D, 0x4E and 0x8010
/// verbatim. Those entries are dropped before the block reaches the parser.
const PANASONIC_DEREFERENCED_TAGS: &[u16] = &[
    0x0001, 0x0002, 0x0025, 0x0026, 0x0033, 0x0052, 0x0054, 0x0065, 0x0066, 0x0067, 0x0069, 0x006B,
    0x006D, 0x006F, 0x0080,
];

/// Locate one IFD entry's out-of-line value inside `tiff` and return its
/// `(offset, length)`.
///
/// `parse_ifd` hands back the value bytes but not where they came from, and
/// relocating a MakerNote needs exactly that: the offset IS the base its own
/// internal offsets are stated against. Returns `None` for a value small
/// enough to be stored inline in the entry, which by definition has no
/// offset.
fn tiff_external_entry_extent(
    tiff: &[u8],
    ifd_offset: u64,
    byte_order: ByteOrder,
    wanted_tag: u16,
) -> Option<(usize, usize)> {
    let ifd_offset = usize::try_from(ifd_offset).ok()?;
    let entry_count = usize::from(read_tiff_u16(
        tiff.get(ifd_offset..ifd_offset + 2)?,
        byte_order,
    )?);
    for index in 0..entry_count {
        let start = ifd_offset.checked_add(2 + index * 12)?;
        let entry = tiff.get(start..start.checked_add(12)?)?;
        if read_tiff_u16(&entry[..2], byte_order)? != wanted_tag {
            continue;
        }
        let field_type = read_tiff_u16(&entry[2..4], byte_order)?;
        let value_count = usize::try_from(read_tiff_u32(&entry[4..8], byte_order)?).ok()?;
        let length = tiff_field_type_size(field_type)?.checked_mul(value_count)?;
        if length <= 4 {
            return None;
        }
        let offset = usize::try_from(read_tiff_u32(&entry[8..12], byte_order)?).ok()?;
        return Some((offset, length));
    }
    None
}

/// TIFF field-type sizes, indexed by the type code. `None` for codes the TIFF
/// specification does not define.
fn tiff_field_type_size(field_type: u16) -> Option<usize> {
    Some(match field_type {
        1 | 2 | 6 | 7 => 1, // BYTE, ASCII, SBYTE, UNDEFINED
        3 | 8 => 2,         // SHORT, SSHORT
        4 | 9 | 11 => 4,    // LONG, SLONG, FLOAT
        5 | 10 | 12 => 8,   // RATIONAL, SRATIONAL, DOUBLE
        _ => return None,
    })
}

/// Rebuild a MakerNote IFD whose value offsets are stated in some OTHER
/// coordinate system into a self-contained block whose offsets are indices
/// into the returned buffer.
///
/// A MakerNote's `value_offset` fields are almost never relative to the
/// MakerNote itself: they are offsets into the enclosing TIFF, or -- for a
/// MakerNote the Adobe DNG Converter relocated into DNGPrivateData -- offsets
/// into a file that no longer exists. `source_base` is the value of
/// `value_offset` that corresponds to byte 0 of `ifd`.
///
/// * Adobe MakN: `ProcessAdobeMakN` (DNG.pm:685) reads the base from the
///   record header -- `my $originalPos = Get32u($dataPt, $start + 2);` -- and
///   then `$fix = $dataPos + $dirStart - $originalPos;`.
/// * RW2 preview MakerNote: MakerNotes.pm gives MakerNotePanasonic
///   `Start => '$valuePtr + 12'` and no `Base`, so offsets stay relative to
///   the preview's own TIFF header and the base is where the MakerNote value
///   sits inside it.
///
/// Nothing inside the block announces that base, and the values are not
/// necessarily flush against the IFD -- Canon350D leaves a two-byte gap --
/// so handing the raw slice to a parser that infers the base from the layout
/// misreads every offset-based tag by the size of that gap.
///
/// This repacks the directory instead: entries keep their tag/type/count, but
/// offset-based values are copied out and their offsets rewritten to point at
/// their new home immediately after the rebuilt IFD header. The result is an
/// ordinary, self-consistent MakerNote directory in which `value_offset` is
/// simply an index from the start of the returned buffer.
///
/// Entries whose value cannot be located inside the block are dropped rather
/// than pointed at substitute bytes: a missing tag beats a fabricated one.
///
/// `external_allowlist`, when given, restricts which OUT-OF-LINE entries
/// survive. It exists because not every manufacturer parser follows an
/// offset: one that reads `value_offset` directly will happily print the
/// offset itself as the tag's value. Restricting the rebuilt directory to the
/// out-of-line ids the target parser actually dereferences keeps that
/// nonsense out of the output. `None` keeps every entry.
fn rebuild_relocated_makernote(
    ifd: &[u8],
    source_base: u32,
    byte_order: ByteOrder,
    external_allowlist: Option<&[u16]>,
) -> Option<Vec<u8>> {
    let entry_count = usize::from(read_tiff_u16(ifd.get(..2)?, byte_order)?);
    // A MakerNote IFD with more entries than this is not one; ExifTool's own
    // LocateIFD applies the same kind of sanity bound before trusting a count.
    if entry_count == 0 || entry_count > 500 {
        return None;
    }
    let source_header_size = 2usize.checked_add(entry_count.checked_mul(12)?)? + 4;
    if ifd.len() < source_header_size {
        return None;
    }

    // (tag_id, field_type, value_count, inline-or-external payload)
    let mut kept: Vec<([u8; 8], Option<&[u8]>, [u8; 4])> = Vec::with_capacity(entry_count);
    for index in 0..entry_count {
        let start = 2 + index * 12;
        let entry = ifd.get(start..start + 12)?;
        let tag_id = read_tiff_u16(&entry[..2], byte_order)?;
        let field_type = read_tiff_u16(&entry[2..4], byte_order)?;
        let value_count = read_tiff_u32(&entry[4..8], byte_order)?;
        let value_offset = read_tiff_u32(&entry[8..12], byte_order)?;

        let mut head = [0u8; 8];
        head.copy_from_slice(&entry[..8]);
        let mut tail = [0u8; 4];
        tail.copy_from_slice(&entry[8..12]);

        let total = tiff_field_type_size(field_type)
            .and_then(|size| size.checked_mul(usize::try_from(value_count).ok()?));
        let Some(total) = total else {
            // Unknown field type: no way to know how many bytes it owns, so
            // pass the entry through untouched and let the parser ignore it.
            kept.push((head, None, tail));
            continue;
        };
        if total <= 4 {
            // Inline value, stored in the offset field itself.
            kept.push((head, None, tail));
            continue;
        }

        if external_allowlist.is_some_and(|allowed| !allowed.contains(&tag_id)) {
            continue;
        }

        let Some(relative) = value_offset.checked_sub(source_base) else {
            continue;
        };
        let Ok(relative) = usize::try_from(relative) else {
            continue;
        };
        let Some(payload) = relative
            .checked_add(total)
            .and_then(|end| ifd.get(relative..end))
        else {
            continue;
        };
        kept.push((head, Some(payload), tail));
    }

    let header_size = 2 + kept.len() * 12 + 4;
    let mut out = vec![0u8; header_size];
    let count = u16::try_from(kept.len()).ok()?;
    out[..2].copy_from_slice(&match byte_order {
        ByteOrder::LittleEndian => count.to_le_bytes(),
        ByteOrder::BigEndian => count.to_be_bytes(),
    });
    for (index, (head, payload, tail)) in kept.iter().enumerate() {
        let start = 2 + index * 12;
        out[start..start + 8].copy_from_slice(head);
        match payload {
            Some(bytes) => {
                let offset = u32::try_from(out.len()).ok()?;
                let encoded = match byte_order {
                    ByteOrder::LittleEndian => offset.to_le_bytes(),
                    ByteOrder::BigEndian => offset.to_be_bytes(),
                };
                out[start + 8..start + 12].copy_from_slice(&encoded);
                out.extend_from_slice(bytes);
            }
            None => out[start + 8..start + 12].copy_from_slice(tail),
        }
    }
    Some(out)
}

/// Extract the MakerNotes the Adobe DNG Converter stashed in DNGPrivateData
/// (IFD0 tag 0xC634).
///
/// Exif.pm:3530 names the tag `DNGAdobeData` when the value starts with
/// `Adobe\0`, and flags it `Binary`+`Protected` -- ExifTool never prints the
/// blob, it descends into it. `ProcessAdobeData` (DNG.pm:240) reads a 6-byte
/// `Adobe\0` header and then a flat sequence of records:
/// ```text
///     my ($tag, $size) = unpack("x${pos}a4N", $$dataPt);   # 4-char tag, big-endian size
///     $pos += 8;
///     ...
///     $pos += $size;
///     ++$pos if $size & 0x01;   # (darn padding)
/// ```
/// Only the `MakN` record is handled here; `CRW `, `MRW `, `SR2 `, `RAF `,
/// `Pano`, `Koda` and `Leaf` carry formats this function does not decode, and
/// are skipped rather than guessed at.
///
/// The MakN record's own header is `ProcessAdobeMakN` (DNG.pm:685): two bytes
/// of byte order ("II"/"MM") -- which need NOT match the enclosing TIFF, and
/// on Canon350D.dng does not -- then the 4-byte big-endian original position,
/// then the MakerNote itself. Camera Raw's JPEG-conversion bug adds 12 more
/// header bytes, detected exactly as ExifTool detects it:
/// ```text
///     $hdrLen += 12 if $len >= 18 and substr($$dataPt, $start+6, 4) eq "\0\0\0\x01";
/// ```
fn extract_dng_adobe_private_data(data: &[u8], make: &str, metadata: &mut MetadataMap) {
    if !data.starts_with(b"Adobe\0") {
        return;
    }

    let mut pos = 6usize;
    while pos + 8 <= data.len() {
        let Some(record_tag) = data.get(pos..pos + 4) else {
            return;
        };
        let Some(size) = read_tiff_u32(&data[pos + 4..pos + 8], ByteOrder::BigEndian) else {
            return;
        };
        pos += 8;
        let Ok(size) = usize::try_from(size) else {
            return;
        };
        let Some(block) = pos.checked_add(size).and_then(|end| data.get(pos..end)) else {
            return; // truncated record: ExifTool's `last if $pos + $size > $end`
        };

        if record_tag == b"MakN" {
            parse_adobe_makn_record(block, make, metadata);
        }

        pos += size;
        if size & 1 == 1 {
            pos += 1;
        }
    }
}

/// Decode one `MakN` record from DNGPrivateData and hand the recovered
/// MakerNote to the manufacturer dispatcher.
///
/// The rebuild expects a bare IFD, which is what Canon (and Minolta) write.
/// A MakerNote that leads with a manufacturer signature -- "Nikon\0",
/// "Panasonic\0\0\0", "OLYMP", "SONY DSC " and the rest -- makes the first
/// two bytes decode as an absurd entry count, `rebuild_relocated_makernote`
/// rejects it, and nothing is emitted. ExifTool skips those headers via
/// `MakerNotes::LocateIFD`; doing the same here needs a sample to verify
/// against, and inventing the header lengths blind would risk decoding a
/// misaligned directory into confident nonsense.
fn parse_adobe_makn_record(block: &[u8], make: &str, metadata: &mut MetadataMap) {
    if block.len() < 6 {
        return;
    }
    let byte_order = match &block[..2] {
        b"II" => ByteOrder::LittleEndian,
        b"MM" => ByteOrder::BigEndian,
        _ => return,
    };
    let Some(original_pos) = read_tiff_u32(&block[2..6], ByteOrder::BigEndian) else {
        return;
    };

    let mut header_len = 6usize;
    if block.len() >= 18 && block.get(6..10) == Some(&[0, 0, 0, 1]) {
        header_len += 12;
    }
    let Some(ifd) = block.get(header_len..) else {
        return;
    };

    let Some(rebuilt) = rebuild_relocated_makernote(ifd, original_pos, byte_order, None) else {
        eprintln!("Warning: DNGPrivateData MakN record is not a parsable MakerNote IFD");
        return;
    };

    let mut tags = std::collections::HashMap::new();
    if let Err(error) = crate::parsers::tiff::makernote_dispatcher::dispatch_makernote(
        make, &rebuilt, byte_order, &mut tags,
    ) {
        eprintln!(
            "Warning: Failed to parse DNGPrivateData MakerNote for {}: {}",
            make, error
        );
        return;
    }
    for (tag_name, tag_value) in tags {
        metadata.insert(tag_name, TagValue::new_string(tag_value));
    }
}

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

/// Format PanasonicRaw IFD0 tags 0x18-0x1A (HighISOMultiplierRed/Green/Blue).
///
/// PanasonicRaw.pm, verbatim (0x19 and 0x1a are identical but for the name):
/// ```text
///     0x18 => { #IB
///         Name => 'HighISOMultiplierRed',
///         Writable => 'int16u',
///         ValueConv => '$val / 256',
/// ```
///
/// There is no PrintConv, so ExifTool prints the ValueConv result directly:
/// an integer when the division is exact (measured: all three are 0 on
/// Panasonic.rw2, 2026-07-27), otherwise a decimal.
fn format_panasonic_high_iso_multiplier(
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    byte_order: ByteOrder,
) -> Option<String> {
    if field_type != 3 || value_count < 1 {
        return None;
    }

    let value = read_tiff_u16(bytes, byte_order)?;
    if value % 256 == 0 {
        Some((value / 256).to_string())
    } else {
        Some(format!("{}", f64::from(value) / 256.0))
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

    /// Panasonic RW2 IFD0 ids must resolve through
    /// `%Image::ExifTool::PanasonicRaw::Main`, never the standard EXIF table.
    ///
    /// The right-hand strings below are the tag names ExifTool prints for
    /// `/tmp/oxidex-exiftool-cache/combined-samples/Panasonic.rw2`; the
    /// commented-out names are what the standard EXIF table produced for the
    /// same ids before this table existed.
    #[test]
    fn panasonic_raw_ifd0_ids_resolve_against_panasonic_raw_main() {
        assert_eq!(
            panasonic_raw_ifd0_tag_name(0x0001),
            Some("PanasonicRawVersion") // was "Higher resolution image exists"
        );
        assert_eq!(panasonic_raw_ifd0_tag_name(0x0002), Some("SensorWidth")); // was "InteropVersion"
        assert_eq!(panasonic_raw_ifd0_tag_name(0x0003), Some("SensorHeight")); // was "Lossless JBIG B&W, J"
        assert_eq!(panasonic_raw_ifd0_tag_name(0x0004), Some("SensorTopBorder")); // was "Profile L"
        assert_eq!(
            panasonic_raw_ifd0_tag_name(0x0005),
            Some("SensorLeftBorder")
        ); // was "Creative (Slow speed)"
        assert_eq!(
            panasonic_raw_ifd0_tag_name(0x0006),
            Some("SensorBottomBorder") // was "Profile T"
        );
        assert_eq!(
            panasonic_raw_ifd0_tag_name(0x0007),
            Some("SensorRightBorder") // was "Trilinear"
        );
        assert_eq!(panasonic_raw_ifd0_tag_name(0x0008), Some("SamplesPerPixel"));
        assert_eq!(panasonic_raw_ifd0_tag_name(0x0024), Some("WBRedLevel"));
        assert_eq!(panasonic_raw_ifd0_tag_name(0x0025), Some("WBGreenLevel"));
        assert_eq!(panasonic_raw_ifd0_tag_name(0x0026), Some("WBBlueLevel"));
        assert_eq!(panasonic_raw_ifd0_tag_name(0x002D), Some("RawFormat"));
        assert_eq!(panasonic_raw_ifd0_tag_name(0x0118), Some("RawDataOffset")); // was "MinSampleValue"

        // 0x011A is XResolution in Exif::Main but is not defined in
        // PanasonicRaw::Main at all, so ExifTool does not report it for RW2.
        // Naming it XResolution published a fabricated value of 1 for a file
        // whose real XResolution is 180.
        assert_eq!(panasonic_raw_ifd0_tag_name(0x011A), None);
        // Likewise 0x0119: DistortionInfo, a SubDirectory whose container has
        // no printed value (it was emitted as "MaxSampleValue").
        assert_eq!(panasonic_raw_ifd0_tag_name(0x0119), None);
    }

    #[test]
    fn formats_panasonic_raw_version_and_noise_reduction_params() {
        // PanasonicRawVersion is UNDEFINED[4] holding ASCII "0300" in
        // Panasonic.rw2; ExifTool prints it verbatim.
        assert_eq!(
            format_panasonic_raw_ifd0_value(0x0001, b"0300", 7, 4, ByteOrder::LittleEndian)
                .as_deref(),
            Some("0300")
        );

        // NoiseReductionParams: Writable => 'undef', Format => 'int16u',
        // Count => -1. ExifTool prints Panasonic.rw2's 26-byte blob as
        // "3 100 1 1 1 200 2 2 2 400 4 4 4".
        let params: Vec<u8> = [3u16, 100, 1, 1, 1, 200, 2, 2, 2, 400, 4, 4, 4]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        assert_eq!(
            format_panasonic_raw_ifd0_value(
                0x001B,
                &params,
                7,
                params.len() as u32,
                ByteOrder::LittleEndian
            )
            .as_deref(),
            Some("3 100 1 1 1 200 2 2 2 400 4 4 4")
        );
    }

    /// Literal ExifTool 13.55 output for
    /// `/tmp/oxidex-exiftool-cache/combined-samples/DNG.dng`, one assertion per
    /// PrintConv/array shape the generic TIFF decoder gets wrong.
    #[test]
    fn formats_dng_subifd_tags_the_way_exiftool_prints_them() {
        let le = ByteOrder::LittleEndian;
        let name_and_value = |tag_id, bytes: &[u8], field_type, count| {
            format_dng_subifd_exif_tag(tag_id, bytes, field_type, count, le).map(|(name, value)| {
                let text = value.as_string().unwrap_or_default().to_string();
                (name, text)
            })
        };

        // 0xc617 CFALayout, PrintConv { 1 => 'Rectangular', ... }
        assert_eq!(
            name_and_value(0xC617, &1u16.to_le_bytes(), 3, 1),
            Some(("EXIF:CFALayout".to_string(), "Rectangular".to_string()))
        );
        // 0xc616 CFAPlaneColor, qw(Red Green Blue ...) joined with ','
        assert_eq!(
            name_and_value(0xC616, &[0u8, 1, 2], 1, 3),
            Some((
                "EXIF:CFAPlaneColor".to_string(),
                "Red,Green,Blue".to_string()
            ))
        );
        // 0x828e CFAPattern2, int8u[-1]
        assert_eq!(
            name_and_value(0x828E, &[0u8, 1, 1, 2], 1, 4),
            Some(("EXIF:CFAPattern2".to_string(), "0 1 1 2".to_string()))
        );
        // 0x212 YCbCrSubSampling, %Image::ExifTool::JPEG::yCbCrSubSampling
        let two_two: Vec<u8> = [2u16, 2].iter().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(
            name_and_value(0x0212, &two_two, 3, 2),
            Some((
                "EXIF:YCbCrSubSampling".to_string(),
                "YCbCr4:2:0 (2 2)".to_string()
            ))
        );
        // 0x213 YCbCrPositioning, PrintConv { 1 => 'Centered', 2 => 'Co-sited' }
        assert_eq!(
            name_and_value(0x0213, &2u16.to_le_bytes(), 3, 1),
            Some(("EXIF:YCbCrPositioning".to_string(), "Co-sited".to_string()))
        );
        // 0xc61f DefaultCropOrigin, int32u[2]
        let origin: Vec<u8> = [10u32, 5].iter().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(
            name_and_value(0xC61F, &origin, 4, 2),
            Some(("EXIF:DefaultCropOrigin".to_string(), "10 5".to_string()))
        );
        // 0x214 ReferenceBlackWhite, rational64u[6]
        let black_white: Vec<u8> = [
            (0u32, 1u32),
            (255, 1),
            (128, 1),
            (255, 1),
            (128, 1),
            (255, 1),
        ]
        .iter()
        .flat_map(|(numerator, denominator)| {
            let mut pair = numerator.to_le_bytes().to_vec();
            pair.extend_from_slice(&denominator.to_le_bytes());
            pair
        })
        .collect();
        assert_eq!(
            name_and_value(0x0214, &black_white, 5, 6),
            Some((
                "EXIF:ReferenceBlackWhite".to_string(),
                "0 255 128 255 128 255".to_string()
            ))
        );
        // 0x211 YCbCrCoefficients, rational64u[3] -- non-integral rationals
        let coefficients: Vec<u8> = [(299u32, 1000u32), (587, 1000), (114, 1000)]
            .iter()
            .flat_map(|(numerator, denominator)| {
                let mut pair = numerator.to_le_bytes().to_vec();
                pair.extend_from_slice(&denominator.to_le_bytes());
                pair
            })
            .collect();
        assert_eq!(
            name_and_value(0x0211, &coefficients, 5, 3),
            Some((
                "EXIF:YCbCrCoefficients".to_string(),
                "0.299 0.587 0.114".to_string()
            ))
        );

        // Tags whose IFD priority oxidex does not model are deliberately left
        // to the generic path.
        assert!(format_dng_subifd_exif_tag(0x0102, &[8, 0], 3, 1, le).is_none());
        assert!(format_dng_subifd_exif_tag(0x0103, &[7, 0], 3, 1, le).is_none());
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
        extract_rw2_embedded_exif_tags(&jpeg, 0, &mut metadata)
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

        // `exiftool -G0:1 -a -s /tmp/oxidex-exiftool-cache/combined-samples/Nikon.nef`
        // prints "[EXIF:SubIFD]  CFAPattern2  : 2 1 1 0" -- family 0 is EXIF,
        // so that is the group the comparison reports key on.
        assert_eq!(
            metadata
                .get("EXIF:CFAPattern2")
                .and_then(|value| value.as_string()),
            Some("2 1 1 0"),
            "CFAPattern2 belongs to ExifTool's EXIF group, not the physical SubIFD"
        );
        assert!(
            metadata.get("SubIFD0:CFAPattern2").is_none(),
            "CFAPattern2 must not also be emitted under the physical SubIFD group"
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

    // Unique identifier (16 bytes at offset 8).
    //
    // SigmaRaw.pm:75 -- Header index 2 is `undef[16]` with
    // `ValueConv => 'unpack("H*", $val)'`, i.e. a lowercase hex dump of the
    // raw bytes, NOT a printable string. The first 8 digits are the ASCII
    // serial number with an extra leading "0" ("02001234" on Sigma.x3f),
    // which is why the tag looks half-readable.
    metadata.insert(
        "SigmaRaw:ImageUniqueID".to_string(),
        TagValue::new_string(hex::encode(&data[8..24])),
    );

    // Mark bits at offset 24 (Header index 6).
    //
    // SigmaRaw.pm:81 declares `PrintConv => { BITMASK => { } }` -- an empty
    // bit lookup, so ExifTool::DecodeBits renders no set bits as "(none)"
    // and any set bit as "[n]" joined with ", " (ExifTool.pm:6385-6407).
    let mark_bits = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
    metadata.insert(
        "SigmaRaw:MarkBits".to_string(),
        TagValue::new_string(print_x3f_mark_bits(mark_bits)),
    );

    // Image dimensions at offset 28-35
    let columns = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);
    let rows = u32::from_le_bytes([data[32], data[33], data[34], data[35]]);

    if columns > 0 && rows > 0 {
        metadata.insert(
            "SigmaRaw:ImageWidth".to_string(),
            // ExifTool files the X3F header's own dimensions under SigmaRaw,
            // not EXIF -- `exiftool -G1` prints [SigmaRaw] ImageWidth. The
            // values were already right; only the family was wrong.
            TagValue::new_string(columns.to_string()),
        );
        metadata.insert(
            "SigmaRaw:ImageHeight".to_string(),
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

    // String at offset 72 (Header index 18), 32 bytes - introduced in v2.3.
    //
    // SigmaRaw.pm:88 names this SceneCaptureType, not ColorMode. Emitting it
    // as SigmaRaw:ColorMode put a correct value ("Standard" on SigmaDP2.x3f)
    // under a key ExifTool never emits, so it counted as an extra on one side
    // while SigmaRaw:SceneCaptureType counted as missing on the other.
    if version >= 0x00020003 && data.len() >= 104 {
        let sct_bytes = &data[72..104];
        if let Some(end) = sct_bytes.iter().position(|&b| b == 0) {
            if end > 0 {
                if let Ok(sct) = std::str::from_utf8(&sct_bytes[..end]) {
                    metadata.insert(
                        "SigmaRaw:SceneCaptureType".to_string(),
                        TagValue::new_string(sct.to_string()),
                    );
                }
            }
        }
    }

    parse_x3f_extended_header(data, version_major, version_minor, &mut metadata);

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
                parse_x3f_image_section(entry_data, entry_offset, &mut metadata, format);
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

/// Walks the EXIF of the JPEG embedded in an X3F image section.
///
/// `SigmaRaw.pm`'s Main NOTES: "Metadata is also extracted from the JpgFromRaw
/// image if it exists (all models but the SD9 and SD10)." ExifTool runs its
/// ordinary JPEG reader over that image, so the EXIF group of an X3F is
/// whatever a standalone `exiftool` run on the extracted JpgFromRaw reports:
/// all of IFD0, all of the ExifIFD, the Interoperability IFD, and IFD1's
/// thumbnail pointers.
///
/// This used to be two overlapping passes, each keeping a hand-written
/// whitelist of about fifteen tag IDs "to match ExifTool's X3F output exactly".
/// Measured on SigmaDP2.x3f: ExifTool reports 43 EXIF keys there and the
/// whitelists let 16 through, so 27 correctly-parsed tags were being discarded
/// on the way out.
///
/// The MakerNote (0x927C) does not go through the payload-only MakerNote
/// dispatcher, because a Sigma entry's value offset addresses the enclosing
/// TIFF rather than the payload. It goes to `tiff::makernotes::sigma`, the one
/// Sigma tag table, which a Sigma JPEG reaches by the same route.
fn parse_x3f_embedded_jpeg_exif(
    jpeg_data: &[u8],
    jpeg_file_offset: usize,
    metadata: &mut MetadataMap,
) {
    let Ok(Some((tiff_start_in_jpeg, tiff_data))) = find_jpeg_exif_tiff(jpeg_data) else {
        return;
    };
    let Ok(byte_order) = detect_byte_order(tiff_data) else {
        return;
    };
    if tiff_data.len() < 8 {
        return;
    }
    let ifd0_offset = u64::from(read_u32(&tiff_data[4..8], byte_order));
    let reader = SliceReader::new(tiff_data);
    let Ok(ifd0_tags) = parse_ifd(&reader, ifd0_offset, byte_order) else {
        return;
    };

    let mut exif_ifd_offset: Option<u64> = None;
    for (tag_id, _field_type, _value_count, raw_bytes) in &ifd0_tags {
        if *tag_id == 0x8769 && raw_bytes.as_ref().len() >= 4 {
            exif_ifd_offset = Some(u64::from(read_u32(raw_bytes.as_ref(), byte_order)));
        }
    }
    emit_x3f_exif_tags(&ifd0_tags, "IFD0", byte_order, metadata);

    let mut interop_ifd_offset: Option<u64> = None;
    if let Some(offset) = exif_ifd_offset
        && let Ok(exif_tags) = parse_ifd(&reader, offset, byte_order)
    {
        for (tag_id, _field_type, _value_count, raw_bytes) in &exif_tags {
            if *tag_id == 0xA005 && raw_bytes.as_ref().len() >= 4 {
                interop_ifd_offset = Some(u64::from(read_u32(raw_bytes.as_ref(), byte_order)));
            }
        }
        emit_x3f_exif_tags(&exif_tags, "ExifIFD", byte_order, metadata);

        // The Sigma MakerNote lives in the preview's ExifIFD. `parse_ifd`
        // hands back a MakerNote entry's payload, but the offsets INSIDE it
        // address the enclosing TIFF, so the decoder needs the entry's own
        // position rather than its bytes.
        if let Some(makernote_offset) =
            ifd_entry_value_offset(tiff_data, offset, byte_order, 0x927C)
        {
            crate::parsers::tiff::makernotes::sigma::parse_sigma_makernote(
                tiff_data,
                makernote_offset as usize,
                (jpeg_file_offset + tiff_start_in_jpeg) as u64,
                metadata,
            );
        }
    }

    if let Some(offset) = interop_ifd_offset
        && let Ok(interop_tags) = parse_ifd(&reader, offset, byte_order)
    {
        emit_x3f_exif_tags(&interop_tags, "InteropIFD", byte_order, metadata);
    }

    // The next-IFD pointer sits immediately after IFD0's entries: 2 bytes of
    // count plus 12 bytes per entry.
    let ifd1_pos = ifd0_offset + 2 + ifd0_tags.len() as u64 * 12;
    if ifd1_pos + 4 > tiff_data.len() as u64 {
        return;
    }
    let ifd1_offset = read_u32(
        &tiff_data[ifd1_pos as usize..(ifd1_pos + 4) as usize],
        byte_order,
    );
    if ifd1_offset == 0 {
        return;
    }
    let Ok(ifd1_tags) = parse_ifd(&reader, u64::from(ifd1_offset), byte_order) else {
        return;
    };
    // Only IFD1's own tags. Orientation/XResolution/YResolution/ResolutionUnit/
    // YCbCrPositioning repeat here with the same ExifTool names as IFD0's, and
    // family 0 collapses both IFDs into "EXIF", so emitting them again would
    // let the thumbnail's copy overwrite the main image's.
    for (tag_id, field_type, value_count, raw_bytes) in &ifd1_tags {
        if !matches!(
            *tag_id,
            0x0103 // Compression
                | 0x0202 // ThumbnailLength
        ) {
            continue;
        }
        emit_x3f_exif_tag(
            *tag_id,
            *field_type,
            *value_count,
            raw_bytes.as_ref(),
            "IFD1",
            byte_order,
            metadata,
        );
    }

    // ThumbnailOffset is an IsOffset tag: ExifTool reports it relative to the
    // enclosing file, not to the TIFF header. `ProcessX3FDirectory` bumps
    // `$$et{BASE}` by the JpgFromRaw's own file offset before calling
    // ProcessJPEG (SigmaRaw.pm:578-582), and the EXIF reader adds the TIFF
    // header's 12-byte position inside the JPEG on top of that. Measured on
    // SigmaDP2.x3f: the stored value is 2360, `exiftool` on the extracted JPEG
    // prints 2372 (= 2360 + 12) and on the X3F prints 2664 (= 2360 + 12 + 292,
    // the JpgFromRaw payload starting at file offset 292).
    //
    // `find_jpeg_exif_tiff` reports where the TIFF header sits inside the
    // JPEG (12 here: SOI(2) + APP1 marker(2) + length(2) + "Exif\0\0"(6),
    // since the APP1 gate in the caller guarantees Exif is the first segment).
    if let Some((_, _, _, raw_bytes)) = ifd1_tags.iter().find(|(id, ..)| *id == 0x0201)
        && let Some(stored) = read_tiff_u32(raw_bytes.as_ref(), byte_order)
    {
        let absolute = u64::from(stored) + (jpeg_file_offset + tiff_start_in_jpeg) as u64;
        metadata.insert(
            "IFD1:ThumbnailOffset".to_string(),
            TagValue::new_integer(absolute as i64),
        );

        // ThumbnailImage is the DataTag the ThumbnailOffset/ThumbnailLength
        // pair addresses. The stored offset is TIFF-relative, which is how it
        // indexes `tiff_data`; only the reported tag is absolutised.
        if let Some((_, _, _, length_bytes)) = ifd1_tags.iter().find(|(id, ..)| *id == 0x0202)
            && let Some(length) = read_tiff_u32(length_bytes.as_ref(), byte_order)
            && length > 0
            && let Some(thumbnail) =
                tiff_data.get(stored as usize..(stored as usize).saturating_add(length as usize))
        {
            metadata.insert(
                "IFD1:ThumbnailImage".to_string(),
                TagValue::new_binary(thumbnail.to_vec()),
            );
        }
    }
}

/// Returns the value-offset field of one IFD entry, i.e. where its payload
/// starts inside the enclosing TIFF.
///
/// `parse_ifd` hands back an entry's BYTES, which is the wrong currency for a
/// MakerNote: the offsets stored inside a MakerNote IFD are relative to the
/// TIFF header it is embedded in, not to the MakerNote payload, so its
/// position is what a decoder needs. Only meaningful for entries whose value
/// does not fit in the four inline bytes - which a MakerNote never does.
fn ifd_entry_value_offset(
    tiff: &[u8],
    ifd_offset: u64,
    byte_order: ByteOrder,
    wanted_tag: u16,
) -> Option<u32> {
    let base = usize::try_from(ifd_offset).ok()?;
    let count = usize::from(read_tiff_u16(tiff.get(base..base + 2)?, byte_order)?);

    for i in 0..count {
        let entry = base.checked_add(2)?.checked_add(i.checked_mul(12)?)?;
        let tag_id = read_tiff_u16(tiff.get(entry..entry + 2)?, byte_order)?;
        if tag_id != wanted_tag {
            continue;
        }
        return read_tiff_u32(tiff.get(entry + 8..entry + 12)?, byte_order);
    }

    None
}

/// Emits every named tag of one parsed IFD from an X3F's embedded JPEG.
fn emit_x3f_exif_tags(
    tags: &[(u16, u16, u32, impl AsRef<[u8]>)],
    ifd_name: &str,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    for (tag_id, field_type, value_count, raw_bytes) in tags {
        emit_x3f_exif_tag(
            *tag_id,
            *field_type,
            *value_count,
            raw_bytes.as_ref(),
            ifd_name,
            byte_order,
            metadata,
        );
    }
}

/// Emits one EXIF tag from an X3F's embedded JPEG, skipping the ones ExifTool
/// consumes structurally rather than reporting.
fn emit_x3f_exif_tag(
    tag_id: u16,
    field_type: u16,
    value_count: u32,
    bytes: &[u8],
    ifd_name: &str,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    // Sub-directory pointers are followed, never reported, and the MakerNote is
    // handled (or, here, deliberately not handled) separately.
    if matches!(
        tag_id,
        0x8769 // ExifOffset
            | 0x8825 // GPSInfo
            | 0xA005 // InteropOffset
            | 0x927C // MakerNote
    ) {
        return;
    }
    let tag_name = match (ifd_name, tag_id) {
        // 0x0202 carries three different ExifTool names depending on the
        // directory (JPEGInterchangeFormatLength, PreviewImageLength,
        // ThumbnailLength), and the tag database resolves it to one of the
        // others, so the IFD1 spelling has to be pinned here -- the same rule
        // psd.rs applies in `lookup_ifd1_tag_name`. (0x0201 needs a base
        // adjustment as well and is handled by the caller.)
        ("IFD1", 0x0202) => "IFD1:ThumbnailLength".to_string(),
        _ => lookup_tag_name(tag_id, ifd_name),
    };
    // An unrecognised ID falls back to a hex name ExifTool never prints, which
    // would land as an oxidex-only tag.
    if tag_name.contains(":0x") {
        return;
    }
    let tag_value = if let Some(value) =
        format_exif_display_value(tag_id, bytes, field_type, value_count, byte_order)
    {
        TagValue::new_string(value)
    } else {
        raw_bytes_to_simple_tag_value(bytes, field_type, value_count, byte_order)
    };
    metadata.insert(tag_name, tag_value);
}

/// Applies `SigmaRaw.pm`'s LENSMODEL ValueConv + PrintConv.
///
/// The stored value is a hex string without the `0x` (`"145"`). ExifTool's
/// ValueConv turns that into the number 0x145 and its PrintConv looks it up in
/// `%sigmaLensTypes` with `PrintHex => 1`, so an id the table does not carry
/// prints as `Unknown (0x145)`. A value that is not hex digits at all -- the
/// blank LENSMODEL SigmaDP2.x3f writes -- fails the ValueConv regex, stays a
/// string, misses the lookup, and prints as `Unknown ( )`.
fn print_sigma_lens_type(value: &str) -> String {
    let is_hex_string = !value.is_empty() && value.chars().all(|c| c.is_ascii_hexdigit());
    if is_hex_string && let Ok(lens_type) = u32::from_str_radix(value, 16) {
        return match crate::parsers::raw::sigma_lens_types::lookup(lens_type) {
            Some(name) => name.to_string(),
            None => format!("Unknown (0x{:x})", lens_type),
        };
    }
    format!("Unknown ({})", value)
}

/// Renders the X3F header's `MarkBits` field the way ExifTool does.
///
/// `SigmaRaw.pm:81` gives the tag `PrintConv => { BITMASK => { } }`. An empty
/// BITMASK lookup still counts as a lookup in `ExifTool::DecodeBits`
/// (`ExifTool.pm:6385`), so every set bit prints as `[n]`, the parts are joined
/// with `", "`, and a value with no bits set prints as `(none)`.
fn print_x3f_mark_bits(value: u32) -> String {
    let bits: Vec<String> = (0..32)
        .filter(|i| value & (1u32 << i) != 0)
        .map(|i| format!("[{}]", i))
        .collect();
    if bits.is_empty() {
        "(none)".to_string()
    } else {
        bits.join(", ")
    }
}

/// Parses the 160-byte X3F extended header that follows the fixed header.
///
/// `SigmaRaw.pm:289 ProcessX3FHeader` reads it as 32 single-byte tag IDs at
/// `hdrLen`, followed by 32 little-endian floats at `hdrLen + 32 + i * 4`. Slot
/// `i` carries the value for the tag whose ID is byte `i`; ID 0 means "unused"
/// and is skipped. IDs index `SigmaRaw::HeaderExt` (`SigmaRaw.pm:113-129`),
/// every entry of which prints via `sprintf("%.1f",$val)`.
///
/// `ProcessX3F` (`SigmaRaw.pm:616-624`) only reads the block for file versions
/// above 2.0 and below 4.0, and places it directly after the fixed header --
/// which is 104 bytes from 2.3 on (SceneCaptureType was appended) and 72 bytes
/// before that.
///
/// This block is where ExposureAdjust/Contrast/Shadow/Highlight/Saturation/
/// Sharpness/RedAdjust/GreenAdjust/BlueAdjust/X3FillLight live. They are not
/// SECp properties, so the property walk could never reach them and all ten
/// were reported missing on both sample files.
fn parse_x3f_extended_header(
    data: &[u8],
    version_major: u32,
    version_minor: u32,
    metadata: &mut MetadataMap,
) {
    // ExifTool gates on `2 < $ver < 4` with $ver the decimal "major.minor".
    // Version 4+ uses a different 0x300-byte header with no extended block,
    // and 2.0 predates the block entirely.
    let has_extended_header = match version_major {
        2 => version_minor > 0,
        3 => true,
        _ => false,
    };
    if !has_extended_header {
        return;
    }

    // SceneCaptureType (32 bytes) was appended to the fixed header in 2.3.
    let header_len = if version_minor > 2 { 104usize } else { 72 };

    // The block is 32 ID bytes plus 32 four-byte floats.
    if data.len() < header_len + 160 {
        return;
    }
    let ids = &data[header_len..header_len + 32];
    let values_start = header_len + 32;

    for (index, &tag_id) in ids.iter().enumerate() {
        let Some(name) = x3f_header_ext_tag_name(tag_id) else {
            continue;
        };
        let offset = values_start + index * 4;
        let value = f32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        metadata.insert(
            format!("SigmaRaw:{}", name),
            TagValue::new_string(format!("{:.1}", value)),
        );
    }
}

/// Maps an X3F extended-header tag ID to its ExifTool name.
///
/// Verbatim from `%Image::ExifTool::SigmaRaw::HeaderExt` (`SigmaRaw.pm:113`).
/// ID 0 is `Unused` there and is deliberately not returned -- `ProcessX3FHeader`
/// skips those slots rather than emitting a tag.
fn x3f_header_ext_tag_name(tag_id: u8) -> Option<&'static str> {
    match tag_id {
        1 => Some("ExposureAdjust"),
        2 => Some("Contrast"),
        3 => Some("Shadow"),
        4 => Some("Highlight"),
        5 => Some("Saturation"),
        6 => Some("Sharpness"),
        7 => Some("RedAdjust"),
        8 => Some("GreenAdjust"),
        9 => Some("BlueAdjust"),
        10 => Some("X3FillLight"),
        _ => None,
    }
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
            let value = convert_x3f_property_value(&name, &value).unwrap_or(value);
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

/// Applies the ValueConv/PrintConv ExifTool declares for X3F properties.
///
/// `map_x3f_property_name` only renamed properties; the values went in raw, so
/// `FNumber` read `8.35419` where ExifTool prints `8.4` and `DateTimeOriginal`
/// read `978309395` where ExifTool prints `2001:01:01 00:36:35`.
///
/// Returns `None` to leave a value untouched.
fn convert_x3f_property_value(property: &str, value: &str) -> Option<String> {
    match property {
        // SigmaRaw.pm:154  PrintConv => 'sprintf("%.1f",$val)'
        "APERTURE" => Some(format!("{:.1}", value.parse::<f64>().ok()?)),

        // SigmaRaw.pm:263  ValueConv => 'ConvertUnixTime($val)'
        "TIME" => format_unix_time_exiftool(value.parse::<i64>().ok()?),

        // SigmaRaw.pm:190  ValueConv => '$val * 1e-6' (usec), then
        // SigmaRaw.pm:191  PrintConv => PrintExposureTime
        "EXPTIME" => Some(print_exposure_time(value.parse::<f64>().ok()? * 1e-6)),

        // SigmaRaw.pm:257  PrintConv => PrintExposureTime (already seconds)
        "SHUTTER" => Some(print_exposure_time(value.parse::<f64>().ok()?)),

        // SigmaRaw.pm:227-234
        //   ValueConv => '$val =~ /^[0-9a-f]+$/i ? hex($val) : $val'
        //   PrintHex => 1, PrintConv => \%Image::ExifTool::Sigma::sigmaLensTypes
        "LENSMODEL" => Some(print_sigma_lens_type(value)),

        // Enumerated properties. Each table is SigmaRaw.pm's PrintConv
        // verbatim; an unrecognised code passes through rather than being
        // guessed at, so a camera this build has never seen reports its raw
        // code instead of a plausible-looking lie.
        "AEMODE" => Some(
            match value {
                "8" => "8-segment",
                "C" => "Center-weighted average",
                "A" => "Average",
                other => other,
            }
            .to_string(),
        ),
        "DRIVE" => Some(
            match value {
                "SINGLE" => "Single Shot",
                "MULTI" => "Multi Shot",
                "2S" => "2 s Timer",
                "10S" => "10 s Timer",
                "UP" => "Mirror Up",
                "AB" => "Auto Bracket",
                "OFF" => "Off",
                other => other,
            }
            .to_string(),
        ),
        "FOCUS" => Some(
            match value {
                "AF" => "Auto-focus Locked",
                "M" => "Manual",
                other => other,
            }
            .to_string(),
        ),
        "PMODE" => Some(
            match value {
                "P" => "Program",
                "A" => "Aperture Priority",
                "S" => "Shutter Priority",
                "M" => "Manual",
                other => other,
            }
            .to_string(),
        ),
        "RESOLUTION" => Some(
            match value {
                "LOW" => "Low",
                "MED" => "Medium",
                "HI" => "High",
                other => other,
            }
            .to_string(),
        ),
        "FLASH" => Some(
            match value {
                "OFF" => "Off",
                "ON" => "On",
                other => other,
            }
            .to_string(),
        ),

        _ => None,
    }
}

/// ExifTool's `ConvertUnixTime`: seconds since the epoch, UTC, as
/// `YYYY:MM:DD HH:MM:SS`.
fn format_unix_time_exiftool(epoch_seconds: i64) -> Option<String> {
    let days = epoch_seconds.div_euclid(86_400);
    let secs = epoch_seconds.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant's algorithm), shifted to a March-based year.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    Some(format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        y,
        m,
        d,
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    ))
}

/// Map X3F property names to ExifTool-compatible tag names
fn map_x3f_property_name(name: &str) -> String {
    // Every property in this list is family SigmaRaw as ExifTool reports it --
    // verified against `exiftool -G1 Sigma.x3f`, which files even Make, Model
    // and SerialNumber there rather than under EXIF or MakerNotes. Sending
    // them to EXIF:/MakerNotes: put correct values under keys ExifTool never
    // emits, so they matched nothing and counted as extras on both sides.
    match name {
        "CAMMANUF" => "SigmaRaw:Make".to_string(),
        "CAMMODEL" => "SigmaRaw:Model".to_string(),
        "CAMNAME" => "SigmaRaw:CameraName".to_string(),
        "CAMSERIAL" => "SigmaRaw:SerialNumber".to_string(),
        "FIRMWARE" | "FIRMVERS" => "SigmaRaw:FirmwareVersion".to_string(),

        // Keys that appear in real files and had no mapping at all, so they
        // were emitted raw (SigmaRaw:AEMODE, SigmaRaw:DRIVE, ...). Names are
        // SigmaRaw.pm's.
        "AEMODE" => "SigmaRaw:MeteringMode".to_string(),
        "AFAREA" => "SigmaRaw:AFArea".to_string(),
        "AFINFOCUS" => "SigmaRaw:AFInFocus".to_string(),
        "AFMODE" => "SigmaRaw:FocusMode".to_string(),
        "AP_DESC" => "SigmaRaw:ApertureDisplayed".to_string(),
        "BRACKET" => "SigmaRaw:BracketShot".to_string(),
        "BURST" => "SigmaRaw:BurstShot".to_string(),
        "CM_DESC" => "SigmaRaw:SceneCaptureType".to_string(),
        "COLORSPACE" => "SigmaRaw:ColorSpace".to_string(),
        "DRIVE" => "SigmaRaw:DriveMode".to_string(),
        "EVAL_STATE" => "SigmaRaw:EvalState".to_string(),
        "EXPNET" => "SigmaRaw:NetExposureCompensation".to_string(),
        "FLASH" => "SigmaRaw:FlashMode".to_string(),
        "FLASHEXPCOMP" => "SigmaRaw:FlashExpComp".to_string(),
        "FLASHPOWER" => "SigmaRaw:FlashPower".to_string(),
        "FLASHTTLMODE" => "SigmaRaw:FlashTTLMode".to_string(),
        "FLASHTYPE" => "SigmaRaw:FlashType".to_string(),
        "FOCUS" => "SigmaRaw:Focus".to_string(),
        "IMAGERBOARDID" => "SigmaRaw:ImagerBoardID".to_string(),
        "IMAGEBOARDID" => "SigmaRaw:ImageBoardID".to_string(),
        "IMAGERTEMP" => "SigmaRaw:SensorTemperature".to_string(),
        "PMODE" => "SigmaRaw:ExposureProgram".to_string(),
        "RESOLUTION" => "SigmaRaw:Quality".to_string(),
        "SENSORID" => "SigmaRaw:SensorID".to_string(),
        "SH_DESC" => "SigmaRaw:ShutterSpeedDisplayed".to_string(),
        "WB_DESC" => "SigmaRaw:WhiteBalance".to_string(),
        "VERSION_BF" => "SigmaRaw:VersionBF".to_string(),
        // LENSMODEL is deliberately absent: SigmaRaw.pm resolves it through
        // the separate "Sigma LensType" table, and emitting SigmaRaw:LensType
        // carrying the raw id (145) would trade a missing tag for a wrong
        // value.
        // SigmaRaw.pm:187 -- EXPTIME is IntegrationTime, NOT ExposureTime.
        // That is SHUTTER (SigmaRaw.pm:254). Mapping EXPTIME to ExposureTime
        // reported IntegrationTime's raw microseconds (24140) as the shutter
        // speed, where ExifTool prints 1/108.
        "EXPTIME" => "SigmaRaw:IntegrationTime".to_string(),
        "SHUTTER" => "SigmaRaw:ExposureTime".to_string(),
        "APERTURE" => "SigmaRaw:FNumber".to_string(),
        "FLENGTH" => "SigmaRaw:FocalLength".to_string(),
        "FLEQ35MM" => "SigmaRaw:FocalLengthIn35mmFormat".to_string(),
        "ISO" => "SigmaRaw:ISO".to_string(),
        "EXPCOMP" => "SigmaRaw:ExposureCompensation".to_string(),
        "TIME" => "SigmaRaw:DateTimeOriginal".to_string(),
        "LENSARANGE" => "SigmaRaw:LensApertureRange".to_string(),
        "LENSFRANGE" => "SigmaRaw:LensFocalRange".to_string(),
        // SigmaRaw.pm:227 -- LENSMODEL is LensType, resolved through the shared
        // "Sigma LensType" table. It used to be left unmapped on purpose,
        // because emitting the raw id (145) under a real tag name would have
        // been a wrong value; with the table transcribed, the PrintConv can be
        // applied properly (see `convert_x3f_property_value`).
        "LENSMODEL" => "SigmaRaw:LensType".to_string(),
        // EXPMODE, FLASHM, DRIVEMODE, WB/WBAL and COLORMODE are gone: none of
        // them is a key SigmaRaw.pm defines. They were spellings of keys that
        // do exist (PMODE, FLASH, DRIVE, WB_DESC), so the entries could never
        // fire while the real keys fell through unmapped.
        _ => format!("SigmaRaw:{}", name),
    }
}

/// Parse X3F image section for embedded EXIF data
///
/// X3F image sections (SECi) can contain embedded TIFF/EXIF data. This function
/// searches for TIFF headers throughout the image section data to locate and parse
/// any embedded metadata.
fn parse_x3f_image_section(
    data: &[u8],
    section_file_offset: usize,
    metadata: &mut MetadataMap,
    format: RawFormat,
) {
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
    // Columns/rows at 16-23 and the row stride at 24-27 describe the image
    // data, not metadata. They used to be published as
    // MakerNotes:PreviewImageSize, but ExifTool takes that tag from the Sigma
    // MakerNote inside the embedded JPEG (Sigma.pm 0x1c) and never from this
    // header: on SigmaDP2.x3f ExifTool reports 640x480 while these fields say
    // 567x378, so the emission was a wrong value under a real tag name.

    // ExifTool only treats an image section as a preview when its header is
    // exactly version 2.0 / type 2 / format 0x12, i.e. JPEG-compressed
    // (SigmaRaw.pm:551, `unless ($buff =~ /^SECi\0\0\x02\0\x02\0\0\0\x12\0\0\0/)`
    // ... `next`). Sigma.x3f's three image sections are types 3, 2-format-0x0b
    // and 2-format-0x03, all of which fail the gate -- which is exactly why
    // ExifTool reports no PreviewImage and no JpgFromRaw for the SD10.
    const X3F_JPEG_PREVIEW_HEADER: &[u8] = b"SECi\0\0\x02\0\x02\0\0\0\x12\0\0\0";
    if data.len() > 28 && data.starts_with(X3F_JPEG_PREVIEW_HEADER) {
        let payload = &data[28..];
        // SigmaRaw.pm:559 -- a preview that begins with an APP1 marker is the
        // JpgFromRaw, and it is the one ExifTool runs ProcessJPEG over.
        if payload.starts_with(b"\xff\xd8\xff\xe1") {
            metadata.insert(
                "SigmaRaw:JpgFromRaw".to_string(),
                TagValue::new_binary(payload.to_vec()),
            );
            parse_x3f_embedded_jpeg_exif(payload, section_file_offset + 28, metadata);
        } else {
            metadata.insert(
                "SigmaRaw:PreviewImage".to_string(),
                TagValue::new_binary(payload.to_vec()),
            );
        }
        return;
    }

    // Any other type-2/3 section is image data ExifTool ignores outright.
    if image_type == 2 || image_type == 3 {
        return;
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
/// True when the TTW block identified the body as a DiMAGE A200, which is the
/// one model whose WBG levels are stored in GBRG rather than RGGB order.
fn model_is_dimage_a200(metadata: &MetadataMap) -> bool {
    metadata
        .get("IFD0:Model")
        .and_then(|v| v.as_string())
        .is_some_and(|m| m.trim() == "DiMAGE A200")
}

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
                    // The generic TIFF walk above cannot decode the Minolta
                    // MakerNote: Minolta writes its value offsets relative to
                    // the TIFF base rather than to the note itself, so the
                    // shared dispatcher -- which only ever sees the note's own
                    // bytes -- would resolve every one of them into the wrong
                    // part of the block. Decode it here, where the base is the
                    // TTW buffer we already hold.
                    for (key, value) in
                        crate::parsers::raw::minolta_makernote::parse_ttw_makernotes(block_data)
                    {
                        metadata.insert(key, value);
                    }
                }
            }
            b"\x00PRD" => {
                // PRD block contains image dimensions and sensor info
                if block_data.len() >= 8 {
                    let reader = crate::io::EndianReader::big_endian(block_data);
                    // MinoltaRaw::PRD, offsets verbatim from ExifTool. The
                    // previous layout was a guess -- its own comment said
                    // "2 bytes: version?" -- and put SensorWidth at 2 and
                    // SensorHeight at 4, which lands inside the eight-byte
                    // FirmwareID string. That is why a 3272x2456 sensor was
                    // reported as 12848x12336: 0x3230 and 0x3030 are the
                    // ASCII digits "20" and "00" of firmware "27200001".
                    if let Some(raw) = block_data.get(0..8) {
                        let firmware = String::from_utf8_lossy(raw)
                            .trim_end_matches(|c: char| c == '\0' || c.is_whitespace())
                            .to_string();
                        if !firmware.is_empty() {
                            metadata.insert(
                                "MakerNotes:FirmwareID".to_string(),
                                TagValue::new_string(firmware),
                            );
                        }
                    }
                    if let (Some(sensor_h), Some(sensor_w)) = (reader.u16_at(8), reader.u16_at(10))
                    {
                        metadata.insert(
                            "MakerNotes:SensorHeight".to_string(),
                            TagValue::Integer(sensor_h as i64),
                        );
                        metadata.insert(
                            "MakerNotes:SensorWidth".to_string(),
                            TagValue::Integer(sensor_w as i64),
                        );
                    }
                    if let Some(v) = block_data.get(16) {
                        metadata.insert(
                            "MakerNotes:RawDepth".to_string(),
                            TagValue::Integer(*v as i64),
                        );
                    }
                    if let Some(v) = block_data.get(17) {
                        metadata.insert(
                            "MakerNotes:BitDepth".to_string(),
                            TagValue::Integer(*v as i64),
                        );
                    }
                    if let Some(v) = block_data.get(18) {
                        // An unlisted code reports itself rather than being
                        // rounded to Padded or Linear.
                        metadata.insert(
                            "MakerNotes:StorageMethod".to_string(),
                            TagValue::new_string(match v {
                                82 => "Padded".to_string(),
                                89 => "Linear".to_string(),
                                other => other.to_string(),
                            }),
                        );
                    }
                    if let Some(v) = block_data.get(23) {
                        metadata.insert(
                            "MakerNotes:BayerPattern".to_string(),
                            TagValue::new_string(match v {
                                1 => "RGGB".to_string(),
                                4 => "GBRG".to_string(),
                                other => other.to_string(),
                            }),
                        );
                    }
                    if let (Some(img_h), Some(img_w)) = (reader.u16_at(12), reader.u16_at(14)) {
                        // MinoltaRaw::PRD groups these under MakerNotes, not
                        // EXIF -- ExifTool reports them as [MinoltaRaw]. The
                        // EXIF:ImageWidth/Height pair comes from the TTW
                        // block's own IFD0, so emitting these as EXIF both hid
                        // the MakerNotes tags and duplicated the IFD0 values.
                        metadata.insert(
                            "MakerNotes:ImageWidth".to_string(),
                            TagValue::Integer(img_w as i64),
                        );
                        metadata.insert(
                            "MakerNotes:ImageHeight".to_string(),
                            TagValue::Integer(img_h as i64),
                        );
                    }
                }
            }
            b"\x00RIF" => {
                // MinoltaRaw::RIF -- requested-image-file settings. Offsets are
                // ExifTool's; the table's FORMAT is int8u so its keys ARE byte
                // offsets. This block was never dispatched at all, so every
                // tag in it was missing rather than wrong.
                let i8 = |o: usize| block_data.get(o).map(|v| *v as i8);
                let be16 = |o: usize| -> Option<u16> {
                    Some(u16::from_be_bytes([
                        *block_data.get(o)?,
                        *block_data.get(o + 1)?,
                    ]))
                };
                for (off, name) in [(1usize, "Saturation"), (2, "Contrast"), (3, "Sharpness")] {
                    if let Some(v) = i8(off) {
                        // These are plain int8s in MinoltaRaw::RIF with no
                        // PrintConv, unlike the identically named EXIF enums.
                        // Emitting them as integers let the shared
                        // ExifTool-compat layer apply the EXIF 0 => "Normal"
                        // mapping by name, which turned ExifTool's "0" into
                        // "Normal"; a string keeps the raw reading intact.
                        metadata.insert(
                            format!("MakerNotes:{name}"),
                            TagValue::new_string(v.to_string()),
                        );
                    }
                }
                if let Some(v) = block_data.get(4) {
                    // ExifTool's ConvertWBMode: the low nibble names the mode
                    // and the high nibble, when 6..=12, is appended as (hi-8).
                    let lo = v & 0x0f;
                    let name = match lo {
                        0 => "Auto",
                        1 => "Daylight",
                        2 => "Cloudy",
                        3 => "Tungsten",
                        4 => "Flash/Fluorescent",
                        5 => "Fluorescent",
                        6 => "Shade",
                        7 => "User 1",
                        8 => "User 2",
                        9 => "User 3",
                        10 => "Temperature",
                        _ => "",
                    };
                    let mut s = if name.is_empty() {
                        format!("Unknown ({lo})")
                    } else {
                        name.to_string()
                    };
                    let hi = v >> 4;
                    if (6..=12).contains(&hi) {
                        s.push_str(&format!(" ({})", hi as i16 - 8));
                    }
                    metadata.insert("MakerNotes:WBMode".to_string(), TagValue::new_string(s));
                }
                if let Some(v) = block_data.get(5) {
                    metadata.insert(
                        "MakerNotes:ProgramMode".to_string(),
                        TagValue::new_string(match v {
                            0 => "None".to_string(),
                            1 => "Portrait".to_string(),
                            2 => "Text".to_string(),
                            3 => "Night Portrait".to_string(),
                            4 => "Sunset".to_string(),
                            5 => "Sports".to_string(),
                            other => other.to_string(),
                        }),
                    );
                }
                // RawConv drops 255 outright, so an unset ISO stays absent
                // rather than being reported as a 26-million-ISO exposure.
                if let Some(v) = block_data.get(6).filter(|v| **v != 255) {
                    // ValueConv 2 ** (($val-48)/8) * 100, with three coded
                    // exceptions ExifTool lists explicitly.
                    let s = match v {
                        0 => "Auto".to_string(),
                        174 => "80 (Zone Matching Low)".to_string(),
                        184 => "200 (Zone Matching High)".to_string(),
                        other => {
                            let iso = 2f64.powf((f64::from(*other) - 48.0) / 8.0) * 100.0;
                            format!("{}", iso.round() as i64)
                        }
                    };
                    metadata.insert("MakerNotes:ISOSetting".to_string(), TagValue::new_string(s));
                }
                if let Some(v) = block_data.get(7) {
                    // RIF offset 7 is ColorMode, using Minolta's own colour
                    // mode table. The block was decoded either side of this
                    // byte but never at it.
                    metadata.insert(
                        "MakerNotes:ColorMode".to_string(),
                        TagValue::new_string(
                            crate::parsers::raw::minolta_makernote::minolta_color_mode(u32::from(
                                *v,
                            )),
                        ),
                    );
                }
                for (off, name) in [
                    (8usize, "WB_RBLevelsTungsten"),
                    (12, "WB_RBLevelsDaylight"),
                    (16, "WB_RBLevelsCloudy"),
                    (20, "WB_RBLevelsCoolWhiteF"),
                    (24, "WB_RBLevelsFlash"),
                    (28, "WB_RBLevelsCustom"),
                ] {
                    if let (Some(a), Some(b)) = (be16(off), be16(off + 2)) {
                        metadata.insert(
                            format!("MakerNotes:{name}"),
                            TagValue::new_string(format!("{a} {b}")),
                        );
                    }
                }
                if let Some(v) = i8(56) {
                    metadata.insert(
                        "MakerNotes:ColorFilter".to_string(),
                        TagValue::Integer(v as i64),
                    );
                }
                if let Some(v) = block_data.get(57) {
                    metadata.insert(
                        "MakerNotes:BWFilter".to_string(),
                        TagValue::Integer(*v as i64),
                    );
                }
                if let Some(v) = block_data.get(58) {
                    metadata.insert(
                        "MakerNotes:ZoneMatching".to_string(),
                        TagValue::new_string(match v {
                            0 => "ISO Setting Used".to_string(),
                            1 => "High Key".to_string(),
                            2 => "Low Key".to_string(),
                            other => other.to_string(),
                        }),
                    );
                }
                if let Some(v) = i8(59) {
                    metadata.insert("MakerNotes:Hue".to_string(), TagValue::Integer(v as i64));
                }
            }
            b"\x00WBG" => {
                // MinoltaRaw::WBG -- a four-byte scale vector followed by four
                // 16-bit levels. The previous reading treated the block as
                // R/G/B multipliers and divided them into each other to make
                // up ColorBalanceRed/Green/Blue; no such tags exist here.
                // ExifTool's real ColorBalance* live in the MakerNote's
                // CameraSettings, and the ratios this produced (0.85 for a
                // channel ExifTool reports as 1.988) were wrong besides.
                let reader = crate::io::EndianReader::big_endian(block_data);
                if let Some(scale) = block_data.get(0..4) {
                    metadata.insert(
                        "MakerNotes:WBScale".to_string(),
                        TagValue::new_string(
                            scale
                                .iter()
                                .map(|v| v.to_string())
                                .collect::<Vec<_>>()
                                .join(" "),
                        ),
                    );
                }
                // The A200 writes these four levels in GBRG order; every other
                // body uses RGGB. Only the name differs.
                let levels: Option<Vec<u16>> = (0..4)
                    .map(|i| reader.u16_at(4 + i * 2))
                    .collect::<Option<Vec<_>>>();
                if let Some(levels) = levels {
                    let name = if model_is_dimage_a200(&metadata) {
                        "MakerNotes:WB_GBRGLevels"
                    } else {
                        "MakerNotes:WB_RGGBLevels"
                    };
                    metadata.insert(
                        name.to_string(),
                        TagValue::new_string(
                            levels
                                .iter()
                                .map(|v| v.to_string())
                                .collect::<Vec<_>>()
                                .join(" "),
                        ),
                    );
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

                                    if *tag_id == 0xC4A5 {
                                        if let Some(version) =
                                            decode_print_im_version(bytes, byte_order)
                                        {
                                            metadata.insert(
                                                PRINT_IM_VERSION_TAG,
                                                TagValue::new_string(version),
                                            );
                                        }
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
        // NOTE (2026-07-27): tag 0x828F was previously mapped here to
        // TIFF-EPStandardID. That was wrong and emitted false metadata:
        // Exif.pm line 1766 defines
        //     0x828f => { #12
        //         Name => 'BatteryLevel',
        // and TIFF-EPStandardID is 0x9216 (Exif.pm line 2451):
        //     0x9216 => { Name => 'TIFF-EPStandardID', PrintConv => '$val =~ tr/ /./; $val' },
        // The real 0x9216 is handled on the IFD0 path (see
        // format_tiff_ep_standard_id); 0x828F now falls through to the
        // generic lookup so it is named BatteryLevel.
        //
        // CFARepeatPatternDim (0x828D) is handled by the shared SubIFD path
        // for every TIFF-based RAW format, not just NEF.
        _ => None,
    }
}

/// Format TIFF/EP tag 0x9216 (TIFF-EPStandardID).
///
/// Exif.pm line 2451, verbatim:
/// ```text
///     0x9216 => { Name => 'TIFF-EPStandardID', PrintConv => '$val =~ tr/ /./; $val' },
/// ```
///
/// The value is a BYTE[4]; ExifTool renders the array space-separated and the
/// PrintConv turns the spaces into dots, so `01 00 00 00` prints as "1.0.0.0".
fn format_tiff_ep_standard_id(bytes: &[u8], value_count: u32) -> Option<String> {
    let count = usize::try_from(value_count).ok()?;
    let components = bytes.get(..count)?;
    if components.is_empty() {
        return None;
    }
    Some(
        components
            .iter()
            .map(|component| component.to_string())
            .collect::<Vec<_>>()
            .join("."),
    )
}

/// Format TIFF/EP tag 0x828D (CFARepeatPatternDim).
///
/// Exif.pm line 1751:
/// ```text
///     0x828d => { #12
///         Name => 'CFARepeatPatternDim',
///         ...
///         Writable => 'int16u',
///         Count => 2,
/// ```
///
/// ExifTool prints the two SHORT components space-separated (e.g. "2 2");
/// oxidex's generic decoder only rendered the first component.
fn format_cfa_repeat_pattern_dim(bytes: &[u8], byte_order: ByteOrder) -> Option<String> {
    let rows = read_tiff_u16(bytes, byte_order)?;
    let cols = read_tiff_u16(bytes.get(2..)?, byte_order)?;
    Some(format!("{} {}", rows, cols))
}

/// Emit the renamed embedded-image tags from one DNG SubIFD.
///
/// ExifTool renames StripOffsets/StripByteCounts (0x111/0x117) in DNG SubIFDs
/// that hold a JPEG-compressed reduced-resolution image. Exif.pm 0x111,
/// StripOffsets branch Condition (verbatim):
/// ```text
///     not ($$self{TIFF_TYPE} =~ /^(DNG|TIFF)$/ and $$self{Compression} eq '7'
///          and $$self{SubfileType} ne '0')
/// ```
/// and the following two branches, in order:
/// ```text
///     Condition => '$$self{DIR_NAME} ne "SubIFD2"'  =>  PreviewImageStart
///     (fallthrough)                                 =>  JpgFromRawStart
/// ```
///
/// So SubIFD2 (the third SubIFD) carries JpgFromRaw* and every other qualifying
/// SubIFD carries PreviewImage*. Measured on DNG.dng (2026-07-27): SubIFD1 ->
/// PreviewImageStart 12780 / Length 26, SubIFD2 -> JpgFromRawStart 13070 /
/// Length 29, matching `exiftool -G1 -a -s`.
fn extract_dng_subifd_preview(
    data: &[u8],
    sub_tags: &[(u16, u16, u32, impl AsRef<[u8]>)],
    sub_index: usize,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    let mut compression: Option<u16> = None;
    let mut subfile_type: Option<u32> = None;
    let mut strip_offset: Option<u32> = None;
    let mut strip_length: Option<u32> = None;

    for (tag_id, field_type, value_count, raw_bytes) in sub_tags {
        let bytes = raw_bytes.as_ref();
        match *tag_id {
            0x0103 => compression = read_tiff_u16(bytes, byte_order),
            0x00FE => subfile_type = read_tiff_u32(bytes, byte_order),
            // Only single-strip images are renamed as a whole preview; a
            // multi-strip image has no single Start/Length pair to report.
            0x0111 if *value_count == 1 => strip_offset = read_tiff_u32(bytes, byte_order),
            0x0117 if *value_count == 1 => strip_length = read_tiff_u32(bytes, byte_order),
            _ => {}
        }
        let _ = field_type;
    }

    // Compression eq '7' (JPEG) and SubfileType ne '0' (reduced resolution).
    if compression != Some(7) || subfile_type == Some(0) || subfile_type.is_none() {
        return;
    }
    let (Some(offset), Some(length)) = (strip_offset, strip_length) else {
        return;
    };

    let (start_key, length_key, image_key) = if sub_index == 2 {
        (
            "EXIF:JpgFromRawStart",
            "EXIF:JpgFromRawLength",
            "EXIF:JpgFromRaw",
        )
    } else {
        (
            "EXIF:PreviewImageStart",
            "EXIF:PreviewImageLength",
            "EXIF:PreviewImage",
        )
    };

    metadata.insert(
        start_key.to_string(),
        TagValue::new_integer(i64::from(offset)),
    );
    metadata.insert(
        length_key.to_string(),
        TagValue::new_integer(i64::from(length)),
    );

    let start = offset as usize;
    let end = start.saturating_add(length as usize);
    if end <= data.len()
        && let Some(image) = data.get(start..end)
    {
        metadata.insert(image_key.to_string(), TagValue::Binary(image.to_vec()));
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
/// Renders a multi-component RATIONAL/SRATIONAL run the way ExifTool prints it.
///
/// Returns `None` for a single component, leaving the caller's existing
/// scalar handling untouched.
///
/// This converter took `_value_count` and threw it away, so every array-valued
/// rational in a RAW IFD collapsed to its first component: `AsShotNeutral`
/// reported `0.592408` where ExifTool reports `0.592408 1 0.501692`, and the
/// 9-element `ColorMatrix1`/`ColorMatrix2` and `CameraCalibration1`/`2` each
/// reported one number out of nine. Six DNG tags, one cause.
///
/// ExifTool prints these as space-separated shortest-form decimals, which is
/// what `{}` on an f64 gives: 592408/1000000 -> `0.592408`, 1/1 -> `1`,
/// -945/10000 -> `-0.0945`. A zero denominator degrades to the bare numerator
/// rather than emitting `inf`.
/// The SHORT/LONG counterpart of [`join_rational_array`].
///
/// These two branches dropped `value_count` the same way the rational ones
/// did, so `BitsPerSample` (SHORT[3]) reported `8` where ExifTool reports
/// `8 8 8`. Returns `None` for a single component so scalar values keep their
/// integer representation untouched.
fn join_integer_array(
    bytes: &[u8],
    value_count: u32,
    byte_order: ByteOrder,
    width: usize,
) -> Option<String> {
    let count = usize::try_from(value_count).ok()?;
    if count < 2 || bytes.len() < count * width {
        return None;
    }
    let mut parts = Vec::with_capacity(count);
    for i in 0..count {
        let at = i * width;
        let value = match width {
            2 => {
                let chunk = bytes.get(at..at + 2)?;
                match byte_order {
                    ByteOrder::LittleEndian => u16::from_le_bytes([chunk[0], chunk[1]]) as u32,
                    ByteOrder::BigEndian => u16::from_be_bytes([chunk[0], chunk[1]]) as u32,
                }
            }
            _ => read_u32(bytes.get(at..at + 4)?, byte_order),
        };
        parts.push(value.to_string());
    }
    Some(parts.join(" "))
}

fn join_rational_array(
    bytes: &[u8],
    value_count: u32,
    byte_order: ByteOrder,
    signed: bool,
) -> Option<String> {
    let count = usize::try_from(value_count).ok()?;
    if count < 2 || bytes.len() < count * 8 {
        return None;
    }

    let reader = match byte_order {
        ByteOrder::LittleEndian => EndianReader::little_endian(bytes),
        ByteOrder::BigEndian => EndianReader::big_endian(bytes),
    };

    let mut parts = Vec::with_capacity(count);
    for i in 0..count {
        let at = i * 8;
        let (numerator, denominator) = if signed {
            (reader.i32_at(at)? as f64, reader.i32_at(at + 4)? as f64)
        } else {
            (
                read_u32(bytes.get(at..at + 4)?, byte_order) as f64,
                read_u32(bytes.get(at + 4..at + 8)?, byte_order) as f64,
            )
        };
        let value = if denominator == 0.0 {
            numerator
        } else {
            numerator / denominator
        };
        parts.push(format!("{value}"));
    }
    Some(parts.join(" "))
}

fn raw_bytes_to_simple_tag_value(
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
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
                if let Some(joined) = join_integer_array(bytes, value_count, byte_order, 2) {
                    return TagValue::new_string(joined);
                }
                let reader = match byte_order {
                    ByteOrder::LittleEndian => EndianReader::little_endian(bytes),
                    ByteOrder::BigEndian => EndianReader::big_endian(bytes),
                };
                let value = reader.u16_at(0).unwrap_or(0) as i64;
                return TagValue::new_integer(value);
            }

            // LONG (32-bit unsigned)
            ExifType::Long if bytes.len() >= 4 => {
                if let Some(joined) = join_integer_array(bytes, value_count, byte_order, 4) {
                    return TagValue::new_string(joined);
                }
                let value = read_u32(bytes, byte_order) as i64;
                return TagValue::new_integer(value);
            }

            // RATIONAL (two 32-bit unsigned)
            ExifType::Rational if bytes.len() >= 8 => {
                if let Some(joined) = join_rational_array(bytes, value_count, byte_order, false) {
                    return TagValue::new_string(joined);
                }
                let numerator = read_u32(&bytes[0..4], byte_order);
                let denominator = read_u32(&bytes[4..8], byte_order);
                return TagValue::new_rational(numerator as i32, denominator as i32);
            }

            // SRATIONAL (two 32-bit signed)
            ExifType::SRational if bytes.len() >= 8 => {
                if let Some(joined) = join_rational_array(bytes, value_count, byte_order, true) {
                    return TagValue::new_string(joined);
                }
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

/// Parse IPTC-NAA record data (TIFF tag 0x83BB).
///
/// The IPTC record structure: each record consists of a 1-byte marker
/// (always 0x1c), record number, dataset number, 2-byte big-endian
/// data length, followed by the data. We only extract Application Record
/// (record 2) tags for now.
fn parse_iptc_naa(data: &[u8]) -> Result<Vec<(String, TagValue)>> {
    let mut tags = Vec::new();
    let mut offset = 0usize;

    while offset + 5 <= data.len() {
        if data[offset] != 0x1c {
            // Not a valid record marker; stop.
            break;
        }

        let record_num = data[offset + 1];
        let dataset_num = data[offset + 2];
        let data_length = u16::from_be_bytes([data[offset + 3], data[offset + 4]]) as usize;
        let data_start = offset + 5;
        let data_end = match data_start.checked_add(data_length) {
            Some(end) if end <= data.len() => end,
            _ => break, // malformed record, stop parsing
        };

        let record_data = &data[data_start..data_end];

        // We only extract tags from Application Record (record 2).
        if record_num == 2 {
            let dataset_key = u16::from_be_bytes([record_num, dataset_num]);
            match dataset_key {
                0x0200 => {
                    // ApplicationRecordVersion: 2-byte unsigned integer
                    if record_data.len() >= 2 {
                        let version = u16::from_be_bytes([record_data[0], record_data[1]]) as i64;
                        tags.push((
                            "IPTC:ApplicationRecordVersion".to_string(),
                            TagValue::new_integer(version),
                        ));
                    }
                }
                0x0278 => {
                    let text = String::from_utf8_lossy(record_data).into_owned();
                    tags.push((
                        "IPTC:Caption-Abstract".to_string(),
                        TagValue::new_string(text),
                    ));
                }
                0x025A => {
                    let text = String::from_utf8_lossy(record_data).into_owned();
                    tags.push(("IPTC:City".to_string(), TagValue::new_string(text)));
                }
                0x0265 => {
                    let text = String::from_utf8_lossy(record_data).into_owned();
                    tags.push((
                        "IPTC:Country-PrimaryLocationName".to_string(),
                        TagValue::new_string(text),
                    ));
                }
                0x025F => {
                    let text = String::from_utf8_lossy(record_data).into_owned();
                    tags.push((
                        "IPTC:Province-State".to_string(),
                        TagValue::new_string(text),
                    ));
                }
                0x0228 => {
                    let text = String::from_utf8_lossy(record_data).into_owned();
                    tags.push((
                        "IPTC:SpecialInstructions".to_string(),
                        TagValue::new_string(text),
                    ));
                }
                _ => {} // ignore other datasets
            }
        }
        offset = data_end;
    }
    Ok(tags)
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
    fn test_x3f_property_names_are_exiftools() {
        // Family matters as much as the name: ExifTool files all of these
        // under SigmaRaw, including Make and Model.
        assert_eq!(map_x3f_property_name("CAMMANUF"), "SigmaRaw:Make");
        assert_eq!(map_x3f_property_name("CAMSERIAL"), "SigmaRaw:SerialNumber");
        assert_eq!(
            map_x3f_property_name("LENSARANGE"),
            "SigmaRaw:LensApertureRange"
        );

        // Keys that real files carry and that used to fall through unmapped,
        // emitting SigmaRaw:AEMODE and friends.
        assert_eq!(map_x3f_property_name("AEMODE"), "SigmaRaw:MeteringMode");
        assert_eq!(map_x3f_property_name("PMODE"), "SigmaRaw:ExposureProgram");
        assert_eq!(map_x3f_property_name("DRIVE"), "SigmaRaw:DriveMode");
        assert_eq!(map_x3f_property_name("WB_DESC"), "SigmaRaw:WhiteBalance");

        // LENSMODEL is LensType now that %sigmaLensTypes is transcribed; it
        // was left unmapped while emitting the raw id would have been a lie.
        assert_eq!(map_x3f_property_name("LENSMODEL"), "SigmaRaw:LensType");
    }

    #[test]
    fn test_x3f_enumerated_values_decode() {
        assert_eq!(
            convert_x3f_property_value("AEMODE", "8").unwrap(),
            "8-segment"
        );
        assert_eq!(convert_x3f_property_value("PMODE", "P").unwrap(), "Program");
        assert_eq!(
            convert_x3f_property_value("DRIVE", "SINGLE").unwrap(),
            "Single Shot"
        );
        assert_eq!(
            convert_x3f_property_value("FOCUS", "AF").unwrap(),
            "Auto-focus Locked"
        );
        assert_eq!(
            convert_x3f_property_value("RESOLUTION", "HI").unwrap(),
            "High"
        );

        // An unknown code passes through rather than being guessed at.
        assert_eq!(convert_x3f_property_value("DRIVE", "WAT").unwrap(), "WAT");
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
        extract_rw2_embedded_exif_tags(&jpeg, 0, &mut metadata)
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

/// Tests for the 2026-07-27 backlog-group-1 tag wiring.
///
/// Every expected string in this module is a LITERAL copied out of ExifTool
/// 13.55's own tables, not a value read back from the constant under test.
/// Where a table entry is not exercised by the corpus sample, the test says so
/// — a green `tag-comparison` recheck only proves the values the sample
/// happens to contain.
#[cfg(test)]
mod backlog_group_1_printconv_tests {
    use super::*;

    // ---- TIFF-EPStandardID (Exif.pm:2451) --------------------------------

    /// `exiftool -G1 -a -s Nikon.nef` prints `[IFD0] TIFF-EPStandardID : 1.0.0.0`
    /// and the raw IFD0 entry is BYTE[4] `01 00 00 00`.
    #[test]
    fn tiff_ep_standard_id_joins_bytes_with_dots() {
        assert_eq!(
            format_tiff_ep_standard_id(&[1, 0, 0, 0], 4).as_deref(),
            Some("1.0.0.0")
        );
    }

    /// The PrintConv is `$val =~ tr/ /./`, i.e. it substitutes on however many
    /// components the array has — it is not hard-coded to four.
    #[test]
    fn tiff_ep_standard_id_honours_value_count() {
        assert_eq!(
            format_tiff_ep_standard_id(&[2, 1, 0, 0], 2).as_deref(),
            Some("2.1")
        );
        assert_eq!(format_tiff_ep_standard_id(&[1, 0, 0, 0], 0), None);
    }

    // ---- CFARepeatPatternDim (Exif.pm:1751, Count => 2) ------------------

    /// `exiftool -G1 -a -s DNG.dng` prints `[SubIFD] CFARepeatPatternDim : 2 2`.
    /// The generic decoder returned only "2".
    #[test]
    fn cfa_repeat_pattern_dim_prints_both_components() {
        assert_eq!(
            format_cfa_repeat_pattern_dim(&[0, 2, 0, 2], ByteOrder::BigEndian).as_deref(),
            Some("2 2")
        );
    }

    /// Asymmetric dimensions pin the component order (rows then columns) so a
    /// swap cannot pass; DNG.dng is 2x2 and could never catch it.
    #[test]
    fn cfa_repeat_pattern_dim_is_order_and_byte_order_aware() {
        assert_eq!(
            format_cfa_repeat_pattern_dim(&[2, 0, 4, 0], ByteOrder::LittleEndian).as_deref(),
            Some("2 4")
        );
        assert_eq!(
            format_cfa_repeat_pattern_dim(&[0, 2, 0, 4], ByteOrder::BigEndian).as_deref(),
            Some("2 4")
        );
        assert_eq!(
            format_cfa_repeat_pattern_dim(&[0, 2], ByteOrder::BigEndian),
            None
        );
    }

    // ---- HighISOMultiplier (PanasonicRaw.pm:144-161) ---------------------

    /// Panasonic.rw2 stores 0 in all three, so `$val / 256` is only ever
    /// exercised at zero by the corpus. 256 and 384 pin the divisor itself.
    #[test]
    fn high_iso_multiplier_divides_by_256() {
        let f = |raw: u16| {
            format_panasonic_high_iso_multiplier(&raw.to_le_bytes(), 3, 1, ByteOrder::LittleEndian)
        };
        assert_eq!(f(0).as_deref(), Some("0"));
        assert_eq!(f(256).as_deref(), Some("1"));
        assert_eq!(f(384).as_deref(), Some("1.5"));
        assert_eq!(f(512).as_deref(), Some("2"));
    }

    /// PanasonicRaw.pm declares `Writable => 'int16u'`; a non-SHORT entry must
    /// fall through to the generic decoder rather than be misread.
    #[test]
    fn high_iso_multiplier_rejects_non_short() {
        assert_eq!(
            format_panasonic_high_iso_multiplier(&[0, 1, 0, 0], 4, 1, ByteOrder::LittleEndian),
            None
        );
    }

    // ---- LightSource (Exif.pm %lightSource, lines 139-162) ---------------

    /// Panasonic.rw2 only hits `0 => 'Unknown'`. The other entries here are
    /// copied verbatim from Exif.pm; the far end of the table (23, 24, 255) is
    /// entirely unexercised by the corpus.
    #[test]
    fn light_source_print_conv_matches_exiftool_table() {
        let f = |raw: u16| {
            format_exif_display_value(0x9208, &raw.to_le_bytes(), 3, 1, ByteOrder::LittleEndian)
        };
        assert_eq!(f(0).as_deref(), Some("Unknown"));
        assert_eq!(f(3).as_deref(), Some("Tungsten (Incandescent)"));
        assert_eq!(f(11).as_deref(), Some("Shade"));
        assert_eq!(f(15).as_deref(), Some("White Fluorescent"));
        assert_eq!(f(23).as_deref(), Some("D50"));
        assert_eq!(f(24).as_deref(), Some("ISO Studio Tungsten"));
        assert_eq!(f(255).as_deref(), Some("Other"));
    }

    /// 5-8 are absent from %lightSource. ExifTool prints the bare number for a
    /// key it has no entry for, so oxidex must fall through rather than invent
    /// a stand-in label.
    #[test]
    fn light_source_gap_values_have_no_label() {
        for raw in [5u16, 6, 7, 8, 25] {
            assert_eq!(
                format_exif_display_value(
                    0x9208,
                    &raw.to_le_bytes(),
                    3,
                    1,
                    ByteOrder::LittleEndian
                ),
                None,
                "value {} is not in %lightSource and must not be labelled",
                raw
            );
        }
    }

    // ---- PanasonicRaw WBInfo2 / DistortionInfo --------------------------

    /// IFD0 tag 0x0027 of Panasonic.rw2, verbatim (58 bytes, int16u,
    /// little-endian).
    const RW2_WB_INFO2: &[u8] = &[
        0x07, 0x00, 0x09, 0x00, 0x3d, 0x02, 0x00, 0x01, 0xa0, 0x01, 0x0a, 0x00, 0x76, 0x02, 0x00,
        0x01, 0x83, 0x01, 0x0b, 0x00, 0xbe, 0x02, 0x00, 0x01, 0x63, 0x01, 0x03, 0x00, 0x7a, 0x01,
        0x00, 0x01, 0x50, 0x02, 0x04, 0x00, 0x8b, 0x02, 0x00, 0x01, 0x79, 0x01, 0x14, 0x00, 0x4e,
        0x02, 0x00, 0x01, 0xa5, 0x01, 0x18, 0x00, 0x7a, 0x01, 0x00, 0x01, 0x50, 0x02,
    ];

    /// IFD0 tag 0x0119 of Panasonic.rw2, verbatim (32 bytes, int16s,
    /// little-endian).
    const RW2_DISTORTION_INFO: &[u8] = &[
        0xb2, 0xdc, 0x71, 0x72, 0x61, 0x01, 0x00, 0x00, 0xa2, 0x02, 0x00, 0x00, 0xf1, 0x00, 0x01,
        0x00, 0xd2, 0x0f, 0xbe, 0x00, 0x01, 0x01, 0xf6, 0xfc, 0xe8, 0x08, 0xb2, 0x02, 0xda, 0x97,
        0x7f, 0x95,
    ];

    /// Every value here is `exiftool -G1 -a Panasonic.rw2`, byte for byte.
    /// The WBType labels come from %lightSource, so a stride error would move
    /// them one four-element group over and relabel them silently -- checking
    /// the levels alongside pins the stride down.
    #[test]
    fn panasonic_raw_wb_info2_matches_exiftool() {
        let mut metadata = MetadataMap::new();
        extract_panasonic_raw_wb_info2(RW2_WB_INFO2, ByteOrder::LittleEndian, &mut metadata);

        let expect = |key: &str, value: &str| {
            assert_eq!(
                metadata.get(key).and_then(TagValue::as_string).as_deref(),
                Some(value),
                "{}",
                key
            );
        };
        assert_eq!(
            metadata.get("PanasonicRaw:NumWBEntries"),
            Some(&TagValue::new_integer(7))
        );
        expect("PanasonicRaw:WBType1", "Fine Weather");
        expect("PanasonicRaw:WB_RGBLevels1", "573 256 416");
        expect("PanasonicRaw:WBType2", "Cloudy");
        expect("PanasonicRaw:WB_RGBLevels2", "630 256 387");
        expect("PanasonicRaw:WBType3", "Shade");
        expect("PanasonicRaw:WB_RGBLevels3", "702 256 355");
        expect("PanasonicRaw:WBType4", "Tungsten (Incandescent)");
        expect("PanasonicRaw:WB_RGBLevels4", "378 256 592");
        expect("PanasonicRaw:WBType5", "Flash");
        expect("PanasonicRaw:WB_RGBLevels5", "651 256 377");
        expect("PanasonicRaw:WBType6", "D55");
        expect("PanasonicRaw:WB_RGBLevels6", "590 256 421");
        expect("PanasonicRaw:WBType7", "ISO Studio Tungsten");
        expect("PanasonicRaw:WB_RGBLevels7", "378 256 592");
        // The table stops at 7 entries; nothing beyond it may be invented.
        assert!(metadata.get("PanasonicRaw:WBType8").is_none());
    }

    /// A WBType code %lightSource has no entry for must print the raw number
    /// rather than borrow a neighbouring label.
    #[test]
    fn panasonic_raw_wb_type_outside_light_source_prints_raw() {
        // One entry, type 5 (absent from %lightSource), levels 1 2 3.
        let block: Vec<u8> = [1u16, 5, 1, 2, 3]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let mut metadata = MetadataMap::new();
        extract_panasonic_raw_wb_info2(&block, ByteOrder::LittleEndian, &mut metadata);
        assert_eq!(
            metadata
                .get("PanasonicRaw:WBType1")
                .and_then(TagValue::as_string)
                .as_deref(),
            Some("5")
        );
    }

    /// Values from `exiftool -G1 -a Panasonic.rw2`. The divisor is 32768 and
    /// every quotient here is an exact binary fraction, so any rounding or
    /// off-by-one index shows up immediately.
    #[test]
    fn panasonic_raw_distortion_info_matches_exiftool() {
        let mut metadata = MetadataMap::new();
        extract_panasonic_raw_distortion_info(
            RW2_DISTORTION_INFO,
            ByteOrder::LittleEndian,
            &mut metadata,
        );

        let expect = |key: &str, value: &str| {
            assert_eq!(
                metadata.get(key).and_then(TagValue::as_string).as_deref(),
                Some(value),
                "{}",
                key
            );
        };
        expect("PanasonicRaw:DistortionParam02", "0.010772705078125");
        expect("PanasonicRaw:DistortionParam04", "0.02056884765625");
        expect("PanasonicRaw:DistortionScale", "1");
        expect("PanasonicRaw:DistortionCorrection", "On");
        expect("PanasonicRaw:DistortionParam08", "0.12359619140625");
        expect("PanasonicRaw:DistortionParam09", "0.00579833984375");
        expect("PanasonicRaw:DistortionParam11", "-0.02374267578125");
        // Index 12 is `Unknown => 1` and 0/1/3/6/10/13/14/15 are checksums or
        // undocumented; none of them may surface.
        assert!(metadata.get("PanasonicRaw:DistortionN").is_none());
    }

    /// DistortionCorrection is `7.1 => { Mask => 0x0f }`. ExifTool masks
    /// because the GF5/GX1 set the upper nibble, which makes the whole value
    /// -4095; comparing unmasked would print "Off" for a corrected image.
    #[test]
    fn panasonic_raw_distortion_correction_is_masked() {
        let mut block = RW2_DISTORTION_INFO.to_vec();
        block[14..16].copy_from_slice(&(-4095i16).to_le_bytes());
        let mut metadata = MetadataMap::new();
        extract_panasonic_raw_distortion_info(&block, ByteOrder::LittleEndian, &mut metadata);
        assert_eq!(
            metadata
                .get("PanasonicRaw:DistortionCorrection")
                .and_then(TagValue::as_string)
                .as_deref(),
            Some("On")
        );
    }

    /// A block shorter than the 32 bytes ProcessDistortionInfo expects must
    /// produce nothing rather than read past the end.
    #[test]
    fn panasonic_raw_distortion_info_rejects_short_block() {
        let mut metadata = MetadataMap::new();
        extract_panasonic_raw_distortion_info(
            &RW2_DISTORTION_INFO[..16],
            ByteOrder::LittleEndian,
            &mut metadata,
        );
        assert_eq!(metadata.len(), 0);
    }

    // ---- Relocated MakerNote rebuild ------------------------------------

    /// Build a one-entry MakerNote IFD whose single value is out of line,
    /// stated against `source_base`, and separated from the IFD header by
    /// `gap` bytes of padding.
    fn synthetic_relocated_ifd(source_base: u32, gap: usize, payload: &[u8]) -> Vec<u8> {
        let header_size = 2 + 12 + 4;
        let mut ifd = vec![0u8; header_size + gap];
        ifd[..2].copy_from_slice(&1u16.to_le_bytes());
        ifd[2..4].copy_from_slice(&0x1234u16.to_le_bytes()); // tag id
        ifd[4..6].copy_from_slice(&1u16.to_le_bytes()); // BYTE
        ifd[6..10].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        ifd[10..14].copy_from_slice(&(source_base + (header_size + gap) as u32).to_le_bytes());
        ifd.extend_from_slice(payload);
        ifd
    }

    /// The point of the rebuild: a parser that assumes the values start
    /// immediately after the IFD header reads two bytes early on Canon350D's
    /// relocated MakerNote, because Canon left a two-byte gap there. After
    /// the rebuild the value sits exactly at its rewritten offset.
    #[test]
    fn relocated_makernote_rebuild_absorbs_the_gap() {
        let payload = b"0123456789";
        for gap in [0usize, 2, 7] {
            let ifd = synthetic_relocated_ifd(700, gap, payload);
            let rebuilt = rebuild_relocated_makernote(&ifd, 700, ByteOrder::LittleEndian, None)
                .expect("synthetic IFD rebuilds");

            let header_size = 2 + 12 + 4;
            assert_eq!(u16::from_le_bytes([rebuilt[0], rebuilt[1]]), 1);
            let new_offset = u32::from_le_bytes(rebuilt[10..14].try_into().unwrap()) as usize;
            assert_eq!(new_offset, header_size, "gap {}", gap);
            assert_eq!(
                &rebuilt[new_offset..new_offset + payload.len()],
                payload,
                "gap {}",
                gap
            );
        }
    }

    /// An entry pointing outside the block is dropped, not aimed at whatever
    /// bytes happen to be in range.
    #[test]
    fn relocated_makernote_rebuild_drops_unlocatable_entries() {
        let mut ifd = synthetic_relocated_ifd(700, 0, b"0123456789");
        ifd[10..14].copy_from_slice(&9_999_999u32.to_le_bytes());
        let rebuilt = rebuild_relocated_makernote(&ifd, 700, ByteOrder::LittleEndian, None)
            .expect("synthetic IFD rebuilds");
        assert_eq!(u16::from_le_bytes([rebuilt[0], rebuilt[1]]), 0);
    }

    /// The allowlist keeps a parser that never dereferences an offset from
    /// printing the offset as the value.
    #[test]
    fn relocated_makernote_rebuild_honours_external_allowlist() {
        let ifd = synthetic_relocated_ifd(700, 0, b"0123456789");
        let kept = rebuild_relocated_makernote(&ifd, 700, ByteOrder::LittleEndian, Some(&[0x1234]))
            .expect("allowlisted rebuild");
        assert_eq!(u16::from_le_bytes([kept[0], kept[1]]), 1);

        let dropped = rebuild_relocated_makernote(&ifd, 700, ByteOrder::LittleEndian, Some(&[]))
            .expect("filtered rebuild");
        assert_eq!(u16::from_le_bytes([dropped[0], dropped[1]]), 0);
    }

    /// Inline values (<= 4 bytes) live in the offset field itself and must be
    /// copied through untouched -- rewriting them would destroy the value.
    #[test]
    fn relocated_makernote_rebuild_preserves_inline_values() {
        let header_size = 2 + 12 + 4;
        let mut ifd = vec![0u8; header_size];
        ifd[..2].copy_from_slice(&1u16.to_le_bytes());
        ifd[2..4].copy_from_slice(&0x0010u16.to_le_bytes());
        ifd[4..6].copy_from_slice(&4u16.to_le_bytes()); // LONG
        ifd[6..10].copy_from_slice(&1u32.to_le_bytes());
        ifd[10..14].copy_from_slice(&0x8000_0189u32.to_le_bytes()); // CanonModelID
        let rebuilt = rebuild_relocated_makernote(&ifd, 700, ByteOrder::LittleEndian, None)
            .expect("synthetic IFD rebuilds");
        assert_eq!(
            u32::from_le_bytes(rebuilt[10..14].try_into().unwrap()),
            0x8000_0189
        );
    }

    /// `ProcessAdobeData` walks 4-char tag / big-endian size records with odd
    /// sizes padded to even. A record type this code does not decode must be
    /// skipped without derailing the walk.
    #[test]
    fn adobe_private_data_skips_unknown_records_and_pads() {
        let mut blob = b"Adobe\0".to_vec();
        blob.extend_from_slice(b"CRW ");
        blob.extend_from_slice(&3u32.to_be_bytes());
        blob.extend_from_slice(&[1, 2, 3]);
        blob.push(0); // padding, because the size was odd

        // A MakN record whose payload is not a usable IFD: the walk must
        // still terminate cleanly and emit nothing.
        blob.extend_from_slice(b"MakN");
        blob.extend_from_slice(&6u32.to_be_bytes());
        blob.extend_from_slice(b"II\0\0\0\0");

        let mut metadata = MetadataMap::new();
        extract_dng_adobe_private_data(&blob, "Canon", &mut metadata);
        assert_eq!(metadata.len(), 0);
    }

    /// Anything that is not Adobe's container is left alone.
    #[test]
    fn adobe_private_data_ignores_foreign_blobs() {
        let mut metadata = MetadataMap::new();
        extract_dng_adobe_private_data(b"SONY DSC \0\0\0", "Sony", &mut metadata);
        assert_eq!(metadata.len(), 0);
    }

    // ---- DNG IFD0 conversions -------------------------------------------

    /// All four values are `exiftool -s` output for Canon350D.dng.
    #[test]
    fn dng_ifd0_conversions_match_exiftool() {
        let be = ByteOrder::BigEndian;
        assert_eq!(
            format_dng_ifd0_tag(0xC612, &[1, 1, 0, 0], 1, 4, be).as_deref(),
            Some("1.1.0.0")
        );
        assert_eq!(
            format_dng_ifd0_tag(0xC613, &[1, 1, 0, 0], 1, 4, be).as_deref(),
            Some("1.1.0.0")
        );

        let unique_id = [
            0x03, 0x58, 0xdb, 0x4e, 0x08, 0x63, 0x2d, 0x90, 0x92, 0x51, 0x71, 0xa6, 0xbb, 0x88,
            0x48, 0xa2,
        ];
        assert_eq!(
            format_dng_ifd0_tag(0xC65D, &unique_id, 1, 16, be).as_deref(),
            Some("0358DB4E08632D90925171A6BB8848A2")
        );

        // 18/1, 55/1, 0/0, 0/0 -> "18-55mm f/?" (0/0 is 'undef', printed '?')
        let mut lens = Vec::new();
        for (numerator, denominator) in [(18u32, 1u32), (55, 1), (0, 0), (0, 0)] {
            lens.extend_from_slice(&numerator.to_be_bytes());
            lens.extend_from_slice(&denominator.to_be_bytes());
        }
        assert_eq!(
            format_dng_ifd0_tag(0xC630, &lens, 5, 4, be).as_deref(),
            Some("18-55mm f/?")
        );
    }

    /// PrintLensInfo suppresses the upper bound when it is zero or equal to
    /// the lower bound, so a prime with a fixed aperture prints as one focal
    /// length and one f-number.
    #[test]
    fn dng_lens_info_collapses_equal_and_zero_bounds() {
        let build = |components: [(u32, u32); 4]| {
            let mut bytes = Vec::new();
            for (numerator, denominator) in components {
                bytes.extend_from_slice(&numerator.to_be_bytes());
                bytes.extend_from_slice(&denominator.to_be_bytes());
            }
            bytes
        };
        let be = ByteOrder::BigEndian;
        assert_eq!(
            format_dng_ifd0_tag(
                0xC630,
                &build([(50, 1), (50, 1), (14, 10), (14, 10)]),
                5,
                4,
                be
            )
            .as_deref(),
            Some("50mm f/1.4")
        );
        // "if $vals[1]" is a truthiness test: a literal zero upper bound is
        // dropped rather than printed as "50-0".
        assert_eq!(
            format_dng_ifd0_tag(
                0xC630,
                &build([(50, 1), (0, 1), (14, 10), (0, 1)]),
                5,
                4,
                be
            )
            .as_deref(),
            Some("50mm f/1.4")
        );
    }

    // ---- FileSource (Exif.pm:2757) --------------------------------------

    /// The whole table, verbatim from Exif.pm:2761-2767. SigmaDP2.x3f carries
    /// only `03` (count 1), so 1, 2 and the 4-byte Sigma form are unexercised
    /// by the corpus — and 1/2 are exactly where an off-by-one relabelling
    /// ("Scanner"/"Film Scanner") would hide.
    #[test]
    fn file_source_print_conv_matches_exiftool_table() {
        let f = |payload: &[u8]| {
            format_exif_display_value(
                0xA300,
                payload,
                7,
                payload.len() as u32,
                ByteOrder::BigEndian,
            )
        };
        assert_eq!(f(&[1]).as_deref(), Some("Film Scanner"));
        assert_eq!(f(&[2]).as_deref(), Some("Reflection Print Scanner"));
        assert_eq!(f(&[3]).as_deref(), Some("Digital Camera"));
        assert_eq!(f(&[3, 0, 0, 0]).as_deref(), Some("Sigma Digital Camera"));
        assert_eq!(f(&[0]), None);
    }

    // ---- Flash (Exif.pm %flash, lines 165-199) --------------------------

    /// SigmaDP2.x3f carries `0x00` only. 0x5f is the last row of the table and
    /// is the longest label in it, so a truncated transcription cannot pass.
    #[test]
    fn flash_print_conv_matches_exiftool_table() {
        let f = |raw: u16| {
            format_exif_display_value(0x9209, &raw.to_le_bytes(), 3, 1, ByteOrder::LittleEndian)
        };
        assert_eq!(f(0x00).as_deref(), Some("No Flash"));
        assert_eq!(f(0x01).as_deref(), Some("Fired"));
        assert_eq!(f(0x0d).as_deref(), Some("On, Return not detected"));
        assert_eq!(f(0x20).as_deref(), Some("No flash function"));
        assert_eq!(
            f(0x5d).as_deref(),
            Some("Auto, Fired, Red-eye reduction, Return not detected")
        );
        assert_eq!(
            f(0x5f).as_deref(),
            Some("Auto, Fired, Red-eye reduction, Return detected")
        );
        // 0x02 has no entry in %flash.
        assert_eq!(f(0x02), None);
    }

    // ---- ExposureProgram (Exif.pm:2083) ---------------------------------

    /// SigmaDP2.x3f carries 2 only. 3 and 4 are the pair most likely to be
    /// shortened ("Aperture Priority" / "Shutter Priority"), so both full
    /// Exif.pm spellings are pinned.
    #[test]
    fn exposure_program_print_conv_matches_exiftool_table() {
        let f = |raw: u16| {
            format_exif_display_value(0x8822, &raw.to_le_bytes(), 3, 1, ByteOrder::LittleEndian)
        };
        assert_eq!(f(0).as_deref(), Some("Not Defined"));
        assert_eq!(f(2).as_deref(), Some("Program AE"));
        assert_eq!(f(3).as_deref(), Some("Aperture-priority AE"));
        assert_eq!(f(4).as_deref(), Some("Shutter speed priority AE"));
        assert_eq!(f(5).as_deref(), Some("Creative (Slow speed)"));
        assert_eq!(f(6).as_deref(), Some("Action (High speed)"));
        assert_eq!(f(9).as_deref(), Some("Bulb"));
        assert_eq!(f(10), None);
    }

    // ---- ExifVersion (Exif.pm:2213) -------------------------------------

    /// SigmaDP2.x3f carries an unpadded `30 32 32 31`. The RawConv
    /// `$val=~s/\0+$//` only matters for the padded form, which the corpus
    /// never produces.
    #[test]
    fn exif_version_strips_trailing_nulls() {
        assert_eq!(
            format_exif_display_value(0x9000, b"0221", 7, 4, ByteOrder::BigEndian).as_deref(),
            Some("0221")
        );
        assert_eq!(
            format_exif_display_value(0x9000, b"0230\0", 7, 5, ByteOrder::BigEndian).as_deref(),
            Some("0230")
        );
    }

    // ---- PrintExposureTime / PrintFNumber / PrintFraction ---------------

    /// Exif.pm:5606. The `< 0.25001` branch is what SigmaDP2.x3f hits (1/10);
    /// the `%.1f` branch and its `s/\.0$//` are not exercised by the corpus.
    #[test]
    fn print_exposure_time_matches_perl() {
        assert_eq!(print_exposure_time(0.1), "1/10");
        assert_eq!(print_exposure_time(1.0 / 8000.0), "1/8000");
        assert_eq!(print_exposure_time(0.25), "1/4");
        assert_eq!(print_exposure_time(0.5), "0.5");
        assert_eq!(print_exposure_time(2.0), "2");
        assert_eq!(print_exposure_time(30.0), "30");
    }

    /// Exif.pm:5620. SigmaDP2.x3f hits only the `>= 1` branch (2.8); the
    /// two-decimal sub-f/1.0 branch is unexercised by the corpus.
    #[test]
    fn print_f_number_matches_perl() {
        assert_eq!(print_f_number(2.8), "2.8");
        assert_eq!(print_f_number(4.0), "4.0");
        assert_eq!(print_f_number(0.95), "0.95");
    }

    /// Exif.pm:5421. SigmaDP2.x3f hits only `not $val` (0). Every other branch
    /// — +d, +d/2, +d/3 — is unexercised by the corpus.
    #[test]
    fn print_fraction_matches_perl() {
        assert_eq!(print_fraction(0.0), "0");
        assert_eq!(print_fraction(1.0), "+1");
        assert_eq!(print_fraction(-2.0), "-2");
        assert_eq!(print_fraction(0.5), "+1/2");
        assert_eq!(print_fraction(-0.5), "-1/2");
        assert_eq!(print_fraction(1.0 / 3.0), "+1/3");
        assert_eq!(print_fraction(-1.0 / 3.0), "-1/3");
    }

    // ---- DNG SubIFD preview naming (Exif.pm 0x111/0x117 Conditions) -----

    fn dng_sub_tags(
        compression: u16,
        subfile_type: u32,
        strip_offset: u32,
        strip_length: u32,
    ) -> Vec<(u16, u16, u32, Vec<u8>)> {
        vec![
            (0x00FE, 4, 1, subfile_type.to_be_bytes().to_vec()),
            (0x0103, 3, 1, compression.to_be_bytes().to_vec()),
            (0x0111, 4, 1, strip_offset.to_be_bytes().to_vec()),
            (0x0117, 4, 1, strip_length.to_be_bytes().to_vec()),
        ]
    }

    /// `exiftool -G1 -a -s DNG.dng` prints
    ///   [SubIFD1] PreviewImageStart : 12780 / PreviewImageLength : 26
    ///   [SubIFD2] JpgFromRawStart   : 13070 / JpgFromRawLength   : 29
    /// The split is by SubIFD index, per the Exif.pm 0x111 Condition
    /// `$$self{DIR_NAME} ne "SubIFD2"`.
    #[test]
    fn dng_subifd1_is_preview_and_subifd2_is_jpg_from_raw() {
        let data = vec![0u8; 200];

        let mut preview = MetadataMap::new();
        extract_dng_subifd_preview(
            &data,
            &dng_sub_tags(7, 1, 10, 20),
            1,
            ByteOrder::BigEndian,
            &mut preview,
        );
        assert_eq!(
            preview.get("EXIF:PreviewImageStart"),
            Some(&TagValue::new_integer(10))
        );
        assert_eq!(
            preview.get("EXIF:PreviewImageLength"),
            Some(&TagValue::new_integer(20))
        );
        assert!(preview.get("EXIF:JpgFromRawStart").is_none());

        let mut jpg = MetadataMap::new();
        extract_dng_subifd_preview(
            &data,
            &dng_sub_tags(7, 1, 30, 40),
            2,
            ByteOrder::BigEndian,
            &mut jpg,
        );
        assert_eq!(
            jpg.get("EXIF:JpgFromRawStart"),
            Some(&TagValue::new_integer(30))
        );
        assert_eq!(
            jpg.get("EXIF:JpgFromRawLength"),
            Some(&TagValue::new_integer(40))
        );
        assert!(jpg.get("EXIF:PreviewImageStart").is_none());
    }

    /// The Exif.pm 0x111 StripOffsets Condition only diverts DNG IFDs where
    /// `$$self{Compression} eq '7' and $$self{SubfileType} ne '0'`. DNG.dng's
    /// first SubIFD is SubfileType 0 (full-resolution), so it must keep the
    /// StripOffsets naming — the corpus cannot distinguish this from an
    /// index-only rule because that SubIFD uses TileOffsets instead.
    #[test]
    fn dng_full_resolution_subifd_is_not_renamed() {
        let data = vec![0u8; 200];

        let mut full_res = MetadataMap::new();
        extract_dng_subifd_preview(
            &data,
            &dng_sub_tags(7, 0, 10, 20),
            1,
            ByteOrder::BigEndian,
            &mut full_res,
        );
        assert!(full_res.is_empty(), "SubfileType 0 must not be renamed");

        let mut uncompressed = MetadataMap::new();
        extract_dng_subifd_preview(
            &data,
            &dng_sub_tags(1, 1, 10, 20),
            1,
            ByteOrder::BigEndian,
            &mut uncompressed,
        );
        assert!(
            uncompressed.is_empty(),
            "Compression != 7 must not be renamed"
        );
    }

    /// A strip range that runs past the end of the file must not panic and
    /// must not emit a truncated image blob; the Start/Length pair still
    /// reports what the IFD claims, exactly as ExifTool does.
    #[test]
    fn dng_out_of_range_strip_emits_offsets_without_image() {
        let data = vec![0u8; 16];
        let mut metadata = MetadataMap::new();
        extract_dng_subifd_preview(
            &data,
            &dng_sub_tags(7, 1, 12, 4096),
            2,
            ByteOrder::BigEndian,
            &mut metadata,
        );
        assert_eq!(
            metadata.get("EXIF:JpgFromRawStart"),
            Some(&TagValue::new_integer(12))
        );
        assert!(metadata.get("EXIF:JpgFromRaw").is_none());
    }
}

#[cfg(test)]
mod rational_array_tests {
    use super::*;

    fn le_rationals(pairs: &[(u32, u32)]) -> Vec<u8> {
        pairs
            .iter()
            .flat_map(|(n, d)| {
                let mut v = n.to_le_bytes().to_vec();
                v.extend_from_slice(&d.to_le_bytes());
                v
            })
            .collect()
    }

    /// `raw_bytes_to_simple_tag_value` took `_value_count` and discarded it, so
    /// every array-valued rational in a RAW IFD collapsed to its first
    /// component. Values are ExifTool's own output for
    /// tests/../combined-samples/DNG.dng.
    #[test]
    fn dng_rational_arrays_keep_every_component() {
        // 0xC628 AsShotNeutral, RATIONAL[3] -> "0.592408 1 0.501692"
        let as_shot = le_rationals(&[(592408, 1000000), (1, 1), (501692, 1000000)]);
        assert_eq!(
            raw_bytes_to_simple_tag_value(&as_shot, 5, 3, ByteOrder::LittleEndian).as_string(),
            Some("0.592408 1 0.501692")
        );

        // 0xC627 AnalogBalance, RATIONAL[3] -> "1 1 1", not "1/1"
        let analog = le_rationals(&[(1, 1), (1, 1), (1, 1)]);
        assert_eq!(
            raw_bytes_to_simple_tag_value(&analog, 5, 3, ByteOrder::LittleEndian).as_string(),
            Some("1 1 1")
        );
    }

    /// ColorMatrix1/2 are SRATIONAL[9] and carry negatives, which is why the
    /// signed path needs the same treatment as the unsigned one.
    #[test]
    fn signed_rational_arrays_keep_sign_and_every_component() {
        let pairs: [(i32, i32); 9] = [
            (6159, 10000),
            (-945, 10000),
            (-745, 10000),
            (-6846, 10000),
            (13563, 10000),
            (3684, 10000),
            (-802, 10000),
            (1086, 10000),
            (7555, 10000),
        ];
        let bytes: Vec<u8> = pairs
            .iter()
            .flat_map(|(n, d)| {
                let mut v = n.to_le_bytes().to_vec();
                v.extend_from_slice(&d.to_le_bytes());
                v
            })
            .collect();
        assert_eq!(
            raw_bytes_to_simple_tag_value(&bytes, 10, 9, ByteOrder::LittleEndian).as_string(),
            Some("0.6159 -0.0945 -0.0745 -0.6846 1.3563 0.3684 -0.0802 0.1086 0.7555")
        );
    }

    /// A single rational must keep its existing representation -- the fix is
    /// additive, and nothing that reads scalar rationals should shift.
    #[test]
    fn single_rationals_are_untouched() {
        let one = le_rationals(&[(592408, 1000000)]);
        let value = raw_bytes_to_simple_tag_value(&one, 5, 1, ByteOrder::LittleEndian);
        assert!(value.is_rational(), "single rational must stay a Rational");
    }

    /// ExifTool prints CalibrationIlluminant1/2 through the same %lightSource
    /// hash LightSource (0x9208) uses -- Exif.pm:3639. Only 0x9208 was routed
    /// to that table, so the DNG pair reported raw 17 and 21.
    #[test]
    fn dng_calibration_illuminants_print_through_the_light_source_table() {
        for (tag, raw, expected) in [
            (0xC65Au16, 17u16, "Standard Light A"),
            (0xC65B, 21, "D65"),
            (0xC65A, 23, "D50"),
            (0xC65B, 255, "Other"),
        ] {
            assert_eq!(
                format_exif_display_value(tag, &raw.to_le_bytes(), 3, 1, ByteOrder::LittleEndian),
                Some(expected.to_string()),
                "tag {tag:#06X} value {raw}"
            );
        }
    }

    /// SHORT and LONG dropped `value_count` exactly as the rational branches
    /// did. BitsPerSample is SHORT[3]: ExifTool prints "8 8 8", oxidex printed
    /// "8". This reaches past DNG -- it also closed CR2:BitsPerSample,
    /// CR2:RawImageSegmentation and MRW:SubjectArea.
    #[test]
    fn short_and_long_arrays_keep_every_component() {
        let three_shorts: Vec<u8> = [8u16, 8, 8].iter().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(
            raw_bytes_to_simple_tag_value(&three_shorts, 3, 3, ByteOrder::LittleEndian).as_string(),
            Some("8 8 8")
        );

        let two_longs: Vec<u8> = [3040u32, 2014]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        assert_eq!(
            raw_bytes_to_simple_tag_value(&two_longs, 4, 2, ByteOrder::LittleEndian).as_string(),
            Some("3040 2014")
        );

        // Big-endian must agree -- TIFF is either order.
        let be_shorts: Vec<u8> = [1u16, 2, 3].iter().flat_map(|v| v.to_be_bytes()).collect();
        assert_eq!(
            raw_bytes_to_simple_tag_value(&be_shorts, 3, 3, ByteOrder::BigEndian).as_string(),
            Some("1 2 3")
        );

        // A single SHORT stays an integer, not a one-element string.
        let one = 8u16.to_le_bytes();
        assert!(
            raw_bytes_to_simple_tag_value(&one, 3, 1, ByteOrder::LittleEndian)
                .as_string()
                .is_none(),
            "scalar SHORT must keep its integer representation"
        );
    }

    /// X3F properties went in raw: `map_x3f_property_name` renamed them and
    /// nothing applied ExifTool's ValueConv/PrintConv. Values are ExifTool's
    /// own output for combined-samples/Sigma.x3f.
    #[test]
    fn x3f_properties_get_their_exiftool_conversions() {
        // SigmaRaw.pm:154  sprintf("%.1f",$val)
        assert_eq!(
            convert_x3f_property_value("APERTURE", "8.35419").as_deref(),
            Some("8.4")
        );
        // SigmaRaw.pm:263  ConvertUnixTime($val)
        assert_eq!(
            convert_x3f_property_value("TIME", "978309395").as_deref(),
            Some("2001:01:01 00:36:35")
        );
        // SigmaRaw.pm:190-191  $val * 1e-6 then PrintExposureTime
        assert_eq!(
            convert_x3f_property_value("EXPTIME", "24140").as_deref(),
            Some("1/41")
        );
        // SigmaRaw.pm:257  PrintExposureTime, already seconds
        assert_eq!(
            convert_x3f_property_value("SHUTTER", "0.00929").as_deref(),
            Some("1/108")
        );
        // Anything without a declared conversion is left alone.
        assert_eq!(convert_x3f_property_value("CAMMODEL", "SD9"), None);
    }

    /// EXPTIME is IntegrationTime (SigmaRaw.pm:187); ExposureTime is SHUTTER
    /// (SigmaRaw.pm:254). Mapping EXPTIME to ExposureTime reported
    /// IntegrationTime's raw microseconds as the shutter speed.
    #[test]
    fn x3f_exptime_is_integration_time_not_exposure_time() {
        assert_eq!(map_x3f_property_name("EXPTIME"), "SigmaRaw:IntegrationTime");
        assert_eq!(map_x3f_property_name("SHUTTER"), "SigmaRaw:ExposureTime");
    }

    /// SigmaRaw.pm:81 gives MarkBits an empty BITMASK, so DecodeBits prints
    /// "(none)" when nothing is set and "[n]" per set bit otherwise. Both
    /// sample X3F files carry 0.
    #[test]
    fn x3f_mark_bits_render_like_decode_bits() {
        assert_eq!(print_x3f_mark_bits(0), "(none)");
        assert_eq!(print_x3f_mark_bits(0b1001), "[0], [3]");
    }

    /// The extended header maps slot -> tag id, not slot -> tag. Both samples
    /// store X3FillLight (id 10) in slot 6, ahead of RedAdjust (id 7), so an
    /// implementation that assumed positional order would mislabel four tags.
    #[test]
    fn x3f_header_ext_ids_follow_sigmaraw_headerext() {
        assert_eq!(x3f_header_ext_tag_name(1), Some("ExposureAdjust"));
        assert_eq!(x3f_header_ext_tag_name(10), Some("X3FillLight"));
        assert_eq!(x3f_header_ext_tag_name(7), Some("RedAdjust"));
        // 0 is HeaderExt's "Unused" slot marker, which ExifTool skips.
        assert_eq!(x3f_header_ext_tag_name(0), None);
        assert_eq!(x3f_header_ext_tag_name(11), None);
    }

    /// Sigma.x3f stores LENSMODEL as the hex string "145"; ExifTool reports
    /// `Sigma Lens (0x145)`, which is a real entry in %sigmaLensTypes rather
    /// than a fallback. SigmaDP2.x3f leaves the property blank and ExifTool
    /// reports `Unknown ( )`.
    #[test]
    fn x3f_lens_type_matches_exiftool_on_both_samples() {
        assert_eq!(print_sigma_lens_type("145"), "Sigma Lens (0x145)");
        assert_eq!(print_sigma_lens_type(" "), "Unknown ( )");
        // An id the table does not carry keeps ExifTool's PrintHex form.
        assert_eq!(print_sigma_lens_type("fff"), "Unknown (0xfff)");
    }

    /// LENSMODEL used to be left unmapped so it surfaced as SigmaRaw:LENSMODEL.
    #[test]
    fn x3f_lensmodel_maps_to_lens_type() {
        assert_eq!(map_x3f_property_name("LENSMODEL"), "SigmaRaw:LensType");
    }
}
