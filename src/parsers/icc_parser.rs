//! ICC Profile parser for embedded color profiles
//!
//! This module handles parsing of ICC (International Color Consortium)
//! profiles embedded in various file formats (PDF, JPEG, PNG, TIFF, etc.).
//! ICC profiles describe color characteristics for accurate color reproduction
//! across different devices.
//!
//! # ICC Profile Structure
//!
//! An ICC profile consists of:
//! 1. **Profile Header** (128 bytes): Contains profile metadata
//!    - Profile size, CMM type, version, class, color space, etc.
//! 2. **Tag Table**: List of tags with their signatures, offsets, and sizes
//! 3. **Tagged Element Data**: Actual tag data (descriptions, calibration data, etc.)
//!
//! # PDF Integration
//!
//! ICC profiles in PDFs are typically found in:
//! - OutputIntents array in the document catalog
//! - Embedded as compressed streams in PDF objects
//!
//! # Example Profile Header Layout
//!
//! ```text
//! Offset  Size  Field
//! 0       4     Profile size
//! 4       4     CMM Type (e.g., "Lino" for Linotronic)
//! 8       4     Profile version (major.minor.bugfix)
//! 12      4     Profile class (e.g., "mntr" for display)
//! 16      4     Color space (e.g., "RGB ")
//! 20      4     PCS (Profile Connection Space)
//! 24      12    Date and time
//! 36      4     Profile signature ('acsp')
//! 40      4     Primary platform
//! 44      4     Profile flags
//! 48      4     Device manufacturer
//! 52      4     Device model
//! 56      8     Device attributes
//! 64      4     Rendering intent
//! 68      12    Illuminant XYZ
//! 80      4     Profile creator
//! ...
//! 128     ...   Tag table starts here
//! ```

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use flate2::read::ZlibDecoder;
use std::collections::HashMap;
use std::io::Read;
use std::str;

/// Extracts ICC profile metadata from a PDF file.
///
/// This function searches for ICC profiles in the PDF's OutputIntents,
/// extracts the profile stream, decompresses if necessary, and parses
/// the ICC profile header and tags.
///
/// # Parameters
///
/// - `reader`: FileReader implementation for accessing the PDF file
///
/// # Returns
///
/// - `Ok(MetadataMap)`: Extracted ICC profile metadata with "Profile:" prefix
/// - `Err(ExifToolError)`: Parse error or I/O error
///
/// # ICC Profile Tags Extracted
///
/// - Profile:ProfileCMMType
/// - Profile:ProfileVersion
/// - Profile:ProfileClass
/// - Profile:ColorSpaceData
/// - Profile:ProfileConnectionSpace
/// - Profile:ProfileDateTime
/// - Profile:ProfileFileSignature
/// - Profile:PrimaryPlatform
/// - Profile:DeviceManufacturer
/// - Profile:DeviceModel
/// - Profile:RenderingIntent
/// - Profile:ConnectionSpaceIlluminant
/// - Profile:ProfileCreator
/// - Profile:MediaWhitePoint
/// - Profile:MediaBlackPoint
/// - And many more ICC tags
pub fn extract_icc_profile(reader: &dyn FileReader) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Try to find and extract ICC profile from PDF
    match extract_icc_from_pdf(reader) {
        Ok(icc_data) => {
            // Parse the ICC profile header and tags
            match parse_icc_profile(&icc_data) {
                Ok(icc_metadata) => {
                    for (key, value) in icc_metadata {
                        metadata.insert(format!("Profile:{}", key), value);
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Failed to parse ICC profile: {}", e);
                    return Err(e);
                }
            }
        }
        Err(e) => {
            return Err(e);
        }
    }

    if metadata.is_empty() {
        return Err(ExifToolError::parse_error("No ICC profile found in PDF"));
    }

    Ok(metadata)
}

/// Extracts the raw ICC profile data from a PDF file.
///
/// This function searches for the ICC profile stream in the PDF's OutputIntents
/// and returns the decompressed profile data.
///
/// The function performs these steps:
/// 1. Finds the Catalog object from the trailer
/// 2. Locates /OutputIntents array in the Catalog
/// 3. Finds /DestOutputProfile stream reference
/// 4. Reads and decompresses the stream (if FlateDecode)
fn extract_icc_from_pdf(reader: &dyn FileReader) -> Result<Vec<u8>> {
    let file_size = reader.size();

    // Read the last 1024 bytes to find trailer
    let tail_size = std::cmp::min(1024, file_size as usize);
    let tail_offset = file_size.saturating_sub(tail_size as u64);
    let tail_data = reader.read(tail_offset, tail_size)?;

    // Find startxref and get xref offset
    let xref_offset = find_xref_offset(tail_data)?;

    // Read xref table and trailer region
    let xref_size = std::cmp::min(8192, file_size.saturating_sub(xref_offset) as usize);
    let xref_data = reader.read(xref_offset, xref_size)?;

    // Parse xref table
    let xref_map = parse_xref_table(xref_data)?;

    // Find /Root reference in trailer
    let root_ref = find_root_reference(xref_data)?;

    // Get Root/Catalog object offset
    let root_offset = *xref_map.get(&root_ref).ok_or_else(|| {
        ExifToolError::parse_error(format!("Root object {} not found in xref table", root_ref))
    })?;

    // Read Root/Catalog object (up to 8KB)
    let root_size = std::cmp::min(8192, file_size.saturating_sub(root_offset) as usize);
    let root_data = reader.read(root_offset, root_size)?;

    // Try to find ICC profile reference
    // Method 1: /OutputIntents array in Catalog
    // Method 2: /ICCBased in ColorSpace
    let output_profile_ref = find_output_profile_reference(root_data).or_else(|_| {
        // If OutputIntents not found, search entire PDF for /ICCBased reference
        find_icc_based_reference(reader, &xref_map)
    })?;

    // Get ICC profile stream object offset
    let profile_offset = *xref_map.get(&output_profile_ref).ok_or_else(|| {
        ExifToolError::parse_error(format!(
            "ICC profile object {} not found in xref table",
            output_profile_ref
        ))
    })?;

    // Read ICC profile stream object (up to 128KB for large ICC profiles)
    let profile_size = std::cmp::min(131072, file_size.saturating_sub(profile_offset) as usize);
    let profile_data = reader.read(profile_offset, profile_size)?;

    // Extract and decompress the stream
    extract_and_decompress_stream(profile_data)
}

