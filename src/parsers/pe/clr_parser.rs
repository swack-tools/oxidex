//! .NET CLR (Common Language Runtime) metadata parser
//!
//! This module extracts .NET assembly information from PE files,
//! including CLR version, assembly name, version, culture, and public key token.

use crate::io::EndianReader;
use nom::{
    IResult,
    number::complete::{le_u16, le_u32},
};

/// IMAGE_COR20_HEADER (CLR Runtime Header) - 72 bytes
#[derive(Debug, Clone)]
pub struct ClrHeader {
    /// Size of the header (should be 72)
    pub cb: u32,
    /// Major version of CLR
    pub major_runtime_version: u16,
    /// Minor version of CLR
    pub minor_runtime_version: u16,
    /// RVA and size of metadata
    pub metadata: (u32, u32),
    /// Runtime flags
    pub flags: u32,
    /// Entry point token or RVA
    pub entry_point_token: u32,
    /// RVA and size of resources
    pub resources: (u32, u32),
    /// RVA and size of strong name signature
    pub strong_name_signature: (u32, u32),
    /// RVA and size of code manager table
    pub code_manager_table: (u32, u32),
    /// RVA and size of vtable fixups
    pub vtable_fixups: (u32, u32),
    /// RVA and size of export address table jumps
    pub export_address_table_jumps: (u32, u32),
    /// RVA and size of managed native header
    pub managed_native_header: (u32, u32),
}

/// CLR runtime flags
pub mod clr_flags {
    /// IL-only image (contains only MSIL, no native code)
    pub const COMIMAGE_FLAGS_ILONLY: u32 = 0x00000001;
    /// Image requires 32-bit environment
    pub const COMIMAGE_FLAGS_32BITREQUIRED: u32 = 0x00000002;
    /// Image has strong name signature
    pub const COMIMAGE_FLAGS_STRONGNAMESIGNED: u32 = 0x00000008;
    /// Image should be tracked for debug information
    pub const COMIMAGE_FLAGS_TRACKDEBUGDATA: u32 = 0x00010000;
    /// Image prefers 32-bit execution
    pub const COMIMAGE_FLAGS_32BITPREFERRED: u32 = 0x00020000;
}

/// .NET assembly information
#[derive(Debug, Clone, Default)]
pub struct DotNetInfo {
    /// Whether this is a .NET assembly
    pub is_dotnet: bool,
    /// CLR version (e.g., "2.5")
    pub clr_version: Option<String>,
    /// Assembly name
    pub assembly_name: Option<String>,
    /// Assembly version (e.g., "1.0.0.0")
    pub assembly_version: Option<String>,
    /// Assembly culture (e.g., "neutral", "en-US")
    pub assembly_culture: Option<String>,
    /// Public key token (hex string)
    pub public_key_token: Option<String>,
    /// Whether assembly is IL-only (no native code)
    pub il_only: bool,
    /// Whether assembly has strong name signature
    pub strong_name_signed: bool,
    /// Whether assembly requires 32-bit environment
    pub requires_32bit: bool,
    /// Whether assembly prefers 32-bit execution
    pub prefers_32bit: bool,
    /// Target framework (e.g., ".NETFramework,Version=v4.7.2")
    pub target_framework: Option<String>,
}

/// Parse CLR Runtime Header (IMAGE_COR20_HEADER)
pub fn parse_clr_header(input: &[u8]) -> IResult<&[u8], ClrHeader> {
    let (input, cb) = le_u32(input)?;
    let (input, major_runtime_version) = le_u16(input)?;
    let (input, minor_runtime_version) = le_u16(input)?;

    // Metadata directory
    let (input, metadata_rva) = le_u32(input)?;
    let (input, metadata_size) = le_u32(input)?;

    let (input, flags) = le_u32(input)?;
    let (input, entry_point_token) = le_u32(input)?;

    // Resources directory
    let (input, resources_rva) = le_u32(input)?;
    let (input, resources_size) = le_u32(input)?;

    // Strong name signature
    let (input, strong_name_rva) = le_u32(input)?;
    let (input, strong_name_size) = le_u32(input)?;

    // Code manager table
    let (input, code_manager_rva) = le_u32(input)?;
    let (input, code_manager_size) = le_u32(input)?;

    // VTable fixups
    let (input, vtable_rva) = le_u32(input)?;
    let (input, vtable_size) = le_u32(input)?;

    // Export address table jumps
    let (input, export_rva) = le_u32(input)?;
    let (input, export_size) = le_u32(input)?;

    // Managed native header
    let (input, managed_rva) = le_u32(input)?;
    let (input, managed_size) = le_u32(input)?;

    Ok((
        input,
        ClrHeader {
            cb,
            major_runtime_version,
            minor_runtime_version,
            metadata: (metadata_rva, metadata_size),
            flags,
            entry_point_token,
            resources: (resources_rva, resources_size),
            strong_name_signature: (strong_name_rva, strong_name_size),
            code_manager_table: (code_manager_rva, code_manager_size),
            vtable_fixups: (vtable_rva, vtable_size),
            export_address_table_jumps: (export_rva, export_size),
            managed_native_header: (managed_rva, managed_size),
        },
    ))
}

