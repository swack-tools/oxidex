//! ExifTool `M2TS` IFD-style tables, generated from ExifTool
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

/// `Image::ExifTool::M2TS::AC3` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_M2TS_AC3: IfdTable = IfdTable {
    module: "M2TS",
    table: "AC3",
    group0: "M2TS",
    group1: "AC3",
    group2: "Audio",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 4)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::M2TS::Main` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_M2TS_MAIN: IfdTable = IfdTable {
    module: "M2TS",
    table: "Main",
    group0: "M2TS",
    group1: "M2TS",
    group2: "Video",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 3)],
    },
    tags: &[],
    variants: &[],
};
