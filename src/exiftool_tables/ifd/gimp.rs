//! ExifTool `GIMP` IFD-style tables, generated from ExifTool
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

/// `Image::ExifTool::GIMP::Main` -- 5 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_GIMP_MAIN: IfdTable = IfdTable {
    module: "GIMP",
    table: "Main",
    group0: "GIMP",
    group1: "GIMP",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[
        IfdTag {
            id: 0x0011,
            name: "Compression",
            format: Some(Fmt::Int8u),
            count: None,
            writable: None,
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
            condition: None,
            omitted: Omitted::NONE,
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::IntEnum(&[
                (0, "None"),
                (1, "RLE Encoding"),
                (2, "Zlib"),
                (3, "Fractal"),
            ]),
            subdir: None,
        },
        IfdTag {
            id: 0x0013,
            name: "Resolution",
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
                module: "GIMP",
                table: "Resolution",
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
            id: 0x0014,
            name: "Tattoo",
            format: Some(Fmt::Int32u),
            count: None,
            writable: None,
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
            condition: None,
            omitted: Omitted::NONE,
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
        },
        IfdTag {
            id: 0x0015,
            name: "Parasites",
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
                module: "GIMP",
                table: "Parasite",
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
            id: 0x0016,
            name: "Units",
            format: Some(Fmt::Int32u),
            count: None,
            writable: None,
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
            condition: None,
            omitted: Omitted::NONE,
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::IntEnum(&[
                (1, "Inches"),
                (2, "mm"),
                (3, "Points"),
                (4, "Picas"),
            ]),
            subdir: None,
        },
    ],
    variants: &[],
};
