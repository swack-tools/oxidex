//! Mach-O header parser
//!
//! This module handles parsing of Mach-O headers for both 32-bit and 64-bit
//! formats, as well as FAT/Universal binary headers.

use crate::io::EndianReader;
use nom::{
    IResult,
    number::complete::{be_i32, be_u32, le_i32, le_u32},
};

use super::structures::{FatArch, FatHeader, MachHeader, magic};

// =============================================================================
// Magic Number Detection
// =============================================================================

/// Determines if the data starts with a valid Mach-O or FAT binary magic number
pub fn is_macho_magic(data: &[u8]) -> bool {
    let reader = EndianReader::big_endian(data);
    let Some(magic) = reader.u32_at(0) else {
        return false;
    };
    matches!(
        magic,
        magic::MH_MAGIC
            | magic::MH_MAGIC_64
            | magic::MH_CIGAM
            | magic::MH_CIGAM_64
            | magic::FAT_MAGIC
            | magic::FAT_CIGAM
            | magic::FAT_MAGIC_64
            | magic::FAT_CIGAM_64
    )
}

/// Determines if the data starts with a FAT binary magic number
pub fn is_fat_magic(data: &[u8]) -> bool {
    let reader = EndianReader::big_endian(data);
    let Some(magic) = reader.u32_at(0) else {
        return false;
    };
    matches!(
        magic,
        magic::FAT_MAGIC | magic::FAT_CIGAM | magic::FAT_MAGIC_64 | magic::FAT_CIGAM_64
    )
}

/// Reads magic number from data as big-endian (always read as BE first)
fn read_magic(data: &[u8]) -> Option<u32> {
    let reader = EndianReader::big_endian(data);
    reader.u32_at(0)
}

// =============================================================================
// Mach-O Header Parsing
// =============================================================================

/// Parse a Mach-O header (32-bit or 64-bit, native or swapped byte order)
///
/// Returns the parsed header and the remaining input.
pub fn parse_mach_header(input: &[u8]) -> IResult<&[u8], MachHeader> {
    // First, read the magic number to determine format and byte order
    let magic_val = read_magic(input).ok_or_else(|| {
        nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::TooLarge,
        ))
    })?;

    // Determine format characteristics from magic
    let (is_64bit, is_swapped) = match magic_val {
        magic::MH_MAGIC => (false, false),
        magic::MH_MAGIC_64 => (true, false),
        magic::MH_CIGAM => (false, true),
        magic::MH_CIGAM_64 => (true, true),
        _ => {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            )));
        }
    };

    // Parse based on byte order
    // is_swapped=true means CIGAM magic was seen - file is little-endian
    // is_swapped=false means MAGIC was seen - file is big-endian (native PPC order)
    if is_swapped {
        parse_mach_header_le(input, is_64bit)
    } else {
        parse_mach_header_be(input, is_64bit)
    }
}

/// Parse Mach-O header in little-endian byte order (CIGAM files - swapped from original BE)
fn parse_mach_header_le(input: &[u8], is_64bit: bool) -> IResult<&[u8], MachHeader> {
    let (input, magic) = le_u32(input)?;
    let (input, cputype) = le_i32(input)?;
    let (input, cpusubtype) = le_i32(input)?;
    let (input, filetype) = le_u32(input)?;
    let (input, ncmds) = le_u32(input)?;
    let (input, sizeofcmds) = le_u32(input)?;
    let (input, flags) = le_u32(input)?;

    // 64-bit has an additional reserved field
    let (input, reserved) = if is_64bit { le_u32(input)? } else { (input, 0) };

    Ok((
        input,
        MachHeader {
            magic,
            cputype,
            cpusubtype,
            filetype,
            ncmds,
            sizeofcmds,
            flags,
            reserved,
            is_64bit,
            is_swapped: true, // LE = CIGAM = swapped from original BE
        },
    ))
}

/// Parse Mach-O header in big-endian byte order (MAGIC files - native original PPC order)
fn parse_mach_header_be(input: &[u8], is_64bit: bool) -> IResult<&[u8], MachHeader> {
    let (input, magic) = be_u32(input)?;
    let (input, cputype) = be_i32(input)?;
    let (input, cpusubtype) = be_i32(input)?;
    let (input, filetype) = be_u32(input)?;
    let (input, ncmds) = be_u32(input)?;
    let (input, sizeofcmds) = be_u32(input)?;
    let (input, flags) = be_u32(input)?;

    // 64-bit has an additional reserved field
    let (input, reserved) = if is_64bit { be_u32(input)? } else { (input, 0) };

    Ok((
        input,
        MachHeader {
            magic,
            cputype,
            cpusubtype,
            filetype,
            ncmds,
            sizeofcmds,
            flags,
            reserved,
            is_64bit,
            is_swapped: true,
        },
    ))
}

// =============================================================================
// FAT Binary Header Parsing
// =============================================================================

