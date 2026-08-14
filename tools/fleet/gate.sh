#!/bin/bash
# Full gate, sccache-free, on PERSISTENT storage.
#
# Two measured lessons are baked in here:
#  1. sccache was the throughput ceiling, not CPU. 18 gates funnelling through ONE
#     sccache server sat at 65.8% idle with 2 rustc; unsetting RUSTC_WRAPPER took it
#     to 60 rustc and 0% idle. Under high fan-out with a cold cache sccache is a
#     serialisation point, not an accelerator.
#  2. /tmp here is a container emptyDir with an EPHEMERAL-STORAGE QUOTA far below the
#     456G that `df` reports for the overlay. Twenty ~10G target dirs blew the quota and
#     the platform evicted the whole of /tmp -- taking the pinned oracle, the corpus,
#     sccache and every gate log with it. Build output therefore lives on /home, and
#     concurrency is bounded by that budget rather than by core count.
#
# --- T0.3 recording additions (fleet/FLEET_SPEC.md M6/M7) ---
# GATE_VERSION and tools/fleet/gate_version.txt must always hold the same value;
# bump both together whenever gate BEHAVIOUR changes (which checks run, their
# order, or what counts as pass/fail). Pure logging/formatting changes don't
# need a bump, but when in doubt, bump it.
# This pass adds recording only: a GATE_VERSION stamp, the oracle capability
# probe promoted from "logged" to "hard precondition", a --json-out verdict
# alongside the existing plaintext .verdict, and write_set capture. It does
# NOT change which checks run, their order, or the plaintext .verdict
# contract -- those are unchanged from the verbatim move. See docs/FLEET.md.
set -u
GATE_VERSION="1"
BRANCH="$1"; TAG="$2"
START_TS=$(date +%s)
HOST=$(hostname)
export PATH="$HOME/.nvm/versions/node/v24.13.1/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
unset RUSTC_WRAPPER
export CARGO_INCREMENTAL=0
export EXIFTOOL="/tmp/oxidex-exiftool-cache/exiftool-pinned.sh"
export CARGO_TARGET_DIR="$HOME/tgt/nc-$TAG"
export OXIDEX="$CARGO_TARGET_DIR/release/oxidex"
export TAGMATRIX_WORK="$HOME/tgt/tagmap-$TAG"
mkdir -p "$HOME/gatelogs" "$HOME/tgt"
L="$HOME/gatelogs/gate-$TAG.log"; V="$HOME/gatelogs/gate-$TAG.verdict"; J="$HOME/gatelogs/gate-$TAG.json"

# toolchain_id = sha256(rustc -vV), per M7. Computed once, after PATH is set,
# so it reflects the same rustc the checks below will actually invoke.
TOOLCHAIN_ID=$(rustc -vV 2>/dev/null | { command -v sha256sum >/dev/null 2>&1 && sha256sum || shasum -a 256; } | awk '{print $1}')
TREE_SHA=""
BASE_TIP=""
WRITE_SET=""

json_escape() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

# write_json <result> <stage> -- emits the --json-out verdict ALONGSIDE the
# existing plaintext $V file. Never replaces it; never gates on it.
write_json() {
  local result="$1"
  local stage="$2"
  local duration
  duration=$(( $(date +%s) - START_TS ))
  local ws_items="" f esc
  if [ -n "$WRITE_SET" ]; then
    while IFS= read -r f; do
      [ -z "$f" ] && continue
      esc=$(json_escape "$f")
      if [ -z "$ws_items" ]; then ws_items="\"$esc\""; else ws_items="$ws_items,\"$esc\""; fi
    done <<< "$WRITE_SET"
  fi
  cat > "$J" <<JSONEOF
{
  "tree_sha": "$(json_escape "$TREE_SHA")",
  "base_tip": "$(json_escape "$BASE_TIP")",
  "branch": "$(json_escape "$BRANCH")",
  "result": "$(json_escape "$result")",
  "stage": "$(json_escape "$stage")",
  "gate_version": "$(json_escape "$GATE_VERSION")",
  "toolchain_id": "$(json_escape "$TOOLCHAIN_ID")",
  "host": "$(json_escape "$HOST")",
  "duration_s": $duration,
  "write_set": [$ws_items]
}
JSONEOF
}
# --- end recording additions; original logic resumes below, unreordered ---

