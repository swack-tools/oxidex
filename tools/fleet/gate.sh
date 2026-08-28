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
#
# --- T7 additions (ARCH-FIX-SPEC.md R7, "the gate tests the fleet's own
# code") --- GATE_VERSION bumped 4 -> 5: a new "fleet-tests" stage runs
# between `cargo test --workspace` and `just verify-tables` (search this
# file for "fleet-tests" below), consisting of (1) a `python3 -m
# py_compile` sweep of tools/fleet/*.py and (2) `python3 -m unittest
# discover -s tools/fleet/tests` with FLEET_TESTS_HERMETIC=1. The fleet's
# own coordination code (claims, the train, hooks, ...) had unit tests
# for every half of its safety contracts while their COMPOSITION never
# ran anywhere -- see AGENTS.md/ARCH-FIX-SPEC.md's diagnosis. This stage
# is what makes the gate the thing that runs them. A red fleet-tests
# stage is always a FAIL, never an ABORT (classify_failure() special-cases
# stage=="fleet-tests" below): a broken test in the tool that computes
# verdicts must not be waved through as a flaky OOM signature match.
# Hermetic mode is what the gate always runs here: the ledger/intent
# behavioral tests that build a release binary and shell to the real
# pinned oracle+corpus are skipped on purpose, because this exact gate
# run already builds that release binary a few lines earlier (see each
# skipped test's own skipUnless comment in
# tools/fleet/tests/test_intent.py and test_ledger.py for the itemized
# list of what hermetic mode skips and why).
#
# --- BLOCKER 6 additions ("R7 gets teeth") --- GATE_VERSION bumped 5 -> 6:
# R7 landed the fleet-tests stage but nothing forced it to actually run
# with teeth -- a hang in it would have blocked a host forever (no wall
# clock), and it silently included seam tests never meant to run under
# per-gate contention. Two behaviour changes:
#   1. WALL-CLOCK TIMEOUT. The whole fleet-tests stage (py_compile sweep +
#      unittest run) is now bounded by FLEET_TESTS_TIMEOUT_S (default
#      600s, overridable for tests). A hang past that budget is killed by
#      process group and recorded as stage "fleet-tests-timeout" -- a
#      FAIL, not an ABORT, same reasoning as plain "fleet-tests" below:
#      classify_failure() forces both, since a hang in the fleet's own
#      test suite is exactly as damning as a red assertion in it, and must
#      never be waved through as a flaky OOM signature match.
#   2. test_seams.py IS EXCLUDED from this stage (POLICY, decided). Seam
#      tests are the fleet's slowest and least deterministic suite by
#      design -- they drive real subprocess fleetds, real SIGKILLs, real
#      lease timers -- and several of that suite's OWN commit messages
#      document contention flakes when several gates run it concurrently
#      on a shared host. R7's goal is a gate that reliably blocks broken
#      coordination code; a gate stage that is itself flaky under fleet
#      load does not do that, it intermittently condemns branches that
#      never touched the code the flake is in (R4: the queue must trust
#      what it is told, and a flaky per-gate stage poisons that trust for
#      every branch waiting behind it). Seams keep running in CI and in
#      burn-in, where contention with sibling gates is not a factor --
#      just not in the per-gate hot path. See `_fleet_test_modules()`
#      below for the mechanics of the exclusion.
#
# --- BLOCKER A additions ("a false FAIL is worse than a slow gate") ---
# GATE_VERSION bumped 6 -> 7: the fleet-tests stage gains a ONE-ROUND
# isolation retry, which changes what counts as pass/fail and therefore
# needs a bump.
#
# MEASUREMENT FIRST. On a clean tree (staging/afx-integration @ 9efc39b2)
# the whole fleet-tests STAGE went red on 2 of 8 runs -- ~25% -- on two
# DIFFERENT tests each time (test_lease_protocol.TestFleetdSingletonRenews
# once; test_adoption.TestRestartAdoption's tearDown with
# OSError(ENOTEMPTY) once), while both of those modules passed 10/10 in
# ISOLATION on that same tree (instrument: scratchpad/flakeloop.sh, 10
# iterations per tree, run exactly as gate.sh runs them -- from
# tools/fleet/tests with FLEET_TESTS_HERMETIC=1). The two shapes are what
# you would expect from tests that drive real subprocesses, real git
# pushes and real lease timers all inside ONE python process: cross-module
# interference, not a defect in the branch being gated.
#
# WHY THAT IS A BLOCKER RATHER THAN AN ANNOYANCE. This stage's verdict is
# published to the SHARED verdict cache (see store_verdict below) and keyed
# on the merged tree, so a false FAIL does not merely waste the 20-45min
# gate that produced it: it condemns the branch fleet-wide (needs_author)
# and every later host reads the cached answer instead of re-running. The
# cost of a false FAIL is therefore unbounded, while the cost of the retry
# below is bounded by the same FLEET_TESTS_TIMEOUT_S budget the stage
# already had.
#
# THE POLICY (evidence-matched, deliberately narrow):
#   1. Stage red -> parse the failing MODULE names out of the unittest
#      output (`_fleet_tests_failed_modules`). If nothing parses -- a
#      py_compile syntax error, an import-time loader error, a crash with
#      no FAIL:/ERROR: header -- there is no module to retry and the stage
#      FAILs exactly as before. Silence is never treated as a flake.
#   2. Each failing module is re-run ALONE, in a fresh python process,
#      same env, same cwd -- at most ONE round, never a loop.
#   3. Every failing module passing alone == interference between modules,
#      not a broken branch: the stage PASSES, and the verdict JSON gains
#      `fleet_tests_flakes` (module name + the failure text) so burn-in
#      MEASURES the real rate off the shared cache instead of us guessing
#      at it. That field is the whole reason this is a recording policy
#      and not a shrug.
#   4. Any module that fails ALONE is a genuine failure: FAIL, as today.
#      A module list that recovers only partially is a FAIL too, and
#      records no flakes -- a run that is failing for a real reason must
#      not also publish flake telemetry that would dilute the rate.
#   5. Running out of the wall-clock budget mid-retry is
#      "fleet-tests-timeout" -- FAIL, same as before. The retry may never
#      buy itself more time than the stage originally had.
# What this does NOT do: retry any other stage, retry an individual test
# (the module is the isolation unit because the interference is between
# modules), or run more than one round.
#
# --- PLAN Stage 1 task 4 additions ("de-hardcode URLs and paths") ---
# GATE_VERSION NOT bumped (still 7): this pass removes hardcoded defaults
# for two remotes -- the verdict-cache hub and the code repo holding
# staging branches, both previously defaulting to a host-specific path or
# the old work2.oxidex.net hub over ssh -- and for the pinned-oracle
# cache directory, previously hardcoded to /tmp/oxidex-exiftool-cache in
# two places. All three now come from the environment. It changes
# neither which checks run, their order, what counts as pass/fail, nor
# the plaintext/JSON verdict contract: config only, verdict semantics
# unchanged.
#   - FLEET_HUB_URL is now REQUIRED (verdict-cache hub): absent -> ABORT,
#     stage "config", before anything is cloned or built.
#   - FLEET_CODE_URL (the repo holding staging/* and refactor/tag-machinery)
#     DEFAULTS TO FLEET_HUB_URL when unset. B4 (Stage 1 integration review):
#     the two-repo split is the steady state, but a single-repo stand-in
#     (one host serving both roles, e.g. the i7 workflow's
#     FLEET_HUB_URL=<local repo> with no FLEET_CODE_URL at all) must keep
#     working exactly as it did before the split introduced a second
#     variable -- that is the whole point of a default, not a second
#     required knob. ABORT, stage "config", fires only when NEITHER var is
#     set (in practice unreachable here since the FLEET_HUB_URL check above
#     already exits first, but the guard stays in case that ordering ever
#     changes).
#   - EXIFTOOL_CACHE_DIR overrides the pinned-oracle cache directory;
#     unset keeps today's exact default. See
#     tools/fleet/tests/test_no_hardcoded_hosts.py for the fence this
#     keeps green.
#
# GATE_VERSION bumped 7 -> 8: FLEET_TESTS_TIMEOUT_S default 600 -> 1800.
# The budget exists to catch HANGS, not to race a growing suite: at 600 s
# it was sized for the ~360-test suite that existed when BLOCKER 6 added
# it, the suite has since grown past 650 tests (Stage 1e + Stage 2 land
# ~790), and gate keel2 on the i7 FAILed "fleet-tests-timeout" at 600 s
# (2026-08-22) with a fully green suite -- the exact false-FAIL the flake
# policy above calls unbounded, manufactured by the budget itself. 1800 s
# still kills a genuine hang three tenths into a typical 90-min gate
# while leaving a green-but-slow suite room to finish. This changes what
# counts as pass/fail (a 900 s green run was FAIL at v7, PASS at v8), so
# it needs the bump; gate_version is part of the shared verdict-cache key
# (tree_sha, gate_version, platform_id), so v7 verdicts simply never
# collide with v8 ones -- no cached v7 FAIL manufactured by the old
# budget can condemn a branch under v8, and no re-measurement is needed.
set -u
GATE_VERSION="8"

