//! ELF file structure definitions
//!
//! This module defines Rust structures matching ELF file headers as per the
//! System V Application Binary Interface (ABI) specification and Linux extensions.
//!
//! Both ELF32 and ELF64 structures are defined, with the appropriate variant
//! selected based on e_ident[EI_CLASS].

#![allow(dead_code)]

// =============================================================================
// ELF Identification (e_ident) Constants
// =============================================================================

/// ELF magic bytes (indices 0-3)
pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

/// Index constants for e_ident array
pub mod ei_index {
    /// Magic number byte 0 (0x7F)
    pub const EI_MAG0: usize = 0;
    /// Magic number byte 1 ('E')
    pub const EI_MAG1: usize = 1;
    /// Magic number byte 2 ('L')
    pub const EI_MAG2: usize = 2;
    /// Magic number byte 3 ('F')
    pub const EI_MAG3: usize = 3;
    /// File class (32-bit or 64-bit)
    pub const EI_CLASS: usize = 4;
    /// Data encoding (endianness)
    pub const EI_DATA: usize = 5;
    /// ELF version
    pub const EI_VERSION: usize = 6;
    /// OS/ABI identification
    pub const EI_OSABI: usize = 7;
    /// ABI version
    pub const EI_ABIVERSION: usize = 8;
    /// Start of padding bytes
    pub const EI_PAD: usize = 9;
    /// Size of e_ident array
    pub const EI_NIDENT: usize = 16;
}

/// ELF class values (e_ident[EI_CLASS])
pub mod elf_class {
    /// Invalid class
    pub const ELFCLASSNONE: u8 = 0;
    /// 32-bit objects
    pub const ELFCLASS32: u8 = 1;
    /// 64-bit objects
    pub const ELFCLASS64: u8 = 2;
}

/// Data encoding values (e_ident[EI_DATA])
pub mod elf_data {
    /// Invalid encoding
    pub const ELFDATANONE: u8 = 0;
    /// Little-endian (2's complement, LSB first)
    pub const ELFDATA2LSB: u8 = 1;
    /// Big-endian (2's complement, MSB first)
    pub const ELFDATA2MSB: u8 = 2;
}

/// OS/ABI values (e_ident[EI_OSABI])
pub mod elf_osabi {
    /// UNIX System V ABI
    pub const ELFOSABI_NONE: u8 = 0;
    /// Alias for ELFOSABI_NONE
    pub const ELFOSABI_SYSV: u8 = 0;
    /// HP-UX
    pub const ELFOSABI_HPUX: u8 = 1;
    /// NetBSD
    pub const ELFOSABI_NETBSD: u8 = 2;
    /// GNU/Linux (historically called ELFOSABI_LINUX)
    pub const ELFOSABI_GNU: u8 = 3;
    /// Alias for ELFOSABI_GNU
    pub const ELFOSABI_LINUX: u8 = 3;
    /// Sun Solaris
    pub const ELFOSABI_SOLARIS: u8 = 6;
    /// IBM AIX
    pub const ELFOSABI_AIX: u8 = 7;
    /// SGI IRIX
    pub const ELFOSABI_IRIX: u8 = 8;
    /// FreeBSD
    pub const ELFOSABI_FREEBSD: u8 = 9;
    /// Compaq TRU64 UNIX
    pub const ELFOSABI_TRU64: u8 = 10;
    /// Novell Modesto
    pub const ELFOSABI_MODESTO: u8 = 11;
    /// OpenBSD
    pub const ELFOSABI_OPENBSD: u8 = 12;
    /// ARM EABI
    pub const ELFOSABI_ARM_AEABI: u8 = 64;
    /// ARM
    pub const ELFOSABI_ARM: u8 = 97;
    /// Standalone (embedded) application
    pub const ELFOSABI_STANDALONE: u8 = 255;
}

// =============================================================================
// ELF Header (e_type) Object File Types
// =============================================================================

/// Object file type values (e_type)
pub mod elf_type {
    /// No file type
    pub const ET_NONE: u16 = 0;
    /// Relocatable file
    pub const ET_REL: u16 = 1;
    /// Executable file
    pub const ET_EXEC: u16 = 2;
    /// Shared object file
    pub const ET_DYN: u16 = 3;
    /// Core file
    pub const ET_CORE: u16 = 4;
    /// OS-specific range start
    pub const ET_LOOS: u16 = 0xFE00;
    /// OS-specific range end
    pub const ET_HIOS: u16 = 0xFEFF;
    /// Processor-specific range start
    pub const ET_LOPROC: u16 = 0xFF00;
    /// Processor-specific range end
    pub const ET_HIPROC: u16 = 0xFFFF;
}

// =============================================================================
// Machine Types (e_machine)
// =============================================================================

/// Machine architecture values (e_machine)
pub mod machine_types {
    /// No machine
    pub const EM_NONE: u16 = 0;
    /// AT&T WE 32100
    pub const EM_M32: u16 = 1;
    /// SPARC
    pub const EM_SPARC: u16 = 2;
    /// Intel 80386
    pub const EM_386: u16 = 3;
    /// Motorola 68000
    pub const EM_68K: u16 = 4;
    /// Motorola 88000
    pub const EM_88K: u16 = 5;
    /// Intel MCU
    pub const EM_IAMCU: u16 = 6;
    /// Intel 80860
    pub const EM_860: u16 = 7;
    /// MIPS I Architecture
    pub const EM_MIPS: u16 = 8;
    /// IBM System/370
    pub const EM_S370: u16 = 9;
    /// MIPS RS3000 Little-endian
    pub const EM_MIPS_RS3_LE: u16 = 10;
    /// Hewlett-Packard PA-RISC
    pub const EM_PARISC: u16 = 15;
    /// Fujitsu VPP500
    pub const EM_VPP500: u16 = 17;
    /// Enhanced SPARC (SPARC32PLUS)
    pub const EM_SPARC32PLUS: u16 = 18;
    /// Intel 80960
    pub const EM_960: u16 = 19;
    /// PowerPC
    pub const EM_PPC: u16 = 20;
    /// 64-bit PowerPC
    pub const EM_PPC64: u16 = 21;
    /// IBM System/390
    pub const EM_S390: u16 = 22;
    /// IBM SPU/SPC
    pub const EM_SPU: u16 = 23;
    /// NEC V800
    pub const EM_V800: u16 = 36;
    /// Fujitsu FR20
    pub const EM_FR20: u16 = 37;
    /// TRW RH-32
    pub const EM_RH32: u16 = 38;
    /// Motorola RCE
    pub const EM_RCE: u16 = 39;
    /// ARM 32-bit
    pub const EM_ARM: u16 = 40;
    /// Digital Alpha
    pub const EM_ALPHA: u16 = 41;
    /// Hitachi SH
    pub const EM_SH: u16 = 42;
    /// SPARC V9 (64-bit)
    pub const EM_SPARCV9: u16 = 43;
    /// Siemens TriCore
    pub const EM_TRICORE: u16 = 44;
    /// Argonaut RISC Core
    pub const EM_ARC: u16 = 45;
    /// Hitachi H8/300
    pub const EM_H8_300: u16 = 46;
    /// Hitachi H8/300H
    pub const EM_H8_300H: u16 = 47;
    /// Hitachi H8S
    pub const EM_H8S: u16 = 48;
    /// Hitachi H8/500
    pub const EM_H8_500: u16 = 49;
    /// Intel Itanium
    pub const EM_IA_64: u16 = 50;
    /// Stanford MIPS-X
    pub const EM_MIPS_X: u16 = 51;
    /// Motorola ColdFire
    pub const EM_COLDFIRE: u16 = 52;
    /// Motorola 68HC12
    pub const EM_68HC12: u16 = 53;
    /// AMD x86-64
    pub const EM_X86_64: u16 = 62;
    /// Sony/Toshiba/IBM Cell BE SPU
    pub const EM_SPU_2: u16 = 23;
    /// ARM 64-bit (AArch64)
    pub const EM_AARCH64: u16 = 183;
    /// RISC-V
    pub const EM_RISCV: u16 = 243;
    /// Berkeley Packet Filter
    pub const EM_BPF: u16 = 247;
    /// WDC 65C816
    pub const EM_65816: u16 = 257;
    /// LoongArch
    pub const EM_LOONGARCH: u16 = 258;
}

