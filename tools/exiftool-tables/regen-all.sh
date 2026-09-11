#!/usr/bin/env bash
#
# Run both currently wired ExifTool-table generation tiers against the
# pinned release. Remaining source-resolution/transaction gaps are in README.md.
#
# Tier 1 (tools/exiftool-tables/regen.sh) produces binary_tables.rs, the
# filetype tables, Composite definitions and FITS keywords. Tier 2 is
# everything downstream of it that regen.sh never touched: the MakerNote
# sub-directory tables (codegen_subdirs.py), the Nikon AF-point name grids
# (dump_af_points.pl + codegen_af_points.py), the six one-off
# `scripts/gen_*.pl` transcriptions (Leica lens types, Canon custom
# functions, InfiRay/Qualcomm APPn tables, Samsung and Olympus lookups) and
# the four Macintosh CJK charset tables (tier 2e).
#
# Before this script existed, a bump only ever ran tier 1 -- `just
# regen-tables` calls regen.sh directly, and nothing called the tier-2
# scripts as a group at all. A bump could therefore refresh binary_tables.rs
# to a new ExifTool release while every tier-2 file quietly stayed on the
# old one, and nothing in the repo could tell: each generator individually
# looked fine, verify.py only ever checked tier 1, and the tier-2 outputs
# carry no version stamp of their own to compare. That is intra-repo
# mixed-release skew, and it is invisible from either tier alone.
#
# Both tiers use the same explicit library and interpreter. Tier 2 refreshes
# its dump even when run alone, so a readable cache is never identity evidence.
#
# Usage:
#   tools/exiftool-tables/regen-all.sh              # tier 1 + tier 2
#   tools/exiftool-tables/regen-all.sh --tier2-only  # skip regen.sh (fast path
#                                                     # once tier 1 is already
#                                                     # current -- CI's diff
#                                                     # step uses this so it
#                                                     # does not re-verify
#                                                     # tier 1 twice)
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
source "$HERE/artifact-env.sh"

TIER1=1
if [[ "${1:-}" == "--tier2-only" ]]; then
    TIER1=0
    shift
fi

PIN_FILE="$ROOT/.exiftool-version"
[[ -r "$PIN_FILE" ]] || { echo "no ExifTool pin at $PIN_FILE" >&2; exit 1; }
PIN="$(tr -d '[:space:]' < "$PIN_FILE")"
[[ -n "$PIN" ]] || { echo "$PIN_FILE is empty" >&2; exit 1; }

# Select one library and interpreter for both tiers.
CACHE="${OXIDEX_ET_CACHE:-$ROOT/target/exiftool-src}"
PERL="${EXIFTOOL_PERL:-$(command -v perl)}"
export EXIFTOOL_PERL="$PERL"
if [[ "$TIER1" == "1" ]]; then
    begin_regeneration all "$CACHE"
else
    begin_regeneration 2 "$CACHE"
fi
if [[ -n "${OXIDEX_EXIFTOOL_LIB:-}" ]]; then
    LIB="$OXIDEX_EXIFTOOL_LIB"
else
    LIB="$CACHE/exiftool-$PIN/lib"
    if [[ ! -d "$LIB" ]]; then
        SHARED_ROOT="${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}/exiftool"
        if [[ -r "$SHARED_ROOT/lib/Image/ExifTool.pm" ]] \
            && grep -q "VERSION *= *['\"]$PIN['\"]" "$SHARED_ROOT/lib/Image/ExifTool.pm"; then
            echo ">> reusing shared oracle tree at $SHARED_ROOT (already ExifTool $PIN)"
            mkdir -p "$CACHE"
            ln -sfn "$SHARED_ROOT" "$CACHE/exiftool-$PIN"
        fi
    fi
fi

if [[ "$TIER1" == "1" ]]; then
    echo "=========================================================="
    echo ">> TIER 1: binary_tables.rs, filetypes, Composite, FITS"
    echo "=========================================================="
    # A missing default tree may still be fetched by tier 1. Explicit trees
    # fail if absent; they never fall back to an unrelated cached source.
    if [[ -d "$LIB" ]]; then export OXIDEX_EXIFTOOL_LIB="$LIB"; fi
    "$HERE/regen.sh"
fi

[[ -d "$LIB" ]] || { echo "no ExifTool lib at $LIB even after tier 1" >&2; exit 1; }

# Verify $LIB actually is $PIN before tier 2 reads a single field from it --
# the same refusal ExiftoolPin.pm applies to every gen_*.pl script, applied
# here once for dump_tables.pl/dump_af_points.pl/codegen_subdirs.py, which
# read Perl directly rather than through that module. A plain (non-GNU) `sed`
# ships on macOS without `\s`/`\d`, so this uses `grep -E` with bracket
# classes rather than a Perl-flavoured regex.
LIB_VERSION="$(grep -m1 -E "^[[:space:]]*\\\$VERSION[[:space:]]*=" "$LIB/Image/ExifTool.pm" 2>/dev/null \
    | sed -E "s/^[^'\"]*['\"]([^'\"]*)['\"].*/\\1/")"
