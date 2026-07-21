//! Metadata extraction from ELF files
//!
//! This module orchestrates the extraction of metadata from ELF files by coordinating
//! the various sub-parsers (header, program headers, sections, dynamic, symbols, notes).

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::parsers::elf::dynamic_parser::{
    extract_dynamic_info, find_dynstr_info, parse_dynamic_entries,
};
use crate::parsers::elf::header_parser::parse_elf_header;
use crate::parsers::elf::note_parser::{extract_build_id, extract_gnu_abi_tag, parse_notes};
use crate::parsers::elf::program_header_parser::parse_program_headers;
use crate::parsers::elf::section_header_parser::{parse_section_headers, resolve_section_names};
use crate::parsers::elf::structures::{
    ElfHeader, ElfInfo, ProgramHeader, SectionHeader, elf_type, pt_type, sh_type,
};
use crate::parsers::elf::symbol_parser::{
    detect_security_features, extract_symbol_info, parse_symbol_table, resolve_symbol_names,
};

/// Maximum number of program headers to parse (sanity limit)
const MAX_PROGRAM_HEADERS: u16 = 1000;

/// Maximum number of section headers to parse (sanity limit)
const MAX_SECTION_HEADERS: u16 = 10000;

/// Maximum number of exported function names to include
const MAX_EXPORTED_FUNCTIONS: usize = 50;

/// Maximum number of imported function names to include
const MAX_IMPORTED_FUNCTIONS: usize = 50;