// =============================================================================
// Program Header Types (p_type)
// =============================================================================

/// Program header type values (p_type)
pub mod pt_type {
    /// Unused entry
    pub const PT_NULL: u32 = 0;
    /// Loadable segment
    pub const PT_LOAD: u32 = 1;
    /// Dynamic linking information
    pub const PT_DYNAMIC: u32 = 2;
    /// Interpreter pathname
    pub const PT_INTERP: u32 = 3;
    /// Auxiliary information
    pub const PT_NOTE: u32 = 4;
    /// Reserved (unused)
    pub const PT_SHLIB: u32 = 5;
    /// Program header table
    pub const PT_PHDR: u32 = 6;
    /// Thread-local storage
    pub const PT_TLS: u32 = 7;
    /// OS-specific range start
    pub const PT_LOOS: u32 = 0x60000000;
    /// GNU exception handling frame
    pub const PT_GNU_EH_FRAME: u32 = 0x6474E550;
    /// GNU stack executability
    pub const PT_GNU_STACK: u32 = 0x6474E551;
    /// GNU read-only after relocation
    pub const PT_GNU_RELRO: u32 = 0x6474E552;
    /// GNU property
    pub const PT_GNU_PROPERTY: u32 = 0x6474E553;
    /// OS-specific range end
    pub const PT_HIOS: u32 = 0x6FFFFFFF;
    /// Processor-specific range start
    pub const PT_LOPROC: u32 = 0x70000000;
    /// Processor-specific range end
    pub const PT_HIPROC: u32 = 0x7FFFFFFF;
}

/// Program header flags (p_flags)
pub mod pf_flags {
    /// Execute permission
    pub const PF_X: u32 = 0x1;
    /// Write permission
    pub const PF_W: u32 = 0x2;
    /// Read permission
    pub const PF_R: u32 = 0x4;
    /// OS-specific mask
    pub const PF_MASKOS: u32 = 0x0FF00000;
    /// Processor-specific mask
    pub const PF_MASKPROC: u32 = 0xF0000000;
}

// =============================================================================
// Section Header Types (sh_type)
// =============================================================================

/// Section header type values (sh_type)
pub mod sh_type {
    /// Inactive section
    pub const SHT_NULL: u32 = 0;
    /// Program data
    pub const SHT_PROGBITS: u32 = 1;
    /// Symbol table
    pub const SHT_SYMTAB: u32 = 2;
    /// String table
    pub const SHT_STRTAB: u32 = 3;
    /// Relocation entries with addends
    pub const SHT_RELA: u32 = 4;
    /// Symbol hash table
    pub const SHT_HASH: u32 = 5;
    /// Dynamic linking information
    pub const SHT_DYNAMIC: u32 = 6;
    /// Notes
    pub const SHT_NOTE: u32 = 7;
    /// BSS (uninitialized data)
    pub const SHT_NOBITS: u32 = 8;
    /// Relocation entries without addends
    pub const SHT_REL: u32 = 9;
    /// Reserved
    pub const SHT_SHLIB: u32 = 10;
    /// Dynamic linker symbol table
    pub const SHT_DYNSYM: u32 = 11;
    /// Array of constructors
    pub const SHT_INIT_ARRAY: u32 = 14;
    /// Array of destructors
    pub const SHT_FINI_ARRAY: u32 = 15;
    /// Array of pre-constructors
    pub const SHT_PREINIT_ARRAY: u32 = 16;
    /// Section group
    pub const SHT_GROUP: u32 = 17;
    /// Extended section indices
    pub const SHT_SYMTAB_SHNDX: u32 = 18;
    /// OS-specific range start
    pub const SHT_LOOS: u32 = 0x60000000;
    /// GNU attributes
    pub const SHT_GNU_ATTRIBUTES: u32 = 0x6FFFFFF5;
    /// GNU symbol hash table
    pub const SHT_GNU_HASH: u32 = 0x6FFFFFF6;
    /// GNU library list
    pub const SHT_GNU_LIBLIST: u32 = 0x6FFFFFF7;
    /// GNU version definition
    pub const SHT_GNU_VERDEF: u32 = 0x6FFFFFFD;
    /// GNU version requirements
    pub const SHT_GNU_VERNEED: u32 = 0x6FFFFFFE;
    /// GNU version symbol table
    pub const SHT_GNU_VERSYM: u32 = 0x6FFFFFFF;
    /// OS-specific range end
    pub const SHT_HIOS: u32 = 0x6FFFFFFF;
    /// Processor-specific range start
    pub const SHT_LOPROC: u32 = 0x70000000;
    /// Processor-specific range end
    pub const SHT_HIPROC: u32 = 0x7FFFFFFF;
    /// Application-specific range start
    pub const SHT_LOUSER: u32 = 0x80000000;
    /// Application-specific range end
    pub const SHT_HIUSER: u32 = 0x8FFFFFFF;
}

/// Section header flags (sh_flags)
pub mod sh_flags {
    /// Writable data
    pub const SHF_WRITE: u64 = 0x1;
    /// Occupies memory during execution
    pub const SHF_ALLOC: u64 = 0x2;
    /// Executable instructions
    pub const SHF_EXECINSTR: u64 = 0x4;
    /// Might be merged
    pub const SHF_MERGE: u64 = 0x10;
    /// Contains null-terminated strings
    pub const SHF_STRINGS: u64 = 0x20;
    /// sh_info contains SHT index
    pub const SHF_INFO_LINK: u64 = 0x40;
    /// Preserve order after combining
    pub const SHF_LINK_ORDER: u64 = 0x80;
    /// Non-standard OS-specific handling required
    pub const SHF_OS_NONCONFORMING: u64 = 0x100;
    /// Section is member of a group
    pub const SHF_GROUP: u64 = 0x200;
    /// Section holds thread-local data
    pub const SHF_TLS: u64 = 0x400;
    /// Section is compressed
    pub const SHF_COMPRESSED: u64 = 0x800;
    /// OS-specific mask
    pub const SHF_MASKOS: u64 = 0x0FF00000;
    /// Processor-specific mask
    pub const SHF_MASKPROC: u64 = 0xF0000000;
}

