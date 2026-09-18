//! ExifTool `VCard` IFD-style tables, generated from ExifTool
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

/// `Image::ExifTool::VCard::Main` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_VCARD_MAIN: IfdTable = IfdTable {
    module: "VCard",
    table: "Main",
    group0: "VCard",
    group1: "VCard",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 31)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::VCard::VCalendar` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_VCARD_VCALENDAR: IfdTable = IfdTable {
    module: "VCard",
    table: "VCalendar",
    group0: "VCard",
    group1: "VCalendar",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 65)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::VCard::VNote` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_VCARD_VNOTE: IfdTable = IfdTable {
    module: "VCard",
    table: "VNote",
    group0: "VCard",
    group1: "VNote",
    group2: "Document",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 4)],
    },
    tags: &[],
    variants: &[],
};
