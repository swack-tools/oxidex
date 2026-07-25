//! PE file structure definitions
//!
//! This module defines Rust structures matching PE file headers
//! as per Microsoft PE/COFF specification.

#![allow(dead_code)]

/// DOS Header (IMAGE_DOS_HEADER) - 64 bytes
#[derive(Debug, Clone, Copy)]
pub struct DosHeader {
    /// Magic number "MZ" (0x5A4D)
    pub e_magic: u16,
    /// Bytes on last page of file
    pub e_cblp: u16,
    /// Pages in file
    pub e_cp: u16,
    /// Relocations
    pub e_crlc: u16,
    /// Size of header in paragraphs
    pub e_cparhdr: u16,
    /// Minimum extra paragraphs needed
    pub e_minalloc: u16,
    /// Maximum extra paragraphs needed
    pub e_maxalloc: u16,
    /// Initial (relative) SS value
    pub e_ss: u16,
    /// Initial SP value
    pub e_sp: u16,
    /// Checksum
    pub e_csum: u16,
    /// Initial IP value
    pub e_ip: u16,
    /// Initial (relative) CS value
    pub e_cs: u16,
    /// File address of relocation table
    pub e_lfarlc: u16,
    /// Overlay number
    pub e_ovno: u16,
    /// Reserved words
    pub e_res: [u16; 4],
    /// OEM identifier
    pub e_oemid: u16,
    /// OEM information
    pub e_oeminfo: u16,
    /// Reserved words
    pub e_res2: [u16; 10],
    /// File address of new exe header
    pub e_lfanew: u32,
}

/// COFF File Header (IMAGE_FILE_HEADER) - 20 bytes
#[derive(Debug, Clone, Copy)]
pub struct CoffHeader {
    /// Target machine type
    pub machine: u16,
    /// Number of sections
    pub number_of_sections: u16,
    /// Time/date stamp
    pub time_date_stamp: u32,
    /// File offset of symbol table
    pub pointer_to_symbol_table: u32,
    /// Number of symbols
    pub number_of_symbols: u32,
    /// Size of optional header
    pub size_of_optional_header: u16,
    /// File characteristics flags
    pub characteristics: u16,
}

/// Optional Header Standard Fields
#[derive(Debug, Clone, Copy)]
pub struct OptionalHeaderStandard {
    /// Image format identifier (0x10B=PE32, 0x20B=PE32+)
    pub magic: u16,
    /// Linker major version
    pub major_linker_version: u8,
    /// Linker minor version
    pub minor_linker_version: u8,
    /// Size of code section
    pub size_of_code: u32,
    /// Size of initialized data
    pub size_of_initialized_data: u32,
    /// Size of uninitialized data
    pub size_of_uninitialized_data: u32,
    /// Entry point RVA
    pub address_of_entry_point: u32,
    /// Code section base RVA
    pub base_of_code: u32,
}

/// Optional Header NT-Specific Fields (PE32)
#[derive(Debug, Clone)]
pub struct OptionalHeaderNT {
    /// Preferred load address
    pub image_base: u64,
    /// Memory alignment
    pub section_alignment: u32,
    /// File alignment
    pub file_alignment: u32,
    /// OS major version
    pub major_operating_system_version: u16,
    /// OS minor version
    pub minor_operating_system_version: u16,
    /// Image major version
    pub major_image_version: u16,
    /// Image minor version
    pub minor_image_version: u16,
    /// Subsystem major version
    pub major_subsystem_version: u16,
    /// Subsystem minor version
    pub minor_subsystem_version: u16,
    /// Reserved
    pub win32_version_value: u32,
    /// Image size in memory
    pub size_of_image: u32,
    /// Header size
    pub size_of_headers: u32,
    /// Checksum
    pub checksum: u32,
    /// Subsystem type
    pub subsystem: u16,
    /// DLL characteristics
    pub dll_characteristics: u16,
    /// Stack reserve size
    pub size_of_stack_reserve: u64,
    /// Stack commit size
    pub size_of_stack_commit: u64,
    /// Heap reserve size
    pub size_of_heap_reserve: u64,
    /// Heap commit size
    pub size_of_heap_commit: u64,
    /// Loader flags (obsolete)
    pub loader_flags: u32,
    /// Data directory count
    pub number_of_rva_and_sizes: u32,
    /// Data directories (RVA, Size pairs)
    pub data_directories: Vec<(u32, u32)>,
}