if [[ "$LIB_VERSION" != "$PIN" ]]; then
    echo "refusing: $LIB is ExifTool '${LIB_VERSION:-<unreadable>}' but $PIN_FILE pins $PIN" >&2
    exit 1
fi
JSON="$CACHE/tables-$PIN.json"
echo ">> refreshing the tier-2 dump from $LIB with $PERL"
mkdir -p "$CACHE"
"$PERL" "$HERE/dump_tables.pl" "$LIB" > "$JSON"

# Every tier-2 generator that shells out to Perl reads $OXIDEX_EXIFTOOL_LIB in
# preference to its own default (scripts/lib/ExiftoolPin.pm), so this is the
# single point where "which ExifTool" is decided for the rest of the run.
export OXIDEX_EXIFTOOL_LIB="$LIB"

echo "=========================================================="
echo ">> TIER 2a: MakerNote sub-directory tables (codegen_subdirs.py)"
echo "=========================================================="

# module -> output path -> ordered --table list. This is the manifest
# regen.sh's tier 1 never had a reason to carry (codegen_subdirs.py is a
# narrow, per-vendor tool, not a whole-corpus one), and until now it lived
# nowhere at all -- these three files existed with no committed record of the
# exact invocation that produced them. Table lists were recovered by reading
# each committed file's own tag names back out.
gen_subdir() {
    local module="$1" out="$2"; shift 2
    local args=(--module "$module" -o "$out" --allow-skip)
    for t in "$@"; do args+=(--table "$t"); done
    python3 "$HERE/codegen_subdirs.py" "$JSON" "${args[@]}"
}

gen_subdir FujiFilm "$(artifact_path fujifilm)" \
    PrioritySettings FocusSettings AFCSettings DriveSettings

gen_subdir Panasonic "$(artifact_path panasonic)" \
    FaceDetInfo FaceRecInfo

PENTAX_OUT="$(artifact_path pentax)"
gen_subdir Pentax "$PENTAX_OUT" \
    SRInfo2 FaceInfo AWBInfo TimeInfo LensCorr FlashInfo KelvinWB EVStepInfo \
    FacePos FaceSize LevelInfo WBLevels LensInfoQ AFInfo BatteryInfo TempInfo \
    ShotInfo FilterInfo CameraSettings

# One documented post-generation patch: `pentax.rs`'s top-level
# HometownCity/DestinationCity tags (0x0023/0x0024) reuse this exact
# `%pentaxCities` transcription via `SeparateTable => 'City'`, so the
# constant was widened from `const` to `pub(crate) const` by hand. The
# generator itself always emits a private `const`; re-applying the same
# textual patch here (instead of teaching codegen_subdirs.py a visibility
# override it would use exactly once) is what keeps this rerun-able and
# byte-identical rather than silently reverting a real dependency each time.
python3 - "$PENTAX_OUT" <<'PATCH'
import sys
path = sys.argv[1]
text = open(path).read()
old = "const PENTAX_CONV6: &[(i64, &str)] = &["
new_comment = (
    "// Made `pub(crate)` (generator emits `const`) so `pentax.rs` can reuse this\n"
    "// exact transcription of `%pentaxCities` for the top-level 0x0023/0x0024\n"
    "// `HometownCity`/`DestinationCity` tags, which carry the same\n"
    "// `SeparateTable => 'City'` PrintConv as the `TimeInfo` sub-fields below.\n"
)
if "pub(crate) const PENTAX_CONV6" in text:
    sys.exit(0)  # already patched
if old not in text:
    sys.exit("PENTAX_CONV6 patch site not found -- codegen_subdirs.py output shape changed")
open(path, "w").write(text.replace(old, new_comment + "pub(crate) " + old, 1))
PATCH

echo "=========================================================="
echo ">> TIER 2b: Nikon AF-point name grids"
echo "=========================================================="
"$PERL" "$HERE/dump_af_points.pl" "$LIB/Image/ExifTool/Nikon.pm" "$(artifact_path af-points-json)"
python3 "$HERE/codegen_af_points.py" "$(artifact_path af-points-json)" \
    "$(artifact_path af-points)"

echo "=========================================================="
echo ">> TIER 2c: scripts/gen_*.pl one-off transcriptions"
echo "=========================================================="

run_gen() {
    local script="$1" out="$2"
    "$PERL" "$ROOT/scripts/$script" > "$out"
}

run_gen gen_canon_custom_functions2.pl \
    "$(artifact_path canon-custom)"
run_gen gen_infiray_tables.pl \
    "$(artifact_path infiray)"
