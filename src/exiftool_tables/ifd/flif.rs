//! ExifTool `FLIF` IFD-style tables, generated from ExifTool
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

/// `Image::ExifTool::FLIF::Main` -- 6 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_FLIF_MAIN: IfdTable = IfdTable {
    module: "FLIF",
    table: "Main",
    group0: "File",
    group1: "File",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 3)],
    },
    tags: &[
        IfdTag {
            id: 0x0000,
            name: "ImageType",
            format: None,
            count: None,
            writable: None,
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
            condition: None,
            omitted: Omitted::NONE,
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::StrEnum(&[
                ("1", "Grayscale (non-interlaced)"),
                ("3", "RGB (non-interlaced)"),
                ("4", "RGBA (non-interlaced)"),
                ("A", "Grayscale (interlaced)"),
                ("C", "RGB (interlaced)"),
                ("D", "RGBA (interlaced)"),
                ("Q", "Grayscale Animation (non-interlaced)"),
                ("S", "RGB Animation (non-interlaced)"),
                ("T", "RGBA Animation (non-interlaced)"),
                ("a", "Grayscale Animation (interlaced)"),
                ("c", "RGB Animation (interlaced)"),
                ("d", "RGBA Animation (interlaced)"),
            ]),
            subdir: None,
        },
        IfdTag {
            id: 0x0001,
            name: "BitDepth",
            format: None,
            count: None,
            writable: None,
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
            condition: None,
            omitted: Omitted::NONE,
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::IntEnum(&[(0, "Custom"), (1, "8"), (2, "16")]),
            subdir: None,
        },
        IfdTag {
            id: 0x0002,
            name: "ImageWidth",
            format: None,
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
            id: 0x0003,
            name: "ImageHeight",
            format: None,
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
            id: 0x0004,
            name: "AnimationFrames",
            format: None,
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
            id: 0x0005,
            name: "Encoding",
            format: None,
            count: None,
            writable: None,
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
            condition: None,
            omitted: Omitted::NONE,
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::IntEnum(&[(0, "FLIF16")]),
            subdir: None,
        },
    ],
    variants: &[],
};
