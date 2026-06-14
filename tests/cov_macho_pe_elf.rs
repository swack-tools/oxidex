//! Coverage tests for executable parsers: Mach-O, PE, ELF.
//!
//! These tests build synthetic byte buffers that are valid enough to drive the
//! parsers deep (load commands, sections, data directories, program/section
//! headers, dynamic/symbol/note tables) and exercise as many distinct lines as
//! possible across the target source files. They also drive the production path
//! `oxidex::core::operations::read_metadata` on tempfiles with the correct magic
//! bytes (detection is by magic, not extension).

#[path = "common/mod.rs"]
mod common;

use common::TestReader;

use oxidex::core::operations::read_metadata;
use oxidex::core::{TagValue, decode_flags as core_decode_flags};
use std::io::Write;
use tempfile::NamedTempFile;

// =============================================================================
// Mach-O imports
// =============================================================================
use oxidex::parsers::macho::dylib_parser::{
    DylibCategory, DylibStats, DylibType, categorize_dylibs, extract_library_name, get_dylib_names,
    get_dylib_paths, is_system_dylib,
};
use oxidex::parsers::macho::header_parser::{
    fat_arch_size, header_size as macho_header_size, is_fat_magic, is_macho_magic,
    parse_fat_arch_32, parse_fat_arch_64, parse_fat_archs, parse_fat_header, parse_mach_header,
};
use oxidex::parsers::macho::load_command_parser::{
    LoadCommand, parse_all_load_commands, parse_build_version_command, parse_dylib_command,
    parse_dysymtab_command, parse_encryption_info_command, parse_entry_point_command,
    parse_linkedit_data_command, parse_load_command, parse_load_command_header,
    parse_rpath_command, parse_segment_command_32, parse_segment_command_64,
    parse_source_version_command, parse_symtab_command, parse_uuid_command,
    parse_version_min_command,
};
use oxidex::parsers::macho::metadata_extractor::{extract_macho_metadata, populate_macho_info};
use oxidex::parsers::macho::parse_macho_metadata;
use oxidex::parsers::macho::segment_parser::{
    SegmentStats, decode_section_attrs, find_section, get_section_names, get_segment_names,
    section_attrs, section_type, section_type_name,
};
use oxidex::parsers::macho::signature_parser::{
    decode_cs_flags, has_developer_id, is_adhoc_signed, parse_code_directory,
    parse_code_signature_info, parse_super_blob,
};
use oxidex::parsers::macho::structures::{
    MachHeader, MachOInfo, SourceVersionCommand, UuidCommand, build_tool, cpu_subtype_arm64,
    cpu_subtype_x86_64, cpu_type, decode_flags, file_type, file_type_name, flags as mh_flags,
    hash_type_name, load_command, load_command_name, magic, platform, platform_name,
};
use oxidex::parsers::macho::symbol_parser::{
    SymbolCategory, SymbolStats, decode_n_desc, detect_language, get_library_ordinal,
    is_cpp_symbol, is_external, is_mangled_name, is_objc_method, is_swift_symbol, is_undefined,
    n_desc, n_type as macho_n_type, symbol_type_name,
};
use oxidex::parsers::macho::version_parser::{
    VersionInfo, compare_versions, format_version_with_name, ios_version_name, macos_version_name,
    meets_min_version, parse_source_version, parse_version,
};

// =============================================================================
// PE imports
// =============================================================================
use oxidex::parsers::pe::coff_parser::parse_coff_header;
use oxidex::parsers::pe::dos_parser::parse_dos_header;
use oxidex::parsers::pe::optional_parser::{
    parse_optional_header_nt, parse_optional_header_standard,
};
use oxidex::parsers::pe::parse_pe_metadata;
use oxidex::parsers::pe::section_parser::{parse_section_header, parse_section_table};
use oxidex::parsers::pe::signature_parser::{
    cert_revision, cert_type, parse_signature_info, parse_win_certificate,
};
use oxidex::parsers::pe::structures::{
    SectionHeader as PeSectionHeader, VsFixedFileInfo, machine_types as pe_machine, subsystem_types,
};

// =============================================================================
// ELF imports
// =============================================================================
use oxidex::parsers::elf::dynamic_parser::{
    extract_dynamic_info, find_dynstr_info, find_dynsym_info, parse_dynamic_entries,
};
use oxidex::parsers::elf::header_parser::parse_elf_header;
use oxidex::parsers::elf::note_parser::{
    extract_build_id, extract_gnu_abi_tag, extract_gold_version, parse_notes,
};
use oxidex::parsers::elf::parse_elf_metadata;
use oxidex::parsers::elf::program_header_parser::parse_program_headers;
use oxidex::parsers::elf::section_header_parser::{
    get_string_from_strtab, parse_section_headers, resolve_section_names,
};
use oxidex::parsers::elf::structures::{
    DynamicEntry, NoteEntry, SectionHeader as ElfSectionHeader, Symbol, df1_flags, dt_tag,
    elf_class, elf_data, elf_type, machine_types as elf_machine, pf_flags, pt_type, sh_flags,
    sh_type, shn_index, stb_binding, stt_type,
};
use oxidex::parsers::elf::symbol_parser::{
    detect_security_features, extract_symbol_info, parse_symbol_table, resolve_symbol_names,
};

// =============================================================================
// Mach-O fixture builders
// =============================================================================

/// Build an LC_UUID command (24 bytes).
fn lc_uuid(uuid: [u8; 16]) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&load_command::LC_UUID.to_le_bytes());
    d.extend_from_slice(&24u32.to_le_bytes());
    d.extend_from_slice(&uuid);
    d
}

/// Build an LC_SEGMENT_64 with zero sections (72 bytes).
fn lc_segment_64(name: &str, vmsize: u64) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&load_command::LC_SEGMENT_64.to_le_bytes());
    d.extend_from_slice(&72u32.to_le_bytes()); // cmdsize (header only, 0 sections)
    let mut segname = [0u8; 16];
    let bytes = name.as_bytes();
    segname[..bytes.len().min(16)].copy_from_slice(&bytes[..bytes.len().min(16)]);
    d.extend_from_slice(&segname);
    d.extend_from_slice(&0x1000u64.to_le_bytes()); // vmaddr
    d.extend_from_slice(&vmsize.to_le_bytes()); // vmsize
    d.extend_from_slice(&0u64.to_le_bytes()); // fileoff
    d.extend_from_slice(&vmsize.to_le_bytes()); // filesize
    d.extend_from_slice(&7i32.to_le_bytes()); // maxprot
    d.extend_from_slice(&5i32.to_le_bytes()); // initprot
    d.extend_from_slice(&0u32.to_le_bytes()); // nsects
    d.extend_from_slice(&0u32.to_le_bytes()); // flags
    d
}

/// Build an LC_SEGMENT_64 with a single 64-bit section.
fn lc_segment_64_with_section(seg: &str, sect: &str) -> Vec<u8> {
    let mut d = Vec::new();
    let cmdsize = 72u32 + 80u32; // header + one 64-bit section (80 bytes)
    d.extend_from_slice(&load_command::LC_SEGMENT_64.to_le_bytes());
    d.extend_from_slice(&cmdsize.to_le_bytes());
    let mut segname = [0u8; 16];
    segname[..seg.len()].copy_from_slice(seg.as_bytes());
    d.extend_from_slice(&segname);
    d.extend_from_slice(&0x1000u64.to_le_bytes());
    d.extend_from_slice(&0x4000u64.to_le_bytes());
    d.extend_from_slice(&0u64.to_le_bytes());
    d.extend_from_slice(&0x4000u64.to_le_bytes());
    d.extend_from_slice(&7i32.to_le_bytes());
    d.extend_from_slice(&5i32.to_le_bytes());
    d.extend_from_slice(&1u32.to_le_bytes()); // nsects = 1
    d.extend_from_slice(&0u32.to_le_bytes());
    // section (80 bytes)
    let mut sectname = [0u8; 16];
    sectname[..sect.len()].copy_from_slice(sect.as_bytes());
    let mut s_segname = [0u8; 16];
    s_segname[..seg.len()].copy_from_slice(seg.as_bytes());
    d.extend_from_slice(&sectname);
    d.extend_from_slice(&s_segname);
    d.extend_from_slice(&0x1000u64.to_le_bytes()); // addr
    d.extend_from_slice(&0x800u64.to_le_bytes()); // size
    d.extend_from_slice(&0u32.to_le_bytes()); // offset
    d.extend_from_slice(&3u32.to_le_bytes()); // align
    d.extend_from_slice(&0u32.to_le_bytes()); // reloff
    d.extend_from_slice(&0u32.to_le_bytes()); // nreloc
    d.extend_from_slice(&0x80000400u32.to_le_bytes()); // flags
    d.extend_from_slice(&0u32.to_le_bytes()); // reserved1
    d.extend_from_slice(&0u32.to_le_bytes()); // reserved2
    d.extend_from_slice(&0u32.to_le_bytes()); // reserved3
    d
}

/// Build a dylib command (LC_LOAD_DYLIB / LC_ID_DYLIB / etc.)
fn lc_dylib(cmd: u32, name: &str) -> Vec<u8> {
    // header (24) + name (padded to 4)
    let name_off = 24u32;
    let name_bytes = name.as_bytes();
    let raw_len = 24 + name_bytes.len() + 1; // include null
    let cmdsize = raw_len.div_ceil(4) * 4;
    let mut d = Vec::new();
    d.extend_from_slice(&cmd.to_le_bytes());
    d.extend_from_slice(&(cmdsize as u32).to_le_bytes());
    d.extend_from_slice(&name_off.to_le_bytes()); // name offset
    d.extend_from_slice(&0u32.to_le_bytes()); // timestamp
    d.extend_from_slice(&0x00010203u32.to_le_bytes()); // current_version 1.2.3
    d.extend_from_slice(&0x00010000u32.to_le_bytes()); // compat_version 1.0.0
    d.extend_from_slice(name_bytes);
    d.push(0);
    while d.len() < cmdsize {
        d.push(0);
    }
    d
}

/// Build an LC_RPATH command.
fn lc_rpath(path: &str) -> Vec<u8> {
    let path_off = 12u32;
    let path_bytes = path.as_bytes();
    let raw_len = 12 + path_bytes.len() + 1;
    let cmdsize = raw_len.div_ceil(4) * 4;
    let mut d = Vec::new();
    d.extend_from_slice(&load_command::LC_RPATH.to_le_bytes());
    d.extend_from_slice(&(cmdsize as u32).to_le_bytes());
    d.extend_from_slice(&path_off.to_le_bytes());
    d.extend_from_slice(path_bytes);
    d.push(0);
    while d.len() < cmdsize {
        d.push(0);
    }
    d
}

/// Build an LC_VERSION_MIN_MACOSX command (16 bytes).
fn lc_version_min(cmd: u32, version: u32, sdk: u32) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&cmd.to_le_bytes());
    d.extend_from_slice(&16u32.to_le_bytes());
    d.extend_from_slice(&version.to_le_bytes());
    d.extend_from_slice(&sdk.to_le_bytes());
    d
}

/// Build an LC_BUILD_VERSION command with 1 tool (32 bytes:
/// cmd+cmdsize+platform+minos+sdk+ntools = 24, plus one tool entry = 8).
fn lc_build_version(plat: u32, minos: u32, sdk: u32) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&load_command::LC_BUILD_VERSION.to_le_bytes());
    d.extend_from_slice(&32u32.to_le_bytes());
    d.extend_from_slice(&plat.to_le_bytes());
    d.extend_from_slice(&minos.to_le_bytes());
    d.extend_from_slice(&sdk.to_le_bytes());
    d.extend_from_slice(&1u32.to_le_bytes()); // ntools
    d.extend_from_slice(&build_tool::TOOL_CLANG.to_le_bytes());
    d.extend_from_slice(&0x000F0000u32.to_le_bytes()); // tool version 15.0.0
    d
}

/// Build an LC_SOURCE_VERSION command (16 bytes).
fn lc_source_version(version: u64) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&load_command::LC_SOURCE_VERSION.to_le_bytes());
    d.extend_from_slice(&16u32.to_le_bytes());
    d.extend_from_slice(&version.to_le_bytes());
    d
}

/// Build an LC_MAIN command (24 bytes).
fn lc_main(entryoff: u64, stacksize: u64) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&load_command::LC_MAIN.to_le_bytes());
    d.extend_from_slice(&24u32.to_le_bytes());
    d.extend_from_slice(&entryoff.to_le_bytes());
    d.extend_from_slice(&stacksize.to_le_bytes());
    d
}

/// Build an LC_SYMTAB command (24 bytes).
fn lc_symtab(symoff: u32, nsyms: u32, stroff: u32, strsize: u32) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&load_command::LC_SYMTAB.to_le_bytes());
    d.extend_from_slice(&24u32.to_le_bytes());
    d.extend_from_slice(&symoff.to_le_bytes());
    d.extend_from_slice(&nsyms.to_le_bytes());
    d.extend_from_slice(&stroff.to_le_bytes());
    d.extend_from_slice(&strsize.to_le_bytes());
    d
}

/// Build an LC_DYSYMTAB command (80 bytes).
fn lc_dysymtab() -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&load_command::LC_DYSYMTAB.to_le_bytes());
    d.extend_from_slice(&80u32.to_le_bytes());
    // 18 u32 fields
    let vals: [u32; 18] = [0, 50, 50, 30, 80, 20, 0, 0, 0, 0, 0, 0, 0, 10, 0, 5, 0, 3];
    for v in vals {
        d.extend_from_slice(&v.to_le_bytes());
    }
    d
}

/// Build an LC_ENCRYPTION_INFO command (20 bytes).
fn lc_encryption_info(cryptid: u32) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&load_command::LC_ENCRYPTION_INFO.to_le_bytes());
    d.extend_from_slice(&20u32.to_le_bytes());
    d.extend_from_slice(&0x4000u32.to_le_bytes()); // cryptoff
    d.extend_from_slice(&0x8000u32.to_le_bytes()); // cryptsize
    d.extend_from_slice(&cryptid.to_le_bytes());
    d
}

/// Build a linkedit data command (e.g. LC_FUNCTION_STARTS).
fn lc_linkedit(cmd: u32, dataoff: u32, datasize: u32) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&cmd.to_le_bytes());
    d.extend_from_slice(&16u32.to_le_bytes());
    d.extend_from_slice(&dataoff.to_le_bytes());
    d.extend_from_slice(&datasize.to_le_bytes());
    d
}

/// Build an unknown load command (treated as Unknown).
fn lc_unknown(cmd: u32, cmdsize: u32) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&cmd.to_le_bytes());
    d.extend_from_slice(&cmdsize.to_le_bytes());
    while d.len() < cmdsize as usize {
        d.push(0);
    }
    d
}

