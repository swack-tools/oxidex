//! ExifTool `WavPack` ProcessBinaryData tables, generated from ExifTool
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

/// `Image::ExifTool::WavPack::Main` -- 5 fields,
/// 0 `_variants` groups (Step 23).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static WAVPACK_MAIN: BinaryTable = BinaryTable {
    module: "WavPack",
    table: "Main",
    group0: "File",
    group1: "File",
    group2: "Audio",
    first_entry: 0,
    default_format: Fmt::Int32u,
    offsets_sound_until: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    fields: &[
        Field {
            index: 6,
            sub: Some(1),
            name: "BytesPerSample",
            format: None,
            count: 1,
            mask: Some(Mask {
                bits: 0x3,
                shift: 0,
            }),
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: Some(ExprId::Val1A3F49A),
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 6,
            sub: Some(2),
            name: "AudioType",
            format: None,
            count: 1,
            mask: Some(Mask {
                bits: 0x4,
                shift: 2,
            }),
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::IntEnum(&[(0, "Stereo"), (1, "Mono")]),
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 6,
            sub: Some(3),
            name: "Compression",
            format: None,
            count: 1,
            mask: Some(Mask {
                bits: 0x8,
                shift: 3,
            }),
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::IntEnum(&[(0, "Lossless"), (1, "Hybrid")]),
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 6,
            sub: Some(4),
            name: "DataFormat",
            format: None,
            count: 1,
            mask: Some(Mask {
                bits: 0x80,
                shift: 7,
            }),
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::IntEnum(&[(0, "Integer"), (1, "Floating Point")]),
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 6,
            sub: Some(5),
            name: "SampleRate",
            format: None,
            count: 1,
            mask: Some(Mask {
                bits: 0x7800000,
                shift: 23,
            }),
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: None,
            print_conv: PrintConv::IntEnum(&[
                (0, "6000"),
                (1, "8000"),
                (2, "9600"),
                (3, "11025"),
                (4, "12000"),
                (5, "16000"),
                (6, "22050"),
                (7, "24000"),
                (8, "32000"),
                (9, "44100"),
                (10, "48000"),
                (11, "64000"),
                (12, "88200"),
                (13, "96000"),
                (14, "192000"),
                (15, "Custom"),
            ]),
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ],
    variants: &[],
};
