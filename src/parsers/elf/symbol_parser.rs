//! ELF symbol table parsing
//!
//! This module provides parsing for symbol tables (.symtab and .dynsym sections).
//! Symbol tables contain information about functions, variables, and other symbols.

use crate::parsers::elf::section_header_parser::get_string_from_strtab;
use crate::parsers::elf::structures::{Symbol, SymbolInfo, shn_index, stb_binding, stt_type};
use nom::{
    IResult,
    number::complete::{be_u8, be_u16, be_u32, be_u64, le_u8, le_u16, le_u32, le_u64},
};

/// Parses a single ELF64 symbol in little-endian format
fn parse_elf64_sym_le(input: &[u8]) -> IResult<&[u8], Symbol> {
    let (input, st_name) = le_u32(input)?;
    let (input, st_info) = le_u8(input)?;
    let (input, st_other) = le_u8(input)?;
    let (input, st_shndx) = le_u16(input)?;
    let (input, st_value) = le_u64(input)?;
    let (input, st_size) = le_u64(input)?;

    Ok((
        input,
        Symbol {
            st_name,
            name: None,
            st_info,
            st_other,
            st_shndx,
            st_value,
            st_size,
        },
    ))
}

/// Parses a single ELF64 symbol in big-endian format
fn parse_elf64_sym_be(input: &[u8]) -> IResult<&[u8], Symbol> {
    let (input, st_name) = be_u32(input)?;
    let (input, st_info) = be_u8(input)?;
    let (input, st_other) = be_u8(input)?;
    let (input, st_shndx) = be_u16(input)?;
    let (input, st_value) = be_u64(input)?;
    let (input, st_size) = be_u64(input)?;

    Ok((
        input,
        Symbol {
            st_name,
            name: None,
            st_info,
            st_other,
            st_shndx,
            st_value,
            st_size,
        },
    ))
}

/// Parses a single ELF32 symbol in little-endian format
///
/// Note: ELF32 has a different field order than ELF64
fn parse_elf32_sym_le(input: &[u8]) -> IResult<&[u8], Symbol> {
    let (input, st_name) = le_u32(input)?;
    let (input, st_value) = le_u32(input)?;
    let (input, st_size) = le_u32(input)?;
    let (input, st_info) = le_u8(input)?;
    let (input, st_other) = le_u8(input)?;
    let (input, st_shndx) = le_u16(input)?;

    Ok((
        input,
        Symbol {
            st_name,
            name: None,
            st_info,
            st_other,
            st_shndx,
            st_value: st_value as u64,
            st_size: st_size as u64,
        },
    ))
}

/// Parses a single ELF32 symbol in big-endian format
fn parse_elf32_sym_be(input: &[u8]) -> IResult<&[u8], Symbol> {
    let (input, st_name) = be_u32(input)?;
    let (input, st_value) = be_u32(input)?;
    let (input, st_size) = be_u32(input)?;
    let (input, st_info) = be_u8(input)?;
    let (input, st_other) = be_u8(input)?;
    let (input, st_shndx) = be_u16(input)?;

    Ok((
        input,
        Symbol {
            st_name,
            name: None,
            st_info,
            st_other,
            st_shndx,
            st_value: st_value as u64,
            st_size: st_size as u64,
        },
    ))
}

/// Parses all symbols from a symbol table section
///
/// # Arguments
/// * `input` - Byte slice containing the symbol table data
/// * `is_64bit` - True for ELF64, false for ELF32
/// * `is_little_endian` - True for little-endian, false for big-endian
///
/// # Returns
/// * `Vec<Symbol>` - All symbols found in the table
pub fn parse_symbol_table(input: &[u8], is_64bit: bool, is_little_endian: bool) -> Vec<Symbol> {
    let entry_size = if is_64bit { 24 } else { 16 };
    let parser: fn(&[u8]) -> IResult<&[u8], Symbol> = match (is_64bit, is_little_endian) {
        (true, true) => parse_elf64_sym_le,
        (true, false) => parse_elf64_sym_be,
        (false, true) => parse_elf32_sym_le,
        (false, false) => parse_elf32_sym_be,
    };

    let mut symbols = Vec::new();
    let mut remaining = input;

    while remaining.len() >= entry_size {
        match parser(remaining) {
            Ok((rest, symbol)) => {
                symbols.push(symbol);
                remaining = rest;
            }
            Err(_) => break,
        }
    }

    symbols
}