/// Build a full 64-bit little-endian Mach-O header with the given commands.
fn macho_64(cputype: i32, cpusubtype: i32, filetype: u32, flags: u32, cmds: &[Vec<u8>]) -> Vec<u8> {
    let ncmds = cmds.len() as u32;
    let sizeofcmds: u32 = cmds.iter().map(|c| c.len() as u32).sum();
    let mut d = Vec::new();
    d.extend_from_slice(&magic::MH_MAGIC_64.to_le_bytes());
    d.extend_from_slice(&(cputype as u32).to_le_bytes());
    d.extend_from_slice(&(cpusubtype as u32).to_le_bytes());
    d.extend_from_slice(&filetype.to_le_bytes());
    d.extend_from_slice(&ncmds.to_le_bytes());
    d.extend_from_slice(&sizeofcmds.to_le_bytes());
    d.extend_from_slice(&flags.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes()); // reserved
    for c in cmds {
        d.extend_from_slice(c);
    }
    d
}

// =============================================================================
// Mach-O tests
// =============================================================================

#[test]
fn test_macho_minimal_uuid() {
    let cmds = vec![lc_uuid([
        0x55, 0x0E, 0x84, 0x00, 0xE2, 0x9B, 0x41, 0xD4, 0xA7, 0x16, 0x44, 0x66, 0x55, 0x44, 0x00,
        0x00,
    ])];
    let data = macho_64(
        cpu_type::CPU_TYPE_ARM64,
        cpu_subtype_arm64::CPU_SUBTYPE_ARM64E,
        file_type::MH_EXECUTE,
        mh_flags::MH_PIE | mh_flags::MH_TWOLEVEL | mh_flags::MH_DYLDLINK,
        &cmds,
    );
    let reader = TestReader::new(data);
    let md = parse_macho_metadata(&reader).expect("macho parse");
    assert_eq!(md.get_string("MachO:CPUType"), Some("ARM64"));
    assert_eq!(md.get_string("MachO:FileType"), Some("Executable"));
    assert_eq!(md.get_integer("MachO:Is64Bit"), Some(1));
    assert_eq!(md.get_integer("MachO:IsPIE"), Some(1));
    assert_eq!(
        md.get_string("MachO:UUID"),
        Some("550E8400-E29B-41D4-A716-446655440000")
    );
    assert!(md.contains_key("MachO:FlagsDecoded"));
    assert!(md.contains_key("MachO:CPUSubtype"));
}

#[test]
fn test_macho_full_dylib_executable() {
    let cmds = vec![
        lc_segment_64("__PAGEZERO", 0x100000000),
        lc_segment_64_with_section("__TEXT", "__text"),
        lc_segment_64("__DATA", 0x4000),
        lc_segment_64("__LINKEDIT", 0x8000),
        lc_dylib(load_command::LC_LOAD_DYLIB, "/usr/lib/libSystem.B.dylib"),
        lc_dylib(
            load_command::LC_LOAD_WEAK_DYLIB,
            "/System/Library/Frameworks/Foundation.framework/Foundation",
        ),
        lc_dylib(
            load_command::LC_REEXPORT_DYLIB,
            "@rpath/MyFramework.framework/MyFramework",
        ),
        lc_rpath("@executable_path/../Frameworks"),
        lc_version_min(load_command::LC_VERSION_MIN_MACOSX, 0x000F0000, 0x000F0100),
        lc_source_version((1 << 40) | (2 << 30) | (3 << 20) | (4 << 10) | 5),
        lc_main(0x4000, 0x100000),
        lc_symtab(0x1000, 100, 0x2000, 0x500),
        lc_dysymtab(),
        lc_encryption_info(1),
        lc_linkedit(load_command::LC_FUNCTION_STARTS, 0x5000, 0x100),
        lc_linkedit(load_command::LC_DATA_IN_CODE, 0x5100, 0x40),
        lc_unknown(load_command::LC_DYLD_ENVIRONMENT, 16),
    ];
    let data = macho_64(
        cpu_type::CPU_TYPE_X86_64,
        cpu_subtype_x86_64::CPU_SUBTYPE_X86_64_H,
        file_type::MH_EXECUTE,
        mh_flags::MH_PIE | mh_flags::MH_NO_HEAP_EXECUTION | mh_flags::MH_ALLOW_STACK_EXECUTION,
        &cmds,
    );
    let reader = TestReader::new(data);
    let md = parse_macho_metadata(&reader).expect("macho parse");

    assert_eq!(md.get_string("MachO:CPUType"), Some("x86_64"));
    assert!(md.contains_key("MachO:SegmentCount"));
    assert!(md.contains_key("MachO:SectionCount"));
    assert_eq!(md.get_integer("MachO:HasPagezero"), Some(1));
    assert!(md.contains_key("MachO:SegmentNames"));
    assert!(md.contains_key("MachO:DylibCount"));
    assert!(md.contains_key("MachO:DylibPaths"));
    assert!(md.contains_key("MachO:DylibNames"));
    assert_eq!(md.get_integer("MachO:ReexportDylibCount"), Some(1));
    assert_eq!(md.get_integer("MachO:WeakDylibCount"), Some(1));
    assert!(md.contains_key("MachO:Platform"));
    assert!(md.contains_key("MachO:MinOSVersion"));
    assert!(md.contains_key("MachO:SourceVersion"));
    assert!(md.contains_key("MachO:EntryPointOffset"));
    assert!(md.contains_key("MachO:StackSize"));
    assert!(md.contains_key("MachO:SymbolCount"));
    assert!(md.contains_key("MachO:RPathCount"));
    assert_eq!(md.get_integer("MachO:IsEncrypted"), Some(1));
    assert!(md.contains_key("MachO:EncryptionType"));
    assert_eq!(md.get_integer("MachO:AllowStackExecution"), Some(1));
}

#[test]
fn test_macho_dylib_with_id_and_build_version() {
    let cmds = vec![
        lc_dylib(load_command::LC_ID_DYLIB, "/usr/lib/libfoo.1.dylib"),
        lc_dylib(load_command::LC_LOAD_DYLIB, "/usr/lib/libbar.dylib"),
        lc_build_version(platform::PLATFORM_IOS, 0x00110000, 0x00110100),
    ];
    let data = macho_64(
        cpu_type::CPU_TYPE_ARM64,
        0,
        file_type::MH_DYLIB,
        mh_flags::MH_TWOLEVEL,
        &cmds,
    );
    let reader = TestReader::new(data);
    let md = parse_macho_metadata(&reader).expect("dylib parse");
    assert_eq!(md.get_string("MachO:FileType"), Some("Dynamic Library"));
    assert!(md.contains_key("MachO:DylibID"));
    assert!(md.contains_key("MachO:DylibCurrentVersion"));
    assert!(md.contains_key("MachO:DylibCompatVersion"));
    assert!(md.contains_key("MachO:BuildTools"));
    assert_eq!(md.get_string("MachO:Platform"), Some("iOS"));
}

#[test]
fn test_macho_with_code_signature() {
    // Build a code signature SuperBlob with a CodeDirectory + entitlements + CMS slot.
    let cs_blob = build_code_signature_blob();
    let cs_offset_in_file: u32; // computed after header assembly
    // We build a Mach-O whose __LINKEDIT segment is large enough and place the
    // signature blob at a known file offset via LC_CODE_SIGNATURE.
    let cmds_pre = vec![
        lc_segment_64("__TEXT", 0x4000),
        // placeholder for code signature command, filled later
    ];
    // header is 32 bytes; one segment is 72 bytes -> commands start at 32.
    // We'll append the cs command then the blob.
    let seg = lc_segment_64("__TEXT", 0x4000);
    let header_size = 32u32;
    let cs_cmd_size = 16u32;
    cs_offset_in_file = header_size + seg.len() as u32 + cs_cmd_size;
    let cs_cmd = lc_linkedit(
        load_command::LC_CODE_SIGNATURE,
        cs_offset_in_file,
        cs_blob.len() as u32,
    );
    let _ = cmds_pre;
    let cmds = vec![seg, cs_cmd];
    let mut data = macho_64(
        cpu_type::CPU_TYPE_ARM64,
        0,
        file_type::MH_EXECUTE,
        mh_flags::MH_PIE,
        &cmds,
    );
    // Append the signature blob at the file offset we declared.
    assert_eq!(data.len() as u32, cs_offset_in_file);
    data.extend_from_slice(&cs_blob);
    let reader = TestReader::new(data);
    let md = parse_macho_metadata(&reader).expect("signed macho");
    assert_eq!(md.get_integer("MachO:IsSigned"), Some(1));
    assert!(md.contains_key("MachO:CodeSignatureSize"));
    assert!(md.contains_key("MachO:HasEntitlements"));
}

/// Build a minimal embedded code signature SuperBlob with a CodeDirectory and
/// an entitlements blob and a CMS slot, all at known offsets.
fn build_code_signature_blob() -> Vec<u8> {
    use oxidex::parsers::macho::structures::cs_magic;

    // Layout:
    //   [SuperBlob header: magic(4) length(4) count(4)]
    //   [3 index entries: (blob_type(4) offset(4)) each]
    //   [CodeDirectory blob]
    //   [Entitlements blob]
    //   [CMS blob]
    let header_len = 12usize;
    let index_len = 3 * 8usize;
    let cd_off = (header_len + index_len) as u32;

    // CodeDirectory (version 0x20200 to include team_offset).
    let mut cd = Vec::new();
    let ident = b"com.example.app\0";
    let team = b"ABCDE12345\0";
    // We'll compute ident/team offsets relative to start of cd blob.
    // Fixed prefix of CD before identifier string region:
    //   magic(4)+length(4)+version(4)+flags(4)+hash_offset(4)+ident_offset(4)
    //   +n_special(4)+n_code(4)+code_limit(4)+hash_size(1)+hash_type(1)
    //   +platform(1)+page_size(1)+spare2(4)+scatter(4)+team_offset(4) = 52 bytes
    let cd_prefix = 52u32;
    let ident_off = cd_prefix;
    let team_off = cd_prefix + ident.len() as u32;
    cd.extend_from_slice(&cs_magic::CSMAGIC_CODEDIRECTORY.to_be_bytes());
    let cd_len = cd_prefix + ident.len() as u32 + team.len() as u32;
    cd.extend_from_slice(&cd_len.to_be_bytes()); // length
    cd.extend_from_slice(&0x00020200u32.to_be_bytes()); // version
    cd.extend_from_slice(&0u32.to_be_bytes()); // flags
    cd.extend_from_slice(&0u32.to_be_bytes()); // hash_offset
    cd.extend_from_slice(&ident_off.to_be_bytes()); // ident_offset
    cd.extend_from_slice(&0u32.to_be_bytes()); // n_special_slots
    cd.extend_from_slice(&3u32.to_be_bytes()); // n_code_slots
    cd.extend_from_slice(&0u32.to_be_bytes()); // code_limit
    cd.push(32); // hash_size
    cd.push(2); // hash_type = SHA-256
    cd.push(0); // platform
    cd.push(12); // page_size log2
    cd.extend_from_slice(&0u32.to_be_bytes()); // spare2
    cd.extend_from_slice(&0u32.to_be_bytes()); // scatter_offset (version>=0x20100)
    cd.extend_from_slice(&team_off.to_be_bytes()); // team_offset (version>=0x20200)
    cd.extend_from_slice(ident);
    cd.extend_from_slice(team);

    // Entitlements blob (CSMAGIC_EMBEDDED_ENTITLEMENTS = 0xFADE7171) with a plist.
    let plist = b"<?xml version=\"1.0\"?><plist><dict><key>com.apple.security.app-sandbox</key><true/><key>keychain-access-groups</key></dict></plist>";
    let mut ent = Vec::new();
    let ent_len = 8u32 + plist.len() as u32;
    ent.extend_from_slice(&0xFADE7171u32.to_be_bytes()); // magic
    ent.extend_from_slice(&ent_len.to_be_bytes()); // length
    ent.extend_from_slice(plist);

    // CMS blob (CSMAGIC_BLOBWRAPPER) with a DER CN OID + printable string.
    let mut cms = Vec::new();
    let cms_payload: Vec<u8> = {
        let mut p = Vec::new();
        // pad some bytes then the CN OID followed by a printable string
        p.extend_from_slice(&[0u8; 24]);
        p.extend_from_slice(&[0x06, 0x03, 0x55, 0x04, 0x03]); // OID CN
        p.extend_from_slice(&[0x13, 0x0E]); // PrintableString len 14
        p.extend_from_slice(b"Developer ID A"); // 14 chars
        p
    };
    let cms_len = 8u32 + cms_payload.len() as u32;
    cms.extend_from_slice(&cs_magic::CSMAGIC_BLOBWRAPPER.to_be_bytes());
    cms.extend_from_slice(&cms_len.to_be_bytes());
    cms.extend_from_slice(&cms_payload);

    let ent_off = cd_off + cd.len() as u32;
    let cms_off = ent_off + ent.len() as u32;
    let total_len = cms_off + cms.len() as u32;

    let mut blob = Vec::new();
    blob.extend_from_slice(&cs_magic::CSMAGIC_EMBEDDED_SIGNATURE.to_be_bytes());
    blob.extend_from_slice(&total_len.to_be_bytes());
    blob.extend_from_slice(&3u32.to_be_bytes()); // count
    // index entries
    blob.extend_from_slice(&0u32.to_be_bytes()); // CSSLOT_CODEDIRECTORY
    blob.extend_from_slice(&cd_off.to_be_bytes());
    blob.extend_from_slice(&5u32.to_be_bytes()); // CSSLOT_ENTITLEMENTS
    blob.extend_from_slice(&ent_off.to_be_bytes());
    blob.extend_from_slice(&0x10000u32.to_be_bytes()); // CSSLOT_SIGNATURESLOT
    blob.extend_from_slice(&cms_off.to_be_bytes());
    blob.extend_from_slice(&cd);
    blob.extend_from_slice(&ent);
    blob.extend_from_slice(&cms);
    blob
}

