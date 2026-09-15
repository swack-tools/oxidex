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

Public evidence requires an immutable build receipt. From a clean committed
checkout, run the explicit build wrapper in an allocated build slot. It runs
`cargo build --bin oxidex --all-features --message-format=json --jobs 4`, records
the actual Cargo executable, and checks source inputs before and after the build.
It does not infer build provenance from timestamps. The comparison never builds
implicitly. Keep the shared target and lock allocation supplied by the controller.

```sh
python3 tools/exiftool-tables/verify_quicktime_userdata_reader.py \
  --build-proof-dir ../evidence/userdata-build
build_proof=../evidence/userdata-build/build-proof.json
fresh_oxidex=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["binary"]["path"])' "$build_proof")
python3 tools/exiftool-tables/verify_quicktime_userdata_reader.py \
  --perl "$perl" --lib "$lib" --lock "$validation_lock" \
  --oxidex "$fresh_oxidex" --build-proof "$build_proof" \
  --output ../evidence/userdata-public
```

Use `--source`, `--ledger` and `--rust` to select regenerated artifacts. The
verifier recompiles the source and requires the ledger and Rust to replay; for
public observations, the Rust must also equal the artifact in the built checkout.
The clean commit, runtime input manifest, instrument inputs, Cargo transcript,
binary and pinned native files are rechecked after observation. A dirty override
cannot grant observed-read credit.

Omitting both `--oxidex` and `--build-proof` runs only the bounded native fixture
capture. It preserves the `quicktime_userdata_native_fixtures_v1` output used by
the Rust fixture test and reports zero public observations.

`comparison.json` retains fixture bytes hashes, full native and OxiDex stdout and
stderr bytes and hashes, exact commands, pinned native identity, regenerated
input identities and the build receipt. The validator requires every generated
fixture in both print and raw modes. It compares JSON values without coercing
strings or booleans to numbers. Missing targets or wrong Group1 identities cannot
receive credit. Raw partial receipts survive parsing or admission failures.

The canonical grid has 63 fixtures and 126 mode operations. Successful public
mode observations, fixture occurrences, fully matched fixture occurrences,
distinct source coordinates and distinct Group1 names are separate counts. A
Group1 name receives credit only when at least one fixture for it matches in
both modes. The 17 declarations represent 15 distinct Group1 names; declarations
alone do not contribute observations.

For evidence consumers, `validate_evidence(report, source, ledger, rust)` is the
live import boundary: it authenticates generated artifacts, native files,
producer inputs, binary, transcripts and persisted fixtures before replaying the
claims. `validate_report(report, ledger)` checks the complete grid, typed
transcripts and derived counts, but its caller must separately authenticate the
source and producer context. For cross-host receipt replay,
`validate_build_receipt(proof, expected_snapshot)` verifies embedded Cargo logs
against an independently authenticated expected snapshot without requiring the
producer host's files. It does not establish that expected snapshot itself.
Catalog import is a separate consumer; this verifier does not modify the catalog.

Focused portable evidence-forgery tests (no native processes or Cargo build):

```sh
python3 -m unittest discover -s tools/exiftool-tables \
  -p 'test_verify_quicktime_userdata_reader.py'
```