/// Finds the /OutputIntents -> /DestOutputProfile reference in the Catalog
fn find_output_profile_reference(root_data: &[u8]) -> Result<u32> {
    // Search for /OutputIntents in binary data
    let output_intents_marker = b"/OutputIntents";
    let output_intents_pos = find_bytes(root_data, output_intents_marker).ok_or_else(|| {
        ExifToolError::parse_error("No /OutputIntents found in Catalog (no ICC profile)")
    })?;

    let after_output = &root_data[output_intents_pos..];

    // Find /DestOutputProfile reference
    let dest_profile_marker = b"/DestOutputProfile";
    let dest_profile_pos = find_bytes(after_output, dest_profile_marker).ok_or_else(|| {
        ExifToolError::parse_error("No /DestOutputProfile found in OutputIntents")
    })?;

    // Extract object reference after /DestOutputProfile
    let after_dest = &after_output[dest_profile_pos + 18..]; // "/DestOutputProfile".len() = 18

    // Convert to string for parsing (object references are always ASCII)
    let after_dest_str =
        str::from_utf8(&after_dest[..std::cmp::min(100, after_dest.len())]).unwrap_or("");

    // Parse object reference (e.g., "5 0 R")
    let obj_ref = parse_object_ref(after_dest_str)?;

    Ok(obj_ref)
}

/// Finds a byte sequence in a larger byte slice
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Finds ICC profile reference via /ICCBased in the PDF
///
/// This method searches for /ICCBased references in ColorSpace objects,
/// which is an alternative way PDFs embed ICC profiles (common in macOS PDFs)
fn find_icc_based_reference(reader: &dyn FileReader, _xref_map: &HashMap<u32, u64>) -> Result<u32> {
    let file_size = reader.size();

    // Search through all objects for /ICCBased reference
    // We'll read the first 64KB which should contain the page resources
    let search_size = std::cmp::min(65536, file_size as usize);
    let search_data = reader.read(0, search_size)?;

    // Find /ICCBased marker
    let icc_based_marker = b"/ICCBased";
    let icc_based_pos = find_bytes(search_data, icc_based_marker).ok_or_else(|| {
        ExifToolError::parse_error("No /ICCBased reference found (no ICC profile)")
    })?;

    // Extract object reference after /ICCBased (e.g., "36 0 R")
    let after_icc = &search_data[icc_based_pos + 9..]; // "/ICCBased".len() = 9

    // Convert to string for parsing (object references are always ASCII)
    // Handle potential UTF-8 issues by replacing invalid chars
    let after_icc_bytes = &after_icc[..std::cmp::min(100, after_icc.len())];
    let after_icc_str = String::from_utf8_lossy(after_icc_bytes);

    // Parse object reference
    let obj_ref = parse_object_ref(&after_icc_str)?;

    Ok(obj_ref)
}

/// Parses an object reference like "5 0 R" and returns the object number
fn parse_object_ref(s: &str) -> Result<u32> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.is_empty() {
        return Err(ExifToolError::parse_error("No object reference found"));
    }

    // Find the 'R' marker
    let r_index = parts.iter().position(|&p| p == "R" || p.starts_with('R'));

    if let Some(idx) = r_index {
        if idx >= 2 {
            // Object reference is "N G R" format
            return parts[idx - 2]
                .parse::<u32>()
                .map_err(|_| ExifToolError::parse_error("Invalid object number in reference"));
        } else if idx >= 1 {
            // Try first part
            return parts[0]
                .parse::<u32>()
                .map_err(|_| ExifToolError::parse_error("Invalid object number in reference"));
        }
    }

    // Fallback: try first numeric part
    for part in &parts {
        if let Ok(num) = part.parse::<u32>() {
            return Ok(num);
        }
    }

    Err(ExifToolError::parse_error(
        "Invalid object reference format",
    ))
}