AVAIL=$(df -BG --output=avail /home 2>/dev/null | tail -1 | tr -dc 0-9)
if [ -n "$AVAIL" ] && [ "$AVAIL" -lt 14 ]; then echo "ABORT low-disk ${AVAIL}G" > "$V"; write_json "ABORT" "low-disk"; exit 7; fi
D="$HOME/git/gate-$TAG"; rm -rf "$D"
git clone -q --branch "$BRANCH" "$HOME/git/oxidex.git" "$D" || { echo "FAIL clone" > "$V"; write_json "FAIL" "clone"; exit 9; }
cd "$D" || exit 9

# Recording only: tree/base/write_set for the JSON verdict. Not a check --
# failure to resolve these never aborts the gate, it just leaves the field empty.
TREE_SHA=$(git rev-parse HEAD^{tree} 2>/dev/null || echo "")
BASE_TIP=$(git merge-base HEAD origin/refactor/tag-machinery 2>/dev/null || git rev-parse origin/refactor/tag-machinery 2>/dev/null || echo "")
if [ -n "$BASE_TIP" ]; then
  WRITE_SET=$(git diff --name-only "$BASE_TIP" HEAD 2>/dev/null || echo "")
fi

FEAT="--features jpeg-tag-matrix-binary"
# Same two probes the verbatim script always ran, captured into variables
# instead of streamed straight to the log, so the result can also gate
# (below) without invoking the oracle a second time. Log content is
# unchanged: same two lines, same order, same values.
ORACLE_VER=$("$EXIFTOOL" -ver 2>&1)
ORACLE_DOCX=$("$EXIFTOOL" -s3 -FileType "/tmp/oxidex-exiftool-cache/exiftool/t/images/OOXML.docx" 2>&1)
{ echo "=== $BRANCH @ $(git log --oneline -1) ==="; echo "$ORACLE_VER"; echo "$ORACLE_DOCX"; } > "$L" 2>&1
fail(){ echo "FAIL $1" > "$V"; echo "GATE FAIL: $1" >> "$L"; echo "GATE DONE $TAG" >> "$L"; write_json "FAIL" "$1"; rm -rf "$CARGO_TARGET_DIR" "$D"; exit 1; }

# Hard precondition (T0.3): a matching `-ver` alone is not a working oracle --
# the pinned tree's exiftool can resolve to a Homebrew perl with no
# Archive::Zip and silently report FileType: ZIP for every container format.
ORACLE_VER_CLEAN=$(printf '%s' "$ORACLE_VER" | tr -d '[:space:]')
ORACLE_DOCX_CLEAN=$(printf '%s' "$ORACLE_DOCX" | tr -d '[:space:]')
if [ "$ORACLE_VER_CLEAN" != "13.59" ] || [ "$ORACLE_DOCX_CLEAN" != "DOCX" ]; then
  fail oracle-precondition
fi

cargo fmt --all -- --check >> "$L" 2>&1 || fail fmt
cargo clippy --release --all-features $FEAT -- -D warnings >> "$L" 2>&1 || fail clippy
cargo build --release $FEAT --bin oxidex --bin jpeg-tag-matrix >> "$L" 2>&1 || fail release-build
cargo test --workspace --release $FEAT >> "$L" 2>&1 || fail tests
just verify-tables >> "$L" 2>&1 || fail verify-tables
[ -x "$OXIDEX" ] || fail no-binary
"$CARGO_TARGET_DIR/release/jpeg-tag-matrix" manifest --flag-noops >> "$L" 2>&1 \
  && "$CARGO_TARGET_DIR/release/jpeg-tag-matrix" run --workers 12 >> "$L" 2>&1 \
  && "$CARGO_TARGET_DIR/release/jpeg-tag-matrix" report --check-baseline >> "$L" 2>&1 || fail ratchet
echo "PASS" > "$V"; echo "GATE DONE $TAG" >> "$L"
write_json "PASS" "complete"
# reclaim the ~10G target dir immediately; the verdict and log are what matter
rm -rf "$CARGO_TARGET_DIR" "$D"
