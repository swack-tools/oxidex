//! ELF dynamic section parsing
//!
//! This module provides parsing for the .dynamic section and PT_DYNAMIC segment,
//! which contains dynamic linking information (needed libraries, rpaths, etc.).

use crate::parsers::elf::section_header_parser::get_string_from_strtab;
use crate::parsers::elf::structures::{DynamicEntry, DynamicInfo, dt_tag};
use nom::{
    IResult,
    number::complete::{be_u32, be_u64, le_u32, le_u64},
};

/// Parses a single ELF64 dynamic entry in little-endian format
fn parse_elf64_dyn_le(input: &[u8]) -> IResult<&[u8], DynamicEntry> {
    let (input, d_tag) = le_u64(input)?;
    let (input, d_val) = le_u64(input)?;

    Ok((
        input,
        DynamicEntry {
            d_tag: d_tag as i64,
            d_val,
        },
    ))
}

/// Parses a single ELF64 dynamic entry in big-endian format
fn parse_elf64_dyn_be(input: &[u8]) -> IResult<&[u8], DynamicEntry> {
    let (input, d_tag) = be_u64(input)?;
    let (input, d_val) = be_u64(input)?;

    Ok((
        input,
        DynamicEntry {
            d_tag: d_tag as i64,
            d_val,
        },
    ))
}

/// Parses a single ELF32 dynamic entry in little-endian format
fn parse_elf32_dyn_le(input: &[u8]) -> IResult<&[u8], DynamicEntry> {
    let (input, d_tag) = le_u32(input)?;
    let (input, d_val) = le_u32(input)?;

    Ok((
        input,
        DynamicEntry {
            d_tag: d_tag as i64,
            d_val: d_val as u64,
        },
    ))
}

/// Parses a single ELF32 dynamic entry in big-endian format
fn parse_elf32_dyn_be(input: &[u8]) -> IResult<&[u8], DynamicEntry> {
    let (input, d_tag) = be_u32(input)?;
    let (input, d_val) = be_u32(input)?;

    Ok((
        input,
        DynamicEntry {
            d_tag: d_tag as i64,
            d_val: d_val as u64,
        },
    ))
}

/// Parses all dynamic entries from the dynamic section
///
/// Parsing stops when DT_NULL is encountered or input is exhausted.
///
/// # Arguments
/// * `input` - Byte slice containing the dynamic section data
/// * `is_64bit` - True for ELF64, false for ELF32
/// * `is_little_endian` - True for little-endian, false for big-endian
///
/// # Returns
/// * `Vec<DynamicEntry>` - All dynamic entries found
pub fn parse_dynamic_entries(
    input: &[u8],
    is_64bit: bool,
    is_little_endian: bool,
) -> Vec<DynamicEntry> {
    let entry_size = if is_64bit { 16 } else { 8 };
    let parser: fn(&[u8]) -> IResult<&[u8], DynamicEntry> = match (is_64bit, is_little_endian) {
        (true, true) => parse_elf64_dyn_le,
        (true, false) => parse_elf64_dyn_be,
        (false, true) => parse_elf32_dyn_le,
        (false, false) => parse_elf32_dyn_be,
    };

    let mut entries = Vec::new();
    let mut remaining = input;

    while remaining.len() >= entry_size {
        match parser(remaining) {
            Ok((rest, entry)) => {
                // Stop at DT_NULL
                if entry.d_tag == dt_tag::DT_NULL {
                    entries.push(entry);
                    break;
                }
                entries.push(entry);
                remaining = rest;
            }
            Err(_) => break,
        }
    }

    entries
}

/// Extracts dynamic linking information from dynamic entries and string table
///
/// This function interprets the raw dynamic entries and resolves string
/// references using the dynamic string table (.dynstr).
///
/// # Arguments
/// * `entries` - Parsed dynamic entries
/// * `dynstr` - Dynamic string table data (from DT_STRTAB section)
///
/// # Returns
/// * `DynamicInfo` - Structured dynamic linking information
pub fn extract_dynamic_info(entries: &[DynamicEntry], dynstr: &[u8]) -> DynamicInfo {
    let mut info = DynamicInfo::default();

    for entry in entries {
        match entry.d_tag {
            dt_tag::DT_NEEDED => {
                // d_val is an offset into dynstr
                if let Some(name) = get_string_from_strtab(dynstr, entry.d_val as u32) {
                    info.needed.push(name);
                }
            }
            dt_tag::DT_SONAME => {
                if let Some(name) = get_string_from_strtab(dynstr, entry.d_val as u32) {
                    info.soname = Some(name);
                }
            }
            dt_tag::DT_RPATH => {
                if let Some(paths) = get_string_from_strtab(dynstr, entry.d_val as u32) {
                    // RPATH can contain multiple paths separated by colons
                    info.rpath = paths.split(':').map(|s| s.to_string()).collect();
                }
            }
            dt_tag::DT_RUNPATH => {
                if let Some(paths) = get_string_from_strtab(dynstr, entry.d_val as u32) {
                    info.runpath = paths.split(':').map(|s| s.to_string()).collect();
                }
            }
            dt_tag::DT_TEXTREL => {
                info.has_textrel = true;
            }
            dt_tag::DT_BIND_NOW => {
                info.bind_now = true;
            }
            dt_tag::DT_FLAGS => {
                info.flags = entry.d_val;
            }
            dt_tag::DT_FLAGS_1 => {
                info.flags_1 = entry.d_val;
            }
            _ => {}
        }
    }

    info
}