/// Extracts and decompresses a stream from a PDF object
fn extract_and_decompress_stream(obj_data: &[u8]) -> Result<Vec<u8>> {
    // Find stream markers in binary data
    let stream_marker = b"stream";
    let stream_start_pos = find_bytes(obj_data, stream_marker)
        .ok_or_else(|| ExifToolError::parse_error("No stream marker found in object"))?;

    // Stream data starts after "stream" + newline
    // The newline can be \n or \r\n
    let stream_content_start = stream_start_pos + 6; // "stream".len() = 6
    let mut stream_offset = stream_content_start;

    // Skip newline after "stream"
    if obj_data.len() > stream_offset {
        if obj_data[stream_offset] == b'\r' {
            stream_offset += 1;
        }
        if obj_data.len() > stream_offset && obj_data[stream_offset] == b'\n' {
            stream_offset += 1;
        }
    }

    // Find end of stream
    let endstream_marker = b"endstream";
    let endstream_pos = find_bytes(obj_data, endstream_marker).ok_or_else(|| {
        ExifToolError::parse_error("No endstream marker found - stream may be truncated")
    })?;

    // Extract raw stream data
    let stream_data = &obj_data[stream_offset..endstream_pos];

    // Check if stream is compressed (FlateDecode) - check in the header before stream
    let header_data = &obj_data[..stream_start_pos];
    let is_compressed = find_bytes(header_data, b"/FlateDecode").is_some()
        || find_bytes(header_data, b"/Fl").is_some();

    if is_compressed {
        // Decompress using zlib
        let mut decoder = ZlibDecoder::new(stream_data);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).map_err(|e| {
            ExifToolError::parse_error(format!("Failed to decompress FlateDecode stream: {}", e))
        })?;
        Ok(decompressed)
    } else {
        // Return raw stream data
        Ok(stream_data.to_vec())
    }
}

/// Finds the /Root reference from trailer
fn find_root_reference(xref_data: &[u8]) -> Result<u32> {
    let xref_str = str::from_utf8(xref_data)
        .map_err(|_| ExifToolError::parse_error("xref data contains invalid UTF-8"))?;

    let trailer_pos = xref_str
        .find("trailer")
        .ok_or_else(|| ExifToolError::parse_error("trailer not found in PDF"))?;

    let after_trailer = &xref_str[trailer_pos..];

    // Find /Root reference
    let root_pos = after_trailer
        .find("/Root")
        .ok_or_else(|| ExifToolError::parse_error("No /Root reference in trailer"))?;

    let after_root = &after_trailer[root_pos + 5..]; // "/Root".len() = 5

    parse_object_ref(after_root)
}

/// Finds startxref offset from PDF tail
fn find_xref_offset(tail_data: &[u8]) -> Result<u64> {
    let tail_str = str::from_utf8(tail_data)
        .map_err(|_| ExifToolError::parse_error("PDF tail contains invalid UTF-8"))?;

    let startxref_pos = tail_str
        .rfind("startxref")
        .ok_or_else(|| ExifToolError::parse_error("startxref not found in PDF"))?;

    let after_keyword = &tail_str[startxref_pos + 9..]; // "startxref".len() = 9

    // Extract the number
    let num_str: String = after_keyword
        .chars()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_ascii_digit())
        .collect();

    num_str
        .parse::<u64>()
        .map_err(|_| ExifToolError::parse_error("Invalid xref offset after startxref"))
}

/// Parses xref table and builds object offset map
fn parse_xref_table(xref_data: &[u8]) -> Result<HashMap<u32, u64>> {
    let xref_str = str::from_utf8(xref_data)
        .map_err(|_| ExifToolError::parse_error("xref table contains invalid UTF-8"))?;

    let xref_pos = xref_str
        .find("xref")
        .ok_or_else(|| ExifToolError::parse_error("xref table not found"))?;

    let after_xref = &xref_str[xref_pos + 4..]; // "xref".len() = 4

    let mut xref_map = HashMap::new();
    let lines: Vec<&str> = after_xref.lines().collect();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();

        // Stop at trailer
        if line.starts_with("trailer") {
            break;
        }

        // Check if this is a subsection header (two numbers)
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 2 {
            if let (Ok(start_obj), Ok(count)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                // Parse xref entries for this subsection
                for j in 0..count {
                    i += 1;
                    if i >= lines.len() {
                        break;
                    }

                    let entry_line = lines[i].trim();
                    let entry_parts: Vec<&str> = entry_line.split_whitespace().collect();

                    if entry_parts.len() >= 3 {
                        if let Ok(offset) = entry_parts[0].parse::<u64>() {
                            let in_use = entry_parts[2];
                            if in_use == "n" {
                                let obj_num = start_obj + j;
                                xref_map.insert(obj_num, offset);
                            }
                        }
                    }
                }
            }
        }

        i += 1;
    }

    if xref_map.is_empty() {
        return Err(ExifToolError::parse_error("No valid xref entries found"));
    }

    Ok(xref_map)
}

/// Parses ICC profile binary data and extracts metadata.
///
/// This is the main entry point for parsing ICC profile data from any source
/// (JPEG APP2 segments, PDF streams, PNG chunks, etc.).
///
/// # ICC Profile Format
///
/// The profile starts with a 128-byte header followed by a tag table.
/// Each tag has a 4-byte signature, 4-byte offset, and 4-byte size.
///
/// # Parameters
///
/// - `data`: Raw ICC profile binary data
///
/// # Returns
///
/// - `Ok(HashMap)`: Map of ICC tag names to their values (without "Profile:" prefix)
/// - `Err(ExifToolError)`: Parse error
///
/// # Example
///
/// ```no_run
/// use exiftool_rs::parsers::icc_parser::parse_icc_profile_data;
///
/// # fn example(icc_data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
/// let metadata = parse_icc_profile_data(icc_data)?;
/// # Ok(())
/// # }
/// ```
pub fn parse_icc_profile_data(data: &[u8]) -> Result<HashMap<String, TagValue>> {
    parse_icc_profile(data)
}