/// Resolves symbol names from a string table
///
/// # Arguments
/// * `symbols` - Mutable slice of symbols
/// * `strtab` - String table data
pub fn resolve_symbol_names(symbols: &mut [Symbol], strtab: &[u8]) {
    for symbol in symbols.iter_mut() {
        symbol.name = get_string_from_strtab(strtab, symbol.st_name);
    }
}

/// Extracts symbol information (counts, exports, imports) from a symbol table
///
/// # Arguments
/// * `symbols` - Parsed symbols with resolved names
/// * `max_exports` - Maximum number of exported function names to collect
/// * `max_imports` - Maximum number of imported function names to collect
///
/// # Returns
/// * `SymbolInfo` - Aggregated symbol information
pub fn extract_symbol_info(
    symbols: &[Symbol],
    max_exports: usize,
    max_imports: usize,
) -> SymbolInfo {
    let mut info = SymbolInfo {
        symbol_count: symbols.len(),
        dynamic_symbol_count: 0, // Set separately for .dynsym
        exported_functions: Vec::new(),
        imported_functions: Vec::new(),
    };

    for symbol in symbols {
        // Skip symbols without names or with empty names
        let name = match &symbol.name {
            Some(n) if !n.is_empty() => n.clone(),
            _ => continue,
        };

        // Check if this is a function
        if symbol.sym_type() != stt_type::STT_FUNC {
            continue;
        }

        // Check binding (global or weak)
        let binding = symbol.binding();
        if binding != stb_binding::STB_GLOBAL && binding != stb_binding::STB_WEAK {
            continue;
        }

        // Categorize as export or import
        if symbol.st_shndx == shn_index::SHN_UNDEF {
            // Undefined symbol = import
            if info.imported_functions.len() < max_imports {
                info.imported_functions.push(name);
            }
        } else {
            // Defined symbol = export
            if info.exported_functions.len() < max_exports {
                info.exported_functions.push(name);
            }
        }
    }

    info
}