# BLOCKER 6 (i): wall-clock budget for the whole fleet-tests stage
# (py_compile sweep + unittest run together), overridable for tests that
# need to prove the timeout path itself without waiting 600s for it.
# BLOCKER A: this is now the budget for the stage AND its isolation
# retries together -- the retry round spends what the first run left over,
# never a fresh allowance.
# GATE_VERSION 8: default 600 -> 1800 (see the version-history block
# above -- the budget catches hangs, it does not race the suite's size).
FLEET_TESTS_TIMEOUT_S="${FLEET_TESTS_TIMEOUT_S:-1800}"

# BLOCKER A: JSON array ITEMS (no brackets) for the verdict's optional
# `fleet_tests_flakes` field. Empty means the field is omitted entirely --
# `write_json` is called long before the fleet-tests stage runs (low-disk,
# clone, merge-conflict), and a clean run must not carry an empty array
# that later readers would have to distinguish from "this gate version
# didn't record flakes".
FLEET_TESTS_FLAKES=""

# run_with_wall_clock_timeout <timeout_s> <timed-out-flag-file> -- <cmd...>
#
# No `timeout(1)`/`gtimeout` dependency: neither is guaranteed present on
# every fleet host (observed absent outright on at least one dev box), so
# this is a plain bash watchdog. The command runs in the background under
# its own process group (`set -m` job control gives it one); a sibling
# watchdog subshell sleeps for the budget and, if the command is still
# alive when it wakes, touches the flag file and SIGTERMs the whole group
# (SIGKILL after a short grace) -- the same "kill the group, not just the
# leader" reasoning `classify_failure`'s neighbours use elsewhere in this
# file, so a hung `python3 -m unittest` cannot leave orphaned children
# behind. The watchdog is torn down in either outcome so it can never fire
# late against a future, unrelated stage.
run_with_wall_clock_timeout() {
  local timeout_s="$1" flag="$2"
  shift 2
  rm -f "$flag"
  set -m
  ( "$@" ) &
  local cmd_pid=$!
  set +m
  (
    sleep "$timeout_s"
    if kill -0 "$cmd_pid" 2>/dev/null; then
      : > "$flag"
      kill -TERM "-$cmd_pid" 2>/dev/null || kill -TERM "$cmd_pid" 2>/dev/null
      sleep 5
      kill -KILL "-$cmd_pid" 2>/dev/null || kill -KILL "$cmd_pid" 2>/dev/null
    fi
  ) &
  local watchdog_pid=$!
  wait "$cmd_pid" 2>/dev/null
  local rc=$?
  kill "$watchdog_pid" 2>/dev/null
  wait "$watchdog_pid" 2>/dev/null
  return "$rc"
}