/// Machine type constants
#[allow(dead_code)]
pub mod machine_types {
    /// Unknown machine type
    pub const IMAGE_FILE_MACHINE_UNKNOWN: u16 = 0x0;
    /// Intel 386+
    pub const IMAGE_FILE_MACHINE_I386: u16 = 0x014C;
    /// MIPS R3000
    pub const IMAGE_FILE_MACHINE_R3000: u16 = 0x0162;
    /// MIPS R4000
    pub const IMAGE_FILE_MACHINE_R4000: u16 = 0x0166;
    /// MIPS R10000
    pub const IMAGE_FILE_MACHINE_R10000: u16 = 0x0168;
    /// Alpha AXP
    pub const IMAGE_FILE_MACHINE_ALPHA: u16 = 0x0184;
    /// PowerPC
    pub const IMAGE_FILE_MACHINE_POWERPC: u16 = 0x01F0;
    /// Intel Itanium
    pub const IMAGE_FILE_MACHINE_IA64: u16 = 0x0200;
    /// AMD64 (x64)
    pub const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
    /// ARM
    pub const IMAGE_FILE_MACHINE_ARM: u16 = 0x01C0;
    /// ARM64
    pub const IMAGE_FILE_MACHINE_ARM64: u16 = 0xAA64;
}

/// Subsystem type constants
#[allow(dead_code)]
pub mod subsystem_types {
    /// Unknown subsystem
    pub const IMAGE_SUBSYSTEM_UNKNOWN: u16 = 0;
    /// Native (driver)
    pub const IMAGE_SUBSYSTEM_NATIVE: u16 = 1;
    /// Windows GUI
    pub const IMAGE_SUBSYSTEM_WINDOWS_GUI: u16 = 2;
    /// Windows Console
    pub const IMAGE_SUBSYSTEM_WINDOWS_CUI: u16 = 3;
    /// OS/2 Console
    pub const IMAGE_SUBSYSTEM_OS2_CUI: u16 = 5;
    /// POSIX Console
    pub const IMAGE_SUBSYSTEM_POSIX_CUI: u16 = 7;
    /// EFI Application
    pub const IMAGE_SUBSYSTEM_EFI_APPLICATION: u16 = 10;
    /// EFI Boot Service
    pub const IMAGE_SUBSYSTEM_EFI_BOOT_SERVICE_DRIVER: u16 = 11;
    /// EFI Runtime Driver
    pub const IMAGE_SUBSYSTEM_EFI_RUNTIME_DRIVER: u16 = 12;
    /// Xbox
    pub const IMAGE_SUBSYSTEM_XBOX: u16 = 14;
}

/// PE Section Header (40 bytes)
#[derive(Debug, Clone)]
pub struct SectionHeader {
    /// 8-byte section name (null-padded ASCII string)
    pub name: [u8; 8],
    /// Virtual size of the section when loaded
    pub virtual_size: u32,
    /// Virtual address (RVA) where section should be loaded
    pub virtual_address: u32,
    /// Size of initialized data on disk
    pub size_of_raw_data: u32,
    /// File offset to section's raw data
    pub pointer_to_raw_data: u32,
    /// File offset to relocation entries
    pub pointer_to_relocations: u32,
    /// File offset to line number entries
    pub pointer_to_line_numbers: u32,
    /// Number of relocation entries
    pub number_of_relocations: u16,
    /// Number of line number entries
    pub number_of_line_numbers: u16,
    /// Section characteristics flags
    pub characteristics: u32,
}

impl SectionHeader {
    /// Returns the section name as a UTF-8 string, trimming null bytes
    pub fn name_str(&self) -> String {
        String::from_utf8_lossy(&self.name)
            .trim_end_matches('\0')
            .to_string()
    }
}

/// Resource Directory (16 bytes)
#[derive(Debug, Clone)]
pub struct ResourceDirectory {
    /// Resource directory characteristics (usually 0)
    pub characteristics: u32,
    /// Time/date stamp of resource creation
    pub time_date_stamp: u32,
    /// Major version number
    pub major_version: u16,
    /// Minor version number
    pub minor_version: u16,
    /// Number of named resource entries
    pub number_of_name_entries: u16,
    /// Number of ID-based resource entries
    pub number_of_id_entries: u16,
}

