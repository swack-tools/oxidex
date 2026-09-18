//! ExifTool `PCAP` IFD-style tables, generated from ExifTool
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

/// `Image::ExifTool::PCAP::Main` -- 5 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_PCAP_MAIN: IfdTable = IfdTable {
    module: "PCAP",
    table: "Main",
    group0: "File",
    group1: "File",
    group2: "Other",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_tag_id_unrepresentable", 24)],
    },
    tags: &[
        IfdTag {
            id: 0x0001,
            name: "Comment",
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
            id: 0x0bac,
            name: "CustomOption1",
            format: None,
            count: None,
            writable: None,
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
            condition: None,
            omitted: Omitted {
                value_conv: true,
                raw_conv: false,
                condition: false,
                hook: false,
                subdirectory: false,
                print_conv: false,
            },
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
        },
        IfdTag {
            id: 0x0bad,
            name: "CustomOption2",
            format: None,
            count: None,
            writable: None,
            groups: TagGroups::NONE,
            flags: IfdFlags {
                unknown: false,
                binary: true,
                list: false,
                protected: false,
                avoid: false,
                priority: None,
            },
            condition: None,
            omitted: Omitted::NONE,
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
        },
        IfdTag {
            id: 0x4bac,
            name: "CustomOption3",
            format: None,
            count: None,
            writable: None,
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
            condition: None,
            omitted: Omitted {
                value_conv: true,
                raw_conv: false,
                condition: false,
                hook: false,
                subdirectory: false,
                print_conv: false,
            },
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
        },
        IfdTag {
            id: 0x4bad,
            name: "CustomOption4",
            format: None,
            count: None,
            writable: None,
            groups: TagGroups::NONE,
            flags: IfdFlags {
                unknown: false,
                binary: true,
                list: false,
                protected: false,
                avoid: false,
                priority: None,
            },
            condition: None,
            omitted: Omitted::NONE,
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
        },
    ],
    variants: &[],
};
