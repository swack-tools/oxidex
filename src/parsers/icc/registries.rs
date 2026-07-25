//! ICC Profile registries and lookup tables
//!
//! This module contains all static definitions for ICC tag processing:
//! - TAG_REGISTRY: Tag signatures, names, and types
//! - HEADER_FIELDS: Header field definitions and extractors
//! - Lookup tables: Profile classes, platforms, technologies, etc.

use crate::core::TagValue;
use crate::error::Result;
use std::collections::HashMap;

// ============================================================================
// TYPE ALIASES
// ============================================================================

/// Type alias for header field extractor functions
/// Maps bytes at offset into metadata using the provided HashMap
pub type ExtractFn = fn(&[u8], usize, &mut HashMap<String, TagValue>) -> Result<()>;

// ============================================================================
// CORE REGISTRY STRUCTURES
// ============================================================================

/// Type of ICC tag data - determines which decoder to use
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TagType {
    /// Text description (desc/mluc)
    TextDescription,
    /// Simple text (text/mluc)
    Text,
    /// XYZ coordinate triple
    Xyz,
    /// Binary curve data
    Curve,
    /// Viewing conditions structure
    ViewingConditions,
    /// Measurement structure
    Measurement,
    /// 4-byte signature
    Signature,
}

/// Registry entry for an ICC tag
pub struct TagDef {
    /// 4-character ICC tag signature
    pub signature: &'static str,
    /// Human-readable tag name (added to metadata)
    pub name: &'static str,
    /// Type of data this tag contains
    pub tag_type: TagType,
}

/// Header field definition for structured parsing
pub struct HeaderField {
    /// Byte offset in ICC header
    pub offset: usize,
    /// Field name in metadata
    pub name: &'static str,
    /// Extractor function
    pub extract: ExtractFn,
}

/// Lookup table entry for mapping codes to names
pub struct LookupEntry {
    /// Code or signature to match
    pub code: &'static str,
    /// Human-readable name
    pub name: &'static str,
}

// ============================================================================
// TAG REGISTRY
// ============================================================================

/// Complete ICC tag registry
///
/// This table defines all supported ICC tags with their signatures, names,
/// and associated decoder types. Adding a new tag is as simple as adding
/// a new entry to this table.
pub static TAG_REGISTRY: &[TagDef] = &[
    // Text description tags
    TagDef {
        signature: "desc",
        name: "ProfileDescription",
        tag_type: TagType::TextDescription,
    },
    TagDef {
        signature: "cprt",
        name: "ProfileCopyright",
        tag_type: TagType::Text,
    },
    TagDef {
        signature: "dmnd",
        name: "DeviceMfgDesc",
        tag_type: TagType::TextDescription,
    },
    TagDef {
        signature: "dmdd",
        name: "DeviceModelDesc",
        tag_type: TagType::TextDescription,
    },
    TagDef {
        signature: "vued",
        name: "ViewingCondDesc",
        tag_type: TagType::TextDescription,
    },
    // XYZ coordinate tags
    TagDef {
        signature: "wtpt",
        name: "MediaWhitePoint",
        tag_type: TagType::Xyz,
    },
    TagDef {
        signature: "bkpt",
        name: "MediaBlackPoint",
        tag_type: TagType::Xyz,
    },
    TagDef {
        signature: "rXYZ",
        name: "RedMatrixColumn",
        tag_type: TagType::Xyz,
    },
    TagDef {
        signature: "gXYZ",
        name: "GreenMatrixColumn",
        tag_type: TagType::Xyz,
    },
    TagDef {
        signature: "bXYZ",
        name: "BlueMatrixColumn",
        tag_type: TagType::Xyz,
    },
    TagDef {
        signature: "lumi",
        name: "Luminance",
        tag_type: TagType::Xyz,
    },
    // Curve tags (binary data)
    TagDef {
        signature: "rTRC",
        name: "RedToneReproductionCurve",
        tag_type: TagType::Curve,
    },
    TagDef {
        signature: "gTRC",
        name: "GreenToneReproductionCurve",
        tag_type: TagType::Curve,
    },
    TagDef {
        signature: "bTRC",
        name: "BlueToneReproductionCurve",
        tag_type: TagType::Curve,
    },
    // Structured data tags
    TagDef {
        signature: "view",
        name: "ViewingConditions",
        tag_type: TagType::ViewingConditions,
    },
    TagDef {
        signature: "meas",
        name: "Measurement",
        tag_type: TagType::Measurement,
    },
    TagDef {
        signature: "tech",
        name: "Technology",
        tag_type: TagType::Signature,
    },
];

// ============================================================================
// LOOKUP TABLES
// ============================================================================