/// Resource Directory Entry (8 bytes)
#[derive(Debug, Clone)]
pub struct ResourceDirectoryEntry {
    /// Resource name or ID (high bit indicates if name or ID)
    pub name_id: u32,
    /// Offset to resource data or subdirectory (high bit indicates which)
    pub data_offset: u32,
}

/// Resource Data Entry (16 bytes)
#[derive(Debug, Clone)]
pub struct ResourceDataEntry {
    /// RVA of resource data
    pub data_rva: u32,
    /// Size of resource data in bytes
    pub size: u32,
    /// Code page for resource data
    pub codepage: u32,
    /// Reserved field (must be 0)
    pub reserved: u32,
}

/// Resource type constants for Windows PE files
pub mod resource_types {
    /// Cursor resource
    pub const RT_CURSOR: u32 = 1;
    /// Bitmap resource
    pub const RT_BITMAP: u32 = 2;
    /// Icon resource
    pub const RT_ICON: u32 = 3;
    /// Menu resource
    pub const RT_MENU: u32 = 4;
    /// Dialog box resource
    pub const RT_DIALOG: u32 = 5;
    /// String table resource
    pub const RT_STRING: u32 = 6;
    /// Font directory resource
    pub const RT_FONTDIR: u32 = 7;
    /// Font resource
    pub const RT_FONT: u32 = 8;
    /// Keyboard accelerator resource
    pub const RT_ACCELERATOR: u32 = 9;
    /// Raw data resource
    pub const RT_RCDATA: u32 = 10;
    /// Message table resource
    pub const RT_MESSAGETABLE: u32 = 11;
    /// Cursor group resource
    pub const RT_GROUP_CURSOR: u32 = 12;
    /// Icon group resource
    pub const RT_GROUP_ICON: u32 = 14;
    /// Version information resource
    pub const RT_VERSION: u32 = 16;
    /// Dialog include resource
    pub const RT_DLGINCLUDE: u32 = 17;
    /// Plug and Play resource
    pub const RT_PLUGPLAY: u32 = 19;
    /// VXD driver resource
    pub const RT_VXD: u32 = 20;
    /// Animated cursor resource
    pub const RT_ANICURSOR: u32 = 21;
    /// Animated icon resource
    pub const RT_ANIICON: u32 = 22;
    /// HTML resource
    pub const RT_HTML: u32 = 23;
    /// Side-by-side assembly manifest resource
    pub const RT_MANIFEST: u32 = 24;
}

/// VS_FIXEDFILEINFO structure (52 bytes)
#[derive(Debug, Clone)]
pub struct VsFixedFileInfo {
    /// Structure signature (0xFEEF04BD)
    pub signature: u32,
    /// Structure version (typically 0x00010000)
    pub struct_version: u32,
    /// High 32 bits of file version number
    pub file_version_ms: u32,
    /// Low 32 bits of file version number
    pub file_version_ls: u32,
    /// High 32 bits of product version number
    pub product_version_ms: u32,
    /// Low 32 bits of product version number
    pub product_version_ls: u32,
    /// Bitmask for valid file flags
    pub file_flags_mask: u32,
    /// File attribute flags
    pub file_flags: u32,
    /// Operating system for which file was designed
    pub file_os: u32,
    /// File type (application, DLL, driver, etc.)
    pub file_type: u32,
    /// File subtype (varies by file type)
    pub file_subtype: u32,
    /// High 32 bits of file creation date
    pub file_date_ms: u32,
    /// Low 32 bits of file creation date
    pub file_date_ls: u32,
}

impl VsFixedFileInfo {
    /// Returns the file version as a formatted string (e.g., "1.2.3.4")
    pub fn file_version(&self) -> String {
        format!(
            "{}.{}.{}.{}",
            (self.file_version_ms >> 16) & 0xFFFF,
            self.file_version_ms & 0xFFFF,
            (self.file_version_ls >> 16) & 0xFFFF,
            self.file_version_ls & 0xFFFF
        )
    }

