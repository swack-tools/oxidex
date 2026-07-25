//! Mach-O file structure definitions
//!
//! This module defines Rust structures matching Mach-O file headers and load commands
//! as per Apple's Mach-O ABI specification (mach/loader.h).

#![allow(dead_code)]

// =============================================================================
// Magic Numbers
// =============================================================================

/// Mach-O magic numbers for file identification
pub mod magic {
    /// 32-bit Mach-O file, native byte order (0xFEEDFACE)
    pub const MH_MAGIC: u32 = 0xFEED_FACE;
    /// 64-bit Mach-O file, native byte order (0xFEEDFACF)
    pub const MH_MAGIC_64: u32 = 0xFEED_FACF;
    /// 32-bit Mach-O file, swapped byte order (0xCEFAEDFE)
    pub const MH_CIGAM: u32 = 0xCEFA_EDFE;
    /// 64-bit Mach-O file, swapped byte order (0xCFFAEDFE)
    pub const MH_CIGAM_64: u32 = 0xCFFA_EDFE;
    /// FAT binary magic (0xCAFEBABE)
    pub const FAT_MAGIC: u32 = 0xCAFE_BABE;
    /// FAT binary magic, swapped (0xBEBAFECA)
    pub const FAT_CIGAM: u32 = 0xBEBA_FECA;
    /// FAT binary 64-bit magic (0xCAFEBABF)
    pub const FAT_MAGIC_64: u32 = 0xCAFE_BABF;
    /// FAT binary 64-bit magic, swapped (0xBFBAFECA)
    pub const FAT_CIGAM_64: u32 = 0xBFBA_FECA;
}

// =============================================================================
// CPU Types
// =============================================================================

/// CPU type constants (cputype field in mach_header)
pub mod cpu_type {
    /// Any CPU type
    pub const CPU_TYPE_ANY: i32 = -1;
    /// x86 (i386)
    pub const CPU_TYPE_I386: i32 = 7;
    /// x86_64
    pub const CPU_TYPE_X86_64: i32 = 0x0100_0007;
    /// ARM 32-bit
    pub const CPU_TYPE_ARM: i32 = 12;
    /// ARM 64-bit
    pub const CPU_TYPE_ARM64: i32 = 0x0100_000C;
    /// ARM 64-bit (32-bit pointers)
    pub const CPU_TYPE_ARM64_32: i32 = 0x0200_000C;
    /// PowerPC
    pub const CPU_TYPE_POWERPC: i32 = 18;
    /// PowerPC 64-bit
    pub const CPU_TYPE_POWERPC64: i32 = 0x0100_0012;
}

/// CPU subtype constants for ARM64
pub mod cpu_subtype_arm64 {
    /// All ARM64 subtypes
    pub const CPU_SUBTYPE_ARM64_ALL: i32 = 0;
    /// ARM64e (Apple Silicon with pointer authentication)
    pub const CPU_SUBTYPE_ARM64E: i32 = 2;
    /// ARM64 v8
    pub const CPU_SUBTYPE_ARM64_V8: i32 = 1;
}

/// CPU subtype constants for x86_64
pub mod cpu_subtype_x86_64 {
    /// All x86_64 subtypes
    pub const CPU_SUBTYPE_X86_64_ALL: i32 = 3;
    /// Haswell and later
    pub const CPU_SUBTYPE_X86_64_H: i32 = 8;
}

// =============================================================================
// File Types
// =============================================================================

/// Mach-O file type constants (filetype field in mach_header)
pub mod file_type {
    /// Relocatable object file
    pub const MH_OBJECT: u32 = 0x1;
    /// Demand paged executable file
    pub const MH_EXECUTE: u32 = 0x2;
    /// Fixed VM shared library file
    pub const MH_FVMLIB: u32 = 0x3;
    /// Core file
    pub const MH_CORE: u32 = 0x4;
    /// Preloaded executable file
    pub const MH_PRELOAD: u32 = 0x5;
    /// Dynamically bound shared library
    pub const MH_DYLIB: u32 = 0x6;
    /// Dynamic link editor
    pub const MH_DYLINKER: u32 = 0x7;
    /// Dynamically bound bundle file
    pub const MH_BUNDLE: u32 = 0x8;
    /// Shared library stub for static linking only
    pub const MH_DYLIB_STUB: u32 = 0x9;
    /// Companion file with only debug sections
    pub const MH_DSYM: u32 = 0xA;
    /// Kext bundle
    pub const MH_KEXT_BUNDLE: u32 = 0xB;
    /// Set of Mach-Os to be run in same process space
    pub const MH_FILESET: u32 = 0xC;
}

/// Returns human-readable file type name
pub fn file_type_name(filetype: u32) -> &'static str {
    match filetype {
        file_type::MH_OBJECT => "Object",
        file_type::MH_EXECUTE => "Executable",
        file_type::MH_FVMLIB => "Fixed VM Library",
        file_type::MH_CORE => "Core",
        file_type::MH_PRELOAD => "Preload",
        file_type::MH_DYLIB => "Dynamic Library",
        file_type::MH_DYLINKER => "Dynamic Linker",
        file_type::MH_BUNDLE => "Bundle",
        file_type::MH_DYLIB_STUB => "Dynamic Library Stub",
        file_type::MH_DSYM => "Debug Symbols",
        file_type::MH_KEXT_BUNDLE => "Kernel Extension",
        file_type::MH_FILESET => "Fileset",
        _ => "Unknown",
    }
}

// =============================================================================
// Header Flags
// =============================================================================

