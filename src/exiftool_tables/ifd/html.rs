//! ExifTool `HTML` IFD-style tables, generated from ExifTool
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

/// `Image::ExifTool::HTML::Main` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_HTML_MAIN: IfdTable = IfdTable {
    module: "HTML",
    table: "Main",
    group0: "HTML",
    group1: "HTML",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 31)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::HTML::Office` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_HTML_OFFICE: IfdTable = IfdTable {
    module: "HTML",
    table: "Office",
    group0: "HTML",
    group1: "HTML-office",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 21)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::HTML::dc` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_HTML_DC: IfdTable = IfdTable {
    module: "HTML",
    table: "dc",
    group0: "HTML",
    group1: "HTML-dc",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 15)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::HTML::equiv` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_HTML_EQUIV: IfdTable = IfdTable {
    module: "HTML",
    table: "equiv",
    group0: "HTML",
    group1: "HTTP-equiv",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 22)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::HTML::ncc` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_HTML_NCC: IfdTable = IfdTable {
    module: "HTML",
    table: "ncc",
    group0: "HTML",
    group1: "HTML-ncc",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 26)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::HTML::prod` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_HTML_PROD: IfdTable = IfdTable {
    module: "HTML",
    table: "prod",
    group0: "HTML",
    group1: "HTML-prod",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 2)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::HTML::vw96` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_HTML_VW96: IfdTable = IfdTable {
    module: "HTML",
    table: "vw96",
    group0: "HTML",
    group1: "HTML-vw96",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};
