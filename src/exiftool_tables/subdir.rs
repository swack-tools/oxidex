//! `SubdirEdge`: the compiled form of a binary-table field's `SubDirectory`
//! pointer, over a closed grammar for the `Start`/`Base` eval-strings
//! ExifTool's `ProcessBinaryData` runs.
//!
//! # Why this exists
//!
//! Step 9 (`OVERHAUL_OXIDEX_PLAN.md`) added `Omitted::subdirectory`: a flag
//! that stops a `SubDirectory`-carrying field from being silently decoded as
//! a plain scalar, but goes no further -- the field is refused, and where
//! its bytes actually point is thrown away. Step 27 recovers that: for every
//! field whose `Start`/`Base`/`ProcessProc` fall within the grammar this
//! module compiles, [`super::Field::subdir`] carries a [`SubdirEdge`] naming
//! the target table and how to locate it. `Omitted::subdirectory` still
//! stays set (decoding the bytes as *this field's own scalar value* is still
//! wrong -- that has not changed), but a caller that instead wants to know
//! *where the pointer leads* now has an answer instead of nothing.
//!
//! Walking the edge -- opening the target table's bytes and decoding through
//! it recursively -- is Step 28, deliberately not this step (see
//! `OVERHAUL_OXIDEX_PLAN.md`; a maintainer design checkpoint, not a
//! mechanical follow-on). This module only proves the pointer's shape is
//! understood: [`Start::eval`] and [`BaseExpr::eval`] are pure arithmetic
//! over already-known integers, not directory I/O.
//!
//! # The source: ExifTool.pm's `ProcessBinaryData`, not Exif.pm's `ProcessExif`
//!
//! Every field this schema emits comes from a `PROCESS_PROC ==
//! \&ProcessBinaryData` table (`tools/exiftool-tables/codegen.py`'s
//! `is_binary_table`), so the SubDirectory semantics that matter are
//! `ProcessBinaryData`'s own (ExifTool.pm:9877, the `if ($$tagInfo{SubDirectory})`
//! branch at ExifTool.pm:10102-10151) -- NOT Exif.pm's `ProcessExif`
//! (Exif.pm:6917 "Handle SubDirectory tag types" onward), which is a
//! different function serving IFD-style (tag-ID-keyed) tables and is never
//! in the call path for any field modeled here. The two share key names
//! (`Start`, `Base`, `ByteOrder`, `Validate`) but not always semantics --
//! `ProcessExif`'s `Start` eval scope is `($valuePtr, $val)` (Exif.pm:6950-6952)
//! while `ProcessBinaryData`'s is `($val, $dirStart)` (ExifTool.pm:10129) --
//! so this module cites only ExifTool.pm and treats Exif.pm purely as
//! negative evidence for `byte_order`/`validate` below.
//!
//! ```text
//! ExifTool.pm:10102  if ($$tagInfo{SubDirectory}) {
//! ExifTool.pm:10103      my $subdir = $$tagInfo{SubDirectory};
//! ExifTool.pm:10104      my $subTablePtr = GetTagTable($$subdir{TagTable});
//! ExifTool.pm:10118      my $subdirBase = $base;
//! ExifTool.pm:10119      if (defined $$subdir{Base}) {
//! ExifTool.pm:10120          #### eval Base ($start,$base)
//! ExifTool.pm:10121          my $start = $entry + $dirStart + $dataPos;
//! ExifTool.pm:10122          $subdirBase = eval($$subdir{Base}) + $base;
//! ExifTool.pm:10123      }
//! ExifTool.pm:10124      my $start = $$subdir{Start} || 0;
//! ExifTool.pm:10125      my $notDup;
//! ExifTool.pm:10126      if ($start =~ /\$/) {
//! ExifTool.pm:10127          # ignore directories with a zero offset (ie. missing Nikon ShotInfo entries)
//! ExifTool.pm:10128          next unless $val;
//! ExifTool.pm:10129          #### eval Start ($val, $dirStart)
//! ExifTool.pm:10130          $start = eval($start);
//! ExifTool.pm:10131          next if $start < $dirStart or $start > $dataLen;
//! ExifTool.pm:10132          $len = $$subdir{DirLen};
//! ExifTool.pm:10133          $len = $dataLen - $start unless $len and $len <= $dataLen - $start;
//! ExifTool.pm:10134      } else {
//! ExifTool.pm:10135          $start += $dirStart + $entry;
//! ExifTool.pm:10136          $notDup = 1,
//! ExifTool.pm:10137      }
//! ExifTool.pm:10148      $self->ProcessDirectory(\%subdirInfo, $subTablePtr, $$subdir{ProcessProc});
//! ```
//!
//! # Why `ProcessProc` is refused rather than modeled
//!
//! `$$subdir{ProcessProc}` (ExifTool.pm:10148) overrides which function
//! walks the *target* table -- when set, the target is not read by the
//! ordinary `ProcessBinaryData`/`ProcessDirectory` dispatch this schema's
//! consumers assume, but by a bespoke parser (e.g. Panasonic.pm's `PANA`
//! table points its three `ExifData` fields at
//! `Image::ExifTool::ProcessTIFF` with `Start => '12'`, and its
//! `MakerNoteLeica5` field at `Image::ExifTool::Panasonic::ProcessLeicaLEIC`
//! -- four fields in the pinned 13.59 tree that reach this check at all; a
//! fifth PANA field routed the same way, `JPEG-likeData`, never gets this
//! far, because its `Format => 'undef[$size-0x10]'` is a data-dependent
//! width `codegen.py` already refuses on unrelated grounds). A [`SubdirEdge`]
//! that named the target table but silently dropped the fact that it is not
//! read the ordinary way would be a plausible-but-wrong description of the
//! edge, the exact thing `AGENTS.md` forbids -- so `codegen.py` refuses these
//! instead (`subdir_refused_processproc`), the same "refuse and count" discipline
//! `Omitted` itself established for `Hook`.
//!
//! # Why `byte_order`/`validate` are always inert here
//!
//! `SubDirectory.ByteOrder` and `SubDirectory.Validate` are real ExifTool
//! constructs -- but only in Exif.pm's `ProcessExif` (`ByteOrder`:
//! Exif.pm:6972-6996, sets a new byte order via `SetByteOrder` at
//! Exif.pm:7078; `Validate`: Exif.pm:7083, `eval`s a boolean check before
//! processing). ExifTool.pm's `ProcessBinaryData` SubDirectory branch quoted
//! above never reads either key and never calls `SetByteOrder` for a nested
//! BinaryData directory at all. Since every field this schema models goes
//! through `ProcessBinaryData`, not `ProcessExif`, a `ByteOrder` or
//! `Validate` key on one of these fields' `SubDirectory` hash (none declare
//! either in the pinned 13.59 census -- see `codegen.py`'s
//! `subdir_refused_byteorder`/`subdir_refused_validate` counters, both 0)
//! would be dead data ExifTool itself never consults for this field. Rather
//! than assume that forever, `codegen.py` refuses (counts, does not drop
//! silently) any field that declares either key, so a future release adding
//! one is a loud refusal instead of a silently-wrong edge.