/// Mach-O header flags (flags field in mach_header)
pub mod flags {
    /// The object file has no undefined references
    pub const MH_NOUNDEFS: u32 = 0x0000_0001;
    /// The object file is the output of an incremental link
    pub const MH_INCRLINK: u32 = 0x0000_0002;
    /// The object file is input for the dynamic linker
    pub const MH_DYLDLINK: u32 = 0x0000_0004;
    /// The object file's undefined references are bound by the dynamic linker
    pub const MH_BINDATLOAD: u32 = 0x0000_0008;
    /// The file has its dynamic undefined references prebound
    pub const MH_PREBOUND: u32 = 0x0000_0010;
    /// The file has its read-only and read-write segments split
    pub const MH_SPLIT_SEGS: u32 = 0x0000_0020;
    /// The shared library init routine is to be run lazily
    pub const MH_LAZY_INIT: u32 = 0x0000_0040;
    /// The image is using two-level namespace bindings
    pub const MH_TWOLEVEL: u32 = 0x0000_0080;
    /// The executable is forcing flat namespace bindings
    pub const MH_FORCE_FLAT: u32 = 0x0000_0100;
    /// This umbrella guarantees no multiple definitions
    pub const MH_NOMULTIDEFS: u32 = 0x0000_0200;
    /// Do not have dyld notify the prebinding agent
    pub const MH_NOFIXPREBINDING: u32 = 0x0000_0400;
    /// The binary is not prebound but can have its prebinding redone
    pub const MH_PREBINDABLE: u32 = 0x0000_0800;
    /// Indicates that this binary binds to all two-level namespace modules
    pub const MH_ALLMODSBOUND: u32 = 0x0000_1000;
    /// Safe to divide up sections into sub-sections
    pub const MH_SUBSECTIONS_VIA_SYMBOLS: u32 = 0x0000_2000;
    /// The binary has been canonicalized
    pub const MH_CANONICAL: u32 = 0x0000_4000;
    /// The final linked image contains external weak symbols
    pub const MH_WEAK_DEFINES: u32 = 0x0000_8000;
    /// The final linked image uses weak symbols
    pub const MH_BINDS_TO_WEAK: u32 = 0x0001_0000;
    /// Allow stack execution
    pub const MH_ALLOW_STACK_EXECUTION: u32 = 0x0002_0000;
    /// The binary declares it is safe for use in processes with uid zero
    pub const MH_ROOT_SAFE: u32 = 0x0004_0000;
    /// The binary declares it is safe for use in processes when issetugid() is true
    pub const MH_SETUID_SAFE: u32 = 0x0008_0000;
    /// The static linker does not need to examine dependent dylibs
    pub const MH_NO_REEXPORTED_DYLIBS: u32 = 0x0010_0000;
    /// The OS will load the main executable at a random address (ASLR/PIE)
    pub const MH_PIE: u32 = 0x0020_0000;
    /// Only for use on dylibs. When linking against a dylib
    pub const MH_DEAD_STRIPPABLE_DYLIB: u32 = 0x0040_0000;
    /// Contains a section of type S_THREAD_LOCAL_VARIABLES
    pub const MH_HAS_TLV_DESCRIPTORS: u32 = 0x0080_0000;
    /// When this bit is set, the OS will run the main executable with a non-executable heap
    pub const MH_NO_HEAP_EXECUTION: u32 = 0x0100_0000;
    /// The code was linked for use in an application extension
    pub const MH_APP_EXTENSION_SAFE: u32 = 0x0200_0000;
    /// External symbols listed in nlist should not be considered
    pub const MH_NLIST_OUTOFSYNC_WITH_DYLDINFO: u32 = 0x0400_0000;
    /// Allow LC_MIN_VERSION_MACOS and LC_BUILD_VERSION load commands with the platforms macOS, macCatalyst, iOSSimulator, tvOSSimulator and watchOSSimulator
    pub const MH_SIM_SUPPORT: u32 = 0x0800_0000;
}

/// Returns list of flag names for a given flags value
pub fn decode_flags(flag_value: u32) -> Vec<&'static str> {
    let mut result = Vec::new();
    if flag_value & flags::MH_NOUNDEFS != 0 {
        result.push("NOUNDEFS");
    }
    if flag_value & flags::MH_INCRLINK != 0 {
        result.push("INCRLINK");
    }
    if flag_value & flags::MH_DYLDLINK != 0 {
        result.push("DYLDLINK");
    }
    if flag_value & flags::MH_BINDATLOAD != 0 {
        result.push("BINDATLOAD");
    }
    if flag_value & flags::MH_PREBOUND != 0 {
        result.push("PREBOUND");
    }
    if flag_value & flags::MH_SPLIT_SEGS != 0 {
        result.push("SPLIT_SEGS");
    }
    if flag_value & flags::MH_LAZY_INIT != 0 {
        result.push("LAZY_INIT");
    }
    if flag_value & flags::MH_TWOLEVEL != 0 {
        result.push("TWOLEVEL");
    }
    if flag_value & flags::MH_FORCE_FLAT != 0 {
        result.push("FORCE_FLAT");
    }
    if flag_value & flags::MH_NOMULTIDEFS != 0 {
        result.push("NOMULTIDEFS");
    }
    if flag_value & flags::MH_NOFIXPREBINDING != 0 {
        result.push("NOFIXPREBINDING");
    }
    if flag_value & flags::MH_PREBINDABLE != 0 {
        result.push("PREBINDABLE");
    }
    if flag_value & flags::MH_ALLMODSBOUND != 0 {
        result.push("ALLMODSBOUND");
    }
    if flag_value & flags::MH_SUBSECTIONS_VIA_SYMBOLS != 0 {
        result.push("SUBSECTIONS_VIA_SYMBOLS");
    }
    if flag_value & flags::MH_CANONICAL != 0 {
        result.push("CANONICAL");
    }
    if flag_value & flags::MH_WEAK_DEFINES != 0 {
        result.push("WEAK_DEFINES");
    }
    if flag_value & flags::MH_BINDS_TO_WEAK != 0 {
        result.push("BINDS_TO_WEAK");
    }
    if flag_value & flags::MH_ALLOW_STACK_EXECUTION != 0 {
        result.push("ALLOW_STACK_EXECUTION");
    }
    if flag_value & flags::MH_ROOT_SAFE != 0 {
        result.push("ROOT_SAFE");
    }
    if flag_value & flags::MH_SETUID_SAFE != 0 {
        result.push("SETUID_SAFE");
    }
    if flag_value & flags::MH_NO_REEXPORTED_DYLIBS != 0 {
        result.push("NO_REEXPORTED_DYLIBS");
    }
    if flag_value & flags::MH_PIE != 0 {
        result.push("PIE");
    }
    if flag_value & flags::MH_DEAD_STRIPPABLE_DYLIB != 0 {
        result.push("DEAD_STRIPPABLE_DYLIB");
    }
    if flag_value & flags::MH_HAS_TLV_DESCRIPTORS != 0 {
        result.push("HAS_TLV_DESCRIPTORS");
    }
    if flag_value & flags::MH_NO_HEAP_EXECUTION != 0 {
        result.push("NO_HEAP_EXECUTION");
    }
    if flag_value & flags::MH_APP_EXTENSION_SAFE != 0 {
        result.push("APP_EXTENSION_SAFE");
    }
    if flag_value & flags::MH_SIM_SUPPORT != 0 {
        result.push("SIM_SUPPORT");
    }
    result
}

// =============================================================================
// Load Command Types
// =============================================================================

