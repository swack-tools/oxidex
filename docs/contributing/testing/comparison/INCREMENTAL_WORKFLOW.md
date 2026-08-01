# Incremental parity workflow

The full ExifTool corpus is useful for release validation, but it is a poor
inner loop: most of the oracle output does not change while one parser fix is
being developed. The focused workflow keeps the expensive work out of the
critical path.

```sh
# Refresh only the formats touched by the current fix.
EXIFTOOL_SAMPLES=/tmp/oxidex-exiftool-cache/combined-samples \
EXIFTOOL_PATH=/tmp/oxidex-exiftool-cache/exiftool/exiftool \
bash scripts/compare_incremental.sh --formats JPEG,TIFF

# Run the local code/tests gate without requiring a downloaded corpus.
bash scripts/verify_parity_fast.sh --skip-corpus
```

The comparison binary persists independent ExifTool and OxiDex extraction
results beside the corpus. ExifTool results are keyed by ExifTool version and
the corpus signature; OxiDex results are keyed by the current executable hash
and the same signature. A focused invocation uses `--reuse-output` to retain
untouched format reports, then rebuilds a deterministic ranked gap queue across
all retained formats. The queue favors missing tags, then value differences,
then OxiDex-only tags; repeated gaps across formats receive a higher priority.

The `fixloop` Cargo profile is intentionally cheap to rebuild. The wrapper
checks source timestamps before building, so repeated runs that only change the
corpus cache do not relink the binary. `--with-markdown` is available when a
human-readable report is needed; JSON-only mode is faster for iteration.

Individual files are processed in parallel after their paths are sorted, and
batch ExifTool invocations run concurrently. Results are folded back in path
order, preserving canonical values and byte-for-byte reproducible reports.

Generated `ProcessBinaryData` tables expose a shared runtime decoder and a
`tables_for_module()` registry iterator. New format readers can consume raw
fields as `TagValue` without duplicating integer/string/rational conversion;
`PrintConv` remains an explicit opt-in because unsupported `ValueConv` steps
must never be guessed.

Use the full `just compare-exiftool-full` and `just verify-tables` commands
before merging or publishing a release. The incremental workflow is designed
for the tight loop between those full gates.
