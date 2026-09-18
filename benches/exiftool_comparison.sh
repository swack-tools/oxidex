#!/usr/bin/env bash
#
# OxiDex vs pinned Perl ExifTool comparative benchmark suite (hyperfine).
#
# Scenarios (all CLI wall-clock, hyperfine --warmup 5 --runs 30, no shell, both
# commands in the same invocation):
#   1.  Single file extraction  -- tests/fixtures/jpeg/simple/sample_with_exif.jpg
#                                  (a 112-byte stub, kept for continuity with the
#                                  historical table) and t/images/Canon.jpg
#   2.  Batch processing        -- 1000 replicated JPEG fixtures, `-r`
#   3.  Write one tag           -- `-EXIF:Artist=...` / `-Artist=... -overwrite_original`
#   4.  Format detection        -- one JPEG, default text output
#   5.  Corpus, one invocation  -- the pinned tree's 194-file t/images, `-j -a -G1`
#                                  (oxidex once with its default rayon parallelism
#                                  and once with RAYON_NUM_THREADS=1; ExifTool is
#                                  single-threaded)
#
# The instrument is named before any number (benches/instrument_check.py):
#   * the oxidex binary is resolved through scripts/instrument.py's
#     resolve_binary() and its staleness note is printed -- a missing or stale
#     binary refuses rather than silently timing something else;
#   * the ExifTool oracle is scripts/exiftool_oracle.py's: its -ver must equal
#     .exiftool-version AND it must pass the OOXML.docx capability probe;
#   * load1/5/15 and the top CPU consumers are recorded before and after every
#     scenario and stamped into the report (no absolute load gate: the host this
#     runs on idles at load1 4-6 from processes outside our control; pass
#     OXIDEX_MAX_LOAD=N to refuse above N).
#
# Usage:
#   ./benches/exiftool_comparison.sh [path/to/oxidex]      # default target/release/oxidex
# Environment:
#   EXIFTOOL_PERL, EXIFTOOL_CACHE_DIR, EXIFTOOL  -- see scripts/exiftool_oracle.py
#   OXIDEX_ALLOW_DIRTY_TREE=1                    -- record anyway (stamped in the header)
#   OXIDEX_MAX_LOAD=N, OXIDEX_ALLOW_LOAD=1       -- optional load gate and its override
#   HYPERFINE_WARMUP=5 HYPERFINE_RUNS=30          -- hyperfine sampling (defaults shown; both
#                                                   commands in one invocation, exactly N runs each)
#
# Output: benches/benchmark_results.md (new run on top; everything from the
# `<!-- historical -->` marker down is preserved verbatim) and
# benches/benchmark_results.json.

set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
REQUESTED_BIN="${1:-${OXIDEX_BIN:-$PROJECT_ROOT/target/release/oxidex}}"
FIXTURE_DIR="$PROJECT_ROOT/tests/fixtures"
TEMP_DIR=$(mktemp -d)
RESULTS_MD="$SCRIPT_DIR/benchmark_results.md"
RESULTS_JSON="$SCRIPT_DIR/benchmark_results.json"
HF=(hyperfine --warmup "${HYPERFINE_WARMUP:-5}" --runs "${HYPERFINE_RUNS:-30}" -N)
RUN_LOG="$SCRIPT_DIR/benchmark_results.log"

