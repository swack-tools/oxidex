//! ExifTool `JPEG` IFD-style tables, generated from ExifTool
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

/// `Image::ExifTool::JPEG::EPPIM` -- 1 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_JPEG_EPPIM: IfdTable = IfdTable {
    module: "JPEG",
    table: "EPPIM",
    group0: "APP6",
    group1: "EPPIM",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    tags: &[IfdTag {
        id: 0xc4a5,
        name: "PrintIM",
        format: None,
        count: None,
        writable: Some("undef"),
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

/// `Image::ExifTool::JPEG::GraphConv` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_JPEG_GRAPHCONV: IfdTable = IfdTable {
    module: "JPEG",
    table: "GraphConv",
    group0: "APP15",
    group1: "GraphConv",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 1)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::JPEG::Main` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_JPEG_MAIN: IfdTable = IfdTable {
    module: "JPEG",
    table: "Main",
    group0: "JPEG",
    group1: "JPEG",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 20)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::JPEG::MediaJukebox` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_JPEG_MEDIAJUKEBOX: IfdTable = IfdTable {
    module: "JPEG",
    table: "MediaJukebox",
    group0: "XML",
    group1: "MediaJukebox",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 9)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::JPEG::SOF` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_JPEG_SOF: IfdTable = IfdTable {
    module: "JPEG",
    table: "SOF",
    group0: "File",
    group1: "File",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 6)],
    },
    tags: &[],
    variants: &[],
};