/// Main entry point for ELF metadata extraction
///
/// This function parses the ELF file and extracts comprehensive metadata
/// including header information, segment details, section information,
/// dynamic linking data, and security features.
///
/// # Arguments
/// * `reader` - File reader providing access to the ELF file
///
/// # Returns
/// * `Ok(MetadataMap)` - Extracted metadata
/// * `Err` - If parsing fails
pub fn extract_elf_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Add basic file info
    metadata.insert("FileType".to_string(), TagValue::String("ELF".to_string()));
    metadata.insert(
        "FileSize".to_string(),
        TagValue::Integer(reader.size() as i64),
    );

    // Step 1: Parse ELF header
    // The header is always at offset 0 and is 52 bytes (ELF32) or 64 bytes (ELF64)
    let header_size = if reader.size() >= 64 { 64 } else { 52 };
    let header_data = reader.read(0, header_size)?;

    let header = match parse_elf_header(header_data) {
        Ok((_, h)) => h,
        Err(e) => {
            return Err(ExifToolError::parse_error(format!(
                "Failed to parse ELF header: {:?}",
                e
            )));
        }
    };

    // Extract header metadata
    extract_header_metadata(&header, &mut metadata);

    // Create ElfInfo to accumulate all parsed data
    let mut elf_info = ElfInfo::new(header.clone());

    // Step 2: Parse program headers
    if header.e_phnum > 0 && header.e_phoff > 0 && header.e_phnum <= MAX_PROGRAM_HEADERS {
        let ph_size = header.e_phentsize as u64 * header.e_phnum as u64;
        if header.e_phoff + ph_size <= reader.size()
            && let Ok(ph_data) = reader.read(header.e_phoff, ph_size as usize)
            && let Ok((_, phdrs)) = parse_program_headers(
                ph_data,
                header.e_phnum,
                header.is_64bit,
                header.is_little_endian,
            )
        {
            elf_info.program_headers = phdrs;
        }
    }

    // Extract program header metadata
    extract_program_header_metadata(&elf_info.program_headers, &mut metadata);

    // Detect RELRO and executable stack from program headers
    for phdr in &elf_info.program_headers {
        if phdr.p_type == pt_type::PT_GNU_RELRO {
            elf_info.has_relro = true;
        }
        if phdr.p_type == pt_type::PT_GNU_STACK && phdr.is_executable() {
            elf_info.has_executable_stack = true;
        }
    }

    // Step 3: Parse section headers
    if header.e_shnum > 0 && header.e_shoff > 0 && header.e_shnum <= MAX_SECTION_HEADERS {
        let sh_size = header.e_shentsize as u64 * header.e_shnum as u64;
        if header.e_shoff + sh_size <= reader.size()
            && let Ok(sh_data) = reader.read(header.e_shoff, sh_size as usize)
            && let Ok((_, shdrs)) = parse_section_headers(
                sh_data,
                header.e_shnum,
                header.is_64bit,
                header.is_little_endian,
            )
        {
            elf_info.section_headers = shdrs;

            // Resolve section names from .shstrtab
            if header.e_shstrndx > 0
                && (header.e_shstrndx as usize) < elf_info.section_headers.len()
            {
                let shstrtab = &elf_info.section_headers[header.e_shstrndx as usize];
                if shstrtab.sh_offset + shstrtab.sh_size <= reader.size()
                    && let Ok(strtab_data) =
                        reader.read(shstrtab.sh_offset, shstrtab.sh_size as usize)
                {
                    resolve_section_names(&mut elf_info.section_headers, strtab_data);
                }
            }
        }
    }

    // Extract section metadata
    extract_section_metadata(&elf_info.section_headers, &mut metadata);

    // Step 4: Parse dynamic section and interpreter
    // First, find PT_INTERP for the interpreter path
    for phdr in &elf_info.program_headers {
        if phdr.p_type == pt_type::PT_INTERP
            && phdr.p_filesz > 0
            && phdr.p_offset + phdr.p_filesz <= reader.size()
            && let Ok(interp_data) = reader.read(phdr.p_offset, phdr.p_filesz as usize)
        {
            let interp = String::from_utf8_lossy(interp_data)
                .trim_end_matches('\0')
                .to_string();
            elf_info.dynamic_info.interpreter = Some(interp);
        }
    }

    // Parse .dynamic section
    if let Some(dynamic_section) =
        find_section_by_type(&elf_info.section_headers, sh_type::SHT_DYNAMIC)
        && dynamic_section.sh_offset + dynamic_section.sh_size <= reader.size()
        && let Ok(dyn_data) =
            reader.read(dynamic_section.sh_offset, dynamic_section.sh_size as usize)
    {
        let entries = parse_dynamic_entries(dyn_data, header.is_64bit, header.is_little_endian);

        // Find and read the dynamic string table
        if let Some((strtab_addr, strsz)) = find_dynstr_info(&entries) {
            // Try to find .dynstr section by matching address
            if let Some(dynstr_section) =
                find_section_by_addr(&elf_info.section_headers, strtab_addr)
                && dynstr_section.sh_offset + strsz <= reader.size()
                && let Ok(dynstr_data) = reader.read(dynstr_section.sh_offset, strsz as usize)
            {
                elf_info.dynamic_info = extract_dynamic_info(&entries, dynstr_data);
            }
        }
    }

    // Extract dynamic info metadata
    extract_dynamic_metadata(&elf_info.dynamic_info, &mut metadata);

    // Step 5: Parse symbol tables
    // Parse .dynsym (dynamic symbols)
    if let Some(dynsym_section) =
        find_section_by_type(&elf_info.section_headers, sh_type::SHT_DYNSYM)
        && dynsym_section.sh_offset + dynsym_section.sh_size <= reader.size()
        && let Ok(sym_data) = reader.read(dynsym_section.sh_offset, dynsym_section.sh_size as usize)
    {
        let mut symbols = parse_symbol_table(sym_data, header.is_64bit, header.is_little_endian);

        // Find and read the associated string table (.dynstr)
        // sh_link points to the string table section
        if (dynsym_section.sh_link as usize) < elf_info.section_headers.len() {
            let strtab = &elf_info.section_headers[dynsym_section.sh_link as usize];
            if strtab.sh_offset + strtab.sh_size <= reader.size()
                && let Ok(strtab_data) = reader.read(strtab.sh_offset, strtab.sh_size as usize)
            {
                resolve_symbol_names(&mut symbols, strtab_data);
            }
        }

        // Detect security features
        let (has_canary, _has_fortify) = detect_security_features(&symbols);
        elf_info.has_stack_canary = has_canary;

        // Extract symbol info
        let sym_info =
            extract_symbol_info(&symbols, MAX_EXPORTED_FUNCTIONS, MAX_IMPORTED_FUNCTIONS);
        elf_info.symbol_info.dynamic_symbol_count = sym_info.symbol_count;
        elf_info.symbol_info.exported_functions = sym_info.exported_functions;
        elf_info.symbol_info.imported_functions = sym_info.imported_functions;
    }

    // Parse .symtab (full symbol table) if present
    if let Some(symtab_section) =
        find_section_by_type(&elf_info.section_headers, sh_type::SHT_SYMTAB)
        && symtab_section.sh_offset + symtab_section.sh_size <= reader.size()
        && let Ok(sym_data) = reader.read(symtab_section.sh_offset, symtab_section.sh_size as usize)
    {
        let symbols = parse_symbol_table(sym_data, header.is_64bit, header.is_little_endian);
        elf_info.symbol_info.symbol_count = symbols.len();
    }

    // Extract symbol metadata
    extract_symbol_metadata(&elf_info.symbol_info, &mut metadata);

    // Step 6: Parse notes (build ID, ABI tag)
    // Try PT_NOTE segments first
    for phdr in &elf_info.program_headers {
        if phdr.p_type == pt_type::PT_NOTE
            && phdr.p_filesz > 0
            && phdr.p_offset + phdr.p_filesz <= reader.size()
            && let Ok(note_data) = reader.read(phdr.p_offset, phdr.p_filesz as usize)
        {
            let notes = parse_notes(note_data, header.is_little_endian);
            elf_info.notes.extend(notes);
        }
    }

    // Also check SHT_NOTE sections
    for section in &elf_info.section_headers {
        if section.sh_type == sh_type::SHT_NOTE
            && section.sh_size > 0
            && section.sh_offset + section.sh_size <= reader.size()
            && let Ok(note_data) = reader.read(section.sh_offset, section.sh_size as usize)
        {
            let notes = parse_notes(note_data, header.is_little_endian);
            // Avoid duplicates by checking if we already have this type
            for note in notes {
                if !elf_info
                    .notes
                    .iter()
                    .any(|n| n.note_type == note.note_type && n.name == note.name)
                {
                    elf_info.notes.push(note);
                }
            }
        }
    }

    // Extract build ID
    elf_info.build_id = extract_build_id(&elf_info.notes);

    // Extract note metadata
    extract_note_metadata(
        &elf_info.notes,
        &elf_info.build_id,
        header.is_little_endian,
        &mut metadata,
    );

    // Step 7: Extract security features metadata
    extract_security_metadata(&elf_info, &mut metadata);

    Ok(metadata)
}