#[test]
fn test_macho_code_signature_parsers_direct() {
    let blob = build_code_signature_blob();
    // parse_super_blob
    let (_, sb) = parse_super_blob(&blob).expect("super blob");
    assert_eq!(sb.count, 3);
    assert_eq!(sb.index.len(), 3);
    // parse the code directory directly at its offset
    let cd_off = sb.index[0].offset as usize;
    let (_, cd) = parse_code_directory(&blob[cd_off..]).expect("cd");
    assert_eq!(cd.identifier, "com.example.app");
    assert_eq!(cd.team_id.as_deref(), Some("ABCDE12345"));
    assert_eq!(hash_type_name(cd.hash_type), "SHA-256");
    // parse_code_signature_info (full)
    let info = parse_code_signature_info(&blob).expect("cs info");
    assert!(info.is_signed);
    assert!(info.has_entitlements);
    assert!(info.has_cms_signature);
    assert_eq!(info.identifier.as_deref(), Some("com.example.app"));
    assert!(is_adhoc_signed(
        &oxidex::parsers::macho::structures::CodeSignatureInfo {
            is_signed: true,
            has_cms_signature: false,
            ..Default::default()
        }
    ));
    assert!(!is_adhoc_signed(&info));
    assert!(has_developer_id(&info));
    // too-short data returns None
    assert!(parse_code_signature_info(&[0u8; 4]).is_none());
    // non-superblob magic
    let not_sig = vec![0u8; 32];
    let info2 = parse_code_signature_info(&not_sig).expect("info2");
    assert!(!info2.is_signed);
    let flags = decode_cs_flags(0x0000_0001 | 0x0000_0100 | 0x0001_0000);
    assert!(flags.contains(&"VALID"));
    assert!(flags.contains(&"RUNTIME"));
}

#[test]
fn test_macho_fat_binary() {
    // FAT header (big-endian) with 2 architectures: x86_64 and ARM64.
    // Each arch is 20 bytes (32-bit fat arch). The ARM64 slice contains a real
    // Mach-O; the x86_64 slice offset points elsewhere.
    let inner = macho_64(
        cpu_type::CPU_TYPE_ARM64,
        0,
        file_type::MH_EXECUTE,
        mh_flags::MH_PIE,
        &[lc_uuid([0xAA; 16])],
    );

    // Build the FAT container. Layout:
    //   [fat_header 8][arch0 20][arch1 20][padding...][arm64 mach-o][x86 filler]
    let header_len = 8u32;
    let archs_len = 2 * 20u32;
    let table_end = header_len + archs_len;
    // Place arm64 slice at an aligned offset.
    let arm_off = 0x1000u32;
    let x86_off = arm_off + 0x1000;

    let mut data = Vec::new();
    data.extend_from_slice(&magic::FAT_MAGIC.to_be_bytes());
    data.extend_from_slice(&2u32.to_be_bytes()); // nfat_arch
    // arch0: x86_64
    data.extend_from_slice(&(cpu_type::CPU_TYPE_X86_64).to_be_bytes());
    data.extend_from_slice(&0i32.to_be_bytes());
    data.extend_from_slice(&x86_off.to_be_bytes());
    data.extend_from_slice(&0x100u32.to_be_bytes());
    data.extend_from_slice(&12u32.to_be_bytes());
    // arch1: arm64 (preferred)
    data.extend_from_slice(&(cpu_type::CPU_TYPE_ARM64).to_be_bytes());
    data.extend_from_slice(&0i32.to_be_bytes());
    data.extend_from_slice(&arm_off.to_be_bytes());
    data.extend_from_slice(&(inner.len() as u32).to_be_bytes());
    data.extend_from_slice(&14u32.to_be_bytes());
    // pad up to arm_off
    while data.len() < arm_off as usize {
        data.push(0);
    }
    data.extend_from_slice(&inner);
    // pad up to x86_off + some filler
    while data.len() < (x86_off as usize + 0x100) {
        data.push(0);
    }
    let _ = table_end;

    let reader = TestReader::new(data);
    let md = parse_macho_metadata(&reader).expect("fat macho");
    assert_eq!(md.get_integer("MachO:IsFromUniversalBinary"), Some(1));
    assert!(md.contains_key("MachO:UniversalArchCount"));
    assert!(md.contains_key("MachO:UniversalArchitectures"));
    assert_eq!(
        md.get_string("MachO:UUID"),
        Some("AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA")
    );
}

#[test]
fn test_macho_errors() {
    // Too small
    let small = TestReader::new(vec![0xFE, 0xED]);
    assert!(parse_macho_metadata(&small).is_err());
    // Wrong magic
    let bad = TestReader::new(vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07]);
    assert!(parse_macho_metadata(&bad).is_err());
    // verify_signature behavior
    let ok = macho_64(cpu_type::CPU_TYPE_ARM64, 0, file_type::MH_OBJECT, 0, &[]);
    let reader = TestReader::new(ok);
    assert!(oxidex::parsers::macho::MachOParser::verify_signature(&reader).unwrap());
    let too_small = TestReader::new(vec![0x00]);
    assert!(!oxidex::parsers::macho::MachOParser::verify_signature(&too_small).unwrap());
}

#[test]
fn test_macho_header_and_fat_parsers_direct() {
    // is_macho_magic / is_fat_magic
    assert!(is_macho_magic(&[0xCF, 0xFA, 0xED, 0xFE]));
    assert!(is_macho_magic(&[0xFE, 0xED, 0xFA, 0xCF]));
    assert!(is_macho_magic(&[0xCA, 0xFE, 0xBA, 0xBE]));
    assert!(!is_macho_magic(&[0x00, 0x00, 0x00, 0x00]));
    assert!(!is_macho_magic(&[0x01, 0x02]));
    assert!(is_fat_magic(&[0xBE, 0xBA, 0xFE, 0xCA]));
    assert!(!is_fat_magic(&[0xFE, 0xED, 0xFA, 0xCF]));
    assert_eq!(macho_header_size(true), 32);
    assert_eq!(macho_header_size(false), 28);
    assert_eq!(fat_arch_size(true), 32);
    assert_eq!(fat_arch_size(false), 20);

    // parse_mach_header for 32-bit LE magic
    let mut hdr = Vec::new();
    hdr.extend_from_slice(&magic::MH_MAGIC.to_le_bytes());
    hdr.extend_from_slice(&7u32.to_le_bytes()); // i386
    hdr.extend_from_slice(&3u32.to_le_bytes());
    hdr.extend_from_slice(&file_type::MH_EXECUTE.to_le_bytes());
    hdr.extend_from_slice(&2u32.to_le_bytes());
    hdr.extend_from_slice(&100u32.to_le_bytes());
    hdr.extend_from_slice(&0x85u32.to_le_bytes());
    let (_, mh) = parse_mach_header(&hdr).expect("32-bit header");
    assert!(!mh.is_64bit);
    assert_eq!(mh.cpu_type_name(), "i386");

    // parse_mach_header rejects non-mach magic
    let bad = [0u8; 8];
    assert!(parse_mach_header(&bad).is_err());

    // FAT header big-endian + 64-bit arch entries
    let mut fat = Vec::new();
    fat.extend_from_slice(&magic::FAT_MAGIC_64.to_be_bytes());
    fat.extend_from_slice(&1u32.to_be_bytes());
    let (_, fh) = parse_fat_header(&fat).expect("fat64 header");
    assert!(fh.is_64bit);

    // parse a single 64-bit fat arch (big-endian)
    let mut arch64 = Vec::new();
    arch64.extend_from_slice(&(cpu_type::CPU_TYPE_ARM64).to_be_bytes());
    arch64.extend_from_slice(&0i32.to_be_bytes());
    arch64.extend_from_slice(&0x4000u64.to_be_bytes());
    arch64.extend_from_slice(&0x10000u64.to_be_bytes());
    arch64.extend_from_slice(&14u32.to_be_bytes());
    arch64.extend_from_slice(&0u32.to_be_bytes()); // reserved
    let (_, a64) = parse_fat_arch_64(&arch64, false).expect("fat arch 64");
    assert_eq!(a64.cpu_type_name(), "ARM64");

    // parse a single 32-bit fat arch (little-endian/swapped)
    let mut arch32 = Vec::new();
    arch32.extend_from_slice(&(cpu_type::CPU_TYPE_X86_64).to_le_bytes());
    arch32.extend_from_slice(&0i32.to_le_bytes());
    arch32.extend_from_slice(&0x4000u32.to_le_bytes());
    arch32.extend_from_slice(&0x10000u32.to_le_bytes());
    arch32.extend_from_slice(&14u32.to_le_bytes());
    let (_, a32) = parse_fat_arch_32(&arch32, true).expect("fat arch 32 swapped");
    assert_eq!(a32.cpu_type_name(), "x86_64");

    // parse_fat_archs over a vec
    let mut archs_data = Vec::new();
    archs_data.extend_from_slice(&arch64);
    let (_, archs) = parse_fat_archs(&archs_data, 1, true, false).expect("fat archs");
    assert_eq!(archs.len(), 1);
}

#[test]
fn test_macho_load_command_parsers_direct() {
    // header
    let mut h = Vec::new();
    h.extend_from_slice(&load_command::LC_SEGMENT_64.to_le_bytes());
    h.extend_from_slice(&72u32.to_le_bytes());
    let (_, lch) = parse_load_command_header(&h).expect("lc header");
    assert_eq!(lch.cmd, load_command::LC_SEGMENT_64);

    // segment 64
    let seg = lc_segment_64_with_section("__TEXT", "__text");
    let (_, scmd) = parse_segment_command_64(&seg).expect("seg64");
    assert_eq!(scmd.segname, "__TEXT");
    assert_eq!(scmd.sections.len(), 1);

    // segment 32
    let mut seg32 = Vec::new();
    seg32.extend_from_slice(&load_command::LC_SEGMENT.to_le_bytes());
    seg32.extend_from_slice(&56u32.to_le_bytes());
    let mut name = [0u8; 16];
    name[..7].copy_from_slice(b"__DATA\0");
    seg32.extend_from_slice(&name);
    seg32.extend_from_slice(&0x2000u32.to_le_bytes()); // vmaddr
    seg32.extend_from_slice(&0x1000u32.to_le_bytes()); // vmsize
    seg32.extend_from_slice(&0u32.to_le_bytes()); // fileoff
    seg32.extend_from_slice(&0x1000u32.to_le_bytes()); // filesize
    seg32.extend_from_slice(&7i32.to_le_bytes());
    seg32.extend_from_slice(&3i32.to_le_bytes());
    seg32.extend_from_slice(&0u32.to_le_bytes()); // nsects
    seg32.extend_from_slice(&0u32.to_le_bytes()); // flags
    let (_, scmd32) = parse_segment_command_32(&seg32).expect("seg32");
    assert_eq!(scmd32.segname, "__DATA");

    // dylib
    let dyl = lc_dylib(load_command::LC_LOAD_DYLIB, "/usr/lib/libc.dylib");
    let (_, dcmd) = parse_dylib_command(&dyl).expect("dylib");
    assert_eq!(dcmd.name, "/usr/lib/libc.dylib");
    assert_eq!(dcmd.current_version_string(), "1.2.3");

    // uuid
    let u = lc_uuid([0x11; 16]);
    let (_, ucmd) = parse_uuid_command(&u).expect("uuid");
    assert_eq!(ucmd.uuid[0], 0x11);

    // version min
    let vm = lc_version_min(
        load_command::LC_VERSION_MIN_IPHONEOS,
        0x00100000,
        0x00100000,
    );
    let (_, vcmd) = parse_version_min_command(&vm).expect("vmin");
    assert_eq!(vcmd.platform_name(), "iOS");

    // build version
    let bv = lc_build_version(platform::PLATFORM_MACOS, 0x000E0000, 0x000E0100);
    let (_, bcmd) = parse_build_version_command(&bv).expect("bversion");
    assert_eq!(bcmd.platform_name(), "macOS");
    assert_eq!(bcmd.tools.len(), 1);

    // source version
    let sv = lc_source_version((9 << 40) | 1);
    let (_, svcmd) = parse_source_version_command(&sv).expect("sversion");
    assert!(svcmd.version_string().starts_with("9."));

    // entry point
    let ep = lc_main(0x100, 0x4000);
    let (_, ecmd) = parse_entry_point_command(&ep).expect("entry");
    assert_eq!(ecmd.entryoff, 0x100);

    // symtab
    let st = lc_symtab(1, 2, 3, 4);
    let (_, stcmd) = parse_symtab_command(&st).expect("symtab");
    assert_eq!(stcmd.nsyms, 2);

    // dysymtab
    let dys = lc_dysymtab();
    let (_, dyscmd) = parse_dysymtab_command(&dys).expect("dysymtab");
    assert_eq!(dyscmd.nlocalsym, 50);

    // linkedit
    let le = lc_linkedit(load_command::LC_CODE_SIGNATURE, 0x10, 0x20);
    let (_, lecmd) = parse_linkedit_data_command(&le).expect("linkedit");
    assert_eq!(lecmd.datasize, 0x20);

    // rpath
    let rp = lc_rpath("@loader_path/lib");
    let (_, rpcmd) = parse_rpath_command(&rp).expect("rpath");
    assert_eq!(rpcmd.path, "@loader_path/lib");

    // encryption info (32 + 64)
    let enc = lc_encryption_info(2);
    let (_, ecmd) = parse_encryption_info_command(&enc, false).expect("enc");
    assert_eq!(ecmd.cryptid, 2);
    let mut enc64 = lc_encryption_info(3);
    // 64-bit variant has a trailing pad u32
    enc64.extend_from_slice(&0u32.to_le_bytes());
    let (_, ecmd64) = parse_encryption_info_command(&enc64, true).expect("enc64");
    assert_eq!(ecmd64.cryptid, 3);

    // single parse_load_command dispatch + unknown
    let unk = lc_unknown(0x12345678, 16);
    let (_, lc) = parse_load_command(&unk, true).expect("unknown lc");
    assert!(matches!(lc, LoadCommand::Unknown(_)));

    // parse_all_load_commands across a multi-command buffer
    let mut all = Vec::new();
    let uuid_cmd = lc_uuid([0x22; 16]);
    let main_cmd = lc_main(1, 2);
    all.extend_from_slice(&uuid_cmd);
    all.extend_from_slice(&main_cmd);
    let (_, cmds) = parse_all_load_commands(&all, 2, true).expect("all cmds");
    assert_eq!(cmds.len(), 2);

    // parse_load_command too-small triggers error path
    let truncated = vec![0x1Bu8, 0, 0, 0, 0xFF, 0xFF, 0, 0];
    assert!(parse_load_command(&truncated, true).is_err());
}

