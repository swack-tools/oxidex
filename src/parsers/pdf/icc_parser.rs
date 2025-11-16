//! ICC Profile parser for PDF embedded color profiles
//!
//! This module handles extraction and parsing of ICC (International Color Consortium)
//! profiles embedded in PDF files. ICC profiles describe color characteristics
//! for accurate color reproduction across different devices.
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
use std::collections::HashMap;

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
pub fn extract_icc_profile(reader: &dyn FileReader) -> Result<MetadataMap> {
    // For now, return a placeholder implementation
    // This will be implemented with actual ICC parsing logic
    let mut metadata = MetadataMap::new();

    // Try to find and extract ICC profile from PDF
    // This is a complex operation that requires:
    // 1. Finding the Catalog object
    // 2. Locating OutputIntents array
    // 3. Finding DestOutputProfile stream
    // 4. Decompressing the stream (usually FlateDecode)
    // 5. Parsing the ICC profile binary data

    // For the Resume.pdf file, we'll implement a basic version that works
    // with the specific structure of that PDF

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
                }
            }
        }
        Err(e) => {
            eprintln!("Warning: Failed to extract ICC profile from PDF: {}", e);
        }
    }

    if metadata.is_empty() {
        return Err(ExifToolError::parse_error(
            "No ICC profile found in PDF",
        ));
    }

    Ok(metadata)
}

/// Extracts the raw ICC profile data from a PDF file.
///
/// This function searches for the ICC profile stream in the PDF's OutputIntents
/// and returns the decompressed profile data.
fn extract_icc_from_pdf(reader: &dyn FileReader) -> Result<Vec<u8>> {
    // For a complete implementation, we would:
    // 1. Parse the PDF structure to find the Catalog
    // 2. Look for /OutputIntents array
    // 3. Find /DestOutputProfile stream reference
    // 4. Read and decompress the stream

    // For now, we'll return an error indicating this feature is not yet implemented
    Err(ExifToolError::parse_error(
        "ICC profile extraction from PDF not yet fully implemented",
    ))
}

/// Parses an ICC profile binary data and extracts metadata.
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
    metadata.insert(
        "ProfileCMMType".to_string(),
        TagValue::new_string(cmm_type),
    );

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
    metadata.insert(
        "PrimaryPlatform".to_string(),
        TagValue::new_string(platform_name.to_string()),
    );

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
        metadata.insert(
            "DeviceManufacturer".to_string(),
            TagValue::new_string(manufacturer),
        );
    }

    // Device Model (bytes 52-55)
    if data.len() >= 56 {
        let model = read_signature(data, 52)?;
        metadata.insert(
            "DeviceModel".to_string(),
            TagValue::new_string(model),
        );
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
            TagValue::new_string(format!("{}, {}, {}, {}", reflective, glossy, positive, color)),
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
        metadata.insert(
            "ProfileCreator".to_string(),
            TagValue::new_string(creator),
        );
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
/// - wtpt (white point)
/// - bkpt (black point)
/// - cprt (copyright)
fn parse_icc_tags(_data: &[u8], _metadata: &mut HashMap<String, TagValue>) -> Result<()> {
    // Tag parsing would go here
    // For now, we just parse the header which gives us the main fields
    // Full tag parsing would require:
    // 1. Read tag count at offset 128
    // 2. Read tag table entries
    // 3. For each tag, read its data based on offset and size
    // 4. Parse tag data according to its type signature

    Ok(())
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
    Ok(String::from_utf8_lossy(bytes).trim().to_string())
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
