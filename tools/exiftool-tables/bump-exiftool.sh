#!/usr/bin/env bash
#
# Tag-machinery overhaul Step 17 (R8): the ExifTool-release bump machinery.
#
#   write pin -> fetch/cache -> capability-probe (version AND a real
#   container check) -> snapshot the pre-bump dump JSON -> regen ALL tiers
#   (regen-all.sh: tier 1 + tier 2) -> verify (verify.py's widened columns)
#   -> triage_bump.py classifies every JSON-to-JSON delta AUTO/EXPR/COND/HAND
#   -> conformance double-run (before/after binaries, same oracle, same
#   corpus) with floors -> gate check (zero group-qualified VALUE
#   regressions; MISSING growth <= EXPR+COND+HAND).
#
# This is the ONE recipe: `just bump-exiftool <version>`. It does not commit
# anything -- a real bump's regenerated files, triage report and gate
# results are left in the working tree (generated files) and under
# --report-dir (the report) for a human/CI to review and commit together,
# re-baselining any ratchet (jpeg-tag-matrix baseline, docs/reference/
# comparison/baseline.json, tag-coverage-analysis.md) ONLY if this run's
# numbers legitimately moved it -- this script does not do that for you.
#
# --dry-run: runs the EXACT same pipeline (same fetch, same probe, same
# regen-all.sh, same verify.py, same triage_bump.py, same conformance
# double-run) but reverts .exiftool-version and every generated file to
# their pre-run committed state before exiting, and asserts the working
# tree is clean afterward. This is what lets the pipeline be exercised
# end-to-end against a real, already-published release delta without ever
# moving the repo's actual pin -- see OVERHAUL_OXIDEX_PLAN.md Step 17's
# "change from the plan" note for why that matters (ExifTool 13.60 does not
# exist; 13.59 is the newest published release, and it is what this repo is
# pinned to).
#
# --from <old-version>: the baseline release the triage report and the
# "before" conformance binary are built against. Defaults to whatever
# .exiftool-version says on entry (the ordinary case: bumping FORWARD from
# the current pin). A real, non-dry-run bump refuses a --from that disagrees
# with the pin it started from -- a real bump is always relative to the
# release actually shipping today. --dry-run lifts that restriction, which
# is what makes a retrospective exercise like "13.58 -> 13.59" possible
# while the repo sits at 13.59 already.
#
# Usage:
#   tools/exiftool-tables/bump-exiftool.sh <new-version> [options]
#     --dry-run                 revert every change before exiting
#     --from <old-version>      baseline release (default: current pin)
#     --report-dir <dir>        where reports land (default: target/bump-report)
#     --corpus <dir>            corpus root for the conformance double-run
#                                (repeatable; default: tests/fixtures)
#     --min-files <n>           conformance floor (default: file count of
#                                the resolved corpus, i.e. "score everything
#                                found" -- see the floor-caveat note below)
#     --min-tags <n>            conformance floor (default: 1000)
#     --skip-conformance        skip the double-run (triage report only)
#
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
cd "$ROOT"

NEW_VERSION="${1:?usage: bump-exiftool.sh <new-version> [--dry-run] [--from <old-version>] [options]}"
shift

DRY_RUN=0
FROM_VERSION=""
REPORT_DIR="$ROOT/target/bump-report"
CORPORA=()
MIN_FILES=""
MIN_TAGS="1000"
SKIP_CONFORMANCE=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run) DRY_RUN=1; shift ;;
        --from) FROM_VERSION="$2"; shift 2 ;;
        --report-dir) REPORT_DIR="$2"; shift 2 ;;
        --corpus) CORPORA+=("$2"); shift 2 ;;
        --min-files) MIN_FILES="$2"; shift 2 ;;
        --min-tags) MIN_TAGS="$2"; shift 2 ;;
        --skip-conformance) SKIP_CONFORMANCE=1; shift ;;
        *) echo "unknown argument: $1" >&2; exit 1 ;;
    esac
done