/// Load command type constants (cmd field in load_command)
pub mod load_command {
    /// Segment of this file to be mapped
    pub const LC_SEGMENT: u32 = 0x1;
    /// Link-edit stab symbol table info
    pub const LC_SYMTAB: u32 = 0x2;
    /// Link-edit gdb symbol table info (obsolete)
    pub const LC_SYMSEG: u32 = 0x3;
    /// Thread
    pub const LC_THREAD: u32 = 0x4;
    /// Unix thread (includes a stack)
    pub const LC_UNIXTHREAD: u32 = 0x5;
    /// Load a fixed VM shared library (obsolete)
    pub const LC_LOADFVMLIB: u32 = 0x6;
    /// Fixed VM shared library identification (obsolete)
    pub const LC_IDFVMLIB: u32 = 0x7;
    /// Object identification info (obsolete)
    pub const LC_IDENT: u32 = 0x8;
    /// Fixed VM file inclusion (internal use)
    pub const LC_FVMFILE: u32 = 0x9;
    /// Prepage command (internal use)
    pub const LC_PREPAGE: u32 = 0xA;
    /// Dynamic link-edit symbol table info
    pub const LC_DYSYMTAB: u32 = 0xB;
    /// Load a dynamically linked shared library
    pub const LC_LOAD_DYLIB: u32 = 0xC;
    /// Dynamically linked shared library identification
    pub const LC_ID_DYLIB: u32 = 0xD;
    /// Load a dynamic linker
    pub const LC_LOAD_DYLINKER: u32 = 0xE;
    /// Dynamic linker identification
    pub const LC_ID_DYLINKER: u32 = 0xF;
    /// Modules prebound for a dynamically linked shared library
    pub const LC_PREBOUND_DYLIB: u32 = 0x10;
    /// Image routines
    pub const LC_ROUTINES: u32 = 0x11;
    /// Sub framework
    pub const LC_SUB_FRAMEWORK: u32 = 0x12;
    /// Sub umbrella
    pub const LC_SUB_UMBRELLA: u32 = 0x13;
    /// Sub client
    pub const LC_SUB_CLIENT: u32 = 0x14;
    /// Sub library
    pub const LC_SUB_LIBRARY: u32 = 0x15;
    /// Two-level namespace lookup hints
    pub const LC_TWOLEVEL_HINTS: u32 = 0x16;
    /// Prebind checksum
    pub const LC_PREBIND_CKSUM: u32 = 0x17;
    /// Load weak dylib
    pub const LC_LOAD_WEAK_DYLIB: u32 = 0x8000_0018;
    /// 64-bit segment of this file to be mapped
    pub const LC_SEGMENT_64: u32 = 0x19;
    /// 64-bit image routines
    pub const LC_ROUTINES_64: u32 = 0x1A;
    /// The uuid
    pub const LC_UUID: u32 = 0x1B;
    /// Runpath additions
    pub const LC_RPATH: u32 = 0x8000_001C;
    /// Local of code signature
    pub const LC_CODE_SIGNATURE: u32 = 0x1D;
    /// Local of info to split segments
    pub const LC_SEGMENT_SPLIT_INFO: u32 = 0x1E;
    /// Load and re-export dylib
    pub const LC_REEXPORT_DYLIB: u32 = 0x8000_001F;
    /// Delay load of dylib until first use
    pub const LC_LAZY_LOAD_DYLIB: u32 = 0x20;
    /// Encrypted segment information
    pub const LC_ENCRYPTION_INFO: u32 = 0x21;
    /// Compressed dyld info
    pub const LC_DYLD_INFO: u32 = 0x22;
    /// Compressed dyld info (only)
    pub const LC_DYLD_INFO_ONLY: u32 = 0x8000_0022;
    /// Load upward dylib
    pub const LC_LOAD_UPWARD_DYLIB: u32 = 0x8000_0023;
    /// Build for MacOSX min OS version
    pub const LC_VERSION_MIN_MACOSX: u32 = 0x24;
    /// Build for iPhoneOS min OS version
    pub const LC_VERSION_MIN_IPHONEOS: u32 = 0x25;
    /// Compressed table of function start addresses
    pub const LC_FUNCTION_STARTS: u32 = 0x26;
    /// String for dyld to treat like environment variable
    pub const LC_DYLD_ENVIRONMENT: u32 = 0x27;
    /// Replacement for LC_UNIXTHREAD
    pub const LC_MAIN: u32 = 0x8000_0028;
    /// Table of non-instructions in __text
    pub const LC_DATA_IN_CODE: u32 = 0x29;
    /// Source version used to build binary
    pub const LC_SOURCE_VERSION: u32 = 0x2A;
    /// Code signing DRs copied from linked dylibs
    pub const LC_DYLIB_CODE_SIGN_DRS: u32 = 0x2B;
    /// 64-bit encrypted segment information
    pub const LC_ENCRYPTION_INFO_64: u32 = 0x2C;
    /// Linker options in MH_OBJECT files
    pub const LC_LINKER_OPTION: u32 = 0x2D;
    /// Optimization hints in MH_OBJECT files
    pub const LC_LINKER_OPTIMIZATION_HINT: u32 = 0x2E;
    /// Build for watchOS min OS version
    pub const LC_VERSION_MIN_WATCHOS: u32 = 0x30;
    /// Build for tvOS min OS version
    pub const LC_VERSION_MIN_TVOS: u32 = 0x2F;
    /// Arbitrary data included within a Mach-O file
    pub const LC_NOTE: u32 = 0x31;
    /// Build for platform min OS version
    pub const LC_BUILD_VERSION: u32 = 0x32;
    /// Dyld exports trie
    pub const LC_DYLD_EXPORTS_TRIE: u32 = 0x8000_0033;
    /// Chained fixups
    pub const LC_DYLD_CHAINED_FIXUPS: u32 = 0x8000_0034;
    /// Fileset entry
    pub const LC_FILESET_ENTRY: u32 = 0x8000_0035;
}