/// Parse a FAT (Universal) binary header
pub fn parse_fat_header(input: &[u8]) -> IResult<&[u8], FatHeader> {
    // Read magic to determine byte order and format
    let magic_val = read_magic(input).ok_or_else(|| {
        nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::TooLarge,
        ))
    })?;

    let (is_64bit, is_swapped) = match magic_val {
        magic::FAT_MAGIC => (false, false),
        magic::FAT_MAGIC_64 => (true, false),
        magic::FAT_CIGAM => (false, true),
        magic::FAT_CIGAM_64 => (true, true),
        _ => {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            )));
        }
    };

    // FAT headers are always in big-endian, but CIGAM variants are swapped to little-endian
    if is_swapped {
        // Little-endian (unusual but possible)
        let (input, magic) = le_u32(input)?;
        let (input, nfat_arch) = le_u32(input)?;
        Ok((
            input,
            FatHeader {
                magic,
                nfat_arch,
                is_64bit,
                is_swapped,
            },
        ))
    } else {
        // Big-endian (standard FAT format)
        let (input, magic) = be_u32(input)?;
        let (input, nfat_arch) = be_u32(input)?;
        Ok((
            input,
            FatHeader {
                magic,
                nfat_arch,
                is_64bit,
                is_swapped,
            },
        ))
    }
}

/// Parse a FAT architecture entry (32-bit version)
pub fn parse_fat_arch_32(input: &[u8], is_swapped: bool) -> IResult<&[u8], FatArch> {
    if is_swapped {
        let (input, cputype) = le_i32(input)?;
        let (input, cpusubtype) = le_i32(input)?;
        let (input, offset) = le_u32(input)?;
        let (input, size) = le_u32(input)?;
        let (input, align) = le_u32(input)?;
        Ok((
            input,
            FatArch {
                cputype,
                cpusubtype,
                offset: offset as u64,
                size: size as u64,
                align,
            },
        ))
    } else {
        let (input, cputype) = be_i32(input)?;
        let (input, cpusubtype) = be_i32(input)?;
        let (input, offset) = be_u32(input)?;
        let (input, size) = be_u32(input)?;
        let (input, align) = be_u32(input)?;
        Ok((
            input,
            FatArch {
                cputype,
                cpusubtype,
                offset: offset as u64,
                size: size as u64,
                align,
            },
        ))
    }
}

/// Parse a FAT architecture entry (64-bit version)
pub fn parse_fat_arch_64(input: &[u8], is_swapped: bool) -> IResult<&[u8], FatArch> {
    use nom::number::complete::{be_u64, le_u64};

    if is_swapped {
        let (input, cputype) = le_i32(input)?;
        let (input, cpusubtype) = le_i32(input)?;
        let (input, offset) = le_u64(input)?;
        let (input, size) = le_u64(input)?;
        let (input, align) = le_u32(input)?;
        let (input, _reserved) = le_u32(input)?; // Reserved field in 64-bit
        Ok((
            input,
            FatArch {
                cputype,
                cpusubtype,
                offset,
                size,
                align,
            },
        ))
    } else {
        let (input, cputype) = be_i32(input)?;
        let (input, cpusubtype) = be_i32(input)?;
        let (input, offset) = be_u64(input)?;
        let (input, size) = be_u64(input)?;
        let (input, align) = be_u32(input)?;
        let (input, _reserved) = be_u32(input)?;
        Ok((
            input,
            FatArch {
                cputype,
                cpusubtype,
                offset,
                size,
                align,
            },
        ))
    }
}