/// Restricted arithmetic over the two variables ExifTool.pm's `#### eval
/// Start ($val, $dirStart)` marker (ExifTool.pm:10129) exposes to a
/// `SubDirectory`'s `Start` expression, when `Start` contains a literal `$`.
/// `tools/exiftool-tables/subdirs.py` compiles a `Start` string into this
/// tree by parsing it as an arithmetic expression (Python's own `ast`
/// module, walked against exactly the node shapes below -- the same
/// AST-not-blacklist discipline `tools/exiftool-tables/conds.py` uses for
/// regex patterns) and refuses (counts, does not approximate) anything the
/// tree does not reach: function calls, comparisons, string ops, `**`, and
/// so on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartExpr {
    Const(i64),
    /// `$val`: this field's own already-decoded value.
    Val,
    /// `$dirStart`: the enclosing directory's start offset.
    DirStart,
    Add(&'static StartExpr, &'static StartExpr),
    Sub(&'static StartExpr, &'static StartExpr),
    Mul(&'static StartExpr, &'static StartExpr),
    Neg(&'static StartExpr),
}

impl StartExpr {
    /// Evaluate, mirroring Perl's `eval($start)` at ExifTool.pm:10130 with
    /// `$val`/`$dirStart` bound to `val`/`dir_start`. The result is an
    /// ABSOLUTE offset into the directory's data -- ExifTool does not add
    /// `dir_start` or `entry` again afterward in this branch (contrast
    /// [`Start::FieldRelative`], which does).
    #[must_use]
    pub const fn eval(&self, val: i64, dir_start: i64) -> i64 {
        match self {
            StartExpr::Const(n) => *n,
            StartExpr::Val => val,
            StartExpr::DirStart => dir_start,
            StartExpr::Add(a, b) => a.eval(val, dir_start) + b.eval(val, dir_start),
            StartExpr::Sub(a, b) => a.eval(val, dir_start) - b.eval(val, dir_start),
            StartExpr::Mul(a, b) => a.eval(val, dir_start) * b.eval(val, dir_start),
            StartExpr::Neg(a) => -a.eval(val, dir_start),
        }
    }
}

