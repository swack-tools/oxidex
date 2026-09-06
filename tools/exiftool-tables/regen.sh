#!/usr/bin/env bash
#
# Extract ExifTool's tag tables, generate Rust, and verify the result against
# ExifTool itself. Fails loudly rather than emitting unverified tables.
#
# Usage:
#   tools/exiftool-tables/regen.sh [exiftool-version]
#
# The ExifTool source is downloaded if not already cached. We need the .pm
# sources, not the installed binary: the tables are Perl data structures, and
# `exiftool -listx` flattens away the layout information that makes them
# useful (see src/exiftool_tables/mod.rs).

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"

# The release comes from the repo pin, never from a literal here. A hardcoded
# default drifted 29 releases behind `.exiftool-version` and, because verify.py
# used to take its expected release from the generated artifact's own stamp,
# nothing could report it: `just regen-tables` with no argument re-froze the
# whole transcription set at the stale release and still passed verification.
PIN_FILE="$ROOT/.exiftool-version"
[[ -r "$PIN_FILE" ]] || { echo "no ExifTool pin at $PIN_FILE" >&2; exit 1; }
PIN="$(tr -d '[:space:]' < "$PIN_FILE")"
[[ -n "$PIN" ]] || { echo "$PIN_FILE is empty" >&2; exit 1; }

VERSION="${1:-$PIN}"
if [[ "$VERSION" != "$PIN" ]]; then
    # Fail before doing the work rather than after: regen.sh writes the
    # committed tables, so generating them from a release the repo does not
    # grade against produces exactly the skew this pin exists to prevent.
    # verify.py would refuse at the end anyway; saying so now costs less.
    echo "refusing to transcribe ExifTool $VERSION while $PIN_FILE pins $PIN." >&2
    echo "To move the repo to $VERSION, update .exiftool-version first, then" >&2
    echo "re-run this script with no argument." >&2
    exit 1
fi
CACHE="${OXIDEX_ET_CACHE:-$ROOT/target/exiftool-src}"
LIB="$CACHE/exiftool-$VERSION/lib"
OUT="$ROOT/src/exiftool_tables/binary_tables.rs"
JSON="$CACHE/tables-$VERSION.json"
EXPR_LEDGER="$ROOT/tools/exiftool-tables/expr_oracle_ledger.json"
VALUE_CONV_LEDGER="$ROOT/tools/exiftool-tables/value_conv_ledger.json"

if [[ ! -d "$LIB" ]]; then
    echo ">> fetching ExifTool $VERSION"
    mkdir -p "$CACHE"
    curl -sSL -o "$CACHE/et.tar.gz" \
        "https://github.com/exiftool/exiftool/archive/refs/tags/$VERSION.tar.gz"
    tar xzf "$CACHE/et.tar.gz" -C "$CACHE"
fi
[[ -d "$LIB" ]] || { echo "no ExifTool lib at $LIB" >&2; exit 1; }

echo ">> extracting tag tables from Perl symbol table"
perl "$HERE/dump_tables.pl" "$LIB" > "$JSON"

echo ">> coverage analysis"
python3 "$HERE/analyze.py" "$JSON"

echo
echo ">> differential expression oracle (must PASS before conversion rollout)"
# R2's non-negotiable ordering: codegen receives a PASS-only ledger, never a
# grammar-shaped expression.  verify_exprs.py capability-probes the pinned
# Perl library before evaluating any conversion (Image/ExifTool.pm:9378).
python3 "$HERE/verify_exprs.py" "$JSON" \
    --perl "$(command -v perl)" --et-lib "$LIB" --ledger-out "$EXPR_LEDGER"

echo
echo ">> generating Rust"
python3 "$HERE/codegen.py" "$JSON" -o "$OUT" \
    --expr-ledger "$EXPR_LEDGER" --value-conv-ledger-out "$VALUE_CONV_LEDGER"

echo
echo ">> extracting file-identification tables"
perl "$HERE/dump_filetypes.pl" "$LIB" > "$CACHE/filetypes-$VERSION.json"
python3 "$HERE/codegen_filetypes.py" "$CACHE/filetypes-$VERSION.json" \
    -o "$ROOT/src/filetype/tables.rs"

echo
echo ">> generating Composite definitions"
python3 "$HERE/codegen_composite.py" "$JSON" -o "$ROOT/src/composite/tables.rs"

echo
echo ">> generating FITS keyword names"
python3 "$HERE/codegen_fits.py" "$JSON" \
    -o "$ROOT/src/parsers/specialized/fits/tables.rs"

echo
echo ">> formatting generated sources"
# rustfmt is part of generation, not an afterthought: without it the committed
# files (which do get formatted) differ from freshly generated ones on every
# run, and a generator whose output churns cannot be reviewed in a diff.
cargo fmt -- "$OUT" "$ROOT/src/composite/tables.rs" "$ROOT/src/filetype/tables.rs" \
    "$ROOT/src/parsers/specialized/fits/tables.rs" \
    2>/dev/null || echo "   (rustfmt unavailable; output left unformatted)"

echo
echo ">> verifying generated Rust against ExifTool (independent path)"
# The tree is dirty by construction at this point -- this script just wrote
# $OUT and both ledgers -- and verify.py refuses a dirty tree unless told
# otherwise (scripts/instrument.py, Step 30). That refusal exists so a
# published NUMBER cannot be attributed to an uncommitted tree; this step is a
# pre-commit check of the artifact regen just produced, which is exactly the
# case the override was written for, and verify.py's own header records the
# override so nothing about the provenance is hidden. Without this variable
# `just regen-tables` has ended in the refusal text and exit 1 -- not in a
# verdict -- on every run since the check landed (reproduced 2026-09-06 on the
# i7). A generator must never commit on the operator's behalf, so the
# alternative ordering is not available.
OXIDEX_ALLOW_DIRTY_TREE=1 python3 "$HERE/verify.py" "$OUT" "$LIB" --oracle "$HERE/oracle.pl"

echo
echo ">> done: $OUT"