// =============================================================================
// Dynamic Section Tags (d_tag)
// =============================================================================

/// Dynamic section tag values (d_tag)
pub mod dt_tag {
    /// Marks end of dynamic section
    pub const DT_NULL: i64 = 0;
    /// Name of needed library
    pub const DT_NEEDED: i64 = 1;
    /// Size in bytes of PLT relocs
    pub const DT_PLTRELSZ: i64 = 2;
    /// Processor-defined value
    pub const DT_PLTGOT: i64 = 3;
    /// Address of symbol hash table
    pub const DT_HASH: i64 = 4;
    /// Address of string table
    pub const DT_STRTAB: i64 = 5;
    /// Address of symbol table
    pub const DT_SYMTAB: i64 = 6;
    /// Address of RELA relocs
    pub const DT_RELA: i64 = 7;
    /// Total size of RELA relocs
    pub const DT_RELASZ: i64 = 8;
    /// Size of one RELA reloc
    pub const DT_RELAENT: i64 = 9;
    /// Size of string table
    pub const DT_STRSZ: i64 = 10;
    /// Size of one symbol table entry
    pub const DT_SYMENT: i64 = 11;
    /// Address of init function
    pub const DT_INIT: i64 = 12;
    /// Address of fini function
    pub const DT_FINI: i64 = 13;
    /// Name of shared object
    pub const DT_SONAME: i64 = 14;
    /// Library search path (deprecated)
    pub const DT_RPATH: i64 = 15;
    /// Start symbol search here
    pub const DT_SYMBOLIC: i64 = 16;
    /// Address of REL relocs
    pub const DT_REL: i64 = 17;
    /// Total size of REL relocs
    pub const DT_RELSZ: i64 = 18;
    /// Size of one REL reloc
    pub const DT_RELENT: i64 = 19;
    /// Type of reloc in PLT
    pub const DT_PLTREL: i64 = 20;
    /// For debugging; unspecified
    pub const DT_DEBUG: i64 = 21;
    /// Reloc might modify .text
    pub const DT_TEXTREL: i64 = 22;
    /// Address of PLT relocs
    pub const DT_JMPREL: i64 = 23;
    /// Process relocations of object
    pub const DT_BIND_NOW: i64 = 24;
    /// Array with addresses of init functions
    pub const DT_INIT_ARRAY: i64 = 25;
    /// Array with addresses of fini functions
    pub const DT_FINI_ARRAY: i64 = 26;
    /// Size in bytes of DT_INIT_ARRAY
    pub const DT_INIT_ARRAYSZ: i64 = 27;
    /// Size in bytes of DT_FINI_ARRAY
    pub const DT_FINI_ARRAYSZ: i64 = 28;
    /// Library search path
    pub const DT_RUNPATH: i64 = 29;
    /// Flags for the object
    pub const DT_FLAGS: i64 = 30;
    /// Start of encoded range
    pub const DT_ENCODING: i64 = 32;
    /// Array with addresses of preinit functions
    pub const DT_PREINIT_ARRAY: i64 = 32;
    /// Size in bytes of DT_PREINIT_ARRAY
    pub const DT_PREINIT_ARRAYSZ: i64 = 33;
    /// Address of SYMTAB_SHNDX section
    pub const DT_SYMTAB_SHNDX: i64 = 34;
    /// GNU hash table
    pub const DT_GNU_HASH: i64 = 0x6FFFFEF5;
    /// Relocation count
    pub const DT_RELACOUNT: i64 = 0x6FFFFFF9;
    /// Relocation count
    pub const DT_RELCOUNT: i64 = 0x6FFFFFFA;
    /// State flags
    pub const DT_FLAGS_1: i64 = 0x6FFFFFFB;
    /// Version definition table
    pub const DT_VERDEF: i64 = 0x6FFFFFFC;
    /// Number of version definitions
    pub const DT_VERDEFNUM: i64 = 0x6FFFFFFD;
    /// Version requirements table
    pub const DT_VERNEED: i64 = 0x6FFFFFFE;
    /// Number of version requirements
    pub const DT_VERNEEDNUM: i64 = 0x6FFFFFFF;
}

/// DT_FLAGS values
pub mod df_flags {
    /// Object may use DF_ORIGIN
    pub const DF_ORIGIN: u64 = 0x00000001;
    /// Symbol resolutions start here
    pub const DF_SYMBOLIC: u64 = 0x00000002;
    /// Object contains text relocations
    pub const DF_TEXTREL: u64 = 0x00000004;
    /// No lazy binding for this object
    pub const DF_BIND_NOW: u64 = 0x00000008;
    /// Module uses the static TLS model
    pub const DF_STATIC_TLS: u64 = 0x00000010;
}

/// DT_FLAGS_1 values
pub mod df1_flags {
    /// Set RTLD_NOW for this object
    pub const DF_1_NOW: u64 = 0x00000001;
    /// Set RTLD_GLOBAL for this object
    pub const DF_1_GLOBAL: u64 = 0x00000002;
    /// Set RTLD_GROUP for this object
    pub const DF_1_GROUP: u64 = 0x00000004;
    /// Set RTLD_NODELETE for this object
    pub const DF_1_NODELETE: u64 = 0x00000008;
    /// Trigger filtee loading at runtime
    pub const DF_1_LOADFLTR: u64 = 0x00000010;
    /// Set RTLD_INITFIRST for this object
    pub const DF_1_INITFIRST: u64 = 0x00000020;
    /// Set RTLD_NOOPEN for this object
    pub const DF_1_NOOPEN: u64 = 0x00000040;
    /// $ORIGIN must be handled
    pub const DF_1_ORIGIN: u64 = 0x00000080;
    /// Direct binding enabled
    pub const DF_1_DIRECT: u64 = 0x00000100;
    /// Object is used to interpose
    pub const DF_1_INTERPOSE: u64 = 0x00000400;
    /// Ignore default library search path
    pub const DF_1_NODEFLIB: u64 = 0x00000800;
    /// Object cannot be dlopen()'d
    pub const DF_1_NODUMP: u64 = 0x00001000;
    /// Configuration alternative created
    pub const DF_1_CONFALT: u64 = 0x00002000;
    /// Filtee terminates filters search
    pub const DF_1_ENDFILTEE: u64 = 0x00004000;
    /// Disp reloc applied at build time
    pub const DF_1_DISPRELDNE: u64 = 0x00008000;
    /// Disp reloc applied at run-time
    pub const DF_1_DISPRELPND: u64 = 0x00010000;
    /// Object has no-direct binding
    pub const DF_1_NODIRECT: u64 = 0x00020000;
    /// Internal use
    pub const DF_1_IGNMULDEF: u64 = 0x00040000;
    /// Internal use
    pub const DF_1_NOKSYMS: u64 = 0x00080000;
    /// Internal use
    pub const DF_1_NOHDR: u64 = 0x00100000;
    /// Object is modified after built
    pub const DF_1_EDITED: u64 = 0x00200000;
    /// Internal use
    pub const DF_1_NORELOC: u64 = 0x00400000;
    /// Object has individual interposers
    pub const DF_1_SYMINTPOSE: u64 = 0x00800000;
    /// Global auditing required
    pub const DF_1_GLOBAUDIT: u64 = 0x01000000;
    /// Singleton symbols are used
    pub const DF_1_SINGLETON: u64 = 0x02000000;
    /// Stub
    pub const DF_1_STUB: u64 = 0x04000000;
    /// Object is a Position-Independent Executable
    pub const DF_1_PIE: u64 = 0x08000000;
}