/// How a `SubDirectory`'s start offset is computed -- ExifTool.pm's
/// two-mode branch on whether the raw `Start` string contains a literal `$`
/// (ExifTool.pm:10126-10137), decided once at compile time by
/// `tools/exiftool-tables/subdirs.py` from the source string itself (the
/// same test ExifTool runs at eval time via `$start =~ /\$/`).
#[derive(Clone, Copy, Debug)]
pub enum Start {
    /// `Start` absent (default 0, ExifTool.pm:10124) or a bare integer
    /// literal with no `$` in it. ExifTool.pm:10134-10136: `$start +=
    /// $dirStart + $entry` -- the literal is added to the enclosing
    /// directory's start AND this field's own byte offset (`entry`, i.e.
    /// [`super::BinaryTable::byte_offset`] of the field this edge hangs
    /// off): absolute = `dir_start + entry + literal`. The overwhelmingly
    /// common case (absent Start, literal 0): the subdirectory begins
    /// exactly at this field's own bytes, e.g. Canon `PictureStyleInfo`
    /// nested directly inline in `CameraInfo*`.
    FieldRelative(i64),
    /// `Start` contains a `$` -- evaluated per [`StartExpr::eval`] as an
    /// ABSOLUTE offset (ExifTool.pm:10129-10130), not added to `entry`.
    /// Two behaviors a caller implementing the walk (Step 28) must
    /// reproduce, both still in ExifTool.pm's source, neither performed by
    /// this module:
    ///   * ExifTool.pm:10128 (`next unless $val`): the whole subdirectory is
    ///     skipped when `$val` is falsy (0) -- "ignore directories with a
    ///     zero offset (ie. missing Nikon ShotInfo entries)".
    ///   * ExifTool.pm:10131: the evaluated `$start` is bounds-checked
    ///     against `dir_start..data_len` and the field skipped if it falls
    ///     outside.
    Expr(&'static StartExpr),
}

/// Restricted arithmetic over `#### eval Base ($start,$base)`
/// (ExifTool.pm:10120). NOTE: the `$start` bound here is `$entry + $dirStart
/// + $dataPos` (ExifTool.pm:10121) -- an unrelated LOCAL variable that
/// happens to share a name with the *separate* `$start` computed afterward
/// for [`Start`] (ExifTool.pm:10124); the two are different Perl lexicals in
/// different scopes, evaluated from different source strings, and this
/// module keeps them as different Rust types ([`BaseExpr`] vs [`StartExpr`])
/// for exactly that reason -- a shared "generic arithmetic expression" type
/// would let a `Base` expression reference `$val`, which real ExifTool does
/// not allow in this scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseExpr {
    Const(i64),
    /// `$start` in `Base`'s eval scope: `entry + dir_start + data_pos`
    /// (ExifTool.pm:10121) -- this field's own absolute position, NOT the
    /// [`Start`] value.
    Start,
    /// `$base`: the enclosing directory's own Base, before this override.
    Base,
    Add(&'static BaseExpr, &'static BaseExpr),
    Sub(&'static BaseExpr, &'static BaseExpr),
    Mul(&'static BaseExpr, &'static BaseExpr),
    Neg(&'static BaseExpr),
}

impl BaseExpr {
    /// Evaluate `eval($$subdir{Base})` (ExifTool.pm:10122). The caller still
    /// has to add the enclosing `base` afterward
    /// (`$subdirBase = eval($$subdir{Base}) + $base`) -- that `+ $base` is
    /// NOT folded into this expression, because `$base` here is the value
    /// *before* this override, the same one `start` (the argument) was
    /// computed relative to, and folding it in would double-count it if a
    /// caller (reasonably) also adds the enclosing base separately. Callers
    /// implementing the walk (Step 28) compute the final subdirectory base
    /// as `expr.eval(field_start, enclosing_base) + enclosing_base`.
    #[must_use]
    pub const fn eval(&self, start: i64, base: i64) -> i64 {
        match self {
            BaseExpr::Const(n) => *n,
            BaseExpr::Start => start,
            BaseExpr::Base => base,
            BaseExpr::Add(a, b) => a.eval(start, base) + b.eval(start, base),
            BaseExpr::Sub(a, b) => a.eval(start, base) - b.eval(start, base),
            BaseExpr::Mul(a, b) => a.eval(start, base) * b.eval(start, base),
            BaseExpr::Neg(a) => -a.eval(start, base),
        }
    }
}

/// `SubDirectory.ByteOrder`'s two literal forms Exif.pm's `ProcessExif`
/// recognizes (`/^Little/i` -> `'II'`, `/^Big/i` -> `'MM'`, Exif.pm:6974-6977).
/// Reserved, not compiled to: see this module's doc comment for why
/// `ProcessBinaryData` never reads this key, so no field in the pinned
/// 13.59 tree needs it modeled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteOrderRule {
    Little,
    Big,
}

