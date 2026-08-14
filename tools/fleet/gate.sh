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
#
# --- T1.2 additions (fleet/FLEET_SPEC.md M6 parts 1-2, P2 -- the
# correctness phase, docs/FLEET.md §7 addenda) ---
# GATE_VERSION bumped 1 -> 2: this pass changes gate BEHAVIOUR, not just
# recording. Three things:
#   1. The gate's input is now `tip + branch merged`, never the branch
#      alone (M6 "gate the merge result"). BASE_TIP is the tip actually
#      merged against; TREE_SHA is the merged tree, not the branch's own.
#   2. `toolchain_id` is split into `rustc_id` (host: line stripped -- "is
#      this host on the canonical compiler?") and `platform_id` (host: line
#      kept -- "is this verdict transferable to that host?", part of the
#      verdict cache key). Collapsing them let a Linux PASS on
#      ffi_c_integration silently satisfy a macOS gate slot; see
#      tools/fleet/verdict.py's module docstring.
#   3. `result` gains ABORT alongside PASS/FAIL, and the verdict cache
#      (tools/fleet/verdict.py) is consulted before paying for a build and
#      updated after one -- see classify_failure() and the cache
#      lookup/store calls below.
set -u
GATE_VERSION="2"
BRANCH="$1"; TAG="$2"
START_TS=$(date +%s)
HOST=$(hostname)
SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export PATH="$HOME/.nvm/versions/node/v24.13.1/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
unset RUSTC_WRAPPER
export CARGO_INCREMENTAL=0
export EXIFTOOL="/tmp/oxidex-exiftool-cache/exiftool-pinned.sh"
export CARGO_TARGET_DIR="$HOME/tgt/nc-$TAG"
export OXIDEX="$CARGO_TARGET_DIR/release/oxidex"
export TAGMATRIX_WORK="$HOME/tgt/tagmap-$TAG"
mkdir -p "$HOME/gatelogs" "$HOME/tgt"
L="$HOME/gatelogs/gate-$TAG.log"; V="$HOME/gatelogs/gate-$TAG.verdict"; J="$HOME/gatelogs/gate-$TAG.json"

# rustc_id / platform_id (T1.2, replaces the single T0.3 TOOLCHAIN_ID).
# Computed once, after PATH is set, so both reflect the same rustc the
# checks below will actually invoke. platform_id hashes `rustc -vV`
# verbatim; rustc_id hashes it with the `host:` line stripped -- the exact
# split tools/fleet/verdict.py's compute_ids() documents and mirrors.
RUSTC_VV=$(rustc -vV 2>/dev/null)
_sha256() { { command -v sha256sum >/dev/null 2>&1 && sha256sum || shasum -a 256; } | awk '{print $1}'; }
PLATFORM_ID=$(printf '%s' "$RUSTC_VV" | _sha256)
RUSTC_ID=$(printf '%s\n' "$RUSTC_VV" | grep -v '^host:' | _sha256)

# The hub this gate's verdict cache reads/writes -- same remote this host
# already trusts as the source of branches it clones below (see the T1.2
# clone/merge block), overridable for a differently-configured host.
HUB_URL="${FLEET_HUB_URL:-$HOME/git/oxidex.git}"
VERDICT_WORKDIR="${FLEET_VERDICT_CACHE_DIR:-$HOME/.cache/oxidex-fleet-verdict-cache}"

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
  "rustc_id": "$(json_escape "$RUSTC_ID")",
  "platform_id": "$(json_escape "$PLATFORM_ID")",
  "host": "$(json_escape "$HOST")",
  "duration_s": $duration,
  "write_set": [$ws_items]
}
JSONEOF
}

# store_verdict -- best-effort push of the just-written $J to the verdict
# cache (tools/fleet/verdict.py, T1.2). Never allowed to change this run's
# own exit status: a hub hiccup here means the next gate on this tree pays
# for a rebuild it didn't strictly need to, not that this run's own result
# becomes wrong. Skipped entirely when TREE_SHA never resolved (the
# low-disk abort below fires before any clone happens).
store_verdict() {
  [ -n "$TREE_SHA" ] || return 0
  python3 "$SELF_DIR/verdict.py" store \
    --hub-url "$HUB_URL" --workdir "$VERDICT_WORKDIR" --json-file "$J" \
    >> "$L" 2>&1 || echo "verdict cache store failed (non-fatal)" >> "$L"
}

