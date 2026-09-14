# Native write acceptance matrix

`native_write_matrix.py` records real, default-option `SetNewValue` and
`WriteInfo` behavior for `EXIF:HostComputer` and `IFD0:HostComputer`. It runs
eight scalar operations (insert, update, growth, shrinkage, delete,
defined-empty, UTF-8, and embedded NUL) against little-endian TIFF,
big-endian TIFF, and the committed small JPEG carrier.

The output JSON retains the selected Perl/library identity, scrubbed Perl
environment, raw native stdout/stderr, API return values and errors, every
input/output path, actual IFD type/count/value bytes, and carrier-image checks.
TIFF validation is endian-aware and rejects truncated/out-of-bounds IFDs; it
also reports whether short values are actually inline. JPEG validation finds
the Exif APP1 and hashes complete SOS-to-end image bytes separately.

The instrument uses the shared checkout fence before native calls. Its header
and JSON record the source commit, dirty state and explicit override, plus the
instrument and JPEG fixture hashes. Both the main module and writer must exist
inside the selected library; native probes also check every loaded ExifTool
module's resolved path before and after writing to reject a mixed installation.

For every non-delete row, the matrix also requires the exact pinned ExifTool
13.59 HostComputer result: TIFF ASCII type 2, a count equal to the explicitly
constructed scalar bytes plus one terminator, and those complete value bytes.
The UTF-8 case is a flagged Perl character scalar and embedded-NUL is a bytes
scalar. This is an observed native acceptance contract, not a rule for the
generator or an OxiDex conformance claim. Unit negative controls mutate type,
count, and value bytes independently and require rejection.

The matrix reads `.exiftool-version` and refuses a stale acceptance baseline,
even if the supplied library still matches 13.59. It also refuses a library
that differs from the pin before producing rows. A future release rehearsal
must preserve its native identity and capture
a separate expectation; it must not be tested as a regression against this
13.59 baseline.

Run it only with the pinned native inputs:

```sh
python3 tools/exiftool-tables/native_write_matrix.py \
  --perl "$EXIFTOOL_PERL" \
  --lib "$OXIDEX_PINNED_EXIFTOOL" \
  --jpeg-base tests/fixtures/jpeg/tag_matrix_base.jpg \
  --output <evidence-root>/native-write-matrix.json
```

This is a native behavioral baseline. It makes no OxiDex parity claim and does
not cover generated Rust writes, non-default writer options, arbitrary TIFFs,
or general tag types.