/// Extracts metadata from the ELF header
fn extract_header_metadata(header: &ElfHeader, metadata: &mut MetadataMap) {
    // ELF class (32-bit or 64-bit)
    metadata.insert(
        "EXE:Class".to_string(),
        TagValue::String(header.class_str().to_string()),
    );

    // Endianness
    let endian_str = header.endian_str().to_string();
    metadata.insert(
        "EXE:Endianness".to_string(),
        TagValue::String(endian_str.clone()),
    );

    // Add ELF:DataEncoding as alias for Endianness (for ExifTool compatibility)
    metadata.insert("EXE:DataEncoding".to_string(), TagValue::String(endian_str));

    // OS/ABI
    metadata.insert(
        "EXE:OSABI".to_string(),
        TagValue::String(header.osabi_str().to_string()),
    );

    // ABI version
    metadata.insert(
        "EXE:ABIVersion".to_string(),
        TagValue::Integer(header.e_ident[8] as i64),
    );

    // Object type
    let type_str = header.type_str().to_string();
    metadata.insert(
        "EXE:ObjectType".to_string(),
        TagValue::String(type_str.clone()),
    );

    // Add ELF:Type as alias for ObjectType (for ExifTool compatibility)
    metadata.insert("EXE:Type".to_string(), TagValue::String(type_str));

    // Machine architecture
    metadata.insert(
        "EXE:Machine".to_string(),
        TagValue::String(header.machine_str().to_string()),
    );
    metadata.insert(
        "EXE:MachineRaw".to_string(),
        TagValue::Integer(header.e_machine as i64),
    );

    // Version
    metadata.insert(
        "EXE:Version".to_string(),
        TagValue::Integer(header.e_version as i64),
    );

    // Add ELF:Version as string as well for ExifTool compatibility
    metadata.insert(
        "EXE:VersionStr".to_string(),
        TagValue::String(header.e_version.to_string()),
    );

    // Entry point
    if header.e_entry > 0 {
        metadata.insert(
            "EXE:EntryPoint".to_string(),
            TagValue::String(format!("0x{:X}", header.e_entry)),
        );
    }

    // Header size
    metadata.insert(
        "EXE:HeaderSize".to_string(),
        TagValue::Integer(header.e_ehsize as i64),
    );

    // Flags
    if header.e_flags > 0 {
        metadata.insert(
            "EXE:Flags".to_string(),
            TagValue::String(format!("0x{:X}", header.e_flags)),
        );
    }

    // Program header info
    metadata.insert(
        "EXE:PHOffset".to_string(),
        TagValue::Integer(header.e_phoff as i64),
    );
    metadata.insert(
        "EXE:PHSize".to_string(),
        TagValue::Integer(header.e_phentsize as i64),
    );
    metadata.insert(
        "EXE:PHCount".to_string(),
        TagValue::Integer(header.e_phnum as i64),
    );

    // Add ELF:ProgramHeaderCount as alias for PHCount (for ExifTool compatibility)
    metadata.insert(
        "EXE:ProgramHeaderCount".to_string(),
        TagValue::Integer(header.e_phnum as i64),
    );

    // Section header info
    metadata.insert(
        "EXE:SHOffset".to_string(),
        TagValue::Integer(header.e_shoff as i64),
    );
    metadata.insert(
        "EXE:SHSize".to_string(),
        TagValue::Integer(header.e_shentsize as i64),
    );
    metadata.insert(
        "EXE:SHCount".to_string(),
        TagValue::Integer(header.e_shnum as i64),
    );

    // Add ELF:SectionCount as alias for SHCount (for ExifTool compatibility)
    metadata.insert(
        "EXE:SectionCount".to_string(),
        TagValue::Integer(header.e_shnum as i64),
    );
}