[[ ${#CORPORA[@]} -gt 0 ]] || CORPORA=("$ROOT/tests/fixtures")

PIN_FILE="$ROOT/.exiftool-version"
[[ -r "$PIN_FILE" ]] || { echo "no ExifTool pin at $PIN_FILE" >&2; exit 1; }
CURRENT_PIN="$(tr -d '[:space:]' < "$PIN_FILE")"
[[ -n "$CURRENT_PIN" ]] || { echo "$PIN_FILE is empty" >&2; exit 1; }

if [[ -z "$FROM_VERSION" ]]; then
    FROM_VERSION="$CURRENT_PIN"
elif [[ "$FROM_VERSION" != "$CURRENT_PIN" && $DRY_RUN -eq 0 ]]; then
    echo "refusing: --from $FROM_VERSION disagrees with the current pin $CURRENT_PIN." >&2
    echo "A real (non---dry-run) bump is always relative to the release actually" >&2
    echo "pinned today. Pass --dry-run to exercise a retrospective delta instead." >&2
    exit 1
fi

if [[ "$NEW_VERSION" == "$CURRENT_PIN" && $DRY_RUN -eq 0 ]]; then
    echo "refusing: target $NEW_VERSION equals the current pin $CURRENT_PIN -- nothing to bump." >&2
    echo "Pass --dry-run to still exercise the pipeline (e.g. a self-check, or a" >&2
    echo "retrospective --from delta)." >&2
    exit 1
fi

mkdir -p "$REPORT_DIR"
CACHE="${OXIDEX_ET_CACHE:-$ROOT/target/exiftool-src}"
mkdir -p "$CACHE"

# Every committed generated file any tier writes. Used to (a) back up the
# forward (NEW) state before the conformance double-run temporarily
# regenerates an OLD comparison build, and (b) revert cleanly at the end of
# a --dry-run.
TIER1_FILES=(
    "src/exiftool_tables/binary_tables.rs"
    "src/composite/tables.rs"
    "src/filetype/tables.rs"
    "src/parsers/specialized/fits/tables.rs"
)
TIER2_FILES=(
    "src/parsers/tiff/makernotes/fujifilm/settings_tables.rs"
    "src/parsers/tiff/makernotes/panasonic/face_tables.rs"
    "src/parsers/tiff/makernotes/pentax/subdir_tables.rs"
    "src/parsers/tiff/makernotes/nikon/af_points.rs"
    "src/parsers/tiff/makernotes/canon/custom_functions2_tables.rs"
    "src/parsers/jpeg/app_segments/infiray_tables.rs"
    "src/parsers/jpeg/app_segments/qualcomm_tables.rs"
    "src/parsers/tiff/makernotes/samsung/lookups.rs"
    "src/parsers/tiff/makernotes/olympus/lookups.rs"
    "src/parsers/tiff/makernotes/lens_data.rs"
    "tools/exiftool-tables/af_points.json"
)
ALL_GENERATED_FILES=("${TIER1_FILES[@]}" "${TIER2_FILES[@]}")

fetch_tree() {
    local v="$1"
    local dir="$CACHE/exiftool-$v"
    if [[ ! -d "$dir" ]]; then
        echo ">> fetching ExifTool $v"
        curl -sSL -o "$CACHE/et-$v.tar.gz" \
            "https://github.com/exiftool/exiftool/archive/refs/tags/$v.tar.gz"
        tar xzf "$CACHE/et-$v.tar.gz" -C "$CACHE"
    fi
    [[ -d "$dir" ]] || { echo "no ExifTool tree at $dir even after fetch (does tag $v exist?)" >&2; exit 1; }
}

# Capability-probe: version string AND a real container check. A matching
# `-ver` alone is not a working oracle (AGENTS.md) -- the pinned tree's
# `exiftool` starts `#!/usr/bin/env perl`, which can find a Homebrew perl
# with no Archive::Zip, silently degrading every ZIP-container format while
# `-ver` still prints the right release. scripts/exiftool_oracle.py already
# encodes exactly this probe (resolve_tree + check_container_support); this
# reuses it rather than re-deriving it, so the two cannot drift apart.
probe_capability() {
    local v="$1" dir="$2"
    python3 - "$v" "$dir" "$ROOT" <<'PY'
import sys
v, tree, root = sys.argv[1], sys.argv[2], sys.argv[3]
sys.path.insert(0, f"{root}/scripts")
import exiftool_oracle as eo

oracle = eo.resolve_tree(tree)
print(f"   probed: {oracle.provenance()}")
if oracle.version != v:
    print(f"capability-probe: tree at {tree} reports version {oracle.version!r}, "
          f"expected {v!r}", file=sys.stderr)
    sys.exit(1)
if oracle.missing_modules:
    print(f"capability-probe: perl {oracle.interpreter} cannot load "
          f"{oracle.missing_modules} -- degraded oracle", file=sys.stderr)
    sys.exit(1)

docx = eo.cache_dir() / "combined-samples" / "OOXML.docx"
if docx.is_file():
    oracle.check_container_support(docx)
    print("   docx container probe: FileType DOCX (Archive::Zip present)")
else:
    print("   (no OOXML.docx sample cached -- container probe skipped; "
          "version+module probe still ran)", file=sys.stderr)
PY
}

dump_tree() {
    local v="$1" out="$2"
    if [[ ! -r "$out" ]]; then
        echo ">> dumping ExifTool $v tables ($out)"
        perl "$HERE/dump_tables.pl" "$CACHE/exiftool-$v/lib" > "$out"
    fi
}

echo "=========================================================="
echo ">> STEP 1: fetch + capability-probe $NEW_VERSION (target)"
echo "=========================================================="
fetch_tree "$NEW_VERSION"
probe_capability "$NEW_VERSION" "$CACHE/exiftool-$NEW_VERSION"

echo "=========================================================="
echo ">> STEP 2: fetch + capability-probe $FROM_VERSION (baseline)"
echo "=========================================================="
fetch_tree "$FROM_VERSION"
probe_capability "$FROM_VERSION" "$CACHE/exiftool-$FROM_VERSION"

OLD_DUMP="$CACHE/tables-$FROM_VERSION.json"
dump_tree "$FROM_VERSION" "$OLD_DUMP"
echo ">> snapshotting pre-bump dump to $REPORT_DIR/tables-$FROM_VERSION.snapshot.json"
cp "$OLD_DUMP" "$REPORT_DIR/tables-$FROM_VERSION.snapshot.json"

# --- point of no return for the working tree: everything from here mutates
# committed files. A --dry-run restores them all in the trap below. ---
RESTORE_NEEDED=0
restore_working_tree() {
    if [[ $RESTORE_NEEDED -eq 1 ]]; then
        echo ">> --dry-run: reverting .exiftool-version and every generated file"
        git -C "$ROOT" checkout -- "$PIN_FILE" "${ALL_GENERATED_FILES[@]}" 2>/dev/null || true
        DIRTY="$(git -C "$ROOT" status --porcelain -- "$PIN_FILE" "${ALL_GENERATED_FILES[@]}")"
        if [[ -n "$DIRTY" ]]; then
            echo "❌ --dry-run could not cleanly revert the working tree:" >&2
            echo "$DIRTY" >&2
            exit 1
        fi
        echo "   working tree confirmed clean (git status --porcelain empty for pin + generated files)"
    fi
}
if [[ $DRY_RUN -eq 1 ]]; then
    trap restore_working_tree EXIT
fi

echo "=========================================================="
echo ">> STEP 3: write pin ($CURRENT_PIN -> $NEW_VERSION)"
echo "=========================================================="
echo "$NEW_VERSION" > "$PIN_FILE"
RESTORE_NEEDED=1

echo "=========================================================="
echo ">> STEP 4: regen ALL tiers (tier 1 + tier 2) against $NEW_VERSION"
echo "=========================================================="
"$HERE/regen-all.sh"

echo "=========================================================="
echo ">> STEP 5: verify committed tables against ExifTool $NEW_VERSION"
echo "=========================================================="
python3 "$HERE/verify.py" "$ROOT/src/exiftool_tables/binary_tables.rs" \
    "$CACHE/exiftool-$NEW_VERSION/lib" --oracle "$HERE/oracle.pl"

NEW_DUMP="$CACHE/tables-$NEW_VERSION.json"
[[ -r "$NEW_DUMP" ]] || dump_tree "$NEW_VERSION" "$NEW_DUMP"

echo "=========================================================="
echo ">> STEP 6: triage -- classify every JSON-to-JSON delta"
echo "=========================================================="
TRIAGE_MD="$REPORT_DIR/triage-$FROM_VERSION-to-$NEW_VERSION.md"
TRIAGE_JSON="$REPORT_DIR/triage-$FROM_VERSION-to-$NEW_VERSION.json"
python3 "$HERE/triage_bump.py" "$OLD_DUMP" "$NEW_DUMP" \
    --markdown-out "$TRIAGE_MD" --json-out "$TRIAGE_JSON"

GATE_STATUS=0
if [[ $SKIP_CONFORMANCE -eq 1 ]]; then
    echo ">> --skip-conformance: not running the before/after double-run"
else
    echo "=========================================================="
    echo ">> STEP 7: conformance double-run"
    echo "=========================================================="
    SCRATCH="$(mktemp -d)"

    echo ">> building AFTER binary (tables: $NEW_VERSION)"
    cargo build --bin oxidex 2>&1 | tail -5
    cp "$ROOT/target/debug/oxidex" "$SCRATCH/oxidex-after"

    echo ">> backing up tier-1 tables (NEW state) before the temporary OLD rebuild"
    BACKUP="$(mktemp -d)"
    for f in "${TIER1_FILES[@]}"; do
        mkdir -p "$BACKUP/$(dirname "$f")"
        cp "$ROOT/$f" "$BACKUP/$f"
    done

    echo ">> temporarily regenerating tier-1 tables for $FROM_VERSION (BEFORE binary)"
    echo "$FROM_VERSION" > "$PIN_FILE"
    "$HERE/regen.sh"
    cargo build --bin oxidex 2>&1 | tail -5
    cp "$ROOT/target/debug/oxidex" "$SCRATCH/oxidex-before"

    echo ">> restoring tier-1 tables and pin to $NEW_VERSION"
    echo "$NEW_VERSION" > "$PIN_FILE"
    for f in "${TIER1_FILES[@]}"; do
        cp "$BACKUP/$f" "$ROOT/$f"
    done
    rm -rf "$BACKUP"

    # Both runs grade against the SAME (new-pin) oracle -- the point is to
    # isolate what the table regeneration changed, not to re-litigate which
    # ExifTool release is correct.
    if [[ -z "$MIN_FILES" ]]; then
        # A raw `find -type f` count over-counts relative to what
        # conformance.py actually scores (its own --recursive walk applies
        # --exclude-ext and de-duplicates overlapping roots), so an exact-
        # match floor here is a false-positive "vacuous run" waiting to
        # happen on the very next run of this same corpus. 90% of the raw
        # count is a floor that still catches a genuinely degraded corpus
        # (see AGENTS.md's degraded-oracle-doesn't-crash warning) without
        # tripping on ordinary filtering.
        COUNTED=0
        for c in "${CORPORA[@]}"; do
            n="$(find "$c" -type f 2>/dev/null | wc -l | tr -d ' ')"
            COUNTED=$((COUNTED + n))
        done
        MIN_FILES=$((COUNTED * 90 / 100))
    fi
    echo ">> corpus: ${CORPORA[*]} (floors: --min-files $MIN_FILES --min-tags $MIN_TAGS)"

    BEFORE_JSON="$REPORT_DIR/conformance-before-$FROM_VERSION.json"
    AFTER_JSON="$REPORT_DIR/conformance-after-$NEW_VERSION.json"

    # --exiftool-dir pins both runs to the exact tree fetched and
    # capability-probed for $NEW_VERSION in STEP 1, rather than trusting
    # whatever release the shared /tmp oracle cache happens to hold -- a
    # real bump to a version nobody has warmed that cache for yet would
    # otherwise hit exiftool_oracle's own (correct) SkewError refusal.
    echo ">> conformance: BEFORE ($FROM_VERSION tier-1 tables)"
    python3 "$HERE/conformance.py" "${CORPORA[@]}" --recursive \
        --exclude-ext sh,md,py,json \
        --exiftool-dir "$CACHE/exiftool-$NEW_VERSION" \
        --oxidex "$SCRATCH/oxidex-before" \
        --min-files "$MIN_FILES" --min-tags "$MIN_TAGS" \
        --json-out "$BEFORE_JSON" > "$REPORT_DIR/conformance-before.log"
    tail -20 "$REPORT_DIR/conformance-before.log"

    echo ">> conformance: AFTER ($NEW_VERSION tier-1 tables, i.e. this run's regen)"
    python3 "$HERE/conformance.py" "${CORPORA[@]}" --recursive \
        --exclude-ext sh,md,py,json \
        --exiftool-dir "$CACHE/exiftool-$NEW_VERSION" \
        --oxidex "$SCRATCH/oxidex-after" \
        --min-files "$MIN_FILES" --min-tags "$MIN_TAGS" \
        --json-out "$AFTER_JSON" > "$REPORT_DIR/conformance-after.log"
    tail -20 "$REPORT_DIR/conformance-after.log"

    echo "=========================================================="
    echo ">> STEP 8: gate check"
    echo "=========================================================="
    GATE_MD="$REPORT_DIR/gates-$FROM_VERSION-to-$NEW_VERSION.md"
    if ! python3 "$HERE/bump_conformance_gate.py" "$BEFORE_JSON" "$AFTER_JSON" "$TRIAGE_JSON" \
            --report-out "$GATE_MD"; then
        GATE_STATUS=1
        echo "❌ one or more Step 17 gates FAILED -- see $GATE_MD" >&2
    else
        echo "✅ Step 17 gates passed -- see $GATE_MD"
    fi
    rm -rf "$SCRATCH"
fi

echo
echo ">> done. Reports under $REPORT_DIR:"
ls -la "$REPORT_DIR"
echo
if [[ $DRY_RUN -eq 1 ]]; then
    echo "--dry-run: .exiftool-version and every generated file will be reverted on exit."
else
    echo "Live bump: .exiftool-version now reads $NEW_VERSION and every generated tier is"
    echo "regenerated in the working tree. Nothing has been committed. Before committing:"
    echo "  - run 'just verify-tables', 'cargo test --lib', 'cargo test --test integration'"
    echo "  - review the triage report ($TRIAGE_MD) and gate report"
    echo "  - re-baseline any ratchet (jpeg-tag-matrix baseline, docs/reference/comparison/"
    echo "    baseline.json, docs/reference/tag-coverage-analysis.md) ONLY if this run's"
    echo "    numbers legitimately moved it, and commit that alongside"
fi

exit "$GATE_STATUS"