// =============================================================================
// Note Section Types
// =============================================================================

/// Note types for GNU namespace
pub mod nt_gnu {
    /// Build ID (unique binary identifier)
    pub const NT_GNU_BUILD_ID: u32 = 3;
    /// Gold version note
    pub const NT_GNU_GOLD_VERSION: u32 = 4;
    /// GNU property note
    pub const NT_GNU_PROPERTY_TYPE_0: u32 = 5;
}

/// Note types for core namespace (process info)
pub mod nt_core {
    /// OS ABI tag (GNU)
    pub const NT_GNU_ABI_TAG: u32 = 1;
}

// =============================================================================
// Symbol Table Structures
// =============================================================================

/// Symbol binding values (upper 4 bits of st_info)
pub mod stb_binding {
    /// Local symbol
    pub const STB_LOCAL: u8 = 0;
    /// Global symbol
    pub const STB_GLOBAL: u8 = 1;
    /// Weak symbol
    pub const STB_WEAK: u8 = 2;
    /// OS-specific range start
    pub const STB_LOOS: u8 = 10;
    /// GNU unique symbol
    pub const STB_GNU_UNIQUE: u8 = 10;
    /// OS-specific range end
    pub const STB_HIOS: u8 = 12;
    /// Processor-specific range start
    pub const STB_LOPROC: u8 = 13;
    /// Processor-specific range end
    pub const STB_HIPROC: u8 = 15;
}

/// Symbol type values (lower 4 bits of st_info)
pub mod stt_type {
    /// Symbol type is unspecified
    pub const STT_NOTYPE: u8 = 0;
    /// Symbol is a data object
    pub const STT_OBJECT: u8 = 1;
    /// Symbol is a code object (function)
    pub const STT_FUNC: u8 = 2;
    /// Symbol is a section
    pub const STT_SECTION: u8 = 3;
    /// Symbol's name is file name
    pub const STT_FILE: u8 = 4;
    /// Symbol is a common data object
    pub const STT_COMMON: u8 = 5;
    /// Symbol is thread-local storage object
    pub const STT_TLS: u8 = 6;
    /// OS-specific range start
    pub const STT_LOOS: u8 = 10;
    /// Symbol is an indirect function (GNU extension)
    pub const STT_GNU_IFUNC: u8 = 10;
    /// OS-specific range end
    pub const STT_HIOS: u8 = 12;
    /// Processor-specific range start
    pub const STT_LOPROC: u8 = 13;
    /// Processor-specific range end
    pub const STT_HIPROC: u8 = 15;
}

/// Special section indices
pub mod shn_index {
    /// Undefined section
    pub const SHN_UNDEF: u16 = 0;
    /// Start of processor-specific
    pub const SHN_LOPROC: u16 = 0xFF00;
    /// End of processor-specific
    pub const SHN_HIPROC: u16 = 0xFF1F;
    /// Start of OS-specific
    pub const SHN_LOOS: u16 = 0xFF20;
    /// End of OS-specific
    pub const SHN_HIOS: u16 = 0xFF3F;
    /// Associated symbol is absolute
    pub const SHN_ABS: u16 = 0xFFF1;
    /// Associated symbol is common
    pub const SHN_COMMON: u16 = 0xFFF2;
    /// Index is in extra table
    pub const SHN_XINDEX: u16 = 0xFFFF;
}

// =============================================================================
// ELF Header Structures
// =============================================================================

/// ELF64 Header (64 bytes)
///
/// This is the main header at the start of every 64-bit ELF file.
#[derive(Debug, Clone)]
pub struct Elf64Ehdr {
    /// ELF identification bytes (magic, class, endianness, version, OS/ABI)
    pub e_ident: [u8; 16],
    /// Object file type (ET_EXEC, ET_DYN, etc.)
    pub e_type: u16,
    /// Target machine architecture
    pub e_machine: u16,
    /// Object file version
    pub e_version: u32,
    /// Virtual address of entry point
    pub e_entry: u64,
    /// Program header table file offset
    pub e_phoff: u64,
    /// Section header table file offset
    pub e_shoff: u64,
    /// Processor-specific flags
    pub e_flags: u32,
    /// ELF header size in bytes
    pub e_ehsize: u16,
    /// Program header table entry size
    pub e_phentsize: u16,
    /// Program header table entry count
    pub e_phnum: u16,
    /// Section header table entry size
    pub e_shentsize: u16,
    /// Section header table entry count
    pub e_shnum: u16,
    /// Section name string table index
    pub e_shstrndx: u16,
}

/// ELF32 Header (52 bytes)
///
/// This is the main header at the start of every 32-bit ELF file.
#[derive(Debug, Clone)]
pub struct Elf32Ehdr {
    /// ELF identification bytes (magic, class, endianness, version, OS/ABI)
    pub e_ident: [u8; 16],
    /// Object file type (ET_EXEC, ET_DYN, etc.)
    pub e_type: u16,
    /// Target machine architecture
    pub e_machine: u16,
    /// Object file version
    pub e_version: u32,
    /// Virtual address of entry point
    pub e_entry: u32,
    /// Program header table file offset
    pub e_phoff: u32,
    /// Section header table file offset
    pub e_shoff: u32,
    /// Processor-specific flags
    pub e_flags: u32,
    /// ELF header size in bytes
    pub e_ehsize: u16,
    /// Program header table entry size
    pub e_phentsize: u16,
    /// Program header table entry count
    pub e_phnum: u16,
    /// Section header table entry size
    pub e_shentsize: u16,
    /// Section header table entry count
    pub e_shnum: u16,
    /// Section name string table index
    pub e_shstrndx: u16,
}

