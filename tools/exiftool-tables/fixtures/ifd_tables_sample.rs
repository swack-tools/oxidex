//! A hand-written sample of `src/exiftool_tables/ifd_tables.rs` in EXACTLY
//! the shape `docs/superpowers/specs/2026-09-06-ifd-tables-design.md`
//! section 2 specifies for the generator (one `pub static IFD_<MODULE>_<TABLE>:
//! IfdTable` per table, `IfdTag` members in `ifd_schema.rs` order, then
//! `ALL_IFD_TABLES`), after `rustfmt`.
//!
//! It exists so `tools/exiftool-tables/verify.py`'s IFD stage (and
//! `reachability.py`'s IFD census) can be developed and unit-tested
//! (`test_verify_ifd.py`) before the generator's own output lands, and so
//! the parser is exercised against every literal shape the schema allows:
//! `Fmt::Str(N)`/scalar formats, `TagGroups {}` overrides, every `IfdFlags`
//! member, `Omitted {}` flags, `RawConvEffect::SetMember`, a `value_conv`
//! `ExprId`, `IntEnum`/`StrEnum`/`Bitmask` print conversions, an
//! `IfdSubdirEdge` with a `BaseExpr`, `IfdStart::Val`, `MaxSubdirs`/`DirName`,
//! a refused edge, and an `IfdVariantGroup`.
//!
//! Every TAG FACT below is transcribed from the pinned 13.59 tree (the
//! oracle rows for these keys), so the whole file also verifies clean
//! against the real `oracle.pl` output:
//!
//! ```sh
//! python3 tools/exiftool-tables/verify.py src/exiftool_tables/binary_tables.rs <lib> \
//!     --oracle tools/exiftool-tables/oracle.pl \
//!     --ifd-generated tools/exiftool-tables/fixtures/ifd_tables_sample.rs
//! ```
//!
//! Two things are NOT facts and are placeholders: the `Cond` values in the
//! variant group (`verify_cond.py` is the instrument for conditions;
//! `verify.py` never compares them) and the one `value_conv: Some(ExprId::..)`
//! (`verify_exprs.py` owns conversions), both present only so the parser
//! meets the shape. This file is not compiled into the crate.

#![allow(clippy::unreadable_literal, clippy::too_many_lines, unused_parens)]

#[allow(unused_imports)]
use super::cond::{CmpOp, Cond};
#[allow(unused_imports)]
use super::ifd_schema::{
    IfdByteOrder, IfdFlags, IfdStart, IfdSubdirEdge, IfdTable, IfdTag, IfdVariantGroup,
    RawConvEffect,
};
#[allow(unused_imports)]
use super::subdir::BaseExpr;
#[allow(unused_imports)]
use super::{ExprId, Fmt, GateA, Omitted, PrintConv, TagGroups};

pub const EXIFTOOL_VERSION: &str = "13.59";

