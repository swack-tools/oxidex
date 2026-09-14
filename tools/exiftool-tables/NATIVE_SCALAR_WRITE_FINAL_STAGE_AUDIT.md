# Native scalar write final-stage audit

Scope: pinned ExifTool 13.59 source, default writer route, standard TIFF/EXIF
IFD entries. This identifies the missing stage after the existing source-derived
`WriteValue` scalar helper. It is an implementation boundary, not an OxiDex
writer, public-tag admission rule, runtime claim, or general conformance claim.

## Source path and facts

`Image/ExifTool/Exif.pm:411-416` gives the main EXIF table its group identity,
`WriteExif`/`CheckExif` procedures, and default write group. `HostComputer` is
the useful pilot: it is raw ID `0x013c`, `Writable => 'string'`, and
`WriteGroup => 'IFD0'` (`Image/ExifTool/Exif.pm:927-931`). It has neither
`Format` nor `Count`.

The currently generated helper accurately stops at a narrower boundary. It
models `Scalar::{Undefined,Bytes,Utf8}` (`src/writers/generated_scalar.rs:5-11`)
and the scalar `WriteValue` branch (`src/writers/generated_scalar.rs:218-264`),
with the generated recipe limited to `string`/`undef`
(`src/writers/generated_scalar_rules.rs:30-42`). Its own comment says that its
count is before later TIFF admission (`src/writers/generated_scalar.rs:72-78`).
The inactive descriptor compiler is intentionally even narrower: it admits only
literal `Name`, `Writable`, and effective `WriteGroup`
(`tools/exiftool-tables/write_descriptors.py:40-41,279-337`) and labels its
output inactive (`tools/exiftool-tables/write_descriptors.py:29-33,502-579`).
Neither is enough to choose the final wire type or write an IFD.

The scalar arriving at this stage is the byte scalar after `SetNewValue`
sanitization and inverse conversion, not a new public text input. Native
`SetNewValue` calls `Sanitize` before routing (`Image/ExifTool/Writer.pl:360-369`),
and `Sanitize` UTF-8-encodes an upgraded Perl scalar specifically to clear the
UTF-8 flag (`Image/ExifTool/Writer.pl:2923-2952`). Thus default-scope Unicode
text is represented here as post-sanitization bytes; it is not excluded.

The source call chain is:

1. `CheckExif` chooses `Format || Writable || table WRITABLE` before calling
   `CheckValue` (`Image/ExifTool/WriteExif.pl:112-124`). For string/undef,
   `CheckValue` only applies a positive fixed count; it rejects an overlong
   string, otherwise NUL-pads it (`Image/ExifTool/Writer.pl:6862-6879`).
2. `WriteValue` handles string/undef separately. `string` receives one NUL;
   a positive count truncates or pads it, while absent/nonpositive count becomes
   the current scalar length (`Image/ExifTool/Writer.pl:5384-5444`). UTF-8
   scalar length here remains Perl character length until an encoding step.
3. `WriteExif` resolves a physical entry, calls `WriteValue`, then optionally
   encodes the resulting bytes. `utf8` uses `Encode(...,'UTF8')`; `string`
   uses `CharsetEXIF` only when the table's family-0 group is EXIF
   (`Image/ExifTool/WriteExif.pl:584-585,1275-1301`). `CharsetEXIF` defaults
   to undefined, so default options do not re-encode (`Image/ExifTool.pm:1112-1123`).
   `Encode` delegates to `Decode`, using the current Charset when no explicit
   source charset is supplied (`Image/ExifTool.pm:6344-6379`).
4. The final entry derives count from the post-encoding byte length and the
   selected format size unless the old row declares `FixedSize`
   (`Image/ExifTool/WriteExif.pl:1964-1973`). Values up to four bytes are put
   directly in the IFD field and zero-padded; larger values are padded to even
   length/format multiple and stored in the value buffer with an offset
   (`Image/ExifTool/WriteExif.pl:1975-2033`).