/// Extracts metadata from program headers
fn extract_program_header_metadata(phdrs: &[ProgramHeader], metadata: &mut MetadataMap) {
    // Count loadable segments
    let loadable_count = phdrs
        .iter()
        .filter(|p| p.p_type == pt_type::PT_LOAD)
        .count();
    metadata.insert(
        "EXE:LoadableSegmentCount".to_string(),
        TagValue::Integer(loadable_count as i64),
    );

    // List segment types
    let segment_types: Vec<String> = phdrs.iter().map(|p| p.type_str().to_string()).collect();
    if !segment_types.is_empty() {
        metadata.insert(
            "EXE:SegmentTypes".to_string(),
            TagValue::String(segment_types.join(", ")),
        );
    }

    // Check for special segments
    let has_dynamic = phdrs.iter().any(|p| p.p_type == pt_type::PT_DYNAMIC);
    let has_interp = phdrs.iter().any(|p| p.p_type == pt_type::PT_INTERP);
    let has_tls = phdrs.iter().any(|p| p.p_type == pt_type::PT_TLS);

    metadata.insert(
        "EXE:HasDynamic".to_string(),
        TagValue::Integer(if has_dynamic { 1 } else { 0 }),
    );
    metadata.insert(
        "EXE:HasInterpreter".to_string(),
        TagValue::Integer(if has_interp { 1 } else { 0 }),
    );
    metadata.insert(
        "EXE:HasTLS".to_string(),
        TagValue::Integer(if has_tls { 1 } else { 0 }),
    );
}

/// Extracts metadata from section headers
fn extract_section_metadata(sections: &[SectionHeader], metadata: &mut MetadataMap) {
    // Collect section names
    let section_names: Vec<String> = sections
        .iter()
        .filter_map(|s| s.name.clone())
        .filter(|n| !n.is_empty())
        .collect();

    if !section_names.is_empty() {
        metadata.insert(
            "EXE:SectionNames".to_string(),
            TagValue::String(section_names.join(", ")),
        );
    }

    // Find specific section sizes
    for section in sections {
        if let Some(ref name) = section.name {
            match name.as_str() {
                ".text" => {
                    metadata.insert(
                        "EXE:TextSectionSize".to_string(),
                        TagValue::Integer(section.sh_size as i64),
                    );
                }
                ".data" => {
                    metadata.insert(
                        "EXE:DataSectionSize".to_string(),
                        TagValue::Integer(section.sh_size as i64),
                    );
                }
                ".bss" => {
                    metadata.insert(
                        "EXE:BssSectionSize".to_string(),
                        TagValue::Integer(section.sh_size as i64),
                    );
                }
                ".rodata" => {
                    metadata.insert(
                        "EXE:RodataSectionSize".to_string(),
                        TagValue::Integer(section.sh_size as i64),
                    );
                }
                _ => {}
            }
        }
    }
}

