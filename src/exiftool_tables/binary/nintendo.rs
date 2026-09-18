//! ExifTool `Nintendo` ProcessBinaryData tables, generated from ExifTool
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

/// `Image::ExifTool::Nintendo::CameraInfo` -- 5 fields,
/// 0 `_variants` groups (Step 23).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static NINTENDO_CAMERAINFO: BinaryTable = BinaryTable {
    module: "Nintendo",
    table: "CameraInfo",
    group0: "MakerNotes",
    group1: "Nintendo",
    group2: "Image",
    first_entry: 0,
    default_format: Fmt::Int8u,
    offsets_sound_until: None,
    priority: Some(0),
    gate_a: GateA { blocked_by: &[] },
    fields: &[
        Field {
            index: 0,
            sub: None,
            name: "ModelID",
            format: Some(Fmt::Undef(4)),
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
            index: 8,
            sub: None,
            name: "TimeStamp",
            format: Some(Fmt::Int32u),
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: Some(ExprId::ConvertUnixTimeVal109572436009A7011),
            print_conv: PrintConv::Expr(ExprId::SelfConvertDateTimeVal7455B8),
            subdir: None,
            hook: &[],
            groups: TagGroups {
                g0: None,
                g1: None,
                g2: Some("Time"),
            },
        },
        Field {
            index: 24,
            sub: None,
            name: "InternalSerialNumber",
            format: Some(Fmt::Undef(4)),
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: Some(ExprId::E0xUnpackHVal582375),
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups {
                g0: None,
                g1: None,
                g2: Some("Camera"),
            },
        },
        Field {
            index: 40,
            sub: None,
            name: "Parallax",
            format: Some(Fmt::Float),
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::Expr(ExprId::Sprintf2fVal25FBBD),
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 48,
            sub: None,
            name: "Category",
            format: Some(Fmt::Int16u),
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::PartialEnumInt {
                exact: &[
                    (0, "(none)"),
                    (4096, "Mii"),
                    (8192, "Man"),
                    (16384, "Woman"),
                ],
                other: None,
                print_hex: true,
            },
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ],
    variants: &[],
};