pub static IFD_EXIF_MAIN: IfdTable = IfdTable {
    module: "Exif",
    table: "Main",
    group0: "EXIF",
    group1: "IFD0",
    group2: "Image",
    set_group1: Some("1"),
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    tags: &[
        IfdTag {
            id: 254,
            name: "SubfileType",
            format: None,
            count: None,
            writable: Some("int32u"),
            groups: TagGroups::NONE,
            flags: IfdFlags {
                unknown: false,
                binary: false,
                list: false,
                protected: true,
                avoid: false,
                priority: None,
            },
            omitted: Omitted {
                value_conv: false,
                raw_conv: true,
                condition: false,
                hook: false,
                subdirectory: false,
                print_conv: false,
            },
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::Bitmask {
                exact: &[
                    (0, "Full-resolution image"),
                    (1, "Reduced-resolution image"),
                    (2, "Single page of multi-page image"),
                    (3, "Single page of multi-page reduced-resolution image"),
                    (4, "Transparency mask"),
                    (5, "Transparency mask of reduced-resolution image"),
                    (6, "Transparency mask of multi-page image"),
                    (
                        7,
                        "Transparency mask of reduced-resolution multi-page image",
                    ),
                    (8, "Depth map"),
                    (9, "Depth map of reduced-resolution image"),
                    (16, "Enhanced image data"),
                    (65537, "Alternate reduced-resolution image"),
                    (65540, "Semantic Mask"),
                    (4294967295, "invalid"),
                ],
                bits: &[
                    (0, "Reduced resolution"),
                    (1, "Single page"),
                    (2, "Transparency mask"),
                    (3, "TIFF/IT final page"),
                    (4, "TIFF-FX mixed raster content"),
                ],
            },
            subdir: None,
        },
        IfdTag {
            id: 33424,
            name: "KodakIFD",
            format: None,
            count: None,
            writable: None,
            groups: TagGroups {
                g0: None,
                g1: Some("KodakIFD"),
                g2: None,
            },
            flags: IfdFlags::NONE,
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
                module: "Kodak",
                table: "IFD",
                start: IfdStart::Val(0),
                base: None,
                byte_order: IfdByteOrder::Inherit,
                fix_format: None,
                sub_ifd: true,
                max_subdirs: Some(1),
                dir_name: Some("KodakIFD"),
                validate: false,
            }),
        },
        IfdTag {
            id: 33723,
            name: "IPTC-NAA",
            format: None,
            count: None,
            writable: Some("int32u"),
            groups: TagGroups::NONE,
            flags: IfdFlags {
                unknown: false,
                binary: true,
                list: false,
                protected: true,
                avoid: false,
                priority: None,
            },
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
                module: "IPTC",
                table: "Main",
                start: IfdStart::ValuePtr(0),
                base: None,
                byte_order: IfdByteOrder::Inherit,
                fix_format: None,
                sub_ifd: false,
                max_subdirs: None,
                dir_name: Some("IPTC"),
                validate: false,
            }),
        },
        IfdTag {
            id: 34665,
            name: "ExifOffset",
            format: None,
            count: None,
            writable: None,
            groups: TagGroups {
                g0: None,
                g1: Some("ExifIFD"),
                g2: None,
            },
            flags: IfdFlags::NONE,
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
        IfdTag {
            id: 41985,
            name: "CustomRendered",
            format: None,
            count: None,
            writable: Some("int16u"),
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
            omitted: Omitted::NONE,
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::IntEnum(&[
                (0, "Normal"),
                (1, "Custom"),
                (2, "HDR (no original saved)"),
            ]),
            subdir: None,
        },
        IfdTag {
            id: 50706,
            name: "DNGVersion",
            format: None,
            count: Some(4),
            writable: Some("int8u"),
            groups: TagGroups::NONE,
            flags: IfdFlags {
                unknown: false,
                binary: false,
                list: false,
                protected: true,
                avoid: false,
                priority: None,
            },
            omitted: Omitted::NONE,
            raw_conv: Some(RawConvEffect::SetMember {
                member: "DNGVersion",
            }),
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
        },
        IfdTag {
            id: 51157,
            name: "NikonNEFInfo",
            format: None,
            count: None,
            writable: None,
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
            omitted: Omitted {
                value_conv: false,
                raw_conv: false,
                condition: true,
                hook: false,
                subdirectory: true,
                print_conv: false,
            },
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: Some(IfdSubdirEdge {
                module: "Nikon",
                table: "NEFInfo",
                start: IfdStart::ValuePtr(18),
                base: Some(&BaseExpr::Sub(&BaseExpr::Start, &BaseExpr::Const(8))),
                byte_order: IfdByteOrder::Unknown,
                fix_format: None,
                sub_ifd: false,
                max_subdirs: None,
                dir_name: None,
                validate: false,
            }),
        },
    ],
    variants: &[],
};

pub static IFD_FLIR_MAIN: IfdTable = IfdTable {
    module: "FLIR",
    table: "Main",
    group0: "MakerNotes",
    group1: "FLIR",
    group2: "Camera",
    set_group1: None,
    priority: Some(0),
    gate_a: GateA { blocked_by: &[] },
    tags: &[IfdTag {
        id: 1,
        name: "ImageTemperatureMax",
        format: Some(Fmt::Rational64s),
        count: None,
        writable: Some("rational64u"),
        groups: TagGroups::NONE,
        flags: IfdFlags::NONE,
        omitted: Omitted::NONE,
        raw_conv: None,
        value_conv: Some(ExprId::Val1000F42638),
        print_conv: PrintConv::None,
        subdir: None,
    }],
    variants: &[],
};