/// Returns the name of a load command
pub fn load_command_name(cmd: u32) -> &'static str {
    match cmd {
        load_command::LC_SEGMENT => "LC_SEGMENT",
        load_command::LC_SYMTAB => "LC_SYMTAB",
        load_command::LC_SYMSEG => "LC_SYMSEG",
        load_command::LC_THREAD => "LC_THREAD",
        load_command::LC_UNIXTHREAD => "LC_UNIXTHREAD",
        load_command::LC_LOADFVMLIB => "LC_LOADFVMLIB",
        load_command::LC_IDFVMLIB => "LC_IDFVMLIB",
        load_command::LC_IDENT => "LC_IDENT",
        load_command::LC_FVMFILE => "LC_FVMFILE",
        load_command::LC_PREPAGE => "LC_PREPAGE",
        load_command::LC_DYSYMTAB => "LC_DYSYMTAB",
        load_command::LC_LOAD_DYLIB => "LC_LOAD_DYLIB",
        load_command::LC_ID_DYLIB => "LC_ID_DYLIB",
        load_command::LC_LOAD_DYLINKER => "LC_LOAD_DYLINKER",
        load_command::LC_ID_DYLINKER => "LC_ID_DYLINKER",
        load_command::LC_PREBOUND_DYLIB => "LC_PREBOUND_DYLIB",
        load_command::LC_ROUTINES => "LC_ROUTINES",
        load_command::LC_SUB_FRAMEWORK => "LC_SUB_FRAMEWORK",
        load_command::LC_SUB_UMBRELLA => "LC_SUB_UMBRELLA",
        load_command::LC_SUB_CLIENT => "LC_SUB_CLIENT",
        load_command::LC_SUB_LIBRARY => "LC_SUB_LIBRARY",
        load_command::LC_TWOLEVEL_HINTS => "LC_TWOLEVEL_HINTS",
        load_command::LC_PREBIND_CKSUM => "LC_PREBIND_CKSUM",
        load_command::LC_LOAD_WEAK_DYLIB => "LC_LOAD_WEAK_DYLIB",
        load_command::LC_SEGMENT_64 => "LC_SEGMENT_64",
        load_command::LC_ROUTINES_64 => "LC_ROUTINES_64",
        load_command::LC_UUID => "LC_UUID",
        load_command::LC_RPATH => "LC_RPATH",
        load_command::LC_CODE_SIGNATURE => "LC_CODE_SIGNATURE",
        load_command::LC_SEGMENT_SPLIT_INFO => "LC_SEGMENT_SPLIT_INFO",
        load_command::LC_REEXPORT_DYLIB => "LC_REEXPORT_DYLIB",
        load_command::LC_LAZY_LOAD_DYLIB => "LC_LAZY_LOAD_DYLIB",
        load_command::LC_ENCRYPTION_INFO => "LC_ENCRYPTION_INFO",
        load_command::LC_DYLD_INFO => "LC_DYLD_INFO",
        load_command::LC_DYLD_INFO_ONLY => "LC_DYLD_INFO_ONLY",
        load_command::LC_LOAD_UPWARD_DYLIB => "LC_LOAD_UPWARD_DYLIB",
        load_command::LC_VERSION_MIN_MACOSX => "LC_VERSION_MIN_MACOSX",
        load_command::LC_VERSION_MIN_IPHONEOS => "LC_VERSION_MIN_IPHONEOS",
        load_command::LC_FUNCTION_STARTS => "LC_FUNCTION_STARTS",
        load_command::LC_DYLD_ENVIRONMENT => "LC_DYLD_ENVIRONMENT",
        load_command::LC_MAIN => "LC_MAIN",
        load_command::LC_DATA_IN_CODE => "LC_DATA_IN_CODE",
        load_command::LC_SOURCE_VERSION => "LC_SOURCE_VERSION",
        load_command::LC_DYLIB_CODE_SIGN_DRS => "LC_DYLIB_CODE_SIGN_DRS",
        load_command::LC_ENCRYPTION_INFO_64 => "LC_ENCRYPTION_INFO_64",
        load_command::LC_LINKER_OPTION => "LC_LINKER_OPTION",
        load_command::LC_LINKER_OPTIMIZATION_HINT => "LC_LINKER_OPTIMIZATION_HINT",
        load_command::LC_VERSION_MIN_WATCHOS => "LC_VERSION_MIN_WATCHOS",
        load_command::LC_VERSION_MIN_TVOS => "LC_VERSION_MIN_TVOS",
        load_command::LC_NOTE => "LC_NOTE",
        load_command::LC_BUILD_VERSION => "LC_BUILD_VERSION",
        load_command::LC_DYLD_EXPORTS_TRIE => "LC_DYLD_EXPORTS_TRIE",
        load_command::LC_DYLD_CHAINED_FIXUPS => "LC_DYLD_CHAINED_FIXUPS",
        load_command::LC_FILESET_ENTRY => "LC_FILESET_ENTRY",
        _ => "LC_UNKNOWN",
    }
}

// =============================================================================
// Platform Types
// =============================================================================

/// Platform type constants for LC_BUILD_VERSION
pub mod platform {
    /// macOS
    pub const PLATFORM_MACOS: u32 = 1;
    /// iOS
    pub const PLATFORM_IOS: u32 = 2;
    /// tvOS
    pub const PLATFORM_TVOS: u32 = 3;
    /// watchOS
    pub const PLATFORM_WATCHOS: u32 = 4;
    /// bridgeOS
    pub const PLATFORM_BRIDGEOS: u32 = 5;
    /// Mac Catalyst
    pub const PLATFORM_MACCATALYST: u32 = 6;
    /// iOS Simulator
    pub const PLATFORM_IOSSIMULATOR: u32 = 7;
    /// tvOS Simulator
    pub const PLATFORM_TVOSSIMULATOR: u32 = 8;
    /// watchOS Simulator
    pub const PLATFORM_WATCHOSSIMULATOR: u32 = 9;
    /// DriverKit
    pub const PLATFORM_DRIVERKIT: u32 = 10;
    /// visionOS
    pub const PLATFORM_VISIONOS: u32 = 11;
    /// visionOS Simulator
    pub const PLATFORM_VISIONOSSIMULATOR: u32 = 12;
}

/// Returns human-readable platform name
pub fn platform_name(platform: u32) -> &'static str {
    match platform {
        platform::PLATFORM_MACOS => "macOS",
        platform::PLATFORM_IOS => "iOS",
        platform::PLATFORM_TVOS => "tvOS",
        platform::PLATFORM_WATCHOS => "watchOS",
        platform::PLATFORM_BRIDGEOS => "bridgeOS",
        platform::PLATFORM_MACCATALYST => "Mac Catalyst",
        platform::PLATFORM_IOSSIMULATOR => "iOS Simulator",
        platform::PLATFORM_TVOSSIMULATOR => "tvOS Simulator",
        platform::PLATFORM_WATCHOSSIMULATOR => "watchOS Simulator",
        platform::PLATFORM_DRIVERKIT => "DriverKit",
        platform::PLATFORM_VISIONOS => "visionOS",
        platform::PLATFORM_VISIONOSSIMULATOR => "visionOS Simulator",
        _ => "Unknown",
    }
}

