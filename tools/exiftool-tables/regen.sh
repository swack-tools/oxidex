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
source "$HERE/artifact-env.sh"

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
LIB="${OXIDEX_EXIFTOOL_LIB:-$CACHE/exiftool-$VERSION/lib}"
PERL="${EXIFTOOL_PERL:-$(command -v perl)}"
export EXIFTOOL_PERL="$PERL"
mkdir -p "$CACHE"
begin_regeneration 1 "$CACHE"
OUT="$(artifact_path binary)"
# Slice I-1: the IFD-style (`Exif::ProcessExif`) tables, one generator run,
# second output file. Written together with $OUT so the two can never come
# from different dumps (their EXIFTOOL_VERSION stamps are tested equal).
IFD_OUT="$(artifact_path ifd)"
IFD_IDENTITY_LEDGER="$(artifact_path ifd-identity-ledger)"
# Generate and verify inactive keyed definitions too; publishing their source
# facts does not enable a runtime parser route.
KEYED_OUT="$(artifact_path keyed)"
SERIAL_OUT="$(artifact_path serial)"
JSON="$CACHE/tables-$VERSION.json"
HYDRATED_JSON="$CACHE/tables-hydrated-reader-$VERSION.json"
EXPR_LEDGER="$(artifact_path expr-ledger)"
VALUE_CONV_LEDGER="$(artifact_path value-ledger)"

if [[ ! -d "$LIB" && -z "${OXIDEX_EXIFTOOL_LIB:-}" ]]; then
    echo ">> fetching ExifTool $VERSION"
    mkdir -p "$CACHE"
    curl -sSL -o "$CACHE/et.tar.gz" \
        "https://github.com/exiftool/exiftool/archive/refs/tags/$VERSION.tar.gz"
    tar xzf "$CACHE/et.tar.gz" -C "$CACHE"
fi
[[ -d "$LIB" ]] || { echo "no ExifTool lib at $LIB" >&2; exit 1; }

echo ">> extracting tag tables from Perl symbol table"
# Keep writer capture separate from reader hydration. Hydration attaches a
# large native object graph; traversing it again in the writer projection is
# unnecessary. Both captures are fresh, using this exact interpreter/library,
# and each consumer records the digest of its own source document.
"$PERL" "$HERE/dump_tables.pl" "$LIB" > "$JSON"
echo ">> extracting effective hydrated reader tables"
"$PERL" "$HERE/dump_tables.pl" --reader-only --hydrated-layouts "$LIB" > "$HYDRATED_JSON"

echo ">> coverage analysis"
python3 "$HERE/analyze.py" "$JSON"

# The expression oracle builds the runtime crate. Refresh source-only address
# types before that build so a new runtime consumer can use newly generated
# identity fields. This step emits lookup operands, not expression conversions;
# conversion generation below still requires the oracle PASS ledger.
echo
echo ">> generating authenticated SetNewValue address operands"
ADDRESS_ROWS="$CACHE/setnewvalue-address-rows-$VERSION.json"
ADDRESS_REPORT="$CACHE/setnewvalue-address-report-$VERSION.json"
ADDRESS_OBSERVATIONS="$CACHE/setnewvalue-address-observations-$VERSION.json"
ADDRESS_OWNERSHIP="$(artifact_path setnewvalue-ownership-ledger)"
python3 "$HERE/setnewvalue_addressing.py" "$JSON" \
    --rows "$ADDRESS_ROWS" --report "$ADDRESS_REPORT"
"$PERL" "$HERE/setnewvalue_address_probe.pl" "$LIB" "$ADDRESS_ROWS" > "$ADDRESS_OBSERVATIONS"
ADDRESS_LEDGER_ARGS=(--ownership-ledger "$ADDRESS_OWNERSHIP")
if [[ ! -f "$ADDRESS_OWNERSHIP" ]]; then
    # Bootstrap is an explicit one-time creation path only. Later regenerations
    # validate and carry forward the committed ownership history.
    ADDRESS_LEDGER_ARGS=(--bootstrap-ownership-ledger)
