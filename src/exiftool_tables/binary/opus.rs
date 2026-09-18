//! ExifTool `Opus` ProcessBinaryData tables, generated from ExifTool
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

/// `Image::ExifTool::Opus::Header` -- 4 fields,
/// 0 `_variants` groups (Step 23).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static OPUS_HEADER: BinaryTable = BinaryTable {
    module: "Opus",
    table: "Header",
    group0: "Opus",
    group1: "Opus",
    group2: "Audio",
    first_entry: 0,
    default_format: Fmt::Int8u,
    offsets_sound_until: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    fields: &[
        Field {
            index: 0,
            sub: None,
            name: "OpusVersion",
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
            name: "AudioChannels",
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
            index: 4,
            sub: None,
            name: "SampleRate",
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
            index: 8,
            sub: None,
            name: "OutputGain",
            format: Some(Fmt::Int16u),
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: Some(ExprId::E10Val512012548B),
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ],
    variants: &[],
};
