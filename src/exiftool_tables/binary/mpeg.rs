//! ExifTool `MPEG` ProcessBinaryData tables, generated from ExifTool
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

/// `Image::ExifTool::MPEG::Lame` -- 4 fields,
/// 0 `_variants` groups (Step 23).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static MPEG_LAME: BinaryTable = BinaryTable {
    module: "MPEG",
    table: "Lame",
    group0: "MPEG",
    group1: "MPEG",
    group2: "Audio",
    first_entry: 0,
    default_format: Fmt::Int8u,
    offsets_sound_until: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("expr_unsupported", 1)],
    },
    fields: &[
        Field {
            index: 9,
            sub: None,
            name: "LameMethod",
            format: None,
            count: 1,
            mask: Some(Mask {
                bits: 0xf,
                shift: 0,
            }),
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::IntEnum(&[
                (1, "CBR"),
                (2, "ABR"),
                (3, "VBR (old/rh)"),
                (4, "VBR (new/mtrh)"),
                (5, "VBR (old/rh)"),
                (6, "VBR"),
                (8, "CBR (2-pass)"),
                (9, "ABR (2-pass)"),
            ]),
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 10,
            sub: None,
            name: "LameLowPassFilter",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: Some(ExprId::Val1008BA43F),
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 20,
            sub: None,
            name: "LameBitrate",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: Some(ExprId::Val1000BF3024),
            print_conv: PrintConv::Expr(ExprId::ConvertBitrateVal792F69),
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 24,
            sub: None,
            name: "LameStereoMode",
            format: None,
            count: 1,
            mask: Some(Mask {
                bits: 0x1c,
                shift: 2,
            }),
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::IntEnum(&[
                (0, "Mono"),
                (1, "Stereo"),
                (2, "Dual Channels"),
                (3, "Joint Stereo"),
                (4, "Forced Joint Stereo"),
                (6, "Auto"),
                (7, "Intensity Stereo"),
            ]),
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ],
    variants: &[],
};
