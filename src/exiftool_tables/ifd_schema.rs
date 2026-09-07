//! The schema of a generated IFD-style tag table -- ExifTool's `ProcessExif`
//! tables (EXIF, GPS, most MakerNotes `Main` tables and their IFD-shaped
//! sub-directories), the 495 tables of the pinned 13.59 tree whose
//! `PROCESS_PROC` is absent or `Image::ExifTool::Exif::ProcessExif`.
//!
//! This module is hand-written and stable; `tools/exiftool-tables/codegen.py`
//! emits `ifd_tables.rs` -- one `pub static IFD_<MODULE>_<TABLE>: IfdTable`
//! per table plus `ALL_IFD_TABLES` -- against exactly these types. The
//! binary-table schema (`binary_tables.rs`'s prelude: [`Fmt`], [`Omitted`],
//! [`PrintConv`], [`TagGroups`], [`GateA`], `ExprId`) is reused unchanged:
//! every conversion here is the same oracle-verified `ExprId` / enum the
//! `ProcessBinaryData` tables carry, and `verify_exprs.py`'s census is
//! dump-wide, so an expression is verified once for both table kinds.
//!
//! # What is different from a [`super::BinaryTable`]
//!
//! An IFD entry names its own tag id, format and count (Exif.pm's
//! `ProcessExif`, the 12-byte entry at Exif.pm:6640 ff.), so:
//!
//! * a tag is keyed by `id`, not by a byte index -- `tags` is sorted by id
//!   for binary search (Exif::Main alone has 513 entries);
//! * `format` is an OVERRIDE of the entry's declared type (ExifTool's
//!   `Format => 'int16u'` on an IFD tag reinterprets the entry's bytes; the
//!   byte length still comes from the entry's own type), and `count` is data
//!   the walk does not need (the entry says how many);
//! * the per-tag flags ExifTool reads at report time (`Unknown`, `Binary`,
//!   `List`, `Protected`, `Avoid`, `Priority`) are carried, because for an
//!   IFD table they decide what is reported, not merely how;
//! * a `SubDirectory` edge is an [`IfdSubdirEdge`], NOT a
//!   [`super::SubdirEdge`]: `ProcessExif` evaluates `Start` with
//!   `($valuePtr, $val)` in scope (Exif.pm:6950-6952) where
//!   `ProcessBinaryData` uses `($val, $dirStart)`, and `ByteOrder` /
//!   `FixFormat` / `Flags => 'SubIFD'` are live here and inert there
//!   (`subdir.rs`'s module doc). The two grammars stay two types so neither
//!   can be walked with the other's semantics by accident;
//! * a `RawConv` of the one shape the walk can honour as DATA -- the
//!   data-member capture `$$self{X} = $val` -- is carried as
//!   [`RawConvEffect::SetMember`] instead of withholding the field, since it
//!   is what a later [`Cond`] reads. Every other `RawConv` sets
//!   `omitted.raw_conv` and the field is withheld, as for binary tables.
//!
//! Gate A / Gate B are the same two gates as for binary tables: `gate_a` is
//! computed by `codegen.py` from its refusal counters (empty = every entry
//! transcribed or explicitly refused, nothing silently wrong), and
//! `super::enabled_ifd::ENABLED_IFD` is the measured per-table allowlist a
//! walk consults through [`IfdTable::enabled`].

use super::cond::Cond;
use super::subdir::BaseExpr;
use super::{ExprId, Fmt, GateA, Omitted, PrintConv, TagGroups};

/// One `ProcessExif` (IFD-style) tag table.
#[derive(Clone, Copy, Debug)]
pub struct IfdTable {
    pub module: &'static str,
    pub table: &'static str,
    /// Effective groups after `GetTagTable`'s defaulting (ExifTool.pm:
    /// 8980-8991): declared value, else the module name (group 0/1) or
    /// `"Other"` (group 2). Never empty.
    pub group0: &'static str,
    pub group1: &'static str,
    pub group2: &'static str,
    /// The table's `SET_GROUP1` payload, verbatim (`Some("1")` for every
    /// declaration in 13.59). It is a FLAG, not a name: Exif.pm:7183 sets
    /// family 1 to the directory NAME the table was reached under
    /// (`IFD0`/`IFD1`/`ExifIFD` for Exif::Main, a SubDirectory's `DirName`
    /// for Kodak::IFD), which the walk takes from `IfdDir::group1`; a
    /// `SET_GROUP1` table walked with no directory name is withheld.
    pub set_group1: Option<&'static str>,
    /// The table-level `PRIORITY` (ExifTool.pm:9471): `Some(0)` means a
    /// value from this table never displaces one already reported under the
    /// same name.
    pub priority: Option<i64>,
    /// Static soundness, computed by `codegen.py`; see [`GateA`].
    pub gate_a: GateA,
    /// Sorted by `id`, ids unique, disjoint from `variants`.
    pub tags: &'static [IfdTag],
    /// ExifTool's arrayref-of-alternatives entries (`dump_tables.pl`'s
    /// `_variants`), one group per id, sorted by `id`, first match wins.
    pub variants: &'static [IfdVariantGroup],
}