/// Profile class lookup table
pub static PROFILE_CLASSES: &[LookupEntry] = &[
    LookupEntry {
        code: "scnr",
        name: "Input Device Profile",
    },
    LookupEntry {
        code: "mntr",
        name: "Display Device Profile",
    },
    LookupEntry {
        code: "prtr",
        name: "Output Device Profile",
    },
    LookupEntry {
        code: "link",
        name: "DeviceLink Profile",
    },
    LookupEntry {
        code: "spac",
        name: "ColorSpace Profile",
    },
    LookupEntry {
        code: "abst",
        name: "Abstract Profile",
    },
    LookupEntry {
        code: "nmcl",
        name: "Named Color Profile",
    },
];

/// Platform lookup table
pub static PLATFORMS: &[LookupEntry] = &[
    LookupEntry {
        code: "APPL",
        name: "Apple Computer Inc.",
    },
    LookupEntry {
        code: "MSFT",
        name: "Microsoft Corporation",
    },
    LookupEntry {
        code: "SGI",
        name: "Silicon Graphics Inc.",
    },
    LookupEntry {
        code: "SUNW",
        name: "Sun Microsystems",
    },
];

/// Technology lookup table
pub static TECHNOLOGIES: &[LookupEntry] = &[
    LookupEntry {
        code: "fscn",
        name: "Film Scanner",
    },
    LookupEntry {
        code: "dcam",
        name: "Digital Camera",
    },
    LookupEntry {
        code: "rscn",
        name: "Reflective Scanner",
    },
    LookupEntry {
        code: "ijet",
        name: "Ink Jet Printer",
    },
    LookupEntry {
        code: "twax",
        name: "Thermal Wax Printer",
    },
    LookupEntry {
        code: "epho",
        name: "Electrophotographic Printer",
    },
    LookupEntry {
        code: "esta",
        name: "Electrostatic Printer",
    },
    LookupEntry {
        code: "dsub",
        name: "Dye Sublimation Printer",
    },
    LookupEntry {
        code: "rpho",
        name: "Photographic Paper Printer",
    },
    LookupEntry {
        code: "fprn",
        name: "Film Writer",
    },
    LookupEntry {
        code: "vidm",
        name: "Video Monitor",
    },
    LookupEntry {
        code: "vidc",
        name: "Video Camera",
    },
    LookupEntry {
        code: "pjtv",
        name: "Projection Television",
    },
    LookupEntry {
        code: "CRT",
        name: "Cathode Ray Tube Display",
    },
    LookupEntry {
        code: "PMD",
        name: "Passive Matrix Display",
    },
    LookupEntry {
        code: "AMD",
        name: "Active Matrix Display",
    },
    LookupEntry {
        code: "KPCD",
        name: "Photo CD",
    },
    LookupEntry {
        code: "imgs",
        name: "Photo Image Setter",
    },
    LookupEntry {
        code: "grav",
        name: "Gravure",
    },
    LookupEntry {
        code: "offs",
        name: "Offset Lithography",
    },
    LookupEntry {
        code: "silk",
        name: "Silkscreen",
    },
    LookupEntry {
        code: "flex",
        name: "Flexography",
    },
];

/// Rendering intent names (indexed by code 0-3)
/// Names match ExifTool output format
pub static RENDERING_INTENTS: &[&str] = &[
    "Perceptual",
    "Media-Relative Colorimetric",
    "Saturation",
    "ICC-Absolute Colorimetric",
];