# BLOCKER 6 (iv): the module list the fleet-tests stage actually runs --
# every tools/fleet/tests/test_*.py EXCEPT test_seams.py (POLICY, see the
# header comment above `GATE_VERSION` for the rationale). `unittest`'s own
# `discover` has no exclude-by-name option, so this enumerates explicitly
# and runs `python3 -m unittest <module...>` instead -- same test bodies,
# same HERMETIC gating inside them, just minus one filename. The list is
# sorted for a deterministic run order and log.
_fleet_test_modules() {
  local f base
  for f in tools/fleet/tests/test_*.py; do
    [ -e "$f" ] || continue
    base="$(basename "$f")"
    [ "$base" = "test_seams.py" ] && continue
    printf '%s\n' "${base%.py}"
  done | sort
}
BRANCH="$1"; TAG="$2"
START_TS=$(date +%s)
HOST=$(hostname)
SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# L1 (Keel Stage 1 LIVE, 2026-08-27/28): the toolchain PATH prefix and the
# rustc_id/platform_id formula both come from the ONE resolver now --
# units/fleet-toolchain.sh (shell entry point) over tools/fleet/toolchain.py
# (the formula). This gate and the fleetd that spawns it therefore cannot
# derive different verdict-cache keys for the same compiler, which is
# exactly what they did on the i7: fleetd stored b2bdf493..., this script
# stored b6613b19..., and the scheduler could not read the PASS it had
# just paid twenty-one minutes for. Sourced BEFORE the PATH export so the
# prefix below is the resolver's, not a second spelling of it. Guarded
# because the very next line reads FLEET_TOOLCHAIN_PATH_PREFIX under
# `set -u`: without this, a missing resolver kills the gate with bash's
# own "unbound variable" and no hint as to what is actually missing.
. "$SELF_DIR/units/fleet-toolchain.sh" || {
  echo "gate.sh: cannot source $SELF_DIR/units/fleet-toolchain.sh -- refusing to gate without a toolchain identity" >&2
  exit 7
}
export PATH="$HOME/.nvm/versions/node/v24.13.1/bin:$FLEET_TOOLCHAIN_PATH_PREFIX:$PATH"
unset RUSTC_WRAPPER
export CARGO_INCREMENTAL=0
# PLAN Stage 1 task 4: EXIFTOOL_CACHE_DIR overrides the pinned-oracle
# cache directory; unset keeps today's exact default, which lives in ONE
# shell file (units/fleet-env.sh, mirrored by config.py for Python) -- R6:
# this used to be a two-piece reassembly written to dodge
# test_no_hardcoded_hosts.py.
. "$SELF_DIR/units/fleet-env.sh"
export EXIFTOOL="$EXIFTOOL_CACHE_DIR/exiftool-pinned.sh"
export CARGO_TARGET_DIR="$HOME/tgt/nc-$TAG"
export OXIDEX="$CARGO_TARGET_DIR/release/oxidex"
export TAGMATRIX_WORK="$HOME/tgt/tagmap-$TAG"
mkdir -p "$HOME/gatelogs" "$HOME/tgt"
L="$HOME/gatelogs/gate-$TAG.log"; V="$HOME/gatelogs/gate-$TAG.verdict"; J="$HOME/gatelogs/gate-$TAG.json"
# R4: a marker beside the verdict, written only when `store_verdict` below
# could not push to the hub cache -- see that function for why this exists.
# T2: the SUFFIX comes from units/fleet-env.sh (sourced above), which is
# the shell mirror of config.py's VERDICT_STORE_FAILED_SUFFIX. It used to
# be spelled out here AND in fleetd._verdict_store_failed_marker with
# nothing comparing the two, so renaming one left the suite green while
# fleetd stopped seeing a marker this script was still writing.
SV="$HOME/gatelogs/gate-$TAG$FLEET_VERDICT_STORE_FAILED_SUFFIX"