/// Internal ICC profile parser.
///
/// # Parameters
///
/// - `data`: Raw ICC profile binary data
///
/// # Returns
///
/// - `Ok(HashMap)`: Map of ICC tag names to their values
/// - `Err(ExifToolError)`: Parse error
fn parse_icc_profile(data: &[u8]) -> Result<HashMap<String, TagValue>> {
    // ICC profile must be at least 128 bytes (header size)
    if data.len() < 128 {
        return Err(ExifToolError::parse_error(
            "ICC profile too small (< 128 bytes)",
        ));
    }

    let mut metadata = HashMap::new();

    // Parse ICC profile header (128 bytes)
    parse_icc_header(data, &mut metadata)?;

    // Parse tag table and tags (after header)
    if data.len() > 128 {
        parse_icc_tags(data, &mut metadata)?;
    }

    Ok(metadata)
}

/// Parses the ICC profile header (first 128 bytes).
///
/// Extracts metadata from the fixed-format header including:
/// - Profile size, CMM type, version, class, color space
/// - Date/time, signature, platform, rendering intent, illuminant, creator
fn parse_icc_header(data: &[u8], metadata: &mut HashMap<String, TagValue>) -> Result<()> {
    // Profile size (bytes 0-3, big-endian u32)
    let _profile_size = read_u32_be(data, 0)?;

    // CMM Type (bytes 4-7, 4-char signature)
    let cmm_type = read_signature(data, 4)?;
    if !cmm_type.is_empty() {
        metadata.insert("ProfileCMMType".to_string(), TagValue::new_string(cmm_type));
    }

    // Profile Version (bytes 8-11)
    let version = format!(
        "{}.{}.{}",
        data.get(8).copied().unwrap_or(0),
        (data.get(9).copied().unwrap_or(0) >> 4) & 0x0F,
        data.get(9).copied().unwrap_or(0) & 0x0F
    );
    metadata.insert("ProfileVersion".to_string(), TagValue::new_string(version));

    // Profile Class (bytes 12-15, e.g., "mntr" for display device)
    let class = read_signature(data, 12)?;
    let class_name = match class.as_str() {
        "scnr" => "Input Device Profile",
        "mntr" => "Display Device Profile",
        "prtr" => "Output Device Profile",
        "link" => "DeviceLink Profile",
        "spac" => "ColorSpace Profile",
        "abst" => "Abstract Profile",
        "nmcl" => "Named Color Profile",
        _ => &class,
    };
    metadata.insert(
        "ProfileClass".to_string(),
        TagValue::new_string(class_name.to_string()),
    );

    // Color Space Data (bytes 16-19, e.g., "RGB ")
    let color_space = read_signature(data, 16)?.trim().to_string();
    metadata.insert(
        "ColorSpaceData".to_string(),
        TagValue::new_string(color_space),
    );

    // Profile Connection Space (bytes 20-23, usually "XYZ " or "Lab ")
    let pcs = read_signature(data, 20)?.trim().to_string();
    metadata.insert(
        "ProfileConnectionSpace".to_string(),
        TagValue::new_string(pcs),
    );

    // Date and Time (bytes 24-35)
    if data.len() >= 36 {
        let year = read_u16_be(data, 24)?;
        let month = read_u16_be(data, 26)?;
        let day = read_u16_be(data, 28)?;
        let hour = read_u16_be(data, 30)?;
        let minute = read_u16_be(data, 32)?;
        let second = read_u16_be(data, 34)?;
        let datetime = format!(
            "{}:{:02}:{:02} {:02}:{:02}:{:02}",
            year, month, day, hour, minute, second
        );
        metadata.insert(
            "ProfileDateTime".to_string(),
            TagValue::new_string(datetime),
        );
    }

    // Profile File Signature (bytes 36-39, should be 'acsp')
    let signature = read_signature(data, 36)?;
    metadata.insert(
        "ProfileFileSignature".to_string(),
        TagValue::new_string(signature),
    );

    // Primary Platform (bytes 40-43)
    let platform = read_signature(data, 40)?;
    let platform_name = match platform.trim() {
        "APPL" => "Apple Computer Inc.",
        "MSFT" => "Microsoft Corporation",
        "SGI" => "Silicon Graphics Inc.",
        "SUNW" => "Sun Microsystems",
        _ => platform.trim(),
    };
    if !platform_name.is_empty() {
        metadata.insert(
            "PrimaryPlatform".to_string(),
            TagValue::new_string(platform_name.to_string()),
        );
    }

    // CMM Flags (bytes 44-47)
    let flags = read_u32_be(data, 44)?;
    let embedded = if flags & 0x01 == 0 {
        "Not Embedded"
    } else {
        "Embedded"
    };
    let independent = if flags & 0x02 == 0 {
        "Independent"
    } else {
        "Dependent"
    };
    metadata.insert(
        "CMMFlags".to_string(),
        TagValue::new_string(format!("{}, {}", embedded, independent)),
    );

    // Device Manufacturer (bytes 48-51)
    if data.len() >= 52 {
        let manufacturer = read_signature(data, 48)?;
        if !manufacturer.trim().is_empty() {
            metadata.insert(
                "DeviceManufacturer".to_string(),
                TagValue::new_string(manufacturer),
            );
        }
    }

    // Device Model (bytes 52-55)
    if data.len() >= 56 {
        let model = read_signature(data, 52)?;
        if !model.trim().is_empty() {
            metadata.insert("DeviceModel".to_string(), TagValue::new_string(model));
        }
    }

    // Device Attributes (bytes 56-63)
    if data.len() >= 64 {
        let attrs = read_u64_be(data, 56)?;
        let reflective = if attrs & 0x01 == 0 {
            "Reflective"
        } else {
            "Transparency"
        };
        let glossy = if attrs & 0x02 == 0 { "Glossy" } else { "Matte" };
        let positive = if attrs & 0x04 == 0 {
            "Positive"
        } else {
            "Negative"
        };
        let color = if attrs & 0x08 == 0 { "Color" } else { "B&W" };
        metadata.insert(
            "DeviceAttributes".to_string(),
            TagValue::new_string(format!(
                "{}, {}, {}, {}",
                reflective, glossy, positive, color
            )),
        );
    }

    // Rendering Intent (bytes 64-67)
    if data.len() >= 68 {
        let intent = read_u32_be(data, 64)?;
        let intent_name = match intent {
            0 => "Perceptual",
            1 => "Relative Colorimetric",
            2 => "Saturation",
            3 => "Absolute Colorimetric",
            _ => "Unknown",
        };
        metadata.insert(
            "RenderingIntent".to_string(),
            TagValue::new_string(intent_name.to_string()),
        );
    }

    // Connection Space Illuminant (bytes 68-79, XYZ values)
    if data.len() >= 80 {
        let x = read_s15fixed16(data, 68)?;
        let y = read_s15fixed16(data, 72)?;
        let z = read_s15fixed16(data, 76)?;
        metadata.insert(
            "ConnectionSpaceIlluminant".to_string(),
            TagValue::new_string(format!("{} {} {}", x, y, z)),
        );
    }

    // Profile Creator (bytes 80-83)
    if data.len() >= 84 {
        let creator = read_signature(data, 80)?;
        if !creator.trim().is_empty() {
            metadata.insert("ProfileCreator".to_string(), TagValue::new_string(creator));
        }
    }

    // Profile ID (bytes 84-99) - 16 bytes MD5 hash
    if data.len() >= 100 {
        let id_bytes = &data[84..100];
        // Check if all zeros
        if id_bytes.iter().all(|&b| b == 0) {
            metadata.insert(
                "ProfileID".to_string(),
                TagValue::new_string("0".to_string()),
            );
        } else {
            let id_hex = id_bytes
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();
            metadata.insert("ProfileID".to_string(), TagValue::new_string(id_hex));
        }
    }

    Ok(())
}