fi
python3 "$HERE/setnewvalue_address_rust_codegen.py" "$JSON" "$ADDRESS_OBSERVATIONS" \
    --output "$(artifact_path setnewvalue-address-rules)" \
    --report "$(artifact_path setnewvalue-address-ledger)" \
    --write-ownership-ledger "$ADDRESS_OWNERSHIP" "${ADDRESS_LEDGER_ARGS[@]}"


echo
echo ">> differential expression oracle (must PASS before conversion rollout)"
# R2's non-negotiable ordering: codegen receives a PASS-only ledger, never a
# grammar-shaped expression.  verify_exprs.py capability-probes the pinned
# Perl library before evaluating any conversion (Image/ExifTool.pm:9378).
python3 "$HERE/verify_exprs.py" "$JSON" \
    --perl "$PERL" --et-lib "$LIB" --ledger-out "$EXPR_LEDGER"

echo
echo ">> generating Rust"
python3 "$HERE/codegen.py" "$JSON" -o "$OUT" --ifd-out "$IFD_OUT" \
    --ifd-identity-ledger-out "$IFD_IDENTITY_LEDGER" --keyed-out "$KEYED_OUT" \
    --expr-ledger "$EXPR_LEDGER" --value-conv-ledger-out "$VALUE_CONV_LEDGER"

echo
echo ">> generating QuickTime ItemList declarations from the fresh hydrated dump"
# Unlike the bounded fixture used by its unit tests, this invocation consumes
# this regeneration's $HYDRATED_JSON. Ordinary new source rows therefore enter the
# declaration artifact without hand-editing the captured fixture.
OXIDEX_ALLOW_DIRTY_TREE=1 python3 "$HERE/quicktime_generated_specs.py" \
    --dump "$HYDRATED_JSON" --replace
OXIDEX_ALLOW_DIRTY_TREE=1 python3 "$HERE/quicktime_keys_specs.py" \
    --dump "$HYDRATED_JSON" --replace
OXIDEX_ALLOW_DIRTY_TREE=1 python3 "$HERE/quicktime_userdata_specs.py" \
    --dump "$HYDRATED_JSON" --replace

echo
echo ">> generating inactive serial-directory facts"
python3 "$HERE/serial_directory.py" "$JSON" \
    --output "$CACHE/serial-$VERSION.json" --rust-output "$SERIAL_OUT"

echo
echo ">> generating source-derived scalar writer helper operands"
python3 "$HERE/scalar_helper_codegen.py" "$JSON" \
    --output "$(artifact_path scalar-helpers)" \
    --report "$(artifact_path scalar-helper-ledger)"

echo
echo ">> generating source-derived table validation operands"
python3 "$HERE/checkexif_rust_codegen.py" "$JSON" \
    --output "$(artifact_path checkexif-rules)" \
    --report "$(artifact_path checkexif-ledger)"

echo
echo ">> generating source-derived input-normalization operands"
python3 "$HERE/sanitize_rust_codegen.py" "$JSON" \
    --output "$(artifact_path sanitize-rules)" \
    --report "$(artifact_path sanitize-ledger)"

echo
echo ">> generating source-derived inverse-conversion operands"
python3 "$HERE/convinv_rust_codegen.py" "$JSON" \
    --output "$(artifact_path convinv-rules)" \
    --report "$(artifact_path convinv-ledger)"

echo
echo ">> generating static inverse-conversion row inputs"
python3 "$HERE/convinv_row_codegen.py" "$JSON" \
    --output "$(artifact_path convinv-rows)" \
    --report "$(artifact_path convinv-row-ledger)"

echo
echo ">> generating source-derived final scalar writer operands"
python3 "$HERE/final_scalar_stage.py" "$JSON" \
    --output "$(artifact_path tiff-scalar-final-rules)" \
    --report "$(artifact_path tiff-scalar-final-ledger)"

echo
echo ">> capturing and generating source-derived mandatory directory defaults"
MANDATORY_FACT="$CACHE/mandatory-defaults-$VERSION.json"
"$PERL" "$HERE/capture_exif_mandatory_fact.pl" "$LIB" > "$MANDATORY_FACT"
python3 "$HERE/mandatory_defaults_codegen.py" "$MANDATORY_FACT" \
    --writer-tables "$JSON" --selected-perl "$PERL" \
    --output "$(artifact_path mandatory-default-rules)" \
    --report "$(artifact_path mandatory-default-ledger)"