# rustc_id / platform_id (T1.2, replaces the single T0.3 TOOLCHAIN_ID).
#
# L1 (Keel Stage 1 LIVE, 2026-08-27/28): this block USED to spell the two
# digests itself --
#     RUSTC_VV=$(rustc -vV 2>/dev/null)
#     PLATFORM_ID=$(printf '%s' "$RUSTC_VV" | _sha256)
#     RUSTC_ID=$(printf '%s\n' "$RUSTC_VV" | grep -v '^host:' | _sha256)
# -- one of THREE implementations in the tree, no two of which agreed on
# both fields. `$(...)` strips the trailing newline; `subprocess.run`'s
# stdout keeps it; so fleetd and this script hashed the same compiler to
# different platform_ids and wrote/read different verdict-cache slots on
# the same host. The formula now lives in tools/fleet/toolchain.py alone
# and `fleet_toolchain_ids` (units/fleet-toolchain.sh, sourced above)
# fetches it, preserving these exact bytes so every verdict already on
# the state repo stays readable. Checked for failure below, beside the
# FLEET_HUB_URL check, once `write_json` exists to ABORT loudly.
fleet_toolchain_ids || true

# The hub this gate's verdict cache reads/writes. PLAN Stage 1 task 4:
# REQUIRED, no default -- a gate with no FLEET_HUB_URL must never guess
# at one (silently reading/writing the wrong verdict cache is worse than
# refusing to run); checked below once write_json exists to fail loud.
HUB_URL="${FLEET_HUB_URL:-}"
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
  # BLOCKER A: the optional flake record. Present ONLY when an isolation
  # round actually proved a flake; absent (not `[]`) otherwise, so a
  # reader can tell "no flakes seen" from "this run recorded nothing".
  local flakes_field=""
  if [ -n "$FLEET_TESTS_FLAKES" ]; then
    flakes_field=",
  \"fleet_tests_flakes\": [$FLEET_TESTS_FLAKES]"
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
  "write_set": [$ws_items]$flakes_field
}
JSONEOF
}

