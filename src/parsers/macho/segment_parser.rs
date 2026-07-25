//! Segment and section analysis
//!
//! This module provides utilities for analyzing segment and section data
//! extracted from Mach-O files.

use super::structures::{Section, SegmentCommand};

// =============================================================================
// Segment Analysis
// =============================================================================

/// Statistics about segments in a Mach-O file
#[derive(Debug, Clone, Default)]
pub struct SegmentStats {
    /// Total number of segments
    pub segment_count: usize,
    /// Total number of sections across all segments
    pub section_count: usize,
    /// Size of __TEXT segment
    pub text_size: u64,
    /// Size of __DATA segment
    pub data_size: u64,
    /// Size of __DATA_CONST segment
    pub data_const_size: u64,
    /// Size of __LINKEDIT segment
    pub linkedit_size: u64,
    /// Total virtual memory size
    pub total_vmsize: u64,
    /// Total file size of all segments
    pub total_filesize: u64,
    /// Whether __PAGEZERO segment is present
    pub has_pagezero: bool,
    /// __PAGEZERO size (typically large for 64-bit executables)
    pub pagezero_size: u64,
}

impl SegmentStats {
    /// Compute statistics from a list of segments
    pub fn from_segments(segments: &[SegmentCommand]) -> Self {
        let mut stats = SegmentStats {
            segment_count: segments.len(),
            ..Default::default()
        };

        for seg in segments {
            stats.section_count += seg.sections.len();
            stats.total_vmsize += seg.vmsize;
            stats.total_filesize += seg.filesize;

            match seg.segname.as_str() {
                "__TEXT" => stats.text_size = seg.vmsize,
                "__DATA" => stats.data_size = seg.vmsize,
                "__DATA_CONST" => stats.data_const_size = seg.vmsize,
                "__LINKEDIT" => stats.linkedit_size = seg.vmsize,
                "__PAGEZERO" => {
                    stats.has_pagezero = true;
                    stats.pagezero_size = seg.vmsize;
                }
                _ => {}
            }
        }

        stats
    }
}

/// Section type constants
pub mod section_type {
    /// Regular section
    pub const S_REGULAR: u32 = 0x0;
    /// Zero-fill on demand section
    pub const S_ZEROFILL: u32 = 0x1;
    /// Section with only literal C strings
    pub const S_CSTRING_LITERALS: u32 = 0x2;
    /// Section with only 4-byte literals
    pub const S_4BYTE_LITERALS: u32 = 0x3;
    /// Section with only 8-byte literals
    pub const S_8BYTE_LITERALS: u32 = 0x4;
    /// Section with only literal pointers
    pub const S_LITERAL_POINTERS: u32 = 0x5;
    /// Non-lazy symbol pointers section
    pub const S_NON_LAZY_SYMBOL_POINTERS: u32 = 0x6;
    /// Lazy symbol pointers section
    pub const S_LAZY_SYMBOL_POINTERS: u32 = 0x7;
    /// Symbol stubs section
    pub const S_SYMBOL_STUBS: u32 = 0x8;
    /// Mod init function pointers
    pub const S_MOD_INIT_FUNC_POINTERS: u32 = 0x9;
    /// Mod term function pointers
    pub const S_MOD_TERM_FUNC_POINTERS: u32 = 0xA;
    /// Section contains symbols to be coalesced
    pub const S_COALESCED: u32 = 0xB;
    /// Zero-fill on demand section (larger)
    pub const S_GB_ZEROFILL: u32 = 0xC;
    /// Section with only pairs of function pointers for interposing
    pub const S_INTERPOSING: u32 = 0xD;
    /// Section with only 16-byte literals
    pub const S_16BYTE_LITERALS: u32 = 0xE;
    /// Section containing DTrace Object Format
    pub const S_DTRACE_DOF: u32 = 0xF;
    /// Section with lazy symbol pointers to lazy loaded dylibs
    pub const S_LAZY_DYLIB_SYMBOL_POINTERS: u32 = 0x10;
    /// Thread local variable section
    pub const S_THREAD_LOCAL_REGULAR: u32 = 0x11;
    /// Thread local zerofill section
    pub const S_THREAD_LOCAL_ZEROFILL: u32 = 0x12;
    /// Thread local variable descriptors
    pub const S_THREAD_LOCAL_VARIABLES: u32 = 0x13;
    /// Pointers to TLV descriptors
    pub const S_THREAD_LOCAL_VARIABLE_POINTERS: u32 = 0x14;
    /// Functions to call to initialize TLV values
    pub const S_THREAD_LOCAL_INIT_FUNCTION_POINTERS: u32 = 0x15;
    /// 32-bit offsets for initializers
    pub const S_INIT_FUNC_OFFSETS: u32 = 0x16;
}

