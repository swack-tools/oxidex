//! ExifTool `Vorbis` IFD-style tables, generated from ExifTool
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

/// `Image::ExifTool::Vorbis::Composite` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_VORBIS_COMPOSITE: IfdTable = IfdTable {
    module: "Vorbis",
    table: "Composite",
    group0: "Composite",
    group1: "Composite",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::Vorbis::Main` -- 2 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_VORBIS_MAIN: IfdTable = IfdTable {
    module: "Vorbis",
    table: "Main",
    group0: "Vorbis",
    group1: "Vorbis",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    tags: &[
        IfdTag {
            id: 0x0001,
            name: "Identification",
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
                module: "Vorbis",
                table: "Identification",
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
        },
        IfdTag {
            id: 0x0003,
            name: "Comments",
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
                module: "Vorbis",
                table: "Comments",
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
        },
    ],
    variants: &[],
};
