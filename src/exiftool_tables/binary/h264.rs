//! ExifTool `H264` ProcessBinaryData tables, generated from ExifTool
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

/// `Image::ExifTool::H264::Camera1` -- 5 fields,
/// 0 `_variants` groups (Step 23).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static H264_CAMERA1: BinaryTable = BinaryTable {
    module: "H264",
    table: "Camera1",
    group0: "H264",
    group1: "H264",
    group2: "Camera",
    first_entry: 0,
    default_format: Fmt::Int8u,
    offsets_sound_until: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[
            ("expr_unsupported", 1),
            ("other_unregistered", 1),
            ("tag_bad_index", 1),
        ],
    },
    fields: &[
        Field {
            index: 0,
            sub: None,
            name: "ApertureSetting",
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
            name: "Gain",
            format: None,
            count: 1,
            mask: Some(Mask {
                bits: 0xf,
                shift: 0,
            }),
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: Some(ExprId::Val13349221),
            print_conv: PrintConv::Expr(ExprId::Val42OutOfRangeValDB52FE9F),
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 1,
            sub: Some(1),
            name: "ExposureProgram",
            format: None,
            count: 1,
            mask: Some(Mask {
                bits: 0xf0,
                shift: 4,
            }),
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: Some(ExprId::Val15UndefVal224247),
            print_conv: PrintConv::IntEnum(&[
                (0, "Program AE"),
                (1, "Gain"),
                (2, "Shutter speed priority AE"),
                (3, "Aperture-priority AE"),
                (4, "Manual"),
            ]),
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 2,
            sub: Some(1),
            name: "WhiteBalance",
            format: None,
            count: 1,
            mask: Some(Mask {
                bits: 0xe0,
                shift: 5,
            }),
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: Some(ExprId::Val7UndefValE72356),
            print_conv: PrintConv::IntEnum(&[
                (0, "Auto"),
                (1, "Hold"),
                (2, "1-Push"),
                (3, "Daylight"),
            ]),
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
        Field {
            index: 3,
            sub: None,
            name: "Focus",
            format: None,
            count: 1,
            mask: None,
            condition: None,
            raw_conv: None,
            omitted: Omitted::NONE,
            value_conv: Some(ExprId::Val0xffUndefVal69ED0A),
            print_conv: PrintConv::None,
            subdir: None,
            hook: &[],
            groups: TagGroups::NONE,
        },
    ],
    variants: &[],
};

/// `Image::ExifTool::H264::Camera2` -- 1 fields,
/// 0 `_variants` groups (Step 23).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static H264_CAMERA2: BinaryTable = BinaryTable {
    module: "H264",
    table: "Camera2",
    group0: "H264",
    group1: "H264",
    group2: "Camera",
    first_entry: 0,
    default_format: Fmt::Int8u,
    offsets_sound_until: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("other_unregistered", 1), ("tag_bad_index", 1)],
    },
    fields: &[Field {
        index: 1,
        sub: None,
        name: "ImageStabilization",
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
    }],
    variants: &[],
};

/// `Image::ExifTool::H264::FrameInfo` -- 2 fields,
/// 0 `_variants` groups (Step 23).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static H264_FRAMEINFO: BinaryTable = BinaryTable {
    module: "H264",
    table: "FrameInfo",
    group0: "H264",
    group1: "H264",
    group2: "Video",
    first_entry: 0,
    default_format: Fmt::Int8u,
    offsets_sound_until: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    fields: &[
        Field {
            index: 0,
            sub: None,
            name: "CaptureFrameRate",
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
            name: "VideoFrameRate",
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

/// `Image::ExifTool::H264::MakeModel` -- 1 fields,
/// 0 `_variants` groups (Step 23).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static H264_MAKEMODEL: BinaryTable = BinaryTable {
    module: "H264",
    table: "MakeModel",
    group0: "H264",
    group1: "H264",
    group2: "Camera",
    first_entry: 0,
    default_format: Fmt::Int16u,
    offsets_sound_until: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    fields: &[Field {
        index: 0,
        sub: None,
        name: "Make",
        format: None,
        count: 1,
        mask: None,
        condition: None,
        raw_conv: None,
        omitted: Omitted {
            value_conv: false,
            raw_conv: true,
            condition: false,
            hook: false,
            subdirectory: false,
            print_conv: false,
        },
        value_conv: None,
        print_conv: PrintConv::PartialEnumInt {
            exact: &[
                (259, "Panasonic"),
                (264, "Sony"),
                (4113, "Canon"),
                (4356, "JVC"),
            ],
            other: None,
            print_hex: true,
        },
        subdir: None,
        hook: &[],
        groups: TagGroups::NONE,
    }],
    variants: &[],
};

/// `Image::ExifTool::H264::RecInfo` -- 1 fields,
/// 0 `_variants` groups (Step 23).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static H264_RECINFO: BinaryTable = BinaryTable {
    module: "H264",
    table: "RecInfo",
    group0: "H264",
    group1: "H264",
    group2: "Camera",
    first_entry: 0,
    default_format: Fmt::Int8u,
    offsets_sound_until: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    fields: &[Field {
        index: 0,
        sub: None,
        name: "RecordingMode",
        format: None,
        count: 1,
        mask: None,
        condition: None,
        raw_conv: None,
        omitted: Omitted::NONE,
        value_conv: None,
        print_conv: PrintConv::IntEnum(&[(2, "XP+"), (4, "SP"), (5, "LP"), (6, "FXP"), (7, "MXP")]),
        subdir: None,
        hook: &[],
        groups: TagGroups::NONE,
    }],
    variants: &[],
};

/// `Image::ExifTool::H264::Shutter` -- 1 fields,
/// 0 `_variants` groups (Step 23).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static H264_SHUTTER: BinaryTable = BinaryTable {
    module: "H264",
    table: "Shutter",
    group0: "H264",
    group1: "H264",
    group2: "Image",
    first_entry: 0,
    default_format: Fmt::Int16u,
    offsets_sound_until: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("tag_bad_index", 1)],
    },
    fields: &[Field {
        index: 1,
        sub: Some(1),
        name: "ExposureTime",
        format: None,
        count: 1,
        mask: Some(Mask {
            bits: 0x7fff,
            shift: 0,
        }),
        condition: None,
        raw_conv: None,
        omitted: Omitted {
            value_conv: false,
            raw_conv: true,
            condition: false,
            hook: false,
            subdirectory: false,
            print_conv: false,
        },
        value_conv: Some(ExprId::Val2812533855B),
        print_conv: PrintConv::Expr(ExprId::ImageExifToolExifPrintExposureTimeVal6037F3),
        subdir: None,
        hook: &[],
        groups: TagGroups::NONE,
    }],
    variants: &[],
};