/// Section attributes (high byte)
pub mod section_attrs {
    /// Section contains only true machine instructions
    pub const S_ATTR_PURE_INSTRUCTIONS: u32 = 0x8000_0000;
    /// Section contains coalesced symbols
    pub const S_ATTR_NO_TOC: u32 = 0x4000_0000;
    /// Ok to strip static symbols in this section
    pub const S_ATTR_STRIP_STATIC_SYMS: u32 = 0x2000_0000;
    /// No dead stripping
    pub const S_ATTR_NO_DEAD_STRIP: u32 = 0x1000_0000;
    /// Blocks are live if they reference live blocks
    pub const S_ATTR_LIVE_SUPPORT: u32 = 0x0800_0000;
    /// Used with i386 code stubs
    pub const S_ATTR_SELF_MODIFYING_CODE: u32 = 0x0400_0000;
    /// A debug section
    pub const S_ATTR_DEBUG: u32 = 0x0200_0000;
    /// Section contains some machine instructions
    pub const S_ATTR_SOME_INSTRUCTIONS: u32 = 0x0000_0400;
    /// Section has external relocation entries
    pub const S_ATTR_EXT_RELOC: u32 = 0x0000_0200;
    /// Section has local relocation entries
    pub const S_ATTR_LOC_RELOC: u32 = 0x0000_0100;
}

/// Returns the section type from flags
pub fn section_type(flags: u32) -> u32 {
    flags & 0xFF
}

/// Returns the section type name
pub fn section_type_name(section_type: u32) -> &'static str {
    match section_type {
        section_type::S_REGULAR => "Regular",
        section_type::S_ZEROFILL => "Zerofill",
        section_type::S_CSTRING_LITERALS => "C String Literals",
        section_type::S_4BYTE_LITERALS => "4-byte Literals",
        section_type::S_8BYTE_LITERALS => "8-byte Literals",
        section_type::S_LITERAL_POINTERS => "Literal Pointers",
        section_type::S_NON_LAZY_SYMBOL_POINTERS => "Non-lazy Symbol Pointers",
        section_type::S_LAZY_SYMBOL_POINTERS => "Lazy Symbol Pointers",
        section_type::S_SYMBOL_STUBS => "Symbol Stubs",
        section_type::S_MOD_INIT_FUNC_POINTERS => "Mod Init Func Pointers",
        section_type::S_MOD_TERM_FUNC_POINTERS => "Mod Term Func Pointers",
        section_type::S_COALESCED => "Coalesced",
        section_type::S_GB_ZEROFILL => "GB Zerofill",
        section_type::S_INTERPOSING => "Interposing",
        section_type::S_16BYTE_LITERALS => "16-byte Literals",
        section_type::S_DTRACE_DOF => "DTrace DOF",
        section_type::S_LAZY_DYLIB_SYMBOL_POINTERS => "Lazy Dylib Symbol Pointers",
        section_type::S_THREAD_LOCAL_REGULAR => "Thread Local Regular",
        section_type::S_THREAD_LOCAL_ZEROFILL => "Thread Local Zerofill",
        section_type::S_THREAD_LOCAL_VARIABLES => "Thread Local Variables",
        section_type::S_THREAD_LOCAL_VARIABLE_POINTERS => "Thread Local Variable Pointers",
        section_type::S_THREAD_LOCAL_INIT_FUNCTION_POINTERS => "Thread Local Init Func Pointers",
        section_type::S_INIT_FUNC_OFFSETS => "Init Func Offsets",
        _ => "Unknown",
    }
}

/// Decode section attributes into a list of attribute names
pub fn decode_section_attrs(flags: u32) -> Vec<&'static str> {
    let mut attrs = Vec::new();

    if flags & section_attrs::S_ATTR_PURE_INSTRUCTIONS != 0 {
        attrs.push("PURE_INSTRUCTIONS");
    }
    if flags & section_attrs::S_ATTR_NO_TOC != 0 {
        attrs.push("NO_TOC");
    }
    if flags & section_attrs::S_ATTR_STRIP_STATIC_SYMS != 0 {
        attrs.push("STRIP_STATIC_SYMS");
    }
    if flags & section_attrs::S_ATTR_NO_DEAD_STRIP != 0 {
        attrs.push("NO_DEAD_STRIP");
    }
    if flags & section_attrs::S_ATTR_LIVE_SUPPORT != 0 {
        attrs.push("LIVE_SUPPORT");
    }
    if flags & section_attrs::S_ATTR_SELF_MODIFYING_CODE != 0 {
        attrs.push("SELF_MODIFYING_CODE");
    }
    if flags & section_attrs::S_ATTR_DEBUG != 0 {
        attrs.push("DEBUG");
    }
    if flags & section_attrs::S_ATTR_SOME_INSTRUCTIONS != 0 {
        attrs.push("SOME_INSTRUCTIONS");
    }
    if flags & section_attrs::S_ATTR_EXT_RELOC != 0 {
        attrs.push("EXT_RELOC");
    }
    if flags & section_attrs::S_ATTR_LOC_RELOC != 0 {
        attrs.push("LOC_RELOC");
    }

    attrs
}

