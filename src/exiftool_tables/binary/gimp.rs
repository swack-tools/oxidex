//! ExifTool `GIMP` ProcessBinaryData tables, generated from ExifTool
//! 13.59's own Perl hashes -- one file per module; `mod.rs` beside this
//! file is the hub that declares and re-exports it.
//!
//! DO NOT EDIT. Regenerate with `just regen-tables` (see `mod.rs`).

#![allow(clippy::unreadable_literal, clippy::too_many_lines, unused_parens)]

// Everything a table literal names -- the schema types, `ExprId`, the
// `cond`/`subdir`/`ifd_schema` imports -- is in scope in the hub, and a glob
// import of the parent module brings its private imports along (RFC 1560).
#[allow(unused_imports)]
use super::*;

/// `Image::ExifTool::GIMP::Header` -- 4 fields,
/// 0 `_variants` groups (Step 23).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static GIMP_HEADER: BinaryTable = BinaryTable {
    module: "GIMP",
    table: "Header",
    group0: "GIMP",
    group1: "GIMP",
    group2: "Image",
    first_entry: 0,
    default_format: Fmt::Int8u,
    offsets_sound_until: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("other_unregistered", 1)],
    },
    fields: &[
        Field {
            index: 9,
            sub: None,
            name: "XCFVersion",
            format: Some(Fmt::Str(5)),
            count: 1,
            mask: None,
            condition: None,
            raw_conv: Some(RawConvEffect::SetMember {
                member: "XCFVersion",
            }),
            omitted: Omitted {
                value_conv: false,
                raw_conv: true,
                condition: false,
                hook: false,
                subdirectory: false,
                print_conv: false,
            },
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 14,
            sub: None,
            name: "ImageWidth",
            format: Some(Fmt::Int32u),
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 18,
            sub: None,
            name: "ImageHeight",
            format: Some(Fmt::Int32u),
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 22,
            sub: None,
            name: "ColorMode",
            format: Some(Fmt::Int32u),
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::IntEnum(&[
                (0, "RGB Color"),
                (1, "Grayscale"),
                (2, "Indexed Color"),
            ]),
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ],
    variants: &[],
};

/// `Image::ExifTool::GIMP::Resolution` -- 2 fields,
/// 0 `_variants` groups (Step 23).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static GIMP_RESOLUTION: BinaryTable = BinaryTable {
    module: "GIMP",
    table: "Resolution",
    group0: "GIMP",
    group1: "GIMP",
    group2: "Image",
    first_entry: 0,
    default_format: Fmt::Float,
    offsets_sound_until: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    fields: &[
        Field {
            index: 0,
            sub: None,
            name: "XResolution",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 1,
            sub: None,
            name: "YResolution",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ],
    variants: &[],
};
