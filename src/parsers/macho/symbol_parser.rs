//! Symbol table analysis
//!
//! This module provides utilities for analyzing symbol table data
//! extracted from Mach-O files.

use super::structures::{DysymtabCommand, SymtabCommand};

// =============================================================================
// Symbol Statistics
// =============================================================================

/// Statistics about symbols in a Mach-O file
#[derive(Debug, Clone, Default)]
pub struct SymbolStats {
    /// Total number of symbols in the symbol table
    pub total_symbols: u32,
    /// Number of local symbols
    pub local_symbols: u32,
    /// Number of externally defined symbols (exports)
    pub external_symbols: u32,
    /// Number of undefined symbols (imports)
    pub undefined_symbols: u32,
    /// Size of the string table in bytes
    pub string_table_size: u32,
    /// Number of indirect symbols
    pub indirect_symbols: u32,
    /// Number of external relocations
    pub external_relocations: u32,
    /// Number of local relocations
    pub local_relocations: u32,
}

impl SymbolStats {
    /// Create symbol stats from symtab and optional dysymtab commands
    pub fn from_commands(symtab: &SymtabCommand, dysymtab: Option<&DysymtabCommand>) -> Self {
        let mut stats = SymbolStats {
            total_symbols: symtab.nsyms,
            string_table_size: symtab.strsize,
            ..Default::default()
        };

        if let Some(dys) = dysymtab {
            stats.local_symbols = dys.nlocalsym;
            stats.external_symbols = dys.nextdefsym;
            stats.undefined_symbols = dys.nundefsym;
            stats.indirect_symbols = dys.nindirectsyms;
            stats.external_relocations = dys.nextrel;
            stats.local_relocations = dys.nlocrel;
        }

        stats
    }
}

// =============================================================================
// Symbol Types
// =============================================================================

/// Symbol type constants (from n_type field)
pub mod n_type {
    /// Undefined symbol
    pub const N_UNDF: u8 = 0x0;
    /// Absolute symbol
    pub const N_ABS: u8 = 0x2;
    /// Section-relative symbol
    pub const N_SECT: u8 = 0xe;
    /// Prebound undefined (obsolete)
    pub const N_PBUD: u8 = 0xc;
    /// Indirect symbol
    pub const N_INDR: u8 = 0xa;
}

/// Symbol type masks
pub mod n_type_mask {
    /// Mask for symbol type
    pub const N_TYPE: u8 = 0x0e;
    /// External symbol
    pub const N_EXT: u8 = 0x01;
    /// Private external symbol
    pub const N_PEXT: u8 = 0x10;
    /// Stab symbol (debug info)
    pub const N_STAB: u8 = 0xe0;
}

/// Returns the symbol type name
pub fn symbol_type_name(n_type: u8) -> &'static str {
    // Check if stab symbol first
    if n_type & n_type_mask::N_STAB != 0 {
        return "Debug (STAB)";
    }

    match n_type & n_type_mask::N_TYPE {
        n_type::N_UNDF => {
            if n_type & n_type_mask::N_EXT != 0 {
                "Undefined External"
            } else {
                "Undefined"
            }
        }
        n_type::N_ABS => {
            if n_type & n_type_mask::N_EXT != 0 {
                "Absolute External"
            } else {
                "Absolute"
            }
        }
        n_type::N_SECT => {
            if n_type & n_type_mask::N_EXT != 0 {
                "Defined External"
            } else if n_type & n_type_mask::N_PEXT != 0 {
                "Private External"
            } else {
                "Defined"
            }
        }
        n_type::N_PBUD => "Prebound Undefined",
        n_type::N_INDR => "Indirect",
        _ => "Unknown",
    }
}

/// Check if a symbol is external (exported)
pub fn is_external(n_type: u8) -> bool {
    (n_type & n_type_mask::N_EXT) != 0 && (n_type & n_type_mask::N_TYPE) == n_type::N_SECT
}

/// Check if a symbol is undefined (imported)
pub fn is_undefined(n_type: u8) -> bool {
    (n_type & n_type_mask::N_TYPE) == n_type::N_UNDF
}

/// Check if a symbol is a debug symbol (STAB)
pub fn is_debug_symbol(n_type: u8) -> bool {
    (n_type & n_type_mask::N_STAB) != 0
}

// =============================================================================
// Symbol Description Flags
// =============================================================================