pub static IFD_OLYMPUS_EQUIPMENT: IfdTable = IfdTable {
    module: "Olympus",
    table: "Equipment",
    group0: "MakerNotes",
    group1: "Olympus",
    group2: "Camera",
    set_group1: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    tags: &[
        IfdTag {
            id: 0,
            name: "EquipmentVersion",
            format: None,
            count: Some(4),
            writable: Some("undef"),
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
            omitted: Omitted {
                value_conv: false,
                raw_conv: true,
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
            id: 256,
            name: "CameraType2",
            format: None,
            count: Some(6),
            writable: Some("string"),
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
            omitted: Omitted::NONE,
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::StrEnum(&[
                ("D4028", "X-2,C-50Z"),
                ("D4029", "E-20,E-20N,E-20P"),
                ("D4040", "E-1"),
            ]),
            subdir: None,
        },
    ],
    variants: &[],
};

pub static IFD_OLYMPUS_MAIN: IfdTable = IfdTable {
    module: "Olympus",
    table: "Main",
    group0: "MakerNotes",
    group1: "Olympus",
    group2: "Camera",
    set_group1: None,
    priority: None,
    gate_a: GateA {
        blocked_by: &[("ifd_expr_domain_unknown", 2)],
    },
    tags: &[
        IfdTag {
            id: 1,
            name: "MinoltaCameraSettingsOld",
            format: None,
            count: None,
            writable: None,
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
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
                module: "Minolta",
                table: "CameraSettings",
                start: IfdStart::ValuePtr(0),
                base: None,
                byte_order: IfdByteOrder::Big,
                fix_format: None,
                sub_ifd: false,
                max_subdirs: None,
                dir_name: None,
                validate: false,
            }),
        },
        IfdTag {
            id: 256,
            name: "ThumbnailImage",
            format: None,
            count: None,
            writable: Some("undef"),
            groups: TagGroups {
                g0: None,
                g1: None,
                g2: Some("Preview"),
            },
            flags: IfdFlags {
                unknown: false,
                binary: true,
                list: false,
                protected: false,
                avoid: false,
                priority: None,
            },
            omitted: Omitted::NONE,
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
        },
        IfdTag {
            id: 512,
            name: "SpecialMode",
            format: None,
            count: Some(3),
            writable: Some("int32u"),
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
            omitted: Omitted {
                value_conv: false,
                raw_conv: false,
                condition: false,
                hook: false,
                subdirectory: false,
                print_conv: true,
            },
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
        },
        IfdTag {
            id: 519,
            name: "CameraType",
            format: None,
            count: None,
            writable: Some("string"),
            groups: TagGroups::NONE,
            flags: IfdFlags {
                unknown: false,
                binary: false,
                list: false,
                protected: false,
                avoid: false,
                priority: Some(0),
            },
            omitted: Omitted {
                value_conv: false,
                raw_conv: true,
                condition: true,
                hook: false,
                subdirectory: false,
                print_conv: false,
            },
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::StrEnum(&[("D4028", "X-2,C-50Z"), ("D4040", "E-1")]),
            subdir: None,
        },
    ],
    variants: &[IfdVariantGroup {
        id: 8208,
        alternatives: &[
            (
                Cond::And(
                    &Cond::FormatEq { value: "ifd" },
                    &Cond::CountCmp {
                        op: CmpOp::Gt,
                        value: 0,
                    },
                ),
                IfdTag {
                    id: 8208,
                    name: "Equipment",
                    format: None,
                    count: None,
                    writable: None,
                    groups: TagGroups::NONE,
                    flags: IfdFlags::NONE,
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
                        module: "Olympus",
                        table: "Equipment",
                        start: IfdStart::ValuePtr(0),
                        base: None,
                        byte_order: IfdByteOrder::Unknown,
                        fix_format: None,
                        sub_ifd: false,
                        max_subdirs: None,
                        dir_name: None,
                        validate: false,
                    }),
                },
            ),
            (
                Cond::Always,
                IfdTag {
                    id: 8208,
                    name: "EquipmentIFD",
                    format: None,
                    count: None,
                    writable: None,
                    groups: TagGroups {
                        g0: None,
                        g1: Some("MakerNotes"),
                        g2: None,
                    },
                    flags: IfdFlags::NONE,
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
                        module: "Olympus",
                        table: "Equipment",
                        start: IfdStart::Val(0),
                        base: None,
                        byte_order: IfdByteOrder::Inherit,
                        fix_format: None,
                        sub_ifd: true,
                        max_subdirs: None,
                        dir_name: None,
                        validate: false,
                    }),
                },
            ),
        ],
    }],
};

pub static ALL_IFD_TABLES: &[&IfdTable] = &[
    &IFD_EXIF_MAIN,
    &IFD_FLIR_MAIN,
    &IFD_OLYMPUS_EQUIPMENT,
    &IFD_OLYMPUS_MAIN,
];