# classify_failure <stage> -- ABORT vs FAIL for a just-failed step, per the
# T1.2 addendum ("ABORT covers OOM, low disk, lost oracle, killed
# process... non-admissible but non-damning: it schedules a retry rather
# than condemning the branch"). Replays the rb-s26 shape directly: rustc
# taking `signal: 9, SIGKILL` during an `-C lto -C codegen-units=1` link is
# indistinguishable from a genuine test failure by exit code alone (cargo
# itself still exits non-zero and reports success at the shell level), so
# this greps the tail of the just-appended log for the signatures a killed
# child leaves behind rather than trusting $?.
#
# This can only classify a child process that died loudly enough for its
# parent (cargo, python, etc.) to report it and hand control back to this
# script. If the gate script's own process is killed from outside (the
# host OOM-killing THIS script, not a child), nothing here runs at all --
# that failure mode is only visible externally, as a claim whose lease
# expires with no verdict ever written (T1.1's territory).
classify_failure() {
  local stage="$1"
  # "lost oracle" is named explicitly in the ABORT list -- no signature to
  # grep for, it's a precondition check, not a killed process, so force it.
  if [ "$stage" = "oracle-precondition" ]; then
    echo "ABORT"
    return
  fi
  local tail
  tail=$(tail -c 4000 "$L" 2>/dev/null)
  if printf '%s' "$tail" | grep -Eiq 'signal: (6|7|9|11)\b|SIGKILL|SIGSEGV|SIGABRT|SIGBUS|out of memory|cannot allocate memory|oom-kill|oom_kill|killed\\)|no space left on device'; then
    echo "ABORT"
  else
    echo "FAIL"
  fi
}
# --- end recording additions; original logic resumes below, unreordered ---

AVAIL=$(df -BG --output=avail /home 2>/dev/null | tail -1 | tr -dc 0-9)
if [ -n "$AVAIL" ] && [ "$AVAIL" -lt 14 ]; then echo "ABORT low-disk ${AVAIL}G" > "$V"; write_json "ABORT" "low-disk"; exit 7; fi

# --- T1.2: gate the merge result, not the branch (M6 part 1) ---
# Previously this cloned `--branch "$BRANCH"` directly and treated the
# branch's own tree as the thing under test, with BASE_TIP recorded only
# after the fact as the branch's merge-base with the tip. That is a
# verdict about the branch, not about the tree that will actually exist
# once it merges -- incidents 5/6/7 (a branch 24 commits behind, and two
# pairs of individually-green branches that broke combined) all trace to
# exactly that gap. Now the tip is checked out first and the branch is
# merged into it; everything below runs against that merge commit.
D="$HOME/git/gate-$TAG"; rm -rf "$D"
git clone -q "$HOME/git/oxidex.git" "$D" || { echo "FAIL clone" > "$V"; write_json "FAIL" "clone"; exit 9; }
cd "$D" || exit 9

git checkout -q origin/refactor/tag-machinery \
  || { echo "FAIL checkout-tip" > "$V"; write_json "FAIL" "checkout-tip"; rm -rf "$D"; exit 9; }
BASE_TIP=$(git rev-parse HEAD)

git fetch -q origin "$BRANCH" \
  || { echo "FAIL fetch-branch" > "$V"; write_json "FAIL" "fetch-branch"; rm -rf "$D"; exit 9; }
BRANCH_SHA=$(git rev-parse FETCH_HEAD)

# write_set is the BRANCH's own changes, not the merge's -- diffed from
# where it actually forked (which may sit behind BASE_TIP if the branch is
# drifting; that's T1.3's job to fix, this just records honestly either
# way) to BRANCH_SHA.
FORK_POINT=$(git merge-base "$BASE_TIP" "$BRANCH_SHA" 2>/dev/null || echo "$BASE_TIP")
WRITE_SET=$(git diff --name-only "$FORK_POINT" "$BRANCH_SHA" 2>/dev/null || echo "")

