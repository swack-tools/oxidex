//! PE (Portable Executable) format parser
//!
//! This module provides parsing for PE files (.exe, .dll, .sys),
//! extracting metadata from DOS headers, COFF headers, Optional headers,
//! and section information.

#![allow(dead_code)]

pub mod anomaly_detector;
pub mod clr_parser;
pub mod coff_parser;
pub mod debug_parser;
pub mod dos_parser;
pub mod export_parser;
pub mod import_parser;
pub mod metadata_extractor;
pub mod optional_parser;
pub mod resource_parser;
pub mod rich_header_parser;
pub mod section_parser;
pub mod signature_parser;
pub mod structures;
pub mod version_info_parser;

use crate::core::{FileReader, MetadataMap};
use crate::error::{ExifToolError, Result};

/// PE signature/magic bytes
const PE_SIGNATURE: &[u8] = b"PE\0\0";
const DOS_SIGNATURE: &[u8] = b"MZ";

/// Parses PE file and extracts all metadata.
pub fn parse_pe_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    use clr_parser::{parse_clr_header, parse_dotnet_metadata};
    use coff_parser::parse_coff_header;
    use debug_parser::parse_debug_directory_entry;
    use dos_parser::parse_dos_header;
    use export_parser::parse_exports;
    use import_parser::{parse_dll_imports, parse_import_descriptor};
    use metadata_extractor::{
        extract_coff_metadata, extract_dos_metadata, extract_dotnet_metadata,
        extract_export_metadata, extract_import_metadata, extract_nb10_metadata,
        extract_optional_metadata, extract_rich_header_metadata, extract_rsds_metadata,
        extract_signature_metadata, extract_version_info_metadata,
    };
    use optional_parser::{parse_optional_header_nt, parse_optional_header_standard};
    use resource_parser::find_resource_data;
    use rich_header_parser::parse_rich_header;
    use section_parser::parse_section_table;
    use signature_parser::{parse_signature_info, parse_win_certificate};
    use structures::{debug_types, resource_types};
    use version_info_parser::parse_version_info;

    let mut metadata = MetadataMap::new();

    // Step 1: Parse DOS header (first 64 bytes)
    let dos_data = reader.read(0, 64)?;
    let (_, dos_header) = parse_dos_header(dos_data)
        .map_err(|e| ExifToolError::parse_error(format!("Failed to parse DOS header: {:?}", e)))?;

    // Verify DOS signature
    if dos_header.e_magic != 0x5A4D {
        return Err(ExifToolError::parse_error(
            "Invalid DOS signature (expected MZ)",
        ));
    }

    extract_dos_metadata(&dos_header, &mut metadata);

    // Step 1.5: Parse Rich Header (between DOS stub and PE header)
    // Rich Header typically starts after DOS header (0x80) and ends before PE signature
    let pe_offset = dos_header.e_lfanew as u64;
    if pe_offset > 0x80 {
        // Read data between DOS stub and PE header for Rich Header parsing
        let rich_region_size = (pe_offset - 0x80) as usize + 128;
        if let Ok(rich_data) = reader.read(0, 0x80 + rich_region_size)
            && let Some(rich_header) = parse_rich_header(rich_data, 0x80, pe_offset as usize)
        {
            extract_rich_header_metadata(&rich_header, &mut metadata);
        }
        // Note: If Rich Header parsing fails, we silently continue (not all PE files have it)
    }

    // Step 2: Parse COFF header at e_lfanew offset
    // We need to read PE signature (4) + COFF header (20) + Optional header (variable)
    // Read up to 512 bytes to cover typical optional headers
    let pe_offset = dos_header.e_lfanew as u64;
    let pe_data = reader.read(pe_offset, 512)?;
    let (remaining, coff_header) = parse_coff_header(pe_data)
        .map_err(|e| ExifToolError::parse_error(format!("Failed to parse COFF header: {:?}", e)))?;

    extract_coff_metadata(&coff_header, &mut metadata);

    // Step 3: Parse Optional Header if present
    // Store NT header and format info for later use
    let resource_dir_rva: Option<u32>;
    let nt_header_opt: Option<structures::OptionalHeaderNT>;
    let is_pe32_plus: bool;
    if coff_header.size_of_optional_header > 0 {
        let (opt_remaining, std_header) =
            parse_optional_header_standard(remaining).map_err(|e| {
                ExifToolError::parse_error(format!("Failed to parse Optional Header: {:?}", e))
            })?;

        is_pe32_plus = std_header.magic == 0x020B;
        let (_, nt_header) =
            parse_optional_header_nt(opt_remaining, is_pe32_plus).map_err(|e| {
                ExifToolError::parse_error(format!(
                    "Failed to parse Optional Header NT fields: {:?}",
                    e
                ))
            })?;

        extract_optional_metadata(&std_header, &nt_header, &mut metadata);

        // Extract resource directory RVA from data directories (index 2)
        resource_dir_rva = nt_header.data_directories.get(2).map(|(rva, _size)| *rva);

        // Store nt_header for debug directory parsing
        nt_header_opt = Some(nt_header);
    } else {
        resource_dir_rva = None;
        nt_header_opt = None;
        is_pe32_plus = false;
    }

    // Step 4: Calculate section table offset
    // Section table starts after PE signature + COFF header + Optional header
    // PE signature = 4 bytes, COFF header = 20 bytes
    let section_table_offset = pe_offset + 4 + 20 + coff_header.size_of_optional_header as u64;

    // Step 5: Parse section table
    let section_table_size = (coff_header.number_of_sections as u64) * 40; // Each section header is 40 bytes
    let section_data = reader.read(section_table_offset, section_table_size as usize)?;
    let (_, sections) =
        parse_section_table(section_data, coff_header.number_of_sections).map_err(|e| {
            ExifToolError::parse_error(format!("Failed to parse section table: {:?}", e))
        })?;

    // Step 6: Find .rsrc section
    if let Some(_resource_dir_rva) = resource_dir_rva
        && let Some(rsrc_section) = sections.iter().find(|s| s.name_str() == ".rsrc")
    {
        // Step 7: Read resource section data
        let rsrc_file_offset = rsrc_section.pointer_to_raw_data as u64;
        let rsrc_size = rsrc_section.size_of_raw_data as usize;
        let rsrc_data = reader.read(rsrc_file_offset, rsrc_size)?;

        // Step 8: Find VERSION_INFO resource (type 16)
        if let Some((version_rva, version_size)) = find_resource_data(
            rsrc_data,
            rsrc_file_offset,
            resource_types::RT_VERSION,
            None,
        ) {
            // Convert RVA to file offset
            // RVA is relative to image base, need to convert using section info
            let version_offset_in_section = version_rva - rsrc_section.virtual_address as u64;
            let version_file_offset = rsrc_file_offset + version_offset_in_section;

            // Step 9: Read and parse VERSION_INFO data
            let version_data = reader.read(version_file_offset, version_size as usize)?;
            if let Some((fixed_info, strings)) = parse_version_info(version_data) {
                // Step 10: Extract VERSION_INFO metadata
                extract_version_info_metadata(&fixed_info, &strings, &mut metadata);
            }
        }
    }

    // Step 11: Parse debug directory if present (data directory index 6)
    if let Some(ref nt_header) = nt_header_opt
        && let Some(&(debug_rva, debug_size)) = nt_header.data_directories.get(6)
        && debug_rva > 0
        && debug_size > 0
    {
        // Find section containing debug directory
        if let Some(debug_section) = sections.iter().find(|s| {
            debug_rva >= s.virtual_address && debug_rva < s.virtual_address + s.virtual_size
        }) {
            let debug_offset = debug_section.pointer_to_raw_data as u64
                + (debug_rva - debug_section.virtual_address) as u64;

            let debug_data = reader.read(debug_offset, debug_size as usize)?;

            // Parse debug directory entries
            let mut offset = 0;
            while offset + 28 <= debug_size as usize {
                if let Ok((_, entry)) = parse_debug_directory_entry(&debug_data[offset..]) {
                    // Check for CodeView debug info
                    if entry.debug_type == debug_types::IMAGE_DEBUG_TYPE_CODEVIEW
                        && entry.pointer_to_raw_data > 0
                        && entry.size_of_data > 0
                    {
                        let cv_data = reader.read(
                            entry.pointer_to_raw_data as u64,
                            entry.size_of_data as usize,
                        )?;

                        use debug_parser::{parse_codeview_nb10, parse_codeview_rsds};

                        // Try RSDS first (newer format)
                        if let Some(rsds) = parse_codeview_rsds(cv_data) {
                            extract_rsds_metadata(&rsds, &mut metadata);
                        } else if let Some(nb10) = parse_codeview_nb10(cv_data) {
                            extract_nb10_metadata(&nb10, &mut metadata);
                        }
                        break;
                    }
                    offset += 28;
                } else {
                    break;
                }
            }
        }
    }

    // Step 12: Parse export directory if present (data directory index 0)
    if let Some(ref nt_header) = nt_header_opt
        && let Some(&(export_rva, export_size)) = nt_header.data_directories.first()
        && export_rva > 0
        && export_size > 0
    {
        // Parse exports (pass sections for RVA resolution)
        if let Ok(export_info) = parse_exports(reader, export_rva, export_size, &sections) {
            extract_export_metadata(&export_info, &mut metadata);
        }
    }

    // Step 13: Parse import directory if present (data directory index 1)
    if let Some(ref nt_header) = nt_header_opt
        && let Some(&(import_rva, import_size)) = nt_header.data_directories.get(1)
        && import_rva > 0
    {
        // Find section containing import directory
        if let Some(import_section) = sections.iter().find(|s| {
            import_rva >= s.virtual_address && import_rva < s.virtual_address + s.virtual_size
        }) {
            let import_offset = import_section.pointer_to_raw_data as u64
                + (import_rva - import_section.virtual_address) as u64;

            // Read import directory (use directory size from PE header, or limit to 100 descriptors max)
            let max_descriptors = 100;
            // Use the actual import directory size if available, otherwise estimate
            let import_data_size = if import_size > 0 && import_size < 20 * max_descriptors as u32 {
                import_size as usize
            } else {
                20 * max_descriptors
            };

            if let Ok(import_data) = reader.read(import_offset, import_data_size) {
                let mut imports = Vec::new();
                let mut offset = 0;

                // Parse import descriptors until we hit a null descriptor
                while offset + 20 <= import_data.len() && imports.len() < max_descriptors {
                    if let Ok((_, descriptor)) = parse_import_descriptor(&import_data[offset..]) {
                        if descriptor.is_null() {
                            break;
                        }

                        // Parse imports for this DLL (limit to 100 functions per DLL)
                        if let Some(import_info) =
                            parse_dll_imports(reader, &descriptor, &sections, is_pe32_plus, 100)
                        {
                            imports.push(import_info);
                        }

                        offset += 20;
                    } else {
                        break;
                    }
                }

                // Extract import metadata
                if !imports.is_empty() {
                    extract_import_metadata(&imports, &mut metadata);
                }
            }
        }
    }

    // Step 14: Parse digital signature if present (data directory index 4 - Security)
    // NOTE: This is a FILE offset, not an RVA (unlike other data directories)
    if let Some(ref nt_header) = nt_header_opt
        && let Some(&(cert_offset, cert_size)) = nt_header.data_directories.get(4)
        && cert_offset > 0
        && cert_size > 0
    {
        // Read certificate data from file offset
        if let Ok(cert_data) = reader.read(cert_offset as u64, cert_size as usize) {
            // Parse WIN_CERTIFICATE structure
            if let Ok((_, win_cert)) = parse_win_certificate(cert_data) {
                // Extract signature information from PKCS#7 data
                if let Some(sig_info) = parse_signature_info(&win_cert.certificate_data) {
                    extract_signature_metadata(&sig_info, &mut metadata);
                }
            }
        }
    }

    // Step 15: Parse .NET CLR header if present (data directory index 14)
    if let Some(ref nt_header) = nt_header_opt
        && let Some(&(clr_rva, clr_size)) = nt_header.data_directories.get(14)
        && clr_rva > 0
        && clr_size > 0
    {
        // Find section containing CLR header
        if let Some(clr_section) = sections
            .iter()
            .find(|s| clr_rva >= s.virtual_address && clr_rva < s.virtual_address + s.virtual_size)
        {
            let clr_offset = clr_section.pointer_to_raw_data as u64
                + (clr_rva - clr_section.virtual_address) as u64;

            // Read CLR header (72 bytes)
            if let Ok(clr_data) = reader.read(clr_offset, 72)
                && let Ok((_, clr_header)) = parse_clr_header(clr_data)
            {
                // Parse .NET metadata if present
                let (metadata_rva, metadata_size) = clr_header.metadata;
                if metadata_rva > 0 && metadata_size > 0 {
                    // Find section containing metadata
                    if let Some(metadata_section) = sections.iter().find(|s| {
                        metadata_rva >= s.virtual_address
                            && metadata_rva < s.virtual_address + s.virtual_size
                    }) {
                        let metadata_offset = metadata_section.pointer_to_raw_data as u64
                            + (metadata_rva - metadata_section.virtual_address) as u64;

                        // Read metadata (limit to 64KB for safety)
                        let metadata_read_size = std::cmp::min(metadata_size as usize, 65536);
                        if let Ok(metadata_data) = reader.read(metadata_offset, metadata_read_size)
                            && let Some(dotnet_info) =
                                parse_dotnet_metadata(metadata_data, &clr_header)
                        {
                            extract_dotnet_metadata(&dotnet_info, &mut metadata);
                        }
                    }
                }
            }
        }
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pe_metadata_placeholder() {
        // This test will be replaced with real tests
        let metadata = MetadataMap::new();
        assert_eq!(metadata.len(), 0);
    }
}