/// Find a section by name within segments
pub fn find_section<'a>(
    segments: &'a [SegmentCommand],
    segname: &str,
    sectname: &str,
) -> Option<&'a Section> {
    for seg in segments {
        if seg.segname == segname {
            for sect in &seg.sections {
                if sect.sectname == sectname {
                    return Some(sect);
                }
            }
        }
    }
    None
}

/// Get all section names as a list
pub fn get_section_names(segments: &[SegmentCommand]) -> Vec<String> {
    let mut names = Vec::new();
    for seg in segments {
        for sect in &seg.sections {
            names.push(format!("{},{}", seg.segname, sect.sectname));
        }
    }
    names
}

/// Get all segment names
pub fn get_segment_names(segments: &[SegmentCommand]) -> Vec<String> {
    segments.iter().map(|s| s.segname.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_segment(name: &str, vmsize: u64, sections: Vec<Section>) -> SegmentCommand {
        SegmentCommand {
            segname: name.to_string(),
            vmaddr: 0,
            vmsize,
            fileoff: 0,
            filesize: vmsize,
            maxprot: 7,
            initprot: 5,
            nsects: sections.len() as u32,
            flags: 0,
            sections,
        }
    }

    fn create_test_section(segname: &str, sectname: &str, size: u64) -> Section {
        Section {
            sectname: sectname.to_string(),
            segname: segname.to_string(),
            addr: 0,
            size,
            offset: 0,
            align: 0,
            reloff: 0,
            nreloc: 0,
            flags: 0,
            reserved1: 0,
            reserved2: 0,
            reserved3: 0,
        }
    }

    #[test]
    fn test_segment_stats() {
        let segments = vec![
            create_test_segment("__PAGEZERO", 0x100000000, vec![]),
            create_test_segment(
                "__TEXT",
                0x10000,
                vec![
                    create_test_section("__TEXT", "__text", 0x8000),
                    create_test_section("__TEXT", "__stubs", 0x100),
                ],
            ),
            create_test_segment(
                "__DATA",
                0x4000,
                vec![create_test_section("__DATA", "__data", 0x2000)],
            ),
            create_test_segment("__LINKEDIT", 0x8000, vec![]),
        ];

        let stats = SegmentStats::from_segments(&segments);
        assert_eq!(stats.segment_count, 4);
        assert_eq!(stats.section_count, 3);
        assert_eq!(stats.text_size, 0x10000);
        assert_eq!(stats.data_size, 0x4000);
        assert_eq!(stats.linkedit_size, 0x8000);
        assert!(stats.has_pagezero);
        assert_eq!(stats.pagezero_size, 0x100000000);
    }

    #[test]
    fn test_section_type_name() {
        assert_eq!(section_type_name(section_type::S_REGULAR), "Regular");
        assert_eq!(section_type_name(section_type::S_ZEROFILL), "Zerofill");
        assert_eq!(
            section_type_name(section_type::S_CSTRING_LITERALS),
            "C String Literals"
        );
    }

    #[test]
    fn test_decode_section_attrs() {
        let flags =
            section_attrs::S_ATTR_PURE_INSTRUCTIONS | section_attrs::S_ATTR_SOME_INSTRUCTIONS;
        let attrs = decode_section_attrs(flags);
        assert!(attrs.contains(&"PURE_INSTRUCTIONS"));
        assert!(attrs.contains(&"SOME_INSTRUCTIONS"));
    }

    #[test]
    fn test_find_section() {
        let segments = vec![create_test_segment(
            "__TEXT",
            0x10000,
            vec![
                create_test_section("__TEXT", "__text", 0x8000),
                create_test_section("__TEXT", "__stubs", 0x100),
            ],
        )];

        let sect = find_section(&segments, "__TEXT", "__text");
        assert!(sect.is_some());
        assert_eq!(sect.unwrap().size, 0x8000);

        let sect = find_section(&segments, "__TEXT", "__nonexistent");
        assert!(sect.is_none());
    }

    #[test]
    fn test_get_section_names() {
        let segments = vec![
            create_test_segment(
                "__TEXT",
                0x10000,
                vec![create_test_section("__TEXT", "__text", 0x8000)],
            ),
            create_test_segment(
                "__DATA",
                0x4000,
                vec![create_test_section("__DATA", "__data", 0x2000)],
            ),
        ];

        let names = get_section_names(&segments);
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"__TEXT,__text".to_string()));
        assert!(names.contains(&"__DATA,__data".to_string()));
    }
}