git -c user.email=fleet@oxidex.local -c user.name=oxidex-fleet merge -q --no-edit "$BRANCH_SHA" \
  > "$L" 2>&1
if [ $? -ne 0 ]; then
  echo "FAIL merge-conflict" > "$V"
  echo "GATE FAIL: merge-conflict" >> "$L"; echo "GATE DONE $TAG" >> "$L"
  write_json "FAIL" "merge-conflict"; store_verdict
  rm -rf "$CARGO_TARGET_DIR" "$D"; exit 1
fi
TREE_SHA=$(git rev-parse HEAD^{tree} 2>/dev/null || echo "")

# --- T1.2: verdict cache (M6 part 2) ---
# Consulted before paying for a build: two hosts computing the identical
# merge (same TREE_SHA, same GATE_VERSION, same PLATFORM_ID) derive the
# identical cache key, so the second one reuses the first one's answer --
# "a no-op rebase costs nothing". tools/fleet/verdict.py's lookup() never
# serves an ABORT here, so an aborted tree always falls through to a real
# re-run rather than being cached as a settled non-answer.
CACHED_JSON=$(python3 "$SELF_DIR/verdict.py" lookup \
  --hub-url "$HUB_URL" --workdir "$VERDICT_WORKDIR" \
  --tree-sha "$TREE_SHA" --gate-version "$GATE_VERSION" --platform-id "$PLATFORM_ID" 2>>"$L")
CACHE_STATUS=$?
if [ "$CACHE_STATUS" -eq 0 ] && [ -n "$CACHED_JSON" ]; then
  CACHED_RESULT=$(printf '%s' "$CACHED_JSON" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("result",""))' 2>/dev/null)
  if [ -n "$CACHED_RESULT" ]; then
    printf '%s\n' "$CACHED_JSON" > "$J"
    echo "$CACHED_RESULT cache-hit tree=$TREE_SHA" > "$V"
    echo "GATE CACHE HIT $TAG -> $CACHED_RESULT (tree $TREE_SHA)" >> "$L"
    echo "GATE DONE $TAG" >> "$L"
    rm -rf "$CARGO_TARGET_DIR" "$D"
    exit 0
  fi
fi

FEAT="--features jpeg-tag-matrix-binary"
# Same two probes the verbatim script always ran, captured into variables
# instead of streamed straight to the log, so the result can also gate
# (below) without invoking the oracle a second time. Log content is
# unchanged: same two lines, same order, same values -- just appended after
# the merge-cache preamble above instead of starting a fresh file.
ORACLE_VER=$("$EXIFTOOL" -ver 2>&1)
ORACLE_DOCX=$("$EXIFTOOL" -s3 -FileType "/tmp/oxidex-exiftool-cache/exiftool/t/images/OOXML.docx" 2>&1)
{ echo "=== $BRANCH @ $(git log --oneline -1) (merged onto $BASE_TIP) ==="; echo "$ORACLE_VER"; echo "$ORACLE_DOCX"; } >> "$L" 2>&1
fail(){
  local stage="$1"
  local result
  result=$(classify_failure "$stage")
  echo "$result $stage" > "$V"
  echo "GATE FAIL: $stage ($result)" >> "$L"
  echo "GATE DONE $TAG" >> "$L"
  write_json "$result" "$stage"
  store_verdict
  rm -rf "$CARGO_TARGET_DIR" "$D"
  exit 1
}

# Hard precondition (T0.3): a matching `-ver` alone is not a working oracle --
# the pinned tree's exiftool can resolve to a Homebrew perl with no
# Archive::Zip and silently report FileType: ZIP for every container format.
# T1.2: this is now literally the "lost oracle" case the ABORT addendum
# names -- classify_failure() forces ABORT for this exact stage.
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
store_verdict
# reclaim the ~10G target dir immediately; the verdict and log are what matter
rm -rf "$CARGO_TARGET_DIR" "$D"
