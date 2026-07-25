//! GIF image format parser
//!
//! Implements basic metadata extraction from GIF (Graphics Interchange Format) files.

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// GIF signature: "GIF87a" or "GIF89a"
const GIF87A_SIGNATURE: &[u8] = b"GIF87a";
const GIF89A_SIGNATURE: &[u8] = b"GIF89a";

/// GIF parser for extracting metadata from GIF images
pub struct GIFParser;

impl GIFParser {
    /// Verifies GIF signature
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 6 {
            return Ok(false);
        }
        let header = reader.read(0, 6)?;
        Ok(header == GIF87A_SIGNATURE || header == GIF89A_SIGNATURE)
    }

    /// Reads GIF version (87a or 89a)
    pub fn read_version(reader: &dyn FileReader) -> Result<&'static str> {
        if reader.size() < 6 {
            return Ok("Unknown");
        }
        let header = reader.read(0, 6)?;
        if header == GIF87A_SIGNATURE {
            Ok("87a")
        } else if header == GIF89A_SIGNATURE {
            Ok("89a")
        } else {
            Ok("Unknown")
        }
    }

    /// Reads image dimensions from GIF header (offset 6, 4 bytes: width, height in little-endian)
    pub fn read_dimensions(reader: &dyn FileReader) -> Result<(u16, u16)> {
        if reader.size() < 10 {
            return Ok((0, 0));
        }
        let dims = reader.read(6, 4)?;
        // GIF uses little-endian byte order
        let endian_reader = EndianReader::little_endian(dims);
        let width = endian_reader.u16_at(0).unwrap_or(0);
        let height = endian_reader.u16_at(2).unwrap_or(0);
        Ok((width, height))
    }

    /// Reads the logical screen descriptor (7 bytes at offset 6)
    fn read_logical_screen_descriptor(reader: &dyn FileReader) -> Result<LogicalScreenDescriptor> {
        if reader.size() < 13 {
            return Ok(LogicalScreenDescriptor::default());
        }
        let lsd = reader.read(6, 7)?;
        // GIF uses little-endian byte order
        let endian_reader = EndianReader::little_endian(lsd);

        let width = endian_reader.u16_at(0).unwrap_or(0);
        let height = endian_reader.u16_at(2).unwrap_or(0);

        let packed = lsd[4];
        let global_color_table_flag = (packed & 0b10000000) != 0;
        let color_resolution = ((packed & 0b01110000) >> 4) + 1; // bits per primary color
        let _sort_flag = (packed & 0b00001000) != 0;
        let global_color_table_size = if global_color_table_flag {
            2u32.pow(((packed & 0b00000111) + 1) as u32)
        } else {
            0
        };

        let background_color_index = lsd[5];
        let pixel_aspect_ratio = lsd[6];

        Ok(LogicalScreenDescriptor {
            width,
            height,
            global_color_table_flag,
            color_resolution,
            global_color_table_size,
            background_color_index,
            pixel_aspect_ratio,
        })
    }

    /// Scans the file for extensions and image blocks
    fn scan_blocks(reader: &dyn FileReader) -> Result<BlockScanResult> {
        let lsd = Self::read_logical_screen_descriptor(reader)?;

        // Calculate start position after header and global color table
        let mut pos = 13u64; // 6 byte header + 7 byte logical screen descriptor
        if lsd.global_color_table_flag {
            pos += (lsd.global_color_table_size as u64) * 3;
        }

        let mut frame_count = 0;
        let mut is_animated = false;
        let mut comment = String::new();
        let mut first_gce: Option<GraphicControlExtension> = None;
        let mut has_transparency = false;
        let mut transparent_color: Option<u8> = None;
        let mut icc_profile: Option<Vec<u8>> = None;
        let mut xmp_data: Option<Vec<u8>> = None;

        while pos < reader.size() {
            let byte = match reader.read(pos, 1) {
                Ok(b) => b[0],
                Err(_) => break,
            };

            match byte {
                0x21 => {
                    // Extension introducer
                    if pos + 1 >= reader.size() {
                        break;
                    }
                    let label = reader.read(pos + 1, 1)?[0];
                    pos += 2;

                    match label {
                        0xFF => {
                            // Application extension
                            if let Ok(block_size) = reader.read(pos, 1) {
                                let size = block_size[0] as u64;
                                pos += 1;

                                if size >= 11
                                    && pos + 11 <= reader.size()
                                    && let Ok(app_data) = reader.read(pos, 11)
                                {
                                    // Check for NETSCAPE2.0 animation extension
                                    if &app_data[0..8] == b"NETSCAPE" {
                                        is_animated = true;
                                    }
                                    // Check for ICC profile extension: ICCRGBG1012
                                    // Application identifier: "ICCRGBG1" (8 bytes)
                                    // Authentication code: "012" (3 bytes)
                                    else if &app_data[0..8] == b"ICCRGBG1"
                                        && &app_data[8..11] == b"012"
                                    {
                                        // Collect ICC profile data from sub-blocks
                                        pos += size;
                                        let (new_pos, profile_data) =
                                            Self::read_sub_blocks(reader, pos)?;
                                        if !profile_data.is_empty() {
                                            icc_profile = Some(profile_data);
                                        }
                                        pos = new_pos;
                                        continue; // Skip the normal sub-block skip
                                    }
                                    // Check for XMP extension: "XMP DataXMP"
                                    // Application identifier: "XMP Data" (8 bytes)
                                    // Authentication code: "XMP" (3 bytes)
                                    else if &app_data[0..8] == b"XMP Data"
                                        && &app_data[8..11] == b"XMP"
                                    {
                                        // GIF XMP is NOT stored in sub-blocks - it's stored as raw data
                                        // followed by a 258-byte "magic trailer" (landing zone)
                                        pos += size;

                                        // Read raw XMP data until we hit the end marker
                                        let (new_pos, raw_xmp) = Self::read_xmp_data(reader, pos)?;
                                        if !raw_xmp.is_empty() {
                                            xmp_data = Some(raw_xmp);
                                        }
                                        pos = new_pos;
                                        continue; // Skip the normal sub-block skip
                                    }
                                }
                                pos += size;
                            }
                            pos = Self::skip_sub_blocks(reader, pos)?;
                        }
                        0xFE => {
                            // Comment extension
                            pos = Self::read_comment_blocks(reader, pos, &mut comment)?;
                        }
                        0xF9 => {
                            // Graphic control extension
                            if let Ok((new_pos, gce)) =
                                Self::parse_graphic_control_extension(reader, pos)
                            {
                                pos = new_pos;

                                // Store first GCE for metadata
                                if first_gce.is_none() {
                                    if gce.transparent_color_flag {
                                        has_transparency = true;
                                        transparent_color = Some(gce.transparent_color_index);
                                    }
                                    first_gce = Some(gce);
                                }
                            } else {
                                pos = Self::skip_sub_blocks(reader, pos)?;
                            }
                        }
                        _ => {
                            pos = Self::skip_sub_blocks(reader, pos)?;
                        }
                    }
                }
                0x2C => {
                    // Image descriptor
                    frame_count += 1;
                    pos += 1;

                    if pos + 9 <= reader.size() {
                        let img_desc = reader.read(pos, 9)?;
                        pos += 9;

                        let packed = img_desc[8];
                        let has_local_color_table = (packed & 0b10000000) != 0;

                        if has_local_color_table {
                            let local_color_table_size =
                                2u32.pow(((packed & 0b00000111) + 1) as u32);
                            pos += (local_color_table_size as u64) * 3;
                        }

                        // Skip LZW minimum code size
                        if pos < reader.size() {
                            pos += 1;
                        }

                        // Skip image data sub-blocks
                        pos = Self::skip_sub_blocks(reader, pos)?;
                    }
                }
                0x3B => {
                    // Trailer - end of GIF
                    break;
                }
                _ => {
                    pos += 1;
                }
            }
        }

        Ok(BlockScanResult {
            frame_count,
            is_animated,
            comment: if comment.is_empty() {
                None
            } else {
                Some(comment)
            },
            delay_time: first_gce.as_ref().map(|gce| gce.delay_time),
            disposal_method: first_gce.as_ref().map(|gce| gce.disposal_method),
            has_transparency,
            transparent_color,
            icc_profile,
            xmp_data,
        })
    }

    /// Parses a Graphic Control Extension block
    fn parse_graphic_control_extension(
        reader: &dyn FileReader,
        pos: u64,
    ) -> Result<(u64, GraphicControlExtension)> {
        // Read block size (should be 4)
        let block_size = reader.read(pos, 1)?[0];
        if block_size != 4 {
            return Err(ExifToolError::parse_error(
                "Invalid Graphic Control Extension block size",
            ));
        }

        // Read the 4-byte block data
        let data = reader.read(pos + 1, 4)?;
        // GIF uses little-endian byte order
        let gce_reader = EndianReader::little_endian(data);

        let packed = data[0];
        let disposal_method = (packed & 0b00011100) >> 2;
        let user_input_flag = (packed & 0b00000010) != 0;
        let transparent_color_flag = (packed & 0b00000001) != 0;

        let delay_time = gce_reader.u16_at(1).unwrap_or(0);
        let transparent_color_index = data[3];

        // Skip to terminator (0x00)
        let mut new_pos = pos + 1 + 4;
        if new_pos < reader.size() {
            let terminator = reader.read(new_pos, 1)?[0];
            if terminator == 0 {
                new_pos += 1;
            }
        }

        Ok((
            new_pos,
            GraphicControlExtension {
                disposal_method,
                user_input_flag,
                transparent_color_flag,
                delay_time,
                transparent_color_index,
            },
        ))
    }

    /// Skips sub-blocks (sequence of sized blocks terminated by 0x00)
    fn skip_sub_blocks(reader: &dyn FileReader, mut pos: u64) -> Result<u64> {
        while pos < reader.size() {
            let block_size = reader.read(pos, 1)?[0];
            pos += 1;
            if block_size == 0 {
                break;
            }
            pos += block_size as u64;
        }
        Ok(pos)
    }

    /// Reads sub-blocks and collects their data
    fn read_sub_blocks(reader: &dyn FileReader, mut pos: u64) -> Result<(u64, Vec<u8>)> {
        let mut data = Vec::new();
        while pos < reader.size() {
            let block_size = reader.read(pos, 1)?[0];
            pos += 1;
            if block_size == 0 {
                break;
            }
            if pos + (block_size as u64) <= reader.size() {
                let block_data = reader.read(pos, block_size as usize)?;
                data.extend_from_slice(block_data);
            }
            pos += block_size as u64;
        }
        Ok((pos, data))
    }

    /// Reads XMP data from GIF (special format - not standard sub-blocks)
    /// GIF XMP is stored as raw data followed by a 258-byte "magic trailer"
    /// The trailer consists of: 0x01, 0xFF, 0xFE, ..., 0x01, 0x00, 0x00
    fn read_xmp_data(reader: &dyn FileReader, start_pos: u64) -> Result<(u64, Vec<u8>)> {
        // Read until we find the XMP end marker <?xpacket end=...?>
        // We need to scan the file for the end of XMP content
        let max_xmp_size = 1024 * 1024; // 1MB max XMP size
        let remaining = (reader.size() - start_pos).min(max_xmp_size as u64) as usize;

        if remaining == 0 {
            return Ok((start_pos, Vec::new()));
        }

        let raw_data = reader.read(start_pos, remaining)?;

        // Find the XMP end marker: <?xpacket end='w'?> or <?xpacket end="r"?>
        let end_marker = b"<?xpacket end=";
        let mut xmp_end = None;

        for i in 0..raw_data.len().saturating_sub(end_marker.len() + 5) {
            if &raw_data[i..i + end_marker.len()] == end_marker {
                // Find the closing ?>
                for j in i + end_marker.len()..raw_data.len().saturating_sub(1) {
                    if raw_data[j] == b'?' && raw_data[j + 1] == b'>' {
                        xmp_end = Some(j + 2);
                        break;
                    }
                }
                break;
            }
        }

        if let Some(end) = xmp_end {
            let xmp_data = raw_data[..end].to_vec();
            // Skip past the magic trailer (258 bytes) plus any remaining data
            let new_pos = start_pos + end as u64 + 258;
            Ok((new_pos.min(reader.size()), xmp_data))
        } else {
            // No end marker found - try to strip trailing non-XML bytes
            // The magic trailer starts with 0x01 and ends with 0x00
            if let Some(end_pos) = raw_data.iter().rposition(|&b| b == b'>') {
                let xmp_data = raw_data[..=end_pos].to_vec();
                Ok((start_pos + remaining as u64, xmp_data))
            } else {
                Ok((start_pos + remaining as u64, Vec::new()))
            }
        }
    }

    /// Reads comment blocks and appends to comment string
    fn read_comment_blocks(
        reader: &dyn FileReader,
        mut pos: u64,
        comment: &mut String,
    ) -> Result<u64> {
        while pos < reader.size() {
            let block_size = reader.read(pos, 1)?[0];
            pos += 1;
            if block_size == 0 {
                break;
            }
            if pos + (block_size as u64) <= reader.size()
                && let Ok(data) = reader.read(pos, block_size as usize)
                && let Ok(text) = String::from_utf8(data.to_vec())
            {
                if !comment.is_empty() {
                    comment.push(' ');
                }
                comment.push_str(&text);
            }
            pos += block_size as u64;
        }
        Ok(pos)
    }
}