#[test]
fn test_macho_extractor_and_populate() {
    // Drive populate_macho_info + extract_macho_metadata via parsed commands.
    let cmds_data = {
        let mut v = Vec::new();
        for c in [
            lc_segment_64("__TEXT", 0x1000),
            lc_dylib(load_command::LC_LOAD_DYLIB, "/usr/lib/libz.dylib"),
            lc_uuid([0x33; 16]),
            lc_version_min(load_command::LC_VERSION_MIN_TVOS, 0x000F0000, 0x000F0000),
            lc_rpath("@rpath"),
            lc_encryption_info(0),
        ] {
            v.extend_from_slice(&c);
        }
        v
    };
    let (_, commands) = parse_all_load_commands(&cmds_data, 6, true).expect("cmds");
    let mut info = MachOInfo::new();
    info.header = Some(MachHeader {
        magic: magic::MH_MAGIC_64,
        cputype: cpu_type::CPU_TYPE_POWERPC64,
        cpusubtype: 0,
        filetype: file_type::MH_BUNDLE,
        ncmds: 6,
        sizeofcmds: cmds_data.len() as u32,
        flags: mh_flags::MH_NOUNDEFS,
        reserved: 0,
        is_64bit: true,
        is_swapped: false,
    });
    populate_macho_info(&mut info, &commands);
    let md = extract_macho_metadata(&info);
    assert_eq!(md.get_string("MachO:CPUType"), Some("PowerPC64"));
    assert_eq!(md.get_string("MachO:FileType"), Some("Bundle"));
    assert!(md.contains_key("MachO:SegmentCount"));
    assert!(md.contains_key("MachO:DylibCount"));
    assert_eq!(
        md.get("MachO:UUID"),
        Some(&TagValue::String(
            "33333333-3333-3333-3333-333333333333".to_string()
        ))
    );
    // encryption with cryptid==0 means IsEncrypted=0
    assert_eq!(md.get_integer("MachO:IsEncrypted"), Some(0));

    // UuidCommand and SourceVersionCommand helper methods.
    let uc = UuidCommand { uuid: [0xFF; 16] };
    assert_eq!(uc.uuid_string(), "FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF");
    let sv = SourceVersionCommand {
        version: (1 << 40) | (2 << 30) | (3 << 20) | (4 << 10) | 5,
    };
    assert_eq!(sv.version_string(), "1.2.3.4.5");
}

#[test]
fn test_macho_structures_helpers() {
    // file_type_name / platform_name / load_command_name / decode_flags
    assert_eq!(file_type_name(file_type::MH_DYLINKER), "Dynamic Linker");
    assert_eq!(file_type_name(0xFFFF), "Unknown");
    assert_eq!(platform_name(platform::PLATFORM_WATCHOS), "watchOS");
    assert_eq!(platform_name(0xFFFF), "Unknown");
    assert_eq!(
        load_command_name(load_command::LC_LOAD_WEAK_DYLIB),
        "LC_LOAD_WEAK_DYLIB"
    );
    assert_eq!(load_command_name(0xDEAD), "LC_UNKNOWN");
    let names = decode_flags(
        mh_flags::MH_PIE
            | mh_flags::MH_TWOLEVEL
            | mh_flags::MH_WEAK_DEFINES
            | mh_flags::MH_BINDS_TO_WEAK
            | mh_flags::MH_APP_EXTENSION_SAFE,
    );
    assert!(names.contains(&"PIE"));
    assert!(names.contains(&"WEAK_DEFINES"));
    assert!(names.contains(&"APP_EXTENSION_SAFE"));

    // MachHeader subtype names for several cpu types
    let h = MachHeader {
        magic: magic::MH_MAGIC_64,
        cputype: cpu_type::CPU_TYPE_ARM64,
        cpusubtype: cpu_subtype_arm64::CPU_SUBTYPE_ARM64E,
        filetype: file_type::MH_EXECUTE,
        ncmds: 0,
        sizeofcmds: 0,
        flags: 0,
        reserved: 0,
        is_64bit: true,
        is_swapped: false,
    };
    assert_eq!(h.cpu_subtype_name(), "ARM64E");
    assert_eq!(h.header_size(), 32);
}

#[test]
fn test_macho_dylib_segment_symbol_version_helpers() {
    // dylib helpers
    assert_eq!(
        DylibType::from_cmd(load_command::LC_LOAD_DYLIB),
        DylibType::Regular
    );
    assert_eq!(
        DylibType::from_cmd(load_command::LC_ID_DYLIB).name(),
        "Library ID"
    );
    assert_eq!(DylibType::from_cmd(0x999).name(), "Unknown");
    assert_eq!(
        DylibCategory::from_path("/usr/lib/libSystem.dylib"),
        DylibCategory::SystemLibrary
    );
    assert_eq!(DylibCategory::from_path("foo.dylib").name(), "Unknown");
    assert_eq!(
        DylibCategory::from_path("@loader_path/x").name(),
        "Loader Relative"
    );
    assert!(is_system_dylib(
        "/System/Library/PrivateFrameworks/X.framework/X"
    ));
    assert!(!is_system_dylib("@rpath/Y.framework/Y"));
    assert_eq!(
        extract_library_name("/usr/lib/libfoo.dylib"),
        "libfoo.dylib"
    );
    assert_eq!(
        extract_library_name("/System/Library/Frameworks/Foundation.framework/Foundation"),
        "Foundation"
    );

    use oxidex::parsers::macho::structures::DylibCommand;
    let dylibs = vec![
        DylibCommand {
            cmd: load_command::LC_LOAD_DYLIB,
            name: "/usr/lib/a.dylib".to_string(),
            timestamp: 0,
            current_version: 0x00010000,
            compatibility_version: 0x00010000,
        },
        DylibCommand {
            cmd: load_command::LC_ID_DYLIB,
            name: "self.dylib".to_string(),
            timestamp: 0,
            current_version: 0x00010000,
            compatibility_version: 0x00010000,
        },
        DylibCommand {
            cmd: load_command::LC_LOAD_UPWARD_DYLIB,
            name: "/usr/lib/b.dylib".to_string(),
            timestamp: 0,
            current_version: 0x00010000,
            compatibility_version: 0x00010000,
        },
    ];
    let stats = DylibStats::from_dylibs(&dylibs);
    assert_eq!(stats.dylib_count, 3);
    assert_eq!(stats.upward_count, 1);
    assert_eq!(stats.id_dylib.as_deref(), Some("self.dylib"));
    assert_eq!(get_dylib_paths(&dylibs).len(), 2);
    assert_eq!(get_dylib_names(&dylibs).len(), 2);
    let cats = categorize_dylibs(&dylibs);
    assert!(cats.contains_key(&DylibType::Regular));

    // segment helpers
    use oxidex::parsers::macho::structures::{Section, SegmentCommand};
    let sect = Section {
        sectname: "__text".to_string(),
        segname: "__TEXT".to_string(),
        addr: 0,
        size: 0x100,
        offset: 0,
        align: 0,
        reloff: 0,
        nreloc: 0,
        flags: section_attrs::S_ATTR_PURE_INSTRUCTIONS,
        reserved1: 0,
        reserved2: 0,
        reserved3: 0,
    };
    let seg = SegmentCommand {
        segname: "__TEXT".to_string(),
        vmaddr: 0,
        vmsize: 0x1000,
        fileoff: 0,
        filesize: 0x1000,
        maxprot: 7,
        initprot: 5,
        nsects: 1,
        flags: 0,
        sections: vec![sect],
    };
    let segs = vec![seg];
    let sstats = SegmentStats::from_segments(&segs);
    assert_eq!(sstats.text_size, 0x1000);
    assert!(find_section(&segs, "__TEXT", "__text").is_some());
    assert!(find_section(&segs, "__TEXT", "__nope").is_none());
    assert_eq!(get_segment_names(&segs), vec!["__TEXT".to_string()]);
    assert_eq!(get_section_names(&segs), vec!["__TEXT,__text".to_string()]);
    assert_eq!(section_type(0x80000400), 0);
    assert_eq!(section_type_name(section_type::S_ZEROFILL), "Zerofill");
    assert_eq!(section_type_name(0xFF), "Unknown");
    let attrs =
        decode_section_attrs(section_attrs::S_ATTR_PURE_INSTRUCTIONS | section_attrs::S_ATTR_DEBUG);
    assert!(attrs.contains(&"PURE_INSTRUCTIONS"));
    assert!(attrs.contains(&"DEBUG"));

    // symbol helpers
    use oxidex::parsers::macho::structures::{DysymtabCommand, SymtabCommand};
    let symtab = SymtabCommand {
        symoff: 0,
        nsyms: 10,
        stroff: 0,
        strsize: 200,
    };
    let dysym = DysymtabCommand {
        ilocalsym: 0,
        nlocalsym: 4,
        iextdefsym: 4,
        nextdefsym: 3,
        iundefsym: 7,
        nundefsym: 3,
        tocoff: 0,
        ntoc: 0,
        modtaboff: 0,
        nmodtab: 0,
        extrefsymoff: 0,
        nextrefsyms: 0,
        indirectsymoff: 0,
        nindirectsyms: 2,
        extreloff: 0,
        nextrel: 1,
        locreloff: 0,
        nlocrel: 1,
    };
    let symstats = SymbolStats::from_commands(&symtab, Some(&dysym));
    assert_eq!(symstats.total_symbols, 10);
    assert_eq!(symstats.external_symbols, 3);
    assert_eq!(
        symbol_type_name(macho_n_type::N_SECT | 0x01),
        "Defined External"
    );
    assert_eq!(symbol_type_name(0xE0), "Debug (STAB)");
    assert!(is_external(macho_n_type::N_SECT | 0x01));
    assert!(is_undefined(macho_n_type::N_UNDF));
    assert_eq!(
        SymbolCategory::from_type_desc(macho_n_type::N_UNDF | 0x01, 0),
        SymbolCategory::Import
    );
    assert_eq!(
        SymbolCategory::from_type_desc(macho_n_type::N_SECT, n_desc::N_WEAK_DEF).name(),
        "Weak"
    );
    let descs = decode_n_desc(n_desc::N_WEAK_REF | n_desc::N_ARM_THUMB_DEF);
    assert!(descs.contains(&"WEAK_REF"));
    assert_eq!(get_library_ordinal(0x0500), 5);
    assert!(is_mangled_name("_ZN3fooEv"));
    assert!(is_swift_symbol("$s4mainAAV"));
    assert!(is_cpp_symbol("__ZN3barEv"));
    assert!(is_objc_method("-[NSObject init]"));
    assert_eq!(detect_language("+[X y]"), "Objective-C");
    assert_eq!(detect_language("$sfoo"), "Swift");
    assert_eq!(detect_language("_main"), "C");

    // version helpers
    assert_eq!(parse_version(0x000B0100), (11, 1, 0));
    assert_eq!(parse_source_version((1 << 40) | (2 << 30)), (1, 2, 0, 0, 0));
    assert_eq!(
        compare_versions("11.0", "10.15"),
        std::cmp::Ordering::Greater
    );
    assert!(meets_min_version("11.0.0", "10.0.0"));
    assert_eq!(macos_version_name("10.15.0"), Some("Catalina"));
    assert_eq!(ios_version_name("17.0"), Some("iOS 17"));
    assert_eq!(
        format_version_with_name("macOS", "14.0.0"),
        "14.0.0 (Sonoma)"
    );
    assert_eq!(format_version_with_name("tvOS", "1.0"), "1.0");
    let vi =
        VersionInfo::from_version_min(&oxidex::parsers::macho::structures::VersionMinCommand {
            cmd: load_command::LC_VERSION_MIN_MACOSX,
            version: 0x000B0000,
            sdk: 0x000C0000,
        });
    assert_eq!(vi.platform.as_deref(), Some("macOS"));
}

#[test]
fn test_macho_read_metadata_production_path() {
    // Drive detection + dispatch via read_metadata on a tempfile.
    let cmds = vec![
        lc_segment_64_with_section("__TEXT", "__text"),
        lc_uuid([0x77; 16]),
        lc_dylib(load_command::LC_LOAD_DYLIB, "/usr/lib/libSystem.B.dylib"),
        lc_main(0x4000, 0),
    ];
    let data = macho_64(
        cpu_type::CPU_TYPE_ARM64,
        0,
        file_type::MH_EXECUTE,
        mh_flags::MH_PIE | mh_flags::MH_TWOLEVEL,
        &cmds,
    );
    let mut tmp = NamedTempFile::with_suffix("").unwrap();
    tmp.write_all(&data).unwrap();
    tmp.flush().unwrap();
    let md = read_metadata(tmp.path()).expect("read_metadata macho");
    assert_eq!(md.get_string("MachO:CPUType"), Some("ARM64"));
    assert_eq!(md.get_string("MachO:FileFormat"), Some("Mach-O"));
}

// =============================================================================
// PE fixture builders
// =============================================================================

/// Build a 64-byte DOS header with e_lfanew at a chosen offset.
fn dos_header(e_lfanew: u32) -> Vec<u8> {
    let mut d = vec![0u8; 64];
    d[0] = b'M';
    d[1] = b'Z';
    d[0x3C..0x40].copy_from_slice(&e_lfanew.to_le_bytes());
    d
}

/// Build a COFF header (20 bytes) after the PE signature.
fn coff_header(machine: u16, n_sections: u16, opt_size: u16, characteristics: u16) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&machine.to_le_bytes());
    d.extend_from_slice(&n_sections.to_le_bytes());
    d.extend_from_slice(&0x5F00_0000u32.to_le_bytes()); // time_date_stamp (nonzero)
    d.extend_from_slice(&0u32.to_le_bytes()); // pointer_to_symbol_table
    d.extend_from_slice(&0u32.to_le_bytes()); // number_of_symbols
    d.extend_from_slice(&opt_size.to_le_bytes());
    d.extend_from_slice(&characteristics.to_le_bytes());
    d
}

/// Build a PE32+ optional header with 16 data directories.
/// `data_dirs` provides (rva, size) entries; missing entries are zero-filled.
fn optional_header_pe32plus(
    subsystem: u16,
    dll_characteristics: u16,
    data_dirs: &[(usize, u32, u32)],
) -> Vec<u8> {
    let mut d = Vec::new();
    // Standard fields
    d.extend_from_slice(&0x020Bu16.to_le_bytes()); // magic PE32+
    d.push(14); // major linker
    d.push(0); // minor linker
    d.extend_from_slice(&0x1000u32.to_le_bytes()); // size_of_code
    d.extend_from_slice(&0x2000u32.to_le_bytes()); // size_of_initialized_data
    d.extend_from_slice(&0u32.to_le_bytes()); // size_of_uninitialized_data
    d.extend_from_slice(&0x1000u32.to_le_bytes()); // address_of_entry_point
    d.extend_from_slice(&0x1000u32.to_le_bytes()); // base_of_code
    // NT fields (PE32+ => image_base is u64, no base_of_data)
    d.extend_from_slice(&0x140000000u64.to_le_bytes()); // image_base
    d.extend_from_slice(&0x1000u32.to_le_bytes()); // section_alignment
    d.extend_from_slice(&0x200u32.to_le_bytes()); // file_alignment
    d.extend_from_slice(&10u16.to_le_bytes()); // major OS
    d.extend_from_slice(&0u16.to_le_bytes()); // minor OS
    d.extend_from_slice(&1u16.to_le_bytes()); // major image
    d.extend_from_slice(&0u16.to_le_bytes()); // minor image
    d.extend_from_slice(&10u16.to_le_bytes()); // major subsystem
    d.extend_from_slice(&0u16.to_le_bytes()); // minor subsystem
    d.extend_from_slice(&0u32.to_le_bytes()); // win32_version
    d.extend_from_slice(&0x10000u32.to_le_bytes()); // size_of_image
    d.extend_from_slice(&0x400u32.to_le_bytes()); // size_of_headers
    d.extend_from_slice(&0x12345u32.to_le_bytes()); // checksum
    d.extend_from_slice(&subsystem.to_le_bytes()); // subsystem
    d.extend_from_slice(&dll_characteristics.to_le_bytes()); // dll_characteristics
    d.extend_from_slice(&0x100000u64.to_le_bytes()); // stack reserve
    d.extend_from_slice(&0x1000u64.to_le_bytes()); // stack commit
    d.extend_from_slice(&0x100000u64.to_le_bytes()); // heap reserve
    d.extend_from_slice(&0x1000u64.to_le_bytes()); // heap commit
    d.extend_from_slice(&0u32.to_le_bytes()); // loader_flags
    d.extend_from_slice(&16u32.to_le_bytes()); // number_of_rva_and_sizes
    // 16 data directories
    let mut dirs = [(0u32, 0u32); 16];
    for &(idx, rva, size) in data_dirs {
        dirs[idx] = (rva, size);
    }
    for (rva, size) in dirs {
        d.extend_from_slice(&rva.to_le_bytes());
        d.extend_from_slice(&size.to_le_bytes());
    }
    d
}

