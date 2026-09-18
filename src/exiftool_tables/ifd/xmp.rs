//! ExifTool `XMP` IFD-style tables, generated from ExifTool
//! 13.59's own Perl hashes -- one file per module; `mod.rs` beside this
//! file is the hub that declares and re-exports it.
//!
//! DO NOT EDIT. Regenerate with `just regen-tables` (see `mod.rs`).

#![allow(clippy::unreadable_literal, clippy::too_many_lines, unused_parens)]

// Everything a table literal names -- the `ifd_schema` types, `ExprId`, the
// `cond`/`subdir`/`validation` imports -- is in scope in the hub, and a glob
// import of the parent module brings its private imports along (RFC 1560).
#[allow(unused_imports)]
use super::*;

/// `Image::ExifTool::XMP::Album` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_ALBUM: IfdTable = IfdTable {
    module: "XMP",
    table: "Album",
    group0: "XMP",
    group1: "XMP-album",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 2)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::Composite` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_COMPOSITE: IfdTable = IfdTable {
    module: "XMP",
    table: "Composite",
    group0: "Composite",
    group1: "Composite",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 6)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::ExifTool` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_EXIFTOOL: IfdTable = IfdTable {
    module: "XMP",
    table: "ExifTool",
    group0: "XMP",
    group1: "XMP-et",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 3)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::Lightroom` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_LIGHTROOM: IfdTable = IfdTable {
    module: "XMP",
    table: "Lightroom",
    group0: "XMP",
    group1: "XMP-lr",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 4)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::aux` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_AUX: IfdTable = IfdTable {
    module: "XMP",
    table: "aux",
    group0: "XMP",
    group1: "XMP-aux",
    group2: "Camera",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 26)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::crs` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_CRS: IfdTable = IfdTable {
    module: "XMP",
    table: "crs",
    group0: "XMP",
    group1: "XMP-crs",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 272)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::dc` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_DC: IfdTable = IfdTable {
    module: "XMP",
    table: "dc",
    group0: "XMP",
    group1: "XMP-dc",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 16)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::exif` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_EXIF: IfdTable = IfdTable {
    module: "XMP",
    table: "exif",
    group0: "XMP",
    group1: "XMP-exif",
    group2: "Image",
    set_group1: None,
    priority: Some(0),
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 81)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::exifEX` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_EXIFEX: IfdTable = IfdTable {
    module: "XMP",
    table: "exifEX",
    group0: "XMP",
    group1: "XMP-exifEX",
    group2: "Image",
    set_group1: None,
    priority: Some(0),
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 32)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::iptcCore` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_IPTCCORE: IfdTable = IfdTable {
    module: "XMP",
    table: "iptcCore",
    group0: "XMP",
    group1: "XMP-iptcCore",
    group2: "Author",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 17)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::other` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_OTHER: IfdTable = IfdTable {
    module: "XMP",
    table: "other",
    group0: "XMP",
    group1: "XMP",
    group2: "Unknown",
    set_group1: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::pdf` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_PDF: IfdTable = IfdTable {
    module: "XMP",
    table: "pdf",
    group0: "XMP",
    group1: "XMP-pdf",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 13)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::pdfx` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_PDFX: IfdTable = IfdTable {
    module: "XMP",
    table: "pdfx",
    group0: "XMP",
    group1: "XMP-pdfx",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::photoshop` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_PHOTOSHOP: IfdTable = IfdTable {
    module: "XMP",
    table: "photoshop",
    group0: "XMP",
    group1: "XMP-photoshop",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 27)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::rdf` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_RDF: IfdTable = IfdTable {
    module: "XMP",
    table: "rdf",
    group0: "XMP",
    group1: "XMP-rdf",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::sArea` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_SAREA: IfdTable = IfdTable {
    module: "XMP",
    table: "sArea",
    group0: "XMP",
    group1: "XMP",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 7)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::sColorant` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_SCOLORANT: IfdTable = IfdTable {
    module: "XMP",
    table: "sColorant",
    group0: "XMP",
    group1: "XMP",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 16)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::sDimensions` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_SDIMENSIONS: IfdTable = IfdTable {
    module: "XMP",
    table: "sDimensions",
    group0: "XMP",
    group1: "XMP",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 4)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::specialStruct` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_SPECIALSTRUCT: IfdTable = IfdTable {
    module: "XMP",
    table: "specialStruct",
    group0: "XMP",
    group1: "XMP",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 3)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::tiff` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_TIFF: IfdTable = IfdTable {
    module: "XMP",
    table: "tiff",
    group0: "XMP",
    group1: "XMP-tiff",
    group2: "Image",
    set_group1: None,
    priority: Some(0),
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 27)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::x` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_X: IfdTable = IfdTable {
    module: "XMP",
    table: "x",
    group0: "XMP",
    group1: "XMP-x",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::xmp` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_XMP: IfdTable = IfdTable {
    module: "XMP",
    table: "xmp",
    group0: "XMP",
    group1: "XMP-xmp",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 19)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::xmpBJ` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_XMPBJ: IfdTable = IfdTable {
    module: "XMP",
    table: "xmpBJ",
    group0: "XMP",
    group1: "XMP-xmpBJ",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 2)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::xmpMM` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_XMPMM: IfdTable = IfdTable {
    module: "XMP",
    table: "xmpMM",
    group0: "XMP",
    group1: "XMP-xmpMM",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 23)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::xmpNote` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_XMPNOTE: IfdTable = IfdTable {
    module: "XMP",
    table: "xmpNote",
    group0: "XMP",
    group1: "XMP-xmpNote",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::xmpRights` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_XMPRIGHTS: IfdTable = IfdTable {
    module: "XMP",
    table: "xmpRights",
    group0: "XMP",
    group1: "XMP-xmpRights",
    group2: "Author",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 5)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::xmpTPg` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_XMPTPG: IfdTable = IfdTable {
    module: "XMP",
    table: "xmpTPg",
    group0: "XMP",
    group1: "XMP-xmpTPg",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 14)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::XMP::xmpTableDefaults` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_XMP_XMPTABLEDEFAULTS: IfdTable = IfdTable {
    module: "XMP",
    table: "xmpTableDefaults",
    group0: "XMP",
    group1: "XMP",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    tags: &[],
    variants: &[],
};