/// Unified ELF header that can represent both 32-bit and 64-bit headers
#[derive(Debug, Clone)]
pub struct ElfHeader {
    /// ELF identification bytes
    pub e_ident: [u8; 16],
    /// Object file type
    pub e_type: u16,
    /// Machine architecture
    pub e_machine: u16,
    /// Object file version
    pub e_version: u32,
    /// Entry point (virtual address)
    pub e_entry: u64,
    /// Program header table offset
    pub e_phoff: u64,
    /// Section header table offset
    pub e_shoff: u64,
    /// Processor-specific flags
    pub e_flags: u32,
    /// ELF header size
    pub e_ehsize: u16,
    /// Program header entry size
    pub e_phentsize: u16,
    /// Program header count
    pub e_phnum: u16,
    /// Section header entry size
    pub e_shentsize: u16,
    /// Section header count
    pub e_shnum: u16,
    /// Section name string table index
    pub e_shstrndx: u16,
    /// True if 64-bit, false if 32-bit
    pub is_64bit: bool,
    /// True if little-endian, false if big-endian
    pub is_little_endian: bool,
}

impl ElfHeader {
    /// Returns the ELF class as a human-readable string
    pub fn class_str(&self) -> &'static str {
        if self.is_64bit { "64-bit" } else { "32-bit" }
    }

    /// Returns the endianness as a human-readable string
    pub fn endian_str(&self) -> &'static str {
        if self.is_little_endian {
            "Little-endian"
        } else {
            "Big-endian"
        }
    }

    /// Returns the OS/ABI as a human-readable string
    pub fn osabi_str(&self) -> &'static str {
        match self.e_ident[ei_index::EI_OSABI] {
            elf_osabi::ELFOSABI_SYSV => "UNIX System V",
            elf_osabi::ELFOSABI_HPUX => "HP-UX",
            elf_osabi::ELFOSABI_NETBSD => "NetBSD",
            elf_osabi::ELFOSABI_GNU => "GNU/Linux",
            elf_osabi::ELFOSABI_SOLARIS => "Sun Solaris",
            elf_osabi::ELFOSABI_AIX => "IBM AIX",
            elf_osabi::ELFOSABI_IRIX => "SGI IRIX",
            elf_osabi::ELFOSABI_FREEBSD => "FreeBSD",
            elf_osabi::ELFOSABI_TRU64 => "Compaq TRU64 UNIX",
            elf_osabi::ELFOSABI_MODESTO => "Novell Modesto",
            elf_osabi::ELFOSABI_OPENBSD => "OpenBSD",
            elf_osabi::ELFOSABI_ARM_AEABI => "ARM EABI",
            elf_osabi::ELFOSABI_ARM => "ARM",
            elf_osabi::ELFOSABI_STANDALONE => "Standalone (embedded)",
            _ => "Unknown",
        }
    }

    /// Returns the object type as a human-readable string
    pub fn type_str(&self) -> &'static str {
        match self.e_type {
            elf_type::ET_NONE => "None",
            elf_type::ET_REL => "Relocatable",
            elf_type::ET_EXEC => "Executable",
            elf_type::ET_DYN => "Shared Object",
            elf_type::ET_CORE => "Core",
            _ if self.e_type >= elf_type::ET_LOOS && self.e_type <= elf_type::ET_HIOS => {
                "OS-specific"
            }
            _ if self.e_type >= elf_type::ET_LOPROC => "Processor-specific",
            _ => "Unknown",
        }
    }

    /// Returns the machine architecture as a human-readable string
    pub fn machine_str(&self) -> &'static str {
        match self.e_machine {
            machine_types::EM_NONE => "None",
            machine_types::EM_386 => "Intel 80386",
            machine_types::EM_68K => "Motorola 68000",
            machine_types::EM_MIPS => "MIPS",
            machine_types::EM_SPARC => "SPARC",
            machine_types::EM_PPC => "PowerPC",
            machine_types::EM_PPC64 => "PowerPC64",
            machine_types::EM_ARM => "ARM",
            machine_types::EM_SH => "SuperH",
            machine_types::EM_SPARCV9 => "SPARC V9",
            machine_types::EM_IA_64 => "Intel Itanium",
            machine_types::EM_X86_64 => "AMD x86-64",
            machine_types::EM_S390 => "IBM S390",
            machine_types::EM_AARCH64 => "ARM64",
            machine_types::EM_RISCV => "RISC-V",
            machine_types::EM_BPF => "Berkeley Packet Filter",
            machine_types::EM_LOONGARCH => "LoongArch",
            _ => "Unknown",
        }
    }
}

// =============================================================================
// Program Header Structures
// =============================================================================

/// ELF64 Program Header (56 bytes)
#[derive(Debug, Clone)]
pub struct Elf64Phdr {
    /// Segment type
    pub p_type: u32,
    /// Segment flags
    pub p_flags: u32,
    /// Segment file offset
    pub p_offset: u64,
    /// Segment virtual address
    pub p_vaddr: u64,
    /// Segment physical address
    pub p_paddr: u64,
    /// Segment size in file
    pub p_filesz: u64,
    /// Segment size in memory
    pub p_memsz: u64,
    /// Segment alignment
    pub p_align: u64,
}

/// ELF32 Program Header (32 bytes)
#[derive(Debug, Clone)]
pub struct Elf32Phdr {
    /// Segment type
    pub p_type: u32,
    /// Segment file offset
    pub p_offset: u32,
    /// Segment virtual address
    pub p_vaddr: u32,
    /// Segment physical address
    pub p_paddr: u32,
    /// Segment size in file
    pub p_filesz: u32,
    /// Segment size in memory
    pub p_memsz: u32,
    /// Segment flags
    pub p_flags: u32,
    /// Segment alignment
    pub p_align: u32,
}

/// Unified program header that can represent both 32-bit and 64-bit headers
#[derive(Debug, Clone)]
pub struct ProgramHeader {
    /// Segment type (PT_LOAD, PT_DYNAMIC, etc.)
    pub p_type: u32,
    /// Segment flags (PF_R, PF_W, PF_X)
    pub p_flags: u32,
    /// Segment file offset
    pub p_offset: u64,
    /// Segment virtual address
    pub p_vaddr: u64,
    /// Segment physical address
    pub p_paddr: u64,
    /// Segment size in file
    pub p_filesz: u64,
    /// Segment size in memory
    pub p_memsz: u64,
    /// Segment alignment
    pub p_align: u64,
}