/// Build a 40-byte section header.
fn section_header(name: &str, vsize: u32, vaddr: u32, raw_size: u32, raw_ptr: u32) -> Vec<u8> {
    let mut d = Vec::new();
    let mut name_arr = [0u8; 8];
    let b = name.as_bytes();
    name_arr[..b.len().min(8)].copy_from_slice(&b[..b.len().min(8)]);
    d.extend_from_slice(&name_arr);
    d.extend_from_slice(&vsize.to_le_bytes());
    d.extend_from_slice(&vaddr.to_le_bytes());
    d.extend_from_slice(&raw_size.to_le_bytes());
    d.extend_from_slice(&raw_ptr.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes()); // ptr to relocations
    d.extend_from_slice(&0u32.to_le_bytes()); // ptr to line numbers
    d.extend_from_slice(&0u16.to_le_bytes()); // num relocations
    d.extend_from_slice(&0u16.to_le_bytes()); // num line numbers
    d.extend_from_slice(&0x60000020u32.to_le_bytes()); // characteristics
    d
}

// =============================================================================
// PE tests
// =============================================================================

#[test]
fn test_pe_minimal_pe32plus() {
    // Assemble: DOS(64) [stub padding to e_lfanew] PE\0\0 COFF OptHeader Sections
    let e_lfanew = 0x80u32;
    let mut data = dos_header(e_lfanew);
    // pad DOS stub region up to e_lfanew
    while data.len() < e_lfanew as usize {
        data.push(0);
    }
    let n_sections = 2u16;
    let opt = optional_header_pe32plus(
        subsystem_types::IMAGE_SUBSYSTEM_WINDOWS_GUI,
        0x4160, // ASLR | DEP | NX | CFG
        &[],
    );
    let coff = coff_header(
        pe_machine::IMAGE_FILE_MACHINE_AMD64,
        n_sections,
        opt.len() as u16,
        0x0022, // Executable | Large address aware
    );
    data.extend_from_slice(b"PE\0\0");
    data.extend_from_slice(&coff);
    data.extend_from_slice(&opt);
    // section table
    data.extend_from_slice(&section_header(".text", 0x1000, 0x1000, 0x1000, 0x400));
    data.extend_from_slice(&section_header(".data", 0x1000, 0x2000, 0x1000, 0x1400));
    // pad raw data
    while data.len() < 0x2400 {
        data.push(0);
    }

    let reader = TestReader::new(data);
    let md = parse_pe_metadata(&reader).expect("pe parse");
    assert_eq!(md.get_string("PE:MachineType"), Some("x64 (AMD64)"));
    assert_eq!(md.get_integer("PE:NumberOfSections"), Some(2));
    assert_eq!(md.get_string("PE:ImageFormat"), Some("PE32+"));
    assert_eq!(md.get_string("PE:Subsystem"), Some("Windows GUI"));
    assert_eq!(md.get_integer("PE:ASLR"), Some(1));
    assert_eq!(md.get_integer("PE:DEP"), Some(1));
    assert_eq!(md.get_integer("PE:ControlFlowGuard"), Some(1));
    assert!(md.contains_key("PE:CompileTime"));
    assert!(md.contains_key("PE:DllCharacteristicsDecoded"));
    assert!(md.contains_key("PE:ImageFileCharacteristics"));
}

#[test]
fn test_pe_with_data_directories_and_debug() {
    // PE with debug directory (idx 6) located inside .text, pointing to an
    // RSDS CodeView record. Also an export directory (idx 0).
    let e_lfanew = 0x80u32;
    let mut data = dos_header(e_lfanew);
    while data.len() < e_lfanew as usize {
        data.push(0);
    }

    // Section .text mapped: virtual_address 0x1000, raw at file offset 0x400.
    let text_vaddr = 0x1000u32;
    let text_raw = 0x400u32;
    // Debug directory inside .text at RVA 0x1000 (start of .text).
    let debug_rva = text_vaddr;
    let debug_size = 28u32;
    // Export directory RVA 0x1100, size > 0 (parse_exports will likely fail
    // gracefully on synthetic data, exercising the error path).
    let export_rva = 0x1100u32;
    let export_size = 40u32;

    let opt = optional_header_pe32plus(
        subsystem_types::IMAGE_SUBSYSTEM_WINDOWS_CUI,
        0x0100, // NX only
        &[(0, export_rva, export_size), (6, debug_rva, debug_size)],
    );
    let coff = coff_header(
        pe_machine::IMAGE_FILE_MACHINE_I386,
        1,
        opt.len() as u16,
        0x0102, // Executable | 32-bit
    );
    data.extend_from_slice(b"PE\0\0");
    data.extend_from_slice(&coff);
    data.extend_from_slice(&opt);
    data.extend_from_slice(&section_header(
        ".text", 0x2000, text_vaddr, 0x2000, text_raw,
    ));
    // Pad up to text raw offset.
    while data.len() < text_raw as usize {
        data.push(0);
    }
    // At text_raw (RVA 0x1000): the debug directory entry (28 bytes).
    // debug_type = CODEVIEW (2), pointer_to_raw_data -> file offset of CV record.
    let cv_file_off = text_raw + 0x100; // somewhere later in .text raw data
    let mut debug_entry = Vec::new();
    debug_entry.extend_from_slice(&0u32.to_le_bytes()); // characteristics
    debug_entry.extend_from_slice(&0x5F00_0000u32.to_le_bytes()); // time_date_stamp
    debug_entry.extend_from_slice(&0u16.to_le_bytes()); // major
    debug_entry.extend_from_slice(&0u16.to_le_bytes()); // minor
    debug_entry.extend_from_slice(&2u32.to_le_bytes()); // debug_type CODEVIEW
    // size_of_data: full RSDS record = 4(sig)+16(guid)+4(age)+8("app.pdb\0") = 32
    debug_entry.extend_from_slice(&32u32.to_le_bytes()); // size_of_data
    debug_entry.extend_from_slice(&0x1200u32.to_le_bytes()); // address_of_raw_data (rva)
    debug_entry.extend_from_slice(&cv_file_off.to_le_bytes()); // pointer_to_raw_data
    data.extend_from_slice(&debug_entry);
    // Pad up to cv_file_off.
    while data.len() < cv_file_off as usize {
        data.push(0);
    }
    // RSDS CodeView record: "RSDS" + 16-byte GUID + age(4) + pdb name + null.
    let mut rsds = Vec::new();
    rsds.extend_from_slice(b"RSDS");
    rsds.extend_from_slice(&[0xAB; 16]); // guid
    rsds.extend_from_slice(&7u32.to_le_bytes()); // age
    rsds.extend_from_slice(b"app.pdb\0");
    data.extend_from_slice(&rsds);
    // Tail padding
    while data.len() < 0x2400 {
        data.push(0);
    }

    let reader = TestReader::new(data);
    let md = parse_pe_metadata(&reader).expect("pe with debug");
    assert_eq!(md.get_string("PE:ImageFormat"), Some("PE32+"));
    assert_eq!(md.get_string("PE:Subsystem"), Some("Windows Console"));
    // PDB info may be extracted from the CodeView record.
    if let Some(pdb) = md.get_string("PE:PDBFileName") {
        assert_eq!(pdb, "app.pdb");
    }
    assert_eq!(md.get_integer("PE:DEP"), Some(1));
}

#[test]
fn test_pe_pe32_with_rich_header() {
    // PE32 (magic 0x010B) with e_lfanew > 0x80 to trigger Rich Header parsing.
    let e_lfanew = 0x100u32;
    let mut data = dos_header(e_lfanew);
    // Fill DOS stub region with a plausible Rich Header region (it likely
    // won't parse to a valid Rich Header, exercising the "no rich header" path).
    while data.len() < e_lfanew as usize {
        data.push(0x44);
    }
    // Build a PE32 optional header (magic 0x010B).
    let mut opt = Vec::new();
    opt.extend_from_slice(&0x010Bu16.to_le_bytes()); // PE32 magic
    opt.push(14);
    opt.push(0);
    opt.extend_from_slice(&0x1000u32.to_le_bytes());
    opt.extend_from_slice(&0x2000u32.to_le_bytes());
    opt.extend_from_slice(&0u32.to_le_bytes());
    opt.extend_from_slice(&0x1000u32.to_le_bytes());
    opt.extend_from_slice(&0x1000u32.to_le_bytes());
    // PE32 NT fields: base_of_data (u32) then image_base (u32)
    opt.extend_from_slice(&0x2000u32.to_le_bytes()); // base_of_data
    opt.extend_from_slice(&0x00400000u32.to_le_bytes()); // image_base
    opt.extend_from_slice(&0x1000u32.to_le_bytes());
    opt.extend_from_slice(&0x200u32.to_le_bytes());
    opt.extend_from_slice(&6u16.to_le_bytes());
    opt.extend_from_slice(&1u16.to_le_bytes());
    opt.extend_from_slice(&0u16.to_le_bytes());
    opt.extend_from_slice(&0u16.to_le_bytes());
    opt.extend_from_slice(&6u16.to_le_bytes());
    opt.extend_from_slice(&1u16.to_le_bytes());
    opt.extend_from_slice(&0u32.to_le_bytes());
    opt.extend_from_slice(&0x10000u32.to_le_bytes());
    opt.extend_from_slice(&0x400u32.to_le_bytes());
    opt.extend_from_slice(&0u32.to_le_bytes()); // checksum = 0 (skip branch)
    opt.extend_from_slice(&subsystem_types::IMAGE_SUBSYSTEM_NATIVE.to_le_bytes());
    opt.extend_from_slice(&0u16.to_le_bytes()); // dll characteristics = 0
    opt.extend_from_slice(&0x100000u32.to_le_bytes());
    opt.extend_from_slice(&0x1000u32.to_le_bytes());
    opt.extend_from_slice(&0x100000u32.to_le_bytes());
    opt.extend_from_slice(&0x1000u32.to_le_bytes());
    opt.extend_from_slice(&0u32.to_le_bytes());
    opt.extend_from_slice(&16u32.to_le_bytes());
    for _ in 0..16 {
        opt.extend_from_slice(&0u32.to_le_bytes());
        opt.extend_from_slice(&0u32.to_le_bytes());
    }

    let coff = coff_header(
        pe_machine::IMAGE_FILE_MACHINE_ARM64,
        1,
        opt.len() as u16,
        0x2002, // Executable | DLL
    );
    data.extend_from_slice(b"PE\0\0");
    data.extend_from_slice(&coff);
    data.extend_from_slice(&opt);
    data.extend_from_slice(&section_header(".text", 0x1000, 0x1000, 0x1000, 0x600));
    while data.len() < 0x1600 {
        data.push(0);
    }

    let reader = TestReader::new(data);
    let md = parse_pe_metadata(&reader).expect("pe32 parse");
    assert_eq!(md.get_string("PE:ImageFormat"), Some("PE32"));
    assert_eq!(md.get_string("PE:MachineType"), Some("ARM64"));
    assert_eq!(md.get_string("PE:Subsystem"), Some("Native (Driver)"));
    assert_eq!(md.get_string("PE:FileType"), Some("DLL"));
    assert_eq!(md.get_string("PE:ImageBaseHex"), Some("0x400000"));
}

#[test]
fn test_pe_no_optional_header() {
    // COFF object file: size_of_optional_header == 0, so optional header is skipped.
    let e_lfanew = 0x80u32;
    let mut data = dos_header(e_lfanew);
    while data.len() < e_lfanew as usize {
        data.push(0);
    }
    let coff = coff_header(
        pe_machine::IMAGE_FILE_MACHINE_I386,
        1,
        0,      // no optional header
        0x0000, // no flags -> file type Object
    );
    data.extend_from_slice(b"PE\0\0");
    data.extend_from_slice(&coff);
    data.extend_from_slice(&section_header(".text", 0x100, 0x0, 0x100, 0x200));
    while data.len() < 0x400 {
        data.push(0);
    }
    let reader = TestReader::new(data);
    let md = parse_pe_metadata(&reader).expect("coff obj");
    assert_eq!(md.get_string("PE:FileType"), Some("Object"));
    assert!(!md.contains_key("PE:ImageFormat"));
}

#[test]
fn test_pe_invalid_dos_signature() {
    // Valid length but wrong magic -> error.
    let mut data = vec![0u8; 64];
    data[0] = b'X';
    data[1] = b'Y';
    let reader = TestReader::new(data);
    assert!(parse_pe_metadata(&reader).is_err());
}