/// Parses ICC profile tags from the tag table.
///
/// After the 128-byte header, the tag table contains:
/// - Tag count (4 bytes)
/// - Tag table entries (12 bytes each): signature, offset, size
///
/// This function extracts common tags like:
/// - desc (description)
/// - cprt (copyright)
/// - wtpt (white point)
/// - bkpt (black point)
/// - rXYZ, gXYZ, bXYZ (RGB matrix columns)
/// - rTRC, gTRC, bTRC (tone reproduction curves)
/// - dmnd, dmdd (device manufacturer/model descriptions)
/// - vued, view (viewing conditions)
/// - lumi (luminance)
/// - meas (measurement)
/// - tech (technology)
fn parse_icc_tags(data: &[u8], metadata: &mut HashMap<String, TagValue>) -> Result<()> {
    // Tag count at offset 128
    if data.len() < 132 {
        return Ok(()); // No tag table
    }

    let tag_count = read_u32_be(data, 128)?;

    // Tag table starts at offset 132
    // Each entry is 12 bytes: 4-byte signature, 4-byte offset, 4-byte size
    for i in 0..tag_count {
        let entry_offset = 132 + (i * 12) as usize;
        if entry_offset + 12 > data.len() {
            break;
        }

        let tag_signature = read_signature(data, entry_offset)?;
        let tag_offset = read_u32_be(data, entry_offset + 4)? as usize;
        let tag_size = read_u32_be(data, entry_offset + 8)? as usize;

        // Validate tag offset and size
        if tag_offset >= data.len() || tag_offset + tag_size > data.len() {
            continue; // Skip invalid tags
        }

        let tag_data = &data[tag_offset..tag_offset + tag_size];

        // Parse tag based on signature
        match tag_signature.trim() {
            "desc" => {
                if let Ok(desc) = parse_text_description_type(tag_data) {
                    metadata.insert("ProfileDescription".to_string(), TagValue::new_string(desc));
                }
            }
            "cprt" => {
                if let Ok(cprt) = parse_text_type(tag_data) {
                    metadata.insert("ProfileCopyright".to_string(), TagValue::new_string(cprt));
                }
            }
            "wtpt" => {
                if let Ok(xyz) = parse_xyz_type(tag_data) {
                    metadata.insert(
                        "MediaWhitePoint".to_string(),
                        TagValue::new_string(format!("{} {} {}", xyz.0, xyz.1, xyz.2)),
                    );
                }
            }
            "bkpt" => {
                if let Ok(xyz) = parse_xyz_type(tag_data) {
                    metadata.insert(
                        "MediaBlackPoint".to_string(),
                        TagValue::new_string(format!("{} {} {}", xyz.0, xyz.1, xyz.2)),
                    );
                }
            }
            "rXYZ" => {
                if let Ok(xyz) = parse_xyz_type(tag_data) {
                    metadata.insert(
                        "RedMatrixColumn".to_string(),
                        TagValue::new_string(format!("{} {} {}", xyz.0, xyz.1, xyz.2)),
                    );
                }
            }
            "gXYZ" => {
                if let Ok(xyz) = parse_xyz_type(tag_data) {
                    metadata.insert(
                        "GreenMatrixColumn".to_string(),
                        TagValue::new_string(format!("{} {} {}", xyz.0, xyz.1, xyz.2)),
                    );
                }
            }
            "bXYZ" => {
                if let Ok(xyz) = parse_xyz_type(tag_data) {
                    metadata.insert(
                        "BlueMatrixColumn".to_string(),
                        TagValue::new_string(format!("{} {} {}", xyz.0, xyz.1, xyz.2)),
                    );
                }
            }
            "rTRC" => {
                let desc = format!("(Binary data {} bytes, use -b option to extract)", tag_size);
                metadata.insert(
                    "RedToneReproductionCurve".to_string(),
                    TagValue::new_string(desc),
                );
            }
            "gTRC" => {
                let desc = format!("(Binary data {} bytes, use -b option to extract)", tag_size);
                metadata.insert(
                    "GreenToneReproductionCurve".to_string(),
                    TagValue::new_string(desc),
                );
            }
            "bTRC" => {
                let desc = format!("(Binary data {} bytes, use -b option to extract)", tag_size);
                metadata.insert(
                    "BlueToneReproductionCurve".to_string(),
                    TagValue::new_string(desc),
                );
            }
            "dmnd" => {
                if let Ok(desc) = parse_text_description_type(tag_data) {
                    metadata.insert("DeviceMfgDesc".to_string(), TagValue::new_string(desc));
                }
            }
            "dmdd" => {
                if let Ok(desc) = parse_text_description_type(tag_data) {
                    metadata.insert("DeviceModelDesc".to_string(), TagValue::new_string(desc));
                }
            }
            "vued" => {
                if let Ok(desc) = parse_text_description_type(tag_data) {
                    metadata.insert("ViewingCondDesc".to_string(), TagValue::new_string(desc));
                }
            }
            "view" => {
                if let Ok(viewing_cond) = parse_viewing_conditions(tag_data) {
                    if let Some(illuminant) = viewing_cond.get("illuminant") {
                        metadata.insert(
                            "ViewingCondIlluminant".to_string(),
                            TagValue::new_string(illuminant.clone()),
                        );
                    }
                    if let Some(surround) = viewing_cond.get("surround") {
                        metadata.insert(
                            "ViewingCondSurround".to_string(),
                            TagValue::new_string(surround.clone()),
                        );
                    }
                    if let Some(illum_type) = viewing_cond.get("illuminant_type") {
                        metadata.insert(
                            "ViewingCondIlluminantType".to_string(),
                            TagValue::new_string(illum_type.clone()),
                        );
                    }
                }
            }
            "lumi" => {
                if let Ok(xyz) = parse_xyz_type(tag_data) {
                    metadata.insert(
                        "Luminance".to_string(),
                        TagValue::new_string(format!("{} {} {}", xyz.0, xyz.1, xyz.2)),
                    );
                }
            }
            "meas" => {
                if let Ok(measurement) = parse_measurement(tag_data) {
                    if let Some(observer) = measurement.get("observer") {
                        metadata.insert(
                            "MeasurementObserver".to_string(),
                            TagValue::new_string(observer.clone()),
                        );
                    }
                    if let Some(backing) = measurement.get("backing") {
                        metadata.insert(
                            "MeasurementBacking".to_string(),
                            TagValue::new_string(backing.clone()),
                        );
                    }
                    if let Some(geometry) = measurement.get("geometry") {
                        metadata.insert(
                            "MeasurementGeometry".to_string(),
                            TagValue::new_string(geometry.clone()),
                        );
                    }
                    if let Some(flare) = measurement.get("flare") {
                        metadata.insert(
                            "MeasurementFlare".to_string(),
                            TagValue::new_string(flare.clone()),
                        );
                    }
                    if let Some(illuminant) = measurement.get("illuminant") {
                        metadata.insert(
                            "MeasurementIlluminant".to_string(),
                            TagValue::new_string(illuminant.clone()),
                        );
                    }
                }
            }
            "tech" => {
                if let Ok(tech) = parse_signature_type(tag_data) {
                    let tech_name = match tech.trim() {
                        "fscn" => "Film Scanner",
                        "dcam" => "Digital Camera",
                        "rscn" => "Reflective Scanner",
                        "ijet" => "Ink Jet Printer",
                        "twax" => "Thermal Wax Printer",
                        "epho" => "Electrophotographic Printer",
                        "esta" => "Electrostatic Printer",
                        "dsub" => "Dye Sublimation Printer",
                        "rpho" => "Photographic Paper Printer",
                        "fprn" => "Film Writer",
                        "vidm" => "Video Monitor",
                        "vidc" => "Video Camera",
                        "pjtv" => "Projection Television",
                        "CRT" => "Cathode Ray Tube Display",
                        "PMD" => "Passive Matrix Display",
                        "AMD" => "Active Matrix Display",
                        "KPCD" => "Photo CD",
                        "imgs" => "Photo Image Setter",
                        "grav" => "Gravure",
                        "offs" => "Offset Lithography",
                        "silk" => "Silkscreen",
                        "flex" => "Flexography",
                        _ => &tech,
                    };
                    metadata.insert(
                        "Technology".to_string(),
                        TagValue::new_string(tech_name.to_string()),
                    );
                }
            }
            _ => {
                // Unknown or unhandled tag
            }
        }
    }

    Ok(())
}