impl ProgramHeader {
    /// Returns the segment type as a human-readable string
    pub fn type_str(&self) -> &'static str {
        match self.p_type {
            pt_type::PT_NULL => "NULL",
            pt_type::PT_LOAD => "LOAD",
            pt_type::PT_DYNAMIC => "DYNAMIC",
            pt_type::PT_INTERP => "INTERP",
            pt_type::PT_NOTE => "NOTE",
            pt_type::PT_SHLIB => "SHLIB",
            pt_type::PT_PHDR => "PHDR",
            pt_type::PT_TLS => "TLS",
            pt_type::PT_GNU_EH_FRAME => "GNU_EH_FRAME",
            pt_type::PT_GNU_STACK => "GNU_STACK",
            pt_type::PT_GNU_RELRO => "GNU_RELRO",
            pt_type::PT_GNU_PROPERTY => "GNU_PROPERTY",
            _ if self.p_type >= pt_type::PT_LOPROC => "PROC",
            _ if self.p_type >= pt_type::PT_LOOS => "OS",
            _ => "Unknown",
        }
    }

    /// Returns the flags as a human-readable string (e.g., "RWX")
    pub fn flags_str(&self) -> String {
        let mut flags = String::with_capacity(3);
        if self.p_flags & pf_flags::PF_R != 0 {
            flags.push('R');
        } else {
            flags.push('-');
        }
        if self.p_flags & pf_flags::PF_W != 0 {
            flags.push('W');
        } else {
            flags.push('-');
        }
        if self.p_flags & pf_flags::PF_X != 0 {
            flags.push('X');
        } else {
            flags.push('-');
        }
        flags
    }

    /// Returns true if this is a loadable segment
    pub fn is_load(&self) -> bool {
        self.p_type == pt_type::PT_LOAD
    }

    /// Returns true if this segment is executable
    pub fn is_executable(&self) -> bool {
        self.p_flags & pf_flags::PF_X != 0
    }
}

// =============================================================================
// Section Header Structures
// =============================================================================

/// ELF64 Section Header (64 bytes)
#[derive(Debug, Clone)]
pub struct Elf64Shdr {
    /// Section name (index into string table)
    pub sh_name: u32,
    /// Section type
    pub sh_type: u32,
    /// Section flags
    pub sh_flags: u64,
    /// Section virtual address at execution
    pub sh_addr: u64,
    /// Section file offset
    pub sh_offset: u64,
    /// Section size in bytes
    pub sh_size: u64,
    /// Link to another section
    pub sh_link: u32,
    /// Additional section information
    pub sh_info: u32,
    /// Section alignment
    pub sh_addralign: u64,
    /// Entry size if section holds table
    pub sh_entsize: u64,
}

/// ELF32 Section Header (40 bytes)
#[derive(Debug, Clone)]
pub struct Elf32Shdr {
    /// Section name (index into string table)
    pub sh_name: u32,
    /// Section type
    pub sh_type: u32,
    /// Section flags
    pub sh_flags: u32,
    /// Section virtual address at execution
    pub sh_addr: u32,
    /// Section file offset
    pub sh_offset: u32,
    /// Section size in bytes
    pub sh_size: u32,
    /// Link to another section
    pub sh_link: u32,
    /// Additional section information
    pub sh_info: u32,
    /// Section alignment
    pub sh_addralign: u32,
    /// Entry size if section holds table
    pub sh_entsize: u32,
}

/// Unified section header that can represent both 32-bit and 64-bit headers
#[derive(Debug, Clone)]
pub struct SectionHeader {
    /// Section name (index into string table)
    pub sh_name: u32,
    /// Resolved section name string (if available)
    pub name: Option<String>,
    /// Section type
    pub sh_type: u32,
    /// Section flags
    pub sh_flags: u64,
    /// Section virtual address
    pub sh_addr: u64,
    /// Section file offset
    pub sh_offset: u64,
    /// Section size in bytes
    pub sh_size: u64,
    /// Link to another section
    pub sh_link: u32,
    /// Additional section information
    pub sh_info: u32,
    /// Section alignment
    pub sh_addralign: u64,
    /// Entry size if section holds table
    pub sh_entsize: u64,
}

impl SectionHeader {
    /// Returns the section type as a human-readable string
    pub fn type_str(&self) -> &'static str {
        match self.sh_type {
            sh_type::SHT_NULL => "NULL",
            sh_type::SHT_PROGBITS => "PROGBITS",
            sh_type::SHT_SYMTAB => "SYMTAB",
            sh_type::SHT_STRTAB => "STRTAB",
            sh_type::SHT_RELA => "RELA",
            sh_type::SHT_HASH => "HASH",
            sh_type::SHT_DYNAMIC => "DYNAMIC",
            sh_type::SHT_NOTE => "NOTE",
            sh_type::SHT_NOBITS => "NOBITS",
            sh_type::SHT_REL => "REL",
            sh_type::SHT_SHLIB => "SHLIB",
            sh_type::SHT_DYNSYM => "DYNSYM",
            sh_type::SHT_INIT_ARRAY => "INIT_ARRAY",
            sh_type::SHT_FINI_ARRAY => "FINI_ARRAY",
            sh_type::SHT_PREINIT_ARRAY => "PREINIT_ARRAY",
            sh_type::SHT_GROUP => "GROUP",
            sh_type::SHT_SYMTAB_SHNDX => "SYMTAB_SHNDX",
            sh_type::SHT_GNU_HASH => "GNU_HASH",
            sh_type::SHT_GNU_VERDEF => "VERDEF",
            sh_type::SHT_GNU_VERNEED => "VERNEED",
            sh_type::SHT_GNU_VERSYM => "VERSYM",
            _ if self.sh_type >= sh_type::SHT_LOPROC && self.sh_type <= sh_type::SHT_HIPROC => {
                "PROC"
            }
            _ if self.sh_type >= sh_type::SHT_LOUSER => "USER",
            _ => "Unknown",
        }
    }

    /// Returns the flags as a human-readable string
    pub fn flags_str(&self) -> String {
        let mut flags = Vec::new();
        if self.sh_flags & sh_flags::SHF_WRITE != 0 {
            flags.push("W");
        }
        if self.sh_flags & sh_flags::SHF_ALLOC != 0 {
            flags.push("A");
        }
        if self.sh_flags & sh_flags::SHF_EXECINSTR != 0 {
            flags.push("X");
        }
        if self.sh_flags & sh_flags::SHF_MERGE != 0 {
            flags.push("M");
        }
        if self.sh_flags & sh_flags::SHF_STRINGS != 0 {
            flags.push("S");
        }
        if self.sh_flags & sh_flags::SHF_TLS != 0 {
            flags.push("T");
        }
        if flags.is_empty() {
            "---".to_string()
        } else {
            flags.join("")
        }
    }

    /// Returns the section name, falling back to the name index if not resolved
    pub fn name_str(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("<{}>", self.sh_name))
    }
}

// =============================================================================
// Dynamic Section Structures
// =============================================================================

/// Dynamic section entry
#[derive(Debug, Clone)]
pub struct DynamicEntry {
    /// Entry tag (DT_NEEDED, DT_SONAME, etc.)
    pub d_tag: i64,
    /// Entry value (address or offset)
    pub d_val: u64,
}