#[test]
fn test_pe_low_level_parsers_direct() {
    // DOS header parser
    let dh = dos_header(0xF0);
    let (_, parsed) = parse_dos_header(&dh).expect("dos");
    assert_eq!(parsed.e_magic, 0x5A4D);
    assert_eq!(parsed.e_lfanew, 0xF0);

    // COFF parser
    let mut coff = Vec::new();
    coff.extend_from_slice(b"PE\0\0");
    coff.extend_from_slice(&coff_header(
        pe_machine::IMAGE_FILE_MACHINE_AMD64,
        3,
        0xE0,
        0x0102,
    ));
    let (_, ch) = parse_coff_header(&coff).expect("coff");
    assert_eq!(ch.number_of_sections, 3);
    assert_eq!(ch.machine, pe_machine::IMAGE_FILE_MACHINE_AMD64);
    // bad signature
    assert!(parse_coff_header(b"XX\0\0aaaaaaaaaaaaaaaaaaaa").is_err());

    // Optional header standard + NT (PE32)
    let opt = {
        // standard fields for PE32
        let mut d = Vec::new();
        d.extend_from_slice(&0x010Bu16.to_le_bytes());
        d.push(14);
        d.push(0);
        d.extend_from_slice(&0x1000u32.to_le_bytes());
        d.extend_from_slice(&0x2000u32.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&0x1000u32.to_le_bytes());
        d.extend_from_slice(&0x1000u32.to_le_bytes());
        d
    };
    let (_, std_hdr) = parse_optional_header_standard(&opt).expect("std");
    assert_eq!(std_hdr.magic, 0x010B);

    // NT fields PE32: base_of_data + image_base(u32) + ...
    let mut nt = Vec::new();
    nt.extend_from_slice(&0x1000u32.to_le_bytes()); // base_of_data
    nt.extend_from_slice(&0x00400000u32.to_le_bytes()); // image_base
    nt.extend_from_slice(&0x1000u32.to_le_bytes());
    nt.extend_from_slice(&0x200u32.to_le_bytes());
    for _ in 0..6 {
        nt.extend_from_slice(&0u16.to_le_bytes());
    }
    nt.extend_from_slice(&0u32.to_le_bytes()); // win32 version
    nt.extend_from_slice(&0x10000u32.to_le_bytes());
    nt.extend_from_slice(&0x400u32.to_le_bytes());
    nt.extend_from_slice(&0u32.to_le_bytes()); // checksum
    nt.extend_from_slice(&3u16.to_le_bytes()); // subsystem
    nt.extend_from_slice(&0u16.to_le_bytes());
    for _ in 0..4 {
        nt.extend_from_slice(&0x1000u32.to_le_bytes());
    }
    nt.extend_from_slice(&0u32.to_le_bytes()); // loader flags
    nt.extend_from_slice(&2u32.to_le_bytes()); // number of rva and sizes
    nt.extend_from_slice(&0x10u32.to_le_bytes()); // dir 0 rva
    nt.extend_from_slice(&0x20u32.to_le_bytes()); // dir 0 size
    nt.extend_from_slice(&0x30u32.to_le_bytes()); // dir 1 rva
    nt.extend_from_slice(&0x40u32.to_le_bytes()); // dir 1 size
    let (_, nt_hdr) = parse_optional_header_nt(&nt, false).expect("nt");
    assert_eq!(nt_hdr.image_base, 0x00400000);
    assert_eq!(nt_hdr.data_directories.len(), 2);

    // Section parsers
    let sec = section_header(".rsrc", 0x100, 0x3000, 0x100, 0x800);
    let (_, sh) = parse_section_header(&sec).expect("section");
    assert_eq!(sh.name_str(), ".rsrc");
    let mut table = Vec::new();
    table.extend_from_slice(&section_header(".text", 1, 2, 3, 4));
    table.extend_from_slice(&section_header(".data", 5, 6, 7, 8));
    let (_, secs) = parse_section_table(&table, 2).expect("section table");
    assert_eq!(secs.len(), 2);
    assert_eq!(secs[1].name_str(), ".data");
}

#[test]
fn test_pe_signature_parser_direct() {
    // parse_win_certificate
    let mut cert = Vec::new();
    cert.extend_from_slice(&16u32.to_le_bytes());
    cert.extend_from_slice(&cert_revision::WIN_CERT_REVISION_2_0.to_le_bytes());
    cert.extend_from_slice(&cert_type::WIN_CERT_TYPE_PKCS_SIGNED_DATA.to_le_bytes());
    cert.extend_from_slice(&[0x30, 0x06, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
    let (_, wc) = parse_win_certificate(&cert).expect("win cert");
    assert_eq!(wc.dw_length, 16);
    assert_eq!(wc.certificate_data.len(), 8);

    // parse_signature_info with junk -> Some with signature_valid false branch.
    let junk = vec![0x00u8; 16];
    let info = parse_signature_info(&junk).expect("sig info junk");
    assert!(info.signature_present);
    assert!(!info.signature_valid);

    // parse_signature_info with a small valid-ish ASN.1 SEQUENCE wrapper.
    // Outer SEQUENCE containing a context [0] with an inner SEQUENCE (cert).
    let mut der = Vec::new();
    // inner certificate: SEQUENCE { SEQUENCE(TBS) { ... } }
    // We just need parse_asn1_sequence to succeed for the outer wrapper.
    der.push(0x30); // SEQUENCE
    der.push(0x06); // length 6
    der.extend_from_slice(&[0x02, 0x01, 0x01, 0x02, 0x01, 0x02]); // two INTEGERs
    let info2 = parse_signature_info(&der).expect("sig info der");
    assert!(info2.signature_present);
}

#[test]
fn test_pe_structures_helpers() {
    // VsFixedFileInfo helpers
    let info = VsFixedFileInfo {
        signature: 0xFEEF04BD,
        struct_version: 0x00010000,
        file_version_ms: 0x0001_0002,
        file_version_ls: 0x0003_0004,
        product_version_ms: 0x0005_0006,
        product_version_ls: 0x0007_0008,
        file_flags_mask: 0x3F,
        file_flags: 0x01 | 0x20, // Debug + Special build
        file_os: 0x00040004,
        file_type: 0x1,
        file_subtype: 0,
        file_date_ms: 0,
        file_date_ls: 0,
    };
    assert_eq!(info.file_version(), "1.2.3.4");
    assert_eq!(info.product_version(), "5.6.7.8");
    let flags = info.file_flags_string();
    assert!(flags.contains(&"Debug"));
    assert!(flags.contains(&"Special build"));
    assert_eq!(info.file_os_string(), "Windows NT 32-bit");
    assert_eq!(info.file_type_string(), "Application");

    // SectionHeader name_str trims nulls.
    let sh = PeSectionHeader {
        name: *b".idata\0\0",
        virtual_size: 0,
        virtual_address: 0,
        size_of_raw_data: 0,
        pointer_to_raw_data: 0,
        pointer_to_relocations: 0,
        pointer_to_line_numbers: 0,
        number_of_relocations: 0,
        number_of_line_numbers: 0,
        characteristics: 0,
    };
    assert_eq!(sh.name_str(), ".idata");
}

#[test]
fn test_pe_read_metadata_production_path() {
    // read_metadata with a valid PE so detection->dispatch path runs.
    let e_lfanew = 0x80u32;
    let mut data = dos_header(e_lfanew);
    while data.len() < e_lfanew as usize {
        data.push(0);
    }
    let opt = optional_header_pe32plus(
        subsystem_types::IMAGE_SUBSYSTEM_WINDOWS_GUI,
        0x0140, // Dynamic base + NX
        &[],
    );
    let coff = coff_header(
        pe_machine::IMAGE_FILE_MACHINE_AMD64,
        1,
        opt.len() as u16,
        0x0022,
    );
    data.extend_from_slice(b"PE\0\0");
    data.extend_from_slice(&coff);
    data.extend_from_slice(&opt);
    data.extend_from_slice(&section_header(".text", 0x1000, 0x1000, 0x1000, 0x400));
    while data.len() < 0x1400 {
        data.push(0);
    }
    let mut tmp = NamedTempFile::with_suffix(".exe").unwrap();
    tmp.write_all(&data).unwrap();
    tmp.flush().unwrap();
    let md = read_metadata(tmp.path()).expect("read_metadata pe");
    assert_eq!(md.get_string("PE:MachineType"), Some("x64 (AMD64)"));
}

// =============================================================================
// ELF fixture builders
// =============================================================================

/// Build an ELF identification array (16 bytes).
fn elf_ident(class: u8, data_enc: u8, osabi: u8) -> [u8; 16] {
    let mut id = [0u8; 16];
    id[0] = 0x7F;
    id[1] = b'E';
    id[2] = b'L';
    id[3] = b'F';
    id[4] = class;
    id[5] = data_enc;
    id[6] = 1; // version
    id[7] = osabi;
    id
}

/// Build a 64-byte ELF64 little-endian header.
#[allow(clippy::too_many_arguments)]
fn elf64_le_header(
    osabi: u8,
    e_type: u16,
    e_machine: u16,
    e_entry: u64,
    e_phoff: u64,
    e_phnum: u16,
    e_shoff: u64,
    e_shnum: u16,
    e_shstrndx: u16,
) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&elf_ident(
        elf_class::ELFCLASS64,
        elf_data::ELFDATA2LSB,
        osabi,
    ));
    d.extend_from_slice(&e_type.to_le_bytes());
    d.extend_from_slice(&e_machine.to_le_bytes());
    d.extend_from_slice(&1u32.to_le_bytes()); // e_version
    d.extend_from_slice(&e_entry.to_le_bytes());
    d.extend_from_slice(&e_phoff.to_le_bytes());
    d.extend_from_slice(&e_shoff.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes()); // e_flags
    d.extend_from_slice(&64u16.to_le_bytes()); // e_ehsize
    d.extend_from_slice(&56u16.to_le_bytes()); // e_phentsize
    d.extend_from_slice(&e_phnum.to_le_bytes());
    d.extend_from_slice(&64u16.to_le_bytes()); // e_shentsize
    d.extend_from_slice(&e_shnum.to_le_bytes());
    d.extend_from_slice(&e_shstrndx.to_le_bytes());
    d
}

/// Build an ELF64 LE program header (56 bytes).
fn elf64_phdr(p_type: u32, p_flags: u32, p_offset: u64, p_filesz: u64) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&p_type.to_le_bytes());
    d.extend_from_slice(&p_flags.to_le_bytes());
    d.extend_from_slice(&p_offset.to_le_bytes());
    d.extend_from_slice(&0x400000u64.to_le_bytes()); // p_vaddr
    d.extend_from_slice(&0x400000u64.to_le_bytes()); // p_paddr
    d.extend_from_slice(&p_filesz.to_le_bytes());
    d.extend_from_slice(&p_filesz.to_le_bytes()); // p_memsz
    d.extend_from_slice(&0x1000u64.to_le_bytes()); // p_align
    d
}

/// Build an ELF64 LE section header (64 bytes).
#[allow(clippy::too_many_arguments)]
fn elf64_shdr(
    sh_name: u32,
    sh_type: u32,
    sh_flags: u64,
    sh_addr: u64,
    sh_offset: u64,
    sh_size: u64,
    sh_link: u32,
    sh_entsize: u64,
) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&sh_name.to_le_bytes());
    d.extend_from_slice(&sh_type.to_le_bytes());
    d.extend_from_slice(&sh_flags.to_le_bytes());
    d.extend_from_slice(&sh_addr.to_le_bytes());
    d.extend_from_slice(&sh_offset.to_le_bytes());
    d.extend_from_slice(&sh_size.to_le_bytes());
    d.extend_from_slice(&sh_link.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes()); // sh_info
    d.extend_from_slice(&8u64.to_le_bytes()); // sh_addralign
    d.extend_from_slice(&sh_entsize.to_le_bytes());
    d
}

// =============================================================================
// ELF tests
// =============================================================================

#[test]
fn test_elf_minimal_header_only() {
    let data = elf64_le_header(
        0,
        elf_type::ET_EXEC,
        elf_machine::EM_X86_64,
        0x401000,
        0,
        0,
        0,
        0,
        0,
    );
    let reader = TestReader::new(data);
    let md = parse_elf_metadata(&reader).expect("elf header");
    assert_eq!(md.get_string("FileType"), Some("ELF"));
    assert_eq!(md.get_string("ELF:Class"), Some("64-bit"));
    assert_eq!(md.get_string("ELF:Endianness"), Some("Little-endian"));
    assert_eq!(md.get_string("ELF:ObjectType"), Some("Executable"));
    assert_eq!(md.get_string("ELF:Machine"), Some("AMD x86-64"));
    assert_eq!(md.get_string("ELF:OSABI"), Some("UNIX System V"));
    assert!(md.contains_key("ELF:EntryPoint"));
}