// =============================================================================
// Build Tool Types
// =============================================================================

/// Build tool type constants
pub mod build_tool {
    /// Clang
    pub const TOOL_CLANG: u32 = 1;
    /// Swift
    pub const TOOL_SWIFT: u32 = 2;
    /// ld (linker)
    pub const TOOL_LD: u32 = 3;
    /// lld (LLVM linker)
    pub const TOOL_LLD: u32 = 4;
}

/// Returns human-readable tool name
pub fn build_tool_name(tool: u32) -> &'static str {
    match tool {
        build_tool::TOOL_CLANG => "Clang",
        build_tool::TOOL_SWIFT => "Swift",
        build_tool::TOOL_LD => "ld",
        build_tool::TOOL_LLD => "lld",
        _ => "Unknown",
    }
}

// =============================================================================
// Header Structures
// =============================================================================

/// Mach-O header (shared fields for 32-bit and 64-bit)
#[derive(Debug, Clone)]
pub struct MachHeader {
    /// Magic number (identifies 32/64-bit and byte order)
    pub magic: u32,
    /// CPU type (x86_64, ARM64, etc.)
    pub cputype: i32,
    /// CPU subtype (specific variant)
    pub cpusubtype: i32,
    /// File type (executable, dylib, bundle, etc.)
    pub filetype: u32,
    /// Number of load commands following header
    pub ncmds: u32,
    /// Total size of all load commands in bytes
    pub sizeofcmds: u32,
    /// Header flags
    pub flags: u32,
    /// Reserved field (64-bit only, 0 for 32-bit)
    pub reserved: u32,
    /// True if this is a 64-bit Mach-O
    pub is_64bit: bool,
    /// True if byte order is swapped (non-native)
    pub is_swapped: bool,
}

impl MachHeader {
    /// Returns the size of the header in bytes (28 for 32-bit, 32 for 64-bit)
    pub fn header_size(&self) -> usize {
        if self.is_64bit { 32 } else { 28 }
    }

    /// Returns human-readable CPU type name
    pub fn cpu_type_name(&self) -> &'static str {
        match self.cputype {
            cpu_type::CPU_TYPE_I386 => "i386",
            cpu_type::CPU_TYPE_X86_64 => "x86_64",
            cpu_type::CPU_TYPE_ARM => "ARM",
            cpu_type::CPU_TYPE_ARM64 => "ARM64",
            cpu_type::CPU_TYPE_ARM64_32 => "ARM64_32",
            cpu_type::CPU_TYPE_POWERPC => "PowerPC",
            cpu_type::CPU_TYPE_POWERPC64 => "PowerPC64",
            _ => "Unknown",
        }
    }

    /// Returns human-readable CPU subtype name
    pub fn cpu_subtype_name(&self) -> String {
        match self.cputype {
            cpu_type::CPU_TYPE_ARM64 => match self.cpusubtype & 0xFF {
                cpu_subtype_arm64::CPU_SUBTYPE_ARM64_ALL => "ALL".to_string(),
                cpu_subtype_arm64::CPU_SUBTYPE_ARM64E => "ARM64E".to_string(),
                cpu_subtype_arm64::CPU_SUBTYPE_ARM64_V8 => "V8".to_string(),
                _ => format!("Unknown ({})", self.cpusubtype),
            },
            cpu_type::CPU_TYPE_X86_64 => match self.cpusubtype & 0xFF {
                cpu_subtype_x86_64::CPU_SUBTYPE_X86_64_ALL => "ALL".to_string(),
                cpu_subtype_x86_64::CPU_SUBTYPE_X86_64_H => "Haswell".to_string(),
                _ => format!("Unknown ({})", self.cpusubtype),
            },
            _ => format!("{}", self.cpusubtype),
        }
    }

    /// Returns human-readable file type name
    pub fn file_type_name(&self) -> &'static str {
        file_type_name(self.filetype)
    }

    /// Returns list of flag names
    pub fn flag_names(&self) -> Vec<&'static str> {
        decode_flags(self.flags)
    }
}

// =============================================================================
// Load Command Structures
// =============================================================================

/// Generic load command header (appears at the start of every load command)
#[derive(Debug, Clone, Copy)]
pub struct LoadCommandHeader {
    /// Load command type (LC_SEGMENT, LC_UUID, etc.)
    pub cmd: u32,
    /// Total size of this load command including header
    pub cmdsize: u32,
}

/// Segment command (LC_SEGMENT or LC_SEGMENT_64)
#[derive(Debug, Clone)]
pub struct SegmentCommand {
    /// Segment name (e.g., "__TEXT", "__DATA")
    pub segname: String,
    /// Virtual memory address of this segment
    pub vmaddr: u64,
    /// Virtual memory size of this segment
    pub vmsize: u64,
    /// File offset of this segment
    pub fileoff: u64,
    /// File size of this segment
    pub filesize: u64,
    /// Maximum VM protection
    pub maxprot: i32,
    /// Initial VM protection
    pub initprot: i32,
    /// Number of sections in this segment
    pub nsects: u32,
    /// Flags
    pub flags: u32,
    /// Sections within this segment
    pub sections: Vec<Section>,
}

/// Section within a segment
#[derive(Debug, Clone)]
pub struct Section {
    /// Section name (e.g., "__text", "__data")
    pub sectname: String,
    /// Segment name
    pub segname: String,
    /// Virtual memory address
    pub addr: u64,
    /// Section size
    pub size: u64,
    /// File offset
    pub offset: u32,
    /// Alignment as power of 2
    pub align: u32,
    /// File offset of relocations
    pub reloff: u32,
    /// Number of relocations
    pub nreloc: u32,
    /// Section type and attributes
    pub flags: u32,
    /// Reserved
    pub reserved1: u32,
    /// Reserved
    pub reserved2: u32,
    /// Reserved (64-bit only)
    pub reserved3: u32,
}

/// Dylib command (LC_LOAD_DYLIB, LC_ID_DYLIB, etc.)
#[derive(Debug, Clone)]
pub struct DylibCommand {
    /// Load command type
    pub cmd: u32,
    /// Library path/name
    pub name: String,
    /// Library timestamp
    pub timestamp: u32,
    /// Current version (X.Y.Z encoded as ((X << 16) | (Y << 8) | Z))
    pub current_version: u32,
    /// Compatibility version
    pub compatibility_version: u32,
}

impl DylibCommand {
    /// Returns the current version as a formatted string (e.g., "1.2.3")
    pub fn current_version_string(&self) -> String {
        format_version(self.current_version)
    }

