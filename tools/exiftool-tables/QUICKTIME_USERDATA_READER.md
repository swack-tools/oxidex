# Direct-format QuickTime UserData reader

The generated movie-level `moov/udta` reader covers the complete safe explicit
Format intersection in the pinned 13.59 source: 17 declarations (13 strings,
2 int8u, 1 int16u, 1 int32u). Names, FourCCs, groups, Avoid priorities, formats,
literal enum maps and the effective single-byte character mapping come from
captured native data. No tag-name or tag-ID mapping was added to Rust.

`quicktime_userdata_specs.py` implements a finite reviewed ProcessMOV contract.
It checks complete processor and helper bodies, actual Main/Movie caller
bindings, direct caller edges and controls, core format-registry provenance,
CharsetQuickTime's default, and the loaded MacRoman map. New table declarations
and map operands compile; unknown executable statements or controls refuse.
This is not a general Perl translator or automatic arbitrary-upgrade support.

The direct numeric branch uses the declared width, reads complete big-endian
elements, ignores a partial tail and joins multiple values with spaces. It does
not apply ItemList's payload-dependent width adjustment. Explicit strings stop
at the first NUL. Non-copyright strings of at most 65,536 bytes use native
IsUTF8 detection, otherwise the generated MacRoman map with identity fallback.
Copyright-prefixed explicit Format bypasses IText and that auto-detection.
Raw-byte strings use the existing native FixUTF8 JSON projection because a Rust
String cannot retain malformed UTF8. The limit applies after NUL truncation.

Other UserData paths retain their existing fallbacks. The source ledger retains
all 213 variants: 17 generated and 196 omitted. Of the omitted records, 82 have
no blocker beyond implicit Format; their string behavior is a separate protocol.
IText, language records, custom conversion/control and track-context routing
remain outside this new reader. int64u also explicitly refuses: the current
TagValue representation cannot preserve every native unsigned JSON number.
There is no canonical int64u row in this 17-declaration intersection.

Two new artifacts are registered with the ordinary regeneration entry point:
`generated_userdata_specs.rs` and `quicktime_generated_userdata_ledger.json`.
The existing bounded table snapshot retains its original capture provenance;
its additional UserData protocol carries a separately authenticated canonical
Perl refresh. A fresh full dump captures that protocol directly.

## Verification

The checked-in native fixture file records 63 cases / 126 pinned Group1 JSON
reads, including every generated declaration and numeric/text boundary cases.
The Rust test `recorded_native_json_values_cover_all_direct_declarations_and_edges`
replays their native scalar lengths and SHA256s through the new decoder. Native
transcripts alone do not prove Rust execution. The implementation handoff has
not run Cargo or Clippy because the parent owns the shared build allocation.

Portable and copied-native compiler tests:

```sh
EXIFTOOL_PERL="$perl" OXIDEX_PINNED_EXIFTOOL_LIB="$lib" \
  python3 -m unittest discover -s tools/exiftool-tables -p 'test_quicktime*.py'
python3 tools/exiftool-tables/quicktime_userdata_specs.py --check
```

After the parent builds a fresh `oxidex` CLI from the integrated runtime inputs,
run the real public parser comparison (explicit binary path from that build):

```sh
python3 tools/exiftool-tables/verify_quicktime_userdata_reader.py \
  --perl "$perl" --lib "$lib" --lock "$validation_lock" \
  --oxidex "$fresh_oxidex" --output ../evidence/userdata-public
```

Omitting `--oxidex` runs only the bounded native fixture capture. The evidence
records whether Rust was checked, source/native/binary hashes, full native
transcripts, fixture hashes and a before/after runtime input manifest for the
public route. Keep native-only and public-route verdicts distinct. These files
do not import observed-read credit into the catalog.