#[test]
fn test_elf_full_dynamic_executable() {
    // Build a full ELF64 LE file with program headers, sections, .dynamic,
    // .dynstr, .dynsym, .shstrtab, and a PT_NOTE / SHT_NOTE build-id note.
    //
    // We lay the file out region by region and record offsets.

    // ---- string tables ----
    // .shstrtab content
    let shstrtab: &[u8] =
        b"\0.text\0.dynstr\0.dynsym\0.dynamic\0.note.gnu.build-id\0.shstrtab\0.interp\0";
    // offsets within shstrtab:
    let off_text = 1u32;
    let off_dynstr = 7u32;
    let off_dynsym = 15u32;
    let off_dynamic = 23u32;
    let off_note = 32u32;
    let off_shstrtab = 53u32;
    let off_interp = 63u32;

    // .dynstr content: \0libc.so.6\0libname.so\0
    let dynstr: &[u8] = b"\0libc.so.6\0libname.so\0";
    let dynstr_needed_off = 1u64; // "libc.so.6"
    let dynstr_soname_off = 11u64; // "libname.so"

    // ---- We need known virtual addresses for .dynstr so DT_STRTAB matches ----
    // We'll assign sh_addr == sh_offset for the .dynstr section to satisfy
    // find_section_by_addr(strtab_addr).

    // ---- Build dynamic entries ----
    // (filled in once we know dynstr addr/size)
    let dynstr_size = dynstr.len() as u64;

    // ---- Build .dynsym (3 ELF64 symbols, 24 bytes each) ----
    // symbol 0: null
    // symbol 1: "main" (defined global func) -> export
    // symbol 2: "__stack_chk_fail" (undef global func) -> import + canary
    // .dynstr offsets for symbol names: reuse dynstr; but we need names there.
    // Extend dynstr-like names via the .dynsym's linked strtab == .dynstr.
    // For simplicity, point both names into dynstr where strings exist:
    //   st_name for "libc.so.6"=1, "libname.so"=11; not real func names, but
    //   resolve_symbol_names just reads strings. To get a real canary detection
    //   we craft a dedicated strtab for dynsym below.
    // To keep it correct, we use a separate .dynstr that also holds function
    // names. Redefine dynstr to include them.
    let dynstr2: Vec<u8> = {
        let mut v = Vec::new();
        v.push(0);
        v.extend_from_slice(b"libc.so.6\0"); // off 1
        v.extend_from_slice(b"main\0"); // off 11
        v.extend_from_slice(b"__stack_chk_fail\0"); // off 16
        v
    };
    let off_libc = 1u64;
    let off_main = 11u64;
    let off_chk = 16u64;
    let _ = (dynstr, dynstr_size, dynstr_needed_off, dynstr_soname_off);

    let make_sym = |st_name: u32, st_info: u8, st_shndx: u16, st_value: u64| {
        let mut s = Vec::new();
        s.extend_from_slice(&st_name.to_le_bytes());
        s.push(st_info);
        s.push(0); // st_other
        s.extend_from_slice(&st_shndx.to_le_bytes());
        s.extend_from_slice(&st_value.to_le_bytes());
        s.extend_from_slice(&0u64.to_le_bytes()); // st_size
        s
    };
    let st_info_global_func = (stb_binding::STB_GLOBAL << 4) | stt_type::STT_FUNC;
    let mut dynsym = Vec::new();
    dynsym.extend_from_slice(&make_sym(0, 0, 0, 0)); // null
    dynsym.extend_from_slice(&make_sym(off_main as u32, st_info_global_func, 1, 0x1000)); // main (export)
    dynsym.extend_from_slice(&make_sym(
        off_chk as u32,
        st_info_global_func,
        shn_index::SHN_UNDEF,
        0,
    )); // import + canary

    // ---- note (.note.gnu.build-id) ----
    // namesz=4 ("GNU\0"), descsz=4, type=3 (BUILD_ID); name+desc 4-byte aligned.
    let mut note = Vec::new();
    note.extend_from_slice(&4u32.to_le_bytes()); // namesz
    note.extend_from_slice(&4u32.to_le_bytes()); // descsz
    note.extend_from_slice(&3u32.to_le_bytes()); // type = NT_GNU_BUILD_ID
    note.extend_from_slice(b"GNU\0");
    note.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]); // build id

    // ---- interp segment payload ----
    let interp: &[u8] = b"/lib64/ld-linux-x86-64.so.2\0";

    // ---- layout ----
    // [ehdr 64][phdrs][sections data...][shdr table]
    let e_phnum = 3u16; // PT_PHDR, PT_INTERP, PT_NOTE (plus GNU_STACK and DYNAMIC below)
    let phnum = 5u16;
    let phoff = 64u64;
    let ph_size = 56u64 * phnum as u64;
    let _ = e_phnum;

    // Section data regions are placed after the program headers.
    let mut cursor = phoff + ph_size;
    // align cursor to 16
    cursor = (cursor + 15) & !15;

    let interp_off = cursor;
    cursor += interp.len() as u64;
    cursor = (cursor + 15) & !15;

    let note_off = cursor;
    cursor += note.len() as u64;
    cursor = (cursor + 15) & !15;

    let dynstr_off = cursor;
    cursor += dynstr2.len() as u64;
    cursor = (cursor + 15) & !15;

    let dynsym_off = cursor;
    cursor += dynsym.len() as u64;
    cursor = (cursor + 15) & !15;

    // dynamic entries (built now that we know dynstr addr/size)
    let dynstr_addr = dynstr_off; // sh_addr == sh_offset
    let mut dynamic = Vec::new();
    let dyn_entry = |tag: i64, val: u64, buf: &mut Vec<u8>| {
        buf.extend_from_slice(&(tag as u64).to_le_bytes());
        buf.extend_from_slice(&val.to_le_bytes());
    };
    dyn_entry(dt_tag::DT_NEEDED, off_libc, &mut dynamic);
    dyn_entry(dt_tag::DT_SONAME, off_main, &mut dynamic); // arbitrary string
    dyn_entry(dt_tag::DT_STRTAB, dynstr_addr, &mut dynamic);
    dyn_entry(dt_tag::DT_STRSZ, dynstr2.len() as u64, &mut dynamic);
    dyn_entry(dt_tag::DT_RUNPATH, off_libc, &mut dynamic);
    dyn_entry(dt_tag::DT_FLAGS_1, df1_flags::DF_1_PIE, &mut dynamic);
    dyn_entry(dt_tag::DT_BIND_NOW, 0, &mut dynamic);
    dyn_entry(dt_tag::DT_NULL, 0, &mut dynamic);

    let dynamic_off = cursor;
    cursor += dynamic.len() as u64;
    cursor = (cursor + 15) & !15;

    let shstrtab_off = cursor;
    cursor += shstrtab.len() as u64;
    cursor = (cursor + 15) & !15;

    // Section header table.
    let shoff = cursor;

    // Section indices:
    // 0: NULL
    // 1: .interp  (PROGBITS)
    // 2: .note.gnu.build-id (NOTE)
    // 3: .dynstr (STRTAB)
    // 4: .dynsym (DYNSYM, sh_link=3)
    // 5: .dynamic (DYNAMIC)
    // 6: .text (PROGBITS)
    // 7: .shstrtab (STRTAB)  <- e_shstrndx
    let e_shnum = 8u16;
    let e_shstrndx = 7u16;

    // ---- assemble file ----
    let mut data = elf64_le_header(
        3, // GNU/Linux
        elf_type::ET_DYN,
        elf_machine::EM_X86_64,
        0x1000, // entry (non-zero so PIE detection can fire)
        phoff,
        phnum,
        shoff,
        e_shnum,
        e_shstrndx,
    );
    // program headers
    let mut phdrs = Vec::new();
    phdrs.extend_from_slice(&elf64_phdr(
        pt_type::PT_PHDR,
        pf_flags::PF_R,
        phoff,
        ph_size,
    ));
    phdrs.extend_from_slice(&elf64_phdr(
        pt_type::PT_INTERP,
        pf_flags::PF_R,
        interp_off,
        interp.len() as u64,
    ));
    phdrs.extend_from_slice(&elf64_phdr(
        pt_type::PT_NOTE,
        pf_flags::PF_R,
        note_off,
        note.len() as u64,
    ));
    // GNU_STACK executable -> sets has_executable_stack
    phdrs.extend_from_slice(&elf64_phdr(
        pt_type::PT_GNU_STACK,
        pf_flags::PF_R | pf_flags::PF_W | pf_flags::PF_X,
        0,
        0,
    ));
    // PT_DYNAMIC segment -> sets ELF:HasDynamic (points at the .dynamic section data)
    phdrs.extend_from_slice(&elf64_phdr(
        pt_type::PT_DYNAMIC,
        pf_flags::PF_R | pf_flags::PF_W,
        dynamic_off,
        dynamic.len() as u64,
    ));
    assert_eq!(phdrs.len() as u64, ph_size);
    data.extend_from_slice(&phdrs);

    // pad to interp_off
    while (data.len() as u64) < interp_off {
        data.push(0);
    }
    data.extend_from_slice(interp);
    while (data.len() as u64) < note_off {
        data.push(0);
    }
    data.extend_from_slice(&note);
    while (data.len() as u64) < dynstr_off {
        data.push(0);
    }
    data.extend_from_slice(&dynstr2);
    while (data.len() as u64) < dynsym_off {
        data.push(0);
    }
    data.extend_from_slice(&dynsym);
    while (data.len() as u64) < dynamic_off {
        data.push(0);
    }
    data.extend_from_slice(&dynamic);
    while (data.len() as u64) < shstrtab_off {
        data.push(0);
    }
    data.extend_from_slice(shstrtab);
    while (data.len() as u64) < shoff {
        data.push(0);
    }

    // section header table
    let mut shdrs = Vec::new();
    // 0: NULL
    shdrs.extend_from_slice(&elf64_shdr(0, sh_type::SHT_NULL, 0, 0, 0, 0, 0, 0));
    // 1: .interp
    shdrs.extend_from_slice(&elf64_shdr(
        off_interp,
        sh_type::SHT_PROGBITS,
        sh_flags::SHF_ALLOC,
        interp_off,
        interp_off,
        interp.len() as u64,
        0,
        0,
    ));
    // 2: .note.gnu.build-id
    shdrs.extend_from_slice(&elf64_shdr(
        off_note,
        sh_type::SHT_NOTE,
        sh_flags::SHF_ALLOC,
        note_off,
        note_off,
        note.len() as u64,
        0,
        0,
    ));
    // 3: .dynstr
    shdrs.extend_from_slice(&elf64_shdr(
        off_dynstr,
        sh_type::SHT_STRTAB,
        sh_flags::SHF_ALLOC | sh_flags::SHF_STRINGS,
        dynstr_addr,
        dynstr_off,
        dynstr2.len() as u64,
        0,
        0,
    ));
    // 4: .dynsym (sh_link -> .dynstr index 3)
    shdrs.extend_from_slice(&elf64_shdr(
        off_dynsym,
        sh_type::SHT_DYNSYM,
        sh_flags::SHF_ALLOC,
        dynsym_off,
        dynsym_off,
        dynsym.len() as u64,
        3,
        24,
    ));
    // 5: .dynamic
    shdrs.extend_from_slice(&elf64_shdr(
        off_dynamic,
        sh_type::SHT_DYNAMIC,
        sh_flags::SHF_ALLOC | sh_flags::SHF_WRITE,
        dynamic_off,
        dynamic_off,
        dynamic.len() as u64,
        3,
        16,
    ));
    // 6: .text
    shdrs.extend_from_slice(&elf64_shdr(
        off_text,
        sh_type::SHT_PROGBITS,
        sh_flags::SHF_ALLOC | sh_flags::SHF_EXECINSTR,
        0x1000,
        interp_off, // arbitrary, within file
        0x200,
        0,
        0,
    ));
    // 7: .shstrtab
    shdrs.extend_from_slice(&elf64_shdr(
        off_shstrtab,
        sh_type::SHT_STRTAB,
        0,
        0,
        shstrtab_off,
        shstrtab.len() as u64,
        0,
        0,
    ));
    data.extend_from_slice(&shdrs);

    let reader = TestReader::new(data);
    let md = parse_elf_metadata(&reader).expect("full elf");

    assert_eq!(md.get_string("ELF:ObjectType"), Some("Shared Object"));
    assert_eq!(md.get_string("ELF:OSABI"), Some("GNU/Linux"));
    assert!(md.contains_key("ELF:SectionNames"));
    assert!(md.contains_key("ELF:SegmentTypes"));
    // PT_INTERP segment present -> HasInterpreter flag set (from the program header).
    assert_eq!(md.get_integer("ELF:HasInterpreter"), Some(1));
    // PT_DYNAMIC segment present -> HasDynamic flag set.
    assert_eq!(md.get_integer("ELF:HasDynamic"), Some(1));
    // Note: ELF:Interpreter (the resolved string) is intentionally not asserted
    // here. The extractor overwrites dynamic_info when the .dynamic section's
    // dynstr resolves, which clears the interpreter set from PT_INTERP.
    assert!(md.contains_key("ELF:BuildID"));
    assert_eq!(md.get_integer("ELF:ExecutableStack"), Some(1));
    assert_eq!(md.get_integer("ELF:NXEnabled"), Some(0));
    // canary should be detected via __stack_chk_fail dynsym import
    assert_eq!(md.get_integer("ELF:StackCanary"), Some(1));
    // needed library present (dynamic section + dynstr resolved)
    assert!(md.contains_key("ELF:SharedObjects"));
    // PIE flag is computed from DT_FLAGS_1; the security key is always present.
    assert!(md.contains_key("ELF:PIEEnabled"));
}

#[test]
fn test_elf32_big_endian() {
    // ELF32 big-endian header with a couple of program headers.
    let mut id = elf_ident(elf_class::ELFCLASS32, elf_data::ELFDATA2MSB, 0);
    id[7] = 9; // FreeBSD osabi
    let phoff = 52u32;
    let phnum = 2u16;
    let mut d = Vec::new();
    d.extend_from_slice(&id);
    d.extend_from_slice(&elf_type::ET_EXEC.to_be_bytes());
    d.extend_from_slice(&elf_machine::EM_PPC.to_be_bytes());
    d.extend_from_slice(&1u32.to_be_bytes()); // version
    d.extend_from_slice(&0x10000000u32.to_be_bytes()); // entry
    d.extend_from_slice(&phoff.to_be_bytes()); // phoff
    d.extend_from_slice(&0u32.to_be_bytes()); // shoff
    d.extend_from_slice(&0u32.to_be_bytes()); // flags
    d.extend_from_slice(&52u16.to_be_bytes()); // ehsize
    d.extend_from_slice(&32u16.to_be_bytes()); // phentsize
    d.extend_from_slice(&phnum.to_be_bytes()); // phnum
    d.extend_from_slice(&40u16.to_be_bytes()); // shentsize
    d.extend_from_slice(&0u16.to_be_bytes()); // shnum
    d.extend_from_slice(&0u16.to_be_bytes()); // shstrndx
    // two ELF32 BE program headers (32 bytes each)
    let mut phdr = |p_type: u32, p_flags: u32| {
        d.extend_from_slice(&p_type.to_be_bytes());
        d.extend_from_slice(&0u32.to_be_bytes()); // p_offset
        d.extend_from_slice(&0x10000000u32.to_be_bytes()); // p_vaddr
        d.extend_from_slice(&0x10000000u32.to_be_bytes()); // p_paddr
        d.extend_from_slice(&0u32.to_be_bytes()); // p_filesz
        d.extend_from_slice(&0u32.to_be_bytes()); // p_memsz
        d.extend_from_slice(&p_flags.to_be_bytes()); // p_flags (ELF32 order)
        d.extend_from_slice(&0x1000u32.to_be_bytes()); // p_align
    };
    phdr(pt_type::PT_LOAD, pf_flags::PF_R | pf_flags::PF_X);
    phdr(pt_type::PT_GNU_RELRO, pf_flags::PF_R);

    let reader = TestReader::new(d);
    let md = parse_elf_metadata(&reader).expect("elf32 be");
    assert_eq!(md.get_string("ELF:Class"), Some("32-bit"));
    assert_eq!(md.get_string("ELF:Endianness"), Some("Big-endian"));
    assert_eq!(md.get_string("ELF:Machine"), Some("PowerPC"));
    assert_eq!(md.get_string("ELF:OSABI"), Some("FreeBSD"));
    assert_eq!(md.get_integer("ELF:LoadableSegmentCount"), Some(1));
    assert_eq!(md.get_integer("ELF:RELROEnabled"), Some(1));
}

#[test]
fn test_elf_errors() {
    // verify_signature false for non-ELF
    let bad = TestReader::new(vec![0x4D, 0x5A, 0x00, 0x00]);
    assert!(!oxidex::parsers::elf::ELFParser::verify_signature(&bad).unwrap());
    assert!(parse_elf_metadata(&bad).is_err());
    // too small
    let tiny = TestReader::new(vec![0x7F, b'E']);
    assert!(!oxidex::parsers::elf::ELFParser::verify_signature(&tiny).unwrap());
    // valid magic but invalid class -> extract fails
    let mut data = elf64_le_header(
        0,
        elf_type::ET_EXEC,
        elf_machine::EM_X86_64,
        0,
        0,
        0,
        0,
        0,
        0,
    );
    data[4] = 0xFF; // bad class
    let reader = TestReader::new(data);
    assert!(parse_elf_metadata(&reader).is_err());
}