/// Parses ICC textDescriptionType (old-style text)
fn parse_text_description_type(data: &[u8]) -> Result<String> {
    if data.len() < 12 {
        return Err(ExifToolError::parse_error("textDescriptionType too small"));
    }

    // Check type signature (should be "desc")
    let type_sig = read_signature(data, 0)?;

    if type_sig.trim() == "desc" {
        // ASCII description count at offset 8
        let ascii_count = read_u32_be(data, 8)? as usize;
        if ascii_count > 0 && data.len() >= 12 + ascii_count {
            let text_bytes = &data[12..12 + ascii_count];
            // Remove null terminator if present
            let text_len = text_bytes
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(text_bytes.len());
            return Ok(String::from_utf8_lossy(&text_bytes[..text_len]).to_string());
        }
    } else if type_sig.trim() == "mluc" {
        // MultiLocalizedUnicodeType - modern text format
        return parse_mluc_type(data);
    }

    Err(ExifToolError::parse_error("Invalid text description type"))
}

/// Parses ICC multiLocalizedUnicodeType (modern text format)
fn parse_mluc_type(data: &[u8]) -> Result<String> {
    if data.len() < 16 {
        return Err(ExifToolError::parse_error("mluc type too small"));
    }

    // Number of records at offset 8
    let num_records = read_u32_be(data, 8)? as usize;
    // Record size at offset 12 (should be 12)
    let _record_size = read_u32_be(data, 12)?;

    if num_records == 0 {
        return Err(ExifToolError::parse_error("mluc has no records"));
    }

    // Each record: 4 bytes language code, 4 bytes country code, 4 bytes length, 4 bytes offset
    // We'll just use the first record
    if data.len() < 16 + 12 {
        return Err(ExifToolError::parse_error("mluc record table too small"));
    }

    let str_length = read_u32_be(data, 16 + 8)? as usize;
    let str_offset = read_u32_be(data, 16 + 12)? as usize;

    if str_offset + str_length > data.len() {
        return Err(ExifToolError::parse_error("mluc string out of bounds"));
    }

    // String is UTF-16 Big Endian
    let utf16_bytes = &data[str_offset..str_offset + str_length];
    let u16_vec: Vec<u16> = utf16_bytes
        .chunks(2)
        .filter_map(|chunk| {
            if chunk.len() == 2 {
                Some(u16::from_be_bytes([chunk[0], chunk[1]]))
            } else {
                None
            }
        })
        .collect();

    String::from_utf16(&u16_vec)
        .map_err(|_| ExifToolError::parse_error("Invalid UTF-16 in mluc string"))
}