run_gen gen_qualcomm_tables.pl \
    "$(artifact_path qualcomm)"
run_gen gen_samsung_lookups.pl \
    "$(artifact_path samsung)"
run_gen gen_olympus_lookups.pl \
    "$(artifact_path olympus)"

# gen_leica_lens_types.pl is the one generator with no dedicated output file:
# LEICA_LENS_TYPES lives inside lens_data.rs, a file several OTHER
# manufacturers' lens databases also share. splice_leica.py replaces just
# that array in place; see its header for why a whole-file overwrite does
# not apply here.
LEICA_RAW="$(mktemp)"
REGEN_CLEANUP_FILE="$LEICA_RAW"
"$PERL" "$ROOT/scripts/gen_leica_lens_types.pl" > "$LEICA_RAW"
python3 "$HERE/splice_leica.py" "$LEICA_RAW" "$(artifact_path leica)"

echo "=========================================================="
echo ">> TIER 2d: bespoke sony::binary_data-DSL tables"
echo "=========================================================="
# docs/TRANSCRIPTION.md's "Honest limits" section names six generated files
# that had no committed generator at all -- each targets a bespoke, per-file
# Rust DSL hand-matched against ExifTool's Condition/RawConv/ValueConv/
# PrintConv text, closer in spirit to gen_canon_custom_functions2.pl's
# hard-coded expression dictionary than to codegen_subdirs.py's general
# ProcessBinaryData walk. Two of the six -- the smallest -- were reconstructed
# this way; the other four (sony/plain_tables.rs, sony/enciphered_tables.rs,
# nikon/settings_tables.rs, nikon/encrypted_tables.rs) remain unreconstructed
# and are still called out in that section, each its own similarly-sized
# project.
python3 "$HERE/gen_sony_main_extra_tables.py" "$JSON" \
    -o "$(artifact_path sony-main)"
python3 "$HERE/gen_minolta_a100_tables.py" "$JSON" \
    -o "$(artifact_path minolta-a100)"

echo "=========================================================="
echo ">> TIER 2e: Macintosh CJK charset tables (TrueType name records)"
echo "=========================================================="
# The four `src/parsers/font/mac_charset/mac_*.rs` tables had a committed
# generator all along -- and it was named by nothing. Not regen.sh, not this
# script, not the justfile, not ci.yml (found by the tag-machinery
# reconciliation, docs/TAG_MACHINERY_RECONCILIATION.md defect 5). They are
# live code, not dead output: `src/parsers/font/mac_charset.rs:25-28`
# declares all four as modules and `for_mac_encoding` (:76-79) dispatches
# Mac platform encoding IDs 1/2/3/25 into them for `ttf.rs:353`. So a bump
# would have left ExifTool's own MacJapanese/MacChineseTW/MacKorean/
# MacChineseCN tables frozen at whatever release they were transcribed from,
# silently, while every neighbouring table moved -- exactly the tier-2 skew
# this script exists to end.
#
# The generator reads the `.pm` files directly (they are Perl hash literals,
# not runtime tables, so there is no dump to route through) and writes
# beside itself; its output is byte-identical to what is committed once
# rustfmt has run, which is why all four implicit outputs are in artifacts.py and why
# CI's rerun-and-diff step can gate it.
python3 "$ROOT/src/parsers/font/mac_charset/generate_tables.py" \
    "$LIB/Image/ExifTool/Charset"

echo "=========================================================="
echo ">> TIER 2f: GeoTIFF, DICOM and lens alternatives"
echo "=========================================================="
python3 "$HERE/gen_geotiff_printconv.py" --exiftool-dir "$LIB/.." \
    --perl "$PERL" --out "$(artifact_path geotiff)"
python3 "$HERE/gen_dicom_dict.py" --exiftool-dir "$LIB/.." \
    --perl "$PERL" --out "$(artifact_path dicom)"
"$PERL" "$HERE/dump_lens_alternatives.pl" --exiftool-dir "$LIB/.." \
    --out "$(artifact_path lens-alternatives)"

echo "=========================================================="
echo ">> formatting tier-2 output"
echo "=========================================================="
cd "$ROOT"
format_artifacts 2

echo ">> independently verifying complete GeoTIFF, DICOM and lens facts"
python3 "$HERE/verify_geotiff.py" --exiftool-dir "$LIB/.." \
    --perl "$PERL" --rust-file "$(artifact_path geotiff)"
python3 "$HERE/verify_dicom_dict.py" --exiftool-dir "$LIB/.." \
    --perl "$PERL" --input "$(artifact_path dicom)"
python3 "$HERE/verify_lens_alternatives.py" "$(artifact_path lens-alternatives)" \
    --exiftool-dir "$LIB/.." --perl "$PERL"

echo
echo ">> done: selected tiers regenerated from ExifTool $PIN at $LIB"