/// Symbol description flags (from n_desc field)
pub mod n_desc {
    /// Referenced dynamically
    pub const REFERENCED_DYNAMICALLY: u16 = 0x0010;
    /// No dead strip
    pub const N_NO_DEAD_STRIP: u16 = 0x0020;
    /// Weak reference
    pub const N_WEAK_REF: u16 = 0x0040;
    /// Weak definition
    pub const N_WEAK_DEF: u16 = 0x0080;
    /// Symbol has thumb function (ARM)
    pub const N_ARM_THUMB_DEF: u16 = 0x0008;
    /// Symbol is a resolver function
    pub const N_SYMBOL_RESOLVER: u16 = 0x0100;
    /// Symbol has alternate entry point
    pub const N_ALT_ENTRY: u16 = 0x0200;
    /// Symbol is cold (rarely executed)
    pub const N_COLD_FUNC: u16 = 0x0400;
}

/// Decode symbol description flags
pub fn decode_n_desc(n_desc: u16) -> Vec<&'static str> {
    let mut flags = Vec::new();

    if n_desc & n_desc::REFERENCED_DYNAMICALLY != 0 {
        flags.push("REFERENCED_DYNAMICALLY");
    }
    if n_desc & n_desc::N_NO_DEAD_STRIP != 0 {
        flags.push("NO_DEAD_STRIP");
    }
    if n_desc & n_desc::N_WEAK_REF != 0 {
        flags.push("WEAK_REF");
    }
    if n_desc & n_desc::N_WEAK_DEF != 0 {
        flags.push("WEAK_DEF");
    }
    if n_desc & n_desc::N_ARM_THUMB_DEF != 0 {
        flags.push("ARM_THUMB_DEF");
    }
    if n_desc & n_desc::N_SYMBOL_RESOLVER != 0 {
        flags.push("SYMBOL_RESOLVER");
    }
    if n_desc & n_desc::N_ALT_ENTRY != 0 {
        flags.push("ALT_ENTRY");
    }
    if n_desc & n_desc::N_COLD_FUNC != 0 {
        flags.push("COLD_FUNC");
    }

    flags
}

/// Get the library ordinal from n_desc (for undefined symbols)
pub fn get_library_ordinal(n_desc: u16) -> u8 {
    ((n_desc >> 8) & 0xFF) as u8
}

// =============================================================================
// Symbol Categories
// =============================================================================

/// Categories of symbols for analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolCategory {
    /// Exported symbol (defined and external)
    Export,
    /// Imported symbol (undefined)
    Import,
    /// Local symbol (defined, not external)
    Local,
    /// Debug symbol (STAB)
    Debug,
    /// Weak symbol
    Weak,
    /// Unknown category
    Unknown,
}

impl SymbolCategory {
    /// Categorize a symbol based on its type and description
    pub fn from_type_desc(n_type: u8, n_desc: u16) -> Self {
        if is_debug_symbol(n_type) {
            return SymbolCategory::Debug;
        }

        if n_desc & n_desc::N_WEAK_DEF != 0 || n_desc & n_desc::N_WEAK_REF != 0 {
            return SymbolCategory::Weak;
        }

        if is_undefined(n_type) {
            return SymbolCategory::Import;
        }

        if is_external(n_type) {
            return SymbolCategory::Export;
        }

        if (n_type & n_type_mask::N_TYPE) == n_type::N_SECT {
            return SymbolCategory::Local;
        }

        SymbolCategory::Unknown
    }

    /// Get a human-readable name for the category
    pub fn name(&self) -> &'static str {
        match self {
            SymbolCategory::Export => "Export",
            SymbolCategory::Import => "Import",
            SymbolCategory::Local => "Local",
            SymbolCategory::Debug => "Debug",
            SymbolCategory::Weak => "Weak",
            SymbolCategory::Unknown => "Unknown",
        }
    }
}

// =============================================================================
// Symbol Name Analysis
// =============================================================================

/// Check if a symbol name appears to be mangled (C++ or Swift)
pub fn is_mangled_name(name: &str) -> bool {
    // C++ mangled names start with _Z
    if name.starts_with("_Z") || name.starts_with("__Z") {
        return true;
    }

    // Swift mangled names start with $s or _$s
    if name.starts_with("$s") || name.starts_with("_$s") {
        return true;
    }

    // Older Swift mangling
    if name.starts_with("_T") && name.len() > 2 {
        return true;
    }

    false
}

/// Detect the language from a symbol name
pub fn detect_language(name: &str) -> &'static str {
    if name.starts_with("$s") || name.starts_with("_$s") || name.starts_with("_T") {
        "Swift"
    } else if name.starts_with("_Z") || name.starts_with("__Z") {
        "C++"
    } else if name.starts_with("+[") || name.starts_with("-[") || name.starts_with("_OBJC_") {
        "Objective-C"
    } else {
        "C"
    }
}

/// Check if a symbol name looks like an Objective-C method
pub fn is_objc_method(name: &str) -> bool {
    (name.starts_with("+[") || name.starts_with("-[")) && name.ends_with(']')
}

/// Check if a symbol name looks like a C++ symbol
pub fn is_cpp_symbol(name: &str) -> bool {
    name.starts_with("_Z") || name.starts_with("__Z")
}

