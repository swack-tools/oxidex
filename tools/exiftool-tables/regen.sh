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

VERSION="${1:-13.30}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
CACHE="${OXIDEX_ET_CACHE:-$ROOT/target/exiftool-src}"
LIB="$CACHE/exiftool-$VERSION/lib"
OUT="$ROOT/src/exiftool_tables/binary_tables.rs"
JSON="$CACHE/tables-$VERSION.json"

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
echo ">> generating Rust"
python3 "$HERE/codegen.py" "$JSON" -o "$OUT"

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
python3 "$HERE/verify.py" "$OUT" "$LIB" --oracle "$HERE/oracle.pl"

echo
echo ">> done: $OUT"
