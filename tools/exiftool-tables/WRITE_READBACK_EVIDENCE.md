# Public write/readback evidence

This opt-in sidecar records successful **mutating public write operations** and
**distinct Group1 tag names** separately. It does not turn declarations, native
writes alone, deletes, no-ops, or merely present tags into observed OxiDex writes.
The existing matrix JSON and behavior stay unchanged without the readback options.

Run from a clean committed checkout. Evidence/output directories must be outside
the checkout. A dirty override cannot authorize observed credit. Run build/native
commands inside the session's allocated shared-lock controller, with jobs capped
at four; this helper does not allocate a host or acquire that lock itself.

1. Capture a fresh Cargo-reported lib-test binary with unchanged runtime inputs:

```bash
CARGO_TARGET_DIR=../shared-target python3 tools/exiftool-tables/write_readback_evidence.py \
  --build-proof-dir ../evidence/readback-build
```

The wrapper runs `cargo test --lib --all-features --no-run --message-format=json
--jobs 4`, records its complete log, and writes `build-proof.json`. Read its
`binary.path` for `--test-binary`; do not substitute a convenient older binary.

2. Run the existing matrix plus native readback:

```bash
readback_binary=$(python3 -c 'import json; from pathlib import Path; print(json.loads(Path("../evidence/readback-build/build-proof.json").read_text())["binary"]["path"])')
python3 tools/exiftool-tables/generated_tiff_write_matrix.py \
  --route public-api \
  --test-binary "$readback_binary" \
  --perl /tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2 \
  --lib /tmp/oxidex-exiftool-cache/exiftool/lib \
  --jpeg-base tests/fixtures/jpeg/tag_matrix_base.jpg \
  --output ../evidence/public-write-matrix.json \
  --readback-source ../evidence/cache/tables-13.59.json \
  --readback-build-proof ../evidence/readback-build/build-proof.json \
  --readback-evidence ../evidence/public-write-readback.json
```

The source dump must reproduce the current final/public ledgers and Rust files.
The existing `--ledger` and `--rules` arguments remain available, but alternate
Rust rules must match the compiled runtime artifact. The source directory operand
is joined to the public capture. Readback is restricted to the repository pin;
version rehearsal does not silently inherit this evidence contract.

Each qualifying output pair is read with the explicit pinned Perl and selected
CLI: `perl -I<lib> <tree>/exiftool -config '' -j -a -G1 -s <file>`.
Full stdout/stderr bytes, hashes, commands, and return codes are retained. The
source-selected physical directory determines the Group1 name; EXIF aliases do
not invent `EXIF:Name` readback identities. Target values must be present and
identical in both JSON transcripts, and the existing complete wire/carrier
comparison must pass. The target's wire value must differ from its seeded value.
A missing readback target earns no credit. Read errors, ambiguous duplicate JSON
keys, value mismatches, or source drift refuse the sidecar.

3. Add the sidecar to the existing catalog join invocation:

```text
--writer-read-evidence ../evidence/public-write-readback.json
```

Keep all five existing writer source/final/public inputs. The importer independently
replays their compilers, validates the current runtime manifest and artifacts,
rehashes the saved build, source, fixture/output, matrix and driver files, replays
wire comparisons, and derives counts from read transcripts. Keep these durable
files available; the sidecar is deliberately not a self-asserted standalone claim.

Catalog credit requires both the source coordinate and the exact catalog
`Group1:Name`. Observing `IFD1:Artist` therefore leaves the `IFD0:Artist` catalog
row unobserved. `observed_write_group1_names` lists only names matching that
catalog row; `alternate_context_write_group1_names` retains other observed
physical contexts without granting row credit. The `write_readback` count block
preserves all successful operations and distinct Group1 names, and separately
reports `catalog_matched_write_operations` and `alternate_context_write_operations`.
Reader and writer evidence remain separate.

Portable verification:

```bash
python3 -m unittest discover -s tools/exiftool-tables -p test_write_readback_evidence.py
python3 -m unittest discover -s tools/exiftool-tables -p test_join_catalog_hydrated.py
python3 -m unittest discover -s tools/exiftool-tables -p test_generated_tiff_write_matrix.py
```

These tests use byte/transcript fixtures and mutation cases. They establish the
instrument's validation behavior; they are not themselves native write evidence.