/// Check if a symbol name looks like a Swift symbol
pub fn is_swift_symbol(name: &str) -> bool {
    name.starts_with("$s") || name.starts_with("_$s") || name.starts_with("_T")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_stats_from_commands() {
        let symtab = SymtabCommand {
            symoff: 0x1000,
            nsyms: 100,
            stroff: 0x2000,
            strsize: 0x500,
        };

        let dysymtab = DysymtabCommand {
            ilocalsym: 0,
            nlocalsym: 50,
            iextdefsym: 50,
            nextdefsym: 30,
            iundefsym: 80,
            nundefsym: 20,
            tocoff: 0,
            ntoc: 0,
            modtaboff: 0,
            nmodtab: 0,
            extrefsymoff: 0,
            nextrefsyms: 0,
            indirectsymoff: 0,
            nindirectsyms: 10,
            extreloff: 0,
            nextrel: 5,
            locreloff: 0,
            nlocrel: 3,
        };

        let stats = SymbolStats::from_commands(&symtab, Some(&dysymtab));
        assert_eq!(stats.total_symbols, 100);
        assert_eq!(stats.local_symbols, 50);
        assert_eq!(stats.external_symbols, 30);
        assert_eq!(stats.undefined_symbols, 20);
        assert_eq!(stats.indirect_symbols, 10);
    }

    #[test]
    fn test_symbol_type_name() {
        assert_eq!(
            symbol_type_name(n_type::N_SECT | n_type_mask::N_EXT),
            "Defined External"
        );
        assert_eq!(
            symbol_type_name(n_type::N_UNDF | n_type_mask::N_EXT),
            "Undefined External"
        );
        assert_eq!(symbol_type_name(n_type::N_SECT), "Defined");
        assert_eq!(symbol_type_name(0xE0), "Debug (STAB)");
    }

    #[test]
    fn test_is_external() {
        assert!(is_external(n_type::N_SECT | n_type_mask::N_EXT));
        assert!(!is_external(n_type::N_SECT));
        assert!(!is_external(n_type::N_UNDF | n_type_mask::N_EXT));
    }

    #[test]
    fn test_is_undefined() {
        assert!(is_undefined(n_type::N_UNDF));
        assert!(is_undefined(n_type::N_UNDF | n_type_mask::N_EXT));
        assert!(!is_undefined(n_type::N_SECT));
    }

    #[test]
    fn test_symbol_category() {
        assert_eq!(
            SymbolCategory::from_type_desc(n_type::N_SECT | n_type_mask::N_EXT, 0),
            SymbolCategory::Export
        );
        assert_eq!(
            SymbolCategory::from_type_desc(n_type::N_UNDF | n_type_mask::N_EXT, 0),
            SymbolCategory::Import
        );
        assert_eq!(
            SymbolCategory::from_type_desc(n_type::N_SECT, 0),
            SymbolCategory::Local
        );
        assert_eq!(
            SymbolCategory::from_type_desc(n_type::N_SECT, n_desc::N_WEAK_DEF),
            SymbolCategory::Weak
        );
    }

    #[test]
    fn test_is_mangled_name() {
        assert!(is_mangled_name("_ZN4test3fooEv"));
        assert!(is_mangled_name("__ZN4test3fooEv"));
        assert!(is_mangled_name("$s4test3fooyyF"));
        assert!(is_mangled_name("_$s4test3fooyyF"));
        assert!(!is_mangled_name("_main"));
        assert!(!is_mangled_name("printf"));
    }

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language("_ZN4test3fooEv"), "C++");
        assert_eq!(detect_language("$s4test3fooyyF"), "Swift");
        assert_eq!(detect_language("+[NSObject alloc]"), "Objective-C");
        assert_eq!(detect_language("-[NSObject init]"), "Objective-C");
        assert_eq!(detect_language("_OBJC_CLASS_$_MyClass"), "Objective-C");
        assert_eq!(detect_language("_main"), "C");
    }

    #[test]
    fn test_is_objc_method() {
        assert!(is_objc_method("+[NSObject alloc]"));
        assert!(is_objc_method("-[NSString length]"));
        assert!(!is_objc_method("_main"));
        assert!(!is_objc_method("+[Incomplete"));
    }

    #[test]
    fn test_decode_n_desc() {
        let flags = n_desc::N_WEAK_DEF | n_desc::N_NO_DEAD_STRIP;
        let decoded = decode_n_desc(flags);
        assert!(decoded.contains(&"WEAK_DEF"));
        assert!(decoded.contains(&"NO_DEAD_STRIP"));
    }

    #[test]
    fn test_get_library_ordinal() {
        // Library ordinal 3 in high byte
        let n_desc: u16 = 0x0300;
        assert_eq!(get_library_ordinal(n_desc), 3);
    }
}