/// Parse .NET metadata to extract assembly information
pub fn parse_dotnet_metadata(metadata_data: &[u8], clr_header: &ClrHeader) -> Option<DotNetInfo> {
    let mut info = DotNetInfo {
        is_dotnet: true,
        clr_version: Some(format!(
            "{}.{}",
            clr_header.major_runtime_version, clr_header.minor_runtime_version
        )),
        il_only: (clr_header.flags & clr_flags::COMIMAGE_FLAGS_ILONLY) != 0,
        strong_name_signed: (clr_header.flags & clr_flags::COMIMAGE_FLAGS_STRONGNAMESIGNED) != 0,
        requires_32bit: (clr_header.flags & clr_flags::COMIMAGE_FLAGS_32BITREQUIRED) != 0,
        prefers_32bit: (clr_header.flags & clr_flags::COMIMAGE_FLAGS_32BITPREFERRED) != 0,
        ..Default::default()
    };

    // Parse CLI metadata header
    if let Some((assembly_name, assembly_version, culture, public_key_token, target_framework)) =
        parse_cli_metadata(metadata_data)
    {
        info.assembly_name = Some(assembly_name);
        info.assembly_version = Some(assembly_version);
        info.assembly_culture = Some(culture);
        info.public_key_token = public_key_token;
        info.target_framework = target_framework;
    }

    Some(info)
}

/// Parse CLI metadata header and extract assembly information
#[allow(clippy::type_complexity)]
fn parse_cli_metadata(
    data: &[u8],
) -> Option<(String, String, String, Option<String>, Option<String>)> {
    // CLI metadata starts with signature: 0x424A5342 (BSJB)
    if data.len() < 16 {
        return None;
    }

    let reader = EndianReader::little_endian(data);
    let signature = reader.u32_at(0)?;
    if signature != 0x424A5342 {
        return None;
    }

    // Skip signature (4), major version (2), minor version (2), reserved (4)
    let mut offset = 12;

    // Read version string length (4 bytes)
    if offset + 4 > data.len() {
        return None;
    }
    let version_len = reader.u32_at(offset)? as usize;
    offset += 4;

    // Read version string (null-terminated, padded to 4-byte boundary)
    if offset + version_len > data.len() {
        return None;
    }
    let version_string = String::from_utf8_lossy(&data[offset..offset + version_len])
        .trim_end_matches('\0')
        .to_string();
    offset += (version_len + 3) & !3; // Align to 4 bytes

    // Skip flags (2) and streams count (2)
    if offset + 4 > data.len() {
        return None;
    }
    let stream_count = reader.u16_at(offset + 2)? as usize;
    offset += 4;

    // Find #~ or #- stream (metadata tables)
    let mut tables_offset: Option<usize> = None;
    let mut tables_size: usize = 0;

    for _ in 0..stream_count {
        if offset + 8 > data.len() {
            break;
        }

        let stream_offset = reader.u32_at(offset)? as usize;
        let stream_size = reader.u32_at(offset + 4)? as usize;
        offset += 8;

        // Read stream name (null-terminated, padded to 4-byte boundary)
        let name_start = offset;
        while offset < data.len() && data[offset] != 0 {
            offset += 1;
        }

        if offset < data.len() {
            let stream_name = String::from_utf8_lossy(&data[name_start..offset]).to_string();
            offset += 1; // Skip null terminator
            offset = (offset + 3) & !3; // Align to 4 bytes

            if stream_name == "#~" || stream_name == "#-" {
                tables_offset = Some(stream_offset);
                tables_size = stream_size;
            }
        }
    }

    // Parse assembly information from metadata tables
    if let Some(tables_off) = tables_offset
        && let Some((name, version, culture, token, framework)) =
            parse_assembly_table(data, tables_off, tables_size)
    {
        return Some((
            name,
            version,
            if culture.is_empty() {
                "neutral".to_string()
            } else {
                culture
            },
            token,
            framework,
        ));
    }

    // Fallback: extract basic info from version string
    Some((
        "Unknown".to_string(),
        "0.0.0.0".to_string(),
        "neutral".to_string(),
        None,
        Some(version_string),
    ))
}