    /// Returns the compatibility version as a formatted string
    pub fn compatibility_version_string(&self) -> String {
        format_version(self.compatibility_version)
    }
}

/// Format a Mach-O version number (X.Y.Z packed in u32)
pub fn format_version(version: u32) -> String {
    let major = (version >> 16) & 0xFFFF;
    let minor = (version >> 8) & 0xFF;
    let patch = version & 0xFF;
    format!("{}.{}.{}", major, minor, patch)
}

/// UUID command (LC_UUID)
#[derive(Debug, Clone)]
pub struct UuidCommand {
    /// 128-bit UUID
    pub uuid: [u8; 16],
}

impl UuidCommand {
    /// Returns the UUID as a formatted string (e.g., "550E8400-E29B-41D4-A716-446655440000")
    pub fn uuid_string(&self) -> String {
        format!(
            "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            self.uuid[0],
            self.uuid[1],
            self.uuid[2],
            self.uuid[3],
            self.uuid[4],
            self.uuid[5],
            self.uuid[6],
            self.uuid[7],
            self.uuid[8],
            self.uuid[9],
            self.uuid[10],
            self.uuid[11],
            self.uuid[12],
            self.uuid[13],
            self.uuid[14],
            self.uuid[15]
        )
    }
}

/// Version min command (LC_VERSION_MIN_*)
#[derive(Debug, Clone)]
pub struct VersionMinCommand {
    /// Load command type (identifies platform)
    pub cmd: u32,
    /// Minimum OS version
    pub version: u32,
    /// SDK version
    pub sdk: u32,
}

impl VersionMinCommand {
    /// Returns the minimum version as a formatted string
    pub fn version_string(&self) -> String {
        format_version(self.version)
    }

    /// Returns the SDK version as a formatted string
    pub fn sdk_string(&self) -> String {
        format_version(self.sdk)
    }

    /// Returns the platform name based on load command type
    pub fn platform_name(&self) -> &'static str {
        match self.cmd {
            load_command::LC_VERSION_MIN_MACOSX => "macOS",
            load_command::LC_VERSION_MIN_IPHONEOS => "iOS",
            load_command::LC_VERSION_MIN_WATCHOS => "watchOS",
            load_command::LC_VERSION_MIN_TVOS => "tvOS",
            _ => "Unknown",
        }
    }
}

/// Build tool version info
#[derive(Debug, Clone)]
pub struct BuildToolVersion {
    /// Tool type (TOOL_CLANG, TOOL_SWIFT, etc.)
    pub tool: u32,
    /// Tool version
    pub version: u32,
}

impl BuildToolVersion {
    /// Returns the tool name
    pub fn tool_name(&self) -> &'static str {
        build_tool_name(self.tool)
    }

    /// Returns the version as a formatted string
    pub fn version_string(&self) -> String {
        format_version(self.version)
    }
}

/// Build version command (LC_BUILD_VERSION)
#[derive(Debug, Clone)]
pub struct BuildVersionCommand {
    /// Platform type
    pub platform: u32,
    /// Minimum OS version
    pub minos: u32,
    /// SDK version
    pub sdk: u32,
    /// Number of tool entries
    pub ntools: u32,
    /// Build tool versions
    pub tools: Vec<BuildToolVersion>,
}

impl BuildVersionCommand {
    /// Returns the platform name
    pub fn platform_name(&self) -> &'static str {
        platform_name(self.platform)
    }

    /// Returns the minimum OS version as a formatted string
    pub fn minos_string(&self) -> String {
        format_version(self.minos)
    }

    /// Returns the SDK version as a formatted string
    pub fn sdk_string(&self) -> String {
        format_version(self.sdk)
    }
}

/// Source version command (LC_SOURCE_VERSION)
#[derive(Debug, Clone)]
pub struct SourceVersionCommand {
    /// Source version (A.B.C.D.E packed into u64)
    pub version: u64,
}

impl SourceVersionCommand {
    /// Returns the source version as a formatted string
    pub fn version_string(&self) -> String {
        let a = (self.version >> 40) & 0xFFFFFF;
        let b = (self.version >> 30) & 0x3FF;
        let c = (self.version >> 20) & 0x3FF;
        let d = (self.version >> 10) & 0x3FF;
        let e = self.version & 0x3FF;
        format!("{}.{}.{}.{}.{}", a, b, c, d, e)
    }
}

/// Main entry point command (LC_MAIN)
#[derive(Debug, Clone)]
pub struct EntryPointCommand {
    /// File (__TEXT) offset of main()
    pub entryoff: u64,
    /// Initial stack size (if custom stack)
    pub stacksize: u64,
}

/// Symbol table command (LC_SYMTAB)
#[derive(Debug, Clone)]
pub struct SymtabCommand {
    /// File offset of symbol table
    pub symoff: u32,
    /// Number of symbol table entries
    pub nsyms: u32,
    /// File offset of string table
    pub stroff: u32,
    /// Size of string table in bytes
    pub strsize: u32,
}

/// Dynamic symbol table command (LC_DYSYMTAB)
#[derive(Debug, Clone)]
pub struct DysymtabCommand {
    /// Index to local symbols
    pub ilocalsym: u32,
    /// Number of local symbols
    pub nlocalsym: u32,
    /// Index to external symbols
    pub iextdefsym: u32,
    /// Number of external symbols
    pub nextdefsym: u32,
    /// Index to undefined symbols
    pub iundefsym: u32,
    /// Number of undefined symbols
    pub nundefsym: u32,
    /// File offset to table of contents
    pub tocoff: u32,
    /// Number of entries in table of contents
    pub ntoc: u32,
    /// File offset to module table
    pub modtaboff: u32,
    /// Number of module table entries
    pub nmodtab: u32,
    /// Offset to referenced symbol table
    pub extrefsymoff: u32,
    /// Number of referenced symbol table entries
    pub nextrefsyms: u32,
    /// File offset to indirect symbol table
    pub indirectsymoff: u32,
    /// Number of indirect symbol table entries
    pub nindirectsyms: u32,
    /// Offset to external relocation entries
    pub extreloff: u32,
    /// Number of external relocation entries
    pub nextrel: u32,
    /// Offset to local relocation entries
    pub locreloff: u32,
    /// Number of local relocation entries
    pub nlocrel: u32,
}

/// Linkedit data command (LC_CODE_SIGNATURE, LC_FUNCTION_STARTS, etc.)
#[derive(Debug, Clone)]
pub struct LinkeditDataCommand {
    /// Load command type
    pub cmd: u32,
    /// File offset of data in __LINKEDIT segment
    pub dataoff: u32,
    /// Size of data in bytes
    pub datasize: u32,
}