impl DynamicEntry {
    /// Returns the tag as a human-readable string
    pub fn tag_str(&self) -> &'static str {
        match self.d_tag {
            dt_tag::DT_NULL => "NULL",
            dt_tag::DT_NEEDED => "NEEDED",
            dt_tag::DT_PLTRELSZ => "PLTRELSZ",
            dt_tag::DT_PLTGOT => "PLTGOT",
            dt_tag::DT_HASH => "HASH",
            dt_tag::DT_STRTAB => "STRTAB",
            dt_tag::DT_SYMTAB => "SYMTAB",
            dt_tag::DT_RELA => "RELA",
            dt_tag::DT_RELASZ => "RELASZ",
            dt_tag::DT_RELAENT => "RELAENT",
            dt_tag::DT_STRSZ => "STRSZ",
            dt_tag::DT_SYMENT => "SYMENT",
            dt_tag::DT_INIT => "INIT",
            dt_tag::DT_FINI => "FINI",
            dt_tag::DT_SONAME => "SONAME",
            dt_tag::DT_RPATH => "RPATH",
            dt_tag::DT_SYMBOLIC => "SYMBOLIC",
            dt_tag::DT_REL => "REL",
            dt_tag::DT_RELSZ => "RELSZ",
            dt_tag::DT_RELENT => "RELENT",
            dt_tag::DT_PLTREL => "PLTREL",
            dt_tag::DT_DEBUG => "DEBUG",
            dt_tag::DT_TEXTREL => "TEXTREL",
            dt_tag::DT_JMPREL => "JMPREL",
            dt_tag::DT_BIND_NOW => "BIND_NOW",
            dt_tag::DT_INIT_ARRAY => "INIT_ARRAY",
            dt_tag::DT_FINI_ARRAY => "FINI_ARRAY",
            dt_tag::DT_INIT_ARRAYSZ => "INIT_ARRAYSZ",
            dt_tag::DT_FINI_ARRAYSZ => "FINI_ARRAYSZ",
            dt_tag::DT_RUNPATH => "RUNPATH",
            dt_tag::DT_FLAGS => "FLAGS",
            dt_tag::DT_GNU_HASH => "GNU_HASH",
            dt_tag::DT_FLAGS_1 => "FLAGS_1",
            dt_tag::DT_VERDEF => "VERDEF",
            dt_tag::DT_VERDEFNUM => "VERDEFNUM",
            dt_tag::DT_VERNEED => "VERNEED",
            dt_tag::DT_VERNEEDNUM => "VERNEEDNUM",
            _ => "Unknown",
        }
    }
}

// =============================================================================
// Symbol Table Structures
// =============================================================================

/// ELF64 Symbol table entry (24 bytes)
#[derive(Debug, Clone)]
pub struct Elf64Sym {
    /// Symbol name (index into string table)
    pub st_name: u32,
    /// Symbol type and binding
    pub st_info: u8,
    /// Symbol visibility
    pub st_other: u8,
    /// Section index
    pub st_shndx: u16,
    /// Symbol value
    pub st_value: u64,
    /// Symbol size
    pub st_size: u64,
}

/// ELF32 Symbol table entry (16 bytes)
#[derive(Debug, Clone)]
pub struct Elf32Sym {
    /// Symbol name (index into string table)
    pub st_name: u32,
    /// Symbol value
    pub st_value: u32,
    /// Symbol size
    pub st_size: u32,
    /// Symbol type and binding
    pub st_info: u8,
    /// Symbol visibility
    pub st_other: u8,
    /// Section index
    pub st_shndx: u16,
}

/// Unified symbol table entry
#[derive(Debug, Clone)]
pub struct Symbol {
    /// Symbol name (index into string table)
    pub st_name: u32,
    /// Resolved symbol name (if available)
    pub name: Option<String>,
    /// Symbol type and binding
    pub st_info: u8,
    /// Symbol visibility
    pub st_other: u8,
    /// Section index
    pub st_shndx: u16,
    /// Symbol value
    pub st_value: u64,
    /// Symbol size
    pub st_size: u64,
}

impl Symbol {
    /// Returns the symbol binding (STB_LOCAL, STB_GLOBAL, STB_WEAK)
    pub fn binding(&self) -> u8 {
        self.st_info >> 4
    }

    /// Returns the symbol type (STT_NOTYPE, STT_OBJECT, STT_FUNC, etc.)
    pub fn sym_type(&self) -> u8 {
        self.st_info & 0xF
    }

    /// Returns the symbol binding as a human-readable string
    pub fn binding_str(&self) -> &'static str {
        match self.binding() {
            stb_binding::STB_LOCAL => "LOCAL",
            stb_binding::STB_GLOBAL => "GLOBAL",
            stb_binding::STB_WEAK => "WEAK",
            stb_binding::STB_GNU_UNIQUE => "UNIQUE",
            _ => "Unknown",
        }
    }

    /// Returns the symbol type as a human-readable string
    pub fn type_str(&self) -> &'static str {
        match self.sym_type() {
            stt_type::STT_NOTYPE => "NOTYPE",
            stt_type::STT_OBJECT => "OBJECT",
            stt_type::STT_FUNC => "FUNC",
            stt_type::STT_SECTION => "SECTION",
            stt_type::STT_FILE => "FILE",
            stt_type::STT_COMMON => "COMMON",
            stt_type::STT_TLS => "TLS",
            stt_type::STT_GNU_IFUNC => "IFUNC",
            _ => "Unknown",
        }
    }

    /// Returns true if this is a defined (non-undefined) symbol
    pub fn is_defined(&self) -> bool {
        self.st_shndx != shn_index::SHN_UNDEF
    }

    /// Returns true if this is a function symbol
    pub fn is_function(&self) -> bool {
        self.sym_type() == stt_type::STT_FUNC
    }

    /// Returns true if this is a global symbol
    pub fn is_global(&self) -> bool {
        self.binding() == stb_binding::STB_GLOBAL
    }

    /// Returns the symbol name, falling back to the name index if not resolved
    pub fn name_str(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("<{}>", self.st_name))
    }
}

// =============================================================================
// Note Section Structures
// =============================================================================

/// Note section entry
#[derive(Debug, Clone)]
pub struct NoteEntry {
    /// Owner name
    pub name: String,
    /// Note type
    pub note_type: u32,
    /// Note descriptor (payload)
    pub desc: Vec<u8>,
}

impl NoteEntry {
    /// Returns the note type as a human-readable string for GNU notes
    pub fn gnu_type_str(&self) -> &'static str {
        if self.name == "GNU" {
            match self.note_type {
                nt_core::NT_GNU_ABI_TAG => "ABI tag",
                nt_gnu::NT_GNU_BUILD_ID => "Build ID",
                nt_gnu::NT_GNU_GOLD_VERSION => "Gold version",
                nt_gnu::NT_GNU_PROPERTY_TYPE_0 => "Property",
                _ => "Unknown",
            }
        } else {
            "Unknown"
        }
    }

    /// Returns the build ID as a hex string if this is a build ID note
    pub fn build_id_hex(&self) -> Option<String> {
        if self.name == "GNU" && self.note_type == nt_gnu::NT_GNU_BUILD_ID {
            Some(
                self.desc
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>(),
            )
        } else {
            None
        }
    }
}