impl IfdTable {
    /// The tag ExifTool's table declares for `id`, ignoring `_variants`
    /// (use [`IfdTable::variant_group`] and `cond::first_match_ifd` for
    /// those). Binary search over the sorted `tags`.
    #[must_use]
    pub fn tag(&self, id: u16) -> Option<&'static IfdTag> {
        self.tags
            .binary_search_by_key(&id, |t| t.id)
            .ok()
            .map(|i| &self.tags[i])
    }

    /// The `_variants` group for `id`, if the table declares one.
    #[must_use]
    pub fn variant_group(&self, id: u16) -> Option<&'static IfdVariantGroup> {
        self.variants
            .binary_search_by_key(&id, |v| v.id)
            .ok()
            .map(|i| &self.variants[i])
    }

    /// Gate A and Gate B together: whether a walk may report this table.
    #[must_use]
    pub fn enabled(&self) -> bool {
        super::enabled_ifd::is_enabled(self)
    }
}

/// One tag of an [`IfdTable`].
#[derive(Clone, Copy, Debug)]
pub struct IfdTag {
    /// The IFD tag id. ExifTool keys these tables by integer; a key outside
    /// `0..=0xFFFF` is refused by the generator (`ifd_tag_id_unrepresentable`).
    pub id: u16,
    pub name: &'static str,
    /// ExifTool's `Format` on an IFD tag: reinterpret the entry's bytes as
    /// this type (the byte LENGTH still follows the entry's declared type,
    /// Exif.pm:6733-6760). `None` = read as the entry declares. The UNSIZED
    /// `Some(Fmt::Str(0))` / `Some(Fmt::Undef(0))` are a bare `Format =>
    /// 'string'` / `'undef'`: "this kind, the entry's own byte length"
    /// (Exif.pm:6737-6745; `ReadValue` then NUL-truncates `string` only,
    /// ExifTool.pm:6306-6308) -- a live override, so an `undef`-typed
    /// CameraID under `Format => 'string'` reads as text, not bytes.
    pub format: Option<Fmt>,
    /// ExifTool's `Count` (or the `[N]` of `Format => 'int16u[N]'`). Data
    /// for writers and verifiers; the walk reads the entry's own count.
    pub count: Option<u32>,
    /// ExifTool's `Writable` spelling, verbatim (`"int16u"`, `"string"`,
    /// `"undef"`, ...). Data: the value domain the generator assumed for the
    /// conversions when no `Format` override was declared.
    pub writable: Option<&'static str>,
    pub groups: TagGroups,
    pub flags: IfdFlags,
    /// Which of ExifTool's per-tag semantics the generator did NOT reproduce.
    /// Any flag set means the entry's decoded value is not what ExifTool
    /// reports and the walk withholds it (AGENTS.md: omit and count).
    pub omitted: Omitted,
    /// The one `RawConv` shape carried as data instead of refused.
    pub raw_conv: Option<RawConvEffect>,
    pub value_conv: Option<ExprId>,
    pub print_conv: PrintConv,
    /// `SubDirectory`: the entry's value is a pointer (or the bytes) of a
    /// nested directory, never a reported value.
    pub subdir: Option<IfdSubdirEdge>,
}

/// The per-tag flags ExifTool consults when reporting an IFD tag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfdFlags {
    /// `Unknown => 1`: reported only under `-u`. Carried as a flag (never a
    /// drop) so an unknown-by-design tag is distinguishable from one the
    /// generator failed to transcribe.
    pub unknown: bool,
    /// `Binary => 1`: ExifTool prints `(Binary data N bytes, use -b option
    /// to extract)` unless `-b`.
    pub binary: bool,
    /// `List => 1`: the value is a list, reported as such (`-j` emits a
    /// JSON array) rather than ExifTool's default space-joined string.
    pub list: bool,
    /// `Protected => 1` (write-side policy; data).
    pub protected: bool,
    /// `Avoid => 1`: yield to a same-named tag from another table.
    pub avoid: bool,
    /// Per-tag `Priority` (overrides the table's).
    pub priority: Option<i64>,
}