/// Rpath command (LC_RPATH)
#[derive(Debug, Clone)]
pub struct RpathCommand {
    /// Runtime search path
    pub path: String,
}

/// Encryption info command (LC_ENCRYPTION_INFO or LC_ENCRYPTION_INFO_64)
#[derive(Debug, Clone)]
pub struct EncryptionInfoCommand {
    /// File offset of encrypted area
    pub cryptoff: u32,
    /// Size of encrypted area
    pub cryptsize: u32,
    /// Encryption system (0 = not encrypted)
    pub cryptid: u32,
}

// =============================================================================
// Code Signature Structures
// =============================================================================

/// Code signature blob types
pub mod cs_blob_type {
    /// Code directory
    pub const CSSLOT_CODEDIRECTORY: u32 = 0;
    /// CMS signature
    pub const CSSLOT_SIGNATURESLOT: u32 = 0x10000;
    /// Requirements
    pub const CSSLOT_REQUIREMENTS: u32 = 2;
    /// Entitlements
    pub const CSSLOT_ENTITLEMENTS: u32 = 5;
    /// DER entitlements
    pub const CSSLOT_DER_ENTITLEMENTS: u32 = 7;
    /// Launch constraints
    pub const CSSLOT_LAUNCH_CONSTRAINT_SELF: u32 = 8;
    /// Alternate code directories
    pub const CSSLOT_ALTERNATE_CODEDIRECTORIES: u32 = 0x1000;
}

/// Code signature magic values
pub mod cs_magic {
    /// SuperBlob containing all code signature data
    pub const CSMAGIC_EMBEDDED_SIGNATURE: u32 = 0xFADE0CC0;
    /// Code directory
    pub const CSMAGIC_CODEDIRECTORY: u32 = 0xFADE0C02;
    /// Requirements blob
    pub const CSMAGIC_REQUIREMENTS: u32 = 0xFADE0C01;
    /// Entitlements blob
    pub const CSMAGIC_ENTITLEMENTS: u32 = 0xFADE7171;
    /// CMS signature blob
    pub const CSMAGIC_BLOBWRAPPER: u32 = 0xFADE0B01;
    /// Detached signature
    pub const CSMAGIC_DETACHED_SIGNATURE: u32 = 0xFADE0CC1;
}

/// Code signature SuperBlob (container for all signature data)
#[derive(Debug, Clone)]
pub struct SuperBlob {
    /// Magic number (CSMAGIC_EMBEDDED_SIGNATURE)
    pub magic: u32,
    /// Total length of blob
    pub length: u32,
    /// Number of sub-blobs
    pub count: u32,
    /// Index entries
    pub index: Vec<BlobIndex>,
}

/// Index entry in SuperBlob
#[derive(Debug, Clone)]
pub struct BlobIndex {
    /// Blob type
    pub blob_type: u32,
    /// Offset from SuperBlob start
    pub offset: u32,
}

/// Code Directory (contains hashes and identity info)
#[derive(Debug, Clone)]
pub struct CodeDirectory {
    /// Magic number
    pub magic: u32,
    /// Length of code directory
    pub length: u32,
    /// Version number
    pub version: u32,
    /// Flags
    pub flags: u32,
    /// Offset of hash slot element at index zero
    pub hash_offset: u32,
    /// Offset of identifier string
    pub ident_offset: u32,
    /// Number of special slots
    pub n_special_slots: u32,
    /// Number of code slots
    pub n_code_slots: u32,
    /// Limit to main image signature range
    pub code_limit: u32,
    /// Size of each hash in bytes
    pub hash_size: u8,
    /// Type of hash (CS_HASHTYPE_*)
    pub hash_type: u8,
    /// Platform identifier
    pub platform: u8,
    /// log2(page size)
    pub page_size: u8,
    /// Signing team identifier
    pub team_offset: u32,
    /// Identifier string
    pub identifier: String,
    /// Team identifier
    pub team_id: Option<String>,
}

/// Hash type constants
pub mod hash_type {
    /// No hash
    pub const CS_HASHTYPE_SHA1: u8 = 1;
    /// SHA-256
    pub const CS_HASHTYPE_SHA256: u8 = 2;
    /// SHA-256 truncated to 20 bytes
    pub const CS_HASHTYPE_SHA256_TRUNCATED: u8 = 3;
    /// SHA-384
    pub const CS_HASHTYPE_SHA384: u8 = 4;
    /// SHA-512
    pub const CS_HASHTYPE_SHA512: u8 = 5;
}

/// Returns hash type name
pub fn hash_type_name(hash_type: u8) -> &'static str {
    match hash_type {
        hash_type::CS_HASHTYPE_SHA1 => "SHA-1",
        hash_type::CS_HASHTYPE_SHA256 => "SHA-256",
        hash_type::CS_HASHTYPE_SHA256_TRUNCATED => "SHA-256 (truncated)",
        hash_type::CS_HASHTYPE_SHA384 => "SHA-384",
        hash_type::CS_HASHTYPE_SHA512 => "SHA-512",
        _ => "Unknown",
    }
}

/// Parsed code signature information
#[derive(Debug, Clone, Default)]
pub struct CodeSignatureInfo {
    /// Size of the code signature blob
    pub signature_size: u32,
    /// Whether the binary is signed
    pub is_signed: bool,
    /// Code directory identifier (bundle ID)
    pub identifier: Option<String>,
    /// Team identifier (Apple Developer Team ID)
    pub team_id: Option<String>,
    /// Hash type used
    pub hash_type: Option<String>,
    /// Code directory version
    pub cd_version: u32,
    /// Number of code slots (hashed pages)
    pub n_code_slots: u32,
    /// Whether CMS signature is present
    pub has_cms_signature: bool,
    /// Signer common name (from CMS signature)
    pub signer_name: Option<String>,
    /// Whether entitlements are present
    pub has_entitlements: bool,
    /// List of key entitlement keys extracted from plist (for security analysis)
    pub entitlement_keys: Vec<String>,
}

// =============================================================================
// FAT Binary Structures
// =============================================================================

/// FAT binary header
#[derive(Debug, Clone)]
pub struct FatHeader {
    /// Magic number (FAT_MAGIC or FAT_MAGIC_64)
    pub magic: u32,
    /// Number of architectures
    pub nfat_arch: u32,
    /// Whether this is a 64-bit FAT header
    pub is_64bit: bool,
    /// Whether byte order is swapped
    pub is_swapped: bool,
}