The TIFF number/name/size tables are source operands, not Rust constants to
recreate informally: EXIF format 2 is `string`, format 129 is `utf8`, and the
mapping includes the standard format codes (`Image/ExifTool/Exif.pm:81-122`).
The more general `WriteValue` format-size table includes `string`, `undef`,
`unicode`, `ifd`, and `utf8` and must remain distinct from TIFF wire format
numbers (`Image/ExifTool.pm:6210-6242`).

## Required controls and branch distinctions

The final-stage compiler must receive authenticated source facts for all of the
following; absence or an unrepresented value is refusal.

| Operand/control | Why it is required | Native source |
| --- | --- | --- |
| Selected table/row identity, raw u16 tag ID, source digest, effective groups, `WRITE_PROC`/`CHECK_PROC` provenance | A name or procedure name cannot establish physical placement or behavior. | `Image/ExifTool/Exif.pm:411-416`; `tools/exiftool-tables/write_descriptors.py:145-200` |
| `Format`, `Writable`, table `WRITABLE`, `Count`, `FixedSize`, `WriteCondition`, `RawConvInv`, `IsOverwriting` and `WriteGroup` facts | These choose whether a row writes, converts, deletes, uses a fixed count, or chooses a different wire type. | `Image/ExifTool/WriteExif.pl:1102-1178,1181-1269` |
| Existing-entry presence plus old format number/name, old count/size/value and MakerNotes flag | Existing updates use a different format-precedence path and deletion only has meaning for an existing row. | `Image/ExifTool/WriteExif.pl:751-834,1215-1247` |
| Operation state: create, update/overwrite, no-overwrite, delete; scalar state: undefined, bytes, UTF-8 | Undefined is deletion after overwrite selection; the helper's scalar state is necessary but insufficient. | `Image/ExifTool/WriteExif.pl:1255-1269`; `src/writers/generated_scalar.rs:5-11` |
| Native conversion-format name, IFD wire-format name/number/size | `Format` selects conversion format, while `Writable` may select the IFD code. These are intentionally separable. | `Image/ExifTool/WriteExif.pl:1202-1214,1225-1247,1326-1329`; `Image/ExifTool/Exif.pm:81-122` |
| Table family-0 group, `Charset`, and explicit `CharsetEXIF` state | Encoding happens after `WriteValue`, changes byte length/count, and default `CharsetEXIF` is false. | `Image/ExifTool/WriteExif.pl:584-585,1290-1301`; `Image/ExifTool.pm:1112-1123,6344-6379` |
| File type, IFD byte order, entry/value-buffer placement, `EntryBased`, and JPEG size constraint | These govern byte order, offsets, storage placement, and a format-specific warning/limit. | `Image/ExifTool/WriteExif.pl:581-585,1281-1289,1975-2033` |

Format precedence is branch-specific and must not be compressed to
`Format || Writable` everywhere. For a new entry, `Format` is the conversion
format and `Writable` is retained as the IFD format; otherwise `Writable` is
the conversion and IFD format (`Image/ExifTool/WriteExif.pl:1202-1214`). For
an existing entry, missing/`'1'` `Writable` falls back to old format, then
`Format` overrides conversion; differing non-MakerNotes writable format becomes
the IFD code (`Image/ExifTool/WriteExif.pl:1215-1247`). MakerNotes preserves
the old IFD type and has a separate fixed-size rule; it is outside this pilot.

Zero-length is also not a generic empty-value rule. A defined empty scalar is
first sent through `WriteValue` and a string gains NUL. Only a resulting empty
serialized value warns and falls through `NoOverwrite`
(`Image/ExifTool/WriteExif.pl:1275-1301`). Undefined instead reaches the
delete branch after overwrite resolution (`Image/ExifTool/WriteExif.pl:1255-1269`).

## Proposed closed recipe/IR boundary

Generate an internal `TiffScalarFinalStageRecipe`, authenticated against the
pinned source capture, with these fields:

```text
provenance: { exiftool_release, source files/digests, table/row identity }
target: { raw_tag_id: u16, table_group0, physical_write_group }
controls: {
  format: Option<NativeFormatName>, writable: Option<NativeFormatName>,
  table_writable: Option<NativeFormatName>, count: Absent|Fixed(i64)|Variable,
  fixed_size: bool, write_condition: Absent|AuthenticatedSupported,
  raw_conv_inv: Absent|AuthenticatedSupported, is_overwriting: DefaultOnly
}
existing: Absent | {
  old_wire_format: u16, old_format_name: NativeFormatName,
  old_count: u32, old_size: usize, maker_notes: false
}
options: { charset: NativeCharset, charset_exif: Disabled|NativeCharset }
layout: { byte_order: II|MM, entry_based: false, file_type: TIFF|JPEG }
```

The executor receives the post-`Sanitize`/`ConvInv` scalar plus an explicit
operation. It resolves the format before invoking the generated `WriteValue`
helper; `SerializedScalar` is that invocation's intermediate output, not an
input to it. It must:

1. select create/update/delete using authenticated write-condition and
   overwrite controls;
2. resolve conversion and wire format with the native branch-specific rules;
3. call the already generated scalar helper only when its recipe is proven;
4. apply encoding after helper serialization, then recalculate byte count from
   the selected wire size;
5. model native empty post-serialization behavior as
   `NoOverwrite { native_warning: "Can't write zero length ..." }`, except
   that undefined follows the delete path. This is distinct from a compiler
   refusal: a route that cannot represent native `NoOverwrite` must refuse
   admission rather than substitute a fatal error and call it parity;
6. emit the 12-byte IFD entry with authenticated u16/u32 encoders, using
   inline storage for `<= 4` bytes and aligned out-of-line storage otherwise.

Initial admission should be narrower than the type definition: standard
`Exif::Main`, non-MakerNotes, literal `Writable => 'string'`, no `Format`, no
fixed `Count`, no `FixedSize`, no conversion/condition/overwrite callbacks,
literal `WriteGroup`, `EntryBased == false`, and `CharsetEXIF == Disabled`.
This exactly includes the HostComputer pilot's ASCII, UTF-8-derived-byte, and
embedded-NUL scalar inputs. It leaves explicit `CharsetEXIF` recoding,
Format-overrides, fixed counts, callbacks, MakerNotes, and general directory
rebuild outside the route. Expanding it requires source capture for the next
control, not a tag-name allowlist.

## Control matching and stale-source refusal

Procedure names and provenance digests alone are necessary but insufficient.
The final-stage compiler must capture a closed source slice for the selected
branch: all row/table controls in the preceding table, the exact
Format/Writable fallback chain, post-`WriteValue` encoding guards, count
recalculation, and inline/out-of-line layout guards. The generated recipe must
carry a canonical hash of those parsed operands and guard/action sequence in
addition to source-file and deparsed-body digests.

At generation time, the extractor must reject a source slice containing an
unrepresented reachable control, condition, call, or statement. At runtime,
the route must require exact equality between the recipe's complete control map
and the selected source facts. A changed or inserted source statement then
causes either the file/body digest mismatch or a parsed-sequence/control-map
mismatch; it cannot become admitted merely because `WriteExif` or `WriteValue`
still has the same name. This protects against stale or hand-widened
descriptors more strongly than the current inactive descriptor provenance.

## Uncertainties and deliberately excluded work

No native probe was needed: the needed ordering and operands are direct source
facts. This audit does not establish complete `SetNewValue` routing or inverse
conversion semantics before the post-sanitization scalar boundary, full IFD
rebuilding/fixup correctness, directory creation/deletion, BigTIFF,
MakerNotes, `EntryBased`, non-string formats, explicit `CharsetEXIF`
conversion, JPEG segment admission, or Rust writer conformance. The source
shows `RawConvInv`, `WriteCondition`, `IsOverwriting`, cross-directory deletion
and mandatory-tag handling in the surrounding route; they must remain refused
until each is represented and tested.
