//! PE Resource Directory Parser

use crate::parsers::pe::structures::{
    ResourceDataEntry, ResourceDirectory, ResourceDirectoryEntry,
};
use nom::{
    IResult,
    number::complete::{le_u16, le_u32},
};

/// Parse Resource Directory structure (16 bytes)
pub fn parse_resource_directory(input: &[u8]) -> IResult<&[u8], ResourceDirectory> {
    let (input, characteristics) = le_u32(input)?;
    let (input, time_date_stamp) = le_u32(input)?;
    let (input, major_version) = le_u16(input)?;
    let (input, minor_version) = le_u16(input)?;
    let (input, number_of_name_entries) = le_u16(input)?;
    let (input, number_of_id_entries) = le_u16(input)?;

    Ok((
        input,
        ResourceDirectory {
            characteristics,
            time_date_stamp,
            major_version,
            minor_version,
            number_of_name_entries,
            number_of_id_entries,
        },
    ))
}

/// Parse Resource Directory Entry (8 bytes)
pub fn parse_resource_directory_entry(input: &[u8]) -> IResult<&[u8], ResourceDirectoryEntry> {
    let (input, name_id) = le_u32(input)?;
    let (input, data_offset) = le_u32(input)?;

    Ok((
        input,
        ResourceDirectoryEntry {
            name_id,
            data_offset,
        },
    ))
}

/// Parse Resource Data Entry (16 bytes)
pub fn parse_resource_data_entry(input: &[u8]) -> IResult<&[u8], ResourceDataEntry> {
    let (input, data_rva) = le_u32(input)?;
    let (input, size) = le_u32(input)?;
    let (input, codepage) = le_u32(input)?;
    let (input, reserved) = le_u32(input)?;

    Ok((
        input,
        ResourceDataEntry {
            data_rva,
            size,
            codepage,
            reserved,
        },
    ))
}

/// Find a resource by type and ID in the resource directory tree
pub fn find_resource_data(
    rsrc_data: &[u8],
    _rsrc_base_offset: u64,
    resource_type: u32,
    resource_id: Option<u32>,
) -> Option<(u64, u32)> {
    // Parse root directory
    let (mut input, root_dir) = parse_resource_directory(rsrc_data).ok()?;

    // Skip name entries, look through ID entries
    for _ in 0..root_dir.number_of_name_entries {
        if input.len() < 8 {
            return None;
        }
        input = &input[8..];
    }

    // Find the resource type
    for _ in 0..root_dir.number_of_id_entries {
        let (rest, entry) = parse_resource_directory_entry(input).ok()?;
        input = rest;

        // Check if this is the resource type we're looking for
        if entry.name_id == resource_type {
            // Follow the subdirectory offset
            let is_subdir = (entry.data_offset & 0x80000000) != 0;
            if !is_subdir {
                return None;
            }

            let subdir_offset = (entry.data_offset & 0x7FFFFFFF) as usize;

            return find_resource_by_id(rsrc_data, subdir_offset, _rsrc_base_offset, resource_id);
        }
    }

    None
}

/// Find resource data by ID in a subdirectory
fn find_resource_by_id(
    rsrc_data: &[u8],
    subdir_offset: usize,
    _rsrc_base_offset: u64,
    resource_id: Option<u32>,
) -> Option<(u64, u32)> {
    if subdir_offset >= rsrc_data.len() {
        return None;
    }

    let subdir_data = &rsrc_data[subdir_offset..];
    let (mut input, subdir) = parse_resource_directory(subdir_data).ok()?;

    // Skip name entries
    for _ in 0..subdir.number_of_name_entries {
        if input.len() < 8 {
            return None;
        }
        input = &input[8..];
    }

    // Look through ID entries
    for _ in 0..subdir.number_of_id_entries {
        let (rest, entry) = parse_resource_directory_entry(input).ok()?;
        input = rest;

        // If resource_id specified, match it; otherwise take first
        if resource_id.is_none() || Some(entry.name_id) == resource_id {
            let is_subdir = (entry.data_offset & 0x80000000) != 0;
            if !is_subdir {
                return None;
            }

            let lang_subdir_offset = (entry.data_offset & 0x7FFFFFFF) as usize;

            // One more level - language subdirectory
            // Offset is relative to start of resource section, so use rsrc_data directly
            return find_first_language_resource(rsrc_data, lang_subdir_offset, _rsrc_base_offset);
        }
    }

    None
}