/// Logical Screen Descriptor data structure
#[derive(Debug, Default)]
struct LogicalScreenDescriptor {
    width: u16,
    height: u16,
    global_color_table_flag: bool,
    color_resolution: u8,
    global_color_table_size: u32,
    background_color_index: u8,
    pixel_aspect_ratio: u8,
}

/// Result of scanning GIF blocks
#[derive(Debug)]
struct BlockScanResult {
    frame_count: u32,
    is_animated: bool,
    comment: Option<String>,
    delay_time: Option<u16>,
    disposal_method: Option<u8>,
    has_transparency: bool,
    transparent_color: Option<u8>,
    icc_profile: Option<Vec<u8>>,
    xmp_data: Option<Vec<u8>>,
}

/// Graphic Control Extension data
#[derive(Debug, Default, Clone, Copy)]
struct GraphicControlExtension {
    disposal_method: u8,
    user_input_flag: bool,
    transparent_color_flag: bool,
    delay_time: u16,
    transparent_color_index: u8,
}

impl FormatParser for GIFParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid GIF signature"));
        }

        let mut metadata = MetadataMap::new();

        // Basic file information
        metadata.insert("FileType".to_string(), TagValue::String("GIF".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        let version = Self::read_version(reader)?;
        metadata.insert(
            "GIFVersion".to_string(),
            TagValue::String(version.to_string()),
        );
        // Add GIF: prefixed version for format-specific tagging
        metadata.insert(
            "GIF:Version".to_string(),
            TagValue::String(version.to_string()),
        );

        // Logical Screen Descriptor fields
        let lsd = Self::read_logical_screen_descriptor(reader)?;

        metadata.insert(
            "ImageWidth".to_string(),
            TagValue::String(lsd.width.to_string()),
        );
        metadata.insert(
            "ImageHeight".to_string(),
            TagValue::String(lsd.height.to_string()),
        );
        // Add GIF: prefixed versions for format-specific tagging
        metadata.insert("GIF:Width".to_string(), TagValue::Integer(lsd.width as i64));
        metadata.insert(
            "GIF:Height".to_string(),
            TagValue::Integer(lsd.height as i64),
        );

        // ColorResolutionDepth - ExifTool tag name for bits per primary color
        metadata.insert(
            "ColorResolutionDepth".to_string(),
            TagValue::Integer(lsd.color_resolution as i64),
        );

        // HasColorMap - ExifTool tag for global color table flag
        let has_color_map_str = if lsd.global_color_table_flag {
            "Yes"
        } else {
            "No"
        };
        metadata.insert(
            "HasColorMap".to_string(),
            TagValue::String(has_color_map_str.to_string()),
        );
        // Add GIF: prefixed version for format-specific tagging
        metadata.insert(
            "GIF:GlobalColorTable".to_string(),
            TagValue::String(has_color_map_str.to_string()),
        );

        if lsd.global_color_table_flag {
            metadata.insert(
                "GlobalColorTableSize".to_string(),
                TagValue::Integer(lsd.global_color_table_size as i64),
            );
            // Add GIF: prefixed version for format-specific tagging
            metadata.insert(
                "GIF:ColorTableSize".to_string(),
                TagValue::Integer(lsd.global_color_table_size as i64),
            );
            // BitsPerPixel - log2 of color table size
            let bits_per_pixel = (lsd.global_color_table_size as f64).log2() as i64;
            metadata.insert(
                "BitsPerPixel".to_string(),
                TagValue::Integer(bits_per_pixel),
            );
        }

        // BackgroundColor - ExifTool uses this name (not BackgroundColorIndex)
        metadata.insert(
            "BackgroundColor".to_string(),
            TagValue::Integer(lsd.background_color_index as i64),
        );
        // Add GIF: prefixed version for format-specific tagging
        metadata.insert(
            "GIF:BackgroundColor".to_string(),
            TagValue::String(format!("#{:02x}", lsd.background_color_index)),
        );

        // PixelAspectRatio - convert from raw value to actual ratio
        // If raw value is 0, aspect ratio is not given, otherwise: (value + 15) / 64
        // ExifTool rounds to nearest integer
        if lsd.pixel_aspect_ratio == 0 {
            metadata.insert(
                "PixelAspectRatio".to_string(),
                TagValue::Integer(1), // Default 1:1
            );
        } else {
            let ratio = ((lsd.pixel_aspect_ratio as f64 + 15.0) / 64.0).round() as i64;
            metadata.insert("PixelAspectRatio".to_string(), TagValue::Integer(ratio));
        }

        // Scan for extensions and image blocks
        let scan_result = Self::scan_blocks(reader)?;

        metadata.insert(
            "FrameCount".to_string(),
            TagValue::Integer(scan_result.frame_count as i64),
        );
        // Add GIF: prefixed version for format-specific tagging
        metadata.insert(
            "GIF:FrameCount".to_string(),
            TagValue::Integer(scan_result.frame_count as i64),
        );

        if scan_result.is_animated {
            metadata.insert("Animation".to_string(), TagValue::String("yes".to_string()));
        }

        if let Some(comment) = scan_result.comment {
            metadata.insert("Comment".to_string(), TagValue::String(comment));
        }

        if let Some(delay_time) = scan_result.delay_time {
            metadata.insert(
                "FrameDelay".to_string(),
                TagValue::String(format!("{} cs", delay_time)),
            );
        }

        if let Some(disposal_method) = scan_result.disposal_method {
            let disposal_str = match disposal_method {
                0 => "Unspecified",
                1 => "Do not dispose",
                2 => "Restore to background",
                3 => "Restore to previous",
                _ => "Unknown",
            };
            metadata.insert(
                "DisposalMethod".to_string(),
                TagValue::String(disposal_str.to_string()),
            );
        }

        if scan_result.has_transparency {
            metadata.insert(
                "HasTransparency".to_string(),
                TagValue::String("yes".to_string()),
            );
            if let Some(color) = scan_result.transparent_color {
                metadata.insert(
                    "TransparentColorIndex".to_string(),
                    TagValue::Integer(color as i64),
                );
            }
        }

        // Parse ICC profile if present
        if let Some(icc_data) = scan_result.icc_profile {
            if icc_data.len() >= 128 {
                match crate::parsers::icc::parse_icc_profile_data(&icc_data) {
                    Ok(icc_tags) => {
                        for (tag_name, value) in icc_tags {
                            metadata.insert(format!("ICC_Profile:{}", tag_name), value);
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to parse ICC profile in GIF: {}", e);
                    }
                }
            }
        }

        // Parse XMP data if present
        if let Some(xmp_bytes) = scan_result.xmp_data {
            match crate::parsers::xmp::rdf_parser::parse_xmp(&xmp_bytes) {
                Ok(xmp_tags) => {
                    for (tag_name, value) in xmp_tags {
                        metadata.insert(tag_name, TagValue::String(value));
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Failed to parse XMP in GIF: {}", e);
                }
            }
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::GIF)
    }
}

/// Parses metadata from GIF files.
pub fn parse_gif_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = GIFParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