/// FAT architecture entry (32-bit)
#[derive(Debug, Clone)]
pub struct FatArch {
    /// CPU type
    pub cputype: i32,
    /// CPU subtype
    pub cpusubtype: i32,
    /// Offset in file to the architecture
    pub offset: u64,
    /// Size of the architecture
    pub size: u64,
    /// Alignment as power of 2
    pub align: u32,
}

impl FatArch {
    /// Returns human-readable CPU type name
    pub fn cpu_type_name(&self) -> &'static str {
        match self.cputype {
            cpu_type::CPU_TYPE_I386 => "i386",
            cpu_type::CPU_TYPE_X86_64 => "x86_64",
            cpu_type::CPU_TYPE_ARM => "ARM",
            cpu_type::CPU_TYPE_ARM64 => "ARM64",
            cpu_type::CPU_TYPE_ARM64_32 => "ARM64_32",
            cpu_type::CPU_TYPE_POWERPC => "PowerPC",
            cpu_type::CPU_TYPE_POWERPC64 => "PowerPC64",
            _ => "Unknown",
        }
    }
}

// =============================================================================
// Aggregate Parsing Results
// =============================================================================

/// Complete parsed Mach-O file information
#[derive(Debug, Clone, Default)]
pub struct MachOInfo {
    /// Mach-O header
    pub header: Option<MachHeader>,
    /// Segments
    pub segments: Vec<SegmentCommand>,
    /// Loaded dynamic libraries
    pub dylibs: Vec<DylibCommand>,
    /// UUID
    pub uuid: Option<UuidCommand>,
    /// Version min commands
    pub version_min: Option<VersionMinCommand>,
    /// Build version command
    pub build_version: Option<BuildVersionCommand>,
    /// Source version
    pub source_version: Option<SourceVersionCommand>,
    /// Entry point
    pub entry_point: Option<EntryPointCommand>,
    /// Symbol table info
    pub symtab: Option<SymtabCommand>,
    /// Dynamic symbol table info
    pub dysymtab: Option<DysymtabCommand>,
    /// Code signature info
    pub code_signature: Option<LinkeditDataCommand>,
    /// RPaths
    pub rpaths: Vec<RpathCommand>,
    /// Encryption info
    pub encryption_info: Option<EncryptionInfoCommand>,
    /// Dyld info
    pub dyld_info: Option<LinkeditDataCommand>,
    /// Function starts
    pub function_starts: Option<LinkeditDataCommand>,
    /// Data in code
    pub data_in_code: Option<LinkeditDataCommand>,
    /// Parsed code signature details
    pub code_signature_info: Option<CodeSignatureInfo>,
    /// FAT header (if universal binary)
    pub fat_header: Option<FatHeader>,
    /// FAT architectures (if universal binary)
    pub fat_archs: Vec<FatArch>,
    /// Whether this file is from a FAT/universal binary
    pub is_from_fat: bool,
    /// Index of this architecture in FAT binary (0 if not FAT)
    pub fat_arch_index: usize,
}

impl MachOInfo {
    /// Creates a new empty MachOInfo
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the total number of sections across all segments
    pub fn total_sections(&self) -> usize {
        self.segments.iter().map(|s| s.sections.len()).sum()
    }

    /// Returns the size of the __TEXT segment
    pub fn text_segment_size(&self) -> Option<u64> {
        self.segments
            .iter()
            .find(|s| s.segname == "__TEXT")
            .map(|s| s.vmsize)
    }

    /// Returns the size of the __DATA segment
    pub fn data_segment_size(&self) -> Option<u64> {
        self.segments
            .iter()
            .find(|s| s.segname == "__DATA" || s.segname == "__DATA_CONST")
            .map(|s| s.vmsize)
    }

    /// Returns the count of weak dylibs
    pub fn weak_dylib_count(&self) -> usize {
        self.dylibs
            .iter()
            .filter(|d| d.cmd == load_command::LC_LOAD_WEAK_DYLIB)
            .count()
    }

    /// Returns the count of reexported dylibs
    pub fn reexport_dylib_count(&self) -> usize {
        self.dylibs
            .iter()
            .filter(|d| d.cmd == load_command::LC_REEXPORT_DYLIB)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_version() {
        assert_eq!(format_version(0x0001_0203), "1.2.3");
        assert_eq!(format_version(0x000A_0B0C), "10.11.12");
        assert_eq!(format_version(0), "0.0.0");
    }

    #[test]
    fn test_uuid_string() {
        let uuid_cmd = UuidCommand {
            uuid: [
                0x55, 0x0E, 0x84, 0x00, 0xE2, 0x9B, 0x41, 0xD4, 0xA7, 0x16, 0x44, 0x66, 0x55, 0x44,
                0x00, 0x00,
            ],
        };
        assert_eq!(
            uuid_cmd.uuid_string(),
            "550E8400-E29B-41D4-A716-446655440000"
        );
    }

    #[test]
    fn test_source_version_string() {
        let cmd = SourceVersionCommand {
            version: (1 << 40) | (2 << 30) | (3 << 20) | (4 << 10) | 5,
        };
        assert_eq!(cmd.version_string(), "1.2.3.4.5");
    }

    #[test]
    fn test_decode_flags() {
        let flags = flags::MH_PIE | flags::MH_TWOLEVEL | flags::MH_DYLDLINK;
        let names = decode_flags(flags);
        assert!(names.contains(&"PIE"));
        assert!(names.contains(&"TWOLEVEL"));
        assert!(names.contains(&"DYLDLINK"));
    }

    #[test]
    fn test_file_type_name() {
        assert_eq!(file_type_name(file_type::MH_EXECUTE), "Executable");
        assert_eq!(file_type_name(file_type::MH_DYLIB), "Dynamic Library");
        assert_eq!(file_type_name(0xFF), "Unknown");
    }

    #[test]
    fn test_platform_name() {
        assert_eq!(platform_name(platform::PLATFORM_MACOS), "macOS");
        assert_eq!(platform_name(platform::PLATFORM_IOS), "iOS");
        assert_eq!(platform_name(0xFF), "Unknown");
    }

    #[test]
    fn test_cpu_type_name() {
        let header = MachHeader {
            magic: magic::MH_MAGIC_64,
            cputype: cpu_type::CPU_TYPE_ARM64,
            cpusubtype: 0,
            filetype: file_type::MH_EXECUTE,
            ncmds: 0,
            sizeofcmds: 0,
            flags: 0,
            reserved: 0,
            is_64bit: true,
            is_swapped: false,
        };
        assert_eq!(header.cpu_type_name(), "ARM64");
    }
}