trap 'rm -rf "$TEMP_DIR"' EXIT

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}OxiDex vs ExifTool benchmark suite${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# ---------------------------------------------------------------------------
# Instrument: name the binary, the oracle, the commit and the load, or refuse.
# ---------------------------------------------------------------------------
check_prerequisites() {
    for tool in hyperfine jq python3 bc; do
        if ! command -v "$tool" &> /dev/null; then
            echo -e "${RED}Error: $tool not found${NC}"; exit 1
        fi
    done
    # instrument_check.py prints the `=== instrument: ... ===` header to stderr
    # and KEY=VALUE lines to stdout; a non-zero exit is a refusal, and its
    # reason is already on stderr.
    local kv
    kv="$(python3 "$SCRIPT_DIR/instrument_check.py" "$REQUESTED_BIN" ${OXIDEX_MAX_LOAD:+--max-load "$OXIDEX_MAX_LOAD"})"
    eval "$kv"
    # shellcheck disable=SC2153 # set by the eval above
    read -r -a EXIFTOOL <<< "$EXIFTOOL_CMD"
    [ "$(cat "$PROJECT_ROOT/.exiftool-version")" = "$EXIFTOOL_VERSION" ] \
        || { echo -e "${RED}Error: oracle $EXIFTOOL_VERSION != .exiftool-version${NC}"; exit 2; }
    [ -d "$EXIFTOOL_CORPUS" ] || { echo -e "${RED}Error: corpus $EXIFTOOL_CORPUS missing${NC}"; exit 1; }
    [ -d "$FIXTURE_DIR/jpeg/simple" ] || { echo -e "${RED}Error: fixtures missing at $FIXTURE_DIR${NC}"; exit 1; }
    echo -e "${GREEN}✓ hyperfine:${NC} $(hyperfine --version)"
    echo -e "${GREEN}✓ oxidex:${NC} $OXIDEX_BIN ($OXIDEX_VERSION, sha256 ${OXIDEX_SHA256:0:12}...)"
    [ -n "$OXIDEX_STALENESS" ] && echo -e "${YELLOW}⚠ $OXIDEX_STALENESS${NC}"
    echo -e "${GREEN}✓ exiftool:${NC} $EXIFTOOL_PROVENANCE"
    echo -e "${GREEN}✓ load (1/5/15):${NC} $LOADAVG"
    echo -e "${GREEN}✓ top CPU:${NC} $TOP_CPU"
    : > "$RUN_LOG"
    echo ""
}

load1() { uptime | sed -E 's/.*load averages?: ([0-9.]+).*/\1/'; }
# Machine state around a timed run: load1/5/15 and the top 5 CPU consumers,
# to the console and to benchmark_results.log (kept beside the report).
snap() {
    { echo "[$1] $(uptime)"; ps -Ao pcpu,args -r | sed -n '1,6p' | sed "s/^/[$1]   /" | cut -c1-140; } | tee -a "$RUN_LOG"
}
stamp_load() { snap "$SCENARIO before"; }
stamp_after() { snap "$SCENARIO after"; }

# ---------------------------------------------------------------------------
# Scenario 1: single file
# ---------------------------------------------------------------------------
benchmark_single_file() {
    SCENARIO="single"; echo -e "${BLUE}Benchmark 1: Single File Extraction (JPEG with EXIF)${NC}"; stamp_load
    local stub="$FIXTURE_DIR/jpeg/simple/sample_with_exif.jpg"
    local canon="$EXIFTOOL_CORPUS/Canon.jpg"
    [ -f "$stub" ] && [ -f "$canon" ] || { echo -e "${RED}Error: test files missing${NC}"; exit 1; }
    LOAD_1a=$(load1)
    "${HF[@]}" --export-markdown "$TEMP_DIR/single_file.md" --export-json "$TEMP_DIR/single_file.json" \
        "${EXIFTOOL[*]} $stub" "$OXIDEX_BIN $stub"
    LOAD_1b=$(load1)
    "${HF[@]}" --export-markdown "$TEMP_DIR/single_canon.md" --export-json "$TEMP_DIR/single_canon.json" \
        "${EXIFTOOL[*]} -j -a -G1 $canon" "$OXIDEX_BIN -j -a -G1 $canon"
    stamp_after; echo ""
}

# ---------------------------------------------------------------------------
# Scenario 2: batch (1000 replicated JPEG fixtures)
# ---------------------------------------------------------------------------
benchmark_batch_processing() {
    SCENARIO="batch"; echo -e "${BLUE}Benchmark 2: Batch Processing (1000+ JPEG Files)${NC}"; stamp_load
    local batch_dir="$TEMP_DIR/batch_test"; mkdir -p "$batch_dir"
    local fixtures=(); while IFS= read -r f; do fixtures+=("$f"); done < <(find "$FIXTURE_DIR/jpeg" -name "*.jpg" -type f | sort)
    [ ${#fixtures[@]} -gt 0 ] || { echo -e "${RED}Error: no JPEG fixtures${NC}"; exit 1; }
    local target=1000 idx=0
    while [ $idx -lt $target ]; do
        for f in "${fixtures[@]}"; do
            cp "$f" "$batch_dir/test_$(printf "%04d" $idx).jpg"; idx=$((idx + 1))
            [ $idx -ge $target ] && break
        done
    done
    BATCH_COUNT=$(find "$batch_dir" -name "*.jpg" | wc -l | tr -d ' ')
    echo "  ${#fixtures[@]} source fixtures replicated to $BATCH_COUNT files"
    LOAD_2=$(load1)
    "${HF[@]}" --export-markdown "$TEMP_DIR/batch.md" --export-json "$TEMP_DIR/batch.json" \
        "${EXIFTOOL[*]} -r $batch_dir" "$OXIDEX_BIN -r $batch_dir"
    stamp_after; echo ""
}

# ---------------------------------------------------------------------------
# Scenario 3: write one tag
# ---------------------------------------------------------------------------
benchmark_write_operation() {
    SCENARIO="write"; echo -e "${BLUE}Benchmark 3: Write Operation (Modify EXIF Tag)${NC}"; stamp_load
    local src="$FIXTURE_DIR/jpeg/simple/sample_with_exif.jpg" wdir="$TEMP_DIR/write_test"
    mkdir -p "$wdir"
    LOAD_3=$(load1)
    "${HF[@]}" --export-markdown "$TEMP_DIR/write.md" --export-json "$TEMP_DIR/write.json" \
        --prepare "cp $src $wdir/test_perl.jpg" "${EXIFTOOL[*]} -Artist=BenchmarkTest -overwrite_original $wdir/test_perl.jpg" \
        --prepare "cp $src $wdir/test_rust.jpg" "$OXIDEX_BIN -EXIF:Artist=BenchmarkTest $wdir/test_rust.jpg"
    stamp_after; echo ""
}

# ---------------------------------------------------------------------------
# Scenario 4: format detection (CLI-level; see benches/parse_benchmarks.rs for the library-level one)
# ---------------------------------------------------------------------------
benchmark_format_detection() {
    SCENARIO="detection"; echo -e "${BLUE}Benchmark 4: Format Detection Overhead${NC}"; stamp_load
    local ddir="$TEMP_DIR/detection_test"; mkdir -p "$ddir"
    cp "$FIXTURE_DIR/jpeg/simple/sample_with_exif.jpg" "$ddir/test.jpg"
    LOAD_4=$(load1)
    "${HF[@]}" --export-markdown "$TEMP_DIR/detection.md" --export-json "$TEMP_DIR/detection.json" \
        "${EXIFTOOL[*]} $ddir/test.jpg" "$OXIDEX_BIN $ddir/test.jpg"
    stamp_after; echo ""
}

# ---------------------------------------------------------------------------
# Scenario 5: the pinned tree's t/images corpus in one invocation, -j -a -G1
# ---------------------------------------------------------------------------
benchmark_corpus() {
    SCENARIO="corpus"; echo -e "${BLUE}Benchmark 5: t/images corpus, one invocation (-j -a -G1)${NC}"; stamp_load
    local files=(); while IFS= read -r f; do files+=("$f"); done < <(find "$EXIFTOOL_CORPUS" -maxdepth 1 -type f | sort)
    CORPUS_COUNT=${#files[@]}
    echo "  $CORPUS_COUNT files under $EXIFTOOL_CORPUS"
    LOAD_5=$(load1)
    "${HF[@]}" --export-markdown "$TEMP_DIR/corpus.md" --export-json "$TEMP_DIR/corpus.json" \
        "${EXIFTOOL[*]} -j -a -G1 ${files[*]}" "$OXIDEX_BIN -j -a -G1 ${files[*]}"
    LOAD_5b=$(load1)
    RAYON_NUM_THREADS=1 "${HF[@]}" --export-markdown "$TEMP_DIR/corpus_1t.md" --export-json "$TEMP_DIR/corpus_1t.json" \
        "${EXIFTOOL[*]} -j -a -G1 ${files[*]}" "$OXIDEX_BIN -j -a -G1 ${files[*]}"
    LOAD_END=$(load1); stamp_after; echo ""
}

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------
# One table per scenario from hyperfine's JSON: median, min, mean ± σ, runs.
# Ratios are ExifTool/oxidex on medians and on minima (the minimum is the
# least load-sensitive statistic on a host that is never idle).
table() { # json
    jq -r --arg tmp "$TEMP_DIR/" --arg root "$PROJECT_ROOT/" '
      def ms: . * 1000 | . * 10 | round / 10;
      "| Command | Median | Min | Mean ± σ | Max | Runs |", "|:---|---:|---:|---:|---:|---:|",
      (.results[] | "| `" + (.command | gsub("/tmp/oxidex-perl538-build-[^ ]+/perl5.38.2 -I[^ ]+ [^ ]+/exiftool"; "exiftool") | gsub("[^ ]+/target/release/oxidex"; "oxidex") | gsub("/tmp/oxidex-exiftool-cache/exiftool/t/images/"; "") | gsub($tmp; "") | gsub($root; "") | if (split(" ") | length) > 8 then (split(" ")[0:4] | join(" ")) + " <" + ((split(" ") | length) - 4 | tostring) + " files>" else . end) + "` | " + (.median | ms | tostring) + " ms | " + (.min | ms | tostring) + " ms | " + (.mean | ms | tostring) + " ± " + (.stddev | ms | tostring) + " | " + (.max | ms | tostring) + " ms | " + (.times | length | tostring) + " |")' "$1"
}
speedup() { # json -> ratios on median and on min
    jq -r '"**ExifTool / oxidex**: " + (.results[0].median / .results[1].median * 100 | round / 100 | tostring) + "x on medians, " + (.results[0].min / .results[1].min * 100 | round / 100 | tostring) + "x on minima (σ/mean: exiftool " + (.results[0].stddev / .results[0].mean * 100 | round | tostring) + " %, oxidex " + (.results[1].stddev / .results[1].mean * 100 | round | tostring) + " %)"' "$1"
}
median_ms() { printf '%.1f' "$(echo "$(jq -r ".results[$2].median" "$1")*1000" | bc -l)"; }

compile_results() {
    echo -e "${YELLOW}Compiling benchmark results...${NC}"
    local historical="" 
    if [ -f "$RESULTS_MD" ] && grep -q '<!-- historical -->' "$RESULTS_MD"; then
        historical="$(sed -n '/<!-- historical -->/,$p' "$RESULTS_MD")"
    fi
    {
        cat <<MD
# OxiDex Performance Benchmarks

Comparative CLI wall-clock benchmarks, OxiDex vs the pinned Perl ExifTool.
Generated by \`benches/exiftool_comparison.sh\`; every number below is
attributable to the instrument named here (see AGENTS.md, "Name the
instrument, or the measurement is not evidence").

## Instrument

- **Commit**: \`$GIT_COMMIT\` ($GIT_DESCRIBE$( [ "$GIT_DIRTY" = 1 ] && echo ', DIRTY TREE -- OXIDEX_ALLOW_DIRTY_TREE=1'))
- **oxidex**: \`$OXIDEX_BIN\` -- $OXIDEX_VERSION, sha256 \`$OXIDEX_SHA256\`, shipped \`[profile.release]\` (fat LTO, codegen-units=1, panic=abort)
$( [ -n "$OXIDEX_STALENESS" ] && echo "- **Staleness note**: $OXIDEX_STALENESS" )
- **ExifTool**: $EXIFTOOL_PROVENANCE; \`.exiftool-version\` = $(cat "$PROJECT_ROOT/.exiftool-version"); OOXML.docx probe -> DOCX
- **Machine**: $MACHINE; $(sw_vers -productName 2>/dev/null || uname -s) $(sw_vers -productVersion 2>/dev/null || uname -r)
- **Load (1/5/15 min)**: $LOADAVG at preflight; load1 at each scenario start: 1a $LOAD_1a, 1b $LOAD_1b, 2 $LOAD_2, 3 $LOAD_3, 4 $LOAD_4, 5 $LOAD_5 / $LOAD_5b; $LOAD_END at the end. This host is never idle (top CPU at preflight: $TOP_CPU); the full \`uptime\` and top-5 CPU lines before and after every scenario are in \`benches/benchmark_results.log\`. Both commands of a scenario run in one hyperfine invocation so they see the same background; ratios are given on medians and on minima.$( [ -n "${OXIDEX_MAX_LOAD:-}" ] && echo " Load gate: $OXIDEX_MAX_LOAD${OXIDEX_ALLOW_LOAD:+ (overridden)}")
- **Date**: $(date -u +%Y-%m-%dT%H:%M:%SZ)
- **hyperfine**: $(hyperfine --version), \`--warmup ${HYPERFINE_WARMUP:-5} --runs ${HYPERFINE_RUNS:-30} -N\`, both commands per invocation

## Benchmark Results

### 1. Single File Extraction (JPEG with EXIF)

1a. \`tests/fixtures/jpeg/simple/sample_with_exif.jpg\` (a 112-byte stub; kept for continuity with the historical table -- it measures process startup, not parsing):

$(table "$TEMP_DIR/single_file.json")

$(speedup "$TEMP_DIR/single_file.json")

1b. \`t/images/Canon.jpg\` from the pinned tree, \`-j -a -G1\`:

$(table "$TEMP_DIR/single_canon.json")

$(speedup "$TEMP_DIR/single_canon.json")

### 2. Batch Processing ($BATCH_COUNT JPEG files, \`-r\`)

oxidex reads a directory with rayon across all cores; ExifTool is single-threaded.

$(table "$TEMP_DIR/batch.json")

$(speedup "$TEMP_DIR/batch.json")

### 3. Write Operation (Modify EXIF:Artist)

$(table "$TEMP_DIR/write.json")

$(speedup "$TEMP_DIR/write.json")

### 4. Format Detection (one JPEG, default output)

$(table "$TEMP_DIR/detection.json")

$(speedup "$TEMP_DIR/detection.json")

### 5. Corpus: $CORPUS_COUNT-file \`t/images\` in one invocation, \`-j -a -G1\`

$(table "$TEMP_DIR/corpus.json")

(oxidex with default rayon parallelism) $(speedup "$TEMP_DIR/corpus.json")

oxidex with \`RAYON_NUM_THREADS=1\` (single-core, like ExifTool), same invocation as an ExifTool run:

$(table "$TEMP_DIR/corpus_1t.json")

$(speedup "$TEMP_DIR/corpus_1t.json") -- the like-for-like, per-core ratio.

## Reproducing

\`\`\`bash
cargo build --release                       # the shipped profile; the script refuses a missing binary
export EXIFTOOL_PERL=/path/to/perl5.38.2    # a perl with Archive::Zip; see scripts/exiftool_oracle.py
./benches/exiftool_comparison.sh            # refuses a missing binary, a dirty tree, or an oracle that is not 13.59+DOCX
\`\`\`

Library-level criterion benchmarks: \`cargo bench\` (results under \`target/criterion/\`).

MD
        [ -n "$historical" ] && printf '%s\n' "$historical"
    } > "$RESULTS_MD.tmp"
    mv "$RESULTS_MD.tmp" "$RESULTS_MD"

    jq -n \
        --arg commit "$GIT_COMMIT" --arg describe "$GIT_DESCRIBE" --arg dirty "$GIT_DIRTY" \
        --arg bin "$OXIDEX_BIN" --arg sha "$OXIDEX_SHA256" --arg oxv "$OXIDEX_VERSION" --arg stale "$OXIDEX_STALENESS" \
        --arg et "$EXIFTOOL_PROVENANCE" --arg etv "$EXIFTOOL_VERSION" --arg machine "$MACHINE" --arg load "$LOADAVG" --arg top "$TOP_CPU" \
        --slurpfile single "$TEMP_DIR/single_file.json" --slurpfile single_canon "$TEMP_DIR/single_canon.json" \
        --slurpfile batch "$TEMP_DIR/batch.json" --slurpfile write "$TEMP_DIR/write.json" \
        --slurpfile detection "$TEMP_DIR/detection.json" --slurpfile corpus "$TEMP_DIR/corpus.json" \
        --slurpfile corpus_1t "$TEMP_DIR/corpus_1t.json" \
        '{instrument: {commit: $commit, describe: $describe, dirty: ($dirty == "1"), oxidex: $bin, oxidex_sha256: $sha,
                       oxidex_version: $oxv, staleness_note: $stale, exiftool: $et, exiftool_version: $etv,
                       machine: $machine, loadavg_at_preflight: $load, top_cpu_at_preflight: $top},
          single_file: $single[0], single_canon: $single_canon[0], batch: $batch[0], write: $write[0],
          detection: $detection[0], corpus: $corpus[0], corpus_1thread: $corpus_1t[0]}' > "$RESULTS_JSON"

    echo -e "${GREEN}✓ Results written to:${NC}"
    echo "  - $RESULTS_MD"
    echo "  - $RESULTS_JSON"
}

main() {
    check_prerequisites
    benchmark_single_file
    benchmark_batch_processing
    benchmark_write_operation
    benchmark_format_detection
    benchmark_corpus
    compile_results
    echo -e "${GREEN}Benchmark suite completed.${NC}"
}

main "$@"