    /// Returns the product version as a formatted string (e.g., "1.2.3.4")
    pub fn product_version(&self) -> String {
        format!(
            "{}.{}.{}.{}",
            (self.product_version_ms >> 16) & 0xFFFF,
            self.product_version_ms & 0xFFFF,
            (self.product_version_ls >> 16) & 0xFFFF,
            self.product_version_ls & 0xFFFF
        )
    }

    /// Returns a list of file flag descriptions based on the file_flags field
    pub fn file_flags_string(&self) -> Vec<&'static str> {
        let mut flags = Vec::new();
        let masked_flags = self.file_flags & self.file_flags_mask;

        if (masked_flags & 0x0001) != 0 {
            flags.push("Debug");
        }
        if (masked_flags & 0x0002) != 0 {
            flags.push("Pre-release");
        }
        if (masked_flags & 0x0004) != 0 {
            flags.push("Patched");
        }
        if (masked_flags & 0x0008) != 0 {
            flags.push("Private build");
        }
        if (masked_flags & 0x0010) != 0 {
            flags.push("Info inferred");
        }
        if (masked_flags & 0x0020) != 0 {
            flags.push("Special build");
        }

        flags
    }

    /// Returns a human-readable description of the target operating system
    pub fn file_os_string(&self) -> &'static str {
        match self.file_os {
            0x00010000 => "DOS",
            0x00020000 => "OS/2 16-bit",
            0x00030000 => "OS/2 32-bit",
            0x00040000 => "Windows NT",
            0x00050000 => "Windows CE",
            0x00000001 => "Windows 16-bit",
            0x00000004 => "Windows 32-bit",
            0x00010001 => "DOS-Windows 16-bit",
            0x00010004 => "DOS-Windows 32-bit",
            0x00020001 => "OS/2 16-bit, PM-16",
            0x00030001 => "OS/2 32-bit, PM-32",
            0x00040004 => "Windows NT 32-bit",
            _ => "Unknown",
        }
    }

    /// Returns a human-readable description of the file type
    pub fn file_type_string(&self) -> &'static str {
        match self.file_type {
            0x0 => "Unknown",
            0x1 => "Application",
            0x2 => "DLL",
            0x3 => "Driver",
            0x4 => "Font",
            0x5 => "VXD",
            0x7 => "Static library",
            _ => "Unknown",
        }
    }
}

/// Debug Directory Entry
#[derive(Debug, Clone)]
pub struct DebugDirectoryEntry {
    /// Reserved, must be zero
    pub characteristics: u32,
    /// Time/date stamp indicating when debug data was created
    pub time_date_stamp: u32,
    /// Major version number of debug data format
    pub major_version: u16,
    /// Minor version number of debug data format
    pub minor_version: u16,
    /// Type of debug information
    pub debug_type: u32,
    /// Size of debug data in bytes
    pub size_of_data: u32,
    /// RVA of debug data when loaded
    pub address_of_raw_data: u32,
    /// File offset to debug data
    pub pointer_to_raw_data: u32,
}

/// Debug type constants
pub mod debug_types {
    /// Unknown debug information type
    pub const IMAGE_DEBUG_TYPE_UNKNOWN: u32 = 0;
    /// COFF debug information
    pub const IMAGE_DEBUG_TYPE_COFF: u32 = 1;
    /// CodeView debug information
    pub const IMAGE_DEBUG_TYPE_CODEVIEW: u32 = 2;
    /// Frame pointer omission (FPO) debug information
    pub const IMAGE_DEBUG_TYPE_FPO: u32 = 3;
    /// Miscellaneous debug information
    pub const IMAGE_DEBUG_TYPE_MISC: u32 = 4;
    /// Exception information
    pub const IMAGE_DEBUG_TYPE_EXCEPTION: u32 = 5;
    /// Fixup information
    pub const IMAGE_DEBUG_TYPE_FIXUP: u32 = 6;
    /// OMAP to source mapping information
    pub const IMAGE_DEBUG_TYPE_OMAP_TO_SRC: u32 = 7;
    /// OMAP from source mapping information
    pub const IMAGE_DEBUG_TYPE_OMAP_FROM_SRC: u32 = 8;
    /// Borland debug information
    pub const IMAGE_DEBUG_TYPE_BORLAND: u32 = 9;
    /// Reserved debug type
    pub const IMAGE_DEBUG_TYPE_RESERVED10: u32 = 10;
    /// CLSID debug information
    pub const IMAGE_DEBUG_TYPE_CLSID: u32 = 11;
    /// Visual C++ feature information
    pub const IMAGE_DEBUG_TYPE_VC_FEATURE: u32 = 12;
    /// Profile-guided optimization (POGO) debug information
    pub const IMAGE_DEBUG_TYPE_POGO: u32 = 13;
    /// Incremental link-time code generation debug information
    pub const IMAGE_DEBUG_TYPE_ILTCG: u32 = 14;
    /// MPX debug information
    pub const IMAGE_DEBUG_TYPE_MPX: u32 = 15;
    /// Reproducible build debug information
    pub const IMAGE_DEBUG_TYPE_REPRO: u32 = 16;
}

