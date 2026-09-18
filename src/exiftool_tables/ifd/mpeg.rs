//! ExifTool `MPEG` IFD-style tables, generated from ExifTool
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

/// `Image::ExifTool::MPEG::Audio` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_MPEG_AUDIO: IfdTable = IfdTable {
    module: "MPEG",
    table: "Audio",
    group0: "MPEG",
    group1: "MPEG",
    group2: "Audio",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 11)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::MPEG::Composite` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_MPEG_COMPOSITE: IfdTable = IfdTable {
    module: "MPEG",
    table: "Composite",
    group0: "Composite",
    group1: "Composite",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 2)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::MPEG::Video` -- 0 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_MPEG_VIDEO: IfdTable = IfdTable {
    module: "MPEG",
    table: "Video",
    group0: "MPEG",
    group1: "MPEG",
    group2: "Video",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 5)],
    },
    tags: &[],
    variants: &[],
};

/// `Image::ExifTool::MPEG::Xing` -- 7 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_MPEG_XING: IfdTable = IfdTable {
    module: "MPEG",
    table: "Xing",
    group0: "MPEG",
    group1: "MPEG",
    group2: "Audio",
    set_group1: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    tags: &[
        IfdTag {
            id: 0x0001,
            name: "VBRFrames",
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
            id: 0x0002,
            name: "VBRBytes",
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
            name: "VBRScale",
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
            name: "Encoder",
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
            name: "LameVBRQuality",
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
            id: 0x0006,
            name: "LameQuality",
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
            id: 0x0007,
            name: "LameHeader",
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
                module: "MPEG",
                table: "Lame",
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
