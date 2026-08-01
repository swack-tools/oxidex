#!/usr/bin/env bash
# Fast local gate for the incremental parity workflow.

set -euo pipefail

run_corpus=1
if [[ "${1:-}" == "--skip-corpus" ]]; then
    run_corpus=0
    shift
fi

if (($#)); then
    echo "unexpected argument: $1" >&2
    echo "usage: verify_parity_fast.sh [--skip-corpus]" >&2
    exit 2
fi

echo "checking formatting"
cargo fmt --all -- --check

echo "checking fixloop binary"
CARGO_BUILD_RUSTC_WRAPPER="${CARGO_BUILD_RUSTC_WRAPPER:-sccache}" \
    cargo check --profile fixloop --bin tag-comparison --features tag-comparison-binary

if [[ -n "${VERIFY_ENUM_PM:-}" ]]; then
    echo "checking generated enum maps"
    enum_args=(--pm "$VERIFY_ENUM_PM")
    if [[ -n "${VERIFY_ENUM_TABLE:-}" ]]; then
        enum_args+=(--table "$VERIFY_ENUM_TABLE")
    fi
    python3 scripts/verify_enum_maps.py "${enum_args[@]}"
else
    echo "generated enum-map check skipped (set VERIFY_ENUM_PM=... to enable)"
fi

if ((run_corpus)); then
    if [[ -d "${EXIFTOOL_SAMPLES:-/tmp/oxidex-exiftool-cache/combined-samples}" ]]; then
        bash scripts/compare_incremental.sh --formats "${EXIFTOOL_FORMATS:-JPEG,PNG,TIFF}" \
            --queue-limit "${EXIFTOOL_QUEUE_LIMIT:-50}"
    else
        echo "corpus not found; skipping comparison (use EXIFTOOL_SAMPLES=... to enable)"
    fi
fi

git diff --check
echo "fast parity verification passed"
