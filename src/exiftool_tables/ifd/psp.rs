//! ExifTool `PSP` IFD-style tables, generated from ExifTool
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

/// `Image::ExifTool::PSP::Main` -- 2 tags,
/// 1 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PSP_MAIN: IfdTable = IfdTable {
    module: "PSP",
    table: "Main",
    group0: "PSP",
    group1: "PSP",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[
            ("ifd_subdir_refused_start", 1),
            ("ifd_tag_id_unrepresentable", 1),
        ],
    },
    tags: &[
        IfdTag {
            id: 0x0001,
            name: "CreatorInfo",
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
                module: "PSP",
                table: "Creator",
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
            id: 0x000a,
            name: "ExtendedInfo",
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
                module: "PSP",
                table: "Ext",
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
    variants: &[IfdVariantGroup {
        id: 0x0000,
        alternatives: &[
            (
                Cond::MemberCmp {
                    member: "PSPFileVersion",
                    op: CmpOp::Gt,
                    value: 3,
                },
                IfdTag {
                    id: 0x0000,
                    name: "ImageInfo",
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
                    subdir: None,
                },
            ),
            (
                Cond::Always,
                IfdTag {
                    id: 0x0000,
                    name: "ImageInfo",
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
                        module: "PSP",
                        table: "Image",
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
            ),
        ],
    }],
};
