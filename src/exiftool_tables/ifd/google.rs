//! ExifTool `Google` IFD-style tables, generated from ExifTool
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

/// `Image::ExifTool::Google::Device` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_GOOGLE_DEVICE: IfdTable = IfdTable {
    module: "Google",
    table: "Device",
    group0: "XMP",
    group1: "XMP-Device",
    group2: "Camera",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 8)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::Google::GAudio` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_GOOGLE_GAUDIO: IfdTable = IfdTable {
    module: "Google",
    table: "GAudio",
    group0: "XMP",
    group1: "XMP-GAudio",
    group2: "Audio",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 2)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::Google::GCamera` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_GOOGLE_GCAMERA: IfdTable = IfdTable {
    module: "Google",
    table: "GCamera",
    group0: "XMP",
    group1: "XMP-GCamera",
    group2: "Camera",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 18)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::Google::GContainer` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_GOOGLE_GCONTAINER: IfdTable = IfdTable {
    module: "Google",
    table: "GContainer",
    group0: "XMP",
    group1: "XMP-GContainer",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::Google::GCreations` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_GOOGLE_GCREATIONS: IfdTable = IfdTable {
    module: "Google",
    table: "GCreations",
    group0: "XMP",
    group1: "XMP-GCreations",
    group2: "Camera",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 2)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::Google::GDepth` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_GOOGLE_GDEPTH: IfdTable = IfdTable {
    module: "Google",
    table: "GDepth",
    group0: "XMP",
    group1: "XMP-GDepth",
    group2: "Image",
    set_group1: None,
    priority: Some(0),
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 14)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::Google::GFocus` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_GOOGLE_GFOCUS: IfdTable = IfdTable {
    module: "Google",
    table: "GFocus",
    group0: "XMP",
    group1: "XMP-GFocus",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 4)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::Google::GImage` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_GOOGLE_GIMAGE: IfdTable = IfdTable {
    module: "Google",
    table: "GImage",
    group0: "XMP",
    group1: "XMP-GImage",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 2)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::Google::GPano` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_GOOGLE_GPANO: IfdTable = IfdTable {
    module: "Google",
    table: "GPano",
    group0: "XMP",
    group1: "XMP-GPano",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 27)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::Google::GSpherical` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_GOOGLE_GSPHERICAL: IfdTable = IfdTable {
    module: "Google",
    table: "GSpherical",
    group0: "XMP",
    group1: "XMP-GSpherical",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 16)],
    },
    tags: &[],
    variants: &[],
};
