//! ExifTool `Microsoft` IFD-style tables, generated from ExifTool
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

/// `Image::ExifTool::Microsoft::MP` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_MICROSOFT_MP: IfdTable = IfdTable {
    module: "Microsoft",
    table: "MP",
    group0: "XMP",
    group1: "XMP-MP",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 9)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::Microsoft::MP1` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_MICROSOFT_MP1: IfdTable = IfdTable {
    module: "Microsoft",
    table: "MP1",
    group0: "XMP",
    group1: "XMP-MP1",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 16)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::Microsoft::XMP` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_MICROSOFT_XMP: IfdTable = IfdTable {
    module: "Microsoft",
    table: "XMP",
    group0: "XMP",
    group1: "XMP-microsoft",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 13)],
    },
    tags: &[],
    variants: &[],
};