/// Extracts metadata from dynamic linking information
fn extract_dynamic_metadata(
    dynamic_info: &crate::parsers::elf::structures::DynamicInfo,
    metadata: &mut MetadataMap,
) {
    // Interpreter
    if let Some(ref interp) = dynamic_info.interpreter {
        metadata.insert(
            "EXE:Interpreter".to_string(),
            TagValue::String(interp.clone()),
        );
    }

    // Needed libraries (shared objects)
    if !dynamic_info.needed.is_empty() {
        metadata.insert(
            "EXE:SharedObjectCount".to_string(),
            TagValue::Integer(dynamic_info.needed.len() as i64),
        );
        metadata.insert(
            "EXE:SharedObjects".to_string(),
            TagValue::String(dynamic_info.needed.join(", ")),
        );
    }

    // SONAME
    if let Some(ref soname) = dynamic_info.soname {
        metadata.insert("EXE:SONAME".to_string(), TagValue::String(soname.clone()));
    }

    // RPATH (deprecated)
    if !dynamic_info.rpath.is_empty() {
        metadata.insert(
            "EXE:RPATH".to_string(),
            TagValue::String(dynamic_info.rpath.join(":")),
        );
    }

    // RUNPATH
    if !dynamic_info.runpath.is_empty() {
        metadata.insert(
            "EXE:RUNPATH".to_string(),
            TagValue::String(dynamic_info.runpath.join(":")),
        );
    }

    // Flags
    if dynamic_info.has_textrel {
        metadata.insert("EXE:HasTextRel".to_string(), TagValue::Integer(1));
    }
    if dynamic_info.bind_now {
        metadata.insert("EXE:BindNow".to_string(), TagValue::Integer(1));
    }
}

/// Extracts metadata from symbol tables
fn extract_symbol_metadata(
    symbol_info: &crate::parsers::elf::structures::SymbolInfo,
    metadata: &mut MetadataMap,
) {
    if symbol_info.symbol_count > 0 {
        metadata.insert(
            "EXE:SymbolCount".to_string(),
            TagValue::Integer(symbol_info.symbol_count as i64),
        );
    }

    if symbol_info.dynamic_symbol_count > 0 {
        metadata.insert(
            "EXE:DynamicSymbolCount".to_string(),
            TagValue::Integer(symbol_info.dynamic_symbol_count as i64),
        );
    }

    if !symbol_info.exported_functions.is_empty() {
        metadata.insert(
            "EXE:ExportedFunctions".to_string(),
            TagValue::String(symbol_info.exported_functions.join(", ")),
        );
        metadata.insert(
            "EXE:ExportedFunctionCount".to_string(),
            TagValue::Integer(symbol_info.exported_functions.len() as i64),
        );
    }

    if !symbol_info.imported_functions.is_empty() {
        metadata.insert(
            "EXE:ImportedFunctions".to_string(),
            TagValue::String(symbol_info.imported_functions.join(", ")),
        );
        metadata.insert(
            "EXE:ImportedFunctionCount".to_string(),
            TagValue::Integer(symbol_info.imported_functions.len() as i64),
        );
    }
}

/// Extracts metadata from note sections
fn extract_note_metadata(
    notes: &[crate::parsers::elf::structures::NoteEntry],
    build_id: &Option<String>,
    is_little_endian: bool,
    metadata: &mut MetadataMap,
) {
    // Build ID
    if let Some(id) = build_id {
        metadata.insert("EXE:BuildID".to_string(), TagValue::String(id.clone()));
    }

    // GNU ABI tag
    if let Some(abi) = extract_gnu_abi_tag(notes, is_little_endian) {
        metadata.insert(
            "EXE:ABITagOS".to_string(),
            TagValue::String(abi.os_name().to_string()),
        );
        metadata.insert(
            "EXE:ABITagVersion".to_string(),
            TagValue::String(abi.version_string()),
        );
    }
}