impl IfdFlags {
    pub const NONE: Self = Self {
        unknown: false,
        binary: false,
        list: false,
        protected: false,
        avoid: false,
        priority: None,
    };
}

/// A `RawConv` the walk honours as data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawConvEffect {
    /// `$$self{Member} = $val`: store the raw value as a data member for
    /// later `Condition`s and keep reporting the tag (ExifTool returns the
    /// assignment's value, i.e. `$val`). `verify_cond.py`'s member grammar
    /// is the reader; the walk writes `MemberValue::Num` for a numeric raw
    /// value and `MemberValue::Str` otherwise.
    SetMember { member: &'static str },
}

/// One id whose entry is a Perl arrayref of alternatives.
#[derive(Clone, Copy, Debug)]
pub struct IfdVariantGroup {
    pub id: u16,
    /// First alternative whose [`Cond`] holds wins (`GetTagInfo`,
    /// ExifTool.pm). Compiled atomically: a group with any alternative the
    /// generator cannot express is refused whole (`tag_variant_skipped`),
    /// because dropping one alternative changes first-match order.
    pub alternatives: &'static [(Cond, IfdTag)],
}

/// How `ProcessExif` computes a `SubDirectory`'s start (Exif.pm:6950-6952:
/// `#### eval Start ($valuePtr, $val)`). `$valuePtr` is the absolute
/// position of the entry's value bytes (inline in the entry when the value
/// is 4 bytes or fewer, else at the entry's offset + base); `$val` is the
/// entry's decoded value, i.e. a pointer for the `Flags => 'SubIFD'` /
/// `int32u` alternatives. Only these two spellings, with an optional integer
/// offset, are compiled; anything else is refused (`ifd_subdir_refused_start`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IfdStart {
    /// `Start` absent, or `'$valuePtr'`, or `'$valuePtr + n'`.
    ValuePtr(i64),
    /// `'$val'` or `'$val + n'`.
    Val(i64),
}

/// `SubDirectory.ByteOrder` as `ProcessExif` reads it (Exif.pm:6974-6990).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IfdByteOrder {
    /// Absent: the sub-directory uses the enclosing directory's byte order.
    Inherit,
    /// `'LittleEndian'` / `'II'`.
    Little,
    /// `'BigEndian'` / `'MM'`.
    Big,
    /// `'Unknown'`: detect from the sub-directory's own entry count
    /// (Exif.pm:6981-6989 tries the enclosing order first and flips it when
    /// the count is implausible).
    Unknown,
}

/// One `SubDirectory` edge out of an IFD tag.
#[derive(Clone, Copy, Debug)]
pub struct IfdSubdirEdge {
    /// `SubDirectory.TagTable` split as `find_table` / `find_ifd_table` key
    /// it. The target may be an [`IfdTable`] (walked recursively) or a
    /// [`super::BinaryTable`] (handed to `process_binary_data`); the edge
    /// only says where the pointer leads.
    pub module: &'static str,
    pub table: &'static str,
    pub start: IfdStart,
    /// `SubDirectory.Base`, in `ProcessExif`'s eval scope (`$start`, `$base`)
    /// -- the same restricted arithmetic as [`BaseExpr`]. `None` = inherit.
    pub base: Option<&'static BaseExpr>,
    pub byte_order: IfdByteOrder,
    /// `FixFormat`, carried as DATA only: ExifTool consults it when
    /// WRITING (WriteExif.pl:1760, to correct a mis-typed pointer entry);
    /// `ProcessExif` never reinterprets an entry on it, and neither does the
    /// walk. `FixFormat => 'ifd'` is spelled `sub_ifd: true` instead.
    pub fix_format: Option<Fmt>,
    /// `Flags => 'SubIFD'`: the value is an offset to a sub-IFD (Exif.pm's
    /// SubIFD loop; `$val` may hold several offsets).
    pub sub_ifd: bool,
    /// `MaxSubdirs`, when declared: an upper bound on the SubIFD offsets
    /// the walk follows.
    pub max_subdirs: Option<u32>,
    /// `DirName`, when declared (the family-1 name the sub-directory reports
    /// under, e.g. `Olympus2`).
    pub dir_name: Option<&'static str>,
    /// `Validate` declared: ExifTool evaluates Perl against the directory
    /// bytes before walking it. Not compiled -- an edge that carries it is
    /// emitted for the reachability census but never walked
    /// (`ifd_subdir_refused_validate`).
    pub validate: bool,
}
