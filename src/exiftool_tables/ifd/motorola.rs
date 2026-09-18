//! ExifTool `Motorola` IFD-style tables, generated from ExifTool
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

/// `Image::ExifTool::Motorola::Main` -- 6 tags,
/// 0 `_variants` groups (IFD-style: PROCESS_PROC absent).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static IFD_MOTOROLA_MAIN: IfdTable = IfdTable {
    module: "Motorola",
    table: "Main",
    group0: "MakerNotes",
    group1: "Motorola",
    group2: "Camera",
    set_group1: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    tags: &[
        IfdTag {
            id: 0x5500,
            name: "BuildNumber",
            format: None,
            count: None,
            writable: Some("string"),
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
            id: 0x5501,
            name: "SerialNumber",
            format: None,
            count: None,
            writable: Some("string"),
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
            id: 0x6420,
            name: "CustomRendered",
            format: None,
            count: None,
            writable: Some("string"),
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
            condition: Some(Cond::FormatEq {
                value: "string",
                negate: false,
            }),
            omitted: Omitted::NONE,
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
        },
        IfdTag {
            id: 0x64d0,
            name: "DriveMode",
            format: None,
            count: None,
            writable: Some("string"),
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
            id: 0x665e,
            name: "Sensor",
            format: None,
            count: None,
            writable: Some("string"),
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
            id: 0x6705,
            name: "ManufactureDate",
            format: None,
            count: None,
            writable: Some("string"),
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
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
