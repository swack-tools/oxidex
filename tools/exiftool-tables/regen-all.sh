#!/usr/bin/env bash
#
# Run EVERY committed ExifTool-table generator in this repo -- both
# generation tiers -- against the SAME pinned ExifTool source tree.
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
# The fix is running everything from ONE resolved source tree in one process
# (see LIB below) rather than trusting N independently-cached ExifTool
# checkouts to agree, plus `just verify-tables` (tier 1) and this script's own
# rerun-and-diff (tier 2, wired into CI's verify-tables job) so a skew is a
# red check, not a fact nobody happened to notice.
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

TIER1=1
if [[ "${1:-}" == "--tier2-only" ]]; then
    TIER1=0
    shift
fi

PIN_FILE="$ROOT/.exiftool-version"
[[ -r "$PIN_FILE" ]] || { echo "no ExifTool pin at $PIN_FILE" >&2; exit 1; }
PIN="$(tr -d '[:space:]' < "$PIN_FILE")"
[[ -n "$PIN" ]] || { echo "$PIN_FILE is empty" >&2; exit 1; }

# One resolved tree for the whole run, so tier 1 and tier 2 cannot end up
# reading two different checkouts of "the same" release.
#
# If the caller already pointed at a specific tree via $OXIDEX_EXIFTOOL_LIB
# (CI's verify-tables job does this: it already fetched exiftool-$PIN/lib for
# the tier-1 verify.py step above, so tier 2 reuses that exact checkout rather
# than fetching a second copy), that wins outright -- it is verified below the
# same way ExiftoolPin.pm verifies it for every tier-2 Perl script, so this is
# not a trust-it-blindly fallback.
#
# Otherwise this resolves through regen.sh's own cache (target/exiftool-src by
# default, override with $OXIDEX_ET_CACHE). If that cache does not have $PIN
# yet but the shared oracle tree every other harness in this repo reads from
# ($EXIFTOOL_CACHE_DIR/exiftool, default /tmp/oxidex-exiftool-cache/exiftool)
# already does, symlink it in rather than re-fetching the same tarball;
# regen.sh only fetches when its own $LIB is still missing.
# $CACHE is also where tier 2 stashes the JSON dump it shares with tier 1
# (below), independent of which branch resolved $LIB.
CACHE="${OXIDEX_ET_CACHE:-$ROOT/target/exiftool-src}"
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
    "$HERE/regen.sh"
    # regen.sh resolves its own $LIB independently (it does not read
    # $OXIDEX_EXIFTOOL_LIB); re-derive tier 2's LIB from the pin's default
    # cache so the two agree, unless the caller explicitly overrode it above.
    [[ -n "${OXIDEX_EXIFTOOL_LIB:-}" ]] || LIB="${OXIDEX_ET_CACHE:-$ROOT/target/exiftool-src}/exiftool-$PIN/lib"
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
if [[ ! -r "$JSON" ]]; then
    echo ">> tier-1 JSON dump missing ($JSON); producing it for tier 2"
    mkdir -p "$CACHE"
    perl "$HERE/dump_tables.pl" "$LIB" > "$JSON"
fi

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

gen_subdir FujiFilm "$ROOT/src/parsers/tiff/makernotes/fujifilm/settings_tables.rs" \
    PrioritySettings FocusSettings AFCSettings DriveSettings

gen_subdir Panasonic "$ROOT/src/parsers/tiff/makernotes/panasonic/face_tables.rs" \
    FaceDetInfo FaceRecInfo

PENTAX_OUT="$ROOT/src/parsers/tiff/makernotes/pentax/subdir_tables.rs"
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
perl "$HERE/dump_af_points.pl" "$LIB/Image/ExifTool/Nikon.pm" "$HERE/af_points.json"
python3 "$HERE/codegen_af_points.py" "$HERE/af_points.json" \
    "$ROOT/src/parsers/tiff/makernotes/nikon/af_points.rs"

echo "=========================================================="
echo ">> TIER 2c: scripts/gen_*.pl one-off transcriptions"
echo "=========================================================="

run_gen() {
    local script="$1" out="$2"
    perl "$ROOT/scripts/$script" > "$out"
}

run_gen gen_canon_custom_functions2.pl \
    "$ROOT/src/parsers/tiff/makernotes/canon/custom_functions2_tables.rs"
run_gen gen_infiray_tables.pl \
    "$ROOT/src/parsers/jpeg/app_segments/infiray_tables.rs"
run_gen gen_qualcomm_tables.pl \
    "$ROOT/src/parsers/jpeg/app_segments/qualcomm_tables.rs"
run_gen gen_samsung_lookups.pl \
    "$ROOT/src/parsers/tiff/makernotes/samsung/lookups.rs"
run_gen gen_olympus_lookups.pl \
    "$ROOT/src/parsers/tiff/makernotes/olympus/lookups.rs"

# gen_leica_lens_types.pl is the one generator with no dedicated output file:
# LEICA_LENS_TYPES lives inside lens_data.rs, a file several OTHER
# manufacturers' lens databases also share. splice_leica.py replaces just
# that array in place; see its header for why a whole-file overwrite does
# not apply here.
LEICA_RAW="$(mktemp)"
trap 'rm -f "$LEICA_RAW"' EXIT
perl "$ROOT/scripts/gen_leica_lens_types.pl" > "$LEICA_RAW"
python3 "$HERE/splice_leica.py" "$LEICA_RAW" "$ROOT/src/parsers/tiff/makernotes/lens_data.rs"

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
    -o "$ROOT/src/parsers/tiff/makernotes/sony/main_extra_tables.rs"
python3 "$HERE/gen_minolta_a100_tables.py" "$JSON" \
    -o "$ROOT/src/parsers/tiff/makernotes/minolta_a100_tables.rs"

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
# rustfmt has run, which is why it is in the cargo fmt list below and why
# CI's rerun-and-diff step can gate it.
python3 "$ROOT/src/parsers/font/mac_charset/generate_tables.py" \
    "$LIB/Image/ExifTool/Charset"

echo "=========================================================="
echo ">> formatting tier-2 output"
echo "=========================================================="
cd "$ROOT"
cargo fmt -- \
    src/parsers/tiff/makernotes/fujifilm/settings_tables.rs \
    src/parsers/tiff/makernotes/panasonic/face_tables.rs \
    src/parsers/tiff/makernotes/pentax/subdir_tables.rs \
    src/parsers/tiff/makernotes/nikon/af_points.rs \
    src/parsers/tiff/makernotes/canon/custom_functions2_tables.rs \
    src/parsers/jpeg/app_segments/infiray_tables.rs \
    src/parsers/jpeg/app_segments/qualcomm_tables.rs \
    src/parsers/tiff/makernotes/sony/main_extra_tables.rs \
    src/parsers/tiff/makernotes/minolta_a100_tables.rs \
    src/parsers/tiff/makernotes/samsung/lookups.rs \
    src/parsers/tiff/makernotes/olympus/lookups.rs \
    src/parsers/tiff/makernotes/lens_data.rs \
    src/parsers/font/mac_charset/mac_japanese.rs \
    src/parsers/font/mac_charset/mac_chinese_tw.rs \
    src/parsers/font/mac_charset/mac_korean.rs \
    src/parsers/font/mac_charset/mac_chinese_cn.rs \
    2>/dev/null || echo "   (rustfmt unavailable; output left unformatted)"

echo
echo ">> done: tier 1 + tier 2 regenerated from ExifTool $PIN at $LIB"
