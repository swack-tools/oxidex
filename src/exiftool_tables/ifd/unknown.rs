//! ExifTool `Unknown` IFD-style tables, generated from ExifTool
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

/// `Image::ExifTool::Unknown::Main` -- 1 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_UNKNOWN_MAIN: IfdTable = IfdTable {
    module: "Unknown",
    table: "Main",
    group0: "MakerNotes",
    group1: "MakerUnknown",
    group2: "Camera",
    set_group1: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    tags: &[IfdTag {
        id: 0x0e00,
        name: "PrintIM",
        format: None,
        count: None,
        writable: None,
        groups: TagGroups::NONE,
        flags: IfdFlags::NONE,
        condition: None,
        omitted: Omitted {
            value_conv: false,
            raw_conv: false,
            condition: false,
            hook: false,
            subdirectory: true,
            print_conv: false,
        },
        raw_conv: None,
        value_conv: None,
        print_conv: PrintConv::None,
        subdir: Some(IfdSubdirEdge {
            module: "PrintIM",
            table: "Main",
            start: IfdStart::ValuePtr(0),
            base: None,
            byte_order: IfdByteOrder::Inherit,
            fix_format: None,
            sub_ifd: false,
            max_subdirs: None,
            dir_name: None,
            validate: false,
            validation: None,
            processor: IfdSubdirProcessor::Native,
            unwalked: None,
        }),
    }],
    variants: &[],
};