/// Checks if any imported function matches a security-related pattern
///
/// This is useful for detecting stack canary usage (__stack_chk_fail),
/// FORTIFY_SOURCE usage (__*_chk functions), etc.
///
/// # Arguments
/// * `symbols` - Parsed symbols with resolved names
///
/// # Returns
/// * `(has_stack_canary, has_fortify)` - Security feature indicators
pub fn detect_security_features(symbols: &[Symbol]) -> (bool, bool) {
    let mut has_stack_canary = false;
    let mut has_fortify = false;

    for symbol in symbols {
        if symbol.st_shndx != shn_index::SHN_UNDEF {
            continue; // Only check undefined (imported) symbols
        }

        if let Some(ref name) = symbol.name {
            if name == "__stack_chk_fail" || name == "__stack_chk_guard" {
                has_stack_canary = true;
            }
            if name.ends_with("_chk") && name.starts_with("__") {
                has_fortify = true;
            }
        }
    }

    (has_stack_canary, has_fortify)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a test ELF64 symbol (little-endian)
    fn create_elf64_sym_le(
        st_name: u32,
        st_info: u8,
        st_other: u8,
        st_shndx: u16,
        st_value: u64,
        st_size: u64,
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&st_name.to_le_bytes());
        data.push(st_info);
        data.push(st_other);
        data.extend_from_slice(&st_shndx.to_le_bytes());
        data.extend_from_slice(&st_value.to_le_bytes());
        data.extend_from_slice(&st_size.to_le_bytes());
        data
    }

    /// Creates a test ELF32 symbol (little-endian)
    fn create_elf32_sym_le(
        st_name: u32,
        st_info: u8,
        st_other: u8,
        st_shndx: u16,
        st_value: u32,
        st_size: u32,
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&st_name.to_le_bytes());
        data.extend_from_slice(&st_value.to_le_bytes());
        data.extend_from_slice(&st_size.to_le_bytes());
        data.push(st_info);
        data.push(st_other);
        data.extend_from_slice(&st_shndx.to_le_bytes());
        data
    }

    /// Creates st_info from binding and type
    fn make_st_info(binding: u8, sym_type: u8) -> u8 {
        (binding << 4) | (sym_type & 0xF)
    }

    #[test]
    fn test_parse_elf64_symbol() {
        let st_info = make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_FUNC);
        let data = create_elf64_sym_le(1, st_info, 0, 1, 0x1000, 100);

        let symbols = parse_symbol_table(&data, true, true);
        assert_eq!(symbols.len(), 1);

        let sym = &symbols[0];
        assert_eq!(sym.st_name, 1);
        assert_eq!(sym.st_value, 0x1000);
        assert_eq!(sym.st_size, 100);
        assert_eq!(sym.st_shndx, 1);
        assert_eq!(sym.binding(), stb_binding::STB_GLOBAL);
        assert_eq!(sym.sym_type(), stt_type::STT_FUNC);
        assert_eq!(sym.binding_str(), "GLOBAL");
        assert_eq!(sym.type_str(), "FUNC");
        assert!(sym.is_defined());
        assert!(sym.is_function());
        assert!(sym.is_global());
    }

    #[test]
    fn test_parse_elf32_symbol() {
        let st_info = make_st_info(stb_binding::STB_WEAK, stt_type::STT_OBJECT);
        let data = create_elf32_sym_le(5, st_info, 0, 2, 0x2000, 50);

        let symbols = parse_symbol_table(&data, false, true);
        assert_eq!(symbols.len(), 1);

        let sym = &symbols[0];
        assert_eq!(sym.st_name, 5);
        assert_eq!(sym.st_value, 0x2000);
        assert_eq!(sym.st_size, 50);
        assert_eq!(sym.binding(), stb_binding::STB_WEAK);
        assert_eq!(sym.sym_type(), stt_type::STT_OBJECT);
        assert_eq!(sym.binding_str(), "WEAK");
        assert_eq!(sym.type_str(), "OBJECT");
        assert!(!sym.is_function());
    }

    #[test]
    fn test_parse_multiple_symbols() {
        let mut data = Vec::new();

        // NULL symbol (always first)
        data.extend(create_elf64_sym_le(0, 0, 0, 0, 0, 0));

        // main function (global, defined)
        let st_info = make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_FUNC);
        data.extend(create_elf64_sym_le(1, st_info, 0, 1, 0x1000, 200));

        // printf (global, undefined - import)
        let st_info = make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_FUNC);
        data.extend(create_elf64_sym_le(
            6,
            st_info,
            0,
            shn_index::SHN_UNDEF,
            0,
            0,
        ));

        let symbols = parse_symbol_table(&data, true, true);
        assert_eq!(symbols.len(), 3);

        // NULL symbol
        assert_eq!(symbols[0].binding(), stb_binding::STB_LOCAL);
        assert!(!symbols[0].is_defined());

        // main
        assert!(symbols[1].is_defined());
        assert!(symbols[1].is_function());

        // printf
        assert!(!symbols[2].is_defined());
        assert!(symbols[2].is_function());
    }

    #[test]
    fn test_resolve_symbol_names() {
        let strtab = b"\0main\0printf\0data\0";

        let mut symbols = vec![
            Symbol {
                st_name: 0,
                name: None,
                st_info: 0,
                st_other: 0,
                st_shndx: 0,
                st_value: 0,
                st_size: 0,
            },
            Symbol {
                st_name: 1,
                name: None,
                st_info: make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_FUNC),
                st_other: 0,
                st_shndx: 1,
                st_value: 0x1000,
                st_size: 100,
            },
            Symbol {
                st_name: 6,
                name: None,
                st_info: make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_FUNC),
                st_other: 0,
                st_shndx: 0,
                st_value: 0,
                st_size: 0,
            },
        ];

        resolve_symbol_names(&mut symbols, strtab);

        assert_eq!(symbols[0].name, Some("".to_string()));
        assert_eq!(symbols[1].name, Some("main".to_string()));
        assert_eq!(symbols[2].name, Some("printf".to_string()));

        assert_eq!(symbols[1].name_str(), "main");
        assert_eq!(symbols[2].name_str(), "printf");
    }

    #[test]
    fn test_extract_symbol_info() {
        let symbols = vec![
            // NULL symbol
            Symbol {
                st_name: 0,
                name: Some("".to_string()),
                st_info: 0,
                st_other: 0,
                st_shndx: 0,
                st_value: 0,
                st_size: 0,
            },
            // main (exported)
            Symbol {
                st_name: 1,
                name: Some("main".to_string()),
                st_info: make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_FUNC),
                st_other: 0,
                st_shndx: 1,
                st_value: 0x1000,
                st_size: 100,
            },
            // helper (exported)
            Symbol {
                st_name: 6,
                name: Some("helper".to_string()),
                st_info: make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_FUNC),
                st_other: 0,
                st_shndx: 1,
                st_value: 0x2000,
                st_size: 50,
            },
            // printf (imported)
            Symbol {
                st_name: 13,
                name: Some("printf".to_string()),
                st_info: make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_FUNC),
                st_other: 0,
                st_shndx: shn_index::SHN_UNDEF,
                st_value: 0,
                st_size: 0,
            },
            // global_data (not a function, ignored)
            Symbol {
                st_name: 20,
                name: Some("global_data".to_string()),
                st_info: make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_OBJECT),
                st_other: 0,
                st_shndx: 2,
                st_value: 0x3000,
                st_size: 8,
            },
        ];

        let info = extract_symbol_info(&symbols, 100, 100);

        assert_eq!(info.symbol_count, 5);
        assert_eq!(info.exported_functions.len(), 2);
        assert!(info.exported_functions.contains(&"main".to_string()));
        assert!(info.exported_functions.contains(&"helper".to_string()));
        assert_eq!(info.imported_functions.len(), 1);
        assert!(info.imported_functions.contains(&"printf".to_string()));
    }

    #[test]
    fn test_extract_symbol_info_with_limits() {
        let mut symbols = vec![Symbol {
            st_name: 0,
            name: Some("".to_string()),
            st_info: 0,
            st_other: 0,
            st_shndx: 0,
            st_value: 0,
            st_size: 0,
        }];

        // Add 10 exported functions
        for i in 0..10 {
            symbols.push(Symbol {
                st_name: i,
                name: Some(format!("func{}", i)),
                st_info: make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_FUNC),
                st_other: 0,
                st_shndx: 1,
                st_value: 0x1000 + (i as u64 * 100),
                st_size: 50,
            });
        }

        // Limit to 5 exports
        let info = extract_symbol_info(&symbols, 5, 100);
        assert_eq!(info.exported_functions.len(), 5);
    }

    #[test]
    fn test_detect_security_features() {
        let symbols = vec![
            Symbol {
                st_name: 0,
                name: Some("__stack_chk_fail".to_string()),
                st_info: make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_FUNC),
                st_other: 0,
                st_shndx: shn_index::SHN_UNDEF,
                st_value: 0,
                st_size: 0,
            },
            Symbol {
                st_name: 1,
                name: Some("__printf_chk".to_string()),
                st_info: make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_FUNC),
                st_other: 0,
                st_shndx: shn_index::SHN_UNDEF,
                st_value: 0,
                st_size: 0,
            },
            Symbol {
                st_name: 2,
                name: Some("main".to_string()),
                st_info: make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_FUNC),
                st_other: 0,
                st_shndx: 1,
                st_value: 0x1000,
                st_size: 100,
            },
        ];

        let (has_canary, has_fortify) = detect_security_features(&symbols);
        assert!(has_canary);
        assert!(has_fortify);
    }

    #[test]
    fn test_detect_security_features_none() {
        let symbols = vec![Symbol {
            st_name: 0,
            name: Some("printf".to_string()),
            st_info: make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_FUNC),
            st_other: 0,
            st_shndx: shn_index::SHN_UNDEF,
            st_value: 0,
            st_size: 0,
        }];

        let (has_canary, has_fortify) = detect_security_features(&symbols);
        assert!(!has_canary);
        assert!(!has_fortify);
    }
}
