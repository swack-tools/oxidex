//! ExifTool `PDF` IFD-style tables, generated from ExifTool
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

/// `Image::ExifTool::PDF::AIPrivate` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_AIPRIVATE: IfdTable = IfdTable {
    module: "PDF",
    table: "AIPrivate",
    group0: "PDF",
    group1: "PDF",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 7)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::AdobePhotoshop` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_ADOBEPHOTOSHOP: IfdTable = IfdTable {
    module: "PDF",
    table: "AdobePhotoshop",
    group0: "PDF",
    group1: "PDF",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::ColorSpace` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_COLORSPACE: IfdTable = IfdTable {
    module: "PDF",
    table: "ColorSpace",
    group0: "PDF",
    group1: "PDF",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 4)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::DefaultRGB` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_DEFAULTRGB: IfdTable = IfdTable {
    module: "PDF",
    table: "DefaultRGB",
    group0: "PDF",
    group1: "PDF",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::EF` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_EF: IfdTable = IfdTable {
    module: "PDF",
    table: "EF",
    group0: "PDF",
    group1: "PDF",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::Encrypt` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_ENCRYPT: IfdTable = IfdTable {
    module: "PDF",
    table: "Encrypt",
    group0: "PDF",
    group1: "PDF",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 2)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::F` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_F: IfdTable = IfdTable {
    module: "PDF",
    table: "F",
    group0: "PDF",
    group1: "PDF",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::Illustrator` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_ILLUSTRATOR: IfdTable = IfdTable {
    module: "PDF",
    table: "Illustrator",
    group0: "PDF",
    group1: "PDF",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::Im` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_IM: IfdTable = IfdTable {
    module: "PDF",
    table: "Im",
    group0: "PDF",
    group1: "PDF",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 5)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::Info` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_INFO: IfdTable = IfdTable {
    module: "PDF",
    table: "Info",
    group0: "PDF",
    group1: "PDF",
    group2: "Document",
    set_group1: None,
    priority: Some(0),
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 12)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::Kids` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_KIDS: IfdTable = IfdTable {
    module: "PDF",
    table: "Kids",
    group0: "PDF",
    group1: "PDF",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 4)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::MC` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_MC: IfdTable = IfdTable {
    module: "PDF",
    table: "MC",
    group0: "PDF",
    group1: "PDF",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::Main` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_MAIN: IfdTable = IfdTable {
    module: "PDF",
    table: "Main",
    group0: "PDF",
    group1: "PDF",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 3)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::MarkInfo` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_MARKINFO: IfdTable = IfdTable {
    module: "PDF",
    table: "MarkInfo",
    group0: "PDF",
    group1: "PDF",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::Metadata` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_METADATA: IfdTable = IfdTable {
    module: "PDF",
    table: "Metadata",
    group0: "PDF",
    group1: "PDF",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::Pages` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_PAGES: IfdTable = IfdTable {
    module: "PDF",
    table: "Pages",
    group0: "PDF",
    group1: "PDF",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 3)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::Perms` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_PERMS: IfdTable = IfdTable {
    module: "PDF",
    table: "Perms",
    group0: "PDF",
    group1: "PDF",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 3)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::PieceInfo` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_PIECEINFO: IfdTable = IfdTable {
    module: "PDF",
    table: "PieceInfo",
    group0: "PDF",
    group1: "PDF",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 2)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::Private` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_PRIVATE: IfdTable = IfdTable {
    module: "PDF",
    table: "Private",
    group0: "PDF",
    group1: "PDF",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::Properties` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_PROPERTIES: IfdTable = IfdTable {
    module: "PDF",
    table: "Properties",
    group0: "PDF",
    group1: "PDF",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 2)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::Reference` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_REFERENCE: IfdTable = IfdTable {
    module: "PDF",
    table: "Reference",
    group0: "PDF",
    group1: "PDF",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::Resources` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_RESOURCES: IfdTable = IfdTable {
    module: "PDF",
    table: "Resources",
    group0: "PDF",
    group1: "PDF",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 3)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::Root` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_ROOT: IfdTable = IfdTable {
    module: "PDF",
    table: "Root",
    group0: "PDF",
    group1: "PDF",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 10)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::Signature` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_SIGNATURE: IfdTable = IfdTable {
    module: "PDF",
    table: "Signature",
    group0: "PDF",
    group1: "PDF",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 8)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::TransformParams` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_TRANSFORMPARAMS: IfdTable = IfdTable {
    module: "PDF",
    table: "TransformParams",
    group0: "PDF",
    group1: "PDF",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 10)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::Unknown` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_UNKNOWN: IfdTable = IfdTable {
    module: "PDF",
    table: "Unknown",
    group0: "PDF",
    group1: "PDF",
    group2: "Unknown",
    set_group1: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::PDF::XObject` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PDF_XOBJECT: IfdTable = IfdTable {
    module: "PDF",
    table: "XObject",
    group0: "PDF",
    group1: "PDF",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 2)],
    },
    tags: &[],
    variants: &[],
};