// =============================================================================
// Parsed ELF Information
// =============================================================================

/// Dynamic linking information extracted from the ELF file
#[derive(Debug, Clone, Default)]
pub struct DynamicInfo {
    /// List of needed shared libraries (from DT_NEEDED)
    pub needed: Vec<String>,
    /// Shared object name (from DT_SONAME)
    pub soname: Option<String>,
    /// Library search paths (from DT_RPATH, deprecated)
    pub rpath: Vec<String>,
    /// Library search paths (from DT_RUNPATH)
    pub runpath: Vec<String>,
    /// Interpreter path (from PT_INTERP)
    pub interpreter: Option<String>,
    /// Has TEXTREL flag (indicates writable text relocations)
    pub has_textrel: bool,
    /// Has BIND_NOW flag (no lazy binding)
    pub bind_now: bool,
    /// Raw DT_FLAGS value
    pub flags: u64,
    /// Raw DT_FLAGS_1 value
    pub flags_1: u64,
}

impl DynamicInfo {
    /// Returns true if the binary is PIE (Position Independent Executable)
    pub fn is_pie(&self) -> bool {
        self.flags_1 & df1_flags::DF_1_PIE != 0
    }

    /// Returns true if the binary has RELRO (read-only relocations)
    pub fn has_relro(&self) -> bool {
        // RELRO is detected via PT_GNU_RELRO program header, not DT_FLAGS
        // This is a placeholder - actual detection happens in program header parsing
        false
    }
}

/// Symbol table information
#[derive(Debug, Clone, Default)]
pub struct SymbolInfo {
    /// Total number of symbols in .symtab
    pub symbol_count: usize,
    /// Total number of dynamic symbols in .dynsym
    pub dynamic_symbol_count: usize,
    /// Exported function names (defined, global functions)
    pub exported_functions: Vec<String>,
    /// Imported function names (undefined, global functions)
    pub imported_functions: Vec<String>,
}

/// Comprehensive ELF file information
#[derive(Debug, Clone)]
pub struct ElfInfo {
    /// ELF header
    pub header: ElfHeader,
    /// Program headers
    pub program_headers: Vec<ProgramHeader>,
    /// Section headers
    pub section_headers: Vec<SectionHeader>,
    /// Dynamic linking information
    pub dynamic_info: DynamicInfo,
    /// Symbol table information
    pub symbol_info: SymbolInfo,
    /// Note entries (build ID, ABI tag, etc.)
    pub notes: Vec<NoteEntry>,
    /// Build ID (if found)
    pub build_id: Option<String>,
    /// Has GNU RELRO (read-only relocations after loading)
    pub has_relro: bool,
    /// Has executable stack
    pub has_executable_stack: bool,
    /// Has stack canary (detected via __stack_chk_fail import)
    pub has_stack_canary: bool,
}

impl ElfInfo {
    /// Creates a new ElfInfo with default values
    pub fn new(header: ElfHeader) -> Self {
        Self {
            header,
            program_headers: Vec::new(),
            section_headers: Vec::new(),
            dynamic_info: DynamicInfo::default(),
            symbol_info: SymbolInfo::default(),
            notes: Vec::new(),
            build_id: None,
            has_relro: false,
            has_executable_stack: false,
            has_stack_canary: false,
        }
    }

    /// Returns true if this appears to be a Position Independent Executable
    pub fn is_pie(&self) -> bool {
        // PIE executables are ET_DYN with an entry point
        self.header.e_type == elf_type::ET_DYN && self.header.e_entry != 0
    }

    /// Returns the total size of .text section (if found)
    pub fn text_section_size(&self) -> Option<u64> {
        self.section_headers
            .iter()
            .find(|s| s.name.as_deref() == Some(".text"))
            .map(|s| s.sh_size)
    }

    /// Returns the total size of .data section (if found)
    pub fn data_section_size(&self) -> Option<u64> {
        self.section_headers
            .iter()
            .find(|s| s.name.as_deref() == Some(".data"))
            .map(|s| s.sh_size)
    }

    /// Returns the count of loadable segments
    pub fn loadable_segment_count(&self) -> usize {
        self.program_headers
            .iter()
            .filter(|p| p.p_type == pt_type::PT_LOAD)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elf_header_class_str() {
        let mut header = ElfHeader {
            e_ident: [0; 16],
            e_type: elf_type::ET_EXEC,
            e_machine: machine_types::EM_X86_64,
            e_version: 1,
            e_entry: 0x1000,
            e_phoff: 64,
            e_shoff: 0,
            e_flags: 0,
            e_ehsize: 64,
            e_phentsize: 56,
            e_phnum: 1,
            e_shentsize: 64,
            e_shnum: 0,
            e_shstrndx: 0,
            is_64bit: true,
            is_little_endian: true,
        };

        assert_eq!(header.class_str(), "64-bit");
        header.is_64bit = false;
        assert_eq!(header.class_str(), "32-bit");
    }

    #[test]
    fn test_program_header_flags_str() {
        let mut ph = ProgramHeader {
            p_type: pt_type::PT_LOAD,
            p_flags: pf_flags::PF_R | pf_flags::PF_X,
            p_offset: 0,
            p_vaddr: 0,
            p_paddr: 0,
            p_filesz: 0,
            p_memsz: 0,
            p_align: 0,
        };

        assert_eq!(ph.flags_str(), "R-X");

        ph.p_flags = pf_flags::PF_R | pf_flags::PF_W;
        assert_eq!(ph.flags_str(), "RW-");

        ph.p_flags = pf_flags::PF_R | pf_flags::PF_W | pf_flags::PF_X;
        assert_eq!(ph.flags_str(), "RWX");
    }

    #[test]
    fn test_symbol_binding_and_type() {
        let sym = Symbol {
            st_name: 0,
            name: Some("main".to_string()),
            st_info: (stb_binding::STB_GLOBAL << 4) | stt_type::STT_FUNC,
            st_other: 0,
            st_shndx: 1,
            st_value: 0x1000,
            st_size: 100,
        };

        assert_eq!(sym.binding(), stb_binding::STB_GLOBAL);
        assert_eq!(sym.sym_type(), stt_type::STT_FUNC);
        assert_eq!(sym.binding_str(), "GLOBAL");
        assert_eq!(sym.type_str(), "FUNC");
        assert!(sym.is_defined());
        assert!(sym.is_function());
        assert!(sym.is_global());
    }

    #[test]
    fn test_note_build_id() {
        let note = NoteEntry {
            name: "GNU".to_string(),
            note_type: nt_gnu::NT_GNU_BUILD_ID,
            desc: vec![0xDE, 0xAD, 0xBE, 0xEF],
        };

        assert_eq!(note.build_id_hex(), Some("deadbeef".to_string()));

        let other_note = NoteEntry {
            name: "GNU".to_string(),
            note_type: nt_core::NT_GNU_ABI_TAG,
            desc: vec![0x00],
        };

        assert_eq!(other_note.build_id_hex(), None);
    }
}