/// Parses ICC textType (simple text)
fn parse_text_type(data: &[u8]) -> Result<String> {
    if data.len() < 8 {
        return Err(ExifToolError::parse_error("textType too small"));
    }

    // Type signature at offset 0 (should be "text")
    let type_sig = read_signature(data, 0)?;

    if type_sig.trim() == "text" {
        // Text starts at offset 8
        let text_bytes = &data[8..];
        // Remove null terminator if present
        let text_len = text_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(text_bytes.len());
        return Ok(String::from_utf8_lossy(&text_bytes[..text_len]).to_string());
    } else if type_sig.trim() == "mluc" {
        // Also support mluc for copyright
        return parse_mluc_type(data);
    }

    Err(ExifToolError::parse_error("Invalid text type"))
}

/// Parses ICC XYZType (XYZ color values)
fn parse_xyz_type(data: &[u8]) -> Result<(f64, f64, f64)> {
    if data.len() < 20 {
        return Err(ExifToolError::parse_error("XYZType too small"));
    }

    // Type signature at offset 0 (should be "XYZ ")
    // Reserved at offset 4-7
    // XYZ values start at offset 8 (3 x s15Fixed16Number)
    let x = read_s15fixed16(data, 8)?;
    let y = read_s15fixed16(data, 12)?;
    let z = read_s15fixed16(data, 16)?;

    Ok((x, y, z))
}

/// Parses ICC signatureType (4-byte signature)
fn parse_signature_type(data: &[u8]) -> Result<String> {
    if data.len() < 12 {
        return Err(ExifToolError::parse_error("signatureType too small"));
    }

    // Type signature at offset 0 (should be "sig ")
    // Reserved at offset 4-7
    // Signature at offset 8-11
    let sig = read_signature(data, 8)?;
    Ok(sig)
}

