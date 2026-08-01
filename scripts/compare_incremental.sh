#!/usr/bin/env bash
# Run a focused, cache-aware ExifTool/OxiDex comparison.

set -euo pipefail

usage() {
    cat >&2 <<'EOF'
Usage: compare_incremental.sh [options]

Options:
  --samples DIR          ExifTool sample corpus (or EXIFTOOL_SAMPLES)
  --exiftool PATH        ExifTool executable (or EXIFTOOL_PATH)
  --formats LIST         Comma-separated format names to refresh
  --output PATH          JSON report to reuse/write
  --markdown-dir PATH    Markdown output directory
  --baseline PATH        Optional regression baseline
  --queue-limit N        Number of ranked gaps to retain (default: 50)
  --profile NAME         Cargo profile (default: fixloop)
  --with-markdown        Generate markdown (the default is JSON-only)
EOF
    exit 2
}

samples="${EXIFTOOL_SAMPLES:-}"
exiftool="${EXIFTOOL_PATH:-}"
formats=""
output=""
markdown_dir=""
baseline=""
queue_limit=50
profile=fixloop
with_markdown=0

while (($#)); do
    case "$1" in
        --samples) samples="$2"; shift 2 ;;
        --exiftool) exiftool="$2"; shift 2 ;;
        --formats) formats="$2"; shift 2 ;;
        --output) output="$2"; shift 2 ;;
        --markdown-dir) markdown_dir="$2"; shift 2 ;;
        --baseline) baseline="$2"; shift 2 ;;
        --queue-limit) queue_limit="$2"; shift 2 ;;
        --profile) profile="$2"; shift 2 ;;
        --with-markdown) with_markdown=1; shift ;;
        -h|--help) usage ;;
        *) echo "unknown option: $1" >&2; usage ;;
    esac
done

if [[ -z "$samples" ]]; then
    samples="${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}/combined-samples"
fi
if [[ ! -d "$samples" ]]; then
    echo "sample corpus does not exist: $samples" >&2
    echo "pass --samples DIR or set EXIFTOOL_SAMPLES" >&2
    exit 1
fi

if [[ -z "$exiftool" ]]; then
    exiftool="$(command -v exiftool || true)"
fi
if [[ -z "$exiftool" || ! -x "$exiftool" ]]; then
    echo "ExifTool executable not found; pass --exiftool PATH" >&2
    exit 1
fi

if [[ -z "$output" ]]; then
    output="${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}/incremental-comparison.json"
fi
if [[ -z "$markdown_dir" ]]; then
    markdown_dir="$(dirname "$output")/markdown"
fi

binary="target/$profile/tag-comparison"
needs_build=0
if [[ ! -x "$binary" ]]; then
    needs_build=1
elif find src oxidex-tags oxidex-tags-* tools Cargo.toml Cargo.lock \
        -type f -newer "$binary" -print -quit | grep -q .; then
    needs_build=1
fi
if ((needs_build)); then
    echo "building tag-comparison ($profile profile)"
    CARGO_BUILD_RUSTC_WRAPPER="${CARGO_BUILD_RUSTC_WRAPPER:-sccache}" \
        cargo build --profile "$profile" --bin tag-comparison --features tag-comparison-binary
fi

mkdir -p "$(dirname "$output")"
args=(--samples "$samples" --exiftool "$exiftool" --output "$output"
      --reuse-output --queue-limit "$queue_limit")
if ((with_markdown)); then
    mkdir -p "$markdown_dir"
    args+=(--markdown-dir "$markdown_dir")
else
    args+=(--no-markdown)
fi
if [[ -n "$formats" ]]; then args+=(--formats "$formats"); fi
if [[ -n "$baseline" ]]; then args+=(--baseline "$baseline"); fi

echo "refreshing ${formats:-all formats}; cached formats are reused from $output"
exec "./$binary" "${args[@]}"