/// CMM (Color Management Module) type lookup table
///
/// Maps 4-character CMM signature codes to human-readable CMM names.
/// These codes identify the Color Management Module used to create the profile.
pub static CMM_TYPES: &[LookupEntry] = &[
    LookupEntry {
        code: "ADBE",
        name: "Adobe Systems Inc.",
    },
    LookupEntry {
        code: "ACMS",
        name: "Agfa Color Management System",
    },
    LookupEntry {
        code: "appl",
        name: "Apple Computer Inc.",
    },
    LookupEntry {
        code: "APPL",
        name: "Apple Computer Inc.",
    },
    LookupEntry {
        code: "CCMS",
        name: "ColorGear",
    },
    LookupEntry {
        code: "Efi",
        name: "EFI",
    },
    LookupEntry {
        code: "EFI",
        name: "EFI",
    },
    LookupEntry {
        code: "FF",
        name: "Fuji Film",
    },
    LookupEntry {
        code: "EXAC",
        name: "ExactCode",
    },
    LookupEntry {
        code: "Hcmm",
        name: "Harlequin",
    },
    LookupEntry {
        code: "argl",
        name: "Argyll CMS",
    },
    LookupEntry {
        code: "LgoS",
        name: "Logo Sync",
    },
    LookupEntry {
        code: "HDM",
        name: "Heidelberg",
    },
    LookupEntry {
        code: "lcms",
        name: "Little CMS",
    },
    LookupEntry {
        code: "KCMS",
        name: "Kodak Color Management System",
    },
    LookupEntry {
        code: "Lino",
        name: "Linotronic",
    },
    LookupEntry {
        code: "MCML",
        name: "Konica Minolta",
    },
    LookupEntry {
        code: "NKON",
        name: "Nikon Corporation",
    },
    LookupEntry {
        code: "WCS",
        name: "Microsoft WCS",
    },
    LookupEntry {
        code: "MSFT",
        name: "Microsoft Corporation",
    },
    LookupEntry {
        code: "SIGN",
        name: "Mutoh",
    },
    LookupEntry {
        code: "ONYX",
        name: "Onyx Graphics",
    },
    LookupEntry {
        code: "RGMS",
        name: "DeviceLink",
    },
    LookupEntry {
        code: "SICC",
        name: "SampleICC",
    },
    LookupEntry {
        code: "TCMM",
        name: "Toshiba",
    },
    LookupEntry {
        code: "UCCM",
        name: "Unknown (UCCM)",
    },
    LookupEntry {
        code: "32BT",
        name: "the imaging factory",
    },
    LookupEntry {
        code: "vivo",
        name: "Vivo Mobile",
    },
    LookupEntry {
        code: "WTG",
        name: "Ware to Go",
    },
    LookupEntry {
        code: "zc00",
        name: "Zoran",
    },
];

/// Device manufacturer / profile creator lookup table
///
/// Maps 4-character manufacturer signature codes to human-readable names.
/// Used for DeviceManufacturer and ProfileCreator header fields.
pub static MANUFACTURERS: &[LookupEntry] = &[
    LookupEntry {
        code: "ADBE",
        name: "Adobe Systems Inc.",
    },
    LookupEntry {
        code: "APPL",
        name: "Apple Computer Inc.",
    },
    LookupEntry {
        code: "appl",
        name: "Apple Computer Inc.",
    },
    LookupEntry {
        code: "CANO",
        name: "Canon, Inc. (Canon Development Americas, Inc.)",
    },
    LookupEntry {
        code: "EPSO",
        name: "Epson",
    },
    LookupEntry {
        code: "GOOG",
        name: "Google",
    },
    LookupEntry {
        code: "HP",
        name: "Hewlett-Packard",
    },
    LookupEntry {
        code: "IEC",
        name: "Hewlett-Packard",
    },
    LookupEntry {
        code: "ISL",
        name: "Ichikawa Soft Laboratory",
    },
    LookupEntry {
        code: "KODA",
        name: "Kodak",
    },
    LookupEntry {
        code: "MSFT",
        name: "Microsoft Corporation",
    },
    LookupEntry {
        code: "NKON",
        name: "Nikon",
    },
    LookupEntry {
        code: "SGI",
        name: "Silicon Graphics",
    },
    LookupEntry {
        code: "SUNW",
        name: "Sun Microsystems",
    },
    LookupEntry {
        code: "TOSH",
        name: "Toshiba",
    },
    LookupEntry {
        code: "argl",
        name: "Argyll CMS",
    },
    LookupEntry {
        code: "lcms",
        name: "Little CMS",
    },
    LookupEntry {
        code: "none",
        name: "none",
    },
];

/// Illuminant type names (indexed by code 1-8)
pub static ILLUMINANT_TYPES: &[&str] = &[
    "Unknown",        // 0 - not used
    "D50",            // 1
    "D65",            // 2
    "D93",            // 3
    "F2",             // 4
    "D55",            // 5
    "A",              // 6
    "Equi-Power (E)", // 7
    "F8",             // 8
];

/// Observer types (indexed by code 1-2)
pub static OBSERVER_TYPES: &[&str] = &[
    "Unknown",  // 0
    "CIE 1931", // 1
    "CIE 1964", // 2
];

/// Geometry types (indexed by code 0-2)
pub static GEOMETRY_TYPES: &[&str] = &[
    "Unknown",      // 0
    "0/45 or 45/0", // 1
    "0/d or d/0",   // 2
];

// ============================================================================
// LOOKUP TABLE HELPER
// ============================================================================

/// Generic lookup function for finding names in lookup tables
pub fn lookup_in_table<'a>(table: &'a [LookupEntry], code: &'a str) -> &'a str {
    let trimmed = code.trim();
    table
        .iter()
        .find(|entry| entry.code == trimmed)
        .map(|entry| entry.name)
        .unwrap_or(trimmed)
}