/// One `SubDirectory` pointer, compiled from ExifTool's own `Start`/`Base`
/// eval-string grammar. See this module's doc comment for the full
/// citation; walking the edge (opening and decoding the target table) is
/// Step 28, not implemented here.
#[derive(Clone, Copy, Debug)]
pub struct SubdirEdge {
    /// `SubDirectory.TagTable`'s `Image::ExifTool::<module>::<table>`,
    /// split into the two halves [`super::find_table`] already keys on.
    /// Resolving it is optional for a caller: many targets (`IPTC::Main`,
    /// `LNK::LinkInfo`, ...) are not `ProcessBinaryData` tables this crate
    /// has transcribed a layout for at all, and that is not a defect in the
    /// edge -- the edge only claims to know where the pointer leads, not
    /// that a byte layout exists on the other end.
    pub module: &'static str,
    pub table: &'static str,
    /// How the subdirectory's start offset is computed. See [`Start`].
    pub start: Start,
    /// How the subdirectory's `Base` is computed, if the `SubDirectory`
    /// overrides it. `None` = inherit the enclosing directory's `Base`
    /// unchanged (ExifTool.pm:10118, `my $subdirBase = $base;`, taken when
    /// `$$subdir{Base}` is undef). `Some` per [`BaseExpr::eval`]'s doc for
    /// how a caller combines it with the enclosing base.
    pub base: Option<&'static BaseExpr>,
    /// Always `None` in the pinned 13.59 tree. See this module's doc comment
    /// ("Why `byte_order`/`validate` are always inert here").
    pub byte_order: Option<ByteOrderRule>,
    /// Always `false` in the pinned 13.59 tree, same story as `byte_order`.
    pub validate: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_relative_default_is_the_fields_own_position() {
        // The overwhelmingly common shape: no Start declared at all, e.g.
        // Canon::CameraInfo5DmkIII's PictureStyleInfo (index 944) pointing
        // at Canon::PSInfo2. absolute = dir_start + entry + 0.
        let start = Start::FieldRelative(0);
        let Start::FieldRelative(literal) = start else {
            unreachable!()
        };
        let dir_start = 0i64;
        let entry = 944i64;
        assert_eq!(dir_start + entry + literal, 944);
    }

    #[test]
    fn field_relative_adds_a_nonzero_literal_too() {
        // Panasonic PANA's Start => '12' (paired with a ProcessProc override
        // that codegen.py refuses independently -- this test only checks
        // the FieldRelative arithmetic itself, ExifTool.pm:10134-10136).
        let start = Start::FieldRelative(12);
        let Start::FieldRelative(literal) = start else {
            unreachable!()
        };
        assert_eq!(100i64 + 16i64 + literal, 128);
    }

    #[test]
    fn start_expr_reproduces_nikon_menu_info_dir_start_plus_val() {
        // Nikon::MenuInfoZ7II 0x10 MenuSettingsOffsetZ7II: Start => '$dirStart
        // + $val' (ExifTool.pm:10129-10130's eval scope).
        static VAL: StartExpr = StartExpr::Val;
        static DIR_START: StartExpr = StartExpr::DirStart;
        static EXPR: StartExpr = StartExpr::Add(&DIR_START, &VAL);
        assert_eq!(EXPR.eval(0x200, 0x10), 0x210);
    }

    #[test]
    fn base_expr_reproduces_thumbnail_start_override() {
        // Olympus::MovableInfo 131 Thumbnail: Base => '$start' -- the
        // subdirectory's base becomes this field's own absolute position
        // (entry + dir_start + data_pos), independent of $val.
        static EXPR: BaseExpr = BaseExpr::Start;
        let field_pos = 0x1000i64;
        let enclosing_base = 0x8000i64;
        // Caller contract per BaseExpr::eval's doc: eval(...) + enclosing_base.
        assert_eq!(
            EXPR.eval(field_pos, enclosing_base) + enclosing_base,
            0x9000
        );
    }

    #[test]
    fn nested_arithmetic_evaluates_left_to_right_by_structure() {
        // (dirStart + val) - 4, exercising Sub/Add nesting even though no
        // field in the pinned 13.59 census needs this particular shape --
        // the grammar is general, not a hardcoded single string.
        static VAL: StartExpr = StartExpr::Val;
        static DIR_START: StartExpr = StartExpr::DirStart;
        static SUM: StartExpr = StartExpr::Add(&DIR_START, &VAL);
        static FOUR: StartExpr = StartExpr::Const(4);
        static EXPR: StartExpr = StartExpr::Sub(&SUM, &FOUR);
        assert_eq!(EXPR.eval(10, 20), 26);
    }
}