/// Parses ICC viewing conditions structure
fn parse_viewing_conditions(data: &[u8]) -> Result<HashMap<String, String>> {
    let mut result = HashMap::new();

    if data.len() < 36 {
        return Err(ExifToolError::parse_error("Viewing conditions too small"));
    }

    // Type signature at offset 0 (should be "view")
    // Reserved at offset 4-7
    // Illuminant XYZ at offset 8-19
    let illum_x = read_s15fixed16(data, 8)?;
    let illum_y = read_s15fixed16(data, 12)?;
    let illum_z = read_s15fixed16(data, 16)?;
    result.insert(
        "illuminant".to_string(),
        format!("{} {} {}", illum_x, illum_y, illum_z),
    );

    // Surround XYZ at offset 20-31
    let surr_x = read_s15fixed16(data, 20)?;
    let surr_y = read_s15fixed16(data, 24)?;
    let surr_z = read_s15fixed16(data, 28)?;
    result.insert(
        "surround".to_string(),
        format!("{} {} {}", surr_x, surr_y, surr_z),
    );

    // Illuminant type at offset 32-35
    if data.len() >= 36 {
        let illum_type = read_u32_be(data, 32)?;
        let illum_name = match illum_type {
            1 => "D50",
            2 => "D65",
            3 => "D93",
            4 => "F2",
            5 => "D55",
            6 => "A",
            7 => "Equi-Power (E)",
            8 => "F8",
            _ => "Unknown",
        };
        result.insert("illuminant_type".to_string(), illum_name.to_string());
    }

    Ok(result)
}

/// Parses ICC measurement structure
fn parse_measurement(data: &[u8]) -> Result<HashMap<String, String>> {
    let mut result = HashMap::new();

    if data.len() < 36 {
        return Err(ExifToolError::parse_error("Measurement data too small"));
    }

    // Type signature at offset 0 (should be "meas")
    // Reserved at offset 4-7
    // Standard observer at offset 8-11
    let observer = read_u32_be(data, 8)?;
    let observer_name = match observer {
        1 => "CIE 1931",
        2 => "CIE 1964",
        _ => "Unknown",
    };
    result.insert("observer".to_string(), observer_name.to_string());

    // Measurement backing XYZ at offset 12-23
    let back_x = read_s15fixed16(data, 12)?;
    let back_y = read_s15fixed16(data, 16)?;
    let back_z = read_s15fixed16(data, 20)?;
    result.insert(
        "backing".to_string(),
        format!("{} {} {}", back_x, back_y, back_z),
    );

    // Measurement geometry at offset 24-27
    let geometry = read_u32_be(data, 24)?;
    let geometry_name = match geometry {
        0 => "Unknown",
        1 => "0/45 or 45/0",
        2 => "0/d or d/0",
        _ => "Unknown",
    };
    result.insert("geometry".to_string(), geometry_name.to_string());

    // Measurement flare at offset 28-31 (u16Fixed16Number)
    if data.len() >= 32 {
        let flare = read_u16fixed16(data, 28)?;
        result.insert("flare".to_string(), format!("{}%", flare * 100.0));
    }

    // Standard illuminant at offset 32-35
    if data.len() >= 36 {
        let illuminant = read_u32_be(data, 32)?;
        let illuminant_name = match illuminant {
            1 => "D50",
            2 => "D65",
            3 => "D93",
            4 => "F2",
            5 => "D55",
            6 => "A",
            7 => "Equi-Power (E)",
            8 => "F8",
            _ => "Unknown",
        };
        result.insert("illuminant".to_string(), illuminant_name.to_string());
    }

    Ok(result)
}

/// Reads a 4-byte big-endian unsigned integer.
fn read_u32_be(data: &[u8], offset: usize) -> Result<u32> {
    if offset + 4 > data.len() {
        return Err(ExifToolError::parse_error("Offset out of bounds"));
    }
    Ok(u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]))
}

/// Reads a 2-byte big-endian unsigned integer.
fn read_u16_be(data: &[u8], offset: usize) -> Result<u16> {
    if offset + 2 > data.len() {
        return Err(ExifToolError::parse_error("Offset out of bounds"));
    }
    Ok(u16::from_be_bytes([data[offset], data[offset + 1]]))
}

/// Reads an 8-byte big-endian unsigned integer.
fn read_u64_be(data: &[u8], offset: usize) -> Result<u64> {
    if offset + 8 > data.len() {
        return Err(ExifToolError::parse_error("Offset out of bounds"));
    }
    Ok(u64::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ]))
}

/// Reads a 4-byte signature as a trimmed ASCII string.
fn read_signature(data: &[u8], offset: usize) -> Result<String> {
    if offset + 4 > data.len() {
        return Err(ExifToolError::parse_error("Offset out of bounds"));
    }
    let bytes = &data[offset..offset + 4];
    Ok(String::from_utf8_lossy(bytes).to_string())
}

/// Reads a signed 15.16 fixed-point number and converts to f64.
///
/// ICC profiles use s15Fixed16Number format for XYZ values.
/// This is a 32-bit value where the upper 16 bits are the integer part
/// and the lower 16 bits are the fractional part.
fn read_s15fixed16(data: &[u8], offset: usize) -> Result<f64> {
    let value = read_u32_be(data, offset)? as i32;
    let integer_part = (value >> 16) as i16 as f64;
    let fractional_part = (value & 0xFFFF) as f64 / 65536.0;
    Ok(integer_part + fractional_part)
}

/// Reads an unsigned 16.16 fixed-point number and converts to f64.
///
/// Similar to s15Fixed16Number but unsigned.
fn read_u16fixed16(data: &[u8], offset: usize) -> Result<f64> {
    let value = read_u32_be(data, offset)?;
    let integer_part = (value >> 16) as f64;
    let fractional_part = (value & 0xFFFF) as f64 / 65536.0;
    Ok(integer_part + fractional_part)
}