#[test]
fn test_elf_header_parser_direct() {
    // 64-bit LE
    let d64 = elf64_le_header(
        0,
        elf_type::ET_DYN,
        elf_machine::EM_AARCH64,
        0x1000,
        64,
        1,
        0,
        0,
        0,
    );
    let (_, h64) = parse_elf_header(&d64).expect("h64");
    assert!(h64.is_64bit);
    assert_eq!(h64.machine_str(), "ARM64");
    assert_eq!(h64.type_str(), "Shared Object");
    assert_eq!(h64.class_str(), "64-bit");
    assert_eq!(h64.endian_str(), "Little-endian");

    // invalid class / endian / truncated
    let mut bad_class = d64.clone();
    bad_class[4] = 0;
    assert!(parse_elf_header(&bad_class).is_err());
    let mut bad_endian = d64.clone();
    bad_endian[5] = 0;
    assert!(parse_elf_header(&bad_endian).is_err());
    assert!(parse_elf_header(&[0x7F, b'E', b'L', b'F', 2, 1]).is_err());

    // machine_str / osabi_str fallthroughs
    let dunk = elf64_le_header(99, 0xFE01, 0xFFFF, 0, 0, 0, 0, 0, 0);
    let (_, hunk) = parse_elf_header(&dunk).expect("hunk");
    assert_eq!(hunk.machine_str(), "Unknown");
    assert_eq!(hunk.osabi_str(), "Unknown");
    assert_eq!(hunk.type_str(), "OS-specific");
}

#[test]
fn test_elf_program_section_header_parsers_direct() {
    // program headers (64-bit LE)
    let mut ph_data = Vec::new();
    ph_data.extend_from_slice(&elf64_phdr(
        pt_type::PT_LOAD,
        pf_flags::PF_R | pf_flags::PF_X,
        0,
        0x1000,
    ));
    ph_data.extend_from_slice(&elf64_phdr(pt_type::PT_TLS, pf_flags::PF_R, 0x2000, 0x100));
    let (_, phs) = parse_program_headers(&ph_data, 2, true, true).expect("phdrs");
    assert_eq!(phs.len(), 2);
    assert_eq!(phs[0].type_str(), "LOAD");
    assert_eq!(phs[0].flags_str(), "R-X");
    assert!(phs[0].is_load());
    assert!(phs[0].is_executable());
    assert_eq!(phs[1].type_str(), "TLS");
    // truncated phdr -> error
    assert!(parse_program_headers(&[0u8; 10], 1, true, true).is_err());

    // section headers (64-bit LE) + name resolution
    let strtab = b"\0.text\0.bss\0";
    let mut sh_data = Vec::new();
    sh_data.extend_from_slice(&elf64_shdr(
        1,
        sh_type::SHT_PROGBITS,
        sh_flags::SHF_ALLOC | sh_flags::SHF_EXECINSTR,
        0x1000,
        0x1000,
        0x500,
        0,
        0,
    ));
    sh_data.extend_from_slice(&elf64_shdr(
        7,
        sh_type::SHT_NOBITS,
        sh_flags::SHF_ALLOC | sh_flags::SHF_WRITE,
        0x2000,
        0x2000,
        0x100,
        0,
        0,
    ));
    let (_, mut shs) = parse_section_headers(&sh_data, 2, true, true).expect("shdrs");
    resolve_section_names(&mut shs, strtab);
    assert_eq!(shs[0].name_str(), ".text");
    assert_eq!(shs[1].name_str(), ".bss");
    assert_eq!(shs[0].type_str(), "PROGBITS");
    assert!(shs[0].flags_str().contains('X'));
    assert_eq!(shs[1].type_str(), "NOBITS");

    // get_string_from_strtab boundary
    assert_eq!(get_string_from_strtab(strtab, 1), Some(".text".to_string()));
    assert_eq!(get_string_from_strtab(strtab, 1000), None);

    // ELF32 LE section header path
    let mut sh32 = Vec::new();
    sh32.extend_from_slice(&5u32.to_le_bytes()); // sh_name
    sh32.extend_from_slice(&sh_type::SHT_SYMTAB.to_le_bytes());
    sh32.extend_from_slice(&0u32.to_le_bytes()); // flags
    sh32.extend_from_slice(&0u32.to_le_bytes()); // addr
    sh32.extend_from_slice(&0x2000u32.to_le_bytes()); // offset
    sh32.extend_from_slice(&0x80u32.to_le_bytes()); // size
    sh32.extend_from_slice(&0u32.to_le_bytes()); // link
    sh32.extend_from_slice(&0u32.to_le_bytes()); // info
    sh32.extend_from_slice(&1u32.to_le_bytes()); // addralign
    sh32.extend_from_slice(&16u32.to_le_bytes()); // entsize
    let (_, shs32) = parse_section_headers(&sh32, 1, false, true).expect("shdr32");
    assert_eq!(shs32[0].type_str(), "SYMTAB");
}

#[test]
fn test_elf_dynamic_parser_direct() {
    // ELF64 LE dynamic entries
    let mut data = Vec::new();
    let push = |tag: i64, val: u64, b: &mut Vec<u8>| {
        b.extend_from_slice(&(tag as u64).to_le_bytes());
        b.extend_from_slice(&val.to_le_bytes());
    };
    push(dt_tag::DT_NEEDED, 1, &mut data);
    push(dt_tag::DT_NEEDED, 11, &mut data);
    push(dt_tag::DT_SONAME, 21, &mut data);
    push(dt_tag::DT_RPATH, 32, &mut data);
    push(dt_tag::DT_STRTAB, 0x1000, &mut data);
    push(dt_tag::DT_STRSZ, 100, &mut data);
    push(dt_tag::DT_SYMTAB, 0x2000, &mut data);
    push(dt_tag::DT_SYMENT, 24, &mut data);
    push(dt_tag::DT_TEXTREL, 0, &mut data);
    push(dt_tag::DT_FLAGS_1, df1_flags::DF_1_PIE, &mut data);
    push(dt_tag::DT_NULL, 0, &mut data);

    let entries = parse_dynamic_entries(&data, true, true);
    assert!(entries.iter().any(|e| e.d_tag == dt_tag::DT_NULL));
    let dynstr = b"\0libc.so.6\0libm.so.6\0libtest.so\0/lib64:/usr/lib\0";
    let info = extract_dynamic_info(&entries, dynstr);
    assert_eq!(info.needed.len(), 2);
    assert_eq!(info.soname.as_deref(), Some("libtest.so"));
    assert_eq!(info.rpath.len(), 2);
    assert!(info.has_textrel);
    assert!(info.is_pie());
    let (addr, size) = find_dynstr_info(&entries).unwrap();
    assert_eq!(addr, 0x1000);
    assert_eq!(size, 100);
    let (saddr, sent) = find_dynsym_info(&entries).unwrap();
    assert_eq!(saddr, 0x2000);
    assert_eq!(sent, 24);

    // ELF32 BE entries (different parser path)
    let mut data32 = Vec::new();
    data32.extend_from_slice(&(dt_tag::DT_SONAME as u32).to_be_bytes());
    data32.extend_from_slice(&5u32.to_be_bytes());
    data32.extend_from_slice(&(dt_tag::DT_NULL as u32).to_be_bytes());
    data32.extend_from_slice(&0u32.to_be_bytes());
    let entries32 = parse_dynamic_entries(&data32, false, false);
    assert_eq!(entries32.len(), 2);

    // tag_str for DynamicEntry
    let de = DynamicEntry {
        d_tag: dt_tag::DT_RUNPATH,
        d_val: 0,
    };
    assert_eq!(de.tag_str(), "RUNPATH");
}

#[test]
fn test_elf_symbol_parser_direct() {
    let make_st_info = |b: u8, t: u8| (b << 4) | (t & 0xF);
    // ELF64 LE symbols
    let mut data = Vec::new();
    let sym = |name: u32, info: u8, shndx: u16, val: u64, buf: &mut Vec<u8>| {
        buf.extend_from_slice(&name.to_le_bytes());
        buf.push(info);
        buf.push(0);
        buf.extend_from_slice(&shndx.to_le_bytes());
        buf.extend_from_slice(&val.to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes());
    };
    sym(0, 0, 0, 0, &mut data); // null
    sym(
        1,
        make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_FUNC),
        1,
        0x1000,
        &mut data,
    ); // export
    sym(
        6,
        make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_FUNC),
        shn_index::SHN_UNDEF,
        0,
        &mut data,
    ); // import
    let mut symbols = parse_symbol_table(&data, true, true);
    assert_eq!(symbols.len(), 3);
    let strtab = b"\0main\0printf\0";
    resolve_symbol_names(&mut symbols, strtab);
    assert_eq!(symbols[1].name_str(), "main");
    let info = extract_symbol_info(&symbols, 50, 50);
    assert_eq!(info.exported_functions, vec!["main".to_string()]);
    assert_eq!(info.imported_functions, vec!["printf".to_string()]);

    // ELF32 LE symbol path
    let mut data32 = Vec::new();
    data32.extend_from_slice(&1u32.to_le_bytes()); // name
    data32.extend_from_slice(&0x2000u32.to_le_bytes()); // value
    data32.extend_from_slice(&8u32.to_le_bytes()); // size
    data32.push(make_st_info(stb_binding::STB_WEAK, stt_type::STT_OBJECT));
    data32.push(0);
    data32.extend_from_slice(&2u16.to_le_bytes());
    let syms32 = parse_symbol_table(&data32, false, true);
    assert_eq!(syms32.len(), 1);
    assert_eq!(syms32[0].binding(), stb_binding::STB_WEAK);

    // security features
    let canary_sym = Symbol {
        st_name: 0,
        name: Some("__stack_chk_fail".to_string()),
        st_info: make_st_info(stb_binding::STB_GLOBAL, stt_type::STT_FUNC),
        st_other: 0,
        st_shndx: shn_index::SHN_UNDEF,
        st_value: 0,
        st_size: 0,
    };
    let (has_canary, has_fortify) = detect_security_features(std::slice::from_ref(&canary_sym));
    assert!(has_canary);
    assert!(!has_fortify);
    assert_eq!(canary_sym.binding_str(), "GLOBAL");
    assert_eq!(canary_sym.type_str(), "FUNC");
    assert!(!canary_sym.is_defined());
}

#[test]
fn test_elf_note_parser_direct() {
    // build a build-id note and an ABI tag note (LE)
    let mk_note = |name: &str, ntype: u32, desc: &[u8]| {
        let mut d = Vec::new();
        let name_with_null = format!("{}\0", name);
        let namesz = name_with_null.len() as u32;
        let descsz = desc.len() as u32;
        d.extend_from_slice(&namesz.to_le_bytes());
        d.extend_from_slice(&descsz.to_le_bytes());
        d.extend_from_slice(&ntype.to_le_bytes());
        d.extend_from_slice(name_with_null.as_bytes());
        while d.len() % 4 != 0 {
            d.push(0);
        }
        d.extend_from_slice(desc);
        while d.len() % 4 != 0 {
            d.push(0);
        }
        d
    };
    let mut data = Vec::new();
    data.extend_from_slice(&mk_note("GNU", 3, &[0xDE, 0xAD, 0xBE, 0xEF]));
    let abi_desc = [0u8, 0, 0, 0, 5, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0];
    data.extend_from_slice(&mk_note("GNU", 1, &abi_desc));
    data.extend_from_slice(&mk_note("GNU", 4, b"gold 1.16\0"));

    let notes = parse_notes(&data, true);
    assert!(notes.len() >= 3);
    let build_id = extract_build_id(&notes);
    assert_eq!(build_id.as_deref(), Some("deadbeef"));
    let abi = extract_gnu_abi_tag(&notes, true).expect("abi");
    assert_eq!(abi.os_name(), "Linux");
    assert_eq!(abi.version_string(), "5.4.0");
    let gold = extract_gold_version(&notes);
    assert_eq!(gold.as_deref(), Some("gold 1.16"));

    // NoteEntry helpers
    let ne = NoteEntry {
        name: "GNU".to_string(),
        note_type: 3,
        desc: vec![0x01, 0x02],
    };
    assert_eq!(ne.gnu_type_str(), "Build ID");
    assert_eq!(ne.build_id_hex(), Some("0102".to_string()));
}

#[test]
fn test_elf_read_metadata_production_path() {
    let data = elf64_le_header(
        3,
        elf_type::ET_EXEC,
        elf_machine::EM_RISCV,
        0x10000,
        0,
        0,
        0,
        0,
        0,
    );
    let mut tmp = NamedTempFile::with_suffix("").unwrap();
    tmp.write_all(&data).unwrap();
    tmp.flush().unwrap();
    let md = read_metadata(tmp.path()).expect("read_metadata elf");
    assert_eq!(md.get_string("FileType"), Some("ELF"));
    assert_eq!(md.get_string("ELF:Machine"), Some("RISC-V"));
}

#[test]
fn test_elf_section_header_helper_struct() {
    // ElfSectionHeader flags_str / type_str fallthroughs and USER/PROC ranges.
    let s = ElfSectionHeader {
        sh_name: 0,
        name: Some(".custom".to_string()),
        sh_type: 0x80000001, // USER range
        sh_flags: sh_flags::SHF_WRITE | sh_flags::SHF_TLS | sh_flags::SHF_MERGE,
        sh_addr: 0,
        sh_offset: 0,
        sh_size: 0,
        sh_link: 0,
        sh_info: 0,
        sh_addralign: 0,
        sh_entsize: 0,
    };
    assert_eq!(s.type_str(), "USER");
    let f = s.flags_str();
    assert!(f.contains('W'));
    assert!(f.contains('T'));
    assert!(f.contains('M'));
    assert_eq!(s.name_str(), ".custom");

    let empty = ElfSectionHeader {
        sh_name: 9,
        name: None,
        sh_type: 0x70000000, // PROC range
        sh_flags: 0,
        sh_addr: 0,
        sh_offset: 0,
        sh_size: 0,
        sh_link: 0,
        sh_info: 0,
        sh_addralign: 0,
        sh_entsize: 0,
    };
    assert_eq!(empty.type_str(), "PROC");
    assert_eq!(empty.flags_str(), "---");
    assert_eq!(empty.name_str(), "<9>");
}

#[test]
fn test_core_decode_flags_helper() {
    // core::decode_flags is used by PE extractor; exercise it directly too.
    const FLAGS: &[(u32, &str)] = &[(0x1, "A"), (0x2, "B"), (0x4, "C")];
    let names = core_decode_flags(0x5, FLAGS);
    assert!(names.contains(&"A"));
    assert!(names.contains(&"C"));
    assert!(!names.contains(&"B"));
}