/// Extracts security feature metadata
fn extract_security_metadata(elf_info: &ElfInfo, metadata: &mut MetadataMap) {
    // PIE (Position Independent Executable)
    let is_pie = elf_info.header.e_type == elf_type::ET_DYN
        && elf_info.header.e_entry != 0
        && elf_info.dynamic_info.is_pie();
    metadata.insert(
        "EXE:PIEEnabled".to_string(),
        TagValue::Integer(if is_pie { 1 } else { 0 }),
    );

    // RELRO
    metadata.insert(
        "EXE:RELROEnabled".to_string(),
        TagValue::Integer(if elf_info.has_relro { 1 } else { 0 }),
    );

    // Stack canary
    metadata.insert(
        "EXE:StackCanary".to_string(),
        TagValue::Integer(if elf_info.has_stack_canary { 1 } else { 0 }),
    );

    // Executable stack (NX)
    metadata.insert(
        "EXE:ExecutableStack".to_string(),
        TagValue::Integer(if elf_info.has_executable_stack { 1 } else { 0 }),
    );

    // NX bit (inverse of executable stack - NX enabled means stack is NOT executable)
    metadata.insert(
        "EXE:NXEnabled".to_string(),
        TagValue::Integer(if !elf_info.has_executable_stack { 1 } else { 0 }),
    );
}

/// Finds a section by type
fn find_section_by_type(sections: &[SectionHeader], sh_type: u32) -> Option<&SectionHeader> {
    sections.iter().find(|s| s.sh_type == sh_type)
}

/// Finds a section by virtual address
fn find_section_by_addr(sections: &[SectionHeader], addr: u64) -> Option<&SectionHeader> {
    sections.iter().find(|s| s.sh_addr == addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_section_by_type() {
        let sections = vec![
            SectionHeader {
                sh_name: 0,
                name: Some("".to_string()),
                sh_type: sh_type::SHT_NULL,
                sh_flags: 0,
                sh_addr: 0,
                sh_offset: 0,
                sh_size: 0,
                sh_link: 0,
                sh_info: 0,
                sh_addralign: 0,
                sh_entsize: 0,
            },
            SectionHeader {
                sh_name: 1,
                name: Some(".text".to_string()),
                sh_type: sh_type::SHT_PROGBITS,
                sh_flags: 0,
                sh_addr: 0x1000,
                sh_offset: 0x1000,
                sh_size: 0x500,
                sh_link: 0,
                sh_info: 0,
                sh_addralign: 16,
                sh_entsize: 0,
            },
            SectionHeader {
                sh_name: 7,
                name: Some(".dynsym".to_string()),
                sh_type: sh_type::SHT_DYNSYM,
                sh_flags: 0,
                sh_addr: 0x2000,
                sh_offset: 0x2000,
                sh_size: 0x100,
                sh_link: 2,
                sh_info: 0,
                sh_addralign: 8,
                sh_entsize: 24,
            },
        ];

        let dynsym = find_section_by_type(&sections, sh_type::SHT_DYNSYM);
        assert!(dynsym.is_some());
        assert_eq!(dynsym.unwrap().name, Some(".dynsym".to_string()));

        let missing = find_section_by_type(&sections, sh_type::SHT_SYMTAB);
        assert!(missing.is_none());
    }

    #[test]
    fn test_find_section_by_addr() {
        let sections = vec![SectionHeader {
            sh_name: 0,
            name: Some(".text".to_string()),
            sh_type: sh_type::SHT_PROGBITS,
            sh_flags: 0,
            sh_addr: 0x400000,
            sh_offset: 0x1000,
            sh_size: 0x500,
            sh_link: 0,
            sh_info: 0,
            sh_addralign: 0,
            sh_entsize: 0,
        }];

        let found = find_section_by_addr(&sections, 0x400000);
        assert!(found.is_some());

        let missing = find_section_by_addr(&sections, 0x500000);
        assert!(missing.is_none());
    }
}