# PLAN Stage 1 task 4: fail loud, before anything is cloned or built, if
# the verdict-cache hub was not named. Config only -- does not change
# which checks run or the verdict contract, so GATE_VERSION stays 7.
if [ -z "$HUB_URL" ]; then
  echo "ABORT config: FLEET_HUB_URL not set" > "$V"
  write_json "ABORT" "config"
  exit 7
fi

# L1: same doctrine one line up -- a gate that could not resolve its own
# toolchain identity must not invent one. An empty or guessed PLATFORM_ID
# is not a harmless missing field: it is a verdict written to, and read
# from, a cache slot no other component on this host addresses, which is
# precisely the failure this resolver exists to make impossible. ABORT
# (not FAIL) because it says nothing about the branch. Config only --
# GATE_VERSION stays 8.
if [ -n "$FLEET_TOOLCHAIN_ERROR" ] || [ -z "$PLATFORM_ID" ] || [ -z "$RUSTC_ID" ]; then
  echo "ABORT config: toolchain identity unresolved: ${FLEET_TOOLCHAIN_ERROR:-empty platform_id/rustc_id}" > "$V"
  write_json "ABORT" "config"
  exit 7
fi

# store_verdict -- best-effort push of the just-written $J to the verdict
# cache (tools/fleet/verdict.py, T1.2). Never allowed to change this run's
# own exit status: a hub hiccup here means the next gate on this tree pays
# for a rebuild it didn't strictly need to, not that this run's own result
# becomes wrong. Skipped entirely when TREE_SHA never resolved (the
# low-disk abort below fires before any clone happens).
#
# R4: the old one-line "(non-fatal)" swallow was true about THIS gate's own
# exit status (still is -- nothing below changes that) but false about the
# operator's visibility: a hub outage here left no trace anywhere an
# operator would think to look, so a run of tokenless-host failures read as
# silence rather than as a repeated, diagnosable failure. Loud now means
# three things, all local and none of them touching this function's return
# value: (1) `GATE: VERDICT STORE FAILED` in this gate's own log, (2) a
# `$SV` marker file beside `$V` that outlives this process, for
# `fleetd.py`'s reap loop to notice and fold into the next heartbeat's
# `refused[]` (see fleetd.py's reconcile_once), and (3) a clean rerun that
# succeeds removes any marker a PRIOR failed attempt under this same TAG
# left behind, so the heartbeat cannot report a failure that has since
# resolved.
store_verdict() {
  [ -n "$TREE_SHA" ] || return 0
  rm -f "$SV"
  if ! python3 "$SELF_DIR/verdict.py" store \
    --hub-url "$HUB_URL" --workdir "$VERDICT_WORKDIR" --json-file "$J" \
    >> "$L" 2>&1; then
    echo "GATE: VERDICT STORE FAILED -- verdict.py could not push to $HUB_URL (non-fatal to this gate's own PASS/FAIL; see the output just above)" >> "$L"
    : > "$SV"
  fi
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
  # T7 (ARCH-FIX-SPEC.md R7): a red fleet-tests stage is ALWAYS a FAIL,
  # never an ABORT, forced here for the same reason oracle-precondition is
  # forced the other way above -- this is a verdict about the fleet's own
  # test suite, and letting a stray OOM-shaped log line upgrade a genuine
  # test failure into a non-damning retry would defeat the entire point of
  # R7 (a broken coordination-code test must block the tip, not get waved
  # through). py_compile syntax errors and unittest failures alike land
  # here as plain FAIL.
  # BLOCKER 6: "fleet-tests-timeout" (the wall-clock kill above) is the
  # same verdict for the same reason -- a hung fleet-tests run is exactly
  # as damning as a red assertion in it, not a candidate for a non-damning
  # ABORT retry.
  if [ "$stage" = "fleet-tests" ] || [ "$stage" = "fleet-tests-timeout" ]; then
    echo "FAIL"
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
# The local mirror is a cache, not truth: refresh it from the code repo
# before cloning, and fall back to cloning it directly if the mirror
# still lacks the branch (an unfetched mirror cost a gate an instant
# "couldn't find remote ref" on m5 -- fleet-managed gates start seconds
# after a push, faster than any mirror cron).
#
# PLAN Stage 1 task 4: this is the repo holding staging/* and
# refactor/tag-machinery -- distinct from $HUB_URL (the verdict-cache
# hub) once the two live in separate repos. Previously both this and
# $HUB_URL defaulted (to the old work2.oxidex.net ssh remote and to
# $HOME/git/oxidex.git respectively) and, because they shared one
# variable name, the clone default silently won and store_verdict/lookup
# below ran against it instead of the verdict-cache hub whenever
# FLEET_HUB_URL was unset -- moot in practice because every production
# host always set FLEET_HUB_URL, but a trap for anyone who didn't.
# Separate variables remove both the hardcoded remote and that shadowing.
#
# B4 (Stage 1 integration review): FLEET_CODE_URL DEFAULTS TO
# FLEET_HUB_URL when unset, rather than being independently required. A
# single-repo stand-in (one host, one repo playing both the state and
# code role -- the i7 workflow's FLEET_HUB_URL=<local repo>, no
# FLEET_CODE_URL) was 100% ABORT before this default existed: the repo
# it needed was already named, just under the other variable. The loud
# ABORT below fires only when NEITHER var is set -- $HUB_URL is already
# guaranteed non-empty by the check earlier in this script, so in
# practice this can only be reached with $CODE_URL resolved, but the
# explicit check is kept rather than assumed across a possible future
# reorder.
CODE_URL="${FLEET_CODE_URL:-$HUB_URL}"
if [ -z "$CODE_URL" ]; then
  echo "ABORT config: neither FLEET_CODE_URL nor FLEET_HUB_URL set" > "$V"
  write_json "ABORT" "config"
  exit 7
fi
CLONE_SRC="$HOME/git/oxidex.git"
if [ -d "$CLONE_SRC" ]; then
  git -C "$CLONE_SRC" fetch -q "$CODE_URL" \
      "+refs/heads/staging/*:refs/heads/staging/*" \
      "+refs/heads/refactor/tag-machinery:refs/heads/refactor/tag-machinery" 2>/dev/null || true
  git -C "$CLONE_SRC" rev-parse -q --verify "refs/heads/$BRANCH" >/dev/null 2>&1 || CLONE_SRC="$CODE_URL"
else
  CLONE_SRC="$CODE_URL"
fi
git clone -q "$CLONE_SRC" "$D" || { echo "FAIL clone" > "$V"; write_json "FAIL" "clone"; exit 9; }
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
ORACLE_DOCX=$("$EXIFTOOL" -s3 -FileType "$EXIFTOOL_CACHE_DIR/exiftool/t/images/OOXML.docx" 2>&1)
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
# CARGO_PROFILE_RELEASE_PANIC=unwind: the justfile's own `test` recipe
# (its `unwind :=` variable, line ~38) has always carried this because
# [profile.release] sets panic="abort", which breaks should_panic tests.
# gate.sh lacked it and false-FAILed the first branch to carry such a
# test through the gate (conds12's grammar tests) -- a gate bug, found by
# a producer agent that reproduced the failure both ways. GATE_VERSION 4.
CARGO_PROFILE_RELEASE_PANIC=unwind cargo test --workspace --release $FEAT >> "$L" 2>&1 || fail tests

# --- T7 (ARCH-FIX-SPEC.md R7): the gate tests the fleet's own code ---
# Runs AFTER the workspace test suite and BEFORE the ratchet, on the same
# merged tree everything above already tested. Two checks:
#   1. A `py_compile` sweep of tools/fleet/*.py (direct children only,
#      matching tools/fleet/tests/test_no_raw_hub_push.py's own scope
#      convention -- not hooks/, rollout/, units/, or tests/) so a fleet
#      tool with a plain syntax error fails loudly here instead of at the
#      moment some host tries to run it during an actual gate or train.
#   2. The fleet's own test suite, hermetic. FLEET_TESTS_HERMETIC=1 skips
#      the ledger/intent tests that build a release oxidex binary and
#      shell to the real pinned oracle + combined-samples corpus -- see
#      tools/fleet/tests/test_intent.py / test_ledger.py's own
#      "HERMETIC SKIP" comments for the itemized list of what that skips
#      and why (in short: this exact run already built that release
#      binary two lines above, and a gate must not depend on host-local
#      paths like /tmp/oxidex-exiftool-cache/... that a CI runner may
#      lack).
# Both are a hard FAIL, never an ABORT: classify_failure() special-cases
# stage=="fleet-tests" (and "fleet-tests-timeout", BLOCKER 6) above so a
# stray log line that happens to match an OOM signature can never wave
# through a genuine test failure here.
#
# BLOCKER 6: bounded by FLEET_TESTS_TIMEOUT_S, and test_seams.py is
# excluded from the module list `_fleet_test_modules()` builds (POLICY,
# see the header comment above `GATE_VERSION`) -- everything else under
# tools/fleet/tests/ still runs, `discover`'s own HERMETIC gating inside
# each test is unaffected.

# _fleet_tests_unittest_run <module...> -- the one place that knows HOW
# this stage invokes unittest, so the isolation re-runs below are provably
# the same invocation as the original with a shorter module list, not a
# second dialect of it. Unlike `discover -s tools/fleet/tests`, plain
# `python3 -m unittest <name>` resolves <name> against the CURRENT WORKING
# DIRECTORY (no tools/fleet/tests/__init__.py makes this a namespace
# import, not a package one), so the process must actually BE in that
# directory when unittest loads the modules.
_fleet_tests_unittest_run() {
  ( cd tools/fleet/tests && FLEET_TESTS_HERMETIC=1 python3 -m unittest "$@" )
}

_run_fleet_tests_stage() {
  python3 -m py_compile tools/fleet/*.py || return 1
  # `_fleet_test_modules` is called from the REPO ROOT (its own glob is
  # rooted there) before the `cd` inside `_fleet_tests_unittest_run`, and
  # its output captured first.
  local mods
  mods="$(_fleet_test_modules)"
  # Deliberately unquoted: `mods` is a newline-separated list that must
  # word-split into separate arguments here.
  # shellcheck disable=SC2086
  _fleet_tests_unittest_run $mods
}

# _fleet_tests_failed_modules <output-file> -- the MODULE names unittest
# reported as red, one per line, sorted and deduped.
#
# unittest's summary headers are `FAIL: <test> (<module>.<Class>[.<test>])`
# (the trailing `.<test>` appeared in 3.11; both shapes are matched) and
# the same for `ERROR:`, which is also what a tearDown explosion prints --
# the exact shape of one of the two measured flakes. The module is the
# first dotted component inside the parens.
#
# `sed -n ... p` prints ONLY lines that matched: anything else -- a
# py_compile traceback, a segfault, an import-time loader error whose
# "module" is `unittest.loader._FailedTest` -- yields no name at all, and
# the caller treats an empty list as "nothing retryable, FAIL". That is
# the safe direction: an unparseable red stage is never mistaken for a
# flake.
_fleet_tests_failed_modules() {
  sed -n -E 's/^(FAIL|ERROR): [^(]*\(([A-Za-z0-9_]+)\.[^)]*\).*$/\2/p' "$1" | sort -u
}

# _fleet_tests_isolation_round <stage-output-file> -- the ONE retry round.
# Return codes: 0 == every failing module passed alone (flake, stage
# passes, FLEET_TESTS_FLAKES populated), 1 == genuine failure, 2 == the
# wall-clock budget ran out mid-round.
_fleet_tests_isolation_round() {
  local out="$1"
  local mods known m rc elapsed remaining flag rerun_out ftext acc=""
  mods="$(_fleet_tests_failed_modules "$out")"
  if [ -z "$mods" ]; then
    echo "GATE: fleet-tests red but no failing module name parsed -- not retryable" >> "$L"
    return 1
  fi
  # A parsed name that is not one of the modules this stage actually ran
  # means the parse is wrong, not that a mystery module flaked. Refuse to
  # retry rather than re-run something unrelated and call the result a
  # flake.
  known="$(_fleet_test_modules)"
  for m in $mods; do
    if ! printf '%s\n' "$known" | grep -qx -- "$m"; then
      echo "GATE: fleet-tests reported module '$m', which is not in this stage's module list -- not retryable" >> "$L"
      return 1
    fi
  done
  echo "GATE: fleet-tests red; failing modules: $(printf '%s' "$mods" | tr '\n' ' ')" >> "$L"
  echo "GATE: re-running each ALONE, one round, within the remaining ${FLEET_TESTS_TIMEOUT_S}s stage budget" >> "$L"
  for m in $mods; do
    elapsed=$(( $(date +%s) - FLEET_TESTS_START ))
    remaining=$(( FLEET_TESTS_TIMEOUT_S - elapsed ))
    if [ "$remaining" -le 0 ]; then
      echo "GATE: fleet-tests isolation ran out of wall clock before '$m'" >> "$L"
      return 2
    fi
    flag="$L.fleet-tests-retry-flag"
    rerun_out="$L.fleet-tests-retry-out"
    run_with_wall_clock_timeout "$remaining" "$flag" \
      _fleet_tests_unittest_run "$m" > "$rerun_out" 2>&1
    rc=$?
    { echo "--- fleet-tests isolation re-run: $m (budget ${remaining}s) ---"; cat "$rerun_out"; } >> "$L"
    if [ -f "$flag" ]; then
      rm -f "$flag" "$rerun_out"
      echo "GATE: fleet-tests isolation re-run of '$m' exceeded the remaining wall clock -- killed" >> "$L"
      return 2
    fi
    rm -f "$rerun_out"
    if [ "$rc" -ne 0 ]; then
      echo "GATE: fleet-tests isolation: '$m' FAILED alone too -- genuine failure" >> "$L"
      return 1
    fi
    echo "GATE: fleet-tests isolation: '$m' PASSED alone -- recorded as a flake" >> "$L"
    # The failure text kept for the verdict is this module's own FAIL:/
    # ERROR: header lines from the ORIGINAL red run -- enough for burn-in
    # to tell the two measured shapes apart, bounded so a verdict payload
    # can never grow without limit.
    ftext=$(grep -E "^(FAIL|ERROR): [^(]*\($m\." "$out" | tr '\n\r\t' '   ' | cut -c1-400)
    acc="$acc${acc:+,}{\"module\":\"$(json_escape "$m")\",\"failure\":\"$(json_escape "$ftext")\"}"
  done
  FLEET_TESTS_FLAKES="$acc"
  return 0
}

FLEET_TESTS_TIMEOUT_FLAG="$L.fleet-tests-timeout-flag"
FLEET_TESTS_OUT="$L.fleet-tests-out"
FLEET_TESTS_START=$(date +%s)
run_with_wall_clock_timeout "$FLEET_TESTS_TIMEOUT_S" "$FLEET_TESTS_TIMEOUT_FLAG" \
  _run_fleet_tests_stage > "$FLEET_TESTS_OUT" 2>&1
FLEET_TESTS_RC=$?
cat "$FLEET_TESTS_OUT" >> "$L"
if [ -f "$FLEET_TESTS_TIMEOUT_FLAG" ]; then
  rm -f "$FLEET_TESTS_TIMEOUT_FLAG" "$FLEET_TESTS_OUT"
  echo "GATE: fleet-tests exceeded ${FLEET_TESTS_TIMEOUT_S}s wall clock -- killed" >> "$L"
  fail fleet-tests-timeout
fi
if [ "$FLEET_TESTS_RC" -ne 0 ]; then
  _fleet_tests_isolation_round "$FLEET_TESTS_OUT"
  case $? in
    0) echo "GATE: fleet-tests PASSED after one isolation round (flakes recorded in the verdict)" >> "$L" ;;
    2) rm -f "$FLEET_TESTS_OUT"; fail fleet-tests-timeout ;;
    *) rm -f "$FLEET_TESTS_OUT"; fail fleet-tests ;;
  esac
fi
rm -f "$FLEET_TESTS_OUT"

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