/// CodeView RSDS debug info
#[derive(Debug, Clone)]
pub struct CodeViewRSDS {
    /// Signature bytes ("RSDS")
    pub signature: [u8; 4],
    /// GUID identifying the PDB file
    pub guid: [u8; 16],
    /// Age/iteration of the PDB file
    pub age: u32,
    /// Path to the PDB file
    pub pdb_file_name: String,
}

/// CodeView NB10 debug info
#[derive(Debug, Clone)]
pub struct CodeViewNB10 {
    /// Signature bytes ("NB10")
    pub signature: [u8; 4],
    /// File offset to debug information
    pub offset: u32,
    /// Timestamp when PDB file was created
    pub timestamp: u32,
    /// Age/iteration of the PDB file
    pub age: u32,
    /// Path to the PDB file
    pub pdb_file_name: String,
}

/// IMAGE_EXPORT_DIRECTORY structure (40 bytes)
#[derive(Debug, Clone)]
pub struct ImageExportDirectory {
    /// Export flags (reserved, must be 0)
    pub characteristics: u32,
    /// Time/date stamp
    pub time_date_stamp: u32,
    /// Major version
    pub major_version: u16,
    /// Minor version
    pub minor_version: u16,
    /// RVA of DLL name
    pub name: u32,
    /// Starting ordinal number
    pub base: u32,
    /// Number of entries in the Export Address Table
    pub number_of_functions: u32,
    /// Number of entries in the Name Pointer Table
    pub number_of_names: u32,
    /// RVA of Export Address Table
    pub address_of_functions: u32,
    /// RVA of Export Name Pointer Table
    pub address_of_names: u32,
    /// RVA of Export Ordinal Table
    pub address_of_name_ordinals: u32,
}

/// IMAGE_IMPORT_DESCRIPTOR (20 bytes)
#[derive(Debug, Clone, Copy)]
pub struct ImageImportDescriptor {
    /// RVA to Import Lookup Table (array of RVAs to import names)
    pub original_first_thunk: u32,
    /// Time/date stamp (usually 0 unless bound)
    pub time_date_stamp: u32,
    /// Forwarder chain (usually 0)
    pub forwarder_chain: u32,
    /// RVA to DLL name string
    pub name: u32,
    /// RVA to Import Address Table (array of addresses)
    pub first_thunk: u32,
}

impl ImageImportDescriptor {
    /// Returns true if this is a null descriptor (end of import table)
    pub fn is_null(&self) -> bool {
        self.original_first_thunk == 0
            && self.time_date_stamp == 0
            && self.forwarder_chain == 0
            && self.name == 0
            && self.first_thunk == 0
    }
}

/// Import information for a single DLL
#[derive(Debug, Clone)]
pub struct ImportInfo {
    /// Name of the imported DLL
    pub dll_name: String,
    /// List of imported functions (name or ordinal)
    pub functions: Vec<ImportFunction>,
}

/// Information about an imported function
#[derive(Debug, Clone)]
pub enum ImportFunction {
    /// Import by name
    ByName {
        /// Hint (index into export name pointer table)
        hint: u16,
        /// Function name
        name: String,
    },
    /// Import by ordinal
    ByOrdinal {
        /// Ordinal number
        ordinal: u16,
    },
}

/// Parsed export information
#[derive(Debug, Clone)]
pub struct ExportInfo {
    /// Export directory table
    pub directory: ImageExportDirectory,
    /// DLL name
    pub dll_name: String,
    /// List of exported function names (limited to first 30)
    pub function_names: Vec<String>,
    /// Number of forwarded exports
    pub forwarded_count: u32,
}