/// Find first language variant of a resource
fn find_first_language_resource(
    rsrc_data: &[u8],
    lang_dir_offset: usize,
    _rsrc_base_offset: u64,
) -> Option<(u64, u32)> {
    if lang_dir_offset >= rsrc_data.len() {
        return None;
    }

    let lang_dir_data = &rsrc_data[lang_dir_offset..];
    let (mut input, lang_dir) = parse_resource_directory(lang_dir_data).ok()?;

    // Skip name entries
    for _ in 0..lang_dir.number_of_name_entries {
        if input.len() < 8 {
            return None;
        }
        input = &input[8..];
    }

    // Get first ID entry (first language)
    if lang_dir.number_of_id_entries > 0 {
        let (_, entry) = parse_resource_directory_entry(input).ok()?;

        // This should point to data entry
        let is_subdir = (entry.data_offset & 0x80000000) != 0;
        if is_subdir {
            return None;
        }

        let data_entry_offset = entry.data_offset as usize;
        if data_entry_offset >= rsrc_data.len() {
            return None;
        }

        let (_, data_entry) = parse_resource_data_entry(&rsrc_data[data_entry_offset..]).ok()?;

        // Return RVA and size
        Some((data_entry.data_rva as u64, data_entry.size))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_directory_offsets() {
        // Create a minimal resource directory structure to test offset calculations
        // This simulates the bug we fixed: offsets are relative to start of rsrc_data,
        // not relative to subdirectories

        let mut rsrc_data = Vec::new();

        // Root directory at offset 0 (Type level)
        rsrc_data.extend_from_slice(&0u32.to_le_bytes()); // characteristics
        rsrc_data.extend_from_slice(&0u32.to_le_bytes()); // time_date_stamp
        rsrc_data.extend_from_slice(&0u16.to_le_bytes()); // major_version
        rsrc_data.extend_from_slice(&0u16.to_le_bytes()); // minor_version
        rsrc_data.extend_from_slice(&0u16.to_le_bytes()); // number_of_name_entries
        rsrc_data.extend_from_slice(&1u16.to_le_bytes()); // number_of_id_entries (1 type)

        // Root directory entry for RT_VERSION (type 16)
        rsrc_data.extend_from_slice(&16u32.to_le_bytes()); // name_id = 16 (RT_VERSION)
        rsrc_data.extend_from_slice(&0x80000030u32.to_le_bytes()); // data_offset = subdir at 0x30

        // Pad to offset 0x30 (48 bytes)
        while rsrc_data.len() < 0x30 {
            rsrc_data.push(0);
        }

        // ID subdirectory at offset 0x30
        rsrc_data.extend_from_slice(&0u32.to_le_bytes()); // characteristics
        rsrc_data.extend_from_slice(&0u32.to_le_bytes()); // time_date_stamp
        rsrc_data.extend_from_slice(&0u16.to_le_bytes()); // major_version
        rsrc_data.extend_from_slice(&0u16.to_le_bytes()); // minor_version
        rsrc_data.extend_from_slice(&0u16.to_le_bytes()); // number_of_name_entries
        rsrc_data.extend_from_slice(&1u16.to_le_bytes()); // number_of_id_entries (1 ID)

        // ID directory entry
        rsrc_data.extend_from_slice(&1u32.to_le_bytes()); // name_id = 1
        rsrc_data.extend_from_slice(&0x80000060u32.to_le_bytes()); // data_offset = subdir at 0x60

        // Pad to offset 0x60 (96 bytes)
        while rsrc_data.len() < 0x60 {
            rsrc_data.push(0);
        }

        // Language subdirectory at offset 0x60
        rsrc_data.extend_from_slice(&0u32.to_le_bytes()); // characteristics
        rsrc_data.extend_from_slice(&0u32.to_le_bytes()); // time_date_stamp
        rsrc_data.extend_from_slice(&0u16.to_le_bytes()); // major_version
        rsrc_data.extend_from_slice(&0u16.to_le_bytes()); // minor_version
        rsrc_data.extend_from_slice(&0u16.to_le_bytes()); // number_of_name_entries
        rsrc_data.extend_from_slice(&1u16.to_le_bytes()); // number_of_id_entries (1 language)

        // Language directory entry
        rsrc_data.extend_from_slice(&1033u32.to_le_bytes()); // name_id = 1033 (English)
        rsrc_data.extend_from_slice(&0x00000090u32.to_le_bytes()); // data_offset = data entry at 0x90

        // Pad to offset 0x90 (144 bytes)
        while rsrc_data.len() < 0x90 {
            rsrc_data.push(0);
        }

        // Resource data entry at offset 0x90
        rsrc_data.extend_from_slice(&0x10000u32.to_le_bytes()); // data_rva = 0x10000
        rsrc_data.extend_from_slice(&100u32.to_le_bytes()); // size = 100
        rsrc_data.extend_from_slice(&0u32.to_le_bytes()); // codepage
        rsrc_data.extend_from_slice(&0u32.to_le_bytes()); // reserved

        // Test that we can find the resource
        let result = find_resource_data(&rsrc_data, 0, 16, None);
        assert!(result.is_some(), "Should find resource type 16");

        let (rva, size) = result.unwrap();
        assert_eq!(rva, 0x10000, "RVA should be 0x10000");
        assert_eq!(size, 100, "Size should be 100");
    }
}