/// Finds the dynamic string table offset from dynamic entries
///
/// # Arguments
/// * `entries` - Parsed dynamic entries
///
/// # Returns
/// * `Some((address, size))` - Virtual address and size of the string table
/// * `None` - If DT_STRTAB or DT_STRSZ not found
pub fn find_dynstr_info(entries: &[DynamicEntry]) -> Option<(u64, u64)> {
    let mut strtab_addr = None;
    let mut strsz = None;

    for entry in entries {
        match entry.d_tag {
            dt_tag::DT_STRTAB => {
                strtab_addr = Some(entry.d_val);
            }
            dt_tag::DT_STRSZ => {
                strsz = Some(entry.d_val);
            }
            _ => {}
        }
    }

    match (strtab_addr, strsz) {
        (Some(addr), Some(size)) => Some((addr, size)),
        _ => None,
    }
}

/// Finds the symbol table info from dynamic entries
///
/// # Arguments
/// * `entries` - Parsed dynamic entries
///
/// # Returns
/// * `Some((address, entry_size))` - Virtual address and entry size of the symbol table
/// * `None` - If DT_SYMTAB or DT_SYMENT not found
pub fn find_dynsym_info(entries: &[DynamicEntry]) -> Option<(u64, u64)> {
    let mut symtab_addr = None;
    let mut syment = None;

    for entry in entries {
        match entry.d_tag {
            dt_tag::DT_SYMTAB => {
                symtab_addr = Some(entry.d_val);
            }
            dt_tag::DT_SYMENT => {
                syment = Some(entry.d_val);
            }
            _ => {}
        }
    }

    match (symtab_addr, syment) {
        (Some(addr), Some(ent)) => Some((addr, ent)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::elf::structures::df1_flags;

    /// Creates a dynamic entry for testing (ELF64 LE)
    fn create_dyn64_le(d_tag: i64, d_val: u64) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&(d_tag as u64).to_le_bytes());
        data.extend_from_slice(&d_val.to_le_bytes());
        data
    }

    /// Creates a dynamic entry for testing (ELF32 LE)
    fn create_dyn32_le(d_tag: i64, d_val: u32) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&(d_tag as u32).to_le_bytes());
        data.extend_from_slice(&d_val.to_le_bytes());
        data
    }

    #[test]
    fn test_parse_dynamic_entries_elf64() {
        let mut data = Vec::new();
        data.extend(create_dyn64_le(dt_tag::DT_NEEDED, 1));
        data.extend(create_dyn64_le(dt_tag::DT_NEEDED, 10));
        data.extend(create_dyn64_le(dt_tag::DT_STRTAB, 0x1000));
        data.extend(create_dyn64_le(dt_tag::DT_STRSZ, 100));
        data.extend(create_dyn64_le(dt_tag::DT_NULL, 0));

        let entries = parse_dynamic_entries(&data, true, true);
        assert_eq!(entries.len(), 5);

        assert_eq!(entries[0].d_tag, dt_tag::DT_NEEDED);
        assert_eq!(entries[0].d_val, 1);

        assert_eq!(entries[1].d_tag, dt_tag::DT_NEEDED);
        assert_eq!(entries[1].d_val, 10);

        assert_eq!(entries[2].d_tag, dt_tag::DT_STRTAB);
        assert_eq!(entries[2].d_val, 0x1000);

        assert_eq!(entries[4].d_tag, dt_tag::DT_NULL);
    }

    #[test]
    fn test_parse_dynamic_entries_elf32() {
        let mut data = Vec::new();
        data.extend(create_dyn32_le(dt_tag::DT_SONAME, 5));
        data.extend(create_dyn32_le(dt_tag::DT_FLAGS_1, 0x08000000)); // DF_1_PIE
        data.extend(create_dyn32_le(dt_tag::DT_NULL, 0));

        let entries = parse_dynamic_entries(&data, false, true);
        assert_eq!(entries.len(), 3);

        assert_eq!(entries[0].d_tag, dt_tag::DT_SONAME);
        assert_eq!(entries[0].d_val, 5);

        assert_eq!(entries[1].d_tag, dt_tag::DT_FLAGS_1);
        assert_eq!(entries[1].d_val, df1_flags::DF_1_PIE);
    }

    #[test]
    fn test_extract_dynamic_info() {
        // Create a test string table
        // Format: \0libc.so.6\0libm.so.6\0libtest.so\0/lib64:/usr/lib\0
        // Byte positions:
        //   0: \0
        //   1-9: libc.so.6 (9 chars)
        //   10: \0
        //   11-19: libm.so.6 (9 chars)
        //   20: \0
        //   21-30: libtest.so (10 chars)
        //   31: \0
        //   32-46: /lib64:/usr/lib (15 chars)
        //   47: \0
        let dynstr = b"\0libc.so.6\0libm.so.6\0libtest.so\0/lib64:/usr/lib\0";

        let entries = vec![
            DynamicEntry {
                d_tag: dt_tag::DT_NEEDED,
                d_val: 1,
            }, // libc.so.6 at offset 1
            DynamicEntry {
                d_tag: dt_tag::DT_NEEDED,
                d_val: 11,
            }, // libm.so.6 at offset 11
            DynamicEntry {
                d_tag: dt_tag::DT_SONAME,
                d_val: 21,
            }, // libtest.so at offset 21
            DynamicEntry {
                d_tag: dt_tag::DT_RUNPATH,
                d_val: 32,
            }, // /lib64:/usr/lib at offset 32
            DynamicEntry {
                d_tag: dt_tag::DT_TEXTREL,
                d_val: 0,
            },
            DynamicEntry {
                d_tag: dt_tag::DT_BIND_NOW,
                d_val: 0,
            },
            DynamicEntry {
                d_tag: dt_tag::DT_FLAGS_1,
                d_val: df1_flags::DF_1_PIE | df1_flags::DF_1_NOW,
            },
            DynamicEntry {
                d_tag: dt_tag::DT_NULL,
                d_val: 0,
            },
        ];

        let info = extract_dynamic_info(&entries, dynstr);

        assert_eq!(info.needed.len(), 2);
        assert_eq!(info.needed[0], "libc.so.6");
        assert_eq!(info.needed[1], "libm.so.6");

        assert_eq!(info.soname, Some("libtest.so".to_string()));

        assert_eq!(info.runpath.len(), 2);
        assert_eq!(info.runpath[0], "/lib64");
        assert_eq!(info.runpath[1], "/usr/lib");

        assert!(info.has_textrel);
        assert!(info.bind_now);
        assert!(info.is_pie());
    }

    #[test]
    fn test_find_dynstr_info() {
        let entries = vec![
            DynamicEntry {
                d_tag: dt_tag::DT_STRTAB,
                d_val: 0x400200,
            },
            DynamicEntry {
                d_tag: dt_tag::DT_STRSZ,
                d_val: 256,
            },
            DynamicEntry {
                d_tag: dt_tag::DT_NULL,
                d_val: 0,
            },
        ];

        let result = find_dynstr_info(&entries);
        assert!(result.is_some());

        let (addr, size) = result.unwrap();
        assert_eq!(addr, 0x400200);
        assert_eq!(size, 256);
    }

    #[test]
    fn test_find_dynstr_info_missing() {
        let entries = vec![
            DynamicEntry {
                d_tag: dt_tag::DT_STRTAB,
                d_val: 0x400200,
            },
            // Missing DT_STRSZ
            DynamicEntry {
                d_tag: dt_tag::DT_NULL,
                d_val: 0,
            },
        ];

        assert!(find_dynstr_info(&entries).is_none());
    }

    #[test]
    fn test_find_dynsym_info() {
        let entries = vec![
            DynamicEntry {
                d_tag: dt_tag::DT_SYMTAB,
                d_val: 0x400300,
            },
            DynamicEntry {
                d_tag: dt_tag::DT_SYMENT,
                d_val: 24,
            },
            DynamicEntry {
                d_tag: dt_tag::DT_NULL,
                d_val: 0,
            },
        ];

        let result = find_dynsym_info(&entries);
        assert!(result.is_some());

        let (addr, ent) = result.unwrap();
        assert_eq!(addr, 0x400300);
        assert_eq!(ent, 24);
    }

    #[test]
    fn test_dynamic_entry_tag_str() {
        let test_cases = vec![
            (dt_tag::DT_NULL, "NULL"),
            (dt_tag::DT_NEEDED, "NEEDED"),
            (dt_tag::DT_SONAME, "SONAME"),
            (dt_tag::DT_RPATH, "RPATH"),
            (dt_tag::DT_RUNPATH, "RUNPATH"),
            (dt_tag::DT_STRTAB, "STRTAB"),
            (dt_tag::DT_SYMTAB, "SYMTAB"),
            (dt_tag::DT_FLAGS_1, "FLAGS_1"),
        ];

        for (tag, expected) in test_cases {
            let entry = DynamicEntry {
                d_tag: tag,
                d_val: 0,
            };
            assert_eq!(entry.tag_str(), expected);
        }
    }
}
