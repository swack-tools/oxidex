# IFD-style tag tables: generate, walk, gate — design (2026-09-06)

> **Design baseline: September 6, 2026.** The opening problem statement describes
> the pre-IFD implementation. I-1 through I-3 have since landed; I-4 and IFD1 work
> must be checked separately. See [current status](../../TAG_MACHINERY_STATUS.md)
> before using this original sequence as a task list.

Status: approved direction (maintainer, 2026-09-06 evening: "direction A, after the two parked
binary slices"; then "just do whatever is the fastest, parallel if that works"). This file is the
contract the parallel workers build against. Measurements are in
`docs/FLEET-COMPLETION-PLAN.md`, entry "where the remaining tags live".

## 1. Why

Corpus MISSING at the tip (`conformance.py --json-out`, 4,238 files, pinned 13.59) is 5,885
occurrences; 3,838 (65%) are MakerNotes tags that live in ExifTool's IFD-style tables, which the
generator does not transcribe at all. The pinned tree has 651 ProcessBinaryData tables
(transcribed) and 495 IFD-style tables (17,986 tag entries) using the same conversion shapes
(PrintConv expr 1,360 / enum 1,058 / partial 85; ValueConv expr 1,023), plus 570 Conditions,
1,035 SubDirectory edges (501 to IFD tables, 306 to already-generated binary tables) and 272
RawConvs. The Olympus tree (949 MISSING / 187 files) is entirely IFD-style and hand-parsed in
~4,300 lines that register most tags raw.

## 2. Shape

* Schema: `src/exiftool_tables/ifd_schema.rs` (hand-written, stable): `IfdTable`, `IfdTag`,
  `IfdFlags`, `RawConvEffect`, `IfdVariantGroup`, `IfdStart`, `IfdByteOrder`, `IfdSubdirEdge`.
  Reuses `Fmt`, `Omitted`, `PrintConv`, `TagGroups`, `GateA`, `ExprId`, `cond::Cond`,
  `subdir::BaseExpr`.
* Generated: `src/exiftool_tables/ifd_tables.rs` = `//! header` + one
  `pub static IFD_<MODULE>_<TABLE>: IfdTable = IfdTable { ... };` per table +
  `pub static ALL_IFD_TABLES: &[&IfdTable] = &[...]` (sorted by module, table). Identifier =
  upper-case module and table with non-alphanumerics as `_`, same rule as `BinaryTable` statics.
* Selection (`codegen.py::is_ifd_table(meta)`): `PROCESS_PROC` absent, or its `__name` ends with
  `Exif::ProcessExif`. Everything else keeps being counted `table_not_binary`.
* Per-tag emission (`gen_ifd_tag_literal`), key by key:
  - `id`: the integer key; non-integer or > 0xFFFF -> tag refused, counter
    `ifd_tag_id_unrepresentable` (Gate-A disqualifying).
  - `name`: `Name`; missing -> `tag_no_name` (existing key).
  - `format`: `Format` parsed by the existing scalar parser; `fmt[N]` -> `format: Some(fmt)`,
    `count: Some(N)`; `string`/`undef` without `[N]` -> `Some(Fmt::Str(0))`-style is NOT used —
    emit `format: None` and count from `Count`; a spelling outside `SCALAR_FORMATS` ->
    `ifd_format_unsupported` (disqualifying).
  - `count`: `Count` or the `[N]`.
  - `writable`: `Writable` string verbatim when it is a format spelling (an integer `Writable =>
    0/1` is `None`).
  - `groups`: existing `TagGroups` emission from `Groups`.
  - `flags`: `Unknown`, `Binary`, `List`, `Protected`, `Avoid`, `Priority` -> `IfdFlags`.
    `Unknown` is a FLAG here, never a drop (binary tables keep dropping them; that is unchanged).
  - `omitted` + `raw_conv`: existing `omitted_for`, except a `RawConv` whose normalised text is
    exactly `$$self{<Ident>} = $val` (optionally `;`) becomes `raw_conv: Some(RawConvEffect::
    SetMember { member })` with `omitted.raw_conv: false`.
  - `value_conv` / `print_conv`: the existing `value_conv_for` / `conv_for` with the input domain
    from `Format` if declared, else from `Writable` (same `value_domain` mapping), else the
    conversions are refused with counter `ifd_expr_domain_unknown` (disqualifying) — an
    expression verified in one domain must not run in another.
  - `subdir`: `SubDirectory` -> `IfdSubdirEdge`: `TagTable` split into module/table
    (`ifd_subdir_refused_tagtable` when odd; absent -> the enclosing table, emitted unwalked:
    v1.1 below); `Start` absent -> `IfdStart::ValuePtr(0)`,
    `'$valuePtr'` / `'$valuePtr + n'` / `'$valuePtr - n'` -> `ValuePtr(±n)`, `'$val'` / `'$val
    + n'` -> `Val(±n)`, anything else -> `ifd_subdir_refused_start`; `Base` through the existing
    `BaseExpr` compiler (refused -> `ifd_subdir_refused_base`); `ByteOrder` `LittleEndian`/`II`
    -> `Little`, `BigEndian`/`MM` -> `Big`, `Unknown` -> `Unknown`, absent -> `Inherit`, other
    -> `ifd_subdir_refused_byteorder`; `FixFormat => 'ifd'` -> `sub_ifd: true, fix_format:
    None` (`ifd` is ExifTool's pointer spelling, not a scalar format); any other `FixFormat`
    spelling -> the scalar parser -> `fix_format: Some(fmt)`, unparseable ->
    `ifd_subdir_refused_fixformat`; `Flags => 'SubIFD'` -> `sub_ifd: true`;
    `MaxSubdirs` -> `max_subdirs`; `DirName` -> `dir_name`; `Validate` present ->
    `validate: true` (edge emitted, walk refuses it: `ifd_subdir_refused_validate`, NOT
    disqualifying); `ProcessProc` present -> the edge is emitted; if it names anything but
    `ProcessBinaryData` it carries `unwalked: Some("ProcessProc <sub>")` (v1.1, was refused
    `ifd_subdir_refused_processproc`). `unwalked: Option<&str>` (v1.1): `Some(reason)` marks an
    edge the census sees and `descend` returns on, as it does for `validate` -- no `TagTable`
    (`"same-table recursion (TagTable absent)"`: Exif.pm:6939-6944 walks the pointer with the
    enclosing table, so `module`/`table` name the table the edge sits in; a same-table walk from a
    directory the hand parser reached would re-enter IFD0/ExifIFD behind the engine's guard, so it
    is not walked) or a `ProcessProc` the walk cannot run, with any SubDirectory key the schema does
    not model named after it; counted `ifd_subdir_same_table_unwalked` /
    `ifd_subdir_processproc_unwalked`, neither disqualifying. Every other field of an unwalked
    edge must still compile, or the edge is refused as before.
    A tag with a refused edge is emitted with `omitted.subdirectory: true` and `subdir: None`.
  - `_variants`: `IfdVariantGroup` via the existing `conds.py` compiler, atomic refusal
    (`tag_variant_skipped` + reasons, disqualifying) -- after a classification (v1.1): a group
    whose EVERY alternative is offset-class (`IsOffset`/`OffsetPair`/`DataTag`/`ChangeBase`) or a
    SubDirectory the walk could never follow (no `TagTable`, or a non-`ProcessBinaryData`
    `ProcessProc`; a SubDirectory's own value is never reported, Exif.pm:7103-7104), or the group
    that IS `\@MakerNotes::Main` (Exif.pm:2496 `0x927c`, recognised by equality with the dump's
    `MakerNotes.arrays.Main.rows`), can never give the walk a value or a directory to get right
    whichever alternative wins: NOTHING is emitted for the id (the engine treats an id with no
    tag and no group as unknown), counted `ifd_variant_unreported_skipped` /
    `ifd_variant_makernotes_dispatch`, not disqualifying; the hand post-passes stay its only
    producer. One reportable alternative (a plain tag, or a walkable edge) keeps the group on the
    compiling path and its atomic refusal.
  - `Format => 'binary'` (v1.1) is the unsized `Fmt::Undef(0)`: `%formatSize` has `binary => 1`
    beside `string`/`undef` (ExifTool.pm:6236) and `ReadValue` reads all three with one `substr`,
    NUL-truncating only `string` (:6308-6311); counted `ifd_format_unsized` +
    `ifd_format_binary_as_undef`.
* Gate A for IFD tables: `GATE_A_DISQUALIFYING` + the new `ifd_*` keys named above.
* `dump_tables.pl`: add `FixFormat` to `@TAG_KEYS` (the only key the design needs that the dump
  lacks). `MaxSubdirs`, `DirName` too if absent.
* REPORT: a new "IFD tables" section with tables emitted / tags emitted / variants / edges / each
  refusal counter.

## 3. Engine

`src/exiftool_tables/ifd_engine.rs`, one function:

```
pub fn process_exif(table: &'static IfdTable, dir: IfdDir<'_>, ctx: &mut cond::Ctx, out: &mut Vec<Emitted>)
```

* `IfdDir { data: &[u8], ifd_start: usize, base: Option<i64>, byte_order: ByteOrder, group1: Option<&str> }`
  — `base` is the signed correction added to an entry's `value_offset` (`None` = out-of-line
  values must not be read: the caller could not locate the block). Ported from
  `parsers/tiff/makernotes/shared/table_ifd.rs::{read_ifd, decode_entry_with_floor}` into the
  engine (the engine is the lower layer; `table_ifd.rs` is made to delegate to it in a later
  slice). Floor = end of the directory (`ifd_start + 2 + 12n + 4`).
* Per entry: `table.tag(id)`, else `table.variant_group(id)` resolved with `cond::first_match`
  over `(Cond, IfdTag)` using a `Ctx` whose `format` is the entry's declared type name
  (ExifTool spelling: `int16u`, `undef`, `string`, `ifd`, ...), `count` the entry's count,
  `val_pt` the value bytes; else the entry is skipped (unknown tag: no `-u` in v1).
* Decode: byte length from the entry's declared type × count; reinterpret as `tag.format` when
  set (Exif.pm:6733-6760); `Fmt::Str`/`Undef`-style values become `DecodedValue::String`/
  `Undefined`, numeric counts > 1 become `DecodedValue::Array` (the same shapes the binary engine
  produces), rationals as rationals.
* `raw_conv: SetMember` writes `ctx.members[member]` (Num for an integer-valued scalar, Str
  otherwise) and continues; `omitted.any()` withholds; `apply_value_conv` then `render` exactly as
  `engine::walk`; the reported value is `runtime::to_exiftool_value` unless `flags.list`
  (then `to_tag_value`, i.e. a `TagValue::Array`); `flags.binary` reports
  `(Binary data N bytes, use -b option to extract)` with N = the entry's byte length;
  `flags.unknown` tags are not emitted (v1).
* `Emitted` gains `group1: &'static str`: the tag's `groups.g1`, else the table's `set_group1`,
  else `dir.group1`, else the table's `group1`. `low_priority` = tag `flags.priority == Some(0)`
  or the table's `priority == Some(0)`; `flags.avoid` is carried as a new `avoid: bool`.
* `SubDirectory`: compute the start per `IfdStart` (`ValuePtr` = absolute position of the value
  bytes: the entry's value field when length ≤ 4 else `value_offset + base`; `Val` = the decoded
  pointer(s) + base), the base per `BaseExpr` (`start` bound to the entry's absolute value
  position), the byte order per `IfdByteOrder` (Unknown: try the enclosing order, flip when the
  sub-IFD's entry count is 0 or > 512, Exif.pm:6981-6989); `sub_ifd` walks each offset in the
  value up to `max_subdirs`. Target resolution: `find_ifd_table` -> recursive `process_exif`
  (refused when `!target.enabled()`, as `engine::descend` does); else `find_table` ->
  `process_binary_data` with `Dir { data, dir_start: start, dir_len: Some(len), base, byte_order }`
  where len = the entry's byte length for `ValuePtr` edges; `validate: true` edges are not walked.
  The existing `engine::Guard` (cycle + depth 8) is shared.
* Gate B: `src/exiftool_tables/enabled_ifd.rs` — `ENABLED_IFD: &[(&str, &str)]` sorted, empty in
  slice I-1, `is_enabled(table)` = `gate_a.passes() && listed`, the same four tests as
  `enabled.rs`. `find_ifd_table(module, table)` + `ALL_IFD_TABLES` re-exported from `mod.rs`.

## 4. Verification

* `oracle.pl`: a second scope predicate for IFD tables (PROCESS_PROC absent / `Exif::ProcessExif`)
  emitting, per tag key: `NAME`, `FORMAT` (spelling), `COUNT`, `WRITABLE`, `ENUM`/`OTHER`/`BITMASK`
  rows as today, `GROUPS`, `FLAGS unknown binary list protected avoid priority`, `RAWCONV text`,
  `SUBDIR tagtable start base byteorder validate fixformat subifd maxsubdirs dirname`, and
  `_variants` keyed `"$k#$i"` as today. Table rows: `TGROUPS`, `SETGROUP1`, `PRIORITY`.
* `verify.py`: parse every `IfdTable` static (`IFD_TABLE_RE`, `IFD_TAG_RE`, the nested arrays via
  the existing span helpers), account for every `IfdTag {`, compare each emitted fact with the
  oracle rows; unrecognised shapes fail loudly (same doctrine as the binary parser).
* `verify_exprs.py`, `verify_cond.py`: unchanged (dump-wide).
* `reachability.py`: a second census over `ALL_IFD_TABLES` / `ENABLED_IFD` (enabled / eligible /
  refused + `find_ifd_table` call sites).
* Per enabled table: a real-carrier test in `tests/` in `ape_new_header_binary_table.rs`'s shape
  (allowlist line, on/off A/B, per-field values pinned from `exiftool-pinned.sh`).

## 5. Slices

* **I-1** (this branch, `staging/ifd-tables-1`; parallel workers on `staging/ifd-gen`,
  `staging/ifd-verify`, `staging/ifd-engine`, merged here): schema, generator, oracle + verifier,
  engine + empty allowlist, regen on the i7, reachability census of the 495. No call site.
* **I-2**: Olympus::Main through the dispatcher (`parser_for_make_prefix`), Gate B A/B, real-carrier
  test, `Make`/`Model` members seeded from EXIF.
* **I-3**: the Olympus sub-tables (Equipment, CameraSettings, RawDevelopment(2), ImageProcessing,
  FocusInfo, RawInfo, MainInfo) and deletion of the hand paths they replace; the three hand passes
  the tables cannot express (CameraType/Quality entanglement, FocusInfo model-conditional
  `value_forms`, PreviewImage IsOffset) stay as explicit counted post-passes.
* **I-4+**: one table per landing by MISSING rank (Sony, Pentax, Nikon, Canon::Main).

## 6. Not in v1

Writing; `-u` output of `unknown` tags; `IsOffset`/`OffsetPair`/`DataTag` previews (refused, counted
`ifd_isoffset_unsupported`, not disqualifying); conversions that read data members
(`$$self{X}` inside a ValueConv/PrintConv stay refused by the expression grammar); the standard
EXIF IFDs through the engine's own chain -- **v1.1 (slice IFD1):** `Exif::Main` is Gate-A eligible
(the `unwalked` edges and the unreported variant groups above are what cleared it, with zero
runtime change for any enabled table) and may be walked at a NAMED directory the hand parser hands
it, IFD1 first, under `Exif::Main`'s `SET_GROUP1`; IFD0/ExifIFD/InteropIFD through the engine, and
the same-table edges that would reach them, remain out (`Exif::Main` keeps `tag_db` naming there
until group-1 arbitration is proven). The offset-class ids (`0x0111/0x0117/0x0201/0x0202/0x014a`)
and `0x927c` are absent from the static by construction and stay hand-only.