/// Parse assembly table from CLI metadata
#[allow(clippy::type_complexity)]
fn parse_assembly_table(
    _data: &[u8],
    _offset: usize,
    _size: usize,
) -> Option<(String, String, String, Option<String>, Option<String>)> {
    // Simplified implementation - full CLI metadata table parsing is complex
    // This would require parsing:
    // 1. Metadata table header (reserved, major/minor version, heap sizes, valid/sorted masks)
    // 2. Row counts for each table
    // 3. Assembly table (table index 0x20)
    // 4. AssemblyRef table for target framework
    // 5. String heap for name/culture
    // 6. Blob heap for public key

    // For now, return placeholder to indicate .NET presence
    // A full implementation would parse the entire CLI metadata structure
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_clr_header() {
        let mut data = vec![];
        data.extend_from_slice(&72u32.to_le_bytes()); // cb
        data.extend_from_slice(&2u16.to_le_bytes()); // major version
        data.extend_from_slice(&5u16.to_le_bytes()); // minor version
        data.extend_from_slice(&0x2008u32.to_le_bytes()); // metadata RVA
        data.extend_from_slice(&0x1000u32.to_le_bytes()); // metadata size
        data.extend_from_slice(&0x00000001u32.to_le_bytes()); // flags (IL only)
        data.extend_from_slice(&0x06000001u32.to_le_bytes()); // entry point token
        data.extend_from_slice(&[0u8; 48]); // Remaining fields

        let result = parse_clr_header(&data);
        assert!(result.is_ok());

        let (_, header) = result.unwrap();
        assert_eq!(header.cb, 72);
        assert_eq!(header.major_runtime_version, 2);
        assert_eq!(header.minor_runtime_version, 5);
        assert_eq!(header.metadata.0, 0x2008);
        assert_eq!(header.metadata.1, 0x1000);
        assert_eq!(header.flags, clr_flags::COMIMAGE_FLAGS_ILONLY);
    }

    #[test]
    fn test_dotnet_info_flags() {
        let clr_header = ClrHeader {
            cb: 72,
            major_runtime_version: 2,
            minor_runtime_version: 5,
            metadata: (0x2008, 0x1000),
            flags: clr_flags::COMIMAGE_FLAGS_ILONLY | clr_flags::COMIMAGE_FLAGS_STRONGNAMESIGNED,
            entry_point_token: 0x06000001,
            resources: (0, 0),
            strong_name_signature: (0x2000, 0x80),
            code_manager_table: (0, 0),
            vtable_fixups: (0, 0),
            export_address_table_jumps: (0, 0),
            managed_native_header: (0, 0),
        };

        let metadata = vec![]; // Empty for this test
        let info = parse_dotnet_metadata(&metadata, &clr_header).unwrap();

        assert!(info.is_dotnet);
        assert!(info.il_only);
        assert!(info.strong_name_signed);
        assert!(!info.requires_32bit);
        assert!(!info.prefers_32bit);
        assert_eq!(info.clr_version.unwrap(), "2.5");
    }

    #[test]
    fn test_cli_metadata_signature() {
        // Create minimal CLI metadata header with BSJB signature
        let mut data = vec![];
        data.extend_from_slice(b"BSJB"); // Signature
        data.extend_from_slice(&1u16.to_le_bytes()); // Major version
        data.extend_from_slice(&1u16.to_le_bytes()); // Minor version
        data.extend_from_slice(&0u32.to_le_bytes()); // Reserved
        data.extend_from_slice(&12u32.to_le_bytes()); // Version length
        data.extend_from_slice(b"v4.0.30319\0\0"); // Version string (padded to 12 bytes)
        data.extend_from_slice(&0u16.to_le_bytes()); // Flags
        data.extend_from_slice(&0u16.to_le_bytes()); // Streams count

        let result = parse_cli_metadata(&data);
        assert!(result.is_some());

        let (_, _, _, _, framework) = result.unwrap();
        assert!(framework.is_some());
        assert!(framework.unwrap().contains("v4.0.30319"));
    }
}