/// Parse all FAT architecture entries
pub fn parse_fat_archs(
    input: &[u8],
    count: u32,
    is_64bit: bool,
    is_swapped: bool,
) -> IResult<&[u8], Vec<FatArch>> {
    let mut archs = Vec::with_capacity(count as usize);
    let mut remaining = input;

    for _ in 0..count {
        let (rest, arch) = if is_64bit {
            parse_fat_arch_64(remaining, is_swapped)?
        } else {
            parse_fat_arch_32(remaining, is_swapped)?
        };
        archs.push(arch);
        remaining = rest;
    }

    Ok((remaining, archs))
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Returns the header size based on whether the file is 64-bit
pub fn header_size(is_64bit: bool) -> usize {
    if is_64bit { 32 } else { 28 }
}

/// Returns the FAT arch entry size based on whether the FAT is 64-bit
pub fn fat_arch_size(is_64bit: bool) -> usize {
    if is_64bit { 32 } else { 20 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_macho_magic() {
        // Little-endian 64-bit magic
        assert!(is_macho_magic(&[0xCF, 0xFA, 0xED, 0xFE]));
        // Big-endian 64-bit magic
        assert!(is_macho_magic(&[0xFE, 0xED, 0xFA, 0xCF]));
        // FAT magic
        assert!(is_macho_magic(&[0xCA, 0xFE, 0xBA, 0xBE]));
        // Invalid
        assert!(!is_macho_magic(&[0x00, 0x00, 0x00, 0x00]));
        // Too short
        assert!(!is_macho_magic(&[0xCF, 0xFA, 0xED]));
    }

    #[test]
    fn test_is_fat_magic() {
        assert!(is_fat_magic(&[0xCA, 0xFE, 0xBA, 0xBE]));
        assert!(is_fat_magic(&[0xBE, 0xBA, 0xFE, 0xCA]));
        assert!(!is_fat_magic(&[0xCF, 0xFA, 0xED, 0xFE]));
    }

    #[test]
    fn test_parse_mach_header_64_le() {
        // Create a valid 64-bit little-endian Mach-O header (as found on x86_64/arm64)
        // On disk, a LE Mach-O file starts with bytes [0xCF, 0xFA, 0xED, 0xFE] for 64-bit
        // When read as BE u32, this gives MH_CIGAM_64 (0xCFFAEDFE), indicating little-endian file
        let mut data = Vec::new();
        data.extend_from_slice(&magic::MH_MAGIC_64.to_le_bytes()); // magic in LE order
        data.extend_from_slice(&0x0100000Cu32.to_le_bytes()); // cputype (ARM64)
        data.extend_from_slice(&0x00000000u32.to_le_bytes()); // cpusubtype
        data.extend_from_slice(&0x00000002u32.to_le_bytes()); // filetype (EXECUTE)
        data.extend_from_slice(&0x00000010u32.to_le_bytes()); // ncmds (16)
        data.extend_from_slice(&0x00000400u32.to_le_bytes()); // sizeofcmds (1024)
        data.extend_from_slice(&0x00200085u32.to_le_bytes()); // flags
        data.extend_from_slice(&0x00000000u32.to_le_bytes()); // reserved

        let result = parse_mach_header(&data);
        assert!(result.is_ok());

        let (_, header) = result.unwrap();
        // Magic is read as little-endian, giving native MH_MAGIC_64 value
        assert_eq!(header.magic, magic::MH_MAGIC_64);
        assert!(header.is_64bit);
        assert!(header.is_swapped); // CIGAM detected = is_swapped true (LE file)
        assert_eq!(header.ncmds, 16);
        assert_eq!(header.sizeofcmds, 1024);
    }

    #[test]
    fn test_parse_mach_header_32_le() {
        // Create a valid 32-bit little-endian Mach-O header
        let mut data = Vec::new();
        data.extend_from_slice(&magic::MH_MAGIC.to_le_bytes()); // magic in LE order
        data.extend_from_slice(&0x00000007u32.to_le_bytes()); // cputype (I386)
        data.extend_from_slice(&0x00000003u32.to_le_bytes()); // cpusubtype
        data.extend_from_slice(&0x00000002u32.to_le_bytes()); // filetype (EXECUTE)
        data.extend_from_slice(&0x00000008u32.to_le_bytes()); // ncmds (8)
        data.extend_from_slice(&0x00000200u32.to_le_bytes()); // sizeofcmds (512)
        data.extend_from_slice(&0x00000085u32.to_le_bytes()); // flags

        let result = parse_mach_header(&data);
        assert!(result.is_ok());

        let (_, header) = result.unwrap();
        // Magic is read as little-endian, giving native MH_MAGIC value
        assert_eq!(header.magic, magic::MH_MAGIC);
        assert!(!header.is_64bit);
        assert!(header.is_swapped); // CIGAM detected = is_swapped true (LE file)
        assert_eq!(header.ncmds, 8);
    }

    #[test]
    fn test_parse_fat_header() {
        // Create a valid FAT header (big-endian)
        let mut data = Vec::new();
        data.extend_from_slice(&magic::FAT_MAGIC.to_be_bytes()); // magic
        data.extend_from_slice(&2u32.to_be_bytes()); // nfat_arch

        let result = parse_fat_header(&data);
        assert!(result.is_ok());

        let (_, header) = result.unwrap();
        assert_eq!(header.magic, magic::FAT_MAGIC);
        assert_eq!(header.nfat_arch, 2);
        assert!(!header.is_64bit);
        assert!(!header.is_swapped);
    }

    #[test]
    fn test_parse_fat_arch_32() {
        // Create a valid 32-bit FAT arch entry (big-endian)
        let mut data = Vec::new();
        data.extend_from_slice(&0x0100000Cu32.to_be_bytes()); // cputype (ARM64)
        data.extend_from_slice(&0x00000000u32.to_be_bytes()); // cpusubtype
        data.extend_from_slice(&0x00004000u32.to_be_bytes()); // offset
        data.extend_from_slice(&0x00010000u32.to_be_bytes()); // size
        data.extend_from_slice(&0x0000000Eu32.to_be_bytes()); // align (14 = 2^14)

        let result = parse_fat_arch_32(&data, false);
        assert!(result.is_ok());

        let (_, arch) = result.unwrap();
        assert_eq!(arch.cputype, 0x0100000C);
        assert_eq!(arch.offset, 0x4000);
        assert_eq!(arch.size, 0x10000);
    }

    #[test]
    fn test_header_size() {
        assert_eq!(header_size(false), 28);
        assert_eq!(header_size(true), 32);
    }

    #[test]
    fn test_fat_arch_size() {
        assert_eq!(fat_arch_size(false), 20);
        assert_eq!(fat_arch_size(true), 32);
    }
}