echo
echo ">> capturing and generating source-derived raw JFIF property operands"
RAW_JFIF_FACT="$CACHE/raw-jfif-$VERSION.json"
"$PERL" "$HERE/capture_raw_jfif_fact.pl" "$LIB" > "$RAW_JFIF_FACT"
python3 "$HERE/raw_jfif_codegen.py" "$RAW_JFIF_FACT" \
    --selected-perl "$PERL" \
    --output "$(artifact_path raw-jfif-rules)" \
    --report "$(artifact_path raw-jfif-ledger)"


echo
echo ">> generating composed public SetNewValue migration ownership"
# This is deliberately a separate public fence.  It consumes the same fresh
# selected dump as the address and final-scalar stages, but owns only their
# authenticated intersection.  The larger address inventory remains legacy
# routing scope until a matching final recipe is generated.
PUBLIC_MIGRATION_REPORT="$CACHE/setnewvalue-public-migration-report-$VERSION.json"
PUBLIC_MIGRATION_LEDGER="$(artifact_path setnewvalue-public-migration-ledger)"
PUBLIC_MIGRATION_ARGS=(--prior-ledger "$PUBLIC_MIGRATION_LEDGER")
if [[ ! -f "$PUBLIC_MIGRATION_LEDGER" ]]; then
    # The first public migration boundary is an explicit bootstrap.  Later
    # source upgrades validate and carry prior migrated identities forward.
    PUBLIC_MIGRATION_ARGS=(--bootstrap)
fi
python3 "$HERE/setnewvalue_public_migration_ledger.py" "$JSON" \
    --output "$(artifact_path setnewvalue-public-migration-rules)" \
    --report "$PUBLIC_MIGRATION_REPORT" \
    --write-ledger "$PUBLIC_MIGRATION_LEDGER" "${PUBLIC_MIGRATION_ARGS[@]}"

echo
echo ">> generating authenticated fresh-JPEG byte-order operands"
BYTE_ORDER_OBSERVATIONS="$CACHE/fresh-jpeg-byte-order-$VERSION.json"
python3 "$HERE/fresh_jpeg_byte_order_native.py" \
    --perl "$PERL" --exiftool-dir "$LIB" --output "$BYTE_ORDER_OBSERVATIONS"
python3 "$HERE/fresh_jpeg_byte_order_codegen.py" "$BYTE_ORDER_OBSERVATIONS" \
    --writer-tables "$JSON" \
    --output "$(artifact_path fresh-jpeg-byte-order-rules)" \
    --report "$(artifact_path fresh-jpeg-byte-order-ledger)"

echo
echo ">> extracting file-identification tables"
"$PERL" "$HERE/dump_filetypes.pl" "$LIB" > "$CACHE/filetypes-$VERSION.json"
python3 "$HERE/codegen_filetypes.py" "$CACHE/filetypes-$VERSION.json" \
    -o "$(artifact_path filetypes)"

echo
echo ">> generating Composite definitions"
python3 "$HERE/codegen_composite.py" "$JSON" -o "$(artifact_path composite)" \
    --generated-out "$(artifact_path composite-compute)"

echo
echo ">> generating FITS keyword names"
python3 "$HERE/codegen_fits.py" "$JSON" \
    -o "$(artifact_path fits)"

echo
echo ">> formatting generated sources"
# rustfmt is part of generation, not an afterthought: without it the committed
# files (which do get formatted) differ from freshly generated ones on every
# run, and a generator whose output churns cannot be reviewed in a diff.
format_artifacts 1

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
OXIDEX_ALLOW_DIRTY_TREE=1 python3 "$HERE/verify.py" "$OUT" "$LIB" --oracle "$HERE/oracle.pl" \
    --keyed-generated "$KEYED_OUT" \
    --word-processor Image::ExifTool::CanonCustom::ProcessCanonCustom

echo
echo ">> verifying serial definitions and omission inventory"
python3 "$HERE/verify_serial_directory.py" "$SERIAL_OUT" "$JSON"

echo
echo ">> done: $OUT, $IFD_OUT, $KEYED_OUT and $SERIAL_OUT"
